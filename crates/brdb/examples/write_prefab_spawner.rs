use brdb::{AsBrdbValue, BrFsReader, Brick, Brz, IntoReader, World, WirePort, assets};
use std::path::PathBuf;

/// Builds a world where pressing a button spawns a prefab.
///
/// An interactive button's `bHeld` output is wired into a prefab-spawner
/// gate's `Exec` input, so each press spawns a copy of an embedded
/// single-brick prefab. Demonstrates `World::add_prefab` (content-addressed
/// embedding) together with the wire API.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build the prefab that gets spawned: a single red brick.
    let mut prefab = World::new();
    prefab.meta.bundle.name = "Spawned Brick".to_string();
    prefab.bricks.push(Brick {
        position: (0, 0, 6).into(),
        color: (255, 0, 0).into(),
        ..Default::default()
    });
    prefab.make_prefab();
    let prefab_bytes = prefab.to_brz_vec()?;

    // 2. Build the outer world holding the button + spawner.
    let mut world = World::new();
    // Registers the built-in component/port tables so the Button and
    // PrefabSpawner component types and their wire ports resolve.
    world.register_all_components();
    world.meta.bundle.description = "Button-triggered prefab spawner".to_string();

    // Embed the prefab; the returned path is what the spawner references.
    let prefab_path = world.add_prefab(prefab_bytes);

    // A pressable button (Component_Button on a 1x1 flat round brick). The
    // crate exposes brick assets as constants; components it doesn't model as
    // typed gates (like the button and spawner) are built from string names
    // via LiteralComponent.
    let (button, button_id) = Brick {
        position: (0, 0, 2).into(),
        color: (0, 255, 0).into(),
        asset: assets::bricks::B_1X1F_ROUND,
        ..Default::default()
    }
    .with_component(assets::LiteralComponent::new("Component_Button").with_data([(
        "PromptCustomLabel",
        Box::new("Spawn Brick".to_string()) as Box<dyn AsBrdbValue>,
    )]))
    .with_id_split();

    // The prefab-spawner gate, pointed at the embedded prefab.
    let (spawner, spawner_id) = Brick {
        position: (15, 0, 1).into(),
        color: (0, 0, 255).into(),
        asset: assets::bricks::B_1X1_GATE_EXEC_PREFAB_SPAWNER,
        ..Default::default()
    }
    .with_component(
        assets::LiteralComponent::new("BrickComponentType_WireGraph_Exec_PrefabSpawner").with_data(
            [(
                "Prefab",
                Box::new(prefab_path.clone()) as Box<dyn AsBrdbValue>,
            )],
        ),
    )
    .with_id_split();

    world.add_bricks([button, spawner]);

    // Wire the button's held signal into the spawner's Exec input, so a
    // press fires the spawn.
    world.add_wire_connection(
        WirePort::new(button_id, "Component_Button", "bHeld"),
        WirePort::new(
            spawner_id,
            "BrickComponentType_WireGraph_Exec_PrefabSpawner",
            "Exec",
        ),
    );

    // 3. Write it out.
    let path = PathBuf::from("./example_prefab_spawner.brz");
    world.write_brz(&path)?;
    println!("wrote {}", path.display());

    // Read it back to confirm the embedded prefab and wire survived.
    let reader = Brz::open(&path)?.into_reader();
    println!("embedded prefab: {}", prefab_path);
    println!("prefab paths in archive: {:?}", reader.prefab_paths()?);
    println!("{}", reader.get_fs()?.render());

    Ok(())
}
