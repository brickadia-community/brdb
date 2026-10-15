use indexmap::IndexMap;

use crate::{
    IntVector,
    assets::{LiteralComponent, bricks, entities},
    errors::{BrError, BrdbWorldError},
    schema::{BrdbSchema, BrdbSchemaFormat},
    wrapper::{
        Brick, Entity, Guid, Owner, Position, UnsavedFs, UnsavedWorld, WireConnection, WirePort,
        WorldMeta,
    },
};

use super::{
    component_db::{COMPONENT_TYPE_STRUCT_PAIRS, ENTITY_TYPE_STRUCT_PAIRS, WIRE_PORT_NAMES},
    schemas::BuiltinSchemas,
};

pub struct World {
    pub meta: WorldMeta,
    pub owners: IndexMap<Guid, Owner>,
    /// Bricks on the main grid
    pub bricks: Vec<Brick>,
    /// Non-main grids require an entity to be created for them
    pub grids: Vec<(Entity, Vec<Brick>)>,
    pub wires: Vec<WireConnection>,
    pub entities: Vec<Entity>,
    /// Per-microchip linkage pairs: `(brick_id, entity_id)` where the brick
    /// is the outer microchip shell and the entity is the inner grid.
    pub microchip_links: Vec<(usize, usize)>,
    /// Embedded prefab archives (the root `Prefabs/` folder), keyed by
    /// root-relative path (`Prefabs/Uploads/<BLAKE3>.brz`) with raw `.brz`
    /// bytes as values. Populate via [`World::add_prefab`]; the returned
    /// path is what a `Prefab` component property references.
    ///
    /// The game requires the `Prefabs/Uploads/<BLAKE3>.brz` naming — it
    /// crashes loading a bundle whose embedded prefab has any other filename.
    /// Use [`World::add_prefab`] rather than inserting a custom key here.
    pub prefabs: IndexMap<String, Vec<u8>>,
    pub global_data: crate::schema::BrdbSchemaGlobalData,
    pub component_schema: BrdbSchema,
    pub entity_schema: BrdbSchema,
    /// The save format this world is written in.
    ///
    /// [`BrdbSchemaFormat::Latest`] (the default) writes every schema in the
    /// map form, as the game writes them.
    ///
    /// [`BrdbSchemaFormat::Auto`] writes each `.schema` in the container it
    /// needs: the map form for schemas with a `types` table or dynamic
    /// containers (the component schema), the array form otherwise. Every
    /// current game build reads both, but a world written this way without
    /// components has an EA3 component schema, so
    /// [`crate::migrate::needs_migration`] reports it as an EA3 save.
    ///
    /// Writing EA3 worlds is not supported: [`World::to_unsaved`] fails with
    /// [`BrdbWorldError::Ea3WriteUnsupported`].
    pub schema_format: BrdbSchemaFormat,
}

impl Default for World {
    fn default() -> Self {
        Self {
            meta: Default::default(),
            owners: Default::default(),
            bricks: Default::default(),
            grids: Default::default(),
            wires: Default::default(),
            entities: Default::default(),
            microchip_links: Default::default(),
            prefabs: Default::default(),
            global_data: Default::default(),
            component_schema: Default::default(),
            entity_schema: Default::default(),
            schema_format: BrdbSchemaFormat::Latest,
        }
    }
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    // Write a world to a file in the BRDB format
    #[cfg(feature = "brdb")]
    pub fn write_brdb(&self, path: impl AsRef<std::path::Path>) -> Result<(), BrError> {
        crate::Brdb::new(path)?.save("BRDB-RS", self)
    }

    // Write a world to a file in the BRZ format
    #[cfg(feature = "brz")]
    pub fn write_brz(&self, path: impl AsRef<std::path::Path>) -> Result<(), BrError> {
        crate::Brz::save(path, self)
    }

    // Write a world to a brz in memory
    #[cfg(feature = "brz")]
    pub fn to_brz_vec(&self) -> Result<Vec<u8>, BrError> {
        let unsaved = self.to_unsaved()?;
        let pending = unsaved.to_pending()?;
        let brz = pending.to_brz_data(Some(3))?;
        let mut data = Vec::new();
        brz.write(&mut data, Some(3))?;
        Ok(data)
    }

    /// Pin a schema taken from the built-in set to this world's format.
    fn pin_format(&self, schema: &mut BrdbSchema) {
        if self.schema_format != BrdbSchemaFormat::Auto {
            schema.set_format(self.schema_format);
        }
    }

    /// Load the full component schema and register all known type→struct
    /// mappings and wire port names from the built-in component database.
    pub fn register_all_components(&mut self) {
        let builtin = BuiltinSchemas::get();
        self.component_schema = builtin.components_max.clone();
        self.entity_schema = builtin.entities_chunks.clone();
        let (mut components, mut entities) = (
            std::mem::take(&mut self.component_schema),
            std::mem::take(&mut self.entity_schema),
        );
        self.pin_format(&mut components);
        self.pin_format(&mut entities);
        (self.component_schema, self.entity_schema) = (components, entities);
        for &(type_name, struct_name) in COMPONENT_TYPE_STRUCT_PAIRS {
            if !self.global_data.has_component_type(type_name) {
                self.global_data
                    .component_type_names
                    .insert(type_name.to_owned());
                self.global_data
                    .component_data_struct_names
                    .push(struct_name.to_owned());
            }
        }
        self.global_data
            .component_wire_port_names
            .extend(WIRE_PORT_NAMES.iter().map(|s| s.to_string()));
        for &(type_name, class_name) in ENTITY_TYPE_STRUCT_PAIRS {
            self.global_data
                .add_entity_type_with_class(type_name, class_name);
            // Register the entity data struct in the entity schema if missing.
            if self.entity_schema.get_struct(class_name).is_none() {
                self.entity_schema.add_struct(class_name.to_owned(), vec![]);
            }
        }
    }

    /// Register only the components actually used by this world's bricks,
    /// mirroring how the game embeds a bundle: `ComponentsShared.schema` and the
    /// global-data component tables carry only the data structs that appear,
    /// not the full catalog (which [`register_all_components`] embeds).
    ///
    /// Call this AFTER all bricks/grids have been added. The component schema
    /// starts from the minimal SoA scaffolding and gains each used component's
    /// data struct plus its transitive type dependencies; global-data component
    /// type/struct/port tables are rebuilt to match. Entity types are
    /// registered in full (the catalog is a single microchip-grid entry).
    ///
    /// This keeps generated bundles byte-compatible with what the current game
    /// build writes — embedding the stale full catalog can make the game reject
    /// the schema ("while building schema: while reading struct count").
    pub fn register_used_components(&mut self) {
        use std::collections::HashMap;

        let builtin = BuiltinSchemas::get();
        let max = builtin.components_max;

        // Reset to the minimal SoA scaffolding (no per-component data structs).
        let mut components = builtin.components_min.clone();
        let mut entities = builtin.entities_chunks.clone();
        // The minimal scaffolding has no types table of its own, so carry the
        // max schema's format over before any data is encoded with it.
        components.set_format(max.format());
        self.pin_format(&mut components);
        self.pin_format(&mut entities);
        self.component_schema = components;
        self.entity_schema = entities;
        self.global_data.component_type_names.clear();
        self.global_data.component_data_struct_names.clear();
        self.global_data.component_wire_port_names.clear();

        let type_to_struct: HashMap<&str, &str> =
            COMPONENT_TYPE_STRUCT_PAIRS.iter().copied().collect();

        // Component types present on bricks (main grid + sub-grids), first-seen
        // order. component_type_names (set) and component_data_struct_names
        // (parallel vec) are kept in lockstep.
        let mut seed_structs: Vec<&str> = Vec::new();
        let bricks = self
            .bricks
            .iter()
            .chain(self.grids.iter().flat_map(|(_, bs)| bs.iter()));
        for brick in bricks {
            for c in &brick.components {
                let Some(type_name) = c.component_type() else {
                    continue;
                };
                let type_name = type_name.as_ref();
                if self.global_data.has_component_type(type_name) {
                    continue;
                }
                let struct_name = type_to_struct.get(type_name).copied().unwrap_or("");
                self.global_data
                    .component_type_names
                    .insert(type_name.to_owned());
                self.global_data
                    .component_data_struct_names
                    .push(struct_name.to_owned());
                if !struct_name.is_empty() && !seed_structs.contains(&struct_name) {
                    seed_structs.push(struct_name);
                }
            }
        }

        self.component_schema
            .add_schema_meta(max.extract_structs_transitive(seed_structs));

        // Wire ports referenced by connections.
        for w in &self.wires {
            self.global_data
                .component_wire_port_names
                .insert(w.source.port_name.to_string());
            self.global_data
                .component_wire_port_names
                .insert(w.target.port_name.to_string());
        }

        // Entity types (catalog is a single microchip-grid entry; register all
        // so microchip grids resolve and their schema struct exists).
        for &(type_name, class_name) in ENTITY_TYPE_STRUCT_PAIRS {
            self.global_data
                .add_entity_type_with_class(type_name, class_name);
            if self.entity_schema.get_struct(class_name).is_none() {
                self.entity_schema.add_struct(class_name.to_owned(), vec![]);
            }
        }
    }

    /// Parse a schema string and merge its definitions.
    pub fn register_component_schema(&mut self, schema_str: &str) {
        if let Ok(meta) = BrdbSchema::parse_to_meta(schema_str) {
            self.component_schema.add_schema_meta(meta);
        }
    }

    /// Pull a single struct from the max schema.
    pub fn register_component(&mut self, struct_name: &str) {
        let max = BuiltinSchemas::get().components_max;
        if let Some(meta) = max.extract_struct_meta(struct_name) {
            self.component_schema.add_schema_meta(meta);
        }
    }

    /// Inclusive axis-aligned bounding box over the main-grid bricks, in brick
    /// units, as `(min, max)`. Returns `None` if there are no main-grid bricks.
    /// Non-main grids (microchips) are excluded — their bricks live inside an
    /// entity and are offset to the chunk center.
    pub fn brick_bounds(&self) -> Option<(Position, Position)> {
        let mut iter = self.bricks.iter();
        let (mut min, mut max) = iter.next()?.local_bounds();
        for brick in iter {
            let (bmin, bmax) = brick.local_bounds();
            min.x = min.x.min(bmin.x);
            min.y = min.y.min(bmin.y);
            min.z = min.z.min(bmin.z);
            max.x = max.x.max(bmax.x);
            max.y = max.y.max(bmax.y);
            max.z = max.z.max(bmax.z);
        }
        Some((min, max))
    }

    /// Mark this world's metadata as a prefab: sets the bundle `type` to
    /// `"Prefab"` and fills `Meta/Prefab.json` with pivots/bounds computed from
    /// the main-grid brick bounding box (see [`World::brick_bounds`]). The
    /// write path then emits a prefab bundle (Bundle.json + Prefab.json, plus
    /// the optional Screenshot/Thumbnail; no World.json).
    pub fn make_prefab(&mut self) {
        use crate::wrapper::{Position, PrefabJson};
        self.meta.bundle.level_type = "Prefab".to_string();
        let (min, max) = self
            .brick_bounds()
            .unwrap_or((Position::ZERO, Position::ZERO));
        self.meta.prefab = Some(PrefabJson::from_bounds(min, max));
    }

    /// Embed a prefab archive, content-addressed the way the game does:
    /// `Prefabs/Uploads/<BLAKE3-uppercase-hex>.brz`. Returns that path — the
    /// exact string to store in a `Prefab` component property
    /// (`bundle_path_ref`), e.g. on `BrickComponentType_PrefabSpawn` or
    /// `BrickComponentType_WireGraph_Exec_PrefabSpawner`.
    pub fn add_prefab(&mut self, brz_bytes: impl Into<Vec<u8>>) -> String {
        let bytes = brz_bytes.into();
        let hash = blake3::hash(&bytes).to_hex().to_string().to_uppercase();
        let path = format!("Prefabs/Uploads/{hash}.brz");
        self.prefabs.insert(path.clone(), bytes);
        path
    }

    /// Serialize `world` to an in-memory `.brz` and embed it as a prefab.
    #[cfg(feature = "brz")]
    pub fn add_prefab_world(&mut self, world: &World) -> Result<String, BrError> {
        Ok(self.add_prefab(world.to_brz_vec()?))
    }

    pub fn to_unsaved(&self) -> Result<UnsavedFs, BrError> {
        if self.schema_format == BrdbSchemaFormat::EA3 {
            return Err(BrdbWorldError::Ea3WriteUnsupported.into());
        }
        let mut unsaved_fs = UnsavedFs {
            meta: self.meta.clone(),
            worlds: Default::default(),
            prefabs: self.prefabs.clone(),
        };

        {
            let mut world = UnsavedWorld::default();
            let builtin = BuiltinSchemas::get();
            world.component_schema = if self.component_schema.structs.is_empty() {
                builtin.components_min.clone()
            } else {
                self.component_schema.clone()
            };
            world.entity_schema = if self.entity_schema.structs.is_empty() {
                builtin.entities_chunks.clone()
            } else {
                self.entity_schema.clone()
            };
            self.pin_format(&mut world.component_schema);
            self.pin_format(&mut world.entity_schema);
            world.global_data = self.global_data.clone();
            world.schema_format = self.schema_format;

            for o in self.owners.values() {
                world.owners.add(o);
            }

            // Register every grid's brick meta before packing ANY grid:
            // procedural type indices embed the final basic-asset count
            // (ProceduralBrickStartingIndex), so a basic asset first seen in
            // a later grid must already be registered when earlier grids
            // pack. (add_bricks_to_grid also pre-registers its own grid for
            // direct callers.)
            for b in self
                .bricks
                .iter()
                .chain(self.grids.iter().flat_map(|(_, bricks)| bricks.iter()))
            {
                world.add_brick_meta(b);
            }

            // Main grid bricks are on grid 1. Component types used by bricks
            // must be registered in global_data (e.g. via
            // register_all_components); add_bricks_to_grid surfaces a clear
            // error if one isn't, instead of panicking in the SoA builder.
            world.add_bricks_to_grid(1, &self.bricks)?;
            for (entity, bricks) in &self.grids {
                let grid_id = world.add_entity(entity);
                world.add_bricks_to_grid(grid_id, bricks)?;
            }

            // Add all non-grid entities
            for entity in &self.entities {
                world.add_entity(entity);
            }

            // Microchip linker pass
            for &(brick_id, entity_id) in &self.microchip_links {
                world
                    .add_microchip_link(brick_id, entity_id)
                    .map_err(|e| e.wrap(format!("microchip link brick={brick_id}")))?;
            }

            for (i, wire) in self.wires.iter().enumerate() {
                world
                    .add_wire(wire)
                    .map_err(|e| e.wrap(format!("wire {i}: {wire}")))?;
            }

            // Add the world
            unsaved_fs.worlds.insert(0, world);
        }

        Ok(unsaved_fs)
    }

    /// Add a single brick to the world
    pub fn add_brick(&mut self, brick: Brick) {
        self.bricks.push(brick);
    }
    /// Add multiple bricks to the world
    pub fn add_bricks(&mut self, bricks: impl IntoIterator<Item = Brick>) {
        self.bricks.extend(bricks);
    }
    pub fn add_entity(&mut self, entity: Entity) {
        self.entities.push(entity);
    }
    pub fn add_brick_grid(&mut self, entity: Entity, bricks: impl IntoIterator<Item = Brick>) {
        self.grids.push((
            entity,
            bricks
                .into_iter()
                .map(|mut b| {
                    // Shift all bricks in non-main grids to the center of the chunk
                    // Otherwise the bricks will be offset by half the chunk size
                    b.position = b.position - Position::CHUNK_HALF;
                    b
                })
                .collect(),
        ));
    }

    /// Add a single wire connection to the world
    pub fn add_wire(&mut self, conn: WireConnection) {
        self.wires.push(conn);
    }
    /// Add multiple wire connections to the world
    pub fn add_wires(&mut self, wires: impl IntoIterator<Item = WireConnection>) {
        self.wires.extend(wires);
    }
    /// Add a wire connection from one port to another
    pub fn add_wire_connection(&mut self, source: WirePort, target: WirePort) {
        self.wires.push(WireConnection { source, target });
    }

    /// Register an outer-microchip-brick ↔ inner-grid-entity pairing.
    /// The write path consumes this into
    /// `ComponentChunkSoA.microchip_brick_indices` /
    /// `microchip_brick_grid_references`. Most callers get this for free by
    /// going through `add_microchip`; use this directly only when
    /// constructing the pair manually.
    pub fn register_microchip_link(&mut self, brick_id: usize, entity_id: usize) {
        self.microchip_links.push((brick_id, entity_id));
    }

    /// Build a microchip: spawns the outer microchip brick on the main grid,
    /// calls `register_microchip_link` with the pairing, and returns the
    /// outer brick's id, the grid entity's id, and the
    /// `(Entity, Vec<Brick>)` pair the caller populates before pushing to
    /// `world.grids`.
    ///
    /// Typical usage:
    /// ```ignore
    /// let (chip_brick_id, chip_entity_id, mut inner) = world.add_microchip(
    ///     Position { x: 0, y: 0, z: 6 },
    ///     Vector3f { x: 0.0, y: 0.0, z: 40.0 },
    ///     IntVector { x: 14, y: 14, z: 2 },
    ///     true, // collapsed
    /// );
    /// inner.1.push(some_gate_brick);
    /// world.grids.push(inner);
    /// ```
    pub fn add_microchip(
        &mut self,
        position: Position,
        entity_location: crate::wrapper::Vector3f,
        plane_extent: IntVector,
        collapsed: bool,
    ) -> (usize, usize, (Entity, Vec<Brick>)) {
        // Reserve ids for both the brick and the entity so the linker can
        // resolve them at write time. We reuse Brick::next_id for the entity
        // id too — the read-side maps are keyed separately (brick_id_map vs
        // entity_index_map), so there's no collision risk.
        let entity_id = Brick::next_id();
        let (chip_brick, brick_id) = Brick {
            asset: bricks::B_MICROCHIP,
            position,
            ..Default::default()
        }
        // The saved-world format uses the component's INSTANCE name
        // (Component_Internal_Microchip), not the class name
        // (BrickComponentType_Internal_Microchip). The data struct is empty.
        .with_component_box(Box::new(LiteralComponent::new(
            "Component_Internal_Microchip",
        )))
        .with_id_split();
        self.bricks.push(chip_brick);
        self.register_microchip_link(brick_id, entity_id);
        let entity = Entity {
            asset: entities::MICROCHIP_GRID,
            id: Some(entity_id),
            location: entity_location,
            data: entities::microchip_grid_entity(
                collapsed,
                IntVector { x: 0, y: 0, z: 0 },
                plane_extent,
            ),
            ..Default::default()
        };
        (brick_id, entity_id, (entity, Vec::new()))
    }
}
