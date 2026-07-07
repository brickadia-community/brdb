use std::path::PathBuf;

use crate::{
    BrFsReader, Brdb, IntoReader, assets,
    errors::BrError,
    schema::{ReadBrdbSchema, as_brdb::AsBrdbValue},
    tables::BrBlob,
    wrapper::{Brick, Entity, World, lookup_entity_struct_name, schemas::ENTITY_CHUNK_SOA},
};

#[test]
fn test_memory_db() -> Result<(), Box<dyn std::error::Error>> {
    // Ensures the memory db can be created without errors
    let db = Brdb::new_memory()?;

    // Insert a blob, folder, and file
    let blob_id = db.insert_blob(vec![0], BrBlob::hash(&[0]), None)?;
    let folder_id = db.insert_folder("test_folder", None, 0)?;
    let file_id = db.insert_file("test", Some(folder_id), blob_id, 0)?;

    assert_eq!(
        db.get_fs()?.render(),
        "   |-- test_folder/\n   |   |-- test\n"
    );

    // Ensure the file can be read
    assert_eq!(db.read_file("test_folder/test")?, vec![0]);

    // Delete the file
    db.delete_file(file_id, 1)?;
    assert_eq!(db.get_fs()?.render(), "   |-- test_folder/\n");
    assert!(db.read_file("test_folder/test").is_err());

    // Delete the folder
    db.delete_folder(folder_id, 1)?;
    assert_eq!(db.get_fs()?.render(), "");

    // Ensure the blob can still be found
    assert!(db.find_blob(blob_id).is_ok());
    // Ensure the blob can be found by hash
    assert!(db.find_blob_by_hash(1, &BrBlob::hash(&[0])).is_ok());
    Ok(())
}

#[cfg(feature = "wasm")]
#[test]
fn test_from_bytes_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let db = Brdb::new_memory()?;
    let blob_id = db.insert_blob(vec![1, 2, 3], BrBlob::hash(&[1, 2, 3]), None)?;
    let folder_id = db.insert_folder("test_folder", None, 0)?;
    db.insert_file("test", Some(folder_id), blob_id, 0)?;

    let bytes = db.conn.serialize(rusqlite::MAIN_DB)?.to_vec();

    let loaded = Brdb::from_bytes(&bytes)?;
    assert_eq!(loaded.read_file("test_folder/test")?, vec![1, 2, 3]);
    Ok(())
}

#[test]
fn test_memory_save() -> Result<(), Box<dyn std::error::Error>> {
    // Ensures the memory db can be created without errors
    let db = Brdb::new_memory()?.into_reader();
    let mut world = World::new();
    world.bricks.push(Brick {
        position: (0, 0, 3).into(),
        color: (255, 0, 0).into(),
        ..Default::default()
    });
    db.save("test world", &world)?;

    let mps = db.brick_chunk_soa(1, (0, 0, 0).into())?;
    let color = mps.colors_and_alphas[0];
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 0);
    assert_eq!(color.a, 5);

    Ok(())
}

/// Writes a world with one brick to test.brdb
#[test]
fn test_write_save() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("./test.brdb");

    // Ensures the memory db can be created without errors
    let db = Brdb::new(&path)?.into_reader();
    let mut world = World::new();
    world.meta.bundle.description = "Test World".to_string();
    world.bricks.push(Brick {
        position: (0, 0, 6).into(),
        color: (255, 0, 0).into(),
        ..Default::default()
    });
    db.save("test world", &world)?;

    println!("{}", db.get_fs()?.render());

    let soa = db.brick_chunk_soa(1, (0, 0, 0).into())?;
    let color = soa.colors_and_alphas[0];
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 0);
    assert_eq!(color.a, 5);

    Ok(())
}

/// Writes a world with two bricks and a wire connection to wire_test.brdb
#[test]
fn test_write_wire_save() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("./wire_test.brdb");

    let db = if path.exists() {
        Brdb::open(path)?
    } else {
        Brdb::create(path)?
    };

    let mut world = World::new();
    world.register_all_components();
    world.meta.bundle.description = "Test World".to_string();

    let (a, a_id) = Brick {
        position: (0, 0, 1).into(),
        color: (255, 0, 0).into(),
        asset: assets::bricks::B_REROUTE,
        ..Default::default()
    }
    .with_component(assets::components::Rerouter)
    .with_id_split();
    let (b, b_id) = Brick {
        position: (15, 0, 1).into(),
        color: (255, 0, 0).into(),
        asset: assets::components::LogicGate::BoolNot.brick(),
        ..Default::default()
    }
    .with_component(assets::components::LogicGate::BoolNot.component())
    .with_id_split();

    world.add_bricks([a, b]);
    world.add_wire_connection(
        assets::components::LogicGate::BoolNot.output_of(b_id),
        assets::components::Rerouter::input_of(a_id),
    );

    db.save("test world", &world)?;

    println!("{}", db.get_fs()?.render());

    Ok(())
}

/// Writes a world with one brick to test.brdb
#[test]
fn test_write_entity_save() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("./entity_test.brdb");

    let db = if path.exists() {
        Brdb::open(path)?
    } else {
        Brdb::create(path)?
    };

    let mut world = World::new();
    world.register_all_components();
    world.meta.bundle.description = "Test World".to_string();
    world.add_brick_grid(
        Entity {
            frozen: true,
            location: (0.0, 0.0, 40.0).into(),
            ..Default::default()
        },
        [Brick {
            position: (0, 0, 3).into(),
            color: (0, 255, 0).into(),
            ..Default::default()
        }],
    );

    db.save("test world", &world)?;

    println!("{}", db.get_fs()?.render());

    Ok(())
}

/// Reads the world generated by `test_write_save` and prints the data.
#[test]
fn test_read_test() -> Result<(), BrError> {
    let path = PathBuf::from("./test.brdb");
    if !path.exists() {
        return Ok(());
    }
    let db = Brdb::open(path)?.into_reader();

    println!("{}", db.get_fs()?.render());

    let data = db.brick_chunk_soa(1, (0, 0, 0).into())?;
    println!("data: {data:?}");

    Ok(())
}

/// Read all the components and brick assets
#[test]
fn test_read_all_components() -> Result<(), BrError> {
    let path = PathBuf::from("../../edgea.brdb");
    if !path.exists() {
        return Ok(());
    }
    let db = Brdb::open(path)?.into_reader();

    println!("{}", db.get_fs()?.render());

    let data = db.global_data()?;
    println!("Basic Brick assets: {:?}", data.basic_brick_asset_names);
    println!("wire ports: {:?}", data.component_wire_port_names);
    println!("component types: {:?}", data.component_type_names);
    println!("component structs: {:?}", data.component_data_struct_names);
    println!("component schemas: {}", db.components_schema()?);

    let chunks = db.brick_chunk_index(1)?;
    println!("Brick chunks: {chunks:?}");
    for chunk in chunks {
        let soa = db.brick_chunk_soa(1, chunk.index)?;
        println!("Brick soa: {soa:?}");
        if chunk.num_components > 0 {
            let (_soa, components) = db.component_chunk_soa(1, chunk.index)?;
            // println!("Components soa: {soa}");
            for c in components {
                println!("Component: {c}");
            }
        }
        if chunk.num_wires > 0 {
            let soa = db.wire_chunk_soa(1, chunk.index)?;
            println!("Wires soa: {soa}");
        }
    }

    Ok(())
}

#[test]
fn test_debugging() -> Result<(), BrError> {
    let path = PathBuf::from("./entity.brdb");
    if !path.exists() {
        return Ok(());
    }
    let db = Brdb::open(path)?.into_reader();

    let global_data = db.read_global_data()?;

    println!("{}", db.get_fs()?.render());

    println!(
        "Basic Brick assets: {:?}",
        global_data.basic_brick_asset_names
    );
    println!(
        "Proc Brick assets: {:?}",
        global_data.procedural_brick_asset_names
    );
    println!("Entity assets: {:?}", global_data.entity_type_names);

    let bricks = db.brick_chunk_soa(3, (-1, -1, -1).into())?;
    println!("Bricks: {bricks:?}");

    let entity_schema = db.entities_schema()?;

    for chunk in db.entity_chunk_index()? {
        let buf = db.read_file(format!("World/0/Entities/Chunks/{chunk}.mps"))?;
        let buf = &mut buf.as_slice();

        let entities = buf.read_brdb(&entity_schema, ENTITY_CHUNK_SOA)?;
        println!("entities: {}", entities.display(&entity_schema));

        let type_counters = entities.prop("TypeCounters")?.as_array()?;
        for counter in type_counters {
            let type_idx = counter.prop("TypeIndex")?.as_brdb_u32()?;
            let num_instances = counter.prop("NumEntities")?.as_brdb_u32()?;
            let type_name = global_data
                .entity_type_names
                .get_index(type_idx as usize)
                .cloned()
                .unwrap_or("illegal".to_string());
            let struct_name = lookup_entity_struct_name(&type_name)
                .unwrap_or("unknown")
                .to_string();

            println!(
                "Component type {type_name}/{struct_name} (index {type_idx}) has {num_instances} instances"
            );

            if struct_name == "None" {
                continue;
            }

            for _ in 0..num_instances {
                let component = buf.read_brdb(&entity_schema, &struct_name)?;
                println!("Component: {}", component.display(&entity_schema));
            }
        }
    }

    Ok(())
}

/// Builds a spawner prefab embedding another prefab, writes it to an
/// in-memory .brz, and reads everything back through the new accessors.
#[test]
fn spawner_prefab_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    use crate::{Brz, wrapper::BrickType};

    // Inner: single-brick prefab.
    let mut inner = World::new();
    inner.bricks.push(Brick::default());
    inner.make_prefab();
    inner.meta.bundle.name = "inner brick".to_string();
    let inner_bytes = inner.to_brz_vec()?;

    // Outer: a spawner-gate prefab embedding the inner prefab.
    let mut outer = World::new();
    outer.register_all_components();
    let prefab_path = outer.add_prefab(inner_bytes.clone());
    outer.bricks.push(
        Brick {
            asset: BrickType::str("B_1x1_Gate_Exec_PrefabSpawner"),
            position: (0, 0, 1).into(),
            ..Default::default()
        }
        .with_component(
            assets::LiteralComponent::new("BrickComponentType_WireGraph_Exec_PrefabSpawner")
                .with_data([(
                    "Prefab",
                    Box::new(prefab_path.clone())
                        as Box<dyn crate::schema::as_brdb::AsBrdbValue>,
                )]),
        ),
    );
    outer.make_prefab();
    outer.meta.thumbnail = Some(vec![9, 9, 9]);
    let outer_bytes = outer.to_brz_vec()?;

    // Read back.
    let reader = Brz::read_slice(&outer_bytes)?.into_reader();

    let bundle = reader.bundle_json()?;
    assert_eq!(bundle.level_type, "Prefab");
    assert!(reader.prefab_json()?.is_some());
    assert!(reader.world_json()?.is_none());
    assert_eq!(reader.thumbnail()?, Some(vec![9, 9, 9]));

    let meta = reader.world_meta()?;
    assert!(meta.prefab.is_some());
    assert_eq!(meta.thumbnail, Some(vec![9, 9, 9]));

    // Embedded prefab enumeration + content round trip.
    assert_eq!(reader.prefab_paths()?, vec![prefab_path.clone()]);
    let prefabs = reader.read_prefabs()?;
    assert_eq!(prefabs.get(&prefab_path).unwrap(), &inner_bytes);

    // The component's Prefab property points at the enumerated path.
    let (_soa, components) = reader.component_chunk_soa(1, (0, 0, 0).into())?;
    let rendered = components[0].to_string();
    assert!(rendered.contains(&prefab_path), "component: {rendered}");

    // Nested read: the embedded archive parses and carries the inner bundle.
    let inner_reader = reader.open_prefab(&prefab_path)?.into_reader();
    assert_eq!(inner_reader.bundle_json()?.name, "inner brick");

    // A world without prefabs enumerates empty.
    let mut plain = World::new();
    plain.bricks.push(Brick::default());
    let plain_reader = Brz::read_slice(&plain.to_brz_vec()?)?.into_reader();
    assert!(plain_reader.prefab_paths()?.is_empty());
    assert!(plain_reader.world_json()?.is_some());
    assert!(plain_reader.prefab_json()?.is_none());
    assert!(plain_reader.thumbnail()?.is_none());

    Ok(())
}

#[test]
fn unregistered_component_type_errors_cleanly() {
    // Building a world with a gate component but WITHOUT calling
    // register_all_components() must return a clear, actionable error
    // instead of panicking deep in the SoA builder.
    let mut world = World::new();
    let (brick, _) = Brick {
        position: (0, 0, 1).into(),
        asset: assets::components::LogicGate::BoolNot.brick(),
        ..Default::default()
    }
    .with_component(assets::components::LogicGate::BoolNot.component())
    .with_id_split();
    world.add_bricks([brick]);

    let result = world.to_unsaved();
    assert!(
        matches!(
            result,
            Err(BrError::World(
                crate::errors::BrdbWorldError::UnregisteredComponentType(_)
            ))
        ),
        "expected UnregisteredComponentType error"
    );
}
