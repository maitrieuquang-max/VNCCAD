//! VNCCad: the Visual LISP curve functions (`vlax-curve-*`) and a working subset of the
//! ActiveX object functions (`vla-*`, `vlax-*`) on the drawing's objects.
//!
//! VLA objects are entity names: `vlax-ename->vla-object` returns its argument, so routines that
//! mix both styles keep working. Properties are read and written by name (`vla-get-Layer`,
//! `(vlax-get obj 'Length)`, `(vlax-put-property obj "Color" 1)`).

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, EntityKind, Handle};
use cadcraft_geom::{Arc, Line, PolyVertex, Segment, Vec2, Vec3, bulge_to_arc};
use serde_json::json;

use super::{Ex, R, Run, V, err};

fn arg(a: &[V], i: usize) -> &V {
    a.get(i).unwrap_or(&V::Nil)
}

/// A curve as segments, each with its parameter range.
struct Curve {
    segs: Vec<(Segment, f64, f64)>,
    closed: bool,
    area: f64,
}

impl Curve {
    fn of(d: &Drawing, h: Handle) -> Option<Curve> {
        let e = d.entity(h)?;
        let mut segs = Vec::new();
        let (closed, area);
        match &e.kind {
            EntityKind::Line(l) => {
                let len = l.a.xy().dist(l.b.xy());
                segs.push((Segment::Line(Line::new(l.a.xy(), l.b.xy())), 0.0, len));
                closed = false;
                area = 0.0;
            }
            EntityKind::Arc(a) => {
                let arc = Arc::new(a.center.xy(), a.radius, a.start, a.end);
                let s = arc.sweep();
                segs.push((Segment::Arc { arc, ccw: true }, a.start, a.start + s));
                closed = false;
                area = 0.5 * a.radius * a.radius * (s - s.sin());
            }
            EntityKind::Circle(c) => {
                let arc = Arc::new(c.center.xy(), c.radius, 0.0, std::f64::consts::TAU);
                segs.push((Segment::Arc { arc, ccw: true }, 0.0, std::f64::consts::TAU));
                closed = true;
                area = std::f64::consts::PI * c.radius * c.radius;
            }
            EntityKind::LwPolyline(p) => {
                let n = p.vertices.len();
                let count = if p.closed { n } else { n.saturating_sub(1) };
                for i in 0..count {
                    let (Some(v), Some(w)) = (p.vertices.get(i), p.vertices.get((i + 1) % n.max(1))) else { continue };
                    let seg = match bulge_to_arc(v.p, w.p, v.bulge) {
                        Some((arc, ccw)) => Segment::Arc { arc, ccw },
                        None => Segment::Line(Line::new(v.p, w.p)),
                    };
                    segs.push((seg, i as f64, i as f64 + 1.0));
                }
                closed = p.closed;
                area = cadcraft_geom::Polyline { vertices: p.vertices.clone(), closed: true }.area().abs();
            }
            other => {
                // Ellipses, splines…: their outline as short straight pieces.
                let _ = other;
                let pts: Vec<Vec2> = crate::select::hit_polylines(d, e, 1e-4).into_iter().flatten().collect();
                if pts.len() < 2 {
                    return None;
                }
                let mut acc = 0.0;
                for w in pts.windows(2) {
                    let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                    let l = a.dist(*b);
                    segs.push((Segment::Line(Line::new(*a, *b)), acc, acc + l));
                    acc += l;
                }
                closed = pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-9));
                area = if closed { cadcraft_geom::Polyline::from_points(&pts, true).area().abs() } else { 0.0 };
            }
        }
        if segs.is_empty() {
            return None;
        }
        Some(Curve { segs, closed, area })
    }
    fn start_param(&self) -> f64 {
        self.segs.first().map_or(0.0, |s| s.1)
    }
    fn end_param(&self) -> f64 {
        self.segs.last().map_or(0.0, |s| s.2)
    }
    fn length(&self) -> f64 {
        self.segs.iter().map(|s| s.0.len()).sum()
    }
    /// (segment index, fraction 0..1) at a parameter.
    fn at_param(&self, t: f64) -> Option<(usize, f64)> {
        let t = t.clamp(self.start_param(), self.end_param());
        let i = self.segs.iter().position(|s| t <= s.2 + 1e-12).unwrap_or(self.segs.len().saturating_sub(1));
        let (_, a, b) = self.segs.get(i)?;
        Some((i, if b - a > 1e-15 { (t - a) / (b - a) } else { 0.0 }))
    }
    fn dist_at_param(&self, t: f64) -> Option<f64> {
        let (i, f) = self.at_param(t)?;
        let before: f64 = self.segs.iter().take(i).map(|s| s.0.len()).sum();
        Some(before + self.segs.get(i)?.0.len() * f)
    }
    fn param_at_dist(&self, d: f64) -> Option<f64> {
        if d < -1e-9 || d > self.length() + 1e-9 {
            return None;
        }
        let mut acc = 0.0;
        for (s, a, b) in &self.segs {
            let l = s.len();
            if d <= acc + l + 1e-12 {
                let f = if l > 1e-15 { ((d - acc) / l).clamp(0.0, 1.0) } else { 0.0 };
                return Some(a + (b - a) * f);
            }
            acc += l;
        }
        Some(self.end_param())
    }
    fn point_at_param(&self, t: f64) -> Option<Vec2> {
        let (i, f) = self.at_param(t)?;
        Some(self.segs.get(i)?.0.at(f))
    }
    fn deriv_at_param(&self, t: f64) -> Option<Vec2> {
        let (i, f) = self.at_param(t)?;
        let (s, a, b) = self.segs.get(i)?;
        // d(point)/d(param): unit tangent × (length per parameter unit).
        let k = if b - a > 1e-15 { s.len() / (b - a) } else { 0.0 };
        Some(s.tangent(f) * k)
    }
    /// Closest point and its parameter.
    fn closest(&self, p: Vec2) -> Option<(Vec2, f64)> {
        let mut best: Option<(f64, Vec2, f64)> = None;
        for (s, a, b) in &self.segs {
            let q = s.closest(p);
            let f = fraction_on(s, q);
            let d = q.dist(p);
            if best.is_none_or(|(bd, _, _)| d < bd - 1e-12) {
                best = Some((d, q, a + (b - a) * f));
            }
        }
        best.map(|(_, q, t)| (q, t))
    }
}

/// Fraction 0..1 of a point lying on a segment, in segment direction.
fn fraction_on(s: &Segment, q: Vec2) -> f64 {
    match s {
        Segment::Line(l) => {
            let d = l.b - l.a;
            if d.len2() > 1e-24 { ((q - l.a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 }
        }
        Segment::Arc { arc, ccw } => {
            let sweep = arc.sweep().max(1e-15);
            let mut a = (q - arc.center).angle() - arc.start;
            while a < -1e-12 {
                a += std::f64::consts::TAU;
            }
            while a > std::f64::consts::TAU + 1e-12 {
                a -= std::f64::consts::TAU;
            }
            let f = (a / sweep).clamp(0.0, 1.0);
            if *ccw { f } else { 1.0 - f }
        }
    }
}

fn object_name(k: &EntityKind) -> &'static str {
    match k {
        EntityKind::Line(_) => "AcDbLine",
        EntityKind::Circle(_) => "AcDbCircle",
        EntityKind::Arc(_) => "AcDbArc",
        EntityKind::LwPolyline(_) => "AcDbPolyline",
        EntityKind::Text(_) => "AcDbText",
        EntityKind::MText(_) => "AcDbMText",
        EntityKind::Insert(_) => "AcDbBlockReference",
        EntityKind::Point(_) => "AcDbPoint",
        EntityKind::Ellipse(_) => "AcDbEllipse",
        EntityKind::Hatch(_) => "AcDbHatch",
        EntityKind::Dimension(_) => "AcDbRotatedDimension",
        EntityKind::Spline(_) => "AcDbSpline",
        _ => "AcDbEntity",
    }
}

fn v3(p: Vec3) -> V {
    V::pt3(p.x, p.y, p.z)
}

impl Run<'_> {
    fn ename(&self, v: &V) -> R<Handle> {
        match v {
            V::Ename(h) if self.s.doc().is_ok_and(|d| d.entity(*h).is_some()) => Ok(*h),
            V::Ename(_) => err("đối tượng đã bị xóa"),
            _ => err("cần một đối tượng (ename hoặc vla-object)"),
        }
    }

    fn curve(&self, v: &V) -> R<Curve> {
        let h = self.ename(v)?;
        let d = self.s.doc().map_err(|e| Ex::Error(e.to_string()))?;
        Curve::of(d, h).ok_or_else(|| Ex::Error("đối tượng không phải đường cong".into()))
    }

    fn pt_w(&self, v: &V) -> R<Vec2> {
        v.point().ok_or_else(|| Ex::Error("cần một điểm".into()))
    }

    /// `vlax-curve-*` (points in world coordinates, as in AutoCAD).
    pub(super) fn vlax_curve(&mut self, name: &str, a: &[V]) -> Option<R<V>> {
        let f = name.strip_prefix("vlax-curve-")?;
        let r = (|| -> R<V> {
            let c = self.curve(arg(a, 0))?;
            let p3 = |p: Vec2| V::pt3(p.x, p.y, 0.0);
            let num = |i: usize| arg(a, i).num().ok_or_else(|| Ex::Error(format!("{name}: cần một số")));
            Ok(match f.to_ascii_lowercase().as_str() {
                "getstartparam" => V::Real(c.start_param()),
                "getendparam" => V::Real(c.end_param()),
                "getstartpoint" => c.point_at_param(c.start_param()).map_or(V::Nil, p3),
                "getendpoint" => c.point_at_param(c.end_param()).map_or(V::Nil, p3),
                "isclosed" => V::from_bool(c.closed),
                "isperiodic" => V::from_bool(c.closed && c.segs.len() == 1),
                "isplanar" => V::T,
                "getarea" => V::Real(c.area),
                "getdistatparam" => {
                    let t = num(1)?;
                    if t < c.start_param() - 1e-9 || t > c.end_param() + 1e-9 { V::Nil } else { c.dist_at_param(t).map_or(V::Nil, V::Real) }
                }
                "getparamatdist" => c.param_at_dist(num(1)?).map_or(V::Nil, V::Real),
                "getpointatparam" => {
                    let t = num(1)?;
                    if t < c.start_param() - 1e-9 || t > c.end_param() + 1e-9 { V::Nil } else { c.point_at_param(t).map_or(V::Nil, p3) }
                }
                "getpointatdist" => c.param_at_dist(num(1)?).and_then(|t| c.point_at_param(t)).map_or(V::Nil, p3),
                "getparamatpoint" | "getdistatpoint" => {
                    let p = self.pt_w(arg(a, 1))?;
                    match c.closest(p) {
                        Some((q, t)) if q.dist(p) <= 1e-6 * (1.0 + c.length()) => {
                            if f.eq_ignore_ascii_case("getparamatpoint") {
                                V::Real(t)
                            } else {
                                c.dist_at_param(t).map_or(V::Nil, V::Real)
                            }
                        }
                        _ => V::Nil,
                    }
                }
                "getclosestpointto" | "getclosestpointtoprojection" => {
                    let p = self.pt_w(arg(a, 1))?;
                    let extend = arg(a, if f.eq_ignore_ascii_case("getclosestpointto") { 2 } else { 3 }).truthy();
                    let (mut q, _) = c.closest(p).ok_or_else(|| Ex::Error("đường cong rỗng".into()))?;
                    if extend && let Some((Segment::Line(l), _, _)) = c.segs.first().filter(|_| c.segs.len() == 1) {
                        q = l.project(p);
                    }
                    p3(q)
                }
                "getfirstderiv" => c.deriv_at_param(num(1)?).map_or(V::Nil, p3),
                "getsecondderiv" => V::pt3(0.0, 0.0, 0.0),
                _ => return err(format!("chưa hỗ trợ {name}")),
            })
        })();
        Some(r)
    }

    fn prop_get(&self, h: Handle, prop: &str) -> R<V> {
        let d = self.s.doc().map_err(|e| Ex::Error(e.to_string()))?;
        let e = d.entity(h).ok_or_else(|| Ex::Error("đối tượng đã bị xóa".into()))?;
        let p = prop.to_ascii_lowercase();
        let curve = || Curve::of(d, h);
        Ok(match (p.as_str(), &e.kind) {
            ("layer", _) => V::Str(e.common.layer.clone()),
            ("color", _) => V::Int(match e.common.color {
                Color::ByLayer => 256,
                Color::ByBlock => 0,
                Color::Index(i) => i64::from(i),
                Color::True(_) => 256,
            }),
            ("linetype", _) => V::Str(e.common.linetype.clone()),
            ("handle", _) => V::Str(h.hex()),
            ("objectname", k) => V::Str(object_name(k).into()),
            ("objectid", _) => V::Int(h.0 as i64),
            ("visible", _) => V::from_bool(e.common.visible),
            ("startpoint", EntityKind::Line(l)) => v3(l.a),
            ("endpoint", EntityKind::Line(l)) => v3(l.b),
            ("startpoint" | "endpoint", _) => {
                let c = curve().ok_or_else(|| Ex::Error("không phải đường cong".into()))?;
                let t = if p == "startpoint" { c.start_param() } else { c.end_param() };
                c.point_at_param(t).map_or(V::Nil, |q| V::pt3(q.x, q.y, 0.0))
            }
            ("center", EntityKind::Circle(c)) => v3(c.center),
            ("center", EntityKind::Arc(c)) => v3(c.center),
            ("radius", EntityKind::Circle(c)) => V::Real(c.radius),
            ("radius", EntityKind::Arc(c)) => V::Real(c.radius),
            ("diameter", EntityKind::Circle(c)) => V::Real(c.radius * 2.0),
            ("startangle", EntityKind::Arc(c)) => V::Real(c.start),
            ("endangle", EntityKind::Arc(c)) => V::Real(c.end),
            ("angle", EntityKind::Line(l)) => V::Real(cadcraft_geom::norm_angle((l.b.xy() - l.a.xy()).angle())),
            ("length" | "arclength" | "circumference", _) => V::Real(curve().map_or(0.0, |c| c.length())),
            ("area", EntityKind::Hatch(_)) => V::Real(0.0),
            ("area", _) => V::Real(curve().map_or(0.0, |c| c.area)),
            ("closed", EntityKind::LwPolyline(pl)) => V::from_bool(pl.closed),
            ("coordinates", EntityKind::LwPolyline(pl)) => V::list(pl.vertices.iter().flat_map(|v| [V::Real(v.p.x), V::Real(v.p.y)]).collect()),
            ("constantwidth", EntityKind::LwPolyline(pl)) => V::Real(pl.const_width),
            ("elevation", EntityKind::LwPolyline(pl)) => V::Real(pl.elevation),
            ("textstring", EntityKind::Text(t)) => V::Str(t.value.clone()),
            ("textstring", EntityKind::MText(t)) => V::Str(t.contents.clone()),
            ("height", EntityKind::Text(t)) => V::Real(t.height),
            ("height", EntityKind::MText(t)) => V::Real(t.height),
            ("rotation", EntityKind::Text(t)) => V::Real(t.rotation),
            ("rotation", EntityKind::MText(t)) => V::Real(t.rotation),
            ("rotation", EntityKind::Insert(i)) => V::Real(i.rotation),
            ("stylename", EntityKind::Text(t)) => V::Str(t.style.clone()),
            ("stylename", EntityKind::MText(t)) => V::Str(t.style.clone()),
            ("insertionpoint", EntityKind::Text(t)) => v3(t.insert),
            ("insertionpoint", EntityKind::MText(t)) => v3(t.insert),
            ("insertionpoint", EntityKind::Insert(i)) => v3(i.insert),
            ("insertionpoint", EntityKind::Point(pt)) | ("coordinates", EntityKind::Point(pt)) => v3(pt.p),
            ("name" | "effectivename", EntityKind::Insert(i)) => V::Str(i.block.clone()),
            ("xscalefactor", EntityKind::Insert(i)) => V::Real(i.scale.x),
            ("yscalefactor", EntityKind::Insert(i)) => V::Real(i.scale.y),
            ("zscalefactor", EntityKind::Insert(i)) => V::Real(i.scale.z),
            ("hasattributes", EntityKind::Insert(i)) => V::from_bool(!i.attribs.is_empty()),
            _ => return err(format!("thuộc tính {prop} không có trên {}", object_name(&e.kind))),
        })
    }

    fn prop_put(&mut self, h: Handle, prop: &str, v: &V) -> R<V> {
        let p = prop.to_ascii_lowercase();
        let num = v.num();
        let pt = v.point();
        let text = if let V::Str(s) = v { Some(s.clone()) } else { None };
        let d = self.s.doc_mut().map_err(|e| Ex::Error(e.to_string()))?;
        if p == "layer"
            && let Some(l) = &text
        {
            d.ensure_layer(l);
        }
        let mut ok = true;
        let r = d.modify_entity(h, |e| {
            let c: &mut Common = &mut e.common;
            match (p.as_str(), &mut e.kind) {
                ("layer", _) if text.is_some() => c.layer = text.clone().unwrap_or_default(),
                ("color", _) if num.is_some() => {
                    let n = num.unwrap_or(256.0) as i64;
                    c.color = match n {
                        0 => Color::ByBlock,
                        1..=255 => Color::Index(n as u8),
                        _ => Color::ByLayer,
                    }
                }
                ("linetype", _) if text.is_some() => c.linetype = text.clone().unwrap_or_default(),
                ("visible", _) => c.visible = v.truthy() && !matches!(v, V::Int(0)),
                ("startpoint", EntityKind::Line(l)) if pt.is_some() => l.a = pt.unwrap_or_default().to3(l.a.z),
                ("endpoint", EntityKind::Line(l)) if pt.is_some() => l.b = pt.unwrap_or_default().to3(l.b.z),
                ("center", EntityKind::Circle(x)) if pt.is_some() => x.center = pt.unwrap_or_default().to3(x.center.z),
                ("center", EntityKind::Arc(x)) if pt.is_some() => x.center = pt.unwrap_or_default().to3(x.center.z),
                ("radius", EntityKind::Circle(x)) if num.is_some_and(|r| r > 0.0) => x.radius = num.unwrap_or(1.0),
                ("radius", EntityKind::Arc(x)) if num.is_some_and(|r| r > 0.0) => x.radius = num.unwrap_or(1.0),
                ("diameter", EntityKind::Circle(x)) if num.is_some_and(|r| r > 0.0) => x.radius = num.unwrap_or(2.0) / 2.0,
                ("closed", EntityKind::LwPolyline(x)) => x.closed = v.truthy() && !matches!(v, V::Int(0)),
                ("constantwidth", EntityKind::LwPolyline(x)) if num.is_some() => x.const_width = num.unwrap_or(0.0).max(0.0),
                ("coordinates", EntityKind::LwPolyline(x)) => {
                    let xs: Vec<f64> = v.items().unwrap_or(&[]).iter().filter_map(V::num).collect();
                    if xs.len() >= 4 {
                        x.vertices = xs.chunks(2).filter_map(|c| Some(PolyVertex::new(Vec2::new(*c.first()?, *c.get(1)?)))).collect();
                    } else {
                        ok = false;
                    }
                }
                ("textstring", EntityKind::Text(t)) if text.is_some() => t.value = text.clone().unwrap_or_default(),
                ("textstring", EntityKind::MText(t)) if text.is_some() => t.contents = text.clone().unwrap_or_default(),
                ("height", EntityKind::Text(t)) if num.is_some_and(|x| x > 0.0) => t.height = num.unwrap_or(1.0),
                ("height", EntityKind::MText(t)) if num.is_some_and(|x| x > 0.0) => t.height = num.unwrap_or(1.0),
                ("rotation", EntityKind::Text(t)) if num.is_some() => t.rotation = num.unwrap_or(0.0),
                ("rotation", EntityKind::MText(t)) if num.is_some() => t.rotation = num.unwrap_or(0.0),
                ("rotation", EntityKind::Insert(i)) if num.is_some() => i.rotation = num.unwrap_or(0.0),
                ("insertionpoint", EntityKind::Text(t)) if pt.is_some() => t.insert = pt.unwrap_or_default().to3(t.insert.z),
                ("insertionpoint", EntityKind::MText(t)) if pt.is_some() => t.insert = pt.unwrap_or_default().to3(t.insert.z),
                ("insertionpoint", EntityKind::Insert(i)) if pt.is_some() => i.insert = pt.unwrap_or_default().to3(i.insert.z),
                ("stylename", EntityKind::Text(t)) if text.is_some() => t.style = text.clone().unwrap_or_default(),
                ("xscalefactor", EntityKind::Insert(i)) if num.is_some() => i.scale.x = num.unwrap_or(1.0),
                ("yscalefactor", EntityKind::Insert(i)) if num.is_some() => i.scale.y = num.unwrap_or(1.0),
                _ => ok = false,
            }
        });
        if r.is_err() || !ok {
            return err(format!("không gán được thuộc tính {prop}"));
        }
        self.s.touch();
        Ok(V::Nil)
    }

    fn space_add(&mut self, cmd: &str, p: serde_json::Value) -> R<V> {
        self.s.execute(cmd, &p).map_err(|e| Ex::Error(e.to_string()))?;
        let space = self.s.space();
        Ok(self.s.doc().ok().and_then(|d| d.space(&space)?.last().map(|e| V::Ename(e.handle))).unwrap_or(V::Nil))
    }

    /// `vla-*` and `vlax-*` (other than the curve functions).
    pub(super) fn vla(&mut self, name: &str, a: &[V]) -> Option<R<V>> {
        let n = name.to_ascii_lowercase();
        if n.starts_with("vlax-curve-") {
            return self.vlax_curve(&n, a);
        }
        if let Some(r) = self.vla_extra(&n, a) {
            return Some(r);
        }
        let prop_of = |v: &V| -> Option<String> {
            match v {
                V::Sym(s) => Some(s.clone()),
                V::Str(s) => Some(s.clone()),
                _ => None,
            }
        };
        let r: R<V> = match n.as_str() {
            "vlax-ename->vla-object" | "vlax-vla-object->ename" | "vlax-variant-value" | "vlax-safearray->list" | "vlax-make-variant" => {
                Ok(arg(a, 0).clone())
            }
            "vlax-3d-point" => {
                let p = if a.len() >= 2 { V::list(a.to_vec()) } else { arg(a, 0).clone() };
                Ok(match p.point() {
                    Some(q) => V::pt3(q.x, q.y, p.items().and_then(|i| i.get(2)).and_then(V::num).unwrap_or(0.0)),
                    None => p,
                })
            }
            "vlax-get-acad-object" => Ok(V::Sym("*acad*".into())),
            "vla-get-activedocument" => Ok(V::Sym("*doc*".into())),
            "vla-get-modelspace" | "vla-get-paperspace" | "vla-get-block" => Ok(V::Sym("*space*".into())),
            "vlax-release-object" | "vla-update" | "vla-regen" | "vla-zoomextents" => Ok(V::Nil),
            "vla-delete" | "vlax-erase" => {
                let h = match self.ename(arg(a, 0)) {
                    Ok(h) => h,
                    Err(e) => return Some(Err(e)),
                };
                let removed = self.s.doc_mut().ok().and_then(|d| d.remove_entity(h)).is_some();
                self.s.touch();
                Ok(V::from_bool(removed))
            }
            "vlax-property-available-p" => match (self.ename(arg(a, 0)), prop_of(arg(a, 1))) {
                (Ok(h), Some(p)) => Ok(V::from_bool(self.prop_get(h, &p).is_ok())),
                _ => Ok(V::Nil),
            },
            "vlax-get-property" | "vlax-get" => match (self.ename(arg(a, 0)), prop_of(arg(a, 1))) {
                (Ok(h), Some(p)) => self.prop_get(h, &p),
                (Err(e), _) => Err(e),
                _ => err(format!("{name}: cần tên thuộc tính")),
            },
            "vlax-put-property" | "vlax-put" => match (self.ename(arg(a, 0)), prop_of(arg(a, 1))) {
                (Ok(h), Some(p)) => {
                    let v = arg(a, 2).clone();
                    self.prop_put(h, &p, &v)
                }
                (Err(e), _) => Err(e),
                _ => err(format!("{name}: cần tên thuộc tính")),
            },
            "vla-addline" => match (arg(a, 1).point(), arg(a, 2).point()) {
                (Some(p), Some(q)) => self.space_add("line", json!({ "points": [[p.x, p.y], [q.x, q.y]] })),
                _ => err("vla-AddLine: cần 2 điểm"),
            },
            "vla-addcircle" => match (arg(a, 1).point(), arg(a, 2).num()) {
                (Some(c), Some(r)) => self.space_add("circle", json!({ "center": [c.x, c.y], "radius": r })),
                _ => err("vla-AddCircle: cần tâm và bán kính"),
            },
            "vla-addtext" => match (arg(a, 1), arg(a, 2).point(), arg(a, 3).num()) {
                (V::Str(t), Some(p), Some(h)) => self.space_add("text", json!({ "at": [p.x, p.y], "text": t, "height": h })),
                _ => err("vla-AddText: cần chuỗi, điểm, chiều cao"),
            },
            "vla-addlightweightpolyline" | "vla-addpolyline" => {
                let xs: Vec<f64> = arg(a, 1).items().unwrap_or(&[]).iter().filter_map(V::num).collect();
                let step = if n == "vla-addpolyline" { 3 } else { 2 };
                let pts: Vec<[f64; 2]> = xs.chunks(step).filter_map(|c| Some([*c.first()?, *c.get(1)?])).collect();
                if pts.len() < 2 {
                    err("vla-AddLightWeightPolyline: cần ít nhất 2 điểm")
                } else {
                    self.space_add("pline", json!({ "points": pts }))
                }
            }
            _ => {
                // vla-get-Prop / vla-put-Prop.
                if let Some(p) = n.strip_prefix("vla-get-") {
                    match self.ename(arg(a, 0)) {
                        Ok(h) => self.prop_get(h, p),
                        Err(e) => Err(e),
                    }
                } else if let Some(p) = n.strip_prefix("vla-put-") {
                    match self.ename(arg(a, 0)) {
                        Ok(h) => {
                            let v = arg(a, 1).clone();
                            self.prop_put(h, p, &v)
                        }
                        Err(e) => Err(e),
                    }
                } else if n.starts_with("vla-") || n.starts_with("vlax-") {
                    err(format!("{} chưa được hỗ trợ trong VNCCad", name.to_ascii_uppercase()))
                } else {
                    return None;
                }
            }
        };
        Some(r)
    }
}
