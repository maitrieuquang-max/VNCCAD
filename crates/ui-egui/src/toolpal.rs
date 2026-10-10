//! VNCCad: DesignCenter (browse the blocks, layers and styles of the open drawings and bring
//! them into the active one) and Tool Palettes (one-click tools: commands, hatches, blocks).

use egui::{Color32, RichText};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// A tool of the tool palette.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolItem {
    pub label: String,
    /// "command", "hatch" or "block".
    pub kind: String,
    /// Command line text, hatch pattern or block name.
    pub value: String,
    /// Blocks: the drawing file the block comes from (desktop), when it is not open.
    pub source: Option<String>,
}

impl Default for ToolItem {
    fn default() -> Self {
        ToolItem { label: String::new(), kind: "command".into(), value: String::new(), source: None }
    }
}

fn tool(label: &str, kind: &str, value: &str) -> ToolItem {
    ToolItem { label: label.into(), kind: kind.into(), value: value.into(), source: None }
}

pub fn default_tools() -> Vec<ToolItem> {
    vec![
        tool("Hatch ANSI31 (gạch chéo)", "hatch", "ANSI31"),
        tool("Hatch bê tông AR-CONC", "hatch", "AR-CONC"),
        tool("Hatch cát AR-SAND", "hatch", "AR-SAND"),
        tool("Hatch đất EARTH", "hatch", "EARTH"),
        tool("Tô đặc SOLID", "hatch", "SOLID"),
        tool("Tường / dầm (MLINE)", "command", "mline"),
        tool("Khung dung sai", "command", "tolerance"),
        tool("Ghi chú dẫn (MLEADER)", "command", "mleader"),
        tool("Bảng", "command", "table"),
        tool("Diện tích (FIELD)", "command", "field"),
        tool("Vẽ tay (SKETCH)", "command", "sketch"),
    ]
}

const PREF_KEY: &str = "toolpalette";

/// The tool palette, loaded from the host's preferences the first time.
fn tools(app: &mut CadApp) -> &mut Vec<ToolItem> {
    if app.tools.is_none() {
        let saved = app.services.prefs_get.as_ref().and_then(|g| g(PREF_KEY)).and_then(|t| serde_json::from_str::<Vec<ToolItem>>(&t).ok());
        app.tools = Some(saved.unwrap_or_else(default_tools));
    }
    app.tools.get_or_insert_with(default_tools)
}

fn save_tools(app: &CadApp) {
    if let (Some(set), Some(t)) = (app.services.prefs_set.as_ref(), app.tools.as_ref())
        && let Ok(text) = serde_json::to_string(t)
    {
        set(PREF_KEY, &text);
    }
}

/// Run a tool.
pub fn run_tool(app: &mut CadApp, t: &ToolItem) {
    if app.session.running.is_some() {
        app.session.cancel();
    }
    match t.kind.as_str() {
        "hatch" => {
            if let Ok(d) = app.session.doc_mut() {
                d.header.set_str("HPNAME", &t.value);
            }
            app.start("hatch");
        }
        "block" => {
            let have = app.session.doc().is_ok_and(|d| d.block(&t.value).is_some());
            if !have {
                // From an open drawing that has it, else from its source file.
                let from = app.session.docs.iter().position(|d| d.doc.block(&t.value).is_some());
                let r = match (from, &t.source) {
                    (Some(i), _) => app.run("adcenter.import", json!({ "doc": i, "kind": "blocks", "name": t.value })),
                    (None, Some(path)) => app.run("adcenter.importfile", json!({ "path": path, "kind": "blocks", "name": t.value })),
                    (None, None) => Err(format!("Không tìm thấy block {} (mở bản vẽ chứa nó trước).", t.value)),
                };
                if let Err(e) = r {
                    app.session.echo(e);
                    return;
                }
            }
            app.start("insert");
            if let Err(e) = app.session.input(cadcraft_engine::Input::Text(t.value.clone())) {
                app.session.echo(e.to_string());
            }
        }
        _ => app.cmdline(&t.value),
    }
}

const KINDS: &[(&str, &str)] = &[
    ("blocks", "Khối"),
    ("layers", "Lớp"),
    ("linetypes", "Kiểu đường"),
    ("textStyles", "Kiểu chữ"),
    ("dimStyles", "Kiểu kích thước"),
    ("mlineStyles", "Kiểu MLINE"),
];

pub fn designcenter(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("DesignCenter").size(14.0).color(t.text));
    });
    let n = app.session.docs.len();
    if n == 0 {
        ui.label("Chưa mở bản vẽ nào.");
        return;
    }
    let src = app.adc_doc.filter(|i| *i < n).unwrap_or(app.session.active);
    let titles: Vec<String> = app.session.docs.iter().map(|d| d.title.clone()).collect();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Nguồn").color(t.text_dim));
        egui::ComboBox::from_id_salt("adc_src").width(200.0).selected_text(titles.get(src).cloned().unwrap_or_default()).show_ui(ui, |ui| {
            for (i, title) in titles.iter().enumerate() {
                let label = if i == app.session.active { format!("{title} (đang vẽ)") } else { title.clone() };
                if ui.selectable_label(i == src, label).clicked() {
                    app.adc_doc = Some(i);
                }
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        for (k, label) in KINDS {
            if ui.selectable_label(app.adc_kind == *k, *label).clicked() {
                app.adc_kind = (*k).to_string();
            }
        }
    });
    if src != app.session.active {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Thêm vào: {}", titles.get(app.session.active).cloned().unwrap_or_default())).size(11.5).color(t.text_faint),
            );
        });
    }
    ui.separator();
    let Ok(list) = app.session.execute("adcenter.list", &json!({ "doc": src })) else { return };
    let names: Vec<String> =
        list[app.adc_kind.as_str()].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    let mut action: Option<(String, bool, bool)> = None;
    egui::ScrollArea::vertical().id_salt("adc_list").auto_shrink([false, false]).max_height(ui.available_height() - 4.0).show(ui, |ui| {
        if names.is_empty() {
            ui.label(RichText::new("(trống)").color(t.text_faint));
        }
        for name in names.iter().take(5000) {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let blocks = app.adc_kind == "blocks";
                if blocks {
                    if ui.small_button("Chèn").on_hover_text("Chèn block vào bản vẽ đang vẽ").clicked() {
                        action = Some((name.clone(), true, false));
                    }
                    if ui.small_button("Ghim").on_hover_text("Thêm vào bảng công cụ").clicked() {
                        action = Some((name.clone(), false, true));
                    }
                } else if src != app.session.active && ui.small_button("Thêm").on_hover_text("Thêm vào bản vẽ đang vẽ").clicked() {
                    action = Some((name.clone(), false, false));
                }
                ui.label(RichText::new(name).color(t.text));
            });
        }
    });
    let Some((name, insert, pin)) = action else { return };
    if pin {
        let source = app.session.docs.get(src).and_then(|d| d.path.clone());
        tools(app).push(ToolItem { label: format!("Block {name}"), kind: "block".into(), value: name.clone(), source });
        save_tools(app);
        app.session.echo(format!("Đã ghim block {name} vào bảng công cụ."));
        return;
    }
    if src != app.session.active
        && let Err(e) = app.run("adcenter.import", json!({ "doc": src, "kind": app.adc_kind.clone(), "name": name }))
    {
        app.session.echo(e);
        return;
    }
    if insert {
        run_tool(app, &ToolItem { label: String::new(), kind: "block".into(), value: name, source: None });
    }
}

pub fn tool_palette(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Bảng công cụ").size(14.0).color(t.text));
    });
    let list = tools(app).clone();
    let mut run: Option<ToolItem> = None;
    let mut remove: Option<usize> = None;
    let mut up: Option<usize> = None;
    for (i, it) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let tag = match it.kind.as_str() {
                "hatch" => "▦",
                "block" => "◧",
                _ => "▶",
            };
            let b = egui::Button::new(RichText::new(format!("{tag}  {}", it.label)).color(t.text)).min_size(egui::vec2(196.0, 24.0)).fill(t.control);
            if ui.add(b).on_hover_text(format!("{}: {}", it.kind, it.value)).clicked() {
                run = Some(it.clone());
            }
            if i > 0 && ui.small_button("↑").clicked() {
                up = Some(i);
            }
            if ui.small_button("✕").on_hover_text("Bỏ khỏi bảng").clicked() {
                remove = Some(i);
            }
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Thêm công cụ lệnh").color(t.text_dim));
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.add(egui::TextEdit::singleline(&mut app.new_tool.0).hint_text("Tên").desired_width(70.0));
        ui.add(egui::TextEdit::singleline(&mut app.new_tool.1).hint_text("Lệnh, vd: circle").desired_width(130.0));
        if ui.small_button("+").clicked() && !app.new_tool.1.trim().is_empty() {
            let label = if app.new_tool.0.trim().is_empty() { app.new_tool.1.trim().to_string() } else { app.new_tool.0.trim().to_string() };
            let value = app.new_tool.1.trim().to_string();
            tools(app).push(ToolItem { label, kind: "command".into(), value, source: None });
            app.new_tool = (String::new(), String::new());
            save_tools(app);
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if ui.small_button("Khôi phục mặc định").clicked() {
            app.tools = Some(default_tools());
            save_tools(app);
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Ghim block: DesignCenter → Ghim").size(11.0).color(Color32::GRAY));
    });
    if let Some(i) = remove {
        tools(app).remove(i);
        save_tools(app);
    }
    if let Some(i) = up {
        tools(app).swap(i, i - 1);
        save_tools(app);
    }
    if let Some(it) = run {
        run_tool(app, &it);
    }
}

/// Tools as JSON (automation: `ui.inspect`).
pub fn tools_json(app: &mut CadApp) -> Value {
    serde_json::to_value(tools(app).clone()).unwrap_or(Value::Null)
}
