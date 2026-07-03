use brdb::{BrFsReader, BrReader, Brdb, Brz, IntoReader};
use std::path::PathBuf;

fn run<T: BrFsReader>(db: &BrReader<T>) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Component types: {:?}",
        db.global_data()?.component_type_names
    );
    println!(
        "Component structs: {:?}",
        db.global_data()?.component_data_struct_names
    );
    // Probe grid ids 1.. until one is missing (covers the main grid plus any
    // microchip inner grids, which entity discovery may not surface).
    for gid in 1..32 {
        let chunks = match db.brick_chunk_index(gid) {
            Ok(c) => c,
            Err(_) => break,
        };
        println!("=== grid {gid} ===");
        for chunk in chunks {
            println!(
                "chunk {} bricks={} components={} wires={}",
                chunk.index, chunk.num_bricks, chunk.num_components, chunk.num_wires
            );
            if chunk.num_components > 0 {
                match db.component_chunk_soa(gid, chunk.index) {
                    Ok((_soa, components)) => {
                        for c in components {
                            println!("  component: {c}");
                        }
                    }
                    Err(e) => {
                        println!("  ERROR reading components: {e}");
                        return Err(e.into());
                    }
                }
            }
        }
    }
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
