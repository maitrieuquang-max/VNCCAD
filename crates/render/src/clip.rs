//! Clipping against axis-aligned rectangles (viewport contents, plot sheets).

use cadcraft_geom::{Bounds2, Vec2};

/// Clip the segment `a`→`b` to `r` (Liang–Barsky). Returns the parameter range kept.
fn clip_params(a: Vec2, d: Vec2, r: &Bounds2, mut t0: f64, mut t1: f64) -> Option<(f64, f64)> {
    for (p, q) in [(-d.x, a.x - r.min.x), (d.x, r.max.x - a.x), (-d.y, a.y - r.min.y), (d.y, r.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return None;
                }
                if t > t0 {
                    t0 = t;
                }
            } else {
                if t < t0 {
                    return None;
                }
                if t < t1 {
                    t1 = t;
                }
            }
        }
    }
    (t0 <= t1 && t0.is_finite() && t1.is_finite()).then_some((t0, t1))
}

/// Clip a segment to `r`.
pub fn clip_segment(a: Vec2, b: Vec2, r: &Bounds2) -> Option<(Vec2, Vec2)> {
    if r.is_empty() || !a.is_finite() || !b.is_finite() {
        return None;
    }
    let d = b - a;
    let (t0, t1) = clip_params(a, d, r, 0.0, 1.0)?;
    Some((a + d * t0, a + d * t1))
}

/// Clip an infinite line (or a ray when `ray`) through `base` along `dir` to `r`.
pub fn clip_infinite(base: Vec2, dir: Vec2, ray: bool, r: &Bounds2) -> Option<(Vec2, Vec2)> {
    if r.is_empty() || !base.is_finite() || !dir.is_finite() || dir == Vec2::ZERO {
        return None;
    }
    let far = (r.width() + r.height() + (base - r.center()).len()) * 2.0 / dir.len().max(1e-300);
    let (t0, t1) = clip_params(base, dir, r, if ray { 0.0 } else { -far }, far)?;
    Some((base + dir * t0, base + dir * t1))
}

/// Clip a polyline to `r`; returns the visible pieces (each with at least two points).
pub fn clip_polyline(pts: &[Vec2], r: &Bounds2) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    if r.is_empty() {
        return out;
    }
    // Fast path: everything inside.
    if pts.iter().all(|p| r.contains(*p)) {
        if pts.len() >= 2 {
            out.push(pts.to_vec());
        }
        return out;
    }
    let mut cur: Vec<Vec2> = Vec::new();
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        match clip_segment(*a, *b, r) {
            Some((p, q)) => {
                let joined = cur.last().is_some_and(|l| l.near(p, 1e-12));
                if !joined {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(p);
                }
                cur.push(q);
                // Leaving the rectangle ends the piece.
                if !q.near(*b, 1e-12) {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
            }
            None => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Clip a convex polygon to `r` (Sutherland–Hodgman).
pub fn clip_polygon(poly: &[Vec2], r: &Bounds2) -> Vec<Vec2> {
    if r.is_empty() {
        return Vec::new();
    }
    let mut cur: Vec<Vec2> = poly.to_vec();
    // Each edge: inside test and intersection with the boundary line.
    let edges: [(u8, f64); 4] = [(0, r.min.x), (1, r.max.x), (2, r.min.y), (3, r.max.y)];
    for (side, v) in edges {
        let inside = |p: &Vec2| match side {
            0 => p.x >= v,
            1 => p.x <= v,
            2 => p.y >= v,
            _ => p.y <= v,
        };
        let cross = |a: Vec2, b: Vec2| -> Vec2 {
            let t = match side {
                0 | 1 => (v - a.x) / (b.x - a.x),
                _ => (v - a.y) / (b.y - a.y),
            };
            let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
            a + (b - a) * t
        };
        let mut next = Vec::with_capacity(cur.len() + 2);
        let n = cur.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (cur.get(i).copied(), cur.get((i + 1) % n).copied()) else { continue };
            match (inside(&a), inside(&b)) {
                (true, true) => next.push(b),
                (true, false) => next.push(cross(a, b)),
                (false, true) => {
                    next.push(cross(a, b));
                    next.push(b);
                }
                (false, false) => {}
            }
        }
        cur = next;
        if cur.is_empty() {
            break;
        }
    }
    cur
}

/// Clip a triangle list (three vertices per triangle) to `r`, re-triangulating as fans.
pub fn clip_triangles(tris: &[Vec2], r: &Bounds2) -> Vec<Vec2> {
    let mut out = Vec::new();
    for t in tris.as_chunks::<3>().0 {
        if t.iter().all(|p| r.contains(*p)) {
            out.extend_from_slice(t);
            continue;
        }
        let poly = clip_polygon(t, r);
        if let Some(first) = poly.first().copied() {
            for w in poly.get(1..).unwrap_or(&[]).windows(2) {
                if let (Some(b), Some(c)) = (w.first(), w.get(1)) {
                    out.extend_from_slice(&[first, *b, *c]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Bounds2 {
        Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0))
    }

    #[test]
    fn segments_and_polylines() {
        let (a, b) = clip_segment(Vec2::new(-5.0, 5.0), Vec2::new(15.0, 5.0), &r()).unwrap();
        assert!(a.near(Vec2::new(0.0, 5.0), 1e-9) && b.near(Vec2::new(10.0, 5.0), 1e-9));
        assert!(clip_segment(Vec2::new(-5.0, -5.0), Vec2::new(-1.0, 20.0), &r()).is_none());
        // In, out, back in: two pieces.
        let pieces = clip_polyline(&[Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0), Vec2::new(15.0, 8.0), Vec2::new(5.0, 8.0)], &r());
        assert_eq!(pieces.len(), 2);
        let (a, b) = clip_infinite(Vec2::new(5.0, 5.0), Vec2::new(1.0, 0.0), false, &r()).unwrap();
        assert!(a.near(Vec2::new(0.0, 5.0), 1e-9) && b.near(Vec2::new(10.0, 5.0), 1e-9));
        let (a, _) = clip_infinite(Vec2::new(5.0, 5.0), Vec2::new(1.0, 0.0), true, &r()).unwrap();
        assert!(a.near(Vec2::new(5.0, 5.0), 1e-9));
        assert!(clip_segment(Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0), &r()).is_none());
    }

    #[test]
    fn triangles() {
        let t = clip_triangles(&[Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), Vec2::new(5.0, 10.0)], &r());
        assert!(!t.is_empty() && t.len().is_multiple_of(3));
        assert!(t.iter().all(|p| p.x >= -1e-9));
    }
}
