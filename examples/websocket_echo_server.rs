cfg_if::cfg_if! {
if #[cfg(feature = "build_axamples_server")]{
use futures_util::{SinkExt, StreamExt};
use log::*;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::{Result,Error}};


async fn accept_connection(peer: SocketAddr, stream: TcpStream) {
    if let Err(e) = handle_connection(peer, stream).await {
        match e {
            Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => (),
            err => error!("Error processing connection: {}", err),
        }
    }
}

async fn handle_connection(peer: SocketAddr, stream: TcpStream) -> Result<()> {
    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    info!("New WebSocket connection: {}", peer);

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if msg.is_text() || msg.is_binary() {
            ws_stream.send(msg).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    env_logger::Builder::new().filter_level(LevelFilter::Info).init();

    let addr = "127.0.0.1:8888";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    info!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr().expect("connected streams should have a peer address");
        info!("Peer address: {}", peer);

        tokio::spawn(accept_connection(peer, stream));
    }
}

}else{fn main(){}}}
