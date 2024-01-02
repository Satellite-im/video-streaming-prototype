use dioxus::prelude::*;
use futures_util::StreamExt;
use video_streaming_prototype::video;

fn main() {
    dioxus_desktop::launch(app);
}

enum Cmd {
    Start,
    Stop,
}

fn app(cx: Scope) -> Element {
    let css = include_str!("../app/style.css");

    let eval = use_eval(cx);
    let ch = use_coroutine(cx, |mut rx: UnboundedReceiver<Cmd>| {
        to_owned![eval];
        async move {
            let handle = tokio::spawn(async move {
                let stream = video::get_stream().unwrap();
            });
            while let Some(cmd) = rx.next().await {
                match cmd {
                    Cmd::Start => {}
                    Cmd::Stop => {}
                }
            }
        }
    });

    render! (
        style {
            "{css}"
        }
        main {
        }
    )
}
