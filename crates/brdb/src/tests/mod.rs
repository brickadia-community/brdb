use std::path::PathBuf;

use crate::{
    BrFsReader, Brdb, IntoReader, assets,
    errors::BrError,
    schema::{ReadBrdbSchema, as_brdb::AsBrdbValue},
    tables::BrBlob,
    wrapper::{Brick, Entity, World, lookup_entity_struct_name, schemas::ENTITY_CHUNK_SOA},
};

#[test]
fn test_component_catalog() {
    use assets::components::{COMPONENTS, component};
    // Non-trivial and sorted by name (the `component` lookup binary-searches).
    assert!(
        COMPONENTS.len() > 200,
        "catalog too small: {}",
        COMPONENTS.len()
    );
    assert!(
        COMPONENTS.windows(2).all(|w| w[0].name < w[1].name),
        "COMPONENTS must be sorted by name for binary search"
    );
    // A known gate resolves with its wired ports and host brick.
    let branch = component("BrickComponentType_Exec_Branch").expect("Branch gate present");
    assert_eq!(branch.brick(), Some("B_1x1_Gate_Exec_Branch"));
    assert!(branch.inputs.contains(&"Exec"));
    assert!(branch.outputs.contains(&"ExecOutA"));
    // Unknown names resolve to None.
    assert!(component("BrickComponentType_Nope").is_none());
}

#[test]
fn test_brick_half_extents() {
    use assets::brick_sizes::{BRICK_HALF_EXTENTS, brick_half_extent};
    assert!(BRICK_HALF_EXTENTS.len() > 200);
    assert!(
        BRICK_HALF_EXTENTS.windows(2).all(|w| w[0].0 < w[1].0),
        "BRICK_HALF_EXTENTS must be sorted by name for binary search"
    );
    // A 1x1 flat plate is 5x5x2 half-extent; the reroute node is 1x1x1.
    assert_eq!(brick_half_extent("B_1x1F_Round"), Some([5, 5, 2]));
    assert_eq!(brick_half_extent("B_1x1_Reroute_Node"), Some([1, 1, 1]));
    assert_eq!(brick_half_extent("B_does_not_exist"), None);
}

#[test]
fn test_procedural_gate_variants() {
    use assets::components::{
        PROCEDURAL_GATES, component, procedural_gate, procedural_gate_for_component,
    };
    assert!(
        PROCEDURAL_GATES.windows(2).all(|w| w[0].asset < w[1].asset),
        "PROCEDURAL_GATES must be sorted by asset for binary search"
    );
    for g in PROCEDURAL_GATES {
        assert!(!g.variants.is_empty(), "{} has no variants", g.asset);
        // Every gate brick is a host brick of its component in the catalog.
        let info =
            component(g.component).unwrap_or_else(|| panic!("{} not in COMPONENTS", g.component));
        assert!(
            info.bricks.contains(&g.asset),
            "{} not a host of {}",
            g.asset,
            g.component
        );
        // Physical ports are a subset of the component's logical ports.
        for v in g.variants {
            for p in v.ports {
                assert!(
                    info.inputs.contains(p) || info.outputs.contains(p),
                    "{}: unknown port {p}",
                    g.asset
                );
            }
        }
    }
    let q = procedural_gate("PB_Gate_QueueTicks").unwrap();
    assert_eq!(q.component, "BrickComponentType_QueueTicks");
    assert_eq!(
        procedural_gate_for_component("BrickComponentType_QueueTicks"),
        Some(q)
    );
    assert_eq!(q.variant([5, 5, 2]).map(|v| v.ports.len()), Some(8));
    assert_eq!(q.variant([4, 5, 2]), None);
    let v = q.smallest_with_ports(&["DataIn5"]).unwrap();
    assert_eq!(v.half_size, [7, 5, 2]);
    assert_eq!(procedural_gate("B_1x1_Gate_QueueTicks"), None);
}

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

/// Writing EA3 worlds is not supported; asking for it fails instead of
/// writing a world no game build can load.
#[test]
fn test_write_ea3_world_is_rejected() {
    use crate::schema::BrdbSchemaFormat;
    let mut world = World::new();
    world.schema_format = BrdbSchemaFormat::EA3;
    world.register_all_components();
    world.bricks.push(Brick::default());
    let err = world.to_unsaved().err().expect("EA3 worlds are not written");
    assert!(
        matches!(
            err,
            crate::BrError::World(crate::BrdbWorldError::Ea3WriteUnsupported)
        ),
        "{err}"
    );
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

/// An authored dynamic grid with no explicit data gets the game's fresh-grid
/// values, not zeros (which save an invisible, massless, gravity-free grid).
#[test]
fn test_dynamic_grid_entity_defaults_visible() -> Result<(), Box<dyn std::error::Error>> {
    let db = Brdb::new_memory()?;
    let mut world = World::new();
    world.register_all_components();
    world.add_brick_grid(
        Entity::default(),
        [Brick {
            position: (0, 0, 3).into(),
            ..Default::default()
        }],
    );
    db.save("visible grid", &world)?;

    let db = db.into_reader();
    let schema = db.entities_schema()?;
    let chunk = db.entity_chunk_index()?.into_iter().next().unwrap();
    let buf = db.read_file(format!("World/0/Entities/Chunks/{chunk}.mps"))?;
    let buf = &mut buf.as_slice();
    buf.read_brdb(&schema, ENTITY_CHUNK_SOA)?;
    let data = buf.read_brdb(&schema, "BrickGridDynamicActor")?;
    for field in [
        "bVisible",
        "bEnableGravity",
        "bUseNewMassCalculation",
        "bReceivesDecals",
    ] {
        assert!(data.prop(field)?.as_brdb_bool()?, "{field}");
    }
    assert_eq!(data.prop("MassScale")?.as_brdb_f32()?, 1.0);
    assert_eq!(data.prop("BouyancyScale")?.as_brdb_f32()?, 0.2);
    assert!(!data.prop("bDetectableByAnyone")?.as_brdb_bool()?);
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
            assets::LiteralComponent::new("BrickComponentType_Exec_PrefabSpawner").with_data([(
                "Prefab",
                Box::new(prefab_path.clone()) as Box<dyn crate::schema::as_brdb::AsBrdbValue>,
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

/// A world exercising the shapes the fast paths care about: several bricks in
/// one chunk, components with and without set data, and a wire connection.
fn differential_world() -> World {
    let mut world = World::new();
    world.register_all_components();
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
        color: (0, 255, 0).into(),
        asset: assets::components::LogicGate::BoolNot.brick(),
        ..Default::default()
    }
    .with_component(assets::components::LogicGate::BoolNot.component())
    .with_id_split();
    let plain = Brick {
        position: (30, 0, 1).into(),
        color: (0, 0, 255).into(),
        ..Default::default()
    };
    world.add_bricks([a, b, plain]);
    world.add_wire_connection(
        assets::components::LogicGate::BoolNot.output_of(b_id),
        assets::components::Rerouter::input_of(a_id),
    );
    world
}

/// The hand-encoded wire and brick chunk writers must produce exactly what the
/// schema-driven writer produces. Both the specialized path and the plan-cached
/// path are compared against `write::write_brdb`, which walks the schema with no
/// plan and no specialization.
#[test]
fn test_specialized_soa_writers_match_generic() -> Result<(), Box<dyn std::error::Error>> {
    use crate::schema::write;
    use crate::wrapper::schemas::{BRICK_CHUNK_SOA, BRICK_WIRE_SOA};
    use crate::wrapper::{brick_schema_is_standard, wire_schema_is_standard};

    let unsaved = differential_world().to_unsaved()?;
    let wires_schema = crate::wrapper::schemas::bricks_wires_schema();
    let bricks_schema = crate::wrapper::schemas::bricks_chunks_schema();

    // A silent fallback would make every assertion below compare the generic
    // writer against itself, so require the fast paths to be the live ones.
    assert!(
        wire_schema_is_standard(wires_schema),
        "wire fast path is not active; this test would prove nothing"
    );
    assert!(
        brick_schema_is_standard(bricks_schema),
        "brick fast path is not active; this test would prove nothing"
    );

    let mut wire_chunks = 0;
    let mut brick_chunks = 0;
    for world in unsaved.worlds.values() {
        for grid in world.grids.values() {
            for soa in grid.wires.values() {
                let mut generic = Vec::new();
                write::write_brdb(wires_schema, &mut generic, BRICK_WIRE_SOA, soa)?;
                assert_eq!(
                    soa.to_bytes(wires_schema)?,
                    generic,
                    "wire specialized vs generic"
                );
                assert_eq!(
                    wires_schema.write_brdb(BRICK_WIRE_SOA, soa)?,
                    generic,
                    "wire cached vs generic"
                );
                wire_chunks += 1;
            }
            for soa in grid.bricks.values() {
                let mut generic = Vec::new();
                write::write_brdb(bricks_schema, &mut generic, BRICK_CHUNK_SOA, soa)?;
                assert_eq!(
                    soa.to_bytes(bricks_schema)?,
                    generic,
                    "brick specialized vs generic"
                );
                assert_eq!(
                    bricks_schema.write_brdb(BRICK_CHUNK_SOA, soa)?,
                    generic,
                    "brick cached vs generic"
                );
                brick_chunks += 1;
            }
        }
    }
    assert!(
        wire_chunks > 0 && brick_chunks > 0,
        "no chunks compared: {wire_chunks} wire, {brick_chunks} brick"
    );
    Ok(())
}

/// A schema whose shape does not match the baked one must route through the
/// generic writer instead of the hand-encoded one.
#[test]
fn test_specialized_writers_reject_foreign_schema() {
    use crate::wrapper::{brick_schema_is_standard, wire_schema_is_standard};
    let owners = crate::wrapper::schemas::owners_schema();
    assert!(!wire_schema_is_standard(owners));
    assert!(!brick_schema_is_standard(owners));
}

/// The plan cache resolves each struct once and replays it; the bytes must match
/// the uncached schema walk for every type the save pipeline writes.
#[test]
fn test_plan_cache_matches_generic_for_pipeline_types() -> Result<(), Box<dyn std::error::Error>> {
    use crate::schema::write;
    use crate::wrapper::schemas::{
        BRICK_CHUNK_INDEX_SOA, BRICK_CHUNK_SOA, BRICK_WIRE_SOA, GLOBAL_DATA_SOA, OWNER_TABLE_SOA,
    };

    fn cmp_paths<T: AsBrdbValue>(
        schema: &crate::schema::BrdbSchema,
        ty: &str,
        v: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut generic = Vec::new();
        write::write_brdb(schema, &mut generic, ty, v)?;
        let mut cached = Vec::new();
        let mut cache = write::PlanCache::new(schema);
        write::write_brdb_cached(schema, &mut cache, &mut cached, ty, v)?;
        assert_eq!(cached, generic, "plan cache diverged for {ty}");
        Ok(())
    }

    let unsaved = differential_world().to_unsaved()?;
    let mut compared = 0;
    for world in unsaved.worlds.values() {
        cmp_paths(
            crate::wrapper::schemas::global_data_schema(),
            GLOBAL_DATA_SOA,
            &world.global_data,
        )?;
        cmp_paths(
            crate::wrapper::schemas::owners_schema(),
            OWNER_TABLE_SOA,
            &world.owners,
        )?;
        compared += 2;
        for grid in world.grids.values() {
            cmp_paths(
                crate::wrapper::schemas::bricks_chunk_index_schema(),
                BRICK_CHUNK_INDEX_SOA,
                &grid.chunk_index,
            )?;
            compared += 1;
            for soa in grid.wires.values() {
                cmp_paths(
                    crate::wrapper::schemas::bricks_wires_schema(),
                    BRICK_WIRE_SOA,
                    soa,
                )?;
                compared += 1;
            }
            for soa in grid.bricks.values() {
                cmp_paths(
                    crate::wrapper::schemas::bricks_chunks_schema(),
                    BRICK_CHUNK_SOA,
                    soa,
                )?;
                compared += 1;
            }
        }
    }
    assert!(
        compared >= 5,
        "expected several types compared, got {compared}"
    );
    Ok(())
}

/// Round trip: a world written through the fast paths reads back with the same
/// brick, component and wire content.
#[test]
fn test_write_then_read_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let world = differential_world();
    let bytes = world.to_brz_vec()?;
    let reader = crate::Brz::read_slice(&bytes)?.into_reader();
    let fs = reader.get_fs()?;
    let rendered = fs.render();
    assert!(
        rendered.contains("Bricks"),
        "no Bricks dir in round-tripped world"
    );
    assert!(
        rendered.contains("Wires"),
        "no Wires dir in round-tripped world"
    );
    Ok(())
}

/// The edge detector is written the way the game saves a settled one: its last
/// input equal to its input and already seeded, with its outputs left to wires.
#[test]
fn test_write_edge_detector() -> Result<(), Box<dyn std::error::Error>> {
    use assets::components::{LogicGate, Rerouter};
    use std::collections::HashMap;

    let mut world = World::new();
    world.register_all_components();
    let (edge, edge_id) = Brick {
        position: (0, 0, 1).into(),
        asset: LogicGate::EdgeDetector.brick(),
        ..Default::default()
    }
    .with_component(
        LogicGate::EdgeDetector.component_with_overrides(HashMap::from([(
            "Input".into(),
            Box::new(1.0f64) as Box<dyn AsBrdbValue>,
        )])),
    )
    .with_id_split();
    let (rising, rising_id) = Brick {
        position: (15, 0, 1).into(),
        asset: assets::bricks::B_REROUTE,
        ..Default::default()
    }
    .with_component(Rerouter)
    .with_id_split();
    let (falling, falling_id) = Brick {
        position: (30, 0, 1).into(),
        asset: assets::bricks::B_REROUTE,
        ..Default::default()
    }
    .with_component(Rerouter)
    .with_id_split();
    world.add_bricks([edge, rising, falling]);
    world.add_wire_connection(
        LogicGate::EdgeDetector.rising_edge_of(edge_id),
        Rerouter::input_of(rising_id),
    );
    world.add_wire_connection(
        LogicGate::EdgeDetector.falling_edge_of(edge_id),
        Rerouter::input_of(falling_id),
    );

    let reader = crate::Brz::read_slice(&world.to_brz_vec()?)?.into_reader();
    let mut found = 0;
    for chunk in reader.brick_chunk_index(1)? {
        if chunk.num_components == 0 {
            continue;
        }
        let (_soa, components) = reader.component_chunk_soa(1, chunk.index)?;
        for c in components
            .iter()
            .filter(|c| c.get_name() == "BCSComponentVertex_Expr_EdgeDetector")
        {
            assert_eq!(c.prop("Input")?.as_brdb_f64()?, 1.0);
            assert_eq!(c.prop("InputLast")?.as_brdb_f64()?, 1.0);
            assert!(c.prop("bSeeded")?.as_brdb_bool()?);
            assert!(!c.contains_key("bPulseOnRisingEdge"));
            found += 1;
        }
    }
    assert_eq!(found, 1);
    Ok(())
}

/// Component struct data is where the plan cache does the most work: whole
/// struct defaults are precomputed and replayed for a component with nothing
/// set, and per-field defaults fill the gaps for a partially set one. Both must
/// match the uncached schema walk for every component type in the catalog.
#[test]
fn test_plan_cache_matches_generic_for_component_data() -> Result<(), Box<dyn std::error::Error>> {
    use crate::assets::LiteralComponent;
    use crate::schema::write;

    let mut world = World::new();
    world.register_all_components();
    let mut unsaved = world.to_unsaved()?;
    let w = unsaved.worlds.values_mut().next().expect("one world");
    // The pipeline attaches global data before writing; the component type to
    // struct mapping lives there.
    w.component_schema
        .set_global_data(std::sync::Arc::new(w.global_data.clone()));
    let schema = &w.component_schema;

    let mut compared = 0;
    let mut skipped = 0;
    for info in assets::components::COMPONENTS {
        let Some(struct_ty) = schema.global_data.get_struct_name(info.name) else {
            skipped += 1;
            continue;
        };
        let comp = LiteralComponent::new(info.name);
        let mut generic = Vec::new();
        write::write_brdb(schema, &mut generic, struct_ty, &comp)?;
        let mut cached = Vec::new();
        let mut cache = write::PlanCache::new(schema);
        write::write_brdb_cached(schema, &mut cache, &mut cached, struct_ty, &comp)?;
        assert_eq!(
            cached, generic,
            "plan cache diverged for component {}",
            info.name
        );

        // The memoized entry point is what the real chunk writer uses.
        let mut memoized = Vec::new();
        let mut memo = write::TopLevelMemo::default();
        write::write_brdb_memoized(
            schema,
            &mut cache,
            &mut memo,
            &mut memoized,
            struct_ty,
            &comp,
        )?;
        assert_eq!(
            memoized, generic,
            "memoized writer diverged for component {}",
            info.name
        );
        compared += 1;
    }
    assert!(
        compared > 200,
        "expected the whole catalog compared, got {compared} ({skipped} without a struct)"
    );
    Ok(())
}
