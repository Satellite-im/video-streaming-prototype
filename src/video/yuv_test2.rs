// the video-codec-cli from warp was moved here. it will be adapted to send the captured+encoded camera input to a browser.

// capture video from camera
// convert from rgb to yuv420
// encode using libaom
// decode using libaom
// make the video frame available

use crate::utils::yuv::*;

use anyhow::bail;
use dav1d::{Decoder, Settings};
use eye::{
    colorconvert::Device,
    hal::{
        format::PixelFormat,
        traits::{Context as _, Device as _, Stream as _},
        PlatformContext,
    },
};
use std::{
    ptr::slice_from_raw_parts,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
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

        // todo: use libyuv to convert from rgb to  yuv with hardware acceleration https://chromium.googlesource.com/libyuv/libyuv
        let frame = rgb_to_yuv4202(
            &frame,
            frame_width,
            frame_height,
            stream_descr.width as _,
            stream_descr.height as _,
            ColorScale::Av,
        );

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

                let y_plane = plane.plane_data_ptr(dav1d::PlanarImageComponent::Y) as *const u8;
                let u_plane = plane.plane_data_ptr(dav1d::PlanarImageComponent::U) as *const u8;
                let v_plane = plane.plane_data_ptr(dav1d::PlanarImageComponent::V) as *const u8;

                // todo: make the webgl code worry about the stride. then the entire plane can just be passed over.
                let y_plane =
                    unsafe { &*slice_from_raw_parts(y_plane, y_stride as usize * frame_height) };
                let u_plane = unsafe {
                    &*slice_from_raw_parts(u_plane, u_stride as usize * frame_height / 2)
                };
                let v_plane = unsafe {
                    &*slice_from_raw_parts(v_plane, v_stride as usize * frame_height / 2)
                };

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
