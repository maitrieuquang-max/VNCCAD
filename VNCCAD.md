# VNCCad

VNCCad là bản phát triển riêng (fork) của [CADCraft](https://github.com/storytold/cadcraft), phần mềm vẽ kỹ thuật kiểu AutoCAD viết bằng Rust. Giấy phép MIT OR Apache-2.0, giữ nguyên thông tin bản quyền gốc trong `LICENSE-*`, `NOTICE` và `ATTRIBUTION.md`.

Mục tiêu: một phần mềm CAD dùng được cho kỹ sư Việt Nam, chạy được cả **online** (trình duyệt) và **offline** (bản desktop, hoặc bản web đã cài như ứng dụng).

## Giai đoạn 1 – đã làm

| Hạng mục | Nội dung | File chính |
|---|---|---|
| Chữ tiếng Việt trong bản vẽ | Font nét vẽ có đủ 134 nguyên âm có dấu (sắc, huyền, hỏi, ngã, nặng; ă â ê ô ơ ư; đ Đ) và ký hiệu – — × ÷ ² ³ · ≤ ≥. TEXT/MTEXT/DIM hiển thị đúng trên mọi nền tảng, không cần cài font. | `crates/fonts/src/stroke.rs` |
| Giao diện tiếng Việt | Nhúng font Be Vietnam Pro (SIL OFL 1.1). Dòng lệnh, bảng thuộc tính, tên layer gõ tiếng Việt hiển thị đầy đủ, kể cả trên web. | `crates/ui-egui/src/theme.rs`, `assets/fonts/` |
| Mở file trên web | Nút Open / Ctrl+O mở hộp chọn file của trình duyệt (DXF). Kéo-thả file DXF vào cửa sổ để mở. | `apps/cadcraft-web/src/main.rs` |
| Lưu file trên web | Save / Save As tải bản vẽ về máy dạng DXF. | `crates/ui-egui/src/menus.rs` |
| Xuất PDF | Print / EXPORTPDF: trên web tải PDF về; trên desktop hỏi nơi lưu file. | `crates/ui-egui/src/menus.rs` |
| Chạy offline trên web (PWA) | Service worker lưu ứng dụng vào bộ nhớ trình duyệt sau lần mở đầu tiên. Có manifest để "Cài đặt ứng dụng" từ Chrome/Edge. | `apps/cadcraft-web/sw.js`, `manifest.webmanifest` |
| Desktop | Hộp mở file nhận cả DWG; hộp lưu có DXF, DWG, PDF, SVG, PNG. | `apps/cadcraft/src/main.rs` |
| Build tự động | GitHub Actions build bản web, Windows x64 và Linux mỗi lần push. | `.github/workflows/vnccad.yml` |
| Tự lưu + khôi phục sau sự cố | Bản vẽ có thay đổi chưa lưu được tự ghi DXF dự phòng (desktop: 2 phút/lần, web: 1 phút/lần) và khi thoát. Lần mở sau hiện hộp thoại *Khôi phục bản vẽ*: Khôi phục tất cả / Xóa bản tự lưu / Để sau. Save xong thì bản dự phòng tự xóa. | `crates/ui-egui/src/autosave.rs`, `apps/cadcraft/src/autosave_store.rs` |

### Nơi lưu bản tự lưu

- Windows: `%LOCALAPPDATA%\VNCCad\autosave`
- macOS: `~/Library/Application Support/VNCCad/autosave`
- Linux: `~/.local/share/VNCCad/autosave`
- Web: bộ nhớ trình duyệt (localStorage, khoảng 5 MB mỗi trang). Bản vẽ quá lớn sẽ không tự lưu được; chương trình báo một lần trên dòng lệnh.
- Đổi thư mục bằng biến môi trường `VNCCAD_AUTOSAVE_DIR`.

## Giai đoạn 2 – bù các chức năng 2D còn thiếu so với AutoCAD

| Hạng mục | Lệnh / cách dùng | Ghi chú |
|---|---|---|
| Bản vẽ cũ TCVN3 (ABC) và VNI | Tự chuyển khi mở file; `VNCONVERT` (ép mã `tcvn3`, `tcvn3h`, `vni` cho đối tượng chọn) | Nhận font `.VnTime`, `.VnTimeH` (chữ hoa), `VNI-Times`…, kể cả đổi font giữa dòng trong MTEXT; kiểu chữ được đổi sang Times New Roman/Arial |
| Font SHX | Tự tìm cạnh bản vẽ, thư mục `fonts/` cạnh bản vẽ, `VNCCad/fonts`, `VNCCAD_FONTS`, thư mục Fonts của AutoCAD đã cài; kéo-thả file `.shx`; `SHXFONTS` | Đọc shapes 1.0/1.1, unifont, bigfont; ký tự font thiếu (chữ Việt có dấu) lấy từ font nét có sẵn |
| Ảnh nền | `IMAGEATTACH` (PNG, JPEG, BMP, TIFF), `IMAGE` (liệt kê, báo file thiếu); kéo-thả ảnh | Có world file (`.jgw/.pgw/.tfw/.wld`) thì đặt đúng tọa độ; lưu DXF đầy đủ IMAGEDEF |
| PDF nền | `PDFATTACH` (`page`, `scale` = đơn vị bản vẽ/inch); kéo-thả PDF | Lưu DXF dạng PDFUNDERLAY + PDFDEFINITION |
| Tham chiếu ngoài (Xref) | `XATTACH`, `XREF` (`list`, `reload`, `detach`, `bind`) | Tự nạp lại khi mở; layer/block phụ thuộc đặt tên `XREF\|TÊN` và không lưu vào file |
| Cầu đường | `VNLAYERS` (bộ layer đường/cầu theo TCVN 8-20:2002), `TRACDOC`, `TRACNGANG`, `BANGKL` | Dán bảng số liệu từ Excel (tab, `;` hoặc `,`; số thập phân dấu phẩy; lý trình `Km1+250.5`) |
| Tỷ lệ chú thích | `CANNOSCALE 1:100`; kiểu chữ/kích thước bật Annotative | Trong Model vẽ theo CANNOSCALE; trong viewport theo tỷ lệ viewport |
| Bảng nét in CTB | `PLOTSTYLE monochrome`, `grayscale` hoặc `<file>.ctb`; kéo-thả `.ctb` | Áp màu, độ dày nét, độ đậm nhạt theo số màu khi PLOT/EXPORTPDF; file `.ctb` đặt cạnh bản vẽ hoặc trong `VNCCad/plotstyles` |
| In theo vùng chọn | `PLOTWINDOW` (`VUNGIN`, `PW`): chọn 2 góc khung tên → khổ giấy A4…A0 → lưu PDF; `PAGESETUP` trong Model | Dùng cho bản vẽ đặt nhiều tờ trong Model. Hướng giấy theo hình dạng vùng in; thiết lập in của Model (khổ, vùng in, bảng nét) lưu trong file như AutoCAD |

### Giai đoạn 3 – nhóm, UCS, AutoLISP

| Hạng mục | Lệnh / cách dùng | Ghi chú |
|---|---|---|
| Nhóm đối tượng | `GROUP` (`G`; tùy chọn Name/Description), `UNGROUP`, `GROUPEDIT`, `GROUPS`; `PICKSTYLE` / Ctrl+Shift+A bật-tắt chọn theo nhóm | Bấm một đối tượng chọn cả nhóm. Lưu DXF trong `ACAD_GROUP` như AutoCAD |
| Hệ tọa độ người dùng | `UCS` (gốc + điểm trục X, `Object`, `Z`, `Previous`, `World`, `Named` Save/Restore/Delete), `UCSMAN` | Tọa độ gõ `x,y`, `@dx,dy`, `@d<a` theo UCS; `*x,y` là tọa độ thế giới. Ortho, polar, lưới, snap, con trỏ, biểu tượng UCS và tọa độ thanh trạng thái theo UCS. Lưu `$UCSORG/$UCSXDIR` và bảng UCS |
| Xoay màn hình theo tuyến | `PLAN` (theo UCS hiện tại; `PLAN {"world":true}` về WCS) | Trục X của UCS nằm ngang màn hình — vẽ dọc tuyến đường, trục cầu xiên. Zoom, pan, chọn bằng cửa sổ đều theo màn hình đã xoay |
| AutoLISP | `APPLOAD` (`AP`) hoặc kéo-thả `.lsp`; gõ tên lệnh `C:` đã nạp; gõ `(biểu thức)` hoặc `!biến` ở dòng lệnh; biểu thức trả lời được cả lời nhắc của lệnh khác | Có `defun`, `setq`, `if/cond/while/repeat/foreach`, `lambda/mapcar/apply`, hàm số học, chuỗi, danh sách, `wcmatch`, `vl-string-*`, `vl-sort`…; `command` (kể cả `pause`), `getpoint/getreal/getint/getdist/getangle/getstring/getkword/initget`, `entsel`, `ssget` (kể cả `"X"` + lọc), `entget/entmod/entmake/entdel/entlast/entnext`, `getvar/setvar`, `tblsearch`, `trans`, `osnap`, `*error*`. Một lệnh LISP = một bước Undo |
| Sửa block | `BEDIT` (`BE`: chọn block hoặc gõ tên), `REFEDIT` (chọn block), `BSAVE`, `BCLOSE` (Yes/No) | Block mở trong thẻ riêng, dùng mọi lệnh vẽ/sửa; BSAVE cập nhật mọi block cùng tên trong bản vẽ gốc (một bước Undo ở bản vẽ gốc) |
| Script | `SCRIPT` hoặc kéo-thả `.scr` | Như AutoCAD: dấu cách/xuống dòng là Enter; dòng `(…)` chạy LISP |

Giới hạn LISP: đối tượng ActiveX ngoài phần đã liệt kê ở Giai đoạn 4–5 chưa có; `entmake` hỗ trợ LINE, CIRCLE, ARC, POINT, LWPOLYLINE, TEXT, MTEXT, INSERT. Lệnh LISP được chạy lại từ đầu sau mỗi lần nhập (phát lại các câu trả lời trước) — với chương trình hỏi hàng trăm lần có thể chậm.

### Giai đoạn 4 – in hàng loạt, Visual LISP, DCL

| Hạng mục | Lệnh / cách dùng | Ghi chú |
|---|---|---|
| In hàng loạt | `PUBLISH` (`INHANGLOAT`, `BATCHPLOT`): mọi layout, hoặc tự tìm khung tên (block chèn nhiều lần, lớn nhất) trong Model → một file PDF nhiều trang | Thứ tự trang trái→phải, trên→dưới; khổ mặc định A3 cho khung tên, chọn được khổ/bảng nét. `publish.sheets` xem trước danh sách tờ |
| Ảnh/PDF nền khi in | `PLOT`, `PLOTWINDOW`, `EXPORTPDF`, `PUBLISH` | Ảnh nhúng vào PDF (giữ trong suốt), nằm dưới nét vẽ, cắt theo viewport |
| Phiên bản DWG khi lưu | `DWGVERSION 2000/2004/2007/2010/2013/2018` hoặc `SAVEAS` với `version` | Mặc định AutoCAD 2007 (AC1021), mở được bằng AutoCAD 2007 trở lên. Đã kiểm tra lưu-mở lại đủ chữ Việt, chữ Trung và ô gộp của bảng ở mọi phiên bản |
| Xref lồng nhau | `XATTACH`, `XREF reload` | Theo xref bên trong xref tới 8 cấp, chặn vòng lặp; layer phụ thuộc giữ tên `A\|B\|LAYER` |
| Bảng nét in theo tên (STB) | `PLOTSTYLE <file>.stb`; kéo-thả `.stb`; layer có thuộc tính `plotStyle` | Đọc kiểu in gán cho layer trong DXF/DWG (ACAD_PLOTSTYLENAME) và ghi lại khi lưu |
| Kích thước annotative từ file | `CANNOSCALE` | Kích thước annotative đọc từ file được dựng lại theo tỷ lệ chú thích (không còn cố định theo block `*D`) |
| Chữ Trung/Nhật/Hàn trên web | `CJKFONT` | Chrome/Edge: lấy font CJK của máy (SimSun, Microsoft YaHei…) sau khi bạn cho phép. Không có font máy (hoặc trình duyệt khác): tải một lần font mã nguồn mở ArchLang CJK Sans (SIL OFL, ~11 MB, chữ Trung giản thể/phồn thể, kana Nhật, Hangul Hàn) từ CDN jsDelivr/unpkg, lưu trong bộ nhớ trình duyệt để lần sau và khi offline tự dùng. Khi mở bản vẽ có chữ CJK mà thiếu font, dòng lệnh sẽ nhắc |
| Visual LISP | `vlax-curve-*` (getStartParam/EndParam, getStartPoint/EndPoint, getDistAtParam, getParamAtDist, getPointAtDist/Param, getClosestPointTo, getFirstDeriv, getArea, isClosed…), `vla-get-*/vla-put-*`, `vlax-get-property/put-property`, `vlax-ename->vla-object`, `vlax-3d-point`, `vla-AddLine/AddCircle/AddText/AddLightWeightPolyline`, `vla-delete` | Dùng được cho line, arc, circle, polyline (tham số = chỉ số đỉnh như AutoCAD), spline/ellipse (theo xấp xỉ). Thường dùng để rải cọc, cắm cọc theo tuyến |
| Hộp thoại DCL | `load_dialog`, `new_dialog`, `set_tile/get_tile/get_attr`, `action_tile`, `mode_tile`, `start_list/add_list/end_list`, `start_dialog/done_dialog/term_dialog/unload_dialog`; nạp `.dcl` bằng kéo-thả hoặc mở file (bản desktop đọc được cả đường dẫn ghi trong `load_dialog`) | Hiện: dialog, row/column (boxed), edit_box, popup_list, list_box, toggle, radio_button, button, text, slider, ok_cancel… Tile có `action_tile` chạy ngay khi đổi giá trị, hộp thoại giữ nguyên tới `done_dialog`. Ô ảnh `image`/`image_button`: `start_image`, `fill_image`, `vector_image`, `slide_image` (đọc slide `.sld` — kéo-thả hoặc mở file; thư viện `.slb` chưa đọc), `dimx_tile/dimy_tile`; bấm `image_button` trả `$x $y` |

### Giai đoạn 5 – reactor, block động, kiểm thử trình duyệt

| Hạng mục | Lệnh / cách dùng | Ghi chú |
|---|---|---|
| Reactor AutoLISP | `vlr-command-reactor`, `vlr-lisp-reactor`, `vlr-editor-reactor`, `vlr-dwg-reactor`, `vlr-object-reactor`; `vlr-remove/add/added-p/remove-all/reactors/type/data/data-set/reactions/reaction-set/owners/owner-add/owner-remove/current-reaction-name` | Sự kiện: `:vlr-commandWillStart/Ended/Cancelled/Failed`, `:vlr-lispWillStart/Ended/Cancelled`, `:vlr-beginSave/saveComplete`, `:vlr-modified/erased` (đối tượng). Callback chạy sau lệnh, cùng bước Undo với lệnh, không được hỏi người dùng. Reactor không lưu vào file (`vlr-pers` được chấp nhận, không có tác dụng) |
| Block động | `DYNPROP` (`THUOCTINHDONG`): chọn block → đổi thuộc tính; bảng Properties có nhóm "Dynamic block" khi chọn một block động | Đọc từ file AutoCAD: tham số Linear với hành động Stretch / Move / Array, tham số Flip, tham số Visibility (trạng thái hiển thị). Đổi thuộc tính tạo block `*U` mới như AutoCAD. Đã đối chiếu với bản vẽ thật: dựng lại 8 block động (kéo giãn 1.825–6.000) có khung bao trùng khớp với hình AutoCAD đã lưu; riêng các bản lặp của hành động Array bị thiếu vì bộ đọc DWG làm mất danh sách đối tượng của hành động này |
| Kiểm thử bản web trong trình duyệt thật | Mở trang với `?selftest` (CI chạy tự động bằng Chromium) | Mở DWG có chữ Việt + Trung và DWG 60.000 đối tượng theo đúng đường người dùng mở file, chạy LISP, tải font CJK từ CDN; đo thời gian luồng chính bị chiếm để phát hiện treo trang. Kết quả trên Chromium của GitHub Actions: DWG 60.000 đối tượng mở trong 0,9 s, font CJK tải xong 1,7 s, luồng chính bị chiếm lâu nhất 1,7 s (không treo). Kết quả hiện ở mục Annotations của mỗi lần chạy |

### Giai đoạn 6 – lấp các lệnh 2D còn thiếu (mục tiêu 90% AutoCAD 2D)

| Hạng mục | Lệnh | Ghi chú |
|---|---|---|
| Kiểm tra, sửa bản vẽ | `AUDIT` (`KIEMTRA`), `RECOVER` (`PHUCHOI`, File > Drawing Utilities) | Sửa: layer/kiểu đường/kiểu chữ/kiểu kích thước không tồn tại, block tham chiếu tới block thiếu, block tự chứa chính nó, đối tượng tọa độ hỏng hoặc rỗng, thành viên nhóm và trường đã mất, layer hiện hành không tồn tại. RECOVER = mở file rồi AUDIT |
| Thứ tự vẽ | `DRAWORDER` (`DR`: Above/Under/Front/Back), `TEXTTOFRONT`, `HATCHTOBACK` | Above/Under theo đối tượng tham chiếu như AutoCAD |
| Che nền (Wipeout) | `WIPEOUT`, `TEXTMASK` (`NENCHU`) | Wipeout giờ che thật các đối tượng vẽ trước nó — trên màn hình, PDF, SVG, PNG. Khung theo `WIPEOUTFRAME` (0 ẩn, 1 hiện và in, 2 chỉ hiện) |
| Chọn | `SELECTSIMILAR` (`CHONGIONG`) | Cùng loại, cùng layer, cùng tên block / kiểu chữ / kiểu kích thước |
| Layer | `COPYTOLAYER`, `LAYMRG` (`GIOPLAYER`), `LAYDEL` (`XOALAYER`) | Gộp/xóa cả đối tượng trong block; không cho xóa layer 0, Defpoints, layer xref |
| Chữ | `SCALETEXT`, `JUSTIFYTEXT` (`CANHCHU`) | Đổi căn lề mà chữ đứng yên (TEXT và MTEXT) |
| Mảng liên kết | `ARRAYRECT`, `ARRAYPOLAR` (gõ lệnh: Associative = Yes), `ARRAYEDIT` (`SUAMANG`) | Mảng là một đối tượng; ARRAYEDIT đổi số hàng/cột/khoảng cách, số phần tử/góc/xoay; EXPLODE trả lại từng đối tượng. Lưu trong bản ghi riêng của VNCCad (AutoCAD thấy block thường) |
| Cắt hiển thị block/xref | `XCLIP` (`XC`): New (chữ nhật hoặc chọn polyline/đường tròn), Delete | Đọc và ghi SPATIAL_FILTER như AutoCAD (DXF và DWG): bản vẽ AutoCAD có xref bị cắt hiện đúng |
| Thuộc tính block | `ATTSYNC` | Thêm/bớt thuộc tính theo định nghĩa mới, giữ giá trị theo tên thẻ |
| Trường (Field) | `FIELD` (`TRUONG`), `UPDATEFIELD` | Diện tích, chu vi, chiều dài, bán kính của đối tượng (hệ số đổi đơn vị, số lẻ, chữ trước/sau — vd `S = 12.50 m²`), ngày, tên file, tên layout. Tự cập nhật sau mỗi lệnh (cùng bước Undo) |
| Kích thước gấp khúc | `DIMJOGGED` (`JOG`) | Cho cung bán kính lớn (đường cong tuyến). Lưu DXF/DWG dạng kích thước bán kính kèm block hình vẽ, VNCCad đọc lại đủ điểm gấp |
| Mặt tô 2D | `SOLID` (`SO`) | 3 hoặc 4 điểm như AutoCAD |
| In ra máy in | `PRINTER` (`INMAY`, File > Print to Printer…, Ctrl+Shift+P) | Desktop: gửi PDF tới máy in mặc định (Windows qua chương trình đọc PDF có lệnh In; macOS/Linux qua `lp`), không được thì mở PDF để in; web: hộp thoại in của trình duyệt |
| Viewport đa giác | `VPCLIP` (`CATVP`): chọn viewport → Object (polyline kín/đường tròn) hoặc Delete | Bản vẽ AutoCAD có viewport cắt theo đa giác giờ hiện đúng; lưu lại đúng cách AutoCAD (viewport trỏ tới LWPOLYLINE) |
| Chèn bản vẽ khác làm block | `BLOCKFROMFILE` (`CHENFILE`, Insert > Block from File…) | Như INSERT → Browse của AutoCAD: file DXF/DWG thành block (kèm layer, kiểu chữ, block lồng), rồi INSERT hỏi điểm chèn. Chạy cả trên web |
| Copy/Paste giữa hai bản vẽ | `COPYCLIP` / `PASTECLIP` | Giờ mang theo định nghĩa block (cả block lồng, block động), layer, kiểu đường, kiểu chữ, kiểu kích thước — trước đây dán block sang bản vẽ khác bị mất hình |
| Công thức trong bảng | Gõ `=B2*C2`, `=SUM(B2:B10)`, `AVERAGE`, `MIN`, `MAX`, `COUNT`, `ROUND(x; n)` vào ô | Ô hiện giá trị; file lưu cả giá trị (AutoCAD và trình xem khác thấy số) lẫn công thức (VNCCad đọc lại). Đọc được số kiểu `12,5` và `1.250,75` |
| Cắt kích thước | `DIMBREAK` (`CATKT`): Auto / Manual / Remove | Auto cắt đường kích thước và đường gióng nơi đối tượng khác cắt qua |
| Express Tools | `BURST` (`PHABLOCK`), `TXT2MTXT` (`GOPCHU`), `TCOUNT` (`DANHSO`) | BURST giữ giá trị thuộc tính thành chữ; TCOUNT đánh số cọc/mốc theo thứ tự X hoặc Y (ghi đè, thêm trước, thêm sau) |

### Kiểm thử với bản vẽ thật (DWG 2004, 22.000 đối tượng, ~11.000 block)

Thử với một bản vẽ mặt bằng nhà xưởng do đơn vị Trung Quốc lập (TArch/天正, chữ Trung, font SHX bigfont). Các lỗi tìm ra và đã sửa:

| Lỗi | Sửa |
|---|---|
| Chữ Trung Quốc (và Nhật, Hàn) hiện ô vuông khi máy không có font gốc (sysz/syfs.shx, simhei.ttf) | Ký tự font thiếu lấy từ font hệ thống có chữ CJK (SimSun/Microsoft YaHei trên Windows, PingFang trên macOS, Noto CJK trên Linux); đọc được font dạng `.ttc` |
| Tên layout, layer chữ Trung hiện ô vuông trên giao diện | Thêm font CJK hệ thống làm font dự phòng của giao diện |
| File DXF lưu ra ghi chữ Unicode thô với mã trang ANSI_1252: AutoCAD đọc ra chữ lỗi (cả tiếng Việt) | Ghi `\U+XXXX` như AutoCAD làm khi lưu bản 2000; DWG vốn đã đúng |
| Mẫu hatch dày đặc sinh 8 triệu nét khi thu nhỏ, dựng hình 1,6 s | Đường mẫu cách nhau dưới nửa pixel vẽ thành mảng tô: còn 180 nghìn nét, 0,6 s; khi in vẫn giữ đủ nét |
| Chuột để yên trên vùng vẽ vẫn vẽ lại liên tục (CPU 100%+) | Chỉ vẽ lại khi có thao tác |
| Không in được một tờ trong Model (chỉ in toàn bộ) | Lệnh `PLOTWINDOW`, thiết lập in Model |
| Bảng nét in gán trong bản vẽ không có trên máy → lệnh in báo lỗi và dừng | In theo màu đối tượng, báo tên bảng thiếu (như AutoCAD) |
| Tỷ lệ chú thích, bảng nét in của Model không được lưu vào file | Lưu trong LAYOUT "Model" và XRECORD `VNCCAD_SETTINGS` |

Đã kiểm tra: mở 0,7 s; lưu DWG/DXF rồi mở lại đủ 21.975 đối tượng, 188 block, 127 layer; DXF qua kiểm tra ezdxf 0 lỗi; chọn tất cả – di chuyển – hoàn tác; in tờ 1 ra PDF A1 đơn sắc, khung tên đọc rõ. `cadcraft-cli perf FILE.dwg` đo thời gian trên bản vẽ thật.

### Giới hạn hiện tại

- Chế độ offline của bản web chỉ có hiệu lực khi trang được phục vụ qua **HTTPS** (hoặc `localhost`) và đã mở online ít nhất một lần. Bản vẽ rất lớn trên web chậm hơn desktop.
- Phần lõi CADCraft vẫn đang ở giai đoạn phát triển sớm (xem `ROADMAP.md`). Nên lưu file thường xuyên.
- Block động: lưu file từ VNCCad giữ thuộc tính động cho VNCCad (bản ghi riêng), nhưng AutoCAD mở lại sẽ thấy block thường (các đối tượng tham số/hành động của AutoCAD không được ghi lại). Các tham số khác (Point, Polar, XY, Rotation, Lookup, Alignment) chưa hỗ trợ. Hành động Array trong file DWG có thể mất danh sách đối tượng khi đọc (giới hạn của bộ đọc DWG).
- Reactor LISP không lưu cùng bản vẽ; chưa có `vlr-mouse-reactor`, `vlr-sysvar-reactor` (tạo được nhưng không phát sự kiện).

## Build

### Cách 1: GitHub Actions (khuyến nghị)

1. Đưa mã nguồn lên một repo GitHub của anh (nhánh `vnccad` hoặc `main`).
2. Vào tab **Actions** → **VNCCad build**. Mỗi lần push sẽ tự chạy, hoặc bấm **Run workflow**.
3. Khi chạy xong (khoảng 15–25 phút), tải ở mục **Artifacts**:
   - `vnccad-web`: trang web tĩnh, deploy được ngay.
   - `vnccad-windows-x64`: `VNCCad.exe` chạy trực tiếp, không cần cài.
   - `vnccad-linux-x86_64`.

### Cách 2: Build trên máy

Cần cài Rust: <https://rustup.rs>.

```sh
# Bản desktop
cargo run --release -p cadcraft -- --sample

# Bản web
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd apps/cadcraft-web
trunk build --release        # kết quả trong dist/web
trunk serve                  # chạy thử tại http://127.0.0.1:8771
```

## Bản web trên GitHub Pages

Mỗi lần push lên nhánh `main`, workflow tự build và cập nhật trang:
**https://maitrieuquang-max.github.io/VNCCAD/**

## Deploy bản web lên Vercel (tùy chọn)

1. Giải nén artifact `vnccad-web` (hoặc lấy thư mục `dist/web`).
2. Chạy `npx vercel deploy --prod` trong thư mục đó, hoặc kéo-thả thư mục vào vercel.com → Add New → Project.
3. File `vercel.json` đi kèm đã đặt đúng kiểu MIME cho `.wasm` và tắt cache cho `sw.js`/`index.html`, để bản cập nhật đến người dùng ngay.

Sau khi mở trang lần đầu, Chrome/Edge sẽ hiện nút **Cài đặt** trên thanh địa chỉ. Ứng dụng đã cài sẽ mở như phần mềm riêng và chạy được khi mất mạng.

## Lộ trình tiếp theo

- **Giai đoạn 2:** VN2000/UTM, nhập/xuất KML, nền bản đồ vệ tinh, đường đồng mức (chuyển từ DXF Toolkit), giao diện tiếng Việt.
- **Tiếp theo:** các tham số block động còn lại (Point, Polar, XY, Rotation, Lookup), ghi block động theo định dạng AutoCAD, sheet set.
