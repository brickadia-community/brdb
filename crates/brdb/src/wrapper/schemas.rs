use crate::schema::{BrdbSchema, BrdbSchemaFormat};
use std::sync::OnceLock;

/// Parse a schema embedded as plaintext; the embedded sources are fixed at
/// build time, so a parse failure is a bug.
fn parse_embedded(text: &str) -> BrdbSchema {
    BrdbSchema::new_parsed(text).unwrap()
}

pub const GLOBAL_DATA_SOA: &str = "BRSavedGlobalDataSoA";
pub const BRICK_CHUNK_SOA: &str = "BRSavedBrickChunkSoA";
pub const BRICK_COMPONENT_SOA: &str = "BRSavedComponentChunkSoA";
pub const BRICK_WIRE_SOA: &str = "BRSavedWireChunkSoA";
pub const BRICK_CHUNK_INDEX_SOA: &str = "BRSavedBrickChunkIndexSoA";
pub const ENTITY_CHUNK_SOA: &str = "BRSavedEntityChunkSoA";
pub const ENTITY_CHUNK_INDEX_SOA: &str = "BRSavedEntityChunkIndexSoA";
pub const OWNER_TABLE_SOA: &str = "BRSavedOwnerTableSoA";

/// World/0/GlobalData.schema
pub fn global_data_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA
        .get_or_init(|| parse_embedded(include_str!("../../schemas/BRSavedGlobalDataSoA.schema")))
}
/// World/0/Bricks/ChunksShared.schema
pub fn bricks_chunks_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA
        .get_or_init(|| parse_embedded(include_str!("../../schemas/BRSavedBrickChunkSoA.schema")))
}

/// World/0/Bricks/ChunkIndexShared.schema
pub fn bricks_chunk_index_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        parse_embedded(include_str!(
            "../../schemas/BRSavedBrickChunkIndexSoA.schema"
        ))
    })
}

/// World/0/Bricks/ComponentsShared.schema
pub fn bricks_components_schema_min() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        parse_embedded(include_str!(
            "../../schemas/BRSavedComponentChunkSoA.schema"
        ))
    })
}

/// World/0/Bricks/ComponentsShared.schema
pub fn bricks_components_schema_max() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        parse_embedded(include_str!(
            "../../schemas/BRSavedComponentChunkSoA_max.schema"
        ))
    })
}

/// World/0/Bricks/WiresShared.schema
pub fn bricks_wires_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| parse_embedded(include_str!("../../schemas/BRSavedWireChunkSoA.schema")))
}

/// World/0/Owners.schema
pub fn owners_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA
        .get_or_init(|| parse_embedded(include_str!("../../schemas/BRSavedOwnerTableSoA.schema")))
}

/// World/0/Entities/ChunkIndex.schema
pub fn entities_chunk_index_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        parse_embedded(include_str!(
            "../../schemas/BRSavedEntityChunkIndexSoA.schema"
        ))
    })
}

/// World/0/Entities/ChunksShared.schema
pub fn entities_chunks_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA
        .get_or_init(|| parse_embedded(include_str!("../../schemas/BRSavedEntityChunkSoA.schema")))
}

/// `schema` pinned to an explicit format (borrowed as-is for `Auto`).
pub(crate) fn pinned(
    schema: &'static BrdbSchema,
    format: BrdbSchemaFormat,
) -> std::borrow::Cow<'static, BrdbSchema> {
    if format == BrdbSchemaFormat::Auto || schema.format() == format {
        std::borrow::Cow::Borrowed(schema)
    } else {
        let mut schema = schema.clone();
        schema.set_format(format);
        std::borrow::Cow::Owned(schema)
    }
}

/// The built-in schemas a world is written with.
pub(crate) struct BuiltinSchemas {
    pub global_data: &'static BrdbSchema,
    pub owners: &'static BrdbSchema,
    pub bricks_chunk_index: &'static BrdbSchema,
    pub bricks_chunks: &'static BrdbSchema,
    pub bricks_wires: &'static BrdbSchema,
    pub components_min: &'static BrdbSchema,
    pub components_max: &'static BrdbSchema,
    pub entities_chunk_index: &'static BrdbSchema,
    pub entities_chunks: &'static BrdbSchema,
}

impl BuiltinSchemas {
    pub fn get() -> Self {
        Self {
            global_data: global_data_schema(),
            owners: owners_schema(),
            bricks_chunk_index: bricks_chunk_index_schema(),
            bricks_chunks: bricks_chunks_schema(),
            bricks_wires: bricks_wires_schema(),
            components_min: bricks_components_schema_min(),
            components_max: bricks_components_schema_max(),
            entities_chunk_index: entities_chunk_index_schema(),
            entities_chunks: entities_chunks_schema(),
        }
    }
}

#[cfg(test)]
mod test {
    /// Ensure the above schemas compile and can be instantiated.
    #[test]
    fn test_schema() {
        use super::*;
        let _ = global_data_schema();
        let _ = bricks_chunks_schema();
        let _ = bricks_chunk_index_schema();
        let _ = bricks_components_schema_min();
        let _ = bricks_components_schema_max();
        let _ = bricks_wires_schema();
        let _ = owners_schema();
        let _ = entities_chunk_index_schema();
        let _ = entities_chunks_schema();
        let _ = BuiltinSchemas::get();
    }
}
