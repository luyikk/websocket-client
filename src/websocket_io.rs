use std::cmp::Ordering;
use std::pin::Pin;
use std::task::Poll;

use anyhow::{bail, Context, Result};
use futures_channel::mpsc::Receiver;
use futures_core::stream::Stream;
use futures_io::AsyncBufRead;
use futures_io::AsyncRead;
use futures_io::AsyncWrite;
use futures_util::StreamExt;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{ErrorEvent, MessageEvent, WebSocket};

pub struct WebsocketIO {
    pub(crate) ws: WebSocket,
    pub(crate) reader: WebSocketReader,
    pub(crate) ws_url: String,
}

pub struct WebSocketReader {
    read_rx: Receiver<Uint8Array>,
    remaining: Vec<u8>,
}
pub struct WebSocketWriter {
    pub(crate) ws: WebSocket,
    pub(crate) ws_url: String,
}

unsafe impl Send for WebSocketWriter {}
unsafe impl Sync for WebSocketWriter {}

impl WebsocketIO {
    pub async fn new(addr: &str) -> Result<WebsocketIO> {
        WebsocketIO::new_inner(format!("ws://{}", addr)).await
    }
    pub async fn new_wss(addr: &str) -> Result<WebsocketIO> {
        WebsocketIO::new_inner(format!("wss://{}", addr)).await
    }

    async fn new_inner(url: String) -> Result<WebsocketIO> {
        let ws = {
            match WebSocket::new(&url) {
                Ok(ws) => ws,
                Err(err) => bail!("WebSocket new error:{:?}", err),
            }
        };

        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let buffer = 4;
        let (mut open_tx, mut open_rx) = futures_channel::mpsc::channel(1);
        let (read_tx, read_rx) = futures_channel::mpsc::channel(buffer);

        let onmessage_callback = Closure::wrap(Box::new(move |e: MessageEvent| {
            let mut read_tx = read_tx.clone();
            if let Ok(abuf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let array = js_sys::Uint8Array::new(&abuf);
                if !read_tx.is_closed() {
                    if let Err(err) = read_tx.start_send(array) {
                        log::error!("start_send error: {:?}", err);
                    }
                }
            } else if let Ok(blob) = e.data().dyn_into::<web_sys::Blob>() {
                let fr = web_sys::FileReader::new().expect("web_sys::FileReader::new() fail");
                let fr_c = fr.clone();
                let file_reader_load_end =
                    Closure::wrap(Box::new(move |_e: web_sys::ProgressEvent| {
                        let array = Uint8Array::new(
                            &fr_c.result().expect("web_sys::FileReader result() err"),
                        );
                        if !read_tx.is_closed() {
                            if let Err(err) = read_tx.start_send(array) {
                                log::error!("start_send error: {:?}", err);
                            }
                        }
                    }) as Box<dyn FnMut(web_sys::ProgressEvent)>);
                fr.set_onloadend(Some(file_reader_load_end.as_ref().unchecked_ref()));
                file_reader_load_end.forget();
                fr.read_as_array_buffer(&blob).expect("blob not readable");
            }
            else {
                log::error!("message event, received Unknown: {:?}", e.data());
                return
            }
        }) as Box<dyn Fn(MessageEvent)>);

        let mut error_tx = open_tx.clone();
        let onerror_callback = Closure::wrap(Box::new(move |e: ErrorEvent| {
            log::error!("error event: {:?}", e);
            if !error_tx.is_closed() {
                error_tx.start_send(0).unwrap();
                error_tx.close_channel();
            }
        }) as Box<dyn FnMut(ErrorEvent)>);

        let onopen_callback =
            Closure::wrap(Box::new(move |_| {
                if !open_tx.is_closed() {
                    open_tx.start_send(1).unwrap();
                    open_tx.close_channel();
                }
            })
            as Box<dyn FnMut(JsValue)>);

        ws.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
        onmessage_callback.forget();

        ws.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
        onerror_callback.forget();

        ws.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
        onopen_callback.forget();

        let reader = WebSocketReader {
            read_rx,
            remaining: Vec::new(),
        };

        if  open_rx.next().await.context("open_rx is none")? ==1 {
            open_rx.close();
            drop(open_rx);
            let ws_io = WebsocketIO {
                ws,
                reader,
                ws_url: url,
            };
            Ok(ws_io)
        }else{
            open_rx.close();
            drop(open_rx);
            bail!("connect to:{} fail",url)
        }
    }

    pub fn split(self) -> (WebSocketReader, WebSocketWriter) {
        let WebsocketIO { ws, reader, ws_url } = self;
        (reader, WebSocketWriter { ws, ws_url })
    }
}

impl WebSocketReader {
    fn write_remaining(&mut self, buf: &mut [u8]) -> usize {
        match self.remaining.len().cmp(&buf.len()) {
            Ordering::Less => {
                let amount = self.remaining.len();
                buf[0..amount].copy_from_slice(&self.remaining);
                self.remaining.clear();
                amount
            }
            Ordering::Equal => {
                buf.copy_from_slice(&self.remaining);
                self.remaining.clear();
                buf.len()
            }
            Ordering::Greater => {
                let amount = buf.len();
                buf.copy_from_slice(&self.remaining[..amount]);
                self.remaining.drain(0..amount);
                amount
            }
        }
    }
}

impl AsyncRead for WebSocketReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        if !self.remaining.is_empty() {
            return Poll::Ready(Ok(self.write_remaining(buf)));
        }

        let array = match Pin::new(&mut self.read_rx).poll_next(cx) {
            Poll::Ready(Some(item)) => item,
            Poll::Ready(None) => return Poll::Pending,
            Poll::Pending => return Poll::Pending,
        };

        let array_length = array.length() as usize;

        let read = match array_length.cmp(&buf.len()) {
            Ordering::Equal => {
                array.copy_to(buf);
                buf.len()
            }
            Ordering::Less => {
                array.copy_to(&mut buf[..array_length]);
                array_length
            }
            Ordering::Greater => {
                self.remaining.resize(array_length, 0);
                array.copy_to(self.as_mut().remaining.as_mut_slice());
                self.write_remaining(buf)
            }
        };

        Poll::Ready(Ok(read))
    }
}
impl AsyncBufRead for WebSocketReader {
    fn poll_fill_buf(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<futures_io::Result<&[u8]>> {
        if !self.remaining.is_empty() {
            return Poll::Ready(Ok(self.get_mut().remaining.as_slice()));
        }

        let array = match Pin::new(&mut self.read_rx).poll_next(cx) {
            Poll::Ready(Some(item)) => item,
            Poll::Ready(None) => return Poll::Pending,
            Poll::Pending => return Poll::Pending,
        };

        self.remaining.extend(&array.to_vec());

        if self.remaining.is_empty() {
            return Poll::Pending;
        }
        Poll::Ready(Ok(self.get_mut().remaining.as_slice()))
    }

    fn consume(mut self: std::pin::Pin<&mut Self>, amt: usize) {
        if self.remaining.len() == amt {
            self.remaining.clear();
            return;
        }
        self.remaining.drain(0..amt);
    }
}
impl AsyncWrite for WebSocketWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.ws.send_with_u8_array(buf).unwrap();

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.ws.close().unwrap();
        Poll::Ready(Ok(()))
    }
}
