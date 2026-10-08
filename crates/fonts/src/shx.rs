//! VNCCad: compiled shape fonts (`.shx`): "shapes 1.0/1.1" (8-bit codes), "unifont 1.0"
//! (Unicode code points) and "bigfont 1.0".
//!
//! Written from the publicly documented shape-definition codes (vectors in 16 directions, pen
//! up/down, scale, push/pop, sub-shapes, displacements, octant / fractional / bulge arcs,
//! vertical-only commands). No font files ship with VNCCad: SHX fonts are read at run time from
//! the user's own folders (an installed AutoCAD's Fonts folder, `VNCCad/fonts` in the user data
//! folder, `VNCCAD_FONTS`) or registered from bytes (a `.shx` dropped on the web app).
//!
//! Characters a font lacks fall back to the built-in stroke font, which has every Vietnamese
//! letter.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use cadcraft_geom::Vec2;

use crate::Shaped;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShxKind {
    Shapes,
    Unifont,
    Bigfont,
}

/// A parsed SHX font.
#[derive(Debug)]
pub struct ShxFont {
    pub name: String,
    pub kind: ShxKind,
    /// Height of capitals in shape units (text height maps to this).
    pub above: f64,
    pub below: f64,
    /// Shape number → definition bytes (after the shape name).
    shapes: HashMap<u32, Vec<u8>>,
}

const MAX_SHAPES: usize = 70_000;
const MAX_DEF: usize = 4096;

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// Split a definition into (name, spec bytes).
fn split_def(def: &[u8]) -> (String, &[u8]) {
    match def.iter().position(|&c| c == 0) {
        Some(z) => (def.get(..z).map(|n| n.iter().map(|&c| char::from(c)).collect()).unwrap_or_default(), def.get(z + 1..).unwrap_or(&[])),
        None => (String::new(), def),
    }
}

impl ShxFont {
    /// Parse SHX bytes. `None` for anything that isn't a compiled shape font.
    pub fn parse(bytes: &[u8]) -> Option<ShxFont> {
        let head_end = bytes.iter().take(64).position(|&c| c == 0x1A)?;
        let head: String = bytes.get(..head_end)?.iter().map(|&c| char::from(c)).collect::<String>().to_ascii_lowercase();
        if !head.starts_with("autocad-86") {
            return None;
        }
        let body = head_end + 1;
        let mut shapes = HashMap::new();
        let kind;
        if head.contains("unifont") {
            kind = ShxKind::Unifont;
            let count = u32le(bytes, body)? as usize;
            let info_len = usize::from(u16le(bytes, body + 4)?);
            let info = bytes.get(body + 6..body + 6 + info_len)?;
            shapes.insert(0, split_def(info).1.to_vec());
            let mut at = body + 6 + info_len;
            for _ in 0..count.min(MAX_SHAPES) {
                let (Some(num), Some(len)) = (u16le(bytes, at), u16le(bytes, at + 2)) else { break };
                let len = usize::from(len);
                let Some(def) = bytes.get(at + 4..at + 4 + len) else { break };
                if len <= MAX_DEF {
                    shapes.insert(u32::from(num), split_def(def).1.to_vec());
                }
                at += 4 + len;
            }
        } else if head.contains("bigfont") {
            kind = ShxKind::Bigfont;
            let count = usize::from(u16le(bytes, body + 2)?);
            let ranges = usize::from(u16le(bytes, body + 4)?);
            let mut at = body + 6 + ranges.min(256) * 4;
            for _ in 0..count.min(MAX_SHAPES) {
                let (Some(num), Some(len), Some(off)) = (u16le(bytes, at), u16le(bytes, at + 2), u32le(bytes, at + 4)) else { break };
                at += 8;
                let (off, len) = (off as usize, usize::from(len));
                if num == 0 && len == 0 {
                    continue;
                }
                if let Some(def) = bytes.get(off..off.saturating_add(len))
                    && len <= MAX_DEF
                {
                    shapes.insert(u32::from(num), split_def(def).1.to_vec());
                }
            }
        } else if head.contains("shapes") {
            kind = ShxKind::Shapes;
            let count = usize::from(u16le(bytes, body + 4)?);
            let index = body + 6;
            let mut def_at = index + count * 4;
            for i in 0..count.min(MAX_SHAPES) {
                let (Some(num), Some(len)) = (u16le(bytes, index + i * 4), u16le(bytes, index + i * 4 + 2)) else { break };
                let len = usize::from(len);
                let Some(def) = bytes.get(def_at..def_at + len) else { break };
                if len <= MAX_DEF {
                    shapes.insert(u32::from(num), split_def(def).1.to_vec());
                }
                def_at += len;
            }
        } else {
            return None;
        }
        let info = shapes.get(&0).cloned().unwrap_or_default();
        let above = f64::from(info.first().copied().unwrap_or(0));
        let below = f64::from(info.get(1).copied().unwrap_or(0));
        let name = String::new();
        if shapes.len() <= 1 {
            return None;
        }
        Some(ShxFont { name, kind, above: if above > 0.0 { above } else { 8.0 }, below, shapes })
    }

    pub fn has(&self, c: char) -> bool {
        self.shapes.contains_key(&u32::from(c))
    }
    pub fn len(&self) -> usize {
        self.shapes.len().saturating_sub(1)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// --- shape interpreter -------------------------------------------------------------------------

/// The 16 vector directions.
const DIRS: [(f64, f64); 16] = [
    (1.0, 0.0),
    (1.0, 0.5),
    (1.0, 1.0),
    (0.5, 1.0),
    (0.0, 1.0),
    (-0.5, 1.0),
    (-1.0, 1.0),
    (-1.0, 0.5),
    (-1.0, 0.0),
    (-1.0, -0.5),
    (-1.0, -1.0),
    (-0.5, -1.0),
    (0.0, -1.0),
    (0.5, -1.0),
    (1.0, -1.0),
    (1.0, -0.5),
];

struct Pen {
    pos: (f64, f64),
    down: bool,
    scale: f64,
    stack: Vec<(f64, f64)>,
    path: Vec<(f64, f64)>,
    out: Vec<Vec<(f64, f64)>>,
    steps: usize,
}

impl Pen {
    fn flush(&mut self) {
        if self.path.len() > 1 {
            self.out.push(std::mem::take(&mut self.path));
        } else {
            self.path.clear();
        }
    }
    fn move_to(&mut self, p: (f64, f64), draw: bool) {
        if draw && self.down {
            if self.path.is_empty() {
                self.path.push(self.pos);
            }
            self.path.push(p);
        } else {
            self.flush();
        }
        self.pos = p;
    }
    fn arc(&mut self, center: (f64, f64), r: f64, a0: f64, sweep: f64, draw: bool) {
        let n = ((sweep.abs() / (std::f64::consts::PI / 16.0)).ceil() as usize).clamp(1, 64);
        for k in 1..=n {
            let a = a0 + sweep * (k as f64 / n as f64);
            self.move_to((center.0 + r * a.cos(), center.1 + r * a.sin()), draw);
        }
    }
    fn bulge(&mut self, dx: f64, dy: f64, b: f64, draw: bool) {
        let end = (self.pos.0 + dx, self.pos.1 + dy);
        if b.abs() < 1e-9 {
            self.move_to(end, draw);
            return;
        }
        let chord = (dx * dx + dy * dy).sqrt();
        if chord < 1e-12 {
            return;
        }
        let theta = 4.0 * b.atan();
        let r = chord / (2.0 * (theta / 2.0).sin()).abs();
        let mid = (self.pos.0 + dx / 2.0, self.pos.1 + dy / 2.0);
        let h = r * (theta / 2.0).cos();
        let (nx, ny) = (-dy / chord, dx / chord);
        let sgn = if b > 0.0 { 1.0 } else { -1.0 };
        let center = (mid.0 + nx * h * sgn, mid.1 + ny * h * sgn);
        let a0 = (self.pos.1 - center.1).atan2(self.pos.0 - center.0);
        self.arc(center, r, a0, theta, draw);
        self.pos = end;
    }
}

fn i8b(b: u8) -> f64 {
    f64::from(i8::from_le_bytes([b]))
}

const MAX_STEPS: usize = 200_000;

/// Run one shape. `depth` bounds sub-shape recursion.
fn run(font: &ShxFont, code: u32, pen: &mut Pen, depth: u32) {
    if depth > 8 {
        return;
    }
    let Some(spec) = font.shapes.get(&code) else { return };
    let mut i = 0usize;
    let mut skip_next = false;
    while let Some(&b) = spec.get(i) {
        pen.steps += 1;
        if pen.steps > MAX_STEPS {
            return;
        }
        let draw = !skip_next;
        let was_skip = skip_next;
        skip_next = false;
        i += 1;
        let at = |i: usize, k: usize| spec.get(i + k).copied().unwrap_or(0);
        match b {
            0 => {
                if !was_skip {
                    break;
                }
            }
            1 => {
                if draw {
                    pen.flush();
                    pen.down = true;
                }
            }
            2 => {
                if draw {
                    pen.flush();
                    pen.down = false;
                }
            }
            3 => {
                if draw && at(i, 0) != 0 {
                    pen.scale /= f64::from(at(i, 0));
                }
                i += 1;
            }
            4 => {
                if draw {
                    pen.scale *= f64::from(at(i, 0));
                }
                i += 1;
            }
            5 => {
                if draw && pen.stack.len() < 64 {
                    pen.stack.push(pen.pos);
                }
            }
            6 => {
                if draw && let Some(p) = pen.stack.pop() {
                    pen.flush();
                    pen.pos = p;
                }
            }
            7 => {
                // Sub-shape: one byte (shapes), two bytes (unifont), or extended (bigfont 0).
                let (num, used) = match font.kind {
                    ShxKind::Unifont => (u32::from(u16::from_be_bytes([at(i, 0), at(i, 1)])), 2),
                    ShxKind::Bigfont if at(i, 0) == 0 => (u32::from(u16::from_be_bytes([at(i, 1), at(i, 2)])), 7),
                    _ => (u32::from(at(i, 0)), 1),
                };
                i += used;
                if draw {
                    run(font, num, pen, depth + 1);
                }
            }
            8 => {
                let (dx, dy) = (i8b(at(i, 0)) * pen.scale, i8b(at(i, 1)) * pen.scale);
                i += 2;
                if !was_skip {
                    pen.move_to((pen.pos.0 + dx, pen.pos.1 + dy), draw);
                }
            }
            9 => loop {
                let (a, c) = (at(i, 0), at(i, 1));
                i += 2;
                if (a == 0 && c == 0) || i > spec.len() {
                    break;
                }
                if !was_skip {
                    let (dx, dy) = (i8b(a) * pen.scale, i8b(c) * pen.scale);
                    pen.move_to((pen.pos.0 + dx, pen.pos.1 + dy), draw);
                }
            },
            10 => {
                let r = f64::from(at(i, 0)) * pen.scale;
                let o = at(i, 1);
                i += 2;
                if !was_skip && r > 0.0 {
                    let cw = o & 0x80 != 0;
                    let start = f64::from((o >> 4) & 7) * std::f64::consts::FRAC_PI_4;
                    let n = if o & 7 == 0 { 8.0 } else { f64::from(o & 7) };
                    let sweep = n * std::f64::consts::FRAC_PI_4 * if cw { -1.0 } else { 1.0 };
                    let center = (pen.pos.0 - r * start.cos(), pen.pos.1 - r * start.sin());
                    pen.arc(center, r, start, sweep, draw);
                }
            }
            11 => {
                let (so, eo, rh, rl, o) = (at(i, 0), at(i, 1), at(i, 2), at(i, 3), at(i, 4));
                i += 5;
                let r = (f64::from(rh) * 256.0 + f64::from(rl)) * pen.scale;
                if !was_skip && r > 0.0 {
                    let cw = o & 0x80 != 0;
                    let s_oct = f64::from((o >> 4) & 7);
                    let n = if o & 7 == 0 { 8.0 } else { f64::from(o & 7) };
                    let q = std::f64::consts::FRAC_PI_4;
                    let sign = if cw { -1.0 } else { 1.0 };
                    let start = (s_oct + sign * f64::from(so) / 256.0) * q;
                    let last = s_oct + sign * (n - 1.0);
                    let end = if eo == 0 { (last + sign) * q } else { (last + sign * f64::from(eo) / 256.0) * q };
                    let center = (pen.pos.0 - r * start.cos(), pen.pos.1 - r * start.sin());
                    pen.arc(center, r, start, end - start, draw);
                }
            }
            12 => {
                let (dx, dy, bu) = (i8b(at(i, 0)) * pen.scale, i8b(at(i, 1)) * pen.scale, i8b(at(i, 2)));
                i += 3;
                if !was_skip {
                    pen.bulge(dx, dy, bu / 127.0, draw);
                }
            }
            13 => loop {
                let (a, c) = (at(i, 0), at(i, 1));
                if a == 0 && c == 0 {
                    i += 2;
                    break;
                }
                let bu = at(i, 2);
                i += 3;
                if i > spec.len() {
                    break;
                }
                if !was_skip {
                    pen.bulge(i8b(a) * pen.scale, i8b(c) * pen.scale, i8b(bu) / 127.0, draw);
                }
            },
            14 => skip_next = true,
            v => {
                if !was_skip {
                    let len = f64::from(v >> 4) * pen.scale;
                    let (dx, dy) = DIRS.get(usize::from(v & 0x0F)).copied().unwrap_or((0.0, 0.0));
                    pen.move_to((pen.pos.0 + dx * len, pen.pos.1 + dy * len), draw);
                }
            }
        }
    }
    pen.flush();
}

/// Strokes and advance of one character in shape units.
fn glyph(font: &ShxFont, c: char) -> Option<(Vec<Vec<(f64, f64)>>, f64)> {
    let code = u32::from(c);
    if !font.shapes.contains_key(&code) || code == 0 {
        return None;
    }
    let mut pen = Pen { pos: (0.0, 0.0), down: true, scale: 1.0, stack: Vec::new(), path: Vec::new(), out: Vec::new(), steps: 0 };
    run(font, code, &mut pen, 0);
    pen.flush();
    Some((pen.out, pen.pos.0))
}

/// Lay out one line in an SHX font (falling back to the stroke font per missing character).
pub fn shape(font: &ShxFont, s: &str, height: f64, width_factor: f64, oblique: f64) -> Shaped {
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let k = h / font.above;
    let shear = oblique.tan().clamp(-10.0, 10.0);
    let mut out = Shaped::default();
    let mut x = 0.0;
    for (c, _, _) in crate::decode_controls(s) {
        let c = if c == '⌀' && !font.has(c) { 'Ø' } else { c };
        if let Some((strokes, adv)) = glyph(font, c) {
            for st in strokes {
                out.strokes.push(st.iter().map(|(gx, gy)| Vec2::new(x + gx * k * wf + gy * k * shear, gy * k)).collect());
            }
            x += adv * k * wf;
        } else {
            let run = crate::layout_line(&c.to_string(), h, wf, oblique);
            for st in run.strokes {
                out.strokes.push(st.into_iter().map(|p| Vec2::new(p.x + x, p.y)).collect());
            }
            x += crate::char_advance(c) * h * wf;
        }
    }
    out.width = x;
    out
}

/// Width of one line in an SHX font.
pub fn width(font: &ShxFont, s: &str, height: f64, width_factor: f64) -> f64 {
    shape(font, s, height, width_factor, 0.0).width
}

// --- font lookup -------------------------------------------------------------------------------

type Table = HashMap<String, Option<Arc<ShxFont>>>;

fn table() -> &'static Mutex<Table> {
    static T: OnceLock<Mutex<Table>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    let n = n.rsplit(['/', '\\']).next().unwrap_or(&n).to_string();
    n.trim_end_matches(".shx").to_string()
}

/// Register SHX bytes under a name (web uploads, tests). Returns whether they parsed.
pub fn register(name: &str, bytes: &[u8]) -> bool {
    let parsed = ShxFont::parse(bytes).map(|mut f| {
        f.name = key(name);
        Arc::new(f)
    });
    let ok = parsed.is_some();
    table().lock().unwrap_or_else(PoisonError::into_inner).insert(key(name), parsed);
    ok
}

#[cfg(not(target_arch = "wasm32"))]
fn extra_dirs() -> &'static Mutex<Vec<std::path::PathBuf>> {
    static D: OnceLock<Mutex<Vec<std::path::PathBuf>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Vec::new()))
}

/// Also look for fonts in `dir` (the folder of a drawing being opened: drawings are often sent
/// with their fonts). Fonts not found earlier are looked up again.
#[cfg(not(target_arch = "wasm32"))]
pub fn add_font_dir(dir: &std::path::Path) {
    let mut d = extra_dirs().lock().unwrap_or_else(PoisonError::into_inner);
    if !d.iter().any(|x| x == dir) {
        d.push(dir.to_path_buf());
        d.push(dir.join("fonts"));
        // Forget misses so fonts in the new folder are found.
        table().lock().unwrap_or_else(PoisonError::into_inner).retain(|_, f| f.is_some());
    }
}

/// Folders searched for `.shx` files.
#[cfg(not(target_arch = "wasm32"))]
pub fn font_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut dirs: Vec<PathBuf> = extra_dirs().lock().unwrap_or_else(PoisonError::into_inner).clone();
    if let Some(v) = std::env::var_os("VNCCAD_FONTS") {
        dirs.extend(std::env::split_paths(&v));
    }
    let data = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")).map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
    };
    if let Some(d) = data {
        dirs.push(d.join("VNCCad").join("fonts"));
    }
    if cfg!(windows) {
        // Fonts of AutoCAD / LT installations on this computer (read in place, never copied).
        for pf in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(p) = std::env::var_os(pf) {
                let base = PathBuf::from(p).join("Autodesk");
                if let Ok(rd) = std::fs::read_dir(&base) {
                    for e in rd.flatten().take(64) {
                        dirs.push(e.path().join("Fonts"));
                    }
                }
            }
        }
    }
    dirs
}

#[cfg(not(target_arch = "wasm32"))]
fn load_from_disk(k: &str) -> Option<Arc<ShxFont>> {
    for d in font_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten().take(5000) {
            let p = e.path();
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let ext = p.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext == "shx" && stem == k {
                let meta = std::fs::metadata(&p).ok()?;
                if meta.len() > 64 * 1024 * 1024 {
                    return None;
                }
                let bytes = std::fs::read(&p).ok()?;
                return ShxFont::parse(&bytes).map(|mut f| {
                    f.name = k.to_string();
                    Arc::new(f)
                });
            }
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn load_from_disk(_k: &str) -> Option<Arc<ShxFont>> {
    None
}

/// An SHX font by style font name ("romans.shx", "ROMANS"), if one can be found.
pub fn find(name: &str) -> Option<Arc<ShxFont>> {
    let k = key(name);
    if k.is_empty() {
        return None;
    }
    let mut t = table().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(v) = t.get(&k) {
        return v.clone();
    }
    let f = load_from_disk(&k);
    t.insert(k, f.clone());
    f
}

/// Names of the SHX fonts registered or found so far (for the UI).
pub fn known() -> Vec<String> {
    let t = table().lock().unwrap_or_else(PoisonError::into_inner);
    let mut v: Vec<String> = t.iter().filter(|(_, f)| f.is_some()).map(|(k, _)| k.clone()).collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile a tiny "shapes 1.0" font: shape 0 (info), 'L' and 'O', plus a sub-shape user.
    fn shapes_font() -> Vec<u8> {
        let defs: Vec<(u16, Vec<u8>)> = vec![
            (0, [b"TEST\0".to_vec(), vec![6, 2, 0, 0]].concat()),
            // L: pen up, move up 6, pen down, down 6, right 4, pen up, right 2.
            (u16::from(b'L'), [b"\0".to_vec(), vec![2, 0x64, 1, 0x6C, 0x40, 2, 0x20, 0]].concat()),
            // O: circle of radius 2 (octant arc, 8 octants) around (2,3), then move to x=6.
            (u16::from(b'O'), [b"\0".to_vec(), vec![2, 8, 4, 3, 1, 10, 2, 0x00, 2, 8, 2, 0xFD, 0]].concat()),
            // A: sub-shape L then a bulge arc.
            (u16::from(b'A'), [b"\0".to_vec(), vec![7, b'L', 1, 12, 4, 0, 127, 0]].concat()),
        ];
        let mut b = b"AutoCAD-86 shapes 1.0\r\n\x1a".to_vec();
        b.extend(0u16.to_le_bytes());
        b.extend(255u16.to_le_bytes());
        b.extend((defs.len() as u16).to_le_bytes());
        for (n, d) in &defs {
            b.extend(n.to_le_bytes());
            b.extend((d.len() as u16).to_le_bytes());
        }
        for (_, d) in &defs {
            b.extend(d);
        }
        b
    }

    fn unifont() -> Vec<u8> {
        let mut b = b"AutoCAD-86 unifont 1.0\r\n\x1a".to_vec();
        let info = [b"UNI\0".to_vec(), vec![10, 2, 0, 0, 0, 0]].concat();
        b.extend(2u32.to_le_bytes());
        b.extend((info.len() as u16).to_le_bytes());
        b.extend(&info);
        // U+1EA0 'Ạ' as a single stroke, to prove code points beyond 8 bits work.
        let def = [b"\0".to_vec(), vec![1, 0xA4, 0x50, 0]].concat();
        b.extend(0x1EA0u16.to_le_bytes());
        b.extend((def.len() as u16).to_le_bytes());
        b.extend(def);
        b
    }

    #[test]
    fn parses_shapes_and_draws_vectors() {
        let f = ShxFont::parse(&shapes_font()).unwrap();
        assert_eq!(f.kind, ShxKind::Shapes);
        assert_eq!(f.above, 6.0);
        let (strokes, adv) = glyph(&f, 'L').unwrap();
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0], vec![(0.0, 6.0), (0.0, 0.0), (4.0, 0.0)]);
        assert_eq!(adv, 6.0);
    }

    #[test]
    fn octant_arc_closes_a_circle() {
        let f = ShxFont::parse(&shapes_font()).unwrap();
        let (strokes, adv) = glyph(&f, 'O').unwrap();
        let ring = &strokes[0];
        let first = ring.first().unwrap();
        let last = ring.last().unwrap();
        assert!((first.0 - last.0).abs() < 1e-9 && (first.1 - last.1).abs() < 1e-9, "full circle closes");
        for (x, y) in ring {
            let r = ((x - 2.0).powi(2) + (y - 3.0).powi(2)).sqrt();
            assert!((r - 2.0).abs() < 1e-9, "radius {r}");
        }
        assert!((adv - 6.0).abs() < 1e-9);
    }

    #[test]
    fn subshape_and_bulge() {
        let f = ShxFont::parse(&shapes_font()).unwrap();
        let (strokes, adv) = glyph(&f, 'A').unwrap();
        assert!(strokes.len() >= 2);
        // Bulge 1 (half circle) from x=6 to x=10 rises 2 units at its middle (or dips).
        let arc = strokes.last().unwrap();
        let peak = arc.iter().map(|p| p.1.abs()).fold(0.0, f64::max);
        assert!((peak - 2.0).abs() < 0.05, "peak {peak}");
        assert!((adv - 10.0).abs() < 1e-9);
    }

    #[test]
    fn layout_scales_to_text_height_and_falls_back() {
        let f = ShxFont::parse(&shapes_font()).unwrap();
        let sh = shape(&f, "LL", 3.0, 1.0, 0.0);
        // Advance 6 units per L at above=6 → 3 per L.
        assert!((sh.width - 6.0).abs() < 1e-9);
        let top = sh.strokes.iter().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((top - 3.0).abs() < 1e-9);
        // 'ầ' isn't in the font: drawn with the stroke font, still advancing.
        let mixed = shape(&f, "Lầ", 3.0, 1.0, 0.0);
        assert!(mixed.width > sh.width / 2.0);
        assert!(mixed.strokes.len() > 1);
    }

    #[test]
    fn unifont_code_points() {
        let f = ShxFont::parse(&unifont()).unwrap();
        assert_eq!(f.kind, ShxKind::Unifont);
        assert!(f.has('Ạ'));
        let (strokes, adv) = glyph(&f, 'Ạ').unwrap();
        assert_eq!(strokes.len(), 1);
        assert_eq!(adv, 5.0);
    }

    #[test]
    fn register_and_find() {
        assert!(register("vnccad-test.shx", &shapes_font()));
        assert!(find("VNCCAD-TEST").is_some());
        assert!(find("vnccad-test.shx").is_some());
        assert!(!register("bad.shx", b"not a font"));
        assert!(find("bad").is_none());
    }

    #[test]
    fn hostile_bytes_never_panic() {
        let good = shapes_font();
        for cut in 0..good.len() {
            let _ = ShxFont::parse(&good[..cut]);
        }
        let mut evil = good.clone();
        for b in evil.iter_mut().skip(30) {
            *b = b.wrapping_mul(7).wrapping_add(3);
        }
        if let Some(f) = ShxFont::parse(&evil) {
            let _ = shape(&f, "LOA\u{1}", 1.0, 1.0, 0.0);
        }
        // Self-referencing sub-shape stops at the depth limit.
        let mut b = b"AutoCAD-86 shapes 1.0\r\n\x1a".to_vec();
        let defs: Vec<(u16, Vec<u8>)> = vec![(0, vec![0, 6, 2, 0, 0]), (65, vec![0, 7, 65, 0x14, 0])];
        b.extend(0u16.to_le_bytes());
        b.extend(255u16.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        for (n, d) in &defs {
            b.extend(n.to_le_bytes());
            b.extend((d.len() as u16).to_le_bytes());
        }
        for (_, d) in &defs {
            b.extend(d);
        }
        let f = ShxFont::parse(&b).unwrap();
        let _ = shape(&f, "AAAA", 1.0, 1.0, 0.0);
    }
}
