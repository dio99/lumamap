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

/// Närmaste kant i den slutna polygonen `poly` inom `radius`.
/// Ger index där en ny punkt ska infogas och punkten på kanten.
pub fn nearest_edge(p: Pt, poly: &[Pt], radius: f32) -> Option<(usize, Pt)> {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let e = [b[0] - a[0], b[1] - a[1]];
            let len2 = (e[0] * e[0] + e[1] * e[1]).max(1e-12);
            let t = (((p[0] - a[0]) * e[0] + (p[1] - a[1]) * e[1]) / len2).clamp(0.0, 1.0);
            let q = [a[0] + e[0] * t, a[1] + e[1] * t];
            (i + 1, q, dist2(p, q))
        })
        .filter(|(_, _, d)| *d <= radius * radius)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(i, q, _)| (i, q))
}

pub fn centroid(pts: &[Pt]) -> Pt {
    let n = pts.len().max(1) as f32;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
    [sx / n, sy / n]
}

// ---------- Mesh ----------
//
// Ett mesh är `cols × rows` kontrollpunkter (radvis, övre vänster först). Ytan
// mellan punkterna är en Catmull-Rom-spline i båda riktningarna, så den går
// genom alla punkter och böjer sig mjukt. Ändarna extrapoleras linjärt, vilket
// gör att ett 2×2-mesh blir exakt bilinjärt.

fn lerp(a: Pt, b: Pt, t: f32) -> Pt {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn catmull_rom(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f32) -> Pt {
    let (t2, t3) = (t * t, t * t * t);
    let f = |i: usize| {
        0.5 * (2.0 * p1[i]
            + (p2[i] - p0[i]) * t
            + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * t2
            + (3.0 * p1[i] - p0[i] - 3.0 * p2[i] + p3[i]) * t3)
    };
    [f(0), f(1)]
}

/// Utvärderar en kurva genom `pts` vid `s` ∈ [0, 1].
fn spline(pts: &[Pt], s: f32) -> Pt {
    let n = pts.len();
    if n == 1 {
        return pts[0];
    }
    let x = s.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = (x.floor() as usize).min(n - 2);
    let t = x - i as f32;
    let get = |k: isize| -> Pt {
        if k < 0 {
            lerp(pts[1], pts[0], 2.0)
        } else if k as usize >= n {
            lerp(pts[n - 2], pts[n - 1], 2.0)
        } else {
            pts[k as usize]
        }
    };
    let i = i as isize;
    catmull_rom(get(i - 1), get(i), get(i + 1), get(i + 2), t)
}

/// Punkten på meshytan vid (u, v) ∈ [0, 1]².
pub fn mesh_eval(pts: &[Pt], cols: usize, rows: usize, u: f32, v: f32) -> Pt {
    let along_rows: Vec<Pt> = (0..rows).map(|r| spline(&pts[r * cols..(r + 1) * cols], u)).collect();
    spline(&along_rows, v)
}

/// Utvärderar meshytan i ett rutnät med `nu × nv` punkter (radvis).
pub fn mesh_grid(pts: &[Pt], cols: usize, rows: usize, nu: usize, nv: usize) -> Vec<Pt> {
    let step = |k: usize, n: usize| if n > 1 { k as f32 / (n - 1) as f32 } else { 0.0 };
    // Kurvorna längs raderna beräknas en gång per kolumn i utdata.
    let mut out = vec![[0.0; 2]; nu * nv];
    for i in 0..nu {
        let u = step(i, nu);
        let column: Vec<Pt> = (0..rows).map(|r| spline(&pts[r * cols..(r + 1) * cols], u)).collect();
        for j in 0..nv {
            out[j * nu + i] = spline(&column, step(j, nv));
        }
    }
    out
}

/// Ytans kontur medurs, med `per_cell` steg mellan varje par kontrollpunkter.
pub fn mesh_outline(pts: &[Pt], cols: usize, rows: usize, per_cell: usize) -> Vec<Pt> {
    let (nu, nv) = ((cols - 1) * per_cell + 1, (rows - 1) * per_cell + 1);
    let g = mesh_grid(pts, cols, rows, nu, nv);
    let mut out = Vec::with_capacity(2 * (nu + nv));
    out.extend((0..nu).map(|i| g[i]));
    out.extend((1..nv).map(|j| g[j * nu + nu - 1]));
    out.extend((0..nu - 1).rev().map(|i| g[(nv - 1) * nu + i]));
    out.extend((1..nv - 1).rev().map(|j| g[j * nu]));
    out
}

/// Kontrollpunkter för ett mesh som följer fyrhörningen `quad` perspektivriktigt.
pub fn mesh_from_quad(quad: &[Pt; 4], cols: usize, rows: usize) -> Vec<Pt> {
    const UNIT: [Pt; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let h = Homography::from_points(&UNIT, quad);
    let mut out = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            let uv = [c as f32 / (cols - 1) as f32, r as f32 / (rows - 1) as f32];
            out.push(h.and_then(|h| h.apply(uv)).unwrap_or(uv));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_2x2_is_bilinear() {
        let pts = [[0.1, 0.2], [0.9, 0.1], [0.15, 0.7], [0.8, 0.95]];
        let p = mesh_eval(&pts, 2, 2, 0.25, 0.5);
        let expect = lerp(lerp(pts[0], pts[1], 0.25), lerp(pts[2], pts[3], 0.25), 0.5);
        assert!(close(p, expect));
    }

    #[test]
    fn mesh_passes_through_control_points() {
        let mut pts = mesh_from_quad(&UNIT, 4, 3);
        pts[5] = [0.4, 0.6];
        let g = mesh_grid(&pts, 4, 3, 4, 3);
        for (a, b) in g.iter().zip(&pts) {
            assert!(close(*a, *b));
        }
    }

    #[test]
    fn regular_grid_stays_flat() {
        let pts = mesh_from_quad(&UNIT, 5, 4);
        assert!(close(mesh_eval(&pts, 5, 4, 0.33, 0.71), [0.33, 0.71]));
    }

    #[test]
    fn outline_closes_around_mesh() {
        let pts = mesh_from_quad(&UNIT, 3, 3);
        let o = mesh_outline(&pts, 3, 3, 4);
        assert_eq!(o.len(), 2 * (9 + 9) - 4);
        assert!(point_in_polygon([0.5, 0.5], &o));
        assert!(!point_in_polygon([1.2, 0.5], &o));
    }

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
    fn edge_insert() {
        let (i, q) = nearest_edge([0.5, 1.02], &UNIT, 0.05).unwrap();
        assert_eq!(i, 3);
        assert!(close(q, [0.5, 1.0]));
        assert!(nearest_edge([0.5, 0.5], &UNIT, 0.05).is_none());
    }

    #[test]
    fn nearest() {
        assert_eq!(nearest_point([0.98, 0.02], &UNIT, 0.05), Some(1));
        assert_eq!(nearest_point([0.5, 0.5], &UNIT, 0.05), None);
    }
}
