//! VNCCad: fields (FIELD, UPDATEFIELD) and jogged radius dimensions (DIMJOGGED).
//!
//! A field is a TEXT or MTEXT whose value is computed: an object's area, perimeter, length or
//! radius (with a unit factor, decimals, prefix and suffix — e.g. `S = 12.50 m²`), the date,
//! the file name, the layout name, or how many objects a selection holds. Object fields follow
//! their object: they are refreshed after every command.

use cadcraft_doc::{DimKind, Dimension, EntityKind, FieldLink, Handle, MText};
use cadcraft_geom::Polyline;
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("field", "Field", run_field)
            .menu(&["Insert", "Field"])
            .alias(&["truong", "fld"])
            .params(
                "{kind: area|perimeter|length|radius|date|filename|sheet, handle?: object, at: [x,y], height?, decimals?: 2, factor?: 1, prefix?, suffix?, format?: \"%d/%m/%Y\"}",
            )
            .interactive(|_| Ok(Box::new(Flow::new("field", plan_field, flow_field))))
            .enabled(has_doc),
        CommandSpec::new("updatefield", "Update Fields", run_updatefield).menu(&["Tools", "Update Fields"]).params("{} → recomputes every field").enabled(has_doc),
        CommandSpec::new("dimjogged", "Jogged Dimension", run_dimjogged)
            .menu(&["Dimension", "Jogged"])
            .alias(&["jog", "dimjog"])
            .params("{handle: arc/circle | center + radius, at: point on the dimension line side, centerOverride: [x,y], jog: [x,y]}")
            .interactive(|_| Ok(Box::new(Flow::new("dimjogged", plan_dimjogged, flow_dimjogged))))
            .enabled(has_doc),
    ]
}

// ------------------------------------------------------------------ measuring

/// Area, perimeter (or length) and radius of an object.
fn measure(s: &Session, h: Handle) -> Option<(Option<f64>, f64, Option<f64>)> {
    let d = s.doc().ok()?;
    let e = d.entity(h)?;
    Some(match &e.kind {
        EntityKind::Circle(c) => (Some(c.radius * c.radius * std::f64::consts::PI), c.radius * std::f64::consts::TAU, Some(c.radius)),
        EntityKind::Arc(a) => {
            let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
            (None, a.radius * g.sweep(), Some(a.radius))
        }
        EntityKind::LwPolyline(pl) => {
            let g = Polyline { vertices: pl.vertices.clone(), closed: pl.closed };
            let closed = Polyline { vertices: pl.vertices.clone(), closed: true };
            (Some(closed.area().abs()), g.len(), None)
        }
        EntityKind::Hatch(hh) => {
            let (a, p) = hh.loops.iter().fold((0.0, 0.0), |acc, l| {
                let g = Polyline { vertices: l.vertices.clone(), closed: true };
                (acc.0 + g.area().abs(), acc.1 + g.len())
            });
            (Some(a), p, None)
        }
        _ => {
            let pls = crate::select::hit_polylines(d, e, 1e-3);
            let len: f64 = pls.iter().map(|p| p.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.dist(*b))).sum::<f64>()).sum();
            if len <= 0.0 {
                return None;
            }
            (None, len, None)
        }
    })
}

/// The current date as (year, month, day), from the host's clock (local time on the web).
fn today(s: &Session) -> Option<(i64, u32, u32)> {
    let ms = s.clock.map(|f| f())?;
    let days = (ms / 86_400_000.0).floor() as i64;
    // Civil from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    Some((if m <= 2 { y + 1 } else { y }, m, d))
}

fn number(v: f64, f: &FieldLink) -> String {
    let k = if f.factor.is_finite() && f.factor != 0.0 { f.factor } else { 1.0 };
    format!("{}{:.*}{}", f.prefix, f.decimals.min(10) as usize, v * k, f.suffix)
}

/// The text a field shows now (`None`: cannot be computed — keep the old value).
fn value(s: &Session, f: &FieldLink) -> Option<String> {
    match f.kind.as_str() {
        "area" => measure(s, f.object?)?.0.map(|a| number(a, f)),
        "perimeter" | "length" => measure(s, f.object?).map(|m| number(m.1, f)),
        "radius" => measure(s, f.object?)?.2.map(|r| number(r, f)),
        "date" => {
            let (y, m, d) = today(s)?;
            let fmt = if f.format.is_empty() { "%d/%m/%Y" } else { &f.format };
            Some(format!(
                "{}{}{}",
                f.prefix,
                fmt.replace("%d", &format!("{d:02}")).replace("%m", &format!("{m:02}")).replace("%Y", &y.to_string()),
                f.suffix
            ))
        }
        "filename" => s.state().ok().map(|st| format!("{}{}{}", f.prefix, st.title, f.suffix)),
        "sheet" => s.state().ok().map(|st| {
            let name = match &st.space {
                cadcraft_doc::Space::Model => "Model".to_string(),
                cadcraft_doc::Space::Paper(n) => n.clone(),
            };
            format!("{}{name}{}", f.prefix, f.suffix)
        }),
        _ => None,
    }
}

/// Recompute every field of the active drawing; returns how many texts changed. Fields whose
/// text was erased are dropped.
pub fn refresh(s: &mut Session) -> usize {
    let Ok(d) = s.doc() else { return 0 };
    if d.fields.is_empty() {
        return 0;
    }
    let fields = d.fields.clone();
    let mut changes: Vec<(Handle, String)> = Vec::new();
    let mut keep = Vec::new();
    for f in &fields {
        let Some(e) = d.entity(f.text) else { continue };
        keep.push(f.clone());
        let cur = match &e.kind {
            EntityKind::Text(t) => t.value.clone(),
            EntityKind::MText(t) => t.contents.clone(),
            _ => continue,
        };
        if let Some(v) = value(s, f)
            && v != cur
        {
            changes.push((f.text, v));
        }
    }
    let n = changes.len();
    let Ok(d) = s.doc_mut() else { return 0 };
    if keep.len() != d.fields.len() {
        d.fields = keep;
    }
    for (h, v) in changes {
        let _ = d.modify_entity(h, |e| match &mut e.kind {
            EntityKind::Text(t) => t.value = v,
            EntityKind::MText(t) => t.contents = v,
            _ => {}
        });
    }
    n
}

fn run_updatefield(s: &mut Session, _p: &Value) -> Result<Value> {
    let n = refresh(s);
    Ok(json!({ "updated": n, "message": format!("Đã cập nhật {n} trường.") }))
}

fn make_field(s: &mut Session, f: FieldLink, at: Vec2, height: Option<f64>) -> Result<Handle> {
    let v = value(s, &f).ok_or_else(|| bad("field", "không tính được giá trị (đối tượng không có diện tích/chiều dài?)"))?;
    let d = s.doc()?;
    let h = height.filter(|h| *h > 0.0).unwrap_or_else(|| d.header.f64("TEXTSIZE", 2.5));
    let style = d.header.str("TEXTSTYLE", "Standard");
    let k = EntityKind::MText(MText {
        insert: Vec3::new(at.x, at.y, 0.0),
        height: h,
        width: 0.0,
        attach: 7,
        rotation: 0.0,
        style,
        contents: v,
        line_spacing: 1.0,
    });
    let th = s.add_entity(k)?;
    s.doc_mut()?.fields.push(FieldLink { text: th, ..f });
    Ok(th)
}

fn link_from(p: &Value, object: Option<Handle>) -> FieldLink {
    FieldLink {
        text: Handle(0),
        kind: str_param(p, "kind").unwrap_or("area").to_ascii_lowercase(),
        object,
        factor: f64_or(p, "factor", 1.0),
        decimals: p.get("decimals").and_then(Value::as_u64).unwrap_or(2).min(10) as u32,
        prefix: str_param(p, "prefix").unwrap_or("").to_string(),
        suffix: str_param(p, "suffix").unwrap_or("").to_string(),
        format: str_param(p, "format").unwrap_or("").to_string(),
    }
}

fn run_field(s: &mut Session, p: &Value) -> Result<Value> {
    let object = p.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex);
    let f = link_from(p, object);
    if matches!(f.kind.as_str(), "area" | "perimeter" | "length" | "radius") && object.is_none() {
        return Err(bad("field", "`handle` of the object is required"));
    }
    let at = point_req("field", p, "at")?;
    let h = make_field(s, f, at, p.get("height").and_then(Value::as_f64))?;
    Ok(json!({ "handle": h.hex() }))
}

const KINDS: [&str; 7] = ["Area", "Perimeter", "Length", "Radius", "Date", "Filename", "Sheet"];

fn object_kind(k: &str) -> bool {
    matches!(k, "Area" | "Perimeter" | "Length" | "Radius")
}

fn plan_field(_s: &Session, a: &[Ans]) -> Option<Ask> {
    let k = a.first().map(Ans::text).unwrap_or("");
    let obj = object_kind(k);
    match a.len() {
        0 => Some(Ask::Kw { msg: "Loại trường".into(), kws: KINDS.to_vec(), default: Some("Area") }),
        1 if obj => Some(Ask::One("Chọn đối tượng".into())),
        2 if obj => Some(Ask::Num { msg: "Hệ số đổi đơn vị (vd 0.000001 cho mm² → m²)".into(), default: Some(1.0) }),
        3 if obj => Some(Ask::Num { msg: "Số chữ số thập phân".into(), default: Some(2.0) }),
        4 if obj => Some(Ask::Text { msg: "Chữ đứng trước (vd S = )".into(), default: Some(String::new()) }),
        5 if obj => Some(Ask::Text { msg: "Chữ đứng sau (vd  m²)".into(), default: Some(String::new()) }),
        6 if obj => Some(Ask::Point("Vị trí đặt chữ".into())),
        1 => Some(Ask::Point("Vị trí đặt chữ".into())),
        _ => None,
    }
}

fn flow_field(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let k = a.first().map(Ans::text).unwrap_or("Area").to_ascii_lowercase();
    let (object, factor, decimals, prefix, suffix, at) = if object_kind(a.first().map(Ans::text).unwrap_or("")) {
        (
            a.get(1).map(Ans::sel).and_then(|v| v.first().copied()),
            a.get(2).and_then(Ans::num).unwrap_or(1.0),
            a.get(3).and_then(Ans::num).unwrap_or(2.0),
            a.get(4).map(Ans::text).unwrap_or("").to_string(),
            a.get(5).map(Ans::text).unwrap_or("").to_string(),
            a.get(6),
        )
    } else {
        (None, 1.0, 0.0, String::new(), String::new(), a.get(1))
    };
    let Some(Ans::Point(at)) = at else { return Ok(Value::Null) };
    let f = FieldLink {
        text: Handle(0),
        kind: k,
        object,
        factor,
        decimals: decimals.round().clamp(0.0, 10.0) as u32,
        prefix,
        suffix,
        format: String::new(),
    };
    make_field(s, f, *at, None)?;
    Ok(Value::Null)
}

// ------------------------------------------------------------------ DIMJOGGED

fn circle_of(s: &Session, h: Handle) -> Option<(Vec2, f64)> {
    match &s.doc().ok()?.entity(h)?.kind {
        EntityKind::Circle(c) => Some((c.center.xy(), c.radius)),
        EntityKind::Arc(a) => Some((a.center.xy(), a.radius)),
        _ => None,
    }
}

fn jogged(s: &mut Session, c: Vec2, r: f64, at: Vec2, over: Vec2, jog: Vec2) -> Result<Handle> {
    let u = (at - c).normalized();
    let u = if u.is_finite() && u != Vec2::ZERO { u } else { Vec2::X };
    let p15 = c + u * r;
    let d = s.doc()?;
    let style = d.header.str("DIMSTYLE", "Standard");
    let dm = Dimension {
        kind: DimKind::Jogged,
        defpt: Vec3::new(c.x, c.y, 0.0),
        text_mid: Vec3::new(p15.x, p15.y, 0.0),
        p13: Vec3::new(over.x, over.y, 0.0),
        p14: Vec3::new(jog.x, jog.y, 0.0),
        p15: Vec3::new(p15.x, p15.y, 0.0),
        p16: Vec3::ZERO,
        text: String::new(),
        style,
        measurement: r,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
    };
    s.add_entity(EntityKind::Dimension(dm))
}

fn run_dimjogged(s: &mut Session, p: &Value) -> Result<Value> {
    let (c, r) = match (point_param(p, "center"), p.get("radius").and_then(Value::as_f64)) {
        (Some(c), Some(r)) => (c, r),
        _ => {
            let h = targets(s, p)?.first().copied().ok_or_else(|| bad("dimjogged", "`handle` of an arc/circle, or center + radius"))?;
            circle_of(s, h).ok_or_else(|| bad("dimjogged", "object is not a circle or arc"))?
        }
    };
    let at = point_req("dimjogged", p, "at")?;
    let over = point_req("dimjogged", p, "centerOverride")?;
    let jog = point_param(p, "jog").unwrap_or_else(|| over.mid(at));
    let h = jogged(s, c, r, at, over, jog)?;
    Ok(json!({ "handle": h.hex() }))
}

fn plan_dimjogged(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::One("Chọn cung hoặc đường tròn".into())),
        1 => Some(Ask::Point("Vị trí tâm thay thế".into())),
        2 => Some(Ask::Point("Vị trí đường kích thước".into())),
        3 => Some(Ask::Point("Vị trí gấp khúc".into())),
        _ => None,
    }
}

fn flow_dimjogged(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let h = a.first().map(Ans::sel).and_then(|v| v.first().copied()).ok_or_else(|| bad("dimjogged", "chọn cung"))?;
    let (c, r) = circle_of(s, h).ok_or_else(|| bad("dimjogged", "cần cung hoặc đường tròn"))?;
    let (Some(Ans::Point(over)), Some(Ans::Point(at)), Some(Ans::Point(jog))) = (a.get(1), a.get(2), a.get(3)) else { return Ok(Value::Null) };
    jogged(s, c, r, *at, *over, *jog)?;
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(r: &Value) -> Handle {
        Handle::parse_hex(r["handle"].as_str().or(r["handles"][0].as_str()).unwrap()).unwrap()
    }

    #[test]
    fn area_field_follows_its_object() {
        let mut s = Session::new();
        let r = s.execute("rectang", &json!({ "p1": [0, 0], "p2": [2000, 3000] })).unwrap();
        let pl = hex(&r);
        let r = s
            .execute(
                "field",
                &json!({ "kind": "area", "handle": pl.hex(), "at": [0, -500], "factor": 1e-6, "decimals": 2, "prefix": "S = ", "suffix": " m²" }),
            )
            .unwrap();
        let t = hex(&r);
        let text =
            |s: &Session| if let EntityKind::MText(m) = &s.doc().unwrap().entity(t).unwrap().kind { m.contents.clone() } else { String::new() };
        assert_eq!(text(&s), "S = 6.00 m²");
        // Scaling the rectangle updates the field after the command.
        s.execute("scale", &json!({ "handles": [pl.hex()], "base": [0, 0], "factor": 2 })).unwrap();
        assert_eq!(text(&s), "S = 24.00 m²");
        // One undo step brings both back.
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(text(&s), "S = 6.00 m²");
        // Survives DXF.
        let d = s.doc().unwrap().clone();
        let back = cadcraft_io::read(&cadcraft_io::write(&d, "f.dxf").unwrap(), "f.dxf").unwrap();
        assert_eq!(back.fields.len(), 1);
        assert_eq!(back.fields[0].object, Some(pl));
    }

    #[test]
    fn jogged_dimension_draws_and_round_trips() {
        let mut s = Session::new();
        let r = s.execute("arc", &json!({ "center": [0, 0], "radius": 500, "start": 30, "end": 60 })).unwrap();
        let a = hex(&r);
        let r = s.execute("dimjogged", &json!({ "handle": a.hex(), "at": [400, 300], "centerOverride": [300, 150], "jog": [350, 220] })).unwrap();
        let h = hex(&r);
        let d = s.doc().unwrap().clone();
        let EntityKind::Dimension(dm) = &d.entity(h).unwrap().kind else { panic!() };
        assert!(matches!(dm.kind, DimKind::Jogged));
        assert!((dm.p15.xy().len() - 500.0).abs() < 1e-9);
        let g = cadcraft_render::dimension_geometry(dm, &cadcraft_doc::DimStyle::default(), 1.0);
        assert!(g.mtext.contains("500"), "{}", g.mtext);
        let l = cadcraft_render::build(&d, &cadcraft_doc::Space::Model, &cadcraft_render::Options::default());
        assert!(l.prims.iter().filter(|p| p.handle == h).count() > 5, "line, arrow and text are drawn");
        let back = cadcraft_io::read(&cadcraft_io::write(&d, "j.dxf").unwrap(), "j.dxf").unwrap();
        let EntityKind::Dimension(b) = &back.entity(h).unwrap().kind else { panic!() };
        assert!(matches!(b.kind, DimKind::Jogged) && b.p13.xy().near(Vec2::new(300.0, 150.0), 1e-9));
    }
}
