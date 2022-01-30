//! Test suite for the Web and headless browsers.
#![feature(thread_id_value)]
#![cfg(target_arch = "wasm32")]

use futures_util::{AsyncBufReadExt, StreamExt};
use log::Level;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;
use websocket_client_async::IWebSocketClient;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn pass() {
    assert_eq!(1 + 1, 2);
}

#[wasm_bindgen_test]
async fn test_websocket() {
    wasm_logger::init(wasm_logger::Config::new(Level::Trace));
    let (tx, rx) = futures_channel::oneshot::channel();
    let ws = websocket_client_async::WebSocketClient::connect(
        "127.0.0.1:8888",
        |_, ws, mut reader| async move {
            console_log!("connect websocket server ok");

            let mut buf = Vec::new();
            for _ in 0..1000 {
                reader.read_until(255, &mut buf).await?;
                console_log!("{:?}", buf);
                ws.send_all_ref(&buf).await?;
                buf.clear();
            }
            console_log!("disconnect websocket server");
            tx.send(()).unwrap();
            Ok(true)
        },
        (),
    )
    .await
    .unwrap();

    for i in 0..=254 {
        ws.send_all(vec![0, 1, 2, 3, i, 255]).await.unwrap();
    }

    rx.await.unwrap();
    console_log!("finish");
}

#[wasm_bindgen_test]
async fn test_websocket2() {
    wasm_logger::init(wasm_logger::Config::new(Level::Trace));
    let (mut tx, mut rx) = futures_channel::mpsc::channel(1);
    let ws = websocket_client_async::WebSocketClient::connect(
        "127.0.0.1:8888",
        |_, ws, mut reader| async move {
            console_log!("connect websocket server ok");

            let mut buf = Vec::new();
            for i in 0..100 {
                reader.read_until(255, &mut buf).await?;
                console_log!("{:?}", buf);
                ws.send_all_ref(&buf).await?;
                buf.clear();
                if !tx.is_closed() {
                    tx.start_send(i).unwrap()
                }
            }

            console_log!("disconnect websocket server");

            if !tx.is_closed() {
                tx.close_channel();
            }

            Ok(true)
        },
        (),
    )
    .await
    .unwrap();

    ws.send_all(vec![0, 1, 2, 3, 255]).await.unwrap();

    while let Some(i) = rx.next().await {
        if !ws.is_disconnect() {
            ws.send_ref(&i32::to_be_bytes(i)).await.unwrap();
            ws.send_all_ref(&[255]).await.unwrap();
        } else {
            break;
        }
    }

    console_log!("finish");
}
