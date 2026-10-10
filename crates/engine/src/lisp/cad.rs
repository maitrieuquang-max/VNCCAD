//! AutoLISP functions that talk to the drawing and the user.

use cadcraft_color::Color;
use cadcraft_doc::{Common, EntityKind, Handle};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde_json::{Value, json};

use super::{Ex, R, Run, V, err, to_text};
use crate::snap::Ucs2;
use crate::{Accept, Input, Prompt};

fn arg(a: &[V], i: usize) -> &V {
    a.get(i).unwrap_or(&V::Nil)
}

/// The prompt text a routine passed (`"\nPick a point: "` → `Pick a point`).
fn msg(v: &V, default: &str) -> String {
    match v {
        V::Str(s) => {
            let t = s.trim().trim_end_matches(':').trim_end();
            if t.is_empty() { default.to_string() } else { t.to_string() }
        }
        _ => default.to_string(),
    }
}

fn color_code(c: &Color) -> Option<i64> {
    match c {
        Color::ByLayer => None,
        Color::ByBlock => Some(0),
        Color::Index(i) => Some(i64::from(*i)),
        Color::True(_) => None,
    }
}

fn dotted(code: i64, v: V) -> V {
    V::Dotted(vec![V::Int(code)], Box::new(v))
}

fn p3(code: i64, p: Vec3) -> V {
    V::List(vec![V::Int(code), V::Real(p.x), V::Real(p.y), V::Real(p.z)])
}

/// The value of a DXF group in an entity list: `(code . v)` or `(code x y z)`.
fn group(list: &[V], code: i64) -> Option<V> {
    list.iter().find_map(|e| match e {
        V::Dotted(x, t) if x.len() == 1 && x.first() == Some(&V::Int(code)) => Some((**t).clone()),
        V::List(x) if x.first() == Some(&V::Int(code)) => Some(V::list(x.get(1..).unwrap_or(&[]).to_vec())),
        _ => None,
    })
}

fn group_pt(list: &[V], code: i64) -> Option<Vec3> {
    let v = group(list, code)?;
    let it = v.items()?;
    Some(Vec3::new(it.first()?.num()?, it.get(1)?.num()?, it.get(2).and_then(V::num).unwrap_or(0.0)))
}

fn group_num(list: &[V], code: i64) -> Option<f64> {
    group(list, code)?.num()
}

fn group_str(list: &[V], code: i64) -> Option<String> {
    match group(list, code)? {
        V::Str(s) => Some(s),
        _ => None,
    }
}

fn dxf_name(k: &EntityKind) -> &'static str {
    match k {
        EntityKind::Line(_) => "LINE",
        EntityKind::Circle(_) => "CIRCLE",
        EntityKind::Arc(_) => "ARC",
        EntityKind::LwPolyline(_) => "LWPOLYLINE",
        EntityKind::Text(_) => "TEXT",
        EntityKind::MText(_) => "MTEXT",
        EntityKind::Insert(_) => "INSERT",
        EntityKind::Point(_) => "POINT",
        EntityKind::Ellipse(_) => "ELLIPSE",
        EntityKind::Hatch(_) => "HATCH",
        EntityKind::Dimension(_) => "DIMENSION",
        EntityKind::Spline(_) => "SPLINE",
        EntityKind::Viewport(_) => "VIEWPORT",
        _ => "OTHER",
    }
}

/// `entget`: the entity as a DXF-style association list (world coordinates).
fn entget(d: &cadcraft_doc::Drawing, h: Handle) -> Option<V> {
    let e = d.entity(h)?;
    let mut l = vec![
        dotted(-1, V::Ename(h)),
        dotted(0, V::Str(dxf_name(&e.kind).into())),
        dotted(5, V::Str(h.hex())),
        dotted(410, V::Str(if d.model.get(h).is_some() { "Model".into() } else { "Layout".into() })),
        dotted(8, V::Str(e.common.layer.clone())),
    ];
    if let Some(c) = color_code(&e.common.color) {
        l.push(dotted(62, V::Int(c)));
    }
    if !e.common.linetype.eq_ignore_ascii_case("bylayer") && !e.common.linetype.is_empty() {
        l.push(dotted(6, V::Str(e.common.linetype.clone())));
    }
    match &e.kind {
        EntityKind::Line(x) => {
            l.push(p3(10, x.a));
            l.push(p3(11, x.b));
        }
        EntityKind::Circle(x) => {
            l.push(p3(10, x.center));
            l.push(dotted(40, V::Real(x.radius)));
        }
        EntityKind::Arc(x) => {
            l.push(p3(10, x.center));
            l.push(dotted(40, V::Real(x.radius)));
            l.push(dotted(50, V::Real(x.start)));
            l.push(dotted(51, V::Real(x.end)));
        }
        EntityKind::Point(x) => l.push(p3(10, x.p)),
        EntityKind::LwPolyline(x) => {
            l.push(dotted(90, V::Int(x.vertices.len() as i64)));
            l.push(dotted(70, V::Int(i64::from(x.closed))));
            l.push(dotted(43, V::Real(x.const_width)));
            l.push(dotted(38, V::Real(x.elevation)));
            for v in &x.vertices {
                l.push(V::List(vec![V::Int(10), V::Real(v.p.x), V::Real(v.p.y)]));
                l.push(dotted(42, V::Real(v.bulge)));
            }
        }
        EntityKind::Text(x) => {
            l.push(p3(10, x.insert));
            l.push(dotted(40, V::Real(x.height)));
            l.push(dotted(1, V::Str(x.value.clone())));
            l.push(dotted(50, V::Real(x.rotation)));
            l.push(dotted(41, V::Real(x.width_factor)));
            l.push(dotted(7, V::Str(x.style.clone())));
            if let Some(a) = x.align_pt {
                l.push(p3(11, a));
            }
        }
        EntityKind::MText(x) => {
            l.push(p3(10, x.insert));
            l.push(dotted(40, V::Real(x.height)));
            l.push(dotted(41, V::Real(x.width)));
            l.push(dotted(71, V::Int(i64::from(x.attach))));
            l.push(dotted(1, V::Str(x.contents.clone())));
            l.push(dotted(50, V::Real(x.rotation)));
            l.push(dotted(7, V::Str(x.style.clone())));
        }
        EntityKind::Insert(x) => {
            l.push(dotted(2, V::Str(x.block.clone())));
            l.push(p3(10, x.insert));
            l.push(dotted(41, V::Real(x.scale.x)));
            l.push(dotted(42, V::Real(x.scale.y)));
            l.push(dotted(43, V::Real(x.scale.z)));
            l.push(dotted(50, V::Real(x.rotation)));
            l.push(dotted(66, V::Int(i64::from(!x.attribs.is_empty()))));
        }
        _ => {}
    }
    Some(V::List(l))
}

/// Apply an `entmod` list to an entity (the groups VNCCad knows; others are ignored).
fn apply_groups(common: &mut Common, kind: &mut EntityKind, l: &[V]) {
    if let Some(layer) = group_str(l, 8) {
        common.layer = layer;
    }
    if let Some(V::Int(c)) = group(l, 62) {
        common.color = match c {
            0 => Color::ByBlock,
            1..=255 => Color::Index(c as u8),
            _ => Color::ByLayer,
        };
    }
    if let Some(lt) = group_str(l, 6) {
        common.linetype = lt;
    }
    match kind {
        EntityKind::Line(x) => {
            if let Some(p) = group_pt(l, 10) {
                x.a = p;
            }
            if let Some(p) = group_pt(l, 11) {
                x.b = p;
            }
        }
        EntityKind::Circle(x) => {
            if let Some(p) = group_pt(l, 10) {
                x.center = p;
            }
            if let Some(r) = group_num(l, 40).filter(|r| *r > 0.0) {
                x.radius = r;
            }
        }
        EntityKind::Arc(x) => {
            if let Some(p) = group_pt(l, 10) {
                x.center = p;
            }
            if let Some(r) = group_num(l, 40).filter(|r| *r > 0.0) {
                x.radius = r;
            }
            if let Some(a) = group_num(l, 50) {
                x.start = a;
            }
            if let Some(a) = group_num(l, 51) {
                x.end = a;
            }
        }
        EntityKind::Point(x) => {
            if let Some(p) = group_pt(l, 10) {
                x.p = p;
            }
        }
        EntityKind::LwPolyline(x) => {
            let pts: Vec<Vec2> = l
                .iter()
                .filter_map(|e| match e {
                    V::List(v) if v.first() == Some(&V::Int(10)) => Some(Vec2::new(v.get(1)?.num()?, v.get(2)?.num()?)),
                    _ => None,
                })
                .collect();
            let bulges: Vec<f64> = l
                .iter()
                .filter_map(|e| match e {
                    V::Dotted(v, t) if v.first() == Some(&V::Int(42)) => t.num(),
                    _ => None,
                })
                .collect();
            if !pts.is_empty() {
                x.vertices = pts.iter().enumerate().map(|(i, p)| PolyVertex::with_bulge(*p, bulges.get(i).copied().unwrap_or(0.0))).collect();
            }
            if let Some(V::Int(f)) = group(l, 70) {
                x.closed = f & 1 != 0;
            }
            if let Some(w) = group_num(l, 43) {
                x.const_width = w.max(0.0);
            }
        }
        EntityKind::Text(x) => {
            if let Some(p) = group_pt(l, 10) {
                x.insert = p;
            }
            if let Some(h) = group_num(l, 40).filter(|h| *h > 0.0) {
                x.height = h;
            }
            if let Some(t) = group_str(l, 1) {
                x.value = t;
            }
            if let Some(a) = group_num(l, 50) {
                x.rotation = a;
            }
            if let Some(w) = group_num(l, 41).filter(|w| *w > 0.0) {
                x.width_factor = w;
            }
            if let Some(st) = group_str(l, 7) {
                x.style = st;
            }
        }
        EntityKind::MText(x) => {
            if let Some(p) = group_pt(l, 10) {
                x.insert = p;
            }
            if let Some(h) = group_num(l, 40).filter(|h| *h > 0.0) {
                x.height = h;
            }
            if let Some(t) = group_str(l, 1) {
                x.contents = t;
            }
            if let Some(a) = group_num(l, 50) {
                x.rotation = a;
            }
        }
        EntityKind::Insert(x) => {
            if let Some(p) = group_pt(l, 10) {
                x.insert = p;
            }
            if let Some(a) = group_num(l, 50) {
                x.rotation = a;
            }
            for (code, slot) in [(41, 0), (42, 1), (43, 2)] {
                if let Some(v) = group_num(l, code).filter(|v| *v != 0.0) {
                    match slot {
                        0 => x.scale.x = v,
                        1 => x.scale.y = v,
                        _ => x.scale.z = v,
                    }
                }
            }
        }
        _ => {}
    }
}

/// `ssget` filter: `((0 . "LINE,ARC") (8 . "TIM*") (62 . 1))`.
fn passes(d: &cadcraft_doc::Drawing, h: Handle, filter: &[V]) -> bool {
    let Some(e) = d.entity(h) else { return false };
    filter.iter().all(|f| match f {
        V::Dotted(x, t) => match (x.first(), &**t) {
            (Some(V::Int(0)), V::Str(p)) => super::builtins::wcmatch(dxf_name(&e.kind), p),
            (Some(V::Int(8)), V::Str(p)) => super::builtins::wcmatch(&e.common.layer, p),
            (Some(V::Int(2)), V::Str(p)) => matches!(&e.kind, EntityKind::Insert(i) if super::builtins::wcmatch(&i.block, p)),
            (Some(V::Int(62)), V::Int(c)) => color_code(&e.common.color).unwrap_or(256) == *c,
            (Some(V::Int(6)), V::Str(p)) => super::builtins::wcmatch(&e.common.linetype, p),
            (Some(V::Int(1)), V::Str(p)) => match &e.kind {
                EntityKind::Text(t) => super::builtins::wcmatch(&t.value, p),
                EntityKind::MText(t) => super::builtins::wcmatch(&t.contents, p),
                _ => false,
            },
            // Unknown groups and -4 operators: not filtered.
            _ => true,
        },
        _ => true,
    })
}

impl Run<'_> {
    fn ucs(&self) -> Ucs2 {
        self.s.ucs()
    }
    /// A LISP point (UCS) → world.
    pub(super) fn to_world(&self, v: &V) -> Option<Vec2> {
        v.point().map(|p| self.ucs().to_world(p))
    }
    pub(super) fn ucs_pt(&self, w: Vec2) -> V {
        let p = self.ucs().to_ucs(w);
        V::pt3(p.x, p.y, 0.0)
    }
    fn new_set(&mut self, hs: Vec<Handle>) -> V {
        if hs.is_empty() {
            return V::Nil;
        }
        self.lisp.next_set += 1;
        let id = self.lisp.next_set;
        self.lisp.sets.insert(id, hs);
        V::Ss(id)
    }

    /// Ask for a value. Keywords from `initget` are offered; bit 1 refuses a plain Enter.
    pub(super) fn ask(&mut self, prompt: Prompt) -> R<Input> {
        let (bits, kws) = self.initget();
        let refs: Vec<&str> = kws.iter().map(String::as_str).collect();
        let prompt = if refs.is_empty() { prompt } else { prompt.kw(&refs) };
        loop {
            let i = self.next_input(prompt.clone())?;
            if matches!(i, Input::Enter) && bits & 1 != 0 {
                continue;
            }
            return Ok(i);
        }
    }

    fn keyword_or(&self, i: &Input) -> Option<V> {
        match i {
            Input::Keyword(k) => Some(V::Str(k.clone())),
            _ => None,
        }
    }

    pub(super) fn cad(&mut self, name: &str, a: &[V]) -> Option<R<V>> {
        let r: R<V> = match name {
            "command" | "vl-cmdf" | "command-s" => self.command(a),
            "initget" => {
                let (bits, kw) = match (arg(a, 0), arg(a, 1)) {
                    (V::Int(b), V::Str(k)) => (*b, k.clone()),
                    (V::Int(b), _) => (*b, String::new()),
                    (V::Str(k), _) => (0, k.clone()),
                    _ => (0, String::new()),
                };
                let kws: Vec<String> =
                    kw.split_whitespace().map(|k| k.split('_').next().unwrap_or(k).to_string()).filter(|k| !k.is_empty()).collect();
                self.set_initget(bits, kws);
                Ok(V::Nil)
            }
            "getpoint" | "getcorner" => {
                let (base, m) = match (arg(a, 0), arg(a, 1)) {
                    (b, m) if b.point().is_some() => (self.to_world(b), m.clone()),
                    (m, _) => (None, m.clone()),
                };
                let p = Prompt::new(msg(&m, "Chọn điểm"), Accept::POINT).base_opt(base);
                let i = match self.ask(p) {
                    Ok(i) => i,
                    Err(e) => return Some(Err(e)),
                };
                Ok(match i {
                    Input::Point(w) => self.ucs_pt(w),
                    other => self.keyword_or(&other).unwrap_or(V::Nil),
                })
            }
            "getreal" | "getint" | "getdist" | "getangle" | "getorient" => {
                let (base, m) = match (arg(a, 0), arg(a, 1)) {
                    (b, m) if b.point().is_some() => (self.to_world(b), m.clone()),
                    (m, _) => (None, m.clone()),
                };
                let geometric = matches!(name, "getdist" | "getangle" | "getorient");
                let accept = if geometric { Accept::POINT_OR_NUMBER } else { Accept::NUMBER };
                let default = match name {
                    "getint" => "Nhập số nguyên",
                    "getdist" => "Nhập khoảng cách",
                    "getangle" | "getorient" => "Nhập góc",
                    _ => "Nhập số",
                };
                let prompt = Prompt::new(msg(&m, default), accept).base_opt(base);
                let mut first: Option<Vec2> = base;
                loop {
                    let i = match self.ask(prompt.clone()) {
                        Ok(i) => i,
                        Err(e) => return Some(Err(e)),
                    };
                    match i {
                        Input::Enter => return Some(Ok(V::Nil)),
                        Input::Keyword(k) => return Some(Ok(V::Str(k))),
                        Input::Text(t) => {
                            let t = t.trim();
                            let v = match name {
                                "getint" => t.parse::<i64>().ok().map(V::Int),
                                "getangle" | "getorient" => crate::units::parse_angle(t).map(V::Real),
                                "getdist" => crate::units::parse_distance(t).map(V::Real),
                                _ => t.parse::<f64>().ok().or_else(|| crate::units::parse_distance(t)).map(V::Real),
                            };
                            match v {
                                Some(v) => return Some(Ok(v)),
                                None => self.s.echo("Cần nhập một số."),
                            }
                        }
                        Input::Point(p) if geometric => match first {
                            Some(b) => {
                                return Some(Ok(V::Real(if name == "getdist" {
                                    b.dist(p)
                                } else {
                                    cadcraft_geom::norm_angle((p - b).angle() - self.ucs().angle)
                                })));
                            }
                            None => first = Some(p),
                        },
                        _ => {}
                    }
                }
            }
            "getstring" => {
                let (m, _cr) = match (arg(a, 0), arg(a, 1)) {
                    (V::Str(_), _) => (arg(a, 0).clone(), false),
                    (flag, m) => (m.clone(), flag.truthy()),
                };
                let _ = self.initget();
                match self.next_input(Prompt::new(msg(&m, "Nhập chuỗi"), Accept::TEXT)) {
                    Ok(Input::Text(t)) | Ok(Input::Keyword(t)) => Ok(V::Str(t)),
                    Ok(_) => Ok(V::Str(String::new())),
                    Err(e) => Err(e),
                }
            }
            "getkword" => {
                let m = arg(a, 0).clone();
                let (bits, kws) = self.initget();
                let refs: Vec<&str> = kws.iter().map(String::as_str).collect();
                let prompt = Prompt::new(msg(&m, "Chọn"), Accept::TEXT).kw(&refs);
                loop {
                    match self.next_input(prompt.clone()) {
                        Ok(Input::Keyword(k)) => return Some(Ok(V::Str(k))),
                        Ok(Input::Enter) if bits & 1 == 0 => return Some(Ok(V::Nil)),
                        Ok(Input::Text(t)) => {
                            if let Some(k) = prompt.match_keyword(&t) {
                                return Some(Ok(V::Str(k)));
                            }
                            self.s.echo("Lựa chọn không hợp lệ.");
                        }
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            "entsel" | "nentsel" => {
                let p = Prompt::new(msg(arg(a, 0), "Chọn đối tượng"), Accept::SELECT);
                loop {
                    match self.ask(p.clone()) {
                        Ok(Input::Pick(hs)) => match hs.first() {
                            Some(h) => {
                                let at = self.s.cursor;
                                return Some(Ok(V::List(vec![V::Ename(*h), self.ucs_pt(at)])));
                            }
                            None => self.s.echo("Không chọn được đối tượng nào."),
                        },
                        Ok(Input::Enter) => return Some(Ok(V::Nil)),
                        Ok(Input::Keyword(k)) => return Some(Ok(V::Str(k))),
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            "ssget" => self.ssget(a),
            "sslength" => match arg(a, 0) {
                V::Ss(id) => Ok(V::Int(self.lisp.sets.get(id).map_or(0, Vec::len) as i64)),
                _ => err("sslength: cần bộ chọn"),
            },
            "ssname" => match (arg(a, 0), arg(a, 1)) {
                (V::Ss(id), V::Int(i)) => {
                    Ok(self.lisp.sets.get(id).and_then(|v| usize::try_from(*i).ok().and_then(|i| v.get(i))).map_or(V::Nil, |h| V::Ename(*h)))
                }
                _ => err("ssname: đối số sai"),
            },
            "ssadd" => match (arg(a, 0), arg(a, 1)) {
                (V::Nil, _) => Ok(self.new_set_empty()),
                (V::Ename(h), V::Nil) => Ok(self.new_set(vec![*h])),
                (V::Ename(h), V::Ss(id)) => {
                    let set = self.lisp.sets.entry(*id).or_default();
                    if !set.contains(h) {
                        set.push(*h);
                    }
                    Ok(V::Ss(*id))
                }
                _ => err("ssadd: đối số sai"),
            },
            "ssdel" => match (arg(a, 0), arg(a, 1)) {
                (V::Ename(h), V::Ss(id)) => {
                    let set = self.lisp.sets.entry(*id).or_default();
                    let n = set.len();
                    set.retain(|x| x != h);
                    Ok(if set.len() < n { V::Ss(*id) } else { V::Nil })
                }
                _ => err("ssdel: đối số sai"),
            },
            "ssmemb" => match (arg(a, 0), arg(a, 1)) {
                (V::Ename(h), V::Ss(id)) => Ok(if self.lisp.sets.get(id).is_some_and(|v| v.contains(h)) { V::Ename(*h) } else { V::Nil }),
                _ => Ok(V::Nil),
            },
            "entget" => match arg(a, 0) {
                V::Ename(h) => Ok(self.pseudo_entget(*h).or_else(|| self.s.doc().ok().and_then(|d| entget(d, *h))).unwrap_or(V::Nil)),
                _ => err("entget: cần tên đối tượng"),
            },
            "entmod" => self.entmod(arg(a, 0)),
            "entmake" => self.entmake(arg(a, 0)),
            "entmakex" => match self.entmake(arg(a, 0)) {
                Ok(V::Nil) => Ok(V::Nil),
                Ok(_) => {
                    let space = self.s.space();
                    Ok(self.s.doc().ok().and_then(|d| d.space(&space)?.last().map(|e| V::Ename(e.handle))).unwrap_or(V::Nil))
                }
                Err(e) => Err(e),
            },
            "entdel" => match arg(a, 0) {
                V::Ename(h) => {
                    let h = *h;
                    match self.s.doc_mut() {
                        Ok(d) => {
                            let removed = d.remove_entity(h).is_some();
                            self.s.touch();
                            Ok(if removed { V::Ename(h) } else { V::Nil })
                        }
                        Err(e) => err(e.to_string()),
                    }
                }
                _ => err("entdel: cần tên đối tượng"),
            },
            "entupd" => Ok(arg(a, 0).clone()),
            "entlast" => {
                let space = self.s.space();
                Ok(self.s.doc().ok().and_then(|d| d.space(&space)?.last().map(|e| V::Ename(e.handle))).unwrap_or(V::Nil))
            }
            "entnext" => {
                let space = self.s.space();
                let d = match self.s.doc() {
                    Ok(d) => d,
                    Err(e) => return Some(err(e.to_string())),
                };
                let Some(store) = d.space(&space) else { return Some(Ok(V::Nil)) };
                let hs: Vec<Handle> = store.iter().map(|e| e.handle).collect();
                Ok(match arg(a, 0) {
                    V::Ename(h) => hs.iter().position(|x| x == h).and_then(|i| hs.get(i + 1)).map_or(V::Nil, |h| V::Ename(*h)),
                    _ => hs.first().map_or(V::Nil, |h| V::Ename(*h)),
                })
            }
            "handent" => match arg(a, 0) {
                V::Str(s) => Ok(Handle::parse_hex(s).filter(|h| self.s.doc().is_ok_and(|d| d.entity(*h).is_some())).map_or(V::Nil, V::Ename)),
                _ => Ok(V::Nil),
            },
            "getvar" => match arg(a, 0) {
                V::Str(n) => Ok(self.getvar(n)),
                _ => err("getvar: cần tên biến"),
            },
            "setvar" => match arg(a, 0) {
                V::Str(n) => {
                    let n = n.clone();
                    let v = arg(a, 1).clone();
                    let j = match &v {
                        V::Int(i) => json!(i),
                        V::Real(r) => json!(r),
                        V::Str(s) => json!(s),
                        V::List(_) => match v.point() {
                            Some(p) => json!([p.x, p.y]),
                            None => Value::Null,
                        },
                        _ => Value::Null,
                    };
                    match crate::sysvars::set(self.s, &n, &j) {
                        Ok(()) => Ok(v),
                        Err(_) => {
                            // Unknown variables are kept in the drawing header.
                            if let (Ok(d), Some(h)) = (self.s.doc_mut(), hval(&v)) {
                                d.header.set(&n.to_ascii_uppercase(), h);
                            }
                            Ok(v)
                        }
                    }
                }
                _ => err("setvar: cần tên biến"),
            },
            "tblsearch" => match (arg(a, 0), arg(a, 1)) {
                (V::Str(t), V::Str(n)) => Ok(self.tblsearch(t, n)),
                _ => err("tblsearch: cần tên bảng và tên"),
            },
            "trans" => match (arg(a, 0).point(), arg(a, 1), arg(a, 2)) {
                (Some(p), from, to) => {
                    let ucs = self.ucs();
                    let disp = arg(a, 3).truthy();
                    let code = |v: &V| match v {
                        V::Int(c) => *c,
                        _ => 0,
                    };
                    let world = match code(from) {
                        1 | 2 => {
                            if disp {
                                ucs.dir_to_world(p)
                            } else {
                                ucs.to_world(p)
                            }
                        }
                        _ => p,
                    };
                    let out = match code(to) {
                        1 | 2 => {
                            if disp {
                                world.rotate(-ucs.angle)
                            } else {
                                ucs.to_ucs(world)
                            }
                        }
                        _ => world,
                    };
                    Ok(V::pt3(out.x, out.y, 0.0))
                }
                _ => err("trans: cần một điểm"),
            },
            "osnap" => match (self.to_world(arg(a, 0)), arg(a, 1)) {
                (Some(p), V::Str(modes)) => {
                    use crate::snap::mode::*;
                    let mut m = 0;
                    for k in modes.split(',') {
                        let k = k.trim().trim_start_matches('_').to_ascii_lowercase();
                        m |= match k.get(..3.min(k.len())).unwrap_or("") {
                            "end" => END,
                            "mid" => MID,
                            "cen" => CEN,
                            "nod" => NOD,
                            "qua" => QUA,
                            "int" => INT,
                            "ins" => INS,
                            "per" => PER,
                            "tan" => TAN,
                            "nea" => NEA,
                            _ => 0,
                        };
                    }
                    let space = self.s.space();
                    let ap = self.s.pixel_size() * self.s.settings.aperture.max(1.0);
                    let hit = self.s.doc().ok().and_then(|d| crate::snap::osnap(d, &space, p, ap, m, None));
                    Ok(hit.map_or(V::Nil, |h| self.ucs_pt(h.point)))
                }
                _ => Ok(V::Nil),
            },
            "load" => self.load(a),
            "ver" => Ok(V::Str("Visual LISP 2.0 (VNCCad)".into())),
            "acad_strlsort" => {
                let mut v: Vec<String> =
                    arg(a, 0).items().unwrap_or(&[]).iter().filter_map(|x| if let V::Str(s) = x { Some(s.clone()) } else { None }).collect();
                v.sort();
                Ok(V::list(v.into_iter().map(V::Str).collect()))
            }
            "acad_colordlg" => Ok(arg(a, 0).clone()),
            _ => return None,
        };
        Some(r)
    }

    fn new_set_empty(&mut self) -> V {
        self.lisp.next_set += 1;
        let id = self.lisp.next_set;
        self.lisp.sets.insert(id, Vec::new());
        V::Ss(id)
    }

    fn getvar(&self, n: &str) -> V {
        let up = n.to_ascii_uppercase();
        match up.as_str() {
            "CLAYER" | "TEXTSTYLE" | "DIMSTYLE" | "CELTYPE" => {
                return self.s.doc().map(|d| V::Str(d.header.str(&up, if up == "CLAYER" { "0" } else { "Standard" }))).unwrap_or(V::Nil);
            }
            "ACADVER" => return V::Str("24.0s (VNCCad)".into()),
            "DWGPREFIX" => return V::Str(String::new()),
            "CMDECHO" | "BLIPMODE" | "HIGHLIGHT" => return V::Int(if up == "HIGHLIGHT" { 1 } else { 0 }),
            "VIEWCTR" => {
                let c = self.s.state().map(|s| s.view().center).unwrap_or(Vec2::ZERO);
                return self.ucs_pt(c);
            }
            "VIEWSIZE" => return V::Real(self.s.state().map(|s| s.view().height).unwrap_or(1.0)),
            _ => {}
        }
        match crate::sysvars::get(self.s, &up) {
            Some(Value::Number(n)) => n.as_i64().map(V::Int).or_else(|| n.as_f64().map(V::Real)).unwrap_or(V::Nil),
            Some(Value::String(s)) => V::Str(s),
            Some(Value::Array(a)) => {
                let xs: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
                match xs.as_slice() {
                    [x, y] => V::pt3(*x, *y, 0.0),
                    [x, y, z] => V::pt3(*x, *y, *z),
                    _ => V::Nil,
                }
            }
            Some(Value::Bool(b)) => V::Int(i64::from(b)),
            _ => match self.s.doc().ok().and_then(|d| d.header.get(&up).cloned()) {
                Some(cadcraft_doc::HVal::Int(i)) => V::Int(i),
                Some(cadcraft_doc::HVal::Real(r)) => V::Real(r),
                Some(cadcraft_doc::HVal::Str(s)) => V::Str(s),
                Some(cadcraft_doc::HVal::Point(p)) => V::pt3(p.x, p.y, p.z),
                _ => V::Nil,
            },
        }
    }

    pub(super) fn tblsearch(&self, table: &str, name: &str) -> V {
        let Ok(d) = self.s.doc() else { return V::Nil };
        match table.to_ascii_uppercase().as_str() {
            "LAYER" => d.layers.iter().find(|l| l.name.eq_ignore_ascii_case(name)).map_or(V::Nil, |l| {
                let mut flags = 0;
                if l.frozen {
                    flags |= 1;
                }
                if l.locked {
                    flags |= 4;
                }
                let aci = color_code(&l.color).unwrap_or(7);
                V::List(vec![
                    dotted(0, V::Str("LAYER".into())),
                    dotted(2, V::Str(l.name.clone())),
                    dotted(70, V::Int(flags)),
                    dotted(62, V::Int(if l.on { aci } else { -aci })),
                    dotted(6, V::Str(l.linetype.clone())),
                ])
            }),
            "BLOCK" => d
                .blocks
                .keys()
                .find(|k| k.eq_ignore_ascii_case(name))
                .map_or(V::Nil, |k| V::List(vec![dotted(0, V::Str("BLOCK".into())), dotted(2, V::Str(k.clone())), dotted(70, V::Int(0))])),
            "STYLE" => d.text_styles.iter().find(|s| s.name.eq_ignore_ascii_case(name)).map_or(V::Nil, |s| {
                V::List(vec![
                    dotted(0, V::Str("STYLE".into())),
                    dotted(2, V::Str(s.name.clone())),
                    dotted(40, V::Real(s.height)),
                    dotted(3, V::Str(s.font.clone())),
                ])
            }),
            "LTYPE" => d
                .linetypes
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(name))
                .map_or(V::Nil, |l| V::List(vec![dotted(0, V::Str("LTYPE".into())), dotted(2, V::Str(l.name.clone()))])),
            "DIMSTYLE" => d
                .dim_styles
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
                .map_or(V::Nil, |s| V::List(vec![dotted(0, V::Str("DIMSTYLE".into())), dotted(2, V::Str(s.name.clone()))])),
            _ => V::Nil,
        }
    }

    fn entmod(&mut self, l: &V) -> R<V> {
        let Some(items) = l.items() else { return err("entmod: cần danh sách") };
        let Some(V::Ename(h)) = group(items, -1) else { return err("entmod: thiếu (-1 . tên đối tượng)") };
        let items = items.to_vec();
        let d = self.s.doc_mut().map_err(|e| Ex::Error(e.to_string()))?;
        let mut ok = false;
        let r = d.modify_entity(h, |e| {
            let mut common = e.common.clone();
            let mut kind = e.kind.clone();
            apply_groups(&mut common, &mut kind, &items);
            e.common = common;
            e.kind = kind;
            ok = true;
        });
        if let Some(layer) = group_str(&items, 8) {
            d.ensure_layer(&layer);
        }
        self.s.touch();
        Ok(if r.is_ok() && ok { l.clone() } else { V::Nil })
    }

    fn entmake(&mut self, l: &V) -> R<V> {
        let Some(items) = l.items() else { return err("entmake: cần danh sách") };
        let kind_name = group_str(items, 0).unwrap_or_default().to_ascii_uppercase();
        let z = Vec3::ZERO;
        let mut kind = match kind_name.as_str() {
            "LINE" => EntityKind::Line(cadcraft_doc::Line { a: z, b: z }),
            "CIRCLE" => EntityKind::Circle(cadcraft_doc::Circle { center: z, radius: 1.0 }),
            "ARC" => EntityKind::Arc(cadcraft_doc::Arc { center: z, radius: 1.0, start: 0.0, end: std::f64::consts::PI }),
            "POINT" => EntityKind::Point(cadcraft_doc::Point { p: z, angle: 0.0 }),
            "LWPOLYLINE" => EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
                vertices: Vec::new(),
                closed: false,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
            "TEXT" | "MTEXT" => {
                // Through the TEXT/MTEXT commands so every field gets its default.
                let at = group_pt(items, 10).unwrap_or(z);
                let height = group_num(items, 40).filter(|h| *h > 0.0).unwrap_or(2.5);
                let text = group_str(items, 1).unwrap_or_default();
                let rot = group_num(items, 50).unwrap_or(0.0).to_degrees();
                let cmd = if kind_name == "TEXT" { "text" } else { "mtext" };
                let p = if cmd == "text" {
                    json!({ "at": [at.x, at.y], "text": text, "height": height, "rotation": rot })
                } else {
                    json!({ "at": [at.x, at.y], "text": text, "height": height, "width": group_num(items, 41).unwrap_or(0.0) })
                };
                self.s.execute(cmd, &p).map_err(|e| Ex::Error(e.to_string()))?;
                let space = self.s.space();
                let h = self.s.doc().ok().and_then(|d| d.space(&space)?.last().map(|e| e.handle));
                if let Some(h) = h {
                    let mut with = items.to_vec();
                    with.push(dotted(-1, V::Ename(h)));
                    self.entmod(&V::List(with))?;
                }
                return Ok(l.clone());
            }
            "INSERT" => {
                let name = group_str(items, 2).unwrap_or_default();
                let at = group_pt(items, 10).unwrap_or(z);
                let p = json!({ "name": name, "at": [at.x, at.y], "rotation": group_num(items, 50).unwrap_or(0.0).to_degrees(),
                    "scale": group_num(items, 41).unwrap_or(1.0) });
                self.s.execute("insert", &p).map_err(|e| Ex::Error(e.to_string()))?;
                return Ok(l.clone());
            }
            other => return err(format!("entmake: chưa hỗ trợ loại {other}")),
        };
        let mut common = Common { layer: self.s.doc().map(|d| d.header.str("CLAYER", "0")).unwrap_or_else(|_| "0".into()), ..Common::default() };
        apply_groups(&mut common, &mut kind, items);
        let space = self.s.space();
        self.s.doc_mut().map_err(|e| Ex::Error(e.to_string()))?.add(&space, common, kind).map_err(|e| Ex::Error(e.to_string()))?;
        self.s.touch();
        Ok(l.clone())
    }

    fn ssget(&mut self, a: &[V]) -> R<V> {
        let space = self.s.space();
        let mode = match arg(a, 0) {
            V::Str(m) => Some(m.trim_start_matches('_').to_ascii_uppercase()),
            _ => None,
        };
        let filter: Vec<V> = a
            .iter()
            .rev()
            .find_map(|v| match v {
                V::List(x) if x.iter().all(|e| matches!(e, V::Dotted(..))) => Some(x.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let d = self.s.doc().map_err(|e| Ex::Error(e.to_string()))?;
        let all: Vec<Handle> = d.space(&space).map(|s| s.iter().filter(|e| d.is_visible(e)).map(|e| e.handle).collect()).unwrap_or_default();
        let picked: Vec<Handle> = match mode.as_deref() {
            Some("X") => all,
            Some("P") => self.s.state().map(|st| st.previous_selection.clone()).unwrap_or_default(),
            Some("L") => all.last().copied().into_iter().collect(),
            Some("I") => self.s.selection(),
            Some("W") | Some("C") => {
                let (Some(p1), Some(p2)) = (self.to_world(arg(a, 1)), self.to_world(arg(a, 2))) else { return err("ssget W/C: cần 2 điểm") };
                crate::select::select_window(d, &space, cadcraft_geom::Bounds2::new(p1, p2), mode.as_deref() == Some("C"))
            }
            _ => {
                // Interactive: "Select objects:" until Enter (the pickfirst selection is used at once).
                let pre = self.s.selection();
                if !pre.is_empty() && self.inputs.is_empty() && self.s.settings.pickfirst {
                    pre
                } else {
                    let mut hs: Vec<Handle> = Vec::new();
                    let prompt = Prompt::new("Chọn đối tượng", Accept::SELECT);
                    loop {
                        match self.next_input(prompt.clone())? {
                            Input::Pick(p) => {
                                for h in p {
                                    if !hs.contains(&h) {
                                        hs.push(h);
                                    }
                                }
                            }
                            Input::Enter | Input::Cancel => break,
                            _ => {}
                        }
                    }
                    hs
                }
            }
        };
        let d = self.s.doc().map_err(|e| Ex::Error(e.to_string()))?;
        let hs: Vec<Handle> = picked.into_iter().filter(|h| passes(d, *h, &filter)).collect();
        Ok(self.new_set(hs))
    }

    /// `(command …)`: start a command and feed it the arguments, as if typed. Points are UCS;
    /// `pause` (or "\\") waits for the user.
    fn command(&mut self, a: &[V]) -> R<V> {
        let ucs = self.ucs();
        for v in a {
            let flat: Vec<V> = match v {
                V::List(x) if v.point().is_none() => x.clone(),
                other => vec![other.clone()],
            };
            for v in flat {
                if self.s.running.is_none() {
                    match &v {
                        V::Str(n) if n.is_empty() => {
                            // Enter with no command: repeat nothing.
                        }
                        V::Str(n) => {
                            let n = n.trim().trim_start_matches(['_', '.', '-']).to_string();
                            if self.lisp.has_command(&n) {
                                return err(format!("không gọi lệnh LISP {n} bằng (command) được; dùng (c:{n})"));
                            }
                            if let Err(e) = self.s.start(&n) {
                                return err(format!("lệnh không hợp lệ: {n} ({e})"));
                            }
                        }
                        V::Nil => {}
                        other => return err(format!("(command): cần tên lệnh, nhận {}", to_text(other, true, &self.lisp.sets))),
                    }
                    continue;
                }
                let r = match &v {
                    V::Str(t) if t == "\\" => {
                        let prompt = self.s.current_prompt().unwrap_or_else(|| Prompt::new("", Accept::POINT));
                        let i = self.next_input(prompt)?;
                        self.s.input(i)
                    }
                    V::Str(t) if t.is_empty() => self.s.input(Input::Enter),
                    V::Str(t) => self.s.cmdline(t),
                    V::Int(n) => self.s.cmdline(&n.to_string()),
                    V::Real(r) => self.s.cmdline(&format!("{r}")),
                    V::List(_) => match v.point() {
                        Some(p) => self.s.input(Input::Point(ucs.to_world(p))),
                        None => Ok(()),
                    },
                    V::Ename(h) => self.s.input(Input::Pick(vec![*h])),
                    V::Ss(id) => {
                        let hs = self.lisp.sets.get(id).cloned().unwrap_or_default();
                        self.s.input(Input::Pick(hs))
                    }
                    V::T => Ok(()),
                    V::Nil => self.s.input(Input::Enter),
                    _ => Ok(()),
                };
                if let Err(e) = r {
                    return err(e.to_string());
                }
            }
        }
        // TEXT called from LISP takes one line (AutoCAD); the interactive one goes on like DTEXT.
        if self.s.running.as_ref().is_some_and(|r| matches!(r.id.as_str(), "text" | "dtext"))
            && self.s.current_prompt().is_some_and(|p| p.message.to_ascii_lowercase().contains("enter text"))
            && self.s.state().is_ok_and(|st| {
                !std::sync::Arc::ptr_eq(&st.doc, &self.s.running.as_ref().map(|r| r.before.clone()).unwrap_or_else(|| st.doc.clone()))
            })
        {
            let _ = self.s.input(Input::Enter);
        }
        Ok(V::Nil)
    }

    fn load(&mut self, a: &[V]) -> R<V> {
        let V::Str(f) = arg(a, 0) else { return err("load: cần tên tệp") };
        let key = f.to_ascii_lowercase();
        let key2 = if key.ends_with(".lsp") { key.clone() } else { format!("{key}.lsp") };
        let base = key2.rsplit(['/', '\\']).next().unwrap_or(&key2).to_string();
        let src = self.lisp.files.get(&key2).or_else(|| self.lisp.files.get(&base)).cloned();
        #[cfg(not(target_arch = "wasm32"))]
        let src = src.or_else(|| std::fs::read(f).ok().or_else(|| std::fs::read(&key2).ok()).map(|b| super::machine::decode_source(&b)));
        match src {
            Some(text) => {
                let forms = super::read_all(&text).map_err(Ex::Error)?;
                self.eval_all(&forms)
            }
            None => match a.get(1) {
                Some(v) => Ok(v.clone()),
                None => err(format!("không tìm thấy tệp LISP {f} (nạp bằng APPLOAD hoặc kéo-thả)")),
            },
        }
    }
}

fn hval(v: &V) -> Option<cadcraft_doc::HVal> {
    Some(match v {
        V::Int(i) => cadcraft_doc::HVal::Int(*i),
        V::Real(r) => cadcraft_doc::HVal::Real(*r),
        V::Str(s) => cadcraft_doc::HVal::Str(s.clone()),
        _ => {
            let p = v.point()?;
            cadcraft_doc::HVal::Point(p.to3(0.0))
        }
    })
}
