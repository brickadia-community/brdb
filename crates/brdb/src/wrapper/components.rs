use crate::{
    BrdbSchemaError,
    errors::BrdbWorldError,
    schema::{
        BrdbInterned, BrdbSchema, BrdbSchemaGlobalData, BrdbStruct, BrdbValue,
        as_brdb::{AsBrdbIter, AsBrdbValue},
        write::write_brdb,
    },
    schemas::BRICK_COMPONENT_SOA,
    wrapper::{BString, Quat4f, Vector3f},
};

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
pub struct ComponentTypeCounter {
    pub type_index: u32,
    pub num_instances: u32,
}

impl AsBrdbValue for ComponentTypeCounter {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "TypeIndex" => Ok(&self.type_index),
            "NumInstances" => Ok(&self.num_instances),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}

impl TryFrom<&BrdbValue> for ComponentTypeCounter {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            type_index: value.prop("TypeIndex")?.as_brdb_u32()?,
            num_instances: value.prop("NumInstances")?.as_brdb_u32()?,
        })
    }
}

#[derive(Default)]
pub struct ComponentChunkSoA {
    pub component_type_counters: Vec<ComponentTypeCounter>,
    pub component_brick_indices: Vec<u32>,
    pub joint_brick_indices: Vec<u32>,
    pub joint_entity_references: Vec<u32>,
    pub joint_initial_relative_offsets: Vec<Vector3f>,
    pub joint_initial_relative_rotations: Vec<Quat4f>,
    /// Bricks in this chunk that host a microchip (Internal_Microchip) component.
    /// Each entry is the brick's local index in this chunk; the inner grid is found
    /// via the matching entry in `microchip_brick_grid_references`.
    pub microchip_brick_indices: Vec<u32>,
    /// Entity references for the inner brick grid of each microchip, parallel
    /// to `microchip_brick_indices`.
    pub microchip_brick_grid_references: Vec<u32>,

    pub unwritten_struct_data: Vec<Box<dyn BrdbComponent>>,
}

impl ComponentChunkSoA {
    pub fn add_component(
        &mut self,
        global_data: &BrdbSchemaGlobalData,
        brick_index: u32,
        component: &dyn BrdbComponent,
    ) -> Result<(), BrdbWorldError> {
        let Some(component_ty_name) = component.component_type() else {
            return Ok(());
        };
        // The type must be registered in global_data (e.g. via
        // World::register_all_components) so it resolves to a counter index.
        let Some(type_index) = global_data
            .component_type_names
            .get_index_of(component_ty_name.as_ref())
        else {
            return Err(BrdbWorldError::UnregisteredComponentType(
                component_ty_name.to_string(),
            ));
        };
        let type_index = type_index as u32;

        if let Some(counter) = self.component_type_counters.last_mut()
            && counter.type_index == type_index
        {
            counter.num_instances += 1;
        } else {
            self.component_type_counters.push(ComponentTypeCounter {
                type_index,
                num_instances: 1,
            });
        }
        self.component_brick_indices.push(brick_index);

        if global_data.get_struct_name(component_ty_name.as_ref()).is_some() {
            self.unwritten_struct_data.push(component.boxed_component());
        }
        Ok(())
    }

    pub fn to_bytes(self, schema: &BrdbSchema) -> Result<Vec<u8>, BrdbSchemaError> {
        let mut buf = schema.write_brdb(BRICK_COMPONENT_SOA, &self)?;

        for (i, component_data) in self.unwritten_struct_data.into_iter().enumerate() {
            let struct_ty = component_data
                .component_type()
                .and_then(|ty| schema.global_data.get_struct_name(ty.as_ref()));
            let Some(struct_ty) = struct_ty else { continue };
            write_brdb(&schema, &mut buf, struct_ty, component_data.as_ref())
                .map_err(|e| e.wrap(format!("component data {i}: {struct_ty}")))?;
        }
        Ok(buf)
    }
}

impl AsBrdbValue for ComponentChunkSoA {
    fn as_brdb_struct_prop_array(
        &self,
        schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<crate::schema::as_brdb::BrdbArrayIter<'_>, crate::errors::BrdbSchemaError> {
        Ok(match prop_name.get(schema).unwrap() {
            "ComponentTypeCounters" => self.component_type_counters.as_brdb_iter(),
            "ComponentBrickIndices" => self.component_brick_indices.as_brdb_iter(),
            "JointBrickIndices" => self.joint_brick_indices.as_brdb_iter(),
            "JointEntityReferences" => self.joint_entity_references.as_brdb_iter(),
            "JointInitialRelativeOffsets" => self.joint_initial_relative_offsets.as_brdb_iter(),
            "JointInitialRelativeRotations" => self.joint_initial_relative_rotations.as_brdb_iter(),
            "MicrochipBrickIndices" => self.microchip_brick_indices.as_brdb_iter(),
            "MicrochipBrickGridReferences" => self.microchip_brick_grid_references.as_brdb_iter(),
            n => unimplemented!("unimplemented struct field {n}"),
        })
    }
}

impl TryFrom<&BrdbValue> for ComponentChunkSoA {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            component_type_counters: value.prop("ComponentTypeCounters")?.try_into()?,
            component_brick_indices: value.prop("ComponentBrickIndices")?.try_into()?,
            joint_brick_indices: value.prop("JointBrickIndices")?.try_into()?,
            joint_entity_references: value.prop("JointEntityReferences")?.try_into()?,
            joint_initial_relative_offsets: value
                .prop("JointInitialRelativeOffsets")?
                .try_into()?,
            joint_initial_relative_rotations: value
                .prop("JointInitialRelativeRotations")?
                .try_into()?,
            // Added alongside the microchip schema update. Older saves won't
            // have these; default to empty vectors.
            microchip_brick_indices: if value.contains_key("MicrochipBrickIndices") {
                value.prop("MicrochipBrickIndices")?.try_into()?
            } else {
                Vec::new()
            },
            microchip_brick_grid_references: if value.contains_key("MicrochipBrickGridReferences") {
                value.prop("MicrochipBrickGridReferences")?.try_into()?
            } else {
                Vec::new()
            },
            unwritten_struct_data: Vec::new(),
        })
    }
}

/// This trait allows BrdbComponents to be cloned
/// despite being a dyn trait
pub trait BoxedComponent: Send + Sync {
    fn boxed_component(&self) -> Box<dyn BrdbComponent>;
}

pub trait BrdbComponent: AsBrdbValue + BoxedComponent + Send + Sync {
    /// The component type name (e.g. `"BrickComponentType_WireGraph_Expr_Add"`).
    /// All other metadata (struct name, schema, wire ports, external assets) is
    /// resolved from the `ComponentLibrary` on `World`.
    fn component_type(&self) -> Option<BString> {
        None
    }
}

/// Blanket implement boxed for all BrdbComponents with Clone
/// ... enabling them to be cloned
impl<T: Clone + BrdbComponent + 'static> BoxedComponent for T {
    fn boxed_component(&self) -> Box<dyn BrdbComponent> {
        Box::new(self.clone())
    }
}

// Empty component... may have its usecases
impl BrdbComponent for () {}

impl BrdbComponent for BrdbStruct {}
