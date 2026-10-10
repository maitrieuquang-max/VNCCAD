//! VNCCad: associative arrays (ARRAYRECT / ARRAYPOLAR with `associative: true`, the default of
//! the interactive commands) and ARRAYEDIT.
//!
//! An associative array is one object — an INSERT of an anonymous block holding the items —
//! whose block remembers the source objects and parameters, so ARRAYEDIT can change the number
//! of rows, columns or items and the spacing. EXPLODE turns it back into separate objects.

use std::f64::consts::TAU;
use std::sync::Arc;

use cadcraft_doc::{ArrayDef, Block, Entity, EntityKind, Handle, Insert};
use cadcraft_geom::{Bounds2, Mat3, Vec2, Vec3};
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("arrayedit", "Edit Array", run_arrayedit)
            .menu(&["Modify", "Array", "Edit Array"])
            .alias(&["suamang"])
            .params("{handle, rows?, cols?, rowSpacing?, colSpacing?, count?, angle?, rotate?}")
            .interactive(|_| Ok(Box::new(Flow::new("arrayedit", plan_arrayedit, flow_arrayedit))))
            .enabled(has_doc),
    ]
}

/// The items of an array: the source objects and their copies.
pub fn items(def: &ArrayDef) -> Vec<Entity> {
    let mut out: Vec<Entity> = def.source.clone();
    let copy = |m: &Mat3, out: &mut Vec<Entity>| {
        for e in &def.source {
            let mut c = e.clone();
            c.kind.transform(m);
            out.push(c);
        }
    };
    if def.kind == "polar" {
        let n = def.count.clamp(1, 10_000);
        let total = def.angle.to_radians();
        let step = if (total.abs() - TAU).abs() < 1e-9 { total / f64::from(n) } else { total / f64::from(n.saturating_sub(1).max(1)) };
        let reference =
            def.source.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.insert.xy()) } else { None }).next().unwrap_or_else(
                || def.source.iter().fold(Bounds2::EMPTY, |b, e| b.union(&cadcraft_doc::geom::Bounds2::from_points(e.kind.grips()))).center(),
            );
        for k in 1..n {
            let a = step * f64::from(k);
            let m = if def.rotate { Mat3::rotate_about(def.center, a) } else { Mat3::translate(reference.rotate_about(def.center, a) - reference) };
            copy(&m, &mut out);
        }
    } else {
        for r in 0..def.rows.clamp(1, 1000) {
            for c in 0..def.cols.clamp(1, 1000) {
                if r == 0 && c == 0 {
                    continue;
                }
                copy(&Mat3::translate(Vec2::new(def.col_spacing * f64::from(c), def.row_spacing * f64::from(r))), &mut out);
            }
        }
    }
    out
}

/// Replace the source objects by an associative array. Returns the array's INSERT.
pub fn make(s: &mut Session, hs: &[Handle], mut def: ArrayDef) -> Result<Handle> {
    let space = s.space();
    let d = s.doc_mut()?;
    let mut source = Vec::new();
    for h in hs {
        if let Some(e) = d.entity(*h).map(|e| (**e).clone()) {
            source.push(e);
        }
    }
    if source.is_empty() {
        return Err(bad("array", "chọn đối tượng"));
    }
    let common = source.first().map(|e| e.common.clone()).unwrap_or_default();
    for e in &source {
        d.remove_entity(e.handle);
    }
    def.source = source;
    let name = d.anonymous_block_name("*U");
    let mut b = Block::new(&name);
    b.anonymous = true;
    b.base = Vec3::ZERO;
    for mut e in items(&def) {
        e.handle = d.new_handle();
        b.entities.push(e);
    }
    b.array = Some(def);
    d.blocks.insert(name.clone(), Arc::new(b));
    let ins = Insert {
        block: name,
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    Ok(d.add(&space, common, EntityKind::Insert(ins))?)
}

/// The array an INSERT shows: (block name, parameters).
fn array_of(s: &Session, h: Handle) -> Result<(String, ArrayDef)> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| bad("arrayedit", format!("no entity {}", h.hex())))?;
    let EntityKind::Insert(i) = &e.kind else { return Err(bad("arrayedit", "không phải mảng liên kết")) };
    let b = d.block(&i.block).ok_or_else(|| bad("arrayedit", "không có block"))?;
    let a = b.array.clone().ok_or_else(|| bad("arrayedit", "không phải mảng liên kết (tạo bằng ARRAYRECT/ARRAYPOLAR với Associative = Yes)"))?;
    Ok((b.name.clone(), a))
}

/// Rebuild an array's block with new parameters (a new block: other references keep theirs).
fn rebuild(s: &mut Session, h: Handle, def: ArrayDef) -> Result<()> {
    let d = s.doc_mut()?;
    let name = d.anonymous_block_name("*U");
    let mut b = Block::new(&name);
    b.anonymous = true;
    for mut e in items(&def) {
        e.handle = d.new_handle();
        b.entities.push(e);
    }
    b.array = Some(def);
    d.blocks.insert(name.clone(), Arc::new(b));
    d.modify_entity(h, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block = name;
        }
    })?;
    Ok(())
}

fn describe(a: &ArrayDef) -> String {
    if a.kind == "polar" {
        format!("Mảng tròn: {} phần tử, góc {}°, xoay phần tử: {}", a.count, a.angle, if a.rotate { "có" } else { "không" })
    } else {
        format!("Mảng chữ nhật: {} hàng × {} cột, khoảng cách hàng {}, cột {}", a.rows, a.cols, a.row_spacing, a.col_spacing)
    }
}

fn run_arrayedit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("arrayedit", "`handle` is required"))?;
    let (_, mut a) = array_of(s, h)?;
    let u = |k: &str| p.get(k).and_then(Value::as_u64).map(|v| v.clamp(1, 10_000) as u32);
    if let Some(v) = u("rows") {
        a.rows = v;
    }
    if let Some(v) = u("cols") {
        a.cols = v;
    }
    if let Some(v) = u("count") {
        a.count = v;
    }
    let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(v) = f("rowSpacing") {
        a.row_spacing = v;
    }
    if let Some(v) = f("colSpacing") {
        a.col_spacing = v;
    }
    if let Some(v) = f("angle") {
        a.angle = v;
    }
    if let Some(v) = p.get("rotate").and_then(Value::as_bool) {
        a.rotate = v;
    }
    let msg = describe(&a);
    rebuild(s, h, a)?;
    Ok(json!({ "message": msg }))
}

fn plan_arrayedit(s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::One("Chọn mảng".into())),
        1 => {
            let h = a.first().map(Ans::sel).and_then(|v| v.first().copied())?;
            let (_, def) = array_of(s, h).ok()?;
            Some(if def.kind == "polar" {
                Ask::Kw { msg: format!("{} — đổi", describe(&def)), kws: vec!["Items", "Angle", "Rotate", "eXit"], default: Some("eXit") }
            } else {
                Ask::Kw {
                    msg: format!("{} — đổi", describe(&def)),
                    kws: vec!["Rows", "Columns", "RSpacing", "CSpacing", "eXit"],
                    default: Some("eXit"),
                }
            })
        }
        2 => match a.get(1).map(Ans::text) {
            Some("Rows") => Some(Ask::Num { msg: "Số hàng".into(), default: None }),
            Some("Columns") => Some(Ask::Num { msg: "Số cột".into(), default: None }),
            Some("RSpacing") => Some(Ask::Num { msg: "Khoảng cách giữa các hàng".into(), default: None }),
            Some("CSpacing") => Some(Ask::Num { msg: "Khoảng cách giữa các cột".into(), default: None }),
            Some("Items") => Some(Ask::Num { msg: "Số phần tử".into(), default: None }),
            Some("Angle") => Some(Ask::Num { msg: "Góc cần lấp (độ)".into(), default: Some(360.0) }),
            Some("Rotate") => Some(Ask::Kw { msg: "Xoay phần tử theo mảng?".into(), kws: vec!["Yes", "No"], default: Some("Yes") }),
            _ => None,
        },
        _ => None,
    }
}

fn flow_arrayedit(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let Some(h) = a.first().map(Ans::sel).and_then(|v| v.first().copied()) else { return Ok(Value::Null) };
    let opt = a.get(1).map(Ans::text).unwrap_or("eXit");
    let n = a.get(2).and_then(Ans::num);
    let p = match (opt, n) {
        ("Rows", Some(v)) => json!({ "handle": h.hex(), "rows": v.round().max(1.0) as u64 }),
        ("Columns", Some(v)) => json!({ "handle": h.hex(), "cols": v.round().max(1.0) as u64 }),
        ("RSpacing", Some(v)) => json!({ "handle": h.hex(), "rowSpacing": v }),
        ("CSpacing", Some(v)) => json!({ "handle": h.hex(), "colSpacing": v }),
        ("Items", Some(v)) => json!({ "handle": h.hex(), "count": v.round().max(1.0) as u64 }),
        ("Angle", Some(v)) => json!({ "handle": h.hex(), "angle": v }),
        ("Rotate", _) => json!({ "handle": h.hex(), "rotate": a.get(2).map(Ans::text) == Some("Yes") }),
        _ => return Ok(Value::Null),
    };
    run_arrayedit(s, &p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn associative_rect_and_polar_arrays_edit_and_explode() {
        let mut s = Session::new();
        let r = s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
        let c = r["handle"].as_str().or(r["handles"][0].as_str()).unwrap().to_string();
        let r =
            s.execute("arrayrect", &json!({ "handles": [c], "rows": 2, "cols": 3, "rowSpacing": 5, "colSpacing": 4, "associative": true })).unwrap();
        let arr = Handle::parse_hex(r["array"].as_str().unwrap()).unwrap();
        assert_eq!(s.doc().unwrap().model.len(), 1, "one object");
        let count = |s: &Session| {
            let d = s.doc().unwrap();
            let EntityKind::Insert(i) = &d.entity(arr).unwrap().kind else { panic!() };
            d.block(&i.block).unwrap().entities.len()
        };
        assert_eq!(count(&s), 6);
        // Interactive edit: 4 columns.
        s.set_selection(vec![]);
        s.cmdline("ARRAYEDIT").unwrap();
        s.input(crate::Input::Pick(vec![arr])).unwrap();
        s.cmdline("COL").unwrap();
        s.cmdline("4").unwrap();
        assert!(s.running.is_none(), "{:?}", s.current_prompt());
        assert_eq!(count(&s), 8);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(count(&s), 6);
        // Explode gives the items back.
        s.execute("explode", &json!({ "handles": [arr.hex()] })).unwrap();
        assert_eq!(s.doc().unwrap().model.len(), 6);
        // Polar.
        let r = s.execute("line", &json!({ "points": [[10, 0], [12, 0]] })).unwrap();
        let l = r["handle"].as_str().or(r["handles"][0].as_str()).unwrap().to_string();
        let r = s.execute("arraypolar", &json!({ "handles": [l], "center": [0, 0], "count": 8, "associative": true })).unwrap();
        let p = Handle::parse_hex(r["array"].as_str().unwrap()).unwrap();
        s.execute("arrayedit", &json!({ "handle": p.hex(), "count": 12 })).unwrap();
        let d = s.doc().unwrap();
        let EntityKind::Insert(i) = &d.entity(p).unwrap().kind else { panic!() };
        assert_eq!(d.block(&i.block).unwrap().entities.len(), 12);
    }
}
