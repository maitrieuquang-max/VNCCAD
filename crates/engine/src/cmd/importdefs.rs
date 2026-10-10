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
        CommandSpec::new("adcenter.list", "DesignCenter Contents", run_adc_list)
            .params("{doc?: index of an open drawing (default: active)} → blocks, layers, linetypes, textStyles, dimStyles, mlineStyles")
            .noundo()
            .enabled(has_doc),
        CommandSpec::new("adcenter", "DesignCenter", |_, _| Ok(json!({ "message": "DesignCenter: bảng bên phải, thẻ Khối." })))
            .menu(&["Tools", "Palettes", "DesignCenter"])
            .alias(&["adc", "dc"])
            .noundo(),
        CommandSpec::new("toolpalettes", "Tool Palettes", |_, _| Ok(json!({ "message": "Bảng công cụ: bảng bên phải, thẻ Công cụ." })))
            .menu(&["Tools", "Palettes", "Tool Palettes"])
            .alias(&["tp", "bangcongcu"])
            .noundo(),
        CommandSpec::new("adcenter.importfile", "DesignCenter Add from File", run_adc_importfile)
            .params("{path | data (base64) + file, kind, name, insert?: bool (blocks: start INSERT)}")
            .enabled(has_doc),
        CommandSpec::new("adcenter.import", "DesignCenter Add", run_adc_import)
            .params("{doc: index of the source drawing, kind: block|layer|linetype|textStyle|dimStyle|mlineStyle, name}")
            .enabled(has_doc),
    ]
}

const KINDS: &[&str] = &["blocks", "layers", "linetypes", "textStyles", "dimStyles", "mlineStyles"];

fn names_of(d: &Drawing, kind: &str) -> Vec<String> {
    match kind {
        "blocks" | "block" => d.blocks.iter().filter(|(_, b)| !b.anonymous && b.xref_path.is_none()).map(|(k, _)| k.clone()).collect(),
        "layers" | "layer" => d.layers.iter().map(|l| l.name.clone()).collect(),
        "linetypes" | "linetype" => {
            d.linetypes.iter().filter(|l| !["byblock", "bylayer"].contains(&l.name.to_ascii_lowercase().as_str())).map(|l| l.name.clone()).collect()
        }
        "textStyles" | "textStyle" => d.text_styles.iter().map(|l| l.name.clone()).collect(),
        "dimStyles" | "dimStyle" => d.dim_styles.iter().map(|l| l.name.clone()).collect(),
        "mlineStyles" | "mlineStyle" => d.mline_styles.iter().map(|l| l.name.clone()).collect(),
        _ => Vec::new(),
    }
}

fn run_adc_list(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("doc").and_then(Value::as_u64).map_or(s.active, |i| i as usize);
    let st = s.docs.get(i).ok_or_else(|| bad("adcenter.list", "không có bản vẽ đó"))?;
    let d = st.doc.clone();
    let mut out = serde_json::Map::new();
    out.insert("title".into(), json!(st.title));
    for k in KINDS {
        out.insert((*k).into(), json!(names_of(&d, k)));
    }
    let docs: Vec<String> = s.docs.iter().map(|d| d.title.clone()).collect();
    out.insert("docs".into(), json!(docs));
    out.insert("active".into(), json!(s.active));
    Ok(Value::Object(out))
}

/// Copy one named definition of `src` into `dst` (with what it needs). Returns whether
/// anything was added (an existing definition is kept).
pub fn import_named(dst: &mut Drawing, src: &Drawing, kind: &str, name: &str) -> Result<bool> {
    let k = kind.trim_end_matches('s');
    let has = |d: &Drawing| names_of(d, k).iter().any(|n| n.eq_ignore_ascii_case(name));
    if !has(src) {
        return Err(bad("adcenter.import", format!("bản vẽ nguồn không có {name}")));
    }
    if has(dst) {
        return Ok(false);
    }
    let probe = |kind: EntityKind| Entity { handle: Handle(0), common: cadcraft_doc::Common::default(), kind };
    match k {
        "block" => {
            let e = probe(EntityKind::Insert(cadcraft_doc::Insert {
                block: name.to_string(),
                insert: Default::default(),
                scale: cadcraft_geom::Vec3::new(1.0, 1.0, 1.0),
                rotation: 0.0,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
                clip: None,
            }));
            import_defs(dst, src, &[e]);
        }
        "layer" => {
            let mut e = probe(EntityKind::Point(cadcraft_doc::Point { p: Default::default(), angle: 0.0 }));
            e.common.layer = name.to_string();
            import_defs(dst, src, &[e]);
        }
        "linetype" => {
            if let Some(x) = src.linetype(name) {
                dst.linetypes.push(x.clone());
            }
        }
        "textStyle" => {
            if let Some(x) = src.text_style(name) {
                dst.text_styles.push(x.clone());
            }
        }
        "dimStyle" => {
            let e = probe(EntityKind::Leader(cadcraft_doc::Leader { vertices: Vec::new(), arrow: false, spline: false, style: name.to_string() }));
            import_defs(dst, src, &[e]);
            if let Some(x) = src.dim_style(name)
                && dst.dim_style(name).is_none()
            {
                dst.dim_styles.push(x.clone());
            }
        }
        "mlineStyle" => {
            let e = probe(EntityKind::MLine(cadcraft_doc::MLine {
                style: name.to_string(),
                scale: 1.0,
                justification: 0,
                closed: false,
                no_start_caps: false,
                no_end_caps: false,
                vertices: Vec::new(),
            }));
            import_defs(dst, src, &[e]);
        }
        _ => return Err(bad("adcenter.import", format!("loại không rõ: {kind}"))),
    }
    Ok(has(dst))
}

/// Add a definition from a drawing file that is not open (tool palette blocks).
fn run_adc_importfile(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_param(p, "kind").unwrap_or("blocks").to_string();
    let name = str_param(p, "name").ok_or_else(|| bad("adcenter.importfile", "`name` is required"))?.to_string();
    let (bytes, file) = match (str_param(p, "path"), str_param(p, "data")) {
        (_, Some(data)) => (
            super::file::base64_decode(data).ok_or_else(|| bad("adcenter.importfile", "invalid base64"))?,
            str_param(p, "file").unwrap_or("x.dxf").to_string(),
        ),
        (Some(path), None) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                (std::fs::read(path).map_err(|e| bad("adcenter.importfile", format!("{path}: {e}")))?, path.to_string())
            }
            #[cfg(target_arch = "wasm32")]
            {
                return Err(bad("adcenter.importfile", format!("không đọc được {path} trên web")));
            }
        }
        _ => return Err(bad("adcenter.importfile", "`path` or `data` is required")),
    };
    let hooks = super::file::io().ok_or_else(|| bad("adcenter.importfile", "file formats are not available in this build"))?;
    let mut src = (hooks.read)(&bytes, &file).map_err(|e| bad("adcenter.importfile", e))?;
    cadcraft_doc::vnlegacy::convert_drawing(&mut src);
    let added = import_named(s.doc_mut()?, &src, &kind, &name)?;
    if let Ok(st) = s.state_mut() {
        st.revision += 1;
    }
    if bool_or(p, "insert", false) && kind.starts_with("block") {
        s.start("insert")?;
        s.input(crate::Input::Text(name.clone()))?;
    }
    Ok(json!({ "added": added }))
}

fn run_adc_import(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("doc").and_then(Value::as_u64).ok_or_else(|| bad("adcenter.import", "`doc` is required"))? as usize;
    let kind = str_param(p, "kind").ok_or_else(|| bad("adcenter.import", "`kind` is required"))?.to_string();
    let name = str_param(p, "name").ok_or_else(|| bad("adcenter.import", "`name` is required"))?.to_string();
    let src = s.docs.get(i).map(|d| d.doc.clone()).ok_or_else(|| bad("adcenter.import", "không có bản vẽ nguồn"))?;
    let added = if i == s.active { false } else { import_named(s.doc_mut()?, &src, &kind, &name)? };
    if let Ok(st) = s.state_mut() {
        st.revision += 1;
    }
    let msg = if added { format!("Đã thêm {name} vào bản vẽ.") } else { format!("{name} đã có trong bản vẽ.") };
    Ok(json!({ "added": added, "message": msg }))
}

/// Names an entity needs from its drawing.
#[derive(Default)]
struct Needs {
    layers: Vec<String>,
    linetypes: Vec<String>,
    styles: Vec<String>,
    dimstyles: Vec<String>,
    blocks: Vec<String>,
    mlstyles: Vec<String>,
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
        EntityKind::MLine(m) => push(&mut n.mlstyles, &m.style),
        EntityKind::Tolerance(t) => push(&mut n.dimstyles, &t.style),
        EntityKind::Leader(l) => push(&mut n.dimstyles, &l.style),
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
    for ms in &n.mlstyles {
        if dst.mline_style(ms).is_none()
            && let Some(x) = src.mline_style(ms)
        {
            for e in &x.elements {
                let lt = e.linetype.to_ascii_uppercase();
                if !matches!(lt.as_str(), "BYLAYER" | "BYBLOCK" | "CONTINUOUS" | "") {
                    push(&mut n.linetypes, &e.linetype);
                }
            }
            dst.mline_styles.push(x.clone());
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

    #[test]
    fn designcenter_brings_definitions_from_another_open_drawing() {
        let mut s = Session::new();
        let r = s.execute("line", &json!({ "points": [[0, 0], [600, 0]] })).unwrap();
        let l = r["handle"].as_str().or(r["handles"][0].as_str()).unwrap().to_string();
        s.execute("layer.new", &json!({ "name": "COC", "color": 3 })).unwrap();
        s.execute("laymch", &json!({ "handles": [l], "layer": "COC" })).unwrap();
        s.execute("block", &json!({ "name": "COC_D600", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
        s.execute("mlstyle", &json!({ "name": "TUONG220", "elements": [110, -110] })).unwrap();
        let src = s.active;
        s.execute("new", &json!({})).unwrap();
        let list = s.execute("adcenter.list", &json!({ "doc": src })).unwrap();
        assert!(list["blocks"].as_array().unwrap().iter().any(|b| b == "COC_D600"));
        assert_eq!(list["docs"].as_array().unwrap().len(), 2);
        let r = s.execute("adcenter.import", &json!({ "doc": src, "kind": "blocks", "name": "COC_D600" })).unwrap();
        assert_eq!(r["added"], true);
        let d = s.doc().unwrap();
        assert!(d.block("COC_D600").is_some() && d.layer("COC").is_some(), "the block brings its layer");
        s.execute("adcenter.import", &json!({ "doc": src, "kind": "mlineStyle", "name": "TUONG220" })).unwrap();
        assert!(s.doc().unwrap().mline_style("TUONG220").is_some());
        let r = s.execute("adcenter.import", &json!({ "doc": src, "kind": "layer", "name": "COC" })).unwrap();
        assert_eq!(r["added"], false, "already there");
        assert!(s.execute("adcenter.import", &json!({ "doc": src, "kind": "layer", "name": "KHONG_CO" })).is_err());
    }
}
