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
