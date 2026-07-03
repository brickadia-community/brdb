use std::collections::HashMap;

use crate::{
    Wrap,
    errors::{BrError, BrdbWorldError},
    pending::BrPendingFs,
    schema::{BrdbSchema, BrdbSchemaGlobalData},
    schemas::{BRICK_CHUNK_INDEX_SOA, BRICK_CHUNK_SOA, BRICK_WIRE_SOA},
    wrapper::{
        Brick, BrickChunkIndexSoA, BrickChunkSoA, CHUNK_HALF, CHUNK_SIZE, ChunkIndex,
        ComponentChunkSoA, Entity, EntityChunkIndexSoA, EntityChunkSoA,
        LocalWirePortSource, OwnerTableSoA, RemoteWirePortSource, WireChunkSoA, WireConnection,
        WirePortTarget, WorldMeta, schemas,
    },
};

/// All of the dynamic data needed to serialize a world
pub struct UnsavedFs {
    /// Meta/
    pub meta: WorldMeta,
    /// World/
    pub worlds: HashMap<usize, UnsavedWorld>,
}

impl UnsavedFs {
    pub fn to_pending(self) -> Result<BrPendingFs, BrError> {
        BrPendingFs::from_unsaved(self)
    }
}

pub struct UnsavedWorld {
    /// World/N/GlobalData.mps
    pub global_data: BrdbSchemaGlobalData,
    /// World/N/Owners.mps
    pub owners: OwnerTableSoA,
    /// World/N/Bricks/Grids/ComponentsShared.mps
    pub component_schema: BrdbSchema,
    /// World/N/Bricks/Grids/[key.0]/
    pub grids: HashMap<usize, UnsavedGrid>,
    /// World/N/Bricks/Entities/Chunks/[key].mps
    pub entity_chunks: HashMap<ChunkIndex, EntityChunkSoA>,
    /// World/N/Bricks/Entities/ChunksShared.schema
    pub entity_schema: BrdbSchema,
    /// World/N/Bricks/Entities/ChunkIndex.mps
    pub entity_chunk_index: EntityChunkIndexSoA,

    /// World/N/Minigame.bp
    pub minigame: Option<()>, // TODO: minigames serialization
    /// World/N/Environment.bp
    pub environment: Option<()>, // TODO: environment serialization

    /// Internal map of brick id to (grid_id, chunk_index, brick_index_in_chunk)
    /// This is used to connect wires
    /// and is not saved to the world file.
    brick_id_map: HashMap<usize, (usize, ChunkIndex, usize)>,
    // Maps entity internal id to persistent index
    entity_index_map: HashMap<usize, u32>,
}

impl Default for UnsavedWorld {
    fn default() -> Self {
        Self {
            global_data: Default::default(),
            owners: Default::default(),
            component_schema: schemas::bricks_components_schema_min().to_owned(),
            grids: Default::default(),
            entity_chunks: Default::default(),
            entity_schema: schemas::entities_chunks_schema().to_owned(),
            entity_chunk_index: Default::default(),
            minigame: Default::default(),
            environment: Default::default(),
            brick_id_map: Default::default(),
            entity_index_map: Default::default(),
        }
    }
}

impl UnsavedWorld {
    pub(super) fn add_brick_meta(&mut self, brick: &Brick) {
        self.global_data.add_brick_meta(brick);
    }

    fn add_entity_meta(&mut self, entity: &Entity) {
        let type_name: &str = entity.asset.as_ref();
        if !self.global_data.entity_type_names.contains(type_name) {
            let class_name = self
                .global_data
                .get_entity_class_name(type_name)
                .unwrap_or(type_name)
                .to_owned();
            self.global_data
                .add_entity_type_with_class(type_name, &class_name);
        }
    }

    pub(super) fn add_bricks_to_grid(
        &mut self,
        grid_id: usize,
        bricks: &[Brick],
    ) -> Result<(), BrdbWorldError> {
        let mut grid = UnsavedGrid::default();

        // Register all brick meta (materials + assets) before packing any
        // brick: a procedural brick's on-disk type index embeds the
        // basic-asset count at pack time, but the file's
        // ProceduralBrickStartingIndex is stamped with the FINAL count —
        // interleaving registration with packing corrupts the indices of
        // procedural bricks that precede a first-seen basic asset.
        for b in bricks.iter() {
            self.add_brick_meta(b);
        }

        for b in bricks.iter() {
            let owner_id = b.owner_index.unwrap_or(0);
            self.owners.inc_bricks(owner_id);
            self.owners
                .inc_components(owner_id, b.components.len() as u32);

            let (chunk_index, brick_index) = grid.add_brick_inner(&self.global_data, b)?;

            if let Some(id) = b.id {
                self.brick_id_map
                    .insert(id, (grid_id, chunk_index, brick_index));
            }
        }

        self.grids.insert(grid_id, grid);
        Ok(())
    }

    pub(super) fn add_entity(&mut self, entity: &Entity) -> usize {
        // Add the entity metadata to the global data
        self.add_entity_meta(entity);

        // Update the owner table
        let owner_id = entity.owner_index.unwrap_or(0);
        self.owners.inc_entities(owner_id as usize);

        // Increment the entity persistent index
        let entity_index = self.entity_chunk_index.next_persistent_index;
        self.entity_chunk_index.next_persistent_index += 1;

        // There is only one entity chunk right now...
        let chunk_index = ChunkIndex::ZERO;
        // Create a new entity chunk if it doesn't exist
        if self.entity_chunk_index.chunk_3d_indices.is_empty() {
            self.entity_chunk_index.chunk_3d_indices.push(chunk_index);
        }
        if self.entity_chunk_index.num_entities.is_empty() {
            self.entity_chunk_index.num_entities.push(0);
        }
        self.entity_chunk_index.num_entities[0] += 1;

        self.entity_chunks
            .entry(chunk_index)
            .or_insert_with(EntityChunkSoA::default)
            .add_entity(&self.global_data, entity, entity_index);

        // Map the internal entity id to its persistent index
        if let Some(id) = entity.id {
            self.entity_index_map.insert(id, entity_index);
        }

        entity_index as usize
    }

    /// Record a brick ↔ microchip-grid-entity linkage for this world. Call
    /// once per microchip-hosting brick, after both the brick and the inner
    /// grid entity have been added (via `add_bricks_to_grid` and `add_entity`
    /// respectively — typical for a `World` built through the public API, this
    /// happens in `World::to_unsaved` for us).
    ///
    /// Appends to `ComponentChunkSoA.microchip_brick_indices` /
    /// `microchip_brick_grid_references` on the chunk that holds the brick.
    pub(super) fn add_microchip_link(
        &mut self,
        brick_id: usize,
        entity_id: usize,
    ) -> Result<(), BrError> {
        let (grid_id, chunk_index, brick_index) = self
            .brick_id_map
            .get(&brick_id)
            .copied()
            .ok_or(BrdbWorldError::UnknownBrickId(brick_id))?;
        let entity_persistent = self
            .entity_index_map
            .get(&entity_id)
            .copied()
            .ok_or(BrdbWorldError::UnknownEntityId(entity_id))?;
        let grid = self
            .grids
            .get_mut(&grid_id)
            .ok_or(BrdbWorldError::UnknownGridId(grid_id))?;
        let chunk = grid
            .components
            .entry(chunk_index)
            .or_insert_with(ComponentChunkSoA::default);
        chunk.microchip_brick_indices.push(brick_index as u32);
        chunk
            .microchip_brick_grid_references
            .push(entity_persistent);
        Ok(())
    }

    pub(super) fn add_wire(&mut self, wire: &WireConnection) -> Result<(), BrError> {
        // Resolve source wire metadata
        let (s_grid, s_chunk, s_brick) = self
            .brick_id_map
            .get(&wire.source.brick_id)
            .ok_or_else(|| BrdbWorldError::UnknownBrickId(wire.source.brick_id))?;
        let s_comp_ty = self
            .global_data
            .get_component_type_index(&wire.source.component_type)
            .ok_or_else(|| {
                BrdbWorldError::UnknownComponent(wire.source.component_type.to_string())
            })?;
        let s_port_index = self
            .global_data
            .get_port_index(&wire.source.port_name)
            .ok_or_else(|| BrdbWorldError::UnknownPort(wire.source.port_name.to_string()))?;

        // Resolve target wire metadata
        let (t_grid, t_chunk, t_brick) = self
            .brick_id_map
            .get(&wire.target.brick_id)
            .ok_or_else(|| BrdbWorldError::UnknownBrickId(wire.target.brick_id))?;
        let t_comp_ty = self
            .global_data
            .get_component_type_index(&wire.target.component_type)
            .ok_or_else(|| {
                BrdbWorldError::UnknownComponent(wire.target.component_type.to_string())
            })?;
        let t_port_index = self
            .global_data
            .get_port_index(&wire.target.port_name)
            .ok_or_else(|| BrdbWorldError::UnknownPort(wire.target.port_name.to_string()))?;

        // Create the target port
        let target = WirePortTarget {
            brick_index_in_chunk: *t_brick as u32,
            component_type_index: t_comp_ty,
            port_index: t_port_index,
        };

        // Wires are inserted in the target grid
        let grid = self
            .grids
            .get_mut(t_grid)
            .ok_or_else(|| BrdbWorldError::UnknownGridId(*t_grid))?;

        // Increment the wire count for the target chunk
        let chunk_id = grid.get_chunk_index(*t_chunk);
        grid.chunk_index.num_wires[chunk_id] += 1;

        // If the target and source are in the same grid and chunk,
        // we can use a local wire source.
        if t_grid == s_grid && t_chunk == s_chunk {
            let source = LocalWirePortSource {
                brick_index_in_chunk: *s_brick as u32,
                component_type_index: s_comp_ty,
                port_index: s_port_index,
            };
            grid.add_local_wire(*t_chunk, source, target);
        } else {
            let source = RemoteWirePortSource {
                grid_persistent_index: *s_grid as u32,
                chunk_index: *s_chunk,
                brick_index_in_chunk: *s_brick as u32,
                component_type_index: s_comp_ty,
                port_index: s_port_index,
            };
            grid.add_remote_wire(*t_chunk, source, target);
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct UnsavedGrid {
    /// World/N/Bricks/Grids/I/ChunkIndex.mps
    pub chunk_index: BrickChunkIndexSoA,
    /// World/N/Bricks/Grids/I/Chunks/[key].mps
    pub bricks: HashMap<ChunkIndex, BrickChunkSoA>,
    /// World/N/Bricks/Grids/I/Components/[key].mps
    pub components: HashMap<ChunkIndex, ComponentChunkSoA>,
    /// World/N/Bricks/Grids/I/Wires/[key].mps
    pub wires: HashMap<ChunkIndex, WireChunkSoA>,

    /// Map of 3d chunk index to serial index in the `chunk_index` array
    /// Used to quickly find the index of a chunk in the `chunk_index` array
    chunk_index_map: HashMap<ChunkIndex, usize>,
}

impl UnsavedGrid {
    /// Appends a new chunk to the chunk_index SoA, returning the index of the chunk
    pub fn get_chunk_index(&mut self, chunk_index: ChunkIndex) -> usize {
        // Add the chunk to the index if it doesn't exist
        if let Some(index) = self.chunk_index_map.get(&chunk_index) {
            *index
        } else {
            self.chunk_index.chunk_3d_indices.push(chunk_index);
            // ChunkOffsets and ChunkSizes must always be kept in sync with
            // chunk_3d_indices. Chunk (0,0,0) uses offset (0,0,0);
            // non-zero chunks use (CHUNK_HALF, CHUNK_HALF, CHUNK_HALF).
            let off = if chunk_index == ChunkIndex::ZERO {
                0
            } else {
                CHUNK_HALF
            };
            self.chunk_index.chunk_offsets.push(crate::IntVector {
                x: off,
                y: off,
                z: off,
            });
            self.chunk_index.chunk_sizes.push(CHUNK_SIZE);
            self.chunk_index.num_bricks.push(0);
            self.chunk_index.num_components.push(0);
            self.chunk_index.num_wires.push(0);
            let index = self.chunk_index_map.len();
            self.chunk_index_map.insert(chunk_index, index);
            index
        }
    }

    /// Add a brick to the grid, returning the chunk index and the brick index.
    pub fn add_brick(
        &mut self,
        global_data: &BrdbSchemaGlobalData,
        brick: &Brick,
    ) -> Result<(ChunkIndex, usize), BrdbWorldError> {
        self.add_brick_inner(global_data, brick)
    }

    pub(crate) fn add_brick_inner(
        &mut self,
        global_data: &BrdbSchemaGlobalData,
        brick: &Brick,
    ) -> Result<(ChunkIndex, usize), BrdbWorldError> {
        let chunk_index = brick.position.to_relative().0;
        self.bricks
            .entry(chunk_index)
            .or_insert_with(BrickChunkSoA::default)
            .add_brick(global_data, brick);
        let i = self.get_chunk_index(chunk_index);
        let brick_index = self.chunk_index.num_bricks[i];
        self.chunk_index.num_bricks[i] += 1;
        self.chunk_index.num_components[i] += brick.components.len() as u32;

        if !brick.components.is_empty() {
            let chunk = self
                .components
                .entry(chunk_index)
                .or_insert_with(ComponentChunkSoA::default);
            for c in &brick.components {
                chunk.add_component(global_data, brick_index, c.as_ref())?;
            }
        }

        Ok((chunk_index, brick_index as usize))
    }

    pub fn add_local_wire(
        &mut self,
        chunk: ChunkIndex,
        source: LocalWirePortSource,
        target: WirePortTarget,
    ) {
        self.wires
            .entry(chunk)
            .or_insert_with(WireChunkSoA::default)
            .add_local_wire(source, target);
    }

    pub fn add_remote_wire(
        &mut self,
        chunk: ChunkIndex,
        source: RemoteWirePortSource,
        target: WirePortTarget,
    ) {
        self.wires
            .entry(chunk)
            .or_insert_with(WireChunkSoA::default)
            .add_remote_wire(source, target);
    }

    /// This function converts a single unsaved grid into a pending folder
    /// to be placed in Bricks/Grids/N:
    ///  - (Bricks/Grids/N)/Components
    ///  - (Bricks/Grids/N)/Wires
    ///  - (Bricks/Grids/N)/ChunkIndex.mps
    ///
    /// `proc_brick_starting_index` can be obtained from global_data.proc_brick_starting_index()
    pub fn to_pending(
        self,
        proc_brick_starting_index: u32,
        component_schema: &BrdbSchema,
    ) -> Result<BrPendingFs, BrError> {
        use BrPendingFs::*;
        let brick_chunk_index_schema = schemas::bricks_chunk_index_schema();
        let brick_chunk_schema = schemas::bricks_chunks_schema();
        let wires_schema = schemas::bricks_wires_schema();

        let mut grid_dir = vec![(
            "ChunkIndex.mps".to_owned(),
            File(Some(
                brick_chunk_index_schema
                    .write_brdb(BRICK_CHUNK_INDEX_SOA, &self.chunk_index)
                    .about_f(|| format!("ChunkIndex.mps"))?,
            )),
        )];

        let brick_chunks_dir = self
            .bricks
            .into_iter()
            .map(|(chunk, mut bricks)| {
                bricks.procedural_brick_starting_index = proc_brick_starting_index;
                Ok((
                    format!("{chunk}.mps"),
                    File(Some(
                        brick_chunk_schema
                            .write_brdb(BRICK_CHUNK_SOA, &bricks)
                            .about_f(|| format!("Chunks/{chunk}.mps"))?,
                    )),
                ))
            })
            .collect::<Result<Vec<_>, BrError>>()?;
        let component_chunks_dir = self
            .components
            .into_iter()
            .map(|(chunk, components)| {
                let buf = components
                    .to_bytes(component_schema)
                    .about_f(|| format!("Components/{chunk}.mps"))?;

                Ok((format!("{chunk}.mps"), File(Some(buf))))
            })
            .collect::<Result<Vec<_>, BrError>>()?;
        let wire_chunks_dir = self
            .wires
            .iter()
            .map(|(chunk, wires)| {
                Ok((
                    format!("{chunk}.mps"),
                    File(Some(
                        wires_schema
                            .write_brdb(BRICK_WIRE_SOA, wires)
                            .about_f(|| format!("Wires/{chunk}.mps"))?,
                    )),
                ))
            })
            .collect::<Result<Vec<_>, BrError>>()?;

        // Append non-empty chunk directories to the grid directory
        if !brick_chunks_dir.is_empty() {
            grid_dir.push(("Chunks".to_owned(), Folder(Some(brick_chunks_dir))));
        }
        if !component_chunks_dir.is_empty() {
            grid_dir.push(("Components".to_owned(), Folder(Some(component_chunks_dir))));
        }
        if !wire_chunks_dir.is_empty() {
            grid_dir.push(("Wires".to_owned(), Folder(Some(wire_chunks_dir))));
        }

        Ok(Folder(Some(grid_dir)))
    }
}
