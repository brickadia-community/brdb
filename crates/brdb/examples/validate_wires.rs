//! TEMP diagnostic: validate every wire endpoint in a .brz resolves to a
//! brick that actually carries the referenced component type, mimicking the
//! game loader's wire-port resolution. Usage:
//!   cargo run --example validate_wires -- path/to/world.brz [grid] [lo..hi]
use brdb::{Brdb, Brz, IntoReader, WireChunkSoA};
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args().nth(1).expect("usage: validate_wires <brz|brdb>"));
    if path.extension().is_some_and(|e| e == "brdb") {
        run(Brdb::open(&path)?.into_reader())
    } else {
        run(Brz::open(&path)?.into_reader())
    }
}

fn run<T: brdb::BrFsReader>(
    db: brdb::BrReader<T>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = db.global_data()?;

    // Collect all grid ids: main grid (1) + every brick-grid entity.
    let mut grid_ids = vec![1usize];
    for index in db.entity_chunk_index()? {
        for e in db.entity_chunk(index)? {
            if e.is_brick_grid() || e.is_microchip_grid() {
                if let Some(id) = e.id {
                    grid_ids.push(id);
                }
            }
        }
    }

    // Pass 1: per (grid, chunk) build brick component lists + brick counts.
    // brick_components[(gid, chunk)][brick_index] = Vec<component_type_index>
    let mut brick_counts: HashMap<(usize, String), usize> = HashMap::new();
    let mut brick_components: HashMap<(usize, String), HashMap<u32, Vec<u16>>> = HashMap::new();
    let mut brick_types: HashMap<(usize, String), Vec<u32>> = HashMap::new();
    for &gid in &grid_ids {
        let chunks = match db.brick_chunk_index(gid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for chunk in &chunks {
            let key = (gid, format!("{:?}", chunk.index));
            let soa = db.brick_chunk_soa(gid, chunk.index)?;
            brick_counts.insert(key.clone(), soa.brick_type_indices.len());
            brick_types.insert(key.clone(), soa.brick_type_indices.clone());
            let mut per_brick: HashMap<u32, Vec<u16>> = HashMap::new();
            if chunk.num_components > 0 {
                let (csoa, _components) = db.component_chunk_soa(gid, chunk.index)?;
                let type_indices: Vec<u16> = csoa
                    .component_type_counters
                    .iter()
                    .flat_map(|v| {
                        let index = v.type_index as u16;
                        (0..v.num_instances).map(move |_| index)
                    })
                    .collect();
                for (i, bi) in csoa.component_brick_indices.iter().enumerate() {
                    let brick_index = *bi;
                    per_brick.entry(brick_index).or_default().push(type_indices[i]);
                }
            }
            brick_components.insert(key, per_brick);
        }
    }

    let cname = |t: u16| -> String {
        data.component_type_names
            .get_index(t as usize)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("<type {t}>"))
    };
    let pname = |p: u16| -> String {
        data.component_wire_port_names
            .get_index(p as usize)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("<port {p}>"))
    };

    // Pass 2: validate wires.
    let mut total = 0u64;
    let mut bad = 0u64;
    for &gid in &grid_ids {
        let chunks = match db.brick_chunk_index(gid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for chunk in &chunks {
            if chunk.num_wires == 0 {
                continue;
            }
            let key = (gid, format!("{:?}", chunk.index));
            let soa = db.wire_chunk_soa(gid, chunk.index)?.to_value();
            let soa: WireChunkSoA = (&soa).try_into()?;

            let mut check = |ctx: &str,
                             ggid: usize,
                             ckey: &str,
                             brick_index: u32,
                             ct: u16,
                             port: u16| {
                total += 1;
                let k = (ggid, ckey.to_string());
                let n = brick_counts.get(&k).copied().unwrap_or(0);
                if brick_index as usize >= n {
                    bad += 1;
                    println!(
                        "BAD {ctx}: grid {ggid} chunk {ckey} brick {brick_index} OUT OF RANGE (chunk has {n} bricks); wanted {} {}",
                        cname(ct),
                        pname(port)
                    );
                    return;
                }
                let comps = brick_components
                    .get(&k)
                    .and_then(|m| m.get(&brick_index))
                    .cloned()
                    .unwrap_or_default();
                if !comps.contains(&ct) {
                    bad += 1;
                    println!(
                        "BAD {ctx}: grid {ggid} chunk {ckey} brick {brick_index} has components [{}] but wire wants {} {}",
                        comps.iter().map(|c| cname(*c)).collect::<Vec<_>>().join(", "),
                        cname(ct),
                        pname(port)
                    );
                }
            };

            for p in &soa.local_wire_sources {
                check("local-src", gid, &key.1, p.brick_index_in_chunk, p.component_type_index, p.port_index);
            }
            for p in &soa.local_wire_targets {
                check("local-tgt", gid, &key.1, p.brick_index_in_chunk, p.component_type_index, p.port_index);
            }
            for p in &soa.remote_wire_sources {
                let ckey = format!("{:?}", p.chunk_index);
                check(
                    "remote-src",
                    p.grid_persistent_index as usize,
                    &ckey,
                    p.brick_index_in_chunk,
                    p.component_type_index,
                    p.port_index,
                );
            }
            // Remote wires: the source names another grid; the target is
            // local to THIS chunk.
            for p in &soa.remote_wire_targets {
                check("remote-tgt", gid, &key.1, p.brick_index_in_chunk, p.component_type_index, p.port_index);
            }
        }
    }

    println!("validated {total} wire endpoints, {bad} bad");

    // Optional dump: grid + index range, e.g. `-- file 2 580 590`
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 5 {
        let gid: usize = args[2].parse()?;
        let lo: u32 = args[3].parse()?;
        let hi: u32 = args[4].parse()?;
        for ((g, ckey), types) in &brick_types {
            if *g != gid {
                continue;
            }
            for i in lo..=hi.min(types.len().saturating_sub(1) as u32) {
                let comps = brick_components
                    .get(&(gid, ckey.clone()))
                    .and_then(|m| m.get(&i))
                    .cloned()
                    .unwrap_or_default();
                let asset = data
                    .basic_brick_asset_names
                    .get_index(types[i as usize] as usize)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("<pb {}>", types[i as usize]));
                println!(
                    "grid {gid} chunk {ckey} brick {i}: {asset} [{}]",
                    comps.iter().map(|c| cname(*c)).collect::<Vec<_>>().join(", ")
                );
            }
        }
    }
    Ok(())
}
