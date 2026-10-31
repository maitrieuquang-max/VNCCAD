//! VNCCad: commands added after the second engineer test round — ARRAY (the classic entry
//! point that asks for the array type and hands over to ARRAYRECT / ARRAYPATH / ARRAYPOLAR).

use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::*;
use crate::prompt::{Accept, Input, Prompt, Step};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("vpprops", "Viewport Properties (Properties palette)", run_vp_set)
            .params("{handles? (viewports; default the active one), locked?, scale? (\"1:500\" or model units per paper unit), viewCenter?: [x,y], center?: [x,y], width?, height?}")
            .enabled(has_doc),
        CommandSpec::new("options", "Options...", run_options)
            .menu(&["Tools", "Options..."])
            .alias(&["op", "cauhinh", "preferences"])
            .params("{} → the session settings (the Options dialog edits them; scripts use SETVAR)")
            .noundo()
            .enabled(always),
        CommandSpec::new("vplock", "Lock Viewport", run_vplock)
            .menu(&["View", "Viewports", "Lock / Unlock Viewport"])
            .alias(&["khoavp"])
            .params("{on: bool, handles?} (default: the active viewport)")
            .interactive(|_| Ok(Box::new(Flow::new("vplock", plan_vplock, flow_vplock))))
            .enabled(has_doc),
        CommandSpec::new("vpscale", "Viewport Scale", run_vpscale)
            .menu(&["View", "Viewports", "Viewport Scale"])
            .alias(&["tilevp"])
            .params("{scale: \"1:500\" | number, handles?} (default: the active viewport)")
            .interactive(|_| Ok(Box::new(Flow::new("vpscale", plan_vpscale, flow_vpscale))))
            .enabled(has_doc),
        CommandSpec::new("array", "Array", run_array)
            .menu(&["Modify", "Array", "Array"])
            .alias(&["ar", "mang"])
            .params("{type: \"rectangular\"|\"path\"|\"polar\", …the parameters of ARRAYRECT / ARRAYPATH / ARRAYPOLAR}")
            .interactive(|_| Ok(Box::new(ArrayM::default()))),
    ]
}

fn array_target(kind: &str) -> Option<&'static str> {
    let k = kind.trim().to_ascii_lowercase();
    if k.is_empty() || "rectangular".starts_with(&k) {
        Some("arrayrect")
    } else if k.len() >= 2 && "path".starts_with(&k) {
        Some("arraypath")
    } else if k.len() >= 2 && "polar".starts_with(&k) {
        Some("arraypolar")
    } else {
        None
    }
}

fn run_array(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = p.get("type").and_then(Value::as_str).unwrap_or("rectangular");
    let id = array_target(kind).ok_or_else(|| bad("array", "type: rectangular | path | polar"))?;
    s.execute(id, p)
}

/// Asks the array type, then runs the chosen array command's own prompts.
#[derive(Default)]
struct ArrayM {
    inner: Option<Box<dyn crate::Interactive>>,
}

impl ArrayM {
    fn hand_over(&mut self, s: &mut Session, id: &str) -> Result<Step> {
        let factory = find_command(id).and_then(|c| c.interactive).ok_or_else(|| bad("array", format!("{id} is not available")))?;
        let mut m = factory(s)?;
        let step = m.begin(s)?;
        self.inner = Some(m);
        Ok(step)
    }
}

impl crate::Interactive for ArrayM {
    fn name(&self) -> &'static str {
        "ARRAY"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.inner {
            Some(m) => m.prompt(s),
            None => Prompt::new("Enter array type", Accept { point: false, number: false, text: true, select: false, enter: true })
                .kw(&["Rectangular", "PAth", "POlar"])
                .default("Rectangular"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(m) = self.inner.as_mut() {
            return m.input(s, i);
        }
        let kind = match &i {
            Input::Enter => String::new(),
            Input::Keyword(k) | Input::Text(k) => k.clone(),
            _ => return Ok(Step::Continue),
        };
        match array_target(&kind) {
            Some(id) => self.hand_over(s, id),
            None => {
                s.echo("Chọn Rectangular, PAth hoặc POlar.");
                Ok(Step::Continue)
            }
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.inner.as_ref().map(|m| m.preview(s, c)).unwrap_or_default()
    }
    fn keep_on_cancel(&mut self, s: &mut Session) -> Result<bool> {
        match self.inner.as_mut() {
            Some(m) => m.keep_on_cancel(s),
            None => Ok(false),
        }
    }
}

/// VNCCad: undo a Vietnamese Telex keyboard (Unikey, EVKey…) left on while typing a command
/// name — Telex turns `EX` into `Ẽ`, `OF` into `Ò`, `DD` into `Đ`. Every accented letter maps
/// back to its plain vowel plus the Telex keys that made it (`Ẽ` → `EX`). `None` when the text
/// is plain ASCII already or cannot be mapped.
pub fn detelex(name: &str) -> Option<String> {
    if name.is_ascii() {
        return None;
    }
    let mut out = String::with_capacity(name.len() + 4);
    for ch in name.chars() {
        let upper = ch.is_uppercase();
        let lc = ch.to_lowercase().next().unwrap_or(ch);
        let (base, tone) = split_tone(lc);
        let shape = match base {
            'â' => "aa",
            'ă' => "aw",
            'ê' => "ee",
            'ô' => "oo",
            'ơ' => "ow",
            'ư' => "uw",
            'đ' => "dd",
            _ => "",
        };
        let mut piece = String::new();
        if shape.is_empty() {
            piece.push(base);
        } else {
            piece.push_str(shape);
        }
        if let Some(t) = tone {
            piece.push(t);
        }
        if !piece.is_ascii() {
            return None;
        }
        out.push_str(&if upper { piece.to_ascii_uppercase() } else { piece });
    }
    Some(out)
}

/// A lower-case Vietnamese letter → (letter without tone mark, Telex tone key).
fn split_tone(c: char) -> (char, Option<char>) {
    const ROWS: [(char, &str); 12] = [
        ('a', "áàảãạ"),
        ('ă', "ắằẳẵặ"),
        ('â', "ấầẩẫậ"),
        ('e', "éèẻẽẹ"),
        ('ê', "ếềểễệ"),
        ('i', "íìỉĩị"),
        ('o', "óòỏõọ"),
        ('ô', "ốồổỗộ"),
        ('ơ', "ớờởỡợ"),
        ('u', "úùủũụ"),
        ('ư', "ứừửữự"),
        ('y', "ýỳỷỹỵ"),
    ];
    const KEYS: [char; 5] = ['s', 'f', 'r', 'x', 'j'];
    for (base, toned) in ROWS {
        if let Some(i) = toned.chars().position(|t| t == c) {
            return (base, KEYS.get(i).copied());
        }
    }
    (c, None)
}

// ---------------------------------------------------------------- viewports (VNCCad)

/// Model units per paper unit from `1:500`, `1/500`, `500` or a number.
pub fn parse_vp_scale(v: &Value) -> Option<f64> {
    let k = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(t) => {
            let t = t.trim().replace(',', ".");
            match t.split_once([':', '/']) {
                Some((a, b)) => a.trim().parse::<f64>().ok().zip(b.trim().parse::<f64>().ok()).filter(|(a, _)| *a > 0.0).map(|(a, b)| b / a),
                None => t.parse::<f64>().ok(),
            }
        }
        _ => None,
    }?;
    (k.is_finite() && k > 0.0 && k < 1e9).then_some(k)
}

/// The viewports to change: given handles, the selected viewports, or the active (MSPACE) one.
fn vp_targets(s: &Session, p: &Value) -> Result<Vec<cadcraft_doc::Handle>> {
    let d = s.doc()?;
    let is_vp = |h: &cadcraft_doc::Handle| d.entity(*h).is_some_and(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.id != 1));
    let given = targets(s, p).unwrap_or_default();
    let mut hs: Vec<_> = given.into_iter().filter(is_vp).collect();
    if hs.is_empty()
        && let Some(h) = s.state()?.mspace
    {
        hs.push(h);
    }
    if hs.is_empty() {
        return Err(bad("vpprops", "chọn khung nhìn (viewport) hoặc kích đúp vào trong khung trước"));
    }
    Ok(hs)
}

fn run_vp_set(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = vp_targets(s, p)?;
    let scale = match p.get("scale") {
        Some(v) => Some(parse_vp_scale(v).ok_or_else(|| bad("vpprops", "scale: 1:500, 1/500 hoặc một số dương"))?),
        None => None,
    };
    let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0 && *v < 1e12);
    let pt = |k: &str| p.get(k).and_then(|v| Some(Vec2::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))).filter(|q| q.is_finite());
    let locked = p.get("locked").and_then(Value::as_bool);
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in &hs {
        d.modify_entity(*h, |e| {
            if let EntityKind::Viewport(v) = &mut e.kind {
                if let Some(w) = num("width") {
                    v.width = w;
                }
                if let Some(ht) = num("height") {
                    // Keep the scale when the frame changes height.
                    let k = v.view_height / v.height.max(1e-12);
                    v.height = ht;
                    v.view_height = ht * k;
                }
                if let Some(c) = pt("center") {
                    v.center.x = c.x;
                    v.center.y = c.y;
                }
                if let Some(k) = scale {
                    v.view_height = v.height * k;
                }
                if let Some(c) = pt("viewCenter") {
                    v.view_center = c;
                }
                if let Some(l) = locked {
                    v.locked = l;
                }
                n += 1;
            }
        })?;
    }
    s.touch();
    Ok(json!({ "changed": n }))
}

fn run_vplock(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
    let mut q = p.clone();
    if let Some(o) = q.as_object_mut() {
        o.insert("locked".into(), json!(on));
    }
    let r = run_vp_set(s, &q)?;
    let n = r["changed"].as_u64().unwrap_or(0);
    Ok(json!({ "changed": n, "message": format!("Đã {} {n} khung nhìn.", if on { "khóa" } else { "mở khóa" }) }))
}

fn plan_vplock(s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Kw { msg: "Khóa khung nhìn".into(), kws: vec!["ON", "OFF"], default: Some("OFF") }),
        1 if s.state().ok()?.mspace.is_none() => Some(Ask::Select("Chọn khung nhìn".into())),
        _ => None,
    }
}

fn flow_vplock(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let on = a.first().is_some_and(|x| x.text().eq_ignore_ascii_case("ON"));
    let hs: Vec<String> = a.get(1).map(|x| x.sel().iter().map(|h| h.hex()).collect()).unwrap_or_default();
    run_vplock(s, &json!({ "on": on, "handles": hs }))
}

fn run_vpscale(s: &mut Session, p: &Value) -> Result<Value> {
    let k = p.get("scale").and_then(parse_vp_scale).ok_or_else(|| bad("vpscale", "scale: 1:500, 1/500 hoặc một số dương"))?;
    let mut q = p.clone();
    if let Some(o) = q.as_object_mut() {
        o.insert("scale".into(), json!(k));
    }
    let r = run_vp_set(s, &q)?;
    Ok(json!({ "changed": r["changed"], "message": format!("Tỉ lệ khung nhìn 1:{}", (k * 1e6).round() / 1e6) }))
}

fn plan_vpscale(s: &Session, a: &[Ans]) -> Option<Ask> {
    let in_ms = s.state().ok()?.mspace.is_some();
    match (a.len(), in_ms) {
        (0, false) => Some(Ask::Select("Chọn khung nhìn".into())),
        (0, true) | (1, false) => Some(Ask::Text { msg: "Tỉ lệ (vd 1:500)".into(), default: Some("1:100".into()) }),
        _ => None,
    }
}

fn flow_vpscale(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let (hs, t) = match a {
        [sel, t] => (sel.sel().iter().map(|h| h.hex()).collect::<Vec<_>>(), t.text().to_string()),
        [t] => (Vec::new(), t.text().to_string()),
        _ => return Err(bad("vpscale", "thiếu tỉ lệ")),
    };
    run_vpscale(s, &json!({ "scale": t, "handles": hs }))
}

// ---------------------------------------------------------------- PURGE (VNCCad)

/// Purge categories, as the PURGE dialog lists them.
pub const PURGE_KINDS: [(&str, &str); 6] = [
    ("blocks", "Block"),
    ("layers", "Layer"),
    ("linetypes", "Kiểu đường (Linetype)"),
    ("textStyles", "Kiểu chữ (Text style)"),
    ("dimStyles", "Kiểu kích thước (Dim style)"),
    ("mleaderStyles", "Kiểu ghi chú dẫn (Multileader style)"),
];

/// Names that can be purged now, by category.
pub fn purgeable(d: &cadcraft_doc::Drawing) -> Vec<(&'static str, Vec<String>)> {
    use std::collections::HashSet;
    let low = |s: &str| s.to_ascii_lowercase();
    let all: Vec<&cadcraft_doc::Entity> =
        d.model.iter().map(|e| &**e).chain(d.layouts.iter().flat_map(|l| l.entities.iter().map(|e| &**e))).collect();
    // Blocks referenced from the drawing, followed through nested references.
    let mut used_b: HashSet<String> = HashSet::new();
    let mut frontier: Vec<String> = Vec::new();
    let refs = |e: &cadcraft_doc::Entity, out: &mut Vec<String>| match &e.kind {
        EntityKind::Insert(i) => out.push(i.block.clone()),
        EntityKind::Dimension(dm) => out.extend(dm.block.clone()),
        _ => {}
    };
    // As in AutoCAD, a block referenced from any block definition is in use (until that
    // definition is purged: `nested`).
    for e in all.iter().copied().chain(d.blocks.values().flat_map(|b| b.entities.iter().map(|e| &**e))) {
        refs(e, &mut frontier);
    }
    // Dimension arrow blocks named in dimension styles.
    for ds in &d.dim_styles {
        let v = serde_json::to_value(ds).unwrap_or_default();
        for k in ["arrowBlock", "arrowBlock1", "arrowBlock2", "leaderArrowBlock", "dimblk", "dimblk1", "dimblk2", "dimldrblk"] {
            if let Some(n) = v.get(k).and_then(Value::as_str).filter(|n| !n.is_empty()) {
                frontier.push(n.to_string());
            }
        }
    }
    let mut guard = 0;
    while let Some(n) = frontier.pop() {
        guard += 1;
        if guard > 200_000 || !used_b.insert(low(&n)) {
            continue;
        }
        if let Some(b) = d.block(&n) {
            for e in b.entities.iter() {
                refs(e, &mut frontier);
            }
        }
    }
    // Entities that count: the drawing's and those inside every block definition.
    let live: Vec<&cadcraft_doc::Entity> = all.iter().copied().chain(d.blocks.values().flat_map(|b| b.entities.iter().map(|e| &**e))).collect();
    let blocks: Vec<String> = d
        .blocks
        .values()
        .filter(|b| !b.anonymous && !b.name.starts_with('*') && b.xref_path.is_none() && !used_b.contains(&low(&b.name)) && !b.name.contains('|'))
        .map(|b| b.name.clone())
        .collect();
    let used_l: HashSet<String> = live.iter().map(|e| low(&e.common.layer)).collect();
    let cur_l = low(&d.header.str("CLAYER", "0"));
    let layers: Vec<String> = d
        .layers
        .iter()
        .filter(|l| {
            let n = low(&l.name);
            n != "0" && n != "defpoints" && n != cur_l && !n.contains('|') && !used_l.contains(&n)
        })
        .map(|l| l.name.clone())
        .collect();
    let used_lt: HashSet<String> = live
        .iter()
        .map(|e| low(&e.common.linetype))
        .chain(d.layers.iter().map(|l| low(&l.linetype)))
        .chain(d.mline_styles.iter().flat_map(|m| m.elements.iter().map(|e| low(&e.linetype))))
        .chain(std::iter::once(low(&d.header.str("CELTYPE", "ByLayer"))))
        .collect();
    let linetypes: Vec<String> = d
        .linetypes
        .iter()
        .filter(|l| {
            let n = low(&l.name);
            !["byblock", "bylayer", "continuous"].contains(&n.as_str()) && !n.contains('|') && !used_lt.contains(&n)
        })
        .map(|l| l.name.clone())
        .collect();
    let mut used_ts: HashSet<String> = HashSet::new();
    for e in &live {
        match &e.kind {
            EntityKind::Text(t) => drop(used_ts.insert(low(&t.style))),
            EntityKind::MText(t) => drop(used_ts.insert(low(&t.style))),
            EntityKind::AttDef(a) => drop(used_ts.insert(low(&a.text.style))),
            EntityKind::Insert(i) => used_ts.extend(i.attribs.iter().map(|a| low(&a.text.style))),
            EntityKind::MLeader(m) => used_ts.extend(m.text.iter().map(|t| low(&t.style))),
            _ => {}
        }
    }
    used_ts.extend(d.dim_styles.iter().map(|s| low(&s.text_style)));
    used_ts.extend(d.mleader_styles.iter().map(|s| low(&s.text_style)));
    used_ts.insert(low(&d.header.str("TEXTSTYLE", "Standard")));
    let text_styles: Vec<String> = d
        .text_styles
        .iter()
        .filter(|t| {
            let n = low(&t.name);
            n != "standard" && !n.contains('|') && !used_ts.contains(&n)
        })
        .map(|t| t.name.clone())
        .collect();
    let mut used_ds: HashSet<String> = HashSet::new();
    for e in &live {
        match &e.kind {
            EntityKind::Dimension(dm) => drop(used_ds.insert(low(&dm.style))),
            EntityKind::Leader(l) => drop(used_ds.insert(low(&l.style))),
            EntityKind::Tolerance(t) => drop(used_ds.insert(low(&t.style))),
            _ => {}
        }
    }
    used_ds.insert(low(&d.header.str("DIMSTYLE", "Standard")));
    let dim_styles: Vec<String> = d
        .dim_styles
        .iter()
        .filter(|s| {
            let n = low(&s.name);
            n != "standard" && !n.contains('|') && !n.contains('$') && !used_ds.contains(&n)
        })
        .map(|s| s.name.clone())
        .collect();
    let mut used_ml: HashSet<String> =
        live.iter().filter_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(low(&m.style)) } else { None }).collect();
    used_ml.insert(low(&d.header.str("CMLEADERSTYLE", "Standard")));
    let mleader_styles: Vec<String> = d
        .mleader_styles
        .iter()
        .filter(|s| {
            let n = low(&s.name);
            n != "standard" && !used_ml.contains(&n)
        })
        .map(|s| s.name.clone())
        .collect();
    let mut out = vec![
        ("blocks", blocks),
        ("layers", layers),
        ("linetypes", linetypes),
        ("textStyles", text_styles),
        ("dimStyles", dim_styles),
        ("mleaderStyles", mleader_styles),
    ];
    for (_, v) in &mut out {
        v.sort_by_key(|n| n.to_ascii_lowercase());
    }
    out
}

/// Remove the named items of one category (only those still purgeable). Returns how many.
fn purge_names(d: &mut cadcraft_doc::Drawing, kind: &str, names: &[String]) -> usize {
    let can: std::collections::HashSet<String> =
        purgeable(d).into_iter().find(|(k, _)| *k == kind).map(|(_, v)| v.into_iter().map(|n| n.to_ascii_lowercase()).collect()).unwrap_or_default();
    let want: std::collections::HashSet<String> = names.iter().map(|n| n.to_ascii_lowercase()).filter(|n| can.contains(n)).collect();
    let hit = |n: &str| want.contains(&n.to_ascii_lowercase());
    match kind {
        "blocks" => {
            let before = d.blocks.len();
            d.blocks.retain(|k, _| !hit(k));
            before - d.blocks.len()
        }
        "layers" => {
            let before = d.layers.len();
            d.layers.retain(|l| !hit(&l.name));
            before - d.layers.len()
        }
        "linetypes" => {
            let before = d.linetypes.len();
            d.linetypes.retain(|l| !hit(&l.name));
            before - d.linetypes.len()
        }
        "textStyles" => {
            let before = d.text_styles.len();
            d.text_styles.retain(|l| !hit(&l.name));
            before - d.text_styles.len()
        }
        "dimStyles" => {
            let before = d.dim_styles.len();
            d.dim_styles.retain(|l| !hit(&l.name));
            before - d.dim_styles.len()
        }
        "mleaderStyles" => {
            let before = d.mleader_styles.len();
            d.mleader_styles.retain(|l| !hit(&l.name));
            before - d.mleader_styles.len()
        }
        _ => 0,
    }
}

/// PURGE: `{list: true}` lists what can be purged; `{items: {category: [names]}}` purges those;
/// `{}` (or `all: true`) purges everything purgeable. `nested: true` repeats until nothing is
/// left (a block that only an unused block referenced, then its layers…).
pub fn run_purge(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("list").and_then(Value::as_bool).unwrap_or(false) {
        let d = s.doc()?;
        let m: serde_json::Map<String, Value> = purgeable(d).into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        return Ok(Value::Object(m));
    }
    let nested = p.get("nested").and_then(Value::as_bool).unwrap_or(false);
    let items = p.get("items").and_then(Value::as_object).cloned();
    let d = s.doc_mut()?;
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    // What could be purged before anything was: nested rounds take only what becomes purgeable
    // because of this purge, never other unused items the user did not choose.
    let initial: std::collections::HashSet<(String, String)> =
        purgeable(d).into_iter().flat_map(|(k, v)| v.into_iter().map(move |n| (k.to_string(), n.to_ascii_lowercase()))).collect();
    for round in 0..if nested { 50 } else { 1 } {
        let mut n = 0;
        for (kind, names) in purgeable(d) {
            let chosen: Vec<String> = if round == 0 {
                match &items {
                    Some(m) => {
                        let want: Vec<String> = m
                            .get(kind)
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                            .unwrap_or_default();
                        names.into_iter().filter(|x| want.iter().any(|w| w.eq_ignore_ascii_case(x))).collect()
                    }
                    None => names,
                }
            } else {
                names.into_iter().filter(|x| !initial.contains(&(kind.to_string(), x.to_ascii_lowercase()))).collect()
            };
            if chosen.is_empty() {
                continue;
            }
            let k = purge_names(d, kind, &chosen);
            *counts.entry(kind).or_default() += k;
            n += k;
        }
        if n == 0 {
            break;
        }
    }
    let total: usize = counts.values().sum();
    let parts: Vec<String> = PURGE_KINDS.iter().filter_map(|(k, label)| counts.get(k).filter(|n| **n > 0).map(|n| format!("{n} {label}"))).collect();
    let msg = if total == 0 {
        "Không có gì để xóa (purge).".to_string()
    } else {
        format!("Đã xóa {total} mục không dùng: {}.", parts.join(", "))
    };
    let mut out = json!({ "total": total, "message": msg });
    if let Some(o) = out.as_object_mut() {
        for (k, n) in counts {
            o.insert(k.to_string(), json!(n));
        }
        // Kept for scripts written against the old reply.
        for k in ["blocks", "layers", "linetypes"] {
            o.entry(k).or_insert(json!(0));
        }
    }
    Ok(out)
}

fn run_options(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(serde_json::to_value(&s.settings).unwrap_or(Value::Null))
}

/// VNCCad: a hatch's area with islands taken out (a loop inside an odd number of other loops is
/// a hole), and the total length of its loops.
pub fn hatch_area(h: &cadcraft_doc::Hatch) -> (f64, f64) {
    let loops: Vec<cadcraft_geom::Polyline> =
        h.loops.iter().take(100_000).map(|l| cadcraft_geom::Polyline { vertices: l.vertices.clone(), closed: true }).collect();
    let pts: Vec<Vec<Vec2>> = loops.iter().map(|p| p.tessellate((p.len() * 1e-4).max(1e-9))).collect();
    let inside = |p: Vec2, poly: &[Vec2]| {
        let n = poly.len();
        let mut c = false;
        for i in 0..n {
            let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
                c = !c;
            }
        }
        c
    };
    let mut area = 0.0;
    let mut perim = 0.0;
    for (i, l) in loops.iter().enumerate() {
        let a = l.area().abs();
        perim += l.len();
        let probe = pts.get(i).and_then(|p| p.first().copied());
        let depth = probe.map(|p| pts.iter().enumerate().filter(|(j, q)| *j != i && q.len() >= 3 && inside(p, q)).count()).unwrap_or(0);
        if depth % 2 == 0 {
            area += a;
        } else {
            area -= a;
        }
    }
    (area.max(0.0), perim)
}
