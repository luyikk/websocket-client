#websocket client

```rust 
async fn test_websocket()->anyhow::Result<()> {
    wasm_logger::init(wasm_logger::Config::default());

    let (tx, rx) = futures_channel::oneshot::channel();

    let ws = websocket_client::WebSocketClient::connect(
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
        }
        (),
    )
    .await?;
    
    for i in 0..=254 {
        ws.send_all(vec![0, 1, 2, 3,i, 255]).await?;
    }
    rx.await?;
    console_log!("finish");
    Ok(())
}

```