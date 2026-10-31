//! VNCCad: geographic coordinates — the drawing's coordinate system (GEOCRS: VN-2000 by province
//! or central meridian, or UTM), export to Google Earth (EXPORTKMZ / EXPORTKML) and import of
//! KML / KMZ shapes (IMPORTKML; also by opening or dropping a .kml / .kmz file).

use cadcraft_doc::{Common, EntityKind, Handle};
use cadcraft_geom::PolyVertex;
use cadcraft_geom::Vec2;
use cadcraft_io::kml::{Crs, KmlFilter, KmlItem};
use serde_json::{Value, json};

use super::flow::{Ans, Ask, Flow};
use super::helpers::{lwpoly, v3};
use super::*;
use crate::{Result, Session};

const CRS_KEY: &str = "VNC_CRS";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("geocrs", "Geographic Coordinate System", run_geocrs)
            .menu(&["Tools", "Geographic Location..."])
            .alias(&["hetoado", "geographiclocation"])
            .params(
                "{crs?: \"VN2000 107.75 3\" | \"VN2000 Đà Nẵng\" | \"UTM 48N\", unit?: metres per drawing unit (1 m, 0.001 mm)} → the current system",
            )
            .interactive(|_| Ok(Box::new(Flow::new("geocrs", plan_geocrs, flow_geocrs))))
            .enabled(has_doc),
        CommandSpec::new("exportkmz", "Export to Google Earth (KMZ)...", |s, p| run_export(s, p, true))
            .menu(&["File", "Export to Google Earth (KMZ)..."])
            .alias(&["xuatkmz", "kmz"])
            .params("{path? (else base64 `data`), crs?, unit?, handles? | layers?: [names], text?: bool (default true)}")
            .interactive(|_| Ok(Box::new(Flow::new("exportkmz", plan_export, flow_export))))
            .noundo()
            .enabled(has_doc),
        CommandSpec::new("exportkml", "Export to KML...", |s, p| run_export(s, p, false))
            .alias(&["xuatkml"])
            .params("{path? (else `data` base64), crs?, unit?, handles? | layers?, text?}")
            .noundo()
            .enabled(has_doc),
        CommandSpec::new("importkml", "Import KML / KMZ", run_import)
            .menu(&["Insert", "Google Earth KML / KMZ..."])
            .alias(&["nhapkml", "nhapkmz"])
            .params("{path? | data (base64), name?, crs?, unit?, textHeight?} (shapes go on layers named after the KML folders)")
            .enabled(has_doc),
    ]
}

/// The drawing's coordinate system: `crs` given, or the one saved in the drawing.
pub fn crs_of(s: &Session, p: &Value) -> Result<Crs> {
    let d = s.doc()?;
    let text = str_param(p, "crs").map(str::to_string).or_else(|| Some(d.header.str(CRS_KEY, "")).filter(|t| !t.is_empty()));
    let Some(text) = text else {
        return Err(bad("geocrs", "bản vẽ chưa có hệ tọa độ: gõ GEOCRS (vd VN2000 Đà Nẵng, VN2000 107.75 3, UTM 48N)"));
    };
    let mut c =
        Crs::parse(&text).ok_or_else(|| bad("geocrs", format!("không hiểu hệ tọa độ `{text}` (vd VN2000 107.75 3, VN2000 Hà Nội, UTM 48N)")))?;
    c.unit_m = p
        .get("unit")
        .and_then(Value::as_f64)
        .or_else(|| d.header.get("VNC_CRS_UNIT").and_then(|v| v.as_f64()))
        .filter(|u| u.is_finite() && *u > 0.0)
        .unwrap_or_else(|| cadcraft_io::kml::guess_unit(d));
    Ok(c)
}

fn run_geocrs(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(t) = str_param(p, "crs") {
        let c = Crs::parse(t).ok_or_else(|| bad("geocrs", format!("không hiểu hệ tọa độ `{t}`")))?;
        let d = s.doc_mut()?;
        d.header.set_str(CRS_KEY, &c.to_header());
        if let Some(u) = p.get("unit").and_then(Value::as_f64).filter(|u| u.is_finite() && *u > 0.0) {
            d.header.set_f64("VNC_CRS_UNIT", u);
        }
    }
    let c = crs_of(s, &json!({}))?;
    // Where the drawing lies, to check the choice at a glance.
    let d = s.doc()?;
    let ext = d.extents(&cadcraft_doc::Space::Model);
    // The view the drawing was saved with tells where its main part is better than extents
    // (stray objects far away).
    let at =
        d.header.point("VNC_ACTIVEVIEW").map(|p| Vec2::new(p.x, p.y)).unwrap_or(if ext.is_empty() { Vec2::ZERO } else { (ext.min + ext.max) * 0.5 });
    let (lat, lon) = c.to_wgs84(at);
    let unit = if (c.unit_m - 0.001).abs() < 1e-12 {
        "mm"
    } else if (c.unit_m - 1.0).abs() < 1e-12 {
        "m"
    } else {
        "?"
    };
    let msg = format!("Hệ tọa độ: {} (đơn vị bản vẽ: {unit}). Giữa bản vẽ ≈ {lat:.6}, {lon:.6} (WGS-84).", c.name);
    Ok(json!({ "crs": c.to_header(), "name": c.name, "unitM": c.unit_m, "center": [lat, lon], "message": msg }))
}

fn plan_geocrs(s: &Session, a: &[Ans]) -> Option<Ask> {
    a.is_empty().then(|| {
        let cur = s.doc().ok().map(|d| d.header.str(CRS_KEY, "")).filter(|t| !t.is_empty());
        Ask::Text {
            msg: "Hệ tọa độ (vd VN2000 Đà Nẵng, VN2000 107°45' 3, VN2000 105 6, UTM 48N)".into(),
            default: cur.or(Some("VN2000 Hà Nội".into())),
        }
    })
}

fn flow_geocrs(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let t = a.first().map(|x| x.text().to_string()).unwrap_or_default();
    run_geocrs(s, &json!({ "crs": t }))
}

fn file_title(s: &Session) -> String {
    let t = s.state().map(|st| st.title.clone()).unwrap_or_else(|_| "Drawing".into());
    t.rsplit_once('.').map_or(t.as_str(), |(a, _)| a).to_string()
}

fn run_export(s: &mut Session, p: &Value, kmz: bool) -> Result<Value> {
    let crs = crs_of(s, p)?;
    let handles: Vec<Handle> = match p.get("handles") {
        Some(_) => targets(s, p)?,
        None => Vec::new(),
    };
    let layers: Vec<String> =
        p.get("layers").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let filter = KmlFilter { handles, layers, text: bool_or(p, "text", true) };
    let title = file_title(s);
    let d = s.doc()?;
    let bytes =
        if kmz { cadcraft_io::kml::to_kmz(d, &crs, &filter, &title) } else { cadcraft_io::kml::to_kml(d, &crs, &filter, &title).into_bytes() };
    let name = format!("{title}.{}", if kmz { "kmz" } else { "kml" });
    let msg = format!("Đã xuất {name} ({} KB) theo {} — mở bằng Google Earth.", bytes.len().div_ceil(1024), crs.name);
    if let Some(_path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::write(_path, &bytes).map_err(|e| bad("exportkmz", format!("{_path}: {e}")))?;
            return Ok(json!({ "path": _path, "bytes": bytes.len(), "message": msg }));
        }
    }
    Ok(json!({ "name": name, "data": super::file::base64_encode(&bytes), "bytes": bytes.len(), "message": msg }))
}

fn plan_export(s: &Session, a: &[Ans]) -> Option<Ask> {
    let has_crs = s.doc().ok().is_some_and(|d| !d.header.str(CRS_KEY, "").is_empty());
    let off = usize::from(!has_crs);
    if !has_crs && a.is_empty() {
        return Some(Ask::Text {
            msg: "Bản vẽ chưa có hệ tọa độ. Hệ tọa độ (vd VN2000 Đà Nẵng, VN2000 107.75 3, UTM 48N)".into(), default: None
        });
    }
    match a.len() - off {
        0 => Some(Ask::Kw { msg: "Xuất".into(), kws: vec!["All", "Select"], default: Some("All") }),
        1 if a.get(off).is_some_and(|x| x.text().eq_ignore_ascii_case("Select")) => Some(Ask::Select("Chọn đối tượng xuất".into())),
        _ => None,
    }
}

fn flow_export(s: &mut Session, a: &[Ans]) -> Result<Value> {
    let has_crs = s.doc().ok().is_some_and(|d| !d.header.str(CRS_KEY, "").is_empty());
    let mut rest = a;
    if !has_crs {
        let t = rest.first().map(|x| x.text().to_string()).unwrap_or_default();
        run_geocrs(s, &json!({ "crs": t }))?;
        rest = rest.get(1..).unwrap_or(&[]);
    }
    let mut p = json!({});
    if let Some(sel) = rest.get(1)
        && let Some(o) = p.as_object_mut()
    {
        o.insert("handles".into(), json!(sel.sel().iter().map(|h| h.hex()).collect::<Vec<_>>()));
    }
    // Interactive: the UI saves the file (download on the web, a save dialog on the desktop).
    let r = run_export(s, &p, true)?;
    s.ui_requests.push(("ui.savebytes".into(), json!({ "name": r["name"], "data": r["data"], "message": r["message"] })));
    Ok(json!({ "bytes": r["bytes"] }))
}

/// Shapes of a KML / KMZ file into the drawing.
pub fn import_bytes(s: &mut Session, bytes: &[u8], p: &Value) -> Result<Value> {
    let crs = crs_of(s, p)?;
    let items = cadcraft_io::kml::read_kml_or_kmz(bytes).ok_or_else(|| bad("importkml", "không đọc được tệp KML/KMZ"))?;
    if items.is_empty() {
        return Err(bad("importkml", "tệp KML/KMZ không có đường, vùng hay điểm nào"));
    }
    let th = p.get("textHeight").and_then(Value::as_f64).filter(|h| h.is_finite() && *h > 0.0).unwrap_or(2.0 / crs.unit_m.max(1e-12));
    let space = s.space();
    let mut n = 0usize;
    let layer_name = |f: &str| {
        let clean: String = f.chars().map(|c| if "<>/\\\":;?*|=`,".contains(c) { '_' } else { c }).collect();
        let clean = clean.trim().to_string();
        if clean.is_empty() { "KML".to_string() } else { format!("KML-{clean}") }
    };
    let mut first: Option<Vec2> = None;
    for it in items.into_iter().take(500_000) {
        let (folder, kind, label): (String, Option<EntityKind>, Option<(Vec2, String)>) = match &it {
            KmlItem::Line { folder, name, pts, closed } => {
                let vs: Vec<PolyVertex> = pts.iter().map(|q| PolyVertex::new(crs.from_wgs84(q.y, q.x))).collect();
                first = first.or_else(|| vs.first().map(|v| v.p));
                let _ = name;
                (folder.clone(), Some(lwpoly(vs, *closed)), None)
            }
            KmlItem::Point { folder, name, at } => {
                let q = crs.from_wgs84(at.y, at.x);
                first = first.or(Some(q));
                (
                    folder.clone(),
                    Some(EntityKind::Point(cadcraft_doc::Point { p: v3(q), angle: 0.0 })),
                    (!name.trim().is_empty()).then(|| (q, name.clone())),
                )
            }
        };
        let layer = layer_name(&folder);
        let d = s.doc_mut()?;
        d.ensure_layer(&layer);
        let common = Common { layer: layer.clone(), ..Common::default() };
        if let Some(k) = kind {
            d.add(&space, common.clone(), k)?;
            n += 1;
        }
        if let Some((q, text)) = label {
            let k = super::draw::make_text(
                s,
                q + Vec2::new(th * 0.5, th * 0.5),
                &text,
                th,
                0.0,
                (cadcraft_doc::HAlign::Left, cadcraft_doc::VAlign::Baseline),
            );
            s.doc_mut()?.add(&space, common, k)?;
        }
    }
    let msg = format!("Đã nhập {n} đối tượng từ KML/KMZ theo {} (layer KML-…). Gõ Z E để xem.", crs.name);
    Ok(json!({ "count": n, "at": first.map(|q| [q.x, q.y]), "message": msg }))
}

fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    let bytes = if let Some(data) = str_param(p, "data") {
        super::file::base64_decode(data).ok_or_else(|| bad("importkml", "invalid base64"))?
    } else if let Some(path) = str_param(p, "path") {
        read_file(path)?
    } else {
        return Err(bad("importkml", "`path` or `data` is required (or drop a .kml/.kmz file on the window)"));
    };
    import_bytes(s, &bytes, p)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| bad("importkml", format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn read_file(_path: &str) -> Result<Vec<u8>> {
    Err(bad("importkml", "trên web hãy kéo-thả tệp .kml/.kmz vào cửa sổ"))
}
