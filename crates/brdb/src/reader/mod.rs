mod reader_trait;

use indexmap::{IndexMap, IndexSet};
pub use reader_trait::{BrFsReader, FoundFile};
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Display,
    ops::Deref,
    sync::{Arc, RwLock},
};

use crate::{
    AsBrdbValue, BString, BrFsError, BrdbComponent, BrickChunkSoA, ChunkIndex, ComponentChunkSoA,
    Entity, EntityChunkIndexSoA, EntityChunkSoA, IntVector, Wrap,
    assets::LiteralComponent,
    errors::{BrError, BrdbSchemaError},
    lookup_entity_struct_name,
    pending::BrPendingFs,
    schema::{BrdbSchema, BrdbSchemaGlobalData, BrdbStruct, BrdbValue, ReadBrdbSchema},
    schemas::{BRICK_COMPONENT_SOA, BRICK_WIRE_SOA, ENTITY_CHUNK_SOA},
    wrapper::schemas::{
        BRICK_CHUNK_INDEX_SOA, BRICK_CHUNK_SOA, ENTITY_CHUNK_INDEX_SOA, GLOBAL_DATA_SOA,
        OWNER_TABLE_SOA,
    },
};

pub struct BrReader<T> {
    reader: T,
    global_data: RwLock<Option<Arc<BrdbSchemaGlobalData>>>,
    owners_schema: RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
    components_schema: RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
    wires_schema: RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
    bricks_schema: RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
    brick_chunk_index_schema: RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
    entity_chunk_index_schema: RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
    entities_schema: RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
}
impl<T> Deref for BrReader<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.reader
    }
}

pub trait IntoReader {
    type Inner;
    fn into_reader(self) -> BrReader<Self::Inner>
    where
        Self: Sized;
}

impl<T> IntoReader for T
where
    T: BrFsReader,
{
    type Inner = Self;
    fn into_reader(self) -> BrReader<Self> {
        BrReader::new(self)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkMeta {
    pub index: ChunkIndex,
    pub chunk_offset: IntVector,
    pub chunk_size: i32,
    pub num_bricks: u32,
    pub num_wires: u32,
    pub num_components: u32,
}
impl Deref for ChunkMeta {
    type Target = ChunkIndex;

    fn deref(&self) -> &Self::Target {
        &self.index
    }
}
impl AsRef<ChunkIndex> for ChunkMeta {
    fn as_ref(&self) -> &ChunkIndex {
        &self.index
    }
}
impl From<ChunkMeta> for ChunkIndex {
    fn from(value: ChunkMeta) -> Self {
        value.index
    }
}
impl Display for ChunkMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.index)
    }
}

impl<T> BrReader<T> {
    /// Fallback name used when type/struct names are not found in global data
    const ILLEGAL_NAME: &'static str = "illegal";

    pub fn new(brdb: T) -> Self
    where
        T: BrFsReader,
    {
        Self {
            reader: brdb,
            global_data: Default::default(),
            owners_schema: Default::default(),
            components_schema: Default::default(),
            wires_schema: Default::default(),
            bricks_schema: Default::default(),
            brick_chunk_index_schema: Default::default(),
            entity_chunk_index_schema: Default::default(),
            entities_schema: Default::default(),
        }
    }

    /// Helper method to load a schema at a specific revision
    fn load_schema_rev(
        &self,
        cache: &RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
        revision: i64,
        path: &str,
        parse_fn: impl FnOnce(&mut &[u8]) -> Result<Arc<BrdbSchema>, BrError>,
    ) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        // Check if we have this revision cached
        if let Some(schema) = cache.read().unwrap().get(&revision) {
            return Ok(schema.clone());
        }

        // Load the schema file revision that was live when this data was
        // written. Schemas evolve over a world's history (fields are added to
        // component structs, etc.), so decoding a chunk requires the schema as
        // it existed at the chunk's revision, not the latest one.
        let schema_file = self
            .find_file_by_path_at_revision(path, revision)?
            .ok_or(BrError::Fs(BrFsError::NotFound(path.to_string())))?;

        let schema_data = self.find_blob(schema_file.blob_id)?.read()?;
        let schema = parse_fn(&mut schema_data.as_slice())?;

        cache.write().unwrap().insert(revision, schema.clone());
        Ok(schema)
    }

    /// Helper method to get the latest schema, calling schema_rev if not cached
    fn get_latest_schema(
        &self,
        cache: &RwLock<BTreeMap<i64, Arc<BrdbSchema>>>,
        path: &str,
        schema_rev_fn: impl FnOnce(i64) -> Result<Arc<BrdbSchema>, BrError>,
    ) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        // Check if we have the latest revision cached
        if let Some((_, schema)) = cache.read().unwrap().iter().next_back() {
            return Ok(schema.clone());
        }

        // Find the schema file to get its created_at timestamp
        let found = self
            .find_file_by_path(path)?
            .ok_or(BrError::Fs(BrFsError::NotFound(path.to_string())))?;

        schema_rev_fn(found.created_at)
    }

    /// Helper method to find a file, get its schema at the file's revision, and read the data
    fn read_mps_with_schema(
        &self,
        path: &str,
        schema_getter: impl FnOnce(i64) -> Result<Arc<BrdbSchema>, BrError>,
        struct_name: &str,
    ) -> Result<BrdbValue, BrError>
    where
        T: BrFsReader,
    {
        // Find the file and get its created_at timestamp
        let found = self
            .find_file_by_path(path)?
            .ok_or(BrError::Fs(BrFsError::NotFound(path.to_string())))?;

        // Get the schema at the file's revision
        let schema = schema_getter(found.created_at)?;

        // Read and parse the data
        let data = self.find_blob(found.blob_id)?.read()?;
        Ok(data.as_slice().read_brdb(&schema, struct_name)?)
    }

    /// Convert this filesystem to a pending filesystem with all files present
    pub fn to_pending(&self) -> Result<BrPendingFs, BrFsError>
    where
        T: BrFsReader,
    {
        self.reader.get_fs()?.to_pending(&self.reader)
    }

    /// Convert this filesystem to a pending filesystem all files in Patch mode (None for unchanged)
    pub fn to_pending_patch(&self) -> Result<BrPendingFs, BrFsError>
    where
        T: BrFsReader,
    {
        self.reader.get_fs()?.to_pending_patch()
    }

    /// Parse `Meta/Bundle.json` (present in every bundle).
    pub fn bundle_json(&self) -> Result<crate::wrapper::BundleJson, BrError>
    where
        T: BrFsReader,
    {
        let bytes = self.read_file("Meta/Bundle.json")?;
        serde_json::from_slice(&bytes).about("Meta/Bundle.json")
    }

    /// Read an optional Meta file; `Ok(None)` when the file doesn't exist
    /// (e.g. `World.json` in a prefab bundle).
    fn optional_file(&self, path: &str) -> Result<Option<Vec<u8>>, BrError>
    where
        T: BrFsReader,
    {
        if self.find_file_by_path(path)?.is_none() {
            return Ok(None);
        }
        Ok(Some(self.read_file(path)?))
    }

    /// Parse `Meta/World.json`; `None` for prefab bundles.
    pub fn world_json(&self) -> Result<Option<crate::wrapper::WorldJson>, BrError>
    where
        T: BrFsReader,
    {
        self.optional_file("Meta/World.json")?
            .map(|b| serde_json::from_slice(&b).about("Meta/World.json"))
            .transpose()
    }

    /// Parse `Meta/Prefab.json`; `None` for world bundles.
    pub fn prefab_json(&self) -> Result<Option<crate::wrapper::PrefabJson>, BrError>
    where
        T: BrFsReader,
    {
        self.optional_file("Meta/Prefab.json")?
            .map(|b| serde_json::from_slice(&b).about("Meta/Prefab.json"))
            .transpose()
    }

    /// `Meta/Thumbnail.png` bytes, when present.
    pub fn thumbnail(&self) -> Result<Option<Vec<u8>>, BrError>
    where
        T: BrFsReader,
    {
        self.optional_file("Meta/Thumbnail.png")
    }

    /// `Meta/Screenshot.jpg` bytes, when present.
    pub fn screenshot(&self) -> Result<Option<Vec<u8>>, BrError>
    where
        T: BrFsReader,
    {
        self.optional_file("Meta/Screenshot.jpg")
    }

    /// Assemble the same [`crate::wrapper::WorldMeta`] the wrapper writes:
    /// bundle, world/prefab JSON, screenshot and thumbnail.
    pub fn world_meta(&self) -> Result<crate::wrapper::WorldMeta, BrError>
    where
        T: BrFsReader,
    {
        Ok(crate::wrapper::WorldMeta {
            bundle: self.bundle_json()?,
            screenshot: self.screenshot()?,
            thumbnail: self.thumbnail()?,
            world: self.world_json()?.unwrap_or_default(),
            prefab: self.prefab_json()?,
        })
    }

    /// Root-relative paths of every embedded prefab (files under `Prefabs/`),
    /// e.g. `Prefabs/Uploads/<HASH>.brz`. Empty when the bundle embeds none.
    /// These are the exact strings `Prefab` component properties
    /// (`bundle_path_ref`) reference.
    pub fn prefab_paths(&self) -> Result<Vec<String>, BrError>
    where
        T: BrFsReader,
    {
        use crate::fs::BrFs;
        fn collect(fs: &BrFs, path: String, out: &mut Vec<String>) {
            match fs {
                BrFs::Root(children) | BrFs::Folder(_, children) => {
                    for (name, child) in children {
                        collect(child, format!("{path}/{name}"), out);
                    }
                }
                BrFs::File(_) => out.push(path),
            }
        }
        let mut out = Vec::new();
        if let BrFs::Root(children) = self.get_fs()?
            && let Some(prefabs) = children.get("Prefabs")
        {
            collect(prefabs, "Prefabs".to_string(), &mut out);
        }
        Ok(out)
    }

    /// Read every embedded prefab: root-relative path → raw `.brz` bytes.
    /// The result is directly assignable to `World::prefabs`.
    pub fn read_prefabs(&self) -> Result<IndexMap<String, Vec<u8>>, BrError>
    where
        T: BrFsReader,
    {
        let mut out = IndexMap::new();
        for path in self.prefab_paths()? {
            let bytes = self.read_file(&path)?;
            out.insert(path, bytes);
        }
        Ok(out)
    }

    /// Parse an embedded prefab archive (a path from [`Self::prefab_paths`] or
    /// a component's `Prefab` property). Chain `.into_reader()` to read inside.
    #[cfg(feature = "brz")]
    pub fn open_prefab(&self, path: &str) -> Result<crate::Brz, BrError>
    where
        T: BrFsReader,
    {
        Ok(crate::Brz::read_slice(&self.read_file(path)?)?)
    }

    /// Read the GlobalData
    pub fn read_global_data(&self) -> Result<Arc<BrdbSchemaGlobalData>, BrError>
    where
        T: BrFsReader,
    {
        // Parse the GlobalData schema
        let schema = self
            .read_file("World/0/GlobalData.schema")?
            .as_slice()
            .read_brdb_schema()
            .map_err(|e| e.wrap("Read GlobalData Schema"))?;

        // Parse the GlobalData struct of arrays
        let mps = self
            .read_file("World/0/GlobalData.mps")?
            .as_slice()
            .read_brdb(&schema, GLOBAL_DATA_SOA)
            .map_err(|e| e.wrap("Read BRSavedGlobalDataSoA"))?;

        let mps_struct = mps.as_struct()?;

        let str_set = |prop| {
            mps_struct
                .prop(prop)?
                .as_array()?
                .into_iter()
                .map(|s| Ok(s.as_str()?.to_owned()))
                .collect::<Result<IndexSet<String>, BrdbSchemaError>>()
        };
        let str_vec = |prop| {
            mps_struct
                .prop(prop)?
                .as_array()?
                .into_iter()
                .map(|s| Ok(s.as_str()?.to_owned()))
                .collect::<Result<Vec<String>, BrdbSchemaError>>()
        };

        // Extract the asset names and types from the data
        let mut external_asset_types = HashSet::new();
        let external_asset_references = mps_struct
            .prop("ExternalAssetReferences")?
            .as_array()?
            .into_iter()
            .map(|s| {
                let s = s.as_struct()?;
                let asset_type = s.prop("PrimaryAssetType")?.as_str()?;
                let asset_name = s.prop("PrimaryAssetName")?.as_str()?;
                external_asset_types.insert(asset_type.to_owned());
                Ok((asset_type.to_owned(), asset_name.to_owned()))
            })
            .collect::<Result<IndexSet<_>, BrdbSchemaError>>()?;

        let entity_type_names = str_set("EntityTypeNames")?;

        Ok(Arc::new(BrdbSchemaGlobalData {
            external_asset_types,
            external_asset_references,
            // Handle entity data class names fallback
            entity_data_class_names: if mps_struct.contains_key("EntityDataClassNames") {
                str_set("EntityDataClassNames")?
            } else {
                entity_type_names
                    .iter()
                    .map(|n| lookup_entity_struct_name(n).unwrap_or("Unknown").to_owned())
                    .collect()
            },
            entity_type_names,
            basic_brick_asset_names: str_set("BasicBrickAssetNames")?,
            procedural_brick_asset_names: str_set("ProceduralBrickAssetNames")?,
            material_asset_names: str_set("MaterialAssetNames")?,
            component_type_names: str_set("ComponentTypeNames")?,
            component_data_struct_names: str_vec("ComponentDataStructNames")?,
            component_wire_port_names: str_set("ComponentWirePortNames")?,
            // Added alongside the microchip schema work. Older saves won't
            // have this field; default to -1 (no global grid registered).
            global_grid_entity_type_index: if mps_struct.contains_key("GlobalGridEntityTypeIndex") {
                mps_struct.prop("GlobalGridEntityTypeIndex")?.as_brdb_i32()?
            } else {
                -1
            },
        }))
    }

    /// Read and cache the GlobalData
    pub fn global_data(&self) -> Result<Arc<BrdbSchemaGlobalData>, BrError>
    where
        T: BrFsReader,
    {
        if let Some(data) = self.global_data.read().unwrap().as_ref() {
            return Ok(data.clone());
        }
        let data = self.read_global_data()?;
        self.global_data.write().unwrap().replace(data.clone());
        Ok(data)
    }
    /// Read the Owners table
    pub fn owners_soa(&self) -> Result<BrdbStruct, BrError>
    where
        T: BrFsReader,
    {
        let owners_data = self.read_mps_with_schema(
            "World/0/Owners.mps",
            |rev| self.owners_schema_rev(rev),
            OWNER_TABLE_SOA,
        )?;
        match owners_data {
            BrdbValue::Struct(s) => Ok(*s),
            ty => Err(BrError::Schema(BrdbSchemaError::ExpectedType(
                "Struct".to_string(),
                ty.get_type().to_owned(),
            ))),
        }
    }
    /// Read the Owners schema at a specific revision
    pub fn owners_schema_rev(&self, revision: i64) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.load_schema_rev(
            &self.owners_schema,
            revision,
            "World/0/Owners.schema",
            |data| Ok(data.read_brdb_schema()?),
        )
    }

    /// Read the Owners schema (latest revision)
    pub fn owners_schema(&self) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.get_latest_schema(&self.owners_schema, "World/0/Owners.schema", |rev| {
            self.owners_schema_rev(rev)
        })
    }
    /// Read the shared components chunk schema at a specific revision
    pub fn components_schema_rev(&self, revision: i64) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        let global_data = self.global_data()?;
        self.load_schema_rev(
            &self.components_schema,
            revision,
            "World/0/Bricks/ComponentsShared.schema",
            |data| Ok(data.read_brdb_schema_with_data(global_data)?),
        )
    }

    /// Read the shared components chunk schema (latest revision)
    pub fn components_schema(&self) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.get_latest_schema(
            &self.components_schema,
            "World/0/Bricks/ComponentsShared.schema",
            |rev| self.components_schema_rev(rev),
        )
    }

    /// Read the shared component chunks
    pub fn component_chunk_soa(
        &self,
        grid_id: usize,
        chunk: ChunkIndex,
    ) -> Result<(ComponentChunkSoA, Vec<BrdbStruct>), BrError>
    where
        T: BrFsReader,
    {
        self.component_chunk(grid_id, chunk)
    }

    /// Read the shared component chunks
    pub fn component_chunk(
        &self,
        grid_id: usize,
        chunk: ChunkIndex,
    ) -> Result<(ComponentChunkSoA, Vec<BrdbStruct>), BrError>
    where
        T: BrFsReader,
    {
        let global_data = self.global_data()?;

        let path = format!("World/0/Bricks/Grids/{grid_id}/Components/{chunk}.mps");
        let found = self
            .find_file_by_path(&path)?
            .ok_or(BrError::Fs(BrFsError::NotFound(path.clone())))?;

        let schema = self.components_schema_rev(found.created_at)?;
        let buf = self.find_blob(found.blob_id)?.read()?;
        let buf = &mut buf.as_slice();

        let mps = buf.read_brdb(&schema, BRICK_COMPONENT_SOA)?;
        let soa = ComponentChunkSoA::try_from(&mps)
            .map_err(|e| e.wrap(format!("Read component chunk {chunk}")))?;

        let mut component_data = Vec::new();
        for counter in &soa.component_type_counters {
            let type_idx = counter.type_index as usize;
            let type_name = global_data
                .component_type_names
                .get_index(type_idx)
                .cloned()
                .unwrap_or_else(|| Self::ILLEGAL_NAME.to_string());
            let struct_name = global_data
                .component_data_struct_names
                .get(type_idx)
                .cloned()
                .unwrap_or_else(|| Self::ILLEGAL_NAME.to_string());

            if struct_name == "None" {
                continue;
            }

            for i in 0..counter.num_instances {
                let BrdbValue::Struct(s) = buf
                    .read_brdb(&schema, &struct_name)
                    .map_err(|e| e.wrap(format!("Read component {i} {type_name}/{struct_name}")))?
                else {
                    continue;
                };
                component_data.push(*s);
            }
        }
        Ok((soa, component_data))
    }

    /// Read the shared wires chunk schema at a specific revision
    pub fn wires_schema_rev(&self, revision: i64) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.load_schema_rev(
            &self.wires_schema,
            revision,
            "World/0/Bricks/WiresShared.schema",
            |data| Ok(data.read_brdb_schema()?),
        )
    }

    /// Read the shared wires chunk schema (latest revision)
    pub fn wires_schema(&self) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.get_latest_schema(
            &self.wires_schema,
            "World/0/Bricks/WiresShared.schema",
            |rev| self.wires_schema_rev(rev),
        )
    }
    pub fn wire_chunk_soa(&self, grid_id: usize, chunk: ChunkIndex) -> Result<BrdbStruct, BrError>
    where
        T: BrFsReader,
    {
        let path = format!("World/0/Bricks/Grids/{grid_id}/Wires/{chunk}.mps");
        let mps =
            self.read_mps_with_schema(&path, |rev| self.wires_schema_rev(rev), BRICK_WIRE_SOA)?;
        match mps {
            BrdbValue::Struct(s) => Ok(*s),
            ty => Err(BrError::Schema(BrdbSchemaError::ExpectedType(
                "Struct".to_string(),
                ty.get_type().to_owned(),
            ))),
        }
    }
    /// Read the shared brick-chunk-index schema at a specific revision
    pub fn brick_chunk_index_schema_rev(&self, revision: i64) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.load_schema_rev(
            &self.brick_chunk_index_schema,
            revision,
            "World/0/Bricks/ChunkIndexShared.schema",
            |data| Ok(data.read_brdb_schema()?),
        )
    }

    /// Read the shared brick-chunk-index schema (latest revision)
    pub fn brick_chunk_index_schema(&self) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.get_latest_schema(
            &self.brick_chunk_index_schema,
            "World/0/Bricks/ChunkIndexShared.schema",
            |rev| self.brick_chunk_index_schema_rev(rev),
        )
    }
    /// Read the shared bricks chunk schema at a specific revision
    pub fn bricks_schema_rev(&self, revision: i64) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.load_schema_rev(
            &self.bricks_schema,
            revision,
            "World/0/Bricks/ChunksShared.schema",
            |data| Ok(data.read_brdb_schema()?),
        )
    }

    /// Read the shared bricks chunk schema (latest revision)
    pub fn bricks_schema(&self) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.get_latest_schema(
            &self.bricks_schema,
            "World/0/Bricks/ChunksShared.schema",
            |rev| self.bricks_schema_rev(rev),
        )
    }
    /// Read the brick chunk indices for a specific grid
    pub fn brick_chunk_index(&self, grid_id: usize) -> Result<Vec<ChunkMeta>, BrError>
    where
        T: BrFsReader,
    {
        let path = format!("World/0/Bricks/Grids/{grid_id}/ChunkIndex.mps");
        let brick_index = self.read_mps_with_schema(
            &path,
            |rev| self.brick_chunk_index_schema_rev(rev),
            BRICK_CHUNK_INDEX_SOA,
        )?;
        let num_bricks = brick_index.prop("NumBricks")?;
        let num_wires = brick_index.prop("NumWires")?;
        let num_components = brick_index.prop("NumComponents")?;
        let chunk_offsets = brick_index
            .contains_key("ChunkOffsets")
            .then(|| brick_index.prop("ChunkOffsets"))
            .transpose()?;
        let chunk_sizes = brick_index
            .contains_key("ChunkSizes")
            .then(|| brick_index.prop("ChunkSizes"))
            .transpose()?;

        let chunk_indices = brick_index
            .prop("Chunk3DIndices")?
            .as_array()?
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                Ok(ChunkMeta {
                    index: s.try_into()?,
                    chunk_offset: chunk_offsets
                        .map(|f| f.index_unwrap(i)?.try_into())
                        .transpose()?
                        // Defaults for old worlds
                        .unwrap_or_else(|| IntVector::new(1024, 1024, 1024)),
                    chunk_size: chunk_sizes
                        .map(|f| f.index_unwrap(i)?.as_brdb_i32())
                        .transpose()?
                        // Defaults for old worlds
                        .unwrap_or(2048),
                    num_bricks: num_bricks.index_unwrap(i)?.as_brdb_u32()?,
                    num_wires: num_wires.index_unwrap(i)?.as_brdb_u32()?,
                    num_components: num_components.index_unwrap(i)?.as_brdb_u32()?,
                })
            })
            .collect::<Result<Vec<_>, BrdbSchemaError>>()?;
        Ok(chunk_indices)
    }
    pub fn brick_chunk_soa(
        &self,
        grid_id: usize,
        chunk: ChunkIndex,
    ) -> Result<BrickChunkSoA, BrError>
    where
        T: BrFsReader,
    {
        let path = format!("World/0/Bricks/Grids/{grid_id}/Chunks/{chunk}.mps");
        let mps =
            self.read_mps_with_schema(&path, |rev| self.bricks_schema_rev(rev), BRICK_CHUNK_SOA)?;
        Ok((&mps).try_into()?)
    }
    /// Read the shared entity chunk schema at a specific revision
    pub fn entities_schema_rev(&self, revision: i64) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        let global_data = self.global_data()?;
        self.load_schema_rev(
            &self.entities_schema,
            revision,
            "World/0/Entities/ChunksShared.schema",
            |data| Ok(data.read_brdb_schema_with_data(global_data)?),
        )
    }

    /// Read the shared entity chunk schema (latest revision)
    pub fn entities_schema(&self) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.get_latest_schema(
            &self.entities_schema,
            "World/0/Entities/ChunksShared.schema",
            |rev| self.entities_schema_rev(rev),
        )
    }
    /// Read the entity chunk index schema at a specific revision
    pub fn entities_chunk_index_schema_rev(&self, revision: i64) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.load_schema_rev(
            &self.entity_chunk_index_schema,
            revision,
            "World/0/Entities/ChunkIndex.schema",
            |data| Ok(data.read_brdb_schema()?),
        )
    }

    /// Read the entity chunk index schema (latest revision)
    pub fn entities_chunk_index_schema(&self) -> Result<Arc<BrdbSchema>, BrError>
    where
        T: BrFsReader,
    {
        self.get_latest_schema(
            &self.entity_chunk_index_schema,
            "World/0/Entities/ChunkIndex.schema",
            |rev| self.entities_chunk_index_schema_rev(rev),
        )
    }

    /// Read the entity chunk indices
    pub fn entity_chunk_index(&self) -> Result<Vec<ChunkIndex>, BrError>
    where
        T: BrFsReader,
    {
        let entities_index = self.read_mps_with_schema(
            "World/0/Entities/ChunkIndex.mps",
            |rev| self.entities_chunk_index_schema_rev(rev),
            ENTITY_CHUNK_INDEX_SOA,
        )?;
        Ok(entities_index.prop("Chunk3DIndices")?.try_into()?)
    }

    /// Read the entity chunk indices
    pub fn entity_chunk_index_soa(&self) -> Result<EntityChunkIndexSoA, BrError>
    where
        T: BrFsReader,
    {
        let entities_index = self.read_mps_with_schema(
            "World/0/Entities/ChunkIndex.mps",
            |rev| self.entities_chunk_index_schema_rev(rev),
            ENTITY_CHUNK_INDEX_SOA,
        )?;
        Ok((&entities_index).try_into()?)
    }

    pub fn entity_chunk(&self, chunk: ChunkIndex) -> Result<Vec<Entity>, BrError>
    where
        T: BrFsReader,
    {
        let global_data = self.global_data()?;

        let path = format!("World/0/Entities/Chunks/{chunk}.mps");
        let found = self
            .find_file_by_path(&path)?
            .ok_or(BrError::Fs(BrFsError::NotFound(path.clone())))?;

        let schema = self.entities_schema_rev(found.created_at)?;
        let buf = self.find_blob(found.blob_id)?.read()?;
        let buf = &mut buf.as_slice();

        let mps = buf
            .read_brdb(&schema, ENTITY_CHUNK_SOA)
            .map_err(|e| e.wrap(format!("Read entity chunk {chunk}")))?;
        let soa = EntityChunkSoA::try_from(&mps)
            .map_err(|e| e.wrap(format!("Read entity chunk {chunk}")))?;

        let illegal = Self::ILLEGAL_NAME.to_string();
        let mut entity_data = Vec::new();
        let mut index = 0;

        for counter in soa.type_counters {
            let type_name = global_data
                .entity_type_names
                .get_index(counter.type_index as usize)
                .unwrap_or(&illegal);

            let struct_name = global_data
                .entity_data_class_names
                .get_index(counter.type_index as usize);

            for i in 0..counter.num_entities {
                let data: Arc<Box<dyn BrdbComponent>> = if let Some(struct_name) = struct_name {
                    let value = buf.read_brdb(&schema, struct_name).map_err(|e| {
                        e.wrap(format!("Read entity {i} {type_name}/{struct_name}"))
                    })?;
                    let hm: std::collections::HashMap<crate::BString, Box<dyn crate::AsBrdbValue>> = value
                        .as_struct()?
                        .as_hashmap()?
                        .into_iter()
                        .map(|(k, v)| (crate::BString::from(k), v))
                        .collect();
                    let component = LiteralComponent::new_from_data(type_name, Arc::new(hm));
                    Arc::new(Box::new(component))
                } else {
                    Arc::new(Box::new(()))
                };

                entity_data.push(Entity {
                    asset: BString::from(type_name),
                    id: Some(soa.persistent_indices[index] as usize),
                    owner_index: Some(soa.owner_indices[index]),
                    original_owner_index: soa.original_owner_indices.get(index).copied(),
                    location: soa.locations[index],
                    rotation: soa.rotations[index],
                    velocity: soa.linear_velocities[index],
                    angular_velocity: soa.angular_velocities[index],
                    color_and_alpha: soa.colors_and_alphas[index].clone(),
                    frozen: soa.physics_locked_flags.get(index),
                    sleeping: soa.physics_sleeping_flags.get(index),
                    data,
                });
                index += 1;
            }
        }
        Ok(entity_data)
    }

    pub fn entity_chunk_soa(
        &self,
        chunk: ChunkIndex,
    ) -> Result<(EntityChunkSoA, Vec<Option<BrdbStruct>>), BrError>
    where
        T: BrFsReader,
    {
        let path = format!("World/0/Entities/Chunks/{chunk}.mps");
        let found = self
            .find_file_by_path(&path)?
            .ok_or(BrError::Fs(BrFsError::NotFound(path.clone())))?;

        let schema = self.entities_schema_rev(found.created_at)?;
        let buf = self.find_blob(found.blob_id)?.read()?;
        let buf = &mut buf.as_slice();
        let mps = buf.read_brdb(&schema, ENTITY_CHUNK_SOA)?;
        let soa = EntityChunkSoA::try_from(&mps)
            .map_err(|e| e.wrap(format!("Read entity chunk {chunk}")))?;
        let global_data = self.global_data()?;
        let illegal = Self::ILLEGAL_NAME.to_string();

        let mut entity_data = Vec::new();

        for counter in &soa.type_counters {
            let type_name = global_data
                .entity_type_names
                .get_index(counter.type_index as usize)
                .unwrap_or(&illegal);

            let Some(struct_name) = global_data
                .entity_data_class_names
                .get_index(counter.type_index as usize)
            else {
                entity_data.push(None);
                continue;
            };
            if struct_name == "None" {
                entity_data.push(None);
                continue;
            }

            for i in 0..counter.num_entities {
                let BrdbValue::Struct(s) = buf
                    .read_brdb(&schema, struct_name)
                    .map_err(|e| e.wrap(format!("Read entity {i} {type_name}/{struct_name}")))?
                else {
                    continue;
                };

                entity_data.push(Some(*s));
            }
        }

        Ok((soa, entity_data))
    }
}
