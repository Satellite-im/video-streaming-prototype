// the video-codec-cli from warp was moved here. it will be adapted to send the captured+encoded camera input to a browser.

// capture video from camera
// convert from rgb to yuv420
// encode using libaom
// decode using libaom
// make the video frame available

use crate::utils::yuv::*;

use anyhow::bail;
use av_data::{frame::FrameType, timeinfo::TimeInfo};
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
use std::time::Instant;
use tokio::sync::broadcast;

use libaom::{decoder::AV1Decoder, encoder::*};

#[derive(Clone)]
pub struct YuvFrame {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

pub fn capture_camera(
    frame_tx: broadcast::Sender<YuvFrame>,
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
        .reduce(|s1, s2| {
            // Choose RGB with 8 bit depth
            if s1.pixfmt == PixelFormat::Rgb(24) && s2.pixfmt != PixelFormat::Rgb(24) {
                return s1;
            }

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

    // configure AV1 encoder
    let color_scale = ColorScale::HdTv;
    let multiplier: usize = 1;

    // the camera will likely capture 1270x720. it's ok for width and height to be less than that.
    let frame_width = 512;
    let frame_height = 512;
    let fps = 1000.0 / (stream_descr.interval.as_millis() as f64);

    let mut encoder_config = match AV1EncoderConfig::new_with_usage(AomUsage::RealTime) {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1EncoderConfig: {e:?}"),
    };
    encoder_config.g_h = frame_width * multiplier as u32;
    encoder_config.g_w = frame_width * multiplier as u32;
    let mut encoder = match encoder_config.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };

    let pixel_format = *av_data::pixel::formats::YUV420;
    let pixel_format = Arc::new(pixel_format);

    // configure av1 decoder
    let mut decoder = AV1Decoder::<()>::new().map_err(|e| anyhow::anyhow!(e))?;

    // Start the camera capture
    let mut stream = dev.start_stream(&stream_descr)?;
    let start = Instant::now();
    println!("starting stream with description: {stream_descr:?}");

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || loop {
        let buf = stream.next().unwrap().unwrap();
        tx.send(buf.to_vec()).unwrap();
    });

    while let Ok(frame) = rx.recv() {
        if should_quit.load(Ordering::Relaxed) {
            println!("quitting camera capture");
            break;
        }
        let frame_time = start.elapsed();
        let frame_time_ms = frame_time.as_millis();

        let timestamp = frame_time_ms as f64 / fps;

        let yuv = {
            let p = frame.as_ptr();
            let len = frame_width * frame_height * 3;
            let s = std::ptr::slice_from_raw_parts(p, len as _);
            let s: &[u8] = unsafe { &*s };

            rgb_to_yuv420(s, frame_width as _, frame_height as _, color_scale)
        };

        let yuv_buf = YUV420Buf {
            data: yuv,
            width: frame_width as usize * multiplier,
            height: frame_height as usize * multiplier,
        };

        let frame = av_data::frame::Frame {
            kind: av_data::frame::MediaKind::Video(av_data::frame::VideoInfo::new(
                yuv_buf.width,
                yuv_buf.height,
                false,
                FrameType::I,
                pixel_format.clone(),
            )),
            buf: Box::new(yuv_buf),
            t: TimeInfo {
                pts: Some(timestamp as i64),
                ..Default::default()
            },
        };

        // test encoding
        if let Err(e) = encoder.encode(&frame) {
            eprintln!("encoding error: {e}");
            continue;
        }

        // test decoding
        while let Some(packet) = encoder.get_packet() {
            if let AOMPacket::Packet(p) = packet {
                if let Err(e) = decoder.decode(&p.data, None) {
                    eprintln!("decoding error: {e}");
                    continue;
                }

                while let Some((decoded_frame, _opt)) = decoder.get_frame() {
                    let frame = decoded_frame.buf;
                    let Ok(y) = frame.as_slice_inner(0) else {
                        eprintln!("failed to extract Y plane from frame");
                        continue;
                    };
                    let Ok(u) = frame.as_slice_inner(1) else {
                        eprintln!("failed to extract Cb plane from frame");
                        continue;
                    };
                    let Ok(v) = frame.as_slice_inner(2) else {
                        eprintln!("failed to extract Cr plane from frame");
                        continue;
                    };
                    let _ = frame_tx.send(YuvFrame {
                        y: y.to_vec(),
                        u: u.to_vec(),
                        v: v.to_vec(),
                    });
                }
            }
        }
    }

    Ok(())
}
