use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec2;
use serde_json::json;

use super::*;
use crate::Session;

fn ev(s: &mut Session, src: &str) -> String {
    let v = eval_quiet(s, src).unwrap_or_else(|e| panic!("{src}: {e}"));
    to_text(&v, true, &s.lisp.sets)
}

#[test]
fn core_language() {
    let mut s = Session::new();
    assert_eq!(ev(&mut s, "(+ 1 2 3)"), "6");
    assert_eq!(ev(&mut s, "(/ 7 2)"), "3");
    assert_eq!(ev(&mut s, "(/ 7 2.0)"), "3.5");
    assert_eq!(ev(&mut s, "(* 2 (- 5 3) 1.5)"), "6.0");
    assert_eq!(ev(&mut s, "(sqrt 2)"), "1.41421");
    assert_eq!(ev(&mut s, "(setq a 10 b '(1 2 3))"), "(1 2 3)");
    assert_eq!(ev(&mut s, "(mapcar '(lambda (x) (* x a)) b)"), "(10 20 30)");
    assert_eq!(ev(&mut s, "(mapcar '+ '(1 2) '(10 20))"), "(11 22)");
    assert_eq!(ev(&mut s, "(defun fact (n) (if (<= n 1) 1 (* n (fact (1- n))))) (fact 10)"), "3628800");
    assert_eq!(ev(&mut s, "(setq tong 0) (foreach x b (setq tong (+ tong x))) tong"), "6");
    assert_eq!(ev(&mut s, "(cond ((= a 5) \"nam\") ((= a 10) \"muoi\") (t \"khac\"))"), "\"muoi\"");
    assert_eq!(ev(&mut s, "(strcat \"Km\" (itoa 1) \"+\" (rtos 250.5 2 2))"), "\"Km1+250.50\"");
    assert_eq!(ev(&mut s, "(substr \"Cầu Sông Hàn\" 5 4)"), "\"Sông\"");
    assert_eq!(ev(&mut s, "(strcase \"tim duong\")"), "\"TIM DUONG\"");
    assert_eq!(ev(&mut s, "(assoc 8 '((0 . \"LINE\") (8 . \"TIM\")))"), "(8 . \"TIM\")");
    assert_eq!(ev(&mut s, "(cdr (assoc 8 '((0 . \"LINE\") (8 . \"TIM\"))))"), "\"TIM\"");
    assert_eq!(ev(&mut s, "(wcmatch \"MONG-M1\" \"MONG*,COC*\")"), "T");
    assert_eq!(ev(&mut s, "(wcmatch \"K12\" \"K##\")"), "T");
    assert_eq!(ev(&mut s, "(vl-sort '(3 1 2) '<)"), "(1 2 3)");
    assert_eq!(ev(&mut s, "(polar '(0 0) (/ pi 2) 10)"), "(6.12323e-16 10.0 0.0)");
    assert_eq!(ev(&mut s, "(distance '(0 0) '(3 4))"), "5.0");
    assert_eq!(ev(&mut s, "(inters '(0 0) '(10 10) '(0 10) '(10 0))"), "(5.0 5.0 0.0)");
    assert_eq!(ev(&mut s, "(atof \"12.5m\")"), "12.5");
    assert_eq!(ev(&mut s, "(nth 1 '(a b c))"), "B");
    assert_eq!(ev(&mut s, "(length (append '(1) '(2 3) nil))"), "3");
    assert_eq!(ev(&mut s, "(setq i 0) (while (< i 5) (setq i (1+ i))) i"), "5");
    assert_eq!(ev(&mut s, "(repeat 3 (setq i (* i 2))) i"), "40");
    assert_eq!(ev(&mut s, "(vl-string-search \"+\" \"Km1+250\")"), "3");
    assert_eq!(ev(&mut s, "(cons 1 2)"), "(1 . 2)");
    assert!(eval_quiet(&mut s, "(car 5)").is_err());
    assert!(eval_quiet(&mut s, "(while t)").is_err(), "an endless loop stops");
    assert!(eval_quiet(&mut s, "(defun r (n) (r n)) (r 1)").is_err(), "deep recursion stops");
    assert!(eval_quiet(&mut s, "(getpoint)").is_err(), "no user input in quiet evaluation");
    // Comments, block comments, strings with escapes.
    assert_eq!(ev(&mut s, ";; ghi chu\n(+ 1 ;| khoi |; 1)"), "2");
    assert_eq!(ev(&mut s, "(strlen \"a\\\"b\")"), "3");
}

/// A routine like the ones design offices write: asks for a corner, width and height, draws a
/// closed polyline with (command).
const HCN: &str = r#"
;;; Ve hinh chu nhat
(defun c:HCN (/ p1 w h)
  (setq p1 (getpoint "\nGoc duoi trai: "))
  (initget 7)
  (setq w (getreal "\nChieu rong: "))
  (setq h (getdist p1 "\nChieu cao: "))
  (command "_.PLINE" p1 (polar p1 0 w) (list (+ (car p1) w) (+ (cadr p1) h)) (polar p1 (/ pi 2) h) "_C")
  (princ (strcat "\nDien tich = " (rtos (* w h) 2 2)))
  (princ))
"#;

#[test]
fn c_command_with_user_input() {
    let mut s = Session::new();
    let r = s.execute("appload", &json!({ "text": HCN, "name": "hcn.lsp" })).unwrap();
    assert_eq!(r["new"], json!(["hcn"]));
    s.cmdline("HCN").unwrap();
    assert!(s.running.is_some());
    assert!(s.current_prompt().unwrap().message.contains("Goc duoi trai"));
    s.cmdline("10,20").unwrap();
    s.cmdline("").unwrap(); // initget 1: Enter is refused, the prompt stays
    assert!(s.current_prompt().unwrap().message.contains("Chieu rong"));
    s.cmdline("40").unwrap();
    s.cmdline("25").unwrap();
    assert!(s.running.is_none(), "{:?}", s.current_prompt());
    let polys: Vec<_> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p.clone()) } else { None }).collect();
    assert_eq!(polys.len(), 1, "drawn once despite the replays");
    let p = &polys[0];
    assert!(p.closed && p.vertices.len() == 4);
    assert!(p.vertices[2].p.near(Vec2::new(50.0, 45.0), 1e-9));
    assert!(s.log.iter().any(|l| l.contains("Dien tich = 1000.00")));
    assert_eq!(s.log.iter().filter(|l| l.contains("Dien tich")).count(), 1, "printed once");
    // One undo step removes what the routine drew.
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::LwPolyline(_))));
}

#[test]
fn entities_selection_sets_and_pause() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("circle", &json!({ "center": [5, 5], "radius": 2 })).unwrap();
    // entmake, entget, entmod.
    assert!(!ev(&mut s, "(entmake '((0 . \"LINE\") (8 . \"TIM\") (10 0.0 10.0 0.0) (11 20.0 10.0 0.0)))").is_empty());
    assert_eq!(ev(&mut s, "(cdr (assoc 8 (entget (entlast))))"), "\"TIM\"");
    ev(&mut s, "(setq e (entget (entlast))) (entmod (subst (cons 8 \"MEP\") (assoc 8 e) e))");
    assert_eq!(ev(&mut s, "(cdr (assoc 8 (entget (entlast))))"), "\"MEP\"");
    assert!(s.doc().unwrap().layers.iter().any(|l| l.name == "MEP"));
    // ssget with a filter.
    assert_eq!(ev(&mut s, "(sslength (ssget \"X\" '((0 . \"LINE\"))))"), "2");
    assert_eq!(ev(&mut s, "(sslength (ssget \"_X\" '((0 . \"CIRCLE\"))))"), "1");
    assert_eq!(ev(&mut s, "(ssget \"X\" '((8 . \"KHONGCO\")))"), "nil");
    // getvar / setvar.
    ev(&mut s, "(setvar \"ORTHOMODE\" 1)");
    assert!(s.settings.orthomode);
    assert_eq!(ev(&mut s, "(getvar \"ORTHOMODE\")"), "1");
    // (command "LINE" pause pause "") waits for the user twice.
    s.cmdline(r#"(command "_.LINE" pause pause "")"#).unwrap();
    assert!(s.running.is_some());
    s.cmdline("100,100").unwrap();
    s.cmdline("150,100").unwrap();
    assert!(s.running.is_none());
    let n = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    assert_eq!(n, 3);
    // An expression answers another command's prompt.
    s.start("circle").unwrap();
    s.cmdline("(list 300 300)").unwrap();
    s.cmdline("(* 2 5)").unwrap();
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::Circle(c) if (c.radius - 10.0).abs() < 1e-9)));
    // !var prints a variable.
    ev(&mut s, "(setq ten \"VNCCad\")");
    s.cmdline("!ten").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("\"VNCCad\""));
}

#[test]
fn entsel_and_errors() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let src = r#"(defun c:DOILOP (/ e) (setq e (car (entsel "\nChon doi tuong: "))) (if e (progn (entmod (subst (cons 8 "DIM") (assoc 8 (entget e)) (entget e))) (princ "\nDa doi lop")) (princ "\nKhong chon")) (princ))"#;
    s.execute("appload", &json!({ "text": src, "name": "doilop.lsp" })).unwrap();
    s.cmdline("doilop").unwrap();
    s.cmdline("5,0").unwrap();
    assert!(s.running.is_none());
    let line = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).cloned().unwrap();
    assert_eq!(line.common.layer, "DIM");
    // *error* runs on an error.
    let src = r#"(defun *error* (m) (princ (strcat "\nLoi bat duoc: " m))) (defun c:LOI () (car 1))"#;
    s.execute("appload", &json!({ "text": src })).unwrap();
    s.cmdline("loi").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Loi bat duoc")));
    assert!(s.running.is_none());
    // Syntax errors are reported, not panics.
    assert!(s.execute("appload", &json!({ "text": "(defun c:x ( (" })).is_err());
}

#[test]
fn scripts_run_lines() {
    let mut s = Session::new();
    s.execute("script", &json!({ "text": "LINE 0,0 10,0 10,10 \n(setq k 5)\nCIRCLE 0,0 (* k 2)\n" })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 2);
    assert!(d.model.iter().any(|e| matches!(&e.kind, EntityKind::Circle(c) if (c.radius - 10.0).abs() < 1e-9)));
}

#[test]
fn text_from_lisp_is_one_line() {
    let mut s = Session::new();
    ev(&mut s, "(command \"_.TEXT\" '(0 0) 2.5 0 \"C1-1\") (command \"_.CIRCLE\" '(10 10) 4)");
    let d = s.doc().unwrap();
    assert!(s.running.is_none());
    let texts: Vec<String> = d.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
    assert_eq!(texts, vec!["C1-1".to_string()]);
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Circle(_))));
}

#[test]
fn reals_print_like_autolisp() {
    assert_eq!(fmt_real(1.23456789), "1.23457");
    assert_eq!(fmt_real(1000.0), "1000.0");
    assert_eq!(fmt_real(0.5), "0.5");
    assert_eq!(fmt_real(-2.25), "-2.25");
    assert_eq!(fmt_real(123456.789), "123457.0");
}

#[test]
fn vlax_curve_and_vla_properties() {
    let mut s = Session::new();
    // A road centreline: 100 straight, then a 90° arc of radius 50 (bulge tan(22.5°)).
    s.execute("pline", &json!({ "points": [[0, 0], [100, 0], [150, 50]] })).unwrap();
    let h = s.doc().unwrap().model.last().unwrap().handle;
    let b = (22.5f64).to_radians().tan();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::LwPolyline(p) = &mut e.kind {
                p.vertices[1].bulge = b;
            }
        })
        .unwrap();
    ev(&mut s, "(setq tim (vlax-ename->vla-object (entlast)))");
    let len = 100.0 + 50.0 * std::f64::consts::FRAC_PI_2;
    let got: f64 = ev(&mut s, "(vlax-curve-getDistAtParam tim (vlax-curve-getEndParam tim))").parse().unwrap();
    assert!((got - len).abs() < 1e-3, "{got}");
    assert_eq!(ev(&mut s, "(vlax-curve-getEndParam tim)"), "2.0");
    // Km0+050 lies on the straight; Km0+120 on the arc.
    assert_eq!(ev(&mut s, "(vlax-curve-getPointAtDist tim 50.0)"), "(50.0 0.0 0.0)");
    let p = ev(&mut s, "(setq p (vlax-curve-getPointAtDist tim 120.0))");
    assert!(p.starts_with('('));
    let d: f64 = ev(&mut s, "(vlax-curve-getDistAtPoint tim p)").parse().unwrap();
    assert!((d - 120.0).abs() < 1e-3, "{d}");
    // Closest point and offset distance of a point beside the road.
    assert_eq!(ev(&mut s, "(vlax-curve-getClosestPointTo tim '(30 7))"), "(30.0 0.0 0.0)");
    assert_eq!(ev(&mut s, "(vlax-curve-getStartPoint tim)"), "(0.0 0.0 0.0)");
    assert_eq!(ev(&mut s, "(vlax-curve-isClosed tim)"), "nil");
    // Tangent on the straight.
    assert_eq!(ev(&mut s, "(vlax-curve-getFirstDeriv tim 0.5)"), "(100.0 0.0 0.0)");
    // Properties.
    assert!((ev(&mut s, "(vla-get-Length tim)").parse::<f64>().unwrap() - len).abs() < 1e-3);
    ev(&mut s, "(vla-put-Layer tim \"TIM-TUYEN\") (vla-put-Color tim 1)");
    assert_eq!(ev(&mut s, "(vla-get-Layer tim)"), "\"TIM-TUYEN\"");
    assert_eq!(ev(&mut s, "(vlax-get tim 'Color)"), "1");
    assert_eq!(ev(&mut s, "(vla-get-ObjectName tim)"), "\"AcDbPolyline\"");
    // Adding objects to model space.
    ev(&mut s, "(setq ms (vla-get-ModelSpace (vla-get-ActiveDocument (vlax-get-acad-object))))");
    ev(&mut s, "(setq c (vla-AddCircle ms (vlax-3d-point '(5 5)) 2.0))");
    assert_eq!(ev(&mut s, "(vla-get-Radius c)"), "2.0");
    ev(&mut s, "(setq tx (vla-AddText ms \"Km0+120\" (vlax-3d-point p) 2.5))");
    assert_eq!(ev(&mut s, "(vla-get-TextString tx)"), "\"Km0+120\"");
    // A circle's curve functions.
    assert!((ev(&mut s, "(vlax-curve-getArea c)").parse::<f64>().unwrap() - 4.0 * std::f64::consts::PI).abs() < 1e-3);
    assert!(eval_quiet(&mut s, "(vla-get-Radius tim)").is_err(), "a polyline has no radius");
    assert!(eval_quiet(&mut s, "(vla-SomethingElse tim)").is_err());
}

const COC_DCL: &str = r#"
// Hộp thoại chọn cọc
coc : dialog {
  label = "Bố trí cọc";
  : edit_box { key = "kc"; label = "Khoảng cách"; value = "3"; }
  : popup_list { key = "loai"; label = "Loại cọc"; }
  : toggle { key = "ghi"; label = "Ghi số hiệu"; value = "1"; }
  : radio_row { key = "kieu"; : radio_button { key = "tron"; value = "1"; } : radio_button { key = "vuong"; } }
  : text { key = "msg"; }
  ok_cancel;
}
"#;

const COC_LSP: &str = r#"
(defun c:coc (/ id st)
  (setq id (load_dialog "coc.dcl"))
  (new_dialog "coc" id)
  (start_list "loai") (mapcar 'add_list '("D300" "D400" "D500")) (end_list)
  (set_tile "loai" "1")
  (setq kieu0 (get_tile "kieu"))
  (action_tile "ghi" "(set_tile \"msg\" (strcat \"ghi=\" $value))")
  (action_tile "accept" "(setq kc (atof (get_tile \"kc\")) loai (atoi (get_tile \"loai\"))) (done_dialog 1)")
  (setq st (start_dialog))
  (unload_dialog id)
  (setq ketqua st)
  (princ))
"#;

#[test]
fn dcl_dialog_round_trip() {
    let mut s = Session::new();
    s.execute("open", &json!({ "data": crate::cmd::file::base64_encode(COC_DCL.as_bytes()), "name": "coc.dcl" })).unwrap();
    s.execute("appload", &json!({ "text": COC_LSP, "name": "coc.lsp" })).unwrap();
    s.cmdline("COC").unwrap();
    let p = s.current_prompt().expect("dialog prompt");
    let d = p.dialog.expect("dialog");
    assert_eq!(d.lists.get("loai").map(Vec::len), Some(3));
    assert_eq!(d.values.get("loai").map(String::as_str), Some("1"));
    assert_eq!(d.values.get("kc").map(String::as_str), Some("3"));
    assert_eq!(d.dialog.children.last().map(|t| t.kind.as_str()), Some("ok_cancel"));
    // A tile with an action: the action runs and the dialog stays up with the new values.
    s.input(crate::Input::Text(json!({"pressed":"ghi","values":{"ghi":"0"}}).to_string())).unwrap();
    let d = s.current_prompt().and_then(|p| p.dialog).expect("still open");
    assert_eq!(d.values.get("msg").map(String::as_str), Some("ghi=0"));
    assert_eq!(d.round, 1);
    s.input(crate::Input::Text(json!({"pressed":"accept","values":{"kc":"4.5","loai":"2","ghi":"0"}}).to_string())).unwrap();
    assert!(s.running.is_none(), "{:?}", s.current_prompt());
    assert_eq!(ev(&mut s, "kc"), "4.5");
    assert_eq!(ev(&mut s, "loai"), "2");
    assert_eq!(ev(&mut s, "ketqua"), "1");
    assert_eq!(ev(&mut s, "kieu0"), "\"tron\"");
    // Cancel: status 0, the accept action does not run.
    s.cmdline("COC").unwrap();
    s.input(crate::Input::Text(json!({"pressed":"cancel","values":{}}).to_string())).unwrap();
    assert_eq!(ev(&mut s, "ketqua"), "0");
}

#[test]
fn reactors_follow_commands_and_objects() {
    let mut s = Session::new();
    let src = r#"
(setq log nil)
(defun ghi (r args) (setq log (cons (list (vlr-current-reaction-name) (car args)) log)))
(setq rc (vlr-command-reactor "du lieu" '((:vlr-commandWillStart . ghi) (:vlr-commandEnded . ghi) (:vlr-commandCancelled . ghi))))
(setq rl (vlr-lisp-reactor nil '((:vlr-lispEnded . ghi))))
(defun c:noop () (princ))
(defun suadoi (obj r p) (setq sua (cons (cdr (assoc 0 (entget obj))) sua)))
(defun xoa (obj r p) (setq daxoa T))
"#;
    s.execute("appload", &json!({ "text": src, "name": "r.lsp" })).unwrap();
    assert_eq!(ev(&mut s, "(vlr-data rc)"), "\"du lieu\"");
    assert_eq!(ev(&mut s, "(vlr-type rc)"), ":VLR-COMMAND-REACTOR");
    s.cmdline("LINE").unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(ev(&mut s, "(reverse log)"), "((:VLR-COMMANDWILLSTART \"LINE\") (:VLR-COMMANDENDED \"LINE\"))");
    // An object reactor on the new line: MOVE modifies it, ERASE erases it.
    ev(&mut s, "(setq ro (vlr-object-reactor (list (entlast)) nil '((:vlr-modified . suadoi) (:vlr-erased . xoa))))");
    ev(&mut s, "(setq log nil)");
    s.cmdline("NOOP").unwrap();
    assert_eq!(ev(&mut s, "log"), "((:VLR-LISPENDED \"(C:NOOP)\"))");
    let h = s.doc().unwrap().model.iter().last().unwrap().handle;
    s.set_selection(vec![h]);
    s.cmdline("MOVE").unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("5,5").unwrap();
    assert_eq!(ev(&mut s, "sua"), "(\"LINE\")");
    // Removed reactors stay quiet.
    ev(&mut s, "(vlr-remove rc)");
    ev(&mut s, "(setq log nil)");
    s.cmdline("REGEN").unwrap();
    assert_eq!(ev(&mut s, "log"), "nil");
    s.set_selection(vec![h]);
    s.cmdline("ERASE").unwrap();
    assert_eq!(ev(&mut s, "daxoa"), "T");
    assert_eq!(ev(&mut s, "(length (vlr-reactors))"), "2");
}

/// A small AutoCAD slide (format level 2, little-endian): a square from four vectors, the last
/// three as common-endpoint records, in colour 1, then a fill.
fn sample_slide() -> Vec<u8> {
    let mut b = b"AutoCAD Slide\r\n\x1a\0".to_vec();
    b.push(0x56);
    b.push(2);
    b.extend_from_slice(&100u16.to_le_bytes());
    b.extend_from_slice(&100u16.to_le_bytes());
    b.extend_from_slice(&10_000_000u32.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&0x1234u16.to_le_bytes());
    let w = |b: &mut Vec<u8>, v: u16| b.extend_from_slice(&v.to_le_bytes());
    w(&mut b, 0xFF01); // colour 1
    for v in [10u16, 10, 90, 10] {
        w(&mut b, v); // vector (10,10)-(90,10)
    }
    // From the from-point (10,10): +80 up, then common-endpoint steps.
    b.extend_from_slice(&[0, 0xFE, 80]); // dx 0, dy +80
    b.extend_from_slice(&[80, 0xFE, 0]); // dx=80, dy=0
    b.extend_from_slice(&[0, 0xFE, 0xB0]); // dx=0, dy=-80
    w(&mut b, 0xFC00);
    b
}

#[test]
fn dcl_images_and_slides() {
    let mut s = Session::new();
    s.lisp.files.insert("anh.dcl".into(), "anh : dialog { : image { key = \"hinh\"; width = 20; height = 5; color = -2; } ok_only; }".into());
    s.binary_files.insert("khung.sld".into(), sample_slide());
    let src = r#"
(defun c:anh (/ id)
  (setq id (load_dialog "anh.dcl"))
  (new_dialog "anh" id)
  (setq w (dimx_tile "hinh") h (dimy_tile "hinh"))
  (start_image "hinh")
  (fill_image 0 0 w h -2)
  (vector_image 0 0 w h 1)
  (slide_image 0 0 w h "khung")
  (end_image)
  (start_dialog)
  (princ))
"#;
    s.execute("appload", &json!({ "text": src, "name": "anh.lsp" })).unwrap();
    s.cmdline("ANH").unwrap();
    let d = s.current_prompt().and_then(|p| p.dialog).expect("dialog");
    let ops = d.images.get("hinh").expect("image ops");
    assert_eq!(ev(&mut s, "(list w h)"), "(150 75)");
    assert!(matches!(ops.first(), Some(dcl::ImageOp::Fill { color: -2, .. })));
    let lines: Vec<_> = ops
        .iter()
        .filter_map(|o| if let dcl::ImageOp::Line { x1, y1, x2, y2, color } = o { Some((*x1, *y1, *x2, *y2, *color)) } else { None })
        .collect();
    assert_eq!(lines.len(), 5, "{lines:?}");
    // The slide (100 × 100) fits the 150 × 75 tile at 0.75, centred, y down: its bottom edge
    // (y = 10) lands at 75 − 7.5.
    let (x1, y1, x2, y2, c) = lines[1];
    assert_eq!(c, 1);
    assert!(
        (x1 - (37.5 + 7.5)).abs() < 1e-3 && (y1 - 67.5).abs() < 1e-3 && (x2 - (37.5 + 67.5)).abs() < 1e-3 && (y2 - 67.5).abs() < 1e-3,
        "{:?}",
        lines[1]
    );
    // Common-endpoint vectors start from the last from-point and close the square.
    let last = lines[4];
    assert!((last.2 - x2).abs() < 1e-3 && (last.3 - y2).abs() < 1e-3, "{lines:?}");
    s.cancel();
}
