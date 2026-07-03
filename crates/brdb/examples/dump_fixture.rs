//! Scratch dump tool for inspecting the new brs-js fixtures (wires,
//! components, entities). Mirrors examples/read_brz.rs (global data, brick
//! chunk SoA, component chunk SoA + per-instance component list, wire chunk
//! SoA), generalized to accept any grid id and either a .brz or .brdb path,
//! plus (for entities) the entity chunk index/SoA dump from
//! examples/read_entities.rs. Not part of the committed example set — purely
//! a debugging aid for writing the report's ground-truth dumps.
use brdb::{BrFsReader, Brdb, BrReader, Brz, IntoReader, WireChunkSoA};
use std::path::PathBuf;

fn run<T: BrFsReader>(db: &BrReader<T>) -> Result<(), Box<dyn std::error::Error>> {
    let data = db.global_data()?;
    println!("Basic Brick assets: {:?}", data.basic_brick_asset_names);
    println!("Procedural Brick assets: {:?}", data.procedural_brick_asset_names);
    // register_all_components() embeds the FULL catalog (hundreds of wire
    // ports/component types), so only print counts for those — the fixture's
    // OWN types/ports are visible per-brick below via the type-name lookups.
    println!(
        "Wire port count: {} (registered catalog)",
        data.component_wire_port_names.len()
    );
    println!(
        "Component type count: {} (registered catalog)",
        data.component_type_names.len()
    );
    println!("Entity types: {:?}", data.entity_type_names);
    println!("Entity classes: {:?}", data.entity_data_class_names);

    // Probe grid ids 1.. until one is missing (grid 1 is always the main
    // grid; higher ids are sub-grids / microchip inner grids).
    for gid in 1..32 {
        let chunks = match db.brick_chunk_index(gid) {
            Ok(c) => c,
            Err(_) => break,
        };
        println!("=== grid {gid} ===");
        println!("Brick chunks: {chunks:?}");
        for chunk in &chunks {
            let soa = db.brick_chunk_soa(gid, chunk.index)?;
            println!("Brick soa: {soa:?}");
            let asset_names: Vec<String> = soa
                .brick_type_indices
                .iter()
                .map(|&t| {
                    if (t as usize) < soa.procedural_brick_starting_index as usize {
                        data.basic_brick_asset_names
                            .get_index(t as usize)
                            .cloned()
                            .unwrap_or_default()
                    } else {
                        "<procedural>".to_string()
                    }
                })
                .collect();
            println!("Brick asset names (by index in chunk): {asset_names:?}");

            if chunk.num_components > 0 {
                let (soa, components) = db.component_chunk_soa(gid, chunk.index)?;
                let type_names: Vec<String> = soa
                    .component_type_counters
                    .iter()
                    .flat_map(|c| {
                        let name = data
                            .component_type_names
                            .get_index(c.type_index as usize)
                            .cloned()
                            .unwrap_or_default();
                        (0..c.num_instances).map(move |_| name.clone())
                    })
                    .collect();
                println!(
                    "Component chunk soa: component_brick_indices={:?} microchip_brick_indices={:?} microchip_brick_grid_references={:?}",
                    soa.component_brick_indices,
                    soa.microchip_brick_indices,
                    soa.microchip_brick_grid_references
                );
                println!("Component type names (parallel to ComponentBrickIndices): {type_names:?}");
                for c in components {
                    println!("Component: {c}");
                }
            }
            if chunk.num_wires > 0 {
                let raw = db.wire_chunk_soa(gid, chunk.index)?;
                println!("Wire chunk soa (raw struct): {raw}");
                let value = raw.to_value();
                let soa: WireChunkSoA = (&value).try_into()?;
                let port_name = |i: u16| {
                    data.component_wire_port_names
                        .get_index(i as usize)
                        .cloned()
                        .unwrap_or_default()
                };
                let type_name = |i: u16| {
                    data.component_type_names
                        .get_index(i as usize)
                        .cloned()
                        .unwrap_or_default()
                };
                for s in &soa.local_wire_sources {
                    println!(
                        "  local source: brick_in_chunk={} type={} port={}",
                        s.brick_index_in_chunk,
                        type_name(s.component_type_index),
                        port_name(s.port_index)
                    );
                }
                for t in &soa.local_wire_targets {
                    println!(
                        "  local target: brick_in_chunk={} type={} port={}",
                        t.brick_index_in_chunk,
                        type_name(t.component_type_index),
                        port_name(t.port_index)
                    );
                }
                for s in &soa.remote_wire_sources {
                    println!(
                        "  remote source: grid_persistent_index={} chunk={} brick_in_chunk={} type={} port={}",
                        s.grid_persistent_index,
                        s.chunk_index,
                        s.brick_index_in_chunk,
                        type_name(s.component_type_index),
                        port_name(s.port_index)
                    );
                }
                for t in &soa.remote_wire_targets {
                    println!(
                        "  remote target: brick_in_chunk={} type={} port={}",
                        t.brick_index_in_chunk,
                        type_name(t.component_type_index),
                        port_name(t.port_index)
                    );
                }
            }
        }
    }

    // Entity chunks (present whenever the world has any grids/entities).
    let entity_chunk_indices = db.entity_chunk_index()?;
    println!("=== entities ===");
    println!("Entity chunk indices: {entity_chunk_indices:?}");
    for chunk_index in entity_chunk_indices {
        let (soa, entity_data) = db.entity_chunk_soa(chunk_index)?;
        println!("--- Chunk {chunk_index} (SoA) ---");
        println!("Type counters: {:?}", soa.type_counters);
        let type_names: Vec<String> = soa
            .type_counters
            .iter()
            .flat_map(|c| {
                let name = data
                    .entity_type_names
                    .get_index(c.type_index as usize)
                    .cloned()
                    .unwrap_or_default();
                (0..c.num_entities).map(move |_| name.clone())
            })
            .collect();
        println!("Entity type names (parallel to PersistentIndices): {type_names:?}");
        println!("Persistent indices: {:?}", soa.persistent_indices);
        println!("Locations: {:?}", soa.locations);
        println!("Rotations: {:?}", soa.rotations);
        println!("Physics locked (frozen): {:?}", soa.physics_locked_flags);
        println!("Physics sleeping: {:?}", soa.physics_sleeping_flags);
        for (i, data) in entity_data.iter().enumerate() {
            match data {
                Some(struct_data) => println!("  Entity {i} struct: {struct_data}"),
                None => println!("  Entity {i}: None"),
            }
        }
    }

    println!("Files: {}", db.get_fs()?.render());

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).unwrap());
    if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        run(&Brdb::open(&p)?.into_reader())
    } else {
        run(&Brz::open(&p)?.into_reader())
    }
}
