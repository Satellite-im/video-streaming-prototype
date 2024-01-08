use std::sync::{atomic::AtomicBool, Arc};

use dioxus::prelude::*;
use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpListener, sync::broadcast};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use video_streaming_prototype::{
    app,
    video::{self, YuvFrame},
};

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
        // this won't work because dioxus sends data over the use_eval channel as a string.
        async move {
            /*let should_quit = dropper.flag.clone();
            let (tx, _rx) = broadcast::channel(128);
            let tx2 = tx.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = video::capture_camera(tx2, should_quit) {
                    eprintln!("camera capture failed: {e}");
                }
            });
            // websocket server
            tokio::spawn(async move {
                let addr = "127.0.0.1:8081";
                let url = url::Url::parse(addr).unwrap();
                let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
                println!("WebSocket handshake has been successfully completed");

                let mut rx = tx.subscribe();
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
            });*/
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
            script { app::WEBGL_SCRIPT }
        }
    )
}
