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

### Giới hạn hiện tại

- **DWG chỉ mở/lưu được trên bản desktop.** Trên web, hãy dùng DXF hoặc chuyển DWG sang DXF trước (ODA File Converter, hoặc mở bằng bản desktop rồi lưu DXF).
- Chế độ offline của bản web chỉ có hiệu lực khi trang được phục vụ qua **HTTPS** (hoặc `localhost`) và đã mở online ít nhất một lần.
- Phần lõi CADCraft vẫn đang ở giai đoạn phát triển sớm (xem `ROADMAP.md`). Nên lưu file thường xuyên.

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
- **Giai đoạn 3:** công cụ cầu đường: trắc dọc, trắc ngang, chuẩn hóa layer/ký hiệu theo TCVN, bảng khối lượng.
