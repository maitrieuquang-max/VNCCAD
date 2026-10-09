//! VNCCad: DCL dialogs for LISP routines (`load_dialog`, `new_dialog`, `set_tile`, `get_tile`,
//! `action_tile`, `start_list`/`add_list`/`end_list`, `mode_tile`, `start_dialog`,
//! `done_dialog`, `unload_dialog`).
//!
//! `start_dialog` hands the dialog to the interface (it becomes the prompt the routine waits
//! for); the user's answer — the button pressed and every tile's value — comes back as the next
//! input. The values are set, the pressed tile's action runs (with `$key`, `$value`, `$reason`)
//! and `start_dialog` returns the status given to `done_dialog` (OK 1, Cancel 0 by default).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Ex, R, Run, V, err, read_all};
use crate::{Accept, Input, Prompt};

/// A tile of a DCL dialog.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    pub kind: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Tile>,
}

impl Tile {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn key(&self) -> Option<&str> {
        self.attr("key")
    }
    fn walk<'a>(&'a self, out: &mut Vec<&'a Tile>) {
        out.push(self);
        for c in &self.children {
            c.walk(out);
        }
    }
}

/// A dialog waiting for the user (shown by the interface).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub name: String,
    pub dialog: Tile,
    pub values: HashMap<String, String>,
    pub lists: HashMap<String, Vec<String>>,
    pub disabled: Vec<String>,
    /// Keys of tiles with an `action_tile` (the interface reports changes to them at once).
    pub actions: Vec<String>,
    /// How many times this dialog has been shown (after actions that kept it open).
    #[serde(default)]
    pub round: u32,
}

/// The current `new_dialog` while a routine builds it.
#[derive(Clone, Debug, Default)]
pub struct Building {
    pub pending: Pending,
    pub actions: HashMap<String, String>,
    pub list_key: Option<(String, i64, i64)>,
    pub done: Option<i64>,
}

// ------------------------------------------------------------------ DCL parser

fn tokens(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&ch) = c.get(i) {
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c.get(i) != Some(&'\n') {
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c.get(i) == Some(&'*') && c.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if ch == '"' {
            let mut s = String::from("\"");
            i += 1;
            while let Some(&x) = c.get(i) {
                i += 1;
                if x == '\\' {
                    if let Some(&e) = c.get(i) {
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            o => o,
                        });
                        i += 1;
                    }
                } else if x == '"' {
                    break;
                } else {
                    s.push(x);
                }
            }
            out.push(s);
        } else if "{}:;=".contains(ch) {
            out.push(ch.to_string());
            i += 1;
        } else {
            let st = i;
            while let Some(&x) = c.get(i) {
                if x.is_whitespace() || "{}:;=\"".contains(x) {
                    break;
                }
                i += 1;
            }
            out.push(c.get(st..i).map(|s| s.iter().collect()).unwrap_or_default());
        }
        if out.len() > 200_000 {
            break;
        }
    }
    out
}

/// Parse a DCL file into named definitions (dialogs and reusable tiles).
pub fn parse(src: &str) -> Result<Vec<(String, Tile)>, String> {
    let t = tokens(src);
    let mut i = 0;
    let mut defs = Vec::new();
    while i < t.len() {
        // name : type { … }   |   @include "file";
        if t.get(i).is_some_and(|x| x.starts_with('@')) {
            while i < t.len() && t.get(i).map(String::as_str) != Some(";") {
                i += 1;
            }
            i += 1;
            continue;
        }
        let name = t.get(i).cloned().unwrap_or_default();
        if t.get(i + 1).map(String::as_str) != Some(":") {
            return Err(format!("DCL: cần `tên : loại {{`, gặp `{name}`"));
        }
        i += 2;
        let tile = parse_tile(&t, &mut i, 0)?;
        defs.push((name.to_ascii_lowercase(), tile));
    }
    Ok(defs)
}

fn parse_tile(t: &[String], i: &mut usize, depth: usize) -> Result<Tile, String> {
    if depth > 40 {
        return Err("DCL lồng quá sâu".into());
    }
    let kind = t.get(*i).cloned().ok_or("DCL: thiếu loại ô")?.to_ascii_lowercase();
    *i += 1;
    let mut tile = Tile { kind, ..Tile::default() };
    if t.get(*i).map(String::as_str) != Some("{") {
        return Ok(tile);
    }
    *i += 1;
    loop {
        let Some(tok) = t.get(*i).cloned() else { return Err("DCL: thiếu `}`".into()) };
        match tok.as_str() {
            "}" => {
                *i += 1;
                if t.get(*i).map(String::as_str) == Some(";") {
                    *i += 1;
                }
                return Ok(tile);
            }
            ":" => {
                *i += 1;
                tile.children.push(parse_tile(t, i, depth + 1)?);
            }
            ";" => *i += 1,
            _ => {
                if t.get(*i + 1).map(String::as_str) == Some("=") {
                    let v = t.get(*i + 2).cloned().unwrap_or_default();
                    tile.attrs.push((tok.to_ascii_lowercase(), v.strip_prefix('"').map(str::to_string).unwrap_or(v)));
                    *i += 3;
                } else if t.get(*i + 1).map(String::as_str) == Some(":") {
                    // name : type { … } nested (a named child definition): use it in place.
                    *i += 2;
                    tile.children.push(parse_tile(t, i, depth + 1)?);
                } else {
                    // A reference to a predefined or user tile: `ok_cancel;`, `spacer;`.
                    tile.children.push(Tile { kind: tok.to_ascii_lowercase(), ..Tile::default() });
                    *i += 1;
                }
            }
        }
    }
}

/// Replace references to the file's own tile definitions by their content.
fn expand(t: &Tile, defs: &[(String, Tile)], depth: usize) -> Tile {
    let mut out = t.clone();
    if depth < 20
        && let Some((_, d)) = defs.iter().find(|(n, _)| *n == t.kind)
    {
        let mut e = expand(d, defs, depth + 1);
        e.attrs.extend(t.attrs.iter().cloned());
        return e;
    }
    out.children = t.children.iter().map(|c| expand(c, defs, depth + 1)).collect();
    out
}

fn value_text(v: &V) -> String {
    match v {
        V::Str(s) => s.clone(),
        V::Int(n) => n.to_string(),
        V::Real(r) => super::fmt_real(*r),
        V::Nil => String::new(),
        other => super::to_text(other, false, &Default::default()),
    }
}

impl Run<'_> {
    pub(super) fn dcl(&mut self, name: &str, a: &[V]) -> Option<R<V>> {
        let arg = |i: usize| a.get(i).cloned().unwrap_or(V::Nil);
        let r: R<V> = match name {
            "load_dialog" => {
                let V::Str(f) = arg(0) else { return Some(err("load_dialog: cần tên tệp .dcl")) };
                let key = f.to_ascii_lowercase();
                let key = if key.ends_with(".dcl") { key } else { format!("{key}.dcl") };
                let base = key.rsplit(['/', '\\']).next().unwrap_or(&key).to_string();
                let src = self.lisp.files.get(&key).or_else(|| self.lisp.files.get(&base)).cloned();
                #[cfg(not(target_arch = "wasm32"))]
                let src = src.or_else(|| std::fs::read(&f).ok().map(|b| super::machine::decode_source(&b)));
                match src.map(|s| parse(&s)) {
                    Some(Ok(defs)) => {
                        self.lisp.next_set += 1;
                        let id = self.lisp.next_set as i64;
                        self.lisp.dcl.push((id, defs));
                        Ok(V::Int(id))
                    }
                    Some(Err(m)) => {
                        self.print(&format!("\n; {m}"));
                        Ok(V::Int(-1))
                    }
                    None => Ok(V::Int(-1)),
                }
            }
            "unload_dialog" => {
                if let V::Int(id) = arg(0) {
                    self.lisp.dcl.retain(|(i, _)| *i != id);
                }
                Ok(V::Nil)
            }
            "new_dialog" => {
                let (V::Str(n), V::Int(id)) = (arg(0), arg(1)) else { return Some(err("new_dialog: cần tên hộp thoại và id")) };
                let Some((_, defs)) = self.lisp.dcl.iter().find(|(i, _)| *i == id) else { return Some(Ok(V::Nil)) };
                let Some((_, d)) = defs.iter().find(|(k, t)| *k == n.to_ascii_lowercase() && t.kind == "dialog") else { return Some(Ok(V::Nil)) };
                let dialog = expand(d, defs, 0);
                let mut values = HashMap::new();
                let mut all = Vec::new();
                dialog.walk(&mut all);
                for t in &all {
                    if let (Some(k), Some(v)) = (t.key(), t.attr("value")) {
                        values.insert(k.to_string(), v.to_string());
                    }
                }
                // A radio group's value is the key of its chosen button.
                for g in all.iter().filter(|t| t.kind.contains("radio_") && t.key().is_some()) {
                    let on = g.children.iter().find(|c| c.attr("value").is_some_and(|v| v.trim() == "1")).and_then(Tile::key);
                    if let (Some(gk), Some(on)) = (g.key(), on)
                        && !values.contains_key(gk)
                    {
                        values.insert(gk.to_string(), on.to_string());
                    }
                }
                self.dialog = Some(Building { pending: Pending { name: n.clone(), dialog, values, ..Pending::default() }, ..Building::default() });
                Ok(V::T)
            }
            "set_tile" => {
                let (V::Str(k), v) = (arg(0), arg(1)) else { return Some(err("set_tile: cần khóa ô")) };
                if let Some(b) = self.dialog.as_mut() {
                    b.pending.values.insert(k, value_text(&v));
                }
                Ok(arg(1))
            }
            "get_tile" => {
                let V::Str(k) = arg(0) else { return Some(err("get_tile: cần khóa ô")) };
                Ok(V::Str(self.dialog.as_ref().and_then(|b| b.pending.values.get(&k).cloned()).unwrap_or_default()))
            }
            "get_attr" => {
                let (V::Str(k), V::Str(at)) = (arg(0), arg(1)) else { return Some(Ok(V::Str(String::new()))) };
                let mut all = Vec::new();
                if let Some(b) = self.dialog.as_ref() {
                    b.pending.dialog.walk(&mut all);
                }
                Ok(V::Str(all.into_iter().find(|t| t.key() == Some(k.as_str())).and_then(|t| t.attr(&at)).unwrap_or("").to_string()))
            }
            "action_tile" => {
                let (V::Str(k), V::Str(code)) = (arg(0), arg(1)) else { return Some(err("action_tile: cần khóa ô và mã LISP")) };
                if let Some(b) = self.dialog.as_mut() {
                    b.actions.insert(k, code);
                }
                Ok(V::T)
            }
            "mode_tile" => {
                if let (V::Str(k), V::Int(m)) = (arg(0), arg(1))
                    && let Some(b) = self.dialog.as_mut()
                {
                    b.pending.disabled.retain(|x| *x != k);
                    if m == 1 {
                        b.pending.disabled.push(k);
                    }
                }
                Ok(V::Nil)
            }
            "start_list" => {
                let V::Str(k) = arg(0) else { return Some(err("start_list: cần khóa ô")) };
                let op = if let V::Int(o) = arg(1) { o } else { 3 };
                let idx = if let V::Int(x) = arg(2) { x } else { 0 };
                if let Some(b) = self.dialog.as_mut() {
                    if op == 3 {
                        b.pending.lists.insert(k.clone(), Vec::new());
                    }
                    b.list_key = Some((k.clone(), op, idx));
                }
                Ok(V::Str(k))
            }
            "add_list" => {
                let s = value_text(&arg(0));
                if let Some(b) = self.dialog.as_mut()
                    && let Some((k, op, idx)) = b.list_key.clone()
                {
                    let l = b.pending.lists.entry(k).or_default();
                    match op {
                        1 => {
                            if let Some(slot) = usize::try_from(idx).ok().and_then(|i| l.get_mut(i)) {
                                *slot = s.clone();
                            }
                        }
                        _ => l.push(s.clone()),
                    }
                }
                Ok(V::Str(s))
            }
            "end_list" => {
                if let Some(b) = self.dialog.as_mut() {
                    b.list_key = None;
                }
                Ok(V::Nil)
            }
            "done_dialog" => {
                let st = if let V::Int(n) = arg(0) { n } else { 1 };
                if let Some(b) = self.dialog.as_mut() {
                    b.done = Some(st);
                }
                Ok(V::pt3(0.0, 0.0, 0.0))
            }
            "term_dialog" => {
                self.dialog = None;
                Ok(V::Nil)
            }
            "dimx_tile" | "dimy_tile" => Ok(V::Int(100)),
            "start_image" | "end_image" | "fill_image" | "vector_image" | "slide_image" | "client_data_tile" => Ok(V::Nil),
            "start_dialog" => self.start_dialog(),
            _ => return None,
        };
        Some(r)
    }

    fn start_dialog(&mut self) -> R<V> {
        if self.dialog.is_none() {
            return err("start_dialog: chưa có new_dialog");
        }
        // The dialog stays up until `done_dialog` (or OK/Cancel without an action).
        for round in 0..2000u32 {
            let Some(b) = self.dialog.as_mut() else { return Ok(V::Int(0)) };
            b.done = None;
            let mut pending = b.pending.clone();
            pending.actions = b.actions.keys().cloned().collect();
            pending.actions.sort();
            pending.round = round;
            let input = self.next_input(Prompt::new(format!("Hộp thoại {}", pending.name), Accept::TEXT).dialog(pending))?;
            let Input::Text(t) = input else {
                // Esc / Enter without the dialog's answer: Cancel.
                self.dialog = None;
                return Ok(V::Int(0));
            };
            let ans: Value = serde_json::from_str(&t).map_err(|e| Ex::Error(format!("start_dialog: {e}")))?;
            let pressed = ans.get("pressed").and_then(Value::as_str).unwrap_or("cancel").to_string();
            if let Some(b) = self.dialog.as_mut()
                && let Some(vals) = ans.get("values").and_then(Value::as_object)
            {
                for (k, v) in vals {
                    b.pending.values.insert(k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
                }
            }
            let action = self.dialog.as_ref().and_then(|b| b.actions.get(&pressed).cloned());
            let value = self.dialog.as_ref().and_then(|b| b.pending.values.get(&pressed).cloned()).unwrap_or_default();
            let cancel = pressed == "cancel" || ans.get("cancel").and_then(Value::as_bool) == Some(true);
            match action {
                Some(code) => {
                    let forms = read_all(&code).map_err(Ex::Error)?;
                    self.set("$key", V::Str(pressed.clone()));
                    self.set("$value", V::Str(value));
                    self.set("$reason", V::Int(ans.get("reason").and_then(Value::as_i64).unwrap_or(1)));
                    self.eval_all(&forms)?;
                }
                None if pressed == "accept" => return self.finish_dialog(1),
                None if cancel => return self.finish_dialog(0),
                None => {}
            }
            if let Some(st) = self.dialog.as_ref().and_then(|b| b.done) {
                return self.finish_dialog(st);
            }
            if self.dialog.is_none() {
                return Ok(V::Int(0));
            }
        }
        err("start_dialog: hộp thoại không đóng")
    }

    fn finish_dialog(&mut self, status: i64) -> R<V> {
        if let Some(b) = self.dialog.as_mut() {
            b.done = Some(status);
        }
        Ok(V::Int(status))
    }
}
