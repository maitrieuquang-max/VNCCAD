//! VNCCad: dynamic blocks — the parameters and actions of a dynamic block definition, and the
//! property values of a block reference (an anonymous `*U` block made from the definition).
//!
//! Supported: linear parameters driving stretch, move and array actions; flip parameters with
//! flip actions; visibility parameters (named states). Read from AutoCAD files (the
//! `ACAD_ENHANCEDBLOCK` evaluation graph), kept by VNCCad in its own record when saving.

use cadcraft_geom::Vec2;
use serde::{Deserialize, Serialize};

use crate::Handle;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DynKind {
    #[default]
    Linear,
    Flip,
    Visibility,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DynActionKind {
    #[default]
    Stretch,
    Move,
    Array,
    Flip,
}

/// A named visibility state and the entities shown in it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DynState {
    pub name: String,
    pub visible: Vec<Handle>,
}

/// An action: what a parameter change does to some of the block's entities.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DynAction {
    pub kind: DynActionKind,
    pub name: String,
    pub entities: Vec<Handle>,
    /// Stretch frame (block coordinates).
    pub frame: Vec<Vec2>,
    /// Distance multiplier and angle offset (radians) of stretch and move actions.
    pub factor: f64,
    pub angle: f64,
    /// Array column spacing.
    pub spacing: f64,
}

/// A parameter of a dynamic block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DynParam {
    /// Node id in the evaluation graph (how values and actions refer to it).
    pub id: i64,
    pub name: String,
    pub kind: DynKind,
    /// Linear and flip parameters: the defining points (block coordinates).
    pub base: Vec2,
    pub end: Vec2,
    /// Visibility parameters: every entity the parameter controls, and the states.
    pub controlled: Vec<Handle>,
    pub states: Vec<DynState>,
    /// Flip parameters: the two state labels.
    pub labels: Vec<String>,
    pub actions: Vec<DynAction>,
}

impl DynParam {
    /// The value in the definition (linear: its length; flip: not flipped; visibility: the
    /// first state).
    pub fn default_value(&self) -> DynValue {
        match self.kind {
            DynKind::Linear => DynValue::Distance(self.base.dist(self.end)),
            DynKind::Flip => DynValue::Flipped(false),
            DynKind::Visibility => DynValue::State(self.states.first().map(|s| s.name.clone()).unwrap_or_default()),
        }
    }
}

/// The dynamic part of a block definition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DynDef {
    pub params: Vec<DynParam>,
}

/// A property value of a block reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DynValue {
    Distance(f64),
    Flipped(bool),
    State(String),
}

impl DynValue {
    pub fn text(&self) -> String {
        match self {
            DynValue::Distance(d) => format!("{d:.4}").trim_end_matches('0').trim_end_matches('.').to_string(),
            DynValue::Flipped(f) => if *f { "Lật" } else { "Không lật" }.to_string(),
            DynValue::State(s) => s.clone(),
        }
    }
}

/// An anonymous block made from a dynamic block: its definition and the property values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DynRef {
    pub source: String,
    /// (parameter id, value).
    pub values: Vec<(i64, DynValue)>,
}

/// VNCCad: an associative array (ARRAYRECT / ARRAYPOLAR with Associative = Yes): its source
/// objects and parameters. The array is an INSERT of an anonymous block holding the items;
/// ARRAYEDIT changes the parameters and rebuilds the block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ArrayDef {
    /// "rect" or "polar".
    pub kind: String,
    pub rows: u32,
    pub cols: u32,
    pub row_spacing: f64,
    pub col_spacing: f64,
    pub count: u32,
    /// Polar: angle to fill (degrees), centre, whether items rotate.
    pub angle: f64,
    pub center: Vec2,
    pub rotate: bool,
    /// The source objects (drawing coordinates).
    pub source: Vec<crate::Entity>,
}

/// VNCCad: a text (TEXT or MTEXT) whose value is computed (FIELD / UPDATEFIELD).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FieldLink {
    /// The text showing the value.
    pub text: Handle,
    /// "area", "perimeter", "length", "radius", "date", "filename", "sheet", "count".
    pub kind: String,
    /// The object measured (object fields).
    pub object: Option<Handle>,
    /// Value × factor (unit conversion, e.g. 1e-6 for mm² → m²), decimals, prefix, suffix.
    pub factor: f64,
    pub decimals: u32,
    pub prefix: String,
    pub suffix: String,
    /// Date format (`%d/%m/%Y`…).
    pub format: String,
}
