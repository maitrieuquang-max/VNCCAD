//! VNCCad: colour-dependent plot style tables (`.ctb`).
//!
//! A CTB file is a short text header ("PIAFILEVERSION_2.0,CTBVER1,compress" and
//! "pmzlibcodec"), a few binary size fields and a zlib stream holding a text body of nested
//! `key=value` groups. We read the `plot_style{…}` group (one entry per colour index: colour,
//! lineweight index, screening) and `custom_lineweight_table{…}` (lineweights in mm).
//!
//! No tables ship with VNCCad except the built-in "monochrome" and "grayscale" (our own
//! definitions in the render crate). User tables are found by name next to the drawing, in the
//! `VNCCad/plotstyles` folder, or among files given to the app.

use cadcraft_color::Rgb;
use cadcraft_render::pens::{Pen, PenTable};

/// Upper bound on the decompressed body (hostile files).
const MAX_BODY: usize = 8 * 1024 * 1024;

/// The decompressed text body of a CTB/STB file.
fn body(bytes: &[u8]) -> Option<String> {
    let head = bytes.get(..bytes.len().min(64))?;
    if !head.starts_with(b"PIAFILEVERSION") {
        return None;
    }
    // The zlib stream follows the header and size fields; find its first byte.
    let start = bytes.windows(11).position(|w| w == b"pmzlibcodec").map_or(0, |p| p + 11);
    for i in start..bytes.len().min(start + 64) {
        if bytes.get(i) == Some(&0x78)
            && let Some(rest) = bytes.get(i..)
            && let Ok(out) = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(rest, MAX_BODY)
        {
            return Some(String::from_utf8_lossy(&out).into_owned());
        }
    }
    None
}

/// The text between `name{` and its matching `}`.
fn group<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}{{");
    let at = text.find(&key)? + key.len();
    let mut depth = 1usize;
    for (i, c) in text.get(at..)?.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return text.get(at..at + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Top-level `N{ … }` entries of a group, by number.
fn entries(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let label = rest.get(..open).unwrap_or("").rsplit(['\n', '\r', ' ', '\t']).next().unwrap_or("").trim();
        let after = &rest[open + 1..];
        let mut depth = 1usize;
        let mut end = None;
        for (i, c) in after.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        if let Ok(n) = label.parse::<usize>() {
            out.push((n, &after[..end]));
        }
        rest = &after[end + 1..];
        if out.len() > 1024 {
            break;
        }
    }
    out
}

fn value<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    entry.lines().map(str::trim).find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('='))).map(|v| v.trim().trim_matches('"'))
}

/// Parse a `.ctb` file into pens. `None` if it isn't a colour-dependent plot style table.
pub fn parse(name: &str, bytes: &[u8]) -> Option<PenTable> {
    let text = body(bytes)?;
    let styles = group(&text, "plot_style")?;
    let weights: Vec<f32> = group(&text, "custom_lineweight_table")
        .map(|g| {
            let mut v: Vec<(usize, f32)> = g
                .lines()
                .filter_map(|l| {
                    let (k, w) = l.trim().split_once('=')?;
                    Some((k.trim().parse().ok()?, w.trim().parse().ok()?))
                })
                .collect();
            v.sort_by_key(|x| x.0);
            v.into_iter().map(|x| x.1).collect()
        })
        .unwrap_or_default();
    let mut t = PenTable { name: name.to_string(), pens: vec![Pen { color: None, lineweight: None, screen: 100 }; 256] };
    let mut seen = 0;
    for (n, e) in entries(styles) {
        let Some(pen) = t.pens.get_mut(n + 1) else { continue };
        seen += 1;
        if let Some(c) = value(e, "color").and_then(|v| v.parse::<i64>().ok()) {
            let c = c as u32;
            // 0xC3…… = "use object colour"; otherwise the low 24 bits are RGB.
            if c >> 24 != 0xC3 {
                pen.color = Some(Rgb(((c >> 16) & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, (c & 0xFF) as u8));
            }
        }
        if let Some(i) = value(e, "lineweight").and_then(|v| v.parse::<usize>().ok()) {
            pen.lineweight = weights.get(i).copied().filter(|w| w.is_finite() && *w > 0.0);
        }
        if let Some(sc) = value(e, "screen").and_then(|v| v.parse::<u8>().ok()) {
            pen.screen = sc.min(100);
        }
    }
    (seen > 0).then_some(t)
}

/// A plot style table by name: built in, or a `.ctb` file found on this computer or given to
/// the app. `Err` says why it couldn't be used.
pub fn find(name: &str) -> Result<PenTable, String> {
    let name = name.trim();
    if let Some(t) = PenTable::builtin(name) {
        return Ok(t);
    }
    if name.to_ascii_lowercase().ends_with(".stb") {
        return Err(format!("{name}: bảng nét in theo tên (STB) chưa được hỗ trợ, hãy dùng CTB"));
    }
    let file = if name.to_ascii_lowercase().ends_with(".ctb") { name.to_string() } else { format!("{name}.ctb") };
    let bytes = crate::raster::bytes(&file).or_else(|| user_dir_file(&file)).ok_or_else(|| format!("không tìm thấy bảng nét in {file}"))?;
    parse(&file, &bytes).ok_or_else(|| format!("{file}: không đọc được bảng nét in"))
}

#[cfg(not(target_arch = "wasm32"))]
fn user_dir_file(file: &str) -> Option<std::sync::Arc<Vec<u8>>> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")).map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local").join("share")))
    }?;
    let p = base.join("VNCCad").join("plotstyles").join(crate::raster::file_key(file));
    std::fs::read(p).ok().map(std::sync::Arc::new)
}

#[cfg(target_arch = "wasm32")]
fn user_dir_file(_file: &str) -> Option<std::sync::Arc<Vec<u8>>> {
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A CTB file written the way the format is laid out: header lines, size fields, zlib body.
    pub(crate) fn make_ctb(body_text: &str) -> Vec<u8> {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(body_text.as_bytes(), 6);
        let mut b = b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec".to_vec();
        b.extend(0u32.to_le_bytes());
        b.extend((body_text.len() as u32).to_le_bytes());
        b.extend((z.len() as u32).to_le_bytes());
        b.extend(z);
        b
    }

    pub(crate) fn sample() -> String {
        let mut s = String::from("description=\"VNCCad test\"\naci_table_available=TRUE\nscale_factor=1.0\nplot_style{\n");
        for i in 0..255 {
            // Colour 1 (index 0): red stays red, 0.70 mm; colour 2: black 0.35 mm; colour 3:
            // object colour at 50 % screening; the rest: object colour and lineweight.
            let (color, lw, screen) = match i {
                0 => ("-1023475712", 5, 100),
                1 => ("-1040187392", 3, 100),
                2 => ("-1006632961", 255, 50),
                _ => ("-1006632961", 255, 100),
            };
            s.push_str(&format!(
                " {i}{{\n  name=\"Color_{}\n  localized_name=\"Color_{}\n  description=\"\n  color={color}\n  mode_color={color}\n  color_policy=1\n  physical_pen_number=0\n  virtual_pen_number=0\n  screen={screen}\n  linepattern_size=0.5\n  linetype=31\n  adaptive_linetype=TRUE\n  lineweight={lw}\n  fill_style=73\n  end_style=4\n  join_style=5\n }}\n",
                i + 1,
                i + 1
            ));
        }
        s.push_str("}\ncustom_lineweight_table{\n 0=0.0\n 1=0.05\n 2=0.09\n 3=0.35\n 4=0.5\n 5=0.7\n}\n");
        s
    }

    #[test]
    fn parses_colour_lineweight_and_screening() {
        let t = parse("test.ctb", &make_ctb(&sample())).unwrap();
        // -1023475712 = 0xC2FF0000: red.
        assert_eq!(t.pens[1].color, Some(Rgb(255, 0, 0)));
        assert_eq!(t.pens[1].lineweight, Some(0.7));
        // -1040187392 = 0xC2000000: black.
        assert_eq!(t.pens[2].color, Some(Rgb(0, 0, 0)));
        assert_eq!(t.pens[2].lineweight, Some(0.35));
        assert_eq!(t.pens[3].color, None);
        assert_eq!(t.pens[3].screen, 50);
        assert_eq!(t.pens[3].lineweight, None);
        assert!(parse("x.ctb", b"not a ctb").is_none());
        let good = make_ctb(&sample());
        for cut in (0..good.len()).step_by(7) {
            let _ = parse("x.ctb", &good[..cut]);
        }
    }

    #[test]
    fn find_builtin_registered_and_missing() {
        assert_eq!(find("monochrome.ctb").unwrap().name, "monochrome");
        crate::raster::register("cong-ty.ctb", make_ctb(&sample()));
        assert_eq!(find("cong-ty").unwrap().pens[1].lineweight, Some(0.7));
        assert!(find("khong-co.ctb").is_err());
        assert!(find("named.stb").is_err());
    }
}
