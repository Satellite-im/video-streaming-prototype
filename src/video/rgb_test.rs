// the video-codec-cli from warp was moved here. it will be adapted to send the captured+encoded camera input to a browser.

// capture video from camera
// convert from rgb to yuv420
// encode using libaom
// decode using libaom
// make the video frame available

use anyhow::bail;
use av_data::{
    frame::{FrameType, MediaKind, VideoInfo},
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

    //let fps = 1000.0 / (stream_descr.interval.as_millis() as f64);

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
        .width(frame_width as _)
        .height(frame_height as _)
        .bit_depth(8)
        .input_bit_depth(8)
        .timebase(Rational64::new(1, 1000))
        .pass(0 /*AOM_RC_ONE_PASS*/);

    let mut encoder = match av1_cfg.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };

    let pixel_format = av_data::pixel::formats::RGB24;
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

    while let Ok(frame) = rx.recv() {
        //println!("got frame");
        if should_quit.load(Ordering::Relaxed) {
            println!("quitting camera capture rx thread");
            break;
        }

        // this is to test the webgl code separately from the libaom code
        let half_width: usize = (stream_descr.width as usize - 512) / 2;
        let half_height: usize = (stream_descr.height as usize - 512) / 2;
        let mut v = vec![];
        v.reserve(512 * 512 * 3);
        for row in frame
            .chunks_exact(stream_descr.width as usize * 3)
            .skip(half_height)
            .take(512)
        {
            v.extend_from_slice(&row[(half_width * 3)..(half_width * 3 + 512 * 3)]);
        }
        let _ = frame_tx.send(v);

        continue;

        let mut av_frame = av_data::frame::Frame::new_default_frame(
            MediaKind::Video(VideoInfo::new(
                frame_width as _,
                frame_height as _,
                false,
                FrameType::OTHER,
                pixel_format.clone(),
            )),
            Some(t.clone()),
        );

        debug_assert!(av_frame.buf.count() == 3);

        {
            let rgb_plane = av_frame.buf.as_mut_slice_inner(0).unwrap();
            rgb_plane.copy_from_slice(&frame);
        }

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
                let _ = frame_tx.send(y_buf.to_vec());
            }
        }
    }

    Ok(())
}
