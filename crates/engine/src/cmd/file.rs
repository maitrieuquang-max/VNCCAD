//! File menu: new, close, switching drawings. Open/save of DWG/DXF go through the io layer,
//! registered by the host (desktop/CLI) via [`crate::cmd::file::set_io`].

use std::sync::OnceLock;

use cadcraft_doc::Drawing;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Plot a space to PDF bytes with JSON options (`{paper?, landscape?, fit?, lineweights?, …}`).
pub type PlotHook = fn(&Drawing, &cadcraft_doc::Space, &Value) -> std::result::Result<Vec<u8>, String>;

/// File format hooks (installed by the app so the engine stays I/O-agnostic and wasm-safe).
pub struct IoHooks {
    pub read: fn(&[u8], &str) -> std::result::Result<Drawing, String>,
    pub write: fn(&Drawing, &str) -> std::result::Result<Vec<u8>, String>,
    /// PLOT / EXPORTPDF (optional: builds without a plotter report "not available").
    pub plot: Option<PlotHook>,
}

static IO: OnceLock<IoHooks> = OnceLock::new();

pub fn set_io(h: IoHooks) {
    let _ = IO.set(h);
}
pub fn io() -> Option<&'static IoHooks> {
    IO.get()
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("new", "New Drawing...", run_new)
            .menu(&["File", "New Drawing..."])
            .key("Cmd+N")
            .alias(&["qnew"])
            .params("{metric?: bool}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("open", "Open...", run_open)
            .menu(&["File", "Open..."])
            .key("Cmd+O")
            .params("{path} | {data: base64, name}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("close", "Close", run_close).menu(&["File", "Close"]).key("Cmd+W").params("{index?}").noundo(),
        CommandSpec::new("closeall", "Close All", run_closeall).menu(&["File", "Close All"]).enabled(always).noundo(),
        CommandSpec::new("qsave", "Save", run_save).menu(&["File", "Save"]).key("Cmd+S").alias(&["save"]).params("{path?}").noundo(),
        CommandSpec::new("saveas", "Save As...", run_saveas)
            .menu(&["File", "Save As..."])
            .key("Cmd+Shift+S")
            .params("{path, format?: dxf|dwg}")
            .noundo(),
        CommandSpec::new("document.switch", "Switch Drawing", run_switch).params("{index}").enabled(always).noundo(),
        CommandSpec::new("document.bytes", "Drawing as bytes", run_bytes).params("{format?: dxf} → {data: base64}").noundo(),
    ]
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let i = s.new_drawing(bool_or(p, "metric", false));
    Ok(json!({ "index": i, "title": s.docs.get(i).map(|d| d.title.clone()) }))
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

pub(crate) fn open_bytes(s: &mut Session, bytes: &[u8], name: &str, path: Option<String>) -> Result<usize> {
    // VNCCad: an image is attached to the open drawing; a world file is kept for later images.
    let lower = name.to_ascii_lowercase();
    if [".jgw", ".pgw", ".tfw", ".bpw", ".wld", ".jpgw", ".pngw", ".tifw"].iter().any(|e| lower.ends_with(e)) {
        cadcraft_io::raster::register(&file_name(name), bytes.to_vec());
        s.echo(format!("Đã nạp world file {}. Ảnh cùng tên chèn sau sẽ đặt đúng tọa độ.", file_name(name)));
        return Ok(s.active);
    }
    if lower.ends_with(".ctb") {
        let fname = file_name(name);
        cadcraft_io::raster::register(&fname, bytes.to_vec());
        match cadcraft_io::ctb::find(&fname) {
            Ok(_) => s.echo(format!("Đã nạp bảng nét in {fname}. Dùng PLOTSTYLE để chọn.")),
            Err(e) => return Err(bad("open", e)),
        }
        return Ok(s.active);
    }
    if lower.ends_with(".pdf") {
        let fname = file_name(name);
        cadcraft_io::raster::register(&fname, bytes.to_vec());
        if s.docs.is_empty() {
            s.new_drawing(true);
        }
        let reference = path.clone().unwrap_or(fname);
        let r = super::raster::run_pdfattach(s, &json!({ "path": reference }))?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        return Ok(s.active);
    }
    if cadcraft_io::raster::is_image_name(&lower) {
        let fname = file_name(name);
        cadcraft_io::raster::register(&fname, bytes.to_vec());
        if s.docs.is_empty() {
            s.new_drawing(true);
        }
        // Keep the full path on desktop so the drawing finds the file again.
        let reference = path.clone().unwrap_or(fname);
        let r = super::raster::run_imageattach(s, &json!({ "path": reference }))?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        return Ok(s.active);
    }
    // VNCCad: a dropped or opened .shx is a font to load, not a drawing.
    if name.to_ascii_lowercase().ends_with(".shx") {
        let fname = file_name(name);
        if !cadcraft_fonts::shx::register(&fname, bytes) {
            return Err(bad("open", format!("{fname}: không phải font SHX hợp lệ")));
        }
        s.echo(format!("Đã nạp font SHX {fname}. Chữ dùng font này sẽ hiển thị ở lần vẽ lại tiếp theo (REGEN)."));
        for st in &mut s.docs {
            st.revision += 1;
        }
        return Ok(s.active);
    }
    // VNCCad: AutoLISP routines and scripts.
    let lower_name = name.to_ascii_lowercase();
    if lower_name.ends_with(".lsp") {
        let fname = file_name(name);
        let text = crate::lisp::machine::decode_source(bytes);
        let forms = crate::lisp::read_all(&text).map_err(|m| bad("open", format!("{fname}: {m}")))?;
        s.lisp.files.insert(fname.to_ascii_lowercase(), text);
        let before = s.lisp.commands();
        s.start_lisp(crate::lisp::machine::Job::Eval { forms, echo_result: false, label: String::new() })?;
        let new: Vec<String> = s.lisp.commands().into_iter().filter(|c| !before.contains(c)).map(|c| c.to_ascii_uppercase()).collect();
        s.echo(if new.is_empty() { format!("Đã nạp {fname}.") } else { format!("Đã nạp {fname}. Lệnh mới: {}", new.join(", ")) });
        return Ok(s.active);
    }
    // VNCCad: data files for LISP routines (read with `open`).
    if [".txt", ".csv", ".dat", ".xyz", ".tsv"].iter().any(|e| lower_name.ends_with(e)) {
        let fname = file_name(name);
        let text = crate::lisp::machine::decode_source(bytes);
        s.lisp.files.insert(fname.to_ascii_lowercase(), text);
        s.echo(format!("Đã nạp tệp dữ liệu {fname} (đọc bằng (open \"{fname}\" \"r\") trong LISP)."));
        return Ok(s.active);
    }
    if lower_name.ends_with(".sld") {
        let fname = file_name(name);
        crate::lisp::dcl::parse_slide(bytes).map_err(|m| bad("open", format!("{fname}: {m}")))?;
        s.binary_files.insert(fname.to_ascii_lowercase(), bytes.to_vec());
        s.echo(format!("Đã nạp slide {fname} (dùng bằng slide_image trong hộp thoại LISP)."));
        return Ok(s.active);
    }
    if lower_name.ends_with(".dcl") {
        let fname = file_name(name);
        let text = crate::lisp::machine::decode_source(bytes);
        crate::lisp::dcl::parse(&text).map_err(|m| bad("open", format!("{fname}: {m}")))?;
        s.lisp.files.insert(fname.to_ascii_lowercase(), text);
        s.echo(format!("Đã nạp hộp thoại {fname} (dùng bằng load_dialog trong LISP)."));
        return Ok(s.active);
    }
    if lower_name.ends_with(".scr") {
        let text = crate::lisp::machine::decode_source(bytes);
        s.script(&text)?;
        return Ok(s.active);
    }
    // VNCCad: a TrueType/OpenType font (SimSun, Arial…) given to the app: used by text styles
    // naming it, and as the fallback for characters other fonts lack (web builds read no
    // system fonts).
    if [".ttf", ".otf", ".ttc"].iter().any(|e| lower_name.ends_with(e)) {
        let fname = file_name(name);
        let stem = fname.rsplit_once('.').map_or(fname.as_str(), |(a, _)| a).to_string();
        if !cadcraft_fonts::ttf::is_font(bytes) {
            return Err(bad("open", format!("{fname}: không phải font TrueType/OpenType hợp lệ")));
        }
        cadcraft_fonts::ttf::register(&stem, bytes.to_vec());
        s.echo(format!("Đã nạp font {fname}."));
        for st in &mut s.docs {
            st.revision += 1;
        }
        return Ok(s.active);
    }
    // VNCCad: "Block from File": the drawing becomes a block of the active drawing.
    if s.insert_next_open && s.state().is_ok() && (lower_name.ends_with(".dxf") || lower_name.ends_with(".dwg")) {
        s.insert_next_open = false;
        let block = super::importdefs::block_from_bytes(s, bytes, name, None, false)?;
        s.echo(format!("Đã nạp {} thành block {block}.", file_name(name)));
        if let Ok(st) = s.state_mut() {
            st.revision += 1;
        }
        s.start("insert")?;
        s.input(crate::Input::Text(block))?;
        return Ok(s.active);
    }
    let hooks = io().ok_or_else(|| bad("open", "file formats are not available in this build"))?;
    let mut d = (hooks.read)(bytes, name).map_err(|e| bad("open", e))?;
    // VNCCad: text in legacy Vietnamese fonts (TCVN3 .VnTime…, VNI-Times…) becomes Unicode.
    let vn = cadcraft_doc::vnlegacy::convert_drawing(&mut d);
    // VNCCad: external references are read from their files every time.
    let base = path.as_deref().and_then(|p| std::path::Path::new(p).parent()).map(std::path::Path::to_path_buf);
    let open: Vec<(String, std::sync::Arc<Drawing>)> = s.docs.iter().map(|st| (st.title.clone(), st.doc.clone())).collect();
    let (xok, xmissing) = super::xref::resolve_all(&mut d, base.as_deref(), &open);
    let i = s.open_drawing(d, &file_name(name), path);
    if !xok.is_empty() || !xmissing.is_empty() {
        let mut m = format!("Tham chiếu ngoài: đã nạp {}", xok.len());
        if !xmissing.is_empty() {
            m.push_str(&format!("; chưa tìm thấy {} (đặt file cạnh bản vẽ hoặc mở trong tab khác rồi XREF nạp lại)", xmissing.join(", ")));
        }
        s.echo(m);
    }
    if vn.texts > 0 || !vn.styles.is_empty() {
        s.echo(super::vn::report_text(&vn));
    }
    Ok(i)
}

fn run_open(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(_path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let path = _path;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes = std::fs::read(path).map_err(|e| bad("open", format!("{path}: {e}")))?;
            // Fonts sent alongside the drawing (same folder or its fonts/ subfolder).
            if let Some(dir) = std::path::Path::new(path).parent() {
                cadcraft_fonts::shx::add_font_dir(dir);
                cadcraft_io::raster::add_dir(dir);
            }
            let i = open_bytes(s, &bytes, path, Some(path.to_string()))?;
            recover_if_asked(s, path);
            return Ok(json!({ "index": i, "entities": s.docs.get(i).map(|d| d.doc.entity_count()) }));
        }
        #[cfg(target_arch = "wasm32")]
        return Err(bad("open", "paths are not available on the web; pass `data`"));
    }
    let data = str_param(p, "data").ok_or_else(|| bad("open", "`path` or `data` (base64) is required"))?;
    let bytes = base64_decode(data).ok_or_else(|| bad("open", "invalid base64"))?;
    let name = str_param(p, "name").unwrap_or("Drawing.dxf");
    let i = open_bytes(s, &bytes, name, None)?;
    recover_if_asked(s, name);
    Ok(json!({ "index": i }))
}

/// VNCCad: RECOVER started from the UI: audit and fix the drawing just opened.
fn recover_if_asked(s: &mut Session, name: &str) {
    let l = name.to_ascii_lowercase();
    if !s.audit_next_open || !(l.ends_with(".dxf") || l.ends_with(".dwg")) {
        return;
    }
    s.audit_next_open = false;
    if let Ok(r) = s.execute("audit", &json!({ "fix": true }))
        && let Some(m) = r.get("message").and_then(Value::as_str)
    {
        s.echo(format!("RECOVER — {m}"));
    }
}

fn run_close(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).map(|i| i as usize).unwrap_or(s.active);
    if i >= s.docs.len() {
        return Err(bad("close", "no such drawing"));
    }
    s.cancel();
    s.docs.remove(i);
    if s.active >= s.docs.len() {
        s.active = s.docs.len().saturating_sub(1);
    }
    ok()
}

fn run_closeall(s: &mut Session, _p: &Value) -> Result<Value> {
    s.cancel();
    s.docs.clear();
    s.active = 0;
    ok()
}

pub(crate) fn save_to(s: &mut Session, path: &str) -> Result<usize> {
    let hooks = io().ok_or_else(|| bad("save", "file formats are not available in this build"))?;
    let bytes = (hooks.write)(s.doc()?, path).map_err(|e| bad("save", e))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tmp = format!("{path}.cadcraft-tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| bad("save", format!("{path}: {e}")))?;
        std::fs::rename(&tmp, path).map_err(|e| bad("save", format!("{path}: {e}")))?;
    }
    let st = s.state_mut()?;
    st.saved = st.doc.clone();
    st.path = Some(path.to_string());
    st.title = file_name(path);
    Ok(bytes.len())
}

fn run_save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match str_param(p, "path") {
        Some(p) => p.to_string(),
        None => s.state()?.path.clone().ok_or_else(|| bad("qsave", "drawing has no file name yet; use saveas {path}"))?,
    };
    let n = save_to(s, &path)?;
    Ok(json!({ "path": path, "bytes": n }))
}

fn run_saveas(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("saveas", "`path` is required"))?.to_string();
    if let Some(v) = str_param(p, "version") {
        set_dwg_version(s, v)?;
    }
    let n = save_to(s, &path)?;
    Ok(json!({ "path": path, "bytes": n }))
}

/// VNCCad: the AutoCAD release DWG files of this drawing are saved as.
fn set_dwg_version(s: &mut Session, v: &str) -> Result<String> {
    let code = cadcraft_dwg_code(v).ok_or_else(|| bad("dwgversion", format!("phiên bản không hỗ trợ `{v}` (2000, 2004, 2007, 2010, 2013, 2018)")))?;
    s.doc_mut()?.header.set("ACADVER", cadcraft_doc::HVal::Str(code.to_string()));
    Ok(code.to_string())
}

fn cadcraft_dwg_code(v: &str) -> Option<&'static str> {
    const V: &[(&str, &str)] =
        &[("AC1015", "2000"), ("AC1018", "2004"), ("AC1021", "2007"), ("AC1024", "2010"), ("AC1027", "2013"), ("AC1032", "2018")];
    let v = v.trim().to_ascii_uppercase();
    V.iter().find(|(c, y)| v == *c || v == *y).map(|(c, _)| *c)
}

pub(crate) fn run_dwgversion(s: &mut Session, p: &Value) -> Result<Value> {
    let year = |c: &str| match c {
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        _ => "2018",
    };
    match str_param(p, "version") {
        Some(v) => {
            let c = set_dwg_version(s, v)?;
            Ok(json!({ "version": c, "message": format!("Lưu DWG theo định dạng AutoCAD {}.", year(&c)) }))
        }
        None => {
            let c = s.doc()?.header.str("ACADVER", "AC1021");
            let c = cadcraft_dwg_code(&c).unwrap_or("AC1021");
            Ok(json!({ "version": c, "message": format!("DWG được lưu theo định dạng AutoCAD {} (đổi: DWGVERSION 2007…).", year(c)) }))
        }
    }
}

fn run_switch(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("document.switch", "`index` is required"))? as usize;
    if i >= s.docs.len() {
        return Err(bad("document.switch", "no such drawing"));
    }
    s.cancel();
    s.active = i;
    ok()
}

fn run_bytes(s: &mut Session, p: &Value) -> Result<Value> {
    let hooks = io().ok_or_else(|| bad("document.bytes", "file formats are not available in this build"))?;
    let fmt = str_param(p, "format").unwrap_or("dxf");
    let bytes = (hooks.write)(s.doc()?, &format!("drawing.{fmt}")).map_err(|e| bad("document.bytes", e))?;
    Ok(json!({ "data": base64_encode(&bytes), "bytes": bytes.len() }))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}
