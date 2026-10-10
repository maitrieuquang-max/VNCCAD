//! VNCCad: a small driver for interactive commands that ask a few questions in turn (select
//! objects, pick a keyword, type a name or a number, give a point) and then run.
//!
//! A command supplies `plan` — the next question given the answers so far, `None` when it has
//! all it needs — and `run`, which does the work and returns a result whose `message` is echoed.

use cadcraft_doc::Handle;
use cadcraft_geom::Vec2;
use serde_json::Value;

use super::machines::{SelOutcome, SelectPhase};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// A question.
#[derive(Clone, Debug)]
pub enum Ask {
    /// Select objects (the pickfirst selection answers the first one).
    Select(String),
    /// Select one object.
    One(String),
    /// A keyword among `kws` (shortcut letters in capitals); Enter gives `default`.
    Kw { msg: String, kws: Vec<&'static str>, default: Option<&'static str> },
    /// Free text; Enter gives `default` (or is refused).
    Text { msg: String, default: Option<String> },
    /// A number; Enter gives `default`.
    Num { msg: String, default: Option<f64> },
    /// A point.
    Point(String),
}

/// An answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Ans {
    Sel(Vec<Handle>),
    Text(String),
    Num(f64),
    Point(Vec2),
}

impl Ans {
    pub fn sel(&self) -> &[Handle] {
        if let Ans::Sel(h) = self { h } else { &[] }
    }
    pub fn text(&self) -> &str {
        if let Ans::Text(t) = self { t } else { "" }
    }
    pub fn num(&self) -> Option<f64> {
        if let Ans::Num(n) = self { Some(*n) } else { None }
    }
}

pub type Plan = fn(&Session, &[Ans]) -> Option<Ask>;
pub type Run = fn(&mut Session, &[Ans]) -> Result<Value>;

pub struct Flow {
    name: &'static str,
    plan: Plan,
    run: Run,
    answers: Vec<Ans>,
    cur: Option<Ask>,
    sel: SelectPhase,
}

/// The keyword `t` names, if any: whole word, its capital letters, or a prefix.
pub fn match_kw(t: &str, kws: &[&'static str]) -> Option<&'static str> {
    let t = t.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    kws.iter()
        .copied()
        .find(|k| k.to_lowercase() == t)
        .or_else(|| {
            kws.iter().copied().find(|k| {
                let caps: String = k.chars().filter(|c| c.is_uppercase()).collect::<String>().to_lowercase();
                !caps.is_empty() && caps == t
            })
        })
        .or_else(|| kws.iter().copied().find(|k| k.to_lowercase().starts_with(&t)))
}

impl Flow {
    pub fn new(name: &'static str, plan: Plan, run: Run) -> Self {
        Flow { name, plan, run, answers: Vec::new(), cur: None, sel: SelectPhase::default() }
    }

    /// Ask the next question, or run when there is none.
    fn advance(&mut self, s: &mut Session) -> Result<Step> {
        loop {
            self.cur = (self.plan)(s, &self.answers);
            match &self.cur {
                None => {
                    let r = (self.run)(s, &self.answers)?;
                    if let Some(m) = r.get("message").and_then(Value::as_str) {
                        s.echo(m.to_string());
                    }
                    return Ok(Step::Done);
                }
                Some(Ask::Select(_)) => {
                    // The pickfirst selection answers the first selection only.
                    self.sel = if self.answers.is_empty() { SelectPhase::begin(s) } else { SelectPhase::default() };
                    if self.sel.done {
                        self.answers.push(Ans::Sel(self.sel.picked.clone()));
                        s.set_selection(Vec::new());
                        continue;
                    }
                    return Ok(Step::Continue);
                }
                Some(Ask::One(_)) => {
                    self.sel = SelectPhase::single();
                    return Ok(Step::Continue);
                }
                Some(_) => return Ok(Step::Continue),
            }
        }
    }
}

impl Interactive for Flow {
    fn name(&self) -> &'static str {
        self.name
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.advance(s)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.cur {
            Some(Ask::Select(m)) | Some(Ask::One(m)) => Prompt::new(m.clone(), Accept::SELECT),
            Some(Ask::Kw { msg, kws, default }) => {
                let mut p = Prompt::new(msg.clone(), Accept::TEXT).kw(kws);
                if let Some(d) = default {
                    p = p.default(*d);
                }
                p
            }
            Some(Ask::Text { msg, default }) => {
                let mut p = Prompt::new(msg.clone(), Accept::TEXT);
                if let Some(d) = default {
                    p = p.default(d.clone());
                }
                p
            }
            Some(Ask::Num { msg, default }) => {
                let mut p = Prompt::new(msg.clone(), Accept::NUMBER);
                if let Some(d) = default {
                    p = p.default(format!("{d}"));
                }
                p
            }
            Some(Ask::Point(m)) => Prompt::new(m.clone(), Accept::POINT),
            None => Prompt::new("", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        let Some(cur) = self.cur.clone() else { return Ok(Step::Done) };
        let ans = match cur {
            Ask::Select(_) | Ask::One(_) => match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    Ans::Sel(hs)
                }
            },
            Ask::Kw { kws, default, .. } => match &i {
                Input::Enter => match default {
                    Some(d) => Ans::Text(d.to_string()),
                    None => return Ok(Step::Continue),
                },
                Input::Text(t) | Input::Keyword(t) => match match_kw(t, &kws) {
                    Some(k) => Ans::Text(k.to_string()),
                    None => {
                        s.echo(format!("Hãy chọn một trong: {}", kws.join(", ")));
                        return Ok(Step::Continue);
                    }
                },
                _ => return Ok(Step::Continue),
            },
            Ask::Text { default, .. } => match i {
                Input::Enter => match default {
                    Some(d) => Ans::Text(d),
                    None => return Ok(Step::Continue),
                },
                Input::Text(t) | Input::Keyword(t) => Ans::Text(t.trim().to_string()),
                _ => return Ok(Step::Continue),
            },
            Ask::Num { default, .. } => match i {
                Input::Enter => match default {
                    Some(d) => Ans::Num(d),
                    None => return Ok(Step::Continue),
                },
                Input::Text(t) | Input::Keyword(t) => match super::machines::number(&t) {
                    Some(n) => Ans::Num(n),
                    None => {
                        s.echo("Cần một số.");
                        return Ok(Step::Continue);
                    }
                },
                _ => return Ok(Step::Continue),
            },
            Ask::Point(_) => match i {
                Input::Point(p) => Ans::Point(p),
                Input::Text(t) => match crate::prompt::parse_point(&t, s.last_point) {
                    Some(p) => Ans::Point(p),
                    None => return Ok(Step::Continue),
                },
                _ => return Ok(Step::Continue),
            },
        };
        self.answers.push(ans);
        self.advance(s)
    }
}
