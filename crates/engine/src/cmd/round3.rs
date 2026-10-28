//! VNCCad: commands added after the second engineer test round — ARRAY (the classic entry
//! point that asks for the array type and hands over to ARRAYRECT / ARRAYPATH / ARRAYPOLAR).

use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec2;
use serde_json::Value;

use super::*;
use crate::prompt::{Accept, Input, Prompt, Step};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
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
