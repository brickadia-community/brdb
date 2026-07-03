use brdb::{Brick, BrickSize, BrickType, World, assets};
use std::path::PathBuf;

/// Writes a single-brick prefab and dumps its Meta JSON for inspection.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("./example_prefab.brz");

    let mut world = World::new();
    // A single 1x1 plate-ish brick (half-extent 5,5,2) at the origin so the
    // bounds match the hand-captured reference prefab.
    world.bricks.push(Brick {
        asset: BrickType::Procedural {
            asset: assets::bricks::PB_DEFAULT_BRICK.into(),
            size: BrickSize { x: 5, y: 5, z: 2 },
        },
        position: (0, 0, 0).into(),
        ..Default::default()
    });

    world.make_prefab();
    println!(
        "Prefab.json:\n{}",
        serde_json::to_string_pretty(world.meta.prefab.as_ref().unwrap())?
    );
    println!(
        "Bundle.json:\n{}",
        serde_json::to_string_pretty(&world.meta.bundle)?
    );

    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    world.write_brz(&path)?;
    println!("wrote {}", path.display());
    Ok(())
}
