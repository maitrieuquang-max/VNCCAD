//! VNCCad: the PURGE dialog (what can be purged, by category, with check boxes) and the OPTIONS
//! dialog (display, open/save, drafting, selection settings; kept between sessions).

use egui::RichText;
use serde_json::{Value, json};

use crate::CadApp;

const OPTIONS_PREF: &str = "options";

// =====================================================================================
// PURGE

#[derive(Default)]
pub struct PurgeDlg {
    /// (category, label, names with a check box each).
    groups: Vec<(String, String, Vec<(String, bool)>)>,
    nested: bool,
    filter: String,
}

impl PurgeDlg {
    pub fn load(app: &mut CadApp) -> Self {
        let list = app.session.execute("purge", &json!({ "list": true })).unwrap_or(Value::Null);
        let groups = cadcraft_engine::cmd::round3::PURGE_KINDS
            .iter()
            .map(|(k, label)| {
                let names = list
                    .get(*k)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(|n| (n.to_string(), true))).collect())
                    .unwrap_or_default();
                (k.to_string(), label.to_string(), names)
            })
            .collect();
        PurgeDlg { groups, nested: true, filter: String::new() }
    }
    fn total(&self) -> usize {
        self.groups.iter().map(|g| g.2.len()).sum()
    }
}

pub fn purge_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut dl) = app.vd.purge.take() else { return };
    let mut open = true;
    let mut action: Option<&str> = None;
    egui::Window::new("PURGE — Xóa mục không dùng").open(&mut open).resizable(true).default_width(460.0).collapsible(false).show(ctx, |ui| {
        ui.label(format!("Các mục không dùng trong bản vẽ: {}", dl.total()));
        ui.horizontal(|ui| {
            ui.label("Lọc");
            ui.add(egui::TextEdit::singleline(&mut dl.filter).desired_width(200.0));
        });
        let f = dl.filter.to_lowercase();
        egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
            for (kind, label, names) in &mut dl.groups {
                let n = names.len();
                let checked = names.iter().filter(|x| x.1).count();
                egui::CollapsingHeader::new(format!("{label} ({checked}/{n})")).id_salt(kind.as_str()).default_open(n > 0 && n <= 30).show(
                    ui,
                    |ui| {
                        if n == 0 {
                            ui.label(RichText::new("Không có").weak());
                            return;
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("Chọn hết").clicked() {
                                names.iter_mut().for_each(|x| x.1 = true);
                            }
                            if ui.small_button("Bỏ chọn").clicked() {
                                names.iter_mut().for_each(|x| x.1 = false);
                            }
                        });
                        for (name, on) in names.iter_mut() {
                            if f.is_empty() || name.to_lowercase().contains(&f) {
                                ui.checkbox(on, name.as_str());
                            }
                        }
                    },
                );
            }
        });
        ui.separator();
        ui.checkbox(&mut dl.nested, "Xóa cả mục lồng nhau (Purge nested items)");
        ui.horizontal(|ui| {
            if ui.button("Xóa mục đã chọn (Purge Checked)").clicked() {
                action = Some("checked");
            }
            if ui.button("Xóa tất cả (Purge All)").clicked() {
                action = Some("all");
            }
            if ui.button("Đóng").clicked() {
                action = Some("close");
            }
        });
    });
    match action {
        Some("checked") | Some("all") => {
            let all = action == Some("all");
            let items: serde_json::Map<String, Value> = dl
                .groups
                .iter()
                .map(|(k, _, v)| (k.clone(), json!(v.iter().filter(|x| all || x.1).map(|x| x.0.clone()).collect::<Vec<_>>())))
                .collect();
            let p = json!({ "items": items, "nested": dl.nested });
            match app.run("purge", p) {
                Ok(v) => {
                    if let Some(m) = v.get("message").and_then(Value::as_str) {
                        app.session.echo(m.to_string());
                    }
                }
                Err(e) => app.session.echo(e),
            }
            dl = PurgeDlg { nested: dl.nested, ..PurgeDlg::load(app) };
        }
        Some("close") => open = false,
        _ => {}
    }
    if open {
        app.vd.purge = Some(dl);
    }
}

// =====================================================================================
// OPTIONS

pub struct OptionsDlg {
    tab: usize,
    s: cadcraft_engine::Settings,
    polar_deg: f64,
    autosave_min: f64,
    dwg: String,
    ui: crate::UiState,
}

impl OptionsDlg {
    pub fn load(app: &CadApp) -> Self {
        let s = app.session.settings.clone();
        let dwg = app.session.doc().map(|d| d.header.str("ACADVER", "AC1021")).unwrap_or_else(|_| "AC1021".into());
        OptionsDlg {
            tab: 0,
            polar_deg: s.polarang.to_degrees(),
            s,
            autosave_min: app.services.autosave.as_ref().map(|a| a.interval_s / 60.0).unwrap_or(2.0),
            dwg: year_of(&dwg).to_string(),
            ui: app.ui.clone(),
        }
    }
}

fn year_of(code: &str) -> &'static str {
    match code {
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => "2007",
    }
}

/// Saved options, applied at start-up.
pub fn load_saved(app: &mut CadApp) {
    let Some(v) = app.services.prefs_get.as_ref().and_then(|g| g(OPTIONS_PREF)).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return };
    if let Some(s) = v.get("settings").and_then(|x| serde_json::from_value::<cadcraft_engine::Settings>(x.clone()).ok()) {
        app.session.settings = s;
    }
    if let Some(m) = v.get("autosaveMin").and_then(Value::as_f64).filter(|m| m.is_finite() && *m >= 0.5)
        && let Some(a) = app.services.autosave.as_mut()
    {
        a.interval_s = m * 60.0;
    }
    if let Some(u) = v.get("ui") {
        let b = |k: &str, d: bool| u.get(k).and_then(Value::as_bool).unwrap_or(d);
        app.ui.show_toolbar = b("toolbar", app.ui.show_toolbar);
        app.ui.show_status_bar = b("statusBar", app.ui.show_status_bar);
        app.ui.show_file_tabs = b("fileTabs", app.ui.show_file_tabs);
        app.ui.show_viewcube = b("viewcube", app.ui.show_viewcube);
        app.ui.show_ucs_icon = b("ucsIcon", app.ui.show_ucs_icon);
        if let Some(n) = u.get("historyLines").and_then(Value::as_u64) {
            app.ui.history_lines = (n as usize).min(12);
        }
    }
    if let Some(y) = v.get("dwg").and_then(Value::as_str) {
        app.vd.dwg_default = Some(y.to_string());
    }
}

fn save(app: &CadApp, dl: &OptionsDlg) {
    let v = json!({
        "settings": serde_json::to_value(&app.session.settings).unwrap_or(Value::Null),
        "autosaveMin": dl.autosave_min,
        "dwg": dl.dwg,
        "ui": {
            "toolbar": app.ui.show_toolbar,
            "statusBar": app.ui.show_status_bar,
            "fileTabs": app.ui.show_file_tabs,
            "viewcube": app.ui.show_viewcube,
            "ucsIcon": app.ui.show_ucs_icon,
            "historyLines": app.ui.history_lines,
        },
    });
    if let Some(set) = app.services.prefs_set.as_ref() {
        set(OPTIONS_PREF, &v.to_string());
    }
}

fn apply(app: &mut CadApp, dl: &mut OptionsDlg) {
    dl.s.polarang = dl.polar_deg.clamp(0.1, 180.0).to_radians();
    dl.s.pickbox = dl.s.pickbox.clamp(0.0, 50.0);
    dl.s.aperture = dl.s.aperture.clamp(1.0, 50.0);
    dl.s.gripsize = dl.s.gripsize.clamp(1.0, 255.0);
    dl.s.cursorsize = dl.s.cursorsize.clamp(1.0, 100.0);
    app.session.settings = dl.s.clone();
    if let Some(a) = app.services.autosave.as_mut() {
        a.interval_s = (dl.autosave_min.clamp(0.5, 600.0)) * 60.0;
    }
    app.ui.show_toolbar = dl.ui.show_toolbar;
    app.ui.show_status_bar = dl.ui.show_status_bar;
    app.ui.show_file_tabs = dl.ui.show_file_tabs;
    app.ui.show_viewcube = dl.ui.show_viewcube;
    app.ui.show_ucs_icon = dl.ui.show_ucs_icon;
    app.ui.history_lines = dl.ui.history_lines.min(12);
    // The DWG version for saving: every open drawing, and drawings opened later.
    let cur = app.session.active;
    for i in 0..app.session.docs.len() {
        app.session.active = i;
        let _ = app.session.execute("dwgversion", &json!({ "version": dl.dwg }));
    }
    app.session.active = cur.min(app.session.docs.len().saturating_sub(1));
    app.vd.dwg_default = Some(dl.dwg.clone());
    app.session.touch();
    save(app, dl);
}

fn slider(ui: &mut egui::Ui, label: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>, hint: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(v, range));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).small().weak());
        }
    });
    ui.end_row();
}

pub fn options_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut dl) = app.vd.options.take() else { return };
    let mut open = true;
    let mut action: Option<&str> = None;
    const TABS: [&str; 5] = ["Hiển thị", "Mở & Lưu", "Vẽ (Drafting)", "Chọn (Selection)", "Người dùng"];
    egui::Window::new("OPTIONS — Tùy chọn").open(&mut open).resizable(false).collapsible(false).default_width(560.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (i, t) in TABS.iter().enumerate() {
                if ui.selectable_label(dl.tab == i, *t).clicked() {
                    dl.tab = i;
                }
            }
        });
        ui.separator();
        egui::Grid::new("opt_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| match dl.tab {
            0 => {
                slider(ui, "Cỡ con trỏ chữ thập (CURSORSIZE, % màn hình)", &mut dl.s.cursorsize, 1.0..=100.0, "");
                ui.label("Hiện độ dày nét (LWDISPLAY)");
                ui.checkbox(&mut dl.s.lwdisplay, "");
                ui.end_row();
                ui.label("Thanh công cụ");
                ui.checkbox(&mut dl.ui.show_toolbar, "");
                ui.end_row();
                ui.label("Thanh trạng thái");
                ui.checkbox(&mut dl.ui.show_status_bar, "");
                ui.end_row();
                ui.label("Thẻ bản vẽ (File tabs)");
                ui.checkbox(&mut dl.ui.show_file_tabs, "");
                ui.end_row();
                ui.label("ViewCube");
                ui.checkbox(&mut dl.ui.show_viewcube, "");
                ui.end_row();
                ui.label("Biểu tượng UCS");
                ui.checkbox(&mut dl.ui.show_ucs_icon, "");
                ui.end_row();
                ui.label("Số dòng lịch sử dòng lệnh");
                let mut n = dl.ui.history_lines as f64;
                if ui.add(egui::Slider::new(&mut n, 0.0..=12.0).step_by(1.0)).changed() {
                    dl.ui.history_lines = n as usize;
                }
                ui.end_row();
            }
            1 => {
                slider(ui, "Tự lưu sau mỗi (phút)", &mut dl.autosave_min, 0.5..=60.0, "bản dự phòng khi có thay đổi");
                ui.label("Lưu DWG theo phiên bản");
                egui::ComboBox::from_id_salt("opt_dwg").selected_text(format!("AutoCAD {}", dl.dwg)).show_ui(ui, |ui| {
                    for y in ["2000", "2004", "2007", "2010", "2013", "2018"] {
                        ui.selectable_value(&mut dl.dwg, y.to_string(), format!("AutoCAD {y}"));
                    }
                });
                ui.end_row();
                ui.label("");
                ui.label(RichText::new("2007 mở được bằng AutoCAD 2007 trở lên; chọn 2004/2000 khi gửi cho máy cũ.").small().weak());
                ui.end_row();
            }
            2 => {
                slider(ui, "Ô bắt điểm (APERTURE, px)", &mut dl.s.aperture, 1.0..=50.0, "");
                ui.label("Bám theo điểm bắt (OTRACK, F11)");
                ui.checkbox(&mut dl.s.otrack, "");
                ui.end_row();
                ui.label("Nhập động (DYNMODE, F12)");
                ui.checkbox(&mut dl.s.dynmode, "");
                ui.end_row();
                ui.label("Bám góc cực (POLAR, F10)");
                ui.checkbox(&mut dl.s.polarmode, "");
                ui.end_row();
                slider(ui, "Bước góc cực (°)", &mut dl.polar_deg, 1.0..=90.0, "");
                ui.label("Các điểm bắt (OSNAP)");
                if ui.button("Mở cài đặt bắt điểm (DSETTINGS)…").clicked() {
                    action = Some("dsettings");
                }
                ui.end_row();
            }
            3 => {
                slider(ui, "Ô chọn (PICKBOX, px)", &mut dl.s.pickbox, 0.0..=20.0, "lớn hơn = dễ chọn hơn");
                slider(ui, "Cỡ grip (GRIPSIZE, px)", &mut dl.s.gripsize, 1.0..=20.0, "");
                ui.label("Chọn trước rồi gõ lệnh (PICKFIRST)");
                ui.checkbox(&mut dl.s.pickfirst, "");
                ui.end_row();
                ui.label("Click thêm vào lựa chọn (PICKADD)");
                ui.checkbox(&mut dl.s.pickadd, "");
                ui.end_row();
                ui.label("Bảng thuộc tính nhanh (QPMODE)");
                ui.checkbox(&mut dl.s.qpmode, "");
                ui.end_row();
            }
            _ => {
                ui.label("Phím tắt lệnh");
                if ui.button("Sửa phím tắt (ALIASEDIT)…").clicked() {
                    action = Some("aliasedit");
                }
                ui.end_row();
                ui.label("Đơn vị bản vẽ");
                if ui.button("Đơn vị (UNITS)…").clicked() {
                    action = Some("units");
                }
                ui.end_row();
                ui.label("Chuột phải khi đang chạy lệnh");
                ui.label("= Enter (như AutoCAD mặc định); Shift + chuột phải: menu bắt điểm");
                ui.end_row();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                action = Some("ok");
            }
            if ui.button("Áp dụng").clicked() {
                action = Some("apply");
            }
            if ui.button("Mặc định").clicked() {
                action = Some("defaults");
            }
            if ui.button("Hủy").clicked() {
                action = Some("cancel");
            }
        });
    });
    match action {
        Some("ok") => {
            apply(app, &mut dl);
            app.session.echo("Đã lưu tùy chọn (OPTIONS).");
            open = false;
        }
        Some("apply") => {
            apply(app, &mut dl);
            app.session.echo("Đã áp dụng tùy chọn.");
        }
        Some("defaults") => {
            let tab = dl.tab;
            dl.s = cadcraft_engine::Settings::default();
            dl.polar_deg = dl.s.polarang.to_degrees();
            dl.autosave_min = 2.0;
            dl.dwg = "2007".into();
            dl.ui = crate::UiState { history_lines: 3, ..Default::default() };
            dl.tab = tab;
        }
        Some("cancel") => open = false,
        Some(cmd @ ("dsettings" | "aliasedit" | "units")) => {
            apply(app, &mut dl);
            open = false;
            app.start(cmd);
        }
        _ => {}
    }
    if open {
        app.vd.options = Some(dl);
    }
}
