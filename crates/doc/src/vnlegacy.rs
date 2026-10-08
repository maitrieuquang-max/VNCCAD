//! VNCCad: convert Vietnamese text written with legacy 8-bit fonts to Unicode.
//!
//! Many drawings made in Vietnam before Unicode store text in one of two font-specific
//! encodings, and only look right with those fonts installed:
//!
//! - **TCVN3 (ABC)**: fonts named `.VnTime`, `.VnArial`, … (files `vntime.ttf`, `vnarial.ttf`).
//!   One byte per letter. The `…H` variants (`.VnTimeH`, `vntimeh.ttf`) show every letter as a
//!   capital.
//! - **VNI**: fonts named `VNI-Times`, `VNI-Helve`, … A base letter followed by a mark byte.
//!
//! In the drawing these bytes read as Latin-1 letters ("CÇu", "Caàu"). This module recognises
//! such styles (and inline MTEXT font switches), rewrites their text as Unicode ("Cầu") and
//! points the styles at an ordinary Unicode font.

use std::sync::Arc;

use crate::{Drawing, EntityKind, EntityStore};

/// A legacy Vietnamese encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legacy {
    /// TCVN3 / ABC; `upper` for the capital-only `…H` fonts.
    Tcvn3 { upper: bool },
    /// VNI Windows.
    Vni,
}

/// What a conversion changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VnReport {
    /// Text values rewritten.
    pub texts: usize,
    /// Styles pointed at a Unicode font, as (style, old font, new font).
    pub styles: Vec<(String, String, String)>,
}

fn stem(font: &str) -> String {
    let f = font.trim().trim_start_matches('.').to_ascii_lowercase();
    let f = f.rsplit(['/', '\\']).next().unwrap_or(&f).to_string();
    match f.rsplit_once('.') {
        Some((a, "ttf" | "otf" | "shx" | "ttc" | "pfb")) => a.to_string(),
        _ => f,
    }
    .trim()
    .to_string()
}

/// The legacy encoding a font name implies, if any.
pub fn legacy_of_font(font: &str) -> Option<Legacy> {
    let s = stem(font);
    if s.starts_with("vni") {
        return Some(Legacy::Vni);
    }
    if s.starts_with("vn") && s.len() > 3 {
        let upper = s.ends_with('h');
        return Some(Legacy::Tcvn3 { upper });
    }
    None
}

/// A Unicode font to use instead of a legacy one.
pub fn unicode_font_for(font: &str) -> &'static str {
    let s = stem(font);
    let s = s.trim_start_matches("vni-").trim_start_matches("vni").trim_start_matches("vn");
    if s.starts_with("time") || s.starts_with("book") || s.starts_with("century") || s.starts_with("bodoni") {
        "Times New Roman"
    } else if s.contains("courier") {
        "Courier New"
    } else {
        "Arial"
    }
}

// --- composition -------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modi {
    None,
    Breve,
    Circumflex,
    Horn,
}

/// Rows: base, modifier, letter with no tone, grave, acute, hook above, tilde, dot below.
const ROWS: [(char, Modi, &str); 12] = [
    ('a', Modi::None, "aàáảãạ"),
    ('a', Modi::Breve, "ăằắẳẵặ"),
    ('a', Modi::Circumflex, "âầấẩẫậ"),
    ('e', Modi::None, "eèéẻẽẹ"),
    ('e', Modi::Circumflex, "êềếểễệ"),
    ('i', Modi::None, "iìíỉĩị"),
    ('o', Modi::None, "oòóỏõọ"),
    ('o', Modi::Circumflex, "ôồốổỗộ"),
    ('o', Modi::Horn, "ơờớởỡợ"),
    ('u', Modi::None, "uùúủũụ"),
    ('u', Modi::Horn, "ưừứửữự"),
    ('y', Modi::None, "yỳýỷỹỵ"),
];

/// Tone index: 0 none, 1 grave, 2 acute, 3 hook above, 4 tilde, 5 dot below.
fn compose(base: char, m: Modi, tone: usize) -> Option<char> {
    let upper = base.is_uppercase();
    let lower = base.to_lowercase().next()?;
    let (b, m) = match lower {
        'ơ' => ('o', Modi::Horn),
        'ư' => ('u', Modi::Horn),
        'ô' if m == Modi::None => ('o', Modi::Circumflex),
        c => (c, m),
    };
    let row = ROWS.iter().find(|r| r.0 == b && r.1 == m)?;
    let c = row.2.chars().nth(tone)?;
    if upper { c.to_uppercase().next() } else { Some(c) }
}

// --- TCVN3 -------------------------------------------------------------------------------------

const TCVN3: [(u8, char); 67] = [
    (0xA1, 'Ă'),
    (0xA2, 'Â'),
    (0xA3, 'Ê'),
    (0xA4, 'Ô'),
    (0xA5, 'Ơ'),
    (0xA6, 'Ư'),
    (0xA7, 'Đ'),
    (0xA8, 'ă'),
    (0xA9, 'â'),
    (0xAA, 'ê'),
    (0xAB, 'ô'),
    (0xAC, 'ơ'),
    (0xAD, 'ư'),
    (0xAE, 'đ'),
    (0xB5, 'à'),
    (0xB6, 'ả'),
    (0xB7, 'ã'),
    (0xB8, 'á'),
    (0xB9, 'ạ'),
    (0xBB, 'ằ'),
    (0xBC, 'ẳ'),
    (0xBD, 'ẵ'),
    (0xBE, 'ắ'),
    (0xC6, 'ặ'),
    (0xC7, 'ầ'),
    (0xC8, 'ẩ'),
    (0xC9, 'ẫ'),
    (0xCA, 'ấ'),
    (0xCB, 'ậ'),
    (0xCC, 'è'),
    (0xCE, 'ẻ'),
    (0xCF, 'ẽ'),
    (0xD0, 'é'),
    (0xD1, 'ẹ'),
    (0xD2, 'ề'),
    (0xD3, 'ể'),
    (0xD4, 'ễ'),
    (0xD5, 'ế'),
    (0xD6, 'ệ'),
    (0xD7, 'ì'),
    (0xD8, 'ỉ'),
    (0xDC, 'ĩ'),
    (0xDD, 'í'),
    (0xDE, 'ị'),
    (0xDF, 'ò'),
    (0xE1, 'ỏ'),
    (0xE2, 'õ'),
    (0xE3, 'ó'),
    (0xE4, 'ọ'),
    (0xE5, 'ồ'),
    (0xE6, 'ổ'),
    (0xE7, 'ỗ'),
    (0xE8, 'ố'),
    (0xE9, 'ộ'),
    (0xEA, 'ờ'),
    (0xEB, 'ở'),
    (0xEC, 'ỡ'),
    (0xED, 'ớ'),
    (0xEE, 'ợ'),
    (0xEF, 'ù'),
    (0xF1, 'ủ'),
    (0xF2, 'ũ'),
    (0xF3, 'ú'),
    (0xF4, 'ụ'),
    (0xF5, 'ừ'),
    (0xF6, 'ử'),
    (0xF7, 'ữ'),
];

const TCVN3_TAIL: [(u8, char); 7] = [(0xF8, 'ứ'), (0xF9, 'ự'), (0xFA, 'ỳ'), (0xFB, 'ỷ'), (0xFC, 'ỹ'), (0xFD, 'ý'), (0xFE, 'ỵ')];

fn tcvn3_char(c: char) -> Option<char> {
    let code = u32::from(c);
    if !(0xA1..=0xFE).contains(&code) {
        return None;
    }
    let b = u8::try_from(code).ok()?;
    TCVN3.iter().chain(TCVN3_TAIL.iter()).find(|(k, _)| *k == b).map(|(_, v)| *v)
}

/// Convert TCVN3 text (read as Latin-1 characters) to Unicode.
pub fn tcvn3_to_unicode(s: &str, upper: bool) -> String {
    let out: String = s.chars().map(|c| tcvn3_char(c).unwrap_or(c)).collect();
    if upper { out.to_uppercase() } else { out }
}

// --- VNI ---------------------------------------------------------------------------------------

/// A VNI mark byte: (modifier, tone).
fn vni_mark(c: char) -> Option<(Modi, usize)> {
    Some(match c.to_lowercase().next()? {
        'ø' => (Modi::None, 1),
        'ù' => (Modi::None, 2),
        'û' => (Modi::None, 3),
        'õ' => (Modi::None, 4),
        'ï' => (Modi::None, 5),
        'â' => (Modi::Circumflex, 0),
        'à' => (Modi::Circumflex, 1),
        'á' => (Modi::Circumflex, 2),
        'å' => (Modi::Circumflex, 3),
        'ã' => (Modi::Circumflex, 4),
        'ä' => (Modi::Circumflex, 5),
        'ê' => (Modi::Breve, 0),
        'è' => (Modi::Breve, 1),
        'é' => (Modi::Breve, 2),
        'ú' => (Modi::Breve, 3),
        'ü' => (Modi::Breve, 4),
        'ë' => (Modi::Breve, 5),
        _ => return None,
    })
}

/// VNI characters that stand alone.
fn vni_single(c: char) -> Option<char> {
    Some(match c {
        'ô' => 'ơ',
        'Ô' => 'Ơ',
        'ö' => 'ư',
        'Ö' => 'Ư',
        'ñ' => 'đ',
        'Ñ' => 'Đ',
        'æ' => 'ỉ',
        'Æ' => 'Ỉ',
        'ó' => 'ĩ',
        'Ó' => 'Ĩ',
        'ò' => 'ị',
        'Ò' => 'Ị',
        'î' => 'ỵ',
        'Î' => 'Ỵ',
        _ => return None,
    })
}

/// Convert VNI text (read as Latin-1 characters) to Unicode.
pub fn vni_to_unicode(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let base = vni_single(c).unwrap_or(c);
        let is_base = matches!(base.to_lowercase().next(), Some('a' | 'e' | 'o' | 'u' | 'y' | 'ơ' | 'ư'));
        if is_base
            && let Some((m, tone)) = chars.get(i + 1).and_then(|&n| vni_mark(n))
            && let Some(composed) = compose(base, m, tone)
        {
            out.push(composed);
            i += 2;
            continue;
        }
        out.push(base);
        i += 1;
    }
    out
}

/// Convert one plain string in `enc` to Unicode. Text that already holds characters beyond
/// Latin-1 is Unicode already and is left alone (except the capital-only fonts' case rule).
pub fn to_unicode(s: &str, enc: Legacy) -> String {
    let already = s.chars().any(|c| u32::from(c) > 0xFF);
    match enc {
        Legacy::Tcvn3 { upper } if already => {
            if upper {
                s.to_uppercase()
            } else {
                s.to_string()
            }
        }
        Legacy::Vni if already => s.to_string(),
        Legacy::Tcvn3 { upper } => tcvn3_to_unicode(s, upper),
        Legacy::Vni => vni_to_unicode(s),
    }
}

// --- MTEXT -------------------------------------------------------------------------------------

/// Convert MTEXT contents: text runs are converted in the encoding of the font active at that
/// point (`default` from the style, switched by inline `\f…;`/`\F…;` codes), and legacy inline
/// font names are replaced by Unicode ones. Formatting codes are kept as they are.
pub fn convert_mtext(s: &str, default: Option<Legacy>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut stack: Vec<Option<Legacy>> = Vec::new();
    let mut cur = default;
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String, cur: Option<Legacy>| {
        if !run.is_empty() {
            match cur {
                Some(enc) => out.push_str(&to_unicode(run, enc)),
                None => out.push_str(run),
            }
            run.clear();
        }
    };
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '{' => {
                flush(&mut run, &mut out, cur);
                stack.push(cur);
                out.push(c);
                i += 1;
            }
            '}' => {
                flush(&mut run, &mut out, cur);
                cur = stack.pop().unwrap_or(default);
                out.push(c);
                i += 1;
            }
            '\\' => {
                flush(&mut run, &mut out, cur);
                let Some(&code) = chars.get(i + 1) else {
                    out.push(c);
                    break;
                };
                match code {
                    'f' | 'F' => {
                        let end = chars.iter().skip(i + 2).position(|&x| x == ';').map(|p| i + 2 + p);
                        let Some(end) = end else {
                            out.extend(chars.iter().skip(i));
                            break;
                        };
                        let spec: String = chars.get(i + 2..end).map(|x| x.iter().collect()).unwrap_or_default();
                        let (name, rest) = match spec.split_once('|') {
                            Some((n, r)) => (n.to_string(), format!("|{r}")),
                            None => (spec.clone(), String::new()),
                        };
                        cur = legacy_of_font(&name);
                        let name = if cur.is_some() { unicode_font_for(&name).to_string() } else { name };
                        out.push('\\');
                        out.push(code);
                        out.push_str(&name);
                        out.push_str(&rest);
                        out.push(';');
                        i = end + 1;
                    }
                    // Codes with an argument up to ';' (heights, colours, tracking, …).
                    'H' | 'W' | 'Q' | 'T' | 'A' | 'C' | 'c' | 'p' => {
                        let end = chars.iter().skip(i + 2).position(|&x| x == ';').map_or(chars.len(), |p| i + 2 + p + 1);
                        out.extend(chars.get(i..end).unwrap_or(&[]).iter());
                        i = end;
                    }
                    // Stacked fraction: its text is converted, the code kept.
                    'S' => {
                        let end = chars.iter().skip(i + 2).position(|&x| x == ';').map_or(chars.len(), |p| i + 2 + p);
                        let inner: String = chars.get(i + 2..end).map(|x| x.iter().collect()).unwrap_or_default();
                        out.push_str("\\S");
                        match cur {
                            Some(enc) => out.push_str(&to_unicode(&inner, enc)),
                            None => out.push_str(&inner),
                        }
                        if end < chars.len() {
                            out.push(';');
                        }
                        i = end + 1;
                    }
                    // `\U+XXXX` is already Unicode.
                    'U' if chars.get(i + 2) == Some(&'+') => {
                        out.extend(chars.get(i..(i + 7).min(chars.len())).unwrap_or(&[]).iter());
                        i += 7;
                    }
                    _ => {
                        out.push('\\');
                        out.push(code);
                        i += 2;
                    }
                }
            }
            _ => {
                run.push(c);
                i += 1;
            }
        }
    }
    flush(&mut run, &mut out, cur);
    out
}

// --- drawing -----------------------------------------------------------------------------------

/// Text styles that use a legacy font, by upper-case style name.
fn legacy_styles(d: &Drawing) -> Vec<(String, Legacy)> {
    d.text_styles
        .iter()
        .filter_map(|s| legacy_of_font(&s.font).or_else(|| legacy_of_font(&s.big_font)).map(|l| (s.name.to_ascii_uppercase(), l)))
        .collect()
}

fn style_enc(styles: &[(String, Legacy)], style: &str) -> Option<Legacy> {
    let key = if style.is_empty() { "STANDARD".to_string() } else { style.to_ascii_uppercase() };
    styles.iter().find(|(n, _)| *n == key).map(|(_, l)| *l)
}

fn has_inline_legacy(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.contains("\\f.vn") || lower.contains("\\fvn") || lower.contains("\\fvni")
}

fn convert_kind(kind: &mut EntityKind, enc_of: &dyn Fn(&str) -> Option<Legacy>) -> usize {
    let mut n = 0;
    let mut set = |text: &mut String, new: String| {
        if *text != new {
            *text = new;
            n += 1;
        }
    };
    match kind {
        EntityKind::Text(t) => {
            if let Some(enc) = enc_of(&t.style) {
                let new = to_unicode(&t.value, enc);
                set(&mut t.value, new);
            }
        }
        EntityKind::MText(m) => {
            let enc = enc_of(&m.style);
            if enc.is_some() || has_inline_legacy(&m.contents) {
                let new = convert_mtext(&m.contents, enc);
                set(&mut m.contents, new);
            }
        }
        EntityKind::AttDef(a) => {
            if let Some(enc) = enc_of(&a.text.style) {
                let new = to_unicode(&a.text.value, enc);
                set(&mut a.text.value, new);
                let p = to_unicode(&a.prompt, enc);
                set(&mut a.prompt, p);
            }
        }
        EntityKind::Insert(ins) => {
            for a in &mut ins.attribs {
                if let Some(enc) = enc_of(&a.text.style) {
                    let new = to_unicode(&a.text.value, enc);
                    set(&mut a.text.value, new);
                }
            }
        }
        EntityKind::Dimension(dim) => {
            let enc = enc_of(&dim.style);
            if !dim.text.is_empty() && (enc.is_some() || has_inline_legacy(&dim.text)) {
                let new = convert_mtext(&dim.text, enc);
                set(&mut dim.text, new);
            }
        }
        EntityKind::MLeader(ml) => {
            if let Some(m) = ml.text.as_mut() {
                let enc = enc_of(&m.style);
                if enc.is_some() || has_inline_legacy(&m.contents) {
                    let new = convert_mtext(&m.contents, enc);
                    set(&mut m.contents, new);
                }
            }
        }
        EntityKind::Table(tb) => {
            for row in &mut tb.cells {
                for cell in row {
                    if has_inline_legacy(&cell.text) {
                        let new = convert_mtext(&cell.text, None);
                        set(&mut cell.text, new);
                    }
                }
            }
        }
        _ => {}
    }
    n
}

fn convert_store(store: &mut EntityStore, enc_of: &dyn Fn(&str) -> Option<Legacy>, only: Option<&[crate::Handle]>) -> usize {
    let handles: Vec<_> = store.iter().map(|e| e.handle).filter(|h| only.is_none_or(|o| o.contains(h))).collect();
    let mut total = 0;
    for h in handles {
        let mut n = 0;
        let mut probe = None;
        if let Some(e) = store.get(h) {
            let mut kind = e.kind.clone();
            n = convert_kind(&mut kind, enc_of);
            if n > 0 {
                probe = Some(kind);
            }
        }
        if let Some(kind) = probe {
            store.modify(h, |e| e.kind = kind);
            total += n;
        }
    }
    total
}

/// Convert every legacy-encoded text in the drawing (model, layouts and block definitions) to
/// Unicode and switch the legacy styles to Unicode fonts.
pub fn convert_drawing(d: &mut Drawing) -> VnReport {
    let styles = legacy_styles(d);
    let by_style = |st: &str| style_enc(&styles, st);
    let mut report = VnReport::default();
    report.texts += convert_store(&mut d.model, &by_style, None);
    for l in &mut d.layouts {
        report.texts += convert_store(&mut l.entities, &by_style, None);
    }
    let names: Vec<String> = d.blocks.keys().cloned().collect();
    for name in names {
        if let Some(b) = d.blocks.get_mut(&name) {
            let mut store = b.entities.clone();
            let n = convert_store(&mut store, &by_style, None);
            if n > 0 {
                Arc::make_mut(b).entities = store;
                report.texts += n;
            }
        }
    }
    for s in &mut d.text_styles {
        let font_legacy = legacy_of_font(&s.font).is_some();
        let big_legacy = legacy_of_font(&s.big_font).is_some();
        if font_legacy || big_legacy {
            let old = if font_legacy { s.font.clone() } else { s.big_font.clone() };
            let new = unicode_font_for(&old).to_string();
            if font_legacy {
                s.font = new.clone();
            }
            if big_legacy {
                s.big_font.clear();
            }
            report.styles.push((s.name.clone(), old, new));
        }
    }
    report
}

/// Convert the given entities (in model space or any layout) as `enc`, whatever their style
/// says: for drawings whose legacy font has an unusual name. Returns the number of texts changed.
pub fn convert_entities(d: &mut Drawing, handles: &[crate::Handle], enc: Legacy) -> usize {
    let force = move |_: &str| Some(enc);
    let mut n = convert_store(&mut d.model, &force, Some(handles));
    for l in &mut d.layouts {
        n += convert_store(&mut l.entities, &force, Some(handles));
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_fonts() {
        assert_eq!(legacy_of_font(".VnTime"), Some(Legacy::Tcvn3 { upper: false }));
        assert_eq!(legacy_of_font("VNTIMEH.TTF"), Some(Legacy::Tcvn3 { upper: true }));
        assert_eq!(legacy_of_font("vnarial.ttf"), Some(Legacy::Tcvn3 { upper: false }));
        assert_eq!(legacy_of_font("VNI-Times"), Some(Legacy::Vni));
        assert_eq!(legacy_of_font("VNI-HELVE.TTF"), Some(Legacy::Vni));
        assert_eq!(legacy_of_font("arial.ttf"), None);
        assert_eq!(legacy_of_font("romans.shx"), None);
        assert_eq!(unicode_font_for(".VnTime"), "Times New Roman");
        assert_eq!(unicode_font_for("VNI-Helve"), "Arial");
    }

    fn latin1(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| char::from(b)).collect()
    }

    #[test]
    fn tcvn3_common_words() {
        // Bytes as a TCVN3 drawing stores them.
        assert_eq!(tcvn3_to_unicode(&latin1(b"C\xC7u B\xB9ch \xA7\xBBng"), false), "Cầu Bạch Đằng");
        assert_eq!(tcvn3_to_unicode(&latin1(b"M\xC6t c\xBEt ngang"), false), "Mặt cắt ngang");
        assert_eq!(tcvn3_to_unicode(&latin1(b"Tr\xBEc d\xE4c tuy\xD5n"), false), "Trắc dọc tuyến");
        assert_eq!(tcvn3_to_unicode(&latin1(b"\xAE\xAD\xEAng"), false), "đường");
        assert_eq!(tcvn3_to_unicode(&latin1(b"H\xB5 N\xE9i - \xA7\xB5 N\xBDng"), false), "Hà Nội - Đà Nẵng");
        assert_eq!(tcvn3_to_unicode(&latin1(b"K\xFC thu\xCBt b\xD7nh \xAE\xE5"), false), "Kỹ thuật bình đồ");
        assert_eq!(tcvn3_to_unicode(&latin1(b"c\xC7u"), true), "CẦU");
    }

    #[test]
    fn tcvn3_full_table_is_unique() {
        let all: Vec<(u8, char)> = TCVN3.iter().chain(TCVN3_TAIL.iter()).copied().collect();
        assert_eq!(all.len(), 74);
        let mut bytes: Vec<u8> = all.iter().map(|x| x.0).collect();
        bytes.sort_unstable();
        bytes.dedup();
        assert_eq!(bytes.len(), 74);
        let mut chars: Vec<char> = all.iter().map(|x| x.1).collect();
        chars.sort_unstable();
        chars.dedup();
        assert_eq!(chars.len(), 74);
    }

    #[test]
    fn vni_common_words() {
        assert_eq!(vni_to_unicode(&latin1(b"Ca\xE0u Ba\xEFch \xD1a\xE8ng")), "Cầu Bạch Đằng");
        assert_eq!(vni_to_unicode(&latin1(b"Vie\xE4t Nam")), "Việt Nam");
        assert_eq!(vni_to_unicode(&latin1(b"Ha\xF8 No\xE4i")), "Hà Nội");
        assert_eq!(vni_to_unicode(&latin1(b"\xF1\xF6\xF4\xF8ng")), "đường");
        assert_eq!(vni_to_unicode(&latin1(b"\xD1a\xF8 Na\xFCng")), "Đà Nẵng");
        assert_eq!(vni_to_unicode(&latin1(b"Ky\xF5 thua\xE4t")), "Kỹ thuật");
        assert_eq!(vni_to_unicode(&latin1(b"CA\xC0U")), "CẦU");
        assert_eq!(vni_to_unicode(&latin1(b"b\xF2 m\xF3")), "bị mĩ");
    }

    #[test]
    fn unicode_is_left_alone() {
        assert_eq!(to_unicode("Cầu Bạch Đằng", Legacy::Tcvn3 { upper: false }), "Cầu Bạch Đằng");
        assert_eq!(to_unicode("Đường", Legacy::Vni), "Đường");
        assert_eq!(to_unicode("cầu", Legacy::Tcvn3 { upper: true }), "CẦU");
    }

    #[test]
    fn mtext_runs_and_inline_fonts() {
        let s = format!("{{\\f.VnTime|b0|i0|c0|p18;C{}u}} \\H2.5;abc{{\\fArial|b1;C{}u}}\\P", '\u{C7}', '\u{C7}');
        let out = convert_mtext(&s, None);
        assert_eq!(out, "{\\fTimes New Roman|b0|i0|c0|p18;Cầu} \\H2.5;abc{\\fArial|b1;CÇu}\\P");
        // Style default applies outside groups, and \S stacks convert too.
        let out = convert_mtext(&format!("C{}u \\S1^2;", '\u{C7}'), Some(Legacy::Tcvn3 { upper: false }));
        assert_eq!(out, "Cầu \\S1^2;");
    }

    #[test]
    fn drawing_conversion() {
        let mut d = Drawing::default();
        d.text_styles.push(crate::TextStyle { name: "VN".into(), font: "VNTIME.TTF".into(), ..crate::TextStyle::default() });
        let t = crate::Text {
            insert: crate::geom::Vec3::new(0.0, 0.0, 0.0),
            align_pt: None,
            height: 2.5,
            value: latin1(b"C\xC7u"),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "VN".into(),
            halign: crate::HAlign::default(),
            valign: crate::VAlign::default(),
        };
        let h = d.add(&crate::Space::Model, crate::Common::default(), EntityKind::Text(t)).unwrap();
        let r = convert_drawing(&mut d);
        assert_eq!(r.texts, 1);
        assert_eq!(r.styles.len(), 1);
        let Some(EntityKind::Text(t)) = d.model.get(h).map(|e| e.kind.clone()) else { panic!("text") };
        assert_eq!(t.value, "Cầu");
        assert_eq!(d.text_styles.iter().find(|s| s.name == "VN").map(|s| s.font.as_str()), Some("Times New Roman"));
        // Running it again changes nothing.
        let again = convert_drawing(&mut d);
        assert_eq!(again.texts, 0);
    }
}
