//! Video via GStreamer → `appsink` (RGBA). Samma källa används för filer,
//! kameror och nätverksströmmar; bara pipelinen framför appsink skiljer.
//! Ljudet från filer och strömmar spelas upp av playbin direkt.
//! Hårdvaruavkodning (VA-API) väljs automatiskt av GStreamer.

use crate::{FrameView, MediaSource};
use lm_core::i18n::t;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::*;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Kameror begränsas till högst Full HD – mer behövs inte för projektion
/// och avkodningen av t.ex. 2592×1944 MJPEG blir onödigt tung.
const CAMERA_CAPS: &str = "image/jpeg,width=[1,1920],height=[1,1080];video/x-raw,width=[1,1920],height=[1,1080]";

pub struct VideoSource {
    pipeline: gst::Element,
    latest: Arc<Mutex<Option<gst::Sample>>>,
    size: Option<[u32; 2]>,
    looping: bool,
    playing: bool,
    /// Kamera eller ström: går inte att spola och tar slut i stället för att loopa.
    live: bool,
    error: Option<String>,
}

type Slot = Arc<Mutex<Option<gst::Sample>>>;

impl VideoSource {
    pub fn open(path: &Path, looping: bool, muted: bool) -> Self {
        Self::or_error(Self::try_file(path, looping, muted), looping)
    }

    /// Kamera via V4L2, t.ex. `/dev/video0`.
    pub fn camera(device: &str) -> Self {
        Self::or_error(Self::try_camera(device), false)
    }

    /// Nätverksström eller annan URI som GStreamer förstår (rtsp://, srt://, udp://, http://…).
    pub fn stream(uri: &str, muted: bool) -> Self {
        Self::or_error(Self::try_stream(uri, muted), false)
    }

    fn or_error(result: Result<Self, String>, looping: bool) -> Self {
        result.unwrap_or_else(|e| {
            log::warn!("{e}");
            VideoSource {
                pipeline: gst::ElementFactory::make("fakesink").build().expect("fakesink"),
                latest: Arc::new(Mutex::new(None)),
                size: None,
                looping,
                playing: false,
                live: false,
                error: Some(e),
            }
        })
    }

    /// Kopplar appsink så att bara den senaste bildrutan sparas.
    fn configure_sink(sink: &gst_app::AppSink, sync: bool) -> Slot {
        sink.set_caps(Some(&gst_video::VideoCapsBuilder::new().format(gst_video::VideoFormat::Rgba).build()));
        sink.set_max_buffers(1);
        sink.set_drop(true);
        sink.set_sync(sync);
        let latest: Slot = Arc::new(Mutex::new(None));
        let slot = latest.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    *slot.lock().unwrap() = Some(sample);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        latest
    }

    fn start(pipeline: gst::Element, latest: Slot, looping: bool, live: bool, what: &str) -> Result<Self, String> {
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|_| format!("{} {what}", t("Kunde inte starta", "Could not start")))?;
        Ok(VideoSource {
            pipeline,
            latest,
            size: None,
            looping,
            playing: true,
            live,
            error: None,
        })
    }

    fn playbin(uri: &str, sink: &gst_app::AppSink, muted: bool) -> Result<gst::Element, String> {
        gst::ElementFactory::make("playbin")
            .property("uri", uri)
            .property("video-sink", sink)
            .property("mute", muted)
            .build()
            .map_err(|e| format!("{}: {e}", t("playbin saknas (installera gstreamer1.0-plugins-base)", "playbin is missing (install gstreamer1.0-plugins-base)")))
    }

    fn try_file(path: &Path, looping: bool, muted: bool) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("{}: {}", t("Filen finns inte", "File not found"), path.display()));
        }
        let abs = path.canonicalize().map_err(|e| e.to_string())?;
        let uri = gst::glib::filename_to_uri(&abs, None).map_err(|e| e.to_string())?;
        let sink = gst_app::AppSink::builder().build();
        let latest = Self::configure_sink(&sink, true);
        let playbin = Self::playbin(&uri, &sink, muted)?;
        Self::start(playbin, latest, looping, false, &path.display().to_string())
    }

    fn try_stream(uri: &str, muted: bool) -> Result<Self, String> {
        let Some((scheme, _)) = uri.split_once("://") else {
            return Err(format!("{}: {uri}", t("Ogiltig adress (saknar t.ex. rtsp://)", "Invalid address (missing e.g. rtsp://)")));
        };
        if gst::Element::make_from_uri(gst::URIType::Src, uri, None).is_err() {
            return Err(format!(
                "{} {scheme}:// ({})",
                t("Ingen GStreamer-plugin kan läsa", "No GStreamer plugin can read"),
                t("installera gstreamer1.0-plugins-bad/-good", "install gstreamer1.0-plugins-bad/-good")
            ));
        }
        let sink = gst_app::AppSink::builder().build();
        // Ingen synk: med synk räknas bildrutorna som sena och avkodaren
        // hoppar över dem (QoS). Visa varje bild så fort den är avkodad.
        let latest = Self::configure_sink(&sink, false);
        let playbin = Self::playbin(uri, &sink, muted)?;
        Self::start(playbin, latest, false, true, uri)
    }

    fn try_camera(device: &str) -> Result<Self, String> {
        if !Path::new(device).exists() {
            return Err(format!("{}: {device}", t("Kameran finns inte", "Camera not found")));
        }
        let desc = format!(
            "v4l2src device=\"{}\" ! capsfilter caps=\"{CAMERA_CAPS}\" ! decodebin ! videoconvert ! appsink name=sink",
            device.replace('"', "")
        );
        let pipeline = gst::parse::launch(&desc).map_err(|e| format!("{} {device}: {e}", t("Kunde inte öppna kameran", "Could not open camera")))?;
        let bin = pipeline.clone().downcast::<gst::Bin>().map_err(|_| "Ingen pipeline".to_string())?;
        let sink = bin
            .by_name("sink")
            .and_then(|e| e.downcast::<gst_app::AppSink>().ok())
            .ok_or(t("appsink saknas", "appsink is missing"))?;
        // Ingen synk: visa varje bildruta så fort den kommer (lägst fördröjning).
        let latest = Self::configure_sink(&sink, false);
        Self::start(pipeline, latest, false, true, &format!("{} {device}", t("kameran", "camera")))
    }

    fn handle_bus(&mut self) {
        let Some(bus) = self.pipeline.bus() else { return };
        while let Some(msg) = bus.pop() {
            match msg.view() {
                gst::MessageView::Eos(_) if self.live => {
                    self.error = Some(t("Strömmen tog slut", "The stream ended").into());
                    self.playing = false;
                }
                gst::MessageView::Eos(_) => {
                    if self.looping {
                        self.seek(0.0);
                    } else {
                        self.pause();
                    }
                }
                gst::MessageView::Error(err) => {
                    let e = format!("{} ({:?})", err.error(), err.debug());
                    log::warn!("Videofel: {e}");
                    self.error = Some(err.error().to_string());
                    self.playing = false;
                }
                _ => {}
            }
        }
    }
}

impl MediaSource for VideoSource {
    fn poll(&mut self, f: &mut dyn FnMut(FrameView)) {
        if self.error.is_some() {
            return;
        }
        self.handle_bus();
        let Some(sample) = self.latest.lock().unwrap().take() else { return };
        let (Some(buffer), Some(caps)) = (sample.buffer(), sample.caps()) else { return };
        let Ok(info) = gst_video::VideoInfo::from_caps(caps) else { return };
        let Ok(frame) = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info) else { return };
        let Ok(data) = frame.plane_data(0) else { return };
        let (w, h) = (frame.width(), frame.height());
        self.size = Some([w, h]);
        f(FrameView {
            width: w,
            height: h,
            stride: frame.plane_stride()[0] as u32,
            data,
        });
    }

    fn size(&self) -> Option<[u32; 2]> {
        self.size
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn is_video(&self) -> bool {
        true
    }

    fn play(&mut self) {
        if self.error.is_none() && self.pipeline.set_state(gst::State::Playing).is_ok() {
            self.playing = true;
        }
    }

    fn pause(&mut self) {
        if self.pipeline.set_state(gst::State::Paused).is_ok() {
            self.playing = false;
        }
    }

    fn is_playing(&self) -> bool {
        self.playing
    }

    fn seek(&mut self, seconds: f64) {
        if self.live {
            return;
        }
        let t = gst::ClockTime::from_nseconds((seconds.max(0.0) * 1e9) as u64);
        let _ = self
            .pipeline
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, t);
    }

    fn position(&self) -> Option<f64> {
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1e9)
    }

    fn duration(&self) -> Option<f64> {
        if self.live {
            return None;
        }
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1e9)
    }

    fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    fn set_muted(&mut self, muted: bool) {
        if self.error.is_none() && self.pipeline.find_property("mute").is_some() {
            self.pipeline.set_property("mute", muted);
        }
    }
}

impl Drop for VideoSource {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
