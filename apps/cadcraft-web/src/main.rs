//! CADCraft in the browser.
//!
//! Runs the same [`cadcraft_ui_egui::CadApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from this
//! directory (output in `dist/web`). URL flags: `?webgl` forces WebGL2, `?sample` opens the sample
//! drawing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web {
    use std::{cell::RefCell, rc::Rc};

    use cadcraft_engine::Session;
    use cadcraft_ui_egui::{CadApp, Services};
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::prelude::Closure;

    /// Files picked in the browser, waiting to be opened on the next frame.
    type Inbox = Rc<RefCell<Vec<(String, Vec<u8>)>>>;

    const CANVAS_ID: &str = "cadcraft_canvas";
    const LOADING_ID: &str = "cadcraft_loading";

    /// VNCCad: a DWG being opened in steps, one per frame, so the page keeps painting (and the
    /// browser doesn't report it as hung): announce → convert to DXF → open.
    enum Staged {
        Announced(String, Vec<u8>),
        Converted(String, Vec<u8>),
    }

    struct Shell(CadApp, Inbox, Vec<Staged>, Option<cadcraft_ui_egui::selftest::SelfTest>);

    /// VNCCad: publish the self test's result for the CI browser check: the page title and a
    /// `<pre id="vnccad-selftest" data-result="ok|fail">` element.
    fn report_selftest(r: &Result<String, String>) {
        let (tag, text) = match r {
            Ok(t) => ("ok", format!("SELFTEST OK: {t}")),
            Err(t) => ("fail", format!("SELFTEST FAIL: {t}")),
        };
        log::info!("{text}");
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        document.set_title(&text);
        if let (Ok(pre), Some(body)) = (document.create_element("pre"), document.body()) {
            pre.set_id("vnccad-selftest");
            let _ = pre.set_attribute("data-result", tag);
            pre.set_text_content(Some(&text));
            let _ = body.append_child(&pre);
        }
    }

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // Dropped files: the browser reads them asynchronously into the inbox.
            for f in ctx.input(|i| i.raw.dropped_files.clone()) {
                let name = f.path().file_name().map_or_else(|| "Drawing.dxf".to_string(), |n| n.to_string_lossy().to_string());
                let (inbox, ctx) = (self.1.clone(), ctx.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    match f.bytes_async().await {
                        Ok(bytes) => {
                            inbox.borrow_mut().push((name, bytes));
                            ctx.request_repaint();
                        }
                        Err(e) => log::error!("reading {name} failed: {e}"),
                    }
                });
            }
            let mut picked: Vec<(String, Vec<u8>)> = self.1.borrow_mut().drain(..).collect();
            // World files and fonts before the images and drawings that use them.
            picked.sort_by_key(|(n, _)| {
                let n = n.to_ascii_lowercase();
                if cadcraft_io::raster::is_image_name(&n) {
                    2
                } else if n.ends_with(".dxf") || n.ends_with(".dwg") {
                    3
                } else {
                    1
                }
            });
            // One heavy step per frame (files picked this frame wait for the next, so the
            // "opening…" message gets painted first).
            if !self.2.is_empty() {
                let step = self.2.remove(0);
                match step {
                    Staged::Announced(name, bytes) => match cadcraft_io::dwg_to_dxf(&bytes) {
                        Ok(dxf) => {
                            self.0.set_status(format!("Đang dựng bản vẽ {name}…"));
                            self.2.push(Staged::Converted(name, dxf));
                        }
                        Err(e) => {
                            self.0.session.echo(format!("Không mở được {name}: {e}"));
                            self.0.set_status(format!("Không mở được {name}"));
                        }
                    },
                    // DXF bytes under the DWG's name: the title (and Save) keep the .dwg name.
                    Staged::Converted(name, dxf) => {
                        self.0.open_bytes(&name, &dxf);
                        self.0.set_status(format!("Đã mở {name}"));
                    }
                }
                ctx.request_repaint();
            }
            for (name, bytes) in picked {
                if cadcraft_io::is_dwg(&bytes) {
                    self.0.set_status(format!("Đang mở {name}… (file DWG lớn có thể mất vài giây)"));
                    self.0.session.echo(format!("Đang mở {name}…"));
                    self.2.push(Staged::Announced(name, bytes));
                    ctx.request_repaint();
                } else {
                    self.0.open_bytes(&name, &bytes);
                }
            }
            self.0.logic(ctx);
            if let Some(t) = self.3.as_mut() {
                ctx.request_repaint();
                if let Some(r) = t.step(&mut self.0, js_sys::Date::now()) {
                    report_selftest(&r);
                    self.3 = None;
                }
            }
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
        }
    }

    /// VNCCad: a CJK font installed on this computer, through the browser's Local Font Access
    /// (Chrome/Edge ask the user first). The font lands in `inbox` as `cjk-local.ttc`.
    fn local_cjk_font(inbox: &Inbox, ctx: &egui::Context) {
        use wasm_bindgen::JsValue;
        const WANTED: [&str; 10] = [
            "SimSun",
            "MicrosoftYaHei",
            "SimHei",
            "NSimSun",
            "DengXian-Regular",
            "MS-Gothic",
            "MalgunGothic",
            "PingFangSC-Regular",
            "NotoSansCJKsc-Regular",
            "NotoSansSC-Regular",
        ];
        let (inbox, ctx) = (inbox.clone(), ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let Some(window) = web_sys::window() else { return };
            let Ok(f) = js_sys::Reflect::get(&window, &JsValue::from_str("queryLocalFonts")) else { return };
            let Some(f) = f.dyn_ref::<js_sys::Function>() else {
                log::warn!("queryLocalFonts is not available in this browser");
                cdn_cjk_font(&inbox, &ctx, true).await;
                return;
            };
            let Ok(p) = f.call0(&window) else { return };
            let Ok(p) = p.dyn_into::<js_sys::Promise>() else { return };
            let Ok(list) = wasm_bindgen_futures::JsFuture::from(p).await else {
                log::warn!("local font access was refused");
                cdn_cjk_font(&inbox, &ctx, true).await;
                return;
            };
            let list = js_sys::Array::from(&list);
            let name_of =
                |v: &JsValue| js_sys::Reflect::get(v, &JsValue::from_str("postscriptName")).ok().and_then(|n| n.as_string()).unwrap_or_default();
            let mut chosen: Option<JsValue> = None;
            for want in WANTED {
                if let Some(v) = list.iter().find(|v| name_of(v).eq_ignore_ascii_case(want)) {
                    chosen = Some(v);
                    break;
                }
            }
            let Some(font) = chosen else {
                log::warn!("no CJK font among the local fonts");
                cdn_cjk_font(&inbox, &ctx, true).await;
                return;
            };
            let Ok(blob_fn) = js_sys::Reflect::get(&font, &JsValue::from_str("blob")) else { return };
            let Some(blob_fn) = blob_fn.dyn_ref::<js_sys::Function>() else { return };
            let Ok(p) = blob_fn.call0(&font).and_then(|p| p.dyn_into::<js_sys::Promise>()) else { return };
            let Ok(blob) = wasm_bindgen_futures::JsFuture::from(p).await else { return };
            let Ok(blob) = blob.dyn_into::<web_sys::Blob>() else { return };
            let Ok(buf) = wasm_bindgen_futures::JsFuture::from(blob.array_buffer()).await else { return };
            let bytes = js_sys::Uint8Array::new(&buf).to_vec();
            inbox.borrow_mut().push(("cjk-local.ttc".into(), bytes));
            ctx.request_repaint();
        });
    }

    /// VNCCad: open-licensed CJK fonts (SIL OFL) on public CDNs, tried in order. The first
    /// covers Chinese (simplified and traditional), Japanese kana and Korean Hangul.
    const CJK_FONT_URLS: [&str; 4] = [
        "https://cdn.jsdelivr.net/npm/@chanmeng666/archlang-font-cjk@1.0.0/ArchLangCJKSans-Regular.otf",
        "https://unpkg.com/@chanmeng666/archlang-font-cjk@1.0.0/ArchLangCJKSans-Regular.otf",
        "https://cdn.jsdelivr.net/npm/@embedpdf/fonts-sc@1.0.0/fonts/NotoSansHans-Regular.otf",
        "https://unpkg.com/@embedpdf/fonts-sc@1.0.0/fonts/NotoSansHans-Regular.otf",
    ];

    /// VNCCad: the CJK font from the browser's cache (`network` false: no download, used at
    /// start-up so a font fetched once keeps working offline) or downloaded from a CDN and
    /// cached. The font lands in `inbox` as `cjk-local.otf`.
    async fn cdn_cjk_font(inbox: &Inbox, ctx: &egui::Context, network: bool) {
        use wasm_bindgen::JsValue;
        const BODY: &str = "return (async () => {
            const tryCache = typeof caches !== 'undefined';
            const c = tryCache ? await caches.open('vnccad-fonts').catch(() => null) : null;
            for (const u of urls) {
                let r = c ? await c.match(u).catch(() => undefined) : undefined;
                if (!r && network) {
                    try {
                        r = await fetch(u, { mode: 'cors' });
                        if (!r.ok) { r = undefined; continue; }
                        if (c) { await c.put(u, r.clone()).catch(() => {}); }
                    } catch (e) { r = undefined; }
                }
                if (r) { return new Uint8Array(await r.arrayBuffer()); }
            }
            return null;
        })();";
        let f = js_sys::Function::new_with_args("urls, network", BODY);
        let urls = js_sys::Array::new();
        for u in CJK_FONT_URLS {
            urls.push(&JsValue::from_str(u));
        }
        let Ok(p) = f.call2(&JsValue::NULL, &urls, &JsValue::from_bool(network)) else { return };
        let Ok(p) = p.dyn_into::<js_sys::Promise>() else { return };
        match wasm_bindgen_futures::JsFuture::from(p).await {
            Ok(v) if !v.is_null() && !v.is_undefined() => {
                let bytes = js_sys::Uint8Array::new(&v).to_vec();
                if bytes.len() > 1000 {
                    inbox.borrow_mut().push(("cjk-local.otf".into(), bytes));
                    ctx.request_repaint();
                }
            }
            Ok(_) if network => log::warn!("could not download a CJK font (offline?)"),
            _ => {}
        }
    }

    /// VNCCad: show the browser's file picker; the chosen drawing lands in `inbox`.
    fn request_open(inbox: &Inbox, ctx: &egui::Context) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Some(input) = document.create_element("input").ok().and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        input.set_type("file");
        input.set_accept(".dxf,.dwg,.shx,.lsp,.dcl,.sld,.scr,.ttf,.otf,.ttc,.ctb,.stb,.png,.jpg,.jpeg,.bmp,.tif,.tiff,.jgw,.pgw,.tfw,.wld,.pdf");
        let (inbox, ctx, picker) = (inbox.clone(), ctx.clone(), input.clone());
        input.set_multiple(true);
        let on_change = Closure::once_into_js(move || {
            let Some(files) = picker.files() else { return };
            for i in 0..files.length().min(32) {
                let Some(file) = files.get(i) else { continue };
                let name = file.name();
                let (inbox, ctx) = (inbox.clone(), ctx.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                        Ok(buf) => {
                            let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                            inbox.borrow_mut().push((name, bytes));
                            ctx.request_repaint();
                        }
                        Err(e) => log::error!("reading {name} failed: {e:?}"),
                    }
                });
            }
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
        input.click();
    }

    /// VNCCad: save `bytes` through the browser as a download called `name`.
    fn download(name: &str, bytes: &[u8]) {
        let Some(window) = web_sys::window() else { return };
        let Some(document) = window.document() else { return };
        let parts = js_sys::Array::new();
        parts.push(&js_sys::Uint8Array::from(bytes));
        let opts = web_sys::BlobPropertyBag::new();
        let mime = if name.to_ascii_lowercase().ends_with(".pdf") { "application/pdf" } else { "application/octet-stream" };
        opts.set_type(mime);
        let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts) else { return };
        let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
        if let Some(a) = document.create_element("a").ok().and_then(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().ok()) {
            a.set_href(&url);
            a.set_download(name);
            a.click();
        }
        // Release the blob once the browser has taken it.
        let revoke = Closure::once_into_js(move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 60_000);
    }

    /// VNCCad: autosaves in the browser's localStorage (`vnccad-as:<key>` holds the DXF,
    /// `vnccad-asm:<key>` its title and time). Browsers allow about 5 MB per site, so very large
    /// drawings may not fit; the app then says so once and the user saves normally.
    fn autosave_store() -> Option<cadcraft_ui_egui::autosave::AutosaveStore> {
        use cadcraft_ui_egui::autosave::{AutosaveEntry, AutosaveStore, age_text};
        use serde_json::{Value, json};
        const DATA: &str = "vnccad-as:";
        const META: &str = "vnccad-asm:";
        fn storage() -> Option<web_sys::Storage> {
            web_sys::window()?.local_storage().ok()?
        }
        storage()?;
        Some(AutosaveStore {
            run_tag: format!("w{}", js_sys::Date::now() as u64),
            interval_s: 60.0,
            location: "bộ nhớ trình duyệt (localStorage)".into(),
            put: Box::new(|key, title, path, dxf| {
                let st = storage().ok_or("trình duyệt không cho dùng bộ nhớ")?;
                // localStorage holds about 5 MB per site.
                if dxf.len() > 4_500_000 {
                    return Err(format!(
                        "bản vẽ quá lớn ({:.1} MB) để tự lưu trong trình duyệt — hãy lưu file (Ctrl+S) thường xuyên",
                        dxf.len() as f64 / 1e6
                    ));
                }
                let text = match std::str::from_utf8(dxf) {
                    Ok(t) => t.to_string(),
                    Err(_) => format!("b64:{}", cadcraft_engine::cmd::file::base64_encode(dxf)),
                };
                st.set_item(&format!("{DATA}{key}"), &text).map_err(|_| "bộ nhớ trình duyệt đã đầy".to_string())?;
                let meta = json!({ "title": title, "path": path, "time": js_sys::Date::now() });
                st.set_item(&format!("{META}{key}"), &meta.to_string()).map_err(|_| "bộ nhớ trình duyệt đã đầy".to_string())
            }),
            list: Box::new(|| {
                let Some(st) = storage() else { return Vec::new() };
                let now = js_sys::Date::now();
                let n = st.length().unwrap_or(0);
                let mut out: Vec<(f64, AutosaveEntry)> = (0..n)
                    .filter_map(|i| st.key(i).ok().flatten())
                    .filter_map(|k| {
                        let key = k.strip_prefix(META)?.to_string();
                        let meta: Value = serde_json::from_str(&st.get_item(&k).ok()??).ok()?;
                        let time = meta.get("time").and_then(Value::as_f64).unwrap_or(0.0);
                        let title = meta.get("title").and_then(Value::as_str).unwrap_or("Bản vẽ").to_string();
                        let path = meta.get("path").and_then(Value::as_str).map(str::to_string);
                        Some((time, AutosaveEntry { key, title, path, age: age_text(now - time) }))
                    })
                    .collect();
                out.sort_by(|a, b| b.0.total_cmp(&a.0));
                out.into_iter().map(|(_, e)| e).collect()
            }),
            get: Box::new(|key| {
                let text = storage()?.get_item(&format!("{DATA}{key}")).ok()??;
                match text.strip_prefix("b64:") {
                    Some(b) => cadcraft_engine::cmd::file::base64_decode(b),
                    None => Some(text.into_bytes()),
                }
            }),
            remove: Box::new(|key| {
                if let Some(st) = storage() {
                    let _ = st.remove_item(&format!("{DATA}{key}"));
                    let _ = st.remove_item(&format!("{META}{key}"));
                }
            }),
        })
    }

    /// VNCCad: register the service worker so the app keeps working without a network.
    fn register_offline() {
        let Some(window) = web_sys::window() else { return };
        let proto = window.location().protocol().unwrap_or_default();
        if proto != "https:" && window.location().hostname().unwrap_or_default() != "localhost" {
            return;
        }
        let sw = window.navigator().service_worker();
        let _ = sw.register("./sw.js");
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        register_offline();
        cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
            read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
            plot: Some(|d, space, opts| cadcraft_io::plot(d, space, opts).map_err(|e| e.to_string())),
        });
        wasm_bindgen_futures::spawn_local(async {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
                log::error!("missing <canvas id=\"{CANVAS_ID}\">");
                return;
            };
            let mut options = eframe::WebOptions::default();
            if query().contains("webgl")
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }
            let result = eframe::WebRunner::new()
                .start(
                    canvas,
                    options,
                    Box::new(move |cc| {
                        let inbox: Inbox = Rc::new(RefCell::new(Vec::new()));
                        let (ib, ctx) = (inbox.clone(), cc.egui_ctx.clone());
                        let (ib2, ctx2) = (inbox.clone(), cc.egui_ctx.clone());
                        // A CJK font fetched in an earlier session (from the browser cache only).
                        {
                            let (ib3, ctx3) = (inbox.clone(), cc.egui_ctx.clone());
                            wasm_bindgen_futures::spawn_local(async move { cdn_cjk_font(&ib3, &ctx3, false).await });
                        }
                        let services = Services {
                            request_open: Some(Box::new(move || request_open(&ib, &ctx))),
                            download: Some(Box::new(download)),
                            autosave: autosave_store(),
                            local_fonts: Some(Box::new(move || local_cjk_font(&ib2, &ctx2))),
                            ..Services::default()
                        };
                        let mut app = CadApp::new(Session::new(), services);
                        if let Some(rs) = &cc.wgpu_render_state {
                            app.set_wgpu(rs);
                        }
                        if query().contains("sample") {
                            let _ = app.run("ui.sample", serde_json::json!({}));
                        }
                        // VNCCad: `?selftest` (CI): open generated DWG files as a user would.
                        let selftest = query().contains("selftest").then(|| {
                            inbox.borrow_mut().extend(cadcraft_ui_egui::selftest::SelfTest::files());
                            cadcraft_ui_egui::selftest::SelfTest::new(js_sys::Date::now(), query().contains("cjk"))
                        });
                        Ok(Box::new(Shell(app, inbox, Vec::new(), selftest)))
                    }),
                )
                .await;
            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                match result {
                    Ok(()) => el.remove(),
                    Err(e) => {
                        el.set_inner_html(&format!("<p>CADCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>"))
                    }
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("cadcraft-web only runs in the browser: build it with `trunk build --release` in apps/cadcraft-web");
}
