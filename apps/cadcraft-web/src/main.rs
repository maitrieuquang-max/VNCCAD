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

    struct Shell(CadApp, Inbox);

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
            let picked: Vec<(String, Vec<u8>)> = self.1.borrow_mut().drain(..).collect();
            for (name, bytes) in picked {
                self.0.open_bytes(&name, &bytes);
            }
            self.0.logic(ctx);
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
        }
    }

    /// VNCCad: show the browser's file picker; the chosen drawing lands in `inbox`.
    fn request_open(inbox: &Inbox, ctx: &egui::Context) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Some(input) = document.create_element("input").ok().and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        input.set_type("file");
        input.set_accept(".dxf,.dwg,.DXF,.DWG");
        let (inbox, ctx, picker) = (inbox.clone(), ctx.clone(), input.clone());
        let on_change = Closure::once_into_js(move || {
            let Some(file) = picker.files().and_then(|f| f.get(0)) else { return };
            let name = file.name();
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
                        let services = Services {
                            request_open: Some(Box::new(move || request_open(&ib, &ctx))),
                            download: Some(Box::new(download)),
                            autosave: autosave_store(),
                            ..Services::default()
                        };
                        let mut app = CadApp::new(Session::new(), services);
                        if let Some(rs) = &cc.wgpu_render_state {
                            app.set_wgpu(rs);
                        }
                        if query().contains("sample") {
                            let _ = app.run("ui.sample", serde_json::json!({}));
                        }
                        Ok(Box::new(Shell(app, inbox)))
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
