//! VNCCad: in-place text editing on the drawing — double-click a TEXT, MTEXT, MLEADER or a TABLE
//! cell and type over it where it sits, as in AutoCAD.
//!
//! - TEXT, table cell: **Enter** keeps the text (in a table Enter goes to the cell below, **Tab** /
//!   **Shift+Tab** to the next / previous cell).
//! - MTEXT, MLEADER: Enter starts a new line; **Ctrl+Enter** or a click outside keeps the text.
//! - **Esc** leaves without changes. The "…" button opens the full properties dialog.

use cadcraft_doc::{Entity, EntityKind, Handle};
use cadcraft_geom::Vec2;
use egui::{Color32, Key, Modifiers, Stroke, vec2};
use serde_json::json;

use crate::CadApp;
use crate::canvas::Xf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Text,
    MText,
    MLeader,
    Cell { row: usize, col: usize },
}

pub struct InPlace {
    pub handle: Handle,
    pub target: Target,
    pub buf: String,
    /// Text box in world units: top-left corner, width, height of one line.
    origin: Vec2,
    width: f64,
    line_h: f64,
    focus_pending: bool,
}

impl InPlace {
    fn multiline(&self) -> bool {
        matches!(self.target, Target::MText | Target::MLeader)
    }
}

/// Start editing `h` in place at the double-clicked world point. `false` when this object is
/// not edited in place (the caller opens its dialog instead).
pub fn start(app: &mut CadApp, h: Handle, at: Vec2) -> bool {
    let Ok(d) = app.session.doc() else { return false };
    let Some(e) = d.entity(h).map(|e| (**e).clone()) else { return false };
    let b = cadcraft_doc::entity_bounds(d, &e, 0);
    let ed = match &e.kind {
        EntityKind::Text(t) => {
            let (origin, width) = if b.is_empty() {
                (t.insert.xy() + Vec2::new(0.0, t.height), t.height * 8.0)
            } else {
                (Vec2::new(b.min.x, b.max.y), b.max.x - b.min.x)
            };
            InPlace { handle: h, target: Target::Text, buf: t.value.clone(), origin, width, line_h: t.height, focus_pending: true }
        }
        EntityKind::MText(t) => {
            let (origin, width) = if b.is_empty() { (t.insert.xy(), t.height * 12.0) } else { (Vec2::new(b.min.x, b.max.y), b.max.x - b.min.x) };
            InPlace {
                handle: h,
                target: Target::MText,
                buf: t.contents.replace("\\P", "\n"),
                origin,
                width: width.max(t.width),
                line_h: t.height,
                focus_pending: true,
            }
        }
        EntityKind::MLeader(m) => {
            let Some(t) = &m.text else { return false };
            InPlace {
                handle: h,
                target: Target::MLeader,
                buf: t.contents.replace("\\P", "\n"),
                origin: t.insert.xy(),
                width: if t.width > 0.0 { t.width } else { t.height * 12.0 },
                line_h: t.height,
                focus_pending: true,
            }
        }
        EntityKind::Table(t) => {
            let Some((row, col)) = cell_at(t, at) else { return false };
            let mut ed = InPlace {
                handle: h,
                target: Target::Cell { row, col },
                buf: String::new(),
                origin: at,
                width: 1.0,
                line_h: t.text_height,
                focus_pending: true,
            };
            place_cell(&mut ed, t);
            ed
        }
        _ => return false,
    };
    app.inplace = Some(ed);
    true
}

// ---------------------------------------------------------------- table geometry

fn offsets(v: &[f64]) -> Vec<f64> {
    std::iter::once(0.0)
        .chain(v.iter().take(10_000).scan(0.0, |a, w| {
            *a += w;
            Some(*a)
        }))
        .collect()
}

/// The cell (anchor of a merged range) under world point `p`.
pub fn cell_at(t: &cadcraft_doc::Table, p: Vec2) -> Option<(usize, usize)> {
    let o = t.insert.xy();
    let xs = offsets(&t.col_widths);
    let ys = offsets(&t.row_heights);
    let (dx, dy) = (p.x - o.x, o.y - p.y);
    let col = xs.windows(2).position(|w| dx >= w.first().copied().unwrap_or(0.0) && dx <= w.get(1).copied().unwrap_or(0.0))?;
    let row = ys.windows(2).position(|w| dy >= w.first().copied().unwrap_or(0.0) && dy <= w.get(1).copied().unwrap_or(0.0))?;
    Some(anchor(t, row, col))
}

/// The top-left cell of the merged range covering (row, col), or the cell itself.
fn anchor(t: &cadcraft_doc::Table, row: usize, col: usize) -> (usize, usize) {
    for (r, cells) in t.cells.iter().enumerate().take(row + 1) {
        for (c, cell) in cells.iter().enumerate().take(col + 1) {
            if let Some((rs, cs)) = cell.merged {
                let (rs, cs) = (rs.max(1) as usize, cs.max(1) as usize);
                if row < r + rs && col < c + cs && (r, c) != (row, col) {
                    return (r, c);
                }
            }
        }
    }
    (row, col)
}

fn span(t: &cadcraft_doc::Table, row: usize, col: usize) -> (usize, usize) {
    t.cells.get(row).and_then(|r| r.get(col)).and_then(|c| c.merged).map(|(a, b)| (a.max(1) as usize, b.max(1) as usize)).unwrap_or((1, 1))
}

fn place_cell(ed: &mut InPlace, t: &cadcraft_doc::Table) {
    let Target::Cell { row, col } = ed.target else { return };
    let o = t.insert.xy();
    let xs = offsets(&t.col_widths);
    let ys = offsets(&t.row_heights);
    let (rs, cs) = span(t, row, col);
    let x0 = xs.get(col).copied().unwrap_or(0.0);
    let x1 = xs.get((col + cs).min(xs.len().saturating_sub(1))).copied().unwrap_or(x0 + 1.0);
    let y0 = ys.get(row).copied().unwrap_or(0.0);
    let y1 = ys.get((row + rs).min(ys.len().saturating_sub(1))).copied().unwrap_or(y0 + 1.0);
    ed.origin = o + Vec2::new(x0, -y0);
    ed.width = (x1 - x0).max(1e-9);
    ed.line_h = (y1 - y0).max(1e-9) / 1.6;
    ed.buf = t.cells.get(row).and_then(|r| r.get(col)).map(|c| c.text.clone()).unwrap_or_default();
}

/// Next / previous / lower cell, skipping cells covered by a merge.
fn step_cell(t: &cadcraft_doc::Table, row: usize, col: usize, how: Step) -> Option<(usize, usize)> {
    let (rows, cols) = (t.row_heights.len(), t.col_widths.len());
    if rows == 0 || cols == 0 {
        return None;
    }
    let (rs, cs) = span(t, row, col);
    let mut cur = (row, col);
    for _ in 0..rows.saturating_mul(cols).min(100_000) {
        cur = match how {
            Step::Next => {
                if cur.1 + 1 < cols {
                    (cur.0, cur.1 + 1)
                } else if cur.0 + 1 < rows {
                    (cur.0 + 1, 0)
                } else {
                    return None;
                }
            }
            Step::Prev => {
                if cur.1 > 0 {
                    (cur.0, cur.1 - 1)
                } else if cur.0 > 0 {
                    (cur.0 - 1, cols - 1)
                } else {
                    return None;
                }
            }
            Step::Down => {
                let r = if cur == (row, col) { row + rs } else { cur.0 + 1 };
                if r < rows { (r, col) } else { return None }
            }
        };
        if how == Step::Next && cur.0 == row && cur.1 < col + cs {
            continue;
        }
        let a = anchor(t, cur.0, cur.1);
        if a == cur {
            return Some(cur);
        }
        if how == Step::Down {
            return Some(a);
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Next,
    Prev,
    Down,
}

// ---------------------------------------------------------------- commit

fn entity(app: &CadApp, h: Handle) -> Option<Entity> {
    app.session.doc().ok().and_then(|d| d.entity(h)).map(|e| (**e).clone())
}

/// Write the edited text back (one undo step). `true` when it was applied or unchanged.
fn commit(app: &mut CadApp, ed: &InPlace) -> bool {
    let Some(mut e) = entity(app, ed.handle) else { return false };
    let text = ed.buf.replace('\t', " ");
    let changed = match (&mut e.kind, ed.target) {
        (EntityKind::Text(t), Target::Text) => {
            let v = text.replace(['\n', '\r'], " ");
            let c = t.value != v;
            t.value = v;
            c
        }
        (EntityKind::MText(t), Target::MText) => {
            let v = text.replace("\r\n", "\n").replace('\n', "\\P");
            let c = t.contents != v;
            t.contents = v;
            c
        }
        (EntityKind::MLeader(m), Target::MLeader) => match &mut m.text {
            Some(t) => {
                let v = text.replace("\r\n", "\n").replace('\n', "\\P");
                let c = t.contents != v;
                t.contents = v;
                c
            }
            None => false,
        },
        (EntityKind::Table(t), Target::Cell { row, col }) => {
            let v = text.replace(['\n', '\r'], " ");
            match t.cells.get_mut(row).and_then(|r| r.get_mut(col)) {
                Some(cell) if cell.text != v => {
                    cell.text = v;
                    true
                }
                Some(_) => false,
                None => false,
            }
        }
        _ => false,
    };
    if !changed {
        return true;
    }
    let v = match serde_json::to_value(&e) {
        Ok(v) => v,
        Err(err) => {
            app.session.echo(err.to_string());
            return false;
        }
    };
    match app.session.execute("entity.replace", &json!({ "entity": v })) {
        Ok(_) => true,
        Err(err) => {
            app.session.echo(err.to_string());
            false
        }
    }
}

// ---------------------------------------------------------------- UI

/// Draw the editor over the canvas; handles its keys. Call after the drawing is painted.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, xf: &Xf) {
    let Some(mut ed) = app.inplace.take() else { return };
    if entity(app, ed.handle).is_none() || app.session.running.is_some() {
        return;
    }
    let t = crate::theme::Tokens::get();
    let scale = xf.scale as f32;
    let size = (ed.line_h as f32 * scale).clamp(12.0, 44.0);
    let tl = xf.to_screen(ed.origin);
    let w_px = (ed.width as f32 * scale).clamp(140.0, 900.0);
    let font = egui::FontId::proportional(size);
    let id = egui::Id::new(("vnc_inplace", ed.handle.0));
    // Keys the editor acts on, read before the text field sees them.
    let (enter, ctrl_enter, esc, tab, shift_tab) = ui.input_mut(|i| {
        let ctrl_enter = i.consume_key(Modifiers::COMMAND, Key::Enter);
        let shift_tab = i.consume_key(Modifiers::SHIFT, Key::Tab);
        let tab = i.consume_key(Modifiers::NONE, Key::Tab);
        let esc = i.consume_key(Modifiers::NONE, Key::Escape);
        let enter = !ed.multiline() && i.consume_key(Modifiers::NONE, Key::Enter);
        (enter, ctrl_enter, esc, tab, shift_tab)
    });
    let mut open_dialog = false;
    let area = egui::Area::new(id.with("area")).order(egui::Order::Foreground).fixed_pos(tl - vec2(3.0, 3.0)).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(t.canvas).stroke(Stroke::new(1.0, Color32::from_rgb(0x2f, 0x7d, 0xe1))).inner_margin(egui::Margin::same(2)).show(
            ui,
            |ui| {
                ui.horizontal_top(|ui| {
                    let rows = ed.buf.lines().count().clamp(1, 20);
                    let te = if ed.multiline() {
                        egui::TextEdit::multiline(&mut ed.buf).desired_rows(rows)
                    } else {
                        egui::TextEdit::singleline(&mut ed.buf)
                    };
                    let r =
                        ui.add(te.id(id).font(font.clone()).desired_width(w_px).text_color(Color32::WHITE).lock_focus(true).frame(egui::Frame::NONE));
                    if ed.focus_pending {
                        r.request_focus();
                        ed.focus_pending = false;
                    }
                    if ui.small_button("…").on_hover_text("Mở bảng thuộc tính đầy đủ").clicked() {
                        open_dialog = true;
                    }
                });
                let hint = match ed.target {
                    Target::Cell { .. } => "Enter: ô dưới · Tab / Shift+Tab: ô kế · Esc: thoát",
                    Target::Text => "Enter: xong · Esc: bỏ",
                    _ => "Enter: xuống dòng · Ctrl+Enter hoặc bấm ra ngoài: xong · Esc: bỏ",
                };
                ui.label(egui::RichText::new(hint).size(11.0).color(t.text_faint));
            },
        );
    });
    // A press outside the box on another panel keeps the text too (the canvas handles its own).
    let clicked_out = ui.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !ed.focus_pending;
    if esc {
        app.session.echo("Bỏ sửa chữ.");
        return;
    }
    if open_dialog {
        let h = ed.handle;
        if commit(app, &ed) {
            crate::vdlg::open_edit(app, h);
        }
        return;
    }
    // A table: move to another cell after keeping this one.
    if let Target::Cell { row, col } = ed.target
        && (enter || tab || shift_tab)
    {
        if !commit(app, &ed) {
            return;
        }
        let Some(EntityKind::Table(tb)) = entity(app, ed.handle).map(|e| e.kind) else { return };
        let how = if shift_tab {
            Step::Prev
        } else if tab {
            Step::Next
        } else {
            Step::Down
        };
        if let Some((r, c)) = step_cell(&tb, row, col, how) {
            ed.target = Target::Cell { row: r, col: c };
            place_cell(&mut ed, &tb);
            ed.focus_pending = true;
            app.inplace = Some(ed);
        }
        return;
    }
    if enter || ctrl_enter || clicked_out {
        commit(app, &ed);
        return;
    }
    app.inplace = Some(ed);
}

/// Keep the text being edited (a click on the drawing outside the box).
pub fn finish(app: &mut CadApp) {
    if let Some(ed) = app.inplace.take() {
        commit(app, &ed);
    }
}
