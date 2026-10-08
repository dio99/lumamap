//! Mediekällor. Varje källa lämnar bara sin *senaste* bildruta – aldrig en kö –
//! så att fördröjningen aldrig växer om renderingen hackar till.

mod still;
mod video;

pub use still::{test_pattern, StillSource};
pub use video::VideoSource;

/// En bildruta i RGBA8 (sRGB) som renderaren kan ladda upp till GPU:n.
pub struct FrameView<'a> {
    pub width: u32,
    pub height: u32,
    /// Byte per rad (kan vara större än `width * 4`).
    pub stride: u32,
    pub data: &'a [u8],
}

pub trait MediaSource: Send {
    /// Anropas en gång per bildruta från huvudtråden. Kör `f` om en ny bildruta finns.
    fn poll(&mut self, f: &mut dyn FnMut(FrameView));
    /// Storlek i pixlar, när den är känd.
    fn size(&self) -> Option<[u32; 2]>;
    /// Felmeddelande om källan inte går att spela.
    fn error(&self) -> Option<&str>;

    fn is_video(&self) -> bool {
        false
    }
    fn play(&mut self) {}
    fn pause(&mut self) {}
    fn is_playing(&self) -> bool {
        false
    }
    fn seek(&mut self, _seconds: f64) {}
    fn position(&self) -> Option<f64> {
        None
    }
    fn duration(&self) -> Option<f64> {
        None
    }
    fn set_looping(&mut self, _looping: bool) {}
    fn set_muted(&mut self, _muted: bool) {}
}

/// Initierar GStreamer. Säker att anropa flera gånger.
pub fn init() -> Result<(), String> {
    gstreamer::init().map_err(|e| format!("{}: {e}", lm_core::i18n::t("GStreamer kunde inte startas", "GStreamer could not be started")))
}

/// Filändelser som öppnas som video respektive bild.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "mpg", "mpeg", "ogv", "wmv", "flv", "ts", "mts", "gif",
];
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp", "tif", "tiff"];

pub enum MediaKind {
    Video,
    Image,
}

pub fn classify(path: &std::path::Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some(MediaKind::Image)
    } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        Some(MediaKind::Video)
    } else {
        None
    }
}

/// En kamera som kan väljas som källa.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraInfo {
    pub name: String,
    /// V4L2-enhet, t.ex. `/dev/video0`.
    pub device: String,
}

/// Kameror som GStreamer hittar (via PipeWire eller V4L2). Faller tillbaka på
/// `/dev/video*` om ingen hittas. Kan ta en stund – anropa inte varje bildruta.
pub fn list_cameras() -> Vec<CameraInfo> {
    use gstreamer::prelude::*;
    let mut out: Vec<CameraInfo> = Vec::new();
    let monitor = gstreamer::DeviceMonitor::new();
    monitor.add_filter(Some("Video/Source"), None);
    if monitor.start().is_ok() {
        for d in monitor.devices() {
            let Some(props) = d.properties() else { continue };
            let path = ["api.v4l2.path", "device.path"]
                .iter()
                .find_map(|k| props.get::<String>(*k).ok());
            if let Some(device) = path {
                if !out.iter().any(|c| c.device == device) {
                    out.push(CameraInfo {
                        name: d.display_name().to_string(),
                        device,
                    });
                }
            }
        }
        monitor.stop();
    }
    if out.is_empty() {
        if let Ok(dir) = std::fs::read_dir("/dev") {
            let mut devs: Vec<String> = dir
                .flatten()
                .map(|e| e.path().to_string_lossy().into_owned())
                .filter(|p| p.starts_with("/dev/video"))
                .collect();
            devs.sort();
            out = devs.into_iter().map(|d| CameraInfo { name: d.clone(), device: d }).collect();
        }
    }
    out.sort_by(|a, b| a.device.cmp(&b.device));
    out
}
