//! Planar regions: find the closed boundary around a point from a soup of curves (HATCH pick
//! points, BOUNDARY, REGION).
//!
//! The curves arrive as polylines. Segments are split at every crossing, vertices merged within
//! a tolerance, and the face to the left of the edge hit by a ray from the point is traced by
//! always taking the sharpest left turn.

use std::collections::HashMap;

use crate::{Bounds2, Line, Vec2, line_line, point_in_polygon, shoelace};

/// Upper bound on input segments (keeps the quadratic split bounded on hostile input).
pub const MAX_SEGMENTS: usize = 60_000;

fn key(p: Vec2, q: f64) -> (i64, i64) {
    ((p.x / q).round() as i64, (p.y / q).round() as i64)
}

struct Graph {
    pts: Vec<Vec2>,
    adj: Vec<Vec<usize>>,
}

fn build(polys: &[Vec<Vec2>], tol: f64) -> Option<Graph> {
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for pl in polys {
        for w in pl.windows(2) {
            if let [a, b] = w
                && a.is_finite()
                && b.is_finite()
                && !a.near(*b, tol)
            {
                segs.push((*a, *b));
            }
        }
    }
    if segs.is_empty() || segs.len() > MAX_SEGMENTS {
        return None;
    }
    // Split points per segment (parameters), found with a uniform grid.
    let bb = Bounds2::from_points(segs.iter().flat_map(|(a, b)| [*a, *b]));
    let n = segs.len();
    let cells = ((n as f64).sqrt().ceil() as usize).clamp(1, 512);
    let cw = (bb.width() / cells as f64).max(1e-12);
    let ch = (bb.height() / cells as f64).max(1e-12);
    let cell = |p: Vec2| {
        (
            ((p.x - bb.min.x) / cw).floor().clamp(0.0, (cells - 1) as f64) as usize,
            ((p.y - bb.min.y) / ch).floor().clamp(0.0, (cells - 1) as f64) as usize,
        )
    };
    let mut grid: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        let (x0, y0) = cell(a.min(*b));
        let (x1, y1) = cell(a.max(*b));
        if (x1 - x0 + 1) * (y1 - y0 + 1) > 4096 {
            // Very long segment: register in a coarse band.
            for gx in (x0..=x1).step_by(((x1 - x0) / 64).max(1)) {
                for gy in (y0..=y1).step_by(((y1 - y0) / 64).max(1)) {
                    grid.entry((gx, gy)).or_default().push(i);
                }
            }
            continue;
        }
        for gx in x0..=x1 {
            for gy in y0..=y1 {
                grid.entry((gx, gy)).or_default().push(i);
            }
        }
    }
    let mut splits: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];
    let mut tested = std::collections::HashSet::new();
    for list in grid.values() {
        for (k, &i) in list.iter().enumerate() {
            for &j in list.iter().skip(k + 1) {
                if i == j || !tested.insert((i.min(j), i.max(j))) {
                    continue;
                }
                let (Some(si), Some(sj)) = (segs.get(i), segs.get(j)) else { continue };
                let li = Line::new(si.0, si.1);
                let lj = Line::new(sj.0, sj.1);
                if let Some(x) = line_line(&li, &lj) {
                    if let Some(v) = splits.get_mut(i) {
                        v.push(li.param_of(x).clamp(0.0, 1.0));
                    }
                    if let Some(v) = splits.get_mut(j) {
                        v.push(lj.param_of(x).clamp(0.0, 1.0));
                    }
                }
                if tested.len() > 20_000_000 {
                    return None;
                }
            }
        }
    }
    let mut g = Graph { pts: Vec::new(), adj: Vec::new() };
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    let mut vid = |p: Vec2, g: &mut Graph| -> usize {
        let k = key(p, tol);
        *index.entry(k).or_insert_with(|| {
            g.pts.push(p);
            g.adj.push(Vec::new());
            g.pts.len() - 1
        })
    };
    for (i, (a, b)) in segs.iter().enumerate() {
        let mut ts = splits.get(i).cloned().unwrap_or_default();
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        let line = Line::new(*a, *b);
        for w in ts.windows(2) {
            if let [t0, t1] = w {
                let u = vid(line.at(*t0), &mut g);
                let v = vid(line.at(*t1), &mut g);
                if u != v {
                    if let Some(l) = g.adj.get_mut(u)
                        && !l.contains(&v)
                    {
                        l.push(v);
                    }
                    if let Some(l) = g.adj.get_mut(v)
                        && !l.contains(&u)
                    {
                        l.push(u);
                    }
                }
            }
        }
    }
    Some(g)
}

/// Trace the face left of half-edge u→v.
fn trace(g: &Graph, u0: usize, v0: usize) -> Option<Vec<Vec2>> {
    let (mut u, mut v) = (u0, v0);
    let mut out = vec![*g.pts.get(u)?];
    for _ in 0..g.pts.len().saturating_mul(2).max(16) {
        out.push(*g.pts.get(v)?);
        let pv = *g.pts.get(v)?;
        let back = (*g.pts.get(u)? - pv).angle();
        let mut best: Option<(f64, usize)> = None;
        for &w in g.adj.get(v)? {
            if w == u && g.adj.get(v).is_some_and(|l| l.len() > 1) {
                continue;
            }
            let a = (*g.pts.get(w)? - pv).angle();
            let mut cw = crate::norm_angle(back - a);
            if cw < 1e-12 {
                cw = crate::TAU;
            }
            if best.is_none_or(|(b, _)| cw < b) {
                best = Some((cw, w));
            }
        }
        let (_, w) = best?;
        u = v;
        v = w;
        if u == u0 && v == v0 {
            out.pop();
            return Some(out);
        }
    }
    None
}

/// The closed loop enclosing `p`, counter-clockwise, or `None`.
pub fn enclosing_loop(polys: &[Vec<Vec2>], p: Vec2, tol: f64) -> Option<Vec<Vec2>> {
    let g = build(polys, tol.max(1e-12))?;
    // Try hits along a ray to +X from nearest outward, so nested faces resolve to the innermost.
    let mut hits: Vec<(f64, usize, usize)> = Vec::new();
    for (u, ns) in g.adj.iter().enumerate() {
        for &v in ns {
            if u >= v {
                continue;
            }
            let (Some(a), Some(b)) = (g.pts.get(u), g.pts.get(v)) else { continue };
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y);
                if x > p.x {
                    hits.push((x, u, v));
                }
            }
        }
    }
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    for (_, a, b) in hits.into_iter().take(64) {
        let (pa, pb) = (*g.pts.get(a)?, *g.pts.get(b)?);
        let (u, v) = if (pb - pa).cross(p - pa) > 0.0 { (a, b) } else { (b, a) };
        if let Some(lp) = trace(&g, u, v)
            && lp.len() >= 3
            && shoelace(&lp) > 0.0
            && point_in_polygon(&lp, p)
        {
            return Some(lp);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, s: f64) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s), Vec2::new(x0, y0)]
    }

    #[test]
    fn finds_square_around_point() {
        let l = enclosing_loop(&[square(0.0, 0.0, 10.0)], Vec2::new(5.0, 5.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn crossing_lines_make_faces() {
        // A square cut by a diagonal: the face is a triangle.
        let polys = vec![square(0.0, 0.0, 10.0), vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)]];
        let l = enclosing_loop(&polys, Vec2::new(7.0, 2.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn overlapping_separate_lines() {
        // Four separate lines that overshoot each other (typical hand drafting).
        let polys = vec![
            vec![Vec2::new(-1.0, 0.0), Vec2::new(11.0, 0.0)],
            vec![Vec2::new(10.0, -1.0), Vec2::new(10.0, 6.0)],
            vec![Vec2::new(11.0, 5.0), Vec2::new(-1.0, 5.0)],
            vec![Vec2::new(0.0, 6.0), Vec2::new(0.0, -1.0)],
        ];
        let l = enclosing_loop(&polys, Vec2::new(3.0, 3.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn open_shape_has_no_boundary() {
        let polys = vec![vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)]];
        assert!(enclosing_loop(&polys, Vec2::new(5.0, 2.0), 1e-9).is_none());
        assert!(enclosing_loop(&[], Vec2::ZERO, 1e-9).is_none());
    }

    #[test]
    fn innermost_of_nested() {
        let polys = vec![square(0.0, 0.0, 10.0), square(2.0, 2.0, 2.0)];
        let l = enclosing_loop(&polys, Vec2::new(3.0, 3.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 4.0).abs() < 1e-9);
        let outer = enclosing_loop(&polys, Vec2::new(8.0, 8.0), 1e-9).unwrap();
        assert!((shoelace(&outer) - 100.0).abs() < 1e-9);
    }
}
