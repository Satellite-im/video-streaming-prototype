use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::signal;

use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use tokio::{net::TcpListener, sync::broadcast};
use tokio_tungstenite::tungstenite::protocol::Message;
use video_streaming_prototype::video::{self, yuv_test::YuvFrame};

#[tokio::main]
async fn main() {
    let (tx, mut rx) = broadcast::channel(128);

    tokio::task::spawn_blocking(move || while rx.blocking_recv().is_ok() {});

    let tx2 = tx.clone();
    let should_quit = Arc::new(AtomicBool::new(false));
    let should_quit2 = should_quit.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = video::yuv_test::capture_stream(tx2, should_quit2) {
            eprintln!("camera capture failed: {e}");
        }
        println!("closing video camera capture");
    });
    // websocket server
    tokio::spawn(async move {
        let addr = "127.0.0.1:8081".to_string();
        // Create the event loop and TCP listener we'll accept connections on.
        let try_socket = TcpListener::bind(&addr).await;
        let listener = try_socket.expect("Failed to bind");
        println!("Listening on: {}", addr);

        while let Ok((stream, _)) = listener.accept().await {
            println!("connecting");
            let mut rx = tx.subscribe();
            tokio::spawn(async move {
                let ws_stream = tokio_tungstenite::accept_async(stream).await.unwrap();

                let (mut sink, _stream) = ws_stream.split();
                while let Ok(YuvFrame {
                    mut y,
                    mut u,
                    mut v,
                }) = rx.recv().await
                {
                    y.append(&mut u);
                    y.append(&mut v);
                    if let Err(e) = sink.send(Message::Binary(y)).await {
                        eprintln!("failed to send image: {e}");
                        break;
                    }
                }
                println!("closing websocket connection");
            });
        }

        println!("closing websocket listener");
    });

    signal::ctrl_c()
        .await
        .expect("Unable to listen for shutdown signal");
    println!("shutting down");
    should_quit.store(true, Ordering::Relaxed);
    std::process::exit(0)
}
