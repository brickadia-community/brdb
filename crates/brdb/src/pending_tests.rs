use std::error::Error;

use crate::{
    pending::BrPendingFs,
    schema::{ReadBrdbSchema, as_brdb::AsBrdbValue},
    wrapper::{
        Brick, Entity, World,
        schemas::{
            BRICK_CHUNK_INDEX_SOA, BRICK_CHUNK_SOA, BRICK_COMPONENT_SOA, BRICK_WIRE_SOA,
            ENTITY_CHUNK_SOA, GLOBAL_DATA_SOA, OWNER_TABLE_SOA,
        },
    },
};

#[test]
fn test_brick_write() -> Result<(), Box<dyn Error>> {
    let mut world = World::new();
    world.register_all_components();
    world.add_brick(Brick {
        position: (0, 0, 3).into(),
        color: (255, 0, 0).into(),
        ..Default::default()
    });
    world.add_brick_grid(
        Entity {
            frozen: true,
            ..Default::default()
        },
        [Brick {
            position: (0, 0, 3).into(),
            color: (255, 0, 0).into(),
            ..Default::default()
        }],
    );

    let pending = world.to_unsaved()?.to_pending()?;
    let root = pending.to_root().unwrap();

    // Get the world from the root of the tree,
    // validate the Meta dir exists
    let world_dir = 'world: {
        for (name, root_dir) in root {
            let children = root_dir.to_folder().unwrap();
            match name.as_str() {
                // Ensure all expected meta files exist
                "Meta" => {
                    children.into_iter().for_each(|(n, _)| match n.as_str() {
                        "World.json" | "Bundle.json" | "Screenshot.jpg" | "Thumbnail.png" => {}
                        other => panic!("unknown Meta/{other}"),
                    });
                    continue;
                }
                "World" => {
                    assert_eq!(children.len(), 1);
                    // Get the /0 directory in the world
                    break 'world children.into_iter().next().unwrap().1.to_folder().unwrap();
                }
                other => panic!("unknown {other}"),
            };
        }
        unreachable!()
    };

    let mut owners_schema = None;
    let mut owners_vec = None;
    let mut global_data_schema = None;
    let mut global_data_vec = None;
    let mut bricks_dir = None;
    let mut entities_dir = None;

    for (n, d) in world_dir {
        match (n.as_str(), d) {
            ("Owners.schema", BrPendingFs::File(Some(data))) => {
                owners_schema = Some(data.as_slice().read_brdb_schema()?);
            }
            ("Owners.mps", BrPendingFs::File(data)) => {
                owners_vec = data;
            }
            ("GlobalData.schema", BrPendingFs::File(Some(data))) => {
                global_data_schema = Some(data.as_slice().read_brdb_schema()?);
            }
            ("GlobalData.mps", BrPendingFs::File(data)) => {
                global_data_vec = data;
            }
            ("Bricks", BrPendingFs::Folder(items)) => {
                bricks_dir = items;
            }
            ("Entities", BrPendingFs::Folder(items)) => {
                entities_dir = items;
            }
            (name, BrPendingFs::File(_)) => unreachable!("{name}: no more files"),
            (name, BrPendingFs::Folder(_)) => unreachable!("{name}: no more folders"),
            (name, BrPendingFs::Root(_)) => unreachable!("{name}: no root"),
        }
    }

    // Ensure global data can read completely
    let global_data = global_data_vec
        .unwrap()
        .as_slice()
        .read_brdb(global_data_schema.as_ref().unwrap(), GLOBAL_DATA_SOA)?;

    // Ensure owners can read completely
    let _owners = owners_vec
        .unwrap()
        .as_slice()
        .read_brdb(&owners_schema.unwrap(), OWNER_TABLE_SOA)?;

    let mut brick_index_schema = None;
    let mut brick_schema = None;
    let mut component_schema = None;
    let mut wire_schema = None;
    let mut brick_grids = None;

    for (n, fs) in bricks_dir.unwrap() {
        match (n.as_str(), fs) {
            ("Grids", BrPendingFs::Folder(items)) => {
                brick_grids = items;
            }
            ("ChunkIndexShared.schema", BrPendingFs::File(Some(data))) => {
                brick_index_schema = Some(data.as_slice().read_brdb_schema()?);
            }
            ("ChunksShared.schema", BrPendingFs::File(Some(data))) => {
                brick_schema = Some(data.as_slice().read_brdb_schema()?);
            }
            ("ComponentsShared.schema", BrPendingFs::File(Some(data))) => {
                component_schema = Some(data.as_slice().read_brdb_schema()?);
            }
            ("WiresShared.schema", BrPendingFs::File(Some(data))) => {
                wire_schema = Some(data.as_slice().read_brdb_schema()?);
            }
            (other, f) => unreachable!("unknown Bricks/{other}: {f}"),
        }
    }

    let component_schema = component_schema.as_ref().unwrap();

    for (grid_id, grid) in brick_grids.unwrap() {
        let children = grid.to_folder().unwrap();
        for (n, child) in children {
            match (n.as_str(), child) {
                ("Chunks", BrPendingFs::Folder(Some(chunks))) => {
                    for (_, c) in chunks {
                        let _chunk = c
                            .to_file()
                            .unwrap()
                            .as_slice()
                            .read_brdb(brick_schema.as_ref().unwrap(), BRICK_CHUNK_SOA)?;
                    }
                }
                ("Components", BrPendingFs::Folder(Some(chunks))) => {
                    for (_, c) in chunks {
                        let content = c.to_file().unwrap();
                        let buf = &mut content.as_slice();
                        let chunk = buf.read_brdb(&component_schema, BRICK_COMPONENT_SOA)?;

                        let type_counters = chunk.prop("ComponentTypeCounters")?.as_array()?;
                        for counter in type_counters {
                            let type_idx = counter.as_struct()?.prop("TypeIndex")?.as_brdb_u32()?;
                            let num_instances =
                                counter.as_struct()?.prop("NumInstances")?.as_brdb_u32()?;
                            let struct_name = global_data
                                .prop("ComponentDataStructNames")?
                                .index(type_idx as usize)?
                                .map(|s| s.as_str())
                                .transpose()?
                                .unwrap_or("illegal")
                                .to_owned();

                            if struct_name == "None" {
                                continue;
                            }

                            for _ in 0..num_instances {
                                let _component = buf.read_brdb(&component_schema, &struct_name)?;
                            }
                        }
                    }
                }
                ("Wires", BrPendingFs::Folder(Some(chunks))) => {
                    for (_, c) in chunks {
                        let _chunk = c
                            .to_file()
                            .unwrap()
                            .as_slice()
                            .read_brdb(wire_schema.as_ref().unwrap(), BRICK_WIRE_SOA)?;
                    }
                }
                ("ChunkIndex.mps", BrPendingFs::File(data)) => {
                    // read the chunk index
                    let _chunk_index = data
                        .unwrap()
                        .as_slice()
                        .read_brdb(brick_index_schema.as_ref().unwrap(), BRICK_CHUNK_INDEX_SOA)?;
                }
                (n, other) => unreachable!("unknown Grids/{grid_id}/{n}: {other}"),
            }
        }
    }

    let mut _entity_index_schema = None;
    let mut _entity_index_vec = None;
    let mut entity_schema = None;
    let mut entity_chunks = None;

    for (n, fs) in entities_dir.unwrap() {
        match (n.as_str(), fs) {
            ("Chunks", BrPendingFs::Folder(items)) => {
                entity_chunks = items;
            }
            ("ChunksShared.schema", BrPendingFs::File(data)) => {
                entity_schema = Some(data.unwrap().as_slice().read_brdb_schema()?);
            }
            ("ChunkIndex.schema", BrPendingFs::File(data)) => {
                _entity_index_schema = Some(data.unwrap().as_slice().read_brdb_schema()?);
            }
            ("ChunkIndex.mps", BrPendingFs::File(data)) => {
                _entity_index_vec = data;
            }
            (n, other) => unreachable!("unknown Entities/{n}: {other}"),
        }
    }

    // Ensure all the chunks can be read
    for (_chunk_id, chunk) in entity_chunks.unwrap() {
        let content = chunk.to_file().unwrap();
        let buf = &mut content.as_slice();
        let _chunk_data = buf.read_brdb(entity_schema.as_ref().unwrap(), ENTITY_CHUNK_SOA)?;
    }

    Ok(())
}
