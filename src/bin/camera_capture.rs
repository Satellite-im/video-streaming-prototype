use dioxus::prelude::*;

fn main() {
    dioxus_desktop::launch(app);
}

fn app(cx: Scope) -> Element {
    let css = include_str!("../app/style.css");

    render! (
        style {
            "{css}"
        }
        main {
        }
    )
}
