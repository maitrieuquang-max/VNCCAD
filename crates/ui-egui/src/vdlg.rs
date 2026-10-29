//! VNCCad: dialogs added after the first engineer tests — editing an object (double-click:
//! text, multiline text with background mask, multileader, attributes, table cells, dimension
//! text, hatch, viewport), Drawing Units, Text Style, Dimension Style, Plot / Page Setup, Page
//! Setup Manager, External References and command aliases (ALIASEDIT / REINIT).

use cadcraft_doc::{Entity, EntityKind, Handle};
use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;

const ALIAS_PREF: &str = "aliases";

#[derive(Default)]
pub struct VState {
    pub edit: Option<EditDlg>,
    pub units: Option<UnitsDlg>,
    pub tstyle: Option<TStyleDlg>,
    pub dstyle: Option<DStyleDlg>,
    pub plot: Option<PlotDlg>,
    pub psm: bool,
    pub alias: Option<AliasDlg>,
    pub xref: bool,
    /// VNCCad: Multileader Style manager.
    pub mls: Option<MlsDlg>,
    aliases_loaded: bool,
}

/// Open a dialog for a command typed or picked without parameters. `None` when `id` has none.
pub fn route(app: &mut CadApp, id: &str) -> Option<Result<Value, String>> {
    let raw = id.trim().trim_start_matches(['_', '.']).to_ascii_lowercase();
    if raw.starts_with('-') {
        return None;
    }
    let raw = app.session.user_aliases.get(&raw).cloned().unwrap_or(raw);
    let rid = cadcraft_engine::cmd::resolve_alias(&raw);
    let has_doc = app.session.doc().is_ok();
    match rid.as_str() {
        "units" if has_doc => app.vd.units = Some(UnitsDlg::load(app)),
        "style" if has_doc => app.vd.tstyle = Some(TStyleDlg::load(app, None)),
        "dimstyle" | "dimstyle.dimension" if has_doc => app.vd.dstyle = Some(DStyleDlg::load(app, None)),
        "plot" if has_doc => app.vd.plot = Some(PlotDlg::load(app, None, false)),
        "pagesetup" if has_doc => app.vd.psm = true,
        "aliasedit" => app.vd.alias = Some(AliasDlg::load(app)),
        "reinit" => {
            let n = load_aliases(app);
            app.session.echo(format!("REINIT: đã nạp lại {n} phím tắt riêng."));
        }
        "xref" if has_doc => app.vd.xref = true,
        "mleaderstyle" if has_doc => app.vd.mls = Some(MlsDlg::load(app, None)),
        _ => return None,
    }
    Some(Ok(Value::Null))
}

/// The user's aliases saved in the preferences (ALIASEDIT), into the session.
pub fn load_aliases(app: &mut CadApp) -> usize {
    let saved = app.services.prefs_get.as_ref().and_then(|g| g(ALIAS_PREF)).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let map = saved.unwrap_or(json!({}));
    match app.session.execute("reinit", &json!({ "aliases": map })) {
        Ok(_) => app.session.user_aliases.len(),
        Err(e) => {
            app.session.echo(e.to_string());
            0
        }
    }
}

fn save_aliases(app: &CadApp) {
    let map: serde_json::Map<String, Value> = app.session.user_aliases.iter().map(|(a, c)| (a.clone(), json!(c))).collect();
    if let Some(set) = app.services.prefs_set.as_ref() {
        set(ALIAS_PREF, &Value::Object(map).to_string());
    }
}

pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    if !app.vd.aliases_loaded {
        app.vd.aliases_loaded = true;
        load_aliases(app);
    }
    if let Some(h) = ctx.data_mut(|m| m.remove_temp::<Handle>(egui::Id::new("vnc_open_edit"))) {
        open_edit(app, h);
    }
    edit_dialog(app, ctx);
    units_dialog(app, ctx);
    tstyle_dialog(app, ctx);
    dstyle_dialog(app, ctx);
    plot_dialog(app, ctx);
    psm_dialog(app, ctx);
    alias_dialog(app, ctx);
    xref_dialog(app, ctx);
    mls_dialog(app, ctx);
}

fn run_echo(app: &mut CadApp, id: &str, p: Value) -> bool {
    match app.run(id, p) {
        Ok(v) => {
            if let Some(m) = v.get("message").and_then(Value::as_str) {
                app.session.echo(m.to_string());
            }
            true
        }
        Err(e) => {
            app.session.echo(e);
            false
        }
    }
}

fn num_edit(ui: &mut egui::Ui, v: &mut f64, speed: f64) -> egui::Response {
    ui.add(egui::DragValue::new(v).speed(speed).max_decimals(6))
}

fn text_styles(app: &CadApp) -> Vec<String> {
    app.session.doc().map(|d| d.text_styles.iter().map(|s| s.name.clone()).collect()).unwrap_or_default()
}

fn combo(ui: &mut egui::Ui, id: &str, cur: &mut String, items: &[String]) {
    egui::ComboBox::from_id_salt(id).selected_text(cur.clone()).width(160.0).show_ui(ui, |ui| {
        for it in items {
            ui.selectable_value(cur, it.clone(), it);
        }
    });
}

// =====================================================================================
// Edit an object (double-click)

pub struct EditDlg {
    pub handle: Handle,
    ent: Entity,
    /// Multiline text with real line breaks (MTEXT / MLEADER contents).
    buf: String,
    /// The viewport scale typed as "1:100".
    scale_text: String,
    pos: Option<egui::Pos2>,
}

/// Open the editor for an object (double-click, or the Properties palette button).
pub fn open_edit(app: &mut CadApp, h: Handle) {
    let Ok(d) = app.session.doc() else { return };
    let Some(e) = d.entity(h).map(|e| (**e).clone()) else { return };
    let buf = match &e.kind {
        EntityKind::MText(t) => t.contents.replace("\\P", "\n"),
        EntityKind::MLeader(m) => m.text.as_ref().map(|t| t.contents.replace("\\P", "\n")).unwrap_or_default(),
        _ => String::new(),
    };
    let scale_text = match &e.kind {
        EntityKind::Viewport(v) if v.view_height > 0.0 => format!("1:{}", fmt_num(v.view_height / v.height.max(1e-12))),
        _ => String::new(),
    };
    let pos = app.canvas.rect.zip(app.canvas.xf).map(|(r, _)| r.center() - vec2(200.0, 150.0));
    app.vd.edit = Some(EditDlg { handle: h, ent: e, buf, scale_text, pos });
}

fn fmt_num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if (r - r.round()).abs() < 1e-9 { format!("{}", r.round() as i64) } else { format!("{r}") }
}

fn kind_title(k: &EntityKind) -> &'static str {
    match k {
        EntityKind::Text(_) => "Sửa chữ (TEXT)",
        EntityKind::MText(_) => "Sửa chữ nhiều dòng (MTEXT)",
        EntityKind::MLeader(_) => "Sửa ghi chú dẫn (MLEADER)",
        EntityKind::Insert(_) => "Sửa thuộc tính block (ATTRIBUTES)",
        EntityKind::AttDef(_) => "Sửa định nghĩa thuộc tính (ATTDEF)",
        EntityKind::Table(_) => "Nhập nội dung bảng (TABLE)",
        EntityKind::Dimension(_) => "Sửa kích thước (DIMENSION)",
        EntityKind::Hatch(_) => "Sửa mặt cắt (HATCH)",
        EntityKind::Viewport(_) => "Thuộc tính khung nhìn (VIEWPORT)",
        _ => "Thuộc tính đối tượng",
    }
}

fn text_fields(ui: &mut egui::Ui, id: &str, t: &mut cadcraft_doc::Text, styles: &[String]) {
    egui::Grid::new(id).num_columns(2).show(ui, |ui| {
        ui.label("Kiểu chữ");
        combo(ui, &format!("{id}_st"), &mut t.style, styles);
        ui.end_row();
        ui.label("Chiều cao");
        num_edit(ui, &mut t.height, 0.1);
        ui.end_row();
        ui.label("Góc xoay (°)");
        let mut deg = t.rotation.to_degrees();
        if num_edit(ui, &mut deg, 1.0).changed() {
            t.rotation = deg.to_radians();
        }
        ui.end_row();
        ui.label("Hệ số rộng");
        num_edit(ui, &mut t.width_factor, 0.01);
        ui.end_row();
        ui.label("Góc nghiêng (°)");
        let mut ob = t.oblique.to_degrees();
        if num_edit(ui, &mut ob, 1.0).changed() {
            t.oblique = ob.clamp(-85.0, 85.0).to_radians();
        }
        ui.end_row();
    });
}

fn edit_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.vd.edit.take() else { return };
    // The object may have gone (undo, erase).
    if app.session.doc().ok().and_then(|d| d.entity(dlg.handle)).is_none() {
        return;
    }
    let styles = text_styles(app);
    let layers: Vec<String> = app.session.doc().map(|d| d.layers.iter().map(|l| l.name.clone()).collect()).unwrap_or_default();
    let patterns: Vec<String> = cadcraft_doc::library::standard_patterns().iter().map(|p| p.name.to_string()).collect();
    let mut open = true;
    let mut apply = false;
    let mut close = false;
    let wide = matches!(dlg.ent.kind, EntityKind::Table(_));
    let mut w = egui::Window::new(kind_title(&dlg.ent.kind))
        .open(&mut open)
        .resizable(true)
        .default_width(if wide { 640.0 } else { 420.0 })
        .collapsible(false);
    if let Some(p) = dlg.pos.take() {
        w = w.default_pos(p);
    }
    w.show(ctx, |ui| {
        let e = &mut dlg.ent;
        match &mut e.kind {
            EntityKind::Text(t) => {
                ui.label("Nội dung");
                let r = ui.add(egui::TextEdit::singleline(&mut t.value).desired_width(f32::INFINITY));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                    close = true;
                }
                if !r.has_focus() && ui.memory(|m| m.focused().is_none()) {
                    r.request_focus();
                }
                text_fields(ui, "ed_text", t, &styles);
            }
            EntityKind::AttDef(a) => {
                egui::Grid::new("ed_attdef").num_columns(2).show(ui, |ui| {
                    ui.label("Tag");
                    ui.text_edit_singleline(&mut a.tag);
                    ui.end_row();
                    ui.label("Lời nhắc");
                    ui.text_edit_singleline(&mut a.prompt);
                    ui.end_row();
                    ui.label("Mặc định");
                    ui.text_edit_singleline(&mut a.text.value);
                    ui.end_row();
                    ui.label("Ẩn");
                    ui.checkbox(&mut a.invisible, "");
                    ui.end_row();
                });
                text_fields(ui, "ed_attdef_t", &mut a.text, &styles);
            }
            EntityKind::MText(t) => {
                ui.label("Nội dung (Enter xuống dòng)");
                let r = ui.add(egui::TextEdit::multiline(&mut dlg.buf).desired_rows(5).desired_width(f32::INFINITY));
                if !r.has_focus() && ui.memory(|m| m.focused().is_none()) {
                    r.request_focus();
                }
                egui::Grid::new("ed_mtext").num_columns(2).show(ui, |ui| {
                    ui.label("Kiểu chữ");
                    combo(ui, "ed_mtext_st", &mut t.style, &styles);
                    ui.end_row();
                    ui.label("Chiều cao");
                    num_edit(ui, &mut t.height, 0.1);
                    ui.end_row();
                    ui.label("Bề rộng khung (0 = không ngắt dòng)");
                    num_edit(ui, &mut t.width, 1.0);
                    ui.end_row();
                    ui.label("Góc xoay (°)");
                    let mut deg = t.rotation.to_degrees();
                    if num_edit(ui, &mut deg, 1.0).changed() {
                        t.rotation = deg.to_radians();
                    }
                    ui.end_row();
                    ui.label("Giãn dòng");
                    num_edit(ui, &mut t.line_spacing, 0.05);
                    ui.end_row();
                    ui.label("Nền che (Background mask)");
                    let mut on = t.mask.is_some();
                    if ui.checkbox(&mut on, "").changed() {
                        t.mask = on.then_some(cadcraft_doc::TextMask { scale: 1.5, color: None });
                    }
                    ui.end_row();
                    if let Some(m) = &mut t.mask {
                        ui.label("   Hệ số lề");
                        ui.add(egui::DragValue::new(&mut m.scale).speed(0.05).range(1.0..=5.0));
                        ui.end_row();
                        ui.label("   Màu nền");
                        let mut use_bg = m.color.is_none();
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut use_bg, "Màu nền bản vẽ").changed() {
                                m.color = if use_bg { None } else { Some(cadcraft_doc::color::Color::Index(1)) };
                            }
                            if let Some(cadcraft_doc::color::Color::Index(i)) = &mut m.color {
                                ui.add(egui::DragValue::new(i).range(1..=255).prefix("ACI "));
                            }
                        });
                        ui.end_row();
                    }
                });
            }
            EntityKind::MLeader(m) => {
                ui.label("Nội dung ghi chú (Enter xuống dòng)");
                let r = ui.add(egui::TextEdit::multiline(&mut dlg.buf).desired_rows(4).desired_width(f32::INFINITY));
                if !r.has_focus() && ui.memory(|mm| mm.focused().is_none()) {
                    r.request_focus();
                }
                egui::Grid::new("ed_ml").num_columns(2).show(ui, |ui| {
                    if let Some(t) = &mut m.text {
                        ui.label("Chiều cao chữ");
                        num_edit(ui, &mut t.height, 0.1);
                        ui.end_row();
                        ui.label("Kiểu chữ");
                        combo(ui, "ed_ml_st", &mut t.style, &styles);
                        ui.end_row();
                    }
                    ui.label("Cỡ mũi tên");
                    num_edit(ui, &mut m.arrow_size, 0.1);
                    ui.end_row();
                });
            }
            EntityKind::Insert(ins) => {
                ui.label(RichText::new(format!("Block: {}", ins.block)).strong());
                if ins.attribs.is_empty() {
                    ui.label("Block này không có thuộc tính.");
                }
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (i, a) in ins.attribs.iter_mut().enumerate() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&a.tag).strong());
                            ui.add(egui::TextEdit::singleline(&mut a.text.value).desired_width(220.0));
                        });
                        egui::CollapsingHeader::new("Định dạng chữ").id_salt(("att_fmt", i)).show(ui, |ui| {
                            text_fields(ui, &format!("ed_att_{i}"), &mut a.text, &styles);
                            ui.checkbox(&mut a.invisible, "Ẩn");
                        });
                    }
                });
            }
            EntityKind::Table(t) => {
                ui.horizontal(|ui| {
                    ui.label("Chiều cao chữ");
                    num_edit(ui, &mut t.text_height, 0.1);
                    if ui.button("+ Hàng").clicked() {
                        let n = t.cells.first().map(Vec::len).unwrap_or(t.col_widths.len());
                        t.cells.push(vec![cadcraft_doc::TableCell { text: String::new(), merged: None }; n]);
                        let h = t.row_heights.last().copied().unwrap_or(t.text_height * 2.0);
                        t.row_heights.push(h);
                    }
                    if ui.button("+ Cột").clicked() {
                        for r in &mut t.cells {
                            r.push(cadcraft_doc::TableCell { text: String::new(), merged: None });
                        }
                        let w = t.col_widths.last().copied().unwrap_or(t.text_height * 10.0);
                        t.col_widths.push(w);
                    }
                });
                egui::ScrollArea::both().max_height(380.0).show(ui, |ui| {
                    egui::Grid::new("ed_table").striped(true).show(ui, |ui| {
                        for (ri, row) in t.cells.iter_mut().enumerate() {
                            ui.label(RichText::new(format!("{}", ri + 1)).weak());
                            for cell in row.iter_mut() {
                                ui.add_sized([120.0, 22.0], egui::TextEdit::singleline(&mut cell.text));
                            }
                            ui.end_row();
                        }
                    });
                });
            }
            EntityKind::Dimension(dm) => {
                egui::Grid::new("ed_dim").num_columns(2).show(ui, |ui| {
                    ui.label("Giá trị đo");
                    ui.label(fmt_num(dm.measurement));
                    ui.end_row();
                    ui.label("Chữ ghi đè (trống = số đo; <> = chèn số đo)");
                    ui.text_edit_singleline(&mut dm.text);
                    ui.end_row();
                    let dstyles: Vec<String> = app.session.doc().map(|d| d.dim_styles.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
                    ui.label("Kiểu kích thước");
                    combo(ui, "ed_dim_st", &mut dm.style, &dstyles);
                    ui.end_row();
                    ui.label("Chiều cao chữ riêng (DIMTXT)");
                    let mut h = dm.overrides.get("DIMTXT").and_then(Value::as_f64).unwrap_or(0.0);
                    if num_edit(ui, &mut h, 0.1).changed() {
                        if h > 0.0 {
                            dm.overrides.insert("DIMTXT".into(), json!(h));
                        } else {
                            dm.overrides.remove("DIMTXT");
                        }
                    }
                    ui.end_row();
                });
            }
            EntityKind::Hatch(h) => {
                egui::Grid::new("ed_hatch").num_columns(2).show(ui, |ui| {
                    ui.label("Mẫu");
                    let mut p = h.pattern.clone();
                    combo(ui, "ed_hatch_p", &mut p, &patterns);
                    if p != h.pattern {
                        h.solid = p == "SOLID";
                        h.pattern = p;
                    }
                    ui.end_row();
                    ui.label("Tỉ lệ");
                    num_edit(ui, &mut h.scale, 0.1);
                    ui.end_row();
                    ui.label("Góc (°)");
                    let mut a = h.angle.to_degrees();
                    if num_edit(ui, &mut a, 1.0).changed() {
                        h.angle = a.to_radians();
                    }
                    ui.end_row();
                    ui.label("Liên kết đường bao");
                    if ui.checkbox(&mut h.associative, "").changed() && !h.associative {
                        h.boundary.clear();
                        h.seeds.clear();
                    }
                    ui.end_row();
                });
                if h.boundary.is_empty() {
                    ui.label(RichText::new("Chưa có đường bao liên kết: dùng HATCHGENERATEBOUNDARY (HGB) để tạo.").small());
                }
            }
            EntityKind::Viewport(v) => {
                egui::Grid::new("ed_vp").num_columns(2).show(ui, |ui| {
                    ui.label("Tỉ lệ (giấy : mô hình)");
                    ui.horizontal(|ui| {
                        let mut pick = None;
                        egui::ComboBox::from_id_salt("ed_vp_scale").selected_text(dlg.scale_text.clone()).show_ui(ui, |ui| {
                            for n in [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0] {
                                if ui.selectable_label(false, format!("1:{n}")).clicked() {
                                    pick = Some(format!("1:{n}"));
                                }
                            }
                        });
                        if let Some(p) = pick {
                            dlg.scale_text = p;
                        }
                        ui.add(egui::TextEdit::singleline(&mut dlg.scale_text).desired_width(80.0));
                    });
                    ui.end_row();
                    ui.label("Khóa tỉ lệ (Display locked)");
                    ui.checkbox(&mut v.locked, "");
                    ui.end_row();
                    ui.label("Tâm nhìn X");
                    num_edit(ui, &mut v.view_center.x, 1.0);
                    ui.end_row();
                    ui.label("Tâm nhìn Y");
                    num_edit(ui, &mut v.view_center.y, 1.0);
                    ui.end_row();
                    ui.label("Rộng × cao khung (giấy)");
                    ui.horizontal(|ui| {
                        num_edit(ui, &mut v.width, 1.0);
                        num_edit(ui, &mut v.height, 1.0);
                    });
                    ui.end_row();
                });
                ui.label(RichText::new("Kích đúp vào trong khung để vào MSPACE rồi lăn chuột / giữ nút giữa để pan, zoom nội dung.").small());
            }
            _ => {
                ui.label("Đối tượng này sửa trong bảng Properties (Ctrl+1).");
            }
        }
        ui.separator();
        egui::Grid::new("ed_common").num_columns(2).show(ui, |ui| {
            ui.label("Layer");
            combo(ui, "ed_layer", &mut e.common.layer, &layers);
            ui.end_row();
            ui.label("Màu");
            let mut c = e.common.color.name();
            let r = ui.add(egui::TextEdit::singleline(&mut c).desired_width(120.0));
            if r.changed()
                && let Some(col) = cadcraft_doc::color::Color::parse(&c)
            {
                e.common.color = col;
            }
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                apply = true;
                close = true;
            }
            if ui.button("Áp dụng").clicked() {
                apply = true;
            }
            if ui.button("Hủy").clicked() {
                close = true;
            }
        });
    });
    if apply {
        let e = &mut dlg.ent;
        match &mut e.kind {
            EntityKind::MText(t) => t.contents = dlg.buf.replace("\r\n", "\n").replace('\n', "\\P"),
            EntityKind::MLeader(m) => {
                let txt = dlg.buf.replace("\r\n", "\n").replace('\n', "\\P");
                if let Some(t) = &mut m.text {
                    t.contents = txt;
                }
            }
            EntityKind::Dimension(dm) => dm.block = None,
            EntityKind::Viewport(v) => {
                let sc = dlg.scale_text.trim();
                let k = match sc.split_once([':', '/']) {
                    Some((a, b)) => a.trim().parse::<f64>().ok().zip(b.trim().parse::<f64>().ok()).map(|(a, b)| b / a),
                    None => sc.parse::<f64>().ok().map(|x| 1.0 / x),
                };
                if let Some(k) = k.filter(|k| k.is_finite() && *k > 0.0) {
                    v.view_height = v.height * k;
                }
            }
            _ => {}
        }
        match serde_json::to_value(&dlg.ent) {
            Ok(v) => {
                if run_echo(app, "entity.replace", json!({ "entity": v }))
                    && let Some(e) = app.session.doc().ok().and_then(|d| d.entity(dlg.handle)).map(|e| (**e).clone())
                {
                    dlg.ent = e;
                }
            }
            Err(e) => app.session.echo(e.to_string()),
        }
    }
    if open && !close {
        app.vd.edit = Some(dlg);
    }
}

// =====================================================================================
// UNITS

pub struct UnitsDlg {
    lunits: i64,
    luprec: i64,
    aunits: i64,
    auprec: i64,
    insunits: i64,
    angdir: i64,
    angbase: f64,
}

impl UnitsDlg {
    fn load(app: &CadApp) -> Self {
        let h = app.session.doc().map(|d| d.header.clone()).unwrap_or_default();
        UnitsDlg {
            lunits: h.i64("LUNITS", 2),
            luprec: h.i64("LUPREC", 4),
            aunits: h.i64("AUNITS", 0),
            auprec: h.i64("AUPREC", 0),
            insunits: h.i64("INSUNITS", 4),
            angdir: h.i64("ANGDIR", 0),
            angbase: h.f64("ANGBASE", 0.0).to_degrees(),
        }
    }
}

const LUNITS: [(i64, &str); 5] = [(1, "Scientific"), (2, "Decimal (thập phân)"), (3, "Engineering"), (4, "Architectural"), (5, "Fractional")];
const AUNITS: [(i64, &str); 5] =
    [(0, "Decimal Degrees (độ thập phân)"), (1, "Deg/Min/Sec (độ phút giây)"), (2, "Grads"), (3, "Radians"), (4, "Surveyor's Units")];
const INSUNITS: [(i64, &str); 9] = [
    (0, "Unitless"),
    (1, "Inches"),
    (2, "Feet"),
    (4, "Millimeters (mm)"),
    (5, "Centimeters (cm)"),
    (6, "Meters (m)"),
    (7, "Kilometers (km)"),
    (14, "Decimeters"),
    (3, "Miles"),
];

fn pick_i(ui: &mut egui::Ui, id: &str, v: &mut i64, items: &[(i64, &str)]) {
    let cur = items.iter().find(|(k, _)| k == v).map(|(_, n)| *n).unwrap_or("?");
    egui::ComboBox::from_id_salt(id).selected_text(cur).width(220.0).show_ui(ui, |ui| {
        for (k, n) in items {
            ui.selectable_value(v, *k, *n);
        }
    });
}

fn units_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut u) = app.vd.units.take() else { return };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    egui::Window::new("Drawing Units — Đơn vị bản vẽ").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.columns(2, |c| {
            c[0].heading("Chiều dài (Length)");
            c[0].label("Kiểu");
            pick_i(&mut c[0], "un_lu", &mut u.lunits, &LUNITS);
            c[0].label("Độ chính xác");
            egui::ComboBox::from_id_salt("un_lp").selected_text(cadcraft_engine::units::format_distance(0.0, u.lunits, u.luprec)).show_ui(
                &mut c[0],
                |ui| {
                    for p in 0..=8 {
                        ui.selectable_value(&mut u.luprec, p, cadcraft_engine::units::format_distance(0.0, u.lunits, p));
                    }
                },
            );
            c[1].heading("Góc (Angle)");
            c[1].label("Kiểu");
            pick_i(&mut c[1], "un_au", &mut u.aunits, &AUNITS);
            c[1].label("Độ chính xác");
            egui::ComboBox::from_id_salt("un_ap").selected_text(cadcraft_engine::units::format_angle(0.0, u.aunits, u.auprec)).show_ui(
                &mut c[1],
                |ui| {
                    for p in 0..=8 {
                        ui.selectable_value(&mut u.auprec, p, cadcraft_engine::units::format_angle(0.0, u.aunits, p));
                    }
                },
            );
            let mut cw = u.angdir == 1;
            if c[1].checkbox(&mut cw, "Chiều kim đồng hồ (Clockwise)").changed() {
                u.angdir = i64::from(cw);
            }
        });
        ui.separator();
        ui.heading("Tỉ lệ chèn (Insertion scale)");
        ui.horizontal(|ui| {
            ui.label("Đơn vị nội dung chèn vào");
            pick_i(ui, "un_ins", &mut u.insunits, &INSUNITS);
        });
        ui.horizontal(|ui| {
            ui.label("Hướng góc 0 (Direction, °)");
            num_edit(ui, &mut u.angbase, 1.0);
        });
        ui.separator();
        ui.label("Mẫu hiển thị (Sample output)");
        ui.monospace(format!(
            "{} , {} , 0     {}",
            cadcraft_engine::units::format_distance(1.5, u.lunits, u.luprec),
            cadcraft_engine::units::format_distance(2.0039, u.lunits, u.luprec),
            cadcraft_engine::units::format_angle(45f64.to_radians(), u.aunits, u.auprec)
        ));
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                ok = true;
            }
            if ui.button("Hủy").clicked() {
                cancel = true;
            }
        });
    });
    if ok {
        run_echo(
            app,
            "units",
            json!({ "lunits": u.lunits, "luprec": u.luprec, "aunits": u.aunits, "auprec": u.auprec, "insunits": u.insunits, "angdir": u.angdir, "angbase": u.angbase }),
        );
        app.session.echo("Đã cập nhật đơn vị bản vẽ (UNITS).");
        return;
    }
    if open && !cancel {
        app.vd.units = Some(u);
    }
}

// =====================================================================================
// STYLE (text styles)

pub struct TStyleDlg {
    sel: String,
    cur: cadcraft_doc::TextStyle,
    new_name: String,
    current: String,
}

const FONTS: [&str; 12] = [
    "CADCraft Stroke",
    "txt.shx",
    "simplex.shx",
    "romans.shx",
    "isocp.shx",
    "Arial",
    "Times New Roman",
    "Tahoma",
    "Calibri",
    "Verdana",
    "Courier New",
    "Segoe UI",
];

impl TStyleDlg {
    fn load(app: &CadApp, name: Option<&str>) -> Self {
        let d = app.session.doc().ok();
        let current = d.map(|d| d.header.str("TEXTSTYLE", "Standard")).unwrap_or_else(|| "Standard".into());
        let sel = name.map(str::to_string).unwrap_or_else(|| current.clone());
        let cur = d.and_then(|d| d.text_style(&sel).cloned()).unwrap_or_default();
        let taken: Vec<String> = d.map(|d| d.text_styles.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
        TStyleDlg { sel: cur.name.clone(), cur, new_name: next_free(&taken, "Style"), current }
    }
}

/// VNCCad: a free name for a new style (`Style1`, `Style2`…), so New works with one click.
fn next_free(taken: &[String], stem: &str) -> String {
    (1..10_000).map(|i| format!("{stem}{i}")).find(|n| !taken.iter().any(|t| t.eq_ignore_ascii_case(n))).unwrap_or_else(|| stem.to_string())
}

fn tstyle_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut st) = app.vd.tstyle.take() else { return };
    let names = text_styles(app);
    let mut open = true;
    let mut action: Option<&str> = None;
    let mut select: Option<String> = None;
    egui::Window::new("Text Style — Kiểu chữ").open(&mut open).resizable(false).collapsible(false).default_width(560.0).show(ctx, |ui| {
        ui.label(format!("Kiểu hiện hành: {}", st.current));
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width(180.0);
                ui.label("Các kiểu");
                egui::ScrollArea::vertical().id_salt("ts_list").max_height(220.0).show(ui, |ui| {
                    ui.set_min_width(160.0);
                    for n in &names {
                        if ui.selectable_label(*n == st.sel, n).clicked() {
                            select = Some(n.clone());
                        }
                    }
                });
                if ui.button("Đặt hiện hành").clicked() {
                    action = Some("current");
                }
                if ui.button("Xóa").clicked() {
                    action = Some("delete");
                }
                ui.separator();
                ui.label("Kiểu mới");
                ui.add(egui::TextEdit::singleline(&mut st.new_name).desired_width(170.0));
                if ui.button("Tạo mới (New)").clicked() {
                    action = Some("new");
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.heading(&st.sel);
                egui::Grid::new("ts_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Font");
                    ui.horizontal(|ui| {
                        let mut pick = None;
                        egui::ComboBox::from_id_salt("ts_font").selected_text(st.cur.font.clone()).width(150.0).show_ui(ui, |ui| {
                            for f in FONTS {
                                if ui.selectable_label(st.cur.font == f, f).clicked() {
                                    pick = Some(f.to_string());
                                }
                            }
                        });
                        if let Some(p) = pick {
                            st.cur.font = p;
                        }
                    });
                    ui.end_row();
                    ui.label("Tên font khác");
                    ui.add(egui::TextEdit::singleline(&mut st.cur.font).desired_width(170.0));
                    ui.end_row();
                    ui.label("Big font (.shx)");
                    ui.add(egui::TextEdit::singleline(&mut st.cur.big_font).desired_width(170.0));
                    ui.end_row();
                    ui.label("Chiều cao (0 = khi gõ chữ)");
                    num_edit(ui, &mut st.cur.height, 0.1);
                    ui.end_row();
                    ui.label("Hệ số rộng");
                    num_edit(ui, &mut st.cur.width_factor, 0.01);
                    ui.end_row();
                    ui.label("Góc nghiêng (°)");
                    let mut ob = st.cur.oblique.to_degrees();
                    if num_edit(ui, &mut ob, 1.0).changed() {
                        st.cur.oblique = ob.clamp(-85.0, 85.0).to_radians();
                    }
                    ui.end_row();
                    ui.label("Annotative");
                    ui.checkbox(&mut st.cur.annotative, "");
                    ui.end_row();
                    ui.label("Ngược (Backwards)");
                    ui.checkbox(&mut st.cur.backwards, "");
                    ui.end_row();
                    ui.label("Lộn ngược (Upside down)");
                    ui.checkbox(&mut st.cur.upside_down, "");
                    ui.end_row();
                });
                ui.label(RichText::new("AaBbCc 123 Tiếng Việt").size(18.0));
                if ui.button("Áp dụng (Apply)").clicked() {
                    action = Some("apply");
                }
            });
        });
    });
    let style_params = |name: &str, s: &cadcraft_doc::TextStyle, current: bool| {
        json!({
            "name": name,
            "font": s.font,
            "bigFont": s.big_font,
            "height": s.height,
            "widthFactor": s.width_factor,
            "oblique": s.oblique.to_degrees(),
            "annotative": s.annotative,
            "backwards": s.backwards,
            "upsideDown": s.upside_down,
            "current": current,
        })
    };
    match action {
        Some("apply") => {
            let p = style_params(&st.sel, &st.cur, false);
            if run_echo(app, "style", p) {
                app.session.echo(format!("Đã cập nhật kiểu chữ {}.", st.sel));
            }
        }
        Some("new") => {
            let mut n = st.new_name.trim().to_string();
            if n.is_empty() {
                n = next_free(&names, "Style");
            }
            if names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                app.session.echo(format!("Đã có kiểu chữ {n} — đặt tên khác."));
            } else if run_echo(app, "style", style_params(&n, &st.cur, true)) {
                app.session.echo(format!("Đã tạo kiểu chữ {n} (hiện hành)."));
                st = TStyleDlg::load(app, Some(&n));
            }
        }
        Some("current") if run_echo(app, "style.current", json!({ "name": st.sel })) => {
            st = TStyleDlg::load(app, Some(&st.sel.clone()));
        }
        Some("delete") if run_echo(app, "style.delete", json!({ "name": st.sel })) => {
            st = TStyleDlg::load(app, None);
        }
        _ => {}
    }
    if let Some(n) = select {
        st = TStyleDlg::load(app, Some(&n));
    }
    if open {
        app.vd.tstyle = Some(st);
    }
}

// =====================================================================================
// DIMSTYLE

pub struct DStyleDlg {
    sel: String,
    current: String,
    /// Edited values (DIM variable → text / bool).
    vals: Vec<(&'static str, DVal)>,
    tab: usize,
    new_name: String,
    copy_from: String,
}

#[derive(Clone)]
enum DVal {
    Text(String),
    Bool(bool),
}

const DTABS: [(&str, &[&str]); 7] = [
    ("Đường (Lines)", &["DIMDLE", "DIMDLI", "DIMEXE", "DIMEXO", "DIMSE1", "DIMSE2", "DIMCLRD", "DIMCLRE"]),
    ("Mũi tên (Arrows)", &["DIMBLK", "DIMBLK1", "DIMBLK2", "DIMASZ", "DIMTSZ", "DIMCEN"]),
    ("Chữ (Text)", &["DIMTXSTY", "DIMTXT", "DIMGAP", "DIMTAD", "DIMJUST", "DIMTIH", "DIMTOH", "DIMCLRT"]),
    ("Tỉ lệ (Fit)", &["DIMSCALE"]),
    ("Đơn vị (Primary Units)", &["DIMLUNIT", "DIMDEC", "DIMDSEP", "DIMRND", "DIMLFAC", "DIMPOST", "DIMZIN", "DIMAUNIT", "DIMADEC", "DIMFRAC"]),
    ("Dung sai (Tolerances)", &["DIMTOL", "DIMLIM", "DIMTP", "DIMTM", "DIMTDEC", "DIMTFAC"]),
    ("Đơn vị phụ (Alternate)", &["DIMALT", "DIMALTF", "DIMALTD", "DIMAPOST"]),
];

fn dvar_label(v: &str) -> &'static str {
    match v {
        "DIMDLE" => "Đường kích thước vượt (DIMDLE)",
        "DIMDLI" => "Khoảng cách đường cơ sở (DIMDLI)",
        "DIMEXE" => "Đường gióng vượt qua (DIMEXE)",
        "DIMEXO" => "Khoảng hở gốc đường gióng (DIMEXO)",
        "DIMSE1" => "Bỏ đường gióng 1 (DIMSE1)",
        "DIMSE2" => "Bỏ đường gióng 2 (DIMSE2)",
        "DIMCLRD" => "Màu đường kích thước (DIMCLRD)",
        "DIMCLRE" => "Màu đường gióng (DIMCLRE)",
        "DIMBLK" => "Mũi tên (DIMBLK: trống = đặc, _ARCHTICK, _OBLIQUE, _DOT…)",
        "DIMBLK1" => "Mũi tên 1 (DIMBLK1)",
        "DIMBLK2" => "Mũi tên 2 (DIMBLK2)",
        "DIMASZ" => "Cỡ mũi tên (DIMASZ)",
        "DIMTSZ" => "Cỡ gạch chéo (DIMTSZ, >0 thay mũi tên)",
        "DIMCEN" => "Dấu tâm (DIMCEN)",
        "DIMTXSTY" => "Kiểu chữ (DIMTXSTY)",
        "DIMTXT" => "Chiều cao chữ (DIMTXT)",
        "DIMGAP" => "Khoảng hở chữ (DIMGAP)",
        "DIMTAD" => "Vị trí chữ dọc (DIMTAD: 0 giữa, 1 trên)",
        "DIMJUST" => "Vị trí chữ ngang (DIMJUST)",
        "DIMTIH" => "Chữ trong nằm ngang (DIMTIH)",
        "DIMTOH" => "Chữ ngoài nằm ngang (DIMTOH)",
        "DIMCLRT" => "Màu chữ (DIMCLRT)",
        "DIMSCALE" => "Tỉ lệ toàn bộ (DIMSCALE)",
        "DIMLUNIT" => "Kiểu đơn vị (DIMLUNIT: 2 thập phân)",
        "DIMDEC" => "Số chữ số thập phân (DIMDEC)",
        "DIMDSEP" => "Dấu thập phân (DIMDSEP)",
        "DIMRND" => "Làm tròn (DIMRND)",
        "DIMLFAC" => "Hệ số đo (DIMLFAC)",
        "DIMPOST" => "Tiền tố/hậu tố (DIMPOST, ví dụ <> mm)",
        "DIMZIN" => "Bỏ số 0 (DIMZIN)",
        "DIMAUNIT" => "Đơn vị góc (DIMAUNIT)",
        "DIMADEC" => "Số lẻ góc (DIMADEC)",
        "DIMFRAC" => "Kiểu phân số (DIMFRAC)",
        "DIMTOL" => "Ghi dung sai (DIMTOL)",
        "DIMLIM" => "Ghi giới hạn (DIMLIM)",
        "DIMTP" => "Dung sai trên (DIMTP)",
        "DIMTM" => "Dung sai dưới (DIMTM)",
        "DIMTDEC" => "Số lẻ dung sai (DIMTDEC)",
        "DIMTFAC" => "Tỉ lệ chữ dung sai (DIMTFAC)",
        "DIMALT" => "Bật đơn vị phụ (DIMALT)",
        "DIMALTF" => "Hệ số đơn vị phụ (DIMALTF)",
        "DIMALTD" => "Số lẻ đơn vị phụ (DIMALTD)",
        "DIMAPOST" => "Hậu tố đơn vị phụ (DIMAPOST)",
        _ => "",
    }
}

impl DStyleDlg {
    fn load(app: &CadApp, name: Option<&str>) -> Self {
        let d = app.session.doc().ok();
        let current = d.map(|d| d.header.str("DIMSTYLE", "Standard")).unwrap_or_else(|| "Standard".into());
        let sel = name.map(str::to_string).unwrap_or_else(|| current.clone());
        let st = d.and_then(|d| d.dim_style(&sel).cloned()).unwrap_or_default();
        let v = serde_json::to_value(&st).unwrap_or(Value::Null);
        let vals = cadcraft_doc::DIMVARS
            .iter()
            .map(|(var, field)| {
                let x = v.get(*field).cloned().unwrap_or(Value::Null);
                let dv = match &x {
                    Value::Bool(b) => DVal::Bool(*b),
                    Value::Number(n) => DVal::Text(n.as_f64().map(fmt_num).unwrap_or_default()),
                    Value::String(s) => DVal::Text(s.clone()),
                    Value::Object(_) => {
                        DVal::Text(serde_json::from_value::<cadcraft_doc::color::Color>(x.clone()).map(|c| c.name()).unwrap_or_default())
                    }
                    _ => DVal::Text(String::new()),
                };
                (*var, dv)
            })
            .collect();
        let taken: Vec<String> = app.session.doc().map(|d| d.dim_styles.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
        let new_name = next_free(&taken, "Kieu KT ");
        DStyleDlg { sel: st.name.clone(), current, vals, tab: 2, new_name, copy_from: st.name }
    }
}

fn dstyle_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut st) = app.vd.dstyle.take() else { return };
    let names: Vec<String> = app.session.doc().map(|d| d.dim_styles.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
    let mut open = true;
    let mut action: Option<&str> = None;
    let mut select: Option<String> = None;
    egui::Window::new("Dimension Style Manager — Kiểu kích thước").open(&mut open).resizable(true).default_width(640.0).collapsible(false).show(
        ctx,
        |ui| {
            ui.label(format!("Kiểu hiện hành: {}", st.current));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(190.0);
                    egui::ScrollArea::vertical().id_salt("ds_list").max_height(200.0).show(ui, |ui| {
                        for n in &names {
                            if ui.selectable_label(*n == st.sel, n).clicked() {
                                select = Some(n.clone());
                            }
                        }
                    });
                    if ui.button("Đặt hiện hành").clicked() {
                        action = Some("current");
                    }
                    if ui.button("Xóa").clicked() {
                        action = Some("delete");
                    }
                    ui.separator();
                    ui.label("Tên kiểu mới");
                    ui.add(egui::TextEdit::singleline(&mut st.new_name).desired_width(170.0));
                    ui.label("Bắt đầu từ");
                    combo(ui, "ds_from", &mut st.copy_from, &names);
                    if ui.button("Tạo mới (New)").clicked() {
                        action = Some("new");
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.heading(format!("Sửa kiểu: {}", st.sel));
                    ui.horizontal_wrapped(|ui| {
                        for (i, (t, _)) in DTABS.iter().enumerate() {
                            if ui.selectable_label(st.tab == i, *t).clicked() {
                                st.tab = i;
                            }
                        }
                    });
                    ui.separator();
                    let tab_vars = DTABS.get(st.tab).map(|t| t.1).unwrap_or(&[]);
                    egui::Grid::new("ds_vars").num_columns(2).striped(true).show(ui, |ui| {
                        for var in tab_vars {
                            let Some((_, val)) = st.vals.iter_mut().find(|(v, _)| v == var) else { continue };
                            ui.label(dvar_label(var));
                            match val {
                                DVal::Bool(b) => {
                                    ui.checkbox(b, "");
                                }
                                DVal::Text(t) => {
                                    ui.add(egui::TextEdit::singleline(t).desired_width(140.0));
                                }
                            }
                            ui.end_row();
                        }
                    });
                    if ui.button("Áp dụng (Apply)").clicked() {
                        action = Some("apply");
                    }
                });
            });
        },
    );
    let fields = |s: &DStyleDlg| -> serde_json::Map<String, Value> {
        s.vals
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    match v {
                        DVal::Bool(b) => json!(*b),
                        DVal::Text(t) => json!(t),
                    },
                )
            })
            .collect()
    };
    match action {
        Some("apply") => {
            let mut p = fields(&st);
            p.insert("name".into(), json!(st.sel));
            p.insert("current".into(), json!(false));
            match app.run("dimstyle", Value::Object(p)) {
                Ok(v) => {
                    let ign = v.get("ignored").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
                    app.session.echo(if ign > 0 {
                        format!("Đã cập nhật kiểu {} ({ign} giá trị không hợp lệ bị bỏ qua).", st.sel)
                    } else {
                        format!("Đã cập nhật kiểu kích thước {}.", st.sel)
                    });
                    st = DStyleDlg::load(app, Some(&st.sel.clone()));
                }
                Err(e) => app.session.echo(e),
            }
        }
        Some("new") => {
            let mut n = st.new_name.trim().to_string();
            if n.is_empty() {
                n = next_free(&names, "Kieu KT ");
            }
            if names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                app.session.echo(format!("Đã có kiểu {n}."));
            } else if run_echo(app, "dimstyle", json!({ "name": n, "copyFrom": st.copy_from, "current": true })) {
                app.session.echo(format!("Đã tạo kiểu kích thước {n} (hiện hành) — sửa các thông số bên phải rồi bấm Áp dụng."));
                st = DStyleDlg::load(app, Some(&n));
            }
        }
        Some("current") if run_echo(app, "dimstyle.current", json!({ "name": st.sel })) => {
            st = DStyleDlg::load(app, Some(&st.sel.clone()));
        }
        Some("delete") if run_echo(app, "dimstyle.delete", json!({ "name": st.sel })) => {
            st = DStyleDlg::load(app, None);
        }
        _ => {}
    }
    if let Some(n) = select {
        st = DStyleDlg::load(app, Some(&n));
    }
    if open {
        app.vd.dstyle = Some(st);
    }
}

// =====================================================================================
// PLOT / PAGE SETUP

pub struct PlotDlg {
    /// The layout ("Model" for model space).
    layout: String,
    page: cadcraft_doc::PageSetup,
    scale_text: String,
    /// Page Setup only (no plotting buttons).
    setup_only: bool,
}

impl PlotDlg {
    fn load(app: &CadApp, layout: Option<&str>, setup_only: bool) -> Self {
        let layout = layout.map(str::to_string).unwrap_or_else(|| match app.session.space() {
            cadcraft_doc::Space::Model => "Model".into(),
            cadcraft_doc::Space::Paper(n) => n,
        });
        let page = app
            .session
            .doc()
            .map(|d| if layout == "Model" { cadcraft_io::pdf::model_page(d) } else { d.layout(&layout).map(|l| l.page.clone()).unwrap_or_default() })
            .unwrap_or_default();
        let scale_text = if page.scale > 0.0 { format!("1:{}", fmt_num(1.0 / page.scale)) } else { "1:1".into() };
        PlotDlg { layout, page, scale_text, setup_only }
    }
    fn params(&self) -> Value {
        let mut p = json!({
            "layout": self.layout,
            "paper": self.page.paper,
            "landscape": self.page.landscape,
            "plotArea": self.page.plot_area,
            "scaleToFit": self.page.scale_to_fit,
            "center": self.page.center,
            "lineweights": self.page.lineweights,
            "plotStyleTable": self.page.plot_style_table,
            "margins": self.page.margins_mm,
        });
        if !self.page.scale_to_fit
            && let Some(o) = p.as_object_mut()
        {
            o.insert("scale".into(), json!(self.scale_text.trim()));
        }
        if let (Some(w), Some(o)) = (self.page.window, p.as_object_mut()) {
            o.insert("window".into(), json!([[w[0], w[1]], [w[2], w[3]]]));
        }
        p
    }
}

fn plot_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut pd) = app.vd.plot.take() else { return };
    let mut open = true;
    let mut action: Option<&str> = None;
    let title = if pd.setup_only { format!("Page Setup — {}", pd.layout) } else { format!("Plot — In bản vẽ ({})", pd.layout) };
    egui::Window::new(title).open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        egui::Grid::new("plot_grid").num_columns(2).show(ui, |ui| {
            ui.label("Khổ giấy");
            egui::ComboBox::from_id_salt("plot_paper").selected_text(pd.page.paper.clone()).width(260.0).show_ui(ui, |ui| {
                for p in cadcraft_render::PAPER_SIZES {
                    if ui.selectable_label(pd.page.paper == p.name, p.name).clicked() {
                        pd.page.paper = p.name.to_string();
                        pd.page.width_mm = p.width_mm;
                        pd.page.height_mm = p.height_mm;
                    }
                }
            });
            ui.end_row();
            ui.label("Hướng giấy");
            ui.horizontal(|ui| {
                ui.radio_value(&mut pd.page.landscape, true, "Ngang (Landscape)");
                ui.radio_value(&mut pd.page.landscape, false, "Đứng (Portrait)");
            });
            ui.end_row();
            ui.label("Vùng in");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("plot_area").selected_text(pd.page.plot_area.clone()).show_ui(ui, |ui| {
                    for a in ["layout", "extents", "display", "window", "limits"] {
                        ui.selectable_value(&mut pd.page.plot_area, a.to_string(), a);
                    }
                });
                if ui.button("Chọn cửa sổ <").on_hover_text("Chọn hai góc vùng in trên bản vẽ").clicked() {
                    action = Some("window");
                }
            });
            ui.end_row();
            if let Some(w) = pd.page.window {
                ui.label("   Cửa sổ");
                ui.label(format!("({:.1}, {:.1}) – ({:.1}, {:.1})", w[0], w[1], w[2], w[3]));
                ui.end_row();
            }
            ui.label("Tỉ lệ in");
            ui.horizontal(|ui| {
                ui.checkbox(&mut pd.page.scale_to_fit, "Vừa khổ giấy (Fit)");
                ui.add_enabled_ui(!pd.page.scale_to_fit, |ui| {
                    let mut pick = None;
                    egui::ComboBox::from_id_salt("plot_scale").selected_text(pd.scale_text.clone()).show_ui(ui, |ui| {
                        for n in [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0] {
                            if ui.selectable_label(false, format!("1:{n}")).clicked() {
                                pick = Some(format!("1:{n}"));
                            }
                        }
                    });
                    if let Some(p) = pick {
                        pd.scale_text = p;
                    }
                    ui.add(egui::TextEdit::singleline(&mut pd.scale_text).desired_width(70.0)).on_hover_text("mm trên giấy : đơn vị bản vẽ");
                });
            });
            ui.end_row();
            ui.label("Canh giữa");
            ui.checkbox(&mut pd.page.center, "Center the plot");
            ui.end_row();
            ui.label("In nét đậm (lineweights)");
            ui.checkbox(&mut pd.page.lineweights, "");
            ui.end_row();
            ui.label("Bảng nét in (CTB)");
            ui.horizontal(|ui| {
                let mut pick = None;
                egui::ComboBox::from_id_salt("plot_ctb")
                    .selected_text(if pd.page.plot_style_table.is_empty() { "None".to_string() } else { pd.page.plot_style_table.clone() })
                    .show_ui(ui, |ui| {
                        for c in ["", "monochrome.ctb", "grayscale.ctb", "acad.ctb"] {
                            if ui.selectable_label(pd.page.plot_style_table == c, if c.is_empty() { "None" } else { c }).clicked() {
                                pick = Some(c.to_string());
                            }
                        }
                    });
                if let Some(p) = pick {
                    pd.page.plot_style_table = p;
                }
                ui.add(egui::TextEdit::singleline(&mut pd.page.plot_style_table).desired_width(120.0));
            });
            ui.end_row();
            ui.label("Lề giấy (mm) T-D-P-T");
            ui.horizontal(|ui| {
                for m in pd.page.margins_mm.iter_mut() {
                    ui.add(egui::DragValue::new(m).speed(0.5).range(0.0..=100.0));
                }
            });
            ui.end_row();
        });
        ui.separator();
        ui.horizontal(|ui| {
            if pd.setup_only {
                if ui.button("OK").clicked() {
                    action = Some("ok");
                }
            } else {
                if ui.button("In ra PDF").clicked() {
                    action = Some("pdf");
                }
                if app.services.print_pdf.is_some() && ui.button("In ra máy in").clicked() {
                    action = Some("printer");
                }
                if ui.button("Áp dụng cho layout").clicked() {
                    action = Some("apply");
                }
            }
            if ui.button("Hủy").clicked() {
                action = Some("cancel");
            }
        });
    });
    match action {
        Some("cancel") => return,
        Some("window") => {
            // Pick the window on the drawing; the dialog comes back afterwards.
            let p = pd.params();
            let _ = app.run("pagesetup", p);
            app.start("plot.window");
            return;
        }
        Some(a @ ("ok" | "apply" | "pdf" | "printer")) => {
            if pd.page.plot_area == "window" && pd.page.window.is_none() {
                app.session.echo("Vùng in Window: bấm \"Chọn cửa sổ <\" để chọn hai góc.");
            } else {
                match app.run("pagesetup", pd.params()) {
                    Ok(_) => match a {
                        "pdf" => {
                            if let Some(Err(e)) = crate::menus::run_ui_command(app, "plot.now", &Value::Null) {
                                app.session.echo(e);
                            }
                            return;
                        }
                        "printer" => {
                            if let Some(Err(e)) = crate::menus::run_ui_command(app, "printer", &Value::Null) {
                                app.session.echo(e);
                            }
                            return;
                        }
                        "ok" => {
                            app.session.echo(format!("Đã lưu thiết lập trang cho {}.", pd.layout));
                            return;
                        }
                        _ => app.session.echo(format!("Đã áp dụng thiết lập trang cho {}.", pd.layout)),
                    },
                    Err(e) => app.session.echo(e),
                }
            }
        }
        _ => {}
    }
    if open {
        app.vd.plot = Some(pd);
    }
}

/// Page Setup Manager: every layout's page setup, edit one, or copy one to all layouts.
fn psm_dialog(app: &mut CadApp, ctx: &egui::Context) {
    if !app.vd.psm {
        return;
    }
    let Ok(d) = app.session.doc() else {
        app.vd.psm = false;
        return;
    };
    let mut rows: Vec<(String, cadcraft_doc::PageSetup)> = vec![("Model".into(), cadcraft_io::pdf::model_page(d))];
    rows.extend(d.layouts.iter().map(|l| (l.name.clone(), l.page.clone())));
    let mut open = true;
    let mut edit: Option<String> = None;
    let mut copy_all: Option<String> = None;
    egui::Window::new("Page Setup Manager — Quản lý thiết lập trang").open(&mut open).resizable(true).default_width(620.0).show(ctx, |ui| {
        egui::Grid::new("psm").striped(true).num_columns(5).show(ui, |ui| {
            ui.strong("Layout");
            ui.strong("Khổ giấy");
            ui.strong("Vùng in / tỉ lệ");
            ui.strong("");
            ui.strong("");
            ui.end_row();
            for (n, p) in &rows {
                ui.label(n);
                ui.label(format!("{} {}", p.paper, if p.landscape { "(ngang)" } else { "(đứng)" }));
                ui.label(format!(
                    "{} — {}",
                    p.plot_area,
                    if p.scale_to_fit { "Fit".to_string() } else { format!("1:{}", fmt_num(1.0 / p.scale.max(1e-12))) }
                ));
                if ui.button("Sửa…").clicked() {
                    edit = Some(n.clone());
                }
                if n != "Model" && ui.button("Áp cho mọi layout").on_hover_text("Chép thiết lập này sang tất cả layout").clicked() {
                    copy_all = Some(n.clone());
                }
                ui.end_row();
            }
        });
    });
    if let Some(n) = edit {
        app.vd.plot = Some(PlotDlg::load(app, Some(&n), true));
    }
    if let Some(src) = copy_all {
        let from = PlotDlg::load(app, Some(&src), true);
        let names: Vec<String> = rows.iter().map(|(n, _)| n.clone()).filter(|n| n != "Model" && *n != src).collect();
        let mut k = 0;
        for n in names {
            let mut p = from.params();
            if let Some(o) = p.as_object_mut() {
                o.insert("layout".into(), json!(n));
            }
            if app.run("pagesetup", p).is_ok() {
                k += 1;
            }
        }
        app.session.echo(format!("Đã chép thiết lập trang của {src} sang {k} layout."));
    }
    app.vd.psm = open;
}

// =====================================================================================
// ALIASEDIT

pub struct AliasDlg {
    alias: String,
    command: String,
    filter: String,
}

impl AliasDlg {
    fn load(_app: &CadApp) -> Self {
        AliasDlg { alias: String::new(), command: String::new(), filter: String::new() }
    }
}

fn alias_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut dl) = app.vd.alias.take() else { return };
    let mut open = true;
    let mut add = false;
    let mut remove: Option<String> = None;
    let user: Vec<(String, String)> = app.session.user_aliases.iter().map(|(a, c)| (a.clone(), c.clone())).collect();
    egui::Window::new("ALIASEDIT — Phím tắt lệnh").open(&mut open).resizable(true).default_width(480.0).show(ctx, |ui| {
        ui.label("Phím tắt riêng của bạn (lưu trong máy, dùng ngay; REINIT để nạp lại):");
        egui::ScrollArea::vertical().id_salt("al_user").max_height(160.0).show(ui, |ui| {
            egui::Grid::new("al_grid").striped(true).num_columns(3).show(ui, |ui| {
                for (a, c) in &user {
                    ui.monospace(a.to_ascii_uppercase());
                    ui.label(format!("→ {}", c.to_ascii_uppercase()));
                    if ui.small_button("Xóa").clicked() {
                        remove = Some(a.clone());
                    }
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Phím tắt");
            ui.add(egui::TextEdit::singleline(&mut dl.alias).desired_width(70.0));
            ui.label("Lệnh");
            ui.add(egui::TextEdit::singleline(&mut dl.command).desired_width(140.0).hint_text("gõ vài chữ đầu…"));
            if ui.button("Thêm / Sửa").clicked() {
                add = true;
            }
        });
        // VNCCad: AutoComplete for the command name, as on the command line.
        let typed = dl.command.trim().to_string();
        let sug = crate::cmdline::suggestions(&typed);
        if !typed.is_empty() && !sug.iter().any(|(id, _)| id.eq_ignore_ascii_case(&typed)) && !sug.is_empty() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                for (id, label) in &sug {
                    if ui.selectable_label(false, format!("{id}   —   {label}")).clicked() {
                        dl.command = id.clone();
                    }
                }
            });
        }
        ui.separator();
        ui.collapsing("Phím tắt có sẵn (tham khảo)", |ui| {
            ui.horizontal(|ui| {
                ui.label("Lọc");
                ui.text_edit_singleline(&mut dl.filter);
            });
            let f = dl.filter.to_ascii_lowercase();
            egui::ScrollArea::vertical().id_salt("al_builtin").max_height(200.0).show(ui, |ui| {
                for c in cadcraft_engine::command_specs() {
                    if c.aliases.is_empty() || (!f.is_empty() && !c.id.contains(&f) && !c.aliases.iter().any(|a| a.contains(&f))) {
                        continue;
                    }
                    ui.label(format!("{}  ←  {}", c.id.to_ascii_uppercase(), c.aliases.join(", ").to_ascii_uppercase()));
                }
            });
        });
    });
    if add && run_echo(app, "aliasedit", json!({ "alias": dl.alias.trim(), "command": dl.command.trim() })) {
        save_aliases(app);
        app.session.echo(format!("Đã thêm phím tắt {} → {}.", dl.alias.trim().to_ascii_uppercase(), dl.command.trim().to_ascii_uppercase()));
        dl.alias.clear();
        dl.command.clear();
    }
    if let Some(a) = remove
        && run_echo(app, "aliasedit", json!({ "remove": a }))
    {
        save_aliases(app);
    }
    if open {
        app.vd.alias = Some(dl);
    }
}

// =====================================================================================
// XREF

fn xref_dialog(app: &mut CadApp, ctx: &egui::Context) {
    if !app.vd.xref {
        return;
    }
    let Ok(d) = app.session.doc() else {
        app.vd.xref = false;
        return;
    };
    let refs: Vec<(String, String)> = d.blocks.values().filter_map(|b| b.xref_path.as_ref().map(|p| (b.name.clone(), p.clone()))).collect();
    let mut open = true;
    let mut action: Option<(&str, String)> = None;
    let mut attach = false;
    egui::Window::new("External References — Tham chiếu ngoài (XREF)").open(&mut open).resizable(true).default_width(520.0).show(ctx, |ui| {
        if ui.button("📎 Chèn bản vẽ tham chiếu (Attach DWG/DXF)…").clicked() {
            attach = true;
        }
        ui.separator();
        if refs.is_empty() {
            ui.label("Bản vẽ chưa có tham chiếu ngoài.");
        }
        egui::Grid::new("xr").striped(true).num_columns(5).show(ui, |ui| {
            for (n, p) in &refs {
                ui.label(n);
                ui.label(RichText::new(p).small());
                if ui.button("Nạp lại").clicked() {
                    action = Some(("reload", n.clone()));
                }
                if ui.button("Gỡ (Detach)").clicked() {
                    action = Some(("detach", n.clone()));
                }
                if ui.button("Gắn hẳn (Bind)").clicked() {
                    action = Some(("bind", n.clone()));
                }
                ui.end_row();
            }
        });
    });
    if attach {
        app.vd.xref = false;
        if let Some(Err(e)) = crate::menus::run_ui_command(app, "xattach", &Value::Null) {
            app.session.echo(e);
        }
        return;
    }
    if let Some((a, n)) = action {
        run_echo(app, "xref", json!({ "action": a, "name": n }));
    }
    app.vd.xref = open;
}

// =====================================================================================
// MLEADERSTYLE (VNCCad)

pub struct MlsDlg {
    sel: String,
    cur: cadcraft_doc::MLeaderStyle,
    current: String,
    new_name: String,
}

impl MlsDlg {
    fn load(app: &CadApp, name: Option<&str>) -> Self {
        let d = app.session.doc().ok();
        let current = d.map(|d| d.header.str("CMLEADERSTYLE", "Standard")).unwrap_or_else(|| "Standard".into());
        let sel = name.map(str::to_string).unwrap_or_else(|| current.clone());
        let cur = d
            .and_then(|d| d.mleader_styles.iter().find(|m| m.name.eq_ignore_ascii_case(&sel)).cloned())
            .unwrap_or(cadcraft_doc::MLeaderStyle { name: sel.clone(), ..Default::default() });
        let taken: Vec<String> = d.map(|d| d.mleader_styles.iter().map(|m| m.name.clone()).collect()).unwrap_or_default();
        MlsDlg { sel: cur.name.clone(), cur, current, new_name: next_free(&taken, "Leader") }
    }
}

fn mls_dialog(app: &mut CadApp, ctx: &egui::Context) {
    let Some(mut st) = app.vd.mls.take() else { return };
    let mut names: Vec<String> = app.session.doc().map(|d| d.mleader_styles.iter().map(|m| m.name.clone()).collect()).unwrap_or_default();
    if names.is_empty() {
        names.push("Standard".into());
    }
    let styles = text_styles(app);
    let mut open = true;
    let mut action: Option<&str> = None;
    let mut select: Option<String> = None;
    egui::Window::new("Multileader Style — Kiểu ghi chú dẫn").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.label(format!("Kiểu hiện hành: {}", st.current));
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width(170.0);
                for n in &names {
                    if ui.selectable_label(*n == st.sel, n).clicked() {
                        select = Some(n.clone());
                    }
                }
                if ui.button("Đặt hiện hành").clicked() {
                    action = Some("current");
                }
                ui.separator();
                ui.label("Kiểu mới");
                ui.add(egui::TextEdit::singleline(&mut st.new_name).desired_width(160.0));
                if ui.button("Tạo mới (New)").clicked() {
                    action = Some("new");
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.heading(&st.sel);
                egui::Grid::new("mls_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Cỡ mũi tên (Arrowhead size)");
                    num_edit(ui, &mut st.cur.arrow_size, 0.01);
                    ui.end_row();
                    ui.label("Chiều cao chữ");
                    num_edit(ui, &mut st.cur.text_height, 0.01);
                    ui.end_row();
                    ui.label("Khe hở chữ (Landing gap)");
                    num_edit(ui, &mut st.cur.landing_gap, 0.01);
                    ui.end_row();
                    ui.label("Đoạn gãy (Dogleg)");
                    num_edit(ui, &mut st.cur.dogleg, 0.01);
                    ui.end_row();
                    ui.label("Kiểu chữ");
                    combo(ui, "mls_ts", &mut st.cur.text_style, &styles);
                    ui.end_row();
                });
                ui.label(RichText::new("Kích thước thật = giá trị × DIMSCALE. Cỡ từng ghi chú: Properties → Arrow size / Overall scale.").small());
                if ui.button("Áp dụng (Apply)").clicked() {
                    action = Some("apply");
                }
            });
        });
    });
    let params = |name: &str, c: &cadcraft_doc::MLeaderStyle, current: bool| {
        json!({
            "name": name,
            "arrowSize": c.arrow_size,
            "textHeight": c.text_height,
            "landingGap": c.landing_gap,
            "dogleg": c.dogleg,
            "textStyle": c.text_style,
            "current": current,
        })
    };
    match action {
        Some("apply") => {
            let p = params(&st.sel, &st.cur, st.sel.eq_ignore_ascii_case(&st.current));
            if run_echo(app, "mleaderstyle", p) {
                app.session.echo(format!("Đã cập nhật kiểu ghi chú dẫn {}.", st.sel));
            }
        }
        Some("new") => {
            let mut n = st.new_name.trim().to_string();
            if n.is_empty() {
                n = next_free(&names, "Leader");
            }
            if names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                app.session.echo(format!("Đã có kiểu {n} — đặt tên khác."));
            } else if run_echo(app, "mleaderstyle", params(&n, &st.cur, true)) {
                app.session.echo(format!("Đã tạo kiểu ghi chú dẫn {n} (hiện hành)."));
                st = MlsDlg::load(app, Some(&n));
            }
        }
        Some("current") if run_echo(app, "mleaderstyle", json!({ "name": st.sel, "current": true })) => {
            st = MlsDlg::load(app, Some(&st.sel.clone()));
        }
        _ => {}
    }
    if let Some(n) = select {
        st = MlsDlg::load(app, Some(&n));
    }
    if open {
        app.vd.mls = Some(st);
    }
}
