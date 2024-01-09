// the video-codec-cli from warp was moved here. it will be adapted to send the captured+encoded camera input to a browser.

// capture video from camera
// convert from rgb to yuv420
// encode using libaom
// decode using libaom
// make the video frame available

use crate::utils::yuv::*;

use anyhow::bail;
use av_data::frame::{Frame, VideoInfo};
use av_data::rational::*;
use av_data::{frame::FrameType, rational::Rational64, timeinfo::TimeInfo};
use eye::{
    colorconvert::Device,
    hal::{
        format::PixelFormat,
        traits::{Context as _, Device as _, Stream as _},
        PlatformContext,
    },
};
use image::ImageBuffer;
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

    // the camera will likely capture 1270x720. it's ok for width and height to be less than that.
    let frame_width = 512;
    let frame_height = 512;
    let fps = 1000.0 / (stream_descr.interval.as_millis() as f64);

    let t = TimeInfo {
        pts: Some(0),
        dts: Some(0),
        duration: Some(1),
        timebase: Some(Rational64::new(1, 1000)),
        user_private: None,
    };

    let mut av1_cfg = AV1EncoderConfig::new().map_err(|e| anyhow::anyhow!("{e}"))?;
    av1_cfg = av1_cfg
        .usage(AomUsage::RealTime)
        .rc_min_quantizer(0 /*determine programmatically */)
        .rc_max_quantizer(0 /*determine programmatically */)
        .rc_end_usage(1 /*AOM_CBR*/)
        .threads(4 /*max threads*/)
        .profile(BitstreamProfile::Profile0 /*8 bit 4:2:0*/)
        .width(frame_width)
        .height(frame_height)
        .bit_depth(8)
        .input_bit_depth(8)
        .timebase(Rational64::new(1, 1000))
        .pass(0 /*AOM_RC_ONE_PASS*/);

    let mut encoder = match av1_cfg.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };

    // encoder
    //     .control(4 /*AOME_SET_CQ_LEVEL*/, 4)
    //     .map_err(|e: u32| anyhow::anyhow!("encoder.control failed: {e}"))?;
    // encoder
    //     .control(2 /*AOME_SET_CPUUSED*/, 2)
    //     .map_err(|e| anyhow::anyhow!("encoder.control failed: {e}"))?;

    /*let mut encoder_config = match AV1EncoderConfig::new_with_usage(AomUsage::RealTime) {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1EncoderConfig: {e:?}"),
    };
    encoder_config.g_h = frame_height as u32;
    encoder_config.g_w = frame_width as u32;
    let mut encoder = match encoder_config.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };*/

    let pixel_format = av_data::pixel::formats::YUV420;
    let pixel_format = Arc::new(pixel_format.clone());

    // configure av1 decoder
    let mut decoder = AV1Decoder::<()>::new().map_err(|e| anyhow::anyhow!(e))?;

    // Start the camera capture
    let mut stream = dev.start_stream(&stream_descr)?;
    println!("starting stream with description: {stream_descr:?}");

    let (tx, rx) = std::sync::mpsc::channel();
    let should_quit2 = should_quit.clone();
    std::thread::spawn(move || loop {
        if should_quit2.load(Ordering::Relaxed) {
            println!("quitting camera capture tx thread");
            return;
        }
        let buf = stream.next().unwrap().unwrap();
        tx.send(buf.to_vec()).unwrap();
    });
    let mut frame_counter = 0;
    while let Ok(frame) = rx.recv() {
        //println!("got frame");
        if should_quit.load(Ordering::Relaxed) {
            println!("quitting camera capture rx thread");
            break;
        }

        let yuv = rgb_to_yuv420(
            &frame,
            frame_width as _,
            frame_height as _,
            stream_descr.width as _,
            stream_descr.height as _,
            color_scale,
        );

        // this is to test the webgl code separately from the libaom code
        /*let (y, uv) = yuv.split_at(frame_width as usize * frame_height as usize);
        let (u, v) = uv.split_at(uv.len() / 2);
        let _ = frame_tx.send(YuvFrame {
            y: y.to_vec(),
            u: u.to_vec(),
            v: v.to_vec(),
        });

        continue;*/

        let yuv_buf = YUV420Buf {
            data: yuv,
            width: frame_width as usize,
            height: frame_height as usize,
        };
        frame_counter += 1;
        let mut frame = av_data::frame::Frame {
            kind: av_data::frame::MediaKind::Video(av_data::frame::VideoInfo::new(
                yuv_buf.width,
                yuv_buf.height,
                false,
                FrameType::I,
                pixel_format.clone(),
            )),
            buf: Box::new(yuv_buf),
            t: t.clone(),
        };

        frame.t.pts = Some(frame_counter);

        /*let v = VideoInfo::new(
            frame_width as usize,
            frame_height as usize,
            false,
            FrameType::OTHER,
            pixel_format.clone(),
        );

        let frame = Frame::new_default_frame(v, Some(t.clone()));*/

        // test encoding
        if let Err(e) = encoder.encode(&frame) {
            eprintln!("encoding error: {e}");
            continue;
        }

        // test decoding
        while let Some(packet) = encoder.get_packet() {
            let AOMPacket::Packet(p) = packet else {
                continue;
            };
            if let Err(e) = decoder.decode(&p.data, None) {
                eprintln!("decoding error: {e}");
                continue;
            }

            while let Some((decoded_frame, _opt)) = decoder.get_frame() {
                let frame = decoded_frame.buf;
                let Ok(y_buf) = frame.as_slice_inner(0) else {
                    eprintln!("failed to extract Y plane from frame");
                    continue;
                };
                let Ok(y_stride) = frame.linesize(0) else {
                    eprintln!("failed to get stride for Y plane");
                    continue;
                };
                let Ok(u_buf) = frame.as_slice_inner(1) else {
                    eprintln!("failed to extract Cb plane from frame");
                    continue;
                };
                let Ok(u_stride) = frame.linesize(1) else {
                    eprintln!("failed to get stride for U plane");
                    continue;
                };
                let Ok(v_buf) = frame.as_slice_inner(2) else {
                    eprintln!("failed to extract Cr plane from frame");
                    continue;
                };
                let Ok(v_stride) = frame.linesize(2) else {
                    eprintln!("failed to get stride for V plane");
                    continue;
                };

                // let mut y = vec![];
                // y.reserve(512 * 512);
                // let mut u = vec![];
                // u.reserve(256 * 256);
                // let mut v = vec![];
                // v.reserve(256 * 256);

                // for row in y_buf.chunks_exact(y_stride) {
                //     y.extend_from_slice(&row[0..512]);
                // }
                // for row in u_buf.chunks_exact(u_stride) {
                //     u.extend_from_slice(&row[0..256]);
                // }
                // for row in v_buf.chunks_exact(v_stride) {
                //     v.extend_from_slice(&row[0..256]);
                // }

                // due to the implementation of FrameBuf for Yuv420BUf, the above code isn't needed.
                let y = y_buf;
                let u = u_buf;
                let v = v_buf;
                println!(
                    "sending frame of size {}, {}, {}",
                    y.len(),
                    u.len(),
                    v.len()
                );
                let _ = frame_tx.send(YuvFrame {
                    y: y.to_vec(),
                    u: u.to_vec(),
                    v: v.to_vec(),
                });
            }
        }
    }

    Ok(())
}
