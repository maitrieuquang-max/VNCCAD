//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use cadcraft_engine::Session;
use serde_json::{Value, json};

/// Something that answers control-channel methods (`engine.execute`, `cmdline.input`,
/// `drawing.inspect`, `ui.render`…). See `cadcraft_ui_egui::control` for the full list.
pub trait Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real UI is attached (`ui.screenshot` works).
    fn has_ui(&self) -> bool;
    fn describe(&self) -> String;
}

/// A running CADCraft app reached through its loopback control port.
pub struct Remote {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Remote {
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1 };
        r.reconnect()?;
        Ok(r)
    }
    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(Duration::from_secs(90))).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }
    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.reconnect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else { return Err(std::io::Error::other("not connected")) };
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = String::new();
        let n = reader.read_line(&mut reply)?;
        if n == 0 {
            self.conn = None;
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        Ok(reply)
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        let reply = match self.roundtrip(&line) {
            Ok(r) => r,
            Err(_) => self.roundtrip(&line).map_err(|e| format!("CADCraft at {}: {e}", self.addr))?,
        };
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        format!("connected to CADCraft at {}", self.addr)
    }
}

/// An in-process headless session (no window).
pub struct Headless {
    pub session: Session,
}

impl Default for Headless {
    fn default() -> Self {
        Headless { session: Session::new() }
    }
}

fn state(s: &Session) -> Value {
    let p = s.current_prompt();
    json!({
        "prompt": s.prompt_text(),
        "running": s.running.as_ref().map(|r| r.id.clone()),
        "keywords": p.as_ref().map(|p| p.keywords.clone()).unwrap_or_default(),
        "history": s.log.iter().rev().take(20).rev().cloned().collect::<Vec<_>>(),
    })
}

impl Backend for Headless {
    fn call(&mut self, method: &str, p: Value) -> Result<Value, String> {
        let s = &mut self.session;
        let str_p = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
        match method {
            "engine.execute" | "command" => {
                let id = str_p("command").ok_or("missing `command`")?;
                let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
                s.execute(&id, &params).map_err(|e| e.to_string())
            }
            "cmdline.input" => {
                let before = s.log.len();
                let r = s.cmdline(&str_p("text").unwrap_or_default());
                let out: Vec<String> = s.log.iter().skip(before).cloned().collect();
                let mut st = state(s);
                if let Some(o) = st.as_object_mut() {
                    o.insert("output".into(), json!(out));
                    if let Err(e) = r {
                        o.insert("error".into(), json!(e.to_string()));
                    }
                }
                Ok(st)
            }
            "cmdline.script" => {
                let before = s.log.len();
                s.script(&str_p("text").unwrap_or_default()).map_err(|e| e.to_string())?;
                let out: Vec<String> = s.log.iter().skip(before).cloned().collect();
                Ok(json!({"output": out, "state": state(s)}))
            }
            "cmdline.key" => {
                if str_p("key").is_some_and(|k| k.eq_ignore_ascii_case("escape") || k.eq_ignore_ascii_case("esc")) {
                    s.cancel();
                } else {
                    s.input(cadcraft_engine::Input::Enter).map_err(|e| e.to_string())?;
                }
                Ok(state(s))
            }
            "cmdline.state" => Ok(state(s)),
            "engine.commands" => {
                Ok(Value::Array(cadcraft_engine::command_specs().iter().map(|c| serde_json::to_value(c.info(s)).unwrap_or_default()).collect()))
            }
            "drawing.inspect" | "document.inspect" => s.execute("drawing.inspect", &p).map_err(|e| e.to_string()),
            "ui.render" => {
                let w = p.get("width").and_then(Value::as_f64).unwrap_or(1600.0).clamp(16.0, 8192.0) as u32;
                let h = p.get("height").and_then(Value::as_f64).unwrap_or(1000.0).clamp(16.0, 8192.0) as u32;
                let st = s.state().map_err(|e| e.to_string())?;
                let list = cadcraft_render::build(&st.doc, &st.space, &cadcraft_render::Options::default());
                let fit = p.get("fit").and_then(Value::as_bool).unwrap_or(true);
                let view = if fit {
                    cadcraft_render::raster::View::fit(&list.bounds, w, h, 0.05)
                } else {
                    let v = st.view();
                    cadcraft_render::raster::View { center: v.center, scale: f64::from(h) / v.height.max(1e-12), width: w, height: h }
                };
                let png =
                    cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default()).ok_or("render failed")?;
                if let Some(path) = str_p("path") {
                    std::fs::write(&path, &png).map_err(|e| format!("{path}: {e}"))?;
                }
                Ok(json!({"pngBase64": cadcraft_engine::cmd::file::base64_encode(&png), "width": w, "height": h}))
            }
            "app.open" => s.execute("open", &json!({"path": str_p("path")})).map_err(|e| e.to_string()),
            "app.save" => {
                let id = if p.get("path").is_some() { "saveas" } else { "qsave" };
                s.execute(id, &p).map_err(|e| e.to_string())
            }
            "ui.screenshot" => Err("no UI in headless mode; use render".into()),
            other => Err(format!("unknown method `{other}`")),
        }
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "headless CADCraft session".into()
    }
}
