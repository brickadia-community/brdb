//! Migration of worlds from older save formats to the current one.
//!
//! Brickadia's breaking updates rename component types, data structs, fields
//! and gate bricks, and change how some values are stored. Each update is a
//! [`MigrationStep`] in `migrations.json` (shared with brs-js), oldest first.
//! [`to_latest`] runs every step a world needs, the same way the game
//! upgrades an old world when it loads it, so the rest of this crate only has
//! to understand the current vocabulary.
//!
//! A world runs the first step that recognises it and every step after it.
//! The steps rename names and reshape values; the result is then encoded
//! once, file by file. Files whose meaning did not change keep their original
//! encoding (every game build reads the older formats):
//!
//! - `GlobalData.mps`: component type, data struct and brick asset names are
//!   renamed; gates that became procedural bricks add their `PB_Gate_*` asset.
//! - `ComponentsShared.schema` and the component chunks: data is re-encoded
//!   into the current structs.
//! - brick chunks: only those holding a brick whose asset changed index.
//!
//! Adding a step for a new game update means adding its renames to
//! `migrations.json`, and a value conversion in `values.rs` only when values
//! change shape.
mod steps;
mod values;

pub use steps::{MigrationStep, steps};

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use crate::{
    BrFsReader, BrReader, BrickChunkSoA, BrickSize, BrickSizeCounter, ComponentChunkSoA,
    assets::components::procedural_gate,
    errors::{BrError, BrdbSchemaError, BrdbWorldError},
    pending::BrPendingFs,
    schema::{
        BrdbInterned, BrdbSchema, BrdbValue, ReadBrdbSchema,
        as_brdb::{AsBrdbIter, AsBrdbValue},
    },
    schemas,
    wrapper::{BString, BrdbComponent, component_db::COMPONENT_TYPE_STRUCT_PAIRS},
};
use steps::BrickStep;

/// What an older brick asset becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrickMigration {
    /// A renamed brick.
    Basic(String),
    /// A basic gate brick replaced by a procedural gate brick, placed at its
    /// largest size (the one exposing every port).
    Procedural(String, BrickSize),
}

/// The steps a world needs: the first step that recognises it and every step
/// after it. Empty for a world in the current format.
pub fn pending_steps<T: BrFsReader>(
    reader: &BrReader<T>,
) -> Result<&'static [MigrationStep], BrError> {
    let components = reader.components_schema()?;
    let all = steps();
    Ok(all
        .iter()
        .position(|s| s.detects(&components))
        .map_or(&[][..], |i| &all[i..]))
}

/// Whether a world is in an older save format.
pub fn needs_migration<T: BrFsReader>(reader: &BrReader<T>) -> Result<bool, BrError> {
    Ok(!pending_steps(reader)?.is_empty())
}

/// The current name of a component type from any older format. Names that
/// did not change are returned as they are.
pub fn component_type(name: &str) -> Cow<'_, str> {
    component_type_through(steps(), name)
}

fn component_type_through<'a>(steps: &[MigrationStep], name: &'a str) -> Cow<'a, str> {
    let mut name = Cow::Borrowed(name);
    for step in steps {
        if let Some(to) = step.component_type(&name) {
            name = Cow::Owned(to);
        }
    }
    name
}

/// What a brick asset from any older format becomes, or `None` when it is
/// unchanged.
pub fn brick_asset(name: &str) -> Option<BrickMigration> {
    brick_through(steps(), name)
}

fn brick_through(steps: &[MigrationStep], name: &str) -> Option<BrickMigration> {
    let mut current = name.to_owned();
    let (mut changed, mut gate) = (false, false);
    for step in steps {
        match step.brick(&current) {
            Some(BrickStep::Rename(to)) => current = to.to_owned(),
            Some(BrickStep::ToGate(to)) => {
                current = to.to_owned();
                gate = true;
            }
            None => continue,
        }
        changed = true;
    }
    if !changed {
        return None;
    }
    Some(if gate {
        let [x, y, z] = procedural_gate(&current)
            .and_then(|g| g.variants.last())
            .map_or([10, 5, 2], |v| v.half_size);
        BrickMigration::Procedural(current, BrickSize::new(x as u16, y as u16, z as u16))
    } else {
        BrickMigration::Basic(current)
    })
}

/// Reshape a value decoded with an older schema into the form the current
/// schema encodes, applying every step's value conversions. Values that did
/// not change shape are returned as they are.
pub fn migrate_value(value: &BrdbValue) -> Result<BrdbValue, BrdbSchemaError> {
    values_through(steps(), value)
}

fn values_through(steps: &[MigrationStep], value: &BrdbValue) -> Result<BrdbValue, BrdbSchemaError> {
    let mut value = value.clone();
    for step in steps {
        for &rule in step.value_rules() {
            value = values::apply(rule, &value)?;
        }
    }
    Ok(value)
}

/// The fields of a component's data renamed by `steps`, as current field
/// name -> field name in the stored data. `ty` is the stored type name.
fn field_aliases(steps: &[MigrationStep], ty: &str) -> HashMap<String, String> {
    let mut aliases: HashMap<String, String> = HashMap::new();
    let mut ty = ty.to_owned();
    for step in steps {
        if let Some(to) = step.component_type(&ty) {
            ty = to;
        }
        for rename in step.field_renames(&ty) {
            let stored = aliases
                .remove(&rename.from)
                .unwrap_or_else(|| rename.from.clone());
            aliases.insert(rename.to.clone(), stored);
        }
    }
    aliases
}

/// The current data struct of a current component type, or `None` when the
/// type is unknown to this crate.
fn current_struct(component_type: &str) -> Option<&'static str> {
    static MAP: LazyLock<HashMap<&'static str, &'static str>> =
        LazyLock::new(|| COMPONENT_TYPE_STRUCT_PAIRS.iter().copied().collect());
    MAP.get(component_type).copied()
}

/// A component instance being written into a migrated chunk. Fields renamed
/// by a step are found under their stored name.
#[derive(Clone)]
struct MigratedComponent {
    ty: String,
    data: BrdbValue,
    aliases: Arc<HashMap<String, String>>,
}

impl MigratedComponent {
    /// The stored value of a renamed field, when `prop_name` is one.
    fn aliased(&self, schema: &BrdbSchema, prop_name: BrdbInterned) -> Option<Option<&BrdbValue>> {
        let name = schema.intern.lookup_ref(prop_name)?;
        let stored = self.aliases.get(name)?;
        Some(match &self.data {
            BrdbValue::Struct(s) => s.get(stored),
            _ => None,
        })
    }
}

impl AsBrdbValue for MigratedComponent {
    fn has_brdb_struct_prop(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> bool {
        match self.aliased(schema, prop_name) {
            Some(value) => value.is_some(),
            None => self.data.has_brdb_struct_prop(schema, struct_name, prop_name),
        }
    }
    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        match self.aliased(schema, prop_name) {
            Some(Some(value)) => Ok(value),
            _ => self.data.as_brdb_struct_prop_value(schema, struct_name, prop_name),
        }
    }
    fn as_brdb_struct_prop_array(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<crate::schema::as_brdb::BrdbArrayIter<'_>, BrdbSchemaError> {
        match self.aliased(schema, prop_name) {
            Some(Some(BrdbValue::Array(items) | BrdbValue::FlatArray(items))) => {
                Ok(items.as_brdb_iter())
            }
            _ => self.data.as_brdb_struct_prop_array(schema, struct_name, prop_name),
        }
    }
    fn as_brdb_struct_prop_map(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<crate::schema::as_brdb::BrdbMapIter<'_>, BrdbSchemaError> {
        match self.aliased(schema, prop_name) {
            Some(Some(BrdbValue::Map(map))) => Ok(Box::new(
                map.iter()
                    .map(|(k, v)| (k as &dyn AsBrdbValue, v as &dyn AsBrdbValue)),
            )),
            _ => self.data.as_brdb_struct_prop_map(schema, struct_name, prop_name),
        }
    }
}

impl BrdbComponent for MigratedComponent {
    fn component_type(&self) -> Option<BString> {
        Some(BString::from(self.ty.clone()))
    }
    fn component_type_ref(&self) -> Option<&str> {
        Some(&self.ty)
    }
}

/// Rename the entries of a name table in place, keeping every index. When a
/// new name is already taken (by an unrenamed entry, or by an earlier entry
/// renamed to the same name), the entry keeps its old name and the returned
/// map sends its index to the entry holding the new name.
fn rename_table(
    table: &mut indexmap::IndexSet<String, rustc_hash::FxBuildHasher>,
    rename: impl Fn(&str) -> Option<String>,
) -> Result<HashMap<usize, usize>, BrdbWorldError> {
    let renames: Vec<Option<String>> = table.iter().map(|n| rename(n)).collect();
    let mut names: Vec<String> = table.iter().cloned().collect();
    // Entries that keep their name own it.
    let mut owner: HashMap<String, usize> = names
        .iter()
        .enumerate()
        .filter(|(i, _)| renames[*i].is_none())
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut remap = HashMap::new();
    for (i, to) in renames.into_iter().enumerate() {
        let Some(to) = to else { continue };
        if let Some(&at) = owner.get(&to) {
            remap.insert(i, at);
            continue;
        }
        owner.insert(to.clone(), i);
        names[i] = to;
    }
    // A remapped entry's old name can collide with another entry's new name.
    let mut new = indexmap::IndexSet::with_capacity_and_hasher(names.len(), Default::default());
    for name in names {
        if new.contains(&name) {
            return Err(BrdbWorldError::MigrationDuplicateName(name));
        }
        new.insert(name);
    }
    *table = new;
    Ok(remap)
}

/// Rewrite a brick chunk's columns for renamed and procedural assets:
/// procedural counters follow `procedural_remap`, basic bricks follow
/// `basic_remap`, and basic bricks in `to_procedural` become procedural
/// bricks. Returns whether anything changed.
fn migrate_brick_columns(
    soa: &mut BrickChunkSoA,
    basic_remap: &HashMap<usize, usize>,
    procedural_remap: &HashMap<usize, usize>,
    to_procedural: &HashMap<usize, (usize, BrickSize)>,
) -> Result<bool, BrdbWorldError> {
    let counted: usize = soa.brick_size_counters.iter().map(|c| c.num_sizes as usize).sum();
    if counted > soa.brick_sizes.len() {
        return Err(BrdbWorldError::BrickSizeCountersExceedSizes {
            counted,
            sizes: soa.brick_sizes.len(),
        });
    }
    let start = soa.procedural_brick_starting_index;
    let mut changed = false;
    for counter in &mut soa.brick_size_counters {
        if let Some(&target) = procedural_remap.get(&(counter.asset_index as usize)) {
            counter.asset_index = target as u32;
            changed = true;
        }
    }
    for i in 0..soa.brick_type_indices.len() {
        let ty = soa.brick_type_indices[i];
        if ty >= start {
            continue;
        }
        if let Some(&(asset, size)) = to_procedural.get(&(ty as usize)) {
            let slot = procedural_slot(soa, asset as u32, size);
            soa.brick_type_indices[i] = start + slot;
            changed = true;
        } else if let Some(&target) = basic_remap.get(&(ty as usize)) {
            soa.brick_type_indices[i] = target as u32;
            changed = true;
        }
    }
    Ok(changed)
}

/// The procedural slot holding `(asset, size)` in a brick chunk: an existing
/// one, else a new slot at the end of the asset's run (or a new run). Slots
/// map one to one onto `brick_sizes`; sizes past the counted slots are
/// unreferenced and stay so.
fn procedural_slot(soa: &mut BrickChunkSoA, asset: u32, size: BrickSize) -> u32 {
    let start = soa.procedural_brick_starting_index;
    let mut end = 0usize;
    for c in 0..soa.brick_size_counters.len() {
        let begin = end;
        end += soa.brick_size_counters[c].num_sizes as usize;
        if soa.brick_size_counters[c].asset_index != asset {
            continue;
        }
        if let Some(s) = (begin..end).find(|&s| soa.brick_sizes[s] == size) {
            return s as u32;
        }
        soa.brick_size_counters[c].num_sizes += 1;
        soa.brick_sizes.insert(end, size);
        for ty in &mut soa.brick_type_indices {
            if *ty >= start + end as u32 {
                *ty += 1;
            }
        }
        return end as u32;
    }
    soa.brick_size_counters.push(BrickSizeCounter {
        asset_index: asset,
        num_sizes: 1,
    });
    soa.brick_sizes.insert(end, size);
    end as u32
}

/// Upgrade a world to the current save format, returning its latest
/// revision as a pending filesystem. A world already in the current format
/// is returned unchanged.
pub fn to_latest<T: BrFsReader>(reader: &BrReader<T>) -> Result<BrPendingFs, BrError> {
    to_latest_with(reader, pending_steps(reader)?)
}

fn to_latest_with<T: BrFsReader>(
    reader: &BrReader<T>,
    steps: &[MigrationStep],
) -> Result<BrPendingFs, BrError> {
    let mut pending = reader.to_pending()?;
    if steps.is_empty() {
        return Ok(pending);
    }

    // --- GlobalData: rename names, keeping indices.
    let mut gd = (*reader.global_data()?).clone();
    let old_types: Vec<String> = gd.component_type_names.iter().cloned().collect();
    let type_remap = rename_table(&mut gd.component_type_names, |n| {
        let to = component_type_through(steps, n);
        (to != n).then(|| to.into_owned())
    })?;
    // Merging two component types would mean rewriting every reference to a
    // type index (component and wire chunks, component refs in data); no
    // step does that.
    if let Some((&from, &to)) = type_remap.iter().next() {
        return Err(BrdbWorldError::MigrationMergesComponentType {
            from: old_types[from].clone(),
            to: old_types[to].clone(),
        }
        .into());
    }
    let mut component_schema = schemas::bricks_components_schema_max().clone();
    let old_structs = gd.component_data_struct_names.clone();
    for (i, ty) in gd.component_type_names.iter().enumerate() {
        if let Some(st) = current_struct(ty) {
            gd.component_data_struct_names[i] = st.to_owned();
        }
        // A type with no current data struct (removed from the game) keeps
        // its instances without data, as there is no current struct to
        // encode it in.
        let st = &mut gd.component_data_struct_names[i];
        if st != "None" && component_schema.get_struct(st).is_none() {
            *st = "None".to_owned();
        }
    }

    let basic_remap = rename_table(&mut gd.basic_brick_asset_names, |n| {
        match brick_through(steps, n) {
            Some(BrickMigration::Basic(to)) => Some(to),
            _ => None,
        }
    })?;
    let procedural_remap = rename_table(&mut gd.procedural_brick_asset_names, |n| {
        match brick_through(steps, n) {
            Some(BrickMigration::Basic(to)) => Some(to),
            _ => None,
        }
    })?;
    // Basic bricks that become procedural: old basic index -> (asset, size).
    let mut to_procedural: HashMap<usize, (usize, BrickSize)> = HashMap::new();
    for (i, name) in gd.basic_brick_asset_names.clone().iter().enumerate() {
        if let Some(BrickMigration::Procedural(asset, size)) = brick_through(steps, name) {
            let (pi, _) = gd.procedural_brick_asset_names.insert_full(asset);
            to_procedural.insert(i, (pi, size));
        }
    }
    let gd = Arc::new(gd);

    // --- Brick chunks whose asset indices changed.
    let mut grids = Vec::new();
    for gid in 1..512usize {
        let Ok(chunks) = reader.brick_chunk_index(gid) else {
            continue;
        };
        grids.push((gid, chunks));
    }
    if !basic_remap.is_empty() || !procedural_remap.is_empty() || !to_procedural.is_empty() {
        let bricks_schema = reader.bricks_schema()?;
        for (gid, chunks) in &grids {
            for chunk in chunks {
                let mut soa: BrickChunkSoA = reader.brick_chunk_soa(*gid, chunk.index)?;
                if migrate_brick_columns(&mut soa, &basic_remap, &procedural_remap, &to_procedural)? {
                    let bytes = soa.to_bytes(&bricks_schema)?;
                    *pending
                        .cd_mut(format!("World/0/Bricks/Grids/{gid}/Chunks/{}.mps", chunk.index))? =
                        BrPendingFs::File(Some(bytes));
                }
            }
        }
    }

    // --- Components: re-encode every chunk with the current schema.
    component_schema.set_global_data(gd.clone());
    let aliases: Vec<Arc<HashMap<String, String>>> = old_types
        .iter()
        .map(|ty| Arc::new(field_aliases(steps, ty)))
        .collect();
    for (gid, chunks) in &grids {
        for chunk in chunks.iter().filter(|c| c.num_components > 0) {
            let (soa, structs) = reader.component_chunk(*gid, chunk.index)?;
            let mut data = structs.into_iter();
            let mut components: Vec<Box<dyn BrdbComponent>> = Vec::new();
            for counter in &soa.component_type_counters {
                let i = counter.type_index as usize;
                let had = old_structs.get(i).is_some_and(|s| s != "None");
                let has = gd
                    .component_data_struct_names
                    .get(i)
                    .is_some_and(|s| s != "None");
                let ty = gd
                    .component_type_names
                    .get_index(i)
                    .cloned()
                    .unwrap_or_else(|| old_types.get(i).cloned().unwrap_or_default());
                for _ in 0..counter.num_instances {
                    let old = if had { data.next() } else { None };
                    if has {
                        let value = match old {
                            Some(s) => values_through(steps, &BrdbValue::Struct(Box::new(s)))?,
                            // A type that gained a data struct: every field
                            // takes its default.
                            None => values::current_struct(&gd.component_data_struct_names[i], vec![]),
                        };
                        components.push(Box::new(MigratedComponent {
                            ty: ty.clone(),
                            data: value,
                            aliases: aliases.get(i).cloned().unwrap_or_default(),
                        }));
                    }
                }
            }
            let out = ComponentChunkSoA {
                unwritten_struct_data: components,
                ..soa
            };
            let bytes = out.to_bytes(&component_schema)?;
            *pending.cd_mut(format!(
                "World/0/Bricks/Grids/{gid}/Components/{}.mps",
                chunk.index
            ))? = BrPendingFs::File(Some(bytes));
        }
    }
    *pending.cd_mut("World/0/Bricks/ComponentsShared.schema")? =
        BrPendingFs::File(Some(component_schema.to_bytes()?));

    // --- GlobalData, written with the world's own GlobalData schema.
    let gd_schema = reader
        .read_file("World/0/GlobalData.schema")?
        .as_slice()
        .read_brdb_schema()?;
    *pending.cd_mut("World/0/GlobalData.mps")? = BrPendingFs::File(Some(
        gd_schema.write_brdb(schemas::GLOBAL_DATA_SOA, gd.as_ref())?,
    ));

    Ok(pending)
}

/// Open a world in the current save format: an older world is upgraded with
/// [`to_latest`] into an in-memory `.brz`, which the returned reader reads.
/// A world already in the current format is copied as is.
///
/// ```no_run
/// use brdb::{Brdb, IntoReader, migrate};
///
/// let world = migrate::open_latest(&Brdb::open("old.brdb")?.into_reader())?;
/// let data = world.global_data()?; // current component type names
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[cfg(feature = "brz")]
pub fn open_latest<T: BrFsReader>(
    reader: &BrReader<T>,
) -> Result<BrReader<crate::BrzIndex<crate::Brz>>, BrError> {
    use crate::IntoReader;
    Ok(to_latest(reader)?.to_brz_data(None)?.into_reader())
}

#[cfg(test)]
mod tests;
