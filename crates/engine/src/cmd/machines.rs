//! Building blocks for interactive commands: object selection and simple point sequences.

use cadcraft_doc::Handle;

use crate::{Accept, Input, Prompt, Result, Session};

/// A "Select objects:" phase. Uses the pickfirst selection when there is one.
#[derive(Clone, Debug, Default)]
pub struct SelectPhase {
    pub picked: Vec<Handle>,
    pub done: bool,
    pub single: bool,
    pub removing: bool,
}

pub enum SelOutcome {
    /// Keep prompting.
    More,
    /// Selection finished with these objects.
    Done(Vec<Handle>),
    /// Selection finished empty: the command should end.
    Empty,
}

impl SelectPhase {
    /// Start with the pickfirst selection, if any (then the phase is already done).
    pub fn begin(s: &mut Session) -> Self {
        let pre = s.selection();
        if !pre.is_empty() && s.settings.pickfirst {
            s.remember_selection(&pre);
            SelectPhase { picked: pre, done: true, single: false, removing: false }
        } else {
            SelectPhase::default()
        }
    }
    pub fn single() -> Self {
        SelectPhase { single: true, ..Default::default() }
    }
    pub fn prompt(&self) -> Prompt {
        let msg = if self.removing {
            "Remove objects"
        } else if self.single {
            "Select object"
        } else {
            "Select objects"
        };
        Prompt::new(msg, Accept::SELECT)
    }
    pub fn feed(&mut self, s: &mut Session, i: &Input) -> Result<SelOutcome> {
        match i {
            Input::Pick(hs) => {
                let before = self.picked.len();
                if self.removing {
                    self.picked.retain(|h| !hs.contains(h));
                } else {
                    for h in hs {
                        if !self.picked.contains(h) {
                            self.picked.push(*h);
                        }
                    }
                }
                let n = hs.len();
                let _ = before;
                if n > 0 {
                    s.echo(format!("{n} found, {} total", self.picked.len()));
                } else {
                    s.echo("0 found");
                }
                s.set_selection(self.picked.clone());
                if self.single && !self.picked.is_empty() {
                    return Ok(self.finish(s));
                }
                Ok(SelOutcome::More)
            }
            Input::Text(t) | Input::Keyword(t) => {
                match t.trim().to_ascii_lowercase().as_str() {
                    "r" | "remove" => self.removing = true,
                    "a" | "add" => self.removing = false,
                    _ => s.echo("*Invalid selection*"),
                }
                Ok(SelOutcome::More)
            }
            Input::Enter => Ok(self.finish(s)),
            _ => Ok(SelOutcome::More),
        }
    }
    fn finish(&mut self, s: &mut Session) -> SelOutcome {
        self.done = true;
        if self.picked.is_empty() {
            SelOutcome::Empty
        } else {
            s.remember_selection(&self.picked);
            SelOutcome::Done(self.picked.clone())
        }
    }
}

/// Parse a number from text input.
pub fn number(t: &str) -> Option<f64> {
    crate::units::parse_distance(t)
}
