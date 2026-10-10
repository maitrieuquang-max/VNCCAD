//! VNCCad: geometric tolerance frames (TOLERANCE) laid out with the dimension style's text.

use cadcraft_doc::{Drawing, TolRun, Tolerance, gdt_strokes, tolerance_cells};
use cadcraft_fonts::{Shaped, TextParams};
use cadcraft_geom::Vec2;

/// Frame lines and text of a tolerance, in drawing coordinates.
#[derive(Clone, Debug, Default)]
pub struct TolGeometry {
    pub lines: Vec<Vec<Vec2>>,
    pub text: Shaped,
}

/// Text height of a tolerance: DIMTXT × DIMSCALE (or the text style's fixed height).
pub fn tolerance_height(d: &Drawing, t: &Tolerance) -> f64 {
    d.tolerance_height(t)
}

pub fn tolerance_geometry(d: &Drawing, t: &Tolerance) -> TolGeometry {
    let st = d.dim_style(&t.style).cloned().unwrap_or_default();
    let font = crate::dim_text(d, &st);
    let h = tolerance_height(d, t);
    let row_h = h * 2.0;
    let pad = h * 0.5;
    let dir = if t.dir.len() > 1e-9 { t.dir.normalized() } else { Vec2::X };
    let up = dir.perp();
    let o = t.insert.xy();
    let at = |x: f64, y: f64| o + dir * x + up * y;
    let rot = dir.angle();
    let wf = if font.width_factor > 0.0 && font.width_factor.is_finite() { font.width_factor } else { 1.0 };
    let mut g = TolGeometry::default();
    for (r, row) in tolerance_cells(&t.text).iter().enumerate().take(100) {
        let mid = -(r as f64) * row_h;
        let (y0, y1) = (mid - row_h / 2.0, mid + row_h / 2.0);
        let mut x = 0.0;
        for (c, cell) in row.iter().enumerate().take(20) {
            if cell.is_empty() {
                continue;
            }
            let only_symbol = c == 0 && cell.iter().all(|r| matches!(r, TolRun::Symbol(_)));
            // Lay out the runs from the cell's left edge.
            let mut cx = x + if only_symbol { 0.0 } else { pad };
            let mut parts: Vec<(f64, &TolRun)> = Vec::new();
            for run in cell {
                parts.push((cx, run));
                cx += match run {
                    TolRun::Symbol(_) => h * 1.1,
                    TolRun::Text(s) => {
                        let (sh, _) = cadcraft_fonts::place(&font.font, s, &TextParams { width_factor: wf, ..TextParams::new(Vec2::ZERO, h) });
                        sh.width
                    }
                };
            }
            let width = if only_symbol { row_h.max(cx - x) } else { cx - x + pad };
            let shift = if only_symbol { (width - (cx - x)) / 2.0 } else { 0.0 };
            for (px, run) in parts {
                let px = px + shift;
                match run {
                    TolRun::Symbol(ch) => {
                        let (sx, sy) = (px + h * 0.05, mid - h / 2.0);
                        for s in gdt_strokes(*ch) {
                            g.lines.push(s.iter().map(|q| at(sx + q.x * h, sy + q.y * h)).collect());
                        }
                    }
                    TolRun::Text(s) => {
                        let p = TextParams { rotation: rot, width_factor: wf, oblique: font.oblique, ..TextParams::new(at(px, mid - h / 2.0), h) };
                        let (sh, _) = cadcraft_fonts::place(&font.font, s, &p);
                        g.text.strokes.extend(sh.strokes);
                        g.text.glyphs.extend(sh.glyphs);
                    }
                }
            }
            g.lines.push(vec![at(x, y0), at(x + width, y0), at(x + width, y1), at(x, y1), at(x, y0)]);
            x += width;
        }
    }
    g
}
