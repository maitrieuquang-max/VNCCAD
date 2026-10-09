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
