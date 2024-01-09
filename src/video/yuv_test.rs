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

use libaom::{decoder::AV1Decoder, encoder::*};

#[derive(Clone)]
pub struct YuvFrame {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

pub fn capture_stream(
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
        // Choose RGB with 8 bit depth
        .filter(|s| matches!(s.pixfmt, PixelFormat::Rgb(24)))
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

    // configure AV1 encoder
    let color_scale = ColorScale::HdTv;

    // the camera will likely capture 1270x720. it's ok for width and height to be less than that.
    let frame_width = 512 as usize;
    let frame_height = 512 as usize;
    let fps = 1000.0 / (stream_descr.interval.as_millis() as f64);

    let t = TimeInfo {
        pts: Some(0),
        dts: Some(0),
        duration: Some(1),
        timebase: Some(Rational64::new(1, fps as _)),
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
        .width(frame_width as _)
        .height(frame_height as _)
        .bit_depth(8)
        .input_bit_depth(8)
        .timebase(t.timebase.unwrap())
        .pass(0 /*AOM_RC_ONE_PASS*/);

    let mut encoder = match av1_cfg.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };

    // warning: pixels in range [16, 235]
    let pixel_format = av_data::pixel::formats::YUV420;
    let pixel_format = Arc::new(pixel_format.clone());

    // configure av1 decoder
    let mut decoder = AV1Decoder::<()>::new().map_err(|e| anyhow::anyhow!(e))?;

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
    let mut frame_counter = 0;
    while let Ok(frame) = rx.recv() {
        //println!("got frame");
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
            color_scale,
        );
        drop(frame);

        // this is to test the webgl code separately from the libaom code
        /*let (y, uv) = yuv.split_at(frame_width as usize * frame_height as usize);
        let (u, v) = uv.split_at(uv.len() / 2);
        let _ = frame_tx.send(YuvFrame {
            y: y.to_vec(),
            u: u.to_vec(),
            v: v.to_vec(),
        });

        continue;*/

        // this was an attempt at constructing the Frame differently
        /*let mut av_frame = av_data::frame::Frame::new_default_frame(
            MediaKind::Video(VideoInfo::new(
                frame_width as _,
                frame_height as _,
                false,
                FrameType::I,
                pixel_format.clone(),
            )),
            Some(t.clone()),
        );

        debug_assert!(av_frame.buf.count() == 3);

        let (y_input, uv_input) = yuv.split_at(y_len);
        let (u_input, v_input) = uv_input.split_at(uv_len);

        {
            let y_plane = av_frame.buf.as_mut_slice_inner(0).unwrap();
            y_plane.copy_from_slice(y_input);
        }

        {
            let u_plane = av_frame.buf.as_mut_slice_inner(1).unwrap();
            u_plane.copy_from_slice(u_input);
        }

        {
            let v_plane = av_frame.buf.as_mut_slice_inner(2).unwrap();
            v_plane.copy_from_slice(v_input);
        }*/

        let yuv_buf = YUV420Buf {
            data: yuv,
            width: frame_width as usize,
            height: frame_height as usize,
        };

        // insert key frames
        let frame_type = if frame_counter % 30 == 0 {
            FrameType::P
        } else {
            FrameType::I
        };

        let mut av_frame = av_data::frame::Frame {
            kind: av_data::frame::MediaKind::Video(av_data::frame::VideoInfo::new(
                yuv_buf.width,
                yuv_buf.height,
                false,
                frame_type,
                pixel_format.clone(),
            )),
            buf: Box::new(yuv_buf),
            t: t.clone(),
        };

        av_frame.t.pts = Some(frame_counter);
        frame_counter += 1;

        // test encoding
        if let Err(e) = encoder.encode(&av_frame) {
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
                let Ok(_y_stride) = frame.linesize(0) else {
                    eprintln!("failed to get stride for Y plane");
                    continue;
                };
                let Ok(u_buf) = frame.as_slice_inner(1) else {
                    eprintln!("failed to extract Cb plane from frame");
                    continue;
                };
                let Ok(_u_stride) = frame.linesize(1) else {
                    eprintln!("failed to get stride for U plane");
                    continue;
                };
                let Ok(v_buf) = frame.as_slice_inner(2) else {
                    eprintln!("failed to extract Cr plane from frame");
                    continue;
                };
                let Ok(_v_stride) = frame.linesize(2) else {
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
