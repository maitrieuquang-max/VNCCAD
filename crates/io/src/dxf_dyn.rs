//! VNCCad: dynamic blocks from AutoCAD files.
//!
//! A dynamic block definition's BLOCK_RECORD has an extension dictionary entry
//! `ACAD_ENHANCEDBLOCK` → an `ACAD_EVALUATION_GRAPH` owning the parameter and action objects
//! (`BLOCKLINEARPARAMETER`, `BLOCKSTRETCHACTION`…). A reference whose properties were changed
//! inserts an anonymous `*U` block; its extension dictionary `AcDbBlockRepresentation` names the
//! definition (`AcDbRepData` → 340) and caches the property values per parameter node
//! (`AppDataCache` → `ACAD_ENHANCEDBLOCKDATA` → XRECORD named by node id).

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_doc::{Drawing, DynAction, DynActionKind, DynDef, DynKind, DynParam, DynRef, DynState, DynValue, Handle};
use cadcraft_dxf::Tag;
use cadcraft_geom::Vec2;

const MAX: usize = 1_000_000;

/// The extension dictionary handle of a record (`{ACAD_XDICTIONARY 360 … }`).
pub(crate) fn xdict(tags: &[Tag]) -> Option<String> {
    let start = tags.iter().position(|t| t.code == 102 && t.str() == "{ACAD_XDICTIONARY")?;
    tags.get(start + 1..)?
        .iter()
        .take_while(|t| !(t.code == 102 && t.str() == "}"))
        .find(|t| t.code == 360)
        .map(|t| t.str().trim().to_ascii_uppercase())
}

fn up(s: String) -> String {
    s.trim().to_ascii_uppercase()
}

/// Objects collected from the OBJECTS section.
#[derive(Default)]
pub(crate) struct DynObjects {
    /// Dictionary handle → (entry name, entry handle).
    dicts: HashMap<String, Vec<(String, String)>>,
    /// Parameter, action, graph and representation objects: handle → (type, groups).
    objs: HashMap<String, (String, Vec<Tag>)>,
    /// Owner handle → objects it owns (parameters and actions by their graph).
    by_owner: HashMap<String, Vec<String>>,
}

impl DynObjects {
    pub(crate) fn add(&mut self, kind: &str, tags: &[Tag]) {
        let Some(h) = tags.iter().find(|t| t.code == 5).map(|t| up(t.str())) else { return };
        match kind {
            "DICTIONARY" | "ACDBDICTIONARYWDFLT" if self.dicts.len() < MAX => {
                let mut entries = Vec::new();
                let mut name: Option<String> = None;
                for t in tags {
                    match t.code {
                        3 => name = Some(t.str()),
                        350 | 360 => {
                            if let Some(n) = name.take() {
                                entries.push((n, up(t.str())));
                            }
                        }
                        _ => {}
                    }
                }
                self.dicts.insert(h, entries);
            }
            "SPATIAL_FILTER" if self.objs.len() < MAX => {
                self.objs.insert(h, (kind.to_string(), tags.to_vec()));
            }
            k if ((k.starts_with("BLOCK") && (k.ends_with("PARAMETER") || k.ends_with("ACTION"))) || k == "ACDB_BLOCKREPRESENTATION_DATA")
                && self.objs.len() < MAX =>
            {
                if let Some(o) = crate::dxf_ext::owner(tags) {
                    self.by_owner.entry(up(o)).or_default().push(h.clone());
                }
                self.objs.insert(h, (k.to_string(), tags.to_vec()));
            }
            _ => {}
        }
    }

    fn entry(&self, dict: &str, name: &str) -> Option<&String> {
        self.dicts.get(dict)?.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, h)| h)
    }
}

/// Groups after a subclass marker, up to the next marker.
fn section<'a>(tags: &'a [Tag], marker: &str) -> &'a [Tag] {
    let Some(i) = tags.iter().position(|t| t.code == 100 && t.str() == marker) else { return &[] };
    let rest = tags.get(i + 1..).unwrap_or(&[]);
    let j = rest.iter().position(|t| t.code == 100).unwrap_or(rest.len());
    rest.get(..j).unwrap_or(&[])
}

fn first_f(tags: &[Tag], code: i32) -> Option<f64> {
    tags.iter().find(|t| t.code == code).map(Tag::f64).filter(|v| v.is_finite())
}

fn pt(tags: &[Tag], cx: i32, cy: i32) -> Vec2 {
    Vec2::new(first_f(tags, cx).unwrap_or(0.0), first_f(tags, cy).unwrap_or(0.0))
}

fn handles(tags: &[Tag], code: i32) -> Vec<Handle> {
    tags.iter().filter(|t| t.code == code).filter_map(|t| Handle::parse_hex(&t.str())).filter(|h| h.0 != 0).collect()
}

fn node_id(tags: &[Tag]) -> i64 {
    section(tags, "AcDbEvalExpr").iter().find(|t| t.code == 90).map(Tag::i64).unwrap_or(-1)
}

fn element_name(tags: &[Tag]) -> String {
    section(tags, "AcDbBlockElement").iter().find(|t| t.code == 300).map(Tag::str).unwrap_or_default()
}

fn param(kind: &str, tags: &[Tag]) -> Option<DynParam> {
    let id = node_id(tags);
    let mut p = DynParam { id, name: element_name(tags), ..DynParam::default() };
    match kind {
        "BLOCKLINEARPARAMETER" | "BLOCKFLIPPARAMETER" => {
            let two = section(tags, "AcDbBlock2PtParameter");
            p.base = pt(two, 1010, 1020);
            p.end = pt(two, 1011, 1021);
            if kind == "BLOCKLINEARPARAMETER" {
                p.kind = DynKind::Linear;
                if let Some(n) = section(tags, "AcDbBlockLinearParameter").iter().find(|t| t.code == 305).map(Tag::str).filter(|n| !n.is_empty()) {
                    p.name = n;
                }
            } else {
                p.kind = DynKind::Flip;
                let f = section(tags, "AcDbBlockFlipParameter");
                if let Some(n) = f.iter().find(|t| t.code == 305).map(Tag::str).filter(|n| !n.is_empty()) {
                    p.name = n;
                }
                p.labels = [307, 308].iter().filter_map(|c| f.iter().find(|t| t.code == *c).map(Tag::str)).collect();
            }
        }
        "BLOCKVISIBILITYPARAMETER" => {
            p.kind = DynKind::Visibility;
            let v = section(tags, "AcDbBlockVisibilityParameter");
            if let Some(n) = v.iter().find(|t| t.code == 301).map(Tag::str).filter(|n| !n.is_empty()) {
                p.name = n;
            }
            for t in v {
                match t.code {
                    331 => p.controlled.extend(Handle::parse_hex(&t.str())),
                    303 => p.states.push(DynState { name: t.str(), visible: Vec::new() }),
                    332 => {
                        if let (Some(s), Some(h)) = (p.states.last_mut(), Handle::parse_hex(&t.str())) {
                            s.visible.push(h);
                        }
                    }
                    _ => {}
                }
            }
            if p.states.is_empty() {
                return None;
            }
        }
        _ => return None,
    }
    Some(p)
}

/// An action and the node id of the parameter that drives it.
fn action(kind: &str, tags: &[Tag]) -> Option<(i64, DynAction)> {
    let (k, marker) = match kind {
        "BLOCKSTRETCHACTION" => (DynActionKind::Stretch, "AcDbBlockStretchAction"),
        "BLOCKMOVEACTION" => (DynActionKind::Move, "AcDbBlockMoveAction"),
        "BLOCKARRAYACTION" => (DynActionKind::Array, "AcDbBlockArrayAction"),
        "BLOCKFLIPACTION" => (DynActionKind::Flip, "AcDbBlockFlipAction"),
        _ => return None,
    };
    let common = section(tags, "AcDbBlockAction");
    let own = section(tags, marker);
    let driver = own.iter().find(|t| (92..=95).contains(&t.code)).map(Tag::i64)?;
    let mut a = DynAction { kind: k, name: element_name(tags), entities: handles(common, 330), factor: 1.0, ..DynAction::default() };
    match k {
        DynActionKind::Stretch => {
            let xs: Vec<f64> = own.iter().filter(|t| t.code == 1011).map(Tag::f64).collect();
            let ys: Vec<f64> = own.iter().filter(|t| t.code == 1021).map(Tag::f64).collect();
            a.frame = xs.iter().zip(&ys).map(|(x, y)| Vec2::new(*x, *y)).collect();
            if a.frame.len() == 2
                && let (Some(p), Some(q)) = (a.frame.first().copied(), a.frame.get(1).copied())
            {
                // Two corners: a rectangle.
                a.frame = vec![p, Vec2::new(q.x, p.y), q, Vec2::new(p.x, q.y)];
            }
            a.factor = first_f(own, 140).unwrap_or(1.0);
            a.angle = first_f(own, 141).unwrap_or(0.0);
        }
        DynActionKind::Move => {
            a.factor = first_f(own, 140).unwrap_or(1.0);
            a.angle = first_f(own, 141).unwrap_or(0.0);
        }
        DynActionKind::Array => {
            // Column spacing (the larger of the two offsets for a one-way array).
            a.spacing = [first_f(own, 140), first_f(own, 141)].into_iter().flatten().fold(0.0f64, |m, v| m.max(v.abs()));
        }
        DynActionKind::Flip => {}
    }
    Some((driver, a))
}

/// Property values cached for a reference, by parameter.
fn values(objs: &DynObjects, xrecords: &HashMap<String, Vec<Tag>>, cache_dict: &str, def: &DynDef) -> Vec<(i64, DynValue)> {
    let mut out = Vec::new();
    for p in &def.params {
        let Some(xh) = objs.entry(cache_dict, &p.id.to_string()) else { continue };
        let Some(tags) = xrecords.get(xh) else { continue };
        let v = match p.kind {
            DynKind::Linear => {
                let xs: Vec<f64> = tags.iter().filter(|t| t.code == 10).map(Tag::f64).collect();
                let ys: Vec<f64> = tags.iter().filter(|t| t.code == 20).map(Tag::f64).collect();
                match (xs.first().zip(ys.first()), xs.get(1).zip(ys.get(1))) {
                    (Some((x0, y0)), Some((x1, y1))) => {
                        let d = Vec2::new(*x0, *y0).dist(Vec2::new(*x1, *y1));
                        if d.is_finite() { Some(DynValue::Distance(d)) } else { None }
                    }
                    _ => None,
                }
            }
            DynKind::Flip => tags.iter().filter(|t| t.code == 70).nth(2).map(|t| DynValue::Flipped(t.i64() != 0)),
            DynKind::Visibility => tags
                .iter()
                .find(|t| t.code == 1 || t.code == 300)
                .map(Tag::str)
                .filter(|s| p.states.iter().any(|st| st.name == *s))
                .map(DynValue::State),
        };
        if let Some(v) = v {
            out.push((p.id, v));
        }
    }
    out
}

/// Attach dynamic definitions and reference values to the drawing's blocks.
pub(crate) fn apply(
    d: &mut Drawing,
    objs: &DynObjects,
    xrecords: &HashMap<String, Vec<Tag>>,
    br_xdict: &HashMap<String, String>,
    br_handles: &HashMap<String, String>,
    insert_xdict: &[(String, String, Handle)],
) {
    if objs.objs.is_empty() {
        return;
    }
    // Definitions.
    let mut defs: HashMap<String, DynDef> = HashMap::new();
    for (xd, bname) in br_xdict {
        let Some(graph) = objs.entry(xd, "ACAD_ENHANCEDBLOCK") else { continue };
        let owned = objs.by_owner.get(graph).cloned().unwrap_or_default();
        let mut params = Vec::new();
        let mut actions = Vec::new();
        for h in &owned {
            let Some((kind, tags)) = objs.objs.get(h) else { continue };
            if let Some(p) = param(kind, tags) {
                params.push(p);
            } else if let Some(a) = action(kind, tags) {
                actions.push(a);
            }
        }
        for (driver, a) in actions {
            if let Some(p) = params.iter_mut().find(|p| p.id == driver) {
                p.actions.push(a);
            }
        }
        params.retain(|p| p.kind == DynKind::Visibility || !p.actions.is_empty());
        if !params.is_empty() {
            defs.insert(bname.clone(), DynDef { params });
        }
    }
    if defs.is_empty() {
        return;
    }
    // References: the anonymous block an INSERT uses, its definition and values.
    let mut refs: HashMap<String, DynRef> = HashMap::new();
    for (xd, bname, _) in insert_xdict {
        if refs.contains_key(bname) || defs.contains_key(bname) {
            continue;
        }
        let Some(rep) = objs.entry(xd, "AcDbBlockRepresentation") else { continue };
        let Some(data) = objs.entry(rep, "AcDbRepData") else { continue };
        let Some((_, tags)) = objs.objs.get(data) else { continue };
        let Some(src) = tags.iter().find(|t| t.code == 340).map(|t| up(t.str())).and_then(|h| br_handles.get(&h).cloned()) else { continue };
        let Some(def) = defs.get(&src) else { continue };
        let vals = objs
            .entry(rep, "AppDataCache")
            .and_then(|c| objs.entry(c, "ACAD_ENHANCEDBLOCKDATA"))
            .map(|c| values(objs, xrecords, c, def))
            .unwrap_or_default();
        refs.insert(bname.clone(), DynRef { source: src, values: vals });
    }
    for (name, def) in defs {
        if let Some(b) = d.blocks.get_mut(&name) {
            Arc::make_mut(b).dyn_def = Some(def);
        }
    }
    for (name, r) in refs {
        if let Some(b) = d.blocks.get_mut(&name)
            && b.dyn_ref.is_none()
        {
            Arc::make_mut(b).dyn_ref = Some(r);
        }
    }
}

/// VNCCad: XCLIP boundaries (INSERT → `ACAD_FILTER` → `SPATIAL` → SPATIAL_FILTER). The points
/// are mapped to block coordinates by the filter's inverse insert matrix (4×3, column-major).
pub(crate) fn apply_clips(d: &mut Drawing, objs: &DynObjects, insert_xdict: &[(String, String, Handle)]) {
    for (xd, _, h) in insert_xdict {
        let Some(fd) = objs.entry(xd, "ACAD_FILTER") else { continue };
        let Some(sf) = objs.entry(fd, "SPATIAL") else { continue };
        let Some((kind, tags)) = objs.objs.get(sf) else { continue };
        if kind != "SPATIAL_FILTER" {
            continue;
        }
        let t = section(tags, "AcDbSpatialFilter");
        let n = t.iter().find(|x| x.code == 70).map(Tag::i64).unwrap_or(0).clamp(0, 100_000) as usize;
        let xs: Vec<f64> = t.iter().filter(|x| x.code == 10).map(Tag::f64).take(n).collect();
        let ys: Vec<f64> = t.iter().filter(|x| x.code == 20).map(Tag::f64).take(n).collect();
        let pts: Vec<Vec2> = xs.iter().zip(&ys).map(|(x, y)| Vec2::new(*x, *y)).filter(|p| p.is_finite()).collect();
        if pts.len() < 2 {
            continue;
        }
        let m: Vec<f64> = t.iter().filter(|x| x.code == 40).map(Tag::f64).collect();
        let pts = match m.get(0..12) {
            Some(v) if v.iter().all(|x| x.is_finite()) => {
                // Columns: (a b ·) (c d ·) (· · ·) (e f ·).
                let (a, b, c, dd, e, f) = (v[0], v[1], v[3], v[4], v[9], v[10]);
                pts.iter().map(|p| Vec2::new(a * p.x + c * p.y + e, b * p.x + dd * p.y + f)).collect()
            }
            _ => pts,
        };
        let _ = d.modify_entity(*h, |e| {
            if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
                i.clip = Some(pts);
            }
        });
    }
}
