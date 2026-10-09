//! VNCCad raster images referenced by IMAGE entities: finding the file, decoding it (PNG,
//! JPEG, BMP, TIFF), and reading world files that place an image at real coordinates.
//!
//! Drawings store only a path. On desktop the file is read from that path, or by file name from
//! the drawing's folder and the folders registered with [`add_dir`]. On the web, files the user
//! drops are registered by name with [`register`] and found by the file name in the path.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// A decoded image, RGBA8, at most [`MAX_SIDE`] pixels on its longer side.
#[derive(Debug)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    /// Size of the original file in pixels (before any downscaling).
    pub source_size: (u32, u32),
    pub rgba: Vec<u8>,
}

/// Longest side kept in memory for display.
pub const MAX_SIDE: u32 = 4096;
/// Largest file accepted.
const MAX_FILE: u64 = 400 * 1024 * 1024;
/// Largest source image accepted (pixels per side).
const MAX_SOURCE_SIDE: u32 = 40_000;

struct Store {
    bytes: HashMap<String, Arc<Vec<u8>>>,
    decoded: HashMap<String, Option<Arc<Decoded>>>,
    #[cfg(not(target_arch = "wasm32"))]
    dirs: Vec<std::path::PathBuf>,
}

fn store() -> &'static Mutex<Store> {
    static S: OnceLock<Mutex<Store>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(Store {
            bytes: HashMap::new(),
            decoded: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            dirs: Vec::new(),
        })
    })
}

/// The file name of a path, lower-case ("C:\\Anh\\Ve Tinh.JPG" → "ve tinh.jpg"). A PDF page
/// suffix ("ban-ve.pdf#2") is kept, so each page is its own image.
pub fn file_key(path: &str) -> String {
    path.trim().rsplit(['/', '\\']).next().unwrap_or(path).to_lowercase()
}

/// Resolution PDF pages are drawn at, in pixels per inch.
pub const PDF_DPI: f64 = 150.0;

/// Split "file.pdf#3" into ("file.pdf", 3); `None` for anything that isn't a PDF.
pub fn pdf_parts(path: &str) -> Option<(&str, usize)> {
    let (file, page) = match path.rsplit_once('#') {
        Some((f, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (f, p.parse::<usize>().ok()?.max(1)),
        _ => (path, 1),
    };
    file.trim().to_ascii_lowercase().ends_with(".pdf").then_some((file, page))
}

/// The file part of a stored path (without a PDF page suffix).
fn file_part(path: &str) -> &str {
    pdf_parts(path).map_or(path, |(f, _)| f)
}

/// Size of a PDF page in points (1/72 inch).
pub fn pdf_page_size(bytes: &[u8], page: usize) -> Option<(f64, f64)> {
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).ok()?;
    let pages = pdf.pages();
    let p = pages.iter().nth(page.checked_sub(1)?)?;
    let (w, h) = p.render_dimensions();
    (w > 0.0 && h > 0.0).then_some((f64::from(w), f64::from(h)))
}

/// Number of pages in a PDF.
pub fn pdf_page_count(bytes: &[u8]) -> usize {
    hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map(|p| p.pages().iter().count()).unwrap_or(0)
}

fn decode_pdf(bytes: &[u8], page: usize) -> Option<Decoded> {
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).ok()?;
    let pages = pdf.pages();
    let p = pages.iter().nth(page.checked_sub(1)?)?;
    let (w, h) = p.render_dimensions();
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let full = (PDF_DPI / 72.0) as f32;
    let fit = MAX_SIDE as f32 / w.max(h);
    let scale = full.min(fit);
    let cache = hayro::RenderCache::new();
    let settings = hayro::hayro_interpret::InterpreterSettings::default();
    let pix = hayro::render(
        p,
        &cache,
        &settings,
        &hayro::RenderSettings::default(),
        &hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE },
    );
    let (pw, ph) = (u32::from(pix.width()), u32::from(pix.height()));
    let source = ((f64::from(w) * PDF_DPI / 72.0).round() as u32, (f64::from(h) * PDF_DPI / 72.0).round() as u32);
    Some(Decoded { width: pw, height: ph, source_size: source, rgba: pix.data_as_u8_slice().to_vec() })
}

/// Is this a file name of an image format we read?
pub fn is_image_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if pdf_parts(&n).is_some() {
        return false;
    }
    [".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff"].iter().any(|e| n.ends_with(e))
}

/// Make image bytes available under a file name (web uploads, tests).
pub fn register(name: &str, bytes: Vec<u8>) {
    let mut s = store().lock().unwrap_or_else(PoisonError::into_inner);
    let k = file_key(name);
    s.decoded.remove(&k);
    s.bytes.insert(k, Arc::new(bytes));
}

/// Also look for images in `dir` (the folder of a drawing being opened).
#[cfg(not(target_arch = "wasm32"))]
pub fn add_dir(dir: &std::path::Path) {
    let mut s = store().lock().unwrap_or_else(PoisonError::into_inner);
    if !s.dirs.iter().any(|d| d == dir) {
        s.dirs.push(dir.to_path_buf());
        s.decoded.retain(|_, v| v.is_some());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &std::path::Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    std::fs::read(path).ok()
}

/// The bytes of an image by its stored path.
pub fn bytes(path: &str) -> Option<Arc<Vec<u8>>> {
    let path = file_part(path);
    let k = file_key(path);
    if k.is_empty() {
        return None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let dirs = {
        let s = store().lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(b) = s.bytes.get(&k) {
            return Some(b.clone());
        }
        s.dirs.clone()
    };
    #[cfg(target_arch = "wasm32")]
    {
        return store().lock().unwrap_or_else(PoisonError::into_inner).bytes.get(&k).cloned();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let p = std::path::Path::new(path.trim());
        let name = p.file_name().map(std::path::PathBuf::from);
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if p.is_absolute() {
            candidates.push(p.to_path_buf());
        }
        for d in &dirs {
            if !p.is_absolute() {
                candidates.push(d.join(p));
            }
            if let Some(n) = &name {
                candidates.push(d.join(n));
            }
        }
        for c in candidates {
            if let Some(b) = read_file(&c) {
                let arc = Arc::new(b);
                store().lock().unwrap_or_else(PoisonError::into_inner).bytes.insert(k, arc.clone());
                return Some(arc);
            }
        }
        None
    }
}

/// Pixel size of encoded image bytes.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

fn decode_bytes(bytes: &[u8]) -> Option<Decoded> {
    let (w, h) = dimensions(bytes)?;
    if w == 0 || h == 0 || w > MAX_SOURCE_SIDE || h > MAX_SOURCE_SIDE {
        return None;
    }
    let img = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;
    let img = if w.max(h) > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    let rgba = img.to_rgba8();
    Some(Decoded { width: rgba.width(), height: rgba.height(), source_size: (w, h), rgba: rgba.into_raw() })
}

/// The decoded image for a stored path (cached; `None` when missing or unreadable).
pub fn decoded(path: &str) -> Option<Arc<Decoded>> {
    let k = file_key(path);
    if let Some(v) = store().lock().unwrap_or_else(PoisonError::into_inner).decoded.get(&k) {
        return v.clone();
    }
    let d = match pdf_parts(path) {
        Some((_, page)) => bytes(path).and_then(|b| decode_pdf(&b, page)),
        None => bytes(path).and_then(|b| decode_bytes(&b)),
    }
    .map(Arc::new);
    store().lock().unwrap_or_else(PoisonError::into_inner).decoded.insert(k, d.clone());
    d
}

/// A world file's six numbers: pixel size in x (A), rotation terms (D, B), pixel size in y (E,
/// negative for north-up), and the centre of the upper-left pixel (C, F).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldFile {
    pub a: f64,
    pub d: f64,
    pub b: f64,
    pub e: f64,
    pub c: f64,
    pub f: f64,
}

/// Parse world-file text (six numbers, one per line).
pub fn parse_world_file(text: &str) -> Option<WorldFile> {
    let n: Vec<f64> = text.split_whitespace().take(6).filter_map(|t| t.replace(',', ".").parse().ok()).collect();
    let (&a, &d, &b, &e, &c, &f) = (n.first()?, n.get(1)?, n.get(2)?, n.get(3)?, n.get(4)?, n.get(5)?);
    if [a, d, b, e, c, f].iter().any(|x| !x.is_finite()) || (a == 0.0 && d == 0.0) {
        return None;
    }
    Some(WorldFile { a, d, b, e, c, f })
}

/// World-file names that go with an image ("anh.jpg" → "anh.jgw", "anh.jpgw", "anh.wld").
pub fn world_file_names(image: &str) -> Vec<String> {
    let (stem, ext) = match image.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), e.to_ascii_lowercase()),
        None => (image.to_string(), String::new()),
    };
    let short = match ext.len() {
        0 => String::new(),
        1 | 2 => format!("{ext}w"),
        _ => format!("{}{}w", ext.chars().next().unwrap_or('x'), ext.chars().last().unwrap_or('x')),
    };
    let mut v = vec![format!("{stem}.{short}"), format!("{stem}.{ext}w"), format!("{stem}.wld")];
    if ext == "tif" || ext == "tiff" {
        v.push(format!("{stem}.tfw"));
    }
    v.dedup();
    v
}

/// The world file next to an image, if any (registered by name, or on disk beside it).
pub fn world_file(image_path: &str) -> Option<WorldFile> {
    for name in world_file_names(image_path) {
        if let Some(b) = bytes(&name)
            && let Some(w) = parse_world_file(&String::from_utf8_lossy(&b))
        {
            return Some(w);
        }
    }
    None
}

/// Placement of an image from its world file: (lower-left corner, u = one pixel along a row,
/// v = one pixel up a column), as an IMAGE entity stores them.
pub fn placement_from_world(w: &WorldFile, height_px: u32) -> ([f64; 2], [f64; 2], [f64; 2]) {
    let u = [w.a, w.d];
    let down = [w.b, w.e];
    let ul = [w.c - 0.5 * u[0] - 0.5 * down[0], w.f - 0.5 * u[1] - 0.5 * down[1]];
    let h = f64::from(height_px);
    let ll = [ul[0] + h * down[0], ul[1] + h * down[1]];
    (ll, u, [-down[0], -down[1]])
}

/// Encode RGBA pixels as PNG (used by other crates' tests to make images).
#[doc(hidden)]
pub fn encode_png_for_tests(rgba: &[u8], w: u32, h: u32, out: &mut Vec<u8>) {
    if let Some(img) = image::RgbaImage::from_raw(w, h, rgba.to_vec()) {
        let mut c = std::io::Cursor::new(Vec::new());
        if image::DynamicImage::ImageRgba8(img).write_to(&mut c, image::ImageFormat::Png).is_ok() {
            out.extend(c.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 10) as u8, (y * 10) as u8, 128, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn register_find_decode() {
        register("Ảnh Nền.PNG", png(20, 10));
        assert_eq!(dimensions(&bytes("C:\\Du an\\ảnh nền.png").unwrap()), Some((20, 10)));
        let d = decoded("/x/y/Ảnh Nền.png").unwrap();
        assert_eq!((d.width, d.height), (20, 10));
        assert_eq!(d.rgba.len(), 20 * 10 * 4);
        assert!(decoded("khong-co.png").is_none());
        register("hong.png", b"not an image".to_vec());
        assert!(decoded("hong.png").is_none());
    }

    /// A one-page PDF (A4 portrait, 595×842 pt) with a filled rectangle, written by hand.
    fn tiny_pdf() -> Vec<u8> {
        let content = "0 0 1 rg 100 100 200 300 re f";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    #[test]
    fn pdf_pages_render() {
        let pdf = tiny_pdf();
        assert_eq!(pdf_page_count(&pdf), 1);
        assert_eq!(pdf_page_size(&pdf, 1), Some((595.0, 842.0)));
        assert!(pdf_page_size(&pdf, 2).is_none());
        register("mat-bang.pdf", pdf);
        assert_eq!(pdf_parts("D:\\x\\Mat-Bang.PDF#1"), Some(("D:\\x\\Mat-Bang.PDF", 1)));
        assert!(!is_image_name("mat-bang.pdf"));
        let d = decoded("mat-bang.pdf").unwrap();
        assert_eq!(d.source_size, (1240, 1754));
        // The rasteriser rounds the page size down to whole pixels.
        assert!(d.width.abs_diff(1240) <= 1 && d.height.abs_diff(1754) <= 1, "{}x{}", d.width, d.height);
        // White page with a blue rectangle at (100..300, 100..400) pt from the bottom-left.
        let px = |x: u32, y: u32| {
            let i = ((y * d.width + x) * 4) as usize;
            [d.rgba[i], d.rgba[i + 1], d.rgba[i + 2]]
        };
        assert_eq!(px(10, 10), [255, 255, 255]);
        let (x, y) = ((150.0 * 150.0 / 72.0) as u32, ((842.0 - 200.0) * 150.0 / 72.0) as u32);
        assert_eq!(px(x, y), [0, 0, 255]);
        assert!(decoded("mat-bang.pdf#2").is_none());
    }

    #[test]
    fn world_files() {
        assert_eq!(world_file_names("anh.jpg"), vec!["anh.jgw", "anh.jpgw", "anh.wld"]);
        assert_eq!(world_file_names("b.tif")[0], "b.tfw");
        let w = parse_world_file("0.5\n0\n0\n-0.5\n1000.25\n2000.75\n").unwrap();
        // 4 px tall: upper-left corner (1000, 2001), lower-left (1000, 1999).
        let (ll, u, v) = placement_from_world(&w, 4);
        assert_eq!(ll, [1000.0, 1999.0]);
        assert_eq!(u, [0.5, 0.0]);
        assert_eq!(v, [0.0, 0.5]);
        register("vt.png", png(8, 4));
        register("vt.pgw", b"0.5\n0\n0\n-0.5\n1000.25\n2000.75\n".to_vec());
        assert_eq!(world_file("D:\\anh\\vt.png"), Some(w));
        assert!(parse_world_file("1 2 3").is_none());
    }
}
