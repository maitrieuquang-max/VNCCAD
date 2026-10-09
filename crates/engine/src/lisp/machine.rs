//! Running LISP as an interactive command: replay on every user input.

use std::collections::VecDeque;
use std::sync::Arc;

use cadcraft_doc::{Drawing, Handle};
use cadcraft_geom::Vec2;

use super::{Ex, Lisp, Run, V, to_text};
use crate::{Accept, Input, Interactive, Prompt, Result, Running, Session, Settings, Snapshot, Step};

/// What the command runs.
#[derive(Clone, Debug)]
pub enum Job {
    /// Forms typed at the command line or loaded from a file (`print` the result for the former).
    Eval { forms: Vec<V>, echo_result: bool, label: String },
    /// A `C:` command.
    Call(String),
}

/// Session state put back before every replay.
struct Snap {
    doc: Arc<Drawing>,
    undo_len: usize,
    redo: Vec<Snapshot>,
    selection: Vec<Handle>,
    settings: Settings,
    last_point: Vec2,
    log_len: usize,
    lisp: Lisp,
}

pub struct LispM {
    job: Job,
    inputs: Vec<Input>,
    snap: Option<Snap>,
    prompt: Option<Prompt>,
    /// The command a `pause` is waiting in (kept only for its rubber band).
    nested: Option<Running>,
}

impl LispM {
    pub fn new(job: Job) -> Self {
        LispM { job, inputs: Vec::new(), snap: None, prompt: None, nested: None }
    }

    fn take_snapshot(s: &Session) -> Option<Snap> {
        let st = s.state().ok()?;
        Some(Snap {
            doc: st.doc.clone(),
            undo_len: st.undo.len(),
            redo: st.redo.clone(),
            selection: st.selection.clone(),
            settings: s.settings.clone(),
            last_point: s.last_point,
            log_len: s.log.len(),
            lisp: s.lisp.clone(),
        })
    }

    fn restore(&self, s: &mut Session) {
        let Some(sn) = &self.snap else { return };
        s.running = None;
        if let Ok(st) = s.state_mut() {
            st.doc = sn.doc.clone();
            st.undo.truncate(sn.undo_len);
            st.redo = sn.redo.clone();
            st.selection = sn.selection.clone();
        }
        s.settings = sn.settings.clone();
        s.last_point = sn.last_point;
        s.log.truncate(sn.log_len);
        s.lisp = sn.lisp.clone();
    }

    /// Run the job from the start with the inputs so far.
    fn run(&mut self, s: &mut Session) -> Result<Step> {
        self.nested = None;
        if self.snap.is_none() {
            self.snap = Self::take_snapshot(s);
        } else {
            self.restore(s);
        }
        let mut lisp = std::mem::take(&mut s.lisp);
        let (result, out) = {
            let mut run = Run::new(s, &mut lisp, self.inputs.iter().cloned().collect::<VecDeque<_>>(), true);
            let r = match &self.job {
                Job::Eval { forms, .. } => run.eval_all(forms),
                Job::Call(name) => {
                    let f = run.lookup(&format!("c:{name}"));
                    run.call(&f, Vec::new())
                }
            };
            // The routine's own error handler.
            let r = match r {
                Err(Ex::Error(m)) => {
                    let handler = run.lookup("*error*");
                    if matches!(handler, V::Fun(_)) {
                        let _ = run.call(&handler, vec![V::Str(m.clone())]);
                    }
                    Err(Ex::Error(m))
                }
                other => other,
            };
            (r, run.take_output())
        };
        let shown = to_text(&result.clone().unwrap_or(V::Nil), true, &lisp.sets);
        s.lisp = lisp;
        for l in out.split('\n').filter(|l| !l.trim().is_empty()) {
            s.echo(l.to_string());
        }
        match result {
            Ok(v) => {
                let echo = match &self.job {
                    Job::Eval { echo_result, .. } => *echo_result,
                    Job::Call(_) => true,
                };
                if echo && v != V::Sym(String::new()) {
                    s.echo(shown);
                }
                Ok(Step::Done)
            }
            Err(Ex::Need(p)) => {
                // Keep the waiting command for its preview; replay rebuilds it.
                self.nested = s.running.take();
                self.prompt = Some(p);
                Ok(Step::Continue)
            }
            Err(Ex::Quit) => {
                s.running = None;
                Ok(Step::Done)
            }
            Err(Ex::Error(m)) => {
                s.running = None;
                s.echo(format!("; lỗi: {m}"));
                Ok(Step::Done)
            }
        }
    }
}

impl Interactive for LispM {
    fn name(&self) -> &'static str {
        "LISP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.run(s)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.prompt.clone().unwrap_or_else(|| Prompt::new("", Accept::POINT))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.inputs.len() > 2000 {
            s.echo("; lỗi: quá nhiều lần nhập trong một lệnh LISP");
            return Ok(Step::Done);
        }
        self.inputs.push(i);
        self.run(s)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<cadcraft_doc::EntityKind> {
        match &self.nested {
            Some(r) => r.machine.preview(s, c),
            None => Vec::new(),
        }
    }
}

/// Text of a `.lsp` / `.scr` file: UTF-8 (with or without BOM), else Windows-1252 bytes.
pub fn decode_source(b: &[u8]) -> String {
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| char::from(c)).collect(),
    }
}
