//! VNCCad: a self test run inside the app (`?selftest` on the web build), used by CI to check
//! the real browser build: opening DWG files through the same path as a user's file (a small one
//! with Vietnamese and Chinese text, and a large one), running LISP, and getting a CJK font.
//!
//! The host feeds [`SelfTest::files`] to the app as if the user had opened them, then calls
//! [`SelfTest::step`] every frame until it returns the result.

use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Line, Space, Text};
use cadcraft_geom::Vec3;

use crate::CadApp;

/// Texts in the small test drawing (they must survive DWG → DXF → drawing).
pub const TEXTS: [&str; 3] = ["Mố cầu M1 – cọc khoan nhồi D1000", "建筑面积 中汽工程", "Lý trình Km1+250.50"];
/// Entities in the large test drawing.
pub const BIG: usize = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Small,
    Big,
    Lisp,
    Cjk,
    Done,
}

pub struct SelfTest {
    stage: Stage,
    started: f64,
    stage_started: f64,
    pub want_cjk: bool,
    asked_font: bool,
    notes: Vec<String>,
    failures: Vec<String>,
}

fn text(at: (f64, f64), h: f64, s: &str) -> EntityKind {
    EntityKind::Text(Text {
        insert: Vec3::new(at.0, at.1, 0.0),
        align_pt: None,
        height: h,
        value: s.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    })
}

fn dwg(d: &Drawing) -> Vec<u8> {
    cadcraft_io::write(d, "selftest.dwg").unwrap_or_default()
}

impl SelfTest {
    pub fn new(now_ms: f64, want_cjk: bool) -> Self {
        SelfTest { stage: Stage::Small, started: now_ms, stage_started: now_ms, want_cjk, asked_font: false, notes: Vec::new(), failures: Vec::new() }
    }

    /// The test files: (name, DWG bytes).
    pub fn files() -> Vec<(String, Vec<u8>)> {
        let mut small = Drawing::new_metric();
        for (i, t) in TEXTS.iter().enumerate() {
            let _ = small.add(&Space::Model, Common::default(), text((0.0, i as f64 * 10.0), 2.5, t));
        }
        let _ = small.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, -5.0, 0.0), b: Vec3::new(100.0, -5.0, 0.0) }));
        let mut big = Drawing::new_metric();
        for i in 0..BIG {
            let (x, y) = ((i % 300) as f64 * 10.0, (i / 300) as f64 * 10.0);
            let k = if i % 10 == 0 {
                text((x, y), 2.0, &format!("Cọc {i}"))
            } else {
                EntityKind::Line(Line { a: Vec3::new(x, y, 0.0), b: Vec3::new(x + 8.0, y + 6.0, 0.0) })
            };
            let _ = big.add(&Space::Model, Common::default(), k);
        }
        vec![("selftest-small.dwg".into(), dwg(&small)), ("selftest-big.dwg".into(), dwg(&big))]
    }

    fn doc_named<'a>(app: &'a CadApp, name: &str) -> Option<&'a Drawing> {
        app.session.docs.iter().find(|d| d.title.eq_ignore_ascii_case(name)).map(|d| d.doc.as_ref())
    }

    fn next(&mut self, stage: Stage, now: f64) {
        self.stage = stage;
        self.stage_started = now;
    }

    /// One frame. `Some` when finished: `Ok(summary)` or `Err(what failed)`.
    pub fn step(&mut self, app: &mut CadApp, now: f64) -> Option<Result<String, String>> {
        let waited = now - self.stage_started;
        match self.stage {
            Stage::Small => {
                if let Some(d) = Self::doc_named(app, "selftest-small.dwg") {
                    let texts: Vec<String> = d
                        .model
                        .iter()
                        .filter_map(|e: &std::sync::Arc<Entity>| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None })
                        .collect();
                    for t in TEXTS {
                        if !texts.iter().any(|x| x == t) {
                            self.failures.push(format!("DWG nhỏ: thiếu chữ `{t}` (đọc được {texts:?})"));
                        }
                    }
                    self.notes.push(format!("DWG nhỏ mở sau {:.0} ms", now - self.started));
                    self.next(Stage::Big, now);
                } else if waited > 60_000.0 {
                    self.failures.push("DWG nhỏ: không mở được sau 60 s".into());
                    self.next(Stage::Big, now);
                }
            }
            Stage::Big => {
                if let Some(d) = Self::doc_named(app, "selftest-big.dwg") {
                    let n = d.model.len();
                    if n != BIG {
                        self.failures.push(format!("DWG lớn: {n} đối tượng, cần {BIG}"));
                    }
                    self.notes.push(format!("DWG lớn ({BIG} đối tượng) mở sau {:.0} ms", waited));
                    self.next(Stage::Lisp, now);
                } else if waited > 180_000.0 {
                    self.failures.push("DWG lớn: không mở được sau 180 s".into());
                    self.next(Stage::Lisp, now);
                }
            }
            Stage::Lisp => {
                let _ = app.session.cmdline("(setq vnc_selftest (* 6 7))");
                match cadcraft_engine::lisp::eval_quiet(&mut app.session, "vnc_selftest") {
                    Ok(v) if cadcraft_engine::lisp::to_text(&v, false, &Default::default()) == "42" => self.notes.push("LISP chạy đúng".into()),
                    other => self.failures.push(format!("LISP: {other:?}")),
                }
                self.next(if self.want_cjk { Stage::Cjk } else { Stage::Done }, now);
            }
            Stage::Cjk => {
                if cadcraft_fonts::ttf::fallback_cjk().is_some() {
                    self.notes.push(format!("Font CJK có sau {:.0} ms", waited));
                    self.next(Stage::Done, now);
                } else if !self.asked_font {
                    self.asked_font = true;
                    let _ = app.run("cjkfont", serde_json::json!({}));
                } else if waited > 120_000.0 {
                    self.failures.push("Font CJK: không có sau 120 s".into());
                    self.next(Stage::Done, now);
                }
            }
            Stage::Done => {
                let summary = format!("{} (tổng {:.0} ms)", self.notes.join("; "), now - self.started);
                return Some(if self.failures.is_empty() { Ok(summary) } else { Err(format!("{} | {summary}", self.failures.join("; "))) });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selftest_passes_on_the_desktop_build() {
        cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
            read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
            plot: None,
        });
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let mut t = SelfTest::new(0.0, false);
        for (name, bytes) in SelfTest::files() {
            assert!(cadcraft_io::is_dwg(&bytes), "{name}");
            let dxf = cadcraft_io::dwg_to_dxf(&bytes).unwrap();
            app.open_bytes(&name, &dxf);
        }
        let mut r = None;
        for f in 0..100 {
            r = t.step(&mut app, f as f64 * 10.0);
            if r.is_some() {
                break;
            }
        }
        let r = r.expect("finished");
        assert!(r.is_ok(), "{r:?}");
    }
}
