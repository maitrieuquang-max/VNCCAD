//! VNCCad external references (xrefs): another drawing shown inside this one, read from its
//! file every time the host opens, never copied into the host.
//!
//! An xref is a block with `xref_path` set. Resolving it reads the referenced drawing and fills
//! the block with its model space; the referenced drawing's layers, blocks, text and dimension
//! styles come along renamed "XREFNAME|NAME" so they never clash with the host's. Those
//! dependent names are not saved (see the DXF writer); the reference itself is.
//!
//! The referenced file is found, in order, among the drawings open in this session (by file
//! name: the way to attach on the web), next to the host drawing, at its stored path, or among
//! files given to the app by name.

use std::sync::Arc;

use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Insert};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("xattach", "External Reference...", run_xattach)
            .menu(&["Insert", "DWG Reference..."])
            .alias(&["xa", "gantham chieu"])
            .params("{path | name (an open drawing's file name), insert?: [x,y], scale?: number, rotation?: degrees}"),
        CommandSpec::new("xref", "External References", run_xref)
            .menu(&["Insert", "External References"])
            .alias(&["xr", "er", "externalreferences"])
            .params("{action?: \"list\" | \"reload\" | \"detach\" | \"bind\", name?: xref (all for list/reload)}"),
    ]
}

/// Base folder of the active drawing, if it was opened from disk.
fn base_dir(s: &Session) -> Option<std::path::PathBuf> {
    let p = s.state().ok()?.path.clone()?;
    std::path::Path::new(&p).parent().map(std::path::Path::to_path_buf)
}

fn stem(path: &str) -> String {
    let f = path.trim().rsplit(['/', '\\']).next().unwrap_or(path);
    f.rsplit_once('.').map_or(f, |(a, _)| a).to_string()
}

fn file_key(path: &str) -> String {
    path.trim().rsplit(['/', '\\']).next().unwrap_or(path).to_lowercase()
}

/// Find and read the referenced drawing.
fn load(path: &str, base: Option<&std::path::Path>, open: &[(String, Arc<Drawing>)]) -> Option<Drawing> {
    let key = file_key(path);
    if let Some((_, d)) = open.iter().find(|(t, _)| t.to_lowercase() == key) {
        return Some(d.as_ref().clone());
    }
    let hooks = file::io()?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let p = std::path::Path::new(path.trim());
        let mut candidates = Vec::new();
        if let Some(b) = base {
            if !p.is_absolute() {
                candidates.push(b.join(p));
            }
            if let Some(n) = p.file_name() {
                candidates.push(b.join(n));
            }
        }
        if p.is_absolute() {
            candidates.push(p.to_path_buf());
        }
        for c in candidates {
            if let Ok(bytes) = std::fs::read(&c) {
                let name = c.to_string_lossy().to_string();
                return (hooks.read)(&bytes, &name).ok();
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = base;
    let bytes = cadcraft_io::raster::bytes(path)?;
    (hooks.read)(&bytes, path).ok()
}

fn pre(prefix: &str, name: &str) -> String {
    format!("{prefix}|{name}")
}

fn remap(e: &mut Entity, prefix: &str) {
    if e.common.layer != "0" && !e.common.layer.is_empty() {
        e.common.layer = pre(prefix, &e.common.layer);
    }
    match &mut e.kind {
        EntityKind::Insert(i) => {
            i.block = pre(prefix, &i.block);
            for a in &mut i.attribs {
                a.text.style = pre(prefix, &a.text.style);
            }
        }
        EntityKind::Text(t) => t.style = pre(prefix, &t.style),
        EntityKind::MText(m) => m.style = pre(prefix, &m.style),
        EntityKind::AttDef(a) => a.text.style = pre(prefix, &a.text.style),
        EntityKind::Dimension(d) => {
            d.style = pre(prefix, &d.style);
            if let Some(b) = d.block.as_mut() {
                *b = pre(prefix, b);
            }
        }
        _ => {}
    }
}

/// Remove everything a previous load of xref `name` brought in.
fn clear_dependents(d: &mut Drawing, name: &str) {
    let p = format!("{}|", name.to_lowercase());
    let dep = |n: &str| n.to_lowercase().starts_with(&p);
    d.blocks.retain(|k, _| !dep(k));
    d.layers.retain(|l| !dep(&l.name));
    d.text_styles.retain(|s| !dep(&s.name));
    d.dim_styles.retain(|s| !dep(&s.name));
}

/// Fill xref block `name` from drawing `xd`. Returns the number of entities brought in.
fn merge(d: &mut Drawing, name: &str, xd: &Drawing) -> usize {
    clear_dependents(d, name);
    for l in &xd.layers {
        if l.name != "0" && !l.name.contains('|') {
            let mut l = l.clone();
            l.name = pre(name, &l.name);
            d.layers.push(l);
        }
    }
    for s in &xd.text_styles {
        let mut s = s.clone();
        s.name = pre(name, &s.name);
        d.text_styles.push(s);
    }
    for s in &xd.dim_styles {
        let mut s = s.clone();
        s.name = pre(name, &s.name);
        d.dim_styles.push(s);
    }
    for (bn, b) in &xd.blocks {
        if b.xref_path.is_some() {
            continue; // nested references are not followed
        }
        let mut nb = Block::new(&pre(name, bn));
        nb.base = b.base;
        nb.anonymous = b.anonymous;
        for e in b.entities.iter() {
            let mut e = e.as_ref().clone();
            e.handle = d.new_handle();
            remap(&mut e, name);
            nb.entities.push(e);
        }
        d.blocks.insert(nb.name.clone(), Arc::new(nb));
    }
    let mut count = 0;
    let Some(xb) = d.blocks.get(name).cloned() else { return 0 };
    let mut xb = xb.as_ref().clone();
    xb.entities = cadcraft_doc::EntityStore::new();
    for e in xd.model.iter() {
        let mut e = e.as_ref().clone();
        e.handle = d.new_handle();
        remap(&mut e, name);
        xb.entities.push(e);
        count += 1;
    }
    d.blocks.insert(name.to_string(), Arc::new(xb));
    count
}

/// Resolve every xref of `d`. Returns (loaded names, missing names).
pub fn resolve_all(d: &mut Drawing, base: Option<&std::path::Path>, open: &[(String, Arc<Drawing>)]) -> (Vec<String>, Vec<String>) {
    let refs: Vec<(String, String)> = d.blocks.values().filter_map(|b| b.xref_path.clone().map(|p| (b.name.clone(), p))).collect();
    let (mut ok, mut missing) = (Vec::new(), Vec::new());
    for (name, path) in refs {
        match load(&path, base, open) {
            Some(mut xd) => {
                cadcraft_doc::vnlegacy::convert_drawing(&mut xd);
                merge(d, &name, &xd);
                ok.push(name);
            }
            None => missing.push(name),
        }
    }
    (ok, missing)
}

fn open_docs(s: &Session) -> Vec<(String, Arc<Drawing>)> {
    s.docs.iter().enumerate().filter(|(i, _)| *i != s.active).map(|(_, st)| (st.title.clone(), st.doc.clone())).collect()
}

fn run_xattach(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").or_else(|| str_param(p, "name")).ok_or_else(|| bad("xattach", "`path` or `name` is required"))?.to_string();
    let name = stem(&path);
    if name.is_empty() || name.contains('|') {
        return Err(bad("xattach", "tên tham chiếu không hợp lệ"));
    }
    let base = base_dir(s);
    let open = open_docs(s);
    let xd = load(&path, base.as_deref(), &open)
        .ok_or_else(|| bad("xattach", format!("không tìm thấy bản vẽ {path} (mở nó trong một tab khác hoặc đặt cạnh bản vẽ này)")))?;
    let insert = p
        .get("insert")
        .and_then(Value::as_array)
        .map(|a| Vec3::new(a.first().and_then(Value::as_f64).unwrap_or(0.0), a.get(1).and_then(Value::as_f64).unwrap_or(0.0), 0.0));
    let scale = p.get("scale").and_then(Value::as_f64).filter(|x| x.is_finite() && *x != 0.0).unwrap_or(1.0);
    let rotation = p.get("rotation").and_then(Value::as_f64).unwrap_or(0.0).to_radians();
    let space = s.space();
    let layer = s.doc()?.header.str("CLAYER", "0");
    let d = s.doc_mut()?;
    match d.blocks.get(&name) {
        Some(b) if b.xref_path.is_none() => return Err(bad("xattach", format!("đã có block thường tên {name}"))),
        Some(_) => {}
        None => {
            let mut b = Block::new(&name);
            b.anonymous = false;
            b.xref_path = Some(path.clone());
            d.blocks.insert(name.clone(), Arc::new(b));
        }
    }
    let mut xd = xd;
    cadcraft_doc::vnlegacy::convert_drawing(&mut xd);
    let n = merge(d, &name, &xd);
    let ins = Insert {
        block: name.clone(),
        insert: insert.unwrap_or(Vec3::ZERO),
        scale: Vec3::new(scale, scale, scale),
        rotation,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let h = d.add(&space, Common { layer, ..Common::default() }, EntityKind::Insert(ins)).map_err(|e| bad("xattach", e.to_string()))?;
    let msg = format!("Đã gắn tham chiếu {name} ({n} đối tượng).");
    Ok(json!({ "handle": h.hex(), "name": name, "entities": n, "message": msg }))
}

fn list(d: &Drawing) -> Vec<Value> {
    d.blocks
        .values()
        .filter_map(|b| {
            let path = b.xref_path.clone()?;
            let n = b.entities.iter().count();
            Some(json!({ "name": b.name, "path": path, "loaded": n > 0, "entities": n }))
        })
        .collect()
}

/// Give dependent names of xref `name` ordinary names ("X|Layer" → "X$0$Layer").
fn bind(d: &mut Drawing, name: &str) -> Result<()> {
    let p = format!("{}|", name.to_lowercase());
    let fix = |n: &str| if n.to_lowercase().starts_with(&p) { n.replacen('|', "$0$", 1) } else { n.to_string() };
    let fix_entity = |e: &mut Entity| {
        e.common.layer = fix(&e.common.layer);
        match &mut e.kind {
            EntityKind::Insert(i) => {
                i.block = fix(&i.block);
                for a in &mut i.attribs {
                    a.text.style = fix(&a.text.style);
                }
            }
            EntityKind::Text(t) => t.style = fix(&t.style),
            EntityKind::MText(m) => m.style = fix(&m.style),
            EntityKind::AttDef(a) => a.text.style = fix(&a.text.style),
            EntityKind::Dimension(dm) => {
                dm.style = fix(&dm.style);
                if let Some(b) = dm.block.as_mut() {
                    *b = fix(b);
                }
            }
            _ => {}
        }
    };
    let names: Vec<String> = d.blocks.keys().filter(|k| k.to_lowercase().starts_with(&p) || k.eq_ignore_ascii_case(name)).cloned().collect();
    for k in names {
        let Some(b) = d.blocks.remove(&k) else { continue };
        let mut b = b.as_ref().clone();
        b.name = fix(&b.name);
        b.xref_path = None;
        let hs: Vec<_> = b.entities.iter().map(|e| e.handle).collect();
        for h in hs {
            b.entities.modify(h, |e| fix_entity(e));
        }
        d.blocks.insert(b.name.clone(), Arc::new(b));
    }
    for l in &mut d.layers {
        l.name = fix(&l.name);
    }
    for s in &mut d.text_styles {
        s.name = fix(&s.name);
    }
    for s in &mut d.dim_styles {
        s.name = fix(&s.name);
    }
    Ok(())
}

fn detach(d: &mut Drawing, name: &str) {
    clear_dependents(d, name);
    d.blocks.retain(|k, b| !(k.eq_ignore_ascii_case(name) && b.xref_path.is_some()));
    let mut stores: Vec<&mut cadcraft_doc::EntityStore> = vec![&mut d.model];
    for l in &mut d.layouts {
        stores.push(&mut l.entities);
    }
    for st in stores {
        let hs: Vec<_> =
            st.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name))).map(|e| e.handle).collect();
        for h in hs {
            st.remove(h);
        }
    }
}

fn run_xref(s: &mut Session, p: &Value) -> Result<Value> {
    let action = str_param(p, "action").unwrap_or("list").to_ascii_lowercase();
    let name = str_param(p, "name").map(str::to_string);
    let msg = match action.as_str() {
        "list" | "l" | "" => {
            let l = list(s.doc()?);
            let msg = if l.is_empty() {
                "Bản vẽ không có tham chiếu ngoài.".to_string()
            } else {
                let parts: Vec<String> = l
                    .iter()
                    .map(|x| format!("{} ({})", x["name"].as_str().unwrap_or(""), if x["loaded"] == true { "đã nạp" } else { "chưa tìm thấy" }))
                    .collect();
                format!("Tham chiếu: {}", parts.join(", "))
            };
            return Ok(json!({ "xrefs": l, "message": msg }));
        }
        "reload" | "r" => {
            let base = base_dir(s);
            let open = open_docs(s);
            let d = s.doc_mut()?;
            if let Some(n) = &name
                && !d.blocks.get(n).is_some_and(|b| b.xref_path.is_some())
            {
                return Err(bad("xref", format!("không có tham chiếu {n}")));
            }
            let (ok, missing) = resolve_all(d, base.as_deref(), &open);
            format!(
                "Đã nạp lại {} tham chiếu{}.",
                ok.len(),
                if missing.is_empty() { String::new() } else { format!("; chưa tìm thấy: {}", missing.join(", ")) }
            )
        }
        "detach" | "d" => {
            let n = name.ok_or_else(|| bad("xref", "`name` is required"))?;
            detach(s.doc_mut()?, &n);
            format!("Đã gỡ tham chiếu {n}.")
        }
        "bind" | "b" => {
            let n = name.ok_or_else(|| bad("xref", "`name` is required"))?;
            if !s.doc()?.blocks.get(&n).is_some_and(|b| b.xref_path.is_some()) {
                return Err(bad("xref", format!("không có tham chiếu {n}")));
            }
            bind(s.doc_mut()?, &n)?;
            format!("Đã nhập hẳn tham chiếu {n} vào bản vẽ.")
        }
        other => return Err(bad("xref", format!("unknown action `{other}` (list, reload, detach, bind)"))),
    };
    Ok(json!({ "message": msg }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_io() {
        file::set_io(file::IoHooks {
            read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
            plot: None,
        });
    }

    #[test]
    fn attach_from_open_drawing_reload_bind_detach() {
        install_io();
        let mut s = Session::new();
        // The referenced drawing, open in another tab: a line on layer "TIM", a block, a text.
        {
            let d = s.doc_mut().unwrap();
            let line = EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(100.0, 0.0, 0.0) });
            d.add(&cadcraft_doc::Space::Model, Common { layer: "TIM".into(), ..Common::default() }, line).unwrap();
        }
        s.cmdline("text 0,10 5 0 Trục tim tuyến").unwrap();
        s.cmdline("").unwrap();
        if let Ok(st) = s.state_mut() {
            st.title = "binh-do.dxf".into();
        }
        s.new_drawing(true);
        let r = s.execute("xattach", &json!({"name": "binh-do.dxf", "insert": [1000, 2000]})).unwrap();
        assert!(r["entities"].as_u64().unwrap() >= 2);
        let d = s.doc().unwrap();
        assert!(d.layer("binh-do|TIM").is_some(), "dependent layer");
        let xb = d.block("binh-do").unwrap();
        assert!(xb.xref_path.is_some());
        assert!(xb.entities.iter().any(|e| e.common.layer == "binh-do|TIM"));
        // Saved as a reference: no dependent layer, a flag-4 block with its path.
        let dxf = String::from_utf8(cadcraft_io::write(d, "host.dxf").unwrap()).unwrap();
        assert!(!dxf.contains("binh-do|"), "no dependent layer, style, dim style or block is saved");
        assert!(dxf.contains("binh-do.dxf"));
        let back = cadcraft_io::read(dxf.as_bytes(), "host.dxf").unwrap();
        assert_eq!(back.block("binh-do").and_then(|b| b.xref_path.clone()).as_deref(), Some("binh-do.dxf"));
        // List and reload.
        let l = s.execute("xref", &json!({})).unwrap();
        assert_eq!(l["xrefs"].as_array().map(Vec::len), Some(1));
        s.execute("xref", &json!({"action": "reload"})).unwrap();
        // Bind: ordinary names, no longer a reference.
        s.execute("xref", &json!({"action": "bind", "name": "binh-do"})).unwrap();
        let d = s.doc().unwrap();
        assert!(d.layer("binh-do$0$TIM").is_some());
        assert!(d.block("binh-do").unwrap().xref_path.is_none());
        s.execute("undo", &json!({})).unwrap();
        s.execute("xref", &json!({"action": "detach", "name": "binh-do"})).unwrap();
        let d = s.doc().unwrap();
        assert!(d.block("binh-do").is_none());
        assert!(d.layer("binh-do|TIM").is_none());
        assert!(!d.model.iter().any(|e| matches!(&e.kind, EntityKind::Insert(_))));
    }
}
