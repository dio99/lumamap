//! Rörelse i en kamerabild: hur stor del av bilden som ändrats sedan förra
//! bildrutan. Räknas på ett litet rutnät av ljusstyrkor, så det kostar nästan
//! ingenting och klarar brus från kameran.

use crate::{FrameView, PixelFormat};

pub const GRID_W: usize = 32;
pub const GRID_H: usize = 24;

/// Hur mycket en ruta måste ändras (0..1) för att räknas som rörelse.
const NOISE: f32 = 0.06;

impl FrameView<'_> {
    /// Ljusstyrkan i ett rutnät (medel av fyra punkter per ruta).
    pub fn luma_grid(&self) -> Vec<f32> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut out = vec![0.0; GRID_W * GRID_H];
        if w == 0 || h == 0 {
            return out;
        }
        let luma = |x: usize, y: usize| -> f32 {
            match &self.format {
                PixelFormat::Rgba => {
                    let i = y * self.stride as usize + x * 4;
                    let c = |k| self.data.get(i + k).copied().unwrap_or(0) as f32;
                    (0.2126 * c(0) + 0.7152 * c(1) + 0.0722 * c(2)) / 255.0
                }
                // YUV: Y-planet är ljusstyrkan.
                _ => self.data.get(y * self.stride as usize + x).copied().unwrap_or(0) as f32 / 255.0,
            }
        };
        for gy in 0..GRID_H {
            for gx in 0..GRID_W {
                let mut sum = 0.0;
                for (fx, fy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                    let x = (gx * 4 + fx) * w / (GRID_W * 4);
                    let y = (gy * 4 + fy) * h / (GRID_H * 4);
                    sum += luma(x.min(w - 1), y.min(h - 1));
                }
                out[gy * GRID_W + gx] = sum / 4.0;
            }
        }
        out
    }
}

/// Följer rörelsen över tid: stiger snabbt, sjunker långsamt.
#[derive(Default)]
pub struct MotionDetector {
    prev: Option<Vec<f32>>,
    level: f32,
}

impl MotionDetector {
    /// Tar nästa bildrutas rutnät och ger rörelsen 0..1 (andel av bilden som rört sig).
    pub fn feed(&mut self, grid: Vec<f32>) -> f32 {
        let changed = match &self.prev {
            Some(prev) if prev.len() == grid.len() => {
                grid.iter().zip(prev).filter(|(a, b)| (**a - **b).abs() > NOISE).count() as f32 / grid.len() as f32
            }
            _ => 0.0,
        };
        self.prev = Some(grid);
        self.level = if changed > self.level { self.level + (changed - self.level) * 0.6 } else { self.level * 0.9 + changed * 0.1 };
        self.level
    }

    pub fn level(&self) -> f32 {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(data: &[u8], w: u32, h: u32) -> FrameView<'_> {
        FrameView { width: w, height: h, stride: w * 4, data, format: PixelFormat::Rgba }
    }

    #[test]
    fn still_picture_has_no_motion_and_a_moving_square_has_some() {
        let (w, h) = (128u32, 96u32);
        let black = vec![0u8; (w * h * 4) as usize];
        let mut d = MotionDetector::default();
        d.feed(frame(&black, w, h).luma_grid());
        assert_eq!(d.feed(frame(&black, w, h).luma_grid()), 0.0);
        // En vit fyrkant över en fjärdedel av bilden dyker upp.
        let mut lit = black.clone();
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                let i = ((y * w + x) * 4) as usize;
                lit[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let m = d.feed(frame(&lit, w, h).luma_grid());
        assert!(m > 0.1 && m < 0.3, "{m}");
        // Sedan står allt still igen: nivån sjunker långsamt mot noll.
        let a = d.feed(frame(&lit, w, h).luma_grid());
        let b = d.feed(frame(&lit, w, h).luma_grid());
        assert!(a < m && b < a && b > 0.0);
    }

    #[test]
    fn camera_noise_is_ignored() {
        let (w, h) = (64u32, 48u32);
        let a = vec![100u8; (w * h * 4) as usize];
        let b: Vec<u8> = a.iter().enumerate().map(|(i, v)| v + (i % 7) as u8).collect();
        let mut d = MotionDetector::default();
        d.feed(frame(&a, w, h).luma_grid());
        assert_eq!(d.feed(frame(&b, w, h).luma_grid()), 0.0);
    }
}
