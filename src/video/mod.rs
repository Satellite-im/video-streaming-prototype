// the video-codec-cli from warp was moved here. it will be adapted to send the captured+encoded camera input to a browser.

// capture video from camera
// convert from rgb to yuv420
// encode using libaom
// decode using libaom
// make the video frame available

use crate::utils::yuv::*;

use async_stream::stream;
use futures_core::stream::{BoxStream, Stream};
use futures_util::StreamExt;

use anyhow::{bail, Result};
use av_data::{frame::FrameType, timeinfo::TimeInfo};
use eye::{
    colorconvert::Device,
    hal::{
        format::PixelFormat,
        traits::{Context as _, Device as _, Stream as _},
        PlatformContext,
    },
};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    sync::Arc,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use libaom::{decoder::AV1Decoder, encoder::*};

type CameraStream<'a> = BoxStream<'a, Vec<u8>>;
type AvPacketStream<'a> = BoxStream<'a, av_data::packet::Packet>;
pub type AvFrameStream<'a> = BoxStream<'a, Box<dyn av_data::frame::FrameBuffer>>;

struct Camera<'a> {
    pub stream: CameraStream<'a>,
    pub descr: eye::hal::stream::Descriptor,
}

pub fn get_stream<'a>() -> Result<AvFrameStream<'a>> {
    // shows how to read frames from eye-rs
    let cs = camera_stream()?;
    // shows how to encode frames
    let avs = av_packet_stream(cs)?;
    // shows how to decode frames
    let afs = av_frame_stream(avs)?;
    Ok(afs)
}

fn camera_stream<'a>() -> Result<Camera<'a>> {
    // Create a context
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

            // Strive for HD (1280 x 720)
            let distance = |width: u32, height: u32| {
                f32::sqrt(((640 - width as i32).pow(2) + (480 - height as i32).pow(2)) as f32)
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

    // Start the stream
    let mut stream = dev.start_stream(&stream_descr)?;

    println!("starting stream with description: {stream_descr:?}");

    let s = stream! {
        while let Some(Ok(buf)) = stream.next() {
            yield buf.to_vec();
        }
    };

    Ok(Camera {
        stream: Box::pin(s),
        descr: stream_descr,
    })
}
fn av_packet_stream<'a>(mut camera: Camera<'a>) -> Result<AvPacketStream<'a>> {
    let color_scale = ColorScale::HdTv;
    let multiplier: usize = 1;

    let frame_width = camera.descr.width;
    let frame_height = camera.descr.height;
    let fps = 1000.0 / (camera.descr.interval.as_millis() as f64);

    let mut encoder_config = match AV1EncoderConfig::new_with_usage(AomUsage::RealTime) {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1EncoderConfig: {e:?}"),
    };
    encoder_config.g_h = frame_height * multiplier as u32;
    encoder_config.g_w = frame_width * multiplier as u32;
    let mut encoder = match encoder_config.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };

    let pixel_format = *av_data::pixel::formats::YUV420;
    let pixel_format = Arc::new(pixel_format);

    let s = stream! {
        let start = Instant::now();
        while let Some(frame) = camera.stream.next().await {
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

            // println!("encoding");
            if let Err(e) = encoder.encode(&frame) {
                eprintln!("encoding error: {e}");
                continue;
            }

            // println!("calling get_packet");
            while let Some(packet) = encoder.get_packet() {
                if let AOMPacket::Packet(p) = packet {
                   yield p;
                }
            }
        }
    };
    Ok(Box::pin(s))
}

fn av_frame_stream<'a>(mut packet_stream: AvPacketStream<'a>) -> Result<AvFrameStream<'a>> {
    let mut decoder = AV1Decoder::<()>::new().map_err(|e| anyhow::anyhow!(e))?;
    let s = stream! {
        while let Some(packet) = packet_stream.next().await {
            if let Err(e) = decoder.decode(&packet.data, None) {
                eprintln!("decoding error: {e}");
                continue;
            }

            while let Some((decoded_frame, _opt)) = decoder.get_frame() {
                yield decoded_frame.buf
            }
        }
    };

    Ok(Box::pin(s))
}
