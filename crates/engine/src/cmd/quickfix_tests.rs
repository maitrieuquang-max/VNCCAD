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
