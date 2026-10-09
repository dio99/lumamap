//! Mediekällor. Varje källa lämnar bara sin *senaste* bildruta – aldrig en kö –
//! så att fördröjningen aldrig växer om renderingen hackar till.

pub mod audio;
mod still;
mod video;

pub use still::{test_pattern, StillSource};

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(c: YuvColor, yuv: [f32; 3]) -> [f32; 3] {
        let (m, o) = c.to_rgb();
        let d = [yuv[0] - o[0], yuv[1] - o[1], yuv[2] - o[2]];
        [0, 1, 2].map(|r| m[0][r] * d[0] + m[1][r] * d[1] + m[2][r] * d[2])
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
    }

    #[test]
    fn video_range_black_white_and_red() {
        let c = YuvColor { matrix: YuvMatrix::Bt709, full_range: false };
        assert!(close(convert(c, [16.0 / 255.0, 0.5, 0.5]), [0.0, 0.0, 0.0]));
        assert!(close(convert(c, [235.0 / 255.0, 0.5, 0.5]), [1.0, 1.0, 1.0]));
        // Rent rött i BT.709, begränsat omfång: Y=63, U=102, V=240.
        assert!(close(convert(c, [63.0 / 255.0, 102.0 / 255.0, 240.0 / 255.0]), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn average_color_of_rgba_and_nv12() {
        let red: Vec<u8> = [255u8, 0, 0, 255].repeat(64 * 32);
        let f = FrameView { width: 64, height: 32, stride: 256, data: &red, format: PixelFormat::Rgba };
        assert!(close(f.average_color(), [1.0, 0.0, 0.0]));
        // Samma röda i NV12 (BT.709, begränsat omfång): Y=63, U=102, V=240.
        let y = vec![63u8; 64 * 32];
        let uv: Vec<u8> = [102u8, 240].repeat(32 * 16);
        let color = YuvColor { matrix: YuvMatrix::Bt709, full_range: false };
        let f = FrameView { width: 64, height: 32, stride: 64, data: &y, format: PixelFormat::Nv12 { uv: &uv, uv_stride: 64, color } };
        assert!(close(f.average_color(), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn full_range_bt601() {
        let c = YuvColor { matrix: YuvMatrix::Bt601, full_range: true };
        assert!(close(convert(c, [1.0, 0.5, 0.5]), [1.0, 1.0, 1.0]));
        // Rent grönt i BT.601, fullt omfång: Y=150, U=44, V=21.
        assert!(close(convert(c, [150.0 / 255.0, 44.0 / 255.0, 21.0 / 255.0]), [0.0, 1.0, 0.0]));
    }
}
pub use video::VideoSource;

/// En bildruta som renderaren kan ladda upp till GPU:n.
pub struct FrameView<'a> {
    pub width: u32,
    pub height: u32,
    /// Byte per rad i `data` (kan vara större än bredden kräver).
    pub stride: u32,
    /// RGBA-pixlar, eller luminansplanet (Y) för YUV-format.
    pub data: &'a [u8],
    pub format: PixelFormat<'a>,
}

/// Bildrutans pixelformat. Video lämnas helst som YUV direkt från avkodaren:
/// det sparar en konvertering på CPU:n och mer än halverar uppladdningen.
pub enum PixelFormat<'a> {
    Rgba,
    /// Färgen i ett plan med U och V växelvis, halv upplösning åt båda håll.
    Nv12 { uv: &'a [u8], uv_stride: u32, color: YuvColor },
    /// Färgen i två plan, U och V, halv upplösning åt båda håll.
    I420 { u: &'a [u8], u_stride: u32, v: &'a [u8], v_stride: u32, color: YuvColor },
}

/// Hur YUV ska räknas om till RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YuvColor {
    pub matrix: YuvMatrix,
    /// 0–255 i stället för det vanliga videoomfånget 16–235.
    pub full_range: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum YuvMatrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl FrameView<'_> {
    /// Bildens medelfärg (0..1), från ett glest rutnät av punkter.
    pub fn average_color(&self) -> [f32; 3] {
        let (w, h) = (self.width as usize, self.height as usize);
        if w == 0 || h == 0 {
            return [0.0; 3];
        }
        let mut sum = [0.0f32; 3];
        let mut n = 0.0;
        for gy in 0..12 {
            for gx in 0..16 {
                let (x, y) = ((gx * 2 + 1) * w / 32, (gy * 2 + 1) * h / 24);
                let rgb = match &self.format {
                    PixelFormat::Rgba => {
                        let i = y * self.stride as usize + x * 4;
                        [0, 1, 2].map(|c| self.data.get(i + c).copied().unwrap_or(0) as f32 / 255.0)
                    }
                    PixelFormat::Nv12 { uv, uv_stride, color } => {
                        let yv = self.data.get(y * self.stride as usize + x).copied().unwrap_or(16);
                        let ci = (y / 2) * *uv_stride as usize + (x / 2) * 2;
                        let (u, v) = (uv.get(ci).copied().unwrap_or(128), uv.get(ci + 1).copied().unwrap_or(128));
                        yuv_to_rgb(color, yv, u, v)
                    }
                    PixelFormat::I420 { u, u_stride, v, v_stride, color } => {
                        let yv = self.data.get(y * self.stride as usize + x).copied().unwrap_or(16);
                        let uu = u.get((y / 2) * *u_stride as usize + x / 2).copied().unwrap_or(128);
                        let vv = v.get((y / 2) * *v_stride as usize + x / 2).copied().unwrap_or(128);
                        yuv_to_rgb(color, yv, uu, vv)
                    }
                };
                for c in 0..3 {
                    sum[c] += rgb[c];
                }
                n += 1.0;
            }
        }
        sum.map(|s| s / n)
    }
}

fn yuv_to_rgb(color: &YuvColor, y: u8, u: u8, v: u8) -> [f32; 3] {
    let (m, o) = color.to_rgb();
    let d = [y as f32 / 255.0 - o[0], u as f32 / 255.0 - o[1], v as f32 / 255.0 - o[2]];
    [0, 1, 2].map(|r| (m[0][r] * d[0] + m[1][r] * d[1] + m[2][r] * d[2]).clamp(0.0, 1.0))
}

impl YuvColor {
    /// Matris (kolumnvis) och förskjutning: `rgb = M · (yuv − offset)`.
    pub fn to_rgb(&self) -> ([[f32; 3]; 3], [f32; 3]) {
        let (kr, kb) = match self.matrix {
            YuvMatrix::Bt601 => (0.299, 0.114),
            YuvMatrix::Bt709 => (0.2126, 0.0722),
            YuvMatrix::Bt2020 => (0.2627, 0.0593),
        };
        let kg = 1.0 - kr - kb;
        let (sy, sc, oy) = if self.full_range {
            (1.0, 1.0, 0.0)
        } else {
            (255.0 / 219.0, 255.0 / 224.0, 16.0 / 255.0)
        };
        let cr_r = 2.0 * (1.0 - kr);
        let cb_b = 2.0 * (1.0 - kb);
        let cb_g = -2.0 * kb * (1.0 - kb) / kg;
        let cr_g = -2.0 * kr * (1.0 - kr) / kg;
        // Kolumner: bidrag från Y, U (Cb) och V (Cr).
        let m = [[sy, sy, sy], [0.0, cb_g * sc, cb_b * sc], [cr_r * sc, cr_g * sc, 0.0]];
        (m, [oy, 128.0 / 255.0, 128.0 / 255.0])
    }
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
    fn set_speed(&mut self, _speed: f64) {}
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
