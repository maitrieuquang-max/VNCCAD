//! VNCCad raster images: IMAGEATTACH places a PNG/JPEG/BMP/TIFF image as an IMAGE entity
//! (at its world-file coordinates when one is found beside it), IMAGE lists the images and
//! whether their files are found.

use cadcraft_doc::{Common, EntityKind, Image};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::raster;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("imageattach", "Raster Image Reference...", run_imageattach)
            .menu(&["Insert", "Raster Image Reference..."])
            .alias(&["iat", "chenanh"])
            .params("{path | name (a file already given to the app), insert?: [x,y] lower-left corner, width?: drawing units (default: fits the view), rotation?: degrees, world?: bool (use a world file .jgw/.pgw/.tfw/.wld when found, default true)}"),
        CommandSpec::new("pdfattach", "PDF Underlay...", run_pdfattach)
            .menu(&["Insert", "PDF Underlay..."])
            .alias(&["pdfa", "chenpdf"])
            .params("{path | name, page?: 1, insert?: [x,y] lower-left corner, scale?: drawing units per inch of the page (default: fits the view), rotation?: degrees}"),
        CommandSpec::new("image", "Image Manager", run_image_list)
            .menu(&["Insert", "Image Manager"])
            .alias(&["im", "imagemanager"])
            .params("{} → the drawing's images and whether each file is found")
            .noundo(),
    ]
}

pub(crate) fn run_imageattach(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").or_else(|| str_param(p, "name")).ok_or_else(|| bad("imageattach", "`path` or `name` is required"))?.to_string();
    let bytes = raster::bytes(&path).ok_or_else(|| bad("imageattach", format!("không tìm thấy ảnh {path}")))?;
    let (w, h) = raster::dimensions(&bytes).ok_or_else(|| bad("imageattach", format!("{path}: không đọc được ảnh (PNG, JPEG, BMP, TIFF)")))?;
    let (wf, hf) = (f64::from(w), f64::from(h));
    let use_world = p.get("world").and_then(Value::as_bool).unwrap_or(true);
    let (insert, u, v, how) = match raster::world_file(&path).filter(|_| use_world) {
        Some(wld) => {
            let (ll, u, v) = raster::placement_from_world(&wld, h);
            (Vec2::new(ll[0], ll[1]), Vec2::new(u[0], u[1]), Vec2::new(v[0], v[1]), "theo world file")
        }
        None => {
            let view = s.state().map(|st| st.view()).ok();
            let width = p.get("width").and_then(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0).unwrap_or_else(|| match view {
                Some(vw) => (vw.height * 0.6 * wf / hf).max(1e-9),
                None => wf,
            });
            let px = width / wf;
            let rot = p.get("rotation").and_then(Value::as_f64).unwrap_or(0.0).to_radians();
            let u = Vec2::new(rot.cos(), rot.sin()) * px;
            let v = Vec2::new(-rot.sin(), rot.cos()) * px;
            let insert = match p.get("insert").and_then(Value::as_array) {
                Some(a) => Vec2::new(a.first().and_then(Value::as_f64).unwrap_or(0.0), a.get(1).and_then(Value::as_f64).unwrap_or(0.0)),
                None => match view {
                    Some(vw) => vw.center - (u * wf + v * hf) * 0.5,
                    None => Vec2::ZERO,
                },
            };
            (insert, u, v, "")
        }
    };
    let image = Image {
        insert: Vec3::new(insert.x, insert.y, 0.0),
        u: Vec3::new(u.x, u.y, 0.0),
        v: Vec3::new(v.x, v.y, 0.0),
        size: Vec2::new(wf, hf),
        path: path.clone(),
    };
    let space = s.space();
    let layer = s.doc()?.header.str("CLAYER", "0");
    let common = Common { layer, ..Common::default() };
    let handle = s.doc_mut()?.add(&space, common, EntityKind::Image(image)).map_err(|e| bad("imageattach", e.to_string()))?;
    let name = raster::file_key(&path);
    let msg =
        if how.is_empty() { format!("Đã chèn ảnh {name} ({w}×{h} px).") } else { format!("Đã chèn ảnh {name} ({w}×{h} px) {how}.") };
    s.echo(msg.clone());
    Ok(json!({ "handle": handle.hex(), "width": w, "height": h, "georeferenced": !how.is_empty(), "message": msg }))
}

pub(crate) fn run_pdfattach(s: &mut Session, p: &Value) -> Result<Value> {
    let file = str_param(p, "path").or_else(|| str_param(p, "name")).ok_or_else(|| bad("pdfattach", "`path` or `name` is required"))?.to_string();
    let page = p.get("page").and_then(Value::as_u64).unwrap_or(1).clamp(1, 100_000) as usize;
    let bytes = raster::bytes(&file).ok_or_else(|| bad("pdfattach", format!("không tìm thấy file {file}")))?;
    let pages = raster::pdf_page_count(&bytes);
    if pages == 0 {
        return Err(bad("pdfattach", format!("{file}: không đọc được PDF (file mã hóa hoặc hỏng)")));
    }
    let (wpt, hpt) = raster::pdf_page_size(&bytes, page).ok_or_else(|| bad("pdfattach", format!("{file} chỉ có {pages} trang")))?;
    let (wpx, hpx) = ((wpt * raster::PDF_DPI / 72.0).round(), (hpt * raster::PDF_DPI / 72.0).round());
    let view = s.state().map(|st| st.view()).ok();
    // Drawing units per inch of the page.
    let scale = p.get("scale").and_then(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0).unwrap_or_else(|| match view {
        Some(vw) => vw.height * 0.8 / (hpt / 72.0),
        None => 1.0,
    });
    let px = scale / raster::PDF_DPI;
    let rot = p.get("rotation").and_then(Value::as_f64).unwrap_or(0.0).to_radians();
    let u = Vec2::new(rot.cos(), rot.sin()) * px;
    let v = Vec2::new(-rot.sin(), rot.cos()) * px;
    let insert = match p.get("insert").and_then(Value::as_array) {
        Some(a) => Vec2::new(a.first().and_then(Value::as_f64).unwrap_or(0.0), a.get(1).and_then(Value::as_f64).unwrap_or(0.0)),
        None => match view {
            Some(vw) => vw.center - (u * wpx + v * hpx) * 0.5,
            None => Vec2::ZERO,
        },
    };
    let path = if page == 1 { file.clone() } else { format!("{file}#{page}") };
    let image = Image {
        insert: Vec3::new(insert.x, insert.y, 0.0),
        u: Vec3::new(u.x, u.y, 0.0),
        v: Vec3::new(v.x, v.y, 0.0),
        size: Vec2::new(wpx, hpx),
        path,
    };
    let space = s.space();
    let layer = s.doc()?.header.str("CLAYER", "0");
    let handle =
        s.doc_mut()?.add(&space, Common { layer, ..Common::default() }, EntityKind::Image(image)).map_err(|e| bad("pdfattach", e.to_string()))?;
    let msg = format!("Đã chèn trang {page}/{pages} của {} làm nền.", raster::file_key(&file));
    s.echo(msg.clone());
    Ok(json!({ "handle": handle.hex(), "page": page, "pages": pages, "message": msg }))
}

fn run_image_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let mut out: Vec<Value> = Vec::new();
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities));
    for st in stores {
        for e in st.iter() {
            if let EntityKind::Image(i) = &e.kind {
                out.push(
                    json!({ "handle": e.handle.hex(), "path": i.path, "found": raster::bytes(&i.path).is_some(), "pixels": [i.size.x, i.size.y] }),
                );
            }
        }
    }
    let missing = out.iter().filter(|v| v["found"] == false).count();
    let msg = if out.is_empty() {
        "Bản vẽ không có ảnh nền.".to_string()
    } else if missing == 0 {
        format!("{} ảnh, đã tìm thấy đủ file.", out.len())
    } else {
        format!("{} ảnh, thiếu {missing} file. Đặt file ảnh cạnh bản vẽ hoặc kéo-thả file ảnh vào cửa sổ.", out.len())
    };
    s.echo(msg.clone());
    Ok(json!({ "images": out, "message": msg }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        // A minimal valid PNG made by the io crate's image support (via a tiny encoder below).
        let mut v = Vec::new();
        let img = vec![200u8; (w * h * 4) as usize];
        cadcraft_io::raster::encode_png_for_tests(&img, w, h, &mut v);
        v
    }

    #[test]
    fn attach_fits_view_and_by_world_file() {
        let mut s = Session::new();
        raster::register("nen-test.png", png(40, 20));
        let r = s.execute("imageattach", &json!({"name": "nen-test.png", "insert": [10, 20], "width": 80})).unwrap();
        assert_eq!(r["width"], 40);
        let img = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Image(i) = &e.kind { Some(i.clone()) } else { None }).unwrap();
        assert_eq!(img.insert, Vec3::new(10.0, 20.0, 0.0));
        assert!((img.u.x - 2.0).abs() < 1e-12 && (img.v.y - 2.0).abs() < 1e-12);
        raster::register("vt-test.png", png(10, 10));
        raster::register("vt-test.pgw", b"2\n0\n0\n-2\n501\n599\n".to_vec());
        let r = s.execute("imageattach", &json!({"name": "vt-test.png"})).unwrap();
        assert_eq!(r["georeferenced"], true);
        let list = s.execute("image", &json!({})).unwrap();
        assert_eq!(list["images"].as_array().map(Vec::len), Some(2));
        assert!(s.execute("imageattach", &json!({"name": "khong-co.png"})).is_err());
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.execute("image", &json!({})).unwrap()["images"].as_array().map(Vec::len), Some(1));
    }
}
