//! VNCCad: Google Earth export and import (KML / KMZ) with Vietnamese coordinate systems.
//!
//! - Projections: VN-2000 (Transverse Mercator on the WGS-84 ellipsoid, 3° zones k0 = 0.9999 or
//!   6° zones k0 = 0.9996, false easting 500 000 m) and UTM (WGS-84, k0 = 0.9996).
//! - Datum: VN-2000 → WGS-84 with the published 7-parameter Helmert transformation (Bộ TN&MT,
//!   QĐ 05/2007; EPSG transformation 6960, position-vector convention).
//! - KMZ is a ZIP archive holding `doc.kml`; written and read here (deflate via miniz_oxide).

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Space};
use cadcraft_geom::{Polyline, Vec2};

// ---------------------------------------------------------------- coordinate systems

const A: f64 = 6_378_137.0;
const F: f64 = 1.0 / 298.257_223_563;

/// A projected coordinate system for the drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct Crs {
    /// Central meridian (degrees).
    pub cm: f64,
    pub k0: f64,
    pub false_easting: f64,
    pub false_northing: f64,
    /// VN-2000 datum (Helmert shift to WGS-84) or plain WGS-84 (UTM).
    pub vn2000: bool,
    /// Drawing units per metre is `1 / unit_m` (1 = metres, 0.001 = millimetres).
    pub unit_m: f64,
    /// Human-readable name, e.g. "VN-2000 KTT 107°45' múi 3°".
    pub name: String,
}

/// Central meridians of VN-2000 3° zones by province (Thông tư 973/2001/TT-TCĐC), keyed by the
/// province name without diacritics. Provinces merged in 2025 keep their old names here.
const PROVINCES: &[(&str, f64)] = &[
    ("lai chau", 103.5),
    ("dien bien", 103.0),
    ("son la", 104.0),
    ("lao cai", 104.75),
    ("yen bai", 104.75),
    ("ha giang", 105.5),
    ("tuyen quang", 106.0),
    ("phu tho", 104.75),
    ("vinh phuc", 105.0),
    ("cao bang", 105.75),
    ("lang son", 107.25),
    ("bac kan", 106.5),
    ("thai nguyen", 106.5),
    ("bac giang", 107.0),
    ("bac ninh", 105.5),
    ("quang ninh", 107.75),
    ("hai phong", 105.75),
    ("hai duong", 105.5),
    ("hung yen", 105.5),
    ("ha noi", 105.0),
    ("hoa binh", 106.0),
    ("ha nam", 105.0),
    ("nam dinh", 105.5),
    ("thai binh", 105.5),
    ("ninh binh", 105.0),
    ("thanh hoa", 105.0),
    ("nghe an", 104.75),
    ("ha tinh", 105.5),
    ("quang binh", 106.0),
    ("quang tri", 106.25),
    ("thua thien hue", 107.0),
    ("hue", 107.0),
    ("da nang", 107.75),
    ("quang nam", 107.75),
    ("quang ngai", 108.0),
    ("binh dinh", 108.25),
    ("kon tum", 107.5),
    ("gia lai", 108.5),
    ("dak lak", 108.5),
    ("dak nong", 108.5),
    ("phu yen", 108.5),
    ("khanh hoa", 108.25),
    ("ninh thuan", 108.25),
    ("binh thuan", 108.5),
    ("lam dong", 107.75),
    ("binh phuoc", 106.25),
    ("binh duong", 105.75),
    ("dong nai", 107.75),
    ("ba ria vung tau", 107.75),
    ("vung tau", 107.75),
    ("tay ninh", 105.5),
    ("ho chi minh", 105.75),
    ("tp hcm", 105.75),
    ("hcm", 105.75),
    ("sai gon", 105.75),
    ("long an", 105.75),
    ("tien giang", 105.75),
    ("ben tre", 105.75),
    ("dong thap", 105.0),
    ("vinh long", 105.5),
    ("tra vinh", 105.5),
    ("an giang", 104.75),
    ("kien giang", 104.5),
    ("can tho", 105.0),
    ("hau giang", 105.0),
    ("soc trang", 105.5),
    ("bac lieu", 105.0),
    ("ca mau", 104.5),
];

/// Lower-case ASCII without Vietnamese diacritics (for province names).
pub fn fold(s: &str) -> String {
    const FROM: &str = "àáảãạăằắẳẵặâầấẩẫậèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵđ";
    const TO: &str = "aaaaaaaaaaaaaaaaaeeeeeeeeeeeiiiiiooooooooooooooooouuuuuuuuuuuyyyyyd";
    let to: Vec<char> = TO.chars().collect();
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        match FROM.chars().position(|x| x == c) {
            Some(i) => out.push(to.get(i).copied().unwrap_or(c)),
            None if c.is_ascii_alphanumeric() || c == '.' => out.push(c),
            None => out.push(' '),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Degrees from `107.75`, `107°45'`, `107 45`, `107d45m`.
fn parse_deg(t: &str) -> Option<f64> {
    let t = t.trim().replace(',', ".");
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    let nums: Vec<f64> = t.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
    match nums.as_slice() {
        [d] => Some(*d),
        [d, m] => Some(d + m / 60.0),
        [d, m, s, ..] => Some(d + m / 60.0 + s / 3600.0),
        _ => None,
    }
}

impl Crs {
    pub fn vn2000(cm: f64, zone6: bool) -> Crs {
        let d = cm.trunc();
        let m = ((cm - d) * 60.0).round();
        Crs {
            cm,
            k0: if zone6 { 0.9996 } else { 0.9999 },
            false_easting: 500_000.0,
            false_northing: 0.0,
            vn2000: true,
            unit_m: 1.0,
            name: format!("VN-2000 KTT {d}°{m:02}' múi {}°", if zone6 { 6 } else { 3 }),
        }
    }
    pub fn utm(zone: u32, north: bool) -> Crs {
        Crs {
            cm: f64::from(zone) * 6.0 - 183.0,
            k0: 0.9996,
            false_easting: 500_000.0,
            false_northing: if north { 0.0 } else { 10_000_000.0 },
            vn2000: false,
            unit_m: 1.0,
            name: format!("WGS-84 UTM {zone}{}", if north { "N" } else { "S" }),
        }
    }

    /// Parse `VN2000 107.75`, `VN2000 107°45' 3`, `VN2000 Đà Nẵng`, `VN2000 105 6`, `UTM 48N`,
    /// `UTM48`. A bare province name or meridian means VN-2000 3°.
    pub fn parse(text: &str) -> Option<Crs> {
        let t = fold(text);
        let t = t.trim();
        if t.is_empty() {
            return None;
        }
        if let Some(rest) = t.strip_prefix("utm") {
            let r = rest.trim();
            let digits: String = r.chars().take_while(char::is_ascii_digit).collect();
            let zone: u32 = digits.parse().ok().filter(|z| (1..=60).contains(z))?;
            let south = r[digits.len()..].trim().starts_with('s');
            return Some(Crs::utm(zone, !south));
        }
        let rest = t.strip_prefix("vn2000").or_else(|| t.strip_prefix("vn 2000")).or_else(|| t.strip_prefix("vn-2000")).unwrap_or(t).trim();
        let (rest, zone6) = if let Some(r) = rest.strip_suffix(" 6").or_else(|| rest.strip_suffix(" mui 6")) {
            (r.trim(), true)
        } else if let Some(r) = rest.strip_suffix(" 3").or_else(|| rest.strip_suffix(" mui 3")) {
            (r.trim(), false)
        } else {
            (rest, false)
        };
        if rest.is_empty() {
            return None;
        }
        let cm = PROVINCES
            .iter()
            .find(|(n, _)| rest == *n || rest.trim_start_matches("tinh ").trim_start_matches("tp ").trim_start_matches("thanh pho ") == *n)
            .map(|(_, c)| *c)
            .or_else(|| parse_deg(rest).filter(|d| (90.0..=120.0).contains(d)))?;
        Some(Crs::vn2000(cm, zone6))
    }

    /// The string kept in the drawing (header `VNC_CRS`).
    pub fn to_header(&self) -> String {
        if self.vn2000 {
            format!("VN2000 {} {}", self.cm, if (self.k0 - 0.9996).abs() < 1e-9 { 6 } else { 3 })
        } else {
            let zone = ((self.cm + 183.0) / 6.0).round() as i64;
            format!("UTM {zone}{}", if self.false_northing > 0.0 { "S" } else { "N" })
        }
    }

    /// Drawing point → (latitude, longitude) in WGS-84 degrees.
    pub fn to_wgs84(&self, p: Vec2) -> (f64, f64) {
        let e = p.x * self.unit_m - self.false_easting;
        let n = p.y * self.unit_m - self.false_northing;
        let (lat, lon) = tm_inverse(e, n, self.cm.to_radians(), self.k0);
        if !self.vn2000 {
            return (lat.to_degrees(), lon.to_degrees());
        }
        let (x, y, z) = geodetic_to_ecef(lat, lon, 0.0);
        let (x, y, z) = helmert(x, y, z, false);
        let (lat, lon, _) = ecef_to_geodetic(x, y, z);
        (lat.to_degrees(), lon.to_degrees())
    }

    /// (latitude, longitude) in WGS-84 degrees → drawing point.
    pub fn from_wgs84(&self, lat: f64, lon: f64) -> Vec2 {
        let (mut la, mut lo) = (lat.to_radians(), lon.to_radians());
        if self.vn2000 {
            let (x, y, z) = geodetic_to_ecef(la, lo, 0.0);
            let (x, y, z) = helmert(x, y, z, true);
            let (a, b, _) = ecef_to_geodetic(x, y, z);
            la = a;
            lo = b;
        }
        let (e, n) = tm_forward(la, lo, self.cm.to_radians(), self.k0);
        let k = if self.unit_m > 0.0 { 1.0 / self.unit_m } else { 1.0 };
        Vec2::new((e + self.false_easting) * k, (n + self.false_northing) * k)
    }
}

fn e2() -> f64 {
    F * (2.0 - F)
}

/// Transverse Mercator (Snyder, USGS PP 1395): (lat, lon) radians → (x, y) metres before false
/// easting/northing.
pub fn tm_forward(lat: f64, lon: f64, lon0: f64, k0: f64) -> (f64, f64) {
    let e2 = e2();
    let ep2 = e2 / (1.0 - e2);
    let (s, c) = lat.sin_cos();
    let n = A / (1.0 - e2 * s * s).sqrt();
    let t = (s / c).powi(2);
    let cc = ep2 * c * c;
    let a = (lon - lon0) * c;
    let m = meridian(lat);
    let x = k0 * n * (a + (1.0 - t + cc) * a.powi(3) / 6.0 + (5.0 - 18.0 * t + t * t + 72.0 * cc - 58.0 * ep2) * a.powi(5) / 120.0);
    let y = k0
        * (m + n
            * (s / c)
            * (a * a / 2.0
                + (5.0 - t + 9.0 * cc + 4.0 * cc * cc) * a.powi(4) / 24.0
                + (61.0 - 58.0 * t + t * t + 600.0 * cc - 330.0 * ep2) * a.powi(6) / 720.0));
    (x, y)
}

fn meridian(lat: f64) -> f64 {
    let e2 = e2();
    let (e4, e6) = (e2 * e2, e2 * e2 * e2);
    A * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * lat - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * lat).sin()
        + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * lat).sin()
        - (35.0 * e6 / 3072.0) * (6.0 * lat).sin())
}

/// Inverse Transverse Mercator: (x, y) metres (false easting/northing removed) → (lat, lon) radians.
pub fn tm_inverse(x: f64, y: f64, lon0: f64, k0: f64) -> (f64, f64) {
    let e2 = e2();
    let ep2 = e2 / (1.0 - e2);
    let (e4, e6) = (e2 * e2, e2 * e2 * e2);
    let m = y / k0;
    let mu = m / (A * (1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0));
    let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());
    let p1 = mu
        + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1 * e1 / 16.0 - 55.0 * e1.powi(4) / 32.0) * (4.0 * mu).sin()
        + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin()
        + (1097.0 * e1.powi(4) / 512.0) * (8.0 * mu).sin();
    let (s, c) = p1.sin_cos();
    let c1 = ep2 * c * c;
    let t1 = (s / c).powi(2);
    let n1 = A / (1.0 - e2 * s * s).sqrt();
    let r1 = A * (1.0 - e2) / (1.0 - e2 * s * s).powf(1.5);
    let d = x / (n1 * k0);
    let lat = p1
        - (n1 * (s / c) / r1)
            * (d * d / 2.0 - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * ep2) * d.powi(4) / 24.0
                + (61.0 + 90.0 * t1 + 298.0 * c1 + 45.0 * t1 * t1 - 252.0 * ep2 - 3.0 * c1 * c1) * d.powi(6) / 720.0);
    let lon = lon0
        + (d - (1.0 + 2.0 * t1 + c1) * d.powi(3) / 6.0
            + (5.0 - 2.0 * c1 + 28.0 * t1 - 3.0 * c1 * c1 + 8.0 * ep2 + 24.0 * t1 * t1) * d.powi(5) / 120.0)
            / c;
    (lat, lon)
}

fn geodetic_to_ecef(lat: f64, lon: f64, h: f64) -> (f64, f64, f64) {
    let e2 = e2();
    let n = A / (1.0 - e2 * lat.sin().powi(2)).sqrt();
    ((n + h) * lat.cos() * lon.cos(), (n + h) * lat.cos() * lon.sin(), (n * (1.0 - e2) + h) * lat.sin())
}

fn ecef_to_geodetic(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let e2 = e2();
    let p = x.hypot(y);
    let lon = y.atan2(x);
    let mut lat = z.atan2(p * (1.0 - e2));
    let mut h = 0.0;
    for _ in 0..10 {
        let n = A / (1.0 - e2 * lat.sin().powi(2)).sqrt();
        h = p / lat.cos() - n;
        lat = z.atan2(p * (1.0 - e2 * n / (n + h)));
    }
    (lat, lon, h)
}

/// VN-2000 ↔ WGS-84 (EPSG 6960, position vector): `inverse` = WGS-84 → VN-2000.
fn helmert(x: f64, y: f64, z: f64, inverse: bool) -> (f64, f64, f64) {
    const T: [f64; 3] = [-191.904_414_29, -39.303_182_79, -111.450_328_35];
    const R_SEC: [f64; 3] = [-0.009_288_36, 0.019_754_79, -0.004_273_72];
    const S_PPM: f64 = 0.252_906_278;
    let sec = std::f64::consts::PI / (180.0 * 3600.0);
    let (rx, ry, rz) = (R_SEC[0] * sec, R_SEC[1] * sec, R_SEC[2] * sec);
    let k = 1.0 + S_PPM * 1e-6;
    if !inverse {
        (T[0] + k * (x - rz * y + ry * z), T[1] + k * (rz * x + y - rx * z), T[2] + k * (-ry * x + rx * y + z))
    } else {
        let (u, v, w) = (x - T[0], y - T[1], z - T[2]);
        ((u + rz * v - ry * w) / k, (-rz * u + v + rx * w) / k, (ry * u - rx * v + w) / k)
    }
}

/// Guess drawing units: eastings around 500 km in metres are 1; in millimetres 0.001.
pub fn guess_unit(d: &Drawing) -> f64 {
    let ext = d.extents(&Space::Model);
    if ext.is_empty() {
        return 1.0;
    }
    let c = (ext.min + ext.max) * 0.5;
    if c.x.abs() > 5.0e7 && c.x.abs() < 1.0e9 { 0.001 } else { 1.0 }
}

// ---------------------------------------------------------------- KML writing

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Plain text of an MTEXT (inline codes dropped).
fn mtext_plain(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('P') => out.push(' '),
                Some('~') => out.push(' '),
                Some('\\') => out.push('\\'),
                Some('{') => out.push('{'),
                Some('}') => out.push('}'),
                Some(_) => {
                    for x in chars.by_ref() {
                        if x == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// What to export.
#[derive(Clone, Debug, Default)]
pub struct KmlFilter {
    /// Only these entities (empty = all of model space).
    pub handles: Vec<Handle>,
    /// Only these layers (empty = all visible layers).
    pub layers: Vec<String>,
    /// Labels for TEXT / MTEXT.
    pub text: bool,
}

struct Shape {
    layer: String,
    color: cadcraft_color::Rgb,
    lines: Vec<Vec<Vec2>>,
    polys: Vec<(Vec<Vec2>, Vec<Vec<Vec2>>)>,
    label: Option<(Vec2, String)>,
}

fn coords(crs: &Crs, pts: &[Vec2], out: &mut String) {
    for p in pts {
        let (lat, lon) = crs.to_wgs84(*p);
        if lat.is_finite() && lon.is_finite() {
            out.push_str(&format!("{lon:.8},{lat:.8},0 "));
        }
    }
}

fn hatch_polys(h: &cadcraft_doc::Hatch, tol: f64) -> Vec<(Vec<Vec2>, Vec<Vec<Vec2>>)> {
    let loops: Vec<Vec<Vec2>> = h
        .loops
        .iter()
        .take(10_000)
        .map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(tol))
        .filter(|l| l.len() >= 3)
        .collect();
    let inside = |p: Vec2, poly: &[Vec2]| {
        let n = poly.len();
        let mut c = false;
        for i in 0..n {
            let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
                c = !c;
            }
        }
        c
    };
    let depth: Vec<usize> = loops
        .iter()
        .enumerate()
        .map(|(i, l)| l.first().map(|p| loops.iter().enumerate().filter(|(j, q)| *j != i && inside(*p, q)).count()).unwrap_or(0))
        .collect();
    let mut out = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        if depth.get(i).copied().unwrap_or(0) % 2 != 0 {
            continue;
        }
        let d = depth.get(i).copied().unwrap_or(0);
        let holes: Vec<Vec<Vec2>> = loops
            .iter()
            .enumerate()
            .filter(|(j, q)| depth.get(*j).copied() == Some(d + 1) && q.first().is_some_and(|p| inside(*p, l)))
            .map(|(_, q)| q.clone())
            .collect();
        out.push((l.clone(), holes));
    }
    out
}

/// The drawing (model space) as KML.
pub fn to_kml(d: &Drawing, crs: &Crs, filter: &KmlFilter, title: &str) -> String {
    let tol = 0.05 / crs.unit_m.max(1e-12);
    let opts =
        cadcraft_render::Options { tolerance: tol, min_dash: f64::MAX, text: false, fill: false, lineweights: false, pens: None, anno_scale: 0.0 };
    let want_layer = |e: &Entity| filter.layers.is_empty() || filter.layers.iter().any(|l| l.eq_ignore_ascii_case(&e.common.layer));
    let layer_on = |e: &Entity| d.is_visible(e);
    let mut shapes: Vec<Shape> = Vec::new();
    let pick: Vec<&Entity> = if filter.handles.is_empty() {
        d.model.iter().map(|e| &**e).collect()
    } else {
        filter.handles.iter().filter_map(|h| d.entity(*h).map(|e| &**e)).collect()
    };
    for e in pick.into_iter().take(2_000_000) {
        if !want_layer(e) || !layer_on(e) {
            continue;
        }
        let layer = e.common.layer.clone();
        let layer_color = d.layer(&layer).map(|l| l.color).unwrap_or(cadcraft_color::Color::Index(7));
        let color = e.common.color.resolve(layer_color, cadcraft_color::Color::Index(7));
        match &e.kind {
            EntityKind::Text(t) if filter.text => {
                let at = t.align_pt.filter(|_| t.halign != cadcraft_doc::HAlign::Left).unwrap_or(t.insert).xy();
                shapes.push(Shape { layer, color, lines: Vec::new(), polys: Vec::new(), label: Some((at, t.value.clone())) });
            }
            EntityKind::MText(t) if filter.text => {
                shapes.push(Shape { layer, color, lines: Vec::new(), polys: Vec::new(), label: Some((t.insert.xy(), mtext_plain(&t.contents))) });
            }
            EntityKind::Text(_) | EntityKind::MText(_) => {}
            EntityKind::Hatch(h) => {
                shapes.push(Shape { layer, color, lines: Vec::new(), polys: hatch_polys(h, tol), label: None });
            }
            _ => {
                let list = cadcraft_render::build_entities(d, std::iter::once(e), &opts);
                let mut lines = Vec::new();
                let mut col = color;
                for p in &list.prims {
                    if p.kind == cadcraft_render::Kind::Polyline {
                        let pts = list.points(p);
                        if pts.len() >= 2 {
                            lines.push(pts.to_vec());
                            col = p.color;
                        }
                    }
                }
                if !lines.is_empty() {
                    shapes.push(Shape { layer, color: col, lines, polys: Vec::new(), label: None });
                }
            }
        }
    }
    // Styles by colour.
    let mut colors: Vec<u32> = shapes.iter().map(|s| s.color.to_u32()).collect();
    colors.sort_unstable();
    colors.dedup();
    let mut k = String::with_capacity(1 << 20);
    k.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>\n");
    k.push_str(&format!("<name>{}</name>\n<description>{}</description>\n", esc(title), esc(&format!("Xuất từ VNCCad — {}", crs.name))));
    for c in &colors {
        let (r, g, b) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
        k.push_str(&format!(
            "<Style id=\"c{c:06x}\"><LineStyle><color>ff{b:02x}{g:02x}{r:02x}</color><width>1.5</width></LineStyle><PolyStyle><color>66{b:02x}{g:02x}{r:02x}</color></PolyStyle><IconStyle><scale>0</scale></IconStyle><LabelStyle><color>ff{b:02x}{g:02x}{r:02x}</color><scale>0.8</scale></LabelStyle></Style>\n"
        ));
    }
    // Google Earth opens on the main part of the drawing (the middle 96 % of its shapes), not on
    // stray objects far away.
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for sh in &shapes {
        let p = sh
            .label
            .as_ref()
            .map(|l| l.0)
            .or_else(|| sh.lines.first().and_then(|l| l.first().copied()))
            .or_else(|| sh.polys.first().and_then(|p| p.0.first().copied()));
        if let Some(p) = p {
            xs.push(p.x);
            ys.push(p.y);
        }
    }
    if !xs.is_empty() {
        xs.sort_by(f64::total_cmp);
        ys.sort_by(f64::total_cmp);
        let q = |v: &[f64], f: f64| v.get(((v.len() - 1) as f64 * f) as usize).copied().unwrap_or(0.0);
        let (x0, x1, y0, y1) = (q(&xs, 0.02), q(&xs, 0.98), q(&ys, 0.02), q(&ys, 0.98));
        let (lat, lon) = crs.to_wgs84(Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0));
        let span = (x1 - x0).max(y1 - y0).max(1.0) * crs.unit_m * 1.6;
        if lat.is_finite() && lon.is_finite() {
            k.push_str(&format!("<LookAt><longitude>{lon:.8}</longitude><latitude>{lat:.8}</latitude><altitude>0</altitude><range>{span:.0}</range><tilt>0</tilt><heading>0</heading></LookAt>\n"));
        }
    }
    let mut layers: Vec<String> = shapes.iter().map(|s| s.layer.clone()).collect();
    layers.sort_by_key(|l| l.to_lowercase());
    layers.dedup();
    for l in &layers {
        k.push_str(&format!("<Folder><name>{}</name>\n", esc(l)));
        for s in shapes.iter().filter(|s| &s.layer == l) {
            let style = format!("#c{:06x}", s.color.to_u32());
            if let Some((at, text)) = &s.label {
                if text.trim().is_empty() {
                    continue;
                }
                let mut c = String::new();
                coords(crs, std::slice::from_ref(at), &mut c);
                k.push_str(&format!(
                    "<Placemark><name>{}</name><styleUrl>{style}</styleUrl><Point><coordinates>{}</coordinates></Point></Placemark>\n",
                    esc(text),
                    c.trim()
                ));
                continue;
            }
            k.push_str(&format!("<Placemark><styleUrl>{style}</styleUrl><MultiGeometry>"));
            for line in &s.lines {
                let mut c = String::new();
                coords(crs, line, &mut c);
                k.push_str(&format!("<LineString><tessellate>1</tessellate><coordinates>{}</coordinates></LineString>", c.trim()));
            }
            for (outer, holes) in &s.polys {
                let ring = |pts: &[Vec2]| {
                    let mut c = String::new();
                    let mut v = pts.to_vec();
                    if let Some(f) = v.first().copied() {
                        v.push(f);
                    }
                    coords(crs, &v, &mut c);
                    format!("<LinearRing><coordinates>{}</coordinates></LinearRing>", c.trim())
                };
                k.push_str("<Polygon><outerBoundaryIs>");
                k.push_str(&ring(outer));
                k.push_str("</outerBoundaryIs>");
                for h in holes {
                    k.push_str("<innerBoundaryIs>");
                    k.push_str(&ring(h));
                    k.push_str("</innerBoundaryIs>");
                }
                k.push_str("</Polygon>");
            }
            k.push_str("</MultiGeometry></Placemark>\n");
        }
        k.push_str("</Folder>\n");
    }
    k.push_str("</Document>\n</kml>\n");
    k
}

/// The drawing as a KMZ archive (`doc.kml` inside a ZIP).
pub fn to_kmz(d: &Drawing, crs: &Crs, filter: &KmlFilter, title: &str) -> Vec<u8> {
    zip_one("doc.kml", to_kml(d, crs, filter, title).as_bytes())
}

// ---------------------------------------------------------------- ZIP

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c = table.get(((c ^ u32::from(*b)) & 0xff) as usize).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// A ZIP archive holding one deflated file.
pub fn zip_one(name: &str, data: &[u8]) -> Vec<u8> {
    let comp = miniz_oxide::deflate::compress_to_vec(data, 6);
    let crc = crc32(data);
    let n = name.as_bytes();
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    let (csize, usize_) = (u32::try_from(comp.len()).unwrap_or(u32::MAX), u32::try_from(data.len()).unwrap_or(u32::MAX));
    let mut z = Vec::with_capacity(comp.len() + 200);
    // Local file header.
    le32(&mut z, 0x0403_4b50);
    le16(&mut z, 20);
    le16(&mut z, 0x0800); // UTF-8 names
    le16(&mut z, 8); // deflate
    le16(&mut z, 0);
    le16(&mut z, 0x21);
    le32(&mut z, crc);
    le32(&mut z, csize);
    le32(&mut z, usize_);
    le16(&mut z, n.len() as u16);
    le16(&mut z, 0);
    z.extend_from_slice(n);
    z.extend_from_slice(&comp);
    let cd_at = z.len() as u32;
    // Central directory.
    le32(&mut z, 0x0201_4b50);
    le16(&mut z, 20);
    le16(&mut z, 20);
    le16(&mut z, 0x0800);
    le16(&mut z, 8);
    le16(&mut z, 0);
    le16(&mut z, 0x21);
    le32(&mut z, crc);
    le32(&mut z, csize);
    le32(&mut z, usize_);
    le16(&mut z, n.len() as u16);
    le16(&mut z, 0);
    le16(&mut z, 0);
    le16(&mut z, 0);
    le16(&mut z, 0);
    le32(&mut z, 0);
    le32(&mut z, 0);
    z.extend_from_slice(n);
    let cd_len = z.len() as u32 - cd_at;
    // End of central directory.
    le32(&mut z, 0x0605_4b50);
    le16(&mut z, 0);
    le16(&mut z, 0);
    le16(&mut z, 1);
    le16(&mut z, 1);
    le32(&mut z, cd_len);
    le32(&mut z, cd_at);
    le16(&mut z, 0);
    z
}

/// The first `.kml` file inside a ZIP (KMZ), read through the local headers.
pub fn unzip_kml(z: &[u8]) -> Option<String> {
    let mut i = 0usize;
    let rd16 = |at: usize| z.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let rd32 = |at: usize| z.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let mut guard = 0;
    while guard < 10_000 && rd32(i)? == 0x0403_4b50 {
        guard += 1;
        let method = rd16(i + 8)?;
        let flags = rd16(i + 6)?;
        let csize = rd32(i + 18)?;
        let nlen = rd16(i + 26)?;
        let xlen = rd16(i + 28)?;
        let name = String::from_utf8_lossy(z.get(i + 30..i + 30 + nlen)?).to_string();
        let start = i + 30 + nlen + xlen;
        // Sizes in a data descriptor (flag 8): take the central directory's size instead.
        let csize = if flags & 8 != 0 { central_size(z, &name)? } else { csize };
        let data = z.get(start..start.checked_add(csize)?)?;
        if name.to_ascii_lowercase().ends_with(".kml") {
            let raw = match method {
                0 => data.to_vec(),
                8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, 512 << 20).ok()?,
                _ => return None,
            };
            return Some(String::from_utf8_lossy(&raw).to_string());
        }
        i = start + csize + if flags & 8 != 0 { 16 } else { 0 };
    }
    None
}

fn central_size(z: &[u8], name: &str) -> Option<usize> {
    let rd16 = |at: usize| z.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let rd32 = |at: usize| z.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let mut i = (0..z.len().saturating_sub(3)).rev().take(70_000).find(|&k| rd32(k) == Some(0x0605_4b50))?;
    let n = rd16(i + 10)?;
    i = rd32(i + 16)?;
    for _ in 0..n.min(10_000) {
        if rd32(i)? != 0x0201_4b50 {
            return None;
        }
        let csize = rd32(i + 20)?;
        let (nl, xl, cl) = (rd16(i + 28)?, rd16(i + 30)?, rd16(i + 32)?);
        if z.get(i + 46..i + 46 + nl).is_some_and(|b| b == name.as_bytes()) {
            return Some(csize);
        }
        i += 46 + nl + xl + cl;
    }
    None
}

// ---------------------------------------------------------------- KML reading

/// A shape read from KML (WGS-84 degrees: x = longitude, y = latitude).
#[derive(Clone, Debug, PartialEq)]
pub enum KmlItem {
    Line { folder: String, name: String, pts: Vec<Vec2>, closed: bool },
    Point { folder: String, name: String, at: Vec2 },
}

fn tag_text<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let i = s.find(&open)?;
    let gt = s.get(i..)?.find('>')? + i + 1;
    let close = format!("</{tag}>");
    let j = s.get(gt..)?.find(&close)? + gt;
    s.get(gt..j)
}

fn unesc(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")).unwrap_or(s);
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn parse_coords(s: &str) -> Vec<Vec2> {
    s.split_whitespace()
        .take(1_000_000)
        .filter_map(|t| {
            let mut it = t.split(',');
            let lon: f64 = it.next()?.trim().parse().ok()?;
            let lat: f64 = it.next()?.trim().parse().ok()?;
            (lon.is_finite() && lat.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0).then_some(Vec2::new(lon, lat))
        })
        .collect()
}

/// Every coordinate list inside `<tag>` elements of `s`.
fn all_tags<'a>(s: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = s;
    let close = format!("</{tag}>");
    while out.len() < 1_000_000
        && let Some(i) = rest.find(&format!("<{tag}"))
    {
        let Some(gt) = rest.get(i..).and_then(|r| r.find('>')).map(|g| g + i + 1) else { break };
        let Some(j) = rest.get(gt..).and_then(|r| r.find(&close)).map(|j| j + gt) else { break };
        if let Some(x) = rest.get(gt..j) {
            out.push(x);
        }
        rest = rest.get(j + close.len()..).unwrap_or("");
    }
    out
}

/// Shapes of a KML document: placemarks with their folder (layer) name.
pub fn parse_kml(text: &str) -> Vec<KmlItem> {
    let mut out = Vec::new();
    // Walk the document keeping the innermost folder name.
    let mut folders: Vec<String> = Vec::new();
    let mut i = 0usize;
    let len = text.len();
    let mut guard = 0usize;
    while i < len && guard < 5_000_000 {
        guard += 1;
        let Some(rel) = text.get(i..).and_then(|r| r.find('<')) else { break };
        let at = i + rel;
        let rest = text.get(at..).unwrap_or("");
        if rest.starts_with("<Folder") || rest.starts_with("<Document") {
            let end = rest.find("<Placemark").unwrap_or(rest.len()).min(rest.find("<Folder").filter(|x| *x > 0).unwrap_or(rest.len()));
            let head = rest.get(..end).unwrap_or("");
            folders.push(tag_text(head, "name").map(unesc).unwrap_or_default());
            i = at + 1;
        } else if rest.starts_with("</Folder>") || rest.starts_with("</Document>") {
            folders.pop();
            i = at + 1;
        } else if rest.starts_with("<Placemark") {
            let Some(end) = rest.find("</Placemark>") else { break };
            let pm = rest.get(..end).unwrap_or("");
            let folder = folders.iter().rev().find(|f| !f.is_empty()).cloned().unwrap_or_else(|| "KML".into());
            let name = tag_text(pm, "name").map(unesc).unwrap_or_default();
            for ls in all_tags(pm, "LineString") {
                if let Some(c) = tag_text(ls, "coordinates") {
                    let pts = parse_coords(c);
                    if pts.len() >= 2 {
                        let closed = pts.len() > 2 && pts.first() == pts.last();
                        out.push(KmlItem::Line { folder: folder.clone(), name: name.clone(), pts, closed });
                    }
                }
            }
            for lr in all_tags(pm, "LinearRing") {
                if let Some(c) = tag_text(lr, "coordinates") {
                    let mut pts = parse_coords(c);
                    if pts.len() > 2 && pts.first() == pts.last() {
                        pts.pop();
                    }
                    if pts.len() >= 3 {
                        out.push(KmlItem::Line { folder: folder.clone(), name: name.clone(), pts, closed: true });
                    }
                }
            }
            for pt in all_tags(pm, "Point") {
                if let Some(c) = tag_text(pt, "coordinates")
                    && let Some(p) = parse_coords(c).first()
                {
                    out.push(KmlItem::Point { folder: folder.clone(), name: name.clone(), at: *p });
                }
            }
            i = at + end + 1;
        } else {
            i = at + 1;
        }
    }
    out
}

/// KML or KMZ bytes → shapes.
pub fn read_kml_or_kmz(bytes: &[u8]) -> Option<Vec<KmlItem>> {
    let text = if bytes.starts_with(b"PK") { unzip_kml(bytes)? } else { String::from_utf8_lossy(bytes).to_string() };
    Some(parse_kml(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tm_round_trip_and_known_utm_point() {
        // Reference values from PROJ 9 (EPSG:32648).
        let c = Crs::utm(48, true);
        let p = c.from_wgs84(21.0285, 105.8542);
        assert!((p.x - 588_761.587_340).abs() < 0.001 && (p.y - 2_325_539.252_354).abs() < 0.001, "{p:?}");
        let (lat, lon) = c.to_wgs84(p);
        assert!((lat - 21.0285).abs() < 1e-8 && (lon - 105.8542).abs() < 1e-8);
    }

    #[test]
    fn vn2000_round_trip_and_shift() {
        let c = Crs::parse("VN2000 Đà Nẵng").unwrap();
        assert!((c.cm - 107.75).abs() < 1e-12 && (c.k0 - 0.9999).abs() < 1e-12);
        let p = Vec2::new(535_112.0, 1_777_960.5);
        let (lat, lon) = c.to_wgs84(p);
        // PROJ 9 with the same +towgs84: 16.075 492 279, 108.080 016 723 (Hòa Liên, Đà Nẵng).
        assert!((lat - 16.075_492_279).abs() < 2e-7 && (lon - 108.080_016_723).abs() < 2e-7, "{lat} {lon}");
        let q = c.from_wgs84(lat, lon);
        assert!(q.dist(p) < 0.01, "{q:?}");
        // The datum shift is a few hundred metres at most against plain WGS-84 TM.
        let plain = Crs { vn2000: false, ..c.clone() };
        let (lat2, lon2) = plain.to_wgs84(p);
        let dm = ((lat - lat2) * 111_000.0).hypot((lon - lon2) * 107_000.0);
        assert!(dm > 1.0 && dm < 600.0, "{dm}");
    }

    #[test]
    fn crs_parse_forms() {
        assert_eq!(Crs::parse("vn2000 107°45' 3").map(|c| c.cm), Some(107.75));
        assert_eq!(Crs::parse("VN2000 105 6").map(|c| c.k0), Some(0.9996));
        assert_eq!(Crs::parse("UTM 49N").map(|c| c.cm), Some(111.0));
        assert_eq!(Crs::parse("Hà Nội").map(|c| c.cm), Some(105.0));
        assert!(Crs::parse("xyz").is_none());
        let c = Crs::parse("VN2000 108.25 3").unwrap();
        assert_eq!(Crs::parse(&c.to_header()), Some(c));
    }

    #[test]
    fn zip_round_trip_and_kml_parse() {
        let kml = "<kml><Document><name>D</name><Folder><name>Ranh &amp; giới</name><Placemark><name>A</name><LineString><coordinates>108.0,16.0,0 108.1,16.1,0</coordinates></LineString></Placemark><Placemark><name>P</name><Point><coordinates>108.05,16.05</coordinates></Point></Placemark></Folder><Placemark><Polygon><outerBoundaryIs><LinearRing><coordinates>1,1 2,1 2,2 1,1</coordinates></LinearRing></outerBoundaryIs></Polygon></Placemark></Document></kml>";
        let z = zip_one("doc.kml", kml.as_bytes());
        assert_eq!(unzip_kml(&z).as_deref(), Some(kml));
        let items = read_kml_or_kmz(&z).unwrap();
        assert_eq!(items.len(), 3, "{items:?}");
        assert!(matches!(&items[0], KmlItem::Line { folder, pts, closed: false, .. } if folder == "Ranh & giới" && pts.len() == 2));
        assert!(matches!(&items[1], KmlItem::Point { name, .. } if name == "P"));
        assert!(matches!(&items[2], KmlItem::Line { folder, closed: true, pts, .. } if folder == "D" && pts.len() == 3));
        // Hostile input never panics.
        let _ = read_kml_or_kmz(b"PK\x03\x04garbage");
        let _ = parse_kml("<Placemark><LineString><coordinates>a,b c,d");
    }
}
