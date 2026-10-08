//! Video via GStreamer `playbin` → `appsink` (RGBA). Ljudet spelas upp av
//! playbin direkt. Hårdvaruavkodning (VA-API) väljs automatiskt av GStreamer.

use crate::{FrameView, MediaSource};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::*;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct VideoSource {
    playbin: gst::Element,
    latest: Arc<Mutex<Option<gst::Sample>>>,
    size: Option<[u32; 2]>,
    looping: bool,
    playing: bool,
    error: Option<String>,
}

impl VideoSource {
    pub fn open(path: &Path, looping: bool, muted: bool) -> Self {
        match Self::try_open(path, looping, muted) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("{e}");
                VideoSource {
                    playbin: gst::ElementFactory::make("fakesink").build().expect("fakesink"),
                    latest: Arc::new(Mutex::new(None)),
                    size: None,
                    looping,
                    playing: false,
                    error: Some(e),
                }
            }
        }
    }

    fn try_open(path: &Path, looping: bool, muted: bool) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("Filen finns inte: {}", path.display()));
        }
        let abs = path.canonicalize().map_err(|e| e.to_string())?;
        let uri = gst::glib::filename_to_uri(&abs, None).map_err(|e| e.to_string())?;

        let appsink = gst_app::AppSink::builder()
            .caps(&gst_video::VideoCapsBuilder::new().format(gst_video::VideoFormat::Rgba).build())
            .max_buffers(1)
            .drop(true)
            .sync(true)
            .build();

        let latest = Arc::new(Mutex::new(None));
        let slot = latest.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    *slot.lock().unwrap() = Some(sample);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let playbin = gst::ElementFactory::make("playbin")
            .property("uri", uri.as_str())
            .property("video-sink", &appsink)
            .property("mute", muted)
            .build()
            .map_err(|e| format!("playbin saknas (installera gstreamer1.0-plugins-base): {e}"))?;

        playbin
            .set_state(gst::State::Playing)
            .map_err(|_| format!("Kunde inte spela {}", path.display()))?;

        Ok(VideoSource {
            playbin,
            latest,
            size: None,
            looping,
            playing: true,
            error: None,
        })
    }

    fn handle_bus(&mut self) {
        let Some(bus) = self.playbin.bus() else { return };
        while let Some(msg) = bus.pop() {
            match msg.view() {
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
        if self.error.is_none() && self.playbin.set_state(gst::State::Playing).is_ok() {
            self.playing = true;
        }
    }

    fn pause(&mut self) {
        if self.playbin.set_state(gst::State::Paused).is_ok() {
            self.playing = false;
        }
    }

    fn is_playing(&self) -> bool {
        self.playing
    }

    fn seek(&mut self, seconds: f64) {
        let t = gst::ClockTime::from_nseconds((seconds.max(0.0) * 1e9) as u64);
        let _ = self
            .playbin
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, t);
    }

    fn position(&self) -> Option<f64> {
        self.playbin
            .query_position::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1e9)
    }

    fn duration(&self) -> Option<f64> {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1e9)
    }

    fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    fn set_muted(&mut self, muted: bool) {
        if self.error.is_none() {
            self.playbin.set_property("mute", muted);
        }
    }
}

impl Drop for VideoSource {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}
