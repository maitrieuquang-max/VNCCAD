//! VNCCad: the sample drawing of the engineers' test kit (packaging/testkit/mau).
//! Regenerate with `cargo test -p cadcraft-engine --lib make_testkit_drawing -- --ignored`.

use serde_json::{Value, json};

use crate::Session;

fn h(r: &Value) -> String {
    r["handle"].as_str().or(r["handles"][0].as_str()).unwrap_or_default().to_string()
}

#[test]
#[ignore]
fn make_testkit_drawing() {
    let mut s = Session::new();
    s.execute("new", &json!({ "template": "metric" })).unwrap();
    {
        let d = s.doc_mut().unwrap();
        *d = cadcraft_doc::Drawing::new_metric();
        d.header.set_i64("MEASUREMENT", 1);
        // Drawn 1:1 in mm, annotated for 1:50.
        d.header.set_f64("DIMSCALE", 50.0);
        d.header.set_f64("LTSCALE", 50.0);
        for st in &mut d.dim_styles {
            st.scale = 50.0;
        }
        for m in &mut d.mleader_styles {
            // Scaled by DIMSCALE when drawn.
            m.text_height = 2.5;
            m.arrow_size = 2.5;
            m.landing_gap = 1.2;
            m.dogleg = 6.0;
        }
        for t in &mut d.table_styles {
            t.text_height = 125.0;
            t.margin = 50.0;
        }
    }
    let ex = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"));
    for (n, c) in [("TIM_TUYEN", 1), ("KET_CAU", 7), ("TUONG", 2), ("COC", 3), ("KICH_THUOC", 4), ("HATCH", 8), ("CHU", 7), ("KHUNG", 7)] {
        ex(&mut s, "layer.new", json!({ "name": n, "color": c }));
    }
    let cur = |s: &mut Session, l: &str| {
        s.doc_mut().unwrap().header.set_str("CLAYER", l);
    };
    // 1. Mặt cắt ngang dầm I (mm), hatch bê tông, kích thước.
    cur(&mut s, "KET_CAU");
    let beam = [
        [0, 0],
        [600, 0],
        [600, 150],
        [350, 250],
        [350, 1450],
        [700, 1550],
        [700, 1800],
        [-700, 1800],
        [-700, 1550],
        [-350, 1450],
        [-350, 250],
        [-600, 150],
        [-600, 0],
    ];
    let r = ex(&mut s, "pline", json!({ "points": beam, "closed": true }));
    let beam_h = h(&r);
    cur(&mut s, "HATCH");
    ex(&mut s, "hatch", json!({ "handles": [beam_h], "pattern": "AR-CONC", "scale": 1.0 }));
    cur(&mut s, "KICH_THUOC");
    ex(&mut s, "dimlinear", json!({ "p1": [-600, 0], "p2": [600, 0], "at": [0, -300] }));
    ex(&mut s, "dimlinear", json!({ "p1": [700, 0], "p2": [700, 1800], "at": [1100, 900] }));
    ex(&mut s, "dimlinear", json!({ "p1": [-700, 1800], "p2": [700, 1800], "at": [0, 2100] }));
    cur(&mut s, "CHU");
    ex(&mut s, "text", json!({ "at": [-700, 2500], "text": "MẶT CẮT NGANG DẦM I (bê tông 40MPa)", "height": 120 }));
    ex(&mut s, "mleader", json!({ "points": [[400, 1000], [1400, 1400]], "text": "Sườn dầm dày 700" }));
    ex(&mut s, "tolerance", json!({ "at": [-700, -700], "symbol": "flatness", "tolerance1": "2" }));
    // 2. Mặt bằng tường (MLINE) với chữ T, góc.
    cur(&mut s, "TUONG");
    ex(
        &mut s,
        "mlstyle",
        json!({ "name": "TUONG220", "elements": [{ "offset": 110, "color": 2 }, { "offset": -110, "color": 2 }], "startCap": "line", "endCap": "line", "fill": true, "fillColor": 8 }),
    );
    ex(
        &mut s,
        "mline",
        json!({ "points": [[4000, 0], [10000, 0], [10000, 4000], [4000, 4000]], "closed": true, "scale": 1, "justification": "zero" }),
    );
    ex(&mut s, "mline", json!({ "points": [[7000, 3500], [7000, 500]], "scale": 1, "justification": "zero" }));
    ex(&mut s, "mledit", json!({ "tool": "closedTee", "p1": [7000, 2000], "p2": [7000, 0] }));
    ex(&mut s, "mledit", json!({ "tool": "closedTee", "p1": [7000, 2000], "p2": [7000, 4000] }));
    ex(&mut s, "mledit", json!({ "tool": "cutAll", "p1": [5000, 0], "p2": [5900, 0] }));
    // 3. Block cọc D600 + mảng liên kết 3x4.
    cur(&mut s, "COC");
    let c = ex(&mut s, "circle", json!({ "center": [0, 0], "radius": 300 }));
    let l1 = ex(&mut s, "line", json!({ "points": [[-400, 0], [400, 0]] }));
    let l2 = ex(&mut s, "line", json!({ "points": [[0, -400], [0, 400]] }));
    ex(&mut s, "block", json!({ "name": "COC_D600", "base": [0, 0], "handles": [h(&c), h(&l1), h(&l2)], "keep": "delete" }));
    let ins = ex(&mut s, "insert", json!({ "name": "COC_D600", "at": [12000, 500] }));
    ex(&mut s, "arrayrect", json!({ "handles": [h(&ins)], "rows": 3, "cols": 4, "rowSpacing": 1800, "colSpacing": 1800, "associative": true }));
    // 4. Tim tuyến (spline) và bảng.
    cur(&mut s, "TIM_TUYEN");
    ex(&mut s, "spline", json!({ "fit": [[-2000, -3000], [2000, -2500], [6000, -3500], [11000, -3000], [17000, -2000]] }));
    cur(&mut s, "CHU");
    ex(
        &mut s,
        "table",
        json!({ "at": [12000, -1000], "cells": [["C1", "1000.000", "1000.000"], ["C2", "4000.000", "1000.000"], ["C3", "7000.000", "1000.000"]], "title": "TỌA ĐỘ CỌC", "header": ["TÊN", "X", "Y"], "colWidth": 1500, "rowHeight": 350 }),
    );
    // 5. Khung tên có thuộc tính (cho thử Bộ bản vẽ) trên Layout1 và Layout2.
    cur(&mut s, "KHUNG");
    let mut ents = vec![h(&ex(&mut s, "rectang", json!({ "p1": [0, 0], "p2": [180, 40] })))];
    for (tag, y, def) in [("SO_TO", 28, "01"), ("TEN_BAN_VE", 16, "Tên bản vẽ"), ("TONG_SO_TO", 4, "1"), ("DU_AN", 28, "Dự án")] {
        let x = if tag == "DU_AN" { 90 } else { 5 };
        ents.push(h(&ex(&mut s, "attdef", json!({ "tag": tag, "at": [x, y], "height": 4, "default": def }))));
    }
    ex(&mut s, "block", json!({ "name": "KHUNG_TEN", "base": [0, 0], "handles": ents, "keep": "delete" }));
    for l in ["Layout1", "Layout2"] {
        ex(&mut s, "layout.set", json!({ "name": l }));
        ex(&mut s, "insert", json!({ "name": "KHUNG_TEN", "at": [5, 5] }));
    }
    ex(&mut s, "layout.set", json!({ "name": "Model" }));
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/testkit/mau/");
    for name in ["ban_ve_mau.dxf", "ban_ve_mau.dwg"] {
        let bytes = cadcraft_io::write(s.doc().unwrap(), name).unwrap();
        std::fs::write(format!("{dir}{name}"), &bytes).unwrap();
        // Reads back.
        let back = cadcraft_io::read(&bytes, name).unwrap();
        assert!(back.model.len() > 10, "{name}");
    }
}
