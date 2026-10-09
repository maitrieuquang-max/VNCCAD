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
        CommandSpec::new("cannoscale", "Annotation Scale", run_cannoscale)
            .menu(&["Format", "Annotation Scale"])
            .alias(&["tyle", "tylechuthich"])
            .params("{scale: \"1:100\" | \"1:500\" | … } → annotative text and dimensions in model space are drawn at this scale"),
        CommandSpec::new("plotstyle", "Plot Style Table", run_plotstyle)
            .menu(&["File", "Plot Style Table..."])
            .alias(&["ctb", "bangnet"])
            .params("{name: \"monochrome\" | \"grayscale\" | \"<file>.ctb\" | \"none\"} → used by PLOT/EXPORTPDF of the current layout (or of model space)"),
        CommandSpec::new("shxfonts", "SHX Fonts", run_shxfonts)
            .menu(&["Tools", "Vietnamese", "SHX Fonts"])
            .params("{} → the SHX fonts loaded and the folders searched")
            .enabled(always)
            .noundo(),
    ]
}

fn run_cannoscale(s: &mut Session, p: &Value) -> Result<Value> {
    let Some(v) = str_param(p, "scale") else {
        let cur = s.doc()?.header.str("CANNOSCALE", "1:1");
        return Ok(json!({ "scale": cur, "message": format!("Tỷ lệ chú thích hiện tại: {cur}") }));
    };
    let v = v.trim().to_string();
    let f = cadcraft_render::parse_anno_scale(&v);
    if (f - 1.0).abs() < 1e-12 && !matches!(v.as_str(), "1:1" | "1/1" | "1") {
        return Err(bad("cannoscale", format!("tỷ lệ không hợp lệ `{v}` (ví dụ 1:100)")));
    }
    s.doc_mut()?.header.set("CANNOSCALE", cadcraft_doc::HVal::Str(v.clone()));
    let msg = format!("Tỷ lệ chú thích: {v}. Chữ và kích thước có kiểu annotative trong Model vẽ theo tỷ lệ này.");
    s.echo(msg.clone());
    Ok(json!({ "scale": v, "factor": f, "message": msg }))
}

fn run_plotstyle(s: &mut Session, p: &Value) -> Result<Value> {
    let name =
        str_param(p, "name").ok_or_else(|| bad("plotstyle", "`name` is required (monochrome, grayscale, <file>.ctb, none)"))?.trim().to_string();
    let none = name.is_empty() || name.eq_ignore_ascii_case("none") || name.eq_ignore_ascii_case("khong");
    if !none {
        cadcraft_io::ctb::find(&name).map_err(|e| bad("plotstyle", e))?;
    }
    let value = if none { String::new() } else { name.clone() };
    match s.layout_space() {
        cadcraft_doc::Space::Paper(layout) => {
            let d = s.doc_mut()?;
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name == layout) {
                l.page.plot_style_table = value.clone();
            }
        }
        cadcraft_doc::Space::Model => {
            let d = s.doc_mut()?;
            if none {
                d.header.set("VNCCAD_PLOTSTYLE", cadcraft_doc::HVal::Str("None".into()));
            } else {
                d.header.set("VNCCAD_PLOTSTYLE", cadcraft_doc::HVal::Str(value.clone()));
            }
        }
    }
    let msg = if none { "Đã bỏ bảng nét in.".to_string() } else { format!("Bảng nét in: {name} (áp dụng khi PLOT/EXPORTPDF).") };
    s.echo(msg.clone());
    Ok(json!({ "name": value, "message": msg }))
}

fn run_shxfonts(_s: &mut Session, _p: &Value) -> Result<Value> {
    let fonts = cadcraft_fonts::shx::known();
    #[cfg(not(target_arch = "wasm32"))]
    let dirs: Vec<String> = cadcraft_fonts::shx::font_dirs().iter().map(|d| d.display().to_string()).collect();
    #[cfg(target_arch = "wasm32")]
    let dirs: Vec<String> = Vec::new();
    let msg = if fonts.is_empty() {
        "Chưa nạp font SHX nào. Đặt file .shx cạnh bản vẽ, trong thư mục VNCCad/fonts, hoặc kéo-thả file .shx vào cửa sổ.".to_string()
    } else {
        format!("Font SHX đã nạp: {}", fonts.join(", "))
    };
    Ok(json!({ "fonts": fonts, "dirs": dirs, "message": msg }))
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

    #[test]
    fn annotation_scale_and_plot_style() {
        let mut s = Session::new();
        let r = s.execute("cannoscale", &json!({"scale": "1:100"})).unwrap();
        assert_eq!(r["factor"], 100.0);
        assert!(s.execute("cannoscale", &json!({"scale": "abc"})).is_err());
        // A red line plots red without a table and black with "monochrome".
        let line = cadcraft_doc::EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::ZERO, b: cadcraft_geom::Vec3::new(100.0, 0.0, 0.0) });
        let common = cadcraft_doc::Common { color: cadcraft_color::Color::Index(1), ..cadcraft_doc::Common::default() };
        s.doc_mut().unwrap().add(&cadcraft_doc::Space::Model, common, line).unwrap();
        // The real plotter (other tests may install fake IO hooks first).
        let pdf_text = |s: &mut Session| {
            let b = cadcraft_io::plot(s.doc().unwrap(), &cadcraft_doc::Space::Model, &json!({"compress": false})).unwrap();
            String::from_utf8_lossy(&b).into_owned()
        };
        assert!(pdf_text(&mut s).contains("1 0 0 RG"));
        s.execute("plotstyle", &json!({"name": "monochrome"})).unwrap();
        let mono = pdf_text(&mut s);
        assert!(!mono.contains("1 0 0 RG") && mono.contains("0 0 0 RG"), "monochrome plots black");
        assert!(s.execute("plotstyle", &json!({"name": "khong-co.ctb"})).is_err());
        s.execute("plotstyle", &json!({"name": "none"})).unwrap();
        assert!(pdf_text(&mut s).contains("1 0 0 RG"));
    }
}
