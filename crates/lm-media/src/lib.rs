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
    gstreamer::init().map_err(|e| format!("GStreamer kunde inte startas: {e}"))
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
