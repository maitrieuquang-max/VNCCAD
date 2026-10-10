//! The CADCraft drawing database.
//!
//! A [`Drawing`] holds the header variables, symbol tables (layers, linetypes, styles), block
//! definitions, model space and paper-space layouts. Entity collections are copy-on-write
//! ([`EntityStore`]), so a whole-drawing clone is cheap and serves as an undo snapshot.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod constraint;
mod dynblock;
mod entity;
mod extents;
pub mod formula;
mod header;
pub mod library;
mod store;
mod tables;
pub mod vnlegacy;

use std::collections::BTreeMap;

pub use cadcraft_color as color;
pub use cadcraft_geom as geom;
pub use constraint::*;
pub use dynblock::*;
pub use entity::*;
pub use extents::{MAX_BLOCK_DEPTH, entity_bounds};
pub use header::{HVal, Header};
pub use store::EntityStore;
pub use tables::*;

#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("no such layer `{0}`")]
    NoLayer(String),
    #[error("no such block `{0}`")]
    NoBlock(String),
    #[error("no such entity {0:?}")]
    NoEntity(Handle),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, DocError>;

/// Which entity collection: model space or a named layout's paper space.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Space {
    #[default]
    Model,
    Paper(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub header: Header,
    pub layers: Vec<Layer>,
    pub linetypes: Vec<Linetype>,
    pub text_styles: Vec<TextStyle>,
    pub dim_styles: Vec<DimStyle>,
    pub mleader_styles: Vec<MLeaderStyle>,
    pub table_styles: Vec<TableStyle>,
    pub blocks: BTreeMap<String, std::sync::Arc<Block>>,
    pub model: EntityStore,
    pub layouts: Vec<Layout>,
    pub views: Vec<NamedView>,
    pub ucss: Vec<Ucs>,
    pub layer_states: Vec<LayerState>,
    pub groups: Vec<Group>,
    /// Parametric constraints (GEOMCONSTRAINT / DIMCONSTRAINT).
    pub constraints: Vec<Constraint>,
    /// User parameters and parametric settings.
    pub parametric: Parametric,
    /// Next free handle.
    pub handseed: u64,
    /// VNCCad: page setup of model space (paper, plot window, plot style table), saved in the
    /// "Model" LAYOUT object like AutoCAD does. `None`: defaults (fit extents on A4/Letter).
    pub model_page: Option<PageSetup>,
    /// VNCCad: texts whose value is a field (FIELD): kept up to date after each command.
    pub fields: Vec<FieldLink>,
}

impl Default for Drawing {
    fn default() -> Self {
        Drawing::new_imperial()
    }
}

impl Drawing {
    /// VNCCad: the members of selectable groups that `hs` touch, added to `hs` (members that
    /// no longer exist are skipped). Order: `hs` first.
    pub fn expand_groups(&self, hs: &[Handle]) -> Vec<Handle> {
        let mut out: Vec<Handle> = hs.to_vec();
        if self.groups.is_empty() {
            return out;
        }
        let mut seen: std::collections::HashSet<Handle> = hs.iter().copied().collect();
        for g in self.groups.iter().filter(|g| g.selectable) {
            if g.members.iter().any(|m| hs.contains(m)) {
                for m in &g.members {
                    if self.entity(*m).is_some() && seen.insert(*m) {
                        out.push(*m);
                    }
                }
            }
        }
        out
    }

    /// VNCCad: drop group members that no longer exist, and groups left with none.
    pub fn prune_groups(&mut self) {
        let alive: Vec<Vec<Handle>> = self.groups.iter().map(|g| g.members.iter().copied().filter(|m| self.entity(*m).is_some()).collect()).collect();
        for (g, m) in self.groups.iter_mut().zip(alive) {
            g.members = m;
        }
        self.groups.retain(|g| !g.members.is_empty());
    }

    /// An empty drawing with the standard tables (inch-based, like acad.dwt-style defaults).
    pub fn new_imperial() -> Self {
        let mut d = Drawing::bare();
        d.header = Header::imperial();
        d
    }
    /// An empty metric drawing (millimetres, ISO-25 dimensions).
    pub fn new_metric() -> Self {
        let mut d = Drawing::bare();
        d.header = Header::metric();
        d.dim_styles.push(DimStyle::iso25());
        d.header.set_str("DIMSTYLE", "ISO-25");
        d
    }
    fn bare() -> Self {
        Drawing {
            header: Header::default(),
            layers: vec![Layer::default(), Layer { name: "Defpoints".into(), plot: false, ..Layer::default() }],
            linetypes: vec![
                Linetype { name: "ByBlock".into(), description: String::new(), pattern: Vec::new() },
                Linetype { name: "ByLayer".into(), description: String::new(), pattern: Vec::new() },
                Linetype::continuous(),
            ],
            text_styles: vec![TextStyle::default()],
            dim_styles: vec![DimStyle::default()],
            mleader_styles: vec![MLeaderStyle::default()],
            table_styles: vec![TableStyle::default()],
            blocks: BTreeMap::new(),
            model: EntityStore::new(),
            layouts: vec![Layout::new("Layout1", 1), Layout::new("Layout2", 2)],
            views: Vec::new(),
            ucss: Vec::new(),
            layer_states: Vec::new(),
            groups: Vec::new(),
            constraints: Vec::new(),
            parametric: Parametric::default(),
            handseed: 0x100,
            model_page: None,
            fields: Vec::new(),
        }
    }

    /// Allocate a fresh handle.
    pub fn new_handle(&mut self) -> Handle {
        let h = Handle(self.handseed);
        self.handseed = self.handseed.saturating_add(1);
        h
    }
    /// Make sure future handles don't collide with `h` (after loading a file).
    pub fn bump_handseed(&mut self, h: Handle) {
        if h.0 >= self.handseed {
            self.handseed = h.0.saturating_add(1);
        }
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.name.eq_ignore_ascii_case(name))
    }
    pub fn layer_mut(&mut self, name: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(name))
    }
    pub fn linetype(&self, name: &str) -> Option<&Linetype> {
        self.linetypes.iter().find(|l| l.name.eq_ignore_ascii_case(name))
    }
    pub fn text_style(&self, name: &str) -> Option<&TextStyle> {
        self.text_styles.iter().find(|l| l.name.eq_ignore_ascii_case(name))
    }
    pub fn dim_style(&self, name: &str) -> Option<&DimStyle> {
        self.dim_styles.iter().find(|l| l.name.eq_ignore_ascii_case(name))
    }
    pub fn block(&self, name: &str) -> Option<&std::sync::Arc<Block>> {
        self.blocks.get(name).or_else(|| self.blocks.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v))
    }
    pub fn layout(&self, name: &str) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Add a layer if missing; returns whether it was created.
    pub fn ensure_layer(&mut self, name: &str) -> bool {
        if self.layer(name).is_some() {
            return false;
        }
        self.layers.push(Layer::new(name));
        true
    }

    pub fn space(&self, space: &Space) -> Option<&EntityStore> {
        match space {
            Space::Model => Some(&self.model),
            Space::Paper(n) => self.layouts.iter().find(|l| &l.name == n).map(|l| &l.entities),
        }
    }
    pub fn space_mut(&mut self, space: &Space) -> Option<&mut EntityStore> {
        match space {
            Space::Model => Some(&mut self.model),
            Space::Paper(n) => self.layouts.iter_mut().find(|l| &l.name == n).map(|l| &mut l.entities),
        }
    }

    /// Find an entity in model space or any layout.
    pub fn entity(&self, h: Handle) -> Option<&std::sync::Arc<Entity>> {
        self.model.get(h).or_else(|| self.layouts.iter().find_map(|l| l.entities.get(h)))
    }
    pub fn space_of(&self, h: Handle) -> Option<Space> {
        if self.model.contains(h) {
            return Some(Space::Model);
        }
        self.layouts.iter().find(|l| l.entities.contains(h)).map(|l| Space::Paper(l.name.clone()))
    }
    pub fn modify_entity<F: FnOnce(&mut Entity)>(&mut self, h: Handle, f: F) -> Result<()> {
        let sp = self.space_of(h).ok_or(DocError::NoEntity(h))?;
        let store = self.space_mut(&sp).ok_or(DocError::NoEntity(h))?;
        if store.modify(h, f) { Ok(()) } else { Err(DocError::NoEntity(h)) }
    }
    pub fn remove_entity(&mut self, h: Handle) -> Option<std::sync::Arc<Entity>> {
        let sp = self.space_of(h)?;
        self.space_mut(&sp)?.remove(h)
    }

    /// Add an entity to a space with a new handle, using the given common properties.
    pub fn add(&mut self, space: &Space, common: Common, kind: EntityKind) -> Result<Handle> {
        let h = self.new_handle();
        self.ensure_layer(&common.layer);
        let store = self.space_mut(space).ok_or_else(|| DocError::Invalid(format!("no such space {space:?}")))?;
        store.push(Entity { handle: h, common, kind });
        Ok(h)
    }

    /// Effective visibility of an entity (its own flag, its layer on/thawed).
    pub fn is_visible(&self, e: &Entity) -> bool {
        e.common.visible && self.layer(&e.common.layer).is_none_or(Layer::visible)
    }

    /// Bounds of all visible entities in a space.
    pub fn extents(&self, space: &Space) -> cadcraft_geom::Bounds2 {
        let mut b = cadcraft_geom::Bounds2::EMPTY;
        if let Some(store) = self.space(space) {
            for e in store.iter() {
                if self.is_visible(e) {
                    b = b.union(&entity_bounds(self, e, 0));
                }
            }
        }
        b
    }

    /// Total entity count across model and paper space.
    pub fn entity_count(&self) -> usize {
        self.model.len() + self.layouts.iter().map(|l| l.entities.len()).sum::<usize>()
    }

    /// Next unused anonymous block name with the given prefix (e.g. `*D`).
    pub fn anonymous_block_name(&self, prefix: &str) -> String {
        let mut i = 1usize;
        loop {
            let n = format!("{prefix}{i}");
            if self.block(&n).is_none() || i > 1_000_000 {
                return n;
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests;
