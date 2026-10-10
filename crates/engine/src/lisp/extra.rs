//! VNCCad: the rest of the everyday AutoLISP library — text files (`open`, `read-line`,
//! `write-line`…), file and folder functions (`vl-file-*`, `vl-filename-*`), table and dictionary
//! access (`tblnext`, `tblobjname`, `namedobjdict`, `dictsearch`), more string and list
//! functions, temporary vectors (`grdraw`, `grvecs`), `textbox`, `cvunit`, `boole` and friends.
//!
//! Files: on the desktop they are real files. On the web, files given to the app (dropped or
//! opened) are readable by name, and a file written by a routine is handed to the browser as a
//! download when it is closed.

use cadcraft_doc::Handle;
use cadcraft_geom::Vec2;

use super::{R, Run, V, err, to_text};

fn arg(a: &[V], i: usize) -> &V {
    a.get(i).unwrap_or(&V::Nil)
}

fn s_arg<'a>(a: &'a [V], i: usize, f: &str) -> R<&'a str> {
    match a.get(i) {
        Some(V::Str(s)) => Ok(s),
        _ => err(format!("{f}: đối số {} phải là chuỗi", i + 1)),
    }
}

fn i_arg(a: &[V], i: usize, f: &str) -> R<i64> {
    match a.get(i) {
        Some(V::Int(n)) => Ok(*n),
        Some(V::Real(r)) if r.fract() == 0.0 && r.is_finite() => Ok(*r as i64),
        _ => err(format!("{f}: đối số {} phải là số nguyên", i + 1)),
    }
}

/// An open text file.
#[derive(Clone, Debug, Default)]
pub struct LFile {
    pub name: String,
    /// 'r', 'w' or 'a'.
    pub mode: char,
    /// Read: the whole text; write: what was written.
    pub text: String,
    /// Read position (bytes).
    pub pos: usize,
}

/// Pseudo entity names for symbol-table records and dictionaries (`tblobjname`,
/// `namedobjdict`, `dictsearch`): never real handles.
const PSEUDO: u64 = 0x7F00_0000_0000_0000;
const TABLES: &[&str] = &["LAYER", "LTYPE", "STYLE", "DIMSTYLE", "BLOCK", "VIEW", "UCS"];
const DICTS: &[&str] = &["ACAD_GROUP", "ACAD_MLINESTYLE", "ACAD_LAYOUT"];
const K_ROOT: u64 = 20;
const K_DICT: u64 = 21;
const K_ENTRY: u64 = 30;

fn pseudo(kind: u64, a: u64, b: u64) -> V {
    V::Ename(Handle(PSEUDO | (kind << 48) | ((a & 0xFF) << 32) | (b & 0xFFFF_FFFF)))
}

fn unpseudo(h: Handle) -> Option<(u64, u64, u64)> {
    if h.0 & 0xFF00_0000_0000_0000 != PSEUDO {
        return None;
    }
    Some(((h.0 >> 48) & 0xFF, (h.0 >> 32) & 0xFF, h.0 & 0xFFFF_FFFF))
}

fn dotted(code: i64, v: V) -> V {
    V::Dotted(vec![V::Int(code)], Box::new(v))
}

fn base_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// (directory, base, extension) of a path; the directory keeps its trailing separator.
fn split_path(p: &str) -> (String, String, String) {
    let file = base_name(p);
    let dir = p.get(..p.len() - file.len()).unwrap_or("").to_string();
    match file.rfind('.') {
        Some(i) if i > 0 => (dir, file.get(..i).unwrap_or("").to_string(), file.get(i..).unwrap_or("").to_string()),
        _ => (dir, file.to_string(), String::new()),
    }
}

/// Length units in millimetres, area units in square millimetres, angles in radians.
fn unit(name: &str) -> Option<(char, f64)> {
    let n = name.trim().to_ascii_lowercase();
    let n = n.trim_end_matches('s');
    Some(match n {
        "mm" | "millimeter" | "millimetre" => ('l', 1.0),
        "cm" | "centimeter" | "centimetre" => ('l', 10.0),
        "m" | "meter" | "metre" => ('l', 1000.0),
        "km" | "kilometer" | "kilometre" => ('l', 1.0e6),
        "inch" | "in" | "inche" => ('l', 25.4),
        "foot" | "ft" | "feet" => ('l', 304.8),
        "yard" | "yd" => ('l', 914.4),
        "mile" | "mi" => ('l', 1_609_344.0),
        "sq mm" | "sq_mm" | "mm2" => ('a', 1.0),
        "sq cm" | "sq_cm" | "cm2" => ('a', 100.0),
        "sq m" | "sq_m" | "m2" | "sq meter" => ('a', 1.0e6),
        "hectare" | "ha" => ('a', 1.0e10),
        "sq km" | "sq_km" | "km2" => ('a', 1.0e12),
        "acre" => ('a', 4_046_856_422.4),
        "sq ft" | "sq_ft" | "sq foot" | "sq feet" => ('a', 92_903.04),
        "sq in" | "sq_in" | "sq inch" => ('a', 645.16),
        "radian" | "rad" => ('g', 1.0),
        "degree" | "deg" => ('g', std::f64::consts::PI / 180.0),
        "grad" | "gon" => ('g', std::f64::consts::PI / 200.0),
        "kg" | "kilogram" => ('w', 1000.0),
        "g" | "gram" => ('w', 1.0),
        "ton" | "tonne" | "t" => ('w', 1.0e6),
        _ => return None,
    })
}

impl Run<'_> {
    fn read_source(&self, name: &str) -> Option<String> {
        let key = name.to_ascii_lowercase();
        let base = base_name(&key).to_string();
        if let Some(t) = self.lisp.files.get(&key).or_else(|| self.lisp.files.get(&base)) {
            return Some(t.clone());
        }
        if let Some(b) = self.s.binary_files.get(&key).or_else(|| self.s.binary_files.get(&base)) {
            return Some(super::machine::decode_source(b));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(b) = std::fs::read(name) {
            return Some(super::machine::decode_source(&b));
        }
        None
    }

    /// Write `text` to the file `v` (when it is an open file for writing).
    pub(super) fn file_write(&mut self, v: &V, text: &str) -> bool {
        let V::File(id) = v else { return false };
        match self.lisp.open_files.get_mut(id) {
            Some(f) if f.mode != 'r' => {
                if f.text.len() < 256 * 1024 * 1024 {
                    f.text.push_str(text);
                }
                true
            }
            _ => false,
        }
    }

    fn close_file(&mut self, id: u64) -> R<V> {
        let Some(f) = self.lisp.open_files.remove(&id) else { return Ok(V::Nil) };
        if f.mode == 'r' {
            return Ok(V::Nil);
        }
        let key = f.name.to_ascii_lowercase();
        let full = if f.mode == 'a' { format!("{}{}", self.read_source(&f.name).unwrap_or_default(), f.text) } else { f.text.clone() };
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Write;
            let res = if f.mode == 'a' {
                std::fs::OpenOptions::new().create(true).append(true).open(&f.name).and_then(|mut h| h.write_all(f.text.as_bytes()))
            } else {
                std::fs::write(&f.name, f.text.as_bytes())
            };
            if let Err(e) = res {
                return err(format!("close: không ghi được {}: {e}", f.name));
            }
            // A file the app already knew is updated in memory too.
            if self.lisp.files.contains_key(&key) {
                self.lisp.files.insert(key, full);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let data = crate::cmd::file::base64_encode(full.as_bytes());
            self.s.ui_requests.push(("download".into(), serde_json::json!({ "name": base_name(&f.name), "data": data })));
            self.lisp.files.insert(key, full);
        }
        Ok(V::Nil)
    }

    fn read_line(&mut self, id: u64) -> V {
        let Some(f) = self.lisp.open_files.get_mut(&id) else { return V::Nil };
        if f.mode != 'r' || f.pos >= f.text.len() {
            return V::Nil;
        }
        let rest = f.text.get(f.pos..).unwrap_or("");
        let (line, adv) = match rest.find('\n') {
            Some(i) => (rest.get(..i).unwrap_or(""), i + 1),
            None => (rest, rest.len()),
        };
        let line = line.trim_end_matches('\r').to_string();
        f.pos += adv;
        V::Str(line)
    }

    fn read_char(&mut self, id: u64) -> V {
        let Some(f) = self.lisp.open_files.get_mut(&id) else { return V::Nil };
        let Some(c) = f.text.get(f.pos..).and_then(|r| r.chars().next()) else { return V::Nil };
        f.pos += c.len_utf8();
        // AutoLISP returns 10 for a line end, whatever the file uses.
        if c == '\r' && f.text.get(f.pos..).is_some_and(|r| r.starts_with('\n')) {
            f.pos += 1;
            return V::Int(10);
        }
        V::Int(i64::from(u32::from(c)))
    }

    fn table_names(&self, table: &str) -> Vec<String> {
        let Ok(d) = self.s.doc() else { return Vec::new() };
        match table.to_ascii_uppercase().as_str() {
            "LAYER" => d.layers.iter().map(|l| l.name.clone()).collect(),
            "LTYPE" => d.linetypes.iter().map(|l| l.name.clone()).collect(),
            "STYLE" => d.text_styles.iter().map(|l| l.name.clone()).collect(),
            "DIMSTYLE" => d.dim_styles.iter().map(|l| l.name.clone()).collect(),
            "BLOCK" => d.blocks.iter().filter(|(_, b)| !b.anonymous).map(|(k, _)| k.clone()).collect(),
            "VIEW" => d.views.iter().map(|v| v.name.clone()).collect(),
            "UCS" => d.ucss.iter().map(|u| u.name.clone()).collect(),
            _ => Vec::new(),
        }
    }

    fn dict_entries(&self, dict: u64) -> Vec<String> {
        let Ok(d) = self.s.doc() else { return Vec::new() };
        match dict {
            0 => d.groups.iter().map(|g| g.name.clone()).collect(),
            1 => d.mline_styles.iter().map(|m| m.name.clone()).collect(),
            2 => std::iter::once("Model".to_string()).chain(d.layouts.iter().map(|l| l.name.clone())).collect(),
            _ => Vec::new(),
        }
    }

    fn dict_list(&self, dict: u64) -> V {
        let mut v = vec![dotted(-1, pseudo(K_DICT, dict, 0)), dotted(0, V::Str("DICTIONARY".into()))];
        for (i, n) in self.dict_entries(dict).into_iter().enumerate() {
            v.push(dotted(3, V::Str(n)));
            v.push(dotted(350, pseudo(K_ENTRY, dict, i as u64)));
        }
        V::List(v)
    }

    fn entry_list(&self, dict: u64, i: u64) -> V {
        let Ok(d) = self.s.doc() else { return V::Nil };
        let me = dotted(-1, pseudo(K_ENTRY, dict, i));
        let i = i as usize;
        match dict {
            0 => d.groups.get(i).map_or(V::Nil, |g| {
                let mut v = vec![
                    me,
                    dotted(0, V::Str("GROUP".into())),
                    dotted(300, V::Str(g.description.clone())),
                    dotted(71, V::Int(i64::from(g.selectable))),
                ];
                v.extend(g.members.iter().map(|m| dotted(340, V::Ename(*m))));
                V::List(v)
            }),
            1 => d.mline_styles.get(i).map_or(V::Nil, |m| {
                let mut v = vec![
                    me,
                    dotted(0, V::Str("MLINESTYLE".into())),
                    dotted(2, V::Str(m.name.clone())),
                    dotted(70, V::Int(m.flags())),
                    dotted(3, V::Str(m.description.clone())),
                ];
                v.push(dotted(71, V::Int(m.elements.len() as i64)));
                for e in &m.elements {
                    v.push(dotted(49, V::Real(e.offset)));
                    v.push(dotted(62, V::Int(i64::from(e.color.to_aci()))));
                    v.push(dotted(6, V::Str(e.linetype.clone())));
                }
                V::List(v)
            }),
            2 => {
                let name = if i == 0 { Some("Model".to_string()) } else { d.layouts.get(i - 1).map(|l| l.name.clone()) };
                name.map_or(V::Nil, |n| V::List(vec![me, dotted(0, V::Str("LAYOUT".into())), dotted(1, V::Str(n)), dotted(71, V::Int(i as i64))]))
            }
            _ => V::Nil,
        }
    }

    /// `entget` of a pseudo entity name.
    pub(super) fn pseudo_entget(&self, h: Handle) -> Option<V> {
        let (kind, a, b) = unpseudo(h)?;
        Some(match kind {
            k if (1..=TABLES.len() as u64).contains(&k) => {
                let table = TABLES.get(k as usize - 1)?;
                let name = self.table_names(table).get(b as usize)?.clone();
                let rec = self.tblsearch(table, &name);
                let mut items = vec![dotted(-1, V::Ename(h)), dotted(5, V::Str(format!("{:X}", h.0 & 0xFFFF_FFFF)))];
                items.extend(rec.items().unwrap_or(&[]).iter().cloned());
                V::List(items)
            }
            K_ROOT => {
                let mut v = vec![dotted(-1, V::Ename(h)), dotted(0, V::Str("DICTIONARY".into()))];
                for (i, n) in DICTS.iter().enumerate() {
                    v.push(dotted(3, V::Str((*n).into())));
                    v.push(dotted(350, pseudo(K_DICT, i as u64, 0)));
                }
                V::List(v)
            }
            K_DICT => self.dict_list(a),
            K_ENTRY => self.entry_list(a, b),
            _ => return None,
        })
    }

    pub(super) fn extra(&mut self, name: &str, a: &[V]) -> Option<R<V>> {
        let r: R<V> = match name {
            // ------------------------------------------------------------ text files
            "open" => (|| {
                let file = s_arg(a, 0, name)?.to_string();
                let mode = s_arg(a, 1, name)?.trim().to_ascii_lowercase().chars().next().unwrap_or('r');
                if !matches!(mode, 'r' | 'w' | 'a') {
                    return err("open: chế độ phải là \"r\", \"w\" hoặc \"a\"");
                }
                let text = if mode == 'r' {
                    match self.read_source(&file) {
                        Some(t) => t,
                        None => return Ok(V::Nil),
                    }
                } else {
                    String::new()
                };
                if self.lisp.open_files.len() >= 256 {
                    return err("open: mở quá nhiều tệp");
                }
                self.lisp.next_file += 1;
                let id = self.lisp.next_file;
                let text = text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text);
                self.lisp.open_files.insert(id, LFile { name: file, mode, text, pos: 0 });
                Ok(V::File(id))
            })(),
            "close" => match arg(a, 0) {
                V::File(id) => self.close_file(*id),
                _ => err("close: cần một tệp đã mở"),
            },
            "read-line" => match arg(a, 0) {
                V::File(id) => Ok(self.read_line(*id)),
                _ => Ok(V::Nil),
            },
            "read-char" => match arg(a, 0) {
                V::File(id) => Ok(self.read_char(*id)),
                _ => Ok(V::Nil),
            },
            "write-line" => (|| {
                let t = s_arg(a, 0, name)?.to_string();
                if a.len() > 1 {
                    if !self.file_write(arg(a, 1), &format!("{t}\n")) {
                        return err("write-line: tệp không mở để ghi");
                    }
                } else {
                    self.print(&format!("{t}\n"));
                }
                Ok(V::Str(t))
            })(),
            "write-char" => (|| {
                let c = i_arg(a, 0, name)?;
                let ch = u32::try_from(c).ok().and_then(char::from_u32).unwrap_or('?');
                if a.len() > 1 {
                    if !self.file_write(arg(a, 1), &ch.to_string()) {
                        return err("write-char: tệp không mở để ghi");
                    }
                } else {
                    self.print(&ch.to_string());
                }
                Ok(V::Int(c))
            })(),
            "findfile" => match arg(a, 0) {
                V::Str(f) => Ok(if self.read_source(f).is_some() || self.s.binary_files.contains_key(&f.to_ascii_lowercase()) {
                    V::Str(f.clone())
                } else {
                    V::Nil
                }),
                _ => Ok(V::Nil),
            },
            // ------------------------------------------------------ files and folders
            "vl-filename-base" => s_arg(a, 0, name).map(|p| V::Str(split_path(p).1)),
            "vl-filename-extension" => s_arg(a, 0, name).map(|p| {
                let e = split_path(p).2;
                if e.is_empty() { V::Nil } else { V::Str(e) }
            }),
            "vl-filename-directory" => s_arg(a, 0, name).map(|p| V::Str(split_path(p).0.trim_end_matches(['/', '\\']).to_string())),
            "fnsplitl" => s_arg(a, 0, name).map(|p| {
                let (d, b, e) = split_path(p);
                V::List(vec![V::Str(d), V::Str(b), V::Str(e)])
            }),
            "vl-filename-mktemp" => {
                self.lisp.next_file += 1;
                let pat = match arg(a, 0) {
                    V::Str(s) if !s.is_empty() => s.clone(),
                    _ => "$VL~~".into(),
                };
                let (_, b, e) = split_path(&pat);
                let dir = match arg(a, 1) {
                    V::Str(s) => s.clone(),
                    _ => std::env::temp_dir().to_string_lossy().to_string(),
                };
                let ext = match arg(a, 2) {
                    V::Str(s) => s.clone(),
                    _ if e.is_empty() => ".tmp".into(),
                    _ => e,
                };
                let sep = if dir.is_empty() || dir.ends_with(['/', '\\']) { "" } else { "/" };
                Ok(V::Str(format!("{dir}{sep}{b}{:03x}{ext}", self.lisp.next_file)))
            }
            "vl-file-size" => s_arg(a, 0, name).map(|p| {
                #[cfg(not(target_arch = "wasm32"))]
                if let Ok(m) = std::fs::metadata(p) {
                    return if m.is_dir() { V::Int(0) } else { V::Int(i64::try_from(m.len()).unwrap_or(i64::MAX)) };
                }
                self.read_source(p).map_or(V::Nil, |t| V::Int(t.len() as i64))
            }),
            "vl-file-delete" => s_arg(a, 0, name).map(|p| {
                let key = p.to_ascii_lowercase();
                let mem = self.lisp.files.remove(&key).is_some();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    V::from_bool(std::fs::remove_file(p).is_ok() || mem)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    V::from_bool(mem)
                }
            }),
            "vl-file-copy" => (|| {
                let (src, dst) = (s_arg(a, 0, name)?.to_string(), s_arg(a, 1, name)?.to_string());
                let append = arg(a, 2).truthy();
                let Some(text) = self.read_source(&src) else { return Ok(V::Nil) };
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use std::io::Write;
                    if std::path::Path::new(&dst).exists() && !append {
                        return Ok(V::Nil);
                    }
                    let ok = std::fs::OpenOptions::new().create(true).append(true).open(&dst).and_then(|mut h| h.write_all(text.as_bytes())).is_ok();
                    Ok(if ok { V::Int(text.len() as i64) } else { V::Nil })
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let key = dst.to_ascii_lowercase();
                    let prev = if append { self.lisp.files.get(&key).cloned().unwrap_or_default() } else { String::new() };
                    self.lisp.files.insert(key, prev + &text);
                    Ok(V::Int(text.len() as i64))
                }
            })(),
            "vl-file-rename" => (|| {
                let (src, dst) = (s_arg(a, 0, name)?.to_string(), s_arg(a, 1, name)?.to_string());
                let mem = self.lisp.files.remove(&src.to_ascii_lowercase());
                let moved = mem.is_some();
                if let Some(t) = mem {
                    self.lisp.files.insert(dst.to_ascii_lowercase(), t);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Ok(V::from_bool(std::fs::rename(&src, &dst).is_ok() || moved))
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Ok(V::from_bool(moved))
                }
            })(),
            "vl-file-directory-p" => s_arg(a, 0, name).map(|p| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    V::from_bool(std::path::Path::new(p).is_dir())
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = p;
                    V::Nil
                }
            }),
            "vl-mkdir" => s_arg(a, 0, name).map(|p| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    V::from_bool(std::fs::create_dir(p).is_ok())
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = p;
                    V::Nil
                }
            }),
            "vl-directory-files" => {
                let dir = match arg(a, 0) {
                    V::Str(s) => s.clone(),
                    _ => ".".into(),
                };
                let pat = match arg(a, 1) {
                    V::Str(s) => s.clone(),
                    _ => "*".into(),
                };
                // 1: folders only, -1: files only, else both.
                let mode = match arg(a, 2) {
                    V::Int(i) => *i,
                    _ => 0,
                };
                let mut out: Vec<String> = Vec::new();
                #[cfg(not(target_arch = "wasm32"))]
                if let Ok(rd) = std::fs::read_dir(if dir.is_empty() { "." } else { dir.as_str() }) {
                    for e in rd.flatten().take(100_000) {
                        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                        if (mode == 1 && !is_dir) || (mode == -1 && is_dir) {
                            continue;
                        }
                        let n = e.file_name().to_string_lossy().to_string();
                        if super::builtins::wcmatch(&n.to_ascii_uppercase(), &pat.to_ascii_uppercase()) {
                            out.push(n);
                        }
                    }
                }
                let _ = &dir;
                if out.is_empty() && mode != 1 {
                    out = self
                        .lisp
                        .files
                        .keys()
                        .filter(|n| super::builtins::wcmatch(&n.to_ascii_uppercase(), &pat.to_ascii_uppercase()))
                        .cloned()
                        .collect();
                }
                out.sort();
                Ok(V::list(out.into_iter().map(V::Str).collect()))
            }
            "vl-file-systime" => s_arg(a, 0, name).map(|p| {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(ms) = std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
                    let secs = ms.as_secs() as i64;
                    let days = secs.div_euclid(86_400);
                    let rem = secs.rem_euclid(86_400);
                    let (y, mo, d) = civil(days);
                    let dow = (days + 4).rem_euclid(7);
                    return V::List(vec![
                        V::Int(y),
                        V::Int(mo),
                        V::Int(dow),
                        V::Int(d),
                        V::Int(rem / 3600),
                        V::Int(rem % 3600 / 60),
                        V::Int(rem % 60),
                        V::Int(i64::from(ms.subsec_millis())),
                    ]);
                }
                let _ = p;
                V::Nil
            }),
            "getfiled" => (|| {
                // (getfiled title default ext flags): the file name is typed at the prompt.
                let title = match arg(a, 0) {
                    V::Str(t) => t.clone(),
                    _ => "Tên tệp".into(),
                };
                let def = match arg(a, 1) {
                    V::Str(d) => d.clone(),
                    _ => String::new(),
                };
                let ext = match arg(a, 2) {
                    V::Str(e) if !e.is_empty() => e.trim_start_matches('.').to_string(),
                    _ => String::new(),
                };
                let mut p = crate::Prompt::new(
                    format!("{title} (gõ tên tệp{})", if ext.is_empty() { String::new() } else { format!(", .{ext}") }),
                    crate::Accept::TEXT,
                );
                if !def.is_empty() {
                    p = p.default(def.clone());
                }
                Ok(match self.next_input(p)? {
                    crate::Input::Text(t) | crate::Input::Keyword(t) if !t.trim().is_empty() => {
                        let t = t.trim().trim_matches('"').to_string();
                        if !ext.is_empty() && !t.contains('.') { V::Str(format!("{t}.{ext}")) } else { V::Str(t) }
                    }
                    _ if !def.is_empty() => V::Str(def),
                    _ => V::Nil,
                })
            })(),
            "getenv" => s_arg(a, 0, name).map(|k| {
                let key = k.to_ascii_lowercase();
                if let Some(v) = self.lisp.cfg.get(&format!("env:{key}")) {
                    return V::Str(v.clone());
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Ok(v) = std::env::var(k) {
                    return V::Str(v);
                }
                V::Nil
            }),
            "setenv" => (|| {
                let (k, v) = (s_arg(a, 0, name)?.to_ascii_lowercase(), s_arg(a, 1, name)?.to_string());
                self.lisp.cfg.insert(format!("env:{k}"), v.clone());
                Ok(V::Str(v))
            })(),
            "getcfg" => s_arg(a, 0, name).map(|k| self.lisp.cfg.get(&k.to_ascii_lowercase()).map_or(V::Str(String::new()), |v| V::Str(v.clone()))),
            "setcfg" => (|| {
                let (k, v) = (s_arg(a, 0, name)?.to_ascii_lowercase(), s_arg(a, 1, name)?.to_string());
                self.lisp.cfg.insert(k, v.clone());
                Ok(V::Str(v))
            })(),
            "vl-registry-read" | "vl-registry-write" | "vl-registry-delete" => Ok(V::Nil),
            "startapp" | "menucmd" | "setview" | "vl-get-resource" => Ok(V::Nil),
            "grread" => (|| {
                // (grread [track] [allkeys] [cursor]): (5 pt) moved, (3 pt) picked, (2 code) key.
                let mut p = crate::Prompt::new("", crate::Accept { point: true, number: true, text: true, select: false, enter: true });
                p.track = arg(a, 0).truthy();
                let i = self.next_input(p)?;
                Ok(match i {
                    crate::Input::Motion(w) => V::List(vec![V::Int(5), self.ucs_pt(w)]),
                    crate::Input::Point(w) => V::List(vec![V::Int(3), self.ucs_pt(w)]),
                    crate::Input::Text(t) | crate::Input::Keyword(t) => {
                        let c = t.chars().next().map_or(13, |c| i64::from(u32::from(c)));
                        V::List(vec![V::Int(2), V::Int(c)])
                    }
                    _ => V::List(vec![V::Int(2), V::Int(13)]),
                })
            })(),
            // -------------------------------------------------------- strings & lists
            "vl-string-position" => (|| {
                let c = i_arg(a, 0, name)?;
                let s: Vec<char> = s_arg(a, 1, name)?.chars().collect();
                let start = match arg(a, 2) {
                    V::Int(i) => (*i).max(0) as usize,
                    _ => 0,
                };
                let ch = u32::try_from(c).ok().and_then(char::from_u32);
                let found = if arg(a, 3).truthy() {
                    s.iter().enumerate().skip(start).filter(|(_, x)| Some(**x) == ch).map(|(i, _)| i).next_back()
                } else {
                    s.iter().enumerate().skip(start).find(|(_, x)| Some(**x) == ch).map(|(i, _)| i)
                };
                Ok(found.map_or(V::Nil, |i| V::Int(i as i64)))
            })(),
            "vl-string-translate" => (|| {
                let from: Vec<char> = s_arg(a, 0, name)?.chars().collect();
                let to: Vec<char> = s_arg(a, 1, name)?.chars().collect();
                let s = s_arg(a, 2, name)?;
                Ok(V::Str(
                    s.chars()
                        .map(|c| match from.iter().position(|f| *f == c) {
                            Some(i) => to.get(i).or(to.last()).copied().unwrap_or(c),
                            None => c,
                        })
                        .collect(),
                ))
            })(),
            "vl-string-mismatch" => (|| {
                let s1: Vec<char> = s_arg(a, 0, name)?.chars().collect();
                let s2: Vec<char> = s_arg(a, 1, name)?.chars().collect();
                let p1 = if let V::Int(i) = arg(a, 2) { (*i).max(0) as usize } else { 0 };
                let p2 = if let V::Int(i) = arg(a, 3) { (*i).max(0) as usize } else { 0 };
                let ic = arg(a, 4).truthy();
                let n = s1
                    .iter()
                    .skip(p1)
                    .zip(s2.iter().skip(p2))
                    .take_while(|(x, y)| if ic { x.to_lowercase().eq(y.to_lowercase()) } else { x == y })
                    .count();
                Ok(V::Int(n as i64))
            })(),
            "vl-string-elt" => (|| {
                let s = s_arg(a, 0, name)?;
                let i = i_arg(a, 1, name)?;
                match usize::try_from(i).ok().and_then(|i| s.chars().nth(i)) {
                    Some(c) => Ok(V::Int(i64::from(u32::from(c)))),
                    None => err("vl-string-elt: vị trí vượt quá chuỗi"),
                }
            })(),
            "vl-symbolp" => Ok(V::from_bool(matches!(arg(a, 0), V::Sym(_) | V::T) || matches!(arg(a, 0), V::Nil))),
            "vl-symbol-value" => match arg(a, 0) {
                V::Sym(s) => Ok(self.var(s)),
                V::T => Ok(V::T),
                _ => err("vl-symbol-value: cần một ký hiệu"),
            },
            "vl-list-length" => Ok(match arg(a, 0) {
                V::Nil => V::Int(0),
                V::List(v) => V::Int(v.len() as i64),
                _ => V::Nil,
            }),
            "vl-sort-i" => (|| {
                let items = arg(a, 0).items().map(<[V]>::to_vec).unwrap_or_default();
                let f = arg(a, 1).clone();
                let mut idx: Vec<usize> = (0..items.len()).collect();
                // Insertion sort through the predicate (stable, errors propagate).
                for i in 1..idx.len() {
                    let mut j = i;
                    while j > 0 {
                        let (Some(x), Some(y)) = (idx.get(j).and_then(|k| items.get(*k)), idx.get(j - 1).and_then(|k| items.get(*k))) else { break };
                        if self.call(&f, vec![x.clone(), y.clone()])?.truthy() {
                            idx.swap(j, j - 1);
                            j -= 1;
                        } else {
                            break;
                        }
                    }
                }
                Ok(V::list(idx.into_iter().map(|i| V::Int(i as i64)).collect()))
            })(),
            "vl-member-if-not" => (|| {
                let f = arg(a, 0).clone();
                let items = arg(a, 1).items().map(<[V]>::to_vec).unwrap_or_default();
                for (i, x) in items.iter().enumerate() {
                    if !self.call(&f, vec![x.clone()])?.truthy() {
                        return Ok(V::list(items.get(i..).unwrap_or(&[]).to_vec()));
                    }
                }
                Ok(V::Nil)
            })(),
            "vl-catch-all-error-p" => {
                Ok(V::from_bool(matches!(arg(a, 0), V::List(v) if matches!(v.first(), Some(V::Sym(s)) if s == "*catch-all-error*"))))
            }
            "vl-catch-all-error-message" => Ok(match arg(a, 0) {
                V::List(v) if matches!(v.first(), Some(V::Sym(s)) if s == "*catch-all-error*") => v.get(1).cloned().unwrap_or(V::Nil),
                _ => V::Nil,
            }),
            "vl-doc-set" | "vl-bb-set" => match arg(a, 0) {
                V::Sym(s) => {
                    let v = arg(a, 1).clone();
                    self.lisp.globals.insert(s.clone(), v.clone());
                    Ok(v)
                }
                _ => err(format!("{name}: cần một ký hiệu")),
            },
            "vl-doc-ref" | "vl-bb-ref" => match arg(a, 0) {
                V::Sym(s) => Ok(self.lisp.globals.get(s).cloned().unwrap_or(V::Nil)),
                _ => err(format!("{name}: cần một ký hiệu")),
            },
            "vl-propagate" | "vl-doc-export" | "vl-doc-import" | "vl-arx-import" | "vl-acad-defun" => Ok(V::Nil),
            "boole" => (|| {
                let op = i_arg(a, 0, name)?;
                let mut acc = i_arg(a, 1, name)?;
                for i in 2..a.len() {
                    let b = i_arg(a, i, name)?;
                    let mut r = 0i64;
                    for bit in 0..64 {
                        let x = (acc >> bit) & 1;
                        let y = (b >> bit) & 1;
                        if op & (1 << (3 - (2 * x + y))) != 0 {
                            r |= 1 << bit;
                        }
                    }
                    acc = r;
                }
                Ok(V::Int(acc))
            })(),
            "cvunit" => (|| {
                let from = s_arg(a, 1, name)?;
                let to = s_arg(a, 2, name)?;
                let (Some((k1, f1)), Some((k2, f2))) = (unit(from), unit(to)) else { return Ok(V::Nil) };
                if k1 != k2 {
                    return Ok(V::Nil);
                }
                let conv = |x: f64| x * f1 / f2;
                Ok(match arg(a, 0) {
                    V::List(v) => V::List(v.iter().map(|x| x.num().map_or(x.clone(), |n| V::Real(conv(n)))).collect()),
                    x => x.num().map_or(V::Nil, |n| V::Real(conv(n))),
                })
            })(),
            "snvalid" => s_arg(a, 0, name).map(|n| {
                let bad = ['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'];
                V::from_bool(!n.trim().is_empty() && n.chars().count() <= 255 && !n.contains(bad) && (arg(a, 1).truthy() || !n.contains('|')))
            }),
            "xdsize" => Ok(V::Int(to_text(arg(a, 0), true, &self.lisp.sets).len() as i64)),
            "xdroom" => Ok(V::Int(16383)),
            "regapp" => match arg(a, 0) {
                V::Str(s) if !s.trim().is_empty() => Ok(V::Str(s.clone())),
                _ => Ok(V::Nil),
            },
            "layoutlist" => Ok(self.s.doc().map_or(V::Nil, |d| V::list(d.layouts.iter().map(|l| V::Str(l.name.clone())).collect()))),
            "vports" => {
                Ok(V::List(vec![V::List(vec![V::Int(2), V::List(vec![V::Real(0.0), V::Real(0.0)]), V::List(vec![V::Real(1.0), V::Real(1.0)])])]))
            }
            "textbox" => (|| {
                let items = arg(a, 0).items().map(<[V]>::to_vec).unwrap_or_default();
                let g = |c: i64| {
                    items.iter().find_map(|x| if let V::Dotted(h, t) = x { (h.first() == Some(&V::Int(c))).then(|| (**t).clone()) } else { None })
                };
                let text = match g(1) {
                    Some(V::Str(s)) => s,
                    _ => return Ok(V::Nil),
                };
                let height =
                    g(40).and_then(|v| v.num()).filter(|h| *h > 0.0).unwrap_or_else(|| self.s.doc().map_or(0.2, |d| d.header.f64("TEXTSIZE", 0.2)));
                let style = match g(7) {
                    Some(V::Str(s)) => s,
                    _ => "Standard".into(),
                };
                let t = cadcraft_doc::Text {
                    insert: cadcraft_geom::Vec3::ZERO,
                    align_pt: None,
                    height,
                    value: text.clone(),
                    rotation: 0.0,
                    width_factor: g(41).and_then(|v| v.num()).unwrap_or(1.0),
                    oblique: 0.0,
                    style,
                    halign: Default::default(),
                    valign: Default::default(),
                };
                let b = match self.s.doc() {
                    Ok(d) => cadcraft_render::place_text_entity(d, &t, &text).1,
                    Err(_) => return Ok(V::Nil),
                };
                if b.is_empty() {
                    return Ok(V::List(vec![V::pt3(0.0, 0.0, 0.0), V::pt3(0.0, 0.0, 0.0)]));
                }
                Ok(V::List(vec![V::pt3(b.min.x, b.min.y, 0.0), V::pt3(b.max.x, b.max.y, 0.0)]))
            })(),
            // --------------------------------------------------- tables & dictionaries
            "tblnext" => (|| {
                let table = s_arg(a, 0, name)?.to_ascii_uppercase();
                let names = self.table_names(&table);
                let cur = self.lisp.tbl_cursor.entry(table.clone()).or_insert(0);
                if arg(a, 1).truthy() {
                    *cur = 0;
                }
                let Some(n) = names.get(*cur).cloned() else { return Ok(V::Nil) };
                *cur += 1;
                Ok(self.tblsearch(&table, &n))
            })(),
            "tblobjname" => (|| {
                let table = s_arg(a, 0, name)?.to_ascii_uppercase();
                let n = s_arg(a, 1, name)?;
                let Some(t) = TABLES.iter().position(|x| *x == table) else { return Ok(V::Nil) };
                Ok(self.table_names(&table).iter().position(|x| x.eq_ignore_ascii_case(n)).map_or(V::Nil, |i| pseudo(t as u64 + 1, 0, i as u64)))
            })(),
            "namedobjdict" => Ok(pseudo(K_ROOT, 0, 0)),
            "dictsearch" | "dictnext" => (|| {
                let V::Ename(h) = arg(a, 0) else { return err(format!("{name}: cần tên từ điển")) };
                let Some((kind, which, _)) = unpseudo(*h) else { return Ok(V::Nil) };
                let entries: Vec<(String, V)> = match kind {
                    K_ROOT => DICTS.iter().enumerate().map(|(i, n)| ((*n).to_string(), self.dict_list(i as u64))).collect(),
                    K_DICT => self.dict_entries(which).into_iter().enumerate().map(|(i, n)| (n, self.entry_list(which, i as u64))).collect(),
                    _ => return Ok(V::Nil),
                };
                if name == "dictsearch" {
                    let key = s_arg(a, 1, name)?;
                    return Ok(entries.into_iter().find(|(n, _)| n.eq_ignore_ascii_case(key)).map_or(V::Nil, |(_, v)| v));
                }
                let ckey = format!("dict:{:X}", h.0);
                let cur = self.lisp.tbl_cursor.entry(ckey).or_insert(0);
                if arg(a, 1).truthy() {
                    *cur = 0;
                }
                let r = entries.get(*cur).map_or(V::Nil, |(_, v)| v.clone());
                *cur += 1;
                Ok(r)
            })(),
            // --------------------------------------------------------------- selection
            "ssgetfirst" => {
                let sel = self.s.selection();
                if sel.is_empty() {
                    Ok(V::List(vec![V::Nil, V::Nil]))
                } else {
                    self.lisp.next_set += 1;
                    let id = self.lisp.next_set;
                    self.lisp.sets.insert(id, sel);
                    Ok(V::List(vec![V::Nil, V::Ss(id)]))
                }
            }
            "sssetfirst" => {
                let hs = match arg(a, 1) {
                    V::Ss(id) => self.lisp.sets.get(id).cloned().unwrap_or_default(),
                    _ => match arg(a, 0) {
                        V::Ss(id) => self.lisp.sets.get(id).cloned().unwrap_or_default(),
                        _ => Vec::new(),
                    },
                };
                self.s.set_selection(hs);
                Ok(V::List(a.to_vec()))
            }
            "ssnamex" => match arg(a, 0) {
                V::Ss(id) => {
                    let hs = self.lisp.sets.get(id).cloned().unwrap_or_default();
                    let pick = |h: &Handle| V::List(vec![V::Int(0), V::Ename(*h), V::Int(0)]);
                    Ok(match arg(a, 1) {
                        V::Int(i) => usize::try_from(*i).ok().and_then(|i| hs.get(i)).map_or(V::Nil, |h| V::List(vec![pick(h)])),
                        _ => V::list(hs.iter().map(pick).collect()),
                    })
                }
                _ => Ok(V::Nil),
            },
            // --------------------------------------------------- temporary graphics
            "grdraw" => (|| {
                let (Some(p), Some(q)) = (self.to_world(arg(a, 0)), self.to_world(arg(a, 1))) else { return err("grdraw: cần hai điểm") };
                let c = arg(a, 2).num().unwrap_or(7.0) as i16;
                if self.s.temp_vectors.len() < 100_000 {
                    self.s.temp_vectors.push((p, q, c));
                }
                Ok(V::Nil)
            })(),
            "grvecs" => {
                let items = arg(a, 0).items().map(<[V]>::to_vec).unwrap_or_default();
                let mut color: i16 = 7;
                let mut i = 0;
                while i < items.len() {
                    match items.get(i) {
                        Some(V::Int(c)) => {
                            color = *c as i16;
                            i += 1;
                        }
                        _ => {
                            let (Some(p), Some(q)) = (items.get(i).and_then(|v| self.to_world(v)), items.get(i + 1).and_then(|v| self.to_world(v)))
                            else {
                                break;
                            };
                            if self.s.temp_vectors.len() < 100_000 {
                                self.s.temp_vectors.push((p, q, color));
                            }
                            i += 2;
                        }
                    }
                }
                Ok(V::Nil)
            }
            "redraw" => {
                if a.is_empty() {
                    self.s.temp_vectors.clear();
                }
                Ok(V::Nil)
            }
            "grtext" => {
                if let V::Str(t) = arg(a, 1) {
                    self.s.echo(t.clone());
                }
                Ok(arg(a, 1).clone())
            }
            _ => return None,
        };
        Some(r)
    }

    fn var(&self, s: &str) -> V {
        if s == "pi" {
            return V::Real(std::f64::consts::PI);
        }
        self.lookup(s)
    }
}

/// (year, month, day) of a day count since 1970-01-01.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ======================================================================= ActiveX
//
// Collections are symbols (`*coll:layers*`…), their items pseudo entity names (layers, styles,
// blocks) or real ones (objects of a space).

const COLLS: &[(&str, &str, u64)] = &[
    ("vla-get-layers", "layers", 1),
    ("vla-get-linetypes", "linetypes", 2),
    ("vla-get-textstyles", "textstyles", 3),
    ("vla-get-dimstyles", "dimstyles", 4),
    ("vla-get-blocks", "blocks", 5),
    ("vla-get-views", "views", 6),
    ("vla-get-usercoordinatesystems", "ucss", 7),
];

fn coll_kind(v: &V) -> Option<u64> {
    let V::Sym(s) = v else { return None };
    let n = s.strip_prefix("*coll:")?.strip_suffix('*')?;
    COLLS.iter().find(|(_, c, _)| *c == n).map(|(_, _, k)| *k)
}

impl Run<'_> {
    fn space_handles(&self) -> Vec<Handle> {
        let space = self.s.space();
        self.s.doc().ok().and_then(|d| d.space(&space).map(|st| st.iter().map(|e| e.handle).collect())).unwrap_or_default()
    }

    /// Items of a collection (`vlax-for`, `vla-item`).
    pub(super) fn collection(&self, v: &V) -> Option<Vec<V>> {
        if matches!(v, V::Sym(s) if s == "*space*") {
            return Some(self.space_handles().into_iter().map(V::Ename).collect());
        }
        let k = coll_kind(v)?;
        let table = TABLES.get(k as usize - 1)?;
        Some((0..self.table_names(table).len()).map(|i| pseudo(k, 0, i as u64)).collect())
    }

    /// Run a command and return the objects it added.
    fn added_by(&mut self, cmd: &str, p: serde_json::Value) -> R<Vec<V>> {
        let before: std::collections::HashSet<Handle> = self.space_handles().into_iter().collect();
        self.s.execute(cmd, &p).map_err(|e| super::Ex::Error(e.to_string()))?;
        Ok(self.space_handles().into_iter().filter(|h| !before.contains(h)).map(V::Ename).collect())
    }

    fn obj(&self, v: &V, f: &str) -> R<Handle> {
        match v {
            V::Ename(h) if self.s.doc().is_ok_and(|d| d.entity(*h).is_some()) => Ok(*h),
            _ => err(format!("{f}: cần một đối tượng")),
        }
    }

    fn pseudo_layer(&self, v: &V) -> Option<String> {
        let V::Ename(h) = v else { return None };
        let (k, _, i) = unpseudo(*h)?;
        let table = TABLES.get(usize::try_from(k).ok()?.checked_sub(1)?)?;
        self.table_names(table).get(i as usize).cloned().map(|n| format!("{table}\u{1}{n}"))
    }

    fn table_prop(&mut self, rec: &str, prop: &str, put: Option<&V>) -> R<V> {
        let (table, name) = rec.split_once('\u{1}').unwrap_or(("", rec));
        let p = prop.to_ascii_lowercase();
        if p == "name" && put.is_none() {
            return Ok(V::Str(name.to_string()));
        }
        if table != "LAYER" {
            return err(format!("{prop}: chưa hỗ trợ cho bảng {table}"));
        }
        let d = self.s.doc_mut().map_err(|e| super::Ex::Error(e.to_string()))?;
        let Some(l) = d.layer_mut(name) else { return Ok(V::Nil) };
        let on = |v: &V| v.truthy();
        let r = match (p.as_str(), put) {
            ("color", None) => V::Int(i64::from(l.color.to_aci())),
            ("color", Some(v)) => {
                if let Some(c) = v.num() {
                    l.color = cadcraft_color::Color::from_aci((c as i16).clamp(1, 255));
                }
                V::Nil
            }
            ("linetype", None) => V::Str(l.linetype.clone()),
            ("linetype", Some(V::Str(s))) => {
                l.linetype = s.clone();
                V::Nil
            }
            ("layeron", None) => V::from_bool(l.on),
            ("layeron", Some(v)) => {
                l.on = on(v);
                V::Nil
            }
            ("freeze", None) => V::from_bool(l.frozen),
            ("freeze", Some(v)) => {
                l.frozen = on(v);
                V::Nil
            }
            ("lock", None) => V::from_bool(l.locked),
            ("lock", Some(v)) => {
                l.locked = on(v);
                V::Nil
            }
            ("plottable", None) => V::from_bool(l.plot),
            ("plottable", Some(v)) => {
                l.plot = on(v);
                V::Nil
            }
            ("description", None) => V::Str(l.description.clone()),
            ("description", Some(V::Str(s))) => {
                l.description = s.clone();
                V::Nil
            }
            _ => return err(format!("thuộc tính lớp {prop} chưa hỗ trợ")),
        };
        if put.is_some() {
            self.s.touch();
        }
        Ok(r)
    }

    /// More `vla-*` / `vlax-*`: collections, layer objects, methods, Add… functions.
    pub(super) fn vla_extra(&mut self, n: &str, a: &[V]) -> Option<R<V>> {
        if let Some((_, c, _)) = COLLS.iter().find(|(f, _, _)| *f == n) {
            return Some(Ok(V::Sym(format!("*coll:{c}*"))));
        }
        // Properties of table records (layers…).
        if let Some(rec) = self.pseudo_layer(arg(a, 0)) {
            if let Some(p) = n.strip_prefix("vla-get-") {
                return Some(self.table_prop(&rec, p, None));
            }
            if let Some(p) = n.strip_prefix("vla-put-") {
                let v = arg(a, 1).clone();
                return Some(self.table_prop(&rec, p, Some(&v)));
            }
            if n == "vlax-get-property" || n == "vlax-get" || n == "vlax-put-property" || n == "vlax-put" {
                let prop = match arg(a, 1) {
                    V::Sym(s) | V::Str(s) => s.clone(),
                    _ => return Some(err("cần tên thuộc tính")),
                };
                let v = arg(a, 2).clone();
                return Some(self.table_prop(&rec, &prop, if n.contains("put") { Some(&v) } else { None }));
            }
        }
        let pt = |i: usize| arg(a, i).point();
        let p2 = |p: Vec2| serde_json::json!([p.x, p.y]);
        let first = |v: R<Vec<V>>| v.map(|x| x.into_iter().next().unwrap_or(V::Nil));
        let r: R<V> = match n {
            "vla-get-count" => Ok(V::Int(self.collection(arg(a, 0))?.len() as i64)),
            "vla-item" => (|| {
                let items = self.collection(arg(a, 0)).ok_or_else(|| super::Ex::Error("vla-item: cần một tập hợp".into()))?;
                match arg(a, 1) {
                    V::Int(i) => Ok(usize::try_from(*i).ok().and_then(|i| items.get(i)).cloned().unwrap_or(V::Nil)),
                    V::Str(name) => {
                        let k = coll_kind(arg(a, 0)).unwrap_or(0);
                        let table = TABLES.get((k as usize).saturating_sub(1)).copied().unwrap_or("");
                        match self.table_names(table).iter().position(|x| x.eq_ignore_ascii_case(name)) {
                            Some(i) => Ok(pseudo(k, 0, i as u64)),
                            None => err(format!("vla-item: không có {name}")),
                        }
                    }
                    _ => err("vla-item: cần tên hoặc chỉ số"),
                }
            })(),
            "vla-add" => (|| {
                let name = s_arg(a, 1, n)?.to_string();
                match coll_kind(arg(a, 0)) {
                    Some(1) => {
                        if self.s.doc().is_ok_and(|d| d.layer(&name).is_none()) {
                            self.s.execute("layer.new", &serde_json::json!({ "name": name })).map_err(|e| super::Ex::Error(e.to_string()))?;
                        }
                        let i = self.table_names("LAYER").iter().position(|x| x.eq_ignore_ascii_case(&name)).unwrap_or(0);
                        Ok(pseudo(1, 0, i as u64))
                    }
                    _ => err("vla-Add: VNCCad mới hỗ trợ thêm lớp (Layers)"),
                }
            })(),
            "vla-move" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                let (Some(f), Some(t)) = (pt(1), pt(2)) else { return err("vla-Move: cần hai điểm") };
                self.added_by("move", serde_json::json!({ "handles": [h.hex()], "from": p2(f), "to": p2(t) }))?;
                Ok(V::Nil)
            })(),
            "vla-copy" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                first(self.added_by("copy", serde_json::json!({ "handles": [h.hex()], "delta": [0, 0] })))
            })(),
            "vla-rotate" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                let (Some(b), Some(ang)) = (pt(1), arg(a, 2).num()) else { return err("vla-Rotate: cần điểm gốc và góc") };
                self.added_by("rotate", serde_json::json!({ "handles": [h.hex()], "base": p2(b), "angle": ang.to_degrees() }))?;
                Ok(V::Nil)
            })(),
            "vla-scaleentity" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                let (Some(b), Some(k)) = (pt(1), arg(a, 2).num()) else { return err("vla-ScaleEntity: cần điểm gốc và hệ số") };
                self.added_by("scale", serde_json::json!({ "handles": [h.hex()], "base": p2(b), "factor": k }))?;
                Ok(V::Nil)
            })(),
            "vla-mirror" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                let (Some(p), Some(q)) = (pt(1), pt(2)) else { return err("vla-Mirror: cần hai điểm") };
                first(self.added_by("mirror", serde_json::json!({ "handles": [h.hex()], "p1": p2(p), "p2": p2(q), "erase": false })))
            })(),
            "vla-offset" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                let dist = arg(a, 1).num().ok_or_else(|| super::Ex::Error("vla-Offset: cần khoảng cách".into()))?;
                // Positive: outside of a closed curve, left of an open one.
                let side = {
                    let d = self.s.doc().map_err(|e| super::Ex::Error(e.to_string()))?;
                    let e = d.entity(h).ok_or_else(|| super::Ex::Error("đối tượng đã bị xóa".into()))?;
                    let b = cadcraft_doc::entity_bounds(d, e, 0);
                    let closed = match &e.kind {
                        cadcraft_doc::EntityKind::Circle(_) | cadcraft_doc::EntityKind::Ellipse(_) => true,
                        cadcraft_doc::EntityKind::LwPolyline(p) => p.closed,
                        _ => false,
                    };
                    if closed {
                        let out = b.max + Vec2::new(b.width().max(1.0), b.height().max(1.0));
                        if dist >= 0.0 { out } else { b.center() }
                    } else {
                        let g = e.kind.grips();
                        let (p, q) = (g.first().copied().unwrap_or_default(), g.get(1).copied().unwrap_or(Vec2::X));
                        let left = (q - p).normalized().perp();
                        p.mid(q) + left * if dist >= 0.0 { dist.abs().max(1e-6) } else { -dist.abs().max(1e-6) }
                    }
                };
                let r = self.added_by("offset", serde_json::json!({ "handle": h.hex(), "distance": dist.abs(), "side": p2(side) }))?;
                Ok(V::list(r))
            })(),
            "vla-explode" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                // EXPLODE removes the original; vla-Explode keeps it, as AutoCAD does.
                let keep = self.s.doc().ok().and_then(|d| d.entity(h).map(|e| (**e).clone()));
                let r = self.added_by("explode", serde_json::json!({ "handles": [h.hex()] }))?;
                let space = self.s.space();
                if let Some(e) = keep
                    && let Ok(d) = self.s.doc_mut()
                    && d.entity(h).is_none()
                    && let Some(st) = d.space_mut(&space)
                {
                    st.push(e);
                }
                Ok(V::list(r))
            })(),
            "vla-getboundingbox" => (|| {
                let h = self.obj(arg(a, 0), n)?;
                let b = self
                    .s
                    .doc()
                    .ok()
                    .and_then(|d| d.entity(h).map(|e| cadcraft_doc::entity_bounds(d, e, 0)))
                    .unwrap_or(cadcraft_geom::Bounds2::EMPTY);
                if let V::Sym(lo) = arg(a, 1) {
                    self.set(lo, V::pt3(b.min.x, b.min.y, 0.0));
                }
                if let V::Sym(hi) = arg(a, 2) {
                    self.set(hi, V::pt3(b.max.x, b.max.y, 0.0));
                }
                Ok(V::Nil)
            })(),
            "vla-highlight" | "vla-regen" | "vla-update" => Ok(V::Nil),
            "vla-addmtext" => (|| {
                let p = pt(1).ok_or_else(|| super::Ex::Error("vla-AddMText: cần điểm".into()))?;
                let w = arg(a, 2).num().unwrap_or(0.0);
                let t = s_arg(a, 3, n)?.to_string();
                first(self.added_by("mtext", serde_json::json!({ "at": p2(p), "width": w, "text": t })))
            })(),
            "vla-addarc" => (|| {
                let (Some(c), Some(r), Some(s0), Some(e0)) = (pt(1), arg(a, 2).num(), arg(a, 3).num(), arg(a, 4).num()) else {
                    return err("vla-AddArc: cần tâm, bán kính, góc đầu, góc cuối");
                };
                first(self.added_by("arc", serde_json::json!({ "center": p2(c), "radius": r, "start": s0.to_degrees(), "end": e0.to_degrees() })))
            })(),
            "vla-addpoint" => (|| {
                let p = pt(1).ok_or_else(|| super::Ex::Error("vla-AddPoint: cần điểm".into()))?;
                first(self.added_by("point", serde_json::json!({ "at": p2(p) })))
            })(),
            "vla-addellipse" => (|| {
                let (Some(c), Some(m), Some(ratio)) = (pt(1), pt(2), arg(a, 3).num()) else {
                    return err("vla-AddEllipse: cần tâm, trục chính, tỷ lệ");
                };
                first(self.added_by("ellipse", serde_json::json!({ "center": p2(c), "major": p2(m), "ratio": ratio })))
            })(),
            "vla-insertblock" => (|| {
                let p = pt(1).ok_or_else(|| super::Ex::Error("vla-InsertBlock: cần điểm".into()))?;
                let name = s_arg(a, 2, n)?.to_string();
                let sc = arg(a, 3).num().unwrap_or(1.0);
                let rot = arg(a, 6).num().unwrap_or(0.0);
                first(self.added_by("insert", serde_json::json!({ "name": name, "at": p2(p), "scale": sc, "rotation": rot.to_degrees() })))
            })(),
            "vla-adddimaligned" | "vla-adddimrotated" => (|| {
                let (Some(p), Some(q), Some(t)) = (pt(1), pt(2), pt(3)) else {
                    return err(format!("{n}: cần hai điểm gốc và vị trí đường kích thước"));
                };
                if n == "vla-adddimrotated" {
                    let rot = arg(a, 4).num().unwrap_or(0.0);
                    first(self.added_by("dimlinear", serde_json::json!({ "p1": p2(p), "p2": p2(q), "at": p2(t), "rotation": rot.to_degrees() })))
                } else {
                    first(self.added_by("dimaligned", serde_json::json!({ "p1": p2(p), "p2": p2(q), "at": p2(t) })))
                }
            })(),
            "vlax-invoke" | "vlax-invoke-method" => {
                let m = match arg(a, 1) {
                    V::Sym(s) | V::Str(s) => s.to_ascii_lowercase(),
                    _ => return Some(err(format!("{n}: cần tên phương thức"))),
                };
                let mut args = vec![arg(a, 0).clone()];
                args.extend(a.iter().skip(2).cloned());
                self.call_builtin(&format!("vla-{m}"), args)
            }
            "vlax-make-safearray" | "vlax-get-or-create-object" | "vlax-create-object" => Ok(V::Nil),
            "vlax-safearray-fill" => Ok(arg(a, 1).clone()),
            "vlax-safearray-get-u-bound" => Ok(V::Int(arg(a, 0).items().map_or(0, |v| v.len() as i64) - 1)),
            "vlax-safearray-get-l-bound" => Ok(V::Int(0)),
            "vlax-variant-type" => Ok(V::Int(match arg(a, 0) {
                V::Int(_) => 3,
                V::Real(_) => 5,
                V::Str(_) => 8,
                V::List(_) => 8197,
                _ => 0,
            })),
            "vlax-object-released-p" => Ok(V::Nil),
            "vlax-erased-p" => Ok(V::from_bool(matches!(arg(a, 0), V::Ename(h) if self.s.doc().is_ok_and(|d| d.entity(*h).is_none())))),
            "vlax-typeinfo-available-p" | "vlax-method-applicable-p" => Ok(V::T),
            _ => return None,
        };
        Some(r)
    }
}
