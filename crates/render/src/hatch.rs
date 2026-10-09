//! Pattern hatch line generation: each pattern line family is swept across the boundary
//! (even-odd clipped), then dashed.

use cadcraft_doc::Hatch;
use cadcraft_doc::library::{HatchPattern, pattern};
use cadcraft_geom::{Bounds2, Polyline, Vec2};

/// Hatch line runs in world coordinates (single-point runs are dots).
/// VNCCad: pattern lines at the detail the view can show. Families whose lines are closer than
/// `tol` (half a pixel on screen) can't be told apart: they are left out and `true` is
/// returned so the caller can fill the area instead, the way such a hatch looks anyway. Dash
/// patterns shorter than `tol` are drawn as continuous lines. Zoomed out on a large drawing,
/// this turns millions of invisible segments into one fill.
pub fn pattern_lines_lod(h: &Hatch, tol: f64) -> (Vec<Vec<Vec2>>, bool) {
    let mut dense = false;
    let pat = pattern(&h.pattern).unwrap_or_else(|| pattern("ANSI31").unwrap_or(HatchPattern { name: "ANSI31", description: "", lines: vec![] }));
    let loops: Vec<Vec<Vec2>> = h.loops.iter().map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(tol)).collect();
    let scale = if h.scale.is_finite() && h.scale > 1e-9 { h.scale } else { 1.0 };
    let mut out = Vec::new();
    for fam in &pat.lines {
        let ang = fam.angle.to_radians() + h.angle;
        let origin = h.origin + Vec2::new(fam.origin.0, fam.origin.1).rotate(h.angle) * scale;
        let delta = Vec2::new(fam.delta.0, fam.delta.1).rotate(ang) * scale;
        let spacing = (Vec2::new(fam.delta.0, fam.delta.1).y * scale).abs();
        if spacing < 1e-9 {
            continue;
        }
        if tol > 0.0 && spacing < tol {
            dense = true;
            continue;
        }
        let mut dashes: Vec<f64> = fam.dashes.iter().map(|d| d * scale).collect();
        if tol > 0.0 && dashes.iter().map(|d| d.abs()).sum::<f64>() < tol {
            dashes.clear();
        }
        family(&loops, origin, ang, delta, spacing, &dashes, &mut out);
        if out.len() > 500_000 {
            break;
        }
    }
    (out, dense)
}

fn family(loops: &[Vec<Vec2>], origin: Vec2, ang: f64, delta: Vec2, spacing: f64, dashes: &[f64], out: &mut Vec<Vec<Vec2>>) {
    // Work in a frame where the family's lines are horizontal.
    let to_local = |p: Vec2| (p - origin).rotate(-ang);
    let to_world = |p: Vec2| origin + p.rotate(ang);
    let local: Vec<Vec<Vec2>> = loops.iter().map(|l| l.iter().map(|p| to_local(*p)).collect()).collect();
    let bb = Bounds2::from_points(local.iter().flatten().copied());
    if bb.is_empty() {
        return;
    }
    let shift_per_line = delta.rotate(-ang).x;
    let k0 = (bb.min.y / spacing).floor() as i64;
    let k1 = (bb.max.y / spacing).ceil() as i64;
    if k1.saturating_sub(k0) > 20_000 {
        return;
    }
    let pat_len: f64 = dashes.iter().map(|d| d.abs()).sum();
    for k in k0..=k1 {
        let y = k as f64 * spacing;
        let mut xs: Vec<f64> = Vec::new();
        for l in &local {
            let n = l.len();
            for i in 0..n {
                let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) else { continue };
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let offset = shift_per_line * k as f64;
        for span in xs.chunks(2) {
            let [x0, x1] = span else { continue };
            if dashes.is_empty() || pat_len < 1e-12 {
                out.push(vec![to_world(Vec2::new(*x0, y)), to_world(Vec2::new(*x1, y))]);
                continue;
            }
            // Dash phase anchored at x = offset.
            let start_rep = ((x0 - offset) / pat_len).floor();
            let mut x = offset + start_rep * pat_len;
            let mut guard = 0;
            'outer: while x < *x1 {
                for d in dashes {
                    guard += 1;
                    if guard > 100_000 {
                        break 'outer;
                    }
                    let len = d.abs();
                    if *d > 0.0 {
                        let a = x.max(*x0);
                        let b = (x + len).min(*x1);
                        if b > a {
                            out.push(vec![to_world(Vec2::new(a, y)), to_world(Vec2::new(b, y))]);
                        }
                    } else if *d == 0.0 && x >= *x0 && x <= *x1 {
                        out.push(vec![to_world(Vec2::new(x, y))]);
                    }
                    x += len;
                    if x >= *x1 {
                        break 'outer;
                    }
                }
            }
        }
    }
}
