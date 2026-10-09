//! Vector PDF plotting (PLOT / EXPORTPDF): a minimal PDF 1.4 writer of our own.
//!
//! One page per plot: model-space extents fitted to a sheet, or a layout at 1:1 on its paper
//! (paper units are millimetres in metric drawings, inches otherwise). Lines are stroked paths
//! (`m`/`l`/`S`), fills are `f` paths, colours are RGB with colour 7 printing black on white
//! paper, and line widths come from lineweights when enabled.

use std::fmt::Write as _;

use cadcraft_color::{Rgb, display_rgb};
use cadcraft_doc::{Drawing, PageSetup, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{Kind, Sheet, clip, paper};
use serde_json::Value;

use crate::{IoError, Result};

/// PDF points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Largest sheet side we accept, in millimetres (PDF viewers cap pages at 200 inches anyway).
const MAX_SHEET_MM: f64 = 5080.0;

/// Plot settings. `None` fields fall back to the layout's page setup (or sensible defaults for
/// model space).
#[derive(Clone, Debug, Default)]
pub struct PdfOptions {
    /// Paper name from [`cadcraft_render::PAPER_SIZES`] (`"A4"`, `"Letter"`, `"ANSI B"`…).
    pub paper: Option<String>,
    /// Custom paper size in millimetres (portrait width, height).
    pub paper_mm: Option<(f64, f64)>,
    pub landscape: Option<bool>,
    /// Fit the drawing to the printable area. Default: on for model space, off for layouts (1:1).
    pub fit: Option<bool>,
    /// Plot scale in paper units per drawing unit (when not fitting).
    pub scale: Option<f64>,
    /// Plot object lineweights. Default: the layout's setting; on for model space.
    pub lineweights: Option<bool>,
    /// Flate-compress the content stream.
    pub compress: bool,
    pub title: String,
    /// VNCCad: plot style table (".ctb" name, or the built-in "monochrome" / "grayscale").
    /// Default: the layout's page setup.
    pub plot_style: Option<String>,
    /// VNCCad: plot only this window (drawing units), fitted to the paper unless a scale is
    /// given. Default: the page setup's window when its plot area is "window".
    pub window: Option<Bounds2>,
}

impl PdfOptions {
    /// Parse `{paper?, width?, height?, landscape?, fit?, scale?, lineweights?, compress?, title?}`.
    pub fn from_json(v: &Value) -> PdfOptions {
        let num = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        let paper_mm = match (num("width"), num("height")) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        };
        PdfOptions {
            paper: v.get("paper").and_then(Value::as_str).map(str::to_string),
            paper_mm,
            landscape: v.get("landscape").and_then(Value::as_bool),
            fit: v.get("fit").and_then(Value::as_bool),
            scale: num("scale").filter(|s| *s > 0.0),
            lineweights: v.get("lineweights").and_then(Value::as_bool),
            compress: v.get("compress").and_then(Value::as_bool).unwrap_or(true),
            title: v.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            plot_style: v.get("plotStyleTable").and_then(Value::as_str).map(str::to_string).filter(|s| !s.trim().is_empty()),
            window: window_json(v.get("window")),
        }
    }
}

/// `[[x1, y1], [x2, y2]]` or `[x1, y1, x2, y2]` → a non-empty window.
fn window_json(v: Option<&Value>) -> Option<Bounds2> {
    let a = v?.as_array()?;
    let n: Vec<f64> = match a.len() {
        2 => a.iter().filter_map(Value::as_array).flat_map(|p| p.iter().take(2).filter_map(Value::as_f64)).collect(),
        4 => a.iter().filter_map(Value::as_f64).collect(),
        _ => return None,
    };
    window_of(&n)
}

fn window_of(n: &[f64]) -> Option<Bounds2> {
    let [x1, y1, x2, y2] = *n else { return None };
    let b = Bounds2::new(Vec2::new(x1.min(x2), y1.min(y2)), Vec2::new(x1.max(x2), y1.max(y2)));
    (n.iter().all(|v| v.is_finite()) && b.width() > 1e-9 && b.height() > 1e-9).then_some(b)
}

/// Plot with JSON options (the engine's `plot` hook).
pub fn plot(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>> {
    pdf(d, space, &PdfOptions::from_json(opts))
}

/// The model-space page setup: the saved one, else A4 (metric) or Letter, plotting the extents.
pub fn model_page(d: &Drawing) -> PageSetup {
    if let Some(p) = &d.model_page {
        return p.clone();
    }
    let mut p = PageSetup { plot_area: "extents".into(), scale_to_fit: true, ..PageSetup::default() };
    if paper::paper_unit_mm(d) == 1.0
        && let Some(a4) = paper::paper_size("A4")
    {
        p.paper = a4.name.into();
        p.width_mm = a4.width_mm;
        p.height_mm = a4.height_mm;
    }
    p
}

/// The plot style table saved for `space` (its page setup), if any.
pub fn saved_plot_style(d: &Drawing, space: &Space) -> Option<String> {
    let name = match space {
        Space::Model => {
            let p = model_page(d).plot_style_table;
            if p.is_empty() { d.header.str("VNCCAD_PLOTSTYLE", "") } else { p }
        }
        Space::Paper(n) => d.layout(n).map(|l| l.page.plot_style_table.clone()).unwrap_or_default(),
    };
    Some(name).filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("none"))
}

/// The page setup a plot of `space` uses, with overrides applied.
pub fn page_for(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<PageSetup> {
    let mut page = match space {
        Space::Paper(n) => d.layout(n).map(|l| l.page.clone()).ok_or_else(|| IoError::Format(format!("no layout `{n}`")))?,
        Space::Model => model_page(d),
    };
    if let Some(name) = &o.paper {
        let ps = paper::paper_size(name).ok_or_else(|| IoError::Format(format!("unknown paper size `{name}`")))?;
        page.paper = ps.name.into();
        page.width_mm = ps.width_mm;
        page.height_mm = ps.height_mm;
    }
    if let Some((w, h)) = o.paper_mm {
        page.paper = format!("User ({w:.2} x {h:.2} MM)");
        page.width_mm = w.min(MAX_SHEET_MM);
        page.height_mm = h.min(MAX_SHEET_MM);
    }
    if let Some(l) = o.landscape {
        page.landscape = l;
    }
    page.width_mm = page.width_mm.clamp(1.0, MAX_SHEET_MM);
    page.height_mm = page.height_mm.clamp(1.0, MAX_SHEET_MM);
    Ok(page)
}

/// World → paper-unit mapping: `p' = (p - from) * s + to`.
#[derive(Clone, Copy)]
struct Map {
    from: Vec2,
    s: f64,
    to: Vec2,
    /// Points per paper unit.
    k: f64,
}

impl Map {
    fn pt(&self, p: Vec2) -> Vec2 {
        ((p - self.from) * self.s + self.to) * self.k
    }
}

/// Write a one-page vector PDF of a space.
pub fn pdf(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<Vec<u8>> {
    let page = plot_page(d, space, o)?;
    let title = page.title.clone();
    Ok(assemble_pages(&[page], o.compress, &title))
}

/// VNCCad (PUBLISH): several sheets — layouts, or windows of model space — in one PDF, one page
/// each, in the order given.
pub fn publish(d: &Drawing, sheets: &[(Space, PdfOptions)], compress: bool, title: &str) -> Result<Vec<u8>> {
    if sheets.is_empty() {
        return Err(IoError::Format("không có tờ nào để in".into()));
    }
    let mut pages = Vec::with_capacity(sheets.len());
    for (space, o) in sheets.iter().take(500) {
        pages.push(plot_page(d, space, o)?);
    }
    Ok(assemble_pages(&pages, compress, title))
}

/// A raster image placed on a page.
struct PdfImage {
    width: u32,
    height: u32,
    rgba: std::sync::Arc<Vec<u8>>,
}

/// One plotted page before it is written into the file.
struct PageOut {
    media: Vec2,
    content: String,
    images: Vec<PdfImage>,
    title: String,
}

/// VNCCad: raster images (and PDF underlays) of the plotted space — and of model space seen
/// through the layout's viewports — as `cm … Do` operations drawn under the vectors.
fn image_ops(d: &Drawing, space: &Space, map: &Map, images: &mut Vec<PdfImage>, keys: &mut Vec<String>) -> String {
    let mut ops = String::new();
    let visible = |e: &cadcraft_doc::Entity| d.layer(&e.common.layer).is_none_or(|l| l.on && !l.frozen && l.plot);
    let mut place = |ops: &mut String, im: &cadcraft_doc::Image, to_paper: &dyn Fn(Vec2) -> Vec2, clip: Option<(Vec2, Vec2)>| {
        let key = crate::raster::file_key(&im.path);
        let idx = match keys.iter().position(|k| *k == key) {
            Some(i) => i,
            None => {
                let Some(dec) = crate::raster::decoded(&im.path) else { return };
                if dec.width == 0 || dec.height == 0 || dec.rgba.len() < (dec.width * dec.height * 4) as usize {
                    return;
                }
                images.push(PdfImage { width: dec.width, height: dec.height, rgba: std::sync::Arc::new(dec.rgba.clone()) });
                keys.push(key);
                images.len() - 1
            }
        };
        let o = im.insert.xy();
        let u = im.u.xy() * im.size.x;
        let v = im.v.xy() * im.size.y;
        let (po, pu, pv) = (map.pt(to_paper(o)), map.pt(to_paper(o + u)), map.pt(to_paper(o + v)));
        let (a, b) = (pu - po, pv - po);
        if ![a.x, a.y, b.x, b.y, po.x, po.y].iter().all(|x| x.is_finite()) {
            return;
        }
        ops.push_str("q\n");
        if let Some((c0, c1)) = clip {
            let (q0, q1) = (map.pt(c0), map.pt(c1));
            pt(ops, Vec2::new(q0.x.min(q1.x), q0.y.min(q1.y)));
            ops.push(' ');
            num(ops, (q1.x - q0.x).abs());
            ops.push(' ');
            num(ops, (q1.y - q0.y).abs());
            ops.push_str(" re W n\n");
        }
        for x in [a.x, a.y, b.x, b.y, po.x, po.y] {
            num(ops, x);
            ops.push(' ');
        }
        let _ = writeln!(ops, "cm /Im{} Do\nQ", idx + 1);
    };
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| visible(e)) {
            if let cadcraft_doc::EntityKind::Image(im) = &e.kind {
                place(&mut ops, im, &|p| p, None);
            }
        }
        // Model-space images through the layout's viewports.
        if matches!(space, Space::Paper(_)) {
            for e in store.iter().filter(|e| visible(e)) {
                let cadcraft_doc::EntityKind::Viewport(vp) = &e.kind else { continue };
                if vp.id == 1 || vp.height <= 1e-12 || vp.view_height <= 1e-12 {
                    continue;
                }
                let k = vp.height / vp.view_height;
                let c = vp.center.xy();
                let to_paper = move |p: Vec2| c + (p - vp.view_center) * k;
                let clip = (c - Vec2::new(vp.width, vp.height) / 2.0, c + Vec2::new(vp.width, vp.height) / 2.0);
                for m in d.model.iter().filter(|m| visible(m) && !vp.frozen_layers.iter().any(|f| f.eq_ignore_ascii_case(&m.common.layer))) {
                    if let cadcraft_doc::EntityKind::Image(im) = &m.kind {
                        place(&mut ops, im, &to_paper, Some(clip));
                    }
                }
            }
        }
    }
    ops
}

fn plot_page(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<PageOut> {
    let mut page = page_for(d, space, o)?;
    let window = o.window.or_else(|| page.window.filter(|_| page.plot_area == "window").and_then(|w| window_of(&w)));
    // A window given with the plot turns the paper to match its shape.
    if let (Some(w), Some(_), None) = (window, o.window, o.landscape) {
        page.landscape = w.width() > w.height();
    }
    let unit_mm = paper::paper_unit_mm(d);
    let sheet = Sheet::from_page(&page, unit_mm);
    let lineweights = o.lineweights.unwrap_or(match space {
        Space::Paper(_) => page.lineweights,
        Space::Model => true,
    });
    // A table asked for with the plot must exist; one saved in the drawing that isn't on this
    // computer plots with object colours instead, like AutoCAD (the engine warns).
    let pens = match &o.plot_style {
        Some(name) => Some(std::sync::Arc::new(crate::ctb::find(name).map_err(IoError::Format)?)),
        None => saved_plot_style(d, space).and_then(|n| crate::ctb::find(&n).ok()).map(std::sync::Arc::new),
    };
    let ropts = cadcraft_render::Options { tolerance: 0.001, min_dash: 0.0, text: true, fill: true, lineweights, pens, anno_scale: 0.0 };
    let k = unit_mm * PT_PER_MM;
    let fit = o.fit.unwrap_or(matches!(space, Space::Model) || window.is_some());
    // Chord tolerance: about 0.05 mm on paper.
    let est = plot_scale(&window.unwrap_or_else(|| d.extents(space)), &sheet, fit, o.scale);
    let tol = 0.05 / unit_mm / est.max(1e-300);
    let tolerance = if tol.is_finite() && tol > 0.0 { tol } else { ropts.tolerance };
    let list = cadcraft_render::build_plot(d, space, &cadcraft_render::Options { tolerance, ..ropts });
    let b = window.unwrap_or(list.bounds);
    let s = plot_scale(&b, &sheet, fit, o.scale);
    let map = if fit || matches!(space, Space::Model) || window.is_some() {
        let from = if b.is_empty() { Vec2::ZERO } else { b.center() };
        Map { from, s, to: sheet.printable.center(), k }
    } else {
        Map { from: Vec2::ZERO, s: 1.0, to: Vec2::ZERO, k }
    };
    let media = sheet.size * k;
    let clip_pt = if fit || matches!(space, Space::Model) {
        Bounds2::new(sheet.printable.min * k, sheet.printable.max * k)
    } else {
        Bounds2::new(Vec2::ZERO, media)
    };
    // Nothing outside the window.
    let clip_pt = match window {
        Some(w) => {
            let (a, c) = (map.pt(w.min), map.pt(w.max));
            Bounds2::new(Vec2::new(a.x.max(clip_pt.min.x), a.y.max(clip_pt.min.y)), Vec2::new(c.x.min(clip_pt.max.x), c.y.min(clip_pt.max.y)))
        }
        None => clip_pt,
    };
    let mut content = content_stream(&list, &map, &clip_pt);
    let mut images = Vec::new();
    let ops = image_ops(d, space, &map, &mut images, &mut Vec::new());
    if !ops.is_empty()
        && let Some(at) = content.find(" re W n\n")
    {
        content.insert_str(at + " re W n\n".len(), &ops);
    }
    let title = if o.title.is_empty() {
        match space {
            Space::Model => "Model".to_string(),
            Space::Paper(n) => n.clone(),
        }
    } else {
        o.title.clone()
    };
    Ok(PageOut { media, content, images, title })
}

/// Paper units per drawing unit for a plot.
fn plot_scale(b: &Bounds2, sheet: &Sheet, fit: bool, scale: Option<f64>) -> f64 {
    if let Some(s) = scale.filter(|s| s.is_finite() && *s > 0.0)
        && !fit
    {
        return s;
    }
    if !fit || b.is_empty() {
        return 1.0;
    }
    let pw = sheet.printable.width();
    let ph = sheet.printable.height();
    let s = (pw / b.width().max(1e-12)).min(ph / b.height().max(1e-12));
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// A number for a content stream: short, finite, no exponent.
fn num(out: &mut String, v: f64) {
    let v = if v.is_finite() { v.clamp(-1.0e7, 1.0e7) } else { 0.0 };
    let mut s = format!("{v:.3}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    out.push_str(&s);
}

fn pt(out: &mut String, p: Vec2) {
    num(out, p.x);
    out.push(' ');
    num(out, p.y);
}

fn color(out: &mut String, c: Rgb, op: &str) {
    for v in [c.0, c.1, c.2] {
        num(out, f64::from(v) / 255.0);
        out.push(' ');
    }
    out.push_str(op);
    out.push('\n');
}

fn content_stream(list: &cadcraft_render::DisplayList, map: &Map, clip_pt: &Bounds2) -> String {
    let white = Rgb(255, 255, 255);
    let mut c = String::new();
    c.push_str("q\n1 J 1 j\n");
    // Clip to the plot area.
    pt(&mut c, clip_pt.min);
    c.push(' ');
    num(&mut c, clip_pt.width());
    c.push(' ');
    num(&mut c, clip_pt.height());
    c.push_str(" re W n\n");
    let mut stroke: Option<Rgb> = None;
    let mut fill: Option<Rgb> = None;
    let mut width: Option<f64> = None;
    let mut pending = false;
    let flush = |c: &mut String, pending: &mut bool| {
        if *pending {
            c.push_str("S\n");
            *pending = false;
        }
    };
    for p in &list.prims {
        let rgb = display_rgb(p.color, white);
        let raw = list.points(p);
        if raw.iter().any(|q| !q.is_finite()) {
            continue;
        }
        let w = f64::from(p.lw) * PT_PER_MM;
        let w = if w.is_finite() && w > 0.0 { w.min(100.0) } else { 0.0 };
        // Skip what lies wholly outside the plot area (a window of a large drawing).
        if !matches!(p.kind, Kind::Infinite { .. }) && !Bounds2::from_points(raw.iter().map(|q| map.pt(*q))).intersects(&clip_pt.expand(w + 1.0)) {
            continue;
        }
        match p.kind {
            Kind::Polyline | Kind::Infinite { .. } => {
                let pts: Vec<Vec2> = match p.kind {
                    Kind::Infinite { ray } => match (raw.first(), raw.get(1)) {
                        (Some(base), Some(dir)) => {
                            let a = map.pt(*base);
                            let dir_pt = map.pt(*base + *dir) - a;
                            match clip::clip_infinite(a, dir_pt, ray, clip_pt) {
                                Some((x, y)) => vec![x, y],
                                None => continue,
                            }
                        }
                        _ => continue,
                    },
                    _ => raw.iter().map(|q| map.pt(*q)).collect(),
                };
                if pts.len() < 2 {
                    continue;
                }
                if stroke != Some(rgb) || width != Some(w) {
                    flush(&mut c, &mut pending);
                    if stroke != Some(rgb) {
                        color(&mut c, rgb, "RG");
                        stroke = Some(rgb);
                    }
                    if width != Some(w) {
                        num(&mut c, w);
                        c.push_str(" w\n");
                        width = Some(w);
                    }
                }
                for (i, q) in pts.iter().enumerate() {
                    pt(&mut c, *q);
                    c.push_str(if i == 0 { " m\n" } else { " l\n" });
                }
                pending = true;
            }
            Kind::Tris => {
                flush(&mut c, &mut pending);
                if fill != Some(rgb) {
                    color(&mut c, rgb, "rg");
                    fill = Some(rgb);
                }
                let mut any = false;
                for t in raw.as_chunks::<3>().0 {
                    for (i, q) in t.iter().enumerate() {
                        pt(&mut c, map.pt(*q));
                        c.push_str(if i == 0 { " m\n" } else { " l\n" });
                    }
                    c.push_str("h\n");
                    any = true;
                }
                if any {
                    c.push_str("f\n");
                }
            }
            Kind::Point => {
                let Some(q) = raw.first() else { continue };
                flush(&mut c, &mut pending);
                if fill != Some(rgb) {
                    color(&mut c, rgb, "rg");
                    fill = Some(rgb);
                }
                let half = (w.max(0.5)) / 2.0;
                pt(&mut c, map.pt(*q) - Vec2::new(half, half));
                c.push(' ');
                num(&mut c, half * 2.0);
                c.push(' ');
                num(&mut c, half * 2.0);
                c.push_str(" re f\n");
            }
        }
    }
    flush(&mut c, &mut pending);
    c.push_str("Q\n");
    c
}

/// A PDF literal string with `(`, `)` and `\` escaped; non-ASCII becomes `?`.
fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for ch in s.chars().take(200) {
        match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// Assemble the file: header, five objects, cross-reference table and trailer.
/// Write pages into a PDF file.
fn assemble_pages(pages: &[PageOut], compress: bool, title: &str) -> Vec<u8> {
    let pack = |data: &[u8]| -> (Vec<u8>, &'static str) {
        if compress { (miniz_oxide::deflate::compress_to_vec_zlib(data, 6), " /Filter /FlateDecode") } else { (data.to_vec(), "") }
    };
    let stream = |dict: String, data: &[u8]| -> Vec<u8> {
        let mut out = dict.into_bytes();
        out.extend_from_slice(b"\nstream\n");
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    };
    // Object bodies; object n is objs[n - 1]. 1 = catalog, 2 = page tree (filled in last).
    let mut objs: Vec<Vec<u8>> = vec![Vec::new(), Vec::new()];
    let mut kids = Vec::new();
    for p in pages {
        let (data, filter) = pack(p.content.as_bytes());
        objs.push(stream(format!("<< /Length {}{filter} >>", data.len()), &data));
        let content_ref = objs.len();
        let mut xobjects = String::new();
        for (i, im) in p.images.iter().enumerate() {
            let n = (im.width as usize) * (im.height as usize);
            let rgb: Vec<u8> = im.rgba.chunks_exact(4).take(n).flat_map(|c| [c[0], c[1], c[2]]).collect();
            let alpha: Vec<u8> = im.rgba.chunks_exact(4).take(n).map(|c| c[3]).collect();
            let smask = if alpha.iter().any(|a| *a < 255) {
                let (data, filter) = pack(&alpha);
                objs.push(stream(
                    format!(
                        "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {}{filter} >>",
                        im.width,
                        im.height,
                        data.len()
                    ),
                    &data,
                ));
                format!(" /SMask {} 0 R", objs.len())
            } else {
                String::new()
            };
            let (data, filter) = pack(&rgb);
            objs.push(stream(
                format!(
                    "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8{smask} /Length {}{filter} >>",
                    im.width,
                    im.height,
                    data.len()
                ),
                &data,
            ));
            let _ = write!(xobjects, "/Im{} {} 0 R ", i + 1, objs.len());
        }
        let mut mb = String::new();
        num(&mut mb, p.media.x);
        mb.push(' ');
        num(&mut mb, p.media.y);
        let resources = if xobjects.is_empty() { "<< >>".to_string() } else { format!("<< /XObject << {xobjects}>> >>") };
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {mb}] /Resources {resources} /Contents {content_ref} 0 R >>").into_bytes());
        kids.push(objs.len());
    }
    objs.push(format!("<< /Producer (VNCCad) /Creator (VNCCad) /Title {} >>", pdf_string(title)).into_bytes());
    let info = objs.len();
    if let Some(c) = objs.get_mut(0) {
        *c = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    }
    if let Some(t) = objs.get_mut(1) {
        let k: Vec<String> = kids.iter().map(|k| format!("{k} 0 R")).collect();
        *t = format!("<< /Type /Pages /Kids [{}] /Count {} >>", k.join(" "), kids.len()).into_bytes();
    }
    let mut out: Vec<u8> = Vec::with_capacity(objs.iter().map(Vec::len).sum::<usize>() + 1024);
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(objs.len());
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(x, "{o:010} 00000 n ");
    }
    let _ = write!(x, "trailer\n<< /Size {} /Root 1 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    out.extend_from_slice(x.as_bytes());
    out
}
