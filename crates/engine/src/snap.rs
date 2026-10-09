//! Object snaps (OSNAP), polar tracking and ortho.

use cadcraft_doc::{Drawing, EntityKind, Prim, Space};
use cadcraft_geom::{Bounds2, Circle, PI, Segment, Vec2, intersect_ext};
use serde::Serialize;

/// OSMODE bits.
pub mod mode {
    pub const END: u32 = 1;
    pub const MID: u32 = 2;
    pub const CEN: u32 = 4;
    pub const NOD: u32 = 8;
    pub const QUA: u32 = 16;
    pub const INT: u32 = 32;
    pub const INS: u32 = 64;
    pub const PER: u32 = 128;
    pub const TAN: u32 = 256;
    pub const NEA: u32 = 512;
    pub const GCEN: u32 = 1024;
    pub const APP: u32 = 2048;
    pub const EXT: u32 = 4096;
    pub const PAR: u32 = 8192;
    /// Running snaps temporarily off (F3).
    pub const OFF: u32 = 16384;
    pub const ALL: [(u32, &str); 14] = [
        (END, "Endpoint"),
        (MID, "Midpoint"),
        (CEN, "Center"),
        (GCEN, "Geometric Center"),
        (NOD, "Node"),
        (QUA, "Quadrant"),
        (INT, "Intersection"),
        (EXT, "Extension"),
        (INS, "Insertion"),
        (PER, "Perpendicular"),
        (TAN, "Tangent"),
        (NEA, "Nearest"),
        (APP, "Apparent Intersection"),
        (PAR, "Parallel"),
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapHit {
    pub point: Vec2,
    pub mode: u32,
    pub name: &'static str,
}

fn name_of(m: u32) -> &'static str {
    mode::ALL.iter().find(|(b, _)| *b == m).map(|(_, n)| *n).unwrap_or("Snap")
}

fn prim_segments(p: &Prim) -> Vec<Segment> {
    match p {
        Prim::Seg(s) => vec![*s],
        Prim::Circle(c) => vec![Segment::Arc { arc: cadcraft_geom::Arc::new(c.center, c.radius, 0.0, 0.0), ccw: true }],
        Prim::Infinite { base, dir, .. } => vec![Segment::Line(cadcraft_geom::Line::new(*base - *dir * 1e7, *base + *dir * 1e7))],
        Prim::Ellipse(e) => {
            let mut pts = Vec::new();
            e.tessellate(e.major.len() * 1e-3, &mut pts);
            pts.windows(2).filter_map(|w| Some(Segment::Line(cadcraft_geom::Line::new(*w.first()?, *w.get(1)?)))).collect()
        }
        Prim::Spline(s) => {
            let pts = s.tessellate(1e-3);
            pts.windows(2).filter_map(|w| Some(Segment::Line(cadcraft_geom::Line::new(*w.first()?, *w.get(1)?)))).collect()
        }
        Prim::Fill(f) => {
            let n = f.len();
            (0..n).filter_map(|i| Some(Segment::Line(cadcraft_geom::Line::new(*f.get(i)?, *f.get((i + 1) % n)?)))).collect()
        }
        Prim::Point(_) => Vec::new(),
    }
}

/// Find the best object snap near `cursor` within `aperture` (world units).
pub fn osnap(d: &Drawing, space: &Space, cursor: Vec2, aperture: f64, osmode: u32, base: Option<Vec2>) -> Option<SnapHit> {
    if osmode == 0 || osmode & mode::OFF != 0 {
        return None;
    }
    let ix = crate::spatial::index(d, space);
    osnap_with(d, space, &ix, cursor, aperture, osmode, base)
}

pub(crate) fn osnap_with(
    d: &Drawing,
    space: &Space,
    ix: &Option<std::sync::Arc<crate::spatial::SpatialIndex>>,
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
    base: Option<Vec2>,
) -> Option<SnapHit> {
    if osmode == 0 || osmode & mode::OFF != 0 {
        return None;
    }
    let probe = Bounds2::new(cursor, cursor).expand(aperture);
    let mut cands: Vec<(u32, Vec2)> = Vec::new();
    let mut near_prims: Vec<Prim> = Vec::new();
    let mut count = 0usize;
    for (e, known) in crate::select::candidates(d, space, ix, &probe.expand(aperture), true, false)? {
        if !d.is_visible(e) {
            continue;
        }
        let inf = crate::select::is_infinite(e);
        if !inf && !crate::select::bounds_of(d, e, known).expand(aperture).intersects(&probe) {
            continue;
        }
        count += 1;
        if count > 2000 {
            break;
        }
        if osmode & mode::INS != 0 {
            match &e.kind {
                EntityKind::Text(t) => cands.push((mode::INS, t.insert.xy())),
                EntityKind::MText(t) => cands.push((mode::INS, t.insert.xy())),
                EntityKind::Insert(i) => cands.push((mode::INS, i.insert.xy())),
                _ => {}
            }
        }
        if osmode & mode::END != 0
            && let EntityKind::Solid(s) | EntityKind::Trace(s) = &e.kind
        {
            cands.extend(s.corners.iter().map(|c| (mode::END, c.xy())));
        }
        for p in e.kind.prims() {
            match &p {
                Prim::Seg(s) => {
                    if osmode & mode::END != 0 {
                        cands.push((mode::END, s.start()));
                        cands.push((mode::END, s.end()));
                    }
                    if osmode & mode::MID != 0 {
                        cands.push((mode::MID, s.mid()));
                    }
                    if let Segment::Arc { arc, .. } = s {
                        if osmode & mode::CEN != 0 {
                            cands.push((mode::CEN, arc.center));
                        }
                        if osmode & mode::QUA != 0 {
                            for k in 0..4 {
                                let a = k as f64 * PI / 2.0;
                                if arc.contains_angle(a) {
                                    cands.push((mode::QUA, Vec2::polar(arc.center, arc.radius, a)));
                                }
                            }
                        }
                    }
                }
                Prim::Circle(c) => {
                    if osmode & mode::CEN != 0 {
                        cands.push((mode::CEN, c.center));
                    }
                    if osmode & mode::QUA != 0 {
                        for k in 0..4 {
                            cands.push((mode::QUA, Vec2::polar(c.center, c.radius, k as f64 * PI / 2.0)));
                        }
                    }
                }
                Prim::Ellipse(el) => {
                    if osmode & mode::CEN != 0 {
                        cands.push((mode::CEN, el.center));
                    }
                    if osmode & mode::QUA != 0 {
                        for k in 0..4 {
                            cands.push((mode::QUA, el.at_param(k as f64 * PI / 2.0)));
                        }
                    }
                    if osmode & mode::END != 0 && !el.is_full() {
                        cands.push((mode::END, el.at_param(el.start)));
                        cands.push((mode::END, el.at_param(el.end)));
                    }
                }
                Prim::Spline(s) => {
                    if osmode & mode::END != 0 && !s.closed {
                        let (lo, hi) = s.domain();
                        cands.push((mode::END, s.eval(lo)));
                        cands.push((mode::END, s.eval(hi)));
                    }
                }
                Prim::Point(pt) => {
                    if osmode & mode::NOD != 0 && matches!(e.kind, EntityKind::Point(_)) {
                        cands.push((mode::NOD, *pt));
                    }
                }
                Prim::Infinite { base, .. } => {
                    if osmode & mode::END != 0 && matches!(e.kind, EntityKind::Ray(_)) {
                        cands.push((mode::END, *base));
                    }
                }
                Prim::Fill(_) => {}
            }
            near_prims.push(p);
        }
        if osmode & mode::GCEN != 0
            && let EntityKind::LwPolyline(pl) = &e.kind
            && pl.closed
        {
            let pts = cadcraft_geom::Polyline { vertices: pl.vertices.clone(), closed: true }.tessellate(aperture / 10.0);
            if let Some(c) = centroid(&pts) {
                cands.push((mode::GCEN, c));
            }
        }
    }
    // Intersections between nearby primitives.
    if osmode & (mode::INT | mode::APP) != 0 {
        let segs: Vec<Vec<Segment>> = near_prims.iter().take(200).map(prim_segments).collect();
        for i in 0..segs.len() {
            for j in i + 1..segs.len() {
                for a in segs.get(i).into_iter().flatten() {
                    for b in segs.get(j).into_iter().flatten() {
                        for x in intersect_ext(&fix_circle(a), &fix_circle(b), false) {
                            if x.dist(cursor) <= aperture {
                                cands.push((mode::INT, x));
                            }
                        }
                    }
                }
            }
        }
    }
    // Perpendicular / tangent from the base point.
    if let Some(bp) = base {
        for p in &near_prims {
            if osmode & mode::PER != 0 {
                match p {
                    Prim::Seg(Segment::Line(l)) => cands.push((mode::PER, l.project(bp))),
                    Prim::Seg(Segment::Arc { arc, .. }) => cands.push((mode::PER, Circle::new(arc.center, arc.radius).closest(bp))),
                    Prim::Circle(c) => cands.push((mode::PER, c.closest(bp))),
                    Prim::Infinite { base: b0, dir, .. } => cands.push((mode::PER, *b0 + *dir * (bp - *b0).dot(*dir))),
                    _ => {}
                }
            }
            if osmode & mode::TAN != 0 {
                let circ = match p {
                    Prim::Seg(Segment::Arc { arc, .. }) => Some(Circle::new(arc.center, arc.radius)),
                    Prim::Circle(c) => Some(*c),
                    _ => None,
                };
                if let Some(c) = circ {
                    cands.extend(c.tangent_points(bp).into_iter().map(|t| (mode::TAN, t)));
                }
            }
        }
    }
    let best = cands.iter().filter(|(_, p)| p.dist(cursor) <= aperture).min_by(|a, b| {
        // Prefer more specific modes when nearly equidistant.
        let da = a.1.dist(cursor) - if a.0 == mode::INT || a.0 == mode::END { aperture * 0.15 } else { 0.0 };
        let db = b.1.dist(cursor) - if b.0 == mode::INT || b.0 == mode::END { aperture * 0.15 } else { 0.0 };
        da.total_cmp(&db)
    });
    if let Some((m, p)) = best {
        return Some(SnapHit { point: *p, mode: *m, name: name_of(*m) });
    }
    if osmode & mode::NEA != 0 {
        let mut nb: Option<(f64, Vec2)> = None;
        for p in &near_prims {
            for s in prim_segments(p) {
                let c = fix_circle(&s).closest(cursor);
                let dd = c.dist(cursor);
                if dd <= aperture && nb.is_none_or(|(bd, _)| dd < bd) {
                    nb = Some((dd, c));
                }
            }
            if let Prim::Circle(c) = p {
                let q = c.closest(cursor);
                if q.dist(cursor) <= aperture {
                    nb = Some((q.dist(cursor), q));
                }
            }
        }
        if let Some((_, p)) = nb {
            return Some(SnapHit { point: p, mode: mode::NEA, name: "Nearest" });
        }
    }
    None
}

/// Full circles are stored as zero-sweep arcs in `prim_segments`; give them a full sweep.
fn fix_circle(s: &Segment) -> Segment {
    match *s {
        Segment::Arc { arc, ccw } if (arc.start - arc.end).abs() < 1e-15 => {
            Segment::Arc { arc: cadcraft_geom::Arc { start: 0.0, end: cadcraft_geom::TAU - 1e-12, ..arc }, ccw }
        }
        other => other,
    }
}

fn centroid(pts: &[Vec2]) -> Option<Vec2> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let mut a = 0.0;
    let mut c = Vec2::ZERO;
    for i in 0..n {
        let (p, q) = (pts.get(i)?, pts.get((i + 1) % n)?);
        let cr = p.cross(*q);
        a += cr;
        c += (*p + *q) * cr;
    }
    if a.abs() < 1e-12 { None } else { Some(c / (3.0 * a)) }
}

/// Constrain `p` relative to `base` to the nearest multiple of 90° (ortho).
pub fn ortho(base: Vec2, p: Vec2) -> Vec2 {
    let d = p - base;
    if d.x.abs() >= d.y.abs() { Vec2::new(p.x, base.y) } else { Vec2::new(base.x, p.y) }
}

/// Polar tracking: if the direction base→p is within `tol` radians of a multiple of `inc`,
/// return the point projected onto that ray and the angle.
pub fn polar(base: Vec2, p: Vec2, inc: f64, tol: f64) -> Option<(Vec2, f64)> {
    if inc <= 1e-9 {
        return None;
    }
    let d = p - base;
    let len = d.len();
    if len < 1e-12 {
        return None;
    }
    let a = d.angle();
    let k = (a / inc).round();
    let snapped = k * inc;
    if (a - snapped).abs() <= tol {
        let dir = Vec2::from_angle(snapped);
        Some((base + dir * d.dot(dir), cadcraft_geom::norm_angle(snapped)))
    } else {
        None
    }
}

/// Grid snap.
pub fn grid_snap(p: Vec2, unit: Vec2, origin: Vec2) -> Vec2 {
    let sx = if unit.x > 1e-12 { ((p.x - origin.x) / unit.x).round() * unit.x + origin.x } else { p.x };
    let sy = if unit.y > 1e-12 { ((p.y - origin.y) / unit.y).round() * unit.y + origin.y } else { p.y };
    Vec2::new(sx, sy)
}

/// VNCCad: a 2D user coordinate system: origin and X-axis angle (radians) in world coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ucs2 {
    pub origin: Vec2,
    pub angle: f64,
}

impl Ucs2 {
    pub fn is_world(&self) -> bool {
        self.origin == Vec2::ZERO && self.angle.abs() < 1e-12
    }
    /// World → UCS.
    pub fn to_ucs(&self, p: Vec2) -> Vec2 {
        (p - self.origin).rotate(-self.angle)
    }
    /// UCS → world.
    pub fn to_world(&self, p: Vec2) -> Vec2 {
        self.origin + p.rotate(self.angle)
    }
    /// A UCS-relative displacement in world coordinates.
    pub fn dir_to_world(&self, d: Vec2) -> Vec2 {
        d.rotate(self.angle)
    }
}

/// Ortho along the UCS axes.
pub fn ortho_ucs(base: Vec2, p: Vec2, ucs: &Ucs2) -> Vec2 {
    if ucs.angle.abs() < 1e-12 {
        return ortho(base, p);
    }
    let d = (p - base).rotate(-ucs.angle);
    let d = if d.x.abs() >= d.y.abs() { Vec2::new(d.x, 0.0) } else { Vec2::new(0.0, d.y) };
    base + d.rotate(ucs.angle)
}

/// Polar tracking with angles measured from the UCS X axis; the returned angle is in world.
pub fn polar_ucs(base: Vec2, p: Vec2, inc: f64, tol: f64, ucs: &Ucs2) -> Option<(Vec2, f64)> {
    let local = base + (p - base).rotate(-ucs.angle);
    let (q, a) = polar(base, local, inc, tol)?;
    Some((base + (q - base).rotate(ucs.angle), cadcraft_geom::norm_angle(a + ucs.angle)))
}

/// Grid snap on the UCS grid.
pub fn grid_snap_ucs(p: Vec2, unit: Vec2, ucs: &Ucs2) -> Vec2 {
    ucs.to_world(grid_snap(ucs.to_ucs(p), unit, Vec2::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, Line};
    use cadcraft_geom::Vec3;

    fn drawing() -> Drawing {
        let mut d = Drawing::new_imperial();
        let l = |a: (f64, f64), b: (f64, f64)| EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) });
        d.add(&Space::Model, Common::default(), l((0.0, 0.0), (10.0, 0.0))).unwrap();
        d.add(&Space::Model, Common::default(), l((5.0, -5.0), (5.0, 5.0))).unwrap();
        d.add(&Space::Model, Common::default(), EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::new(20.0, 0.0, 0.0), radius: 2.0 })).unwrap();
        d
    }

    #[test]
    fn endpoint_and_mid() {
        let d = drawing();
        let h = osnap(&d, &Space::Model, Vec2::new(9.8, 0.1), 0.5, mode::END | mode::MID, None).unwrap();
        assert_eq!(h.point, Vec2::new(10.0, 0.0));
        let m = osnap(&d, &Space::Model, Vec2::new(5.1, 0.1), 0.5, mode::MID, None).unwrap();
        assert_eq!(m.point, Vec2::new(5.0, 0.0));
    }

    #[test]
    fn intersection() {
        let d = drawing();
        let h = osnap(&d, &Space::Model, Vec2::new(5.2, 0.2), 0.5, mode::INT, None).unwrap();
        assert!(h.point.near(Vec2::new(5.0, 0.0), 1e-9));
        assert_eq!(h.name, "Intersection");
    }

    #[test]
    fn center_quadrant_tangent() {
        let d = drawing();
        assert_eq!(osnap(&d, &Space::Model, Vec2::new(20.1, 0.1), 0.5, mode::CEN, None).unwrap().point, Vec2::new(20.0, 0.0));
        assert!(osnap(&d, &Space::Model, Vec2::new(20.0, 2.1), 0.5, mode::QUA, None).unwrap().point.near(Vec2::new(20.0, 2.0), 1e-9));
        let t = osnap(&d, &Space::Model, Vec2::new(19.9, 1.9), 1.0, mode::TAN, Some(Vec2::new(10.0, 0.0)));
        assert!(t.is_some());
    }

    #[test]
    fn perpendicular_from_base() {
        let d = drawing();
        let h = osnap(&d, &Space::Model, Vec2::new(3.0, 0.2), 0.5, mode::PER, Some(Vec2::new(3.0, 4.0))).unwrap();
        assert!(h.point.near(Vec2::new(3.0, 0.0), 1e-9));
    }

    #[test]
    fn ucs_ortho_polar_grid() {
        let u = Ucs2 { origin: Vec2::new(100.0, 50.0), angle: 30f64.to_radians() };
        let p = Vec2::new(7.0, 3.0);
        assert!(u.to_ucs(u.to_world(p)).near(p, 1e-9));
        assert!(u.to_world(Vec2::new(10.0, 0.0)).near(Vec2::new(100.0 + 10.0 * 0.866_025_403_784_438_6, 55.0), 1e-9));
        // Ortho follows the rotated X axis.
        let b = Vec2::new(100.0, 50.0);
        let q = ortho_ucs(b, b + Vec2::new(10.0, 6.5), &u);
        assert!(u.to_ucs(q).y.abs() < 1e-9);
        // 0° in the UCS is 30° in the world.
        let (_, a) = polar_ucs(b, b + Vec2::from_angle(31f64.to_radians()) * 5.0, 90f64.to_radians(), 0.05, &u).unwrap();
        assert!((a - 30f64.to_radians()).abs() < 1e-9);
        let g = grid_snap_ucs(u.to_world(Vec2::new(9.8, 20.3)), Vec2::new(10.0, 10.0), &u);
        assert!(u.to_ucs(g).near(Vec2::new(10.0, 20.0), 1e-9));
    }

    #[test]
    fn ortho_and_polar() {
        assert_eq!(ortho(Vec2::ZERO, Vec2::new(5.0, 1.0)), Vec2::new(5.0, 0.0));
        assert_eq!(ortho(Vec2::ZERO, Vec2::new(1.0, 5.0)), Vec2::new(0.0, 5.0));
        let (p, a) = polar(Vec2::ZERO, Vec2::new(5.0, 5.1), 45f64.to_radians(), 3f64.to_radians()).unwrap();
        assert!((a - PI / 4.0).abs() < 1e-12);
        assert!((p.x - p.y).abs() < 1e-9);
        assert_eq!(grid_snap(Vec2::new(0.74, 1.26), Vec2::new(0.5, 0.5), Vec2::ZERO), Vec2::new(0.5, 1.5));
    }

    #[test]
    fn off_bit_disables() {
        let d = drawing();
        assert!(osnap(&d, &Space::Model, Vec2::new(9.8, 0.1), 0.5, mode::END | mode::OFF, None).is_none());
    }
}
