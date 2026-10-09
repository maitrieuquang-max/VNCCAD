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
| Script | `SCRIPT` hoặc kéo-thả `.scr` | Như AutoCAD: dấu cách/xuống dòng là Enter; dòng `(…)` chạy LISP |

Giới hạn LISP: chưa có ActiveX (`vla-*`, `vlax-*`), hộp thoại DCL, reactor; `entmake` hỗ trợ LINE, CIRCLE, ARC, POINT, LWPOLYLINE, TEXT, MTEXT, INSERT. Lệnh LISP được chạy lại từ đầu sau mỗi lần nhập (phát lại các câu trả lời trước) — với chương trình hỏi hàng trăm lần có thể chậm.

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

- **DWG chỉ mở/lưu được trên bản desktop.** Trên web, hãy dùng DXF hoặc chuyển DWG sang DXF trước (ODA File Converter, hoặc mở bằng bản desktop rồi lưu DXF).
- Chế độ offline của bản web chỉ có hiệu lực khi trang được phục vụ qua **HTTPS** (hoặc `localhost`) và đã mở online ít nhất một lần.
- Phần lõi CADCraft vẫn đang ở giai đoạn phát triển sớm (xem `ROADMAP.md`). Nên lưu file thường xuyên.
- Bảng nét in theo tên (STB) chưa hỗ trợ; Xref lồng nhau (xref bên trong xref) không được nạp; ảnh/PDF nền chưa in ra PDF khi PLOT.
- Bản web không có font hệ thống: chữ Trung/Nhật/Hàn chỉ hiện khi kéo-thả kèm một font có các chữ đó (ví dụ `simsun.ttc`).
- Bản vẽ lưu DWG ra định dạng AutoCAD 2000 (mở được bằng mọi bản AutoCAD từ 2000).
- Kích thước annotative chỉ áp dụng khi kích thước được dựng lại trong VNCCad (kích thước đọc từ file kèm block `*D` vẽ theo block đó).

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
- **Tiếp theo:** in ảnh/PDF nền ra PDF, STB, block editor và dynamic block, AutoLISP hoặc bộ lệnh tự động hóa thay thế.
