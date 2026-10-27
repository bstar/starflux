// Adapted from STAR/SCREEN, MIT, Copyright (c) 2026 bstar.
//! GStreamer owns live capture encoding; KIT owns remote decoding/presentation.
use anyhow::{Context, Result, ensure};
use crossbeam_channel::{Receiver, bounded};
use gst::prelude::*;
use starkit::terminal_graphics::{
    live_canvas::{Frame, PixelFormat, Update},
    media::{ToClient, ToHost},
    protocol::ServerMessage,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

// GStreamer retains the immutable allocation until the converter releases it.
// Avoid copying 8 MiB at 1080p (32 MiB at 4K) on every encoded frame.
struct SharedPixels(std::sync::Arc<Vec<u8>>);
impl AsRef<[u8]> for SharedPixels {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

struct Encoder {
    pipeline: gst::Pipeline,
    input: gst_app::AppSrc,
    packets: Receiver<Vec<u8>>,
    size: (u32, u32),
    count: u64,
    encoding: bool,
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
pub struct Video {
    encoder: Option<Encoder>,
    stream: String,
    session: u64,
    generation: u64,
    credit: usize,
    pipelined: bool,
    latest: Option<Frame>,
    pending: VecDeque<ToClient>,
    sent_at: Instant,
    failure: Option<String>,
    stats_began: Instant,
    stats_frames: u64,
    stats_bytes: u64,
    high_quality: bool,
    delivered: u64,
    feedback: u64,
}
impl Default for Video {
    fn default() -> Self {
        Self {
            encoder: None,
            stream: String::new(),
            session: 0,
            generation: 0,
            credit: 0,
            pipelined: false,
            latest: None,
            pending: VecDeque::new(),
            sent_at: Instant::now() - Duration::from_secs(1),
            failure: None,
            stats_began: Instant::now(),
            stats_frames: 0,
            stats_bytes: 0,
            high_quality: false,
            delivered: 0,
            feedback: 0,
        }
    }
}
impl Video {
    pub fn counters(&self) -> (u64, u64) {
        (self.delivered, self.feedback)
    }
    pub fn size(&self) -> Option<(u32, u32)> {
        self.encoder.as_ref().map(|e| e.size)
    }
    pub fn set_pipelined(&mut self, enabled: bool) {
        self.pipelined = enabled;
    }
    pub fn set_high_quality(&mut self, enabled: bool) {
        if self.high_quality != enabled {
            self.stop();
            self.high_quality = enabled;
        }
    }
    pub fn stop(&mut self) {
        if self.encoder.take().is_some() {
            self.pending.push_back(ToClient::CanvasVideoClose {
                session: self.session,
                generation: self.generation,
                stream: self.stream.clone(),
            });
        }
        self.credit = 0;
        self.latest = None;
    }
    fn open(&mut self, stream: String, generation: u64, frame: &Frame) -> Result<()> {
        gst::init()?;
        frame.validate()?;
        ensure!(
            frame.stride == frame.width * 4,
            "Live video needs packed pixels"
        );
        self.stop();
        let (max_width, max_height, max_packet, format, quality) = if self.high_quality {
            (
                3840,
                2160,
                8 * 1024 * 1024,
                "Y444",
                "pass=qual quantizer=18 bitrate=100000",
            )
        } else {
            (1280, 720, 2 * 1024 * 1024, "I420", "bitrate=12000")
        };
        let factor = (f64::from(max_width) / frame.width as f64)
            .min(f64::from(max_height) / frame.height as f64)
            .min(1.);
        let width = ((frame.width as f64 * factor) as u32 / 2 * 2).max(2);
        let height = ((frame.height as f64 * factor) as u32 / 2 * 2).max(2);
        let pipeline=gst::parse::launch(&format!("appsrc name=source is-live=true format=time block=false max-buffers=1 ! videoconvert ! videoscale ! video/x-raw,format={format},width={width},height={height} ! x264enc tune=zerolatency speed-preset=ultrafast threads=2 {quality} key-int-max=60 bframes=0 byte-stream=true ! video/x-h264,stream-format=byte-stream,alignment=au ! appsink name=video sync=false max-buffers=1 drop=false"))?.downcast::<gst::Pipeline>().map_err(|_|anyhow::anyhow!("Invalid live video encoder"))?;
        let input = pipeline
            .by_name("source")
            .unwrap()
            .downcast::<gst_app::AppSrc>()
            .unwrap();
        input.set_caps(Some(
            &gst::Caps::builder("video/x-raw")
                .field(
                    "format",
                    match frame.format {
                        PixelFormat::Bgra8 => "BGRA",
                        PixelFormat::Rgba8 => "RGBA",
                    },
                )
                .field("width", frame.width as i32)
                .field("height", frame.height as i32)
                .field("framerate", gst::Fraction::new(60, 1))
                .build(),
        ));
        let sink = pipeline
            .by_name("video")
            .unwrap()
            .downcast::<gst_app::AppSink>()
            .unwrap();
        let (tx, packets) = bounded(1);
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let map = sample
                        .buffer()
                        .ok_or(gst::FlowError::Error)?
                        .map_readable()
                        .map_err(|_| gst::FlowError::Error)?;
                    if map.is_empty() || map.len() > max_packet {
                        return Err(gst::FlowError::Error);
                    }
                    tx.try_send(map.as_slice().to_vec())
                        .map_err(|_| gst::FlowError::Error)?;
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        let encoder = Encoder {
            pipeline,
            input,
            packets,
            size: (frame.width, frame.height),
            count: 0,
            encoding: false,
        };
        encoder.pipeline.set_state(gst::State::Playing)?;
        self.encoder = Some(encoder);
        self.stream = stream;
        self.generation = generation;
        self.session = uuid::Uuid::new_v4().as_u128() as u64;
        self.pending.push_back(ToClient::CanvasVideoOpen {
            session: self.session,
            generation,
            stream: self.stream.clone(),
            width,
            height,
        });
        #[cfg(feature = "lossless-canvas")]
        if self.pipelined {
            self.pending.push_back(ToClient::CanvasVideoWindow {
                session: self.session,
                generation,
                frames: 2,
            });
        }
        tracing::info!(
            width,
            height,
            high_quality = self.high_quality,
            "Remote preview uses H.264 and local canvas decoding"
        );
        Ok(())
    }
    pub fn submit(&mut self, update: Update) -> Result<()> {
        match update {
            Update::Frame {
                stream,
                generation,
                frame,
            } => {
                if self.encoder.as_ref().is_none_or(|e| {
                    self.stream != stream
                        || self.generation != generation
                        || e.size != (frame.width, frame.height)
                }) {
                    self.open(stream, generation, &frame)?;
                }
                self.latest = Some(frame);
            }
            Update::Close { stream, generation } => {
                if stream == self.stream && generation >= self.generation {
                    self.stop();
                }
            }
        }
        Ok(())
    }
    pub fn receive(&mut self, message: &ToHost) {
        match message {
            ToHost::CanvasVideoCredit {
                session,
                generation,
            } if *session == self.session && *generation == self.generation => {
                self.credit = (self.credit + 1).min(if self.pipelined { 2 } else { 1 });
                self.feedback += 1;
            }
            ToHost::CanvasVideoError {
                session,
                generation,
                message,
            } if *session == self.session && *generation == self.generation => {
                tracing::warn!(%message, "Remote canvas video failed; falling back to image preview");
                self.failure = Some(format!("Remote video: {message}"));
                self.stop();
            }
            _ => {}
        }
    }
    pub fn warning(&mut self) -> Option<String> {
        if let Some(e) = &self.encoder
            && let Some(error) = e.pipeline.bus().unwrap().iter().find_map(|m| {
                if let gst::MessageView::Error(e) = m.view() {
                    Some(e.error().to_string())
                } else {
                    None
                }
            })
        {
            tracing::warn!(%error, "Remote canvas encoder failed; falling back to image preview");
            self.failure = Some(format!("Remote video encoder: {error}"));
            self.stop();
        }
        self.failure.take()
    }
    pub fn messages(&mut self, limit: usize) -> Result<Vec<ServerMessage>> {
        let mut messages = Vec::new();
        while messages.len() < limit {
            if let Some(message) = self.pending.pop_front() {
                messages.push(message);
                continue;
            }
            let Some(encoder) = &mut self.encoder else {
                break;
            };
            if self.credit > 0
                && let Ok(packet) = encoder.packets.try_recv()
            {
                if std::env::var_os("STARMUX_RENDER_TIMING").is_some() {
                    eprintln!("Video encode: us={}", self.sent_at.elapsed().as_micros());
                }
                self.credit -= 1;
                encoder.encoding = false;
                self.stats_frames += 1;
                self.delivered += 1;
                self.stats_bytes += (packet.len().div_ceil(3) * 4) as u64;
                messages.push(ToClient::CanvasVideoPacket {
                    session: self.session,
                    generation: self.generation,
                    data: gst::glib::base64_encode(&packet).into(),
                });
                continue;
            }
            // Prepare one frame while the previous frame travels and decodes.
            // Keep every encoded access unit; coalesce only untouched raw frames.
            if !encoder.encoding
                && let Some(frame) = self.latest.take()
            {
                let mut buffer = gst::Buffer::from_slice(SharedPixels(frame.pixels));
                buffer
                    .get_mut()
                    .unwrap()
                    .set_pts(gst::ClockTime::from_nseconds(
                        encoder.count * 1_000_000_000 / 60,
                    ));
                buffer
                    .get_mut()
                    .unwrap()
                    .set_duration(gst::ClockTime::from_nseconds(1_000_000_000 / 60));
                encoder
                    .input
                    .push_buffer(buffer)
                    .context("Sending live video frame")?;
                encoder.count += 1;
                // The compositor owns frame pacing; prepare the newest raw frame
                // immediately so a second clock does not discard late source frames.
                self.sent_at = Instant::now();
                encoder.encoding = true;
            }
            break;
        }
        if self.stats_began.elapsed() >= Duration::from_secs(2) {
            tracing::debug!(
                frames = self.stats_frames,
                bytes = self.stats_bytes,
                elapsed_ms = self.stats_began.elapsed().as_millis(),
                "H.264 preview transport"
            );
            self.stats_frames = 0;
            self.stats_bytes = 0;
            self.stats_began = Instant::now();
        }
        Ok(messages
            .into_iter()
            .map(|message| ServerMessage::Media { message })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn update(generation: u64, color: u8) -> Update {
        Update::Frame {
            stream: "desktop".into(),
            generation,
            frame: Frame {
                width: 64,
                height: 32,
                stride: 64 * 4,
                format: PixelFormat::Rgba8,
                presentation_ns: 0,
                pixels: Arc::new(vec![color; 64 * 32 * 4]),
            },
        }
    }
    #[test]
    fn full_hd_stream_preserves_native_pixels_and_color_edges() {
        gst::init().unwrap();
        let mut video = Video::default();
        video.set_high_quality(true);
        let (width, height) = (1920u32, 1080u32);
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            pixel.copy_from_slice(if i % 2 == 0 {
                &[230, 20, 40, 255]
            } else {
                &[20, 220, 230, 255]
            });
        }
        video
            .submit(Update::Frame {
                stream: "desktop".into(),
                generation: 1,
                frame: Frame {
                    width,
                    height,
                    stride: width * 4,
                    format: PixelFormat::Rgba8,
                    presentation_ns: 0,
                    pixels: Arc::new(pixels),
                },
            })
            .unwrap();
        let opened = video.messages(1).unwrap();
        assert!(matches!(
            opened.as_slice(),
            [ServerMessage::Media {
                message: ToClient::CanvasVideoOpen {
                    width: 1920,
                    height: 1080,
                    ..
                }
            }]
        ));
        video.receive(&ToHost::CanvasVideoCredit {
            session: video.session,
            generation: 1,
        });
        video.messages(1).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let packet = loop {
            if let Some(data) =
                video
                    .messages(1)
                    .unwrap()
                    .into_iter()
                    .find_map(|message| match message {
                        ServerMessage::Media {
                            message: ToClient::CanvasVideoPacket { data, .. },
                        } => Some(data),
                        _ => None,
                    })
            {
                break gst::glib::base64_decode(&data);
            }
            assert!(
                Instant::now() < deadline,
                "Encoder did not return a full-HD packet: {:?}",
                video.warning()
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        let pipeline = gst::parse::launch("appsrc name=input is-live=true format=time ! h264parse ! avdec_h264 ! videoconvert ! video/x-raw,format=RGBA ! appsink name=output sync=false")
            .unwrap().downcast::<gst::Pipeline>().unwrap();
        let input = pipeline
            .by_name("input")
            .unwrap()
            .downcast::<gst_app::AppSrc>()
            .unwrap();
        input.set_caps(Some(
            &gst::Caps::builder("video/x-h264")
                .field("stream-format", "byte-stream")
                .field("alignment", "au")
                .build(),
        ));
        let output = pipeline
            .by_name("output")
            .unwrap()
            .downcast::<gst_app::AppSink>()
            .unwrap();
        pipeline.set_state(gst::State::Playing).unwrap();
        input
            .push_buffer(gst::Buffer::from_mut_slice(packet))
            .unwrap();
        input.end_of_stream().unwrap();
        let sample = output.try_pull_sample(gst::ClockTime::from_seconds(5));
        pipeline.set_state(gst::State::Null).unwrap();
        let sample = sample.expect("Could not decode full-HD preview");
        let info = gst_video::VideoInfo::from_caps(sample.caps().unwrap()).unwrap();
        assert_eq!((info.width(), info.height()), (1920, 1080));
        let map = sample.buffer().unwrap().map_readable().unwrap();
        let offset = info.stride()[0] as usize * 540 + 960 * 4;
        let a = &map[offset..offset + 4];
        let b = &map[offset + 4..offset + 8];
        assert!(
            a[0] > 190 && a[1] < 60 && b[0] < 60 && b[1] > 180,
            "Full-HD one-pixel color edges were blurred: {a:?}, {b:?}"
        );
        // An older frontend must retain its original dimension contract.
        video.set_high_quality(false);
        video
            .submit(Update::Frame {
                stream: "desktop".into(),
                generation: 2,
                frame: Frame {
                    width,
                    height,
                    stride: width * 4,
                    format: PixelFormat::Rgba8,
                    presentation_ns: 0,
                    pixels: Arc::new(vec![128; width as usize * height as usize * 4]),
                },
            })
            .unwrap();
        assert!(video.messages(3).unwrap().iter().any(|message| matches!(
            message,
            ServerMessage::Media {
                message: ToClient::CanvasVideoOpen {
                    width: 1280,
                    height: 720,
                    ..
                }
            }
        )));
    }

    #[test]
    fn encoder_prepares_one_frame_ahead_but_sends_only_with_current_epoch_credit() {
        gst::init().unwrap();
        let mut video = Video::default();
        video.submit(update(1, 20)).unwrap();
        video.submit(update(1, 90)).unwrap();
        let opened = video.messages(1).unwrap();
        assert!(matches!(
            opened.as_slice(),
            [ServerMessage::Media {
                message: ToClient::CanvasVideoOpen { .. }
            }]
        ));
        assert_eq!(video.latest.as_ref().unwrap().pixels[0], 90);
        assert_eq!(video.encoder.as_ref().unwrap().count, 0);
        let session = video.session;
        video.receive(&ToHost::CanvasVideoCredit {
            session,
            generation: 0,
        });
        video.messages(1).unwrap();
        assert_eq!(video.encoder.as_ref().unwrap().count, 1);
        video.receive(&ToHost::CanvasVideoCredit {
            session,
            generation: 1,
        });
        video.submit(update(1, 120)).unwrap();
        video.submit(update(1, 150)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if video.messages(1).unwrap().iter().any(|m| {
                matches!(
                    m,
                    ServerMessage::Media {
                        message: ToClient::CanvasVideoPacket { .. }
                    }
                )
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "encoder did not return an access unit: {:?}",
                video.warning()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(video.encoder.as_ref().unwrap().count, 1);
        assert_eq!(video.latest.as_ref().unwrap().pixels[0], 150);
        video.sent_at = Instant::now() - Duration::from_secs(1);
        assert!(video.messages(1).unwrap().is_empty());
        assert_eq!(video.encoder.as_ref().unwrap().count, 2);
        let deadline = Instant::now() + Duration::from_secs(3);
        while video.encoder.as_ref().unwrap().packets.is_empty() {
            assert!(video.messages(1).unwrap().is_empty());
            assert!(Instant::now() < deadline, "lookahead encoding stalled");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            video.messages(1).unwrap().is_empty(),
            "encoded frames still require credit"
        );
        video.receive(&ToHost::CanvasVideoCredit {
            session,
            generation: 1,
        });
        assert!(matches!(
            video.messages(1).unwrap().as_slice(),
            [ServerMessage::Media {
                message: ToClient::CanvasVideoPacket { .. }
            }]
        ));
        video.submit(update(2, 200)).unwrap();
        video.receive(&ToHost::CanvasVideoCredit {
            session,
            generation: 1,
        });
        video.messages(2).unwrap();
        assert_ne!(video.session, session);
        let deadline = Instant::now() + Duration::from_millis(50);
        while Instant::now() < deadline {
            assert!(video.messages(1).unwrap().is_empty());
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(video.encoder.as_ref().unwrap().count, 1);
        video.stop();
        assert!(video.latest.is_none() && video.encoder.is_none());
    }
}
