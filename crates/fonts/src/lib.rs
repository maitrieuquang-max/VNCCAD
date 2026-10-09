//! CADCraft text layout.
//!
//! Turns TEXT and MTEXT into stroke polylines with the built-in single-stroke font
//! ("CADCraft Stroke"). Handles the `%%` control codes, alignment modes, width factor,
//! obliquing, MTEXT inline formatting (paragraphs, stacking, height changes), word wrap and
//! attachment points.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod mtext;
pub mod shx;
mod stroke;
pub mod ttf;

use std::sync::Arc;

use cadcraft_geom::{Bounds2, Vec2};

pub use mtext::{MTextColor, MTextLayout, MTextParams, MTextPiece, layout_mtext, layout_mtext_with, plain_mtext};

/// Name of the built-in font.
pub const BUILTIN_FONT: &str = "CADCraft Stroke";

/// One laid-out run of text: strokes in local coordinates (baseline at y = 0, x from 0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub strokes: Vec<Vec<Vec2>>,
    pub width: f64,
}

/// Replace `%%` control codes: `%%d` degree, `%%p` plus/minus, `%%c` diameter, `%%%` percent,
/// `%%nnn` character code. Underline/overline toggles (`%%u`, `%%o`) are returned as flags
/// per character.
pub fn decode_controls(s: &str) -> Vec<(char, bool, bool)> {
    let mut out = Vec::with_capacity(s.len());
    let mut under = false;
    let mut over = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars.get(i).copied().unwrap_or(' ');
        if c == '%' && chars.get(i + 1) == Some(&'%') {
            let code = chars.get(i + 2).copied().unwrap_or(' ');
            match code.to_ascii_lowercase() {
                'd' => out.push(('°', under, over)),
                'p' => out.push(('±', under, over)),
                'c' => out.push(('⌀', under, over)),
                '%' => out.push(('%', under, over)),
                'u' => under = !under,
                'o' => over = !over,
                d if d.is_ascii_digit() => {
                    let digits: String = chars.iter().skip(i + 2).take(3).take_while(|c| c.is_ascii_digit()).collect();
                    let n: u32 = digits.parse().unwrap_or(32);
                    out.push((char::from_u32(n).unwrap_or('?'), under, over));
                    i += 2 + digits.len();
                    continue;
                }
                _ => {
                    out.push(('%', under, over));
                    i += 1;
                    continue;
                }
            }
            i += 3;
            continue;
        }
        out.push((c, under, over));
        i += 1;
    }
    out
}

/// Advance width of one character at height 1 and width factor 1.
pub fn char_advance(c: char) -> f64 {
    match stroke::glyph_strokes(c) {
        Some((w, _)) => (w + stroke::GAP) / stroke::CAP,
        None if c.is_whitespace() => (2.4 + stroke::GAP) / stroke::CAP,
        None => (4.0 + stroke::GAP) / stroke::CAP,
    }
}

/// Lay out a single line at `height`, with `width_factor` and `oblique` (radians).
pub fn layout_line(s: &str, height: f64, width_factor: f64, oblique: f64) -> Run {
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let sc = h / stroke::CAP;
    let shear = oblique.tan().clamp(-10.0, 10.0);
    let mut run = Run::default();
    let mut x = 0.0;
    let mut under_start: Option<f64> = None;
    let mut over_start: Option<f64> = None;
    for (c, under, over) in decode_controls(s) {
        let adv = char_advance(c) * h * wf;
        match stroke::glyph_strokes(c) {
            Some((_, strokes)) => {
                for st in strokes {
                    let pts: Vec<Vec2> = st.iter().map(|(gx, gy)| Vec2::new(x + (gx * sc * wf) + gy * sc * shear, gy * sc)).collect();
                    if pts.len() == 1 {
                        if let Some(p) = pts.first() {
                            run.strokes.push(vec![*p, *p + Vec2::new(sc * 0.2, 0.0)]);
                        }
                    } else {
                        run.strokes.push(pts);
                    }
                }
            }
            None if !c.is_whitespace() => {
                // Unknown glyph: a small box, like a missing-glyph rectangle.
                let w = 4.0 * sc * wf;
                run.strokes.push(vec![Vec2::new(x, 0.0), Vec2::new(x + w, 0.0), Vec2::new(x + w, h), Vec2::new(x, h), Vec2::new(x, 0.0)]);
            }
            None => {}
        }
        // Decorations.
        match (under, under_start) {
            (true, None) => under_start = Some(x),
            (false, Some(sx)) => {
                run.strokes.push(vec![Vec2::new(sx, -h * 0.2), Vec2::new(x, -h * 0.2)]);
                under_start = None;
            }
            _ => {}
        }
        match (over, over_start) {
            (true, None) => over_start = Some(x),
            (false, Some(sx)) => {
                run.strokes.push(vec![Vec2::new(sx, h * 1.2), Vec2::new(x, h * 1.2)]);
                over_start = None;
            }
            _ => {}
        }
        x += adv;
    }
    let end = (x - stroke::GAP * sc * wf).max(0.0);
    if let Some(sx) = under_start {
        run.strokes.push(vec![Vec2::new(sx, -h * 0.2), Vec2::new(end, -h * 0.2)]);
    }
    if let Some(sx) = over_start {
        run.strokes.push(vec![Vec2::new(sx, h * 1.2), Vec2::new(end, h * 1.2)]);
    }
    run.width = end;
    run
}

/// Width of a line without building strokes.
pub fn line_width(s: &str, height: f64, width_factor: f64) -> f64 {
    let n: f64 = decode_controls(s).iter().map(|(c, _, _)| char_advance(*c)).sum();
    (n * height * width_factor - stroke::GAP / stroke::CAP * height * width_factor).max(0.0)
}

/// The font a piece of text is set in: the built-in stroke font or an installed TrueType /
/// OpenType font (outlines).
#[derive(Clone, Debug, Default)]
pub enum TextFont {
    #[default]
    Stroke,
    Outline(Arc<Vec<u8>>),
    /// A compiled shape font (`.shx`) found on this computer or registered at run time.
    Shx(Arc<shx::ShxFont>),
}

impl TextFont {
    /// Resolve a text style font name: an installed TTF/OTF when found, else the stroke font.
    pub fn resolve(name: &str) -> TextFont {
        let lower = name.trim().to_ascii_lowercase();
        if lower.ends_with(".shx") {
            return shx::find(&lower).map(TextFont::Shx).unwrap_or(TextFont::Stroke);
        }
        if let Some(b) = ttf::find(name) {
            return TextFont::Outline(b);
        }
        // A bare name ("romans", "txt") may be an SHX font.
        shx::find(&lower).map(TextFont::Shx).unwrap_or(TextFont::Stroke)
    }
    pub fn is_outline(&self) -> bool {
        matches!(self, TextFont::Outline(_))
    }
}

/// Shaped text in local or world coordinates: open strokes (stroke-font glyphs, underlines,
/// fraction bars) and closed glyph outlines grouped per glyph (fill each group even-odd).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shaped {
    pub strokes: Vec<Vec<Vec2>>,
    pub glyphs: Vec<Vec<Vec<Vec2>>>,
    pub width: f64,
}

impl Shaped {
    /// Apply a point transform to every vertex.
    pub fn map(&mut self, f: impl Fn(Vec2) -> Vec2) {
        for s in &mut self.strokes {
            for p in s.iter_mut() {
                *p = f(*p);
            }
        }
        for g in &mut self.glyphs {
            for c in g.iter_mut() {
                for p in c.iter_mut() {
                    *p = f(*p);
                }
            }
        }
    }
    /// Append another shaped piece (already in the same coordinates).
    pub fn extend(&mut self, other: Shaped) {
        self.strokes.extend(other.strokes);
        self.glyphs.extend(other.glyphs);
    }
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.glyphs.is_empty()
    }
    /// All geometry as polylines (outlines become closed polylines).
    pub fn outlines(&self) -> Vec<Vec<Vec2>> {
        let mut v = self.strokes.clone();
        v.extend(self.glyphs.iter().flatten().cloned());
        v
    }
}

/// Underline / overline strokes from per-character spans `(x0, x1, underline, overline)`.
pub(crate) fn decorations(spans: &[(f64, f64, bool, bool)], h: f64) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    for (which, y) in [(0usize, -h * 0.2), (1usize, h * 1.2)] {
        let mut start: Option<f64> = None;
        let mut end = 0.0;
        for (x0, x1, u, o) in spans {
            let on = if which == 0 { *u } else { *o };
            match (on, start) {
                (true, None) => {
                    start = Some(*x0);
                    end = *x1;
                }
                (true, Some(_)) => end = *x1,
                (false, Some(sx)) => {
                    out.push(vec![Vec2::new(sx, y), Vec2::new(end, y)]);
                    start = None;
                }
                (false, None) => {}
            }
        }
        if let Some(sx) = start {
            out.push(vec![Vec2::new(sx, y), Vec2::new(end, y)]);
        }
    }
    out
}

/// Shape one line in a single font (no fallback).
fn shape_one(font: &TextFont, s: &str, height: f64, width_factor: f64, oblique: f64) -> Shaped {
    if let TextFont::Outline(bytes) = font
        && let Some(sh) = ttf::shape(bytes, s, height, width_factor, oblique)
    {
        return sh;
    }
    if let TextFont::Shx(f) = font {
        return shx::shape(f, s, height, width_factor, oblique);
    }
    let run = layout_line(s, height, width_factor, oblique);
    Shaped { strokes: run.strokes, glyphs: Vec::new(), width: run.width }
}

/// Whether `font` draws `c` itself (SHX fonts also draw what the stroke font has).
fn covers(font: &TextFont, c: char) -> bool {
    if c.is_whitespace() || c.is_control() {
        return true;
    }
    match font {
        TextFont::Stroke => stroke::glyph_strokes(c).is_some(),
        TextFont::Shx(f) => f.has(c) || stroke::glyph_strokes(c).is_some() || (c == '⌀' && stroke::glyph_strokes('Ø').is_some()),
        TextFont::Outline(b) => ttf::has(b, c),
    }
}

/// Re-encode decoded characters as text with `%%` codes, so a piece shapes exactly as it did
/// inside the whole line.
fn encode(chars: &[(char, bool, bool)]) -> String {
    let mut out = String::new();
    let (mut u, mut o) = (false, false);
    for &(c, under, over) in chars {
        if under != u {
            out.push_str("%%u");
            u = under;
        }
        if over != o {
            out.push_str("%%o");
            o = over;
        }
        if c == '%' {
            out.push_str("%%%");
        } else {
            out.push(c);
        }
    }
    out
}

/// Split a line into runs of (uses fallback font, characters).
fn pieces(font: &TextFont, s: &str) -> Option<(Arc<Vec<u8>>, Vec<(bool, Vec<(char, bool, bool)>)>)> {
    let chars = decode_controls(s);
    if chars.iter().all(|(c, _, _)| covers(font, *c)) {
        return None;
    }
    let fb = ttf::fallback_cjk()?;
    let mut out: Vec<(bool, Vec<(char, bool, bool)>)> = Vec::new();
    for ch in chars {
        let alt = !covers(font, ch.0) && ttf::has(&fb, ch.0);
        match out.last_mut() {
            Some((a, v)) if *a == alt => v.push(ch),
            _ => out.push((alt, vec![ch])),
        }
    }
    Some((fb, out))
}

/// Shape one line in `font` (baseline at y = 0, x from 0). Characters the font lacks (Chinese,
/// Japanese, Korean… when the drawing's big font is missing) are drawn with a system font that
/// has them. TrueType fonts that fail to parse fall back to the stroke font.
pub fn shape_line(font: &TextFont, s: &str, height: f64, width_factor: f64, oblique: f64) -> Shaped {
    let Some((fb, parts)) = pieces(font, s) else { return shape_one(font, s, height, width_factor, oblique) };
    let mut out = Shaped::default();
    let mut x = 0.0;
    for (alt, chars) in parts {
        let text = encode(&chars);
        let mut sh = if alt {
            ttf::shape_fallback(&fb, &text, height, width_factor, oblique).unwrap_or_else(|| shape_one(font, &text, height, width_factor, oblique))
        } else {
            shape_one(font, &text, height, width_factor, oblique)
        };
        let w = sh.width;
        sh.map(|p| Vec2::new(p.x + x, p.y));
        out.extend(sh);
        x += w;
    }
    out.width = x;
    out
}

/// Width of one line in `font`.
pub fn text_width(font: &TextFont, s: &str, height: f64, width_factor: f64) -> f64 {
    if pieces(font, s).is_some() {
        return shape_line(font, s, height, width_factor, 0.0).width;
    }
    if let TextFont::Outline(bytes) = font
        && let Some(w) = ttf::width(bytes, s, height, width_factor)
    {
        return w;
    }
    if let TextFont::Shx(f) = font {
        return shx::width(f, s, height, width_factor);
    }
    line_width(s, height, width_factor)
}

/// Horizontal / vertical alignment for single-line text (matches DXF 72/73 semantics).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    /// Fit between two points, scaling height.
    Aligned,
    /// Centred horizontally and vertically.
    Middle,
    /// Fit between two points, scaling width.
    Fit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VAlign {
    #[default]
    Baseline,
    Bottom,
    Middle,
    Top,
}

/// Single-line text placement: insertion, alignment, rotation and the text style's
/// generation flags. Shared by the stroke and TrueType fonts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextParams {
    pub insert: Vec2,
    pub align_pt: Option<Vec2>,
    pub height: f64,
    pub rotation: f64,
    pub width_factor: f64,
    pub oblique: f64,
    pub h: Align,
    pub v: VAlign,
    /// Mirrored in X (text style "Backwards").
    pub backwards: bool,
    /// Mirrored in Y (text style "Upside down").
    pub upside_down: bool,
}

impl TextParams {
    pub fn new(insert: Vec2, height: f64) -> Self {
        TextParams {
            insert,
            align_pt: None,
            height,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            h: Align::Left,
            v: VAlign::Baseline,
            backwards: false,
            upside_down: false,
        }
    }
    /// Centred on `p` both ways (dimension and table text).
    pub fn centered(p: Vec2, height: f64, rotation: f64) -> Self {
        TextParams { align_pt: Some(p), rotation, h: Align::Middle, v: VAlign::Middle, ..TextParams::new(p, height) }
    }
}

/// Place a single-line TEXT in world coordinates with any font. Returns the shaped text and its
/// bounding box.
pub fn place(font: &TextFont, s: &str, p: &TextParams) -> (Shaped, Bounds2) {
    let mut height = if p.height > 0.0 && p.height.is_finite() { p.height } else { 1.0 };
    let mut wf = p.width_factor;
    let mut rot = if p.rotation.is_finite() { p.rotation } else { 0.0 };
    let insert = p.insert;
    if matches!(p.h, Align::Aligned | Align::Fit)
        && let Some(p2) = p.align_pt
    {
        let len = insert.dist(p2);
        rot = insert.angle_to(p2);
        let w = text_width(font, s, height, wf);
        if w > 1e-12 && len > 1e-12 {
            let k = len / w;
            if p.h == Align::Aligned {
                height *= k;
            } else {
                wf *= k;
            }
        }
    }
    let mut sh = shape_line(font, s, height, wf, p.oblique);
    let width = sh.width;
    let origin = match p.h {
        Align::Left | Align::Aligned | Align::Fit => insert,
        _ => p.align_pt.unwrap_or(insert),
    };
    let dx = match p.h {
        Align::Left | Align::Aligned | Align::Fit => 0.0,
        Align::Center | Align::Middle => -width / 2.0,
        Align::Right => -width,
    };
    let dy = match (p.h, p.v) {
        (Align::Middle, _) => -height / 2.0,
        (_, VAlign::Baseline) => 0.0,
        (_, VAlign::Bottom) => height / 3.0,
        (_, VAlign::Middle) => -height / 2.0,
        (_, VAlign::Top) => -height,
    };
    let local = Vec2::new(dx, dy);
    let (mx, my) = (if p.backwards { -1.0 } else { 1.0 }, if p.upside_down { -1.0 } else { 1.0 });
    let xf = |q: Vec2| {
        let l = q + local;
        origin + Vec2::new(l.x * mx, l.y * my).rotate(rot)
    };
    sh.map(xf);
    let bb = Bounds2::from_points(
        [Vec2::new(0.0, -height / 3.0), Vec2::new(width, -height / 3.0), Vec2::new(width, height), Vec2::new(0.0, height)].map(xf),
    );
    (sh, bb)
}

/// Place a single-line TEXT in world coordinates with the stroke font. Returns strokes and the
/// text's bounding box.
#[allow(clippy::too_many_arguments)]
pub fn place_text(
    s: &str,
    insert: Vec2,
    align_pt: Option<Vec2>,
    height: f64,
    rotation: f64,
    width_factor: f64,
    oblique: f64,
    h: Align,
    v: VAlign,
) -> (Vec<Vec<Vec2>>, Bounds2) {
    let p = TextParams { insert, align_pt, height, rotation, width_factor, oblique, h, v, backwards: false, upside_down: false };
    let (sh, bb) = place(&TextFont::Stroke, s, &p);
    (sh.strokes, bb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ascii_printable_has_a_glyph() {
        for c in ' '..='~' {
            assert!(stroke::glyph(c).is_some(), "missing glyph {c:?}");
        }
    }

    #[test]
    fn glyph_points_stay_in_grid() {
        for c in ' '..='~' {
            let (w, spec) = stroke::glyph(c).unwrap();
            for st in stroke::strokes(spec) {
                for (x, y) in st {
                    assert!(x >= 0.0 && x <= w, "{c:?} x={x} w={w}");
                    assert!((-2.0..=6.0).contains(&y), "{c:?} y={y}");
                }
            }
        }
    }

    #[test]
    fn every_vietnamese_letter_has_a_glyph() {
        let all = "aàáảãạăằắẳẵặâầấẩẫậeèéẻẽẹêềếểễệiìíỉĩịoòóỏõọôồốổỗộơờớởỡợuùúủũụưừứửữựyỳýỷỹỵđ";
        for c in all.chars().chain(all.chars().flat_map(char::to_uppercase)) {
            let g = stroke::glyph_strokes(c);
            assert!(g.is_some(), "missing Vietnamese glyph {c:?}");
        }
        let plain = layout_line("Cau Bach Dang", 1.0, 1.0, 0.0).strokes.len();
        let vn = layout_line("Cầu Bạch Đằng", 1.0, 1.0, 0.0).strokes.len();
        assert!(vn > plain, "marks add strokes");
    }

    #[test]
    fn control_codes() {
        let d: String = decode_controls("45%%d %%p0.1 %%c10 100%%%").iter().map(|x| x.0).collect();
        assert_eq!(d, "45° ±0.1 ⌀10 100%");
        let u = decode_controls("%%uab%%uc");
        assert!(u[0].1 && u[1].1 && !u[2].1);
        assert_eq!(decode_controls("%%065")[0].0, 'A');
        assert_eq!(decode_controls("%%").len(), 2);
    }

    #[test]
    fn line_metrics_scale_with_height() {
        let a = layout_line("HELLO", 1.0, 1.0, 0.0);
        let b = layout_line("HELLO", 2.0, 1.0, 0.0);
        assert!((b.width - 2.0 * a.width).abs() < 1e-9);
        assert!((line_width("HELLO", 1.0, 1.0) - a.width).abs() < 1e-9);
    }

    #[test]
    fn center_alignment_centres_on_point() {
        let (_, bb) = place_text("ABC", Vec2::ZERO, Some(Vec2::new(10.0, 0.0)), 1.0, 0.0, 1.0, 0.0, Align::Center, VAlign::Baseline);
        assert!((bb.center().x - 10.0).abs() < 1e-9);
    }

    #[test]
    fn fit_spans_both_points() {
        let (_, bb) = place_text("ABC", Vec2::ZERO, Some(Vec2::new(10.0, 0.0)), 1.0, 0.0, 1.0, 0.0, Align::Fit, VAlign::Baseline);
        assert!((bb.width() - 10.0).abs() < 1e-6);
        assert!((bb.max.y - 1.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;

    #[test]
    fn encode_round_trips_control_codes() {
        for s in ["a%%ub%%uc 100%%%", "%%o%%uX%%o%%u", "中文 ABC", "%%c50"] {
            let d = decode_controls(s);
            assert_eq!(decode_controls(&encode(&d)), d, "{s}");
        }
    }

    #[test]
    fn missing_ideographs_use_a_system_font_when_installed() {
        let Some(fb) = ttf::fallback_cjk() else { return };
        assert!(ttf::has(&fb, '布'));
        let sh = shape_line(&TextFont::Stroke, "A布局1", 10.0, 1.0, 0.0);
        // The ideographs are filled outlines, the Latin letters stay strokes.
        assert_eq!(sh.glyphs.len(), 2);
        assert!(!sh.strokes.is_empty());
        let w = text_width(&TextFont::Stroke, "A布局1", 10.0, 1.0);
        assert!((w - sh.width).abs() < 1e-9 && w > 25.0);
        // An ideograph is about as tall as the text height.
        let b = shape_line(&TextFont::Stroke, "中", 10.0, 1.0, 0.0);
        let top = b.glyphs.iter().flatten().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!(top > 8.0 && top < 12.0, "{top}");
    }
}

#[cfg(test)]
mod ttf_tests {
    #[test]
    fn system_font_outlines_when_available() {
        // Skips cleanly when no common system font is installed.
        let Some(bytes) = ["Arial", "Helvetica", "DejaVuSans", "Verdana", "LiberationSans-Regular"].iter().find_map(|n| crate::ttf::find(n)) else {
            return;
        };
        let run = crate::ttf::layout_line(&bytes, "CAD", 1.0, 1.0, 0.0).unwrap();
        assert!(run.strokes.len() >= 3, "glyph contours");
        assert!(run.width > 1.5 && run.width < 4.0, "width {}", run.width);
        let maxy = run.strokes.iter().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((maxy - 1.0).abs() < 0.1, "cap height ~ text height, got {maxy}");
    }

    #[test]
    fn stroke_names_are_not_ttf() {
        assert!(crate::ttf::find("txt.shx").is_none());
        assert!(crate::ttf::find("").is_none());
        assert!(crate::ttf::layout_line(b"not a font", "x", 1.0, 1.0, 0.0).is_none());
    }
}
