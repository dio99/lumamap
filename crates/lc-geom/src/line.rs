//! Ritade linjer: förenkla det man ritat med musen, jämna ut det till en mjuk
//! kurva och bygga ett band med en viss bredd runt den.

use crate::Pt;

fn dist(a: Pt, b: Pt) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Ramer–Douglas–Peucker: tar bort punkter som ligger närmare än `tol` från linjen.
pub fn simplify(pts: &[Pt], tol: f32) -> Vec<Pt> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (mut far, mut at) = (0.0, 0);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = segment_distance(*p, a, b);
        if d > far {
            far = d;
            at = i;
        }
    }
    if far <= tol {
        return vec![a, b];
    }
    let mut left = simplify(&pts[..=at], tol);
    left.pop();
    left.extend(simplify(&pts[at..], tol));
    left
}

fn segment_distance(p: Pt, a: Pt, b: Pt) -> f32 {
    let e = [b[0] - a[0], b[1] - a[1]];
    let len2 = e[0] * e[0] + e[1] * e[1];
    if len2 < 1e-12 {
        return dist(p, a);
    }
    let t = (((p[0] - a[0]) * e[0] + (p[1] - a[1]) * e[1]) / len2).clamp(0.0, 1.0);
    dist(p, [a[0] + e[0] * t, a[1] + e[1] * t])
}

/// `n` punkter jämnt fördelade längs linjen.
pub fn resample(pts: &[Pt], n: usize) -> Vec<Pt> {
    if pts.len() < 2 || n < 2 {
        return pts.to_vec();
    }
    let total: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
    if total <= 0.0 {
        return vec![pts[0]; n];
    }
    let mut out = Vec::with_capacity(n);
    let (mut seg, mut walked) = (0, 0.0);
    for k in 0..n {
        let target = total * k as f32 / (n - 1) as f32;
        while seg < pts.len() - 2 && walked + dist(pts[seg], pts[seg + 1]) < target {
            walked += dist(pts[seg], pts[seg + 1]);
            seg += 1;
        }
        let len = dist(pts[seg], pts[seg + 1]).max(1e-9);
        let t = ((target - walked) / len).clamp(0.0, 1.0);
        let (a, b) = (pts[seg], pts[seg + 1]);
        out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
    out
}

/// Mjuk kurva genom punkterna (Catmull-Rom), `steps` delar per sträcka.
pub fn smooth(pts: &[Pt], steps: usize) -> Vec<Pt> {
    if pts.len() < 3 || steps < 2 {
        return pts.to_vec();
    }
    let at = |i: isize| pts[i.clamp(0, pts.len() as isize - 1) as usize];
    let mut out = Vec::with_capacity((pts.len() - 1) * steps + 1);
    for i in 0..pts.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let c = |k: usize| {
                0.5 * (2.0 * p1[k] + (p2[k] - p0[k]) * t + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t2 + (3.0 * p1[k] - p0[k] - 3.0 * p2[k] + p3[k]) * t3)
            };
            out.push([c(0), c(1)]);
        }
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// Bandet runt linjen: för varje punkt vänster och höger kant samt hur långt
/// längs linjen den ligger (0..1). `half` är halva bredden i x och y (olika
/// när bilden inte är kvadratisk), så att bredden blir lika i pixlar.
pub fn ribbon(pts: &[Pt], half: [f32; 2]) -> Vec<(Pt, Pt, f32)> {
    let n = pts.len();
    if n < 2 || half[0] <= 0.0 || half[1] <= 0.0 {
        return Vec::new();
    }
    // Räkna i ett rum där halva bredden är 1 åt båda håll.
    let q: Vec<Pt> = pts.iter().map(|p| [p[0] / half[0], p[1] / half[1]]).collect();
    let mut length = vec![0.0; n];
    for i in 1..n {
        length[i] = length[i - 1] + dist(q[i - 1], q[i]);
    }
    let total = length[n - 1].max(1e-9);
    (0..n)
        .map(|i| {
            let (a, b) = (q[i.saturating_sub(1)], q[(i + 1).min(n - 1)]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt().max(1e-9);
            let nrm = [-dy / l, dx / l];
            let left = [(q[i][0] + nrm[0]) * half[0], (q[i][1] + nrm[1]) * half[1]];
            let right = [(q[i][0] - nrm[0]) * half[0], (q[i][1] - nrm[1]) * half[1]];
            (left, right, length[i] / total)
        })
        .collect()
}

/// Bandets kontur som en sluten polygon (för träff med musen).
pub fn ribbon_outline(pts: &[Pt], half: [f32; 2]) -> Vec<Pt> {
    let r = ribbon(pts, half);
    r.iter().map(|e| e.0).chain(r.iter().rev().map(|e| e.1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_keeps_corners_and_drops_jitter() {
        let mut pts: Vec<Pt> = (0..=10).map(|i| [i as f32 * 0.1, if i % 2 == 0 { 0.0 } else { 0.001 }]).collect();
        pts.extend((1..=10).map(|i| [1.0, i as f32 * 0.1]));
        let s = simplify(&pts, 0.01);
        assert_eq!(s, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
    }

    #[test]
    fn resample_spaces_points_evenly() {
        let r = resample(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], 5);
        assert_eq!(r.len(), 5);
        for (a, b) in r.iter().zip([[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 0.5], [1.0, 1.0]]) {
            assert!(dist(*a, b) < 1e-5, "{a:?} {b:?}");
        }
    }

    #[test]
    fn smooth_passes_through_the_points() {
        let pts = [[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]];
        let s = smooth(&pts, 4);
        assert_eq!(s.len(), 9);
        assert_eq!(s[0], pts[0]);
        assert!(dist(s[4], pts[1]) < 1e-6);
        assert_eq!(s[8], pts[2]);
    }

    #[test]
    fn ribbon_has_the_right_width_in_pixels() {
        // Horisontell linje i en 16:9-bild, 10 px bred: halva bredden i y = 5/1080.
        let half = [5.0 / 1920.0, 5.0 / 1080.0];
        let r = ribbon(&[[0.1, 0.5], [0.9, 0.5]], half);
        assert_eq!(r.len(), 2);
        assert!((r[0].0[1] - (0.5 + 5.0 / 1080.0)).abs() < 1e-6 || (r[0].0[1] - (0.5 - 5.0 / 1080.0)).abs() < 1e-6);
        assert!((r[0].0[0] - 0.1).abs() < 1e-6);
        assert_eq!(r[1].2, 1.0);
        assert_eq!(ribbon_outline(&[[0.1, 0.5], [0.9, 0.5]], half).len(), 4);
    }
}
