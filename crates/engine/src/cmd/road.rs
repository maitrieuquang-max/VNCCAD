//! VNCCad road and bridge tools.
//!
//! - VNLAYERS: a road / bridge layer set whose linetypes and lineweights follow the line rules of
//!   TCVN 8-20:2002 (ISO 128-20): thick continuous 0.5 mm for visible outlines and design lines,
//!   thin 0.25 / 0.18 mm for ground lines, dimensions and text, thin chain lines for axes and
//!   centre lines, dashed for hidden edges. Layer names are a VNCCad convention.
//! - TRACDOC: a longitudinal profile from a table of stakes (name, chainage, ground level,
//!   design level) with the data band underneath.
//! - TRACNGANG: cross sections (ground and design lines per stake) with cut and fill areas.
//! - BANGKL: a cut / fill quantity table by the average end area method, as a TABLE.
//!
//! Data come as text pasted from a spreadsheet (tab, semicolon or comma separated; a decimal
//! comma is accepted when the separator is a tab or semicolon) or as JSON rows. Chainages may be
//! written "Km1+250.5", "1+250.5" or 1250.5.

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, Layer, Line, Lineweight, LwPolyline, Table, TableCell, Text, VAlign};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("vnlayers", "Road & Bridge Layers (TCVN)", run_vnlayers)
            .menu(&["Format", "Road & Bridge Layers (TCVN)"])
            .alias(&["lopduong", "layertcvn"])
            .params("{set?: \"duong\" | \"cau\" | \"all\" (default all)}"),
        CommandSpec::new("tracdoc", "Longitudinal Profile", run_tracdoc)
            .menu(&["Draw", "Road", "Longitudinal Profile"])
            .alias(&["td", "profile"])
            .params("{data: text (Tên cọc, Lý trình, Cao độ TN, Cao độ TK?) | rows: [[name, chainage, ground, design?]], hscale?: 1000, vscale?: 100, origin?: [x,y], datum?: m, title?}"),
        CommandSpec::new("tracngang", "Cross Sections", run_tracngang)
            .menu(&["Draw", "Road", "Cross Sections"])
            .alias(&["tn", "xsection"])
            .params("{data: text, one line per ground or design line: Tên cọc, Lý trình, TN|TK, k/c1, cao độ1, k/c2, cao độ2… | sections: [{name, chainage, ground: [[off,z]…], design: [[off,z]…]}], scale?: 200, origin?: [x,y], columns?: 3, table?: bool}"),
        CommandSpec::new("bangkl", "Earthwork Quantity Table", run_bangkl)
            .menu(&["Draw", "Road", "Earthwork Quantity Table"])
            .alias(&["kldaodap", "earthwork"])
            .params("{data: text (Tên cọc, Lý trình, F đào, F đắp) | rows: [[name, chainage, cut, fill]], insert?: [x,y], scale?: 100}"),
    ]
}

// --- layers ------------------------------------------------------------------------------------

/// (name, colour index, linetype, lineweight in 1/100 mm, description).
type LayerDef = (&'static str, u8, &'static str, u16, &'static str);

const ROAD_LAYERS: [LayerDef; 15] = [
    ("TIM_TUYEN", 1, "CENTER", 25, "Tim tuyến – nét chấm gạch mảnh"),
    ("MEP_DUONG", 7, "Continuous", 50, "Mép mặt đường – nét liền đậm"),
    ("LE_DUONG", 8, "Continuous", 25, "Mép lề đường"),
    ("TALUY", 3, "Continuous", 25, "Chân/đỉnh mái ta luy"),
    ("COC", 2, "Continuous", 18, "Cọc, lý trình"),
    ("DIA_HINH", 9, "Continuous", 13, "Địa hình, địa vật"),
    ("DUONG_DONG_MUC", 31, "Continuous", 13, "Đường đồng mức"),
    ("TRAC_DOC_TN", 3, "Continuous", 25, "Trắc dọc – mặt đất tự nhiên"),
    ("TRAC_DOC_TK", 1, "Continuous", 50, "Trắc dọc – đường đỏ thiết kế"),
    ("TRAC_NGANG_TN", 3, "Continuous", 25, "Trắc ngang – mặt đất tự nhiên"),
    ("TRAC_NGANG_TK", 1, "Continuous", 50, "Trắc ngang – thiết kế"),
    ("BANG_SO_LIEU", 7, "Continuous", 18, "Bảng số liệu, lưới"),
    ("KICH_THUOC", 4, "Continuous", 18, "Kích thước – nét liền mảnh"),
    ("CHU", 7, "Continuous", 18, "Chữ, ghi chú"),
    ("KHUNG_TEN", 7, "Continuous", 50, "Khung tên"),
];

const BRIDGE_LAYERS: [LayerDef; 7] = [
    ("TRUC", 1, "CENTER", 18, "Trục, tim kết cấu – nét chấm gạch mảnh"),
    ("KC_BETONG", 7, "Continuous", 50, "Đường bao kết cấu – nét liền đậm"),
    ("KC_COT_THEP", 1, "Continuous", 35, "Cốt thép"),
    ("KC_KHUAT", 8, "HIDDEN", 25, "Cạnh khuất – nét đứt"),
    ("COC_KHOAN", 5, "Continuous", 35, "Cọc khoan nhồi, cọc đóng"),
    ("MAT_CAT", 6, "Continuous", 25, "Ký hiệu mặt cắt"),
    ("VAT_LIEU", 8, "Continuous", 13, "Mặt cắt vật liệu (hatch)"),
];

fn ensure_linetype(d: &mut Drawing, name: &str) {
    if name.eq_ignore_ascii_case("Continuous") || d.linetype(name).is_some() {
        return;
    }
    if let Some(lt) = cadcraft_doc::library::standard_linetypes().into_iter().find(|l| l.name.eq_ignore_ascii_case(name)) {
        d.linetypes.push(lt);
    }
}

/// Create (or update) the layers of a set. Returns how many were added.
fn add_layers(d: &mut Drawing, defs: &[LayerDef]) -> usize {
    let mut added = 0;
    for (name, color, lt, lw, desc) in defs {
        ensure_linetype(d, lt);
        let l = Layer {
            name: (*name).to_string(),
            color: Color::Index(*color),
            linetype: (*lt).to_string(),
            lineweight: Lineweight::Mm100(*lw),
            description: (*desc).to_string(),
            ..Layer::default()
        };
        match d.layers.iter_mut().find(|x| x.name.eq_ignore_ascii_case(name)) {
            Some(x) => {
                x.color = l.color;
                x.linetype = l.linetype;
                x.lineweight = l.lineweight;
                x.description = l.description;
            }
            None => {
                d.layers.push(l);
                added += 1;
            }
        }
    }
    added
}

fn run_vnlayers(s: &mut Session, p: &Value) -> Result<Value> {
    let set = str_param(p, "set").unwrap_or("all").to_ascii_lowercase();
    let d = s.doc_mut()?;
    let n = match set.as_str() {
        "duong" | "road" => add_layers(d, &ROAD_LAYERS),
        "cau" | "bridge" => add_layers(d, &BRIDGE_LAYERS),
        "all" | "" => add_layers(d, &ROAD_LAYERS) + add_layers(d, &BRIDGE_LAYERS),
        other => return Err(bad("vnlayers", format!("unknown set `{other}` (duong, cau, all)"))),
    };
    let msg = format!("Đã tạo {n} layer mới (nét vẽ theo TCVN 8-20:2002); các layer đã có được cập nhật màu, kiểu và độ dày nét.");
    Ok(json!({ "added": n, "message": msg }))
}

// --- parsing -----------------------------------------------------------------------------------

/// Split pasted text into rows of cells.
pub fn parse_rows(text: &str) -> Vec<Vec<String>> {
    let sep = if text.contains('\t') {
        '\t'
    } else if text.contains(';') {
        ';'
    } else {
        ','
    };
    text.lines()
        .take(100_000)
        .map(|l| l.split(sep).map(|c| c.trim().trim_matches('"').to_string()).collect::<Vec<_>>())
        .filter(|r| r.iter().any(|c| !c.is_empty()))
        .collect()
}

/// A number written with a decimal point or comma.
pub fn num(s: &str) -> Option<f64> {
    let t = s.trim().replace(' ', "");
    if t.is_empty() {
        return None;
    }
    let t = if t.contains(',') && !t.contains('.') { t.replace(',', ".") } else { t.replace(',', "") };
    t.parse::<f64>().ok().filter(|x| x.is_finite())
}

/// A chainage: "Km1+250.5", "1+250,5" or "1250.5" → metres.
pub fn chainage(s: &str) -> Option<f64> {
    let t = s.trim().to_ascii_lowercase().replace("km", "");
    match t.split_once('+') {
        Some((km, m)) => Some(num(km)? * 1000.0 + num(m)?),
        None => num(&t),
    }
}

/// "Km1+250.50".
pub fn format_chainage(m: f64) -> String {
    let km = (m / 1000.0).floor();
    let rest = m - km * 1000.0;
    format!("Km{}+{:06.2}", km as i64, rest)
}

fn cells_of(p: &Value) -> Vec<Vec<String>> {
    if let Some(t) = str_param(p, "data") {
        return parse_rows(t);
    }
    p.get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_array)
                .map(|r| r.iter().map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_string)).collect())
                .collect()
        })
        .unwrap_or_default()
}

// --- drawing helpers ---------------------------------------------------------------------------

#[derive(Default)]
struct Gen {
    out: Vec<(String, EntityKind)>,
}

impl Gen {
    fn line(&mut self, layer: &str, a: Vec2, b: Vec2) {
        self.out.push((layer.into(), EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) })));
    }
    fn pline(&mut self, layer: &str, pts: &[Vec2], closed: bool) {
        if pts.len() < 2 {
            return;
        }
        self.out.push((
            layer.into(),
            EntityKind::LwPolyline(LwPolyline {
                vertices: pts.iter().map(|p| PolyVertex::new(*p)).collect(),
                closed,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
        ));
    }
    fn text(&mut self, layer: &str, at: Vec2, h: f64, s: &str, rot_deg: f64, ha: HAlign, va: VAlign) {
        let p = Vec3::new(at.x, at.y, 0.0);
        let aligned = !(ha == HAlign::Left && va == VAlign::Baseline);
        self.out.push((
            layer.into(),
            EntityKind::Text(Text {
                insert: p,
                align_pt: aligned.then_some(p),
                height: h,
                value: s.to_string(),
                rotation: rot_deg.to_radians(),
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign: ha,
                valign: va,
            }),
        ));
    }
    /// Add everything to the active space, creating the layers used.
    fn commit(self, s: &mut Session) -> Result<usize> {
        let space = s.space();
        let d = s.doc_mut()?;
        add_layers(d, &ROAD_LAYERS);
        let n = self.out.len();
        for (layer, kind) in self.out {
            d.add(&space, Common { layer, ..Common::default() }, kind).map_err(|e| bad("road", e.to_string()))?;
        }
        Ok(n)
    }
}

fn fmt2(x: f64) -> String {
    format!("{x:.2}")
}

fn origin(p: &Value, s: &Session) -> Vec2 {
    match p.get("origin").or_else(|| p.get("insert")).and_then(Value::as_array) {
        Some(a) => Vec2::new(a.first().and_then(Value::as_f64).unwrap_or(0.0), a.get(1).and_then(Value::as_f64).unwrap_or(0.0)),
        None => s.state().map(|st| st.view().center).unwrap_or(Vec2::ZERO),
    }
}

// --- longitudinal profile ----------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Stake {
    name: String,
    ch: f64,
    ground: f64,
    design: Option<f64>,
}

fn stakes(rows: &[Vec<String>]) -> Vec<Stake> {
    let mut v: Vec<Stake> = rows
        .iter()
        .filter_map(|r| {
            let ch = chainage(r.get(1)?)?;
            let ground = num(r.get(2)?)?;
            Some(Stake { name: r.first().cloned().unwrap_or_default(), ch, ground, design: r.get(3).and_then(|c| num(c)) })
        })
        .collect();
    v.sort_by(|a, b| a.ch.total_cmp(&b.ch));
    v
}

fn run_tracdoc(s: &mut Session, p: &Value) -> Result<Value> {
    let st = stakes(&cells_of(p));
    if st.len() < 2 {
        return Err(bad("tracdoc", "cần ít nhất 2 cọc: Tên cọc, Lý trình, Cao độ TN, Cao độ TK"));
    }
    let hscale = p.get("hscale").and_then(Value::as_f64).filter(|x| *x > 0.0).unwrap_or(1000.0);
    let vscale = p.get("vscale").and_then(Value::as_f64).filter(|x| *x > 0.0).unwrap_or(100.0);
    let k = hscale / 1000.0; // drawing metres per millimetre of paper
    let ex = hscale / vscale; // vertical exaggeration
    let o = origin(p, s);
    let zmin = st.iter().map(|x| x.ground.min(x.design.unwrap_or(x.ground))).fold(f64::MAX, f64::min);
    let zmax = st.iter().map(|x| x.ground.max(x.design.unwrap_or(x.ground))).fold(f64::MIN, f64::max);
    let datum = p.get("datum").and_then(Value::as_f64).unwrap_or_else(|| (zmin - 2.0).floor());
    let ch0 = st.first().map_or(0.0, |x| x.ch);
    let x = |ch: f64| o.x + (ch - ch0);
    let y = |z: f64| o.y + (z - datum) * ex;
    let (th, lh) = (2.5 * k, 3.0 * k);
    let label_w = 62.0 * k;
    // Data band rows (label, height in paper mm); values are written up along each stake line.
    let rows: [(&str, f64); 6] = [
        ("Tên cọc", 14.0),
        ("Cao độ thiết kế (m)", 18.0),
        ("Cao độ tự nhiên (m)", 18.0),
        ("Đắp(+) / Đào(-) (m)", 16.0),
        ("Khoảng cách lẻ (m)", 10.0),
        ("Lý trình", 26.0),
    ];
    let mut tops = Vec::with_capacity(rows.len() + 1);
    let mut yy = o.y;
    for (_, h) in &rows {
        tops.push(yy);
        yy -= h * k;
    }
    let band_bottom = yy;
    tops.push(band_bottom);
    let mid = |i: usize| (tops.get(i).copied().unwrap_or(o.y) + tops.get(i + 1).copied().unwrap_or(o.y)) / 2.0;
    let x_end = x(st.last().map_or(ch0, |s| s.ch));
    let mut g = Gen::default();
    for t in &tops {
        g.line("BANG_SO_LIEU", Vec2::new(o.x - label_w, *t), Vec2::new(x_end, *t));
    }
    g.line("BANG_SO_LIEU", Vec2::new(o.x - label_w, o.y), Vec2::new(o.x - label_w, band_bottom));
    g.line("BANG_SO_LIEU", Vec2::new(x_end, o.y), Vec2::new(x_end, band_bottom));
    for (i, (label, _)) in rows.iter().enumerate() {
        g.text("CHU", Vec2::new(o.x - label_w + 2.0 * k, mid(i)), lh, label, 0.0, HAlign::Left, VAlign::Middle);
    }
    g.text("CHU", Vec2::new(o.x - label_w, o.y + 2.0 * k), lh, &format!("MSS: {datum:.2} m"), 0.0, HAlign::Left, VAlign::Bottom);
    // Stakes: vertical lines; values just left of each line, reading upwards.
    let beside = 0.8 * k;
    let mut prev: Option<f64> = None;
    for sk in &st {
        let xx = x(sk.ch);
        let top = y(sk.ground.max(sk.design.unwrap_or(sk.ground)));
        g.line("COC", Vec2::new(xx, band_bottom), Vec2::new(xx, top));
        let mut put = |i: usize, s: &str| g.text("CHU", Vec2::new(xx - beside, mid(i)), th, s, 90.0, HAlign::Center, VAlign::Bottom);
        put(0, &sk.name);
        if let Some(dz) = sk.design {
            put(1, &fmt2(dz));
            put(3, &format!("{:+.2}", dz - sk.ground));
        }
        put(2, &fmt2(sk.ground));
        put(5, &format_chainage(sk.ch));
        if let Some(pc) = prev {
            let m = (x(pc) + xx) / 2.0;
            g.text("CHU", Vec2::new(m, mid(4)), th, &fmt2(sk.ch - pc), 0.0, HAlign::Center, VAlign::Middle);
        }
        prev = Some(sk.ch);
    }
    // Ground and design lines.
    let ground: Vec<Vec2> = st.iter().map(|sk| Vec2::new(x(sk.ch), y(sk.ground))).collect();
    g.pline("TRAC_DOC_TN", &ground, false);
    let design: Vec<Vec2> = st.iter().filter_map(|sk| sk.design.map(|z| Vec2::new(x(sk.ch), y(z)))).collect();
    g.pline("TRAC_DOC_TK", &design, false);
    // Datum line and title.
    g.line("BANG_SO_LIEU", Vec2::new(o.x, o.y), Vec2::new(x_end, o.y));
    let title = str_param(p, "title").unwrap_or("TRẮC DỌC TUYẾN");
    g.text("CHU", Vec2::new((o.x + x_end) / 2.0, y(zmax) + 12.0 * k), 5.0 * k, title, 0.0, HAlign::Center, VAlign::Bottom);
    g.text(
        "CHU",
        Vec2::new((o.x + x_end) / 2.0, y(zmax) + 6.0 * k),
        th,
        &format!("Tỷ lệ: ngang 1/{hscale:.0}, đứng 1/{vscale:.0}"),
        0.0,
        HAlign::Center,
        VAlign::Bottom,
    );
    let n = g.commit(s)?;
    let length = st.last().map_or(0.0, |l| l.ch) - ch0;
    let msg = format!("Đã vẽ trắc dọc {} cọc, dài {:.2} m (MSS {datum:.2}).", st.len(), length);
    Ok(json!({ "entities": n, "stakes": st.len(), "length": length, "datum": datum, "message": msg }))
}

// --- cross sections ----------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Section {
    name: String,
    ch: f64,
    ground: Vec<(f64, f64)>,
    design: Vec<(f64, f64)>,
}

fn pairs(cells: &[String]) -> Vec<(f64, f64)> {
    let nums: Vec<f64> = cells.iter().filter_map(|c| num(c)).collect();
    let mut v: Vec<(f64, f64)> = nums.chunks_exact(2).filter_map(|c| Some((*c.first()?, *c.get(1)?))).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

fn sections_from_rows(rows: &[Vec<String>]) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for r in rows {
        let (Some(name), Some(ch), Some(kind)) = (r.first(), r.get(1).and_then(|c| chainage(c)), r.get(2)) else { continue };
        let pts = pairs(r.get(3..).unwrap_or(&[]));
        let idx = match out.iter().position(|s| s.name == *name) {
            Some(i) => i,
            None => {
                out.push(Section { name: name.clone(), ch, ..Section::default() });
                out.len() - 1
            }
        };
        if let Some(sec) = out.get_mut(idx) {
            match kind.trim().to_ascii_uppercase().as_str() {
                "TK" | "DESIGN" | "THIET KE" | "THIẾT KẾ" => sec.design = pts,
                _ => sec.ground = pts,
            }
        }
    }
    out.sort_by(|a, b| a.ch.total_cmp(&b.ch));
    out
}

fn sections_from_json(v: &Value) -> Vec<Section> {
    let pts = |x: Option<&Value>| -> Vec<(f64, f64)> {
        let mut p: Vec<(f64, f64)> = x
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|q| Some((q.get(0)?.as_f64()?, q.get(1)?.as_f64()?))).collect())
            .unwrap_or_default();
        p.sort_by(|a, b| a.0.total_cmp(&b.0));
        p
    };
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|s| Section {
                    name: s.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    ch: s.get("chainage").and_then(|c| c.as_f64().or_else(|| c.as_str().and_then(chainage))).unwrap_or(0.0),
                    ground: pts(s.get("ground")),
                    design: pts(s.get("design")),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Level of a polyline at offset `x` (linear between points; `None` outside it).
fn level_at(line: &[(f64, f64)], x: f64) -> Option<f64> {
    line.windows(2).find_map(|w| {
        let (a, b) = (w.first()?, w.get(1)?);
        (x >= a.0 && x <= b.0).then(|| if (b.0 - a.0).abs() < 1e-12 { a.1 } else { a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0) })
    })
}

/// Cut and fill areas between ground and design over the offsets both cover.
pub fn cut_fill(ground: &[(f64, f64)], design: &[(f64, f64)]) -> (f64, f64) {
    let (Some(g0), Some(g1), Some(d0), Some(d1)) = (ground.first(), ground.last(), design.first(), design.last()) else { return (0.0, 0.0) };
    let lo = g0.0.max(d0.0);
    let hi = g1.0.min(d1.0);
    if hi <= lo {
        return (0.0, 0.0);
    }
    let mut xs: Vec<f64> = ground.iter().chain(design).map(|p| p.0).filter(|x| *x >= lo && *x <= hi).collect();
    xs.push(lo);
    xs.push(hi);
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let (mut cut, mut fill) = (0.0, 0.0);
    for w in xs.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
        let (Some(ga), Some(gb), Some(da), Some(db)) = (level_at(ground, a), level_at(ground, b), level_at(design, a), level_at(design, b)) else {
            continue;
        };
        let (h0, h1) = (da - ga, db - gb); // + fill, - cut
        let dx = b - a;
        if h0 * h1 >= 0.0 {
            let area = (h0 + h1) / 2.0 * dx;
            if area >= 0.0 {
                fill += area;
            } else {
                cut -= area;
            }
        } else {
            // The lines cross inside this interval.
            let t = h0 / (h0 - h1);
            let (a1, a2) = (h0 / 2.0 * dx * t, h1 / 2.0 * dx * (1.0 - t));
            for ar in [a1, a2] {
                if ar >= 0.0 {
                    fill += ar;
                } else {
                    cut -= ar;
                }
            }
        }
    }
    (cut, fill)
}

fn run_tracngang(s: &mut Session, p: &Value) -> Result<Value> {
    let secs = match p.get("sections") {
        Some(v) => sections_from_json(v),
        None => sections_from_rows(&cells_of(p)),
    };
    let secs: Vec<Section> = secs.into_iter().filter(|s| s.ground.len() >= 2).collect();
    if secs.is_empty() {
        return Err(bad("tracngang", "không có mặt cắt nào: mỗi dòng gồm Tên cọc, Lý trình, TN hoặc TK, rồi các cặp khoảng cách – cao độ"));
    }
    let scale = p.get("scale").and_then(Value::as_f64).filter(|x| *x > 0.0).unwrap_or(200.0);
    let k = scale / 1000.0;
    let cols = p.get("columns").and_then(Value::as_u64).unwrap_or(3).clamp(1, 20) as usize;
    let o = origin(p, s);
    let offs = secs.iter().flat_map(|s| s.ground.iter().chain(&s.design)).map(|q| q.0);
    let (omin, omax) = offs.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let dzmax = secs
        .iter()
        .map(|s| {
            let z = s.ground.iter().chain(&s.design).map(|q| q.1);
            let (lo, hi) = z.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
            hi - lo
        })
        .fold(0.0, f64::max);
    let row_h = 8.0 * k;
    let cell_w = (omax - omin) + 50.0 * k;
    let cell_h = dzmax + 2.0 + 3.0 * row_h + 30.0 * k;
    let (th, lh) = (2.0 * k, 3.5 * k);
    let mut g = Gen::default();
    let mut results = Vec::new();
    for (i, sec) in secs.iter().enumerate() {
        let (c, r) = (i % cols, i / cols);
        // Datum point of this section: centre-line x, datum y.
        let cx = o.x + c as f64 * cell_w - omin + 25.0 * k;
        let base_y = o.y - r as f64 * cell_h;
        let zlo = sec.ground.iter().chain(&sec.design).map(|q| q.1).fold(f64::MAX, f64::min);
        let datum = (zlo - 1.0).floor();
        let pt = |q: &(f64, f64)| Vec2::new(cx + q.0, base_y + (q.1 - datum));
        let lmin = sec.ground.first().map_or(0.0, |q| q.0).min(sec.design.first().map_or(0.0, |q| q.0));
        let lmax = sec.ground.last().map_or(0.0, |q| q.0).max(sec.design.last().map_or(0.0, |q| q.0));
        // Datum line, band and labels.
        g.line("BANG_SO_LIEU", Vec2::new(cx + lmin, base_y), Vec2::new(cx + lmax, base_y));
        for j in 1..=2 {
            g.line("BANG_SO_LIEU", Vec2::new(cx + lmin, base_y - row_h * f64::from(j)), Vec2::new(cx + lmax, base_y - row_h * f64::from(j)));
        }
        g.text("CHU", Vec2::new(cx + lmin - 2.0 * k, base_y - row_h * 0.5), th, "Cao độ TN", 0.0, HAlign::Right, VAlign::Middle);
        g.text("CHU", Vec2::new(cx + lmin - 2.0 * k, base_y - row_h * 1.5), th, "Khoảng cách", 0.0, HAlign::Right, VAlign::Middle);
        g.text("CHU", Vec2::new(cx + lmin, base_y + 1.0 * k), th, &format!("MSS {datum:.2}"), 0.0, HAlign::Left, VAlign::Bottom);
        for q in &sec.ground {
            let at = pt(q);
            g.line("COC", Vec2::new(at.x, base_y - row_h * 2.0), Vec2::new(at.x, base_y));
            g.text("CHU", Vec2::new(at.x, base_y - row_h * 0.5), th * 0.9, &fmt2(q.1), 90.0, HAlign::Center, VAlign::Middle);
            g.text("CHU", Vec2::new(at.x, base_y - row_h * 1.5), th * 0.9, &format!("{:.1}", q.0), 90.0, HAlign::Center, VAlign::Middle);
        }
        // Centre line, ground, design.
        let top = sec.ground.iter().chain(&sec.design).map(|q| q.1).fold(f64::MIN, f64::max);
        g.line("TIM_TUYEN", Vec2::new(cx, base_y - row_h * 2.0), Vec2::new(cx, base_y + (top - datum) + 6.0 * k));
        let gpts: Vec<Vec2> = sec.ground.iter().map(pt).collect();
        g.pline("TRAC_NGANG_TN", &gpts, false);
        let dpts: Vec<Vec2> = sec.design.iter().map(pt).collect();
        g.pline("TRAC_NGANG_TK", &dpts, false);
        let (cut, fill) = cut_fill(&sec.ground, &sec.design);
        let title_y = base_y + (top - datum) + 10.0 * k;
        g.text(
            "CHU",
            Vec2::new(cx, title_y + lh * 1.6),
            lh,
            &format!("CỌC {} – {}", sec.name, format_chainage(sec.ch)),
            0.0,
            HAlign::Center,
            VAlign::Bottom,
        );
        g.text("CHU", Vec2::new(cx, title_y), th, &format!("F đào = {cut:.2} m²   F đắp = {fill:.2} m²"), 0.0, HAlign::Center, VAlign::Bottom);
        results.push(json!({ "name": sec.name, "chainage": sec.ch, "cut": cut, "fill": fill }));
    }
    let n = g.commit(s)?;
    // Optional quantity table right below the sections.
    let mut table = Value::Null;
    if p.get("table").and_then(Value::as_bool).unwrap_or(false) {
        let rows: Vec<Vec<String>> = results
            .iter()
            .map(|r| vec![r["name"].as_str().unwrap_or("").to_string(), r["chainage"].to_string(), r["cut"].to_string(), r["fill"].to_string()])
            .collect();
        let rows_n = secs.len().div_ceil(cols);
        let at = Vec2::new(o.x, o.y - rows_n as f64 * cell_h);
        table = quantity_table(s, &rows, at, scale.min(100.0))?;
    }
    let msg = format!("Đã vẽ {} mặt cắt ngang.", secs.len());
    Ok(json!({ "entities": n, "sections": results, "table": table, "message": msg }))
}

// --- quantity table ----------------------------------------------------------------------------

fn quantity_table(s: &mut Session, rows: &[Vec<String>], at: Vec2, scale: f64) -> Result<Value> {
    let mut data: Vec<(String, f64, f64, f64)> =
        rows.iter().filter_map(|r| Some((r.first()?.clone(), chainage(r.get(1)?)?, num(r.get(2)?)?, num(r.get(3)?)?))).collect();
    data.sort_by(|a, b| a.1.total_cmp(&b.1));
    if data.is_empty() {
        return Err(bad("bangkl", "không có dòng số liệu: Tên cọc, Lý trình, F đào, F đắp"));
    }
    let k = scale / 1000.0;
    let mut cells: Vec<Vec<TableCell>> = Vec::new();
    let cell = |t: String| TableCell { text: t, merged: None };
    cells.push(vec![cell("BẢNG TỔNG HỢP KHỐI LƯỢNG ĐÀO ĐẮP NỀN ĐƯỜNG".into())]);
    cells.push(
        ["Tên cọc", "Lý trình", "K/c lẻ (m)", "F đào (m²)", "F đắp (m²)", "V đào (m³)", "V đắp (m³)"]
            .iter()
            .map(|h| cell((*h).to_string()))
            .collect(),
    );
    let (mut tv_cut, mut tv_fill, mut tlen) = (0.0, 0.0, 0.0);
    let mut prev: Option<&(String, f64, f64, f64)> = None;
    let mut out_rows = Vec::new();
    for d in &data {
        let (len, vc, vf) = match prev {
            Some(p) => {
                let l = d.1 - p.1;
                (l, (p.2 + d.2) / 2.0 * l, (p.3 + d.3) / 2.0 * l)
            }
            None => (0.0, 0.0, 0.0),
        };
        tv_cut += vc;
        tv_fill += vf;
        tlen += len;
        cells.push(vec![
            cell(d.0.clone()),
            cell(format_chainage(d.1)),
            cell(if prev.is_some() { fmt2(len) } else { String::new() }),
            cell(fmt2(d.2)),
            cell(fmt2(d.3)),
            cell(if prev.is_some() { fmt2(vc) } else { String::new() }),
            cell(if prev.is_some() { fmt2(vf) } else { String::new() }),
        ]);
        out_rows.push(json!({ "name": d.0, "chainage": d.1, "length": len, "cutVolume": vc, "fillVolume": vf }));
        prev = Some(d);
    }
    cells.push(vec![
        cell("Tổng cộng".into()),
        cell(String::new()),
        cell(fmt2(tlen)),
        cell(String::new()),
        cell(String::new()),
        cell(fmt2(tv_cut)),
        cell(fmt2(tv_fill)),
    ]);
    if let Some(first) = cells.first_mut() {
        first.resize(7, cell(String::new()));
        if let Some(c) = first.first_mut() {
            c.merged = Some((1, 7));
        }
    }
    let table = Table {
        insert: Vec3::new(at.x, at.y, 0.0),
        col_widths: [18.0, 28.0, 20.0, 20.0, 20.0, 22.0, 22.0].iter().map(|w| w * k).collect(),
        row_heights: vec![8.0 * k; cells.len()],
        cells,
        style: "Standard".into(),
        text_height: 2.5 * k,
        title: true,
        header: true,
    };
    let space = s.space();
    let d = s.doc_mut()?;
    add_layers(d, &ROAD_LAYERS);
    let h = d
        .add(&space, Common { layer: "BANG_SO_LIEU".into(), ..Common::default() }, EntityKind::Table(table))
        .map_err(|e| bad("bangkl", e.to_string()))?;
    Ok(json!({ "handle": h.hex(), "rows": out_rows, "cutVolume": tv_cut, "fillVolume": tv_fill, "length": tlen }))
}

fn run_bangkl(s: &mut Session, p: &Value) -> Result<Value> {
    let rows = cells_of(p);
    let at = origin(p, s);
    let scale = p.get("scale").and_then(Value::as_f64).filter(|x| *x > 0.0).unwrap_or(100.0);
    let r = quantity_table(s, &rows, at, scale)?;
    let msg = format!(
        "Bảng khối lượng: V đào = {:.2} m³, V đắp = {:.2} m³ trên {:.2} m.",
        r["cutVolume"].as_f64().unwrap_or(0.0),
        r["fillVolume"].as_f64().unwrap_or(0.0),
        r["length"].as_f64().unwrap_or(0.0)
    );
    let mut r = r;
    if let Some(o) = r.as_object_mut() {
        o.insert("message".into(), json!(msg));
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(chainage("Km1+250.5"), Some(1250.5));
        assert_eq!(chainage("km0+050,25"), Some(50.25));
        assert_eq!(chainage("1250"), Some(1250.0));
        assert_eq!(format_chainage(1250.5), "Km1+250.50");
        assert_eq!(format_chainage(50.0), "Km0+050.00");
        let rows = parse_rows("Cọc\tLý trình\tTN\tTK\nC1\tKm0+000\t12,5\t13,2\nC2\tKm0+050\t12,1\t13,0\n");
        assert_eq!(rows.len(), 3);
        let st = stakes(&rows);
        assert_eq!(st.len(), 2, "header row skipped");
        assert_eq!(st[1].ch, 50.0);
        assert_eq!(st[0].ground, 12.5);
        assert_eq!(st[0].design, Some(13.2));
    }

    #[test]
    fn cut_fill_areas() {
        // Flat ground at 10, design at 11 over 0..10: 10 m² fill.
        assert_eq!(cut_fill(&[(-5.0, 10.0), (15.0, 10.0)], &[(0.0, 11.0), (10.0, 11.0)]), (0.0, 10.0));
        // Design crosses the ground in the middle: triangles of 2.5 m² each side.
        let (c, f) = cut_fill(&[(0.0, 10.0), (10.0, 10.0)], &[(0.0, 11.0), (10.0, 9.0)]);
        assert!((c - 2.5).abs() < 1e-9 && (f - 2.5).abs() < 1e-9, "{c} {f}");
    }

    #[test]
    fn profile_sections_table_and_layers() {
        let mut s = Session::new();
        let r = s.execute("vnlayers", &json!({})).unwrap();
        assert!(r["added"].as_u64().unwrap() >= 20);
        assert_eq!(s.doc().unwrap().layer("TIM_TUYEN").map(|l| l.linetype.as_str()), Some("CENTER"));
        assert!(s.doc().unwrap().linetype("CENTER").is_some());
        let r = s
            .execute(
                "tracdoc",
                &json!({"data": "C0;Km0+000;10,0;11,0\nC1;Km0+050;11,5;11,5\nC2;Km0+100;12,0;12,0\nC3;Km0+120;9,0;12,4", "origin": [0, 0]}),
            )
            .unwrap();
        assert_eq!(r["stakes"], 4);
        assert_eq!(r["length"], 120.0);
        assert_eq!(r["datum"], 7.0);
        let data = "C0,0,TN,-10,10,0,10,10,10\nC0,0,TK,-6,11,0,11,6,11\nC1,20,TN,-10,12,10,12\nC1,20,TK,-6,11,6,11";
        let r = s.execute("tracngang", &json!({"data": data, "origin": [0, -200], "table": true})).unwrap();
        let secs = r["sections"].as_array().unwrap();
        assert_eq!(secs.len(), 2);
        assert!((secs[0]["fill"].as_f64().unwrap() - 12.0).abs() < 1e-9);
        assert!((secs[1]["cut"].as_f64().unwrap() - 12.0).abs() < 1e-9);
        // Average end area: (12 + 0)/2 × 20 = 120 m³ each way.
        assert!((r["table"]["fillVolume"].as_f64().unwrap() - 120.0).abs() < 1e-9);
        assert!((r["table"]["cutVolume"].as_f64().unwrap() - 120.0).abs() < 1e-9);
        assert!(s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::Table(_))));
        assert!(s.execute("tracdoc", &json!({"data": "chỉ một dòng"})).is_err());
    }
}
