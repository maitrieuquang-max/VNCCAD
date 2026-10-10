//! VNCCad: dynamic block properties (DYNPROP, `dynprop.list`, `dynprop.set`).
//!
//! A reference to a dynamic block shows the definition's parameters as properties. Setting one
//! rebuilds the reference's geometry from the definition — linear parameters stretch, move or
//! array entities, flip parameters mirror them, visibility parameters show one named state —
//! into a new anonymous block (`*U…`) that remembers its definition and values, as AutoCAD does.

use std::sync::Arc;

use cadcraft_doc::{Block, DynActionKind, DynKind, DynParam, DynRef, DynValue, Entity, EntityKind, Handle};
use cadcraft_geom::{Mat3, Vec2};
use serde_json::{Value, json};

use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("dynprop", "Dynamic Block Properties", run_set)
            .menu(&["Modify", "Dynamic Block Properties"])
            .alias(&["thuoctinhdong", "bdynprop"])
            .params("{handle, name: parameter name, value: number | state name | true/false (flip)} → sets one property")
            .interactive(|_| Ok(Box::new(DynPropM::default())))
            .enabled(has_doc),
        CommandSpec::new("dynprop.list", "List Dynamic Block Properties", run_list)
            .params("{handle} → {block, properties: [{name, kind, value, states?}]}")
            .enabled(has_doc)
            .noundo(),
    ]
}

// ------------------------------------------------------------------ evaluation

/// Point-in-polygon (even–odd), points on the edge count as inside.
fn in_polygon(poly: &[Vec2], p: Vec2) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let tol = 1e-9 * (1.0 + p.x.abs().max(p.y.abs()));
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        // On the edge.
        let ab = *b - *a;
        let len2 = ab.dot(ab);
        if len2 > 0.0 {
            let t = ((p - *a).dot(ab) / len2).clamp(0.0, 1.0);
            if (*a + ab * t).dist(p) <= tol.max(len2.sqrt() * 1e-9) {
                return true;
            }
        }
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// The current value of a parameter for these values (else the definition's).
fn value_of(p: &DynParam, values: &[(i64, DynValue)]) -> DynValue {
    values.iter().find(|(id, _)| *id == p.id).map(|(_, v)| v.clone()).unwrap_or_else(|| p.default_value())
}

/// The geometry of a dynamic block for some property values. Array copies get handle 0.
pub fn instantiate(def: &Block, values: &[(i64, DynValue)]) -> Vec<Entity> {
    let mut ents: Vec<Entity> = def.entities.iter().map(|e| (**e).clone()).collect();
    let Some(dd) = &def.dyn_def else { return ents };
    for p in &dd.params {
        let v = value_of(p, values);
        match (p.kind, &v) {
            (DynKind::Linear, DynValue::Distance(dist)) => {
                let d0 = p.base.dist(p.end);
                let axis = p.end - p.base;
                if d0.is_nan() || d0 <= 1e-12 || !dist.is_finite() {
                    continue;
                }
                let dir = axis * (1.0 / d0);
                let delta = dist - d0;
                for a in &p.actions {
                    let mv = dir.rotate(a.angle) * (delta * a.factor);
                    match a.kind {
                        DynActionKind::Stretch => {
                            let frame = a.frame.clone();
                            for e in ents.iter_mut().filter(|e| a.entities.contains(&e.handle)) {
                                super::modify::stretch_entity_by(e, &|q| in_polygon(&frame, q), mv);
                            }
                        }
                        DynActionKind::Move => {
                            for e in ents.iter_mut().filter(|e| a.entities.contains(&e.handle)) {
                                e.kind.transform(&Mat3::translate(mv));
                            }
                        }
                        DynActionKind::Array if a.spacing > 1e-12 => {
                            // As many items as fit in the distance (AutoCAD): the original and n copies.
                            let n = (((dist / a.spacing) + 1e-9).floor() - 1.0).clamp(0.0, 5000.0) as usize;
                            let src: Vec<Entity> = ents.iter().filter(|e| a.entities.contains(&e.handle)).cloned().collect();
                            for k in 1..=n {
                                for e in &src {
                                    let mut c = e.clone();
                                    c.handle = Handle(0);
                                    c.kind.transform(&Mat3::translate(dir * (a.spacing * k as f64)));
                                    ents.push(c);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            (DynKind::Flip, DynValue::Flipped(true)) => {
                if p.base.dist(p.end) > 1e-12 {
                    let m = Mat3::mirror(p.base, p.end);
                    for a in p.actions.iter().filter(|a| a.kind == DynActionKind::Flip) {
                        for e in ents.iter_mut().filter(|e| a.entities.contains(&e.handle)) {
                            e.kind.transform(&m);
                        }
                    }
                }
            }
            (DynKind::Visibility, DynValue::State(name)) => {
                if let Some(st) = p.states.iter().find(|s| s.name == *name) {
                    ents.retain(|e| !p.controlled.contains(&e.handle) || st.visible.contains(&e.handle));
                }
            }
            _ => {}
        }
    }
    ents
}

/// The definition and current values of the block an INSERT shows.
fn dyn_of(s: &Session, h: Handle) -> Result<(Handle, String, Arc<Block>, Vec<(i64, DynValue)>)> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| bad("dynprop", format!("no entity {}", h.hex())))?;
    let EntityKind::Insert(ins) = &e.kind else { return Err(bad("dynprop", "không phải block")) };
    let b = d.block(&ins.block).ok_or_else(|| bad("dynprop", format!("không có block {}", ins.block)))?;
    if b.dyn_def.is_some() {
        return Ok((h, b.name.clone(), b.clone(), Vec::new()));
    }
    if let Some(r) = &b.dyn_ref
        && let Some(def) = d.block(&r.source).filter(|x| x.dyn_def.is_some())
    {
        return Ok((h, def.name.clone(), def.clone(), r.values.clone()));
    }
    Err(bad("dynprop", format!("block {} không phải block động", ins.block)))
}

fn kind_name(k: DynKind) -> &'static str {
    match k {
        DynKind::Linear => "linear",
        DynKind::Flip => "flip",
        DynKind::Visibility => "visibility",
    }
}

fn list_json(def: &Block, values: &[(i64, DynValue)]) -> Value {
    let props: Vec<Value> = def
        .dyn_def
        .iter()
        .flat_map(|dd| dd.params.iter())
        .map(|p| {
            let v = value_of(p, values);
            let mut o = json!({ "name": p.name, "kind": kind_name(p.kind), "value": v.text() });
            if p.kind == DynKind::Visibility {
                o["states"] = json!(p.states.iter().map(|s| s.name.clone()).collect::<Vec<_>>());
            }
            if p.kind == DynKind::Flip {
                o["labels"] = json!(p.labels);
            }
            o
        })
        .collect();
    json!({ "block": def.name, "properties": props })
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let h = p.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex).ok_or_else(|| bad("dynprop.list", "`handle` is required"))?;
    let (_, _, def, values) = dyn_of(s, h)?;
    Ok(list_json(&def, &values))
}

/// Parse a typed value for a parameter.
fn parse_value(p: &DynParam, v: &Value) -> Result<DynValue> {
    let text = match v {
        Value::String(t) => t.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    };
    match p.kind {
        DynKind::Linear => {
            let d =
                v.as_f64().or_else(|| text.replace(',', ".").parse::<f64>().ok()).ok_or_else(|| bad("dynprop", format!("{}: cần một số", p.name)))?;
            if !(d.is_finite() && d > 0.0 && d < 1e12) {
                return Err(bad("dynprop", format!("{}: giá trị phải dương", p.name)));
            }
            Ok(DynValue::Distance(d))
        }
        DynKind::Flip => {
            let t = text.to_lowercase();
            let on = matches!(t.as_str(), "true" | "1" | "yes" | "y" | "co" | "có" | "lat" | "lật" | "flipped")
                || p.labels.get(1).is_some_and(|l| l.eq_ignore_ascii_case(&text));
            Ok(DynValue::Flipped(on))
        }
        DynKind::Visibility => {
            let st = p
                .states
                .iter()
                .find(|s| s.name == text)
                .or_else(|| p.states.iter().find(|s| s.name.eq_ignore_ascii_case(&text)))
                .or_else(|| text.parse::<usize>().ok().and_then(|i| p.states.get(i.saturating_sub(1))))
                .ok_or_else(|| bad("dynprop", format!("{}: không có trạng thái `{text}`", p.name)))?;
            Ok(DynValue::State(st.name.clone()))
        }
    }
}

/// Give an INSERT new property values: new anonymous geometry (or the definition itself when
/// every value is the definition's).
fn apply_values(s: &mut Session, h: Handle, def_name: &str, def: &Block, values: Vec<(i64, DynValue)>) -> Result<String> {
    let dd = def.dyn_def.clone().unwrap_or_default();
    let all_default = dd.params.iter().all(|p| value_of(p, &values) == p.default_value());
    let target = if all_default {
        def_name.to_string()
    } else {
        let ents = instantiate(def, &values);
        let d = s.doc_mut()?;
        let mut n = d.blocks.len() + 1;
        let name = loop {
            let cand = format!("*U{n}");
            if d.block(&cand).is_none() {
                break cand;
            }
            n += 1;
        };
        let mut b = Block::new(&name);
        b.base = def.base;
        b.anonymous = true;
        b.dyn_ref = Some(DynRef { source: def_name.to_string(), values });
        for mut e in ents {
            e.handle = d.new_handle();
            b.entities.push(e);
        }
        d.blocks.insert(name.clone(), Arc::new(b));
        name
    };
    let t = target.clone();
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind {
            ins.block = t;
        }
    })?;
    Ok(target)
}

fn set_one(s: &mut Session, h: Handle, name: &str, v: &Value) -> Result<Value> {
    let (h, def_name, def, mut values) = dyn_of(s, h)?;
    let dd = def.dyn_def.clone().unwrap_or_default();
    let p = dd
        .params
        .iter()
        .find(|p| p.name == name)
        .or_else(|| dd.params.iter().find(|p| p.name.eq_ignore_ascii_case(name)))
        .or_else(|| name.parse::<usize>().ok().and_then(|i| dd.params.get(i.saturating_sub(1))))
        .ok_or_else(|| bad("dynprop", format!("block {def_name} không có thuộc tính `{name}`")))?;
    let nv = parse_value(p, v)?;
    values.retain(|(id, _)| *id != p.id);
    values.push((p.id, nv.clone()));
    let block = apply_values(s, h, &def_name, &def, values)?;
    Ok(json!({ "block": block, "message": format!("{} = {}", p.name, nv.text()) }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let h = p.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex).ok_or_else(|| bad("dynprop", "`handle` is required"))?;
    let name = p.get("name").and_then(Value::as_str).ok_or_else(|| bad("dynprop", "`name` is required"))?;
    let v = p.get("value").cloned().unwrap_or(Value::Null);
    set_one(s, h, name, &v)
}

// ------------------------------------------------------------------ DYNPROP

#[derive(Default)]
struct DynPropM {
    target: Option<Handle>,
    param: Option<String>,
}

impl DynPropM {
    fn params(&self, s: &Session) -> Vec<DynParam> {
        self.target.and_then(|h| dyn_of(s, h).ok()).and_then(|(_, _, def, _)| def.dyn_def.clone()).map(|d| d.params).unwrap_or_default()
    }
    fn show(&self, s: &mut Session) {
        if let Some(Ok((_, _, def, values))) = self.target.map(|h| dyn_of(s, h)) {
            let l = list_json(&def, &values);
            s.echo(format!("Block động {}:", def.name));
            for (i, p) in l["properties"].as_array().cloned().unwrap_or_default().iter().enumerate() {
                let extra = p
                    .get("states")
                    .and_then(Value::as_array)
                    .map(|st| format!(" (trạng thái: {})", st.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
                s.echo(format!(
                    "  {}. {} = {}{}",
                    i + 1,
                    p["name"].as_str().unwrap_or(""),
                    p["value"].as_str().unwrap_or(""),
                    extra.unwrap_or_default()
                ));
            }
        }
    }
}

impl Interactive for DynPropM {
    fn name(&self) -> &'static str {
        "dynprop"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        // A dynamic block already selected (pickfirst).
        let sel = s.selection();
        if let Some(h) = sel.into_iter().find(|h| dyn_of(s, *h).is_ok()) {
            self.target = Some(h);
            self.show(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match (&self.target, &self.param) {
            (None, _) => Prompt::new("Chọn block động", Accept::SELECT),
            (Some(_), None) => {
                let names: Vec<String> = self.params(s).iter().map(|p| p.name.clone()).collect();
                Prompt::new(format!("Thuộc tính cần đổi (tên hoặc số 1–{})", names.len()), Accept::TEXT)
            }
            (Some(_), Some(n)) => {
                let p = self.params(s).into_iter().find(|p| p.name == *n);
                match p.map(|p| p.kind) {
                    Some(DynKind::Linear) => Prompt::new(format!("Giá trị mới của {n}"), Accept::POINT_OR_NUMBER),
                    Some(DynKind::Flip) => Prompt::new(format!("{n}: lật?"), Accept::TEXT).kw(&["Yes", "No"]),
                    _ => Prompt::new(format!("Trạng thái {n} (tên hoặc số)"), Accept::TEXT),
                }
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Enter | Input::Cancel) {
            return Ok(if matches!(i, Input::Cancel) { Step::Cancel } else { Step::Done });
        }
        match (self.target, self.param.clone()) {
            (None, _) => {
                let Input::Pick(hs) = i else { return Ok(Step::Continue) };
                let found = hs.into_iter().find(|h| dyn_of(s, *h).is_ok());
                match found {
                    Some(h) => {
                        self.target = Some(h);
                        self.show(s);
                    }
                    None => s.echo("Đối tượng chọn không phải block động."),
                }
                Ok(Step::Continue)
            }
            (Some(_), None) => {
                let t = match i {
                    Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
                    _ => return Ok(Step::Continue),
                };
                let ps = self.params(s);
                let p =
                    ps.iter().find(|p| p.name.eq_ignore_ascii_case(&t)).or_else(|| t.parse::<usize>().ok().and_then(|k| ps.get(k.saturating_sub(1))));
                match p {
                    Some(p) => self.param = Some(p.name.clone()),
                    None => s.echo(format!("Không có thuộc tính `{t}`.")),
                }
                Ok(Step::Continue)
            }
            (Some(h), Some(n)) => {
                let v = match i {
                    Input::Point(pt) => {
                        // A point: the distance from the parameter's base point (in the drawing).
                        let base = self.params(s).into_iter().find(|p| p.name == n).map(|p| p.base).unwrap_or_default();
                        let world = insert_xform(s, h).map(|m| m.apply(base)).unwrap_or(base);
                        json!(world.dist(pt))
                    }
                    Input::Text(t) | Input::Keyword(t) => json!(t),
                    _ => return Ok(Step::Continue),
                };
                let r = set_one(s, h, &n, &v)?;
                super::group::echo_msg(s, &r);
                self.param = None;
                self.show(s);
                Ok(Step::Continue)
            }
        }
    }
}

/// Block → drawing transform of an INSERT.
fn insert_xform(s: &Session, h: Handle) -> Option<Mat3> {
    let d = s.doc().ok()?;
    let e = d.entity(h)?;
    let EntityKind::Insert(ins) = &e.kind else { return None };
    let base = d.block(&ins.block).map(|b| b.base.xy()).unwrap_or_default();
    Some(ins.transform(base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, DynAction, DynDef, DynState, Line};
    use cadcraft_geom::Vec3;

    fn line(h: u64, a: (f64, f64), b: (f64, f64)) -> Entity {
        Entity {
            handle: Handle(h),
            common: Common::default(),
            kind: EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }),
        }
    }

    /// A 1000 × 200 rectangle with a stretch on its right side, a visibility parameter showing
    /// a marker line or not, and a flip about the y axis.
    fn sample() -> Block {
        let mut b = Block::new("DAM");
        b.anonymous = false;
        for e in [
            line(1, (0.0, 0.0), (1000.0, 0.0)),
            line(2, (1000.0, 0.0), (1000.0, 200.0)),
            line(3, (1000.0, 200.0), (0.0, 200.0)),
            line(4, (0.0, 200.0), (0.0, 0.0)),
            line(5, (100.0, 50.0), (200.0, 50.0)),
        ] {
            b.entities.push(e);
        }
        let stretch = DynAction {
            kind: DynActionKind::Stretch,
            entities: vec![Handle(1), Handle(2), Handle(3)],
            frame: vec![Vec2::new(900.0, -10.0), Vec2::new(1100.0, -10.0), Vec2::new(1100.0, 210.0), Vec2::new(900.0, 210.0)],
            factor: 1.0,
            ..DynAction::default()
        };
        let flip = DynAction { kind: DynActionKind::Flip, entities: vec![Handle(5)], ..DynAction::default() };
        b.dyn_def = Some(DynDef {
            params: vec![
                DynParam {
                    id: 10,
                    name: "Dai".into(),
                    kind: DynKind::Linear,
                    base: Vec2::new(0.0, 0.0),
                    end: Vec2::new(1000.0, 0.0),
                    actions: vec![stretch],
                    ..DynParam::default()
                },
                DynParam {
                    id: 11,
                    name: "Hien".into(),
                    kind: DynKind::Visibility,
                    controlled: vec![Handle(5)],
                    states: vec![DynState { name: "Co".into(), visible: vec![Handle(5)] }, DynState { name: "Khong".into(), visible: vec![] }],
                    ..DynParam::default()
                },
                DynParam {
                    id: 12,
                    name: "Lat".into(),
                    kind: DynKind::Flip,
                    base: Vec2::new(500.0, 0.0),
                    end: Vec2::new(500.0, 200.0),
                    actions: vec![flip],
                    ..DynParam::default()
                },
            ],
        });
        b
    }

    fn bounds(es: &[Entity]) -> (f64, f64) {
        let xs: Vec<f64> = es.iter().flat_map(|e| if let EntityKind::Line(l) = &e.kind { vec![l.a.x, l.b.x] } else { vec![] }).collect();
        (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max))
    }

    #[test]
    fn stretch_visibility_and_flip() {
        let b = sample();
        let es = instantiate(&b, &[(10, DynValue::Distance(1500.0))]);
        assert_eq!(bounds(&es), (0.0, 1500.0));
        let es = instantiate(&b, &[(11, DynValue::State("Khong".into()))]);
        assert_eq!(es.len(), 4);
        let es = instantiate(&b, &[(12, DynValue::Flipped(true))]);
        let marker = es.iter().find(|e| e.handle == Handle(5)).unwrap();
        let EntityKind::Line(l) = &marker.kind else { panic!() };
        assert!((l.a.x - 900.0).abs() < 1e-9 && (l.b.x - 800.0).abs() < 1e-9);
    }

    #[test]
    fn dynprop_command_makes_an_anonymous_block() {
        let mut s = Session::new();
        let d = s.doc_mut().unwrap();
        d.blocks.insert("DAM".into(), Arc::new(sample()));
        let h = d
            .add(
                &cadcraft_doc::Space::Model,
                Common::default(),
                EntityKind::Insert(cadcraft_doc::Insert {
                    block: "DAM".into(),
                    insert: Vec3::new(10.0, 0.0, 0.0),
                    scale: Vec3::new(1.0, 1.0, 1.0),
                    rotation: 0.0,
                    attribs: Vec::new(),
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                }),
            )
            .unwrap();
        let r = s.execute("dynprop", &json!({ "handle": h.hex(), "name": "Dai", "value": 2000 })).unwrap();
        let name = r["block"].as_str().unwrap().to_string();
        assert!(name.starts_with("*U"));
        let l = s.execute("dynprop.list", &json!({ "handle": h.hex() })).unwrap();
        assert_eq!(l["properties"][0]["value"], "2000");
        assert_eq!(l["properties"][1]["states"], json!(["Co", "Khong"]));
        let b = s.doc().unwrap().block(&name).unwrap().clone();
        assert_eq!(bounds(&b.entities.iter().map(|e| (**e).clone()).collect::<Vec<_>>()), (0.0, 2000.0));
        // Back to the definition's values: the reference uses the definition again.
        let r = s.execute("dynprop", &json!({ "handle": h.hex(), "name": "1", "value": "1000" })).unwrap();
        assert_eq!(r["block"], "DAM");
        // Interactive: pick, choose the visibility property by number, give a state.
        s.set_selection(vec![]);
        s.cmdline("DYNPROP").unwrap();
        s.input(Input::Pick(vec![h])).unwrap();
        s.cmdline("2").unwrap();
        s.cmdline("Khong").unwrap();
        s.input(Input::Enter).unwrap();
        assert!(s.running.is_none());
        let l = s.execute("dynprop.list", &json!({ "handle": h.hex() })).unwrap();
        assert_eq!(l["properties"][1]["value"], "Khong");
        // One undo step per change.
        s.execute("undo", &json!({})).unwrap();
        let l = s.execute("dynprop.list", &json!({ "handle": h.hex() })).unwrap();
        assert_eq!(l["properties"][1]["value"], "Co");
    }
}
