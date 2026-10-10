//! VNCCad: an AutoLISP interpreter for the LISP routines engineers already use (APPLOAD, `(load …)`,
//! `C:` commands, expressions typed at the command line).
//!
//! Written from the documented AutoLISP language: integers, reals, strings, symbols, lists and
//! dotted pairs, entity names and selection sets; `defun`/`setq`/`if`/`cond`/`while`/`repeat`/
//! `foreach`/`lambda`/`mapcar`…, string, list and math functions; `command`, `getpoint` and the other
//! `get…` functions, `entsel`/`ssget`, `entget`/`entmod`/`entmake`, `getvar`/`setvar`, `tblsearch`.
//! Variables are dynamically scoped, as in AutoLISP. ActiveX (`vla-…`, `vlax-…`) and DCL dialogs are
//! not available.
//!
//! User input (`getpoint`, `pause` inside `command`…) is handled by replay: when the routine needs
//! an input that hasn't been given yet, evaluation stops with the prompt to show; when the user
//! answers, the drawing and variables are put back as they were and the routine runs again from
//! the start with all answers so far. This keeps the interpreter a plain function (it runs the same
//! on the web build) at the cost of re-running the earlier steps, which is cheap for routines that
//! ask the user a handful of times.

mod builtins;
mod cad;
pub mod dcl;
mod extra;
pub mod machine;
mod vla;
pub mod vlr;

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::sync::Arc;

use cadcraft_doc::Handle;

use crate::{Input, Prompt, Session};

/// A LISP value.
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Nil,
    T,
    Int(i64),
    Real(f64),
    Str(String),
    /// Lower-case name. The empty symbol is what `(princ)` returns: printed as nothing.
    Sym(String),
    /// A proper, non-empty list.
    List(Vec<V>),
    /// `(a b . c)`.
    Dotted(Vec<V>, Box<V>),
    Ename(Handle),
    /// Selection set (index into the interpreter's table).
    Ss(u64),
    Fun(Arc<Lambda>),
    /// A built-in function as a value (`(function car)`, `'car` passed to `mapcar` stays a symbol).
    Subr(String),
    /// VNCCad: a reactor (`vlr-…`), by id.
    Vlr(u64),
    /// VNCCad: an open text file (`open`), by id.
    File(u64),
}

#[derive(Debug, PartialEq)]
pub struct Lambda {
    pub name: String,
    pub params: Vec<String>,
    pub locals: Vec<String>,
    pub body: Vec<V>,
}

impl V {
    pub fn truthy(&self) -> bool {
        !matches!(self, V::Nil)
    }
    pub fn from_bool(b: bool) -> V {
        if b { V::T } else { V::Nil }
    }
    pub fn list(v: Vec<V>) -> V {
        if v.is_empty() { V::Nil } else { V::List(v) }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            V::Int(i) => Some(*i as f64),
            V::Real(r) => Some(*r),
            _ => None,
        }
    }
    /// Items of a proper list (nil = empty).
    pub fn items(&self) -> Option<&[V]> {
        match self {
            V::Nil => Some(&[]),
            V::List(v) => Some(v),
            _ => None,
        }
    }
    /// A point `(x y [z])`.
    pub fn point(&self) -> Option<cadcraft_geom::Vec2> {
        let it = self.items()?;
        if !(2..=3).contains(&it.len()) {
            return None;
        }
        let p = cadcraft_geom::Vec2::new(it.first()?.num()?, it.get(1)?.num()?);
        p.is_finite().then_some(p)
    }
    pub fn pt3(x: f64, y: f64, z: f64) -> V {
        V::List(vec![V::Real(x), V::Real(y), V::Real(z)])
    }
    pub fn pair(a: V, b: V) -> V {
        match b {
            V::Nil => V::List(vec![a]),
            V::List(mut v) => {
                v.insert(0, a);
                V::List(v)
            }
            V::Dotted(mut v, t) => {
                v.insert(0, a);
                V::Dotted(v, t)
            }
            other => V::Dotted(vec![a], Box::new(other)),
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            V::Nil => "nil",
            V::T => "SYM",
            V::Int(_) => "INT",
            V::Real(_) => "REAL",
            V::Str(_) => "STR",
            V::Sym(_) => "SYM",
            V::List(_) | V::Dotted(..) => "LIST",
            V::Ename(_) => "ENAME",
            V::Ss(_) => "PICKSET",
            V::Fun(_) => "USUBR",
            V::Subr(_) => "SUBR",
            V::Vlr(_) => "VLR-OBJECT",
            V::File(_) => "FILE",
        }
    }
}

/// A real number written the way AutoLISP prints it (6 significant digits).
pub fn fmt_real(r: f64) -> String {
    if !r.is_finite() {
        return if r.is_nan() {
            "-1.#IND".into()
        } else if r > 0.0 {
            "1.#INF".into()
        } else {
            "-1.#INF".into()
        };
    }
    if r == 0.0 {
        return "0.0".into();
    }
    let mag = r.abs().log10().floor() as i32;
    if !(-5..15).contains(&mag) {
        let s = format!("{r:.5e}");
        // 1.50000e3 → 1.5e+03-ish; keep it short.
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = m.trim_end_matches('0').trim_end_matches('.');
        return format!("{m}e{}{:02}", if e.starts_with('-') { "-" } else { "+" }, e.trim_start_matches('-').parse::<i32>().unwrap_or(0));
    }
    let decimals = (5 - mag).clamp(0, 15) as usize;
    let s = format!("{r:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').to_string() } else { format!("{s}.0") };
    if s.ends_with('.') { format!("{s}0") } else { s }
}

/// `princ` (raw strings) or `prin1` (quoted strings) text of a value.
pub fn to_text(v: &V, quote: bool, sets: &HashMap<u64, Vec<Handle>>) -> String {
    let mut out = String::new();
    write_v(&mut out, v, quote, sets);
    out
}

fn write_v(out: &mut String, v: &V, quote: bool, sets: &HashMap<u64, Vec<Handle>>) {
    match v {
        V::Nil => out.push_str("nil"),
        V::T => out.push('T'),
        V::Int(i) => {
            let _ = write!(out, "{i}");
        }
        V::Real(r) => out.push_str(&fmt_real(*r)),
        V::Str(s) => {
            if quote {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('"');
            } else {
                out.push_str(s);
            }
        }
        V::Sym(s) => out.push_str(&s.to_ascii_uppercase()),
        V::List(items) => {
            out.push('(');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_v(out, x, quote, sets);
            }
            out.push(')');
        }
        V::Dotted(items, tail) => {
            out.push('(');
            for x in items {
                write_v(out, x, quote, sets);
                out.push(' ');
            }
            out.push_str(". ");
            write_v(out, tail, quote, sets);
            out.push(')');
        }
        V::Ename(h) => {
            let _ = write!(out, "<Entity name: {}>", h.hex());
        }
        V::Ss(id) => {
            let _ = write!(out, "<Selection set: {:x}> ({} objects)", id, sets.get(id).map_or(0, Vec::len));
        }
        V::Fun(l) => {
            let _ = write!(out, "#<USUBR @{}>", if l.name.is_empty() { "lambda" } else { &l.name });
        }
        V::Subr(n) => {
            let _ = write!(out, "#<SUBR @{}>", n.to_ascii_uppercase());
        }
        V::Vlr(id) => {
            let _ = write!(out, "#<VLR-Reactor {id:x}>");
        }
        V::File(id) => {
            let _ = write!(out, "#<file {id:x}>");
        }
    }
}

// ---------------------------------------------------------------- reader

/// Read every form in `src`.
pub fn read_all(src: &str) -> Result<Vec<V>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    loop {
        skip_ws(&chars, &mut i);
        if i >= chars.len() {
            return Ok(out);
        }
        if chars.get(i) == Some(&')') {
            // Stray closing parentheses are ignored, like AutoCAD's loader.
            i += 1;
            continue;
        }
        out.push(read(&chars, &mut i, 0)?);
        if out.len() > 100_000 {
            return Err("tệp LISP quá lớn".into());
        }
    }
}

fn skip_ws(c: &[char], i: &mut usize) {
    while let Some(&ch) = c.get(*i) {
        if ch.is_whitespace() {
            *i += 1;
        } else if ch == ';' {
            if c.get(*i + 1) == Some(&'|') {
                // Block comment ;| … |;
                *i += 2;
                while *i < c.len() && !(c.get(*i) == Some(&'|') && c.get(*i + 1) == Some(&';')) {
                    *i += 1;
                }
                *i += 2;
            } else {
                while *i < c.len() && c.get(*i) != Some(&'\n') {
                    *i += 1;
                }
            }
        } else {
            break;
        }
    }
}

fn read(c: &[char], i: &mut usize, depth: usize) -> Result<V, String> {
    if depth > 500 {
        return Err("biểu thức lồng quá sâu".into());
    }
    skip_ws(c, i);
    let Some(&ch) = c.get(*i) else { return Err("thiếu dấu ')' (malformed list)".into()) };
    match ch {
        '(' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                skip_ws(c, i);
                match c.get(*i) {
                    None => return Err("thiếu dấu ')' (malformed list)".into()),
                    Some(')') => {
                        *i += 1;
                        return Ok(V::list(items));
                    }
                    Some('.') if c.get(*i + 1).is_none_or(|n| n.is_whitespace() || *n == '(' || *n == '"') && !items.is_empty() => {
                        *i += 1;
                        let tail = read(c, i, depth + 1)?;
                        skip_ws(c, i);
                        if c.get(*i) != Some(&')') {
                            return Err("cặp chấm sai cú pháp".into());
                        }
                        *i += 1;
                        return Ok(match tail {
                            V::Nil => V::List(items),
                            V::List(rest) => {
                                items.extend(rest);
                                V::List(items)
                            }
                            other => V::Dotted(items, Box::new(other)),
                        });
                    }
                    _ => items.push(read(c, i, depth + 1)?),
                }
            }
        }
        '\'' => {
            *i += 1;
            let x = read(c, i, depth + 1)?;
            Ok(V::List(vec![V::Sym("quote".into()), x]))
        }
        '"' => {
            *i += 1;
            let mut s = String::new();
            loop {
                let Some(&ch) = c.get(*i) else { return Err("chuỗi thiếu dấu \" đóng".into()) };
                *i += 1;
                match ch {
                    '"' => return Ok(V::Str(s)),
                    '\\' => {
                        let Some(&e) = c.get(*i) else { return Err("chuỗi thiếu dấu \" đóng".into()) };
                        *i += 1;
                        match e {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            'e' => s.push('\u{1b}'),
                            '"' => s.push('"'),
                            '\\' => s.push('\\'),
                            d if d.is_ascii_digit() => {
                                // \nnn octal
                                let mut n = d.to_digit(8).unwrap_or(0);
                                for _ in 0..2 {
                                    if let Some(x) = c.get(*i).and_then(|x| x.to_digit(8)) {
                                        n = n * 8 + x;
                                        *i += 1;
                                    }
                                }
                                s.push(char::from_u32(n).unwrap_or('?'));
                            }
                            other => {
                                s.push('\\');
                                s.push(other);
                            }
                        }
                    }
                    other => s.push(other),
                }
            }
        }
        _ => {
            let start = *i;
            while let Some(&ch) = c.get(*i) {
                if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '\'' || ch == '"' || ch == ';' {
                    break;
                }
                *i += 1;
            }
            let tok: String = c.get(start..*i).map(|s| s.iter().collect()).unwrap_or_default();
            Ok(atom(&tok))
        }
    }
}

fn atom(tok: &str) -> V {
    if let Ok(n) = tok.parse::<i64>()
        && n.abs() <= i64::from(i32::MAX)
    {
        return V::Int(n);
    }
    let looks_num = tok.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '+' || c == '.')
        && tok.chars().any(|c| c.is_ascii_digit())
        && tok.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'));
    if looks_num && let Ok(r) = tok.parse::<f64>() {
        return V::Real(r);
    }
    match tok.to_ascii_lowercase().as_str() {
        "nil" => V::Nil,
        "t" => V::T,
        s => V::Sym(s.to_string()),
    }
}

// ---------------------------------------------------------------- interpreter

/// Why evaluation stopped early.
#[derive(Clone, Debug, PartialEq)]
pub enum Ex {
    Error(String),
    /// The routine needs a user input that hasn't been given yet.
    Need(Prompt),
    /// `(exit)` / `(quit)`.
    Quit,
}

pub type R<T> = Result<T, Ex>;

pub fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(Ex::Error(msg.into()))
}

/// Interpreter state kept in the session: global variables, user functions, selection sets.
#[derive(Clone, Debug, Default)]
pub struct Lisp {
    pub globals: HashMap<String, V>,
    pub sets: HashMap<u64, Vec<Handle>>,
    pub next_set: u64,
    /// Source texts given to the app (APPLOAD, dropped `.lsp` files), by lower-case file name.
    pub files: HashMap<String, String>,
    /// Loaded DCL files (`load_dialog` id, definitions).
    pub dcl: Vec<(i64, Vec<(String, dcl::Tile)>)>,
    /// Reactors (`vlr-…`).
    pub reactors: Vec<vlr::Reactor>,
    /// VNCCad: open text files (`open`) and the next id.
    pub open_files: HashMap<u64, extra::LFile>,
    pub next_file: u64,
    /// VNCCad: `setcfg`/`setenv` values.
    pub cfg: HashMap<String, String>,
    /// VNCCad: `tblnext`/`dictnext` positions.
    pub tbl_cursor: HashMap<String, usize>,
}

impl Lisp {
    /// `C:` commands defined so far (lower-case, without the prefix).
    pub fn commands(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .globals
            .iter()
            .filter(|(k, v)| k.starts_with("c:") && matches!(v, V::Fun(_)))
            .map(|(k, _)| k.trim_start_matches("c:").to_string())
            .collect();
        v.sort();
        v
    }
    pub fn has_command(&self, name: &str) -> bool {
        matches!(self.globals.get(&format!("c:{}", name.to_ascii_lowercase())), Some(V::Fun(_)))
    }
}

/// One evaluation run: the session, the interpreter state, and the inputs given so far.
pub struct Run<'a> {
    pub s: &'a mut Session,
    pub lisp: &'a mut Lisp,
    pub inputs: VecDeque<Input>,
    /// Whether `get…` may ask the user (false for expressions typed inside another command).
    pub interactive: bool,
    frames: Vec<HashMap<String, V>>,
    steps: u64,
    out: String,
    initget: (i64, Vec<String>),
    depth: usize,
    pub(crate) dialog: Option<dcl::Building>,
}

const MAX_STEPS: u64 = 20_000_000;
const MAX_DEPTH: usize = 400;

impl<'a> Run<'a> {
    pub fn new(s: &'a mut Session, lisp: &'a mut Lisp, inputs: VecDeque<Input>, interactive: bool) -> Self {
        Run { s, lisp, inputs, interactive, frames: Vec::new(), steps: 0, out: String::new(), initget: (0, Vec::new()), depth: 0, dialog: None }
    }

    /// Text printed so far (not yet on the command line).
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.out)
    }

    pub(crate) fn print(&mut self, text: &str) {
        self.out.push_str(text);
        if self.out.len() > 1_000_000 {
            self.out.truncate(1_000_000);
        }
    }

    pub fn lookup(&self, name: &str) -> V {
        for f in self.frames.iter().rev() {
            if let Some(v) = f.get(name) {
                return v.clone();
            }
        }
        if let Some(v) = self.lisp.globals.get(name) {
            return v.clone();
        }
        match name {
            "pi" => V::Real(std::f64::consts::PI),
            ":vlax-true" => V::T,
            ":vlax-false" | ":vlax-null" => V::Nil,
            "pause" => V::Str("\\".into()),
            _ => V::Nil,
        }
    }

    pub fn set(&mut self, name: &str, v: V) {
        for f in self.frames.iter_mut().rev() {
            if let Some(slot) = f.get_mut(name) {
                *slot = v;
                return;
            }
        }
        if v == V::Nil {
            self.lisp.globals.remove(name);
        } else {
            self.lisp.globals.insert(name.to_string(), v);
        }
    }

    /// Evaluate every form, returning the last value.
    pub fn eval_all(&mut self, forms: &[V]) -> R<V> {
        let mut last = V::Nil;
        for f in forms {
            last = self.eval(f)?;
        }
        Ok(last)
    }

    pub fn eval(&mut self, x: &V) -> R<V> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return err("chương trình chạy quá lâu (vòng lặp vô hạn?)");
        }
        match x {
            V::Sym(name) => Ok(self.lookup(name)),
            V::List(items) => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    self.depth -= 1;
                    return err("đệ quy quá sâu");
                }
                let r = self.eval_list(items);
                self.depth -= 1;
                r
            }
            other => Ok(other.clone()),
        }
    }

    fn eval_list(&mut self, items: &[V]) -> R<V> {
        let Some(head) = items.first() else { return Ok(V::Nil) };
        let args = items.get(1..).unwrap_or(&[]);
        if let V::Sym(name) = head {
            if let Some(r) = self.special(name, args) {
                return r;
            }
            let f = self.lookup(name);
            let vals = self.eval_args(args)?;
            return match f {
                V::Fun(l) => self.apply_lambda(&l, vals),
                V::Subr(n) => self.call_builtin(&n, vals),
                V::Nil | V::T | V::Int(_) | V::Real(_) | V::Str(_) => self.call_builtin(name, vals),
                _ => self.call_builtin(name, vals),
            };
        }
        let f = self.eval(head)?;
        let vals = self.eval_args(args)?;
        self.call(&f, vals)
    }

    fn eval_args(&mut self, args: &[V]) -> R<Vec<V>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a)?);
        }
        Ok(out)
    }

    /// Call a function value: lambda, built-in, symbol naming either, or a quoted lambda list.
    pub fn call(&mut self, f: &V, args: Vec<V>) -> R<V> {
        match f {
            V::Fun(l) => self.apply_lambda(l, args),
            V::Subr(n) => self.call_builtin(n, args),
            V::Sym(n) => match self.lookup(n) {
                V::Fun(l) => self.apply_lambda(&l, args),
                _ => self.call_builtin(n, args),
            },
            V::List(items) if matches!(items.first(), Some(V::Sym(s)) if s == "lambda") => {
                let l = self.make_lambda("", items.get(1..).unwrap_or(&[]))?;
                self.apply_lambda(&l, args)
            }
            other => err(format!("không phải hàm: {}", to_text(other, true, &self.lisp.sets))),
        }
    }

    fn make_lambda(&self, name: &str, rest: &[V]) -> R<Arc<Lambda>> {
        let spec = rest.first().ok_or_else(|| Ex::Error("thiếu danh sách tham số".into()))?;
        let names = spec.items().ok_or_else(|| Ex::Error("danh sách tham số sai".into()))?;
        let mut params = Vec::new();
        let mut locals = Vec::new();
        let mut in_locals = false;
        for n in names {
            match n {
                V::Sym(s) if s == "/" => in_locals = true,
                V::Sym(s) => {
                    if in_locals {
                        locals.push(s.clone());
                    } else {
                        params.push(s.clone());
                    }
                }
                _ => return err("tham số phải là tên"),
            }
        }
        Ok(Arc::new(Lambda { name: name.to_string(), params, locals, body: rest.get(1..).unwrap_or(&[]).to_vec() }))
    }

    pub fn apply_lambda(&mut self, l: &Lambda, args: Vec<V>) -> R<V> {
        if args.len() != l.params.len() {
            return err(format!("số đối số không đúng khi gọi {}", if l.name.is_empty() { "lambda" } else { &l.name }));
        }
        let mut frame: HashMap<String, V> = l.params.iter().cloned().zip(args).collect();
        for n in &l.locals {
            frame.entry(n.clone()).or_insert(V::Nil);
        }
        self.frames.push(frame);
        let r = self.eval_all(&l.body);
        self.frames.pop();
        r
    }

    fn special(&mut self, name: &str, args: &[V]) -> Option<R<V>> {
        Some(match name {
            "quote" => Ok(args.first().cloned().unwrap_or(V::Nil)),
            "function" => match args.first() {
                Some(V::Sym(n)) => Ok(match self.lookup(n) {
                    V::Fun(l) => V::Fun(l),
                    _ => V::Subr(n.clone()),
                }),
                Some(V::List(items)) if matches!(items.first(), Some(V::Sym(s)) if s == "lambda") => {
                    self.make_lambda("", items.get(1..).unwrap_or(&[])).map(V::Fun)
                }
                Some(other) => self.eval(other),
                None => Ok(V::Nil),
            },
            "setq" => {
                let mut last = V::Nil;
                for pair in args.chunks(2) {
                    let [V::Sym(n), e] = pair else { return Some(err("setq: tên biến không hợp lệ")) };
                    let v = match self.eval(e) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    self.set(n, v.clone());
                    last = v;
                }
                Ok(last)
            }
            "defun" | "defun-q" => {
                let Some(V::Sym(n)) = args.first() else { return Some(err("defun: thiếu tên hàm")) };
                match self.make_lambda(n, args.get(1..).unwrap_or(&[])) {
                    Ok(l) => {
                        self.lisp.globals.insert(n.clone(), V::Fun(l));
                        Ok(V::Sym(n.clone()))
                    }
                    Err(e) => Err(e),
                }
            }
            "lambda" => self.make_lambda("", args).map(V::Fun),
            "if" => {
                let t = match args.first().map(|c| self.eval(c)) {
                    Some(Ok(v)) => v,
                    Some(Err(e)) => return Some(Err(e)),
                    None => return Some(err("if: thiếu điều kiện")),
                };
                match if t.truthy() { args.get(1) } else { args.get(2) } {
                    Some(e) => self.eval(e),
                    None => Ok(V::Nil),
                }
            }
            "cond" => {
                for clause in args {
                    let Some(items) = clause.items() else { return Some(err("cond: mệnh đề sai")) };
                    let Some(test) = items.first() else { continue };
                    let t = match self.eval(test) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    if t.truthy() {
                        return Some(if items.len() > 1 { self.eval_all(items.get(1..).unwrap_or(&[])) } else { Ok(t) });
                    }
                }
                Ok(V::Nil)
            }
            "while" => {
                let Some(test) = args.first() else { return Some(Ok(V::Nil)) };
                let body = args.get(1..).unwrap_or(&[]);
                let mut last = V::Nil;
                loop {
                    match self.eval(test) {
                        Ok(t) if t.truthy() => {}
                        Ok(_) => break,
                        Err(e) => return Some(Err(e)),
                    }
                    match self.eval_all(body) {
                        Ok(v) => last = v,
                        Err(e) => return Some(Err(e)),
                    }
                }
                Ok(last)
            }
            "repeat" => {
                let n = match args.first().map(|c| self.eval(c)) {
                    Some(Ok(V::Int(n))) => n,
                    Some(Ok(_)) => return Some(err("repeat: số lần phải là số nguyên")),
                    Some(Err(e)) => return Some(Err(e)),
                    None => 0,
                };
                let mut last = V::Nil;
                for _ in 0..n.max(0) {
                    match self.eval_all(args.get(1..).unwrap_or(&[])) {
                        Ok(v) => last = v,
                        Err(e) => return Some(Err(e)),
                    }
                }
                Ok(last)
            }
            "foreach" | "vlax-for" => {
                let Some(V::Sym(var)) = args.first() else { return Some(err(format!("{name}: thiếu tên biến"))) };
                let list = match args.get(1).map(|c| self.eval(c)) {
                    Some(Ok(v)) => v,
                    Some(Err(e)) => return Some(Err(e)),
                    None => V::Nil,
                };
                // VNCCad: vlax-for walks a collection (objects of a space, layers…).
                let list = if name == "vlax-for" { V::list(self.collection(&list).unwrap_or_default()) } else { list };
                let items: Vec<V> = match &list {
                    V::Nil => Vec::new(),
                    V::List(v) => v.clone(),
                    V::Dotted(v, _) => v.clone(),
                    _ => return Some(err("foreach: cần một danh sách")),
                };
                let body = args.get(2..).unwrap_or(&[]);
                self.frames.push(HashMap::from([(var.clone(), V::Nil)]));
                let mut last = V::Nil;
                let mut res = Ok(());
                for it in items {
                    if let Some(f) = self.frames.last_mut() {
                        f.insert(var.clone(), it);
                    }
                    match self.eval_all(body) {
                        Ok(v) => last = v,
                        Err(e) => {
                            res = Err(e);
                            break;
                        }
                    }
                }
                self.frames.pop();
                res.map(|_| last)
            }
            "progn" => self.eval_all(args),
            "and" => {
                for a in args {
                    match self.eval(a) {
                        Ok(v) if !v.truthy() => return Some(Ok(V::Nil)),
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
                Ok(V::T)
            }
            "or" => {
                for a in args {
                    match self.eval(a) {
                        Ok(v) if v.truthy() => return Some(Ok(V::T)),
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
                Ok(V::Nil)
            }
            "vl-catch-all-apply" => {
                let f = match args.first().map(|a| self.eval(a)) {
                    Some(Ok(v)) => v,
                    Some(Err(e)) => return Some(Err(e)),
                    None => return Some(err("vl-catch-all-apply: thiếu hàm")),
                };
                let a = match args.get(1).map(|a| self.eval(a)) {
                    Some(Ok(v)) => v,
                    Some(Err(e)) => return Some(Err(e)),
                    None => V::Nil,
                };
                let list = a.items().map(<[V]>::to_vec).unwrap_or_default();
                match self.call(&f, list) {
                    Ok(v) => Ok(v),
                    Err(Ex::Error(m)) => Ok(V::List(vec![V::Sym("*catch-all-error*".into()), V::Str(m)])),
                    Err(e) => Err(e),
                }
            }
            _ => return None,
        })
    }

    pub(crate) fn initget(&mut self) -> (i64, Vec<String>) {
        std::mem::take(&mut self.initget)
    }
    pub(crate) fn set_initget(&mut self, bits: i64, kws: Vec<String>) {
        self.initget = (bits, kws);
    }

    /// The next user input, or stop with `prompt` when none is left to replay.
    pub(crate) fn next_input(&mut self, prompt: Prompt) -> R<Input> {
        if let Some(i) = self.inputs.pop_front() {
            return Ok(i);
        }
        if !self.interactive {
            return err("không thể hỏi người dùng ở đây (biểu thức nằm trong một lệnh khác)");
        }
        Err(Ex::Need(prompt))
    }
}

/// Evaluate `src` without user input (an expression typed at another command's prompt).
pub fn eval_quiet(s: &mut Session, src: &str) -> Result<V, String> {
    let forms = read_all(src)?;
    let mut lisp = std::mem::take(&mut s.lisp);
    let r = {
        let mut run = Run::new(s, &mut lisp, VecDeque::new(), false);
        let r = run.eval_all(&forms);
        let out = run.take_output();
        if !out.trim().is_empty() {
            for l in out.lines() {
                run.s.echo(l.to_string());
            }
        }
        r
    };
    s.lisp = lisp;
    r.map_err(|e| match e {
        Ex::Error(m) => m,
        Ex::Need(_) => "cần nhập liệu".into(),
        Ex::Quit => "đã thoát".into(),
    })
}

#[cfg(test)]
mod tests;
