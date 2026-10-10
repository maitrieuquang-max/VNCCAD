use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

use crate::*;

fn sample() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.layers.push(Layer { name: "Walls".into(), color: Color::Index(1), ..Layer::default() });
    let c = |l: &str| Common { layer: l.into(), ..Common::default() };
    d.add(&Space::Model, c("Walls"), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) })).unwrap();
    d.add(&Space::Model, c("0"), EntityKind::Circle(Circle { center: Vec3::new(3.0, 4.0, 0.0), radius: 2.5 })).unwrap();
    d.add(&Space::Model, c("0"), EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 1.0, start: 0.5, end: 2.0 })).unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::LwPolyline(LwPolyline {
            vertices: vec![PolyVertex::new(Vec2::ZERO), PolyVertex::with_bulge(Vec2::new(4.0, 0.0), 0.5), PolyVertex::new(Vec2::new(4.0, 3.0))],
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Text(Text {
            insert: Vec3::new(1.0, 1.0, 0.0),
            align_pt: None,
            height: 0.25,
            value: "Hello Café".into(),
            rotation: 0.3,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::MText(MText {
            insert: Vec3::new(5.0, 5.0, 0.0),
            height: 0.2,
            width: 3.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "line one\\Pline two".into(),
            line_spacing: 1.0,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Ellipse(Ellipse {
            center: Vec3::new(8.0, 8.0, 0.0),
            major: Vec3::new(3.0, 0.0, 0.0),
            ratio: 0.5,
            start: 0.0,
            end: std::f64::consts::TAU,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Spline(cadcraft_geom::Spline::from_fit_points(&[Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(3.0, 1.0), Vec2::new(4.0, 3.0)])),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Dimension(Dimension {
            kind: DimKind::Linear { rotation: 0.0 },
            defpt: Vec3::new(0.0, -1.0, 0.0),
            text_mid: Vec3::ZERO,
            p13: Vec3::ZERO,
            p14: Vec3::new(10.0, 0.0, 0.0),
            p15: Vec3::ZERO,
            p16: Vec3::ZERO,
            text: String::new(),
            style: "Standard".into(),
            measurement: 10.0,
            text_rotation: 0.0,
            user_text_pos: false,
            block: None,
            overrides: Default::default(),
            assoc: Vec::new(),
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Hatch(Hatch {
            pattern: "ANSI31".into(),
            solid: false,
            loops: vec![HatchLoop {
                vertices: vec![PolyVertex::new(Vec2::ZERO), PolyVertex::new(Vec2::new(2.0, 0.0)), PolyVertex::new(Vec2::new(2.0, 2.0))],
                outer: true,
            }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
        }),
    )
    .unwrap();
    let mut b = Block::new("Bolt");
    b.entities.push(Entity::new(Handle(0x50), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 0.25 })));
    d.blocks.insert("Bolt".into(), std::sync::Arc::new(b));
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Insert(Insert {
            block: "Bolt".into(),
            insert: Vec3::new(2.0, 2.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
            clip: None,
        }),
    )
    .unwrap();
    d.add(&Space::Paper("Layout1".into()), c("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) })).unwrap();
    d
}

#[test]
fn dxf_roundtrip_preserves_entities() {
    let d = sample();
    let text = write_dxf(&d);
    let back = read_dxf(text.as_bytes()).unwrap();
    let kinds = |d: &Drawing| d.model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>();
    assert_eq!(kinds(&back), kinds(&d));
    assert_eq!(back.layer("Walls").unwrap().color, Color::Index(1));
    assert!(back.block("Bolt").is_some());
    assert_eq!(back.layout("Layout1").unwrap().entities.len(), 1);
    // Geometry survives.
    for (a, b) in d.model.iter().zip(back.model.iter()) {
        match (&a.kind, &b.kind) {
            (EntityKind::Line(x), EntityKind::Line(y)) => assert_eq!(x, y),
            (EntityKind::Circle(x), EntityKind::Circle(y)) => assert_eq!(x, y),
            (EntityKind::LwPolyline(x), EntityKind::LwPolyline(y)) => assert_eq!(x.vertices, y.vertices),
            (EntityKind::Text(x), EntityKind::Text(y)) => {
                assert_eq!(x.value, y.value);
                assert!((x.rotation - y.rotation).abs() < 1e-9);
            }
            (EntityKind::MText(x), EntityKind::MText(y)) => assert_eq!(x.contents, y.contents),
            (EntityKind::Hatch(x), EntityKind::Hatch(y)) => assert_eq!(x.loops, y.loops),
            _ => {}
        }
        assert_eq!(a.handle, b.handle);
        assert_eq!(a.common.layer, b.common.layer);
    }
    // Handles stay unique after reading.
    let mut back = back;
    let h = back.new_handle();
    assert!(back.entity(h).is_none());
}

#[test]
fn second_roundtrip_is_stable() {
    let d = sample();
    let t1 = write_dxf(&d);
    let d2 = read_dxf(t1.as_bytes()).unwrap();
    let d3 = read_dxf(write_dxf(&d2).as_bytes()).unwrap();
    assert_eq!(d2.model.len(), d3.model.len());
}

#[test]
fn reads_r12_style_polyline_and_paper_flag() {
    let text = "0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n8\n0\n66\n1\n70\n1\n0\nVERTEX\n8\n0\n10\n0\n20\n0\n0\nVERTEX\n8\n0\n10\n5\n20\n0\n42\n1\n0\nVERTEX\n8\n0\n10\n5\n20\n5\n0\nSEQEND\n0\nLINE\n67\n1\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "a.dxf").unwrap();
    assert_eq!(d.model.len(), 1);
    match &d.model.iter().next().unwrap().kind {
        EntityKind::LwPolyline(p) => {
            assert_eq!(p.vertices.len(), 3);
            assert!(p.closed);
            assert_eq!(p.vertices[1].bulge, 1.0);
        }
        _ => panic!(),
    }
    assert_eq!(d.layouts[0].entities.len(), 1);
}

#[test]
fn hostile_dxf_does_not_panic() {
    for t in [
        "0\nSECTION\n2\nENTITIES\n0\nHATCH\n91\n999999999\n92\n2\n93\n99999\n0\nENDSEC\n0\nEOF\n",
        "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n99\n40\n1\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n",
        "0\nSECTION\n2\nENTITIES\n0\nINSERT\n66\n1\n2\nX\n0\nATTRIB\n0\nENDSEC\n",
        "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nINSERT\n2\nA\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n0\nENDSEC\n0\nEOF\n",
        "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n42\n5\n20\n1\n0\nENDSEC\n0\nEOF\n",
    ] {
        if let Ok(d) = read(t.as_bytes(), "x.dxf") {
            let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
            let _ = d.extents(&Space::Model);
        }
    }
}

#[test]
fn svg_and_png_export() {
    let d = sample();
    let svg = String::from_utf8(write(&d, "a.svg").unwrap()).unwrap();
    assert!(svg.starts_with("<svg") && svg.contains("polyline"));
    let png = write(&d, "a.png").unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

/// Structural sanity check of a PDF: header, object count, xref offsets pointing at `n 0 obj`.
fn check_pdf(bytes: &[u8]) -> String {
    assert!(bytes.starts_with(b"%PDF-1.4"));
    let find = |pat: &[u8]| bytes.windows(pat.len()).position(|w| w == pat);
    let rfind = |pat: &[u8]| bytes.windows(pat.len()).rposition(|w| w == pat);
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64)..]).to_string();
    assert!(tail.trim_end().ends_with("%%EOF"));
    let sx = rfind(b"startxref\n").unwrap();
    let after = String::from_utf8_lossy(&bytes[sx + 10..]).to_string();
    let xref_off: usize = after.lines().next().unwrap().trim().parse().unwrap();
    assert!(bytes[xref_off..].starts_with(b"xref\n"));
    let xref = String::from_utf8_lossy(&bytes[xref_off..]).to_string();
    let mut lines = xref.lines().skip(1);
    let count: usize = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let objs = bytes.windows(7).filter(|w| w == b" 0 obj\n").count();
    assert_eq!(count, objs + 1, "xref size = objects + free entry");
    assert_eq!(bytes.windows(6).filter(|w| w == b"endobj").count(), objs);
    let entries: Vec<&str> = lines.take(count).collect();
    assert!(entries[0].starts_with("0000000000 65535 f"));
    for (i, e) in entries.iter().enumerate().skip(1) {
        let off: usize = e[..10].parse().unwrap();
        assert!(bytes[off..].starts_with(format!("{i} 0 obj").as_bytes()), "object {i} offset");
    }
    assert!(xref.contains(&format!("/Size {count}")));
    // The content stream's /Length matches its data.
    let sp = find(b"/Length ").unwrap();
    let after = String::from_utf8_lossy(&bytes[sp + 8..sp + 30]).to_string();
    let len: usize = after.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
    let start = find(b"stream\n").unwrap() + 7;
    assert!(bytes[start + len..].starts_with(b"\nendstream"));
    let compressed = find(b"/FlateDecode").is_some();
    let data = &bytes[start..start + len];
    if compressed {
        String::from_utf8(miniz_oxide::inflate::decompress_to_vec_zlib(data).unwrap()).unwrap()
    } else {
        String::from_utf8(data.to_vec()).unwrap()
    }
}

fn media_box(bytes: &[u8]) -> (f64, f64) {
    let text = String::from_utf8_lossy(bytes).to_string();
    let i = text.find("/MediaBox [0 0 ").unwrap() + 15;
    let v: Vec<f64> = text[i..].split(']').next().unwrap().split_whitespace().map(|x| x.parse().unwrap()).collect();
    (v[0], v[1])
}

#[test]
fn pdf_model_fitted_to_sheet() {
    let d = sample();
    let bytes = plot(&d, &Space::Model, &serde_json::json!({"paper": "A4", "landscape": true})).unwrap();
    let content = check_pdf(&bytes);
    let (w, h) = media_box(&bytes);
    assert!((w - 841.89).abs() < 0.01 && (h - 595.276).abs() < 0.01, "A4 landscape in points: {w} x {h}");
    assert!(content.contains(" m\n") && content.contains(" l\n") && content.contains("S\n"));
    // Walls layer is red; colour 7 prints black.
    assert!(content.contains("1 0 0 RG"));
    assert!(content.contains("0 0 0 RG"));
    // Uncompressed output is plain text and also valid.
    let raw = plot(&d, &Space::Model, &serde_json::json!({"paper": "Letter", "compress": false, "landscape": false})).unwrap();
    let c2 = check_pdf(&raw);
    assert!(!String::from_utf8_lossy(&raw).contains("FlateDecode"));
    assert_eq!(media_box(&raw), (612.0, 792.0));
    assert!(c2.contains("re W n"));
    // write() picks PDF by extension.
    assert!(write(&d, "x.pdf").unwrap().starts_with(b"%PDF"));
}

#[test]
fn pdf_layout_one_to_one_with_viewport() {
    let mut d = sample();
    let paper = Space::Paper("Layout1".into());
    d.layouts[0].page.lineweights = true;
    d.add(
        &paper,
        Common::default(),
        EntityKind::Viewport(Viewport {
            center: Vec3::new(5.0, 4.0, 0.0),
            width: 8.0,
            height: 6.0,
            view_center: Vec2::new(5.0, 2.5),
            view_height: 12.0,
            id: 2,
            locked: false,
            frozen_layers: Vec::new(),
            layer_colors: Vec::new(),
            clip: None,
        }),
    )
    .unwrap();
    let bytes = plot(&d, &paper, &serde_json::json!({})).unwrap();
    let content = check_pdf(&bytes);
    // ANSI A landscape (inches → points).
    assert_eq!(media_box(&bytes), (792.0, 612.0));
    // Viewport border at 1:1: x from 1in to 9in = 72pt..648pt.
    assert!(content.contains("72 72 m") || content.contains("72 72 l"), "{content}");
    // Lineweights on: default 0.25 mm = 0.709 pt.
    assert!(content.contains("0.709 w"));
    let off = plot(&d, &paper, &serde_json::json!({"lineweights": false})).unwrap();
    assert!(check_pdf(&off).contains("0 w"));
    assert!(plot(&d, &Space::Paper("Nope".into()), &serde_json::json!({})).is_err());
    assert!(plot(&d, &paper, &serde_json::json!({"paper": "Z9"})).is_err());
}

#[test]
fn pdf_hostile_options_and_empty() {
    let d = Drawing::new_metric();
    let bytes = plot(&d, &Space::Model, &serde_json::json!({})).unwrap();
    check_pdf(&bytes);
    let (w, h) = media_box(&bytes);
    assert!((w - 841.89).abs() < 0.01 && (h - 595.276).abs() < 0.01, "metric default is A4 landscape");
    let s = sample();
    for o in [
        serde_json::json!({"width": 1e308, "height": -5}),
        serde_json::json!({"width": 1e308, "height": 1e308, "scale": 1e308, "fit": false}),
        serde_json::json!({"scale": -1, "fit": false, "title": "a(b)\\c\u{e9}"}),
        serde_json::json!(null),
        serde_json::json!([1, 2]),
    ] {
        check_pdf(&plot(&s, &Space::Model, &o).unwrap());
    }
}

#[test]
fn dxf_roundtrips_page_setup() {
    let mut d = sample();
    let a3 = cadcraft_render::paper_size("A3").unwrap();
    {
        let p = &mut d.layouts[0].page;
        p.paper = a3.name.into();
        p.width_mm = a3.width_mm;
        p.height_mm = a3.height_mm;
        p.landscape = false;
        p.margins_mm = [5.0, 6.0, 7.0, 8.0];
    }
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    let p = &back.layout("Layout1").unwrap().page;
    assert_eq!((p.width_mm, p.height_mm, p.landscape), (297.0, 420.0, false));
    assert_eq!(p.margins_mm, [5.0, 6.0, 7.0, 8.0]);
    assert_eq!(p.paper, a3.name);
}

// ---------------------------------------------------------------------------------------------
// Round trips of styles, overrides, associativity, constraints and tables.

fn dim(kind: DimKind, p13: Vec3, p14: Vec3) -> Dimension {
    Dimension {
        kind,
        defpt: Vec3::new(p13.x, p13.y - 2.0, 0.0),
        text_mid: Vec3::ZERO,
        p13,
        p14,
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
    }
}

fn first<T>(d: &Drawing, f: impl Fn(&EntityKind) -> Option<T>) -> T {
    d.model.iter().find_map(|e| f(&e.kind)).expect("entity")
}

fn roundtrip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).unwrap()
}

fn full_dim_style() -> DimStyle {
    DimStyle {
        name: "Mech".into(),
        scale: 2.5,
        arrow_size: 0.25,
        ext_offset: 0.1,
        ext_extend: 0.2,
        text_height: 0.3,
        text_gap: 0.05,
        decimals: 3,
        angular_decimals: 2,
        linear_factor: 0.5,
        text_above: 1,
        text_inside_horizontal: false,
        text_outside_horizontal: false,
        arrow_block: "_ArchTick".into(),
        tick_size: 0.15,
        dim_line_color: Color::Index(1),
        ext_line_color: Color::ByLayer,
        text_color: Color::Index(5),
        text_style: "Romans".into(),
        post: "<> mm".into(),
        center_mark: -0.1,
        zero_suppression: 12,
        linear_unit: 4,
        tolerance: true,
        tol_plus: 0.02,
        tol_minus: 0.01,
        baseline_spacing: 0.5,
        annotative: true,
        arrow_block1: "_DOT".into(),
        arrow_block2: "_Open".into(),
        dim_line_extend: 0.125,
        text_just: 2,
        round: 0.25,
        decimal_separator: ",".into(),
        fraction_format: 1,
        limits: true,
        tol_decimals: 2,
        tol_scale: 0.75,
        alt: true,
        alt_factor: 0.03937,
        alt_decimals: 3,
        alt_post: "[<>]".into(),
        angular_unit: 1,
        suppress_ext1: true,
        suppress_ext2: true,
    }
}

#[test]
fn dimstyle_roundtrips_every_field() {
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Romans".into(), font: "romans.shx".into(), ..TextStyle::default() });
    d.dim_styles.push(full_dim_style());
    let text = write_dxf(&d);
    // Arrowheads are blocks referenced by handle (342/343/344) with DIMSAH set.
    assert!(text.contains("_ArchTick") && text.contains("_DOT") && text.contains("_Open"));
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.dim_style("Mech").unwrap(), &full_dim_style());
    assert_eq!(back.dim_style("Standard").unwrap(), &DimStyle::default());
    // Generated arrowhead blocks are not imported as user blocks, so they never pile up.
    assert!(back.blocks.keys().all(|k| !k.starts_with('_')), "{:?}", back.blocks.keys().collect::<Vec<_>>());
    let again = roundtrip(&back);
    assert_eq!(again.dim_styles, back.dim_styles);
    assert_eq!(write_dxf(&again).matches("AcDbBlockBegin").count(), write_dxf(&back).matches("AcDbBlockBegin").count());
}

#[test]
fn user_block_arrowheads_are_referenced_not_regenerated() {
    let mut d = Drawing::new_imperial();
    let mut b = Block::new("MyArrow");
    b.entities.push(Entity::new(Handle(0x60), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 0.5 })));
    d.blocks.insert("MyArrow".into(), std::sync::Arc::new(b));
    d.dim_styles.push(DimStyle { name: "U".into(), arrow_block: "MyArrow".into(), ..DimStyle::default() });
    let back = roundtrip(&d);
    assert_eq!(back.dim_style("U").unwrap().arrow_block, "MyArrow");
    assert!(back.block("MyArrow").is_some());
    assert_eq!(back.blocks.len(), d.blocks.len());
}

#[test]
fn dimension_overrides_roundtrip_as_dstyle_xdata() {
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Romans".into(), font: "romans.shx".into(), ..TextStyle::default() });
    let mut dm = dim(DimKind::Linear { rotation: 0.0 }, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
    let raw = serde_json::json!({
        "arrowSize": 0.5,
        "dimLineColor": 3,
        "textStyle": "Romans",
        "arrowBlock1": "_Dot",
        "arrowBlock": "",
        "decimalSeparator": ",",
        "suppressExt1": true,
        "post": "<> mm",
        "decimals": 2,
        "textJust": 1,
        "altFactor": 25.4,
    });
    dm.overrides = raw.as_object().unwrap().clone();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm.clone())).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("DSTYLE"));
    let back = read_dxf(text.as_bytes()).unwrap();
    let got = first(&back, |k| if let EntityKind::Dimension(x) = k { Some(x.clone()) } else { None });
    // Values come back canonical (colours as colour objects); the effective style is identical.
    assert_eq!(got.overrides.len(), dm.overrides.len(), "{:?}", got.overrides);
    let base = DimStyle::default();
    assert_eq!(base.with_overrides(&got.overrides), base.with_overrides(&dm.overrides));
    assert_eq!(got.overrides.get("dimLineColor"), Some(&serde_json::to_value(Color::Index(3)).unwrap()));
    // Canonical values round-trip exactly.
    let again = first(&roundtrip(&back), |k| if let EntityKind::Dimension(x) = k { Some(x.clone()) } else { None });
    assert_eq!(again.overrides, got.overrides);
}

#[test]
fn dimension_associativity_roundtrips() {
    let mut d = Drawing::new_imperial();
    let c = Common::default;
    let l1 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 5.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::new(10.0, 0.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) })).unwrap();
    let ci = d.add(&Space::Model, c(), EntityKind::Circle(Circle { center: Vec3::new(20.0, 0.0, 0.0), radius: 2.0 })).unwrap();
    let pl = d
        .add(
            &Space::Model,
            c(),
            EntityKind::LwPolyline(LwPolyline {
                vertices: vec![PolyVertex::new(Vec2::new(30.0, 0.0)), PolyVertex::new(Vec2::new(32.0, 0.0)), PolyVertex::new(Vec2::new(32.0, 3.0))],
                closed: false,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
        )
        .unwrap();
    let mut lin = dim(DimKind::Linear { rotation: 0.0 }, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
    lin.assoc = vec![
        DimAssoc { point: "p13".into(), handle: l1, snap: AssocSnap::Start },
        DimAssoc { point: "p14".into(), handle: l2, snap: AssocSnap::Start },
    ];
    let mut ali = dim(DimKind::Aligned, Vec3::new(22.0, 0.0, 0.0), Vec3::new(32.0, 3.0, 0.0));
    ali.assoc = vec![
        DimAssoc { point: "p13".into(), handle: ci, snap: AssocSnap::OnCircle { angle: 0.0 } },
        DimAssoc { point: "p14".into(), handle: pl, snap: AssocSnap::Vertex { index: 2 } },
        DimAssoc { point: "defpt".into(), handle: l1, snap: AssocSnap::Intersection { other: l2 } },
    ];
    let mut rad = dim(DimKind::Radius, Vec3::new(20.0, 0.0, 0.0), Vec3::ZERO);
    rad.assoc = vec![DimAssoc { point: "defpt".into(), handle: ci, snap: AssocSnap::Center }];
    let hl = d.add(&Space::Model, c(), EntityKind::Dimension(lin.clone())).unwrap();
    let ha = d.add(&Space::Model, c(), EntityKind::Dimension(ali.clone())).unwrap();
    let hr = d.add(&Space::Model, c(), EntityKind::Dimension(rad.clone())).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("DIMASSOC") && text.contains("ACAD_DIMASSOC") && text.contains("AcDbOsnapPointRef"));
    let back = read_dxf(text.as_bytes()).unwrap();
    let assoc_of = |d: &Drawing, h: Handle| match &d.entity(h).unwrap().kind {
        EntityKind::Dimension(x) => x.assoc.clone(),
        _ => panic!(),
    };
    assert_eq!(assoc_of(&back, hl), lin.assoc);
    assert_eq!(assoc_of(&back, ha), ali.assoc);
    assert_eq!(assoc_of(&back, hr), rad.assoc);
    // Entities owning reactors still land in model space.
    assert_eq!(back.model.len(), d.model.len());
    assert!(back.layouts.iter().all(|l| l.entities.is_empty()));

    // Without CADCraft's xdata (a file from another writer), the standard DIMASSOC objects
    // still give the extension-line links.
    let tags = cadcraft_dxf::parse(text.as_bytes()).unwrap();
    let mut stripped = Vec::new();
    let mut skipping = false;
    for t in tags {
        if t.code == 1001 {
            skipping = t.str() == "CADCRAFT";
        } else if t.code < 1000 {
            skipping = false;
        }
        if !skipping {
            stripped.push(t);
        }
    }
    let foreign = read_dxf(cadcraft_dxf::write_ascii(&stripped).as_bytes()).unwrap();
    assert_eq!(assoc_of(&foreign, hl), lin.assoc);
    let fa = assoc_of(&foreign, ha);
    assert_eq!(fa.len(), 2);
    assert_eq!(fa[1], ali.assoc[1]);
    assert!(matches!(fa[0].snap, AssocSnap::OnCircle { angle } if angle.abs() < 1e-9));
    assert!(assoc_of(&foreign, hr).is_empty(), "radial links are CADCraft-only");
}

fn parametric_sample(d: &mut Drawing) -> (Vec<Constraint>, Parametric) {
    let l1 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(4.0, 0.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(4.0, 0.0, 0.0), b: Vec3::new(4.0, 3.0, 0.0) })).unwrap();
    let constraints = vec![
        Constraint { id: 1, kind: ConstraintKind::Horizontal, refs: vec![GeomRef::whole(l1)], name: String::new(), expr: String::new() },
        Constraint {
            id: 2,
            kind: ConstraintKind::Coincident,
            refs: vec![GeomRef::new(l1, Sub::End), GeomRef::new(l2, Sub::Start)],
            name: String::new(),
            expr: String::new(),
        },
        Constraint {
            id: 3,
            kind: ConstraintKind::Distance(DistAxis::Vertical),
            refs: vec![GeomRef::new(l2, Sub::Start), GeomRef::new(l2, Sub::End)],
            name: "d1".into(),
            expr: "width/2 + 1".into(),
        },
        Constraint {
            id: 4,
            kind: ConstraintKind::Angular,
            refs: vec![GeomRef::whole(l1), GeomRef::whole(l2)],
            name: "ang1".into(),
            expr: "90".into(),
        },
        Constraint {
            id: 5,
            kind: ConstraintKind::Fix,
            refs: vec![GeomRef::new(l2, Sub::Vertex(7)), GeomRef::new(l2, Sub::Segment(3))],
            name: String::new(),
            expr: String::new(),
        },
    ];
    // Long text (chunk boundaries inside strings, spaces at the edges), backslashes that
    // would look like DXF `\U+` escapes, and non-ASCII text.
    let long = format!("  {}  \\U+0041 \\\\ \"quoted\" Ünïcødé ✓ {}  ", "word ".repeat(80), "x".repeat(300));
    let parametric = Parametric {
        parameters: vec![
            Parameter { name: "width".into(), expr: "12.5".into(), description: long },
            Parameter { name: "h".into(), expr: "width*0.4".into(), description: String::new() },
        ],
        settings: ParametricSettings {
            infer: true,
            distance_tolerance: 0.125,
            angle_tolerance: 2.5,
            auto_types: vec!["Parallel".into(), "Tangent".into()],
            bars_visible: false,
            bar_exceptions: vec![l1],
            dims_visible: false,
            dim_exceptions: vec![l2],
            bar_transparency: 30,
        },
    };
    d.constraints = constraints.clone();
    d.parametric = parametric.clone();
    (constraints, parametric)
}

#[test]
fn constraints_and_parameters_roundtrip_exactly() {
    let mut d = Drawing::new_imperial();
    let (constraints, parametric) = parametric_sample(&mut d);
    let text = write_dxf(&d);
    assert!(text.contains("CADCRAFT_CONSTRAINTS") && text.contains("XRECORD"));
    assert!(!text.contains("\\U+0041"), "no DXF unicode escape may appear in the payload");
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.constraints, constraints);
    assert_eq!(back.parametric, parametric);
    let again = roundtrip(&back);
    assert_eq!(again.constraints, constraints);
    assert_eq!(again.parametric, parametric);
    // Drawings without parametric data carry no record.
    assert!(!write_dxf(&Drawing::new_imperial()).contains("CADCRAFT_CONSTRAINTS"));
}

fn table_sample() -> Table {
    let cell = |t: &str| TableCell { text: t.into(), merged: None };
    let mut rows = vec![
        vec![cell("Door schedule"), cell(""), cell("")],
        vec![cell("Mark"), cell("Size"), cell("Notes")],
        vec![cell("D1"), cell("900 x 2100"), cell(&"long note ".repeat(40))],
        vec![cell("D2"), cell("800 x 2100"), cell("")],
    ];
    rows[0][0].merged = Some((1, 3));
    rows[2][0].merged = Some((2, 1));
    Table {
        insert: Vec3::new(5.0, 20.0, 0.0),
        col_widths: vec![1.5, 2.5, 4.0],
        row_heights: vec![0.5, 0.4, 0.4, 0.4],
        cells: rows,
        style: "Schedule".into(),
        text_height: 0.2,
        title: true,
        header: true,
    }
}

#[test]
fn tables_roundtrip_with_title_header_and_merges() {
    let mut d = Drawing::new_imperial();
    d.table_styles.push(TableStyle { name: "Schedule".into(), text_height: 0.25, margin: 0.1, title: false, header: true });
    let t = table_sample();
    d.add(&Space::Model, Common::default(), EntityKind::Table(t.clone())).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("ACAD_TABLE") && text.contains("TABLESTYLE") && text.contains("*T1"));
    let back = read_dxf(text.as_bytes()).unwrap();
    let got = first(&back, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None });
    assert_eq!(got, t);
    assert_eq!(back.table_styles, d.table_styles);
    assert!(back.block("*T1").is_none(), "table blocks are regenerated, not imported");
    let again = roundtrip(&back);
    assert_eq!(first(&again, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None }), t);
    assert_eq!(again.blocks.len(), back.blocks.len());
}

#[test]
fn text_style_flags_roundtrip() {
    let mut d = Drawing::new_imperial();
    let st = TextStyle {
        name: "Mirror".into(),
        font: "romans.shx".into(),
        big_font: "bigfont.shx".into(),
        height: 0.0,
        width_factor: 0.8,
        oblique: 15f64.to_radians(),
        backwards: true,
        upside_down: true,
        vertical: true,
        annotative: true,
    };
    d.text_styles.push(st.clone());
    d.text_styles.push(TextStyle { name: "Plain".into(), font: "arial.ttf".into(), backwards: true, ..TextStyle::default() });
    let back = roundtrip(&d);
    let got = back.text_style("Mirror").unwrap();
    assert_eq!((got.backwards, got.upside_down, got.vertical, got.annotative), (true, true, true, true));
    assert_eq!(got.big_font, "bigfont.shx");
    assert!((got.oblique - st.oblique).abs() < 1e-12);
    let plain = back.text_style("Plain").unwrap();
    assert_eq!((plain.backwards, plain.upside_down, plain.vertical, plain.annotative), (true, false, false, false));
}

#[test]
fn hostile_extension_data_never_panics() {
    let ent = |body: &str| format!("0\nSECTION\n2\nENTITIES\n{body}0\nENDSEC\n0\nEOF\n");
    let obj = |body: &str| format!("0\nSECTION\n2\nOBJECTS\n{body}0\nENDSEC\n0\nEOF\n");
    let cases = [
        // DSTYLE: unbalanced, odd counts, unknown codes, huge values, bad handles.
        ent("0\nDIMENSION\n5\nA0\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n{\n1070\n"),
        ent("0\nDIMENSION\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n{\n1070\n40\n1000\nnot a number\n1070\n271\n1070\n32767\n1070\n342\n1005\nZZZZ\n1070\n278\n1070\n-5\n1070\n176\n1070\n-32000\n1070\n9999\n1040\n1e308\n1002\n}\n"),
        ent("0\nDIMENSION\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n}\n1002\n{\n1002\n{\n"),
        // ASSOC: malformed links, missing handles, huge vertex index, nested braces.
        ent("0\nDIMENSION\n70\n1\n1001\nCADCRAFT\n1000\nASSOC\n1002\n{\n1002\n{\n1000\np13\n1070\n6\n1071\n-1\n1002\n}\n1002\n{\n1000\nbogus\n1005\n1\n1070\n0\n1002\n}\n1002\n{\n1000\np14\n1005\nFFFFFFFFFFFFFFFFFFFF\n1070\n4\n1002\n}\n1002\n{\n1000\np14\n1005\n2A\n1070\n6\n1071\n2147483647\n1002\n}\n"),
        ent("0\nDIMENSION\n70\n0\n1001\nCADCRAFT\n1000\nASSOC\n1002\n{\n1002\n{\n1002\n{\n1002\n{\n"),
        // Tables: enormous or negative counts, cells without bodies, spans past the edge.
        ent("0\nACAD_TABLE\n2\n*T1\n100\nAcDbTable\n91\n999999999\n92\n999999999\n171\n1\n0\nACAD_TABLE\n100\nAcDbTable\n91\n-4\n92\n3\n171\n"),
        ent("0\nACAD_TABLE\n100\nAcDbTable\n91\n2\n92\n2\n141\nnan\n142\n-1\n171\n1\n173\n1\n175\n99999\n176\n-3\n1\nx\n171\n171\n171\n171\n171\n2\nzzz\n1001\nCADCRAFT\n1000\nTABLE\n1040\n-1\n"),
        // DIMASSOC pointing at nothing, or at a non-dimension.
        format!(
            "{}{}",
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n10\n10\n0\n20\n0\n11\n1\n21\n0\n0\nDIMENSION\n5\n11\n70\n0\n0\nENDSEC\n",
            "0\nSECTION\n2\nOBJECTS\n0\nDIMASSOC\n100\nAcDbDimAssoc\n330\n11\n90\n-1\n1\nAcDbOsnapPointRef\n72\n1\n331\n10\n1\nAcDbOsnapPointRef\n72\n6\n331\n10\n332\n99\n1\n1\n1\n0\nDIMASSOC\n100\nAcDbDimAssoc\n330\n10\n90\n3\n0\nDIMASSOC\n0\nENDSEC\n0\nEOF\n"
        ),
        // Constraint record: garbage, wrong version, deep nesting, unterminated.
        obj("0\nDICTIONARY\n5\nC\n3\nCADCRAFT_CONSTRAINTS\n350\nD\n0\nXRECORD\n5\nD\n1\n{\"version\":1,\"constraints\":[{\"id\":1,\"kind\":\"nope\"}]}\n"),
        obj(&format!("0\nDICTIONARY\n3\nCADCRAFT_CONSTRAINTS\n350\nD\n0\nXRECORD\n5\nD\n1\n{}\n", "[".repeat(5000))),
        obj("0\nDICTIONARY\n3\nCADCRAFT_CONSTRAINTS\n350\nD\n3\nX\n0\nXRECORD\n5\nD\n1\n{\"version\":99}\n"),
        obj("0\nDICTIONARY\n3\nCADCRAFT_CONSTRAINTS\n0\nXRECORD\n1\n{\"version\":1,\n"),
        // Table styles with junk numbers; DIMSTYLE with junk and dangling handles.
        obj("0\nDICTIONARY\n3\nS\n350\nE\n0\nTABLESTYLE\n5\nE\n40\n-7\n140\n0\n280\n9\n"),
        "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nDIMSTYLE\n0\nDIMSTYLE\n2\nX\n271\n99999\n340\nBAD\n342\n0\n343\nFFFF\n176\n70000\n278\n0\n278\n1114112\n77\n-1\n1001\nAcadAnnotative\n1000\nAnnotativeData\n1002\n{\n0\nSTYLE\n2\nS\n70\n-1\n71\n99999\n0\nENDTAB\n0\nENDSEC\n0\nEOF\n".to_string(),
    ];
    for t in &cases {
        if let Ok(d) = read(t.as_bytes(), "x.dxf") {
            let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
            let _ = write_dxf(&d);
        }
    }
    // Huge but well-formed override lists are capped, not trusted.
    let mut big = String::from("0\nSECTION\n2\nENTITIES\n0\nDIMENSION\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n{\n");
    for _ in 0..50_000 {
        big.push_str("1070\n41\n1040\n0.5\n");
    }
    big.push_str("1002\n}\n0\nENDSEC\n0\nEOF\n");
    let d = read(big.as_bytes(), "x.dxf").unwrap();
    let dm = first(&d, |k| if let EntityKind::Dimension(x) = k { Some(x.clone()) } else { None });
    assert_eq!(dm.overrides.len(), 1);
}

/// Everything above in one drawing (also used for external validation).
fn extension_sample() -> Drawing {
    let mut d = sample();
    d.text_styles.push(TextStyle { name: "Romans".into(), font: "romans.shx".into(), backwards: true, annotative: true, ..TextStyle::default() });
    d.dim_styles.push(full_dim_style());
    d.table_styles.push(TableStyle { name: "Schedule".into(), text_height: 0.25, margin: 0.1, title: true, header: true });
    d.add(&Space::Model, Common::default(), EntityKind::Table(table_sample())).unwrap();
    let l1 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 30.0, 0.0), b: Vec3::new(0.0, 35.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(8.0, 30.0, 0.0), b: Vec3::new(8.0, 35.0, 0.0) })).unwrap();
    let mut dm = dim(DimKind::Linear { rotation: 0.0 }, Vec3::new(0.0, 30.0, 0.0), Vec3::new(8.0, 30.0, 0.0));
    dm.style = "Mech".into();
    dm.assoc = vec![
        DimAssoc { point: "p13".into(), handle: l1, snap: AssocSnap::Start },
        DimAssoc { point: "p14".into(), handle: l2, snap: AssocSnap::Start },
    ];
    dm.overrides = serde_json::json!({"arrowBlock2": "_BoxFilled", "textColor": 2, "decimals": 1}).as_object().unwrap().clone();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    let ci = d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(20.0, 30.0, 0.0), radius: 2.0 })).unwrap();
    let mut al = dim(DimKind::Aligned, Vec3::new(22.0, 30.0, 0.0), Vec3::new(8.0, 35.0, 0.0));
    al.assoc = vec![
        DimAssoc { point: "p13".into(), handle: ci, snap: AssocSnap::OnCircle { angle: 0.0 } },
        DimAssoc { point: "p14".into(), handle: l2, snap: AssocSnap::Intersection { other: l1 } },
    ];
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(al)).unwrap();
    parametric_sample(&mut d);
    d
}

#[test]
fn extension_sample_roundtrips_and_can_be_exported() {
    let d = extension_sample();
    let text = write_dxf(&d);
    if let Ok(path) = std::env::var("CADCRAFT_DXF_OUT") {
        std::fs::write(path, &text).unwrap();
    }
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.model.len(), d.model.len());
    assert_eq!(back.constraints, d.constraints);
    assert_eq!(back.dim_style("Mech"), d.dim_style("Mech"));
    let kinds = |d: &Drawing| d.model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>();
    assert_eq!(kinds(&back), kinds(&d));
}

/// DXF → DWG (acadrust) → DXF keeps what the DWG bridge supports: dimension styles with
/// arrow blocks, override and associativity xdata, the constraint XRECORD, tables and text
/// style flags. (TABLESTYLE objects do not survive the bridge.)
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_roundtrip_keeps_extension_data() {
    let d = extension_sample();
    let back = read(&write(&d, "x.dwg").unwrap(), "x.dwg").unwrap();
    assert_eq!(back.dim_style("Mech"), d.dim_style("Mech"));
    assert_eq!(back.constraints, d.constraints);
    assert_eq!(back.parametric, d.parametric);
    let dims =
        |d: &Drawing| d.model.iter().filter_map(|e| if let EntityKind::Dimension(x) = &e.kind { Some(x.clone()) } else { None }).collect::<Vec<_>>();
    let (a, b) = (dims(&d), dims(&back));
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.assoc, y.assoc);
        let st = DimStyle::default();
        assert_eq!(st.with_overrides(&x.overrides), st.with_overrides(&y.overrides));
    }
    assert_eq!(first(&back, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None }), table_sample());
    let r = back.text_style("Romans").unwrap();
    assert!(r.backwards && r.annotative);
}

#[test]
fn mleader_is_written_as_leader_and_mtext() {
    let mut d = Drawing::new_metric();
    let text = cadcraft_doc::MText {
        insert: Vec3::new(12.0, 5.0, 0.0),
        height: 2.5,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        contents: "Note".into(),
        style: "Standard".into(),
        line_spacing: 1.0,
    };
    let m = cadcraft_doc::MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)]],
        landing: Vec3::new(10.0, 5.0, 0.0),
        dogleg: 2.0,
        text: Some(text),
        style: "Standard".into(),
        arrow_size: 2.5,
    };
    d.add(&Space::Model, Default::default(), EntityKind::MLeader(m)).unwrap();
    let back = roundtrip(&d);
    assert!(back.model.iter().any(|e| matches!(e.kind, EntityKind::Leader(_))));
    assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::MText(t) if t.contents == "Note")));
}

#[test]
fn image_entities_round_trip_with_their_file() {
    let mut d = Drawing::new_metric();
    let im = Image {
        insert: Vec3::new(100.0, 200.0, 0.0),
        u: Vec3::new(0.5, 0.0, 0.0),
        v: Vec3::new(0.0, 0.5, 0.0),
        size: Vec2::new(640.0, 480.0),
        path: "D:\\Du an\\Anh ve tinh.jpg".into(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Image(im.clone())).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Image(Image { insert: Vec3::new(0.0, 0.0, 0.0), ..im.clone() })).unwrap();
    let text = crate::write_dxf(&d);
    assert!(text.contains("ACAD_IMAGE_DICT") && text.contains("IMAGEDEF_REACTOR") && text.contains("AcDbRasterImage"));
    // The CLASSES entry plus one IMAGEDEF object for the shared file.
    assert_eq!(text.matches("\nIMAGEDEF\n").count() + text.matches("\r\nIMAGEDEF\r\n").count(), 2, "one IMAGEDEF per file");
    let back = crate::read_dxf(text.as_bytes()).unwrap();
    let imgs: Vec<Image> = back.model.iter().filter_map(|e| if let EntityKind::Image(i) = &e.kind { Some(i.clone()) } else { None }).collect();
    assert_eq!(imgs.len(), 2);
    assert!(imgs.iter().all(|i| i.path == im.path && i.size == im.size && i.u == im.u));
    assert!(imgs.iter().any(|i| i.insert == im.insert));
}

#[test]
fn pdf_underlays_round_trip_as_pdfunderlay() {
    let mut d = Drawing::new_metric();
    let px = 100.0 / crate::raster::PDF_DPI; // 100 drawing units per inch of the page
    let im = Image {
        insert: Vec3::new(5.0, 6.0, 0.0),
        u: Vec3::new(px, 0.0, 0.0),
        v: Vec3::new(0.0, px, 0.0),
        size: Vec2::new(1240.0, 1754.0),
        path: "D:\\Ho so\\Mat bang.pdf#3".into(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Image(im.clone())).unwrap();
    let text = crate::write_dxf(&d);
    assert!(text.contains("PDFUNDERLAY") && text.contains("PDFDEFINITION") && text.contains("ACAD_PDFDEFINITIONS"));
    assert!(!text.contains("IMAGEDEF"), "a PDF is not written as a raster image");
    let back = crate::read_dxf(text.as_bytes()).unwrap();
    let got = back.model.iter().find_map(|e| if let EntityKind::Image(i) = &e.kind { Some(i.clone()) } else { None }).unwrap();
    assert_eq!(got.path, im.path);
    assert_eq!(got.insert, im.insert);
    assert!((got.u.x - px).abs() < 1e-12 && got.u.y.abs() < 1e-12);
}

/// VNCCad: two "sheets" side by side in model space; a window plot shows only one.
#[test]
fn pdf_plot_window_keeps_one_sheet() {
    let mut d = Drawing::new_metric();
    let red = Common { color: Color::Index(1), ..Common::default() };
    let blue = Common { color: Color::Index(5), ..Common::default() };
    let rect = |x: f64| {
        EntityKind::LwPolyline(LwPolyline {
            vertices: [(x, 0.0), (x + 420.0, 0.0), (x + 420.0, 297.0), (x, 297.0)].iter().map(|(a, b)| PolyVertex::new(Vec2::new(*a, *b))).collect(),
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        })
    };
    d.add(&Space::Model, red, rect(0.0)).unwrap();
    d.add(&Space::Model, blue, rect(1000.0)).unwrap();
    let all = check_pdf(&plot(&d, &Space::Model, &serde_json::json!({"compress": false})).unwrap());
    assert!(all.contains("1 0 0 RG") && all.contains("0 0 1 RG"));
    let one = plot(&d, &Space::Model, &serde_json::json!({"compress": false, "paper": "A3", "window": [[-1, -1], [421, 298]]})).unwrap();
    let c = check_pdf(&one);
    assert!(c.contains("1 0 0 RG") && !c.contains("0 0 1 RG"), "only the first sheet");
    // A3 turned to landscape by the window's shape.
    let (w, h) = media_box(&one);
    assert!(w > h && (w - 1190.55).abs() < 0.1, "{w} x {h}");
    // Saved as the model page setup, the window survives DXF (and is used by a plain plot).
    let mut page = pdf::model_page(&d);
    page.plot_area = "window".into();
    page.window = Some([999.0, -1.0, 1421.0, 298.0]);
    page.plot_style_table = "monochrome.ctb".into();
    d.model_page = Some(page);
    d.header.set("CANNOSCALE", HVal::Str("1:100".into()));
    let back = read(&write(&d, "w.dxf").unwrap(), "w.dxf").unwrap();
    let p = back.model_page.clone().expect("model page setup read back");
    assert_eq!(p.plot_area, "window");
    assert_eq!(p.window, Some([999.0, -1.0, 1421.0, 298.0]));
    assert_eq!(p.plot_style_table, "monochrome.ctb");
    assert_eq!(back.header.str("CANNOSCALE", ""), "1:100");
    let c = check_pdf(&plot(&back, &Space::Model, &serde_json::json!({"compress": false})).unwrap());
    // Only the second sheet, plotted black by the monochrome table.
    assert!(!c.contains("0 0 1 RG") && !c.contains("1 0 0 RG") && c.contains("0 0 0 RG"));
    assert!(c.contains(" l\n"));
}

/// VNCCad: raster images are drawn into plotted PDFs (under the vectors), and PUBLISH writes
/// one page per sheet.
#[test]
fn pdf_embeds_images_and_publishes_pages() {
    let mut png = Vec::new();
    let rgba: Vec<u8> = (0..4 * 4).flat_map(|i| if i % 2 == 0 { [255, 0, 0, 255] } else { [0, 0, 255, 128] }).collect();
    crate::raster::encode_png_for_tests(&rgba, 4, 4, &mut png);
    crate::raster::register("nen-test.png", png);
    let mut d = Drawing::new_metric();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Image(Image {
            insert: Vec3::ZERO,
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
            size: Vec2::new(40.0, 40.0),
            path: "nen-test.png".into(),
        }),
    )
    .unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(40.0, 40.0, 0.0) })).unwrap();
    let bytes = plot(&d, &Space::Model, &serde_json::json!({ "compress": false })).unwrap();
    let c = check_pdf(&bytes);
    assert!(c.contains(" cm /Im1 Do"), "image drawn");
    assert!(c.find("/Im1 Do").unwrap() < c.find(" l\n").unwrap(), "under the vectors");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Image /Width 4 /Height 4"));
    assert!(text.contains("/SMask"), "transparency kept");
    // Two pages.
    let sheets = vec![
        (Space::Model, PdfOptions { window: Some(cadcraft_geom::Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(20.0, 20.0))), ..PdfOptions::default() }),
        (Space::Model, PdfOptions::default()),
    ];
    let two = crate::pdf::publish(&d, &sheets, false, "test").unwrap();
    let t = String::from_utf8_lossy(&two);
    assert_eq!(t.matches("/Type /Page ").count(), 2);
    assert!(t.contains("/Count 2"));
    assert!(t.trim_end().ends_with("%%EOF"));
}

/// VNCCad: DWG is written in the drawing's version; text survives in every version.
#[test]
fn dwg_versions_keep_unicode_text() {
    for (ver, magic) in [("AC1015", "AC1015"), ("AC1018", "AC1018"), ("AC1021", "AC1021"), ("AC1027", "AC1027"), ("AC1032", "AC1032")] {
        let mut d = Drawing::new_metric();
        d.header.set("ACADVER", HVal::Str(ver.into()));
        d.layers.push(Layer { name: "Cầu-布局".into(), ..Layer::default() });
        let c = Common { layer: "Cầu-布局".into(), ..Common::default() };
        d.add(
            &Space::Model,
            c,
            EntityKind::Text(
                serde_json::from_value(serde_json::json!({ "insert": {"x":0.0,"y":0.0,"z":0.0}, "height": 2.5, "value": "Mố cầu M1 建筑面积" }))
                    .unwrap(),
            ),
        )
        .unwrap();
        let bytes = write(&d, "t.dwg").unwrap();
        assert!(bytes.starts_with(magic.as_bytes()), "{ver}");
        let back = read(&bytes, "t.dwg").unwrap();
        let texts: Vec<String> =
            back.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
        assert_eq!(texts, vec!["Mố cầu M1 建筑面积".to_string()], "{ver}");
        assert!(back.layers.iter().any(|l| l.name == "Cầu-布局"), "{ver}");
    }
}

/// VNCCad: layer plot style names (STB drawings) survive DXF.
#[test]
fn layer_plot_styles_round_trip() {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "TIM".into(), plot_style: "Net dam".into(), ..Layer::default() });
    d.layers.push(Layer { name: "PHU".into(), ..Layer::default() });
    let back = read(&write(&d, "p.dxf").unwrap(), "p.dxf").unwrap();
    assert_eq!(back.layer("TIM").unwrap().plot_style, "Net dam");
    assert_eq!(back.layer("PHU").unwrap().plot_style, "Normal");
}

/// VNCCad: every DWG version keeps what the bridge carries, merged table cells included
/// (R2010+ files store merges as a range list).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_roundtrip_keeps_extension_data_in_every_version() {
    for v in ["AC1015", "AC1018", "AC1024", "AC1027", "AC1032"] {
        let mut d = extension_sample();
        d.header.set("ACADVER", HVal::Str(v.into()));
        let back = read(&write(&d, "x.dwg").unwrap(), "x.dwg").unwrap();
        assert_eq!(first(&back, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None }), table_sample(), "{v}");
        assert_eq!(back.dim_style("Mech"), d.dim_style("Mech"), "{v}");
        assert_eq!(back.constraints, d.constraints, "{v}");
    }
}

/// VNCCad: a dynamic block as AutoCAD writes it (evaluation graph with a linear parameter and
/// a stretch action; a reference `*U1` with its cached value) is read, then kept by VNCCad's own
/// record through DXF and DWG.
#[test]
fn dynamic_blocks_from_autocad_objects_survive_saving() {
    let g = |pairs: &[(i32, &str)]| pairs.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>();
    let line = |h: &str, x1: &str, y1: &str, x2: &str, y2: &str| {
        g(&[(0, "LINE"), (5, h), (100, "AcDbEntity"), (8, "0"), (100, "AcDbLine"), (10, x1), (20, y1), (30, "0"), (11, x2), (21, y2), (31, "0")])
    };
    let mut s = String::new();
    s += &g(&[(0, "SECTION"), (2, "TABLES"), (0, "TABLE"), (2, "BLOCK_RECORD")]);
    s += &g(&[(0, "BLOCK_RECORD"), (5, "20"), (102, "{ACAD_XDICTIONARY"), (360, "30"), (102, "}"), (100, "AcDbBlockTableRecord"), (2, "COC")]);
    s += &g(&[(0, "BLOCK_RECORD"), (5, "21"), (100, "AcDbBlockTableRecord"), (2, "*U1")]);
    s += &g(&[(0, "ENDTAB"), (0, "ENDSEC"), (0, "SECTION"), (2, "BLOCKS")]);
    s += &g(&[(0, "BLOCK"), (5, "50"), (8, "0"), (2, "COC"), (70, "0"), (10, "0"), (20, "0"), (30, "0")]);
    s += &line("100", "0", "0", "1000", "0");
    s += &line("101", "1000", "0", "1000", "200");
    s += &g(&[(0, "ENDBLK"), (5, "51")]);
    s += &g(&[(0, "BLOCK"), (5, "52"), (8, "0"), (2, "*U1"), (70, "1"), (10, "0"), (20, "0"), (30, "0")]);
    s += &line("110", "0", "0", "1500", "0");
    s += &line("111", "1500", "0", "1500", "200");
    s += &g(&[(0, "ENDBLK"), (5, "53"), (0, "ENDSEC"), (0, "SECTION"), (2, "ENTITIES")]);
    s += &g(&[
        (0, "INSERT"),
        (5, "200"),
        (102, "{ACAD_XDICTIONARY"),
        (360, "40"),
        (102, "}"),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbBlockReference"),
        (2, "*U1"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
    ]);
    s += &g(&[(0, "ENDSEC"), (0, "SECTION"), (2, "OBJECTS")]);
    s += &g(&[(0, "DICTIONARY"), (5, "30"), (330, "20"), (100, "AcDbDictionary"), (3, "ACAD_ENHANCEDBLOCK"), (360, "31")]);
    s += &g(&[(0, "ACAD_EVALUATION_GRAPH"), (5, "31"), (330, "30"), (100, "AcDbEvalGraph")]);
    s += &g(&[
        (0, "BLOCKLINEARPARAMETER"),
        (5, "32"),
        (330, "31"),
        (100, "AcDbEvalExpr"),
        (90, "34"),
        (100, "AcDbBlockElement"),
        (300, "Linear"),
        (100, "AcDbBlockParameter"),
        (100, "AcDbBlock2PtParameter"),
        (1010, "0"),
        (1020, "0"),
        (1030, "0"),
        (1011, "1000"),
        (1021, "0"),
        (1031, "0"),
        (100, "AcDbBlockLinearParameter"),
        (305, "Chiều dài"),
        (140, "1000"),
    ]);
    s += &g(&[
        (0, "BLOCKSTRETCHACTION"),
        (5, "33"),
        (330, "31"),
        (100, "AcDbEvalExpr"),
        (90, "41"),
        (100, "AcDbBlockElement"),
        (300, "Stretch"),
        (100, "AcDbBlockAction"),
        (70, "0"),
        (71, "2"),
        (330, "100"),
        (330, "101"),
        (1010, "0"),
        (1020, "0"),
        (1030, "0"),
        (100, "AcDbBlockStretchAction"),
        (92, "34"),
        (93, "34"),
        (301, "EndXDelta"),
        (302, "EndYDelta"),
        (72, "2"),
        (1011, "900"),
        (1021, "-10"),
        (1011, "1100"),
        (1021, "210"),
        (140, "1.0"),
        (141, "0.0"),
    ]);
    s += &g(&[(0, "DICTIONARY"), (5, "40"), (330, "200"), (100, "AcDbDictionary"), (3, "AcDbBlockRepresentation"), (360, "41")]);
    s += &g(&[(0, "DICTIONARY"), (5, "41"), (330, "40"), (100, "AcDbDictionary"), (3, "AcDbRepData"), (360, "42"), (3, "AppDataCache"), (360, "43")]);
    s += &g(&[(0, "ACDB_BLOCKREPRESENTATION_DATA"), (5, "42"), (330, "41"), (100, "AcDbBlockRepresentationData"), (70, "1"), (340, "20")]);
    s += &g(&[(0, "DICTIONARY"), (5, "43"), (330, "41"), (100, "AcDbDictionary"), (3, "ACAD_ENHANCEDBLOCKDATA"), (360, "44")]);
    s += &g(&[(0, "DICTIONARY"), (5, "44"), (330, "43"), (100, "AcDbDictionary"), (3, "34"), (360, "45")]);
    s += &g(&[
        (0, "XRECORD"),
        (5, "45"),
        (330, "44"),
        (100, "AcDbXrecord"),
        (280, "1"),
        (70, "25"),
        (70, "104"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (10, "1500"),
        (20, "0"),
        (30, "0"),
    ]);
    s += &g(&[(0, "ENDSEC"), (0, "EOF")]);
    let d = read(s.as_bytes(), "dyn.dxf").unwrap();
    let def = d.block("COC").and_then(|b| b.dyn_def.clone()).expect("definition");
    let p = def.params.first().unwrap();
    assert_eq!((p.id, p.name.as_str(), p.kind), (34, "Chiều dài", cadcraft_doc::DynKind::Linear));
    let a = p.actions.first().unwrap();
    assert_eq!(a.kind, cadcraft_doc::DynActionKind::Stretch);
    assert_eq!(a.entities, vec![Handle(0x100), Handle(0x101)]);
    assert_eq!(a.frame.len(), 4);
    let r = d.block("*U1").and_then(|b| b.dyn_ref.clone()).expect("reference");
    assert_eq!(r.source, "COC");
    assert_eq!(r.values, vec![(34, cadcraft_doc::DynValue::Distance(1500.0))]);
    // VNCCad's own record keeps them (AutoCAD's objects are not written back).
    for name in ["x.dxf", "x.dwg"] {
        let back = read(&write(&d, name).unwrap(), name).unwrap();
        assert_eq!(back.block("COC").and_then(|b| b.dyn_def.clone()), Some(def.clone()), "{name}");
        // DWG renames anonymous blocks (*U1 → *U0): found through the INSERT.
        let used = first(&back, |k| if let EntityKind::Insert(i) = k { Some(i.block.clone()) } else { None });
        assert_eq!(back.block(&used).and_then(|b| b.dyn_ref.clone()), Some(r.clone()), "{name}");
        let hs: Vec<Handle> = back.block("COC").unwrap().entities.iter().map(|e| e.handle).collect();
        assert!(a.entities.iter().all(|h| hs.contains(h)), "{name}: entity handles kept");
    }
}

/// VNCCad: an associative array keeps its parameters and source objects through DXF and DWG.
#[test]
fn associative_arrays_survive_saving() {
    let mut d = Drawing::new_metric();
    let mut b = cadcraft_doc::Block::new("*U3");
    let src = cadcraft_doc::Entity {
        handle: Handle(0x500),
        common: Common::default(),
        kind: EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) }),
    };
    let a = cadcraft_doc::ArrayDef {
        kind: "rect".into(),
        rows: 2,
        cols: 2,
        row_spacing: 3.0,
        col_spacing: 4.0,
        source: vec![src.clone()],
        ..Default::default()
    };
    b.entities.push(cadcraft_doc::Entity { handle: Handle(0x501), ..src });
    b.array = Some(a.clone());
    d.blocks.insert("*U3".into(), std::sync::Arc::new(b));
    let ins: cadcraft_doc::Insert = serde_json::from_value(serde_json::json!({ "block": "*U3", "insert": {"x":0.0,"y":0.0,"z":0.0} })).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    for name in ["a.dxf", "a.dwg"] {
        let back = read(&write(&d, name).unwrap(), name).unwrap();
        let used = first(&back, |k| if let EntityKind::Insert(i) = k { Some(i.block.clone()) } else { None });
        assert_eq!(back.block(&used).and_then(|b| b.array.clone()), Some(a.clone()), "{name}");
    }
}

/// VNCCad: XCLIP boundaries are written as AutoCAD's SPATIAL_FILTER and read back (DXF, DWG).
#[test]
fn xclip_boundaries_survive_saving() {
    let mut d = Drawing::new_metric();
    let mut b = cadcraft_doc::Block::new("K");
    b.base = Vec3::new(1.0, 1.0, 0.0);
    b.entities.push(cadcraft_doc::Entity {
        handle: Handle(0x900),
        common: Common::default(),
        kind: EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(9.0, 0.0, 0.0) }),
    });
    d.blocks.insert("K".into(), std::sync::Arc::new(b));
    let clip = vec![cadcraft_geom::Vec2::new(0.0, -1.0), cadcraft_geom::Vec2::new(5.0, -1.0), cadcraft_geom::Vec2::new(3.0, 4.0)];
    let ins: cadcraft_doc::Insert = serde_json::from_value(
        serde_json::json!({ "block": "K", "insert": {"x":50.0,"y":20.0,"z":0.0}, "scale": {"x":2.0,"y":2.0,"z":1.0}, "rotation": 0.5 }),
    )
    .unwrap();
    let ins = cadcraft_doc::Insert { clip: Some(clip.clone()), ..ins };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    for name in ["c.dxf", "c.dwg"] {
        let back = read(&write(&d, name).unwrap(), name).unwrap();
        let got = first(&back, |k| if let EntityKind::Insert(i) = k { i.clip.clone() } else { None });
        assert_eq!(got.len(), 3, "{name}");
        for (a, b) in got.iter().zip(&clip) {
            assert!(a.near(*b, 1e-6), "{name}: {got:?}");
        }
    }
}

/// VNCCad: table formulas: the file holds the values (other programs show them) and the formulas.
#[test]
fn table_formulas_survive_dxf_with_their_values() {
    let mut d = Drawing::new_metric();
    let cell = |t: &str| TableCell { text: t.into(), merged: None };
    let t = Table {
        insert: Vec3::ZERO,
        col_widths: vec![10.0, 10.0, 10.0],
        row_heights: vec![5.0, 5.0],
        cells: vec![vec![cell("2"), cell("3"), cell("=A1*B1")], vec![cell("4"), cell("5"), cell("=SUM(A1:B2)")]],
        style: "Standard".into(),
        text_height: 2.5,
        title: false,
        header: false,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Table(t.clone())).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("\n6\n") || text.contains("\r\n6\r\n") || text.lines().any(|l| l.trim() == "6"), "value written");
    let back = read_dxf(text.as_bytes()).unwrap();
    let got = first(&back, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None });
    assert_eq!(got.cells[0][2].text, "=A1*B1");
    assert_eq!(got.display_text(1, 2), "14");
}

/// VNCCad: a hatch bounded by a full-circle edge (AutoCAD writes start 0°, end 360°) keeps a
/// closed round loop (it used to collapse to one vertex and vanish).
#[test]
fn hatch_with_a_circle_edge_is_a_disc() {
    let g = |pairs: &[(i32, &str)]| pairs.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>();
    let mut s = g(&[(0, "SECTION"), (2, "ENTITIES")]);
    s += &g(&[
        (0, "HATCH"),
        (5, "A1"),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbHatch"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (210, "0"),
        (220, "0"),
        (230, "1"),
        (2, "SOLID"),
        (70, "1"),
        (71, "0"),
        (91, "1"),
        (92, "1"),
        (93, "1"),
        (72, "2"),
        (10, "5"),
        (20, "5"),
        (40, "2"),
        (50, "0"),
        (51, "360"),
        (73, "1"),
        (97, "0"),
        (75, "0"),
        (76, "1"),
        (98, "0"),
    ]);
    s += &g(&[(0, "ENDSEC"), (0, "EOF")]);
    let d = read(s.as_bytes(), "h.dxf").unwrap();
    let h = first(&d, |k| if let EntityKind::Hatch(h) = k { Some(h.clone()) } else { None });
    let l = &h.loops[0];
    assert!(l.vertices.len() >= 2, "{l:?}");
    let area = cadcraft_geom::Polyline { vertices: l.vertices.clone(), closed: true }.area().abs();
    assert!((area - std::f64::consts::PI * 4.0).abs() < 0.01, "{area}");
}

/// VNCCad: wipeouts are saved (they used to be dropped) and come back the same way up;
/// AutoCAD's two-corner rectangular boundary is read as a rectangle.
#[test]
fn wipeout_roundtrip_keeps_its_shape() {
    let mut d = Drawing::new_metric();
    let tri = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 4.0)];
    d.add(&Space::Model, Common::default(), EntityKind::Wipeout(Wipeout { boundary: tri.clone() })).unwrap();
    let bytes = write(&d, "w.dxf").unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("AcDbWipeout"), "class and entity are written");
    let back = read(&bytes, "w.dxf").unwrap();
    let w = first(&back, |k| if let EntityKind::Wipeout(w) = k { Some(w.clone()) } else { None });
    assert_eq!(w.boundary.len(), 3);
    for (a, b) in w.boundary.iter().zip(&tri) {
        assert!(a.dist(*b) < 1e-9, "{a:?} vs {b:?}");
    }
    // Through DWG too.
    let dwg = write(&d, "w.dwg").unwrap();
    let back = read(&dwg, "w.dwg").unwrap();
    let w = first(&back, |k| if let EntityKind::Wipeout(w) = k { Some(w.clone()) } else { None });
    assert!(w.boundary.iter().zip(&tri).all(|(a, b)| a.dist(*b) < 1e-6), "{:?}", w.boundary);
}

/// VNCCad: multilines keep their style, vertices and cuts; tolerances their frame text;
/// both through DXF and DWG.
#[test]
fn mline_and_tolerance_roundtrip() {
    let mut d = Drawing::new_metric();
    let st = MLineStyle {
        name: "TUONG220".into(),
        description: "Tường 220".into(),
        start_line: true,
        end_line: true,
        fill: true,
        fill_color: cadcraft_color::Color::Index(8),
        elements: vec![
            MLineElement { offset: 110.0, color: cadcraft_color::Color::Index(1), linetype: "BYLAYER".into() },
            MLineElement { offset: 0.0, color: cadcraft_color::Color::Index(3), linetype: "BYLAYER".into() },
            MLineElement { offset: -110.0, ..Default::default() },
        ],
        ..MLineStyle::default()
    };
    d.mline_styles.push(st.clone());
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(5000.0, 0.0), Vec2::new(5000.0, 3000.0)];
    let mut m = MLine::build(&pts, false, &st, 1.0, 1);
    m.set_element_pieces(0, 1, &[(0.0, 1000.0), (2000.0, f64::MAX)]);
    d.add(&Space::Model, Common::default(), EntityKind::MLine(m.clone())).unwrap();
    let tol = cadcraft_doc::Tolerance {
        insert: Vec3::new(100.0, 200.0, 0.0),
        dir: Vec2::X,
        text: "{\\Fgdt;j}%%v{\\Fgdt;n}0.05{\\Fgdt;m}%%vA%%vB".into(),
        style: "ISO-25".into(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Tolerance(tol.clone())).unwrap();
    for name in ["m.dxf", "m.dwg"] {
        let bytes = write(&d, name).unwrap();
        let back = read(&bytes, name).unwrap();
        let s2 = back.mline_style("TUONG220").unwrap_or_else(|| panic!("{name}: style"));
        assert_eq!(s2.elements.len(), 3, "{name}");
        assert!((s2.elements[0].offset - 110.0).abs() < 1e-9 && s2.start_line && s2.end_line && s2.fill, "{name}: {s2:?}");
        let m2 = first(&back, |k| if let EntityKind::MLine(m) = k { Some(m.clone()) } else { None });
        assert_eq!(m2.style, "TUONG220", "{name}");
        assert_eq!(m2.points(), m.points(), "{name}");
        assert_eq!(m2.element_pieces(0, 1).len(), 2, "{name}: the cut survives");
        let g = m2.geometry(Some(s2));
        assert_eq!(g.lines.len(), 7, "{name}");
        let t2 = first(&back, |k| if let EntityKind::Tolerance(t) = k { Some(t.clone()) } else { None });
        assert_eq!(t2.text, tol.text, "{name}");
        assert!(t2.insert.xy().dist(tol.insert.xy()) < 1e-9, "{name}");
    }
}
