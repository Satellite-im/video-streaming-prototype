// the video-codec-cli from warp was moved here. it will be adapted to send the captured+encoded camera input to a browser.

// capture video from camera
// convert from rgb to yuv420
// encode using libaom
// decode using libaom
// make the video frame available

use crate::utils::yuv::*;

use anyhow::bail;
use eye::{
    colorconvert::Device,
    hal::{
        format::PixelFormat,
        traits::{Context as _, Device as _, Stream as _},
        PlatformContext,
    },
};
use std::{
    ops::AddAssign,
    ptr::slice_from_raw_parts,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::sync::broadcast;
use openh264::{encoder::{Encoder, EncoderConfig}, decoder::DecodedYUV, OpenH264API, formats::YUVBuffer};
use openh264::decoder::Decoder;
use openh264::nal_units;

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

    let config = EncoderConfig::new(frame_width, frame_height);
    let api = OpenH264API::from_source();
    let mut encoder = Encoder::with_config(api, config)?;

    let api = OpenH264API::from_source();
    let mut decoder = Decoder::new(api)?;

    // Start the camera capture
    let mut stream = dev.start_stream(&stream_descr)?;
    println!("starting stream with description: {stream_descr:?}");

    let (camera_tx, camera_rx) = std::sync::mpsc::channel();
    let should_quit2 = should_quit.clone();
    let mut start = Instant::now();
    let mut times = vec![];
    std::thread::spawn(move || loop {
        if should_quit2.load(Ordering::Relaxed) {
            println!("quitting camera capture tx thread");
            return;
        }
       
        if let Some(r) = stream.next() {
            let elapsed = start.elapsed().as_millis();
            times.push(elapsed);
            if times.len() >= 30 {
              //  println!("times: {times:#?}");
                times.clear()
            }
            start = Instant::now();
            match r {
                Ok(buf) => {
                    if let Err(e) = camera_tx.send(buf.to_vec()) {
                        eprintln!("failed to send camera frame to video task: {e}");
                    }
                }
                Err(e) => eprintln!("failed to receive camera frame: {e}"),
            }
        }
    });

    let (encoder_tx, encoder_rx) = std::sync::mpsc::channel();
    let should_quit2 = should_quit.clone();
    std::thread::spawn(move || loop {
        if should_quit2.load(Ordering::Relaxed) {
            println!("quitting decoder rx thread");
            return;
        }

        let packet: Vec<u8> = match encoder_rx.recv() {
            Ok(f) => f,
            Err(e) => {
                eprintln!("error receiving encoded frame: {e}");
                break;
            }
        };

        // Split H.264 into NAL units and decode each.
        for packet in nal_units(&packet) {
            // On the first few frames this may fail, so you should check the result
            // a few packets before giving up.
            let yuv_frame = match decoder.decode(packet) {
                Err(e) => {
                    eprintln!("error decoding packet: {e}");
                    continue;
                },
                Ok(None) => {
                    eprintln!("None yuv frame yet, continuing");
                    continue;
                },
                Ok(Some(f)) => f,
            };
            let mut target_rgb: Vec<u8> = Vec::new();
            target_rgb.reserve(frame_width * frame_height * 3);
            yuv_frame.write_rgb8(target_rgb.as_slice().as_mut());
            let _ = frame_tx.send(yuv_frame);
    }});

    // Frame is received as RGB.
    while let Ok(rgb_frame) = camera_rx.recv() {
        if should_quit.load(Ordering::Relaxed) {
            println!("quitting camera capture rx thread");
            break;
        }

        let mut yuv_buffer= YUVBuffer::new(frame_width, frame_height);
        yuv_buffer.read_rgb(&rgb_frame);

        // Encode YUV back into H.264.
        let bitstream = match encoder.encode(&yuv_buffer) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error encoding frame: {e}");
                continue;
            }
        };

        let _ = encoder_tx.send(bitstream.to_vec());
    }

    Ok(())
}
