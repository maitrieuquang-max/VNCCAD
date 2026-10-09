//! VNCCad Vietnamese tools.
//!
//! VNCONVERT: turn text written with legacy Vietnamese fonts (TCVN3 `.VnTime…`, VNI `VNI-…`)
//! into Unicode. Opening a drawing already does this for styles whose font name shows the
//! encoding; the command repeats it on demand, or forces an encoding on chosen objects when the
//! legacy font has an unusual name.

use cadcraft_doc::vnlegacy::{self, Legacy, VnReport};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

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
        CommandSpec::new("plotwindow", "Plot Window to PDF", run_plotwindow)
            .menu(&["File", "Plot Window to PDF..."])
            .alias(&["vungin", "inkhung", "pw"])
            .params("{p1: [x,y], p2: [x,y], paper?: \"A3\" (A4…A0), landscape?} | {extents: true} → sets the model page setup's plot window; interactively also opens the PDF save dialog")
            .interactive(|_| Ok(Box::new(PlotWindowM::default()))),
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
    Ok(json!({ "scale": v, "factor": f, "message": msg }))
}

fn run_plotstyle(s: &mut Session, p: &Value) -> Result<Value> {
    let name =
        str_param(p, "name").ok_or_else(|| bad("plotstyle", "`name` is required (monochrome, grayscale, <file>.ctb, none)"))?.trim().to_string();
    let none = name.is_empty() || name.eq_ignore_ascii_case("none") || name.eq_ignore_ascii_case("khong");
    if !none {
        cadcraft_io::ctb::find(&name).map_err(|e| bad("plotstyle", e))?;
    }
    // Saved as a file name (AutoCAD expects "monochrome.ctb", not "monochrome").
    let value = if none {
        String::new()
    } else if name.to_ascii_lowercase().ends_with(".ctb") {
        name.clone()
    } else {
        format!("{name}.ctb")
    };
    match s.layout_space() {
        cadcraft_doc::Space::Paper(layout) => {
            let d = s.doc_mut()?;
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name == layout) {
                l.page.plot_style_table = value.clone();
            }
        }
        cadcraft_doc::Space::Model => {
            // The model page setup (saved in the drawing's "Model" layout like AutoCAD).
            let mut page = cadcraft_io::pdf::model_page(s.doc()?);
            page.plot_style_table = value.clone();
            let d = s.doc_mut()?;
            d.model_page = Some(page);
            d.header.set("VNCCAD_PLOTSTYLE", cadcraft_doc::HVal::Str(String::new()));
        }
    }
    let msg = if none { "Đã bỏ bảng nét in.".to_string() } else { format!("Bảng nét in: {name} (áp dụng khi PLOT/EXPORTPDF).") };
    Ok(json!({ "name": value, "message": msg }))
}

/// Set the model page setup to plot `w` on `paper` (orientation from the window's shape unless
/// given).
fn set_plot_window(s: &mut Session, w: [f64; 4], paper: Option<&str>, landscape: Option<bool>) -> Result<String> {
    let mut page = cadcraft_io::pdf::model_page(s.doc()?);
    if let Some(name) = paper {
        let ps =
            cadcraft_render::paper_size(name).ok_or_else(|| bad("plotwindow", format!("khổ giấy không hợp lệ `{name}` (A4, A3, A2, A1, A0)")))?;
        page.paper = ps.name.into();
        page.width_mm = ps.width_mm;
        page.height_mm = ps.height_mm;
    }
    page.landscape = landscape.unwrap_or(w[2] - w[0] > w[3] - w[1]);
    page.plot_area = "window".into();
    page.window = Some(w);
    page.scale_to_fit = true;
    let msg = format!(
        "Vùng in {:.0} × {:.0}, khổ {} {}, vừa khổ giấy.",
        w[2] - w[0],
        w[3] - w[1],
        page.paper.trim_start_matches("ISO ").split_whitespace().next().unwrap_or(&page.paper),
        if page.landscape { "ngang" } else { "dọc" }
    );
    s.doc_mut()?.model_page = Some(page);
    s.touch();
    Ok(msg)
}

fn run_plotwindow(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("extents").and_then(Value::as_bool) == Some(true) {
        let mut page = cadcraft_io::pdf::model_page(s.doc()?);
        page.plot_area = "extents".into();
        page.window = None;
        s.doc_mut()?.model_page = Some(page);
        s.touch();
        let msg = "In toàn bộ bản vẽ (Extents).".to_string();
        return Ok(json!({ "message": msg }));
    }
    let a = point_req("plotwindow", p, "p1")?;
    let b = point_req("plotwindow", p, "p2")?;
    let w = super::layout::window_param(&json!([[a.x, a.y], [b.x, b.y]])).ok_or_else(|| bad("plotwindow", "vùng in phải có kích thước"))?;
    let msg = set_plot_window(s, w, str_param(p, "paper"), p.get("landscape").and_then(Value::as_bool))?;
    Ok(json!({ "window": w, "message": msg }))
}

/// PLOTWINDOW: pick the corners of a sheet (title block frame), choose the paper, then save it
/// as PDF — the usual way to plot one of many sheets drawn in model space.
#[derive(Default)]
struct PlotWindowM {
    first: Option<Vec2>,
    window: Option<[f64; 4]>,
}

const PAPERS: [&str; 5] = ["A4", "A3", "A2", "A1", "A0"];

impl Interactive for PlotWindowM {
    fn name(&self) -> &'static str {
        "PLOTWINDOW"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match (self.first, self.window) {
            (None, _) => Prompt::new("Chọn góc thứ nhất của vùng in (khung tên)", Accept::POINT).kw(&["Extents"]),
            (Some(a), None) => Prompt::new("Chọn góc đối diện", Accept::POINT).base(a),
            (_, Some(_)) => {
                let cur = s.doc().map(cadcraft_io::pdf::model_page).map(|p| p.paper).unwrap_or_default();
                let def = PAPERS.iter().find(|n| cur.starts_with(&format!("ISO {n}")) || cur == **n).copied().unwrap_or("A3");
                Prompt::new("Khổ giấy", Accept::TEXT).kw(&PAPERS).default(def)
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.window.is_some() {
            let paper = match i {
                Input::Keyword(k) => k,
                Input::Text(t) => t.trim().to_ascii_uppercase(),
                Input::Enter => {
                    let p = self.prompt(s);
                    p.default.clone().unwrap_or_else(|| "A3".into())
                }
                _ => return Ok(Step::Continue),
            };
            let paper = if paper.is_empty() { "A3".to_string() } else { paper };
            let Some(w) = self.window else { return Ok(Step::Done) };
            let msg = set_plot_window(s, w, Some(&paper), None)?;
            s.echo(msg);
            // The host asks where to save the PDF (desktop) or downloads it (web).
            s.ui_requests.push(("plot".into(), Value::Null));
            return Ok(Step::Done);
        }
        match i {
            Input::Keyword(k) if k == "Extents" => {
                let r = run_plotwindow(s, &json!({ "extents": true }))?;
                super::group::echo_msg(s, &r);
                s.ui_requests.push(("plot".into(), Value::Null));
                Ok(Step::Done)
            }
            Input::Point(p) => match self.first {
                None => {
                    self.first = Some(p);
                    Ok(Step::Continue)
                }
                Some(a) => {
                    match super::layout::window_param(&json!([[a.x, a.y], [p.x, p.y]])) {
                        Some(w) => self.window = Some(w),
                        None => s.echo("Vùng in phải có kích thước; chọn lại góc đối diện."),
                    }
                    Ok(Step::Continue)
                }
            },
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<cadcraft_doc::EntityKind> {
        match (self.first, self.window) {
            (Some(a), None) => vec![super::helpers::lwpoly(super::helpers::rect_vertices(a, c), true)],
            _ => Vec::new(),
        }
    }
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

    #[test]
    fn plot_window_sets_the_model_page() {
        let mut s = Session::new();
        let r = s.execute("plotwindow", &json!({"p1": [0, 0], "p2": [840, 594], "paper": "A1"})).unwrap();
        assert!(r["message"].as_str().unwrap().contains("ngang"));
        let page = s.doc().unwrap().model_page.clone().unwrap();
        assert_eq!(page.plot_area, "window");
        assert_eq!(page.window, Some([0.0, 0.0, 840.0, 594.0]));
        assert!(page.landscape && page.paper.contains("A1"));
        assert!(s.execute("plotwindow", &json!({"p1": [0, 0], "p2": [0, 5]})).is_err());
        assert!(s.execute("plotwindow", &json!({"p1": [0, 0], "p2": [5, 5], "paper": "B9"})).is_err());
        s.execute("plotwindow", &json!({"extents": true})).unwrap();
        assert_eq!(s.doc().unwrap().model_page.clone().unwrap().plot_area, "extents");
        // Interactive: two corners, the paper, then the host is asked to save the PDF.
        s.start("plotwindow").unwrap();
        s.cmdline("10,10").unwrap();
        s.cmdline("110,300").unwrap();
        s.cmdline("A3").unwrap();
        let page = s.doc().unwrap().model_page.clone().unwrap();
        assert_eq!(page.window, Some([10.0, 10.0, 110.0, 300.0]));
        assert!(!page.landscape && page.paper.contains("A3"));
        assert_eq!(s.ui_requests.first().map(|r| r.0.as_str()), Some("plot"));
    }
}
