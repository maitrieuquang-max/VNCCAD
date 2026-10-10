//! VNCCad: shows a DCL dialog from a LISP routine (`start_dialog`) and sends the answer — the
//! tile pressed and every tile's value — back as the routine's next input.

use std::collections::HashMap;

use cadcraft_engine::Input;
use cadcraft_engine::lisp::dcl::{ImageOp, Pending, Tile, tile_size};
use egui::vec2;
use serde_json::json;

use crate::CadApp;

#[derive(Clone, Default)]
struct Edit {
    values: HashMap<String, String>,
}

struct Ctx<'a> {
    p: &'a Pending,
    vals: &'a mut HashMap<String, String>,
    pressed: Option<(String, i64)>,
}

impl Ctx<'_> {
    fn val(&self, key: &str) -> String {
        self.vals.get(key).cloned().unwrap_or_default()
    }
    fn enabled(&self, t: &Tile) -> bool {
        let off = t.attr("is_enabled").is_some_and(|v| v.eq_ignore_ascii_case("false"));
        !off && t.key().is_none_or(|k| !self.p.disabled.iter().any(|d| d == k))
    }
    /// A change to a tile with an action: report it at once (reason 1, or 2 while editing).
    fn changed(&mut self, key: &str, reason: i64) {
        if self.p.actions.iter().any(|a| a == key) && self.pressed.is_none() {
            self.pressed = Some((key.to_string(), reason));
        }
    }
}

fn label_of(t: &Tile) -> String {
    t.attr("label").unwrap_or("").replace('&', "")
}

fn width_of(t: &Tile, attr: &str) -> Option<f32> {
    t.attr(attr).and_then(|w| w.trim().parse::<f32>().ok()).map(|w| (w * 7.5).clamp(30.0, 900.0))
}

fn children(ui: &mut egui::Ui, c: &mut Ctx, t: &Tile, horizontal: bool) {
    if horizontal {
        ui.horizontal(|ui| {
            for ch in &t.children {
                tile(ui, c, ch, Some(t));
            }
        });
    } else {
        ui.vertical(|ui| {
            for ch in &t.children {
                tile(ui, c, ch, Some(t));
            }
        });
    }
}

fn button(ui: &mut egui::Ui, c: &mut Ctx, key: &str, label: &str, enabled: bool) {
    let b = egui::Button::new(label).min_size(vec2(84.0, 0.0));
    if ui.add_enabled(enabled, b).clicked() && c.pressed.is_none() {
        c.pressed = Some((key.to_string(), 1));
    }
}

fn tile(ui: &mut egui::Ui, c: &mut Ctx, t: &Tile, parent: Option<&Tile>) {
    let enabled = c.enabled(t);
    let key = t.key().unwrap_or("").to_string();
    match t.kind.as_str() {
        "row" | "concatenation" => children(ui, c, t, true),
        "column" | "paragraph" | "dialog" => children(ui, c, t, false),
        "boxed_row" | "boxed_column" | "boxed_radio_row" | "boxed_radio_column" => {
            ui.group(|ui| {
                let l = label_of(t);
                if !l.is_empty() {
                    ui.strong(l);
                }
                children(ui, c, t, t.kind.contains("row"));
            });
        }
        "radio_row" | "radio_column" => {
            let l = label_of(t);
            if !l.is_empty() {
                ui.label(l);
            }
            children(ui, c, t, t.kind == "radio_row");
        }
        "text" | "text_part" | "errtile" => {
            let v = if key.is_empty() { String::new() } else { c.val(&key) };
            let s = if v.is_empty() { label_of(t) } else { v };
            if !s.is_empty() || t.kind != "errtile" {
                ui.label(s);
            }
        }
        "spacer" | "spacer_0" | "spacer_1" => ui.add_space(6.0),
        "edit_box" => {
            ui.horizontal(|ui| {
                let l = label_of(t);
                if !l.is_empty() {
                    ui.label(l);
                }
                let mut v = c.val(&key);
                let w = width_of(t, "edit_width").or_else(|| width_of(t, "width")).unwrap_or(140.0);
                let password = t.attr("password_char").is_some_and(|p| !p.is_empty());
                let r = ui.add_enabled(enabled, egui::TextEdit::singleline(&mut v).desired_width(w).password(password));
                if r.changed() {
                    c.vals.insert(key.clone(), v);
                }
                if r.lost_focus() && c.p.actions.contains(&key) {
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    c.changed(&key, if enter { 1 } else { 2 });
                }
            });
        }
        "popup_list" => {
            ui.horizontal(|ui| {
                let l = label_of(t);
                if !l.is_empty() {
                    ui.label(l);
                }
                let items = c.p.lists.get(&key).cloned().unwrap_or_default();
                let cur: usize = c.val(&key).trim().parse().unwrap_or(0);
                let mut sel = cur;
                let w = width_of(t, "edit_width").or_else(|| width_of(t, "width")).unwrap_or(160.0);
                ui.add_enabled_ui(enabled, |ui| {
                    egui::ComboBox::from_id_salt(("dcl", key.as_str())).width(w).selected_text(items.get(cur).cloned().unwrap_or_default()).show_ui(
                        ui,
                        |ui| {
                            for (i, it) in items.iter().enumerate() {
                                ui.selectable_value(&mut sel, i, it);
                            }
                        },
                    );
                });
                if sel != cur {
                    c.vals.insert(key.clone(), sel.to_string());
                    c.changed(&key, 1);
                }
            });
        }
        "list_box" => {
            let l = label_of(t);
            if !l.is_empty() {
                ui.label(l);
            }
            let items = c.p.lists.get(&key).cloned().unwrap_or_default();
            let multi = t.attr("multiple_select").is_some_and(|m| m.eq_ignore_ascii_case("true"));
            let mut sel: Vec<usize> = c.val(&key).split_whitespace().filter_map(|x| x.parse().ok()).collect();
            let h = t.attr("height").and_then(|h| h.parse::<f32>().ok()).map(|h| h * 18.0).unwrap_or(150.0);
            let w = width_of(t, "width").unwrap_or(220.0);
            let mut clicked = None;
            ui.add_enabled_ui(enabled, |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(w);
                    egui::ScrollArea::vertical().id_salt(("dcl_list", key.as_str())).max_height(h).show(ui, |ui| {
                        for (i, it) in items.iter().enumerate() {
                            let r = ui.selectable_label(sel.contains(&i), it);
                            if r.double_clicked() {
                                clicked = Some((i, 4));
                            } else if r.clicked() {
                                clicked = Some((i, 1));
                            }
                        }
                    });
                });
            });
            if let Some((i, reason)) = clicked {
                if multi {
                    if let Some(pos) = sel.iter().position(|x| *x == i) {
                        sel.remove(pos);
                    } else {
                        sel.push(i);
                        sel.sort_unstable();
                    }
                } else {
                    sel = vec![i];
                }
                c.vals.insert(key.clone(), sel.iter().map(usize::to_string).collect::<Vec<_>>().join(" "));
                c.changed(&key, reason);
            }
        }
        "toggle" => {
            let mut on = c.val(&key).trim() == "1";
            if ui.add_enabled(enabled, egui::Checkbox::new(&mut on, label_of(t))).changed() {
                c.vals.insert(key.clone(), if on { "1" } else { "0" }.into());
                c.changed(&key, 1);
            }
        }
        "radio_button" => {
            let group = parent.filter(|p| p.kind.contains("radio"));
            let gkey = group.and_then(|g| g.key()).unwrap_or("").to_string();
            let gval = if gkey.is_empty() { String::new() } else { c.val(&gkey) };
            let on = if gval.is_empty() { c.val(&key).trim() == "1" } else { gval == key };
            if ui.add_enabled(enabled, egui::RadioButton::new(on, label_of(t))).clicked() && !on {
                if let Some(g) = group {
                    for sib in &g.children {
                        if let Some(k) = sib.key() {
                            c.vals.insert(k.to_string(), "0".into());
                        }
                    }
                }
                c.vals.insert(key.clone(), "1".into());
                if !gkey.is_empty() {
                    c.vals.insert(gkey.clone(), key.clone());
                    c.changed(&gkey, 1);
                }
                c.changed(&key, 1);
            }
        }
        "slider" => {
            let lo = t.attr("min_value").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
            let hi = t.attr("max_value").and_then(|v| v.parse::<i64>().ok()).unwrap_or(10000);
            let mut v: i64 = c.val(&key).trim().parse().unwrap_or(lo);
            if ui.add_enabled(enabled, egui::Slider::new(&mut v, lo.min(hi)..=hi.max(lo))).changed() {
                c.vals.insert(key.clone(), v.to_string());
                c.changed(&key, 1);
            }
        }
        "button" => {
            let l = label_of(t);
            let l = if l.is_empty() { key.clone() } else { l };
            let cancel = t.attr("is_cancel").is_some_and(|v| v.eq_ignore_ascii_case("true"));
            if cancel && !c.p.actions.contains(&key) {
                if ui.add_enabled(enabled, egui::Button::new(l).min_size(vec2(84.0, 0.0))).clicked() && c.pressed.is_none() {
                    c.pressed = Some(("cancel".into(), 1));
                }
            } else {
                button(ui, c, &key, &l, enabled);
            }
        }
        "image" | "image_button" => {
            let (w, h) = tile_size(t);
            let sense = if t.kind == "image_button" && enabled { egui::Sense::click() } else { egui::Sense::hover() };
            let (r, resp) = ui.allocate_exact_size(vec2(w, h), sense);
            let base = t.attr("color").and_then(color_attr);
            paint_image(ui, r, c.p.images.get(&key).map(Vec::as_slice).unwrap_or(&[]), base);
            if t.kind == "image_button" {
                if resp.hovered() {
                    ui.painter().rect_stroke(r, 2.0, ui.visuals().widgets.hovered.bg_stroke, egui::StrokeKind::Inside);
                }
                if resp.clicked() && c.pressed.is_none() {
                    // AutoCAD sets the button's value to nothing and reports the click point.
                    if let Some(p) = resp.interact_pointer_pos() {
                        let rel = p - r.min;
                        c.vals.insert("$x".into(), format!("{}", rel.x.round()));
                        c.vals.insert("$y".into(), format!("{}", rel.y.round()));
                    }
                    c.pressed = Some((key.clone(), 1));
                }
            }
        }
        "ok_only" | "ok_cancel" | "ok_cancel_help" | "ok_cancel_help_info" | "ok_cancel_help_errtile" => {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                button(
                    ui,
                    c,
                    "accept",
                    "OK",
                    c.enabled(&Tile { kind: String::new(), attrs: vec![("key".into(), "accept".into())], children: vec![] }),
                );
                if t.kind != "ok_only" {
                    button(ui, c, "cancel", "Hủy", true);
                }
                if t.kind.contains("help") && c.p.actions.iter().any(|a| a == "help") {
                    button(ui, c, "help", "Trợ giúp", true);
                }
                if t.kind.contains("info") && c.p.actions.iter().any(|a| a == "info") {
                    button(ui, c, "info", "Thông tin", true);
                }
            });
            if t.kind.ends_with("errtile") {
                let e = c.val("error");
                if !e.is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
            }
        }
        _ => {
            // Unknown tile kinds: show their label and children so nothing is lost.
            let l = label_of(t);
            if !l.is_empty() {
                ui.label(l);
            }
            if !t.children.is_empty() {
                children(ui, c, t, false);
            }
        }
    }
}

/// An AutoCAD colour number in a dialog: negative numbers are the dialog's own colours.
fn dcl_color(ui: &egui::Ui, c: i32) -> egui::Color32 {
    match c {
        -2 => crate::theme::Tokens::get().canvas,
        -15 => ui.visuals().window_fill,
        -16 => ui.visuals().text_color(),
        -18 | -17 => ui.visuals().widgets.noninteractive.bg_stroke.color,
        0 => egui::Color32::BLACK,
        1..=255 => {
            let rgb = cadcraft_color::aci_rgb(c as u8);
            egui::Color32::from_rgb(rgb.0, rgb.1, rgb.2)
        }
        _ => ui.visuals().text_color(),
    }
}

/// The `color` attribute of an image tile: a number or a DCL colour name.
fn color_attr(v: &str) -> Option<i32> {
    let v = v.trim().to_ascii_lowercase();
    v.parse::<i32>().ok().or(match v.as_str() {
        "dialog_line" => Some(-18),
        "dialog_foreground" => Some(-16),
        "dialog_background" => Some(-15),
        "graphics_background" | "black" => Some(0),
        "graphics_foreground" | "white" => Some(7),
        "red" => Some(1),
        "yellow" => Some(2),
        "green" => Some(3),
        "cyan" => Some(4),
        "blue" => Some(5),
        "magenta" => Some(6),
        _ => None,
    })
}

/// Draw an image tile: its background, then what the routine drew (`fill_image`,
/// `vector_image`, `slide_image`), clipped to the tile.
fn paint_image(ui: &egui::Ui, r: egui::Rect, ops: &[ImageOp], base: Option<i32>) {
    let p = ui.painter().with_clip_rect(r);
    p.rect_filled(r, 0.0, dcl_color(ui, base.unwrap_or(-15)));
    let at = |x: f32, y: f32| egui::pos2(r.min.x + x, r.min.y + y);
    for op in ops {
        match op {
            ImageOp::Fill { x, y, w, h, color } => {
                p.rect_filled(egui::Rect::from_min_size(at(*x, *y), vec2(*w, *h)), 0.0, dcl_color(ui, *color));
            }
            ImageOp::Line { x1, y1, x2, y2, color } => {
                p.line_segment([at(*x1, *y1), at(*x2, *y2)], egui::Stroke::new(1.0, dcl_color(ui, *color)));
            }
            ImageOp::Poly { points, color } => {
                let pts: Vec<egui::Pos2> = points.iter().map(|(x, y)| at(*x, *y)).collect();
                p.add(egui::Shape::convex_polygon(pts, dcl_color(ui, *color), egui::Stroke::NONE));
            }
        }
    }
    ui.painter().rect_stroke(r, 0.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
}

/// Show the dialog a LISP routine is waiting on, if any.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let Some(p) = app.session.current_prompt().and_then(|p| p.dialog) else { return };
    let sig = serde_json::to_string(&p).unwrap_or_default();
    let id = egui::Id::new(("dcl_edit", sig));
    let mut edit: Edit = ctx.data(|d| d.get_temp(id)).unwrap_or_else(|| Edit { values: p.values.clone() });
    let title = {
        let l = label_of(&p.dialog);
        if l.is_empty() { p.name.clone() } else { l }
    };
    let mut c = Ctx { p: &p, vals: &mut edit.values, pressed: None };
    egui::Window::new(title)
        .id(egui::Id::new(("dcl_window", p.name.as_str())))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            for ch in &p.dialog.children {
                tile(ui, &mut c, ch, Some(&p.dialog));
            }
        });
    if c.pressed.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        c.pressed = Some(("cancel".into(), 1));
    }
    if c.pressed.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !ctx.egui_wants_keyboard_input() {
        c.pressed = Some(("accept".into(), 1));
    }
    let pressed = c.pressed.take();
    match pressed {
        Some((key, reason)) => {
            ctx.data_mut(|d| d.remove::<Edit>(id));
            let answer = json!({ "pressed": key, "reason": reason, "values": edit.values });
            if let Err(e) = app.session.input(Input::Text(answer.to_string())) {
                app.set_status(e.to_string());
            }
            ctx.request_repaint();
        }
        None => {
            ctx.data_mut(|d| d.insert_temp(id, edit));
        }
    }
}
