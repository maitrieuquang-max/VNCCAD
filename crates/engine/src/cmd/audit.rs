//! VNCCad: AUDIT (find and fix errors in the drawing) and RECOVER (open a drawing and audit it).
//!
//! Checks: objects on layers, linetypes, text or dimension styles that do not exist; block
//! references to missing blocks; blocks that contain themselves; objects with invalid
//! coordinates (NaN, infinite); empty polylines and hatches; groups, fields and links that point
//! at erased objects; a current layer, linetype or style that does not exist.

use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Layer};
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("audit", "Audit", run_audit)
            .menu(&["File", "Drawing Utilities", "Audit"])
            .alias(&["kiemtra"])
            .params("{fix?: true} → {errors, fixed, report: [..]}")
            .interactive(|_| Ok(Box::new(Flow::new("audit", plan_audit, flow_audit))))
            .enabled(has_doc),
        CommandSpec::new("recover", "Recover", run_recover)
            .menu(&["File", "Drawing Utilities", "Recover..."])
            .alias(&["phuchoi"])
            .params("{path} | {data, name} → opens the drawing and fixes the errors AUDIT finds")
            .enabled(always),
    ]
}

/// Problems found (and fixed when `fix`).
pub(crate) fn audit(d: &mut Drawing, fix: bool) -> Vec<String> {
    let mut report: Vec<String> = Vec::new();
    let count = |report: &mut Vec<String>, n: usize, what: &str| {
        if n > 0 {
            report.push(format!("{n} {what}"));
        }
    };
    let layers: std::collections::HashSet<String> = d.layers.iter().map(|l| l.name.to_ascii_uppercase()).collect();
    let linetypes: std::collections::HashSet<String> = d.linetypes.iter().map(|l| l.name.to_ascii_uppercase()).collect();
    let styles: std::collections::HashSet<String> = d.text_styles.iter().map(|l| l.name.to_ascii_uppercase()).collect();
    let dimstyles: std::collections::HashSet<String> = d.dim_styles.iter().map(|l| l.name.to_ascii_uppercase()).collect();
    let blocks: std::collections::HashSet<String> = d.blocks.keys().map(|k| k.to_ascii_uppercase()).collect();
    let lt_ok = |n: &str| {
        let u = n.to_ascii_uppercase();
        u.is_empty() || u == "BYLAYER" || u == "BYBLOCK" || u == "CONTINUOUS" || linetypes.contains(&u)
    };

    // Problems of one entity: (missing layer, bad linetype, bad style, bad dimstyle, missing block, invalid).
    #[derive(Default)]
    struct Found {
        missing_layers: Vec<String>,
        bad_lt: usize,
        bad_style: usize,
        bad_dimstyle: usize,
        missing_block: usize,
        invalid: usize,
    }
    let mut f = Found::default();
    let check = |e: &Entity, f: &mut Found| -> (bool, bool) {
        // (needs repair, must be erased)
        let mut repair = false;
        let mut erase = false;
        if !layers.contains(&e.common.layer.to_ascii_uppercase()) && !f.missing_layers.iter().any(|l| l.eq_ignore_ascii_case(&e.common.layer)) {
            f.missing_layers.push(e.common.layer.clone());
        }
        if !lt_ok(&e.common.linetype) {
            f.bad_lt += 1;
            repair = true;
        }
        match &e.kind {
            EntityKind::Text(t) | EntityKind::AttDef(cadcraft_doc::Attrib { text: t, .. }) if !styles.contains(&t.style.to_ascii_uppercase()) => {
                f.bad_style += 1;
                repair = true;
            }
            EntityKind::MText(t) if !styles.contains(&t.style.to_ascii_uppercase()) => {
                f.bad_style += 1;
                repair = true;
            }
            EntityKind::Dimension(dm) if !dimstyles.contains(&dm.style.to_ascii_uppercase()) => {
                f.bad_dimstyle += 1;
                repair = true;
            }
            EntityKind::Insert(i) if !blocks.contains(&i.block.to_ascii_uppercase()) => {
                f.missing_block += 1;
                erase = true;
            }
            EntityKind::LwPolyline(p) if p.vertices.len() < 2 => {
                f.invalid += 1;
                erase = true;
            }
            EntityKind::Hatch(h) if h.loops.iter().all(|l| l.vertices.len() < 2) => {
                f.invalid += 1;
                erase = true;
            }
            _ => {}
        }
        if !erase && !e.kind.grips().iter().all(|p| p.is_finite()) {
            f.invalid += 1;
            erase = true;
        }
        (repair, erase)
    };
    let repair = |e: &mut Entity| {
        if !lt_ok(&e.common.linetype) {
            e.common.linetype = "ByLayer".into();
        }
        match &mut e.kind {
            EntityKind::Text(t) | EntityKind::AttDef(cadcraft_doc::Attrib { text: t, .. }) if !styles.contains(&t.style.to_ascii_uppercase()) => {
                t.style = "Standard".into()
            }
            EntityKind::MText(t) if !styles.contains(&t.style.to_ascii_uppercase()) => t.style = "Standard".into(),
            EntityKind::Dimension(dm) if !dimstyles.contains(&dm.style.to_ascii_uppercase()) => dm.style = "Standard".into(),
            _ => {}
        }
    };
    let fix_store = |st: &mut cadcraft_doc::EntityStore, f: &mut Found| {
        let mut erase: Vec<Handle> = Vec::new();
        let mut fix_list: Vec<Handle> = Vec::new();
        for e in st.iter() {
            let (r, x) = check(e, f);
            if x {
                erase.push(e.handle);
            } else if r {
                fix_list.push(e.handle);
            }
        }
        if fix {
            for h in erase {
                st.remove(h);
            }
            for h in fix_list {
                st.modify(h, repair);
            }
        }
    };
    fix_store(&mut d.model, &mut f);
    for l in &mut d.layouts {
        fix_store(&mut l.entities, &mut f);
    }
    // Blocks: also references to themselves (directly or through other blocks).
    let names: Vec<String> = d.blocks.keys().cloned().collect();
    let mut self_refs = 0;
    for n in &names {
        let reaches_self = {
            let mut stack: Vec<String> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let start = d.blocks.get(n);
            if let Some(b) = start {
                for e in b.entities.iter() {
                    if let EntityKind::Insert(i) = &e.kind {
                        stack.push(i.block.to_ascii_uppercase());
                    }
                }
            }
            let mut found = false;
            while let Some(x) = stack.pop() {
                if x == n.to_ascii_uppercase() {
                    found = true;
                    break;
                }
                if seen.len() > 100_000 || !seen.insert(x.clone()) {
                    continue;
                }
                if let Some(b) = d.blocks.iter().find(|(k, _)| k.eq_ignore_ascii_case(&x)).map(|(_, b)| b) {
                    for e in b.entities.iter() {
                        if let EntityKind::Insert(i) = &e.kind {
                            stack.push(i.block.to_ascii_uppercase());
                        }
                    }
                }
            }
            found
        };
        let Some(b) = d.blocks.get_mut(n) else { continue };
        if b.xref_path.is_some() {
            continue;
        }
        let mut errs = 0;
        let mut bad: Vec<Handle> = Vec::new();
        for e in b.entities.iter() {
            let (r, x) = check(e, &mut f);
            let cyclic = reaches_self && matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(n));
            if cyclic {
                self_refs += 1;
            }
            if x || r || cyclic {
                errs += 1;
                bad.push(e.handle);
            }
        }
        if fix && errs > 0 {
            let st = &mut Arc::make_mut(b).entities;
            for h in bad {
                let Some(e) = st.get(h).map(|e| (**e).clone()) else { continue };
                let (_, x) = check(&e, &mut Found::default());
                let cyclic = matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(n));
                if x || (cyclic && reaches_self) {
                    st.remove(h);
                } else {
                    st.modify(h, repair);
                }
            }
        }
    }
    count(&mut report, f.missing_layers.len(), &format!("layer không tồn tại ({})", f.missing_layers.join(", ")));
    count(&mut report, f.bad_lt, "đối tượng dùng kiểu đường không tồn tại → ByLayer");
    count(&mut report, f.bad_style, "chữ dùng kiểu chữ không tồn tại → Standard");
    count(&mut report, f.bad_dimstyle, "kích thước dùng kiểu kích thước không tồn tại → Standard");
    count(&mut report, f.missing_block, "block tham chiếu tới block không tồn tại → xóa");
    count(&mut report, f.invalid, "đối tượng có tọa độ hỏng hoặc rỗng → xóa");
    count(&mut report, self_refs, "block tự chứa chính nó → bỏ tham chiếu vòng");
    if fix {
        for l in &f.missing_layers {
            d.ensure_layer(l);
        }
    }
    // Drawing settings.
    let cl = d.header.str("CLAYER", "0");
    if d.layer(&cl).is_none() {
        report.push(format!("layer hiện hành {cl} không tồn tại → 0"));
        if fix {
            d.header.set_str("CLAYER", "0");
        }
    }
    if d.layer("0").is_none() {
        report.push("thiếu layer 0".into());
        if fix {
            d.layers.insert(0, Layer::default());
        }
    }
    let ts = d.header.str("TEXTSTYLE", "Standard");
    if !styles.contains(&ts.to_ascii_uppercase()) && !ts.eq_ignore_ascii_case("Standard") {
        report.push(format!("kiểu chữ hiện hành {ts} không tồn tại → Standard"));
        if fix {
            d.header.set_str("TEXTSTYLE", "Standard");
        }
    }
    // Links to erased objects.
    let alive = |h: &Handle, d: &Drawing| d.entity(*h).is_some();
    let dead_members: usize = d.groups.iter().map(|g| g.members.iter().filter(|m| !alive(m, d)).count()).sum();
    if dead_members > 0 {
        report.push(format!("{dead_members} thành viên nhóm đã bị xóa"));
        if fix {
            d.prune_groups();
        }
    }
    let dead_fields = d.fields.iter().filter(|f| d.entity(f.text).is_none()).count();
    if dead_fields > 0 {
        report.push(format!("{dead_fields} trường (field) mất chữ hiển thị"));
        if fix {
            let keep: Vec<_> = d.fields.iter().filter(|f| d.entity(f.text).is_some()).cloned().collect();
            d.fields = keep;
        }
    }
    report
}

fn summary(report: &[String], fix: bool) -> String {
    if report.is_empty() {
        "AUDIT: không phát hiện lỗi.".into()
    } else {
        format!(
            "AUDIT: {} loại lỗi{}:\n  {}",
            report.len(),
            if fix { " (đã sửa)" } else { " (chưa sửa — chạy lại và chọn Yes để sửa)" },
            report.join("\n  ")
        )
    }
}

fn run_audit(s: &mut Session, p: &Value) -> Result<Value> {
    let fix = bool_or(p, "fix", true);
    let report = if fix {
        audit(s.doc_mut()?, true)
    } else {
        let mut copy = s.doc()?.clone();
        audit(&mut copy, false)
    };
    Ok(json!({ "errors": report.len(), "fixed": fix, "report": report, "message": summary(&report, fix) }))
}

fn plan_audit(_s: &Session, a: &[Ans]) -> Option<Ask> {
    a.is_empty().then(|| Ask::Kw { msg: "Sửa các lỗi tìm thấy?".into(), kws: vec!["Yes", "No"], default: Some("Yes") })
}

fn flow_audit(s: &mut Session, a: &[Ans]) -> Result<Value> {
    run_audit(s, &json!({ "fix": a.first().map(Ans::text) != Some("No") }))
}

fn run_recover(s: &mut Session, p: &Value) -> Result<Value> {
    let r = s.execute("open", p)?;
    let rep = audit(s.doc_mut()?, true);
    if let Ok(st) = s.state_mut() {
        st.revision += 1;
    }
    Ok(json!({ "open": r, "errors": rep.len(), "report": rep, "message": format!("RECOVER — {}", summary(&rep, true)) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_finds_and_fixes_broken_references() {
        let mut s = Session::new();
        let r = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        let h = Handle::parse_hex(r["handle"].as_str().or(r["handles"][0].as_str()).unwrap()).unwrap();
        let r = s.execute("text", &json!({ "at": [0, 5], "height": 2.5, "text": "Mố" })).unwrap();
        let t = Handle::parse_hex(r["handle"].as_str().or(r["handles"][0].as_str()).unwrap()).unwrap();
        {
            let d = s.doc_mut().unwrap();
            d.modify_entity(h, |e| {
                e.common.layer = "MAT".into();
                e.common.linetype = "KHONGCO".into();
            })
            .unwrap();
            d.modify_entity(t, |e| {
                if let EntityKind::Text(x) = &mut e.kind {
                    x.style = "VNTIME_THIEU".into();
                }
            })
            .unwrap();
            let bad: cadcraft_doc::Insert = serde_json::from_value(json!({ "block": "KHONG_CO", "insert": {"x":0.0,"y":0.0,"z":0.0} })).unwrap();
            d.add(&cadcraft_doc::Space::Model, Default::default(), EntityKind::Insert(bad)).unwrap();
            // A block that inserts itself.
            let mut b = cadcraft_doc::Block::new("VONG");
            let me: cadcraft_doc::Insert = serde_json::from_value(json!({ "block": "VONG", "insert": {"x":1.0,"y":0.0,"z":0.0} })).unwrap();
            b.entities.push(Entity { handle: Handle(0x8888), common: Default::default(), kind: EntityKind::Insert(me) });
            d.blocks.insert("VONG".into(), Arc::new(b));
            d.header.set_str("CLAYER", "MAT2");
        }
        let r = s.execute("audit", &json!({ "fix": false })).unwrap();
        assert!(r["errors"].as_u64().unwrap() >= 6, "{r}");
        assert_eq!(s.doc().unwrap().model.len(), 3, "nothing changed without fix");
        s.cmdline("AUDIT").unwrap();
        s.cmdline("Y").unwrap();
        let d = s.doc().unwrap();
        assert!(d.layer("MAT").is_some());
        assert_eq!(d.model.len(), 2);
        assert_eq!(d.entity(h).unwrap().common.linetype, "ByLayer");
        assert!(d.block("VONG").unwrap().entities.is_empty());
        assert_eq!(d.header.str("CLAYER", ""), "0");
        let r = s.execute("audit", &json!({})).unwrap();
        assert_eq!(r["errors"], 0, "{r}");
    }
}
