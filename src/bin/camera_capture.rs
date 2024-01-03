use std::{
    process::{self, ExitCode},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::signal;

use dioxus::prelude::*;
use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use tokio::{net::TcpListener, sync::broadcast};
use tokio_tungstenite::tungstenite::protocol::Message;
use video_streaming_prototype::video::{self, YuvFrame};

//#[tokio::main]
fn main() {
    // dioxus_desktop::launch(app);
    let (tx, mut rx) = broadcast::channel(128);

    /*tokio::task::spawn_blocking(move || while rx.blocking_recv().is_ok() {});

    let tx2 = tx.clone();
    let should_quit = Arc::new(AtomicBool::new(false));
    let should_quit2 = should_quit.clone();
    std::thread::spawn(move || {
        if let Err(e) = video::capture_camera(tx2, should_quit2) {
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
                while let Ok(YuvFrame { y, u, v }) = rx.recv().await {
                    if let Err(e) = sink.send(Message::Binary(y)).await {
                        eprintln!("failed to send Y plane: {e}");
                        break;
                    }
                    if let Err(e) = sink.send(Message::Binary(u)).await {
                        eprintln!("failed to send Cb plane: {e}");
                        break;
                    }
                    if let Err(e) = sink.send(Message::Binary(v)).await {
                        eprintln!("failed to send Cr plane: {e}");
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
    std::process::exit(0)*/

    let should_quit = Arc::new(AtomicBool::new(false));
    let should_quit2 = should_quit.clone();

    ctrlc::set_handler(move || {
        println!("received Ctrl+C!");
        should_quit.store(true, Ordering::Relaxed);
    })
    .unwrap();

    if let Err(e) = video::capture_camera(tx, should_quit2) {
        eprintln!("camera capture failed: {e}");
    }

    std::process::exit(0)
}

/*fn app(cx: Scope) -> Element {
    let css = include_str!("../app/style.css");

    let eval = use_eval(cx);
    let _ch = use_coroutine(cx, |_rx: UnboundedReceiver<()>| {
        to_owned![eval];
        async move {
            let (tx, _rx) = broadcast::channel(128);
            let tx2 = tx.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = video::capture_camera(tx2) {
                    eprintln!("camera capture failed: {e}");
                }
            });
            // websocket server
            tokio::spawn(async move {
                let addr = "127.0.0.1:8081".to_string();
                // Create the event loop and TCP listener we'll accept connections on.
                let try_socket = TcpListener::bind(&addr).await;
                let listener = try_socket.expect("Failed to bind");
                println!("Listening on: {}", addr);

                while let Ok((stream, _)) = listener.accept().await {
                    let mut rx = tx.subscribe();
                    tokio::spawn(async move {
                        let ws_stream = tokio_tungstenite::accept_async(stream).await.unwrap();

                        let (mut sink, _stream) = ws_stream.split();
                        while let Ok(YuvFrame { y, u, v }) = rx.recv().await {
                            if let Err(e) = sink.send(Message::Binary(y)).await {
                                eprintln!("failed to send Y plane: {e}");
                                break;
                            }
                            if let Err(e) = sink.send(Message::Binary(u)).await {
                                eprintln!("failed to send Cb plane: {e}");
                                break;
                            }
                            if let Err(e) = sink.send(Message::Binary(v)).await {
                                eprintln!("failed to send Cr plane: {e}");
                                break;
                            }
                        }
                        println!("closing websocket connection");
                    });
                }
            });

            // let _ = eval(app::WEBGL_SCRIPT);
        }
    });

    render! (
        style {
            "{css}"
        }
        main {
            canvas {
                id: "#canvas",
                height: 512,
                width: 512
            }
        }
    )
}*/
