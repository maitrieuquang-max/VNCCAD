//! VNCCad: bringing objects from one drawing into another with what they need — layers,
//! linetypes, text and dimension styles, block definitions (nested ones too) — as AutoCAD does
//! for copy/paste between drawings and for inserting a drawing file as a block.

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_doc::{Block, Drawing, Entity, EntityKind, Handle};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("blockfromfile", "Insert Drawing File as Block", run_blockfromfile)
            .menu(&["Insert", "Block from File..."])
            .alias(&["chenfile", "insertfile"])
            .params("{path | data (base64) + name, name?: block name (default: file name), redefine?: bool, at?: [x,y] (insert there), scale?, rotation?}")
            .enabled(has_doc),
    ]
}

/// Names an entity needs from its drawing.
#[derive(Default)]
struct Needs {
    layers: Vec<String>,
    linetypes: Vec<String>,
    styles: Vec<String>,
    dimstyles: Vec<String>,
    blocks: Vec<String>,
}

fn push(v: &mut Vec<String>, n: &str) {
    if !n.is_empty() && !v.iter().any(|x| x.eq_ignore_ascii_case(n)) {
        v.push(n.to_string());
    }
}

fn needs_of(e: &Entity, n: &mut Needs) {
    push(&mut n.layers, &e.common.layer);
    let lt = e.common.linetype.to_ascii_uppercase();
    if !matches!(lt.as_str(), "BYLAYER" | "BYBLOCK" | "CONTINUOUS" | "") {
        push(&mut n.linetypes, &e.common.linetype);
    }
    match &e.kind {
        EntityKind::Text(t) => push(&mut n.styles, &t.style),
        EntityKind::MText(t) => push(&mut n.styles, &t.style),
        EntityKind::AttDef(a) => push(&mut n.styles, &a.text.style),
        EntityKind::Insert(i) => {
            push(&mut n.blocks, &i.block);
            for a in &i.attribs {
                push(&mut n.styles, &a.text.style);
            }
        }
        EntityKind::Dimension(d) => {
            push(&mut n.dimstyles, &d.style);
            if let Some(b) = &d.block {
                push(&mut n.blocks, b);
            }
        }
        _ => {}
    }
}

/// Copy into `dst` the definitions `ents` (objects of `src`) need and `dst` lacks. Existing
/// definitions of the same name are kept (as AutoCAD does). Block contents get new handles.
pub fn import_defs(dst: &mut Drawing, src: &Drawing, ents: &[Entity]) {
    let mut n = Needs::default();
    for e in ents {
        needs_of(e, &mut n);
    }
    // Blocks, nested ones too.
    let mut i = 0;
    while let Some(name) = n.blocks.get(i).cloned() {
        i += 1;
        if i > 100_000 {
            break;
        }
        if let Some(b) = src.block(&name) {
            for e in b.entities.iter() {
                needs_of(e, &mut n);
            }
            // A dynamic block reference also needs its definition.
            if let Some(r) = &b.dyn_ref {
                push(&mut n.blocks, &r.source);
            }
        }
    }
    for l in &n.layers {
        if dst.layer(l).is_none() {
            if let Some(sl) = src.layer(l) {
                push(&mut n.linetypes, &sl.linetype.clone());
                dst.layers.push(sl.clone());
            } else {
                dst.ensure_layer(l);
            }
        }
    }
    for lt in &n.linetypes {
        if dst.linetype(lt).is_none()
            && let Some(x) = src.linetype(lt)
        {
            dst.linetypes.push(x.clone());
        }
    }
    for ds in &n.dimstyles {
        if dst.dim_style(ds).is_none()
            && let Some(x) = src.dim_style(ds)
        {
            push(&mut n.styles, &x.text_style.clone());
            dst.dim_styles.push(x.clone());
        }
    }
    for st in &n.styles {
        if dst.text_style(st).is_none()
            && let Some(x) = src.text_style(st)
        {
            dst.text_styles.push(x.clone());
        }
    }
    for name in &n.blocks {
        if dst.block(name).is_some() {
            continue;
        }
        let Some(b) = src.block(name) else { continue };
        let nb = renumbered(dst, b);
        dst.blocks.insert(nb.name.clone(), Arc::new(nb));
    }
}

/// A copy of a block with new entity handles (and its dynamic data following them).
fn renumbered(dst: &mut Drawing, b: &Block) -> Block {
    let mut map: HashMap<Handle, Handle> = HashMap::new();
    let mut out = Block::new(&b.name);
    out.base = b.base;
    out.description = b.description.clone();
    out.anonymous = b.anonymous;
    out.explodable = b.explodable;
    out.units = b.units;
    out.xref_path = b.xref_path.clone();
    out.dyn_ref = b.dyn_ref.clone();
    out.array = b.array.clone();
    for e in b.entities.iter() {
        let mut c = (**e).clone();
        let h = dst.new_handle();
        map.insert(c.handle, h);
        c.handle = h;
        out.entities.push(c);
    }
    let m = |h: &mut Handle| {
        if let Some(n) = map.get(h) {
            *h = *n;
        }
    };
    if let Some(mut dd) = b.dyn_def.clone() {
        for p in &mut dd.params {
            p.controlled.iter_mut().for_each(m);
            for st in &mut p.states {
                st.visible.iter_mut().for_each(m);
            }
            for a in &mut p.actions {
                a.entities.iter_mut().for_each(m);
            }
        }
        out.dyn_def = Some(dd);
    }
    out
}

/// Make a block named `name` from the model space of `src` (its base point: INSBASE).
pub fn block_from_drawing(dst: &mut Drawing, name: &str, src: &Drawing, redefine: bool) -> Result<String> {
    if name.trim().is_empty() || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`']) {
        return Err(bad("blockfromfile", format!("tên block không hợp lệ: {name}")));
    }
    if dst.block(name).is_some() && !redefine {
        return Ok(name.to_string());
    }
    let ents: Vec<Entity> = src.model.iter().map(|e| (**e).clone()).collect();
    import_defs(dst, src, &ents);
    let mut b = Block::new(name);
    b.anonymous = false;
    b.base = src.header.point("INSBASE").unwrap_or_default();
    for mut e in ents {
        e.handle = dst.new_handle();
        b.entities.push(e);
    }
    dst.blocks.insert(name.to_string(), Arc::new(b));
    Ok(name.to_string())
}

fn stem(name: &str) -> String {
    let f = name.rsplit(['/', '\\']).next().unwrap_or(name);
    f.rsplit_once('.').map_or(f, |(a, _)| a).to_string()
}

/// Read a drawing file's bytes into a block of the active drawing.
pub fn block_from_bytes(s: &mut Session, bytes: &[u8], file: &str, name: Option<&str>, redefine: bool) -> Result<String> {
    let hooks = super::file::io().ok_or_else(|| bad("blockfromfile", "file formats are not available in this build"))?;
    let mut src = (hooks.read)(bytes, file).map_err(|e| bad("blockfromfile", e))?;
    cadcraft_doc::vnlegacy::convert_drawing(&mut src);
    let name = name.map(str::to_string).unwrap_or_else(|| stem(file));
    block_from_drawing(s.doc_mut()?, &name, &src, redefine)
}

fn run_blockfromfile(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, file) = match (str_param(p, "path"), str_param(p, "data")) {
        (Some(path), _) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                (std::fs::read(path).map_err(|e| bad("blockfromfile", format!("{path}: {e}")))?, path.to_string())
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = path;
                return Err(bad("blockfromfile", "paths are not available on the web; pass `data`"));
            }
        }
        (None, Some(data)) => (
            super::file::base64_decode(data).ok_or_else(|| bad("blockfromfile", "invalid base64"))?,
            str_param(p, "file").or(str_param(p, "name")).unwrap_or("Block.dxf").to_string(),
        ),
        _ => return Err(bad("blockfromfile", "`path` or `data` is required")),
    };
    let name = block_from_bytes(s, &bytes, &file, str_param(p, "block"), bool_or(p, "redefine", false))?;
    let mut out = json!({ "block": name });
    if let Some(at) = point_param(p, "at") {
        let h = super::blocks::insert(s, &name, at, f64_or(p, "scale", 1.0), f64_or(p, "rotation", 0.0).to_radians(), &serde_json::Map::new())?;
        out["handle"] = json!(h.hex());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cadcraft_engine_io_hooks() {
        super::super::file::set_io(super::super::file::IoHooks {
            read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
            write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
            plot: None,
        });
    }

    #[test]
    fn copy_paste_between_drawings_brings_blocks_and_styles() {
        let mut s = Session::new();
        let r = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        let l = r["handle"].as_str().or(r["handles"][0].as_str()).unwrap().to_string();
        s.execute("layer.new", &json!({ "name": "COC", "color": 3 })).unwrap();
        s.execute("laymch", &json!({ "handles": [l], "layer": "COC" })).unwrap();
        s.execute("block", &json!({ "name": "COC_D600", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
        let r = s.execute("insert", &json!({ "name": "COC_D600", "at": [5, 5] })).unwrap();
        let ins = r["handle"].as_str().unwrap().to_string();
        s.execute("copyclip", &json!({ "handles": [ins] })).unwrap();
        // Another drawing.
        s.execute("new", &json!({})).unwrap();
        s.execute("pasteclip", &json!({ "at": [100, 100] })).unwrap();
        let d = s.doc().unwrap();
        assert!(d.block("COC_D600").is_some(), "the block definition came along");
        assert!(d.layer("COC").is_some_and(|l| l.color == cadcraft_color::Color::Index(3)));
        // Insert a drawing file as a block.
        let mut src = Drawing::new_metric();
        src.add(
            &cadcraft_doc::Space::Model,
            Default::default(),
            EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0), b: cadcraft_geom::Vec3::new(5.0, 5.0, 0.0) }),
        )
        .unwrap();
        let name = block_from_drawing(s.doc_mut().unwrap(), "KHUNG_A3", &src, false).unwrap();
        assert_eq!(s.doc().unwrap().block(&name).unwrap().entities.len(), 1);
        // From the UI: the next file opened becomes a block and INSERT asks where.
        cadcraft_engine_io_hooks();
        let bytes = cadcraft_io::write(&src, "khung.dxf").unwrap();
        let docs = s.docs.len();
        s.insert_next_open = true;
        s.execute("open", &json!({ "data": super::super::file::base64_encode(&bytes), "name": "KHUNG_B.dxf" })).unwrap();
        assert_eq!(s.docs.len(), docs, "no new tab");
        assert!(s.doc().unwrap().block("KHUNG_B").is_some());
        assert!(s.current_prompt().unwrap().message.contains("insertion point"));
        s.input(crate::Input::Point(cadcraft_geom::Vec2::new(50.0, 50.0))).unwrap();
        s.input(crate::Input::Enter).unwrap();
        s.input(crate::Input::Enter).unwrap();
        assert!(s.running.is_none());
        assert!(s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "KHUNG_B")));
    }
}
