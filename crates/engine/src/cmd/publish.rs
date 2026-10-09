//! VNCCad: PUBLISH — several sheets in one multi-page PDF.
//!
//! Sheets are the drawing's layouts, or the title-block frames placed in model space (the usual
//! way drawings are made in Vietnam and China): every insert of the frame block becomes a page,
//! in reading order (top to bottom, then left to right). The frame block is given by name or
//! found automatically (the block inserted at least twice whose inserts are the largest).

use cadcraft_doc::{EntityKind, Space, entity_bounds};
use cadcraft_geom::Bounds2;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("publish", "Publish (multi-page PDF)", run_publish)
            .menu(&["File", "Publish (PDF nhiều trang)..."])
            .alias(&["inhangloat", "batchplot"])
            .params(
                "{path?, source?: \"layouts\" | \"frames\" | \"auto\" (default), frameBlock?: block name, layouts?: [names], windows?: [[[x1,y1],[x2,y2]], …], paper?: \"A3\", plotStyleTable?} → one PDF page per sheet",
            )
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("publish.sheets", "Publish: list sheets", run_sheets)
            .params("{source?, frameBlock?} → the sheets PUBLISH would plot")
            .enabled(has_doc)
            .noundo(),
    ]
}

/// Layouts with something on them besides their viewport frame.
fn used_layouts(s: &Session) -> Result<Vec<String>> {
    let d = s.doc()?;
    let mut ls: Vec<&cadcraft_doc::Layout> =
        d.layouts.iter().filter(|l| l.entities.iter().any(|e| !matches!(&e.kind, EntityKind::Viewport(v) if v.id == 1))).collect();
    ls.sort_by_key(|l| l.tab_order);
    Ok(ls.into_iter().map(|l| l.name.clone()).collect())
}

/// Frames: the bounds of every insert of `block` in model space, in reading order.
fn frames_of(s: &Session, block: &str) -> Result<Vec<Bounds2>> {
    let d = s.doc()?;
    let v: Vec<Bounds2> = d
        .model
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(block)))
        .filter(|e| d.is_visible(e))
        .map(|e| entity_bounds(d, e, 0))
        .filter(|b| !b.is_empty() && b.width() > 1e-9 && b.height() > 1e-9)
        .collect();
    // The same frame inserted twice on top of itself is one sheet.
    let mut uniq: Vec<Bounds2> = Vec::new();
    for b in v {
        let tol = (b.width() + b.height()) * 1e-6;
        if !uniq.iter().any(|u| u.min.near(b.min, tol) && u.max.near(b.max, tol)) {
            uniq.push(b);
        }
    }
    let mut v = uniq;
    // Reading order: rows from the top (frames whose centres are within half a frame height
    // share a row), then left to right.
    v.sort_by(|a, b| b.center().y.total_cmp(&a.center().y));
    let mut rows: Vec<Vec<Bounds2>> = Vec::new();
    for f in v {
        match rows.last_mut() {
            Some(r) if r.first().is_some_and(|x| (x.center().y - f.center().y).abs() < x.height().min(f.height()) / 2.0) => r.push(f),
            _ => rows.push(vec![f]),
        }
    }
    Ok(rows
        .into_iter()
        .flat_map(|mut r| {
            r.sort_by(|a, b| a.center().x.total_cmp(&b.center().x));
            r
        })
        .collect())
}

/// The title-block frame of the drawing: a block inserted at least twice in model space whose
/// inserts are the largest (and shaped like a sheet).
pub fn guess_frame_block(s: &Session) -> Result<Option<String>> {
    let d = s.doc()?;
    let mut by: std::collections::HashMap<String, (usize, f64)> = std::collections::HashMap::new();
    for e in d.model.iter() {
        let EntityKind::Insert(i) = &e.kind else { continue };
        if i.block.starts_with('*') {
            continue;
        }
        let b = entity_bounds(d, e, 0);
        if b.is_empty() {
            continue;
        }
        let ratio = b.width().max(b.height()) / b.width().min(b.height()).max(1e-12);
        if !(1.2..=4.5).contains(&ratio) {
            continue;
        }
        let slot = by.entry(i.block.clone()).or_insert((0, 0.0));
        slot.0 += 1;
        slot.1 += b.width() * b.height();
    }
    let ext = d.extents(&Space::Model);
    let total = (ext.width() * ext.height()).max(1e-12);
    // Frames repeat: prefer blocks inserted twice or more, by the area they cover together.
    let big: Vec<(String, (usize, f64))> = by.into_iter().filter(|(_, (n, area))| area / (*n as f64) > total * 1e-4).collect();
    let repeated = big.iter().filter(|(_, (n, _))| *n >= 2).max_by(|a, b| a.1.1.total_cmp(&b.1.1));
    Ok(repeated.or_else(|| big.iter().max_by(|a, b| a.1.1.total_cmp(&b.1.1))).map(|(k, _)| k.clone()))
}

enum Sheets {
    Layouts(Vec<String>),
    Windows(Vec<Bounds2>, String),
}

fn choose(s: &Session, p: &Value) -> Result<Sheets> {
    if let Some(ls) = p.get("layouts").and_then(Value::as_array) {
        let names: Vec<String> = ls.iter().filter_map(Value::as_str).map(str::to_string).collect();
        let d = s.doc()?;
        for n in &names {
            if d.layout(n).is_none() {
                return Err(bad("publish", format!("không có layout `{n}`")));
            }
        }
        return Ok(Sheets::Layouts(names));
    }
    if let Some(ws) = p.get("windows").and_then(Value::as_array) {
        let mut out = Vec::new();
        for w in ws {
            let b = super::layout::window_param(w).ok_or_else(|| bad("publish", "mỗi `windows` là [[x1,y1],[x2,y2]]"))?;
            out.push(Bounds2::new(cadcraft_geom::Vec2::new(b[0], b[1]), cadcraft_geom::Vec2::new(b[2], b[3])));
        }
        return Ok(Sheets::Windows(out, "cửa sổ".into()));
    }
    let source = str_param(p, "source").unwrap_or("auto").to_ascii_lowercase();
    let frame_block = || -> Result<Option<String>> {
        match str_param(p, "frameBlock") {
            Some(b) => {
                if s.doc()?.block(b).is_none() && !s.doc()?.blocks.keys().any(|k| k.eq_ignore_ascii_case(b)) {
                    return Err(bad("publish", format!("không có block khung tên `{b}`")));
                }
                Ok(Some(b.to_string()))
            }
            None => guess_frame_block(s),
        }
    };
    match source.as_str() {
        "layouts" => Ok(Sheets::Layouts(used_layouts(s)?)),
        "frames" => {
            let b = frame_block()?.ok_or_else(|| bad("publish", "không tìm thấy khung tên trong Model (cho `frameBlock`)"))?;
            Ok(Sheets::Windows(frames_of(s, &b)?, b))
        }
        _ => {
            let ls = used_layouts(s)?;
            if !ls.is_empty() {
                return Ok(Sheets::Layouts(ls));
            }
            match frame_block()? {
                Some(b) => {
                    let f = frames_of(s, &b)?;
                    if f.is_empty() { Ok(Sheets::Layouts(Vec::new())) } else { Ok(Sheets::Windows(f, b)) }
                }
                None => Ok(Sheets::Layouts(Vec::new())),
            }
        }
    }
}

fn run_sheets(s: &mut Session, p: &Value) -> Result<Value> {
    Ok(match choose(s, p)? {
        Sheets::Layouts(ls) => json!({ "kind": "layouts", "sheets": ls, "message": format!("{} layout", ls.len()) }),
        Sheets::Windows(ws, b) => json!({
            "kind": "frames", "frameBlock": b,
            "sheets": ws.iter().map(|w| json!([[w.min.x, w.min.y], [w.max.x, w.max.y]])).collect::<Vec<_>>(),
            "message": format!("{} tờ theo khung tên {b}", ws.len())
        }),
    })
}

fn run_publish(s: &mut Session, p: &Value) -> Result<Value> {
    let sheets = choose(s, p)?;
    let base = || {
        let mut o = cadcraft_io::PdfOptions::from_json(p);
        o.compress = p.get("compress").and_then(Value::as_bool).unwrap_or(true);
        o
    };
    let (list, what): (Vec<(Space, cadcraft_io::PdfOptions)>, String) = match sheets {
        Sheets::Layouts(ls) if !ls.is_empty() => {
            let n = ls.len();
            (ls.into_iter().map(|l| (Space::Paper(l.clone()), cadcraft_io::PdfOptions { title: l, ..base() })).collect(), format!("{n} layout"))
        }
        Sheets::Layouts(_) => (vec![(Space::Model, base())], "Model (toàn bộ)".to_string()),
        Sheets::Windows(ws, b) => {
            let n = ws.len();
            let paper = str_param(p, "paper").map(str::to_string).or_else(|| Some("A3".into()));
            (
                ws.into_iter()
                    .enumerate()
                    .map(|(i, w)| {
                        (Space::Model, cadcraft_io::PdfOptions { window: Some(w), paper: paper.clone(), title: format!("Tờ {}", i + 1), ..base() })
                    })
                    .collect(),
                format!("{n} tờ theo khung tên {b}"),
            )
        }
    };
    let title = s.state().map(|st| st.title.clone()).unwrap_or_default();
    let compress = p.get("compress").and_then(Value::as_bool).unwrap_or(true);
    let bytes = cadcraft_io::pdf::publish(s.doc()?, &list, compress, &title).map_err(|e| bad("publish", e.to_string()))?;
    let pages = list.len();
    let msg = format!("Đã in {what} ra PDF ({pages} trang).");
    match str_param(p, "path") {
        Some(_path) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let path = _path;
                std::fs::write(path, &bytes).map_err(|e| bad("publish", format!("{path}: {e}")))?;
                Ok(json!({ "path": path, "pages": pages, "bytes": bytes.len(), "message": msg }))
            }
            #[cfg(target_arch = "wasm32")]
            Err(bad("publish", "paths are not available on the web; omit `path` to get the PDF as base64 `data`"))
        }
        None => Ok(json!({ "data": super::file::base64_encode(&bytes), "pages": pages, "bytes": bytes.len(), "message": msg })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_in_model_become_pages_in_reading_order() {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [420, 297] })).unwrap();
        let h = s.doc().unwrap().model.last().unwrap().handle;
        s.execute("block", &json!({ "name": "KHUNG-A3", "base": [0, 0], "handles": [h.hex()] })).unwrap();
        // Two rows of two sheets, inserted in a jumbled order.
        for at in [[500, 0], [0, 400], [0, 0], [500, 400]] {
            s.execute("insert", &json!({ "name": "KHUNG-A3", "at": at })).unwrap();
        }
        s.execute("circle", &json!({ "center": [100, 100], "radius": 20 })).unwrap();
        assert_eq!(guess_frame_block(&s).unwrap().as_deref(), Some("KHUNG-A3"));
        let l = s.execute("publish.sheets", &json!({})).unwrap();
        assert_eq!(l["kind"], "frames");
        let sheets = l["sheets"].as_array().unwrap();
        assert_eq!(sheets.len(), 4);
        // Top-left first, bottom-right last.
        assert_eq!(sheets[0][0], json!([0.0, 400.0]));
        assert_eq!(sheets[1][0], json!([500.0, 400.0]));
        assert_eq!(sheets[3][0], json!([500.0, 0.0]));
        let r = s.execute("publish", &json!({ "compress": false })).unwrap();
        assert_eq!(r["pages"], 4);
        let pdf = super::super::file::base64_decode(r["data"].as_str().unwrap()).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert_eq!(text.matches("/Type /Page ").count(), 4);
        assert!(text.contains("/Count 4"));
        // Explicit layouts / windows.
        assert!(s.execute("publish", &json!({ "layouts": ["Khong-co"] })).is_err());
        let r = s.execute("publish", &json!({ "windows": [[[0, 0], [420, 297]]] })).unwrap();
        assert_eq!(r["pages"], 1);
    }
}
