//! Stillbilder: bildfiler (laddas i bakgrunden), enfärgade ytor och testbild.

use crate::{FrameView, MediaSource};
use lm_core::i18n::t;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Största textursida som stöds överallt.
const MAX_SIDE: u32 = 8192;

struct Pixels {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

pub struct StillSource {
    pending: Arc<Mutex<Option<Result<Pixels, String>>>>,
    pixels: Option<Pixels>,
    uploaded: bool,
    error: Option<String>,
}

impl StillSource {
    fn ready(px: Pixels) -> Self {
        StillSource {
            pending: Arc::new(Mutex::new(None)),
            pixels: Some(px),
            uploaded: false,
            error: None,
        }
    }

    pub fn color(rgba: [f32; 4]) -> Self {
        let c = rgba.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        Self::ready(Pixels {
            width: 4,
            height: 4,
            data: c.repeat(16),
        })
    }

    pub fn test_pattern() -> Self {
        let (w, h) = (1024, 1024);
        Self::ready(Pixels {
            width: w,
            height: h,
            data: test_pattern(w, h),
        })
    }

    /// Laddar en bildfil i en bakgrundstråd så att gränssnittet inte fryser.
    pub fn image(path: &Path) -> Self {
        let pending = Arc::new(Mutex::new(None));
        let slot = pending.clone();
        let path = path.to_path_buf();
        std::thread::spawn(move || {
            let result = load_image(&path);
            *slot.lock().unwrap() = Some(result);
        });
        StillSource {
            pending,
            pixels: None,
            uploaded: false,
            error: None,
        }
    }
}

fn load_image(path: &Path) -> Result<Pixels, String> {
    let img = image::open(path).map_err(|e| format!("{} {}: {e}", t("Kunde inte öppna", "Could not open"), path.display()))?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(Pixels {
        width: rgba.width(),
        height: rgba.height(),
        data: rgba.into_raw(),
    })
}

impl MediaSource for StillSource {
    fn poll(&mut self, f: &mut dyn FnMut(FrameView)) {
        if self.pixels.is_none() && self.error.is_none() {
            match self.pending.lock().unwrap().take() {
                Some(Ok(px)) => self.pixels = Some(px),
                Some(Err(e)) => {
                    log::warn!("{e}");
                    self.error = Some(e);
                }
                None => {}
            }
        }
        if let (Some(px), false) = (&self.pixels, self.uploaded) {
            f(FrameView {
                width: px.width,
                height: px.height,
                stride: px.width * 4,
                data: &px.data,
            });
            self.uploaded = true;
        }
    }

    fn size(&self) -> Option<[u32; 2]> {
        self.pixels.as_ref().map(|p| [p.width, p.height])
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Testbild för att rikta in projektorer: rutnät, diagonaler, cirkel och färgade hörn.
pub fn test_pattern(w: u32, h: u32) -> Vec<u8> {
    let mut data = vec![0u8; (w * h * 4) as usize];
    let cells = 8.0;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let radius = w.min(h) as f32 * 0.4;
    let line = (w.min(h) as f32 / 256.0).max(1.5);
    let corners = [[255, 60, 60], [60, 220, 60], [60, 120, 255], [255, 220, 40]];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (u, v) = (fx / w as f32, fy / h as f32);
            let checker = ((u * cells) as u32 + (v * cells) as u32).is_multiple_of(2);
            let mut c = if checker { [40, 40, 48] } else { [70, 70, 80] };

            let gx = (u * cells).fract() * w as f32 / cells;
            let gy = (v * cells).fract() * h as f32 / cells;
            let grid = gx < line || gy < line || fx > w as f32 - line || fy > h as f32 - line;
            let d1 = (fx * h as f32 / w as f32 - fy).abs();
            let d2 = ((w as f32 - fx) * h as f32 / w as f32 - fy).abs();
            let r = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let circle = (r - radius).abs() < line;
            let cross = (fx - cx).abs() < line * 1.5 || (fy - cy).abs() < line * 1.5;

            if grid || d1 < line || d2 < line {
                c = [220, 220, 220];
            }
            if circle || cross {
                c = [255, 255, 255];
            }
            let corner_size = 0.12;
            let corner = match (u < corner_size, u > 1.0 - corner_size, v < corner_size, v > 1.0 - corner_size) {
                (true, _, true, _) => Some(0),
                (_, true, true, _) => Some(1),
                (_, true, _, true) => Some(2),
                (true, _, _, true) => Some(3),
                _ => None,
            };
            if let Some(i) = corner {
                c = corners[i];
            }
            let o = ((y * w + x) * 4) as usize;
            data[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    data
}
