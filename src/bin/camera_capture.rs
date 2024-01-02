use dioxus::prelude::*;
use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::Message;
use video_streaming_prototype::{app, video};

fn main() {
    dioxus_desktop::launch(app);
}

fn app(cx: Scope) -> Element {
    let css = include_str!("../app/style.css");

    let eval = use_eval(cx);
    let _ch = use_coroutine(cx, |_rx: UnboundedReceiver<()>| {
        to_owned![eval];
        async move {
            // websocket server
            tokio::spawn(async move {
                let addr = "127.0.0.1:8081".to_string();
                // Create the event loop and TCP listener we'll accept connections on.
                let try_socket = TcpListener::bind(&addr).await;
                let listener = try_socket.expect("Failed to bind");
                println!("Listening on: {}", addr);

                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        let ws_stream = tokio_tungstenite::accept_async(stream).await.unwrap();
                        let mut video_stream = video::get_stream().unwrap();

                        let (mut sink, _stream) = ws_stream.split();
                        while let Some(frame) = video_stream.next().await {
                            let Ok(y) = frame.as_slice_inner(0) else {
                                eprintln!("failed to extract Y plane from frame");
                                break;
                            };
                            let Ok(u) = frame.as_slice_inner(1) else {
                                eprintln!("failed to extract Cb plane from frame");
                                break;
                            };
                            let Ok(v) = frame.as_slice_inner(2) else {
                                eprintln!("failed to extract Cr plane from frame");
                                break;
                            };

                            if let Err(e) = sink.send(Message::Binary(y.to_vec())).await {
                                eprintln!("failed to send Y plane: {e}");
                                break;
                            }
                            if let Err(e) = sink.send(Message::Binary(u.to_vec())).await {
                                eprintln!("failed to send Cb plane: {e}");
                                break;
                            }
                            if let Err(e) = sink.send(Message::Binary(v.to_vec())).await {
                                eprintln!("failed to send Cr plane: {e}");
                                break;
                            }
                        }
                        println!("closing websocket connection");
                    });
                }
            });

            let _ = eval(app::WEBGL_SCRIPT);
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
