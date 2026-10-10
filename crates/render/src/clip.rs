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

// ---------------------------------------------------------------- VNCCad: XCLIP polygons

/// Even–odd point-in-polygon test.
pub fn in_polygon(poly: &[Vec2], p: Vec2) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// The parts of the polyline `pts` inside `poly` (any simple polygon).
pub fn clip_polyline_to_polygon(pts: &[Vec2], poly: &[Vec2]) -> Vec<Vec<Vec2>> {
    split_polyline(pts, poly, true)
}

/// The parts of the polyline `pts` inside (`keep_inside`) or outside `poly`.
fn split_polyline(pts: &[Vec2], poly: &[Vec2], keep_inside: bool) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    let n = poly.len();
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first().copied(), w.get(1).copied()) else { continue };
        let d = b - a;
        // Parameters where the segment crosses the boundary.
        let mut ts = vec![0.0, 1.0];
        for i in 0..n {
            let (Some(p), Some(q)) = (poly.get(i).copied(), poly.get((i + 1) % n).copied()) else { continue };
            let e = q - p;
            let den = d.x * e.y - d.y * e.x;
            if den.abs() < 1e-300 {
                continue;
            }
            let w0 = p - a;
            let t = (w0.x * e.y - w0.y * e.x) / den;
            let u = (w0.x * d.y - w0.y * d.x) / den;
            if (0.0..=1.0).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&u) {
                ts.push(t);
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        for k in ts.windows(2) {
            let (Some(t0), Some(t1)) = (k.first().copied(), k.get(1).copied()) else { continue };
            let (p0, p1) = (a + d * t0, a + d * t1);
            if in_polygon(poly, (p0 + p1) * 0.5) == keep_inside {
                if cur.last().is_none_or(|l| !l.near(p0, 1e-9)) {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur = vec![p0];
                }
                cur.push(p1);
            } else if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

fn is_convex(poly: &[Vec2]) -> bool {
    let n = poly.len();
    let mut sign = 0.0f64;
    for i in 0..n {
        let (Some(a), Some(b), Some(c)) = (poly.get(i), poly.get((i + 1) % n), poly.get((i + 2) % n)) else { return false };
        let z = (*b - *a).cross(*c - *b);
        if z.abs() > 1e-12 {
            if sign != 0.0 && z.signum() != sign {
                return false;
            }
            sign = z.signum();
        }
    }
    true
}

/// Clip a convex polygon `subject` to a convex polygon `clip` (Sutherland–Hodgman).
fn clip_convex(subject: &[Vec2], clip: &[Vec2]) -> Vec<Vec2> {
    let n = clip.len();
    // Orientation of the clip polygon.
    let area: f64 = (0..n).filter_map(|i| Some(clip.get(i)?.cross(*clip.get((i + 1) % n)?))).sum();
    let s = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut cur = subject.to_vec();
    for i in 0..n {
        let (Some(p), Some(q)) = (clip.get(i).copied(), clip.get((i + 1) % n).copied()) else { continue };
        let inside = |x: Vec2| s * (q - p).cross(x - p) >= -1e-12;
        let input = std::mem::take(&mut cur);
        let m = input.len();
        for j in 0..m {
            let (Some(a), Some(b)) = (input.get(j).copied(), input.get((j + 1) % m).copied()) else { continue };
            let (ia, ib) = (inside(a), inside(b));
            if ia {
                cur.push(a);
            }
            if ia != ib {
                let e = q - p;
                let den = (b - a).cross(e);
                if den.abs() > 1e-300 {
                    let t = (p - a).cross(e) / den;
                    cur.push(a + (b - a) * t);
                }
            }
        }
        if cur.is_empty() {
            break;
        }
    }
    cur
}

/// Clip what was drawn into `list` from primitive `from` on to the polygon `poly` (XCLIP).
pub fn clip_list_to_polygon(list: &mut crate::DisplayList, from: usize, poly: &[Vec2]) {
    if poly.len() < 3 || from >= list.prims.len() {
        return;
    }
    let convex = is_convex(poly);
    let old: Vec<crate::DPrim> = list.prims.split_off(from);
    for p in old {
        match p.kind {
            crate::Kind::Polyline => {
                let pts: Vec<Vec2> = list.points(&p).to_vec();
                for piece in clip_polyline_to_polygon(&pts, poly) {
                    let start = list.verts.len() as u32;
                    let len = piece.len() as u32;
                    list.verts.extend(piece);
                    list.prims.push(crate::DPrim { start, len, ..p });
                }
            }
            crate::Kind::Tris => {
                let pts: Vec<Vec2> = list.points(&p).to_vec();
                let start = list.tris.len() as u32;
                for t in pts.chunks(3) {
                    if t.len() < 3 {
                        continue;
                    }
                    if convex {
                        let c = clip_convex(t, poly);
                        if c.len() >= 3 {
                            let (Some(o), rest) = (c.first().copied(), c.get(1..).unwrap_or(&[])) else { continue };
                            for w in rest.windows(2) {
                                if let (Some(b), Some(d)) = (w.first(), w.get(1)) {
                                    list.tris.extend([o, *b, *d]);
                                }
                            }
                        }
                    } else {
                        let centroid = t.iter().fold(Vec2::ZERO, |a, b| a + *b) * (1.0 / 3.0);
                        if in_polygon(poly, centroid) {
                            list.tris.extend_from_slice(t);
                        }
                    }
                }
                let len = list.tris.len() as u32 - start;
                if len >= 3 {
                    list.prims.push(crate::DPrim { start, len, ..p });
                }
            }
            crate::Kind::Point => {
                if list.points(&p).first().is_some_and(|q| in_polygon(poly, *q)) {
                    list.prims.push(p);
                }
            }
            crate::Kind::Infinite { .. } => {
                let pts = list.points(&p).to_vec();
                let (Some(base), Some(dir)) = (pts.first().copied(), pts.get(1).copied()) else { continue };
                let r = Bounds2::from_points(poly.iter().copied());
                let ray = matches!(p.kind, crate::Kind::Infinite { ray: true });
                if let Some((a, b)) = clip_infinite(base, dir, ray, &r) {
                    for piece in clip_polyline_to_polygon(&[a, b], poly) {
                        let start = list.verts.len() as u32;
                        let len = piece.len() as u32;
                        list.verts.extend(piece);
                        list.prims.push(crate::DPrim { start, len, kind: crate::Kind::Polyline, ..p });
                    }
                }
            }
        }
    }
}

/// Keep the part of the convex polygon `subject` on the left of a→b (`left`) or on the right.
fn half_plane(subject: &[Vec2], a: Vec2, b: Vec2, left: bool) -> Vec<Vec2> {
    let side = |x: Vec2| {
        let z = (b - a).cross(x - a);
        if left { z >= 0.0 } else { z <= 0.0 }
    };
    let mut out = Vec::new();
    let m = subject.len();
    for j in 0..m {
        let (Some(p), Some(q)) = (subject.get(j).copied(), subject.get((j + 1) % m).copied()) else { continue };
        let (ip, iq) = (side(p), side(q));
        if ip {
            out.push(p);
        }
        if ip != iq {
            let e = b - a;
            let den = (q - p).cross(e);
            if den.abs() > 1e-300 {
                out.push(p + (q - p) * ((a - p).cross(e) / den));
            }
        }
    }
    out
}

/// A triangle minus a convex polygon, as triangles.
fn subtract_convex(tri: &[Vec2], poly: &[Vec2]) -> Vec<Vec2> {
    let n = poly.len();
    let area: f64 = (0..n).filter_map(|i| Some(poly.get(i)?.cross(*poly.get((i + 1) % n)?))).sum();
    let ccw = area >= 0.0;
    let mut out = Vec::new();
    // Inside the polygon = left of every edge (ccw). Outside = outside edge k, inside edges < k.
    let mut rest: Vec<Vec2> = tri.to_vec();
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i).copied(), poly.get((i + 1) % n).copied()) else { continue };
        let outside = half_plane(&rest, a, b, !ccw);
        if outside.len() >= 3 {
            let o = outside[0];
            for w in outside[1..].windows(2) {
                out.extend([o, w[0], w[1]]);
            }
        }
        rest = half_plane(&rest, a, b, ccw);
        if rest.len() < 3 {
            break;
        }
    }
    out
}

/// A wipeout: the primitives drawn before primitive `before` lose what lies inside `poly`.
pub struct Mask {
    pub before: usize,
    pub poly: Vec<Vec2>,
    pub bounds: Bounds2,
}

/// Apply wipeouts (masks) to a display list: lines and fills drawn before a wipeout are cut
/// away inside it, so every renderer (screen, PDF, SVG, PNG) shows the mask the same way.
pub fn apply_masks(list: &mut crate::DisplayList, masks: &[Mask]) {
    if masks.is_empty() {
        return;
    }
    let all = masks.iter().fold(Bounds2::EMPTY, |a, m| a.union(&m.bounds));
    if all.is_empty() || !all.min.is_finite() || !all.max.is_finite() {
        return;
    }
    // A coarse grid of mask indices.
    const G: usize = 64;
    let cw = (all.width() / G as f64).max(1e-12);
    let ch = (all.height() / G as f64).max(1e-12);
    let cell = |p: Vec2| -> (usize, usize) {
        let x = ((p.x - all.min.x) / cw).floor().clamp(0.0, (G - 1) as f64) as usize;
        let y = ((p.y - all.min.y) / ch).floor().clamp(0.0, (G - 1) as f64) as usize;
        (x, y)
    };
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); G * G];
    for (k, m) in masks.iter().enumerate() {
        let (x0, y0) = cell(m.bounds.min);
        let (x1, y1) = cell(m.bounds.max);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some(c) = grid.get_mut(y * G + x) {
                    c.push(k);
                }
            }
        }
    }
    let last = masks.iter().map(|m| m.before).max().unwrap_or(0);
    let old: Vec<crate::DPrim> = std::mem::take(&mut list.prims);
    list.prims.reserve(old.len());
    for (i, p) in old.into_iter().enumerate() {
        // Drawn after every wipeout, or a construction line: untouched.
        if i >= last || matches!(p.kind, crate::Kind::Infinite { .. }) {
            list.prims.push(p);
            continue;
        }
        let bb = Bounds2::from_points(list.points(&p).iter().copied());
        if bb.is_empty() || !bb.intersects(&all) {
            list.prims.push(p);
            continue;
        }
        let (x0, y0) = cell(bb.min);
        let (x1, y1) = cell(bb.max);
        let mut cand: Vec<usize> = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &k in grid.get(y * G + x).map(Vec::as_slice).unwrap_or(&[]) {
                    if masks.get(k).is_some_and(|m| m.before > i && m.bounds.intersects(&bb)) && !cand.contains(&k) {
                        cand.push(k);
                    }
                }
            }
        }
        if cand.is_empty() {
            list.prims.push(p);
            continue;
        }
        let pts: Vec<Vec2> = list.points(&p).to_vec();
        match p.kind {
            crate::Kind::Polyline => {
                let mut pieces = vec![pts];
                for k in &cand {
                    let Some(m) = masks.get(*k) else { continue };
                    pieces = pieces.iter().flat_map(|pc| split_polyline(pc, &m.poly, false)).collect();
                }
                for piece in pieces {
                    let start = list.verts.len() as u32;
                    let len = piece.len() as u32;
                    list.verts.extend(piece);
                    list.prims.push(crate::DPrim { start, len, ..p });
                }
            }
            crate::Kind::Tris => {
                let mut tris = pts;
                for k in &cand {
                    let Some(m) = masks.get(*k) else { continue };
                    let convex = is_convex(&m.poly);
                    let mut next = Vec::with_capacity(tris.len());
                    for t in tris.chunks(3) {
                        if t.len() < 3 {
                            continue;
                        }
                        if convex {
                            next.extend(subtract_convex(t, &m.poly));
                        } else {
                            let c = t.iter().fold(Vec2::ZERO, |a, b| a + *b) * (1.0 / 3.0);
                            if !in_polygon(&m.poly, c) {
                                next.extend_from_slice(t);
                            }
                        }
                    }
                    tris = next;
                }
                if tris.len() >= 3 {
                    let start = list.tris.len() as u32;
                    let len = tris.len() as u32;
                    list.tris.extend(tris);
                    list.prims.push(crate::DPrim { start, len, ..p });
                }
            }
            crate::Kind::Point => {
                if !cand.iter().filter_map(|k| masks.get(*k)).any(|m| in_polygon(&m.poly, pts.first().copied().unwrap_or_default())) {
                    list.prims.push(p);
                }
            }
            crate::Kind::Infinite { .. } => list.prims.push(p),
        }
    }
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

    #[test]
    fn wipeouts_cut_lines_and_fills_drawn_before_them() {
        use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space, Wipeout};
        use cadcraft_geom::Vec3;
        let mut d = Drawing::new_metric();
        d.header.set_i64("WIPEOUTFRAME", 0);
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) })).unwrap();
        let solid = cadcraft_doc::Solid {
            corners: [Vec3::new(0.0, -1.0, 0.0), Vec3::new(10.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(10.0, 1.0, 0.0)],
        };
        d.add(&Space::Model, Common::default(), EntityKind::Solid(solid)).unwrap();
        let w = vec![Vec2::new(4.0, -2.0), Vec2::new(6.0, -2.0), Vec2::new(6.0, 2.0), Vec2::new(4.0, 2.0)];
        d.add(&Space::Model, Common::default(), EntityKind::Wipeout(Wipeout { boundary: w })).unwrap();
        // Drawn after the wipeout: stays whole.
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.5, 0.0), b: Vec3::new(10.0, 0.5, 0.0) })).unwrap();
        let l = crate::build(&d, &Space::Model, &crate::Options::default());
        let lines: Vec<Vec<Vec2>> = l.prims.iter().filter(|p| p.kind == crate::Kind::Polyline).map(|p| l.points(p).to_vec()).collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.iter().any(|p| p.len() == 2 && p[0].near(Vec2::new(0.0, 0.0), 1e-9) && p[1].near(Vec2::new(4.0, 0.0), 1e-9)));
        assert!(lines.iter().any(|p| p[0].near(Vec2::new(0.0, 0.5), 1e-9) && p[1].near(Vec2::new(10.0, 0.5), 1e-9)));
        // The fill keeps 2 × 4 = 16 square units of its 20.
        let tris: Vec<Vec2> = l.prims.iter().filter(|p| p.kind == crate::Kind::Tris).flat_map(|p| l.points(p).to_vec()).collect();
        let area: f64 = tris.chunks(3).map(|t| ((t[1] - t[0]).cross(t[2] - t[0]) / 2.0).abs()).sum();
        assert!((area - 16.0).abs() < 1e-6, "{area}");
    }
}
