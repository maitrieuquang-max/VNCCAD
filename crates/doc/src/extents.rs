//! Approximate entity bounds (used for extents, zoom and selection pre-filtering).

use cadcraft_geom::{Bounds2, Polyline, Vec2};

use crate::{Drawing, Entity, EntityKind, Prim};

/// Nested block references deeper than this are ignored (cyclic or hostile files).
pub const MAX_BLOCK_DEPTH: usize = 16;

fn text_box(insert: Vec2, height: f64, len: usize, width_factor: f64, rotation: f64) -> Bounds2 {
    let w = height * 0.8 * width_factor.abs().max(0.01) * len.max(1) as f64;
    let pts = [Vec2::ZERO, Vec2::new(w, 0.0), Vec2::new(w, height), Vec2::new(0.0, height)].map(|p| insert + p.rotate(rotation));
    Bounds2::from_points(pts)
}

pub fn entity_bounds(d: &Drawing, e: &Entity, depth: usize) -> Bounds2 {
    match &e.kind {
        EntityKind::Text(t) => text_box(
            t.align_pt.filter(|_| t.halign != crate::HAlign::Left).unwrap_or(t.insert).xy(),
            t.height,
            t.value.chars().count(),
            t.width_factor,
            t.rotation,
        ),
        EntityKind::AttDef(a) => text_box(a.text.insert.xy(), a.text.height, a.tag.chars().count(), a.text.width_factor, a.text.rotation),
        EntityKind::MText(t) => {
            let lines = t.contents.split("\\P").count().max(1);
            let longest = t.contents.split("\\P").map(|l| l.chars().count()).max().unwrap_or(1);
            let w = if t.width > 0.0 { t.width } else { t.height * 0.8 * longest as f64 };
            let h = t.height * 1.66 * lines as f64;
            let (dx, dy) = match t.attach {
                2 | 5 | 8 => (-w / 2.0, 0.0),
                3 | 6 | 9 => (-w, 0.0),
                _ => (0.0, 0.0),
            };
            let top = match t.attach {
                4..=6 => h / 2.0,
                7..=9 => h,
                _ => 0.0,
            };
            let o = t.insert.xy();
            let pts = [Vec2::new(dx, top + dy), Vec2::new(dx + w, top + dy), Vec2::new(dx + w, top - h + dy), Vec2::new(dx, top - h + dy)]
                .map(|p| o + p.rotate(t.rotation));
            Bounds2::from_points(pts)
        }
        EntityKind::Insert(ins) => {
            if depth >= MAX_BLOCK_DEPTH {
                return Bounds2::EMPTY;
            }
            let Some(blk) = d.block(&ins.block) else { return Bounds2::from_points([ins.insert.xy()]) };
            let m = ins.transform(blk.base.xy());
            let mut inner = Bounds2::EMPTY;
            for be in blk.entities.iter() {
                inner = inner.union(&entity_bounds(d, be, depth + 1));
            }
            let mut b =
                if inner.is_empty() { Bounds2::from_points([ins.insert.xy()]) } else { Bounds2::from_points(inner.corners().map(|c| m.apply(c))) };
            if ins.cols > 1 || ins.rows > 1 {
                let off = Vec2::new(ins.col_spacing * f64::from(ins.cols.saturating_sub(1)), ins.row_spacing * f64::from(ins.rows.saturating_sub(1)))
                    .rotate(ins.rotation);
                b = b.union(&Bounds2::from_points(b.corners().map(|c| c + off)));
            }
            for a in &ins.attribs {
                if !a.invisible {
                    b = b.union(&text_box(a.text.insert.xy(), a.text.height, a.text.value.chars().count(), a.text.width_factor, a.text.rotation));
                }
            }
            b
        }
        EntityKind::Dimension(dm) => {
            let pts: Vec<Vec2> = match dm.kind {
                crate::DimKind::Linear { .. } | crate::DimKind::Aligned | crate::DimKind::ArcLength => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy()],
                crate::DimKind::Radius | crate::DimKind::Diameter => vec![dm.defpt.xy(), dm.p15.xy()],
                crate::DimKind::Angular3P => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy(), dm.p15.xy()],
                crate::DimKind::Angular => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy(), dm.p15.xy(), dm.p16.xy()],
                crate::DimKind::Ordinate { .. } => vec![dm.p13.xy(), dm.p14.xy()],
                crate::DimKind::Jogged => vec![dm.p13.xy(), dm.p14.xy(), dm.p15.xy()],
            };
            let mut b = Bounds2::from_points(pts);
            if dm.user_text_pos {
                b.add(dm.text_mid.xy());
            }
            // Room for text and arrows.
            let th = d.dim_style(&dm.style).map(|s| s.text_height * s.scale.max(1e-9)).unwrap_or(0.18) * d.header.f64("DIMSCALE", 1.0).max(1e-9);
            b = b.expand(th * 2.5);
            if let Some(blk) = dm.block.as_ref().and_then(|n| d.block(n))
                && depth < MAX_BLOCK_DEPTH
            {
                for be in blk.entities.iter() {
                    b = b.union(&entity_bounds(d, be, depth + 1));
                }
            }
            b
        }
        EntityKind::MLeader(m) => {
            let mut b = Bounds2::from_points(m.leaders.iter().flatten().map(|v| v.xy()).chain(std::iter::once(m.landing.xy())));
            if let Some(t) = &m.text {
                b.add(t.insert.xy());
            }
            b
        }
        EntityKind::Table(t) => {
            let w: f64 = t.col_widths.iter().sum();
            let h: f64 = t.row_heights.iter().sum();
            Bounds2::new(t.insert.xy(), t.insert.xy() + Vec2::new(w, -h))
        }
        EntityKind::Image(i) => {
            let o = i.insert.xy();
            let u = i.u.xy() * i.size.x;
            let v = i.v.xy() * i.size.y;
            Bounds2::from_points([o, o + u, o + v, o + u + v])
        }
        EntityKind::LwPolyline(p) => Polyline { vertices: p.vertices.clone(), closed: p.closed }.bounds().expand(p.const_width / 2.0),
        kind => {
            let mut b = Bounds2::EMPTY;
            for p in kind.prims() {
                let pb = match p {
                    Prim::Seg(s) => s.bounds(),
                    Prim::Circle(c) => c.bounds(),
                    Prim::Ellipse(e) => e.bounds(),
                    Prim::Spline(s) => s.bounds(),
                    Prim::Point(p) => Bounds2::from_points([p]),
                    // Infinite lines don't contribute to extents (as in ZOOM Extents).
                    Prim::Infinite { base, ray, .. } => {
                        if ray {
                            Bounds2::from_points([base])
                        } else {
                            Bounds2::EMPTY
                        }
                    }
                    Prim::Fill(pts) => Bounds2::from_points(pts),
                };
                b = b.union(&pb);
            }
            b
        }
    }
}
