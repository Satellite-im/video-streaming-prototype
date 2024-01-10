// the video-codec-cli from warp was moved here. it will be adapted to send the captured+encoded camera input to a browser.

// capture video from camera
// convert from rgb to yuv420
// encode using libaom
// decode using libaom
// make the video frame available

use crate::utils::yuv::*;

use anyhow::bail;
use av_data::{
    frame::{FrameType, MediaKind, VideoInfo},
    pixel::{ColorModel, TrichromaticEncodingSystem, YUVRange, YUVSystem},
    rational::Rational64,
    timeinfo::TimeInfo,
};
use dav1d::{Decoder, Settings};
use eye::{
    colorconvert::Device,
    hal::{
        format::PixelFormat,
        traits::{Context as _, Device as _, Stream as _},
        PlatformContext,
    },
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::broadcast;

use rav1e::prelude::*;

pub fn capture_stream(
    frame_tx: broadcast::Sender<Vec<u8>>,
    should_quit: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    // configure camera capture
    let ctx = PlatformContext::all()
        .next()
        .ok_or(anyhow::anyhow!("No platform context available"))?;

    // Create a list of valid capture devices in the system.
    let dev_descrs = ctx.devices()?;

    // Print the supported formats for each device.
    let dev = ctx.open_device(&dev_descrs[0].uri)?;
    let dev = Device::new(dev)?;
    let stream_descr = dev
        .streams()?
        .into_iter()
        // Choose RGB with 8 bit depth
        .filter(|s| matches!(s.pixfmt, PixelFormat::Rgb(24)))
        .filter(|s| s.interval.as_millis() == 33)
        .reduce(|s1, s2| {
            let distance = |width: u32, height: u32| {
                f32::sqrt(((1280 - width as i32).pow(2) + (720 - height as i32).pow(2)) as f32)
            };

            if distance(s1.width, s1.height) < distance(s2.width, s2.height) {
                s1
            } else {
                s2
            }
        })
        .ok_or(anyhow::anyhow!("failed to get video stream"))?;

    if stream_descr.pixfmt != PixelFormat::Rgb(24) {
        bail!("No RGB3 streams available");
    }

    println!("Selected stream:\n{:?}", stream_descr);

    // the camera will likely capture 1270x720. it's ok for width and height to be less than that.
    let frame_width = 512 as usize;
    let frame_height = 512 as usize;

    let mut config = EncoderConfig::with_speed_preset(10);
    config.width = 512;
    config.height = 512;
    let cfg = Config::default().with_encoder_config(config);
    let mut encoder_ctx: Context<u8> = cfg.new_context()?;

    let decoder_settings = Settings::default();
    let mut decoder = Decoder::with_settings(&decoder_settings)?;

    // Start the camera capture
    let mut stream = dev.start_stream(&stream_descr)?;
    println!("starting stream with description: {stream_descr:?}");

    let (tx, rx) = std::sync::mpsc::channel();
    let should_quit2 = should_quit.clone();
    tokio::task::spawn_blocking(move || loop {
        if should_quit2.load(Ordering::Relaxed) {
            println!("quitting camera capture tx thread");
            return;
        }
        if let Some(r) = stream.next() {
            match r {
                Ok(buf) => {
                    if let Err(e) = tx.send(buf.to_vec()) {
                        eprintln!("failed to send camera frame to video task: {e}");
                    }
                }
                Err(e) => eprintln!("failed to receive camera frame: {e}"),
            }
        }
    });

    while let Ok(frame) = rx.recv() {
        println!("got frame");
        if should_quit.load(Ordering::Relaxed) {
            println!("quitting camera capture rx thread");
            break;
        }

        let yuv = rgb_to_yuv420(
            &frame,
            frame_width,
            frame_height,
            stream_descr.width as _,
            stream_descr.height as _,
            ColorScale::Av,
        );
        drop(frame);

        let mut frame = encoder_ctx.new_frame();
        let (y, uv) = yuv.split_at(frame_width * frame_height);
        let (u, v) = uv.split_at(uv.len() / 2);
        frame.planes[0].copy_from_raw_u8(&y, frame_width, 1);
        frame.planes[1].copy_from_raw_u8(&u, frame_width / 2, 1);
        frame.planes[2].copy_from_raw_u8(&v, frame_width / 2, 1);

        if let Err(e) = encoder_ctx.send_frame(frame) {
            eprintln!("error sending frame to encoder: {e}");
            if !matches!(e, EncoderStatus::EnoughData) {
                continue;
            }
        } else {
            //continue;
        }

        loop {
            let packet = match encoder_ctx.receive_packet() {
                Ok(p) => p,
                Err(e) => {
                    if !matches!(e, EncoderStatus::NeedMoreData | EncoderStatus::Encoded) {
                        eprintln!("error receiving packet from encoder: {e}");
                    }
                    break;
                }
            };
            if let Err(e) = decoder.send_data(packet.data, None, None, None) {
                eprintln!("error sending data to decoder: {e}");
                continue;
            }
            loop {
                let plane = match decoder.get_picture() {
                    Ok(p) => p,
                    Err(e) => {
                        if !matches!(e, dav1d::Error::Again) {
                            eprintln!("error getting picture from decoder: {e}");
                        }
                        break;
                    }
                };

                println!("got picture");

                let y_stride = plane.stride(dav1d::PlanarImageComponent::Y);
                let u_stride = plane.stride(dav1d::PlanarImageComponent::U);
                let v_stride = plane.stride(dav1d::PlanarImageComponent::V);

                // this may be slow. does an extra copy
                let y_plane = plane.plane(dav1d::PlanarImageComponent::Y);
                let u_plane = plane.plane(dav1d::PlanarImageComponent::U);
                let v_plane = plane.plane(dav1d::PlanarImageComponent::V);

                let mut y = vec![];
                y.reserve(frame_width * frame_height);
                let mut u = vec![];
                u.reserve(y.len() / 4);
                let mut v = vec![];
                v.reserve(y.len() / 4);

                for row in y_plane.chunks_exact(y_stride as _) {
                    y.extend_from_slice(&row[0..frame_width]);
                }
                for row in u_plane.chunks_exact(u_stride as _) {
                    u.extend_from_slice(&row[0..frame_width / 2]);
                }
                for row in v_plane.chunks_exact(v_stride as _) {
                    v.extend_from_slice(&row[0..frame_width / 2]);
                }

                y.append(&mut u);
                y.append(&mut v);

                let _ = frame_tx.send(y);
            }
        }
    }

    Ok(())
}
