//! VNCCad: Visual LISP reactors (`vlr-*`).
//!
//! Supported reactor types and events:
//! - `vlr-command-reactor`: `:vlr-commandWillStart`, `:vlr-commandEnded`, `:vlr-commandCancelled`,
//!   `:vlr-commandFailed` (callback `(reactor (list "LINE"))`).
//! - `vlr-lisp-reactor`: `:vlr-lispWillStart`, `:vlr-lispEnded`, `:vlr-lispCancelled`
//!   (callback `(reactor (list "(C:XYZ)"))`).
//! - `vlr-editor-reactor`: the command and LISP events above plus `:vlr-beginSave`,
//!   `:vlr-saveComplete`.
//! - `vlr-dwg-reactor`: `:vlr-beginSave`, `:vlr-saveComplete`.
//! - `vlr-object-reactor` (owners): `:vlr-modified`, `:vlr-erased`, `:vlr-copied` is not raised
//!   (callback `(owner reactor nil)`).
//!
//! Callbacks run after the command, without user input, in the same undo step as the command;
//! changes the callbacks make do not raise reactors again. Reactors are not saved with the
//! drawing (`vlr-pers` is accepted and has no effect).

use std::collections::VecDeque;
use std::sync::Arc;

use cadcraft_doc::{Drawing, Handle};

use super::{Ex, Lisp, R, Run, V};
use crate::Session;

/// A reactor created by a LISP routine.
#[derive(Clone, Debug, PartialEq)]
pub struct Reactor {
    pub id: u64,
    /// "command", "lisp", "editor", "dwg", "object", "docmanager", "mouse", …
    pub kind: String,
    pub data: V,
    /// (event name in lower case without the colon, callback function).
    pub reactions: Vec<(String, V)>,
    pub owners: Vec<Handle>,
    pub active: bool,
}

fn type_symbol(kind: &str) -> String {
    match kind {
        "command" => ":vlr-command-reactor",
        "lisp" => ":vlr-lisp-reactor",
        "editor" => ":vlr-editor-reactor",
        "dwg" => ":vlr-dwg-reactor",
        "object" => ":vlr-object-reactor",
        "docmanager" => ":vlr-docmanager-reactor",
        "mouse" => ":vlr-mouse-reactor",
        "sysvar" => ":vlr-sysvar-reactor",
        "acdb" => ":vlr-acdb-reactor",
        other => return format!(":vlr-{other}-reactor"),
    }
    .to_string()
}

pub fn display(kind: &str) -> String {
    let mut s = String::from("#<VLR-");
    let mut up = true;
    for c in kind.chars() {
        s.push(if up { c.to_ascii_uppercase() } else { c });
        up = c == '-';
    }
    s.push_str("-Reactor>");
    s
}

fn reactions_of(v: &V) -> Vec<(String, V)> {
    let mut out = Vec::new();
    for pair in v.items().unwrap_or(&[]) {
        let (ev, f) = match pair {
            V::Dotted(a, tail) if a.len() == 1 => (a.first().cloned().unwrap_or(V::Nil), (**tail).clone()),
            V::List(a) if a.len() == 2 => (a.first().cloned().unwrap_or(V::Nil), a.get(1).cloned().unwrap_or(V::Nil)),
            _ => continue,
        };
        if let V::Sym(e) = ev {
            out.push((e.trim_start_matches(':').to_string(), f));
        }
    }
    out
}

fn handles_of(v: &V) -> Vec<Handle> {
    v.items().unwrap_or(&[]).iter().filter_map(|x| if let V::Ename(h) = x { Some(*h) } else { None }).collect()
}

impl Run<'_> {
    fn reactor_mut(&mut self, v: &V) -> Option<&mut Reactor> {
        let V::Vlr(id) = v else { return None };
        self.lisp.reactors.iter_mut().find(|r| r.id == *id)
    }
    fn reactor(&self, v: &V) -> Option<&Reactor> {
        let V::Vlr(id) = v else { return None };
        self.lisp.reactors.iter().find(|r| r.id == *id)
    }

    fn new_reactor(&mut self, kind: &str, owners: Vec<Handle>, data: V, reactions: &V) -> V {
        self.lisp.next_set += 1;
        let id = self.lisp.next_set;
        self.lisp.reactors.push(Reactor { id, kind: kind.into(), data, reactions: reactions_of(reactions), owners, active: true });
        V::Vlr(id)
    }

    pub(super) fn vlr(&mut self, name: &str, a: &[V]) -> Option<R<V>> {
        let arg = |i: usize| a.get(i).cloned().unwrap_or(V::Nil);
        let r: R<V> = match name {
            "vlr-object-reactor" => Ok(self.new_reactor("object", handles_of(&arg(0)), arg(1), &arg(2))),
            n if n.starts_with("vlr-") && n.ends_with("-reactor") => {
                let kind = n.trim_start_matches("vlr-").trim_end_matches("-reactor").to_string();
                Ok(self.new_reactor(&kind, Vec::new(), arg(0), &arg(1)))
            }
            "vlr-remove" => Ok(match self.reactor_mut(&arg(0)) {
                Some(r) => {
                    r.active = false;
                    arg(0)
                }
                None => V::Nil,
            }),
            "vlr-add" => Ok(match self.reactor_mut(&arg(0)) {
                Some(r) => {
                    r.active = true;
                    arg(0)
                }
                None => V::Nil,
            }),
            "vlr-added-p" => Ok(V::from_bool(self.reactor(&arg(0)).is_some_and(|r| r.active))),
            "vlr-remove-all" => {
                let kind = if let V::Sym(t) = arg(0) { Some(t) } else { None };
                for r in &mut self.lisp.reactors {
                    if kind.as_deref().is_none_or(|k| type_symbol(&r.kind) == k) {
                        r.active = false;
                    }
                }
                Ok(V::Nil)
            }
            "vlr-reactors" => {
                let kinds: Vec<String> = a.iter().filter_map(|x| if let V::Sym(s) = x { Some(s.clone()) } else { None }).collect();
                let mut groups: Vec<(String, Vec<V>)> = Vec::new();
                for r in self.lisp.reactors.iter().filter(|r| r.active) {
                    let t = type_symbol(&r.kind);
                    if !kinds.is_empty() && !kinds.contains(&t) {
                        continue;
                    }
                    match groups.iter_mut().find(|(k, _)| *k == t) {
                        Some((_, v)) => v.push(V::Vlr(r.id)),
                        None => groups.push((t, vec![V::Vlr(r.id)])),
                    }
                }
                Ok(V::list(
                    groups
                        .into_iter()
                        .map(|(t, mut v)| {
                            v.insert(0, V::Sym(t));
                            V::List(v)
                        })
                        .collect(),
                ))
            }
            "vlr-type" => Ok(self.reactor(&arg(0)).map(|r| V::Sym(type_symbol(&r.kind))).unwrap_or(V::Nil)),
            "vlr-data" => Ok(self.reactor(&arg(0)).map(|r| r.data.clone()).unwrap_or(V::Nil)),
            "vlr-data-set" => {
                let d = arg(1);
                if let Some(r) = self.reactor_mut(&arg(0)) {
                    r.data = d.clone();
                }
                Ok(d)
            }
            "vlr-reactions" => Ok(self
                .reactor(&arg(0))
                .map(|r| V::list(r.reactions.iter().map(|(e, f)| V::pair(V::Sym(format!(":{e}")), f.clone())).collect()))
                .unwrap_or(V::Nil)),
            "vlr-reaction-set" => {
                let (ev, f) = (arg(1), arg(2));
                if let (V::Sym(e), Some(r)) = (ev, self.reactor_mut(&arg(0))) {
                    let e = e.trim_start_matches(':').to_string();
                    r.reactions.retain(|(x, _)| *x != e);
                    if f.truthy() {
                        r.reactions.push((e, f.clone()));
                    }
                }
                Ok(f)
            }
            "vlr-owners" => Ok(self.reactor(&arg(0)).map(|r| V::list(r.owners.iter().map(|h| V::Ename(*h)).collect())).unwrap_or(V::Nil)),
            "vlr-owner-add" => {
                let o = arg(1);
                if let (V::Ename(h), Some(r)) = (&o, self.reactor_mut(&arg(0)))
                    && !r.owners.contains(h)
                {
                    r.owners.push(*h);
                }
                Ok(o)
            }
            "vlr-owner-remove" => {
                let o = arg(1);
                if let (V::Ename(h), Some(r)) = (&o, self.reactor_mut(&arg(0))) {
                    r.owners.retain(|x| x != h);
                }
                Ok(o)
            }
            "vlr-pers" | "vlr-pers-release" => Ok(arg(0)),
            "vlr-pers-p" => Ok(V::Nil),
            "vlr-pers-list" => Ok(V::Nil),
            "vlr-current-reaction-name" => Ok(self.lookup("*vnccad-reaction*")),
            "vlr-types" => Ok(V::list(
                ["command", "lisp", "editor", "dwg", "object", "docmanager", "mouse", "sysvar"].iter().map(|k| V::Sym(type_symbol(k))).collect(),
            )),
            "vlr-reaction-names" => Ok(V::list(
                [
                    "commandWillStart",
                    "commandEnded",
                    "commandCancelled",
                    "commandFailed",
                    "lispWillStart",
                    "lispEnded",
                    "lispCancelled",
                    "beginSave",
                    "saveComplete",
                    "modified",
                    "erased",
                ]
                .iter()
                .map(|e| V::Sym(format!(":vlr-{}", e.to_ascii_lowercase())))
                .collect(),
            )),
            "vlr-notification" => Ok(V::Sym(":active-document-only".into())),
            "vlr-set-notification" => Ok(arg(0)),
            _ => return None,
        };
        Some(r)
    }
}

/// An event to raise.
pub enum Event<'a> {
    /// Command events: name ("vlr-commandended"…) and the command name.
    Command(&'a str, &'a str),
    /// LISP events with the expression text.
    Lisp(&'a str, &'a str),
    /// Save events with the file name.
    Save(&'a str, &'a str),
}

fn wants(kind: &str, ev: &Event) -> bool {
    match ev {
        Event::Command(..) => matches!(kind, "command" | "editor"),
        Event::Lisp(..) => matches!(kind, "lisp" | "editor"),
        Event::Save(..) => matches!(kind, "dwg" | "editor"),
    }
}

/// True when some active reactor listens (cheap test before collecting anything).
pub fn any_active(s: &Session) -> bool {
    !s.in_reactor && s.lisp.reactors.iter().any(|r| r.active && !r.reactions.is_empty())
}

/// Raise an editor/command/LISP/save event.
pub fn raise(s: &mut Session, ev: Event) {
    if !any_active(s) {
        return;
    }
    let (name, arg) = match &ev {
        Event::Command(n, a) | Event::Lisp(n, a) | Event::Save(n, a) => (n.to_ascii_lowercase(), a.to_string()),
    };
    let calls: Vec<(u64, V)> = s
        .lisp
        .reactors
        .iter()
        .filter(|r| r.active && wants(&r.kind, &ev))
        .flat_map(|r| r.reactions.iter().filter(|(e, _)| *e == name).map(move |(_, f)| (r.id, f.clone())))
        .collect();
    for (id, f) in calls {
        run_callback(s, &name, &f, vec![V::Vlr(id), V::list(vec![V::Str(arg.clone())])]);
    }
}

/// Raise `:vlr-modified` / `:vlr-erased` for object reactors whose owners changed between
/// `before` and the current drawing.
pub fn raise_object_events(s: &mut Session, before: &Arc<Drawing>) {
    if !any_active(s) || !s.lisp.reactors.iter().any(|r| r.active && r.kind == "object" && !r.owners.is_empty()) {
        return;
    }
    let Ok(now) = s.doc() else { return };
    let mut calls: Vec<(String, V, u64, Handle)> = Vec::new();
    for r in s.lisp.reactors.iter().filter(|r| r.active && r.kind == "object") {
        for h in &r.owners {
            let (Some(old), new) = (before.entity(*h), now.entity(*h)) else { continue };
            let ev = match new {
                None => "vlr-erased",
                Some(n) if **n != **old => "vlr-modified",
                Some(_) => continue,
            };
            for (e, f) in r.reactions.iter().filter(|(e, _)| e == ev) {
                calls.push((e.clone(), f.clone(), r.id, *h));
            }
        }
    }
    for (ev, f, id, h) in calls {
        run_callback(s, &ev, &f, vec![V::Ename(h), V::Vlr(id), V::Nil]);
    }
}

fn run_callback(s: &mut Session, event: &str, f: &V, args: Vec<V>) {
    s.in_reactor = true;
    let mut lisp: Lisp = std::mem::take(&mut s.lisp);
    let r = {
        let mut run = Run::new(s, &mut lisp, VecDeque::new(), false);
        run.set("*vnccad-reaction*", V::Sym(format!(":{event}")));
        let r = run.call(f, args);
        let out = run.take_output();
        for line in out.split('\n').filter(|x| !x.trim().is_empty()) {
            run.s.echo(line.to_string());
        }
        r
    };
    s.lisp = lisp;
    s.in_reactor = false;
    match r {
        Ok(_) | Err(Ex::Quit) => {}
        Err(Ex::Error(m)) => s.echo(format!("; lỗi trong reactor {event}: {m}")),
        Err(Ex::Need(_)) => s.echo(format!("; reactor {event} không được hỏi người dùng")),
    }
}
