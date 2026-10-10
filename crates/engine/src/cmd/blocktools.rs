//! VNCCad: XCLIP (show only part of a block reference or xref) and ATTSYNC (bring block
//! references' attributes in line with the block's attribute definitions).

use cadcraft_doc::{Attrib, EntityKind, Handle};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("xclip", "Clip Block or Xref", run_xclip)
            .menu(&["Modify", "Clip", "Xref/Block"])
            .alias(&["xc", "catblock"])
            .params("{handles?, boundary?: [[x,y]…] (2 corners or a polygon, drawing coordinates) | polyline?: hex | delete?: true}")
            .interactive(|_| Ok(Box::new(Flow::new("xclip", plan_xclip, flow_xclip))))
            .enabled(has_doc),
        CommandSpec::new("attsync", "Synchronize Attributes", run_attsync)
            .menu(&["Modify", "Object", "Attribute", "Synchronize"])
            .alias(&["dongbothuoctinh"])
            .params("{block?: name | handles?: references} → every reference of the block gets the block's attributes (values kept by tag)")
            .interactive(|_| Ok(Box::new(Flow::new("attsync", plan_attsync, flow_attsync))))
            .enabled(has_doc),
    ]
}

/// Clip (or unclip with `boundary: None`) block references. `boundary` is in drawing
/// coordinates: two corners or a polygon.
fn clip(s: &mut Session, hs: &[Handle], boundary: Option<&[Vec2]>) -> Result<usize> {
    if let Some(b) = boundary
        && (b.len() < 2 || (b.len() == 2 && b.first().zip(b.get(1)).is_some_and(|(p, q)| (p.x - q.x).abs() < 1e-12 || (p.y - q.y).abs() < 1e-12)))
    {
        return Err(bad("xclip", "đường bao cần 2 góc khác nhau hoặc ít nhất 3 điểm"));
    }
    let d = s.doc()?;
    let mut jobs: Vec<(Handle, Option<Vec<Vec2>>)> = Vec::new();
    for h in hs {
        let Some(e) = d.entity(*h) else { continue };
        let EntityKind::Insert(i) = &e.kind else { continue };
        let base = d.block(&i.block).map(|b| b.base.xy()).unwrap_or_default();
        let local = match boundary {
            Some(b) => {
                let Some(inv) = i.transform(base).inverse() else { continue };
                Some(b.iter().map(|p| inv.apply(*p)).collect())
            }
            None => None,
        };
        jobs.push((*h, local));
    }
    let n = jobs.len();
    let d = s.doc_mut()?;
    for (h, c) in jobs {
        d.modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.clip = c;
            }
        })?;
    }
    Ok(n)
}

fn polyline_points(s: &Session, h: Handle) -> Option<Vec<Vec2>> {
    let d = s.doc().ok()?;
    match &d.entity(h)?.kind {
        EntityKind::LwPolyline(p) => Some(p.vertices.iter().map(|v| v.p).collect()),
        EntityKind::Polyline3d(p) => Some(p.points.iter().map(|v| v.xy()).collect()),
        EntityKind::Circle(c) => {
            Some((0..64).map(|k| c.center.xy() + Vec2::from_angle(f64::from(k) * std::f64::consts::TAU / 64.0) * c.radius).collect())
        }
        _ => None,
    }
}

fn run_xclip(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if bool_or(p, "delete", false) {
        let n = clip(s, &hs, None)?;
        return Ok(json!({ "changed": n }));
    }
    let boundary = match (points_param(p, "boundary"), p.get("polyline").and_then(Value::as_str).and_then(Handle::parse_hex)) {
        (Some(b), _) => b,
        (None, Some(pl)) => polyline_points(s, pl).ok_or_else(|| bad("xclip", "`polyline` phải là polyline hoặc đường tròn"))?,
        _ => return Err(bad("xclip", "`boundary`, `polyline` or `delete` is required")),
    };
    let n = clip(s, &hs, Some(&boundary))?;
    Ok(json!({ "changed": n }))
}

fn plan_xclip(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn block hoặc xref".into())),
        1 => Some(Ask::Kw { msg: "Tùy chọn cắt".into(), kws: vec!["New", "Delete"], default: Some("New") }),
        2 if a.get(1).map(Ans::text) == Some("New") => {
            Some(Ask::Kw { msg: "Đường bao".into(), kws: vec!["Rectangular", "Select polyline"], default: Some("Rectangular") })
        }
        3 if a.get(2).map(Ans::text) == Some("Select polyline") => Some(Ask::One("Chọn polyline làm đường bao".into())),
        3 => Some(Ask::Point("Góc thứ nhất".into())),
        4 if a.get(2).map(Ans::text) == Some("Rectangular") => Some(Ask::Point("Góc đối diện".into())),
        _ => None,
    }
}

fn flow_xclip(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let n = match (a.get(1).map(Ans::text), a.get(2).map(Ans::text)) {
        (Some("Delete"), _) => clip(s, &hs, None)?,
        (_, Some("Select polyline")) => {
            let pl = a.get(3).map(Ans::sel).and_then(|v| v.first().copied()).ok_or_else(|| bad("xclip", "chọn polyline"))?;
            let b = polyline_points(s, pl).ok_or_else(|| bad("xclip", "cần polyline hoặc đường tròn"))?;
            clip(s, &hs, Some(&b))?
        }
        _ => {
            let (Some(Ans::Point(p)), Some(Ans::Point(q))) = (a.get(3), a.get(4)) else { return Ok(Value::Null) };
            clip(s, &hs, Some(&[*p, *q]))?
        }
    };
    Ok(json!({ "message": format!("Đã cập nhật đường cắt của {n} block.") }))
}

/// Rebuild the attributes of every reference of `block` from its attribute definitions.
fn attsync(s: &mut Session, block: &str) -> Result<usize> {
    let d = s.doc()?;
    let blk = d.block(block).cloned().ok_or_else(|| bad("attsync", format!("không có block {block}")))?;
    let defs: Vec<Attrib> = blk
        .entities
        .iter()
        .filter_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None })
        .filter(|a| !a.constant)
        .collect();
    let refs: Vec<Handle> = d
        .model
        .iter()
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(block)))
        .map(|e| e.handle)
        .collect();
    let base = blk.base.xy();
    let d = s.doc_mut()?;
    for h in &refs {
        d.modify_entity(*h, |e| {
            let EntityKind::Insert(i) = &mut e.kind else { return };
            let m = i.transform(base);
            let old = std::mem::take(&mut i.attribs);
            for ad in &defs {
                let mut k = EntityKind::Text(ad.text.clone());
                k.transform(&m);
                let EntityKind::Text(mut t) = k else { continue };
                t.value =
                    old.iter().find(|o| o.tag.eq_ignore_ascii_case(&ad.tag)).map(|o| o.text.value.clone()).unwrap_or_else(|| ad.text.value.clone());
                i.attribs.push(Attrib { tag: ad.tag.clone(), text: t, invisible: ad.invisible, constant: false, prompt: String::new() });
            }
        })?;
    }
    Ok(refs.len())
}

fn run_attsync(s: &mut Session, p: &Value) -> Result<Value> {
    let name = match str_param(p, "block") {
        Some(n) => n.to_string(),
        None => {
            let hs = targets(s, p)?;
            let d = s.doc()?;
            hs.iter()
                .filter_map(|h| d.entity(*h))
                .find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None })
                .ok_or_else(|| bad("attsync", "`block` or a block reference is required"))?
        }
    };
    let n = attsync(s, &name)?;
    Ok(json!({ "synced": n, "message": format!("ATTSYNC: đã đồng bộ {n} block {name}.") }))
}

fn plan_attsync(_s: &Session, a: &[Ans]) -> Option<Ask> {
    a.is_empty().then(|| Ask::One("Chọn một block".into()))
}

fn flow_attsync(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    run_attsync(s, &json!({ "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xclip_hides_outside_and_attsync_rebuilds_attributes() {
        let mut s = Session::new();
        // A block of two lines and an attribute.
        let r1 = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        let r2 = s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
        let hx = |r: &Value| r["handle"].as_str().or(r["handles"][0].as_str()).unwrap().to_string();
        s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [hx(&r1), hx(&r2)], "keep": "delete" })).unwrap();
        let r = s.execute("insert", &json!({ "name": "B", "at": [100, 100] })).unwrap();
        let ins = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
        let segs = |s: &Session| {
            let l = cadcraft_render::build(s.doc().unwrap(), &cadcraft_doc::Space::Model, &cadcraft_render::Options::default());
            l.prims.iter().filter(|p| p.kind == cadcraft_render::Kind::Polyline).map(|p| l.points(p).to_vec()).collect::<Vec<_>>()
        };
        assert_eq!(segs(&s).len(), 2);
        // Keep the lower half and the left 4 units.
        s.execute("xclip", &json!({ "handles": [ins.hex()], "boundary": [[99, 98], [104, 103]] })).unwrap();
        let v = segs(&s);
        assert_eq!(v.len(), 1, "{v:?}");
        let p = &v[0];
        assert!(p[0].near(Vec2::new(100.0, 100.0), 1e-9) && p[1].near(Vec2::new(104.0, 100.0), 1e-9), "{p:?}");
        // The clip follows the reference when it moves.
        s.execute("move", &json!({ "handles": [ins.hex()], "from": [0, 0], "to": [10, 0] })).unwrap();
        let v = segs(&s);
        assert!(v[0][1].near(Vec2::new(114.0, 100.0), 1e-9), "{v:?}");
        // Survives DXF.
        let d = s.doc().unwrap().clone();
        let back = cadcraft_io::read(&cadcraft_io::write(&d, "x.dxf").unwrap(), "x.dxf").unwrap();
        let EntityKind::Insert(i) = &back.entity(ins).unwrap().kind else { panic!() };
        let c = i.clip.clone().unwrap();
        assert!(c[0].near(Vec2::new(-1.0, -2.0), 1e-9) && c[1].near(Vec2::new(4.0, 3.0), 1e-9), "{c:?}");
        s.execute("xclip", &json!({ "handles": [ins.hex()], "delete": true })).unwrap();
        assert_eq!(segs(&s).len(), 2);
        // ATTSYNC: add an attribute definition to the block, then sync.
        {
            let d = s.doc_mut().unwrap();
            let b = d.blocks.get_mut("B").unwrap();
            let t: cadcraft_doc::Text = serde_json::from_value(json!({ "insert": {"x":0.0,"y":-3.0,"z":0.0}, "height": 2.0, "value": "?" })).unwrap();
            let h = Handle(0x7777);
            std::sync::Arc::make_mut(b).entities.push(cadcraft_doc::Entity {
                handle: h,
                common: Default::default(),
                kind: EntityKind::AttDef(Attrib { tag: "SOCOC".into(), text: t, invisible: false, constant: false, prompt: "Số cọc".into() }),
            });
        }
        let r = s.execute("attsync", &json!({ "block": "B" })).unwrap();
        assert_eq!(r["synced"], 1);
        let EntityKind::Insert(i) = &s.doc().unwrap().entity(ins).unwrap().kind else { panic!() };
        assert_eq!(i.attribs.len(), 1);
        assert!((i.attribs[0].text.insert.y - 97.0).abs() < 1e-9);
    }
}
