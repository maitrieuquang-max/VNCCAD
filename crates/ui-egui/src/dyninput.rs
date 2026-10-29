//! VNCCad: Dynamic Input fields (F12) at point prompts that have a base point.
//!
//! Two fields follow the cursor: distance and angle (LINE, PLINE, MOVE…) or, for RECTANG's
//! other corner, length and width. Typing goes into the active field; **Tab** locks it and moves
//! to the other one (the locked value then constrains the rubber band); Enter or Space gives the
//! point. A plain number with Enter and nothing locked stays the classic direct distance entry.

use cadcraft_geom::Vec2;
use egui::{Color32, Pos2, Rect, Stroke, vec2};

use crate::CadApp;

#[derive(Default)]
pub struct DynFields {
    /// Locked values: [distance | length, angle (degrees) | width].
    pub locked: [Option<f64>; 2],
    /// The field typing goes to (0 or 1).
    pub active: usize,
    /// The prompt the fields belong to.
    owner: String,
}

/// The prompt Dynamic Input works on: a point prompt with a base, Dynamic Input on.
pub fn applies(app: &CadApp) -> Option<(Vec2, bool)> {
    if !app.session.settings.dynmode || app.session.running.is_none() {
        return None;
    }
    let p = app.session.current_prompt()?;
    if !p.accept.point || p.accept.select || p.picks_object() {
        return None;
    }
    Some((p.base?, p.dyn_rect))
}

fn owner_key(app: &CadApp) -> String {
    let id = app.session.running.as_ref().map(|r| r.id.clone()).unwrap_or_default();
    let p = app.session.current_prompt();
    let base = p.as_ref().and_then(|p| p.base).map(|b| format!("{:.9},{:.9}", b.x, b.y)).unwrap_or_default();
    format!("{id}|{}|{base}", p.map(|p| p.message).unwrap_or_default())
}

/// Clear locked values when the prompt moves on.
pub fn sync(app: &mut CadApp) {
    let k = owner_key(app);
    if app.cmd.dynf.owner != k {
        app.cmd.dynf = DynFields { owner: k, ..Default::default() };
    }
}

fn number(s: &str) -> Option<f64> {
    let t = s.trim().replace(',', ".");
    cadcraft_engine::units::parse_distance(&t).or_else(|| t.parse::<f64>().ok()).filter(|v| v.is_finite())
}

/// Tab at the command line: lock the active field and move to the next. `false` when Dynamic
/// Input does not apply here (Tab then does AutoComplete).
pub fn tab(app: &mut CadApp) -> bool {
    if applies(app).is_none() {
        return false;
    }
    sync(app);
    let typed = app.cmd.buffer.trim().to_string();
    if !typed.is_empty() {
        let Some(v) = number(&typed) else {
            app.session.echo("Ô nhập động chỉ nhận số.");
            return true;
        };
        let i = app.cmd.dynf.active.min(1);
        if let Some(slot) = app.cmd.dynf.locked.get_mut(i) {
            *slot = Some(v);
        }
        app.cmd.buffer.clear();
    }
    app.cmd.dynf.active = 1 - app.cmd.dynf.active.min(1);
    true
}

/// The values the fields give now: (field 0, field 1), each from a lock or the typed text.
fn values(app: &CadApp) -> [Option<f64>; 2] {
    let mut v = app.cmd.dynf.locked;
    let typed = number(&app.cmd.buffer);
    if let Some(slot) = v.get_mut(app.cmd.dynf.active.min(1))
        && typed.is_some()
    {
        *slot = typed;
    }
    v
}

/// Enter/Space: the point text the fields describe, when the user used them (a lock, or typing
/// in the second field). `None` leaves the typed text to the normal command line.
pub fn compose(app: &mut CadApp) -> Option<String> {
    let (base, rect) = applies(app)?;
    sync(app);
    let used = app.cmd.dynf.locked.iter().any(Option::is_some) || (app.cmd.dynf.active == 1 && !app.cmd.buffer.trim().is_empty());
    if !used {
        return None;
    }
    if !app.cmd.buffer.trim().is_empty() && number(&app.cmd.buffer).is_none() {
        return None;
    }
    let [a, b] = values(app);
    let ucs = app.session.ucs();
    let c = ucs.to_ucs(app.canvas.cursor.unwrap_or(base));
    let o = ucs.to_ucs(base);
    let d = c - o;
    let text = if rect {
        let sx = if d.x < 0.0 { -1.0 } else { 1.0 };
        let sy = if d.y < 0.0 { -1.0 } else { 1.0 };
        let l = a.map(f64::abs).unwrap_or(d.x.abs());
        let w = b.map(f64::abs).unwrap_or(d.y.abs());
        format!("@{},{}", sx * l, sy * w)
    } else {
        let dist = a.unwrap_or(d.len());
        let ang = b.unwrap_or(d.angle().to_degrees());
        format!("@{dist}<{ang}")
    };
    app.cmd.dynf = DynFields::default();
    app.cmd.buffer.clear();
    Some(text)
}

/// Keep the rubber band on the locked values.
pub fn constrain(app: &CadApp, p: Vec2) -> Vec2 {
    let Some((base, rect)) = applies(app) else { return p };
    let [a, b] = app.cmd.dynf.locked;
    if a.is_none() && b.is_none() {
        return p;
    }
    let ucs = app.session.ucs();
    let o = ucs.to_ucs(base);
    let d = ucs.to_ucs(p) - o;
    let q = if rect {
        let x = a.map(|l| if d.x < 0.0 { -l.abs() } else { l.abs() }).unwrap_or(d.x);
        let y = b.map(|w| if d.y < 0.0 { -w.abs() } else { w.abs() }).unwrap_or(d.y);
        o + Vec2::new(x, y)
    } else {
        let dist = a.unwrap_or(d.len());
        let ang = b.map(f64::to_radians).unwrap_or(d.angle());
        o + Vec2::from_angle(ang) * dist
    };
    ucs.to_world(q)
}

/// The two fields beside the cursor.
pub fn draw(app: &CadApp, p: &egui::Painter, at: Pos2) -> bool {
    let Some((base, rect)) = applies(app) else { return false };
    if app.cmd.dynf.owner != owner_key(app) && app.cmd.dynf.locked.iter().any(Option::is_some) {
        return false;
    }
    let ucs = app.session.ucs();
    let c = app.canvas.cursor.unwrap_or(base);
    let d = ucs.to_ucs(c) - ucs.to_ucs(base);
    let live = if rect { [d.x.abs(), d.y.abs()] } else { [d.len(), (d.angle().to_degrees().rem_euclid(360.0) * 10.0).round() / 10.0] };
    let names = if rect { ["Dài", "Rộng"] } else { ["Khoảng cách", "Góc"] };
    let font = crate::theme::small();
    let mut x = at.x + 18.0;
    let y = at.y + 14.0;
    for i in 0..2 {
        let active = app.cmd.dynf.active.min(1) == i;
        let locked = app.cmd.dynf.locked.get(i).copied().flatten();
        let value = if active && !app.cmd.buffer.is_empty() {
            app.cmd.buffer.clone()
        } else if let Some(v) = locked {
            format!("{} [khóa]", cadcraft_engine::units::format_distance(v, 2, 4))
        } else {
            let v = live.get(i).copied().unwrap_or(0.0);
            if i == 1 && !rect { format!("{v}°") } else { cadcraft_engine::units::format_distance(v, 2, 4) }
        };
        let text = format!("{}: {}", names.get(i).copied().unwrap_or(""), value);
        let g = p.layout_no_wrap(text, font.clone(), Color32::BLACK);
        let r = Rect::from_min_size(Pos2::new(x, y), g.size() + vec2(10.0, 6.0));
        let bg = if active { Color32::from_rgb(0xff, 0xff, 0xff) } else { Color32::from_rgb(0xd8, 0xdc, 0xe2) };
        p.rect_filled(r, 2.0, bg);
        let border = if active { Color32::from_rgb(0x2f, 0x7d, 0xe1) } else { Color32::from_rgb(0x8a, 0x93, 0xa0) };
        p.rect_stroke(r, 2.0, Stroke::new(if active { 1.5 } else { 1.0 }, border), egui::StrokeKind::Inside);
        p.galley(r.min + vec2(5.0, 3.0), g, Color32::BLACK);
        x = r.right() + 6.0;
    }
    let hint = p.layout_no_wrap("Tab: chuyển ô".into(), crate::theme::small(), Color32::from_rgb(0xc8, 0xcc, 0xd2));
    p.galley(Pos2::new(x, y + 3.0), hint, Color32::from_rgb(0xc8, 0xcc, 0xd2));
    true
}
