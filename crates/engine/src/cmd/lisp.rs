//! VNCCad: AutoLISP and scripts — APPLOAD, LISP, SCRIPT.

use std::collections::VecDeque;

use serde_json::{Value, json};

use super::*;
use crate::lisp::{self, Run, V};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("appload", "Load Application (LISP)", run_appload)
            .menu(&["Tools", "Load Application (LISP)..."])
            .alias(&["ap", "napLisp"])
            .params("{text: LISP source, name?: \"tool.lsp\"} → loads the routines; their C: commands can then be typed")
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("lisp", "Evaluate LISP", run_lisp)
            .params("{expr: \"(+ 1 2)\"} → {value} (no user input: get… functions are not available here)")
            .enabled(has_doc),
        CommandSpec::new("lisp.commands", "LISP Commands", run_lisp_commands).enabled(always).noundo(),
        CommandSpec::new("script", "Run Script", run_script)
            .menu(&["Tools", "Run Script..."])
            .alias(&["scr"])
            .params("{text: script lines} → runs each line as typed at the command line")
            .enabled(has_doc),
    ]
}

/// Load LISP source without user input (defuns, setqs, top-level code).
pub fn load_text(s: &mut Session, name: &str, text: &str) -> Result<Value> {
    let forms = lisp::read_all(text).map_err(|m| bad("appload", format!("{name}: {m}")))?;
    if !name.is_empty() {
        s.lisp.files.insert(name.to_ascii_lowercase(), text.to_string());
    }
    let before = s.lisp.commands();
    let mut l = std::mem::take(&mut s.lisp);
    let r = {
        let mut run = Run::new(s, &mut l, VecDeque::new(), false);
        let r = run.eval_all(&forms);
        let out = run.take_output();
        for line in out.split('\n').filter(|x| !x.trim().is_empty()) {
            run.s.echo(line.to_string());
        }
        r
    };
    s.lisp = l;
    if let Err(lisp::Ex::Error(m)) = &r {
        s.echo(format!("; lỗi khi nạp {name}: {m}"));
    }
    let new: Vec<String> = s.lisp.commands().into_iter().filter(|c| !before.contains(c)).collect();
    let all = s.lisp.commands();
    let msg = if new.is_empty() {
        format!("Đã nạp {name}.")
    } else {
        format!("Đã nạp {name}. Lệnh mới: {}", new.iter().map(|c| c.to_ascii_uppercase()).collect::<Vec<_>>().join(", "))
    };
    Ok(json!({ "file": name, "commands": all, "new": new, "message": msg }))
}

fn run_appload(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("appload", "`text` (nội dung tệp .lsp) is required"))?.to_string();
    let name = str_param(p, "name").unwrap_or("lisp.lsp").to_string();
    load_text(s, &name, &text)
}

fn run_lisp(s: &mut Session, p: &Value) -> Result<Value> {
    let expr = str_param(p, "expr").ok_or_else(|| bad("lisp", "`expr` is required"))?.to_string();
    let v = lisp::eval_quiet(s, &expr).map_err(|m| bad("lisp", m))?;
    let text = lisp::to_text(&v, true, &s.lisp.sets);
    Ok(json!({ "value": text, "message": if v == V::Sym(String::new()) { String::new() } else { text.clone() } }))
}

fn run_lisp_commands(s: &mut Session, _p: &Value) -> Result<Value> {
    let c = s.lisp.commands();
    let msg = if c.is_empty() {
        "Chưa nạp lệnh LISP nào (APPLOAD).".to_string()
    } else {
        format!("Lệnh LISP: {}", c.join(", ").to_ascii_uppercase())
    };
    Ok(json!({ "commands": c, "message": msg }))
}

fn run_script(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("script", "`text` is required"))?.to_string();
    let lines = text.lines().count();
    s.script(&text)?;
    Ok(json!({ "lines": lines, "message": format!("Đã chạy script ({lines} dòng).") }))
}
