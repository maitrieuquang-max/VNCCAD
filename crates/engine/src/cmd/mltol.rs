//! VNCCad: geometric tolerances (TOLERANCE), multiline styles (MLSTYLE) and multiline editing
//! (MLEDIT: crosses, tees, corner joint, vertices, cuts and welds).

use cadcraft_color::Color;
use cadcraft_doc::{EntityKind, Handle, MLine, MLineElement, MLineStyle, Tolerance};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::helpers::{line, lwpoly};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("tolerance", "Geometric Tolerance", run_tolerance)
            .menu(&["Dimension", "Tolerance..."])
            .alias(&["tol", "dungsai"])
            .params(
                "{at: [x,y], text?: AutoCAD tolerance string | symbol?: position|concentricity|symmetry|parallelism|perpendicularity|angularity|cylindricity|flatness|circularity|straightness|profilesurface|profileline|runoutcircular|runouttotal, tolerance1?: \"Ø0.05(M)\", tolerance2?, datum?: \"A B C\" (or datum1..3), rotation?: degrees, style?}",
            )
            .interactive(|_| Ok(Box::new(Flow::new("tolerance", plan_tolerance, flow_tolerance))))
            .enabled(has_doc),
        CommandSpec::new("mlstyle", "Multiline Style", run_mlstyle)
            .menu(&["Format", "Multiline Style..."])
            .alias(&["kieumline"])
            .params(
                "{name?, elements?: [{offset, color?, linetype?}] | [offset,...], description?, fill?, fillColor?, startCap?/endCap?: none|line|arc|both, innerArcs?, miters?, current?: bool | delete?: name} — no parameters: list",
            )
            .interactive(|_| Ok(Box::new(Flow::new("mlstyle", plan_mlstyle, flow_mlstyle))))
            .enabled(has_doc),
        CommandSpec::new("mledit", "Edit Multiline", run_mledit)
            .menu(&["Modify", "Object", "Multiline..."])
            .alias(&["suamline"])
            .params(
                "{tool: closedCross|openCross|mergedCross|closedTee|openTee|mergedTee|cornerJoint|addVertex|deleteVertex|cutSingle|cutAll|weldAll, p1: [x,y] (picks the first mline), p2?: [x,y] (second mline, or the second cut point)}",
            )
            .interactive(|_| Ok(Box::new(Flow::new("mledit", plan_mledit, flow_mledit))))
            .enabled(has_doc),
    ]
}

// ===================================================================== TOLERANCE

/// Command keywords of the GDT symbols.
const SYMBOL_KWS: &[(&str, char)] = &[
    ("POsition", 'j'),
    ("COncentricity", 'r'),
    ("SYmmetry", 'i'),
    ("PArallelism", 'f'),
    ("PErpendicularity", 'b'),
    ("ANgularity", 'a'),
    ("CYlindricity", 'g'),
    ("FLatness", 'c'),
    ("CIrcularity", 'e'),
    ("STraightness", 'u'),
    ("ProfileSurface", 'd'),
    ("ProfileLine", 'k'),
    ("RunoutCircular", 'h'),
    ("RunoutTotal", 't'),
    ("None", ' '),
];

fn symbol_of(name: &str) -> Option<char> {
    let n = name.trim();
    if n.chars().count() == 1 {
        return n.chars().next().map(|c| c.to_ascii_lowercase()).filter(|c| cadcraft_doc::GDT_SYMBOLS.iter().any(|(_, x)| x == c));
    }
    let kws: Vec<&'static str> = SYMBOL_KWS.iter().map(|(k, _)| *k).collect();
    let k = super::flow::match_kw(n, &kws)?;
    SYMBOL_KWS.iter().find(|(x, _)| *x == k).map(|(_, c)| *c)
}

/// A tolerance value as typed (`Ø0.05(M)`, `%%c0.05 M`, `dia 0.05`) in AutoCAD's markup.
pub fn gdt_markup(v: &str) -> String {
    let mut t = v.trim().to_string();
    if t.is_empty() {
        return t;
    }
    let mut pre = String::new();
    for d in ["Ø", "ø", "∅", "%%c", "%%C", "dia ", "DIA "] {
        if let Some(r) = t.strip_prefix(d) {
            pre.push_str("{\\Fgdt;n}");
            t = r.trim_start().to_string();
            break;
        }
    }
    let mut post = String::new();
    for (m, c) in [("(M)", 'm'), ("(L)", 'l'), ("(S)", 's'), ("(P)", 'p'), (" M", 'm'), (" L", 'l'), (" S", 's'), (" P", 'p')] {
        if let Some(r) = t.strip_suffix(m) {
            post = format!("{{\\Fgdt;{c}}}");
            t = r.trim_end().to_string();
            break;
        }
    }
    format!("{pre}{t}{post}")
}

/// The coded string of one frame row.
fn tolerance_text(symbol: Option<char>, tol1: &str, tol2: &str, datums: &[String]) -> String {
    let mut cells: Vec<String> = vec![symbol.filter(|c| *c != ' ').map(|c| format!("{{\\Fgdt;{c}}}")).unwrap_or_default()];
    cells.push(gdt_markup(tol1));
    cells.push(gdt_markup(tol2));
    for d in datums.iter().take(3) {
        cells.push(gdt_markup(d));
    }
    while cells.last().is_some_and(String::is_empty) && cells.len() > 1 {
        cells.pop();
    }
    cells.join("%%v")
}

fn add_tolerance(s: &mut Session, at: Vec2, text: String, rotation: f64, style: Option<&str>) -> Result<Handle> {
    if text.replace("%%v", "").replace("^J", "").trim().is_empty() {
        return Err(bad("tolerance", "khung dung sai trống"));
    }
    let style = match style {
        Some(x) => x.to_string(),
        None => s.doc()?.header.str("DIMSTYLE", "Standard"),
    };
    s.add_entity(EntityKind::Tolerance(Tolerance { insert: Vec3::new(at.x, at.y, 0.0), dir: Vec2::from_angle(rotation), text, style }))
}

fn run_tolerance(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("tolerance", p, "at")?;
    let text = match str_param(p, "text") {
        Some(t) => t.to_string(),
        None => {
            let sym = match str_param(p, "symbol") {
                Some(n) => Some(symbol_of(n).ok_or_else(|| bad("tolerance", format!("ký hiệu không rõ: {n}")))?),
                None => None,
            };
            let mut datums: Vec<String> = match str_param(p, "datum") {
                Some(d) => d.split_whitespace().map(str::to_string).collect(),
                None => Vec::new(),
            };
            for k in ["datum1", "datum2", "datum3"] {
                if let Some(d) = str_param(p, k) {
                    datums.push(d.to_string());
                }
            }
            tolerance_text(sym, str_param(p, "tolerance1").unwrap_or(""), str_param(p, "tolerance2").unwrap_or(""), &datums)
        }
    };
    let h = add_tolerance(s, at, text, f64_or(p, "rotation", 0.0).to_radians(), str_param(p, "style"))?;
    Ok(json!({ "handle": h.hex() }))
}

fn plan_tolerance(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Kw { msg: "Ký hiệu dung sai".into(), kws: SYMBOL_KWS.iter().map(|(k, _)| *k).collect(), default: Some("POsition") }),
        1 => Some(Ask::Text { msg: "Dung sai 1 (vd: Ø0.05(M))".into(), default: Some(String::new()) }),
        2 => Some(Ask::Text { msg: "Dung sai 2".into(), default: Some(String::new()) }),
        3 => Some(Ask::Text { msg: "Chuẩn (vd: A B C)".into(), default: Some(String::new()) }),
        4 => Some(Ask::Point("Điểm đặt khung dung sai".into())),
        _ => None,
    }
}

fn flow_tolerance(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let sym = a.first().and_then(|x| symbol_of(x.text()));
    let datums: Vec<String> = a.get(3).map(|x| x.text().split_whitespace().map(str::to_string).collect()).unwrap_or_default();
    let text = tolerance_text(sym, a.get(1).map(Ans::text).unwrap_or(""), a.get(2).map(Ans::text).unwrap_or(""), &datums);
    let Some(Ans::Point(at)) = a.get(4) else { return Ok(json!({})) };
    add_tolerance(s, *at, text, 0.0, None)?;
    Ok(json!({}))
}

// ======================================================================= MLSTYLE

/// The current multiline style (CMLSTYLE), or STANDARD.
pub fn current_mline_style(s: &Session) -> MLineStyle {
    let Ok(d) = s.doc() else { return MLineStyle::default() };
    let cur = d.header.str("CMLSTYLE", "STANDARD");
    d.mline_style(&cur).or(d.mline_styles.first()).cloned().unwrap_or_default()
}

fn color_value(v: &Value) -> Option<Color> {
    match v {
        Value::Number(n) => n.as_i64().map(|i| Color::from_aci(i.clamp(0, 256) as i16)),
        Value::String(t) => match t.trim().to_ascii_lowercase().as_str() {
            "bylayer" => Some(Color::ByLayer),
            "byblock" => Some(Color::ByBlock),
            x => x.parse::<i16>().ok().map(|i| Color::from_aci(i.clamp(0, 256))),
        },
        _ => None,
    }
}

fn caps(v: Option<&str>) -> (bool, bool) {
    match v.map(|x| x.trim().to_ascii_lowercase()) {
        Some(x) if x == "line" || x == "l" => (true, false),
        Some(x) if x == "arc" || x == "a" => (false, true),
        Some(x) if x == "both" || x == "b" => (true, true),
        _ => (false, false),
    }
}

fn list_styles(s: &Session) -> Result<Value> {
    let d = s.doc()?;
    let cur = d.header.str("CMLSTYLE", "STANDARD");
    let styles: Vec<Value> = d
        .mline_styles
        .iter()
        .map(|m| json!({ "name": m.name, "description": m.description, "offsets": m.elements.iter().map(|e| e.offset).collect::<Vec<_>>(), "fill": m.fill }))
        .collect();
    let names: Vec<String> = d.mline_styles.iter().map(|m| m.name.clone()).collect();
    Ok(json!({ "current": cur, "styles": styles, "message": format!("Kiểu mline: {} (hiện hành: {cur})", names.join(", ")) }))
}

fn delete_style(s: &mut Session, name: &str) -> Result<Value> {
    if name.eq_ignore_ascii_case("STANDARD") {
        return Err(bad("mlstyle", "không xóa được kiểu STANDARD"));
    }
    let d = s.doc()?;
    let is_it = |e: &std::sync::Arc<cadcraft_doc::Entity>| matches!(&e.kind, EntityKind::MLine(m) if m.style.eq_ignore_ascii_case(name));
    let used = d.model.iter().any(is_it)
        || d.layouts.iter().any(|l| l.entities.iter().any(is_it))
        || d.blocks.values().any(|b| b.entities.iter().any(is_it));
    if used {
        return Err(bad("mlstyle", format!("kiểu {name} đang được dùng")));
    }
    let d = s.doc_mut()?;
    let before = d.mline_styles.len();
    d.mline_styles.retain(|m| !m.name.eq_ignore_ascii_case(name));
    if d.mline_styles.len() == before {
        return Err(bad("mlstyle", format!("không có kiểu {name}")));
    }
    if d.header.str("CMLSTYLE", "STANDARD").eq_ignore_ascii_case(name) {
        d.header.set_str("CMLSTYLE", "STANDARD");
    }
    Ok(json!({ "message": format!("Đã xóa kiểu {name}.") }))
}

fn run_mlstyle(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(n) = str_param(p, "delete") {
        return delete_style(s, n);
    }
    let Some(name) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) else { return list_styles(s) };
    if name.len() > 31 || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`']) {
        return Err(bad("mlstyle", format!("tên kiểu không hợp lệ: {name}")));
    }
    let has_def = ["elements", "description", "fill", "fillColor", "startCap", "endCap", "innerArcs", "miters"].iter().any(|k| p.get(*k).is_some());
    if has_def {
        let mut st = s.doc()?.mline_style(name).cloned().unwrap_or(MLineStyle { name: name.to_string(), ..MLineStyle::default() });
        if let Some(els) = p.get("elements").and_then(Value::as_array) {
            let mut v = Vec::new();
            for e in els.iter().take(16) {
                let el = match e {
                    Value::Number(n) => MLineElement { offset: n.as_f64().unwrap_or(0.0), ..Default::default() },
                    Value::Object(_) => MLineElement {
                        offset: e.get("offset").and_then(Value::as_f64).unwrap_or(0.0),
                        color: e.get("color").and_then(color_value).unwrap_or(Color::ByLayer),
                        linetype: e.get("linetype").and_then(Value::as_str).unwrap_or("BYLAYER").to_string(),
                    },
                    _ => continue,
                };
                if !el.offset.is_finite() {
                    return Err(bad("mlstyle", "độ lệch phải là số"));
                }
                // The element's linetype must be in the drawing: load it from the standard
                // library when it is not.
                let lt = el.linetype.trim().to_ascii_uppercase();
                if !matches!(lt.as_str(), "" | "BYLAYER" | "BYBLOCK" | "CONTINUOUS") && s.doc()?.linetype(&el.linetype).is_none() {
                    let found = cadcraft_doc::library::standard_linetypes().into_iter().find(|l| l.name.eq_ignore_ascii_case(&el.linetype));
                    match found {
                        Some(l) => s.doc_mut()?.linetypes.push(l),
                        None => return Err(bad("mlstyle", format!("không có kiểu đường {}", el.linetype))),
                    }
                }
                v.push(el);
            }
            if v.is_empty() {
                return Err(bad("mlstyle", "cần ít nhất một đường"));
            }
            v.sort_by(|a, b| b.offset.total_cmp(&a.offset));
            st.elements = v;
        }
        if let Some(t) = str_param(p, "description") {
            st.description = t.to_string();
        }
        st.fill = bool_or(p, "fill", st.fill);
        if let Some(c) = p.get("fillColor").and_then(color_value) {
            st.fill_color = c;
        }
        if p.get("startCap").is_some() {
            (st.start_line, st.start_outer_arc) = caps(str_param(p, "startCap"));
        }
        if p.get("endCap").is_some() {
            (st.end_line, st.end_outer_arc) = caps(str_param(p, "endCap"));
        }
        if let Some(b) = p.get("innerArcs").and_then(Value::as_bool) {
            st.start_inner_arcs = b;
            st.end_inner_arcs = b;
        }
        st.show_miters = bool_or(p, "miters", st.show_miters);
        let d = s.doc_mut()?;
        match d.mline_styles.iter_mut().find(|m| m.name.eq_ignore_ascii_case(name)) {
            Some(x) => *x = st,
            None => d.mline_styles.push(st),
        }
    } else if s.doc()?.mline_style(name).is_none() {
        return Err(bad("mlstyle", format!("không có kiểu {name}")));
    }
    if bool_or(p, "current", true) {
        let real = s.doc()?.mline_style(name).map(|m| m.name.clone()).unwrap_or_else(|| name.to_string());
        s.doc_mut()?.header.set_str("CMLSTYLE", &real);
    }
    Ok(json!({ "name": name, "message": format!("Kiểu mline {name} là hiện hành.") }))
}

fn plan_mlstyle(_s: &Session, a: &[Ans]) -> Option<Ask> {
    let mode = a.first().map(Ans::text).unwrap_or("");
    match (a.len(), mode) {
        (0, _) => Some(Ask::Kw { msg: "Kiểu mline".into(), kws: vec!["New", "Current", "Delete", "List"], default: Some("List") }),
        (1, "New") => Some(Ask::Text { msg: "Tên kiểu mới".into(), default: None }),
        (1, "Current") => Some(Ask::Text { msg: "Tên kiểu hiện hành".into(), default: None }),
        (1, "Delete") => Some(Ask::Text { msg: "Tên kiểu cần xóa".into(), default: None }),
        (2, "New") => {
            Some(Ask::Text { msg: "Độ lệch các đường, cách nhau dấu phẩy (vd: 110,0,-110)".into(), default: Some("0.5,-0.5".into()) })
        }
        (3, "New") => Some(Ask::Kw { msg: "Đầu mút".into(), kws: vec!["None", "Line", "Arc"], default: Some("None") }),
        (4, "New") => Some(Ask::Kw { msg: "Tô nền".into(), kws: vec!["No", "Yes"], default: Some("No") }),
        _ => None,
    }
}

fn flow_mlstyle(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let name = a.get(1).map(Ans::text).unwrap_or("").to_string();
    match a.first().map(Ans::text).unwrap_or("List") {
        "New" => {
            let offs: Vec<f64> = a.get(2).map(Ans::text).unwrap_or("").split([',', ';', ' ']).filter_map(|x| x.trim().parse::<f64>().ok()).collect();
            let cap = a.get(3).map(Ans::text).unwrap_or("None").to_ascii_lowercase();
            run_mlstyle(s, &json!({ "name": name, "elements": offs, "startCap": cap, "endCap": cap, "fill": a.get(4).map(Ans::text) == Some("Yes") }))
        }
        "Current" => run_mlstyle(s, &json!({ "name": name })),
        "Delete" => delete_style(s, &name),
        _ => list_styles(s),
    }
}

// ======================================================================== MLEDIT

const TOOLS: &[&str] = &[
    "ClosedCross",
    "OpenCross",
    "MergedCross",
    "closedTee",
    "openTEe",
    "mergedtEe",
    "CornerJoint",
    "AddVertex",
    "DeleteVertex",
    "CutSingle",
    "cutAll",
    "WeldAll",
];

fn tool_of(t: &str) -> Option<&'static str> {
    let k = t.trim().to_ascii_lowercase();
    TOOLS.iter().copied().find(|x| x.to_ascii_lowercase() == k).or_else(|| super::flow::match_kw(t, TOOLS))
}

/// The multiline nearest `p` in the current space (other than `skip`).
fn pick(s: &Session, p: Vec2, skip: Option<Handle>) -> Result<(Handle, MLine)> {
    let d = s.doc()?;
    let space = d.space(&s.space()).ok_or_else(|| bad("mledit", "no space"))?;
    let mut best: Option<(f64, Handle, MLine)> = None;
    for e in space.iter() {
        let EntityKind::MLine(m) = &e.kind else { continue };
        if Some(e.handle) == skip || !d.is_visible(e) {
            continue;
        }
        let dist = crate::select::entity_distance(d, e, p, 1e-3);
        if best.as_ref().is_none_or(|(b, _, _)| dist < *b) {
            best = Some((dist, e.handle, m.clone()));
        }
    }
    best.map(|(_, h, m)| (h, m)).ok_or_else(|| bad("mledit", "không có mline nào ở đó"))
}

/// Segment of `m` nearest `p`, and how far along it `p` projects.
fn nearest_seg(m: &MLine, p: Vec2) -> Option<(usize, f64)> {
    let n = m.vertices.len();
    (0..m.segment_count())
        .filter_map(|i| {
            let a = m.vertices.get(i)?.p;
            let b = m.vertices.get((i + 1) % n)?.p;
            let ab = b - a;
            let l2 = ab.dot(ab).max(1e-18);
            let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
            Some((i, (a + ab * t).dist(p), t * l2.sqrt()))
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(i, _, t)| (i, t))
}

/// Element `e` of segment `i`: its start and direction.
fn elem_line(m: &MLine, i: usize, e: usize) -> Option<(Vec2, Vec2)> {
    Some((m.element_point(i, e)?, m.vertices.get(i)?.dir))
}

/// Where the line `a + d·t` meets the line through `p` along `q`.
fn hit(a: Vec2, d: Vec2, p: Vec2, q: Vec2) -> Option<f64> {
    let den = d.cross(q);
    if den.abs() < 1e-12 {
        return None;
    }
    Some((p - a).cross(q) / den)
}

/// Element indices from the most negative offset to the most positive.
fn order(m: &MLine) -> Vec<usize> {
    let off = m.offsets();
    let mut idx: Vec<usize> = (0..off.len()).collect();
    idx.sort_by(|a, b| off.get(*a).unwrap_or(&0.0).total_cmp(off.get(*b).unwrap_or(&0.0)));
    idx
}

fn outer(m: &MLine) -> Vec<usize> {
    let o = order(m);
    let mut v: Vec<usize> = o.first().into_iter().chain(o.last()).copied().collect();
    v.dedup();
    v
}

/// Hide `[a, b]` of element `e` on segment `i`.
fn cut(m: &mut MLine, i: usize, e: usize, a: f64, b: f64) {
    let (a, b) = (a.min(b), a.max(b));
    let mut out = Vec::new();
    for (x, y) in m.element_pieces(i, e) {
        if y <= a || x >= b {
            out.push((x, y));
            continue;
        }
        if x < a {
            out.push((x, a));
        }
        if y > b {
            out.push((b, y));
        }
    }
    let full = m.element_span(i, e).map_or(0.0, |(_, l)| l);
    let out: Vec<(f64, f64)> = out.into_iter().map(|(x, y)| (x, if y >= full - 1e-9 { f64::MAX } else { y })).collect();
    m.set_element_pieces(i, e, &out);
}

/// Show `[a, b]` of element `e` on segment `i` again.
fn weld(m: &mut MLine, i: usize, e: usize, a: f64, b: f64) {
    let (a, b) = (a.min(b), a.max(b));
    let mut ps = m.element_pieces(i, e);
    ps.push((a, b));
    ps.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (x, y) in ps {
        match out.last_mut() {
            Some(l) if x <= l.1 + 1e-9 => l.1 = l.1.max(y),
            _ => out.push((x, y)),
        }
    }
    let full = m.element_span(i, e).map_or(0.0, |(_, l)| l);
    let out: Vec<(f64, f64)> = out.into_iter().map(|(x, y)| (x.max(0.0), if y >= full - 1e-9 { f64::MAX } else { y })).collect();
    m.set_element_pieces(i, e, &out);
}

/// Cut elements `which` of `m` (segment `i`) where they cross `other`'s outer lines (segment `j`).
fn cut_across(m: &mut MLine, i: usize, which: &[usize], other: &MLine, j: usize) {
    let lines: Vec<(Vec2, Vec2)> = outer(other).iter().filter_map(|e| elem_line(other, j, *e)).collect();
    for &e in which {
        let Some((a, d)) = elem_line(m, i, e) else { continue };
        let ts: Vec<f64> = lines.iter().filter_map(|(p, q)| hit(a, d, *p, *q)).collect();
        if let (Some(t0), Some(t1)) = (ts.iter().copied().reduce(f64::min), ts.iter().copied().reduce(f64::max)) {
            cut(m, i, e, t0, t1);
        }
    }
}

/// Rebuild `m` through `pts` (cuts dropped).
fn rebuilt(m: &MLine, pts: &[Vec2]) -> MLine {
    let off = m.offsets();
    let mut n = m.clone();
    n.vertices.clear();
    n.set_points(pts, &off);
    n
}

/// Trim or extend `m` so that the end on the far side of `pick` lands on `x`. Returns the
/// new multiline and whether `x` is its last vertex (else its first).
fn trim_to(m: &MLine, pick: Vec2, x: Vec2) -> Option<(MLine, bool)> {
    let (i, _) = nearest_seg(m, pick)?;
    let a = m.vertices.get(i)?.p;
    let dir = m.vertices.get(i)?.dir;
    let pts = m.points();
    let keep_before = (pick - a).dot(dir) < (x - a).dot(dir);
    let new_pts: Vec<Vec2> = if keep_before {
        pts.iter().take(i + 1).copied().chain(std::iter::once(x)).collect()
    } else {
        std::iter::once(x).chain(pts.iter().skip(i + 1).copied()).collect()
    };
    if new_pts.len() < 2 || new_pts.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(p, q)| p.dist(*q) < 1e-9)) {
        return None;
    }
    let mut n = rebuilt(m, &new_pts);
    n.closed = false;
    // The joined end is open (no cap), as AutoCAD leaves it.
    if keep_before {
        n.no_end_caps = true;
    } else {
        n.no_start_caps = true;
    }
    Some((n, keep_before))
}

/// Give the end vertex of `m` (last or first) the miter `miter`, keeping the offsets.
fn set_end_miter(m: &mut MLine, last: bool, miter: Vec2) {
    let off = m.offsets();
    let n = m.vertices.len();
    let Some(v) = m.vertices.get_mut(if last { n.saturating_sub(1) } else { 0 }) else { return };
    let mut mt = miter.normalized();
    if mt.dot(v.dir.perp()) < 0.0 {
        mt = -mt;
    }
    let k = mt.dot(v.dir.perp());
    if k.abs() < 0.05 {
        return;
    }
    v.miter = mt;
    for (ps, o) in v.params.iter_mut().zip(&off) {
        if let Some(f) = ps.first_mut() {
            *f = o / k;
        }
    }
}

fn store(s: &mut Session, h: Handle, m: MLine) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::MLine(m))?;
    Ok(())
}

/// Run an MLEDIT tool.
pub fn mledit(s: &mut Session, tool: &str, p1: Vec2, p2: Option<Vec2>) -> Result<String> {
    let tool = tool_of(tool).ok_or_else(|| bad("mledit", format!("công cụ không rõ: {tool}")))?;
    let (h1, mut m1) = pick(s, p1, None)?;
    let (i1, t1) = nearest_seg(&m1, p1).ok_or_else(|| bad("mledit", "mline không có đoạn"))?;
    let need_two = !matches!(tool, "AddVertex" | "DeleteVertex");
    let p2v = if need_two { Some(p2.ok_or_else(|| bad("mledit", "cần điểm thứ hai (`p2`)"))?) } else { p2 };
    let ne = m1.vertices.first().map_or(0, |v| v.params.len());
    match tool {
        "AddVertex" => {
            let a = m1.vertices.get(i1).map(|v| v.p).unwrap_or_default();
            let dir = m1.vertices.get(i1).map(|v| v.dir).unwrap_or(Vec2::X);
            let mut pts = m1.points();
            pts.insert(i1 + 1, a + dir * t1);
            store(s, h1, rebuilt(&m1, &pts))?;
            Ok("Đã thêm đỉnh.".into())
        }
        "DeleteVertex" => {
            let pts = m1.points();
            if pts.len() <= 2 {
                return Err(bad("mledit", "mline chỉ còn hai đỉnh"));
            }
            let k = pts.iter().enumerate().min_by(|a, b| a.1.dist(p1).total_cmp(&b.1.dist(p1))).map_or(0, |(k, _)| k);
            let pts: Vec<Vec2> = pts.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, p)| *p).collect();
            store(s, h1, rebuilt(&m1, &pts))?;
            Ok("Đã xóa đỉnh.".into())
        }
        "CutSingle" | "cutAll" | "WeldAll" => {
            let q = p2v.unwrap_or(p1);
            let dir = m1.vertices.get(i1).map(|v| v.dir).unwrap_or(Vec2::X);
            let starts: Vec<Vec2> = (0..ne).map(|e| elem_line(&m1, i1, e).map_or(Vec2::ZERO, |(a, _)| a)).collect();
            let along = |e: usize, p: Vec2| starts.get(e).map_or(0.0, |a| (p - *a).dot(dir));
            let elems: Vec<usize> = if tool == "CutSingle" {
                // The element nearest the first pick.
                let best = (0..ne)
                    .filter_map(|e| elem_line(&m1, i1, e).map(|(a, d)| (e, (p1 - a).cross(d).abs())))
                    .min_by(|x, y| x.1.total_cmp(&y.1))
                    .map(|(e, _)| e);
                best.into_iter().collect()
            } else {
                (0..ne).collect()
            };
            for e in elems {
                let (a, b) = (along(e, p1), along(e, q));
                if tool == "WeldAll" {
                    weld(&mut m1, i1, e, a, b);
                } else {
                    cut(&mut m1, i1, e, a, b);
                }
            }
            store(s, h1, m1)?;
            Ok(if tool == "WeldAll" { "Đã nối lại.".into() } else { "Đã cắt.".into() })
        }
        _ => {
            let p2 = p2v.unwrap_or(p1);
            let (h2, mut m2) = pick(s, p2, Some(h1))?;
            let (i2, _) = nearest_seg(&m2, p2).ok_or_else(|| bad("mledit", "mline không có đoạn"))?;
            let c1 = m1.vertices.get(i1).map(|v| (v.p, v.dir)).ok_or_else(|| bad("mledit", "mline hỏng"))?;
            let c2 = m2.vertices.get(i2).map(|v| (v.p, v.dir)).ok_or_else(|| bad("mledit", "mline hỏng"))?;
            match tool {
                "ClosedCross" | "OpenCross" | "MergedCross" => {
                    let all: Vec<usize> = (0..ne).collect();
                    let o1 = outer(&m1);
                    let first: &[usize] = if tool == "MergedCross" { &o1 } else { &all };
                    let m2c = m2.clone();
                    cut_across(&mut m1, i1, first, &m2c, i2);
                    if tool != "ClosedCross" {
                        let o2 = outer(&m2);
                        cut_across(&mut m2, i2, &o2, &m1, i1);
                        store(s, h2, m2)?;
                    }
                    store(s, h1, m1)?;
                    Ok("Đã giao cắt.".into())
                }
                "CornerJoint" => {
                    let t = hit(c1.0, c1.1, c2.0, c2.1).ok_or_else(|| bad("mledit", "hai mline song song"))?;
                    let x = c1.0 + c1.1 * t;
                    let (mut n1, last1) = trim_to(&m1, p1, x).ok_or_else(|| bad("mledit", "không nối được góc"))?;
                    let (mut n2, last2) = trim_to(&m2, p2, x).ok_or_else(|| bad("mledit", "không nối được góc"))?;
                    // The corner's miter: the bisector of the path in along the first and out
                    // along the second.
                    let d_in = (x - p1).normalized();
                    let d_out = (p2 - x).normalized();
                    let miter = d_in.perp() + d_out.perp();
                    let miter = if miter.len() < 1e-6 { d_in.perp() } else { miter };
                    set_end_miter(&mut n1, last1, miter);
                    set_end_miter(&mut n2, last2, miter);
                    store(s, h1, n1)?;
                    store(s, h2, n2)?;
                    Ok("Đã nối góc.".into())
                }
                _ => {
                    // Tees: the first mline is the stem, the second the bar.
                    let o2 = order(&m2);
                    let side = (p1 - c2.0).cross(c2.1);
                    // The bar's element on the stem's side: positive offsets are on the left.
                    let near = if side < 0.0 { o2.last() } else { o2.first() }.copied().ok_or_else(|| bad("mledit", "mline hỏng"))?;
                    let (np, nd) = elem_line(&m2, i2, near).ok_or_else(|| bad("mledit", "mline hỏng"))?;
                    let merged = tool == "mergedtEe";
                    let target = if merged { c2 } else { (np, nd) };
                    let t = hit(c1.0, c1.1, target.0, target.1).ok_or_else(|| bad("mledit", "hai mline song song"))?;
                    let x = c1.0 + c1.1 * t;
                    let (mut n1, last) = trim_to(&m1, p1, x).ok_or_else(|| bad("mledit", "không nối được chữ T"))?;
                    set_end_miter(&mut n1, last, c2.1);
                    if merged {
                        // The stem's outer lines stop at the bar's near line.
                        let seg = if last { n1.segment_count().saturating_sub(1) } else { 0 };
                        for e in outer(&n1) {
                            let Some((a, d)) = elem_line(&n1, seg, e) else { continue };
                            let Some(tn) = hit(a, d, np, nd) else { continue };
                            let full = n1.element_span(seg, e).map_or(0.0, |(_, l)| l);
                            if last {
                                n1.set_element_pieces(seg, e, &[(0.0, tn.clamp(0.0, full))]);
                            } else {
                                n1.set_element_pieces(seg, e, &[(tn.clamp(0.0, full), f64::MAX)]);
                            }
                        }
                    }
                    if tool != "closedTee" {
                        let which: Vec<usize> = if merged {
                            let off = m2.offsets();
                            (0..off.len()).filter(|e| off.get(*e).is_some_and(|o| if side < 0.0 { *o > 1e-9 } else { *o < -1e-9 })).collect()
                        } else {
                            vec![near]
                        };
                        let seg1 = if last { n1.segment_count().saturating_sub(1) } else { 0 };
                        cut_across(&mut m2, i2, &which, &n1, seg1);
                        store(s, h2, m2)?;
                    }
                    store(s, h1, n1)?;
                    Ok("Đã nối chữ T.".into())
                }
            }
        }
    }
}

fn run_mledit(s: &mut Session, p: &Value) -> Result<Value> {
    let tool = str_param(p, "tool").ok_or_else(|| bad("mledit", "`tool` is required"))?;
    let p1 = point_req("mledit", p, "p1")?;
    let msg = mledit(s, tool, p1, point_param(p, "p2"))?;
    Ok(json!({ "message": msg }))
}

fn plan_mledit(_s: &Session, a: &[Ans]) -> Option<Ask> {
    let tool = a.first().map(Ans::text).unwrap_or("");
    let one = matches!(tool, "AddVertex" | "DeleteVertex");
    let cut = matches!(tool, "CutSingle" | "cutAll" | "WeldAll");
    match a.len() {
        0 => Some(Ask::Kw { msg: "Công cụ sửa mline".into(), kws: TOOLS.to_vec(), default: Some("CornerJoint") }),
        1 => Some(Ask::Point(if cut { "Chọn mline (điểm cắt thứ nhất)".into() } else { "Chọn mline thứ nhất".into() })),
        2 if !one => Some(Ask::Point(if cut { "Điểm cắt thứ hai".into() } else { "Chọn mline thứ hai".into() })),
        _ => None,
    }
}

fn flow_mledit(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let tool = a.first().map(Ans::text).unwrap_or("").to_string();
    let pt = |i: usize| if let Some(Ans::Point(p)) = a.get(i) { Some(*p) } else { None };
    let Some(p1) = pt(1) else { return Ok(json!({})) };
    let msg = mledit(s, &tool, p1, pt(2))?;
    Ok(json!({ "message": msg }))
}

/// Lines and caps of a multiline, as separate objects (EXPLODE).
pub fn explode_mline(d: &cadcraft_doc::Drawing, e: &cadcraft_doc::Entity, m: &MLine) -> Vec<cadcraft_doc::Entity> {
    let st = d.mline_style(&m.style);
    let g = m.geometry(st);
    let mut out = Vec::new();
    for (el, l) in &g.lines {
        let mut c = e.common.clone();
        if let Some(x) = st.and_then(|s| s.elements.get(*el)) {
            if !matches!(x.color, Color::ByLayer | Color::ByBlock) {
                c.color = x.color;
            }
            let lt = x.linetype.trim();
            if !lt.is_empty() && !lt.eq_ignore_ascii_case("bylayer") && !lt.eq_ignore_ascii_case("byblock") {
                c.linetype = lt.to_string();
            }
        }
        out.push(cadcraft_doc::Entity { handle: Handle(0), common: c, kind: line(l[0], l[1]) });
    }
    for cap in &g.caps {
        let kind =
            if cap.len() == 2 { line(cap[0], cap[1]) } else { lwpoly(cap.iter().map(|p| cadcraft_geom::PolyVertex::new(*p)).collect(), false) };
        out.push(cadcraft_doc::Entity { handle: Handle(0), common: e.common.clone(), kind });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mline_of(s: &Session, h: Handle) -> MLine {
        match &s.doc().unwrap().entity(h).unwrap().kind {
            EntityKind::MLine(m) => m.clone(),
            k => panic!("{k:?}"),
        }
    }

    fn handle(r: &Value) -> Handle {
        Handle::parse_hex(r["handle"].as_str().or(r["handles"][0].as_str()).unwrap()).unwrap()
    }

    #[test]
    fn tolerance_from_parts() {
        let mut s = Session::new();
        let r = s.execute("tolerance", &json!({ "at": [10, 10], "symbol": "position", "tolerance1": "Ø0.05(M)", "datum": "A B" })).unwrap();
        let h = handle(&r);
        let EntityKind::Tolerance(t) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
        assert_eq!(t.text, "{\\Fgdt;j}%%v{\\Fgdt;n}0.05{\\Fgdt;m}%%v%%vA%%vB");
        let b = s.doc().unwrap().extents(&cadcraft_doc::Space::Model);
        assert!(b.width() > 0.5, "the frame has a size: {b:?}");
        // Interactive.
        s.start("tolerance").unwrap();
        for t in ["FL", "0.02", "", ""] {
            s.input(crate::Input::Text(t.into())).unwrap();
        }
        s.input(crate::Input::Point(Vec2::new(0.0, 50.0))).unwrap();
        assert!(s.running.is_none());
        let n = s.doc().unwrap().model.iter().filter(|e| matches!(&e.kind, EntityKind::Tolerance(t) if t.text == "{\\Fgdt;c}%%v0.02")).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn mlstyle_and_mline_use_it() {
        let mut s = Session::new();
        s.execute("mlstyle", &json!({ "name": "TUONG220", "elements": [110, 0, -110], "startCap": "line", "endCap": "line" })).unwrap();
        let r = s.execute("mline", &json!({ "points": [[0, 0], [5000, 0]], "scale": 1, "justification": "zero" })).unwrap();
        let m = mline_of(&s, handle(&r));
        assert_eq!(m.style, "TUONG220");
        assert_eq!(m.offsets().len(), 3);
        // Explode: 3 lines and 2 caps.
        s.execute("explode", &json!({ "handles": [handle(&r).hex()] })).unwrap();
        let lines = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
        assert_eq!(lines, 5);
        assert!(s.execute("mlstyle", &json!({ "delete": "STANDARD" })).is_err());
        let r = s.execute("mlstyle", &json!({})).unwrap();
        assert_eq!(r["current"], "TUONG220");
    }

    #[test]
    fn mledit_tools() {
        let mut s = Session::new();
        let a = handle(&s.execute("mline", &json!({ "points": [[0, 0], [100, 0]], "scale": 10, "justification": "zero" })).unwrap());
        let b = handle(&s.execute("mline", &json!({ "points": [[50, -50], [50, 50]], "scale": 10, "justification": "zero" })).unwrap());
        // Open cross: the first is cut through, the second's outer lines too.
        s.execute("mledit", &json!({ "tool": "openCross", "p1": [20, 0], "p2": [50, 30] })).unwrap();
        assert_eq!(mline_of(&s, a).geometry(None).lines.len(), 4);
        assert_eq!(mline_of(&s, b).geometry(None).lines.len(), 4);
        s.execute("undo", &json!({})).unwrap();
        // Closed tee: the stem stops on the bar's near line (y = 5 for a bar at y = 0).
        let c = handle(&s.execute("mline", &json!({ "points": [[200, 0], [300, 0]], "scale": 10, "justification": "zero" })).unwrap());
        let st = handle(&s.execute("mline", &json!({ "points": [[250, 60], [250, 20]], "scale": 10, "justification": "zero" })).unwrap());
        s.execute("mledit", &json!({ "tool": "closedTee", "p1": [250, 50], "p2": [220, 0] })).unwrap();
        let g = mline_of(&s, st).geometry(None);
        for (_, l) in &g.lines {
            assert!((l[1].y - 5.0).abs() < 1e-6, "{l:?}");
        }
        assert_eq!(mline_of(&s, c).geometry(None).lines.len(), 2, "closed tee leaves the bar");
        // Corner joint: both meet with a mitred corner.
        let d1 = handle(&s.execute("mline", &json!({ "points": [[400, 0], [480, 0]], "scale": 10, "justification": "zero" })).unwrap());
        let d2 = handle(&s.execute("mline", &json!({ "points": [[500, 20], [500, 100]], "scale": 10, "justification": "zero" })).unwrap());
        s.execute("mledit", &json!({ "tool": "CJ", "p1": [420, 0], "p2": [500, 80] })).unwrap();
        let g1 = mline_of(&s, d1).geometry(None);
        let g2 = mline_of(&s, d2).geometry(None);
        let ends1: Vec<Vec2> = g1.lines.iter().map(|(_, l)| l[1]).collect();
        let ends2: Vec<Vec2> = g2.lines.iter().map(|(_, l)| l[0]).collect();
        for p in &ends1 {
            assert!(ends2.iter().any(|q| q.dist(*p) < 1e-6), "{ends1:?} vs {ends2:?}");
        }
        // Cut all, weld all.
        s.execute("mledit", &json!({ "tool": "cutAll", "p1": [210, 0], "p2": [215, 0] })).unwrap();
        assert_eq!(mline_of(&s, c).geometry(None).lines.len(), 4);
        s.execute("mledit", &json!({ "tool": "weldAll", "p1": [205, 0], "p2": [220, 0] })).unwrap();
        assert_eq!(mline_of(&s, c).geometry(None).lines.len(), 2);
        // Vertices.
        s.execute("mledit", &json!({ "tool": "addVertex", "p1": [260, 0] })).unwrap();
        assert_eq!(mline_of(&s, c).vertices.len(), 3);
        s.execute("mledit", &json!({ "tool": "deleteVertex", "p1": [260, 0] })).unwrap();
        assert_eq!(mline_of(&s, c).vertices.len(), 2);
    }
}
