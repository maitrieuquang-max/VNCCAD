//! VNCCad autosave and crash recovery.
//!
//! Every drawing with unsaved changes is written as DXF to a host-provided store (a folder on
//! desktop, browser storage on the web) a short while after it changes. A drawing that is saved
//! normally has its autosave removed. On the next start, autosaves left over from an earlier run
//! (a crash, a power cut, or quitting without saving) are offered for recovery.
//!
//! The host supplies the storage through [`AutosaveStore`]; this module only decides when to save
//! and drives the recovery dialog, so it stays portable (no file system or browser APIs here).

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_doc::Drawing;
use serde_json::json;

use crate::CadApp;

/// One autosave found in the store.
#[derive(Clone, Debug)]
pub struct AutosaveEntry {
    /// Store key (unique per drawing per app run).
    pub key: String,
    /// Drawing title when it was autosaved.
    pub title: String,
    /// The drawing's own file, if it had one.
    pub path: Option<String>,
    /// Human-readable age, e.g. "5 phút trước".
    pub age: String,
}

/// Host storage for autosaves.
pub struct AutosaveStore {
    /// Unique per app run, so keys never collide with a previous run's leftovers.
    pub run_tag: String,
    /// Seconds between autosaves of a drawing that keeps changing.
    pub interval_s: f64,
    /// Where autosaves live, shown to the user (e.g. a folder path).
    pub location: String,
    /// Store `dxf` under `key` with its title and original path.
    pub put: Box<dyn Fn(&str, &str, Option<&str>, &[u8]) -> Result<(), String>>,
    /// Every autosave in the store.
    pub list: Box<dyn Fn() -> Vec<AutosaveEntry>>,
    /// The DXF bytes stored under `key`.
    pub get: Box<dyn Fn(&str) -> Option<Vec<u8>>>,
    /// Delete the autosave stored under `key`.
    pub remove: Box<dyn Fn(&str)>,
}

/// Per-app autosave bookkeeping.
#[derive(Default)]
pub struct AutosaveState {
    /// Per drawing uid: the content last written (None = not written yet) and when the timer
    /// started (seconds, egui time).
    tracked: HashMap<u64, (Option<Arc<Drawing>>, f64)>,
    /// Leftovers from an earlier run, waiting for the user's decision.
    pub recovery: Vec<AutosaveEntry>,
    started: bool,
    last_check: f64,
    error_shown: bool,
}

fn key(store: &AutosaveStore, uid: u64) -> String {
    format!("{}-{uid}", store.run_tag)
}

/// Run once per frame: look for leftovers on the first frame, then autosave changed drawings.
pub fn tick(app: &mut CadApp, now: f64) {
    let Some(store) = app.services.autosave.as_ref() else { return };
    if !app.autosave.started {
        app.autosave.started = true;
        app.autosave.last_check = now;
        let prefix = format!("{}-", store.run_tag);
        app.autosave.recovery = (store.list)().into_iter().filter(|e| !e.key.starts_with(&prefix)).collect();
        return;
    }
    if now - app.autosave.last_check < 2.0 {
        return;
    }
    app.autosave.last_check = now;
    save_due(app, now, false);
}

/// Write every changed drawing now (before quitting).
pub fn flush(app: &mut CadApp, now: f64) {
    save_due(app, now, true);
}

fn save_due(app: &mut CadApp, now: f64, force: bool) {
    let Some(store) = app.services.autosave.as_ref() else { return };
    let Some(io) = cadcraft_engine::cmd::file::io() else { return };
    let interval = store.interval_s.max(5.0);
    let mut errors = Vec::new();
    let live: Vec<u64> = app.session.docs.iter().map(|d| d.uid).collect();
    for d in &app.session.docs {
        let k = key(store, d.uid);
        if !d.is_dirty() {
            if app.autosave.tracked.remove(&d.uid).is_some_and(|(written, _)| written.is_some()) {
                (store.remove)(&k);
            }
            continue;
        }
        let entry = app.autosave.tracked.entry(d.uid).or_insert((None, now));
        if entry.0.as_ref().is_some_and(|w| Arc::ptr_eq(w, &d.doc)) {
            continue;
        }
        if !force && now - entry.1 < interval {
            continue;
        }
        match (io.write)(&d.doc, "autosave.dxf").and_then(|bytes| (store.put)(&k, &d.title, d.path.as_deref(), &bytes)) {
            Ok(()) => *entry = (Some(d.doc.clone()), now),
            Err(e) => {
                // Retry after another interval rather than every check.
                entry.1 = now;
                errors.push(format!("{}: {e}", d.title));
            }
        }
    }
    // Drawings closed without saving keep their autosave for the next start.
    app.autosave.tracked.retain(|uid, _| live.contains(uid));
    if !errors.is_empty() && !app.autosave.error_shown {
        app.autosave.error_shown = true;
        for e in errors {
            app.session.echo(format!("Không tự lưu được bản vẽ {e}. Hãy lưu (Save) thường xuyên."));
        }
    }
}

/// Open the chosen leftovers as new, unsaved drawings and drop their old autosaves.
fn recover(app: &mut CadApp, entries: &[AutosaveEntry], now: f64) {
    let mut opened = 0;
    for e in entries {
        let Some(bytes) = app.services.autosave.as_ref().and_then(|s| (s.get)(&e.key)) else {
            app.session.echo(format!("Không đọc được bản tự lưu của {}", e.title));
            continue;
        };
        let before = app.session.docs.len();
        app.open_bytes(&e.title, &bytes);
        if app.session.docs.len() <= before {
            continue;
        }
        opened += 1;
        if let Ok(st) = app.session.state_mut() {
            st.title = format!("{} (khôi phục)", e.title.trim_end_matches(".dxf").trim_end_matches(".dwg"));
            st.path = None;
            // Recovered work is unsaved: mark it so, and autosave it under this run at once.
            st.saved = Arc::new(Drawing::default());
            let uid = st.uid;
            app.autosave.tracked.insert(uid, (None, now - 1.0e9));
        }
        if let Some(s) = app.services.autosave.as_ref() {
            (s.remove)(&e.key);
        }
    }
    if opened > 0 {
        app.session.echo(format!("Đã khôi phục {opened} bản vẽ. Hãy lưu lại (Save As) để giữ kết quả."));
        save_due(app, now, true);
    }
}

fn discard(app: &mut CadApp, entries: &[AutosaveEntry]) {
    if let Some(s) = app.services.autosave.as_ref() {
        for e in entries {
            (s.remove)(&e.key);
        }
    }
}

/// The recovery dialog, shown while leftovers wait for a decision.
pub fn recovery_dialog(app: &mut CadApp, ctx: &egui::Context) {
    if app.autosave.recovery.is_empty() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let entries = app.autosave.recovery.clone();
    let location = app.services.autosave.as_ref().map(|s| s.location.clone()).unwrap_or_default();
    let mut choice: Option<&str> = None;
    egui::Window::new("Khôi phục bản vẽ").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(
        ctx,
        |ui| {
            ui.set_min_width(420.0);
            ui.label("VNCCad tìm thấy bản vẽ được tự lưu từ lần làm việc trước");
            ui.label("(chương trình bị đóng đột ngột hoặc thoát khi chưa lưu):");
            ui.add_space(6.0);
            for e in &entries {
                let path = e.path.as_deref().map(|p| format!("  —  {p}")).unwrap_or_default();
                ui.label(format!("•  {}   ({}){path}", e.title, e.age));
            }
            ui.add_space(6.0);
            if !location.is_empty() {
                ui.small(format!("Nơi lưu: {location}"));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Khôi phục tất cả").clicked() {
                    choice = Some("recover");
                }
                if ui.button("Xóa bản tự lưu").clicked() {
                    choice = Some("discard");
                }
                if ui.button("Để sau").on_hover_text("Giữ các bản tự lưu, hỏi lại lần mở sau").clicked() {
                    choice = Some("later");
                }
            });
        },
    );
    match choice {
        Some("recover") => {
            app.autosave.recovery.clear();
            recover(app, &entries, now);
        }
        Some("discard") => {
            app.autosave.recovery.clear();
            discard(app, &entries);
        }
        Some("later") => app.autosave.recovery.clear(),
        _ => {}
    }
}

/// Status for the control channel and tests.
pub fn status(app: &CadApp) -> serde_json::Value {
    json!({
        "enabled": app.services.autosave.is_some(),
        "tracked": app.autosave.tracked.len(),
        "recovery": app.autosave.recovery.iter().map(|e| json!({"key": e.key, "title": e.title, "age": e.age})).collect::<Vec<_>>(),
    })
}

/// Human-readable age in Vietnamese from milliseconds.
pub fn age_text(ms: f64) -> String {
    let min = (ms / 60_000.0).max(0.0).floor();
    if min < 1.0 {
        "vừa xong".into()
    } else if min < 60.0 {
        format!("{min:.0} phút trước")
    } else if min < 60.0 * 24.0 {
        format!("{:.0} giờ trước", (min / 60.0).floor())
    } else {
        format!("{:.0} ngày trước", (min / 1440.0).floor())
    }
}
