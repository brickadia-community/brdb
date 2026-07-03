use std::collections::HashSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

use crate::{
    BString, BrdbSchemaError,
    schema::as_brdb::{AsBrdbIter, AsBrdbValue, BrdbArrayIter},
    wrapper::{Brick, BrickType},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrdbSchemaGlobalData {
    pub entity_type_names: IndexSet<String>,
    pub entity_data_class_names: IndexSet<String>,
    pub basic_brick_asset_names: IndexSet<String>,
    pub procedural_brick_asset_names: IndexSet<String>,
    pub material_asset_names: IndexSet<String>,
    pub component_type_names: IndexSet<String>,
    pub component_data_struct_names: Vec<String>,
    pub component_wire_port_names: IndexSet<String>,
    /// Internal set for type checking, not used in the BRDB.
    pub external_asset_types: HashSet<String>,
    pub external_asset_references: IndexSet<(String, String)>,
    /// Index into `entity_type_names` for the BP_BrickGrid_Global_C entity type,
    /// or -1 if the save has no global grid.
    pub global_grid_entity_type_index: i32,
}

impl Default for BrdbSchemaGlobalData {
    fn default() -> Self {
        Self {
            entity_type_names: IndexSet::default(),
            entity_data_class_names: IndexSet::default(),
            basic_brick_asset_names: IndexSet::default(),
            procedural_brick_asset_names: IndexSet::default(),
            material_asset_names: IndexSet::default(),
            component_type_names: IndexSet::default(),
            component_data_struct_names: Vec::default(),
            component_wire_port_names: IndexSet::default(),
            external_asset_types: HashSet::default(),
            external_asset_references: IndexSet::default(),
            global_grid_entity_type_index: -1,
        }
    }
}

impl BrdbSchemaGlobalData {
    pub fn add_brick_meta(&mut self, brick: &Brick) {
        // Add material
        if !self.material_asset_names.contains(brick.material.as_ref()) {
            self.material_asset_names.insert(brick.material.to_string());
        }

        // Add brick assets
        match &brick.asset {
            BrickType::Basic(asset) if !self.basic_brick_asset_names.contains(asset.as_ref()) => {
                self.basic_brick_asset_names.insert(asset.to_string());
            }
            BrickType::Procedural { asset, .. }
                if !self.procedural_brick_asset_names.contains(asset.as_ref()) =>
            {
                self.procedural_brick_asset_names.insert(asset.to_string());
            }
            // Material and asset are already handled above
            _ => {}
        }
    }


    pub fn get_struct_name(&self, type_name: &str) -> Option<&str> {
        let idx = self.component_type_names.get_index_of(type_name)?;
        let sn = self.component_data_struct_names.get(idx)?;
        if sn == "None" { None } else { Some(sn.as_str()) }
    }

    pub fn get_entity_class_name(&self, type_name: &str) -> Option<&str> {
        let idx = self.entity_type_names.get_index_of(type_name)?;
        self.entity_data_class_names.get_index(idx).map(|s| s.as_str())
    }

    pub fn get_port_index(&self, port_name: &str) -> Option<u16> {
        self.component_wire_port_names
            .get_index_of(port_name)
            .map(|i| i as u16)
    }

    pub fn get_component_type_index(&self, type_name: &str) -> Option<u16> {
        self.component_type_names
            .get_index_of(type_name)
            .map(|i| i as u16)
    }
    pub fn has_component_type(&self, type_name: &str) -> bool {
        self.component_type_names.contains(type_name)
    }
    pub fn add_entity_type(&mut self, type_name: &str) {
        // Back-compat: if a caller doesn't know the class name, fall back to the
        // legacy lookup table (populated at read time when only type_names were
        // present). When writing, prefer `add_entity_type_with_class` so that
        // `EntityDataClassNames` is paired correctly — the runtime rejects saves
        // whose EntityDataClassNames is empty for new entity types.
        if self.entity_type_names.insert(type_name.to_string()) {
            let class = crate::lookup_entity_struct_name(type_name).unwrap_or(type_name);
            self.entity_data_class_names.insert(class.to_string());
        }
    }

    /// Register an entity type with its explicit data-class name. Keeps
    /// `entity_type_names` and `entity_data_class_names` parallel as the
    /// saved-world format requires.
    pub fn add_entity_type_with_class(&mut self, type_name: &str, class_name: &str) {
        if self.entity_type_names.insert(type_name.to_string()) {
            self.entity_data_class_names.insert(class_name.to_string());
        }
    }

    pub fn basic_brick_asset_by_index(&self, index: usize) -> Result<BString, BrdbSchemaError> {
        Ok(self
            .basic_brick_asset_names
            .get_index(index as usize)
            .ok_or_else(|| {
                BrdbSchemaError::UnknownAsset("basic_brick_asset_name".to_string(), index)
            })?
            .to_owned()
            .into())
    }

    pub fn procedural_brick_asset_by_index(
        &self,
        index: usize,
    ) -> Result<BString, BrdbSchemaError> {
        Ok(self
            .procedural_brick_asset_names
            .get_index(index as usize)
            .ok_or_else(|| {
                BrdbSchemaError::UnknownAsset("procedural_brick_asset_name".to_string(), index)
            })?
            .to_owned()
            .into())
    }

    pub fn material_asset_by_index(&self, index: usize) -> Result<BString, BrdbSchemaError> {
        Ok(self
            .material_asset_names
            .get_index(index as usize)
            .ok_or_else(|| BrdbSchemaError::UnknownAsset("material_asset_name".to_string(), index))?
            .to_owned()
            .into())
    }

    /// `proc_brick_starting_index` needs to exist because the type ids of brick assets are
    /// stored in the GlobalData, and the type ids of procedural
    /// bricks are assigned starting from the end of the basic brick
    /// asset names.
    ///
    /// When new brick assets are added, the length of the basic
    /// brick asset names will increase, and the type ids of procedural
    /// bricks in older chunks will not match the new
    /// basic brick asset names.
    ///
    /// This offset allows older chunks to properly load, assuming the global
    /// data does not change the order of brick asset names (by external tools)
    pub fn proc_brick_starting_index(&self) -> u32 {
        self.basic_brick_asset_names.len() as u32
    }
}

impl AsBrdbValue for BrdbSchemaGlobalData {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &super::BrdbSchema,
        _struct_name: super::BrdbInterned,
        prop_name: super::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "GlobalGridEntityTypeIndex" => Ok(&self.global_grid_entity_type_index),
            n => unimplemented!("unimplemented scalar struct field {n}"),
        }
    }

    fn as_brdb_struct_prop_array(
        &self,
        schema: &super::BrdbSchema,
        _struct_name: super::BrdbInterned,
        prop_name: super::BrdbInterned,
    ) -> Result<BrdbArrayIter<'_>, crate::errors::BrdbSchemaError> {
        Ok(match prop_name.get(schema).unwrap() {
            "EntityTypeNames" => self.entity_type_names.as_brdb_iter(),
            "EntityDataClassNames" => self.entity_data_class_names.as_brdb_iter(),
            "BasicBrickAssetNames" => self.basic_brick_asset_names.as_brdb_iter(),
            "ProceduralBrickAssetNames" => self.procedural_brick_asset_names.as_brdb_iter(),
            "MaterialAssetNames" => self.material_asset_names.as_brdb_iter(),
            "ComponentTypeNames" => self.component_type_names.as_brdb_iter(),
            "ComponentDataStructNames" => self.component_data_struct_names.as_brdb_iter(),
            "ComponentWirePortNames" => self.component_wire_port_names.as_brdb_iter(),
            // BRSavedPrimaryAssetId is automatically inferred from (&str, &str)
            "ExternalAssetReferences" => self.external_asset_references.as_brdb_iter(),
            n => unimplemented!("unimplemented array struct field {n}"),
        })
    }
}
