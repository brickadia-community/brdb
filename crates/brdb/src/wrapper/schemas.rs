use crate::schema::BrdbSchema;
use std::sync::OnceLock;

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

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) =
            BrdbSchema::parse_to_meta(include_str!("../../schemas/BRSavedGlobalDataSoA.schema"))
                .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}
/// World/0/Bricks/ChunksShared.schema
pub fn bricks_chunks_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) =
            BrdbSchema::parse_to_meta(include_str!("../../schemas/BRSavedBrickChunkSoA.schema"))
                .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}

/// World/0/Bricks/ChunkIndexShared.schema
pub fn bricks_chunk_index_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) = BrdbSchema::parse_to_meta(include_str!(
            "../../schemas/BRSavedBrickChunkIndexSoA.schema"
        ))
        .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}

/// World/0/Bricks/ComponentsShared.schema
pub fn bricks_components_schema_min() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) = BrdbSchema::parse_to_meta(include_str!(
            "../../schemas/BRSavedComponentChunkSoA.schema"
        ))
        .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}

/// World/0/Bricks/ComponentsShared.schema
pub fn bricks_components_schema_max() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) = BrdbSchema::parse_to_meta(include_str!(
            "../../schemas/BRSavedComponentChunkSoA_max.schema"
        ))
        .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}

/// World/0/Bricks/WiresShared.schema
pub fn bricks_wires_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) =
            BrdbSchema::parse_to_meta(include_str!("../../schemas/BRSavedWireChunkSoA.schema"))
                .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}

/// World/0/Owners.schema
pub fn owners_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) =
            BrdbSchema::parse_to_meta(include_str!("../../schemas/BRSavedOwnerTableSoA.schema"))
                .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}

/// World/0/Entities/ChunkIndex.schema
pub fn entities_chunk_index_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) = BrdbSchema::parse_to_meta(include_str!(
            "../../schemas/BRSavedEntityChunkIndexSoA.schema"
        ))
        .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
}

/// World/0/Entities/ChunksShared.schema
pub fn entities_chunks_schema() -> &'static BrdbSchema {
    static SCHEMA: OnceLock<BrdbSchema> = OnceLock::new();

    &SCHEMA.get_or_init(|| {
        let (enums, variants, structs) =
            BrdbSchema::parse_to_meta(include_str!("../../schemas/BRSavedEntityChunkSoA.schema"))
                .unwrap();
        BrdbSchema::from_meta(enums, variants, structs)
    })
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
    }
}
