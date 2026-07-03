use brdb::{Brz, IntoReader};
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "world.brz".to_string()),
    );
    let db = Brz::open(path)?.into_reader();

    let mut grid_ids = vec![1];
    for index in db.entity_chunk_index()? {
        for e in db.entity_chunk(index)? {
            if e.is_brick_grid() || e.is_microchip_grid() {
                if let Some(id) = e.id {
                    grid_ids.push(id);
                }
            }
        }
    }

    // For each grid, collect a fingerprint: (num_bricks, num_wires, num_chunks)
    // to find grids that look identical.
    let mut fingerprints: HashMap<(u64, u64, usize), Vec<usize>> = HashMap::new();
    let mut total_bricks = 0u64;
    let mut total_wires = 0u64;
    let mut total_components = 0u64;

    for &gid in &grid_ids {
        let chunks = db.brick_chunk_index(gid)?;
        let mut grid_bricks = 0u64;
        let mut grid_wires = 0u64;
        let mut grid_components = 0u64;
        for chunk in &chunks {
            grid_bricks += chunk.num_bricks as u64;
            grid_wires += chunk.num_wires as u64;
            grid_components += chunk.num_components as u64;
        }
        total_bricks += grid_bricks;
        total_wires += grid_wires;
        total_components += grid_components;

        let fp = (grid_bricks, grid_wires, chunks.len());
        fingerprints.entry(fp).or_default().push(gid);
    }

    println!("=== Grid Summary ===");
    println!("total grids: {}", grid_ids.len());
    println!("total bricks: {total_bricks}");
    println!("total wires: {total_wires}");
    println!("total components: {total_components}");
    println!();

    println!("=== Unique Grid Shapes ===");
    let mut fps: Vec<_> = fingerprints.iter().collect();
    fps.sort_by_key(|((b, w, _), grids)| std::cmp::Reverse(*b * grids.len() as u64));

    for ((bricks, wires, chunks), grids) in &fps {
        let savings = if grids.len() > 1 {
            format!(
                " → {} could be deduplicated (save {} bricks, {} wires)",
                grids.len() - 1,
                bricks * (grids.len() as u64 - 1),
                wires * (grids.len() as u64 - 1)
            )
        } else {
            String::new()
        };
        println!(
            "  {}×  ({} bricks, {} wires, {} chunks){}",
            grids.len(),
            bricks,
            wires,
            chunks,
            savings
        );
    }

    // Top 10 largest grids
    println!();
    println!("=== Top 10 Largest Grids ===");
    let mut grid_sizes: Vec<(usize, u64, u64)> = Vec::new();
    for &gid in &grid_ids {
        let chunks = db.brick_chunk_index(gid)?;
        let b: u64 = chunks.iter().map(|c| c.num_bricks as u64).sum();
        let w: u64 = chunks.iter().map(|c| c.num_wires as u64).sum();
        grid_sizes.push((gid, b, w));
    }
    grid_sizes.sort_by_key(|(_, b, _)| std::cmp::Reverse(*b));
    for (gid, b, w) in grid_sizes.iter().take(10) {
        println!("  grid {gid}: {b} bricks, {w} wires");
    }

    Ok(())
}
