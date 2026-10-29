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
    output_size: (u32, u32),
    format: &'static str,
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
    last_packet: Option<Instant>,
    last_source_size: Option<(u32, u32)>,
    responsive_stage: u8,
    reopen: bool,
    credit_wait_started: Option<Instant>,
    slow_credit_waits: VecDeque<Instant>,
    max_credit_wait_ms: f64,
    packet_sizes: VecDeque<usize>,
    delivery_gaps_ms: VecDeque<f64>,
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
            last_packet: None,
            last_source_size: None,
            responsive_stage: 0,
            reopen: false,
            credit_wait_started: None,
            slow_credit_waits: VecDeque::new(),
            max_credit_wait_ms: 0.,
            packet_sizes: VecDeque::new(),
            delivery_gaps_ms: VecDeque::new(),
        }
    }
}
impl Video {
    pub fn counters(&self) -> (u64, u64) {
        (self.delivered, self.feedback)
    }
    pub fn statistics(&self) -> serde_json::Value {
        let mut sizes = self.packet_sizes.iter().copied().collect::<Vec<_>>();
        sizes.sort_unstable();
        let mut gaps = self.delivery_gaps_ms.iter().copied().collect::<Vec<_>>();
        gaps.sort_by(f64::total_cmp);
        serde_json::json!({
            "bitrate_limit_kbps":if self.high_quality {24000} else {12000},
            "refresh":"cyclic",
            "max_packet_bytes":sizes.last(),
            "p95_packet_bytes":sizes.get(sizes.len()*95/100),
            "max_delivery_gap_ms":gaps.last(),
            "p95_delivery_gap_ms":gaps.get(gaps.len()*95/100),
            "source_size":self.encoder.as_ref().map(|e| e.size).or(self.last_source_size),
            "active":self.encoder.is_some(),
            "responsive_stage":self.responsive_stage,
            "encoded_size":self.encoder.as_ref().map(|e|e.output_size),
            "chroma":self.encoder.as_ref().map(|e|e.format),
            "max_frontend_credit_wait_ms":self.max_credit_wait_ms,
            "delivery_fps":if gaps.is_empty() {None} else {Some(1000.*gaps.len() as f64/gaps.iter().sum::<f64>().max(0.001))},
            "high_quality":self.high_quality,
            "frame_window":if self.pipelined {2} else {1},
            "available_credits":self.credit,
            "packets_sent":self.delivered,
            "presentation_credits":self.feedback
        })
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
        if let Some(encoder) = self.encoder.take() {
            self.last_source_size = Some(encoder.size);
            self.pending.push_back(ToClient::CanvasVideoClose {
                session: self.session,
                generation: self.generation,
                stream: self.stream.clone(),
            });
        }
        self.credit = 0;
        self.latest = None;
        self.last_packet = None;
        self.credit_wait_started = None;
    }
    fn open(&mut self, stream: String, generation: u64, frame: &Frame) -> Result<()> {
        gst::init()?;
        frame.validate()?;
        ensure!(
            frame.stride == frame.width * 4,
            "Live video needs packed pixels"
        );
        self.stop();
        self.packet_sizes.clear();
        self.delivery_gaps_ms.clear();
        self.slow_credit_waits.clear();
        self.max_credit_wait_ms = 0.;
        self.reopen = false;
        let (max_width, max_height, max_packet, format, quality) = if self.high_quality {
            (
                3840,
                2160,
                8 * 1024 * 1024,
                if self.responsive_stage == 0 {
                    "Y444"
                } else {
                    "I420"
                },
                "pass=qual quantizer=18 bitrate=24000 vbv-buf-capacity=600",
            )
        } else {
            (
                1280,
                720,
                2 * 1024 * 1024,
                "I420",
                "bitrate=12000 vbv-buf-capacity=50",
            )
        };
        let factor = (f64::from(max_width) / frame.width as f64)
            .min(f64::from(max_height) / frame.height as f64)
            .min(1.);
        let width = ((frame.width as f64 * factor) as u32 / 2 * 2).max(2);
        let height = ((frame.height as f64 * factor) as u32 / 2 * 2).max(2);
        let pipeline=gst::parse::launch(&format!("appsrc name=source is-live=true format=time block=false max-buffers=1 ! videoconvert ! videoscale ! video/x-raw,format={format},width={width},height={height} ! x264enc tune=zerolatency speed-preset=ultrafast {quality} key-int-max=60 intra-refresh=true bframes=0 byte-stream=true ! video/x-h264,stream-format=byte-stream,alignment=au ! appsink name=video sync=false max-buffers=1 drop=false"))?.downcast::<gst::Pipeline>().map_err(|_|anyhow::anyhow!("Invalid live video encoder"))?;
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
            output_size: (width, height),
            format,
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
                if self.reopen
                    || self.encoder.as_ref().is_none_or(|e| {
                        self.stream != stream
                            || self.generation != generation
                            || e.size != (frame.width, frame.height)
                    })
                {
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
                if let Some(began) = self.credit_wait_started.take() {
                    let wait = began.elapsed().as_secs_f64() * 1000.;
                    self.max_credit_wait_ms = self.max_credit_wait_ms.max(wait);
                    if wait > 20. {
                        self.slow_credit_waits.push_back(Instant::now());
                        if self.slow_credit_waits.len() > 64 {
                            self.slow_credit_waits.pop_front();
                        }
                    }
                }
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
    fn adapt_to_frontend(&mut self) {
        // Decoder support does not guarantee it can present native 4:4:4 at
        // interactive rates. Adapt only after sustained, measured credit stalls,
        // never for a quiet source or a single resize/startup interruption.
        while self
            .slow_credit_waits
            .front()
            .is_some_and(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.slow_credit_waits.pop_front();
        }
        let count = self.delivery_gaps_ms.len();
        if self.high_quality
            && self.encoder.is_some()
            && !self.reopen
            && self.responsive_stage == 0
            && count >= 60
            && self.slow_credit_waits.len() >= 6
            && self.delivery_gaps_ms.iter().sum::<f64>() / count as f64 > 1000. / 45.
        {
            self.responsive_stage += 1;
            self.reopen = true;
            tracing::info!(
                stage = self.responsive_stage,
                "Adapting app video to frontend presentation pressure"
            );
        }
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
            if self.credit == 0 && (self.latest.is_some() || !encoder.packets.is_empty()) {
                self.credit_wait_started.get_or_insert_with(Instant::now);
            }
            if self.credit > 0
                && let Ok(packet) = encoder.packets.try_recv()
            {
                if std::env::var_os("STARFLUX_RENDER_TIMING").is_some() {
                    eprintln!("Video encode: us={}", self.sent_at.elapsed().as_micros());
                }
                let now = Instant::now();
                if let Some(prior) = self.last_packet.replace(now) {
                    self.delivery_gaps_ms
                        .push_back(now.duration_since(prior).as_secs_f64() * 1000.);
                    if self.delivery_gaps_ms.len() > 240 {
                        self.delivery_gaps_ms.pop_front();
                    }
                }
                self.packet_sizes.push_back(packet.len());
                if self.packet_sizes.len() > 240 {
                    self.packet_sizes.pop_front();
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
            // Do not encode ahead of presentation capacity. A prepared H.264
            // frame cannot be replaced safely when newer input arrives, whereas
            // raw frames can keep coalescing until the viewer has room. The
            // negotiated window still overlaps encoding with network/presentation.
            if self.credit > 0
                && !encoder.encoding
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
        self.adapt_to_frontend();
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
    fn slow_frontend_renegotiates_chroma_without_reducing_native_resolution() {
        let mut video = Video::default();
        video.set_high_quality(true);
        video.submit(update(1, 90)).unwrap();
        video.messages(1).unwrap();
        assert_eq!(video.encoder.as_ref().unwrap().format, "Y444");
        video.delivery_gaps_ms.extend([16.; 60]);
        video.slow_credit_waits = std::iter::repeat_n(Instant::now(), 8).collect();
        video.adapt_to_frontend();
        assert_eq!(
            video.responsive_stage, 0,
            "Fast presentation must keep native chroma"
        );
        video.delivery_gaps_ms = std::iter::repeat_n(35., 60).collect();
        video.slow_credit_waits.clear();
        video.adapt_to_frontend();
        assert_eq!(
            video.responsive_stage, 0,
            "A slow producer without credit pressure must not degrade quality"
        );
        for _ in 0..8 {
            video.credit = 0;
            video.credit_wait_started = Some(Instant::now() - Duration::from_millis(30));
            video.receive(&ToHost::CanvasVideoCredit {
                session: video.session,
                generation: 1,
            });
        }
        video.adapt_to_frontend();
        assert_eq!(video.responsive_stage, 1);
        let previous = video.session;
        video.submit(update(1, 100)).unwrap();
        assert_ne!(
            video.session, previous,
            "The decoder must receive a new session for a new format"
        );
        let encoder = video.encoder.as_ref().unwrap();
        assert_eq!(encoder.format, "I420");
        assert_eq!(encoder.output_size, (64, 32));
        video.delivery_gaps_ms = std::iter::repeat_n(35., 60).collect();
        video.slow_credit_waits = std::iter::repeat_n(Instant::now(), 8).collect();
        video.adapt_to_frontend();
        video
            .submit(Update::Frame {
                stream: "desktop".into(),
                generation: 1,
                frame: Frame {
                    width: 1920,
                    height: 1080,
                    stride: 1920 * 4,
                    format: PixelFormat::Rgba8,
                    presentation_ns: 0,
                    pixels: Arc::new(vec![128; 1920 * 1080 * 4]),
                },
            })
            .unwrap();
        let encoder = video.encoder.as_ref().unwrap();
        assert_eq!(encoder.size, (1920, 1080));
        assert_eq!(encoder.output_size, (1920, 1080));
        assert_eq!(video.statistics()["responsive_stage"], 1);
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
    fn encoder_keeps_cursor_frames_replaceable_until_current_epoch_credit() {
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
        assert_eq!(video.encoder.as_ref().unwrap().count, 0);
        video.receive(&ToHost::CanvasVideoCredit {
            session,
            generation: 1,
        });
        video.messages(1).unwrap();
        assert_eq!(video.encoder.as_ref().unwrap().count, 1);
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
        assert_eq!(video.encoder.as_ref().unwrap().count, 1);
        let deadline = Instant::now() + Duration::from_millis(50);
        while Instant::now() < deadline {
            assert!(video.messages(1).unwrap().is_empty());
            video.submit(update(1, 180)).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(video.encoder.as_ref().unwrap().count, 1);
        assert_eq!(video.latest.as_ref().unwrap().pixels[0], 180);
        video.receive(&ToHost::CanvasVideoCredit {
            session,
            generation: 1,
        });
        assert!(video.messages(1).unwrap().is_empty());
        assert_eq!(video.encoder.as_ref().unwrap().count, 2);
        assert!(
            video.latest.is_none(),
            "The latest cursor position was encoded"
        );
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
        assert_eq!(video.encoder.as_ref().unwrap().count, 0);
        let diagnostics = video.statistics();
        video.stop();
        assert!(video.latest.is_none() && video.encoder.is_none());
        assert_eq!(
            video.statistics()["source_size"],
            diagnostics["source_size"]
        );
        assert_eq!(
            video.statistics()["max_packet_bytes"],
            diagnostics["max_packet_bytes"]
        );
        assert_eq!(video.statistics()["active"], false);
    }
}
