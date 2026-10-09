//! VNCCad: editing a block definition — BEDIT, REFEDIT, BSAVE, BCLOSE.
//!
//! BEDIT opens the block in its own tab (a copy of the drawing whose model space holds the block's
//! objects, in block coordinates: the base point is marked). Every drawing tool works there.
//! BSAVE writes the objects back into the block of the original drawing — every insert of it
//! changes — as one undo step there; BCLOSE closes the tab (saving or discarding). REFEDIT picks
//! an inserted block and opens it the same way.

use std::sync::Arc;

use cadcraft_doc::{Block, Drawing, Entity, EntityKind, EntityStore, Handle};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::*;
use crate::{Accept, DocState, Input, Interactive, Prompt, Result, Session, Snapshot, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("bedit", "Block Editor", run_bedit)
            .menu(&["Tools", "Block Editor"])
            .alias(&["be", "suablock"])
            .params("{name} | {handle: an insert} → opens the block in its own tab")
            .enabled(has_doc)
            .noundo()
            .interactive(|_| Ok(Box::new(BeditM::default()))),
        CommandSpec::new("refedit", "Edit Reference", run_bedit)
            .menu(&["Tools", "Xref and Block In-place Editing", "Edit Reference"])
            .params("{handle: an insert} → like BEDIT for that block")
            .enabled(has_doc)
            .noundo()
            .interactive(|_| Ok(Box::new(BeditM { pick_only: true, ..Default::default() }))),
        CommandSpec::new("bsave", "Save Block", run_bsave)
            .alias(&["bsaveas_no", "luublock"])
            .params("{} (in a block editor tab)")
            .enabled(in_block)
            .noundo(),
        CommandSpec::new("bclose", "Close Block Editor", run_bclose)
            .alias(&["refclose", "dongblock"])
            .params("{save?: bool (default true)}")
            .noundo()
            .interactive(|_| Ok(Box::new(BcloseM))),
    ]
}

fn in_block(s: &Session) -> std::result::Result<(), String> {
    match s.state() {
        Ok(st) if st.block_edit.is_some() => Ok(()),
        _ => Err("chỉ dùng trong thẻ sửa block (BEDIT)".into()),
    }
}

/// Open `name` of the active drawing for editing.
fn open_block(s: &mut Session, name: &str) -> Result<Value> {
    let st = s.state()?;
    let src = st.doc.clone();
    let uid = st.uid;
    let key = src.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| bad("bedit", format!("không có block `{name}`")))?;
    let b = src.blocks.get(&key).cloned().ok_or_else(|| bad("bedit", "block không còn"))?;
    if b.xref_path.is_some() || key.contains('|') {
        return Err(bad("bedit", format!("`{key}` là tham chiếu ngoài: mở bản vẽ gốc để sửa")));
    }
    if key.starts_with('*') {
        return Err(bad("bedit", format!("`{key}` là block ẩn danh (kích thước, bảng…), không sửa trực tiếp")));
    }
    // Already open?
    if let Some(i) = s.docs.iter().position(|d| d.block_edit.as_ref().is_some_and(|(u, n)| *u == uid && n.eq_ignore_ascii_case(&key))) {
        s.active = i;
        return Ok(json!({ "index": i, "message": format!("Block {key} đang mở.") }));
    }
    let mut d: Drawing = (*src).clone();
    d.model = EntityStore::default();
    for e in b.entities.iter() {
        d.model.push((**e).clone());
    }
    for l in d.layouts.iter_mut() {
        l.entities = EntityStore::default();
    }
    d.groups.clear();
    d.model_page = None;
    let n = d.model.len();
    let title = format!("[Block] {key}");
    let i = s.open_drawing(d, &title, None);
    if let Some(st) = s.docs.get_mut(i) {
        st.block_edit = Some((uid, key.clone()));
    }
    let _ = s.zoom_extents();
    let msg = format!("Sửa block {key} ({n} đối tượng; điểm chèn tại {:.3},{:.3}). Dùng BSAVE để lưu, BCLOSE để đóng.", b.base.x, b.base.y);
    Ok(json!({ "index": i, "block": key, "count": n, "message": msg }))
}

fn insert_block(s: &Session, h: Handle) -> Result<String> {
    match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => Ok(i.block.clone()),
        _ => Err(bad("bedit", "đối tượng chọn không phải block")),
    }
}

fn run_bedit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = match (str_param(p, "name"), p.get("handle").and_then(|v| v.as_str().and_then(Handle::parse_hex))) {
        (Some(n), _) => n.to_string(),
        (None, Some(h)) => insert_block(s, h)?,
        (None, None) => {
            let sel = s.selection();
            match sel.first() {
                Some(h) => insert_block(s, *h)?,
                None => return Err(bad("bedit", "`name` hoặc `handle` của block là bắt buộc")),
            }
        }
    };
    open_block(s, &name)
}

/// Write the block editor's objects back into the original drawing.
fn save_back(s: &mut Session) -> Result<Value> {
    let st = s.state()?;
    let (uid, name) = st.block_edit.clone().ok_or_else(|| bad("bsave", "không ở trong thẻ sửa block"))?;
    let edited = st.doc.clone();
    let si = s.docs.iter().position(|d| d.uid == uid).ok_or_else(|| bad("bsave", "bản vẽ gốc đã đóng"))?;
    let src_state: &mut DocState = s.docs.get_mut(si).ok_or_else(|| bad("bsave", "bản vẽ gốc đã đóng"))?;
    let before = src_state.doc.clone();
    let old: Arc<Block> = before.blocks.get(&name).cloned().ok_or_else(|| bad("bsave", format!("block `{name}` không còn trong bản vẽ gốc")))?;
    let old_handles: std::collections::HashSet<Handle> = old.entities.iter().map(|e| e.handle).collect();
    let doc = Arc::make_mut(&mut src_state.doc);
    let mut store = EntityStore::default();
    let mut n = 0;
    for e in edited.model.iter() {
        let mut e: Entity = (**e).clone();
        // Objects drawn in the editor get handles of the original drawing.
        if !old_handles.contains(&e.handle) || doc.entity(e.handle).is_some() {
            e.handle = doc.new_handle();
        }
        doc.bump_handseed(e.handle);
        doc.ensure_layer(&e.common.layer);
        // A block can't hold itself.
        if matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(&name)) {
            continue;
        }
        store.push(e);
        n += 1;
    }
    // Layers, styles and blocks made in the editor come along.
    for l in &edited.layers {
        if !doc.layers.iter().any(|x| x.name.eq_ignore_ascii_case(&l.name)) {
            doc.layers.push(l.clone());
        }
    }
    for (k, b) in &edited.blocks {
        if !doc.blocks.contains_key(k) {
            doc.blocks.insert(k.clone(), b.clone());
        }
    }
    let mut nb: Block = (*old).clone();
    nb.entities = store;
    doc.blocks.insert(name.clone(), Arc::new(nb));
    src_state.undo.push(Snapshot { label: format!("BEDIT {name}"), doc: before, selection: src_state.selection.clone() });
    src_state.redo.clear();
    src_state.revision += 1;
    // The editor's copy now matches what was saved.
    if let Ok(st) = s.state_mut() {
        st.saved = st.doc.clone();
    }
    Ok(json!({ "block": name, "count": n, "message": format!("Đã lưu block {name} ({n} đối tượng); mọi block {name} trong bản vẽ đã cập nhật.") }))
}

fn run_bsave(s: &mut Session, _p: &Value) -> Result<Value> {
    save_back(s)
}

fn close_editor(s: &mut Session, save: bool) -> Result<Value> {
    let st = s.state()?;
    let (uid, name) = st.block_edit.clone().ok_or_else(|| bad("bclose", "không ở trong thẻ sửa block"))?;
    let mut msg = String::new();
    if save && st.is_dirty() {
        let r = save_back(s)?;
        msg = r.get("message").and_then(Value::as_str).unwrap_or("").to_string();
    }
    s.cancel();
    let i = s.active;
    s.docs.remove(i);
    s.active = s.docs.iter().position(|d| d.uid == uid).unwrap_or(s.docs.len().saturating_sub(1));
    if msg.is_empty() {
        msg = if save { format!("Đóng trình sửa block {name}.") } else { format!("Đóng trình sửa block {name}, bỏ thay đổi.") };
    }
    Ok(json!({ "block": name, "message": msg }))
}

fn run_bclose(s: &mut Session, p: &Value) -> Result<Value> {
    close_editor(s, bool_or(p, "save", true))
}

/// BEDIT: block name, or pick an inserted block (REFEDIT: pick only).
#[derive(Default)]
struct BeditM {
    pick_only: bool,
    sel: SelectPhase,
}

impl Interactive for BeditM {
    fn name(&self) -> &'static str {
        "BEDIT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let pre = s.selection();
        if let Some(h) = pre.first().copied()
            && insert_block(s, h).is_ok()
        {
            let r = run_bedit(s, &json!({ "handle": h.hex() }))?;
            super::group::echo_msg(s, &r);
            return Ok(Step::Done);
        }
        s.set_selection(Vec::new());
        self.sel = SelectPhase::single();
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.pick_only {
            Prompt::new("Chọn block cần sửa", Accept::SELECT)
        } else {
            Prompt::new("Chọn block hoặc gõ tên block", Accept { point: true, number: false, text: true, select: true, enter: true })
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let r = match &i {
            Input::Text(t) | Input::Keyword(t) if !self.pick_only => run_bedit(s, &json!({ "name": t.trim() })),
            Input::Enter => return Ok(Step::Done),
            _ => match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    match hs.first() {
                        Some(h) => run_bedit(s, &json!({ "handle": h.hex() })),
                        None => return Ok(Step::Done),
                    }
                }
            },
        };
        match r {
            Ok(v) => {
                super::group::echo_msg(s, &v);
                Ok(Step::Done)
            }
            Err(e) => {
                s.echo(e.to_string());
                self.sel = SelectPhase::single();
                Ok(Step::Continue)
            }
        }
    }
}

/// BCLOSE: "Save changes to the block? [Yes/No] <Yes>".
struct BcloseM;

impl Interactive for BcloseM {
    fn name(&self) -> &'static str {
        "BCLOSE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if let Err(m) = in_block(s) {
            s.echo(m);
            return Ok(Step::Done);
        }
        if s.state().is_ok_and(|st| !st.is_dirty()) {
            let r = close_editor(s, false)?;
            super::group::echo_msg(s, &r);
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Lưu thay đổi vào block", Accept::TEXT).kw(&["Yes", "No"]).default("Yes")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let save = match &i {
            Input::Keyword(k) | Input::Text(k) => !k.trim().to_ascii_lowercase().starts_with('n'),
            _ => true,
        };
        // The editor tab goes away: end this command without touching it again.
        let r = close_editor(s, save)?;
        super::group::echo_msg(s, &r);
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_session() -> (Session, Handle) {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 5] })).unwrap();
        let h = s.doc().unwrap().model.last().unwrap().handle;
        s.execute("block", &json!({ "name": "COC", "base": [0, 0], "handles": [h.hex()] })).unwrap();
        s.execute("insert", &json!({ "name": "COC", "at": [100, 0] })).unwrap();
        s.execute("insert", &json!({ "name": "COC", "at": [200, 0] })).unwrap();
        let ins = s.doc().unwrap().model.last().unwrap().handle;
        (s, ins)
    }

    #[test]
    fn bedit_changes_every_insert() {
        let (mut s, ins) = block_session();
        let src = s.active;
        let r = s.execute("bedit", &json!({ "handle": ins.hex() })).unwrap();
        assert_eq!(r["block"], "COC");
        assert_ne!(s.active, src);
        assert!(s.state().unwrap().title.contains("COC"));
        assert_eq!(s.doc().unwrap().model.len(), 1);
        // Add a circle in the block and save.
        s.execute("circle", &json!({ "center": [5, 2.5], "radius": 1 })).unwrap();
        let r = s.execute("bsave", &json!({})).unwrap();
        assert_eq!(r["count"], 2);
        let d = &s.docs[src].doc;
        assert_eq!(d.block("COC").unwrap().entities.len(), 2);
        // The new object got a handle of the original drawing (no clash).
        let hs: Vec<Handle> = d.block("COC").unwrap().entities.iter().map(|e| e.handle).collect();
        assert!(hs.iter().all(|h| d.model.get(*h).is_none()));
        // Close: back to the drawing, one undo step there restores the old block.
        s.execute("bclose", &json!({})).unwrap();
        assert_eq!(s.active, src);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().block("COC").unwrap().entities.len(), 1);
        assert!(s.execute("bsave", &json!({})).is_err(), "only in a block editor");
    }

    #[test]
    fn interactive_bedit_and_discard() {
        let (mut s, _) = block_session();
        s.set_selection(Vec::new());
        s.start("bedit").unwrap();
        s.cmdline("coc").unwrap();
        assert!(s.state().unwrap().block_edit.is_some());
        s.execute("erase", &json!({ "handles": [s.doc().unwrap().model.last().unwrap().handle.hex()] })).unwrap();
        s.start("bclose").unwrap();
        s.cmdline("N").unwrap();
        assert!(s.state().unwrap().block_edit.is_none());
        assert_eq!(s.doc().unwrap().block("COC").unwrap().entities.len(), 1, "discarded");
        assert!(s.execute("bedit", &json!({ "name": "KHONGCO" })).is_err());
    }
}
