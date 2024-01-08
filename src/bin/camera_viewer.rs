use std::sync::{atomic::AtomicBool, Arc};

use dioxus::prelude::*;
use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpListener, sync::broadcast};
use tokio_tungstenite::tungstenite::Message;
use video_streaming_prototype::video::{self, YuvFrame};

fn main() {
    // launch the dioxus app in a webview
    dioxus_desktop::launch(app);
}

struct Dropper {
    flag: Arc<AtomicBool>,
}

impl Drop for Dropper {
    fn drop(&mut self) {
        self.flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn app(cx: Scope) -> Element {
    let css = include_str!("../app/style.css");
    let dropper = use_state(cx, || Dropper {
        flag: Arc::new(AtomicBool::new(false)),
    });

    let eval = use_eval(cx);
    let _ch = use_coroutine(cx, |_rx: UnboundedReceiver<()>| {
        to_owned![eval, dropper];
        async move {
            let should_quit = dropper.flag.clone();
            let (tx, _rx) = broadcast::channel(128);
            let tx2 = tx.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = video::capture_camera(tx2, should_quit) {
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
}
