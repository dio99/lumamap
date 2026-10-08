//! Geometri för mappning: homografi (perspektivtransform) och träfftest.

pub type Pt = [f32; 2];

/// 3×3-matris, radmajor: `[x', y', w'] = H · [x, y, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Homography(pub [[f64; 3]; 3]);

impl Homography {
    pub const IDENTITY: Homography = Homography([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    /// Beräknar homografin som avbildar `from[i]` på `to[i]` (4 punktpar, DLT).
    /// Returnerar `None` om punkterna är degenererade (t.ex. tre på en linje).
    pub fn from_points(from: &[Pt; 4], to: &[Pt; 4]) -> Option<Homography> {
        // Lös A·h = b för h = [h00 h01 h02 h10 h11 h12 h20 h21], h22 = 1.
        let mut a = [[0.0f64; 9]; 8];
        for i in 0..4 {
            let (x, y) = (from[i][0] as f64, from[i][1] as f64);
            let (u, v) = (to[i][0] as f64, to[i][1] as f64);
            a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        let h = solve8(a)?;
        Some(Homography([[h[0], h[1], h[2]], [h[3], h[4], h[5]], [h[6], h[7], 1.0]]))
    }

    pub fn apply(&self, p: Pt) -> Option<Pt> {
        let m = &self.0;
        let (x, y) = (p[0] as f64, p[1] as f64);
        let w = m[2][0] * x + m[2][1] * y + m[2][2];
        if w.abs() < 1e-12 {
            return None;
        }
        Some([
            ((m[0][0] * x + m[0][1] * y + m[0][2]) / w) as f32,
            ((m[1][0] * x + m[1][1] * y + m[1][2]) / w) as f32,
        ])
    }

    pub fn inverse(&self) -> Option<Homography> {
        let m = &self.0;
        let c = |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
        let det = m[0][0] * c(1, 1, 2, 2) - m[0][1] * c(1, 0, 2, 2) + m[0][2] * c(1, 0, 2, 1);
        if det.abs() < 1e-15 {
            return None;
        }
        let inv = [
            [c(1, 1, 2, 2), -c(0, 1, 2, 2), c(0, 1, 1, 2)],
            [-c(1, 0, 2, 2), c(0, 0, 2, 2), -c(0, 0, 1, 2)],
            [c(1, 0, 2, 1), -c(0, 0, 2, 1), c(0, 0, 1, 1)],
        ];
        let mut out = [[0.0; 3]; 3];
        for r in 0..3 {
            for k in 0..3 {
                out[r][k] = inv[r][k] / det;
            }
        }
        Some(Homography(out))
    }

    /// Kolumnmajor f32 för WGSL `mat3x3<f32>` (varje kolumn utfylld till vec4).
    pub fn to_gpu(&self) -> [[f32; 4]; 3] {
        let m = &self.0;
        let mut out = [[0.0; 4]; 3];
        for col in 0..3 {
            for row in 0..3 {
                out[col][row] = m[row][col] as f32;
            }
        }
        out
    }
}

/// Gausselimination med pivotering för 8×8-system (sista kolumnen = högerled).
fn solve8(mut a: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        for row in 0..8 {
            if row != col {
                let (f, pivot_row) = (a[row][col] / a[col][col], a[col]);
                for (x, p) in a[row][col..].iter_mut().zip(&pivot_row[col..]) {
                    *x -= f * p;
                }
            }
        }
    }
    let mut x = [0.0; 8];
    for i in 0..8 {
        x[i] = a[i][8] / a[i][i];
    }
    Some(x)
}

pub fn quad(pts: &[Pt]) -> Option<[Pt; 4]> {
    pts.get(..4)?.try_into().ok()
}

pub fn dist2(a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    dx * dx + dy * dy
}

/// Punkt i (godtycklig, även konkav) polygon – jämn/udda-regeln.
pub fn point_in_polygon(p: Pt, poly: &[Pt]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Index för den punkt som ligger närmast `p` inom `radius`.
pub fn nearest_point(p: Pt, pts: &[Pt], radius: f32) -> Option<usize> {
    let r2 = radius * radius;
    pts.iter()
        .enumerate()
        .map(|(i, q)| (i, dist2(p, *q)))
        .filter(|(_, d)| *d <= r2)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

pub fn centroid(pts: &[Pt]) -> Pt {
    let n = pts.len().max(1) as f32;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
    [sx / n, sy / n]
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: [Pt; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

    fn close(a: Pt, b: Pt) -> bool {
        dist2(a, b) < 1e-8
    }

    #[test]
    fn maps_corners() {
        let to = [[0.1, 0.2], [0.9, 0.1], [0.8, 0.95], [0.15, 0.7]];
        let h = Homography::from_points(&UNIT, &to).unwrap();
        for i in 0..4 {
            assert!(close(h.apply(UNIT[i]).unwrap(), to[i]));
        }
    }

    #[test]
    fn inverse_roundtrip() {
        let to = [[0.1, 0.2], [0.9, 0.1], [0.8, 0.95], [0.15, 0.7]];
        let h = Homography::from_points(&UNIT, &to).unwrap();
        let inv = h.inverse().unwrap();
        let p = [0.3, 0.6];
        assert!(close(inv.apply(h.apply(p).unwrap()).unwrap(), p));
    }

    #[test]
    fn identity_for_same_points() {
        let h = Homography::from_points(&UNIT, &UNIT).unwrap();
        assert!(close(h.apply([0.42, 0.17]).unwrap(), [0.42, 0.17]));
    }

    #[test]
    fn degenerate_is_none() {
        let line = [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0], [0.25, 0.25]];
        assert!(Homography::from_points(&UNIT, &line).is_none());
    }

    #[test]
    fn polygon_hit() {
        assert!(point_in_polygon([0.5, 0.5], &UNIT));
        assert!(!point_in_polygon([1.5, 0.5], &UNIT));
    }

    #[test]
    fn nearest() {
        assert_eq!(nearest_point([0.98, 0.02], &UNIT, 0.05), Some(1));
        assert_eq!(nearest_point([0.5, 0.5], &UNIT, 0.05), None);
    }
}
