//! VNCCad: AutoCAD commands added for daily 2D drafting — draw order (DRAWORDER, TEXTTOFRONT,
//! HATCHTOBACK), SELECTSIMILAR, COPYTOLAYER, LAYMRG, LAYDEL, and text tools (SCALETEXT,
//! JUSTIFYTEXT, TEXTMASK).

use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, EntityKind, HAlign, Handle, VAlign, Wipeout};
use cadcraft_geom::{Bounds2, Mat3, Vec2};
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("draworder", "Draw Order", run_draworder)
            .menu(&["Tools", "Draw Order", "Draw Order"])
            .alias(&["dr", "thutuve"])
            .params("{handles?, order: front|back|above|under, ref?: [hex] (above/under)}")
            .interactive(|_| Ok(Box::new(Flow::new("draworder", plan_draworder, flow_draworder))))
            .enabled(has_doc),
        CommandSpec::new("texttofront", "Bring Text and Dimensions to Front", run_texttofront)
            .menu(&["Tools", "Draw Order", "Bring Text and Dimensions to Front"])
            .params("{what?: text|dimensions|leaders|all (default all)}")
            .interactive(|_| Ok(Box::new(Flow::new("texttofront", plan_texttofront, flow_texttofront))))
            .enabled(has_doc),
        CommandSpec::new("hatchtoback", "Send Hatches to Back", run_hatchtoback)
            .menu(&["Tools", "Draw Order", "Send Hatches to Back"])
            .params("{}")
            .enabled(has_doc),
        CommandSpec::new("selectsimilar", "Select Similar", run_selectsimilar)
            .menu(&["Edit", "Select Similar"])
            .alias(&["chongiong"])
            .params("{handles?} → selects the objects of the same type and layer (same block name, text or dimension style)")
            .interactive(|_| Ok(Box::new(Flow::new("selectsimilar", plan_select_only, flow_selectsimilar))))
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("copytolayer", "Copy to Layer", run_copytolayer)
            .menu(&["Format", "Layer Tools", "Copy Objects to New Layer"])
            .params("{handles?, layer, delta?: [dx, dy]}")
            .interactive(|_| Ok(Box::new(Flow::new("copytolayer", plan_copytolayer, flow_copytolayer))))
            .enabled(has_doc),
        CommandSpec::new("laymrg", "Layer Merge", run_laymrg)
            .menu(&["Format", "Layer Tools", "Layer Merge"])
            .alias(&["gioplayer"])
            .params("{layers?: [names] | handles?: objects on them, target: layer} → moves every object (blocks too) and deletes the merged layers")
            .interactive(|_| Ok(Box::new(Flow::new("laymrg", plan_laymrg, flow_laymrg))))
            .enabled(has_doc),
        CommandSpec::new("laydel", "Layer Delete", run_laydel)
            .menu(&["Format", "Layer Tools", "Layer Delete"])
            .alias(&["xoalayer"])
            .params("{layers?: [names] | handles?: objects on them} → deletes every object on them (blocks too) and the layers")
            .interactive(|_| Ok(Box::new(Flow::new("laydel", plan_laydel, flow_laydel))))
            .enabled(has_doc),
        CommandSpec::new("scaletext", "Scale Text", run_scaletext)
            .menu(&["Modify", "Text", "Scale"])
            .params("{handles?, height?: new height | factor?: scale factor} (each text about its own justification point)")
            .interactive(|_| Ok(Box::new(Flow::new("scaletext", plan_scaletext, flow_scaletext))))
            .enabled(has_doc),
        CommandSpec::new("justifytext", "Justify Text", run_justifytext)
            .menu(&["Modify", "Text", "Justify"])
            .alias(&["canhchu"])
            .params("{handles?, justify: L|C|R|M|TL|TC|TR|ML|MC|MR|BL|BC|BR} → changes the justification point without moving the text")
            .interactive(|_| Ok(Box::new(Flow::new("justifytext", plan_justifytext, flow_justifytext))))
            .enabled(has_doc),
        CommandSpec::new("textmask", "Text Mask", run_textmask)
            .menu(&["Express", "Text", "Text Mask"])
            .alias(&["nenchu"])
            .params("{handles?, offset?: × text height (default 0.35)} → a wipeout under each text")
            .interactive(|_| Ok(Box::new(Flow::new("textmask", plan_textmask, flow_textmask))))
            .enabled(has_doc),
    ]
}

fn hex(hs: &[Handle]) -> Value {
    json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>())
}

fn plan_select_only(_s: &Session, a: &[Ans]) -> Option<Ask> {
    a.is_empty().then(|| Ask::Select("Chọn đối tượng".into()))
}

// ------------------------------------------------------------------ draw order

fn reorder(s: &mut Session, hs: &[Handle], order: &str, refs: &[Handle]) -> Result<usize> {
    let space = s.space();
    let d = s.doc_mut()?;
    let st = d.space_mut(&space).ok_or_else(|| bad("draworder", "no space"))?;
    // Keep the relative order of the moved objects.
    let mut moving = st.order_of(hs);
    moving.sort_by_key(|(_, i)| *i);
    let moving: Vec<Handle> = moving.into_iter().map(|(h, _)| h).collect();
    match order {
        "front" => moving.iter().for_each(|h| {
            st.bring_to_front(*h);
        }),
        "back" => moving.iter().rev().for_each(|h| {
            st.send_to_back(*h);
        }),
        "above" | "under" => {
            let mut r = st.order_of(refs);
            r.retain(|(h, _)| !moving.contains(h));
            r.sort_by_key(|(_, i)| *i);
            let anchor =
                if order == "above" { r.last() } else { r.first() }.map(|(h, _)| *h).ok_or_else(|| bad("draworder", "chọn đối tượng tham chiếu"))?;
            if order == "above" {
                for h in moving.iter().rev() {
                    st.move_relative(*h, anchor, true);
                }
            } else {
                for h in &moving {
                    st.move_relative(*h, anchor, false);
                }
            }
        }
        o => return Err(bad("draworder", format!("order `{o}`: front, back, above or under"))),
    }
    Ok(moving.len())
}

fn run_draworder(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let order = str_param(p, "order").unwrap_or("front").to_ascii_lowercase();
    let refs: Vec<Handle> =
        p.get("ref").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect()).unwrap_or_default();
    let n = reorder(s, &hs, &order, &refs)?;
    Ok(json!({ "moved": n }))
}

fn plan_draworder(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn đối tượng".into())),
        1 => Some(Ask::Kw { msg: "Thứ tự".into(), kws: vec!["Above", "Under", "Front", "Back"], default: Some("Back") }),
        2 if matches!(a.get(1).map(Ans::text), Some("Above" | "Under")) => Some(Ask::Select("Chọn đối tượng tham chiếu".into())),
        _ => None,
    }
}

fn flow_draworder(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let order = a.get(1).map(|x| x.text().to_ascii_lowercase()).unwrap_or_else(|| "back".into());
    let refs = a.get(2).map(Ans::sel).unwrap_or_default().to_vec();
    reorder(s, &hs, &order, &refs)?;
    Ok(Value::Null)
}

fn kinds_to_front(s: &mut Session, what: &str) -> Result<usize> {
    let space = s.space();
    let d = s.doc()?;
    let pick = |k: &EntityKind| match what {
        "text" => matches!(k, EntityKind::Text(_) | EntityKind::MText(_)),
        "dimensions" => matches!(k, EntityKind::Dimension(_)),
        "leaders" => matches!(k, EntityKind::Leader(_) | EntityKind::MLeader(_)),
        _ => matches!(k, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Dimension(_) | EntityKind::Leader(_) | EntityKind::MLeader(_)),
    };
    let hs: Vec<Handle> = d.space(&space).map(|st| st.iter().filter(|e| pick(&e.kind)).map(|e| e.handle).collect()).unwrap_or_default();
    reorder(s, &hs, "front", &[])
}

fn run_texttofront(s: &mut Session, p: &Value) -> Result<Value> {
    let n = kinds_to_front(s, &str_param(p, "what").unwrap_or("all").to_ascii_lowercase())?;
    Ok(json!({ "moved": n, "message": format!("{n} đối tượng được đưa lên trên.") }))
}

fn plan_texttofront(_s: &Session, a: &[Ans]) -> Option<Ask> {
    a.is_empty().then(|| Ask::Kw { msg: "Đưa lên trên".into(), kws: vec!["Text", "Dimensions", "Leaders", "All"], default: Some("All") })
}

fn flow_texttofront(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let what = a.first().map(|x| x.text().to_ascii_lowercase()).unwrap_or_else(|| "all".into());
    run_texttofront(s, &json!({ "what": what }))
}

fn run_hatchtoback(s: &mut Session, _p: &Value) -> Result<Value> {
    let space = s.space();
    let hs: Vec<Handle> =
        s.doc()?.space(&space).map(|st| st.iter().filter(|e| matches!(e.kind, EntityKind::Hatch(_))).map(|e| e.handle).collect()).unwrap_or_default();
    let n = reorder(s, &hs, "back", &[])?;
    Ok(json!({ "moved": n, "message": format!("{n} mặt cắt (hatch) được đưa xuống dưới.") }))
}

// ------------------------------------------------------------------ select similar

/// What makes two objects "similar" (AutoCAD's default SELECTSIMILAR settings): type, layer,
/// and the block name, text style or dimension style.
fn similarity_key(e: &Entity) -> (String, String, String) {
    let name = match &e.kind {
        EntityKind::Insert(i) => i.block.clone(),
        EntityKind::Text(t) => t.style.clone(),
        EntityKind::MText(t) => t.style.clone(),
        EntityKind::Dimension(d) => d.style.clone(),
        _ => String::new(),
    };
    (e.kind.type_name().to_string(), e.common.layer.to_ascii_uppercase(), name.to_ascii_uppercase())
}

fn select_similar(s: &mut Session, hs: &[Handle]) -> Result<Vec<Handle>> {
    let space = s.space();
    let d = s.doc()?;
    let keys: Vec<(String, String, String)> = hs.iter().filter_map(|h| d.entity(*h)).map(|e| similarity_key(e)).collect();
    let found: Vec<Handle> = d
        .space(&space)
        .map(|st| st.iter().filter(|e| d.is_visible(e) && keys.contains(&similarity_key(e))).map(|e| e.handle).collect())
        .unwrap_or_default();
    s.set_selection(found.clone());
    Ok(found)
}

fn run_selectsimilar(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let found = select_similar(s, &hs)?;
    Ok(json!({ "handles": hex(&found), "message": format!("Đã chọn {} đối tượng giống.", found.len()) }))
}

fn flow_selectsimilar(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let found = select_similar(s, &hs)?;
    Ok(json!({ "message": format!("Đã chọn {} đối tượng giống.", found.len()) }))
}

// ------------------------------------------------------------------ layers

fn copy_to_layer(s: &mut Session, hs: &[Handle], layer: &str, delta: Vec2) -> Result<Vec<Handle>> {
    if layer.trim().is_empty() {
        return Err(bad("copytolayer", "`layer` is required"));
    }
    let d = s.doc_mut()?;
    d.ensure_layer(layer);
    let mut out = Vec::new();
    for h in hs {
        let (Some(e), Some(sp)) = (d.entity(*h).map(|e| (**e).clone()), d.space_of(*h)) else { continue };
        let mut c = e.common.clone();
        c.layer = layer.to_string();
        let mut k = e.kind.clone();
        if delta != Vec2::ZERO {
            k.transform(&Mat3::translate(delta));
        }
        out.push(d.add(&sp, c, k)?);
    }
    Ok(out)
}

fn run_copytolayer(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let layer = str_param(p, "layer").unwrap_or("").to_string();
    let delta = point_param(p, "delta").unwrap_or(Vec2::ZERO);
    let out = copy_to_layer(s, &hs, &layer, delta)?;
    Ok(json!({ "handles": hex(&out), "message": format!("{} đối tượng đã được sao chép sang layer {layer}.", out.len()) }))
}

fn plan_copytolayer(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn đối tượng cần sao chép".into())),
        1 => Some(Ask::Text { msg: "Tên layer đích".into(), default: None }),
        _ => None,
    }
}

fn flow_copytolayer(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let layer = a.get(1).map(Ans::text).unwrap_or("").to_string();
    let out = copy_to_layer(s, &hs, &layer, Vec2::ZERO)?;
    s.set_selection(out.clone());
    Ok(json!({ "message": format!("{} đối tượng đã được sao chép sang layer {layer}.", out.len()) }))
}

/// Apply `f` to every entity of the drawing: model, layouts and block definitions.
fn every_entity(d: &mut Drawing, f: &mut dyn FnMut(&mut Entity) -> bool) {
    let mut fix = |st: &mut cadcraft_doc::EntityStore| {
        let hs = st.handles();
        for h in hs {
            let mut e = match st.get(h) {
                Some(e) => (**e).clone(),
                None => continue,
            };
            if f(&mut e) {
                st.replace(e);
            }
        }
    };
    fix(&mut d.model);
    for l in &mut d.layouts {
        fix(&mut l.entities);
    }
    for b in d.blocks.values_mut() {
        if b.xref_path.is_none() {
            fix(&mut Arc::make_mut(b).entities);
        }
    }
}

/// Remove every entity on `layers` (model, layouts, blocks). Returns how many.
fn remove_on_layers(d: &mut Drawing, layers: &[String]) -> usize {
    let on = |e: &Entity| layers.iter().any(|l| l.eq_ignore_ascii_case(&e.common.layer));
    let mut n = 0;
    let mut purge = |st: &mut cadcraft_doc::EntityStore| {
        let hs: Vec<Handle> = st.iter().filter(|e| on(e)).map(|e| e.handle).collect();
        for h in hs {
            st.remove(h);
            n += 1;
        }
    };
    purge(&mut d.model);
    for l in &mut d.layouts {
        purge(&mut l.entities);
    }
    for b in d.blocks.values_mut() {
        if b.xref_path.is_none() && b.entities.iter().any(|e| on(e)) {
            purge(&mut Arc::make_mut(b).entities);
        }
    }
    n
}

fn layer_names(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    if let Some(a) = p.get("layers").and_then(Value::as_array) {
        return Ok(a.iter().filter_map(Value::as_str).map(str::to_string).collect());
    }
    if let Some(l) = str_param(p, "layer") {
        return Ok(vec![l.to_string()]);
    }
    let hs = targets(s, p)?;
    let d = s.doc()?;
    let mut out: Vec<String> = Vec::new();
    for h in hs {
        if let Some(e) = d.entity(h)
            && !out.iter().any(|l| l.eq_ignore_ascii_case(&e.common.layer))
        {
            out.push(e.common.layer.clone());
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, "chọn đối tượng hoặc cho `layers`"));
    }
    Ok(out)
}

/// Layers that may not be deleted or merged away.
fn protected(d: &Drawing, l: &str) -> Option<&'static str> {
    if l == "0" {
        Some("layer 0")
    } else if l.eq_ignore_ascii_case("Defpoints") {
        Some("layer Defpoints")
    } else if l.contains('|') {
        Some("layer của tham chiếu ngoài")
    } else if d.layer(l).is_none() {
        Some("không có layer này")
    } else {
        None
    }
}

fn merge_layers(s: &mut Session, sources: &[String], target: &str) -> Result<Value> {
    if target.trim().is_empty() {
        return Err(bad("laymrg", "`target` is required"));
    }
    let d = s.doc_mut()?;
    let sources: Vec<String> = sources.iter().filter(|l| !l.eq_ignore_ascii_case(target)).cloned().collect();
    for l in &sources {
        if let Some(why) = protected(d, l) {
            return Err(bad("laymrg", format!("{l}: {why}")));
        }
    }
    d.ensure_layer(target);
    let mut n = 0usize;
    every_entity(d, &mut |e| {
        if sources.iter().any(|l| l.eq_ignore_ascii_case(&e.common.layer)) {
            e.common.layer = target.to_string();
            n += 1;
            true
        } else {
            false
        }
    });
    if sources.iter().any(|l| l.eq_ignore_ascii_case(&d.header.str("CLAYER", "0"))) {
        d.header.set_str("CLAYER", target);
    }
    d.layers.retain(|l| !sources.iter().any(|x| x.eq_ignore_ascii_case(&l.name)));
    Ok(json!({ "moved": n, "message": format!("Đã gộp {} vào layer {target} ({n} đối tượng).", sources.join(", ")) }))
}

fn run_laymrg(s: &mut Session, p: &Value) -> Result<Value> {
    let sources = layer_names(s, p, "laymrg")?;
    let target = str_param(p, "target").unwrap_or("").to_string();
    merge_layers(s, &sources, &target)
}

fn plan_laymrg(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn đối tượng trên các layer cần gộp".into())),
        1 => Some(Ask::Text { msg: "Gộp vào layer (tên)".into(), default: None }),
        _ => None,
    }
}

fn flow_laymrg(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let sources = layer_names(s, &json!({ "handles": hex(&hs) }), "laymrg")?;
    merge_layers(s, &sources, a.get(1).map(Ans::text).unwrap_or(""))
}

fn delete_layers(s: &mut Session, layers: &[String]) -> Result<Value> {
    let d = s.doc_mut()?;
    for l in layers {
        if let Some(why) = protected(d, l) {
            return Err(bad("laydel", format!("{l}: {why}")));
        }
    }
    let n = remove_on_layers(d, layers);
    if layers.iter().any(|l| l.eq_ignore_ascii_case(&d.header.str("CLAYER", "0"))) {
        d.header.set_str("CLAYER", "0");
    }
    d.layers.retain(|l| !layers.iter().any(|x| x.eq_ignore_ascii_case(&l.name)));
    d.prune_groups();
    Ok(json!({ "deleted": n, "message": format!("Đã xóa layer {} và {n} đối tượng trên đó.", layers.join(", ")) }))
}

fn run_laydel(s: &mut Session, p: &Value) -> Result<Value> {
    let layers = layer_names(s, p, "laydel")?;
    delete_layers(s, &layers)
}

fn plan_laydel(s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn đối tượng trên layer cần xóa".into())),
        1 => {
            let names = s.doc().ok().map(|d| {
                let mut v: Vec<String> = Vec::new();
                for h in a.first().map(Ans::sel).unwrap_or_default() {
                    if let Some(e) = d.entity(*h)
                        && !v.contains(&e.common.layer)
                    {
                        v.push(e.common.layer.clone());
                    }
                }
                v.join(", ")
            });
            Some(Ask::Kw {
                msg: format!("Xóa layer {} và MỌI đối tượng trên đó (kể cả trong block)?", names.unwrap_or_default()),
                kws: vec!["Yes", "No"],
                default: Some("No"),
            })
        }
        _ => None,
    }
}

fn flow_laydel(s: &mut Session, a: &[Ans]) -> Result<Value> {
    if a.get(1).map(Ans::text) != Some("Yes") {
        return Ok(json!({ "message": "Không xóa." }));
    }
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let layers = layer_names(s, &json!({ "handles": hex(&hs) }), "laydel")?;
    delete_layers(s, &layers)
}

// ------------------------------------------------------------------ text tools

fn scale_texts(s: &mut Session, hs: &[Handle], height: Option<f64>, factor: Option<f64>) -> Result<usize> {
    let f = |h0: f64| match (height, factor) {
        (Some(h), _) => h,
        (None, Some(k)) => h0 * k,
        _ => h0,
    };
    if height.is_some_and(|h| h.is_nan() || h <= 0.0) || factor.is_some_and(|k| k.is_nan() || k <= 0.0) {
        return Err(bad("scaletext", "giá trị phải dương"));
    }
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        d.modify_entity(*h, |e| match &mut e.kind {
            EntityKind::Text(t) | EntityKind::AttDef(cadcraft_doc::Attrib { text: t, .. }) => {
                let nh = f(t.height);
                // Fit/aligned texts keep their two points; others scale about their own point.
                t.height = nh;
                n += 1;
            }
            EntityKind::MText(t) => {
                let nh = f(t.height);
                if t.height > 0.0 {
                    t.width *= nh / t.height;
                }
                t.height = nh;
                n += 1;
            }
            EntityKind::Insert(i) => {
                for a in &mut i.attribs {
                    a.text.height = f(a.text.height);
                }
                n += usize::from(!i.attribs.is_empty());
            }
            _ => {}
        })?;
    }
    Ok(n)
}

fn run_scaletext(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = scale_texts(s, &hs, p.get("height").and_then(Value::as_f64), p.get("factor").and_then(Value::as_f64))?;
    Ok(json!({ "changed": n }))
}

fn plan_scaletext(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn chữ".into())),
        1 => Some(Ask::Kw { msg: "Cách đổi".into(), kws: vec!["Height", "Factor"], default: Some("Height") }),
        2 if a.get(1).map(Ans::text) == Some("Factor") => Some(Ask::Num { msg: "Hệ số tỷ lệ".into(), default: None }),
        2 => Some(Ask::Num { msg: "Chiều cao chữ mới".into(), default: None }),
        _ => None,
    }
}

fn flow_scaletext(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let v = a.get(2).and_then(Ans::num);
    let (h, f) = if a.get(1).map(Ans::text) == Some("Factor") { (None, v) } else { (v, None) };
    let n = scale_texts(s, &hs, h, f)?;
    Ok(json!({ "message": format!("Đã đổi cỡ {n} chữ.") }))
}

/// (horizontal, vertical) for a TEXT justification code.
fn text_justify(code: &str) -> Option<(HAlign, VAlign)> {
    Some(match code.to_ascii_uppercase().as_str() {
        "L" | "LEFT" => (HAlign::Left, VAlign::Baseline),
        "C" | "CENTER" => (HAlign::Center, VAlign::Baseline),
        "R" | "RIGHT" => (HAlign::Right, VAlign::Baseline),
        "M" | "MIDDLE" => (HAlign::Middle, VAlign::Baseline),
        "TL" => (HAlign::Left, VAlign::Top),
        "TC" => (HAlign::Center, VAlign::Top),
        "TR" => (HAlign::Right, VAlign::Top),
        "ML" => (HAlign::Left, VAlign::Middle),
        "MC" => (HAlign::Center, VAlign::Middle),
        "MR" => (HAlign::Right, VAlign::Middle),
        "BL" => (HAlign::Left, VAlign::Bottom),
        "BC" => (HAlign::Center, VAlign::Bottom),
        "BR" => (HAlign::Right, VAlign::Bottom),
        _ => return None,
    })
}

/// MTEXT attachment (1..9) for a justification code.
fn mtext_attach(code: &str) -> Option<u8> {
    Some(match code.to_ascii_uppercase().as_str() {
        "TL" | "L" | "LEFT" => 1,
        "TC" | "C" | "CENTER" => 2,
        "TR" | "R" | "RIGHT" => 3,
        "ML" => 4,
        "MC" | "M" | "MIDDLE" => 5,
        "MR" => 6,
        "BL" => 7,
        "BC" => 8,
        "BR" => 9,
        _ => return None,
    })
}

fn text_anchor(t: &cadcraft_doc::Text) -> Vec2 {
    if t.halign == HAlign::Left && t.valign == VAlign::Baseline { t.insert.xy() } else { t.align_pt.unwrap_or(t.insert).xy() }
}

fn justify_one(d: &Drawing, e: &Entity, code: &str) -> Option<EntityKind> {
    match &e.kind {
        EntityKind::Text(t) => {
            if matches!(t.halign, HAlign::Aligned | HAlign::Fit) {
                return None;
            }
            let (h, v) = text_justify(code)?;
            let (_, before) = cadcraft_render::place_text_entity(d, t, &t.value);
            let p = text_anchor(t);
            let mut n = t.clone();
            n.halign = h;
            n.valign = v;
            let set = |n: &mut cadcraft_doc::Text, p: Vec2| {
                let p3 = cadcraft_geom::Vec3::new(p.x, p.y, n.insert.z);
                n.insert = p3;
                n.align_pt = if n.halign == HAlign::Left && n.valign == VAlign::Baseline { None } else { Some(p3) };
            };
            set(&mut n, p);
            // The same layout shifted: move the point by the difference.
            for _ in 0..2 {
                let (_, after) = cadcraft_render::place_text_entity(d, &n, &n.value);
                let shift = before.min - after.min;
                if !shift.is_finite() || shift.len() < 1e-12 {
                    break;
                }
                let q = text_anchor(&n) + shift;
                set(&mut n, q);
            }
            Some(EntityKind::Text(n))
        }
        EntityKind::MText(t) => {
            let a = mtext_attach(code)?;
            let before = cadcraft_render::layout_mtext_entity(d, t).bounds;
            let mut n = t.clone();
            n.attach = a;
            for _ in 0..2 {
                let after = cadcraft_render::layout_mtext_entity(d, &n).bounds;
                let shift = before.min - after.min;
                if !shift.is_finite() || shift.len() < 1e-12 {
                    break;
                }
                n.insert.x += shift.x;
                n.insert.y += shift.y;
            }
            Some(EntityKind::MText(n))
        }
        _ => None,
    }
}

fn justify_texts(s: &mut Session, hs: &[Handle], code: &str) -> Result<usize> {
    if text_justify(code).is_none() {
        return Err(bad("justifytext", format!("`{code}`: L, C, R, M, TL, TC, TR, ML, MC, MR, BL, BC, BR")));
    }
    let d = s.doc()?;
    let changes: Vec<(Handle, EntityKind)> =
        hs.iter().filter_map(|h| d.entity(*h)).filter_map(|e| justify_one(d, e, code).map(|k| (e.handle, k))).collect();
    let n = changes.len();
    let d = s.doc_mut()?;
    for (h, k) in changes {
        d.modify_entity(h, |e| e.kind = k)?;
    }
    Ok(n)
}

fn run_justifytext(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = justify_texts(s, &hs, str_param(p, "justify").unwrap_or("L"))?;
    Ok(json!({ "changed": n }))
}

fn plan_justifytext(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn chữ".into())),
        1 => Some(Ask::Kw {
            msg: "Căn chữ".into(),
            kws: vec!["Left", "Center", "Right", "Middle", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"],
            default: Some("Left"),
        }),
        _ => None,
    }
}

fn flow_justifytext(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let n = justify_texts(s, &hs, a.get(1).map(Ans::text).unwrap_or("L"))?;
    Ok(json!({ "message": format!("Đã căn lại {n} chữ.") }))
}

/// The four corners of a text's frame, `pad` larger on every side.
fn text_frame(d: &Drawing, e: &Entity, pad: f64) -> Option<Vec<Vec2>> {
    let (rot, origin, b): (f64, Vec2, Bounds2) = match &e.kind {
        EntityKind::Text(t) => {
            let mut z = t.clone();
            z.rotation = 0.0;
            let o = text_anchor(t);
            (t.rotation, o, cadcraft_render::place_text_entity(d, &z, &z.value).1)
        }
        EntityKind::MText(t) => {
            let mut z = t.clone();
            z.rotation = 0.0;
            (t.rotation, t.insert.xy(), cadcraft_render::layout_mtext_entity(d, &z).bounds)
        }
        _ => return None,
    };
    if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
        return None;
    }
    let corners = [
        Vec2::new(b.min.x - pad, b.min.y - pad),
        Vec2::new(b.max.x + pad, b.min.y - pad),
        Vec2::new(b.max.x + pad, b.max.y + pad),
        Vec2::new(b.min.x - pad, b.max.y + pad),
    ];
    Some(corners.iter().map(|p| origin + (*p - origin).rotate(rot)).collect())
}

fn mask_texts(s: &mut Session, hs: &[Handle], offset: f64) -> Result<Vec<Handle>> {
    let space = s.space();
    let d = s.doc()?;
    let jobs: Vec<(Handle, cadcraft_doc::Common, Vec<Vec2>)> = hs
        .iter()
        .filter_map(|h| d.entity(*h))
        .filter_map(|e| {
            let height = match &e.kind {
                EntityKind::Text(t) => t.height,
                EntityKind::MText(t) => t.height,
                _ => return None,
            };
            text_frame(d, e, height * offset).map(|f| (e.handle, e.common.clone(), f))
        })
        .collect();
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for (h, c, frame) in jobs {
        let w = d.add(&space, c, EntityKind::Wipeout(Wipeout { boundary: frame }))?;
        if let Some(st) = d.space_mut(&space) {
            st.move_relative(w, h, false);
        }
        out.push(w);
    }
    Ok(out)
}

fn run_textmask(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let out = mask_texts(s, &hs, f64_or(p, "offset", 0.35))?;
    Ok(json!({ "handles": hex(&out) }))
}

fn plan_textmask(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Select("Chọn chữ cần che nền".into())),
        1 => Some(Ask::Num { msg: "Khoảng hở (× chiều cao chữ)".into(), default: Some(0.35) }),
        _ => None,
    }
}

fn flow_textmask(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(Ans::sel).unwrap_or_default().to_vec();
    let out = mask_texts(s, &hs, a.get(1).and_then(Ans::num).unwrap_or(0.35))?;
    Ok(json!({ "message": format!("Đã che nền {} chữ.", out.len()) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Input;

    fn line(s: &mut Session, layer: &str, a: (f64, f64), b: (f64, f64)) -> Handle {
        s.execute("layer.new", &json!({ "name": layer })).ok();
        let r = s.execute("line", &json!({ "points": [[a.0, a.1], [b.0, b.1]] })).unwrap();
        let h = Handle::parse_hex(r["handles"][0].as_str().or(r["handle"].as_str()).unwrap()).unwrap();
        s.doc_mut().unwrap().modify_entity(h, |e| e.common.layer = layer.into()).unwrap();
        h
    }
    fn order(s: &Session) -> Vec<Handle> {
        s.doc().unwrap().model.handles()
    }

    #[test]
    fn draw_order_above_under_front_back() {
        let mut s = Session::new();
        let a = line(&mut s, "0", (0.0, 0.0), (1.0, 0.0));
        let b = line(&mut s, "0", (0.0, 1.0), (1.0, 1.0));
        let c = line(&mut s, "0", (0.0, 2.0), (1.0, 2.0));
        s.execute("draworder", &json!({ "handles": [c.hex()], "order": "under", "ref": [b.hex()] })).unwrap();
        assert_eq!(order(&s), vec![a, c, b]);
        s.execute("draworder", &json!({ "handles": [a.hex()], "order": "above", "ref": [b.hex()] })).unwrap();
        assert_eq!(order(&s), vec![c, b, a]);
        // Interactive: select, Back.
        s.set_selection(vec![a]);
        s.cmdline("DRAWORDER").unwrap();
        s.cmdline("B").unwrap();
        assert!(s.running.is_none());
        assert_eq!(order(&s), vec![a, c, b]);
    }

    #[test]
    fn select_similar_copy_merge_delete_layers() {
        let mut s = Session::new();
        let a = line(&mut s, "TIM", (0.0, 0.0), (1.0, 0.0));
        let _b = line(&mut s, "TIM", (0.0, 1.0), (1.0, 1.0));
        let c = line(&mut s, "BIEN", (0.0, 2.0), (1.0, 2.0));
        let r = s.execute("selectsimilar", &json!({ "handles": [a.hex()] })).unwrap();
        assert_eq!(r["handles"].as_array().unwrap().len(), 2);
        let r = s.execute("copytolayer", &json!({ "handles": [c.hex()], "layer": "BIEN2", "delta": [0, 5] })).unwrap();
        assert_eq!(r["handles"].as_array().unwrap().len(), 1);
        s.execute("laymrg", &json!({ "layers": ["BIEN2"], "target": "TIM" })).unwrap();
        let d = s.doc().unwrap();
        assert!(d.layer("BIEN2").is_none());
        assert_eq!(d.model.iter().filter(|e| e.common.layer == "TIM").count(), 3);
        // LAYDEL interactively: pick a TIM object, confirm.
        s.set_selection(vec![a]);
        s.cmdline("LAYDEL").unwrap();
        s.cmdline("Y").unwrap();
        let d = s.doc().unwrap();
        assert!(d.layer("TIM").is_none());
        assert_eq!(d.model.len(), 1);
        assert!(s.execute("laydel", &json!({ "layers": ["0"] })).is_err());
    }

    #[test]
    fn scale_justify_and_mask_text() {
        let mut s = Session::new();
        let r = s.execute("text", &json!({ "at": [10, 10], "height": 2.5, "text": "Mố M1" })).unwrap();
        let h = Handle::parse_hex(r["handle"].as_str().or(r["handles"][0].as_str()).unwrap()).unwrap();
        let bounds = |s: &Session| {
            let d = s.doc().unwrap();
            let EntityKind::Text(t) = &d.entity(h).unwrap().kind else { panic!() };
            cadcraft_render::place_text_entity(d, t, &t.value).1
        };
        let b0 = bounds(&s);
        s.execute("justifytext", &json!({ "handles": [h.hex()], "justify": "MC" })).unwrap();
        let b1 = bounds(&s);
        assert!(b0.min.near(b1.min, 1e-6) && b0.max.near(b1.max, 1e-6), "{b0:?} {b1:?}");
        let d = s.doc().unwrap();
        let EntityKind::Text(t) = &d.entity(h).unwrap().kind else { panic!() };
        assert_eq!((t.halign, t.valign), (HAlign::Center, VAlign::Middle));
        s.execute("scaletext", &json!({ "handles": [h.hex()], "factor": 2.0 })).unwrap();
        let EntityKind::Text(t) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
        assert_eq!(t.height, 5.0);
        let r = s.execute("textmask", &json!({ "handles": [h.hex()] })).unwrap();
        let w = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
        // The mask is drawn just under the text.
        let o = order(&s);
        let (iw, it) = (o.iter().position(|x| *x == w).unwrap(), o.iter().position(|x| *x == h).unwrap());
        assert_eq!(iw + 1, it);
        let _ = Input::Enter;
    }
}
