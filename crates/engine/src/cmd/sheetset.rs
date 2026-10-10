//! VNCCad: Sheet Set Manager — a drawing set (bộ bản vẽ) made of layouts from several
//! drawings: numbering, titles, opening a sheet, filling title blocks, a sheet list table and
//! one PDF of the whole set. Saved as a `.vnss` file (JSON).

use std::sync::Arc;

use cadcraft_doc::{EntityKind, Space};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sheet {
    pub number: String,
    pub title: String,
    /// The drawing: its path when saved, else its title.
    pub file: String,
    /// Layout name ("Model" for model space).
    pub layout: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SheetSet {
    pub name: String,
    pub project_name: String,
    pub project_number: String,
    pub sheets: Vec<Sheet>,
    /// Where the set was saved (desktop).
    #[serde(skip)]
    pub path: Option<String>,
}

pub fn specs() -> Vec<CommandSpec> {
    let c = |id: &'static str, label: &'static str, f: fn(&mut Session, &Value) -> Result<Value>, params: &'static str| {
        CommandSpec::new(id, label, f).params(params).noundo()
    };
    vec![
        CommandSpec::new("sheetset", "Sheet Set Manager", |s, _| run_list(s, &Value::Null))
            .menu(&["Tools", "Palettes", "Sheet Set Manager"])
            .alias(&["ssm", "bobanve"])
            .noundo(),
        c("sheetset.new", "New Sheet Set", run_new, "{name, projectName?, projectNumber?}"),
        c("sheetset.list", "List Sheets", run_list, "→ {name, sheets: [{number, title, file, layout, open}]}"),
        c("sheetset.add", "Add Sheets", run_add, "{all?: bool (every layout of the active drawing), layout?: name (default: current), number?, title?}"),
        c("sheetset.remove", "Remove Sheet", run_remove, "{index}"),
        c("sheetset.move", "Move Sheet", run_move, "{index, to}"),
        c("sheetset.set", "Edit Sheet", run_set, "{index?, number?, title?} | {name?, projectName?, projectNumber?} (the set)"),
        c("sheetset.renumber", "Renumber Sheets", run_renumber, "{start?: 1, prefix?: \"\", digits?: 2}"),
        c("sheetset.open", "Open Sheet", run_open, "{index}"),
        c("sheetset.save", "Save Sheet Set", run_save, "{path?} → data (JSON) when no path"),
        c("sheetset.load", "Open Sheet Set", run_load, "{path | data (JSON or base64)}"),
        CommandSpec::new("sheetset.titleblocks", "Update Title Blocks", run_titleblocks).params(
            "{} → fills title block attributes on every sheet: SO_TO/SHEETNO, TEN_BAN_VE/SHEETTITLE, TONG_SO_TO/TOTAL, DU_AN/PROJECT, MA_DU_AN/PROJECTNO",
        ),
        CommandSpec::new("sheetset.table", "Sheet List Table", run_table)
            .params("{at: [x,y], title?}")
            .interactive(|_| {
                Ok(Box::new(super::flow::Flow::new(
                    "sheetset.table",
                    |_, a| if a.is_empty() { Some(super::flow::Ask::Point("Điểm đặt danh mục bản vẽ".into())) } else { None },
                    |s, a| match a.first() {
                        Some(super::flow::Ans::Point(p)) => run_table(s, &json!({ "at": [p.x, p.y] })),
                        _ => Ok(Value::Null),
                    },
                )))
            })
            .enabled(has_doc),
        c("sheetset.publish", "Publish Sheet Set", run_publish, "{path?} → one PDF of every sheet (data when no path)"),
    ]
}

fn set(s: &Session) -> Result<&SheetSet> {
    s.sheet_set.as_ref().ok_or_else(|| bad("sheetset", "chưa có bộ bản vẽ (SHEETSET → Mới hoặc Mở)"))
}

fn set_mut(s: &mut Session) -> Result<&mut SheetSet> {
    s.sheet_set.as_mut().ok_or_else(|| bad("sheetset", "chưa có bộ bản vẽ (SHEETSET → Mới hoặc Mở)"))
}

fn base_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// The open drawing a sheet belongs to.
pub fn find_doc(s: &Session, sh: &Sheet) -> Option<usize> {
    let f = sh.file.as_str();
    s.docs
        .iter()
        .position(|d| d.path.as_deref() == Some(f))
        .or_else(|| s.docs.iter().position(|d| d.title.eq_ignore_ascii_case(f)))
        .or_else(|| s.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| base_name(p).eq_ignore_ascii_case(base_name(f)))))
        .or_else(|| s.docs.iter().position(|d| d.title.eq_ignore_ascii_case(base_name(f))))
}

fn space_of(layout: &str) -> Space {
    if layout.eq_ignore_ascii_case("model") { Space::Model } else { Space::Paper(layout.to_string()) }
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Bộ bản vẽ").to_string();
    s.sheet_set = Some(SheetSet {
        name,
        project_name: str_param(p, "projectName").unwrap_or("").to_string(),
        project_number: str_param(p, "projectNumber").unwrap_or("").to_string(),
        ..Default::default()
    });
    run_list(s, &Value::Null)
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let Some(ss) = s.sheet_set.as_ref() else { return Ok(json!({ "name": null, "sheets": [], "message": "Chưa có bộ bản vẽ." })) };
    let sheets: Vec<Value> = ss
        .sheets
        .iter()
        .map(|sh| json!({ "number": sh.number, "title": sh.title, "file": sh.file, "layout": sh.layout, "open": find_doc(s, sh).is_some() }))
        .collect();
    Ok(json!({ "name": ss.name, "projectName": ss.project_name, "projectNumber": ss.project_number, "path": ss.path, "sheets": sheets }))
}

fn run_add(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.state()?;
    let file = st.path.clone().unwrap_or_else(|| st.title.clone());
    let layouts: Vec<String> = if bool_or(p, "all", false) {
        let mut ls: Vec<_> = st.doc.layouts.iter().map(|l| (l.tab_order, l.name.clone())).collect();
        ls.sort();
        ls.into_iter().map(|(_, n)| n).collect()
    } else {
        vec![match str_param(p, "layout") {
            Some(l) => l.to_string(),
            None => match &st.space {
                Space::Model => "Model".to_string(),
                Space::Paper(n) => n.clone(),
            },
        }]
    };
    let ss = set_mut(s)?;
    let mut added = 0;
    for l in layouts {
        if ss.sheets.iter().any(|x| x.file == file && x.layout.eq_ignore_ascii_case(&l)) {
            continue;
        }
        let n = ss.sheets.len() + 1;
        ss.sheets.push(Sheet {
            number: str_param(p, "number").map(str::to_string).unwrap_or_else(|| format!("{n:02}")),
            title: str_param(p, "title").map(str::to_string).unwrap_or_else(|| l.clone()),
            file: file.clone(),
            layout: l,
        });
        added += 1;
    }
    let mut r = run_list(s, &Value::Null)?;
    r["message"] = json!(format!("Đã thêm {added} tờ."));
    Ok(r)
}

fn index(p: &Value, n: usize, key: &str) -> Result<usize> {
    let i = p.get(key).and_then(Value::as_u64).ok_or_else(|| bad("sheetset", format!("`{key}` is required")))? as usize;
    if i >= n {
        return Err(bad("sheetset", "không có tờ đó"));
    }
    Ok(i)
}

fn run_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let ss = set_mut(s)?;
    let i = index(p, ss.sheets.len(), "index")?;
    ss.sheets.remove(i);
    run_list(s, &Value::Null)
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    let ss = set_mut(s)?;
    let n = ss.sheets.len();
    let i = index(p, n, "index")?;
    let to = (p.get("to").and_then(Value::as_u64).unwrap_or(0) as usize).min(n.saturating_sub(1));
    let sh = ss.sheets.remove(i);
    ss.sheets.insert(to, sh);
    run_list(s, &Value::Null)
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let ss = set_mut(s)?;
    if p.get("index").is_some() {
        let i = index(p, ss.sheets.len(), "index")?;
        if let Some(sh) = ss.sheets.get_mut(i) {
            if let Some(n) = str_param(p, "number") {
                sh.number = n.to_string();
            }
            if let Some(t) = str_param(p, "title") {
                sh.title = t.to_string();
            }
        }
    } else {
        if let Some(n) = str_param(p, "name") {
            ss.name = n.to_string();
        }
        if let Some(n) = str_param(p, "projectName") {
            ss.project_name = n.to_string();
        }
        if let Some(n) = str_param(p, "projectNumber") {
            ss.project_number = n.to_string();
        }
    }
    run_list(s, &Value::Null)
}

fn run_renumber(s: &mut Session, p: &Value) -> Result<Value> {
    let start = p.get("start").and_then(Value::as_i64).unwrap_or(1);
    let prefix = str_param(p, "prefix").unwrap_or("").to_string();
    let digits = p.get("digits").and_then(Value::as_u64).unwrap_or(2).min(6) as usize;
    let ss = set_mut(s)?;
    for (k, sh) in ss.sheets.iter_mut().enumerate() {
        sh.number = format!("{prefix}{:0digits$}", start + k as i64);
    }
    run_list(s, &Value::Null)
}

fn run_open(s: &mut Session, p: &Value) -> Result<Value> {
    let sh = {
        let ss = set(s)?;
        let i = index(p, ss.sheets.len(), "index")?;
        ss.sheets.get(i).cloned().unwrap_or_default()
    };
    let i = match find_doc(s, &sh) {
        Some(i) => i,
        None => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                s.execute("open", &json!({ "path": sh.file }))?;
                find_doc(s, &sh).unwrap_or(s.active)
            }
            #[cfg(target_arch = "wasm32")]
            return Err(bad("sheetset.open", format!("hãy mở {} trước", base_name(&sh.file))));
        }
    };
    s.cancel();
    s.active = i;
    s.execute("layout.set", &json!({ "name": sh.layout }))?;
    Ok(json!({ "message": format!("Tờ {} – {}", sh.number, sh.title) }))
}

fn run_save(s: &mut Session, p: &Value) -> Result<Value> {
    let ss = set(s)?.clone();
    let text = serde_json::to_string_pretty(&ss).map_err(|e| bad("sheetset.save", e.to_string()))?;
    match str_param(p, "path").map(str::to_string).or(ss.path.clone()) {
        Some(_path) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let path = if _path.to_ascii_lowercase().ends_with(".vnss") { _path } else { format!("{_path}.vnss") };
                std::fs::write(&path, &text).map_err(|e| bad("sheetset.save", format!("{path}: {e}")))?;
                if let Some(x) = s.sheet_set.as_mut() {
                    x.path = Some(path.clone());
                }
                Ok(json!({ "path": path, "message": format!("Đã lưu bộ bản vẽ: {path}") }))
            }
            #[cfg(target_arch = "wasm32")]
            Ok(json!({ "data": text, "name": format!("{}.vnss", ss.name) }))
        }
        None => Ok(json!({ "data": text, "name": format!("{}.vnss", ss.name) })),
    }
}

/// Read a sheet set from its JSON text.
pub fn load_text(s: &mut Session, text: &str, path: Option<String>) -> Result<Value> {
    let mut ss: SheetSet = serde_json::from_str(text).map_err(|e| bad("sheetset.load", format!("tệp bộ bản vẽ hỏng: {e}")))?;
    ss.sheets.truncate(5000);
    ss.path = path;
    let n = ss.sheets.len();
    let name = ss.name.clone();
    s.sheet_set = Some(ss);
    let mut r = run_list(s, &Value::Null)?;
    r["message"] = json!(format!("Đã mở bộ bản vẽ {name} ({n} tờ)."));
    Ok(r)
}

fn run_load(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(d) = str_param(p, "data") {
        let text = if d.trim_start().starts_with('{') {
            d.to_string()
        } else {
            String::from_utf8_lossy(&super::file::base64_decode(d).ok_or_else(|| bad("sheetset.load", "invalid data"))?).to_string()
        };
        return load_text(s, &text, None);
    }
    let path = str_param(p, "path").ok_or_else(|| bad("sheetset.load", "`path` or `data` is required"))?.to_string();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let text = std::fs::read_to_string(&path).map_err(|e| bad("sheetset.load", format!("{path}: {e}")))?;
        load_text(s, &text, Some(path))
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("sheetset.load", format!("không đọc được {path} trên web")))
}

const TAG_NO: &[&str] = &["SO_TO", "SOTO", "SHEETNO", "SHEET_NO", "SHEETNUMBER", "SHEET_NUMBER", "SO_BV", "SOHIEU", "SO_HIEU"];
const TAG_TITLE: &[&str] = &["TEN_BAN_VE", "TENBANVE", "TEN_BV", "SHEETTITLE", "SHEET_TITLE", "TITLE", "TEN"];
const TAG_TOTAL: &[&str] = &["TONG_SO_TO", "TONGSOTO", "TONG_TO", "TOTAL", "SHEETS", "TOTAL_SHEETS"];
const TAG_PROJECT: &[&str] = &["DU_AN", "DUAN", "TEN_DU_AN", "TENDUAN", "CONG_TRINH", "CONGTRINH", "PROJECT", "PROJECTNAME"];
const TAG_PROJECTNO: &[&str] = &["MA_DU_AN", "MADUAN", "SO_HIEU_DA", "PROJECTNO", "PROJECT_NO", "PROJECTNUMBER"];

fn run_titleblocks(s: &mut Session, _p: &Value) -> Result<Value> {
    let ss = set(s)?.clone();
    let total = ss.sheets.len().to_string();
    let mut changed = 0;
    let mut missing = 0;
    for sh in &ss.sheets {
        let Some(i) = find_doc(s, sh) else {
            missing += 1;
            continue;
        };
        let space = space_of(&sh.layout);
        let Some(st) = s.docs.get_mut(i) else { continue };
        let value_for = |tag: &str| -> Option<String> {
            let t = tag.to_ascii_uppercase();
            let t = t.as_str();
            if TAG_NO.contains(&t) {
                Some(sh.number.clone())
            } else if TAG_TITLE.contains(&t) {
                Some(sh.title.clone())
            } else if TAG_TOTAL.contains(&t) {
                Some(total.clone())
            } else if TAG_PROJECT.contains(&t) && !ss.project_name.is_empty() {
                Some(ss.project_name.clone())
            } else if TAG_PROJECTNO.contains(&t) && !ss.project_number.is_empty() {
                Some(ss.project_number.clone())
            } else {
                None
            }
        };
        let targets: Vec<cadcraft_doc::Handle> = st
            .doc
            .space(&space)
            .map(|store| {
                store
                    .iter()
                    .filter(|e| matches!(&e.kind, EntityKind::Insert(ins) if ins.attribs.iter().any(|a| value_for(&a.tag).is_some_and(|v| v != a.text.value))))
                    .map(|e| e.handle)
                    .collect()
            })
            .unwrap_or_default();
        if targets.is_empty() {
            continue;
        }
        let doc = Arc::make_mut(&mut st.doc);
        for h in targets {
            let _ = doc.modify_entity(h, |e| {
                if let EntityKind::Insert(ins) = &mut e.kind {
                    for a in &mut ins.attribs {
                        if let Some(v) = value_for(&a.tag) {
                            a.text.value = v;
                        }
                    }
                }
            });
            changed += 1;
        }
        st.revision += 1;
    }
    let mut msg = format!("Đã cập nhật {changed} khung tên.");
    if missing > 0 {
        msg.push_str(&format!(" {missing} tờ chưa mở bản vẽ nên chưa cập nhật."));
    }
    Ok(json!({ "updated": changed, "missing": missing, "message": msg }))
}

fn run_table(s: &mut Session, p: &Value) -> Result<Value> {
    let ss = set(s)?.clone();
    let at = point_req("sheetset.table", p, "at")?;
    let cells: Vec<Vec<String>> = ss.sheets.iter().map(|sh| vec![sh.number.clone(), sh.title.clone()]).collect();
    if cells.is_empty() {
        return Err(bad("sheetset.table", "bộ bản vẽ chưa có tờ nào"));
    }
    let title = str_param(p, "title").map(str::to_string).unwrap_or_else(|| format!("DANH MỤC BẢN VẼ – {}", ss.name));
    s.execute("table", &json!({ "at": [at.x, at.y], "cells": cells, "title": title, "header": ["SỐ TỜ", "TÊN BẢN VẼ"] }))
}

fn run_publish(s: &mut Session, p: &Value) -> Result<Value> {
    let ss = set(s)?.clone();
    let mut pages: Vec<(Arc<cadcraft_doc::Drawing>, Space, cadcraft_io::PdfOptions)> = Vec::new();
    let mut missing = Vec::new();
    for sh in &ss.sheets {
        match find_doc(s, sh).and_then(|i| s.docs.get(i)) {
            Some(st) => {
                let space = space_of(&sh.layout);
                if st.doc.space(&space).is_none() {
                    missing.push(format!("{} ({})", sh.number, sh.layout));
                    continue;
                }
                let o = cadcraft_io::PdfOptions { title: format!("{} {}", sh.number, sh.title), ..cadcraft_io::PdfOptions::from_json(p) };
                pages.push((st.doc.clone(), space, o));
            }
            None => missing.push(format!("{} ({})", sh.number, base_name(&sh.file))),
        }
    }
    let list: Vec<(&cadcraft_doc::Drawing, Space, cadcraft_io::PdfOptions)> =
        pages.iter().map(|(d, sp, o)| (d.as_ref(), sp.clone(), o.clone())).collect();
    let bytes = cadcraft_io::pdf::publish_multi(&list, true, &ss.name).map_err(|e| bad("sheetset.publish", e.to_string()))?;
    let mut msg = format!("Đã in {} tờ của bộ {} ra PDF.", list.len(), ss.name);
    if !missing.is_empty() {
        msg.push_str(&format!(" Bỏ qua (chưa mở): {}", missing.join(", ")));
    }
    match str_param(p, "path") {
        Some(_path) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                std::fs::write(_path, &bytes).map_err(|e| bad("sheetset.publish", format!("{_path}: {e}")))?;
                Ok(json!({ "path": _path, "pages": list.len(), "message": msg }))
            }
            #[cfg(target_arch = "wasm32")]
            Ok(json!({ "data": super::file::base64_encode(&bytes), "pages": list.len(), "message": msg }))
        }
        None => Ok(json!({ "data": super::file::base64_encode(&bytes), "pages": list.len(), "message": msg })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_set_across_two_drawings() {
        let mut s = Session::new();
        // A title block with attributes, in Layout1 of the first drawing.
        s.execute("attdef", &json!({ "tag": "SO_TO", "at": [0, 0], "height": 2.5, "default": "?" })).unwrap();
        s.execute("attdef", &json!({ "tag": "TEN_BAN_VE", "at": [0, 5], "height": 2.5, "default": "?" })).unwrap();
        s.execute("attdef", &json!({ "tag": "TONG_SO_TO", "at": [0, 10], "height": 2.5, "default": "?" })).unwrap();
        let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
        s.execute("block", &json!({ "name": "KHUNG_TEN", "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
        s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
        s.execute("insert", &json!({ "name": "KHUNG_TEN", "at": [100, 10] })).unwrap();
        s.execute("sheetset.new", &json!({ "name": "Cầu Rồng", "projectName": "Cầu Rồng" })).unwrap();
        s.execute("sheetset.add", &json!({ "all": true })).unwrap();
        s.execute("new", &json!({})).unwrap();
        s.execute("sheetset.add", &json!({ "layout": "Model", "title": "Mặt bằng tổng thể" })).unwrap();
        let l = s.execute("sheetset.list", &json!({})).unwrap();
        assert_eq!(l["sheets"].as_array().unwrap().len(), 3);
        s.execute("sheetset.move", &json!({ "index": 2, "to": 0 })).unwrap();
        s.execute("sheetset.renumber", &json!({ "prefix": "KC-", "digits": 2 })).unwrap();
        s.execute("sheetset.set", &json!({ "index": 1, "title": "Bố trí chung" })).unwrap();
        let l = s.execute("sheetset.list", &json!({})).unwrap();
        assert_eq!(l["sheets"][0]["title"], "Mặt bằng tổng thể");
        assert_eq!(l["sheets"][1]["number"], "KC-02");
        // Title blocks follow the set.
        let r = s.execute("sheetset.titleblocks", &json!({})).unwrap();
        assert_eq!(r["updated"], 1);
        let d = s.docs[0].doc.clone();
        let ins = d
            .layouts
            .iter()
            .flat_map(|l| l.entities.iter())
            .find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.clone()) } else { None })
            .unwrap();
        let val = |t: &str| ins.attribs.iter().find(|a| a.tag == t).unwrap().text.value.clone();
        assert_eq!((val("SO_TO"), val("TEN_BAN_VE"), val("TONG_SO_TO")), ("KC-02".to_string(), "Bố trí chung".to_string(), "3".to_string()));
        // Open a sheet: switches drawing and layout.
        s.execute("sheetset.open", &json!({ "index": 1 })).unwrap();
        assert_eq!(s.active, 0);
        assert_eq!(s.state().unwrap().space, Space::Paper("Layout1".into()));
        // List table, PDF, save and reload.
        s.execute("sheetset.table", &json!({ "at": [0, 0] })).unwrap();
        let r = s.execute("sheetset.publish", &json!({})).unwrap();
        assert_eq!(r["pages"], 3);
        let data = s.execute("sheetset.save", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
        s.sheet_set = None;
        s.execute("sheetset.load", &json!({ "data": data })).unwrap();
        assert_eq!(s.sheet_set.as_ref().unwrap().sheets.len(), 3);
    }
}
