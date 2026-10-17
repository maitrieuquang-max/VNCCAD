;;; VNCCad – LISP mẫu để test: nhập/xuất tọa độ cọc qua tệp CSV.
;;; Nạp: kéo-thả tệp này vào cửa sổ VNCCad (hoặc APPLOAD), rồi gõ lệnh NHAPTD / XUATTD.

;; Tách một dòng "A,B,C" thành danh sách chuỗi.
(defun vnc:tach (s / i kq)
  (setq kq nil)
  (while (setq i (vl-string-search "," s))
    (setq kq (cons (substr s 1 i) kq) s (substr s (+ i 2))))
  (reverse (cons s kq)))

;; NHAPTD: đọc CSV (TEN,X,Y), vẽ vòng tròn D600 và ghi tên cọc.
(defun c:NHAPTD (/ ten f dong p n)
  (setq ten (getfiled "Tệp tọa độ" "coc_khoan_nhoi.csv" "csv" 0))
  (if (and ten (setq f (open ten "r")))
    (progn
      (read-line f) ; bỏ dòng tiêu đề
      (setq n 0)
      (while (setq dong (read-line f))
        (setq p (vnc:tach dong))
        (if (= (length p) 3)
          (progn
            (entmake (list '(0 . "CIRCLE") (list 10 (atof (cadr p)) (atof (caddr p)) 0.0) '(40 . 300.0)))
            (entmake (list '(0 . "TEXT") (list 10 (+ (atof (cadr p)) 350) (atof (caddr p)) 0.0) '(40 . 200.0) (cons 1 (car p))))
            (setq n (1+ n)))))
      (close f)
      (princ (strcat "\nĐã nhập " (itoa n) " cọc.")))
    (princ "\nKhông mở được tệp."))
  (princ))

;; XUATTD: chọn các vòng tròn, ghi tâm ra CSV.
(defun c:XUATTD (/ ss ten f i e c)
  (princ "\nChọn các cọc (vòng tròn): ")
  (setq ss (ssget '((0 . "CIRCLE"))))
  (if ss
    (progn
      (setq ten (getfiled "Ghi ra tệp" "toado_xuat.csv" "csv" 1))
      (setq f (open ten "w"))
      (write-line "STT,X,Y,D" f)
      (setq i 0)
      (repeat (sslength ss)
        (setq e (entget (ssname ss i)) c (cdr (assoc 10 e)))
        (write-line (strcat (itoa (1+ i)) "," (rtos (car c) 2 3) "," (rtos (cadr c) 2 3) "," (rtos (* 2 (cdr (assoc 40 e))) 2 0)) f)
        (setq i (1+ i)))
      (close f)
      (princ (strcat "\nĐã ghi " (itoa i) " cọc vào " ten))))
  (princ))

;; DAYCS: kéo dây cao su bằng grread rồi vẽ đường thẳng (thử theo dõi chuột).
(defun c:DAYCS (/ p gr)
  (setq p (getpoint "\nĐiểm đầu: "))
  (if p
    (progn
      (princ "\nDi chuột để thấy dây cao su, bấm để chốt, Enter để bỏ: ")
      (while (= 5 (car (setq gr (grread t 15 0))))
        (redraw)
        (grdraw p (cadr gr) 1))
      (redraw)
      (if (= 3 (car gr))
        (entmake (list '(0 . "LINE") (cons 10 p) (cons 11 (cadr gr)))))))
  (princ))

;; LOPMAU: tạo lớp chuẩn bằng ActiveX (vla).
(defun c:LOPMAU (/ lays l)
  (vl-load-com)
  (setq lays (vla-get-layers (vla-get-activedocument (vlax-get-acad-object))))
  (foreach x '(("TIM_TUYEN" 1) ("KET_CAU" 7) ("KICH_THUOC" 3) ("COT_THEP" 4))
    (setq l (vla-add lays (car x)))
    (vla-put-color l (cadr x)))
  (princ "\nĐã tạo 4 lớp: TIM_TUYEN, KET_CAU, KICH_THUOC, COT_THEP.")
  (princ))

(princ "\nĐã nạp lisp mẫu: NHAPTD, XUATTD, DAYCS, LOPMAU.")
(princ)
