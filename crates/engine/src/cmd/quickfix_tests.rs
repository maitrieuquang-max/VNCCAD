use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec2;
use serde_json::json;

use crate::{Input, Session};

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.last().unwrap().kind.clone()
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.iter().count()
}

#[test]
fn rectang_relative_corner() {
    let mut s = Session::new();
    s.start("rectang").unwrap();
    s.input(Input::Point(Vec2::new(2.0, 3.0))).unwrap();
    s.cmdline("@10,5").unwrap();
    let EntityKind::LwPolyline(p) = last(&s) else { panic!("no rectangle") };
    let xs: Vec<f64> = p.vertices.iter().map(|v| v.p.x).collect();
    let ys: Vec<f64> = p.vertices.iter().map(|v| v.p.y).collect();
    assert!(xs.iter().any(|x| (x - 12.0).abs() < 1e-9), "{xs:?}");
    assert!(ys.iter().any(|y| (y - 8.0).abs() < 1e-9), "{ys:?}");
}

#[test]
fn aliases_and_isolation() {
    let mut s = Session::new();
    s.execute("aliasedit", &json!({"alias": "zz", "command": "line"})).unwrap();
    s.cmdline("zz 0,0 5,0").unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(count(&s), 1);
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("isolateobjects", &json!({"handles": [hs[0]]})).unwrap();
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| e.common.visible).count(), 1);
    s.execute("unisolateobjects", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| e.common.visible).count(), 2);
    s.execute("copyclip", &json!({"handles": hs})).unwrap();
    s.execute("pasteblock", &json!({"at": [20, 0]})).unwrap();
    assert!(matches!(last(&s), EntityKind::Insert(_)));
}

#[test]
fn tracking_and_from_with_move_copy() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.cmdline("move all").unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("0,0").unwrap();
    // TK: track 10 along X, then 5 along Y.
    s.cmdline("tk").unwrap();
    s.input(Input::Point(Vec2::new(10.0, 0.3))).unwrap();
    s.input(Input::Point(Vec2::new(10.2, 5.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    let EntityKind::Circle(c) = last(&s) else { panic!() };
    assert!((c.center.x - 10.0).abs() < 1e-9 && (c.center.y - 5.0).abs() < 1e-9, "{:?}", c.center);
    // FROM with COPY.
    s.cmdline("copy l").unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("10,5").unwrap();
    s.cmdline("fro").unwrap();
    s.input(Input::Point(Vec2::new(20.0, 20.0))).unwrap();
    s.cmdline("@1,2").unwrap();
    s.input(Input::Enter).unwrap();
    let EntityKind::Circle(c) = last(&s) else { panic!() };
    assert!((c.center.x - 21.0).abs() < 1e-9 && (c.center.y - 22.0).abs() < 1e-9, "{:?}", c.center);
}

#[test]
fn scale_and_rotate_reference() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [4, 0]]})).unwrap();
    // SCALE Reference: reference 4 → new length 10.
    s.cmdline("scale l").unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("0,0 r 4 10").unwrap();
    let EntityKind::Line(l) = last(&s) else { panic!() };
    assert!((l.b.x - 10.0).abs() < 1e-9, "{:?}", l.b);
    // Reference by two points (the line's ends) → new length 5.
    s.cmdline("scale l").unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("0,0 r 0,0 10,0 5").unwrap();
    let EntityKind::Line(l) = last(&s) else { panic!() };
    assert!((l.b.x - 5.0).abs() < 1e-9, "{:?}", l.b);
    // ROTATE Reference: 0° → 90°.
    s.cmdline("rotate l").unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("0,0 r 0 90").unwrap();
    let EntityKind::Line(l) = last(&s) else { panic!() };
    assert!(l.b.x.abs() < 1e-9 && (l.b.y - 5.0).abs() < 1e-9, "{:?}", l.b);
}

#[test]
fn break_polylines() {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 5]})).unwrap();
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("break", &json!({"handles": [h], "p1": [2, 0], "p2": [4, 0]})).unwrap();
    let EntityKind::LwPolyline(p) = last(&s) else { panic!() };
    assert!(!p.closed);
    let len: f64 = cadcraft_geom::Polyline { vertices: p.vertices.clone(), closed: false }.segments().iter().map(|x| x.len()).sum();
    assert!((len - 28.0).abs() < 1e-9, "{len}");
    // Open polyline, interactively.
    s.execute("pline", &json!({"vertices": [[0, 20], [10, 20], [10, 30]], "closed": false})).unwrap();
    s.start("break").unwrap();
    s.input(Input::Point(Vec2::new(3.0, 20.0))).unwrap();
    s.input(Input::Point(Vec2::new(10.0, 25.0))).unwrap();
    let n = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::LwPolyline(_))).count();
    assert_eq!(n, 3);
}

#[test]
fn associative_hatch_follows_boundary() {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
    let rect = s.doc().unwrap().model.last().unwrap().handle;
    s.execute("hatch", &json!({"points": [[5, 5]]})).unwrap();
    let hat = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap().handle;
    // Clicking inside the hatch (away from its boundary) picks it.
    let d = s.doc().unwrap();
    assert_eq!(crate::select::pick(d, &cadcraft_doc::Space::Model, Vec2::new(5.0, 5.0), 0.1), Some(hat));
    // Stretch a corner of the rectangle: the hatch follows.
    s.execute("grip.move", &json!({"handle": rect.hex(), "index": 2, "to": [20, 10]})).unwrap();
    let EntityKind::Hatch(h) = &s.doc().unwrap().entity(hat).unwrap().kind else { panic!() };
    let maxx = h.loops[0].vertices.iter().map(|v| v.p.x).fold(f64::MIN, f64::max);
    assert!((maxx - 20.0).abs() < 1e-6, "{maxx}");
    // Lines boundary (traced) follow too, after MOVE of everything.
    let mut s = Session::new();
    for (a, b) in [([0, 0], [10, 0]), ([10, 0], [10, 10]), ([10, 10], [0, 10]), ([0, 10], [0, 0])] {
        s.execute("line", &json!({"points": [a, b]})).unwrap();
    }
    s.execute("hatch", &json!({"points": [[5, 5]]})).unwrap();
    let hat = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap().handle;
    let lines: Vec<String> = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).map(|e| e.handle.hex()).collect();
    s.execute("move", &json!({"handles": lines, "from": [0, 0], "to": [100, 0]})).unwrap();
    let EntityKind::Hatch(h) = &s.doc().unwrap().entity(hat).unwrap().kind else { panic!() };
    let minx = h.loops[0].vertices.iter().map(|v| v.p.x).fold(f64::MAX, f64::min);
    assert!((minx - 100.0).abs() < 1e-6, "{minx}");
    // Generate boundary.
    s.execute("hatchgenerateboundary", &json!({"handles": [hat.hex()]})).unwrap();
    assert!(matches!(last(&s), EntityKind::LwPolyline(_)));
}

/// Đợt 1 (#11): the short names engineers type every day all start a command.
#[test]
fn common_aliases_resolve() {
    let common = [
        "l",
        "c",
        "a",
        "rec",
        "pl",
        "pol",
        "el",
        "spl",
        "xl",
        "h",
        "bh",
        "t",
        "dt",
        "mt",
        "tr",
        "ex",
        "o",
        "f",
        "cha",
        "co",
        "cp",
        "m",
        "ro",
        "sc",
        "mi",
        "e",
        "br",
        "j",
        "len",
        "s",
        "ar",
        "x",
        "di",
        "aa",
        "ma",
        "pu",
        "st",
        "d",
        "le",
        "mld",
        "tb",
        "i",
        "b",
        "w",
        "z",
        "p",
        "re",
        "ed",
        "pe",
        "div",
        "me",
        "po",
        "do",
        "bo",
        "g",
        "lt",
        "lw",
        "pr",
        "ch",
        "mo",
        "la",
        "dli",
        "dal",
        "dan",
        "dra",
        "ddi",
        "dor",
        "dco",
        "dba",
        "dim",
        "al",
        "un",
        "lts",
        "os",
        "ds",
        "att",
        "ate",
        "be",
        "xa",
        "im",
        "revcloud",
        "dimlinear",
        "dimaligned",
        "mleader",
        "purge",
        "hatchedit",
        "he",
        "li",
        "id",
        "u",
        "qdim",
        "rea",
        "ra",
    ];
    let missing: Vec<&str> = common.iter().copied().filter(|a| crate::cmd::find_command(&crate::cmd::resolve_alias(a)).is_none()).collect();
    assert!(missing.is_empty(), "aliases without a command: {missing:?}");
    let mut s = Session::new();
    s.cmdline("REC").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("rectang"));
}

/// Đợt 1 (#13): Escape after picking areas keeps the hatch (AutoCAD behaviour); Space/Enter too.
#[test]
fn hatch_kept_on_escape_and_enter() {
    for finish in ["esc", "enter"] {
        let mut s = Session::new();
        s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
        let n = count(&s);
        s.start("hatch").unwrap();
        s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
        if finish == "esc" {
            s.cancel();
        } else {
            s.cmdline("").unwrap();
        }
        assert!(s.running.is_none(), "{finish}");
        assert_eq!(count(&s), n + 1, "{finish}: hatch kept");
        assert!(s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::Hatch(_))));
    }
    // Escape before any area is picked still cancels without leaving anything.
    let mut s = Session::new();
    s.start("hatch").unwrap();
    s.cancel();
    assert_eq!(count(&s), 0);
}

/// Đợt 1 (#12): hovering the rim of a circle offers its centre; prompts that pick an object
/// (TRIM, OFFSET…) are recognised so the UI shows a pickbox and turns running snaps off.
#[test]
fn circle_center_from_rim_and_pick_prompts() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 10})).unwrap();
    let d = s.doc().unwrap().clone();
    let hit = crate::snap::osnap(&d, &cadcraft_doc::Space::Model, Vec2::new(7.0, 7.2), 0.5, crate::snap::mode::CEN, None).unwrap();
    assert_eq!(hit.point, Vec2::new(0.0, 0.0));
    let q = crate::snap::osnap(&d, &cadcraft_doc::Space::Model, Vec2::new(0.1, 9.9), 0.5, 4351, None).unwrap();
    assert_eq!(q.name, "Quadrant");
    s.start("trim").unwrap();
    while s.current_prompt().is_some_and(|p| p.accept.select) {
        s.input(Input::Enter).unwrap();
    }
    assert!(s.current_prompt().unwrap().picks_object());
    s.cancel();
    s.start("line").unwrap();
    assert!(!s.current_prompt().unwrap().picks_object());
}

/// Đợt 1 (#11): AR → ARRAY asks the type and hands over to ARRAYRECT etc.
#[test]
fn array_classic_entry() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.cmdline("AR").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("array"));
    s.cmdline("R").unwrap();
    assert!(s.running.is_some());
    assert_ne!(s.current_prompt().unwrap().message, "Enter array type");
    s.cancel();
}

/// Đợt 1 (#11): a Telex keyboard left on (Unikey) turns `EX` into `Ẽ`; the command still starts.
#[test]
fn telex_typed_command_names() {
    assert_eq!(crate::cmd::round3::detelex("ẼPLODE").as_deref(), Some("EXPLODE"));
    assert_eq!(crate::cmd::round3::detelex("Đ").as_deref(), Some("DD"));
    let mut s = Session::new();
    s.cmdline("Ẽ").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("extend"));
    s.cancel();
    s.cmdline("ẽplode").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("explode"));
}

/// Đợt 2 (#4): TK inside MOVE and COPY — base and second point from tracking points.
#[test]
fn tk_in_move_and_copy() {
    for cmd in ["move", "copy"] {
        let mut s = Session::new();
        s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
        let h = s.doc().unwrap().model.last().unwrap().handle;
        s.set_selection(vec![h]);
        s.cmdline(cmd).unwrap();
        // Base point: track from (10,0), then 5 up.
        s.cmdline("tk").unwrap();
        s.input(Input::Point(Vec2::new(10.0, 0.0))).unwrap();
        s.cmdline("@0,5").unwrap();
        s.input(Input::Enter).unwrap();
        // Second point via TK too.
        s.cmdline("tk").unwrap();
        s.input(Input::Point(Vec2::new(20.0, 5.0))).unwrap();
        s.input(Input::Enter).unwrap();
        if s.running.is_some() {
            s.input(Input::Enter).unwrap();
        }
        let EntityKind::Circle(c) = last(&s) else { panic!("{cmd}") };
        assert!(
            (c.center.x - 10.0).abs() < 1e-9 && c.center.y.abs() < 1e-9,
            "{cmd}: {:?} log {:?}",
            c.center,
            s.log.iter().rev().take(8).collect::<Vec<_>>()
        );
        assert_eq!(count(&s), if cmd == "copy" { 2 } else { 1 });
    }
}

/// Đợt 2 (#4, #17): typed one-time snaps and a distance along an object-snap tracking line.
#[test]
fn one_shot_snaps_and_tracking_distance() {
    let mut s = Session::new();
    s.start("line").unwrap();
    s.cmdline("mid").unwrap();
    assert_eq!(s.osnap_once, Some(crate::snap::mode::MID));
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert_eq!(s.osnap_once, None);
    s.cmdline("non").unwrap();
    assert_eq!(s.osnap_once, Some(0));
    s.track_line = Some((Vec2::new(14.0, 2.0), Vec2::new(-1.0, 0.0)));
    s.cmdline("3").unwrap();
    s.input(Input::Enter).unwrap();
    let EntityKind::Line(l) = last(&s) else { panic!("no line") };
    assert!(l.b.xy().near(Vec2::new(11.0, 2.0), 1e-9), "{:?}", l.b);
}

/// Đợt 3 (#10): MLEADER Options set the arrowhead size and an overall scale; Properties scale it.
#[test]
fn mleader_arrow_size_and_scale() {
    let mut s = Session::new();
    s.start("mleader").unwrap();
    s.cmdline("O").unwrap();
    s.cmdline("A").unwrap();
    s.cmdline("0.5").unwrap();
    s.cmdline("C").unwrap();
    s.cmdline("2").unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(3.0, 2.0))).unwrap();
    s.cmdline("Ghi chú").unwrap();
    let EntityKind::MLeader(m) = last(&s) else { panic!("no mleader: {:?}", s.log) };
    assert!((m.arrow_size - 1.0).abs() < 1e-9, "{}", m.arrow_size);
    let h0 = m.text.as_ref().unwrap().height;
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("properties.set", &json!({"handles": [h], "scaleBy": 0.5, "text": "Mới"})).unwrap();
    let EntityKind::MLeader(m) = last(&s) else { panic!() };
    assert!((m.arrow_size - 0.5).abs() < 1e-9);
    let t = m.text.unwrap();
    assert!((t.height - h0 * 0.5).abs() < 1e-9);
    assert_eq!(t.contents, "Mới");
}

/// Đợt 3 (#5): clicking a multileader's text picks it (double-click then edits the text).
#[test]
fn mleader_picked_by_its_text() {
    let mut s = Session::new();
    let h = s.execute("mleader", &json!({"points": [[2, -1], [4, -2]], "text": "GHI CHU"})).unwrap();
    let h = cadcraft_doc::Handle::parse_hex(h["handle"].as_str().unwrap()).unwrap();
    let EntityKind::MLeader(m) = last(&s) else { panic!() };
    let t = m.text.unwrap();
    let probe = t.insert.xy() + Vec2::new(t.height * 2.0, 0.0);
    assert_eq!(crate::select::pick(s.doc().unwrap(), &cadcraft_doc::Space::Model, probe, 0.02), Some(h), "{probe:?}");
}

/// Đợt 4 (#19): a layout whose title block sits far from the origin opens on its overall
/// viewport (id 1), not on an empty sheet; a drawing opens at its saved model view.
#[test]
fn layout_opens_on_its_contents_and_saved_model_view() {
    let mut d = cadcraft_doc::Drawing::default();
    d.layouts.push(cadcraft_doc::Layout::new("DRAWINGS", 1));
    let sp = cadcraft_doc::Space::Paper("DRAWINGS".into());
    let k = |v: serde_json::Value| -> EntityKind { serde_json::from_value(v).unwrap() };
    let vp = k(json!({"type": "viewport", "id": 1, "center": {"x": 3_335_000.0, "y": 5_630_000.0, "z": 0.0}, "width": 4800.0, "height": 1800.0,
        "viewCenter": {"x": 3_335_000.0, "y": 5_630_000.0}, "viewHeight": 1800.0, "frozenLayers": [], "locked": false}));
    d.add(&sp, Default::default(), vp).unwrap();
    let line = |a: [f64; 2], b: [f64; 2]| k(json!({"type": "line", "a": {"x": a[0], "y": a[1], "z": 0.0}, "b": {"x": b[0], "y": b[1], "z": 0.0}}));
    d.add(&sp, Default::default(), line([3_333_000.0, 5_629_500.0], [3_337_000.0, 5_631_000.0])).unwrap();
    d.add(&cadcraft_doc::Space::Model, Default::default(), line([0.0, 0.0], [1000.0, 1000.0])).unwrap();
    d.header.set("VNC_ACTIVEVIEW", cadcraft_doc::HVal::Point(cadcraft_geom::Vec3::new(500.0, 500.0, 20.0)));
    let mut s = Session::new();
    s.open_drawing(d, "t", None);
    let v = s.state().unwrap().view();
    assert!((v.center.x - 500.0).abs() < 1e-9 && (v.height - 20.0).abs() < 1e-9, "{v:?}");
    s.execute("layout.set", &json!({"name": "DRAWINGS"})).unwrap();
    let v = s.state().unwrap().view();
    assert!((v.center.x - 3_335_000.0).abs() < 1.0, "{v:?}");
}

/// Đợt 4 (#9): viewport properties — lock, scale, view centre; VPLOCK / VPSCALE work on the
/// viewport being worked in.
#[test]
fn viewport_properties_lock_and_scale() {
    let mut s = Session::new();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.execute("mview", &json!({"p1": [1, 1], "p2": [9, 6]})).unwrap();
    let vp = s
        .doc()
        .unwrap()
        .space(&cadcraft_doc::Space::Paper("Layout1".into()))
        .unwrap()
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.id != 1))
        .last()
        .map(|e| e.handle)
        .unwrap();
    let get = |s: &Session| match &s.doc().unwrap().entity(vp).unwrap().kind {
        EntityKind::Viewport(v) => v.clone(),
        _ => panic!(),
    };
    s.execute("vpprops", &json!({"handles": [vp.hex()], "scale": "1:500", "viewCenter": [100, 200]})).unwrap();
    let v = get(&s);
    assert!((v.view_height / v.height - 500.0).abs() < 1e-9 && v.view_center == Vec2::new(100.0, 200.0), "{v:?} {:?}", s.log);
    s.execute("mspace", &json!({"handle": vp.hex()})).unwrap();
    s.cmdline("vplock").unwrap();
    s.cmdline("ON").unwrap();
    assert!(get(&s).locked);
    s.cmdline("vpscale").unwrap();
    s.cmdline("1:100").unwrap();
    let v = get(&s);
    assert!((v.view_height / v.height - 100.0).abs() < 1e-9, "{}", v.view_height / v.height);
    assert_eq!(crate::cmd::round3::parse_vp_scale(&json!("1/250")), Some(250.0));
    assert_eq!(crate::cmd::round3::parse_vp_scale(&json!("abc")), None);
}

/// Every command id is unique (a duplicate silently hides the later one).
#[test]
fn command_ids_are_unique() {
    let mut seen = std::collections::HashSet::new();
    let dups: Vec<&str> = crate::cmd::command_specs().iter().map(|c| c.id).filter(|id| !seen.insert(*id)).collect();
    assert!(dups.is_empty(), "duplicate command ids: {dups:?}");
}

/// Đợt 5 (#15): PURGE lists purgeable items by category and purges only those chosen; nested
/// purging follows blocks only an unused block referenced.
#[test]
fn purge_list_selective_and_nested() {
    let mut s = Session::new();
    s.execute("layer", &json!({"name": "UNUSED_L"})).unwrap_or_default();
    s.execute("layer", &json!({"op": "new", "name": "UNUSED_L"})).unwrap_or_default();
    s.execute("style", &json!({"name": "TS_UNUSED", "font": "Arial", "current": false})).unwrap();
    s.execute("dimstyle", &json!({"name": "DS_UNUSED", "copyFrom": "Standard", "current": false})).unwrap();
    // Block INNER used only by block OUTER, which nothing uses.
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("block", &json!({"name": "INNER", "base": [0, 0], "handles": [c], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "INNER", "at": [0, 0]})).unwrap();
    let i = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("block", &json!({"name": "OUTER", "base": [0, 0], "handles": [i], "keep": "delete"})).unwrap();
    let list = s.execute("purge", &json!({"list": true})).unwrap();
    let names = |k: &str, l: &serde_json::Value| l[k].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(names("textStyles", &list).contains(&"TS_UNUSED".to_string()), "{list}");
    assert!(names("dimStyles", &list).contains(&"DS_UNUSED".to_string()), "{list}");
    assert!(names("blocks", &list).contains(&"OUTER".to_string()) && !names("blocks", &list).contains(&"INNER".to_string()), "{list}");
    // Only the text style.
    s.execute("purge", &json!({"items": {"textStyles": ["TS_UNUSED"]}})).unwrap();
    let list = s.execute("purge", &json!({"list": true})).unwrap();
    assert!(!names("textStyles", &list).contains(&"TS_UNUSED".to_string()));
    assert!(names("dimStyles", &list).contains(&"DS_UNUSED".to_string()));
    // Nested: OUTER then INNER.
    s.execute("purge", &json!({"items": {"blocks": ["OUTER"]}, "nested": true})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("OUTER").is_none() && d.block("INNER").is_none());
}

/// Đợt 5 (#16): hatch area with an island taken out.
#[test]
fn hatch_area_with_island() {
    let sq = |a: f64, b: f64| -> cadcraft_doc::HatchLoop {
        let vs: Vec<serde_json::Value> = [[a, a], [b, a], [b, b], [a, b]]
            .iter()
            .map(|p| json!({"p": {"x": p[0], "y": p[1]}, "bulge": 0.0, "start_width": 0.0, "end_width": 0.0}))
            .collect();
        serde_json::from_value(json!({"vertices": vs, "outer": false})).unwrap()
    };
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
    s.start("hatch").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    s.input(Input::Enter).unwrap();
    let EntityKind::Hatch(mut h) = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap().kind.clone() else {
        panic!()
    };
    let (a, p) = crate::cmd::round3::hatch_area(&h);
    assert!((a - 100.0).abs() < 1e-6 && (p - 40.0).abs() < 1e-6, "{a} {p}");
    h.loops = vec![sq(0.0, 10.0), sq(2.0, 4.0)];
    let (a, _) = crate::cmd::round3::hatch_area(&h);
    assert!((a - 96.0).abs() < 1e-6, "{a}");
}

/// KMZ: export with VN-2000 and import back lands on the same drawing coordinates.
#[test]
fn kmz_export_import_round_trip() {
    let mut s = Session::new();
    s.execute("geocrs", &json!({"crs": "VN2000 Đà Nẵng"})).unwrap();
    s.execute("line", &json!({"points": [[535000.0, 1778000.0], [535100.0, 1778050.0]]})).unwrap();
    s.execute("rectang", &json!({"p1": [535010.0, 1778010.0], "p2": [535030.0, 1778020.0]})).unwrap();
    s.execute("hatch", &json!({"points": [[535020.0, 1778015.0]]})).unwrap_or_default();
    s.execute("text", &json!({"at": [535050.0, 1778060.0], "text": "Nhà máy A", "height": 2.0})).unwrap();
    let r = s.execute("exportkmz", &json!({})).unwrap();
    let data = r["data"].as_str().unwrap().to_string();
    let mut t = Session::new();
    t.execute("geocrs", &json!({"crs": "VN2000 107.75 3"})).unwrap();
    let r = t.execute("importkml", &json!({"data": data})).unwrap();
    assert!(r["count"].as_u64().unwrap() >= 3, "{r}");
    let d = t.doc().unwrap();
    let near = |q: Vec2| {
        d.model.iter().any(|e| match &e.kind {
            EntityKind::LwPolyline(p) => p.vertices.iter().any(|v| v.p.dist(q) < 0.01),
            EntityKind::Point(p) => p.p.xy().dist(q) < 0.01,
            _ => false,
        })
    };
    assert!(near(Vec2::new(535000.0, 1778000.0)) && near(Vec2::new(535100.0, 1778050.0)));
    assert!(near(Vec2::new(535030.0, 1778020.0)));
    assert!(d.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(x) if x.value == "Nhà máy A")));
    assert!(d.layers.iter().any(|l| l.name.starts_with("KML-")));
    // No coordinate system: a clear error, never a panic.
    let mut u = Session::new();
    assert!(u.execute("exportkmz", &json!({})).is_err());
}

/// EXPORTKMZ typed: asks the coordinate system when the drawing has none, then hands the file
/// to the UI to save.
#[test]
fn exportkmz_interactive_asks_crs() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[535000.0, 1778000.0], [535100.0, 1778050.0]]})).unwrap();
    s.cmdline("xuatkmz").unwrap();
    assert!(s.current_prompt().unwrap().message.contains("hệ tọa độ") || s.current_prompt().unwrap().message.contains("Hệ tọa độ"));
    s.cmdline("VN2000 Đà Nẵng").unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "{:?}", s.log);
    assert!(s.ui_requests.iter().any(|(id, p)| id == "ui.savebytes" && p["name"].as_str().is_some_and(|n| n.ends_with(".kmz"))));
    assert_eq!(s.doc().unwrap().header.str("VNC_CRS", ""), "VN2000 107.75 3");
}
