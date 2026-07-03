use brdb::{Brz, IntoReader};
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

    let mut total_wires = 0u64;
    let mut total_bricks = 0u64;
    for &gid in &grid_ids {
        let chunks = db.brick_chunk_index(gid)?;
        for chunk in &chunks {
            total_bricks += chunk.num_bricks as u64;
            total_wires += chunk.num_wires as u64;
        }
    }

    println!("grids: {}", grid_ids.len());
    println!("bricks: {total_bricks}");
    println!("wires: {total_wires}");
    Ok(())
}
