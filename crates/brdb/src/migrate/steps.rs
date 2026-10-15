//! The migration steps in `migrations.json`, the data both brdb and brs-js
//! migrate saves with.
use std::{collections::HashMap, sync::LazyLock};

use serde::Deserialize;

use crate::schema::{BrdbSchema, BrdbSchemaFormat};

/// One breaking game update: how to recognise a save written before it, and
/// what to rename and reshape to bring the save past it.
///
/// Steps run oldest first; a save runs the first step that recognises it and
/// every step after that.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationStep {
    /// Short identifier, e.g. `"ea3"`.
    pub name: String,
    pub description: String,
    detect: Detect,
    #[serde(default)]
    pub(super) component_types: ComponentTypeRules,
    #[serde(default)]
    pub(super) bricks: BrickRules,
    #[serde(default)]
    fields: Vec<FieldRename>,
    #[serde(default)]
    values: Vec<ValueRule>,
}

/// How to recognise a save from before a step: every condition given must
/// hold for the save's `ComponentsShared.schema`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Detect {
    /// The schema's container format (`"EA3"` or `"Latest"`).
    component_schema_format: Option<String>,
    /// A struct only saves from before the step declare.
    component_schema_has_struct: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ComponentTypeRules {
    /// `[from, to]` prefix replacements, tried in order after `renames`.
    #[serde(default)]
    pub(super) prefixes: Vec<(String, String)>,
    #[serde(default)]
    pub(super) renames: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BrickRules {
    /// Renamed brick assets (basic or procedural).
    #[serde(default)]
    pub(super) renames: HashMap<String, String>,
    /// Basic gate bricks replaced by a procedural gate brick.
    #[serde(default)]
    pub(super) to_gate: HashMap<String, String>,
}

/// A renamed field of a component's data struct. `component` is the
/// component type's name after the step.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FieldRename {
    pub component: String,
    pub from: String,
    pub to: String,
}

/// A named value conversion (see `values.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) enum ValueRule {
    /// EA3 wire graph arrays, maps and wrapped enums become dynamic arrays,
    /// maps and enums; `wire_graph_variant` values become plain values.
    #[serde(rename = "ea3WireGraph")]
    Ea3WireGraph,
}

/// What a step does to one brick asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrickStep<'a> {
    Rename(&'a str),
    ToGate(&'a str),
}

impl MigrationStep {
    /// Whether a save whose component schema is `components` was written
    /// before this step.
    pub fn detects(&self, components: &BrdbSchema) -> bool {
        let Detect {
            component_schema_format,
            component_schema_has_struct,
        } = &self.detect;
        let format_ok = component_schema_format.as_deref().is_none_or(|f| {
            let format = match components.source_format() {
                Some(BrdbSchemaFormat::EA3) => "EA3",
                Some(BrdbSchemaFormat::Latest) => "Latest",
                _ => return false,
            };
            f == format
        });
        let struct_ok = component_schema_has_struct
            .as_deref()
            .is_none_or(|s| components.get_struct(s).is_some());
        format_ok && struct_ok
    }

    /// The component type's name after this step, or `None` when the step
    /// does not rename it.
    pub fn component_type(&self, name: &str) -> Option<String> {
        if let Some(to) = self.component_types.renames.get(name) {
            return Some(to.clone());
        }
        self.component_types
            .prefixes
            .iter()
            .find_map(|(from, to)| name.strip_prefix(from.as_str()).map(|rest| format!("{to}{rest}")))
    }

    pub(super) fn brick(&self, name: &str) -> Option<BrickStep<'_>> {
        if let Some(to) = self.bricks.to_gate.get(name) {
            return Some(BrickStep::ToGate(to));
        }
        self.bricks.renames.get(name).map(|to| BrickStep::Rename(to))
    }

    /// The fields this step renames on `component` (named as after the step).
    pub(super) fn field_renames<'a>(
        &'a self,
        component: &'a str,
    ) -> impl Iterator<Item = &'a FieldRename> + 'a {
        self.fields.iter().filter(move |f| f.component == component)
    }

    pub(super) fn value_rules(&self) -> &[ValueRule] {
        &self.values
    }
}

#[derive(Deserialize)]
struct StepsFile {
    steps: Vec<MigrationStep>,
}

/// Parse a steps file (the format of `migrations.json`).
pub(super) fn parse(json: &str) -> Result<Vec<MigrationStep>, serde_json::Error> {
    Ok(serde_json::from_str::<StepsFile>(json)?.steps)
}

/// Every migration step this crate knows, oldest first.
pub fn steps() -> &'static [MigrationStep] {
    static STEPS: LazyLock<Vec<MigrationStep>> = LazyLock::new(|| {
        parse(include_str!("../../migrations.json")).expect("migrations.json is valid")
    });
    &STEPS
}
