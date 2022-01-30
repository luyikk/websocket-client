mod websocket_io;

use crate::websocket_io::{WebSocketReader, WebSocketInner, WebsocketIO};
use anyhow::{bail, ensure, Error, Result};
use aqueue::Actor;
use futures_util::AsyncWriteExt;
use log::*;
use std::future::Future;
use std::ops::Deref;
use std::sync::Arc;

pub struct WebSocketClient {
    inner: WebSocketInner,
    disconnect: bool
}

impl WebSocketClient {
    #[inline]
    pub async fn connect<F: Future<Output = Result<bool>> + 'static, A: Send + 'static>(
        addr: &str,
        input: impl FnOnce(A, Arc<Actor<Self>>, WebSocketReader) -> F + Send + 'static,
        token: A,
    ) -> Result<Arc<Actor<Self>>> {
        let ws = WebsocketIO::new(addr).await?;
        Self::init(input, token, ws)
    }

    #[inline]
    pub async fn connect_wss<F: Future<Output = Result<bool>> + 'static, A: Send + 'static>(
        addr: &str,
        input: impl FnOnce(A, Arc<Actor<Self>>, WebSocketReader) -> F + Send + 'static,
        token: A,
    ) -> Result<Arc<Actor<Self>>> {
        let ws = WebsocketIO::new_wss(addr).await?;
        Self::init(input, token, ws)
    }

    #[inline]
    fn init<F: Future<Output = Result<bool>> + 'static, A: Send + 'static>(
        input: impl FnOnce(A, Arc<Actor<WebSocketClient>>, WebSocketReader) -> F + Send + 'static,
        token: A,
        ws: WebsocketIO,
    ) -> Result<Arc<Actor<WebSocketClient>>, Error> {
        let (reader, write) = ws.split();
        let client = Arc::new(Actor::new(WebSocketClient {
            disconnect: false,
            inner: write,
        }));
        let read_client = client.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let disconnect_client = read_client.clone();
            let need_disconnect = match input(token, read_client, reader).await {
                Ok(disconnect) => disconnect,
                Err(err) => {
                    error!("reader error:{}", err);
                    true
                }
            };

            if need_disconnect {
                if let Err(er) = disconnect_client.disconnect().await {
                    error!("disconnect to{} err:{}", disconnect_client.get_addr(), er);
                } else {
                    debug!("disconnect to {}", disconnect_client.get_addr())
                }
            } else {
                debug!("{} reader is close", disconnect_client.get_addr());
            }
        });

        Ok(client)
    }

    #[inline]
    async fn close(&mut self) -> Result<()> {
        if !self.disconnect {
            match self.inner.ws.close() {
                Ok(_) => {
                    self.disconnect = true;
                }
                Err(err) => bail!("websocket close error:{:?}", err),
            }
        }
        Ok(())
    }

    #[inline]
    async fn send<'a>(&'a mut self, buff: &'a [u8]) -> Result<usize> {
        if !self.disconnect {
            Ok(self.inner.write(buff).await?)
        } else {
            bail!("Send Error,Disconnect")
        }
    }

    #[inline]
    async fn send_all<'a>(&'a mut self, buff: &'a [u8]) -> Result<()> {
        if !self.disconnect {
            Ok(self.inner.write_all(buff).await?)
        } else {
            bail!("Send Error,Disconnect")
        }
    }

    #[inline]
    async fn flush(&mut self) -> Result<()> {
        if !self.disconnect {
            Ok(self.inner.flush().await?)
        } else {
            bail!("Send Error,Disconnect")
        }
    }
}

#[async_trait::async_trait]
pub trait IWebSocketClient {
    fn get_addr(&self) -> String;
    async fn send<B: Deref<Target = [u8]> + Send + Sync + 'static>(&self, buff: B)
        -> Result<usize>;
    async fn send_all<B: Deref<Target = [u8]> + Send + Sync + 'static>(
        &self,
        buff: B,
    ) -> Result<()>;
    async fn send_ref<'a>(&'a self, buff: &'a [u8]) -> Result<usize>;
    async fn send_all_ref<'a>(&'a self, buff: &'a [u8]) -> Result<()>;
    async fn flush(&mut self) -> Result<()>;
    async fn disconnect(&self) -> Result<()>;
    fn is_disconnect(&self) -> bool;
}

#[async_trait::async_trait]
impl IWebSocketClient for Actor<WebSocketClient> {
    #[inline]
    fn get_addr(&self) -> String {
        unsafe { self.deref_inner().inner.ws_url.clone() }
    }
    #[inline]
    async fn send<B: Deref<Target = [u8]> + Send + Sync + 'static>(
        &self,
        buff: B,
    ) -> Result<usize> {
        self.inner_call(|inner| async move { inner.get_mut().send(&buff).await })
            .await
    }

    #[inline]
    async fn send_all<B: Deref<Target = [u8]> + Send + Sync + 'static>(
        &self,
        buff: B,
    ) -> Result<()> {
        self.inner_call(|inner| async move { inner.get_mut().send_all(&buff).await })
            .await
    }

    #[inline]
    async fn send_ref<'a>(&'a self, buff: &'a [u8]) -> Result<usize> {
        ensure!(!buff.is_empty(), "send buff is null");
        unsafe {
            self.inner_call_ref(|inner| async move { inner.get_mut().send(buff).await })
                .await
        }
    }

    #[inline]
    async fn send_all_ref<'a>(&'a self, buff: &'a [u8]) -> Result<()> {
        ensure!(!buff.is_empty(), "send buff is null");
        unsafe {
            self.inner_call_ref(|inner| async move { inner.get_mut().send_all(buff).await })
                .await
        }
    }

    #[inline]
    async fn flush(&mut self) -> Result<()> {
        self.inner_call(|inner| async move { inner.get_mut().flush().await })
            .await
    }
    #[inline]
    async fn disconnect(&self) -> Result<()> {
        self.inner_call(|inner| async move { inner.get_mut().close().await })
            .await
    }
    #[inline]
    fn is_disconnect(&self) -> bool {
        unsafe { self.deref_inner().disconnect }
    }
}
