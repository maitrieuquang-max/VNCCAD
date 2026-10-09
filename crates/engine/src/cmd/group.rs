//! VNCCad: object groups (GROUP, UNGROUP, GROUPEDIT, PICKSTYLE).
//!
//! A group is a named set of objects that is selected as a whole: picking one member selects
//! them all while PICKSTYLE (group selection) is on. Groups are saved in the DXF named object
//! dictionary `ACAD_GROUP`, as AutoCAD does.

use cadcraft_doc::{Group, Handle};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("group", "Group", run_group)
            .menu(&["Tools", "Group"])
            .alias(&["g", "nhom"])
            .params("{handles?: selection, name?: unnamed when absent, description?, selectable?: true}")
            .interactive(|_| Ok(Box::new(GroupM::default()))),
        CommandSpec::new("ungroup", "Ungroup", run_ungroup)
            .menu(&["Tools", "Ungroup"])
            .alias(&["boinhom"])
            .params("{name?} | {handles?: selection} → removes the groups (the objects stay)")
            .interactive(|_| Ok(Box::new(UngroupM::default()))),
        CommandSpec::new("groupedit", "Group Edit", run_groupedit)
            .menu(&["Tools", "Group Edit"])
            .params("{name, add?: [hex], remove?: [hex], rename?: new name, description?, selectable?: bool}"),
        CommandSpec::new("groups", "List Groups", run_groups).params("{} → [{name, description, selectable, count}]").enabled(has_doc).noundo(),
        CommandSpec::new("pickstyle", "Group Selection On/Off", run_pickstyle)
            .menu(&["Tools", "Group Selection On/Off"])
            .key("Cmd+Shift+A")
            .params("{on?: bool (default: toggle)}")
            .noundo(),
    ]
}

/// Print a result's message (interactive commands; JSON callers get it in the result).
pub(crate) fn echo_msg(s: &mut Session, r: &Value) {
    if let Some(m) = r.get("message").and_then(Value::as_str) {
        s.echo(m.to_string());
    }
}

/// Group selection (PICKSTYLE bit 1), on by default.
pub fn group_selection_on(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("PICKSTYLE", 1) & 1 != 0).unwrap_or(true)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.chars().any(|c| matches!(c, '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | ',' | '*' | '|' | '=' | '`'))
}

/// Create a group of `members`. `name: None` makes an unnamed group (`*A1`, `*A2`…).
fn create(s: &mut Session, members: Vec<Handle>, name: Option<&str>, description: &str, selectable: bool) -> Result<Value> {
    let d = s.doc()?;
    let mut seen = std::collections::HashSet::new();
    let members: Vec<Handle> = members.into_iter().filter(|h| d.entity(*h).is_some() && seen.insert(*h)).collect();
    if members.is_empty() {
        return Err(bad("group", "chọn ít nhất một đối tượng"));
    }
    let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => {
            if !valid_name(n) {
                return Err(bad("group", format!("tên nhóm không hợp lệ `{n}`")));
            }
            if d.groups.iter().any(|g| g.name.eq_ignore_ascii_case(n)) {
                return Err(bad("group", format!("nhóm `{n}` đã có")));
            }
            n.to_string()
        }
        None => {
            let mut k = 1;
            while d.groups.iter().any(|g| g.name.eq_ignore_ascii_case(&format!("*A{k}"))) {
                k += 1;
            }
            format!("*A{k}")
        }
    };
    let n = members.len();
    s.doc_mut()?.groups.push(Group { name: name.clone(), description: description.chars().take(255).collect(), selectable, members });
    s.touch();
    let msg = format!("Đã tạo nhóm {name} ({n} đối tượng).");
    Ok(json!({ "name": name, "count": n, "message": msg }))
}

fn run_group(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    create(s, hs, str_param(p, "name"), str_param(p, "description").unwrap_or(""), bool_or(p, "selectable", true))
}

/// Remove the named group, or every group holding one of the objects.
fn ungroup(s: &mut Session, name: Option<&str>, hs: &[Handle]) -> Result<Value> {
    let d = s.doc_mut()?;
    let before = d.groups.len();
    let mut removed: Vec<String> = Vec::new();
    d.groups.retain(|g| {
        let hit = match name {
            Some(n) => g.name.eq_ignore_ascii_case(n),
            None => g.members.iter().any(|m| hs.contains(m)),
        };
        if hit {
            removed.push(g.name.clone());
        }
        !hit
    });
    if d.groups.len() == before {
        return Err(bad("ungroup", "không có nhóm nào trong các đối tượng đã chọn"));
    }
    s.touch();
    let msg = format!("Đã bỏ nhóm {}.", removed.join(", "));
    Ok(json!({ "removed": removed, "message": msg }))
}

fn run_ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    ungroup(s, str_param(p, "name"), &hs)
}

fn handles(p: &Value, key: &str) -> Vec<Handle> {
    p.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle))).collect())
        .unwrap_or_default()
}

fn run_groupedit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("groupedit", "`name` is required"))?.to_string();
    let add = handles(p, "add");
    let remove = handles(p, "remove");
    let rename = str_param(p, "rename").map(str::trim).map(str::to_string);
    let d = s.doc()?;
    let i = d.groups.iter().position(|g| g.name.eq_ignore_ascii_case(&name)).ok_or_else(|| bad("groupedit", format!("không có nhóm `{name}`")))?;
    if let Some(r) = &rename
        && (!valid_name(r) || d.groups.iter().enumerate().any(|(j, g)| j != i && g.name.eq_ignore_ascii_case(r)))
    {
        return Err(bad("groupedit", format!("không đổi tên được thành `{r}`")));
    }
    let add: Vec<Handle> = add.into_iter().filter(|h| d.entity(*h).is_some()).collect();
    let d = s.doc_mut()?;
    let Some(g) = d.groups.get_mut(i) else { return Err(bad("groupedit", "nhóm không còn")) };
    for h in add {
        if !g.members.contains(&h) {
            g.members.push(h);
        }
    }
    g.members.retain(|h| !remove.contains(h));
    if let Some(r) = rename {
        g.name = r;
    }
    if let Some(desc) = str_param(p, "description") {
        g.description = desc.chars().take(255).collect();
    }
    if let Some(sel) = p.get("selectable").and_then(Value::as_bool) {
        g.selectable = sel;
    }
    let out = json!({ "name": g.name, "count": g.members.len() });
    d.prune_groups();
    s.touch();
    Ok(out)
}

fn run_groups(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let list: Vec<Value> = d
        .groups
        .iter()
        .map(|g| {
            let n = g.members.iter().filter(|m| d.entity(**m).is_some()).count();
            json!({ "name": g.name, "description": g.description, "selectable": g.selectable, "count": n })
        })
        .collect();
    let msg = if list.is_empty() { "Bản vẽ chưa có nhóm nào.".to_string() } else { format!("{} nhóm.", list.len()) };
    Ok(json!({ "groups": list, "message": msg }))
}

fn run_pickstyle(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(!group_selection_on(s));
    let d = s.doc_mut()?;
    let cur = d.header.i64("PICKSTYLE", 1);
    d.header.set("PICKSTYLE", cadcraft_doc::HVal::Int(if on { cur | 1 } else { cur & !1 }));
    let msg = if on { "Chọn theo nhóm: BẬT" } else { "Chọn theo nhóm: TẮT" };
    Ok(json!({ "on": on, "message": msg }))
}

/// GROUP: "Select objects or [Name/Description]".
#[derive(Default)]
struct GroupM {
    sel: SelectPhase,
    started: bool,
    name: Option<String>,
    description: String,
    asking: Option<&'static str>,
}

impl Interactive for GroupM {
    fn name(&self) -> &'static str {
        "GROUP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.started = true;
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let picked = self.sel.picked.clone();
            let r = create(s, picked, None, "", true)?;
            echo_msg(s, &r);
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.asking {
            Some("name") => Prompt::new("Tên nhóm (Enter = nhóm không tên)", Accept::TEXT),
            Some(_) => Prompt::new("Mô tả nhóm", Accept::TEXT),
            None => self.sel.prompt().kw(&["Name", "Description"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(what) = self.asking.take() {
            let t = match i {
                Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
                _ => String::new(),
            };
            if what == "name" {
                if !t.is_empty() {
                    if !valid_name(&t) || s.doc()?.groups.iter().any(|g| g.name.eq_ignore_ascii_case(&t)) {
                        s.echo(format!("Tên nhóm `{t}` không dùng được (trùng hoặc có ký tự cấm)."));
                        self.asking = Some("name");
                        return Ok(Step::Continue);
                    }
                    self.name = Some(t);
                }
            } else {
                self.description = t;
            }
            return Ok(Step::Continue);
        }
        if let Input::Keyword(k) | Input::Text(k) = &i {
            match k.trim().to_ascii_lowercase().as_str() {
                "n" | "name" => {
                    self.asking = Some("name");
                    return Ok(Step::Continue);
                }
                "d" | "description" => {
                    self.asking = Some("description");
                    return Ok(Step::Continue);
                }
                _ => {}
            }
        }
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => {
                let name = self.name.clone();
                let r = create(s, hs, name.as_deref(), &self.description.clone(), true)?;
                echo_msg(s, &r);
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
        }
    }
}

/// UNGROUP: select a member of each group to remove.
#[derive(Default)]
struct UngroupM {
    sel: SelectPhase,
}

impl Interactive for UngroupM {
    fn name(&self) -> &'static str {
        "UNGROUP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let picked = self.sel.picked.clone();
            let r = ungroup(s, None, &picked)?;
            echo_msg(s, &r);
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => {
                match ungroup(s, None, &hs) {
                    Ok(r) => echo_msg(s, &r),
                    Err(e) => s.echo(e.to_string()),
                }
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &mut Session, n: usize) -> Vec<Handle> {
        (0..n)
            .map(|i| {
                let y = i as f64;
                s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
                s.doc().unwrap().model.last().unwrap().handle
            })
            .collect()
    }

    #[test]
    fn group_selects_as_a_whole_and_survives_dxf() {
        let mut s = Session::new();
        let hs = lines(&mut s, 4);
        let hex = |v: &[Handle]| v.iter().map(|h| h.hex()).collect::<Vec<_>>();
        s.execute("group", &json!({ "handles": hex(&hs[..3]), "name": "MONG-M1", "description": "Móng M1" })).unwrap();
        assert!(s.execute("group", &json!({ "handles": hex(&hs[..1]), "name": "MONG-M1" })).is_err(), "duplicate name");
        // Clicking one member selects the group.
        s.set_selection(Vec::new());
        s.idle_click(cadcraft_geom::Vec2::new(5.0, 1.0), false).unwrap();
        let mut sel = s.selection();
        sel.sort();
        assert_eq!(sel, hs[..3].to_vec());
        // Group selection off: just the object.
        s.execute("pickstyle", &json!({ "on": false })).unwrap();
        s.set_selection(Vec::new());
        s.idle_click(cadcraft_geom::Vec2::new(5.0, 1.0), false).unwrap();
        assert_eq!(s.selection(), vec![hs[1]]);
        s.execute("pickstyle", &json!({ "on": true })).unwrap();
        // Unnamed group, edit, list.
        let r = s.execute("group", &json!({ "handles": hex(&hs[3..]) })).unwrap();
        assert_eq!(r["name"], "*A1");
        s.execute("groupedit", &json!({ "name": "MONG-M1", "remove": hex(&hs[2..3]), "rename": "MONG-M2" })).unwrap();
        let l = s.execute("groups", &json!({})).unwrap();
        assert_eq!(l["groups"][0]["name"], "MONG-M2");
        assert_eq!(l["groups"][0]["count"], 2);
        // Erasing members leaves the group with the rest; DXF keeps it.
        s.execute("erase", &json!({ "handles": [hs[0].hex()] })).unwrap();
        let bytes = cadcraft_io::write(s.doc().unwrap(), "g.dxf").unwrap();
        let back = cadcraft_io::read(&bytes, "g.dxf").unwrap();
        let g = back.groups.iter().find(|g| g.name == "MONG-M2").unwrap();
        assert_eq!(g.members, vec![hs[1]]);
        assert_eq!(g.description, "Móng M1");
        assert!(back.groups.iter().any(|g| g.name == "*A1" && g.members == vec![hs[3]]));
        // Ungroup by a member; undo brings it back.
        s.set_selection(vec![hs[1]]);
        s.execute("ungroup", &json!({})).unwrap();
        assert!(!s.doc().unwrap().groups.iter().any(|g| g.name == "MONG-M2"));
        s.execute("undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().groups.iter().any(|g| g.name == "MONG-M2"));
    }

    #[test]
    fn interactive_group_with_a_name() {
        let mut s = Session::new();
        let hs = lines(&mut s, 2);
        s.set_selection(Vec::new());
        s.start("group").unwrap();
        s.cmdline("n").unwrap();
        s.cmdline("COC-K1").unwrap();
        s.cmdline("5,0").unwrap();
        s.cmdline("5,1").unwrap();
        s.cmdline("").unwrap();
        let g = s.doc().unwrap().groups.iter().find(|g| g.name == "COC-K1").cloned().unwrap();
        assert_eq!(g.members.len(), 2);
        assert!(g.members.contains(&hs[0]) && g.members.contains(&hs[1]));
    }
}
