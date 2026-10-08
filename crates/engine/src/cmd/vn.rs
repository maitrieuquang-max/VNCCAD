//! VNCCad Vietnamese tools.
//!
//! VNCONVERT: turn text written with legacy Vietnamese fonts (TCVN3 `.VnTime…`, VNI `VNI-…`)
//! into Unicode. Opening a drawing already does this for styles whose font name shows the
//! encoding; the command repeats it on demand, or forces an encoding on chosen objects when the
//! legacy font has an unusual name.

use cadcraft_doc::vnlegacy::{self, Legacy, VnReport};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("vnconvert", "Convert TCVN3/VNI Text to Unicode", run_vnconvert)
            .menu(&["Tools", "Vietnamese", "Convert TCVN3/VNI Text to Unicode"])
            .alias(&["vnc", "chuyenma"])
            .params("{encoding?: \"auto\" (by text style font) | \"tcvn3\" | \"tcvn3h\" (capital-only fonts) | \"vni\", handles?: forced encodings act on these or the selection, else on every object}")
            .enabled(always),
    ]
}

/// One line for the command line, in Vietnamese.
pub fn report_text(r: &VnReport) -> String {
    let mut s = format!("Đã chuyển {} chuỗi chữ TCVN3/VNI sang Unicode", r.texts);
    if !r.styles.is_empty() {
        let list: Vec<String> = r.styles.iter().map(|(n, old, new)| format!("{n}: {old} → {new}")).collect();
        s.push_str(&format!("; kiểu chữ: {}", list.join(", ")));
    }
    s.push('.');
    s
}

fn run_vnconvert(s: &mut Session, p: &Value) -> Result<Value> {
    let enc = match str_param(p, "encoding").map(str::to_ascii_lowercase).as_deref() {
        None | Some("auto" | "") => None,
        Some("tcvn3" | "abc" | "vn3") => Some(Legacy::Tcvn3 { upper: false }),
        Some("tcvn3h" | "abch" | "vn3h") => Some(Legacy::Tcvn3 { upper: true }),
        Some("vni") => Some(Legacy::Vni),
        Some(other) => return Err(bad("vnconvert", format!("unknown encoding `{other}` (auto, tcvn3, tcvn3h, vni)"))),
    };
    match enc {
        None => {
            let r = vnlegacy::convert_drawing(s.doc_mut()?);
            let msg = report_text(&r);
            s.echo(msg.clone());
            Ok(json!({ "texts": r.texts, "styles": r.styles.len(), "message": msg }))
        }
        Some(enc) => {
            let mut hs = targets(s, p)?;
            if hs.is_empty() {
                let d = s.doc()?;
                hs = d.model.iter().map(|e| e.handle).chain(d.layouts.iter().flat_map(|l| l.entities.iter().map(|e| e.handle))).collect();
            }
            let n = vnlegacy::convert_entities(s.doc_mut()?, &hs, enc);
            let msg = format!("Đã chuyển {n} chuỗi chữ sang Unicode.");
            s.echo(msg.clone());
            Ok(json!({ "texts": n, "message": msg }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_conversion_and_undo() {
        let mut s = Session::new();
        s.cmdline("text 0,0 2.5 0 C\u{C7}u").unwrap();
        s.cmdline("").unwrap();
        let before = s.doc().unwrap().model.iter().count();
        assert!(before >= 1);
        let r = s.execute("vnconvert", &json!({"encoding": "tcvn3"})).unwrap();
        assert_eq!(r["texts"], 1);
        let has =
            |s: &Session, want: &str| s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, cadcraft_doc::EntityKind::Text(t) if t.value == want));
        assert!(has(&s, "Cầu"));
        s.execute("undo", &json!({})).unwrap();
        assert!(has(&s, "C\u{C7}u"));
        assert!(s.execute("vnconvert", &json!({"encoding": "xyz"})).is_err());
    }
}
