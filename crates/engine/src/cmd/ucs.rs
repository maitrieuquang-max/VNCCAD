//! VNCCad: user coordinate systems (UCS) in the drawing plane, and PLAN.
//!
//! A UCS moves the origin and turns the X axis. Typed coordinates (`x,y`, `@dx,dy`, `@d<a`),
//! ortho, polar tracking, grid and snap follow it; `*x,y` is still a world point. PLAN turns the
//! view so the UCS X axis is horizontal on screen — drawing along a road alignment or a skewed
//! bridge axis. The UCS is saved in the drawing ($UCSORG/$UCSXDIR), named UCSs in the UCS table.

use cadcraft_doc::{EntityKind, HVal, Ucs};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::*;
use crate::snap::Ucs2;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Run a UCS option from the interactive command and print its message.
fn ucs_echo(s: &mut Session, p: &Value) -> Result<()> {
    let r = run_ucs(s, p)?;
    super::group::echo_msg(s, &r);
    Ok(())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ucs", "UCS", run_ucs)
            .menu(&["Tools", "New UCS"])
            .alias(&["hetruc"])
            .params(
                "{world: true} | {origin?: [x,y] (world), xpoint?: [x,y] | angle?: deg (absolute) | rotate?: deg (about Z)} | {object: hex} | {previous: true} | {save|restore|delete: name} | {} → current UCS",
            )
            .interactive(|_| Ok(Box::new(UcsM::default()))),
        CommandSpec::new("ucs.list", "Named UCS", run_ucs_list).menu(&["Tools", "Named UCS"]).alias(&["ucsman"]).enabled(has_doc).noundo(),
        CommandSpec::new("plan", "Plan View", run_plan)
            .menu(&["View", "Plan View"])
            .params("{world?: bool} → turn the view so the current UCS (or the world) X axis is horizontal, zoom extents")
            .noundo(),
    ]
}

fn deg(v: f64) -> f64 {
    v.to_degrees()
}

/// Make `u` the drawing's UCS (remembering the old one for UCS Previous).
fn set_ucs(s: &mut Session, u: Ucs2, name: &str) -> Result<Value> {
    if !(u.origin.is_finite() && u.angle.is_finite()) {
        return Err(bad("ucs", "UCS không hợp lệ"));
    }
    let old = s.ucs();
    if let Ok(st) = s.state_mut() {
        st.ucs_history.push(old);
        if st.ucs_history.len() > 10 {
            st.ucs_history.remove(0);
        }
    }
    store(s, u, name)
}

fn store(s: &mut Session, u: Ucs2, name: &str) -> Result<Value> {
    let x = Vec2::from_angle(u.angle);
    let d = s.doc_mut()?;
    d.header.set("UCSORG", HVal::Point(u.origin.to3(0.0)));
    d.header.set("UCSXDIR", HVal::Point(x.to3(0.0)));
    d.header.set("UCSYDIR", HVal::Point(x.perp().to3(0.0)));
    d.header.set("UCSNAME", HVal::Str(name.to_string()));
    s.touch();
    Ok(describe(&u, name))
}

fn describe(u: &Ucs2, name: &str) -> Value {
    let msg = if u.is_world() {
        "UCS: World (WCS).".to_string()
    } else {
        format!(
            "UCS{}: gốc ({:.3}, {:.3}), trục X {:.4}°.",
            if name.is_empty() { String::new() } else { format!(" {name}") },
            u.origin.x,
            u.origin.y,
            deg(cadcraft_geom::norm_angle(u.angle))
        )
    };
    json!({ "origin": [u.origin.x, u.origin.y], "angle": deg(u.angle), "world": u.is_world(), "name": name, "message": msg })
}

/// UCS aligned with an object: lines and polyline segments give origin and X direction; arcs and
/// circles their centre; text and blocks their insertion point and rotation.
fn from_object(s: &Session, h: cadcraft_doc::Handle, near: Option<Vec2>) -> Result<Ucs2> {
    let e = s.doc()?.entity(h).ok_or_else(|| bad("ucs", "không thấy đối tượng"))?;
    let u = match &e.kind {
        EntityKind::Line(l) => Ucs2 { origin: l.a.xy(), angle: (l.b.xy() - l.a.xy()).angle() },
        EntityKind::LwPolyline(p) => {
            let n = p.vertices.len();
            let segs: Vec<(Vec2, Vec2)> = (0..n.saturating_sub(1))
                .chain(if p.closed && n > 2 { Some(n - 1) } else { None })
                .filter_map(|i| Some((p.vertices.get(i)?.p, p.vertices.get((i + 1) % n)?.p)))
                .collect();
            let (a, b) = match near {
                Some(q) => segs.iter().copied().min_by(|x, y| seg_dist(q, *x).total_cmp(&seg_dist(q, *y))),
                None => segs.first().copied(),
            }
            .ok_or_else(|| bad("ucs", "đa tuyến không có đoạn nào"))?;
            Ucs2 { origin: a, angle: (b - a).angle() }
        }
        EntityKind::Arc(a) => Ucs2 { origin: a.center.xy(), angle: a.start },
        EntityKind::Circle(c) => Ucs2 { origin: c.center.xy(), angle: near.map_or(0.0, |q| (q - c.center.xy()).angle()) },
        EntityKind::Text(t) => Ucs2 { origin: t.insert.xy(), angle: t.rotation },
        EntityKind::MText(t) => Ucs2 { origin: t.insert.xy(), angle: t.rotation },
        EntityKind::Insert(i) => Ucs2 { origin: i.insert.xy(), angle: i.rotation },
        other => {
            return Err(bad("ucs", format!("không đặt UCS theo {} được (dùng đường thẳng, đa tuyến, cung, tròn, chữ, block)", other.type_name())));
        }
    };
    if !(u.origin.is_finite() && u.angle.is_finite()) {
        return Err(bad("ucs", "đối tượng không xác định được hướng"));
    }
    Ok(u)
}

fn seg_dist(q: Vec2, (a, b): (Vec2, Vec2)) -> f64 {
    let d = b - a;
    let t = if d.len2() > 1e-24 { ((q - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    q.dist(a + d * t)
}

fn named(s: &Session, name: &str) -> Result<Ucs2> {
    let u = s.doc()?.ucss.iter().find(|u| u.name.eq_ignore_ascii_case(name)).ok_or_else(|| bad("ucs", format!("không có UCS tên `{name}`")))?;
    Ok(Ucs2 { origin: u.origin.xy(), angle: u.x_axis.xy().angle() })
}

fn save_named(s: &mut Session, name: &str) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() || name.len() > 255 || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`']) {
        return Err(bad("ucs", format!("tên UCS không hợp lệ `{name}`")));
    }
    let u = s.ucs();
    let x = Vec2::from_angle(u.angle);
    let rec = Ucs { name: name.to_string(), origin: u.origin.to3(0.0), x_axis: x.to3(0.0), y_axis: x.perp().to3(0.0) };
    let d = s.doc_mut()?;
    match d.ucss.iter_mut().find(|v| v.name.eq_ignore_ascii_case(name)) {
        Some(v) => *v = rec,
        None => d.ucss.push(rec),
    }
    d.header.set("UCSNAME", HVal::Str(name.to_string()));
    s.touch();
    let msg = format!("Đã lưu UCS {name}.");
    Ok(json!({ "name": name, "message": msg }))
}

fn run_ucs(s: &mut Session, p: &Value) -> Result<Value> {
    if p.is_null() || p.as_object().is_some_and(|o| o.is_empty()) {
        let name = s.doc()?.header.str("UCSNAME", "");
        return Ok(describe(&s.ucs(), &name));
    }
    let r = if bool_or(p, "world", false) {
        set_ucs(s, Ucs2::default(), "")?
    } else if bool_or(p, "previous", false) {
        let prev = s.state_mut()?.ucs_history.pop().ok_or_else(|| bad("ucs", "không còn UCS trước đó"))?;
        store(s, prev, "")?
    } else if let Some(name) = str_param(p, "save") {
        return save_named(s, name);
    } else if let Some(name) = str_param(p, "restore") {
        let u = named(s, name)?;
        set_ucs(s, u, name)?
    } else if let Some(name) = str_param(p, "delete") {
        let d = s.doc_mut()?;
        let before = d.ucss.len();
        d.ucss.retain(|u| !u.name.eq_ignore_ascii_case(name));
        if d.ucss.len() == before {
            return Err(bad("ucs", format!("không có UCS tên `{name}`")));
        }
        s.touch();
        return Ok(json!({ "message": format!("Đã xóa UCS {name}.") }));
    } else if let Some(h) = p.get("object").and_then(|v| v.as_str().and_then(cadcraft_doc::Handle::parse_hex)) {
        let u = from_object(s, h, point_param(p, "at"))?;
        set_ucs(s, u, "")?
    } else {
        let cur = s.ucs();
        let origin = point_param(p, "origin").unwrap_or(cur.origin);
        let angle = if let Some(x) = point_param(p, "xpoint") {
            let d = x - origin;
            if d.len() < 1e-12 {
                return Err(bad("ucs", "điểm trên trục X trùng gốc"));
            }
            d.angle()
        } else if let Some(a) = p.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()) {
            a.to_radians()
        } else if let Some(a) = p.get("rotate").and_then(Value::as_f64).filter(|a| a.is_finite()) {
            cur.angle + a.to_radians()
        } else {
            cur.angle
        };
        set_ucs(s, Ucs2 { origin, angle }, "")?
    };
    Ok(r)
}

fn run_ucs_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let list: Vec<Value> =
        d.ucss.iter().map(|u| json!({ "name": u.name, "origin": [u.origin.x, u.origin.y], "angle": deg(u.x_axis.xy().angle()) })).collect();
    let msg = if list.is_empty() { "Chưa có UCS nào được đặt tên.".to_string() } else { format!("{} UCS đã đặt tên.", list.len()) };
    Ok(json!({ "ucs": list, "current": describe(&s.ucs(), &d.header.str("UCSNAME", "")), "message": msg }))
}

fn run_plan(s: &mut Session, p: &Value) -> Result<Value> {
    let t = if bool_or(p, "world", false) { 0.0 } else { s.ucs().angle };
    s.state_mut()?.set_twist(t);
    s.zoom_extents()?;
    let msg = if t.abs() < 1e-12 { "Mặt bằng theo WCS.".to_string() } else { format!("Mặt bằng theo UCS: màn hình xoay {:.4}°.", deg(t)) };
    Ok(json!({ "twist": deg(t), "message": msg }))
}

#[derive(Default)]
enum Stage {
    #[default]
    Origin,
    XAxis(Vec2),
    ZAngle,
    Object(SelectPhase),
    NamedOption,
    NamedName(&'static str),
}

/// UCS: "Specify origin of UCS or [Named/Object/Previous/World/Z] <World>".
#[derive(Default)]
struct UcsM {
    stage: Stage,
}

impl Interactive for UcsM {
    fn name(&self) -> &'static str {
        "UCS"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.stage {
            Stage::Origin => Prompt::new("Chọn gốc UCS hoặc", Accept::POINT).kw(&["Named", "Object", "Previous", "World", "Z"]).default("World"),
            Stage::XAxis(o) => Prompt::new("Chọn điểm trên trục X hoặc <giữ hướng>", Accept::POINT).base(*o),
            Stage::ZAngle => Prompt::new("Góc xoay quanh trục Z", Accept::NUMBER).default("90"),
            Stage::Object(sel) => sel.prompt(),
            Stage::NamedOption => Prompt::new("UCS đặt tên", Accept::TEXT).kw(&["Restore", "Save", "Delete", "?"]),
            Stage::NamedName(_) => Prompt::new("Tên UCS", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.stage) {
            Stage::Origin => match i {
                Input::Point(p) => {
                    self.stage = Stage::XAxis(p);
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    ucs_echo(s, &json!({ "world": true }))?;
                    Ok(Step::Done)
                }
                Input::Keyword(k) | Input::Text(k) => match k.to_ascii_lowercase().as_str() {
                    "world" | "w" => {
                        ucs_echo(s, &json!({ "world": true }))?;
                        Ok(Step::Done)
                    }
                    "previous" | "p" => {
                        if let Err(e) = ucs_echo(s, &json!({ "previous": true })) {
                            s.echo(e.to_string());
                        }
                        Ok(Step::Done)
                    }
                    "z" => {
                        self.stage = Stage::ZAngle;
                        Ok(Step::Continue)
                    }
                    "object" | "ob" | "o" => {
                        s.set_selection(Vec::new());
                        self.stage = Stage::Object(SelectPhase::single());
                        Ok(Step::Continue)
                    }
                    "named" | "na" | "n" => {
                        self.stage = Stage::NamedOption;
                        Ok(Step::Continue)
                    }
                    _ => Ok(Step::Continue),
                },
                Input::Cancel => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
            Stage::XAxis(o) => {
                match i {
                    Input::Point(x) if x.dist(o) > 1e-12 => {
                        ucs_echo(s, &json!({ "origin": [o.x, o.y], "xpoint": [x.x, x.y] }))?;
                    }
                    Input::Cancel => return Ok(Step::Done),
                    _ => {
                        ucs_echo(s, &json!({ "origin": [o.x, o.y] }))?;
                    }
                }
                Ok(Step::Done)
            }
            Stage::ZAngle => {
                let a = match i {
                    Input::Text(t) => super::machines::number(&t).unwrap_or(90.0),
                    Input::Cancel => return Ok(Step::Done),
                    _ => 90.0,
                };
                ucs_echo(s, &json!({ "rotate": a }))?;
                Ok(Step::Done)
            }
            Stage::Object(mut sel) => {
                let at = match &i {
                    Input::Point(p) => Some(*p),
                    _ => None,
                };
                let at = at.or(Some(s.cursor));
                match sel.feed(s, &i)? {
                    SelOutcome::More => {
                        self.stage = Stage::Object(sel);
                        Ok(Step::Continue)
                    }
                    SelOutcome::Empty => Ok(Step::Done),
                    SelOutcome::Done(hs) => {
                        s.set_selection(Vec::new());
                        if let Some(h) = hs.first() {
                            let mut p = json!({ "object": h.hex() });
                            if let (Some(o), Some(a)) = (p.as_object_mut(), at) {
                                o.insert("at".into(), json!([a.x, a.y]));
                            }
                            if let Err(e) = ucs_echo(s, &p) {
                                s.echo(e.to_string());
                            }
                        }
                        Ok(Step::Done)
                    }
                }
            }
            Stage::NamedOption => {
                let k = match i {
                    Input::Keyword(k) | Input::Text(k) => k.to_ascii_lowercase(),
                    _ => return Ok(Step::Done),
                };
                match k.as_str() {
                    "restore" | "r" => self.stage = Stage::NamedName("restore"),
                    "save" | "s" => self.stage = Stage::NamedName("save"),
                    "delete" | "d" => self.stage = Stage::NamedName("delete"),
                    _ => {
                        let names: Vec<String> = s.doc()?.ucss.iter().map(|u| u.name.clone()).collect();
                        s.echo(if names.is_empty() { "Chưa có UCS đặt tên.".to_string() } else { format!("UCS: {}", names.join(", ")) });
                        return Ok(Step::Done);
                    }
                }
                Ok(Step::Continue)
            }
            Stage::NamedName(op) => {
                if let Input::Text(t) | Input::Keyword(t) = i
                    && !t.trim().is_empty()
                {
                    let mut p = serde_json::Map::new();
                    p.insert(op.into(), json!(t.trim()));
                    if let Err(e) = ucs_echo(s, &Value::Object(p)) {
                        s.echo(e.to_string());
                    }
                }
                Ok(Step::Done)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_points_follow_the_ucs() {
        let mut s = Session::new();
        s.execute("ucs", &json!({ "origin": [100, 50], "angle": 30 })).unwrap();
        let u = s.ucs();
        assert!((u.angle - 30f64.to_radians()).abs() < 1e-12);
        // LINE 0,0 → @10,0 in the UCS: from the UCS origin along the turned X axis.
        s.start("line").unwrap();
        s.cmdline("0,0").unwrap();
        s.cmdline("@10,0").unwrap();
        s.cmdline("@5<90").unwrap();
        s.cmdline("*0,0").unwrap();
        s.cmdline("").unwrap();
        let lines: Vec<(Vec2, Vec2)> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 3);
        let (a, b) = lines[0];
        assert!(a.near(Vec2::new(100.0, 50.0), 1e-9));
        assert!(b.near(u.to_world(Vec2::new(10.0, 0.0)), 1e-9));
        assert!(lines[1].1.near(u.to_world(Vec2::new(10.0, 5.0)), 1e-9));
        assert!(lines[2].1.near(Vec2::ZERO, 1e-9), "*x,y is a world point");
        // Saved with the drawing; named UCS; previous; world.
        s.execute("ucs", &json!({ "save": "TUYEN-1" })).unwrap();
        let back = cadcraft_io::read(&cadcraft_io::write(s.doc().unwrap(), "u.dxf").unwrap(), "u.dxf").unwrap();
        assert!(back.header.point("UCSORG").unwrap().xy().near(Vec2::new(100.0, 50.0), 1e-9));
        assert!(back.ucss.iter().any(|u| u.name == "TUYEN-1"));
        s.execute("ucs", &json!({ "world": true })).unwrap();
        assert!(s.ucs().is_world());
        s.execute("ucs", &json!({ "previous": true })).unwrap();
        assert!(!s.ucs().is_world());
        s.execute("ucs", &json!({ "world": true })).unwrap();
        s.execute("ucs", &json!({ "restore": "TUYEN-1" })).unwrap();
        assert!(s.ucs().origin.near(Vec2::new(100.0, 50.0), 1e-9));
        assert!(s.execute("ucs", &json!({ "restore": "KHONG-CO" })).is_err());
    }

    #[test]
    fn ucs_on_an_object_and_plan() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[10, 10], [20, 20]] })).unwrap();
        let h = s.doc().unwrap().model.last().unwrap().handle;
        s.execute("ucs", &json!({ "object": h.hex() })).unwrap();
        let u = s.ucs();
        assert!(u.origin.near(Vec2::new(10.0, 10.0), 1e-9) && (u.angle - 45f64.to_radians()).abs() < 1e-12);
        s.execute("plan", &json!({})).unwrap();
        assert!((s.state().unwrap().twist() - 45f64.to_radians()).abs() < 1e-12);
        // Zoom extents in the turned view still shows the line.
        let v = s.state().unwrap().view();
        assert!(v.center.near(Vec2::new(15.0, 15.0), 1e-6));
        s.execute("plan", &json!({ "world": true })).unwrap();
        assert!(s.state().unwrap().twist().abs() < 1e-12);
        // Interactive: origin, X axis point.
        s.start("ucs").unwrap();
        s.cmdline("*5,5").unwrap();
        s.cmdline("*5,15").unwrap();
        let u = s.ucs();
        assert!(u.origin.near(Vec2::new(5.0, 5.0), 1e-9) && (u.angle - 90f64.to_radians()).abs() < 1e-12);
        s.start("ucs").unwrap();
        s.cmdline("w").unwrap();
        assert!(s.ucs().is_world());
    }
}
