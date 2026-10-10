//! VNCCad: SKETCH — freehand strokes recorded as the cursor moves.
//!
//! A click puts the pen down, the next click lifts it; while the pen is down every move longer
//! than the record increment adds a point. Enter keeps the strokes (as polylines, splines or
//! lines — SKPOLY), Esc throws them away.

use std::sync::Mutex;

use cadcraft_doc::EntityKind;
use cadcraft_geom::{PolyVertex, Spline, Vec2};
use serde_json::{Value, json};

use super::helpers::{line, lwpoly};
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("sketch", "Sketch", run_sketch)
            .menu(&["Draw", "Sketch"])
            .alias(&["vetay"])
            .params("{strokes: [[[x,y],...],...], type?: polyline|spline|line (default SKPOLY), increment?: tolerance for simplifying}")
            .interactive(|s| Ok(Box::new(SketchM::new(s))))
            .enabled(has_doc),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Line,
    Polyline,
    Spline,
}

fn kind_of(t: &str) -> Option<Kind> {
    match t.trim().to_ascii_lowercase().as_str() {
        "l" | "line" | "lines" => Some(Kind::Line),
        "p" | "polyline" | "pline" => Some(Kind::Polyline),
        "s" | "spline" => Some(Kind::Spline),
        _ => None,
    }
}

fn default_kind(s: &Session) -> Kind {
    match s.doc().map(|d| d.header.i64("SKPOLY", 1)).unwrap_or(1) {
        0 => Kind::Line,
        2 => Kind::Spline,
        _ => Kind::Polyline,
    }
}

/// Douglas–Peucker simplification.
fn simplify(pts: &[Vec2], tol: f64) -> Vec<Vec2> {
    if pts.len() < 3 || tol <= 0.0 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    let last = pts.len() - 1;
    keep[0] = true;
    keep[last] = true;
    let mut stack = vec![(0usize, last)];
    while let Some((a, b)) = stack.pop() {
        let (Some(pa), Some(pb)) = (pts.get(a), pts.get(b)) else { continue };
        let d = *pb - *pa;
        let l = d.len();
        let mut best = (0.0, 0usize);
        for (k, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let dist = if l < 1e-12 { p.dist(*pa) } else { (*p - *pa).cross(d).abs() / l };
            if dist > best.0 {
                best = (dist, k);
            }
        }
        if best.0 > tol {
            if let Some(k) = keep.get_mut(best.1) {
                *k = true;
            }
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

fn stroke_kinds(pts: &[Vec2], kind: Kind, tol: f64) -> Vec<EntityKind> {
    let pts = simplify(pts, tol);
    if pts.len() < 2 {
        return Vec::new();
    }
    match kind {
        Kind::Line => pts.windows(2).filter_map(|w| Some(line(*w.first()?, *w.get(1)?))).collect(),
        Kind::Polyline => vec![lwpoly(pts.iter().map(|p| PolyVertex::new(*p)).collect(), false)],
        Kind::Spline if pts.len() >= 3 => vec![EntityKind::Spline(Spline::from_fit_points(&pts))],
        Kind::Spline => vec![line(pts[0], pts[1])],
    }
}

fn run_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let strokes = p.get("strokes").and_then(Value::as_array).ok_or_else(|| bad("sketch", "`strokes` is required"))?;
    let kind = match str_param(p, "type") {
        Some(t) => kind_of(t).ok_or_else(|| bad("sketch", "type must be polyline, spline or line"))?,
        None => default_kind(s),
    };
    let tol = f64_or(p, "increment", 0.0).max(0.0);
    let mut hs = Vec::new();
    for st in strokes.iter().take(10_000) {
        let pts: Vec<Vec2> = st.as_array().map(|a| a.iter().filter_map(point_value).take(1_000_000).collect()).unwrap_or_default();
        for k in stroke_kinds(&pts, kind, tol) {
            hs.push(s.add_entity(k)?.hex());
        }
    }
    Ok(json!({ "handles": hs }))
}

struct SketchM {
    /// Finished strokes and the one being drawn (pen down when `down`).
    strokes: Mutex<(Vec<Vec<Vec2>>, Vec<Vec2>)>,
    down: bool,
    kind: Kind,
    /// Record increment (0: a few pixels).
    increment: f64,
    asking: u8,
}

impl SketchM {
    fn new(s: &Session) -> Self {
        let inc = s.doc().map(|d| d.header.f64("SKETCHINC", 0.0)).unwrap_or(0.0);
        SketchM { strokes: Mutex::new((Vec::new(), Vec::new())), down: false, kind: default_kind(s), increment: inc.max(0.0), asking: 0 }
    }
    fn step(&self, s: &Session) -> f64 {
        if self.increment > 0.0 { self.increment } else { s.pixel_size() * 3.0 }
    }
    fn lift(&self) {
        if let Ok(mut g) = self.strokes.lock() {
            let cur = std::mem::take(&mut g.1);
            if cur.len() >= 2 {
                g.0.push(cur);
            }
        }
    }
}

impl Interactive for SketchM {
    fn name(&self) -> &'static str {
        "SKETCH"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let k = match self.kind {
            Kind::Line => "Line",
            Kind::Polyline => "Polyline",
            Kind::Spline => "Spline",
        };
        s.echo(format!("Kiểu = {k}, Bước ghi = {}", if self.increment > 0.0 { format!("{:.3}", self.increment) } else { "tự động".into() }));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.asking {
            1 => Prompt::new("Kiểu nét", Accept::TEXT).kw(&["Polyline", "Spline", "Line"]).default("Polyline"),
            2 => Prompt::new("Bước ghi (0 = tự động)", Accept::NUMBER).default(format!("{}", self.increment)),
            _ if self.down => Prompt::new("Đang vẽ — bấm để nhấc bút, Enter để kết thúc", Accept::POINT),
            _ => Prompt::new("Bấm để hạ bút, Enter để kết thúc", Accept::POINT).kw(&["Type", "Increment"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking == 1 {
            if let Input::Keyword(t) | Input::Text(t) = &i
                && let Some(k) = kind_of(t)
            {
                self.kind = k;
                s.doc_mut()?.header.set_i64(
                    "SKPOLY",
                    match k {
                        Kind::Line => 0,
                        Kind::Polyline => 1,
                        Kind::Spline => 2,
                    },
                );
            }
            self.asking = 0;
            return Ok(Step::Continue);
        }
        if self.asking == 2 {
            if let Input::Text(t) = &i
                && let Some(v) = super::machines::number(t).filter(|v| v.is_finite() && *v >= 0.0)
            {
                self.increment = v;
                s.doc_mut()?.header.set_f64("SKETCHINC", v);
            }
            self.asking = 0;
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(k) => {
                match k.as_str() {
                    "Type" => self.asking = 1,
                    "Increment" => self.asking = 2,
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                if self.down {
                    if let Ok(mut g) = self.strokes.lock() {
                        g.1.push(p);
                    }
                    self.lift();
                    self.down = false;
                } else {
                    if let Ok(mut g) = self.strokes.lock() {
                        g.1 = vec![p];
                    }
                    self.down = true;
                }
                Ok(Step::Continue)
            }
            Input::Enter => {
                self.lift();
                self.down = false;
                let strokes = self.strokes.lock().map(|g| g.0.clone()).unwrap_or_default();
                let tol = self.step(s) * 0.5;
                let mut n = 0;
                for st in &strokes {
                    for k in stroke_kinds(st, self.kind, tol) {
                        s.add_entity(k)?;
                        n += 1;
                    }
                }
                s.echo(format!("Đã ghi {} nét ({n} đối tượng).", strokes.len()));
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let step = self.step(s);
        let Ok(mut g) = self.strokes.lock() else { return Vec::new() };
        if self.down && g.1.last().is_none_or(|l| l.dist(c) >= step) && g.1.len() < 1_000_000 {
            g.1.push(c);
        }
        g.0.iter()
            .chain(std::iter::once(&g.1))
            .filter(|st| st.len() >= 2)
            .map(|st| lwpoly(st.iter().map(|p| PolyVertex::new(*p)).collect(), false))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sketch_records_moves_while_the_pen_is_down() {
        let mut s = Session::new();
        s.start("sketch").unwrap();
        s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
        for k in 1..=50 {
            let x = f64::from(k);
            let _ = s.preview(Vec2::new(x, (x / 5.0).sin() * 3.0));
        }
        s.input(Input::Point(Vec2::new(50.0, 0.0))).unwrap();
        // Pen up: moves are not recorded.
        let _ = s.preview(Vec2::new(80.0, 80.0));
        s.input(Input::Enter).unwrap();
        assert!(s.running.is_none());
        let pls: Vec<usize> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p.vertices.len()) } else { None })
            .collect();
        assert_eq!(pls.len(), 1, "one stroke");
        assert!(pls[0] > 5, "a curved stroke keeps its shape: {pls:?}");
        let b = s.doc().unwrap().extents(&cadcraft_doc::Space::Model);
        assert!(b.max.y < 10.0, "the pen-up move was not drawn");
        // Programmatic, as splines.
        let r = s.execute("sketch", &json!({ "strokes": [[[0, 0], [1, 1], [2, 0], [3, 1]]], "type": "spline" })).unwrap();
        assert_eq!(r["handles"].as_array().unwrap().len(), 1);
        assert!(simplify(&[Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)], 0.1).len() == 2);
    }
}
