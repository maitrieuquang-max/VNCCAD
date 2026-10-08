//! Maps constrained entities to a flat parameter vector and back.
//!
//! - Line: `ax ay bx by`
//! - Circle: `cx cy r`
//! - Arc: `cx cy r start end` (angles in radians)
//! - LwPolyline: `x0 y0 x1 y1 …` (bulges are kept; segments are treated as straight lines)
//! - Point: `x y`

use std::collections::HashMap;

use cadcraft_doc::{Drawing, EntityKind, GeomRef, Handle, Sub};
use cadcraft_geom::{Vec2, ccw_sweep};

/// Most parameters one solve may have.
pub const MAX_PARAMS: usize = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Line,
    Circle,
    Arc,
    Poly { n: usize, closed: bool },
    Point,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub handle: Handle,
    pub shape: Shape,
    pub off: usize,
    pub len: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    pub slots: Vec<Slot>,
    pub x0: Vec<f64>,
    index: HashMap<Handle, usize>,
}

/// Can this entity take part in constraints?
pub fn supported(kind: &EntityKind) -> bool {
    matches!(kind, EntityKind::Line(_) | EntityKind::Circle(_) | EntityKind::Arc(_) | EntityKind::LwPolyline(_) | EntityKind::Point(_))
}

fn p2(x: &[f64], i: usize) -> Option<Vec2> {
    Some(Vec2::new(*x.get(i)?, *x.get(i + 1)?))
}

impl Model {
    /// Build a model for the given entities. Unsupported or missing entities are skipped.
    pub fn build(d: &Drawing, handles: &[Handle]) -> Model {
        let mut m = Model::default();
        for &h in handles {
            if m.index.contains_key(&h) {
                continue;
            }
            let Some(e) = d.entity(h) else { continue };
            let off = m.x0.len();
            let shape = match &e.kind {
                EntityKind::Line(l) => {
                    m.x0.extend([l.a.x, l.a.y, l.b.x, l.b.y]);
                    Shape::Line
                }
                EntityKind::Circle(c) => {
                    m.x0.extend([c.center.x, c.center.y, c.radius]);
                    Shape::Circle
                }
                EntityKind::Arc(a) => {
                    m.x0.extend([a.center.x, a.center.y, a.radius, a.start, a.end]);
                    Shape::Arc
                }
                EntityKind::LwPolyline(p) => {
                    if p.vertices.len() < 2 || off + p.vertices.len() * 2 > MAX_PARAMS {
                        continue;
                    }
                    for v in &p.vertices {
                        m.x0.extend([v.p.x, v.p.y]);
                    }
                    Shape::Poly { n: p.vertices.len(), closed: p.closed }
                }
                EntityKind::Point(p) => {
                    m.x0.extend([p.p.x, p.p.y]);
                    Shape::Point
                }
                _ => continue,
            };
            if m.x0.len() > MAX_PARAMS || m.x0.get(off..).unwrap_or(&[]).iter().any(|v| !v.is_finite()) {
                m.x0.truncate(off);
                continue;
            }
            let len = m.x0.len() - off;
            m.index.insert(h, m.slots.len());
            m.slots.push(Slot { handle: h, shape, off, len });
        }
        m
    }

    pub fn slot(&self, h: Handle) -> Option<&Slot> {
        self.slots.get(*self.index.get(&h)?)
    }

    /// A point-like reference.
    pub fn point(&self, x: &[f64], r: &GeomRef) -> Option<Vec2> {
        let s = self.slot(r.handle)?;
        let o = s.off;
        match (s.shape, r.sub) {
            (Shape::Line, Sub::Start) => p2(x, o),
            (Shape::Line, Sub::End) => p2(x, o + 2),
            (Shape::Line, Sub::Mid) => Some(p2(x, o)?.mid(p2(x, o + 2)?)),
            (Shape::Circle | Shape::Arc, Sub::Center) => p2(x, o),
            (Shape::Arc, Sub::Start | Sub::End | Sub::Mid) => {
                let c = p2(x, o)?;
                let rad = *x.get(o + 2)?;
                let (a0, a1) = (*x.get(o + 3)?, *x.get(o + 4)?);
                let a = match r.sub {
                    Sub::Start => a0,
                    Sub::End => a1,
                    _ => a0 + ccw_sweep(a0, a1) / 2.0,
                };
                Some(Vec2::polar(c, rad, a))
            }
            (Shape::Poly { n, .. }, Sub::Vertex(i)) => {
                let i = i as usize;
                if i < n { p2(x, o + 2 * i) } else { None }
            }
            (Shape::Poly { .. }, Sub::Start) => p2(x, o),
            (Shape::Poly { n, .. }, Sub::End) => p2(x, o + 2 * n.checked_sub(1)?),
            (Shape::Poly { .. }, Sub::Mid) => None,
            (Shape::Point, Sub::Whole | Sub::Start | Sub::Center) => p2(x, o),
            _ => None,
        }
    }

    /// Vertex indices of a polyline segment.
    fn seg(n: usize, closed: bool, i: usize) -> Option<(usize, usize)> {
        if i + 1 < n {
            Some((i, i + 1))
        } else if closed && i + 1 == n && n > 2 {
            Some((i, 0))
        } else {
            None
        }
    }

    /// A line-like reference (line or polyline segment).
    pub fn line(&self, x: &[f64], r: &GeomRef) -> Option<(Vec2, Vec2)> {
        let s = self.slot(r.handle)?;
        let o = s.off;
        match (s.shape, r.sub) {
            (Shape::Line, Sub::Whole) => Some((p2(x, o)?, p2(x, o + 2)?)),
            (Shape::Poly { n, closed }, Sub::Segment(i)) => {
                let (a, b) = Self::seg(n, closed, i as usize)?;
                Some((p2(x, o + 2 * a)?, p2(x, o + 2 * b)?))
            }
            (Shape::Poly { n: 2, .. }, Sub::Whole) => Some((p2(x, o)?, p2(x, o + 2)?)),
            _ => None,
        }
    }

    /// A circle-like reference (circle or arc).
    pub fn circle(&self, x: &[f64], r: &GeomRef) -> Option<(Vec2, f64)> {
        let s = self.slot(r.handle)?;
        match (s.shape, r.sub) {
            (Shape::Circle | Shape::Arc, Sub::Whole) => Some((p2(x, s.off)?, *x.get(s.off + 2)?)),
            _ => None,
        }
    }

    /// Arc sweep (radians) of an arc reference.
    pub fn arc_sweep(&self, x: &[f64], r: &GeomRef) -> Option<f64> {
        let s = self.slot(r.handle)?;
        match (s.shape, r.sub) {
            (Shape::Arc, Sub::Whole) => Some(ccw_sweep(*x.get(s.off + 3)?, *x.get(s.off + 4)?)),
            _ => None,
        }
    }

    /// Parameters a Fix on this reference locks directly. `None` when the reference is a derived
    /// point (fixed through a residual instead).
    pub fn lock_indices(&self, r: &GeomRef) -> Option<Vec<usize>> {
        let s = self.slot(r.handle)?;
        let o = s.off;
        let v = match (s.shape, r.sub) {
            (_, Sub::Whole) => (o..o + s.len).collect(),
            (Shape::Line, Sub::Start) => vec![o, o + 1],
            (Shape::Line, Sub::End) => vec![o + 2, o + 3],
            (Shape::Circle | Shape::Arc, Sub::Center) => vec![o, o + 1],
            (Shape::Poly { n, .. }, Sub::Vertex(i)) if (i as usize) < n => vec![o + 2 * i as usize, o + 2 * i as usize + 1],
            (Shape::Poly { .. }, Sub::Start) => vec![o, o + 1],
            (Shape::Poly { n, .. }, Sub::End) if n > 0 => vec![o + 2 * (n - 1), o + 2 * (n - 1) + 1],
            (Shape::Poly { n, closed }, Sub::Segment(i)) => {
                let (a, b) = Self::seg(n, closed, i as usize)?;
                vec![o + 2 * a, o + 2 * a + 1, o + 2 * b, o + 2 * b + 1]
            }
            _ => return None,
        };
        Some(v)
    }

    /// Write solved parameters back. Returns the handles that moved.
    pub fn write_back(&self, d: &mut Drawing, x: &[f64]) -> Vec<Handle> {
        let mut moved = Vec::new();
        for s in &self.slots {
            let (Some(new), Some(old)) = (x.get(s.off..s.off + s.len), self.x0.get(s.off..s.off + s.len)) else { continue };
            if new.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let scale = old.iter().fold(1.0f64, |m, v| m.max(v.abs()));
            if new.iter().zip(old).all(|(a, b)| (a - b).abs() <= 1e-13 * scale) {
                continue;
            }
            let shape = s.shape;
            let ok = d.modify_entity(s.handle, |e| match (&mut e.kind, shape) {
                (EntityKind::Line(l), Shape::Line) => {
                    if let [ax, ay, bx, by] = new {
                        l.a.x = *ax;
                        l.a.y = *ay;
                        l.b.x = *bx;
                        l.b.y = *by;
                    }
                }
                (EntityKind::Circle(c), Shape::Circle) => {
                    if let [cx, cy, r] = new {
                        c.center.x = *cx;
                        c.center.y = *cy;
                        c.radius = r.abs().max(1e-12);
                    }
                }
                (EntityKind::Arc(a), Shape::Arc) => {
                    if let [cx, cy, r, s0, s1] = new {
                        a.center.x = *cx;
                        a.center.y = *cy;
                        a.radius = r.abs().max(1e-12);
                        a.start = cadcraft_geom::norm_angle(*s0);
                        a.end = cadcraft_geom::norm_angle(*s1);
                    }
                }
                (EntityKind::LwPolyline(p), Shape::Poly { n, .. }) if p.vertices.len() == n => {
                    for (i, v) in p.vertices.iter_mut().enumerate() {
                        if let (Some(px), Some(py)) = (new.get(2 * i), new.get(2 * i + 1)) {
                            v.p = Vec2::new(*px, *py);
                        }
                    }
                }
                (EntityKind::Point(p), Shape::Point) => {
                    if let [px, py] = new {
                        p.p.x = *px;
                        p.p.y = *py;
                    }
                }
                _ => {}
            });
            if ok.is_ok() {
                moved.push(s.handle);
            }
        }
        moved
    }
}
