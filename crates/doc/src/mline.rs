//! VNCCad: multilines (MLINE) and their styles (MLINESTYLE), stored the way AutoCAD stores them
//! — per vertex a segment direction, a miter direction and, per element, its distance along the
//! miter followed by the break pairs MLEDIT's cuts leave — plus geometric tolerances (TOLERANCE).

use cadcraft_color::Color;
use cadcraft_geom::{Mat3, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// One line of a multiline style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLineElement {
    pub offset: f64,
    pub color: Color,
    pub linetype: String,
}

impl Default for MLineElement {
    fn default() -> Self {
        MLineElement { offset: 0.0, color: Color::ByLayer, linetype: "BYLAYER".into() }
    }
}

/// A multiline style (MLSTYLE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLineStyle {
    pub name: String,
    pub description: String,
    pub fill: bool,
    pub fill_color: Color,
    pub show_miters: bool,
    pub start_line: bool,
    pub start_outer_arc: bool,
    pub start_inner_arcs: bool,
    pub end_line: bool,
    pub end_outer_arc: bool,
    pub end_inner_arcs: bool,
    /// Cap angles in degrees (90 = square).
    pub start_angle: f64,
    pub end_angle: f64,
    pub elements: Vec<MLineElement>,
}

impl Default for MLineStyle {
    fn default() -> Self {
        MLineStyle {
            name: "STANDARD".into(),
            description: String::new(),
            fill: false,
            fill_color: Color::ByLayer,
            show_miters: false,
            start_line: false,
            start_outer_arc: false,
            start_inner_arcs: false,
            end_line: false,
            end_outer_arc: false,
            end_inner_arcs: false,
            start_angle: 90.0,
            end_angle: 90.0,
            elements: vec![MLineElement { offset: 0.5, ..Default::default() }, MLineElement { offset: -0.5, ..Default::default() }],
        }
    }
}

impl MLineStyle {
    /// DXF group 70 flags.
    pub fn flags(&self) -> i64 {
        let mut f = 0;
        for (on, bit) in [
            (self.fill, 1),
            (self.show_miters, 2),
            (self.start_line, 16),
            (self.start_inner_arcs, 32),
            (self.start_outer_arc, 64),
            (self.end_line, 256),
            (self.end_inner_arcs, 512),
            (self.end_outer_arc, 1024),
        ] {
            if on {
                f |= bit;
            }
        }
        f
    }
    pub fn set_flags(&mut self, f: i64) {
        self.fill = f & 1 != 0;
        self.show_miters = f & 2 != 0;
        self.start_line = f & 16 != 0;
        self.start_inner_arcs = f & 32 != 0;
        self.start_outer_arc = f & 64 != 0;
        self.end_line = f & 256 != 0;
        self.end_inner_arcs = f & 512 != 0;
        self.end_outer_arc = f & 1024 != 0;
    }
    /// Element offsets, as drawn (justification and scale applied).
    pub fn offsets(&self, just: u8, scale: f64) -> Vec<f64> {
        let max = self.elements.iter().map(|e| e.offset).fold(f64::NEG_INFINITY, f64::max);
        let min = self.elements.iter().map(|e| e.offset).fold(f64::INFINITY, f64::min);
        let shift = match just {
            0 if max.is_finite() => -max,
            2 if min.is_finite() => -min,
            _ => 0.0,
        };
        self.elements.iter().map(|e| (e.offset + shift) * scale).collect()
    }
}

/// A multiline vertex: its point, the direction of the segment leaving it, the miter
/// direction, and per element `[distance along the miter, dash start, dash end, …]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MLineVertex {
    pub p: Vec2,
    pub dir: Vec2,
    pub miter: Vec2,
    pub params: Vec<Vec<f64>>,
}

/// A multiline (MLINE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MLine {
    pub style: String,
    pub scale: f64,
    /// 0 top, 1 zero, 2 bottom.
    pub justification: u8,
    pub closed: bool,
    #[serde(default)]
    pub no_start_caps: bool,
    #[serde(default)]
    pub no_end_caps: bool,
    pub vertices: Vec<MLineVertex>,
}

/// What a multiline draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MLineGeom {
    /// (element index, piece).
    pub lines: Vec<(usize, [Vec2; 2])>,
    /// End caps and miter lines (polylines).
    pub caps: Vec<Vec<Vec2>>,
    /// Fill polygons, one per segment.
    pub fills: Vec<Vec<Vec2>>,
}

fn dirs_and_miters(pts: &[Vec2], closed: bool) -> Vec<(Vec2, Vec2)> {
    let n = pts.len();
    let seg = |i: usize| -> Vec2 {
        let a = pts.get(i).copied().unwrap_or_default();
        let b = pts.get((i + 1) % n.max(1)).copied().unwrap_or_default();
        (b - a).normalized()
    };
    // A zero-length segment takes its neighbour's direction.
    let seg_dir = |i: usize| -> Vec2 {
        let d = seg(i);
        if d.len() > 0.5 {
            return d;
        }
        (0..n).map(|k| seg((i + k) % n.max(1))).find(|d| d.len() > 0.5).unwrap_or(Vec2::X)
    };
    (0..n)
        .map(|i| {
            let last_open = !closed && i + 1 == n;
            let dir = if last_open { seg_dir(i.saturating_sub(1)) } else { seg_dir(i) };
            let miter = if !closed && (i == 0 || last_open) {
                dir.perp()
            } else {
                let prev = seg_dir((i + n - 1) % n);
                let m = (prev.perp() + dir.perp()).normalized();
                if m.len() < 0.5 { dir.perp() } else { m }
            };
            (dir, miter)
        })
        .collect()
}

impl MLine {
    /// A multiline through `pts` in `style`.
    pub fn build(pts: &[Vec2], closed: bool, style: &MLineStyle, scale: f64, justification: u8) -> MLine {
        let mut m = MLine { style: style.name.clone(), scale, justification, closed, no_start_caps: false, no_end_caps: false, vertices: Vec::new() };
        m.set_points(pts, &style.offsets(justification, scale));
        m
    }

    pub fn points(&self) -> Vec<Vec2> {
        self.vertices.iter().map(|v| v.p).collect()
    }

    /// Element offsets (perpendicular, signed: left of the direction is positive).
    pub fn offsets(&self) -> Vec<f64> {
        let Some(v) = self.vertices.first() else { return Vec::new() };
        let k = v.miter.dot(v.dir.perp());
        v.params.iter().map(|p| p.first().copied().unwrap_or(0.0) * k).collect()
    }

    /// Move the vertices to `pts`, recomputing directions and miters; the cuts are kept.
    pub fn set_points(&mut self, pts: &[Vec2], offsets: &[f64]) {
        let dm = dirs_and_miters(pts, self.closed);
        let old = std::mem::take(&mut self.vertices);
        self.vertices = pts
            .iter()
            .zip(dm)
            .enumerate()
            .map(|(i, (p, (dir, miter)))| {
                let k = miter.dot(dir.perp());
                let k = if k.abs() < 0.1 { 0.1_f64.copysign(k) } else { k };
                let params = offsets
                    .iter()
                    .enumerate()
                    .map(|(e, o)| {
                        let mut v = vec![o / k];
                        match old.get(i).and_then(|ov| ov.params.get(e)).filter(|ps| ps.len() > 1) {
                            Some(ps) => v.extend(ps.iter().skip(1)),
                            None => v.push(0.0),
                        }
                        v
                    })
                    .collect();
                MLineVertex { p: *p, dir, miter, params }
            })
            .collect();
    }

    /// Number of segments.
    pub fn segment_count(&self) -> usize {
        let n = self.vertices.len();
        if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        }
    }

    /// Where element `e` starts at vertex `i`.
    pub fn element_point(&self, i: usize, e: usize) -> Option<Vec2> {
        let v = self.vertices.get(i)?;
        Some(v.p + v.miter * v.params.get(e)?.first().copied().unwrap_or(0.0))
    }

    /// The full extent of element `e` along segment `i` (start point, length).
    pub fn element_span(&self, i: usize, e: usize) -> Option<(Vec2, f64)> {
        let n = self.vertices.len();
        let v = self.vertices.get(i)?;
        let s = self.element_point(i, e)?;
        let t = self.element_point((i + 1) % n, e)?;
        Some((s, (t - s).dot(v.dir).max(0.0)))
    }

    /// Visible pieces of element `e` on segment `i`, as distances along the segment.
    pub fn element_pieces(&self, i: usize, e: usize) -> Vec<(f64, f64)> {
        let Some((_, len)) = self.element_span(i, e) else { return Vec::new() };
        let Some(ps) = self.vertices.get(i).and_then(|v| v.params.get(e)) else { return Vec::new() };
        let l: Vec<f64> = ps.iter().skip(1).copied().collect();
        if l.is_empty() {
            return vec![(0.0, len)];
        }
        let mut out = Vec::new();
        let mut k = 0;
        while k < l.len() {
            let a = l.get(k).copied().unwrap_or(0.0).clamp(0.0, len);
            let b = l.get(k + 1).copied().unwrap_or(len).clamp(0.0, len);
            if b > a + 1e-12 {
                out.push((a, b));
            }
            k += 2;
        }
        out
    }

    /// Set the visible pieces of element `e` on segment `i`.
    pub fn set_element_pieces(&mut self, i: usize, e: usize, pieces: &[(f64, f64)]) {
        let Some(ps) = self.vertices.get_mut(i).and_then(|v| v.params.get_mut(e)) else { return };
        let first = ps.first().copied().unwrap_or(0.0);
        ps.clear();
        ps.push(first);
        if pieces.is_empty() {
            // Hidden entirely: one empty dash.
            ps.extend([0.0, 0.0, f64::MAX / 4.0]);
            return;
        }
        for (k, (a, b)) in pieces.iter().enumerate() {
            ps.push(*a);
            if k + 1 < pieces.len() {
                ps.push(*b);
            } else if b.is_finite() && *b < f64::MAX / 8.0 {
                // The last piece ends before the segment does.
                ps.push(*b);
                ps.push(f64::MAX / 4.0);
            }
        }
    }

    /// Element indices sorted from the most negative offset to the most positive.
    fn order(&self) -> Vec<usize> {
        let off = self.offsets();
        let mut idx: Vec<usize> = (0..off.len()).collect();
        idx.sort_by(|a, b| off.get(*a).unwrap_or(&0.0).total_cmp(off.get(*b).unwrap_or(&0.0)));
        idx
    }

    /// Lines, caps and fills (`style` gives caps, miters and fill; `None` draws lines only).
    pub fn geometry(&self, style: Option<&MLineStyle>) -> MLineGeom {
        let mut g = MLineGeom::default();
        let n = self.vertices.len();
        let ne = self.vertices.first().map_or(0, |v| v.params.len());
        for i in 0..self.segment_count() {
            let Some(v) = self.vertices.get(i) else { continue };
            for e in 0..ne {
                let Some((s, _)) = self.element_span(i, e) else { continue };
                for (a, b) in self.element_pieces(i, e) {
                    g.lines.push((e, [s + v.dir * a, s + v.dir * b]));
                }
            }
        }
        let Some(st) = style else { return g };
        let order = self.order();
        let (lo, hi) = match (order.first(), order.last()) {
            (Some(a), Some(b)) => (*a, *b),
            _ => return g,
        };
        if st.fill && n >= 2 {
            for i in 0..self.segment_count() {
                let j = (i + 1) % n;
                if let (Some(a), Some(b), Some(c), Some(d)) =
                    (self.element_point(i, lo), self.element_point(j, lo), self.element_point(j, hi), self.element_point(i, hi))
                {
                    g.fills.push(vec![a, b, c, d]);
                }
            }
        }
        if st.show_miters {
            let inner = if self.closed { 0..n } else { 1..n.saturating_sub(1) };
            for i in inner {
                if let (Some(a), Some(b)) = (self.element_point(i, lo), self.element_point(i, hi)) {
                    g.caps.push(vec![a, b]);
                }
            }
        }
        if self.closed || n < 2 {
            return g;
        }
        let ends = [
            (0, !self.no_start_caps, st.start_line, st.start_outer_arc, st.start_inner_arcs, -1.0),
            (n - 1, !self.no_end_caps, st.end_line, st.end_outer_arc, st.end_inner_arcs, 1.0),
        ];
        for (i, on, line, outer, inner, sign) in ends {
            if !on {
                continue;
            }
            let Some(v) = self.vertices.get(i) else { continue };
            let out = v.dir * sign;
            let (Some(a), Some(b)) = (self.element_point(i, lo), self.element_point(i, hi)) else { continue };
            if line {
                g.caps.push(vec![a, b]);
            }
            if outer {
                g.caps.push(semicircle(a, b, out));
            }
            if inner {
                let m = order.len();
                for k in 1..m / 2 {
                    if let (Some(x), Some(y)) = (order.get(k), order.get(m - 1 - k))
                        && let (Some(p), Some(q)) = (self.element_point(i, *x), self.element_point(i, *y))
                    {
                        g.caps.push(semicircle(p, q, out));
                    }
                }
            }
        }
        g
    }

    /// Apply a similarity transform (move, rotate, uniform scale, mirror).
    pub fn transform(&mut self, m: &Mat3) {
        let s = m.scale_factor();
        for v in &mut self.vertices {
            v.p = m.apply(v.p);
            let mt = m.apply_vec(v.miter);
            let k = mt.len();
            v.miter = mt.normalized();
            v.dir = m.apply_vec(v.dir).normalized();
            for ps in &mut v.params {
                if let Some(f) = ps.first_mut() {
                    *f *= k;
                }
                for x in ps.iter_mut().skip(1) {
                    if *x < f64::MAX / 8.0 {
                        *x *= s;
                    }
                }
            }
        }
        self.scale *= s;
    }
}

/// Half a circle from `a` to `b`, bulging towards `out`.
fn semicircle(a: Vec2, b: Vec2, out: Vec2) -> Vec<Vec2> {
    let c = a.mid(b);
    let r = a.dist(b) / 2.0;
    if r < 1e-12 {
        return vec![a, b];
    }
    let a0 = c.angle_to(a);
    let mid_ccw = Vec2::polar(c, r, a0 + std::f64::consts::FRAC_PI_2);
    let sweep = if (mid_ccw - c).dot(out) >= 0.0 { std::f64::consts::PI } else { -std::f64::consts::PI };
    (0..=24).map(|k| Vec2::polar(c, r, a0 + sweep * f64::from(k) / 24.0)).collect()
}

/// A geometric tolerance (feature control frame).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tolerance {
    pub insert: Vec3,
    /// Direction of the frame's x axis.
    pub dir: Vec2,
    /// AutoCAD's coded text: `{\Fgdt;j}%%v0.05%%vA`, rows joined by `^J`.
    pub text: String,
    /// Dimension style (text height DIMTXT, gap, colours).
    pub style: String,
}

impl crate::Drawing {
    /// Text height of a tolerance: DIMTXT × DIMSCALE (or the text style's fixed height).
    pub fn tolerance_height(&self, t: &Tolerance) -> f64 {
        let st = self.dim_style(&t.style).cloned().unwrap_or_default();
        let k = if st.scale > 0.0 && st.scale.is_finite() { st.scale } else { self.header.f64("DIMSCALE", 1.0).max(1e-9) };
        let fixed = self.text_style(&st.text_style).map_or(0.0, |s| s.height);
        let h = if fixed > 0.0 { fixed } else { st.text_height * k };
        if h.is_finite() && h > 0.0 { h } else { 0.18 }
    }

    /// Approximate outline of a tolerance frame (without measuring the font).
    pub fn tolerance_outline(&self, t: &Tolerance) -> [Vec2; 4] {
        let h = self.tolerance_height(t);
        let rows = tolerance_cells(&t.text);
        let width = rows
            .iter()
            .map(|r| {
                r.iter()
                    .filter(|c| !c.is_empty())
                    .map(|c| {
                        c.iter()
                            .map(|run| match run {
                                TolRun::Symbol(_) => h * 1.1,
                                TolRun::Text(s) => h * 0.9 * s.chars().count() as f64,
                            })
                            .sum::<f64>()
                            + h
                    })
                    .sum::<f64>()
            })
            .fold(0.0, f64::max)
            .max(h * 2.0);
        let dir = if t.dir.len() > 1e-9 { t.dir.normalized() } else { Vec2::X };
        let up = dir.perp();
        let o = t.insert.xy();
        let top = h;
        let bottom = -h - h * 2.0 * (rows.len().max(1) - 1) as f64;
        [o + up * bottom, o + dir * width + up * bottom, o + dir * width + up * top, o + up * top]
    }
}

/// One cell of a tolerance frame: runs of plain text and GDT symbol letters.
#[derive(Clone, Debug, PartialEq)]
pub enum TolRun {
    Text(String),
    Symbol(char),
}

/// The rows and cells of a tolerance string.
pub fn tolerance_cells(text: &str) -> Vec<Vec<Vec<TolRun>>> {
    text.split("^J")
        .map(|row| {
            row.split("%%v")
                .map(|cell| {
                    let mut runs: Vec<TolRun> = Vec::new();
                    let mut rest = cell;
                    let push_text = |runs: &mut Vec<TolRun>, t: &str| {
                        let t = t.replace("%%c", "\u{2205}").replace("%%C", "\u{2205}").replace("%%d", "°").replace("%%p", "±");
                        if !t.is_empty() {
                            runs.push(TolRun::Text(t));
                        }
                    };
                    while let Some(k) = rest.find("{\\Fgdt;").or_else(|| rest.find("{\\fgdt;")) {
                        push_text(&mut runs, rest.get(..k).unwrap_or(""));
                        let after = rest.get(k + 7..).unwrap_or("");
                        let end = after.find('}').unwrap_or(after.len());
                        for c in after.get(..end).unwrap_or("").chars() {
                            runs.push(TolRun::Symbol(c.to_ascii_lowercase()));
                        }
                        rest = after.get(end + 1..).unwrap_or("");
                    }
                    push_text(&mut runs, rest);
                    runs
                })
                .collect()
        })
        .collect()
}

/// GDT symbol letters by name (as offered by the TOLERANCE command).
pub const GDT_SYMBOLS: &[(&str, char)] = &[
    ("Position", 'j'),
    ("Concentricity", 'r'),
    ("Symmetry", 'i'),
    ("Parallelism", 'f'),
    ("Perpendicularity", 'b'),
    ("Angularity", 'a'),
    ("Cylindricity", 'g'),
    ("Flatness", 'c'),
    ("Circularity", 'e'),
    ("Straightness", 'u'),
    ("SurfaceProfile", 'd'),
    ("LineProfile", 'k'),
    ("CircularRunout", 'h'),
    ("TotalRunout", 't'),
];

/// Strokes of a GDT symbol in a unit box (0..1 × 0..1).
pub fn gdt_strokes(c: char) -> Vec<Vec<Vec2>> {
    let p = |x: f64, y: f64| Vec2::new(x, y);
    let circle = |cx: f64, cy: f64, r: f64| -> Vec<Vec2> {
        (0..=32).map(|k| Vec2::polar(p(cx, cy), r, std::f64::consts::TAU * f64::from(k) / 32.0)).collect()
    };
    let arc = |cx: f64, cy: f64, r: f64, a0: f64, a1: f64| -> Vec<Vec2> {
        (0..=16).map(|k| Vec2::polar(p(cx, cy), r, a0 + (a1 - a0) * f64::from(k) / 16.0)).collect()
    };
    let arrow = |a: Vec2, b: Vec2| -> Vec<Vec<Vec2>> {
        let d = (b - a).normalized();
        let s = 0.22;
        vec![vec![a, b], vec![b - d.rotate(0.45) * s, b, b - d.rotate(-0.45) * s]]
    };
    let letter = |l: char| -> Vec<Vec<Vec2>> {
        let mut v = vec![circle(0.5, 0.5, 0.42)];
        v.extend(match l {
            'M' => vec![vec![p(0.3, 0.3), p(0.3, 0.7), p(0.5, 0.45), p(0.7, 0.7), p(0.7, 0.3)]],
            'L' => vec![vec![p(0.35, 0.72), p(0.35, 0.3), p(0.68, 0.3)]],
            'S' => vec![vec![
                p(0.66, 0.66),
                p(0.55, 0.72),
                p(0.42, 0.72),
                p(0.33, 0.62),
                p(0.38, 0.52),
                p(0.62, 0.48),
                p(0.67, 0.38),
                p(0.58, 0.28),
                p(0.44, 0.28),
                p(0.33, 0.34),
            ]],
            'P' => vec![vec![p(0.38, 0.28), p(0.38, 0.72), p(0.6, 0.72), p(0.68, 0.62), p(0.6, 0.52), p(0.38, 0.52)]],
            _ => Vec::new(),
        });
        v
    };
    match c {
        'a' => vec![vec![p(0.85, 0.15), p(0.15, 0.15), p(0.75, 0.8)]],
        'b' => vec![vec![p(0.1, 0.15), p(0.9, 0.15)], vec![p(0.5, 0.15), p(0.5, 0.85)]],
        'c' => vec![vec![p(0.08, 0.28), p(0.68, 0.28), p(0.92, 0.72), p(0.32, 0.72), p(0.08, 0.28)]],
        'd' => {
            let mut a = arc(0.5, 0.3, 0.38, 0.0, std::f64::consts::PI);
            a.push(p(0.88, 0.3));
            vec![a]
        }
        'e' => vec![circle(0.5, 0.5, 0.35)],
        'f' => vec![vec![p(0.2, 0.15), p(0.5, 0.85)], vec![p(0.5, 0.15), p(0.8, 0.85)]],
        'g' => vec![circle(0.5, 0.5, 0.28), vec![p(0.12, 0.15), p(0.42, 0.85)], vec![p(0.58, 0.15), p(0.88, 0.85)]],
        'h' => arrow(p(0.2, 0.15), p(0.8, 0.85)),
        'i' => vec![vec![p(0.08, 0.5), p(0.92, 0.5)], vec![p(0.25, 0.25), p(0.75, 0.25)], vec![p(0.25, 0.75), p(0.75, 0.75)]],
        'j' => vec![circle(0.5, 0.5, 0.3), vec![p(0.5, 0.05), p(0.5, 0.95)], vec![p(0.05, 0.5), p(0.95, 0.5)]],
        'k' => vec![arc(0.5, 0.3, 0.38, 0.0, std::f64::consts::PI)],
        'l' => letter('L'),
        'm' => letter('M'),
        'n' => vec![circle(0.5, 0.5, 0.32), vec![p(0.2, 0.12), p(0.8, 0.88)]],
        'p' => letter('P'),
        'r' => vec![circle(0.5, 0.5, 0.38), circle(0.5, 0.5, 0.2)],
        's' => letter('S'),
        't' => {
            let mut v = vec![vec![p(0.1, 0.15), p(0.9, 0.15)]];
            v.extend(arrow(p(0.15, 0.15), p(0.5, 0.85)));
            v.extend(arrow(p(0.5, 0.15), p(0.85, 0.85)));
            v
        }
        'u' => vec![vec![p(0.08, 0.5), p(0.92, 0.5)]],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mline_offsets_breaks_and_caps() {
        let st = MLineStyle { start_line: true, end_outer_arc: true, ..MLineStyle::default() };
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)];
        let m = MLine::build(&pts, false, &st, 2.0, 1);
        let off = m.offsets();
        assert!((off[0] - 1.0).abs() < 1e-9 && (off[1] + 1.0).abs() < 1e-9, "{off:?}");
        // Element 0 is on the left (+y), the inside of the left turn: its corner is (9, 1).
        let g = m.geometry(Some(&st));
        assert_eq!(g.lines.len(), 4);
        let (_, l) = g.lines.iter().find(|(e, l)| *e == 0 && l[0].y.abs() > 0.5 && l[0].x.abs() < 1e-9).unwrap();
        assert!((l[1] - Vec2::new(9.0, 1.0)).len() < 1e-9, "{l:?}");
        assert_eq!(g.caps.len(), 2, "a start line and an end arc");
        // Top justification: all lines on the right (below the first segment).
        let top = MLine::build(&pts, false, &st, 2.0, 0);
        assert!(top.offsets().iter().all(|o| *o <= 1e-9));
        // A cut keeps the rest.
        let mut c = m.clone();
        c.set_element_pieces(0, 0, &[(0.0, 3.0), (5.0, f64::MAX)]);
        assert_eq!(c.element_pieces(0, 0).len(), 2);
        assert_eq!(c.geometry(None).lines.len(), 5);
        // Moving a vertex keeps the cut.
        c.set_points(&[Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(20.0, 10.0)], &off);
        assert_eq!(c.element_pieces(0, 0).len(), 2);
    }

    #[test]
    fn tolerance_text_cells() {
        let rows = tolerance_cells("{\\Fgdt;j}%%v{\\Fgdt;n}0.05{\\Fgdt;m}%%vA^J{\\Fgdt;f}%%v0.01%%vB");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], vec![TolRun::Symbol('j')]);
        assert_eq!(rows[0][1], vec![TolRun::Symbol('n'), TolRun::Text("0.05".into()), TolRun::Symbol('m')]);
        assert_eq!(rows[1][2], vec![TolRun::Text("B".into())]);
        for (_, c) in GDT_SYMBOLS {
            assert!(!gdt_strokes(*c).is_empty());
        }
    }
}
