use brdb::{BrFsReader, Brdb, Brick, IntoReader, World, assets};
use std::path::PathBuf;

/// Writes a world with two bricks and a wire to example_wire
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("./example_wire.brdb");

    // Ensures the memory db can be created without errors
    let db = Brdb::new(&path)?.into_reader();
    let mut world = World::new();
    // Register the built-in component type/struct mappings so the gate and
    // rerouter component types resolve when writing.
    world.register_all_components();
    world.meta.bundle.description = "Example World".to_string();

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

    db.save("example world", &world)?;

    println!("{}", db.get_fs()?.render());

    Ok(())
}
