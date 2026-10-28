//! The floating command line: history, the active prompt with clickable keywords, typed input
//! (keyboard focus stays on the drawing — typing anywhere goes here) and AutoComplete.

use cadcraft_engine::Input;
use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

#[derive(Default)]
pub struct CmdLine {
    pub buffer: String,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub expanded: bool,
}

/// Matching command names/aliases for AutoComplete: exact aliases first (what Space/Enter
/// runs), then commands whose name or alias starts with what was typed, shortest first.
pub fn suggestions(prefix: &str) -> Vec<(String, &'static str)> {
    if prefix.is_empty() || prefix.contains(' ') || prefix.starts_with('(') || prefix.starts_with('!') {
        return Vec::new();
    }
    let p = prefix.trim_start_matches(['_', '.', '-', '\'']).to_ascii_lowercase();
    if p.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<(u8, String, &'static str)> = cadcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.id.contains('.'))
        .filter_map(|c| {
            let rank = if c.id == p || c.aliases.iter().any(|a| *a == p) {
                0
            } else if c.id.starts_with(&p) {
                1
            } else if c.aliases.iter().any(|a| a.starts_with(&p)) {
                2
            } else {
                return None;
            };
            Some((rank, c.id.to_ascii_uppercase(), c.label))
        })
        .collect();
    v.sort_by(|a, b| (a.0, a.1.len(), &a.1).cmp(&(b.0, b.1.len(), &b.1)));
    v.dedup_by(|a, b| a.1 == b.1);
    v.truncate(8);
    v.into_iter().map(|(_, id, l)| (id, l)).collect()
}

/// Handle keyboard input destined for the command line (when no text field has focus).
pub fn keyboard(app: &mut CadApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    // VNCCad: a LISP dialog takes Enter/Escape itself.
    if app.session.current_prompt().is_some_and(|p| p.dialog.is_some()) {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    // A hot grip takes Space/Enter (cycle mode, or apply a typed point) and Escape.
    if app.canvas.hot_grip.is_some() {
        for ev in &events {
            match ev {
                egui::Event::Text(t) if t != " " => app.cmd.buffer.push_str(t),
                egui::Event::Key { key: Key::Backspace, pressed: true, .. } => {
                    app.cmd.buffer.pop();
                }
                egui::Event::Key { key: Key::Escape, pressed: true, .. } => {
                    app.canvas.hot_grip = None;
                    app.canvas.hot_set.clear();
                    app.cmd.buffer.clear();
                    app.session.echo("*Cancel*");
                }
                egui::Event::Key { key: Key::Enter | Key::Space, pressed: true, .. } => {
                    let typed = std::mem::take(&mut app.cmd.buffer);
                    if typed.trim().is_empty() {
                        if let Some(g) = app.canvas.hot_grip.as_mut() {
                            g.next_mode();
                            let l = g.label();
                            app.session.echo(l);
                        }
                    } else {
                        let base = app.canvas.hot_grip.map(|g| g.base).unwrap_or_default();
                        match cadcraft_engine::prompt::parse_point(&typed, base) {
                            Some(p) => crate::canvas::apply_hot_grip(app, p),
                            None => app.session.echo("Requires a point (x,y, @dx,dy or @d<a)."),
                        }
                    }
                }
                _ => {}
            }
        }
        return;
    }
    let text_prompt = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number);
    // VNCCad: some keyboards and browsers (IME active, the web text agent) deliver Space only as a
    // key press, without the matching text event; it must still act as Enter.
    let space_as_text = events.iter().any(|e| matches!(e, egui::Event::Text(t) if t.contains(' ')));
    for ev in events {
        match ev {
            egui::Event::Key { key: Key::Space, pressed: true, modifiers, .. } if !space_as_text && !modifiers.any() => {
                if !text_prompt && !lisp_open(&app.cmd.buffer) {
                    submit(app);
                } else {
                    app.cmd.buffer.push(' ');
                }
            }
            egui::Event::Text(t) => {
                if t == " " && !text_prompt && !lisp_open(&app.cmd.buffer) {
                    submit(app);
                } else {
                    app.cmd.buffer.push_str(&t);
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                Key::Enter => submit(app),
                Key::Escape => {
                    app.cmd.buffer.clear();
                    app.session.cancel();
                }
                Key::Backspace => {
                    app.cmd.buffer.pop();
                }
                Key::ArrowUp if !modifiers.any() => {
                    if !app.cmd.history.is_empty() {
                        let n = app.cmd.history.len();
                        let pos = app.cmd.history_pos.map(|p| p.saturating_sub(1)).unwrap_or(n - 1);
                        app.cmd.history_pos = Some(pos);
                        app.cmd.buffer = app.cmd.history.get(pos).cloned().unwrap_or_default();
                    }
                }
                Key::ArrowDown if !modifiers.any() => {
                    if let Some(p) = app.cmd.history_pos {
                        let np = p + 1;
                        if np >= app.cmd.history.len() {
                            app.cmd.history_pos = None;
                            app.cmd.buffer.clear();
                        } else {
                            app.cmd.history_pos = Some(np);
                            app.cmd.buffer = app.cmd.history.get(np).cloned().unwrap_or_default();
                        }
                    }
                }
                Key::Tab => {
                    if let Some((id, _)) = suggestions(&app.cmd.buffer).first() {
                        app.cmd.buffer = id.clone();
                    }
                }
                Key::Delete if app.cmd.buffer.is_empty() && app.session.running.is_none() => {
                    let _ = app.run("erase.selection", serde_json::json!({}));
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// VNCCad: an unfinished LISP expression (`(setq a` …): Space is part of it, not Enter.
fn lisp_open(buf: &str) -> bool {
    let t = buf.trim_start();
    if !t.starts_with('(') {
        return false;
    }
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for c in t.chars() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
    }
    depth > 0 || in_str
}

pub fn submit(app: &mut CadApp) {
    let text = std::mem::take(&mut app.cmd.buffer);
    app.cmd.history_pos = None;
    if !text.trim().is_empty() && app.session.running.is_none() {
        app.cmd.history.push(text.clone());
        if app.cmd.history.len() > 200 {
            app.cmd.history.remove(0);
        }
    }
    if text.is_empty() {
        if let Err(e) = app.session.input(Input::Enter) {
            app.session.echo(e.to_string());
        }
        return;
    }
    let text = complete_first_word(app, text);
    app.cmdline(&text);
}

/// VNCCad: like AutoCAD's AutoComplete, an unknown command name typed at `Command:` runs the
/// highlighted suggestion (`RECT` → RECTANG). Known names, aliases and LISP commands are left alone.
fn complete_first_word(app: &CadApp, text: String) -> String {
    if app.session.running.is_some() {
        return text;
    }
    let t = text.trim();
    if t.contains(' ') || t.starts_with('(') || t.starts_with('!') || t.contains('{') {
        return text;
    }
    let name = t.trim_start_matches(['_', '.', '-', '\'']).to_ascii_lowercase();
    let known = cadcraft_engine::cmd::find_command(&cadcraft_engine::cmd::resolve_alias(&name)).is_some()
        || app.session.user_aliases.contains_key(&name)
        || app.session.lisp.has_command(&name)
        || crate::menus::UI_COMMANDS.iter().any(|c| c.0 == name)
        || cadcraft_engine::cmd::round3::detelex(&name).is_some();
    if known {
        return text;
    }
    match suggestions(t).first() {
        Some((id, _)) => {
            let prefix: String = t.chars().take_while(|c| matches!(c, '_' | '.' | '-' | '\'')).collect();
            format!("{prefix}{id}")
        }
        None => text,
    }
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    let t = Tokens::get();
    keyboard(app, ui.ctx());
    let w = (canvas.width() * 0.48).clamp(360.0, 760.0);
    let h = 24.0;
    let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - h - 10.0), vec2(w, h));
    let p = ui.painter_at(canvas);
    // History lines above the bar.
    let n = app.ui.history_lines.min(12);
    let lines: Vec<String> = app.session.log.iter().rev().take(n).rev().cloned().collect();
    if !lines.is_empty() {
        let lh = 16.0;
        let hr = Rect::from_min_size(pos2(bar.left(), bar.top() - lh * lines.len() as f32 - 4.0), vec2(w, lh * lines.len() as f32 + 2.0));
        p.rect_filled(hr, 3.0, Color32::from_rgba_unmultiplied(0x2a, 0x30, 0x3a, 170));
        for (i, l) in lines.iter().enumerate() {
            p.text(pos2(hr.left() + 8.0, hr.top() + 1.0 + lh * i as f32), egui::Align2::LEFT_TOP, l, crate::theme::small(), t.text_dim);
        }
    }
    p.rect_filled(bar, 3.0, t.cmd_bg);
    p.rect_stroke(bar, 3.0, Stroke::new(1.0, Color32::from_rgb(0x55, 0x5f, 0x70)), egui::StrokeKind::Inside);
    // Prompt glyph.
    p.text(pos2(bar.left() + 8.0, bar.center().y), egui::Align2::LEFT_CENTER, ">_", crate::theme::mono(), t.text_dim);
    let mut x = bar.left() + 30.0;
    let prompt = app.session.current_prompt();
    let font = crate::theme::body();
    if let Some(g) = app.canvas.hot_grip {
        let gl = p.layout_no_wrap(g.label().to_string(), font.clone(), t.text);
        let gw = gl.size().x;
        p.galley(pos2(x, bar.center().y - gl.size().y / 2.0), gl, t.text);
        x += gw + 6.0;
    } else {
        match &prompt {
            Some(pr) => {
                let name = app.session.running.as_ref().map(|r| r.id.to_ascii_uppercase()).unwrap_or_default();
                let g = p.layout_no_wrap(format!("{name} "), font.clone(), t.text_faint);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text_faint);
                x += gw;
                let g = p.layout_no_wrap(pr.message.clone(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
                if !pr.keywords.is_empty() {
                    let g = p.layout_no_wrap(if pr.message.is_empty() { " [".into() } else { " or [".into() }, font.clone(), t.text);
                    let gw = g.size().x;
                    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                    x += gw;
                    let kws = pr.keywords.clone();
                    for (i, k) in kws.iter().enumerate() {
                        let g = p.layout_no_wrap(k.clone(), font.clone(), Color32::from_rgb(0x8f, 0xc1, 0xff));
                        let r = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(g.size().x, h - 6.0));
                        let resp = ui.interact(r, ui.id().with(("kw", i)), Sense::click());
                        if resp.hovered() {
                            p.rect_filled(r, 2.0, Color32::from_rgb(0x2f, 0x5e, 0xa8));
                        }
                        let gw = g.size().x;
                        p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, Color32::from_rgb(0x8f, 0xc1, 0xff));
                        x += gw;
                        if resp.clicked() {
                            let _ = app.session.input(Input::Keyword(k.clone()));
                        }
                        if i + 1 < kws.len() {
                            let g = p.layout_no_wrap("/".into(), font.clone(), t.text);
                            let gw = g.size().x;
                            p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                            x += gw;
                        }
                    }
                    let g = p.layout_no_wrap("]".into(), font.clone(), t.text);
                    let gw = g.size().x;
                    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                    x += gw;
                }
                if let Some(d) = &pr.default {
                    let g = p.layout_no_wrap(format!(" <{d}>"), font.clone(), t.text);
                    let gw = g.size().x;
                    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                    x += gw;
                }
                let g = p.layout_no_wrap(": ".into(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            }
            None => {
                if app.cmd.buffer.is_empty() {
                    p.text(pos2(x, bar.center().y), egui::Align2::LEFT_CENTER, "Type a command", egui::FontId::proportional(12.5), t.text_faint);
                }
            }
        }
    }
    let g = p.layout_no_wrap(app.cmd.buffer.clone(), font, t.text);
    let gw = g.size().x;
    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
    // Caret.
    let blink = (ui.input(|i| i.time) * 2.0).floor() as i64 % 2 == 0;
    if blink {
        p.line_segment([pos2(x + gw + 1.0, bar.top() + 5.0), pos2(x + gw + 1.0, bar.bottom() - 5.0)], Stroke::new(1.0, t.text));
    }
    // History chevron.
    crate::icons::paint(
        &p,
        Rect::from_center_size(pos2(bar.right() - 14.0, bar.center().y), vec2(14.0, 14.0)),
        crate::icons::Icon::ChevronDown,
        false,
    );
    // AutoComplete list.
    if app.session.running.is_none() {
        let sug = suggestions(&app.cmd.buffer);
        if !sug.is_empty() {
            let rh = 20.0;
            let lr = Rect::from_min_size(
                pos2(
                    bar.left() + 24.0,
                    bar.top() - 6.0 - rh * sug.len() as f32 - (if lines.is_empty() { 0.0 } else { 16.0 * lines.len() as f32 + 6.0 }),
                ),
                vec2(300.0, rh * sug.len() as f32),
            );
            p.rect_filled(lr, 3.0, Color32::from_rgb(0x2b, 0x31, 0x3b));
            p.rect_stroke(lr, 3.0, Stroke::new(1.0, Color32::from_rgb(0x55, 0x5f, 0x70)), egui::StrokeKind::Inside);
            for (i, (id, label)) in sug.iter().enumerate() {
                let r = Rect::from_min_size(pos2(lr.left(), lr.top() + rh * i as f32), vec2(lr.width(), rh));
                let resp = ui.interact(r, ui.id().with(("sug", i)), Sense::click());
                if resp.hovered() || i == 0 {
                    p.rect_filled(
                        r.shrink(1.0),
                        2.0,
                        if resp.hovered() { Color32::from_rgb(0x2f, 0x5e, 0xa8) } else { Color32::from_rgb(0x3a, 0x42, 0x50) },
                    );
                }
                p.text(Pos2::new(r.left() + 8.0, r.center().y), egui::Align2::LEFT_CENTER, id, crate::theme::body(), t.text);
                p.text(Pos2::new(r.right() - 8.0, r.center().y), egui::Align2::RIGHT_CENTER, *label, crate::theme::small(), t.text_faint);
                if resp.clicked() {
                    app.cmd.buffer.clear();
                    app.start(&id.to_ascii_lowercase());
                }
            }
        }
    }
}
