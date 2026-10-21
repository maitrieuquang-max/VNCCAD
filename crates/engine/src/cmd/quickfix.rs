//! VNCCad: commands added after the first engineer tests — editing an object from a dialog
//! (`entity.replace`), command aliases (ALIASEDIT, REINIT), hiding and isolating objects
//! (HIDEOBJECTS, ISOLATEOBJECTS, UNISOLATEOBJECTS) and pasting as a block (PASTEBLOCK).

use cadcraft_doc::{Entity, Handle};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("entity.replace", "Edit Object", run_replace)
            .params("{entity: <the whole entity as JSON, same handle>} (from the edit dialogs)"),
        CommandSpec::new("aliasedit", "Edit Aliases...", run_aliasedit)
            .menu(&["Tools", "Customize", "Edit Command Aliases..."])
            .alias(&["phimtat"])
            .params("{alias, command} adds or changes | {remove: alias} | {} lists the user's aliases")
            .noundo()
            .enabled(always),
        CommandSpec::new("reinit", "Reinitialize", run_reinit)
            .alias(&["napphimtat"])
            .params("{aliases?: {alias: command}} (the UI reloads the saved aliases)")
            .noundo()
            .enabled(always),
        CommandSpec::new("hideobjects", "Hide Objects", run_hide)
            .menu(&["Tools", "Isolate", "Hide Objects"])
            .alias(&["hob", "anobj"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(Flow::new("hideobjects", plan_sel, flow_hide))))
            .enabled(has_doc),
        CommandSpec::new("isolateobjects", "Isolate Objects", run_isolate)
            .menu(&["Tools", "Isolate", "Isolate Objects"])
            .alias(&["isoobj", "isolate"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(Flow::new("isolateobjects", plan_sel, flow_isolate))))
            .enabled(has_doc),
        CommandSpec::new("unisolateobjects", "End Object Isolation", run_unisolate)
            .menu(&["Tools", "Isolate", "End Object Isolation"])
            .alias(&["unisolate", "uniso", "unhide"])
            .params("{} (shows every hidden object of the current space)")
            .enabled(has_doc),
        CommandSpec::new("plot.window", "Plot Window", run_plot_window)
            .params("{p1, p2} (the current page setup plots this window; the Plot dialog opens again)")
            .interactive(|_| Ok(Box::new(Flow::new("plot.window", plan_plot_window, flow_plot_window))))
            .enabled(has_doc),
        CommandSpec::new("pasteblock", "Paste as Block", run_pasteblock)
            .menu(&["Edit", "Paste as Block"])
            .key("Cmd+Shift+V")
            .params("{at: [x,y]}")
            .interactive(|_| Ok(Box::new(Flow::new("pasteblock", plan_pasteblock, flow_pasteblock))))
            .enabled(has_doc),
    ]
}

// ---------------------------------------------------------------- entity.replace

fn run_replace(s: &mut Session, p: &Value) -> Result<Value> {
    let v = p.get("entity").cloned().ok_or_else(|| bad("entity.replace", "`entity` is required"))?;
    let e: Entity = serde_json::from_value(v).map_err(|e| bad("entity.replace", e.to_string()))?;
    let h = e.handle;
    let d = s.doc_mut()?;
    if d.entity(h).is_none() {
        return Err(bad("entity.replace", format!("no object {}", h.hex())));
    }
    if !e.common.layer.is_empty() {
        d.ensure_layer(&e.common.layer);
    }
    d.modify_entity(h, |x| *x = e).map_err(|m| bad("entity.replace", m.to_string()))?;
    Ok(json!({ "handle": h.hex() }))
}

// ---------------------------------------------------------------- aliases

fn aliases_json(s: &Session) -> Value {
    let list: Vec<Value> =
        s.user_aliases.iter().map(|(a, c)| json!({ "alias": a.to_ascii_uppercase(), "command": c.to_ascii_uppercase() })).collect();
    let msg = if list.is_empty() {
        "Chưa có phím tắt riêng (ALIASEDIT để thêm).".to_string()
    } else {
        format!(
            "Phím tắt riêng: {}",
            s.user_aliases.iter().map(|(a, c)| format!("{}={}", a.to_ascii_uppercase(), c.to_ascii_uppercase())).collect::<Vec<_>>().join(", ")
        )
    };
    json!({ "aliases": list, "message": msg })
}

fn valid_alias(a: &str) -> bool {
    !a.is_empty() && a.len() <= 32 && a.chars().all(|c| c.is_alphanumeric() || "_-$".contains(c))
}

fn run_aliasedit(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(a) = str_param(p, "remove") {
        s.user_aliases.remove(&a.trim().to_ascii_lowercase());
        return Ok(aliases_json(s));
    }
    if let (Some(a), Some(c)) = (str_param(p, "alias"), str_param(p, "command")) {
        let a = a.trim().to_ascii_lowercase();
        let c = c.trim().trim_start_matches(['_', '.']).to_ascii_lowercase();
        if !valid_alias(&a) {
            return Err(bad("aliasedit", "phím tắt chỉ gồm chữ, số, _ - $ (tối đa 32 ký tự)"));
        }
        let known = find_command(&resolve_alias(&c)).is_some() || s.lisp.has_command(&c);
        if !known {
            return Err(bad("aliasedit", format!("không có lệnh `{}`", c.to_ascii_uppercase())));
        }
        if a == c {
            return Err(bad("aliasedit", "phím tắt trùng tên lệnh"));
        }
        s.user_aliases.insert(a, c);
    }
    Ok(aliases_json(s))
}

fn run_reinit(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(m) = p.get("aliases").and_then(Value::as_object) {
        s.user_aliases = m
            .iter()
            .filter_map(|(a, c)| c.as_str().map(|c| (a.trim().to_ascii_lowercase(), c.trim().to_ascii_lowercase())))
            .filter(|(a, c)| valid_alias(a) && !c.is_empty())
            .collect();
    }
    let n = s.user_aliases.len();
    let mut out = aliases_json(s);
    if let Some(o) = out.as_object_mut() {
        o.insert("message".into(), json!(format!("Đã nạp lại {n} phím tắt riêng.")));
    }
    Ok(out)
}

// ---------------------------------------------------------------- hide / isolate

fn plan_sel(_s: &Session, a: &[Ans]) -> Option<Ask> {
    a.is_empty().then(|| Ask::Select("Chọn đối tượng".into()))
}

fn set_visible(s: &mut Session, hs: &[Handle], on: bool) -> Result<usize> {
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let changed = d.entity(*h).is_some_and(|e| e.common.visible != on);
        if changed && d.modify_entity(*h, |e| e.common.visible = on).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

fn hide(s: &mut Session, hs: &[Handle]) -> Result<Value> {
    if hs.is_empty() {
        return Err(bad("hideobjects", "chưa chọn đối tượng"));
    }
    let n = set_visible(s, hs, false)?;
    s.set_selection(Vec::new());
    Ok(json!({ "hidden": n, "message": format!("Đã ẩn {n} đối tượng (UNISOLATEOBJECTS để hiện lại).") }))
}

fn isolate(s: &mut Session, keep: &[Handle]) -> Result<Value> {
    if keep.is_empty() {
        return Err(bad("isolateobjects", "chưa chọn đối tượng"));
    }
    let space = s.state()?.edit_space();
    let d = s.doc()?;
    let keep: std::collections::HashSet<Handle> = keep.iter().copied().collect();
    let others: Vec<Handle> = d
        .space(&space)
        .map(|st| {
            st.iter()
                .filter(|e| e.common.visible && !keep.contains(&e.handle) && !matches!(e.kind, cadcraft_doc::EntityKind::Viewport(_)))
                .map(|e| e.handle)
                .collect()
        })
        .unwrap_or_default();
    let n = set_visible(s, &others, false)?;
    s.set_selection(Vec::new());
    Ok(json!({ "hidden": n, "message": format!("Chỉ còn hiện {} đối tượng; đã ẩn {n} (UNISOLATEOBJECTS để hiện lại).", keep.len()) }))
}

fn run_hide(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    hide(s, &hs)
}
fn flow_hide(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(|x| x.sel().to_vec()).unwrap_or_default();
    hide(s, &hs)
}
fn run_isolate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    isolate(s, &hs)
}
fn flow_isolate(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let hs = a.first().map(|x| x.sel().to_vec()).unwrap_or_default();
    isolate(s, &hs)
}

fn run_unisolate(s: &mut Session, _p: &Value) -> Result<Value> {
    let space = s.state()?.edit_space();
    let d = s.doc()?;
    let hidden: Vec<Handle> = d.space(&space).map(|st| st.iter().filter(|e| !e.common.visible).map(|e| e.handle).collect()).unwrap_or_default();
    let n = set_visible(s, &hidden, true)?;
    Ok(json!({ "shown": n, "message": format!("Đã hiện lại {n} đối tượng.") }))
}

// ---------------------------------------------------------------- plot window

fn plan_plot_window(_s: &Session, a: &[Ans]) -> Option<Ask> {
    match a.len() {
        0 => Some(Ask::Point("Góc thứ nhất của vùng in".into())),
        1 => Some(Ask::Point("Góc đối diện".into())),
        _ => None,
    }
}

fn plot_window(s: &mut Session, a: Vec2, b: Vec2) -> Result<Value> {
    let r = s.execute("pagesetup", &json!({ "window": [[a.x, a.y], [b.x, b.y]], "plotArea": "window" }))?;
    s.ui_requests.push(("plot".into(), Value::Null));
    Ok(r)
}

fn run_plot_window(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_req("plot.window", p, "p1")?;
    let b = point_req("plot.window", p, "p2")?;
    plot_window(s, a, b)
}

fn flow_plot_window(s: &mut Session, a: &[Ans]) -> Result<Value> {
    match (a.first(), a.get(1)) {
        (Some(Ans::Point(p)), Some(Ans::Point(q))) => plot_window(s, *p, *q),
        _ => Err(bad("plot.window", "cần hai góc")),
    }
}

// ---------------------------------------------------------------- pasteblock

fn plan_pasteblock(_s: &Session, a: &[Ans]) -> Option<Ask> {
    a.is_empty().then(|| Ask::Point("Điểm chèn".into()))
}

fn pasteblock(s: &mut Session, at: Vec2) -> Result<Value> {
    if s.clipboard.is_empty() {
        return Err(bad("pasteblock", "clipboard trống (Ctrl+C hoặc Ctrl+X trước)"));
    }
    let off = at - s.clipboard_base;
    let hs = super::edit::paste(s, off)?;
    let d = s.doc()?;
    let mut name = String::new();
    for i in 0u32..10_000 {
        let h = d.header.str("HANDSEED", "1");
        let n = format!(
            "A$C{:08X}",
            (u64::from_str_radix(h.trim(), 16).unwrap_or(1).wrapping_mul(2_654_435_761).wrapping_add(u64::from(i))) & 0xFFFF_FFFF
        );
        if d.block(&n).is_none() {
            name = n;
            break;
        }
    }
    if name.is_empty() {
        return Err(bad("pasteblock", "không đặt được tên block"));
    }
    let ins = super::blocks::make_block(s, &name, at, &hs, "convert", "")?;
    if let Some(h) = ins {
        s.set_selection(vec![h]);
    }
    Ok(json!({ "block": name, "insert": ins.map(|h| h.hex()), "message": format!("Đã dán thành block {name}.") }))
}

fn run_pasteblock(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_param(p, "at").unwrap_or(s.clipboard_base);
    pasteblock(s, at)
}
fn flow_pasteblock(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let at = match a.first() {
        Some(Ans::Point(p)) => *p,
        _ => s.clipboard_base,
    };
    pasteblock(s, at)
}

#[cfg(test)]
#[path = "quickfix_tests.rs"]
mod tests;
