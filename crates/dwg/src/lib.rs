//! DWG support through a DXF bridge.
//!
//! DWG files are read with the `acadrust` crate (MPL-2.0, used unmodified as a dependency) and
//! converted to DXF bytes, which CADCraft's own DXF reader maps to its document model; saving
//! goes the other way. Keeping the dependency behind this byte-level API isolates it: nothing
//! else in CADCraft depends on its types. Native targets only (it memory-maps files).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// DWG version codes we can write.
pub const VERSIONS: &[(&str, &str)] =
    &[("AC1015", "2000"), ("AC1018", "2004"), ("AC1021", "2007"), ("AC1024", "2010"), ("AC1027", "2013"), ("AC1032", "2018")];

/// A DWG version code from a code (`"AC1027"`) or a release year (`"2013"`).
pub fn version_code(v: &str) -> Option<&'static str> {
    let v = v.trim().to_ascii_uppercase();
    VERSIONS.iter().find(|(code, year)| v == *code || v == *year).map(|(code, _)| *code)
}

/// True when the bytes look like a DWG file (`AC10xx` magic).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && bytes.starts_with(b"AC10") && bytes.get(4..6).is_some_and(|v| v.iter().all(u8::is_ascii_digit))
}

/// The DWG version code of a file (e.g. "AC1032").
pub fn version(bytes: &[u8]) -> Option<String> {
    is_dwg(bytes).then(|| String::from_utf8_lossy(bytes.get(0..6).unwrap_or_default()).to_string())
}

/// VNCCad: on every target, the web build included (acadrust keeps its memory-mapping and
/// threads to native targets). On the web a panic can't be caught (WebAssembly aborts), so the
/// reader's own error handling is what protects the page there.
mod native {
    use std::io::Cursor;

    /// Read DWG bytes and return an ASCII DXF rendition.
    pub fn dwg_to_dxf(bytes: &[u8]) -> Result<Vec<u8>, String> {
        let data = bytes.to_vec();
        let doc = std::panic::catch_unwind(move || acadrust::DwgReader::from_stream(Cursor::new(data)).read())
            .map_err(|_| "the DWG reader failed on this file".to_string())?
            .map_err(|e| format!("DWG: {e}"))?;
        let mut doc = doc;
        sync_table_merges(&mut doc);
        acadrust::DxfWriter::new(&doc).write_to_vec().map_err(|e| format!("DWG→DXF: {e}"))
    }

    /// VNCCad: R2010+ DWG tables come back with a merged-range list but every cell's span at 1,
    /// and the DXF writer only writes the spans: put the spans back so merged cells survive.
    fn sync_table_merges(doc: &mut acadrust::CadDocument) {
        let needs = doc.entities().any(|e| {
            matches!(e, acadrust::EntityType::Table(t) if !t.merged_ranges.is_empty() && !t.rows.iter().any(|r| r.cells.iter().any(|c| c.is_merged())))
        });
        if !needs {
            return;
        }
        for e in doc.entities_mut() {
            let acadrust::EntityType::Table(t) = e else { continue };
            if t.rows.iter().any(|r| r.cells.iter().any(|c| c.is_merged())) {
                continue;
            }
            for r in t.merged_ranges.clone() {
                if r.right_col < r.left_col || r.bottom_row < r.top_row {
                    continue;
                }
                for (ri, row) in t.rows.iter_mut().enumerate().take(r.bottom_row.saturating_add(1)).skip(r.top_row) {
                    for (ci, cell) in row.cells.iter_mut().enumerate().take(r.right_col.saturating_add(1)).skip(r.left_col) {
                        cell.merged = 1;
                        let top_left = ri == r.top_row && ci == r.left_col;
                        cell.merge_width = if top_left { i32::try_from(r.right_col - r.left_col + 1).unwrap_or(1) } else { 0 };
                        cell.merge_height = if top_left { i32::try_from(r.bottom_row - r.top_row + 1).unwrap_or(1) } else { 0 };
                    }
                }
            }
        }
    }

    /// VNCCad: acadrust's DXF reader skips MLINESTYLE flags (fill, caps, miters): take them
    /// from the DXF text (ASCII, as VNCCad writes it).
    fn fix_mline_style_flags(doc: &mut acadrust::CadDocument, dxf: &[u8]) {
        let text = String::from_utf8_lossy(dxf);
        let mut lines = text.lines();
        let mut found: Vec<(String, i32)> = Vec::new();
        let mut cur: Option<(String, i32)> = None;
        while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
            match (code.trim(), value.trim()) {
                ("0", v) => {
                    found.extend(cur.take());
                    if v == "MLINESTYLE" {
                        cur = Some((String::new(), 0));
                    }
                }
                ("2", v) => {
                    if let Some(c) = cur.as_mut() {
                        c.0 = v.to_string();
                    }
                }
                ("70", v) => {
                    if let Some(c) = cur.as_mut() {
                        c.1 = v.parse().unwrap_or(0);
                    }
                }
                _ => {}
            }
            if found.len() > 10_000 {
                break;
            }
        }
        if found.is_empty() {
            return;
        }
        for o in doc.objects.values_mut() {
            if let acadrust::objects::ObjectType::MLineStyle(st) = o
                && let Some((_, f)) = found.iter().find(|(n, _)| n.eq_ignore_ascii_case(&st.name))
            {
                st.flags = acadrust::objects::MLineStyleFlags::from_bits(*f);
            }
        }
    }

    /// Convert DXF bytes into a DWG file.
    pub fn dxf_to_dwg(dxf: &[u8]) -> Result<Vec<u8>, String> {
        dxf_to_dwg_version(dxf, None)
    }

    /// VNCCad: convert DXF bytes into a DWG file of a given version code (`"AC1032"`…); `None`
    /// keeps the version the DXF says. Text is Unicode from AutoCAD 2007 (AC1021) on.
    pub fn dxf_to_dwg_version(dxf: &[u8], version: Option<&str>) -> Result<Vec<u8>, String> {
        let data = dxf.to_vec();
        let mut doc = std::panic::catch_unwind(move || acadrust::DxfReader::from_reader(Cursor::new(data)).and_then(|r| r.read()))
            .map_err(|_| "the DXF→DWG conversion failed".to_string())?
            .map_err(|e| format!("DXF: {e}"))?;
        fix_mline_style_flags(&mut doc, dxf);
        if let Some(v) = version {
            let code = super::version_code(v).ok_or_else(|| format!("phiên bản DWG không hỗ trợ: {v} (2000, 2004, 2007, 2010, 2013, 2018)"))?;
            doc.version = acadrust::types::DxfVersion::parse(code).ok_or_else(|| format!("phiên bản DWG không hỗ trợ: {v}"))?;
        }
        acadrust::DwgWriter::write_to_vec(&doc).map_err(|e| format!("DWG write: {e}"))
    }
}

pub use native::{dwg_to_dxf, dxf_to_dwg, dxf_to_dwg_version};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_magic() {
        assert!(is_dwg(b"AC1032\0\0\0\0"));
        assert!(!is_dwg(b"  0\r\nSECTION"));
        assert_eq!(version(b"AC1018xxxx").as_deref(), Some("AC1018"));
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(dwg_to_dxf(b"AC1032 this is not a dwg file at all").is_err());
        assert!(dwg_to_dxf(b"").is_err());
    }
}
