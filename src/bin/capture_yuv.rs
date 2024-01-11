use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::signal;

use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use tokio::{net::TcpListener, sync::broadcast};
use tokio_tungstenite::tungstenite::protocol::Message;
use video_streaming_prototype::video::{self};

fn main() {
    let (tx, mut rx) = broadcast::channel(128);

    let should_quit = Arc::new(AtomicBool::new(false));

    video::yuv_test2::capture_stream(tx, should_quit);
}
