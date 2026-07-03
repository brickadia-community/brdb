use std::sync::Arc;

use crate::{
    SavedBrickColor,
    assets::entities::{DYNAMIC_GRID, dynamic_grid_entity},
    errors::BrdbSchemaError,
    schema::{
        BrdbSchema, BrdbSchemaGlobalData, BrdbValue,
        as_brdb::{AsBrdbIter, AsBrdbValue, BrdbArrayIter},
        write::write_brdb,
    },
    schemas::ENTITY_CHUNK_SOA,
    wrapper::{BString, BitFlags, BrdbComponent, ChunkIndex, Quat4f, Vector3f},
};

#[derive(Clone)]
pub struct Entity {
    pub asset: BString,
    /// An internal ID for linking entities to joints, etc
    pub id: Option<usize>,
    pub owner_index: Option<u32>,
    /// Saved-world field added alongside the schema update. If `None` at serialization,
    /// mirrors `owner_index` for back-compat with Entities constructed without
    /// explicitly setting an original owner.
    pub original_owner_index: Option<u32>,
    pub location: Vector3f,
    pub rotation: Quat4f,
    pub frozen: bool,
    pub sleeping: bool,
    pub velocity: Vector3f,
    pub angular_velocity: Vector3f,
    pub color_and_alpha: EntityColors,
    pub data: Arc<Box<dyn BrdbComponent>>,
}

impl Entity {
    /// True if this entity is any kind of brick grid: the global (static) world
    /// grid, a dynamic slider/servo grid, or a microchip's inner grid.
    pub fn is_brick_grid(&self) -> bool {
        self.is_dynamic_grid() || self.is_microchip_grid()
    }

    /// A dynamic brick grid — the rigid grid driven by slider/servo joints.
    pub fn is_dynamic_grid(&self) -> bool {
        self.asset == DYNAMIC_GRID
    }
    /// True if this entity is the inner grid of a microchip brick.
    /// Use `ComponentChunkSoA`'s `microchip_brick_grid_references` to find
    /// which microchip brick owns this grid.
    pub fn is_microchip_grid(&self) -> bool {
        self.asset == crate::assets::entities::MICROCHIP_GRID
    }
}

impl Default for Entity {
    fn default() -> Self {
        Self {
            asset: DYNAMIC_GRID,
            id: None,
            owner_index: None,
            original_owner_index: None,
            location: Vector3f::default(),
            rotation: Quat4f::default(),
            frozen: false,
            sleeping: false,
            velocity: Vector3f::default(),
            angular_velocity: Vector3f::default(),
            color_and_alpha: EntityColors::default(),
            data: dynamic_grid_entity(),
        }
    }
}

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
pub struct EntityTypeCounter {
    pub type_index: u32,
    pub num_entities: u32,
}

impl AsBrdbValue for EntityTypeCounter {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "TypeIndex" => Ok(&self.type_index),
            "NumEntities" => Ok(&self.num_entities),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}
impl TryFrom<&BrdbValue> for EntityTypeCounter {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            type_index: value.prop("TypeIndex")?.as_brdb_u32()?,
            num_entities: value.prop("NumEntities")?.as_brdb_u32()?,
        })
    }
}

#[derive(Clone)]
pub struct EntityColors(
    pub SavedBrickColor,
    pub SavedBrickColor,
    pub SavedBrickColor,
    pub SavedBrickColor,
    pub SavedBrickColor,
    pub SavedBrickColor,
    pub SavedBrickColor,
    pub SavedBrickColor,
);
impl Default for EntityColors {
    fn default() -> Self {
        Self(
            SavedBrickColor::entity_default(),
            SavedBrickColor::entity_default(),
            SavedBrickColor::entity_default(),
            SavedBrickColor::entity_default(),
            SavedBrickColor::entity_default(),
            SavedBrickColor::entity_default(),
            SavedBrickColor::entity_default(),
            SavedBrickColor::entity_default(),
        )
    }
}
impl EntityColors {
    /// Convert all eight colors' RGB from linear to sRGB (see
    /// `SavedBrickColor::rgb_to_srgb`).
    pub fn rgb_to_srgb(&self) -> Self {
        Self(
            self.0.rgb_to_srgb(),
            self.1.rgb_to_srgb(),
            self.2.rgb_to_srgb(),
            self.3.rgb_to_srgb(),
            self.4.rgb_to_srgb(),
            self.5.rgb_to_srgb(),
            self.6.rgb_to_srgb(),
            self.7.rgb_to_srgb(),
        )
    }
}

impl TryFrom<&BrdbValue> for EntityColors {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self(
            value.prop("Color0")?.try_into()?,
            value.prop("Color1")?.try_into()?,
            value.prop("Color2")?.try_into()?,
            value.prop("Color3")?.try_into()?,
            value.prop("Color4")?.try_into()?,
            value.prop("Color5")?.try_into()?,
            value.prop("Color6")?.try_into()?,
            value.prop("Color7")?.try_into()?,
        ))
    }
}
impl AsBrdbValue for EntityColors {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "Color0" => Ok(&self.0),
            "Color1" => Ok(&self.1),
            "Color2" => Ok(&self.2),
            "Color3" => Ok(&self.3),
            "Color4" => Ok(&self.4),
            "Color5" => Ok(&self.5),
            "Color6" => Ok(&self.6),
            "Color7" => Ok(&self.7),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}

#[derive(Default)]
pub struct EntityChunkSoA {
    pub type_counters: Vec<EntityTypeCounter>,
    pub persistent_indices: Vec<u32>,
    pub owner_indices: Vec<u32>,
    pub original_owner_indices: Vec<u32>,
    pub locations: Vec<Vector3f>,
    pub rotations: Vec<Quat4f>,
    pub weld_parent_flags: BitFlags,
    pub physics_locked_flags: BitFlags,
    pub physics_sleeping_flags: BitFlags,
    pub weld_parent_indices: Vec<u32>,
    pub linear_velocities: Vec<Vector3f>,
    pub angular_velocities: Vec<Vector3f>,
    pub colors_and_alphas: Vec<EntityColors>,
    /// Per-entity remaining lifespan in seconds (0 = no auto-despawn). Added in
    /// a newer schema; older saves omit it.
    pub remaining_life_spans: Vec<f32>,

    /// Entity data paired with struct name, resolved at add time from the library.
    pub unwritten_struct_data: Vec<Arc<Box<dyn BrdbComponent>>>,
}

impl EntityChunkSoA {
    pub fn add_entity(&mut self, global_data: &BrdbSchemaGlobalData, entity: &Entity, index: u32) {
        let type_index = global_data
            .entity_type_names
            .get_index_of(entity.asset.as_ref())
            .unwrap() as u32;

        self.unwritten_struct_data.push(entity.data.clone());

        // Check if the last counter matches the type index
        if let Some(counter) = self.type_counters.last_mut() {
            if counter.type_index == type_index {
                counter.num_entities += 1;
            } else {
                // Add a new counter for this entity type
                self.type_counters.push(EntityTypeCounter {
                    type_index,
                    num_entities: 1,
                });
            }
        } else {
            // No counters yet, add the first one
            self.type_counters.push(EntityTypeCounter {
                type_index,
                num_entities: 1,
            });
        }

        self.persistent_indices.push(index);
        self.owner_indices.push(entity.owner_index.unwrap_or(0));
        // Mirror current-owner if original is unset (legacy behavior).
        self.original_owner_indices.push(
            entity
                .original_owner_index
                .or(entity.owner_index)
                .unwrap_or(0),
        );
        self.locations.push(entity.location);
        self.rotations.push(entity.rotation);
        self.physics_locked_flags.push(entity.frozen);
        self.physics_sleeping_flags.push(entity.sleeping);
        self.linear_velocities.push(entity.velocity);
        self.angular_velocities.push(entity.angular_velocity);
        self.colors_and_alphas.push(entity.color_and_alpha.clone());
        // No auto-despawn for authored entities.
        self.remaining_life_spans.push(0.0);
    }

    pub fn to_bytes(self, schema: &BrdbSchema) -> Result<Vec<u8>, BrdbSchemaError> {
        let mut buf = schema.write_brdb(ENTITY_CHUNK_SOA, &self)?;

        for (i, entity_data) in self.unwritten_struct_data.into_iter().enumerate() {
            let struct_ty = entity_data
                .component_type()
                .and_then(|ty| schema.global_data.get_entity_class_name(ty.as_ref()));
            let Some(struct_ty) = struct_ty else { continue };
            write_brdb(&schema, &mut buf, struct_ty, &**entity_data)
                .map_err(|e| e.wrap(format!("entity data {i}: {struct_ty}")))?;
        }
        Ok(buf)
    }
}

impl AsBrdbValue for EntityChunkSoA {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "WeldParentFlags" => Ok(&self.weld_parent_flags),
            "PhysicsLockedFlags" => Ok(&self.physics_locked_flags),
            "PhysicsSleepingFlags" => Ok(&self.physics_sleeping_flags),
            // New saves always store sRGB colors, so this is always false.
            "bColorsAreLinear" => Ok(&false),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }

    fn as_brdb_struct_prop_array(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<crate::schema::as_brdb::BrdbArrayIter<'_>, BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "TypeCounters" => Ok(self.type_counters.as_brdb_iter()),
            "PersistentIndices" => Ok(self.persistent_indices.as_brdb_iter()),
            "OwnerIndices" => Ok(self.owner_indices.as_brdb_iter()),
            "OriginalOwnerIndices" => Ok(self.original_owner_indices.as_brdb_iter()),
            "Locations" => Ok(self.locations.as_brdb_iter()),
            "Rotations" => Ok(self.rotations.as_brdb_iter()),
            "WeldParentIndices" => Ok(self.weld_parent_indices.as_brdb_iter()),
            "LinearVelocities" => Ok(self.linear_velocities.as_brdb_iter()),
            "AngularVelocities" => Ok(self.angular_velocities.as_brdb_iter()),
            "ColorsAndAlphas" => Ok(self.colors_and_alphas.as_brdb_iter()),
            "RemainingLifeSpans" => Ok(self.remaining_life_spans.as_brdb_iter()),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}
impl TryFrom<&BrdbValue> for EntityChunkSoA {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            type_counters: value.prop("TypeCounters")?.try_into()?,
            persistent_indices: value.prop("PersistentIndices")?.try_into()?,
            owner_indices: value.prop("OwnerIndices")?.try_into()?,
            // Added alongside the per-entity original-owner schema update.
            // Older saves won't have this field; mirror OwnerIndices.
            original_owner_indices: if value.contains_key("OriginalOwnerIndices") {
                value.prop("OriginalOwnerIndices")?.try_into()?
            } else {
                value.prop("OwnerIndices")?.try_into()?
            },
            locations: value.prop("Locations")?.try_into()?,
            rotations: value.prop("Rotations")?.try_into()?,
            weld_parent_flags: value.prop("WeldParentFlags")?.try_into()?,
            physics_locked_flags: value.prop("PhysicsLockedFlags")?.try_into()?,
            physics_sleeping_flags: value.prop("PhysicsSleepingFlags")?.try_into()?,
            weld_parent_indices: value.prop("WeldParentIndices")?.try_into()?,
            linear_velocities: value.prop("LinearVelocities")?.try_into()?,
            angular_velocities: value.prop("AngularVelocities")?.try_into()?,
            colors_and_alphas: {
                let mut colors: Vec<EntityColors> = value.prop("ColorsAndAlphas")?.try_into()?;
                // Older saves (and any missing the field) store linear colors;
                // default true and convert to sRGB so in-memory colors are
                // always sRGB. New saves store sRGB (bColorsAreLinear = false).
                let linear = if value.contains_key("bColorsAreLinear") {
                    value.prop("bColorsAreLinear")?.as_brdb_bool()?
                } else {
                    true
                };
                if linear {
                    for ec in &mut colors {
                        *ec = ec.rgb_to_srgb();
                    }
                }
                colors
            },
            // Added in a newer schema; older saves omit it.
            remaining_life_spans: if value.contains_key("RemainingLifeSpans") {
                value.prop("RemainingLifeSpans")?.try_into()?
            } else {
                Vec::new()
            },
            unwritten_struct_data: Vec::new(),
        })
    }
}

pub struct EntityChunkIndexSoA {
    pub next_persistent_index: u32,
    pub chunk_3d_indices: Vec<ChunkIndex>,
    pub num_entities: Vec<u32>,
}

impl Default for EntityChunkIndexSoA {
    fn default() -> Self {
        Self {
            next_persistent_index: 2,
            chunk_3d_indices: Vec::new(),
            num_entities: Vec::new(),
        }
    }
}

impl AsBrdbValue for EntityChunkIndexSoA {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "NextPersistentIndex" => Ok(&self.next_persistent_index),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }

    fn as_brdb_struct_prop_array(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<BrdbArrayIter<'_>, BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "Chunk3DIndices" => Ok(self.chunk_3d_indices.as_brdb_iter()),
            "NumEntities" => Ok(self.num_entities.as_brdb_iter()),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}
impl TryFrom<&BrdbValue> for EntityChunkIndexSoA {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            next_persistent_index: value.prop("NextPersistentIndex")?.as_brdb_u32()?,
            chunk_3d_indices: value.prop("Chunk3DIndices")?.try_into()?,
            num_entities: value.prop("NumEntities")?.try_into()?,
        })
    }
}

/// This function may only be useful for legacy worlds from steam next fest.
/// New worlds will properly pair the class name with the entity type
pub fn lookup_entity_struct_name(entity_type: &str) -> Option<&'static str> {
    Some(match entity_type {
        "Entity_Ball" => "BP_Entity_Ball_C",
        "Entity_Ball1" => "BP_Entity_Ball1_C",
        "Entity_DynamicBrickGrid" => "BrickGridDynamicActor",
        "Entity_GlobalBrickGrid" => "BP_BrickGrid_Global_C",
        "Entity_Wheel_Caster" => "BP_Entity_Wheel_Caster_C",
        "Entity_Wheel_Deep1" => "BP_Entity_Wheel_Deep1_C",
        "Entity_Wheel_Deep2" => "BP_Entity_Wheel_Deep2_C",
        "Entity_Wheel_Deep3" => "BP_Entity_Wheel_Deep3_C",
        "Entity_Wheel_DogDish1" => "BP_Entity_Wheel_DogDish1_C",
        "Entity_Wheel_DollarSign" => "BP_Entity_Wheel_DollarSign_C",
        "Entity_Wheel_German5" => "BP_Entity_Wheel_German5_C",
        "Entity_Wheel_GoKart" => "BP_Entity_Wheel_GoKart_C",
        "Entity_Wheel_LandingGear1" => "BP_Entity_Wheel_LandingGear1_C",
        "Entity_Wheel_Muscle1" => "BP_Entity_Wheel_Muscle1_C",
        "Entity_Wheel_Muscle2" => "BP_Entity_Wheel_Muscle2_C",
        "Entity_Wheel_Offroad1" => "BP_Entity_Wheel_Offroad1_C",
        "Entity_Wheel_Offroad2" => "BP_Entity_Wheel_Offroad2_C",
        "Entity_Wheel_Racing1" => "BP_Entity_Wheel_Racing1_C",
        "Entity_Wheel_Racing1_Decal" => "BP_Entity_Wheel_Racing1_Decal_C",
        "Entity_Wheel_Racing2B" => "BP_Entity_Wheel_Racing2B_C",
        "Entity_Wheel_Railroad1" => "BP_Entity_Wheel_Railroad1_C",
        "Entity_Wheel_SaladSpinner" => "BP_Entity_Wheel_SaladSpinner_C",
        "Entity_Wheel_SaladSpinnerFlipped" => "BP_Entity_Wheel_SaladSpinnerFlipped_C",
        "Entity_Wheel_Skateboard" => "BP_Entity_Wheel_Skateboard_C",
        "Entity_Wheel_Sport2" => "BP_Entity_Wheel_Sport2_C",
        "Entity_Wheel_Sport3" => "BP_Entity_Wheel_Sport3_C",
        "Entity_Wheel_Sport4" => "BP_Entity_Wheel_Sport4_C",
        "Entity_Wheel_Stance1" => "BP_Entity_Wheel_Stance1_C",
        "Entity_Wheel_Stance2" => "BP_Entity_Wheel_Stance2_C",
        "Entity_Wheel_Stance3" => "BP_Entity_Wheel_Stance3_C",
        "Entity_Wheel_Steelie1" => "BP_Entity_Wheel_Steelie1_C",
        "Entity_Wheel_Steelie2" => "BP_Entity_Wheel_Steelie2_C",
        "Entity_Wheel_Super1" => "BP_Entity_Wheel_Super1_C",
        "Entity_Wheel_Super1Flipped" => "BP_Entity_Wheel_Super1Flipped_C",
        "Entity_Wheel_Super2" => "BP_Entity_Wheel_Super2_C",
        "Entity_Wheel_Tracked1" => "BP_Entity_Wheel_Tracked1_C",
        "Entity_Wheel_TrackedSprocket1" => "BP_Entity_Wheel_TrackedSprocket1_C",
        "Entity_Wheel_Truck1" => "BP_Entity_Wheel_Truck1_C",
        "Entity_Wheel_Truck2" => "BP_Entity_Wheel_Truck2_C",
        "Entity_Wheel_Truck3" => "BP_Entity_Wheel_Truck3_C",
        "Entity_Wheel_Tuner1" => "BP_Entity_Wheel_Tuner1_C",
        "Entity_Wheel_Tuner2" => "BP_Entity_Wheel_Tuner2_C",
        "Entity_Wheel_Tuner3" => "BP_Entity_Wheel_Tuner3_C",
        "Entity_Wheel_Tuner3Flipped" => "BP_Entity_Wheel_Tuner3Flipped_C",
        "Entity_Wheel_Tuner4" => "BP_Entity_Wheel_Tuner4_C",
        "Entity_Wheel_Tuner5" => "BP_Entity_Wheel_Tuner5_C",
        "Entity_Wheel_Tuner6" => "BP_Entity_Wheel_Tuner6_C",
        "Entity_Wheel_Wagon1" => "BP_Entity_Wheel_Wagon1_C",
        "Entity_Wheel_Wagon2" => "BP_Entity_Wheel_Wagon2_C",
        "Entity_Wheel_Whitewall1" => "BP_Entity_Wheel_Whitewall1_C",
        "Entity_Wheel_Whitewall2" => "BP_Entity_Wheel_Whitewall2_C",
        _ => return None,
    })
}
