//! VNCCad: Object Snap Tracking (OTRACK, F11) and the drawing side of TK.
//!
//! Resting the cursor on an object snap point for a moment *acquires* it (a small `+`; resting
//! again releases it). While a point is wanted, moving the cursor into horizontal or vertical
//! alignment (along the UCS axes) with an acquired point snaps onto that alignment and shows a
//! dotted tracking line; where the alignments of two acquired points cross, the cursor snaps to
//! the crossing. A distance typed while a tracking line shows is measured from the tracked point
//! along the line (see `Session::track_line`).

use cadcraft_geom::Vec2;
use egui::{Color32, Pos2, Shape, Stroke, vec2};

use crate::canvas::Xf;

/// How long the cursor rests on a snap point before it is acquired (seconds).
const DWELL: f64 = 0.4;
/// Screen distance (points) within which the cursor locks onto a tracking line.
const LOCK_PX: f64 = 7.0;
/// At most this many acquired points, as in AutoCAD.
const MAX_ACQUIRED: usize = 7;

#[derive(Default)]
pub struct Track {
    /// Acquired object snap points (world).
    pub acquired: Vec<Vec2>,
    /// The snap point under the cursor and since when (seconds); `bool` = already toggled.
    resting: Option<(Vec2, f64, bool)>,
    /// Tracking lines to draw this frame (from, to), world.
    pub lines: Vec<(Vec2, Vec2)>,
    /// Tooltip for the tracking result.
    pub label: Option<String>,
    /// The command/prompt the acquired points belong to.
    owner: String,
}

impl Track {
    /// Forget acquired points when the command changes or ends.
    pub fn sync_owner(&mut self, owner: &str) {
        if self.owner != owner {
            self.owner = owner.to_string();
            self.acquired.clear();
            self.resting = None;
        }
    }

    /// The cursor is on snap point `p` (or on none): acquire / release after a short rest.
    pub fn dwell(&mut self, p: Option<Vec2>, now: f64) {
        let Some(p) = p else {
            self.resting = None;
            return;
        };
        match &mut self.resting {
            Some((q, since, done)) if q.near(p, 1e-9) => {
                if !*done && now - *since >= DWELL {
                    *done = true;
                    if let Some(i) = self.acquired.iter().position(|a| a.near(p, 1e-9)) {
                        self.acquired.remove(i);
                    } else {
                        self.acquired.push(p);
                        if self.acquired.len() > MAX_ACQUIRED {
                            self.acquired.remove(0);
                        }
                    }
                }
            }
            _ => self.resting = Some((p, now, false)),
        }
    }

    /// Snap `c` onto an alignment with the acquired points. Returns the tracked point and the
    /// (origin, direction) of the line a typed distance should follow (`None` at a crossing).
    pub fn align(&mut self, c: Vec2, ucs_angle: f64, scale: f64) -> Option<(Vec2, Option<(Vec2, Vec2)>)> {
        self.lines.clear();
        self.label = None;
        if self.acquired.is_empty() || scale <= 0.0 {
            return None;
        }
        let tol = LOCK_PX / scale;
        let dirs = [Vec2::from_angle(ucs_angle), Vec2::from_angle(ucs_angle + std::f64::consts::FRAC_PI_2)];
        // Every alignment line within reach: (distance to cursor, origin, direction).
        let mut near: Vec<(f64, Vec2, Vec2)> = Vec::new();
        for a in &self.acquired {
            for d in dirs {
                let q = *a + d * (c - *a).dot(d);
                let dd = q.dist(c);
                if dd <= tol {
                    near.push((dd, *a, d));
                }
            }
        }
        // Crossing of two alignments from different points (or one point's line with another's).
        let mut best_x: Option<(f64, Vec2, (Vec2, Vec2), (Vec2, Vec2))> = None;
        for (i, a) in self.acquired.iter().enumerate() {
            for (j, b) in self.acquired.iter().enumerate() {
                if i == j {
                    continue;
                }
                for da in dirs {
                    for db in dirs {
                        if da.cross(db).abs() < 1e-9 {
                            continue;
                        }
                        // a + da*t = b + db*u
                        let t = (*b - *a).cross(db) / da.cross(db);
                        let x = *a + da * t;
                        let dd = x.dist(c);
                        if dd <= tol * 1.4 && best_x.is_none_or(|(bd, ..)| dd < bd) {
                            best_x = Some((dd, x, (*a, da), (*b, db)));
                        }
                    }
                }
            }
        }
        if let Some((_, x, (a, _), (b, _))) = best_x {
            self.lines.push((a, x));
            self.lines.push((b, x));
            self.label = Some("Giao điểm bám".into());
            return Some((x, None));
        }
        let (_, a, d) = near.into_iter().min_by(|p, q| p.0.total_cmp(&q.0))?;
        let t = (c - a).dot(d);
        let q = a + d * t;
        self.lines.push((a, q));
        let ang = if t >= 0.0 { d.angle() } else { (-d).angle() };
        let deg = ((ang - ucs_angle).to_degrees().rem_euclid(360.0) * 10.0).round() / 10.0;
        self.label = Some(format!("Bám điểm: {} < {}°", cadcraft_engine::units::format_distance(t.abs(), 2, 4), deg));
        let dir = if t >= 0.0 { d } else { -d };
        Some((q, Some((a, dir))))
    }
}

fn green() -> Color32 {
    Color32::from_rgb(0x4c, 0xd1, 0x37)
}

/// Acquired points, tracking lines and the TK path.
pub fn draw(p: &egui::Painter, xf: &Xf, track: &Track, tk: &[Vec2], view_h: f64) {
    let st = Stroke::new(1.0, green());
    for a in &track.acquired {
        let s = xf.to_screen(*a);
        p.line_segment([s + vec2(-4.0, 0.0), s + vec2(4.0, 0.0)], Stroke::new(1.5, green()));
        p.line_segment([s + vec2(0.0, -4.0), s + vec2(0.0, 4.0)], Stroke::new(1.5, green()));
    }
    for (a, b) in &track.lines {
        // Run the dotted line well past the cursor, as AutoCAD does.
        let d = (*b - *a).normalized();
        let far = if d == Vec2::ZERO { *b } else { *b + d * view_h };
        let pts = [xf.to_screen(*a), xf.to_screen(far)];
        p.extend(Shape::dashed_line(&pts, st, 2.0, 3.0));
        let m = xf.to_screen(*b);
        p.line_segment([m + vec2(-4.0, -4.0), m + vec2(4.0, 4.0)], st);
        p.line_segment([m + vec2(4.0, -4.0), m + vec2(-4.0, 4.0)], st);
    }
    // TK: the tracking points so far, joined.
    let pts: Vec<Pos2> = tk.iter().map(|q| xf.to_screen(*q)).collect();
    for s in &pts {
        p.line_segment([*s + vec2(-5.0, 0.0), *s + vec2(5.0, 0.0)], Stroke::new(1.5, green()));
        p.line_segment([*s + vec2(0.0, -5.0), *s + vec2(0.0, 5.0)], Stroke::new(1.5, green()));
    }
    if pts.len() >= 2 {
        p.extend(Shape::dashed_line(&pts, st, 3.0, 3.0));
    }
}
