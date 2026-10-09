//! Inriktning med kamera: hitta projektorns ljuspunkter i kamerabilder och
//! räkna ut sambandet kamera → projektor (en homografi, för plana väggar).

use crate::{Homography, Pt};

/// En gråskalebild (en byte per pixel, radvis).
pub struct Gray<'a> {
    pub width: usize,
    pub height: usize,
    pub data: &'a [u8],
}

/// Hittar en ljuspunkt som finns i `lit` men inte i `dark`. Ger mitten i
/// normaliserade kamerakoordinater (0..1), eller `None` om ingen tydlig
/// punkt syns (skymd, utanför bild, för svag).
pub fn find_dot(dark: &Gray, lit: &Gray) -> Option<Pt> {
    if dark.width != lit.width || dark.height != lit.height || lit.data.len() < lit.width * lit.height {
        return None;
    }
    let diff: Vec<i32> = lit.data.iter().zip(dark.data).map(|(&l, &d)| l as i32 - d as i32).collect();
    let max = *diff.iter().max()?;
    // Måste sticka ut tydligt över bruset.
    if max < 40 {
        return None;
    }
    let threshold = max / 2;
    let (mut sx, mut sy, mut sw, mut count) = (0.0f64, 0.0f64, 0.0f64, 0usize);
    for (i, &v) in diff.iter().enumerate() {
        if v > threshold {
            let w = (v - threshold) as f64;
            sx += w * (i % lit.width) as f64;
            sy += w * (i / lit.width) as f64;
            sw += w;
            count += 1;
        }
    }
    // En punkt är en liten fläck: för stor yta tyder på att hela bilden ändrats
    // (t.ex. kamerans exponering), inte på en punkt.
    if count == 0 || count > lit.width * lit.height / 20 {
        return None;
    }
    Some([((sx / sw + 0.5) / lit.width as f64) as f32, ((sy / sw + 0.5) / lit.height as f64) as f32])
}

impl Homography {
    /// Minsta kvadrat-anpassning från fyra eller fler punktpar. Punkterna
    /// normaliseras först (Hartley) för numerisk stabilitet.
    pub fn fit(from: &[Pt], to: &[Pt]) -> Option<Homography> {
        if from.len() != to.len() || from.len() < 4 {
            return None;
        }
        let (nf, tf) = normalizer(from);
        let (nt, tt) = normalizer(to);
        // Normalekvationer AᵀA·h = Aᵀb för h = [h00 … h21], h22 = 1.
        let mut ata = [[0.0f64; 9]; 8];
        for (p, q) in from.iter().zip(to) {
            let p = tf.apply(*p)?;
            let q = tt.apply(*q)?;
            let (x, y, u, v) = (p[0] as f64, p[1] as f64, q[0] as f64, q[1] as f64);
            for row in [[x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u], [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v]] {
                for i in 0..8 {
                    for j in 0..9 {
                        ata[i][j] += row[i] * row[j];
                    }
                }
            }
        }
        let h = crate::solve8(ata)?;
        let hn = Homography([[h[0], h[1], h[2]], [h[3], h[4], h[5]], [h[6], h[7], 1.0]]);
        // Tillbaka till ursprungliga koordinater: H = Tto⁻¹ · Hn · Tfrom.
        Some(nt.inverse()?.mul(&hn).mul(&nf))
    }

    pub fn mul(&self, o: &Homography) -> Homography {
        let (a, b) = (&self.0, &o.0);
        let mut m = [[0.0; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| a[r][k] * b[k][c]).sum();
            }
        }
        Homography(m)
    }
}

/// Flytta punkternas tyngdpunkt till origo och skala till medelavstånd √2.
fn normalizer(pts: &[Pt]) -> (Homography, Homography) {
    let n = pts.len() as f64;
    let cx = pts.iter().map(|p| p[0] as f64).sum::<f64>() / n;
    let cy = pts.iter().map(|p| p[1] as f64).sum::<f64>() / n;
    let mean = pts.iter().map(|p| ((p[0] as f64 - cx).powi(2) + (p[1] as f64 - cy).powi(2)).sqrt()).sum::<f64>() / n;
    let s = if mean > 1e-12 { 2f64.sqrt() / mean } else { 1.0 };
    let h = Homography([[s, 0.0, -s * cx], [0.0, s, -s * cy], [0.0, 0.0, 1.0]]);
    (h, h)
}

/// Var projektorn ska visa ljuspunkterna: ett 3 × 3-rutnät en bit in från
/// kanterna (normaliserade utgångskoordinater).
pub fn calibration_points() -> Vec<Pt> {
    let mut out = Vec::new();
    for y in [0.15, 0.5, 0.85] {
        for x in [0.15, 0.5, 0.85] {
            out.push([x, y]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pt, b: Pt, tol: f32) -> bool {
        (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol
    }

    #[test]
    fn fit_recovers_exact_homography_from_many_points() {
        let quad = [[0.1, 0.2], [0.85, 0.1], [0.9, 0.95], [0.05, 0.8]];
        let truth = Homography::from_points(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], &quad).unwrap();
        let from = calibration_points();
        let to: Vec<Pt> = from.iter().map(|p| truth.apply(*p).unwrap()).collect();
        let fitted = Homography::fit(&from, &to).unwrap();
        for p in [[0.3, 0.7], [0.0, 0.0], [1.0, 1.0]] {
            assert!(close(fitted.apply(p).unwrap(), truth.apply(p).unwrap(), 1e-4));
        }
    }

    #[test]
    fn fit_averages_out_noise() {
        let truth = Homography::from_points(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], &[[0.2, 0.1], [0.8, 0.2], [0.75, 0.9], [0.1, 0.85]]).unwrap();
        let from = calibration_points();
        // Fel på upp till ±0,5 % i varje mätning.
        let noise = [0.004, -0.003, 0.002, -0.005, 0.001, 0.003, -0.002, 0.004, -0.001];
        let to: Vec<Pt> = from.iter().zip(noise).map(|(p, n)| { let q = truth.apply(*p).unwrap(); [q[0] + n, q[1] - n] }).collect();
        let fitted = Homography::fit(&from, &to).unwrap();
        assert!(close(fitted.apply([0.5, 0.5]).unwrap(), truth.apply([0.5, 0.5]).unwrap(), 0.005));
        assert!(Homography::fit(&from[..3], &to[..3]).is_none(), "tre punkter räcker inte");
    }

    /// En kamerabild: brusig bakgrund, och en suddig ljuspunkt där projektorns
    /// punkt `at` hamnar genom `camera` (projektor → kamera).
    fn render(w: usize, h: usize, at: Option<Pt>, camera: &Homography, seed: u32) -> Vec<u8> {
        let mut img = vec![0u8; w * h];
        let mut state = seed.wrapping_mul(2654435761).wrapping_add(1);
        let centre = at.map(|p| camera.apply(p).unwrap());
        for (i, px) in img.iter_mut().enumerate() {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = (state % 12) as f32;
            let mut v = 30.0 + noise;
            if let Some(c) = centre {
                let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
                let d2 = ((x - c[0]) * w as f32).powi(2) + ((y - c[1]) * h as f32).powi(2);
                v += 180.0 * (-d2 / (2.0 * 4.0f32.powi(2))).exp();
            }
            *px = v.min(255.0) as u8;
        }
        img
    }

    #[test]
    fn dots_found_in_noisy_camera_images_give_the_right_mapping() {
        let (w, h) = (320, 180);
        // Kameran ser projektorbilden snett och lite förminskad.
        let camera = Homography::from_points(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], &[[0.18, 0.12], [0.82, 0.2], [0.78, 0.9], [0.22, 0.82]]).unwrap();
        let dark = render(w, h, None, &camera, 1);
        let gray = |d: &[u8]| Gray { width: w, height: h, data: d }.data.to_vec();
        let mut from = Vec::new();
        let mut seen = Vec::new();
        for (k, p) in calibration_points().into_iter().enumerate() {
            // En punkt skyms (t.ex. av en pelare) och hoppas över.
            if k == 4 {
                let blocked = render(w, h, None, &camera, 99);
                assert!(find_dot(&Gray { width: w, height: h, data: &gray(&dark) }, &Gray { width: w, height: h, data: &blocked }).is_none());
                continue;
            }
            let lit = render(w, h, Some(p), &camera, k as u32 + 2);
            let found = find_dot(&Gray { width: w, height: h, data: &dark }, &Gray { width: w, height: h, data: &lit }).unwrap();
            assert!(close(found, camera.apply(p).unwrap(), 0.01), "punkt {k}: {found:?}");
            from.push(found);
            seen.push(p);
        }
        // Kamera → projektor: klick i kamerabilden blir rätt projektorpunkt.
        let to_projector = Homography::fit(&from, &seen).unwrap();
        let wall_corner = [0.3, 0.6];
        let clicked = camera.apply(wall_corner).unwrap();
        assert!(close(to_projector.apply(clicked).unwrap(), wall_corner, 0.01));
    }
}
