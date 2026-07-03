//! Deterministic fixture generator for the brs-js cross-language test suite.
//! Writes into crates/brdb/fixtures/: six worlds as raw + zstd-14 .brz,
//! hashes.json (per-path BLAKE3 of decompressed payloads), and the nine
//! embedded schemas serialized to binary msgpack.
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

use brdb::fs::BrFs;
use brdb::schema::WireVariant;
use brdb::{
    assets, schemas, AsBrdbValue, BrFsReader, Brick, BrickSize, BrickType, Brz,
    Collision, Direction, Entity, Guid, IntoReader, Owner, Rotation, SavedBrickColor, World,
};

const CAKE_UUID: &str = "a1b2c3d4-e5f6-4789-8abc-def012345678";
const BOB_UUID: &str = "00112233-4455-6677-8899-aabbccddeeff";

fn brick_world() -> World {
    // Mirror of examples/write_brz.rs — the historical example_brick world.
    let mut world = World::new();
    world.meta.bundle.description = "Example World".to_string();
    world.bricks.push(Brick {
        position: (0, 0, 6).into(),
        color: (255, 0, 0).into(),
        ..Default::default()
    });
    world
}

fn features_world() -> World {
    // Single chunk (all coords in [0, 2048)): registries, owners, procedural
    // size run-length grouping (new/extend/reuse), a basic asset, collision
    // and visibility variants, orientations, material intensity.
    let mut world = World::new();
    world.meta.bundle.description = "Feature fixture".to_string();

    let cake = Guid::from_uuid(uuid::Uuid::parse_str(CAKE_UUID).unwrap());
    let bob = Guid::from_uuid(uuid::Uuid::parse_str(BOB_UUID).unwrap());
    world.owners.insert(cake, Owner {
        user_id: cake,
        user_name: "cake".to_string(),
        display_name: "Cake".to_string(),
    });
    world.owners.insert(bob, Owner {
        user_id: bob,
        user_name: "bob".to_string(),
        display_name: "Bob".to_string(),
    });

    let tile = |size: (u16, u16, u16)| BrickType::Procedural {
        asset: assets::bricks::PB_DEFAULT_TILE, // "PB_DefaultTile"
        size: BrickSize { x: size.0, y: size.1, z: size.2 },
    };

    // 1: default brick (PB_DefaultBrick 5x5x6, plastic, intensity 5) — new size slot
    world.bricks.push(Brick {
        position: (0, 0, 6).into(),
        color: (255, 0, 0).into(),
        owner_index: Some(1),
        ..Default::default()
    });
    // 2: tile 10x10x2, metallic, intensity 7, XPositive/Deg90 — new counter entry
    world.bricks.push(Brick {
        asset: tile((10, 10, 2)),
        position: (20, 0, 2).into(),
        color: (0, 255, 0).into(),
        owner_index: Some(1),
        material: assets::materials::METALLIC, // "BMC_Metallic"
        material_intensity: 7,
        direction: Direction::XPositive,
        rotation: Rotation::Deg90,
        ..Default::default()
    });
    // 3: same tile size — size_index_map reuse
    world.bricks.push(Brick {
        asset: tile((10, 10, 2)),
        position: (40, 0, 2).into(),
        color: (0, 0, 255).into(),
        owner_index: Some(2),
        ..Default::default()
    });
    // 4: tile 20x20x2 — extends the tail counter (same asset, new size)
    world.bricks.push(Brick {
        asset: tile((20, 20, 2)),
        position: (80, 0, 2).into(),
        color: (255, 255, 0).into(),
        owner_index: Some(2),
        ..Default::default()
    });
    // 5: default brick again — reuses slot from brick 1 (map hit after other asset)
    world.bricks.push(Brick {
        position: (100, 0, 6).into(),
        color: (255, 255, 255).into(),
        owner_index: Some(1),
        ..Default::default()
    });
    // 6: BASIC asset, PUBLIC owner, glow, partial collision, YNegative/Deg180
    world.bricks.push(Brick {
        asset: assets::bricks::B_2X2_OVERHANG, // "B_2x2_Overhang"
        position: (200, 0, 10).into(),
        color: (128, 64, 32).into(),
        material: assets::materials::GLOW, // "BMC_Glow"
        material_intensity: 3,
        direction: Direction::YNegative,
        rotation: Rotation::Deg180,
        collision: Collision { player: false, ..Default::default() },
        ..Default::default()
    });
    // 7: invisible, all-collision-off, tiny proc brick
    world.bricks.push(Brick {
        asset: BrickType::Procedural {
            asset: assets::bricks::PB_DEFAULT_BRICK,
            size: BrickSize { x: 2, y: 2, z: 2 },
        },
        position: (300, 0, 2).into(),
        color: (10, 20, 30).into(),
        owner_index: Some(1),
        visible: false,
        collision: Collision {
            player: false,
            weapon: false,
            interact: false,
            physics: false,
            ..Default::default()
        },
        ..Default::default()
    });
    world
}

fn chunks_world() -> World {
    // Multi-chunk: euclidean chunking with negative coords, plus chunks
    // 0_0_0 and 1_0_0 carry byte-identical SoA payloads → blob dedup.
    // NOTE: multi-chunk Rust output has nondeterministic FILE ORDER
    // (HashMap iteration), so this fixture is hashes-gate only — never
    // byte-compare its container.
    let mut world = World::new();
    world.meta.bundle.description = "Chunk fixture".to_string();
    for (pos, color) in [
        ((0, 0, 0), (1, 2, 3)),
        ((-1, -1, -1), (4, 5, 6)),
        ((2048, 0, 0), (1, 2, 3)),      // same rel pos + color as brick 1
        ((-2048, 4096, 10), (10, 11, 12)),
        ((500, 500, 500), (42, 42, 42)),
        ((2548, 500, 500), (42, 42, 42)), // same rel pos + color as brick 5
    ] {
        world.bricks.push(Brick {
            position: pos.into(),
            color: color.into(),
            ..Default::default()
        });
    }
    world
}

fn wires_world() -> World {
    // Mirror of examples/write_wire.rs (single grid, single chunk — fully
    // deterministic), extended to 3 bricks/2 wires: a boolean NOT gate feeds
    // one input of an AND gate, whose output feeds a rerouter.
    let mut world = World::new();
    world.register_all_components();
    world.meta.bundle.description = "Wire fixture".to_string();

    let (a, a_id) = Brick {
        position: (30, 0, 1).into(),
        color: (255, 0, 0).into(),
        asset: assets::bricks::B_REROUTE,
        ..Default::default()
    }
    .with_component(assets::components::Rerouter)
    .with_id_split();
    let (b, b_id) = Brick {
        position: (15, 0, 1).into(),
        color: (0, 255, 0).into(),
        asset: assets::components::LogicGate::BoolAnd.brick(),
        ..Default::default()
    }
    .with_component(assets::components::LogicGate::BoolAnd.component())
    .with_id_split();
    let (c, c_id) = Brick {
        position: (0, 0, 1).into(),
        color: (0, 0, 255).into(),
        asset: assets::components::LogicGate::BoolNot.brick(),
        ..Default::default()
    }
    .with_component(assets::components::LogicGate::BoolNot.component())
    .with_id_split();

    world.add_bricks([a, b, c]);
    // Wire 1: NOT.output -> AND.inputA
    world.add_wire_connection(
        assets::components::LogicGate::BoolNot.output_of(c_id),
        assets::components::LogicGate::BoolAnd.input_a_of(b_id),
    );
    // Wire 2: AND.output -> Rerouter.input
    world.add_wire_connection(
        assets::components::LogicGate::BoolAnd.output_of(b_id),
        assets::components::Rerouter::input_of(a_id),
    );

    world
}

fn components_world() -> World {
    // Single grid, single chunk (all positions < 2048) — fully deterministic.
    // register_all_components() embeds the full component catalog so every
    // type below (light/interact/wiregraph-pseudo/expr) resolves; each
    // component below carries non-default property values, and #4/#5 exercise
    // a WireGraphVariant-typed property (BufferTicks' Input/Output, and the
    // Constant gate's Value).
    let mut world = World::new();
    world.register_all_components();
    world.meta.bundle.description = "Component fixture".to_string();

    // 1: Point light — non-default brightness/radius/color, decoupled from
    // brick color (bUseBrickColor: false).
    world.bricks.push(
        Brick {
            position: (0, 0, 6).into(),
            color: (255, 255, 255).into(),
            ..Default::default()
        }
        .with_component(assets::LiteralComponent::new("Component_PointLight").with_data([
            ("bMatchBrickShape", Box::new(false) as Box<dyn AsBrdbValue>),
            ("bEnabled", Box::new(true)),
            ("Brightness", Box::new(500.0f32)),
            ("Radius", Box::new(800.0f32)),
            (
                "Color",
                Box::new(SavedBrickColor { r: 10, g: 20, b: 30, a: 255 }),
            ),
            ("bUseBrickColor", Box::new(false)),
            ("bCastShadows", Box::new(true)),
        ])),
    );

    // 2: Spot light — narrow cone, non-default brightness/color.
    world.bricks.push(
        Brick {
            position: (20, 0, 6).into(),
            color: (255, 255, 0).into(),
            ..Default::default()
        }
        .with_component(assets::LiteralComponent::new("Component_SpotLight").with_data([
            ("InnerConeAngle", Box::new(15.0f32) as Box<dyn AsBrdbValue>),
            ("OuterConeAngle", Box::new(45.0f32)),
            ("bEnabled", Box::new(true)),
            ("Brightness", Box::new(300.0f32)),
            ("Radius", Box::new(600.0f32)),
            (
                "Color",
                Box::new(SavedBrickColor { r: 255, g: 0, b: 0, a: 255 }),
            ),
            ("bUseBrickColor", Box::new(false)),
            ("bCastShadows", Box::new(true)),
        ])),
    );

    // 3: Interact — custom prompt text, hidden interaction.
    world.bricks.push(
        Brick {
            position: (40, 0, 6).into(),
            color: (0, 255, 255).into(),
            ..Default::default()
        }
        .with_component(assets::LiteralComponent::new("Component_Interact").with_data([
            (
                "Message",
                Box::new("You interacted!".to_string()) as Box<dyn AsBrdbValue>,
            ),
            ("ConsoleTag", Box::new("fixture_interact".to_string())),
            ("bAllowNearbyInteraction", Box::new(false)),
            ("bHiddenInteraction", Box::new(true)),
            ("PromptCustomLabel", Box::new("Open Door".to_string())),
        ])),
    );

    // 4: Buffer (ticks) — WireGraphPseudo component whose Input/Output are
    // WireGraphVariant; non-default counters plus a Number/Bool variant pair.
    world.bricks.push(
        Brick {
            position: (60, 0, 1).into(),
            color: (128, 0, 128).into(),
            asset: assets::components::BufferTicks::default().brick(),
            ..Default::default()
        }
        .with_component(assets::components::BufferTicks {
            current_ticks: 3,
            ticks_to_wait: 10,
            input: WireVariant::Number(2.5),
            output: WireVariant::Bool(true),
        }),
    );

    // 5: Blend gate — WireGraph_Expr_MathBlend's InputA/InputB are each a
    // WireGraphPrimMathVariant (here an f64 and an i64 tag, showing the
    // variant is polymorphic per-port); Blend itself is a plain f64.
    world.bricks.push(
        Brick {
            position: (80, 0, 1).into(),
            color: (0, 128, 0).into(),
            asset: assets::components::LogicGate::Blend.brick(),
            ..Default::default()
        }
        .with_component(assets::components::LogicGate::Blend.component_with_overrides(
            HashMap::from([
                (
                    "InputA".into(),
                    Box::new(WireVariant::Number(10.0)) as Box<dyn AsBrdbValue>,
                ),
                ("InputB".into(), Box::new(WireVariant::Int(7))),
                ("Blend".into(), Box::new(0.75f64)),
            ]),
        )),
    );

    world
}

fn entities_world() -> World {
    // Mirror of examples/write_entity.rs's floating sub-grid — the first
    // grid added after the always-present main grid 1 gets persistent index
    // 2, so it lands at Grids/2 — plus a couple of main-grid (Grids/1)
    // bricks. Both grids are single-chunk; note the *outer* Grids/1 vs
    // Grids/2 folder order comes from a HashMap<usize, UnsavedGrid> in the
    // writer and is NOT guaranteed stable run-to-run (see stability notes).
    //
    // register_all_components() is required here (matching
    // test_write_entity_save, NOT the plain write_entity.rs example): without
    // it, Entity_DynamicBrickGrid's class name is never registered and the
    // write fails with `UnknownType("Entity_DynamicBrickGrid")` — confirmed
    // by running the crate's own unmodified write_entity example.
    let mut world = World::new();
    world.register_all_components();
    world.meta.bundle.description = "Entity fixture".to_string();

    world.bricks.push(Brick {
        position: (0, 0, 6).into(),
        color: (200, 50, 50).into(),
        ..Default::default()
    });
    world.bricks.push(Brick {
        position: (20, 0, 6).into(),
        color: (50, 200, 50).into(),
        ..Default::default()
    });

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

    world
}

fn hash_archive(path: &PathBuf) -> Result<BTreeMap<String, serde_json::Value>, Box<dyn std::error::Error>> {
    fn collect(fs: &BrFs, prefix: &str, out: &mut Vec<(String, Option<i64>)>) {
        match fs {
            BrFs::Root(children) => for (n, c) in children { collect(c, n, out); },
            BrFs::Folder(_, children) => for (n, c) in children {
                collect(c, &format!("{prefix}/{n}"), out);
            },
            BrFs::File(f) => out.push((prefix.to_string(), f.content_id)),
        }
    }
    let reader = Brz::open(path)?.into_reader();
    let reader = &*reader;
    let mut files = Vec::new();
    collect(&reader.get_fs()?, "", &mut files);
    let mut out = BTreeMap::new();
    for (p, content_id) in files {
        let content = match content_id {
            Some(id) => reader.find_blob(id)?.read()?,
            None => Vec::new(),
        };
        out.insert(p, serde_json::json!({
            "blake3": blake3::hash(&content).to_hex().to_string(),
            "len": content.len(),
        }));
    }
    Ok(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    fs::create_dir_all(dir.join("schemas"))?;

    let mut hashes = BTreeMap::new();
    for (name, world) in [
        ("brick", brick_world()),
        ("features", features_world()),
        ("chunks", chunks_world()),
        ("wires", wires_world()),
        ("components", components_world()),
        ("entities", entities_world()),
    ] {
        let pending = world.to_unsaved()?.to_pending()?;
        // raw variant: no zstd anywhere — byte-comparable across languages
        let raw_path = dir.join(format!("{name}_raw.brz"));
        let mut f = fs::File::create(&raw_path)?;
        pending.clone().to_brz_data(None)?.write(&mut f, None)?;
        // compressed variant: zstd level 14 (matches Brz::save)
        let mut f = fs::File::create(dir.join(format!("{name}.brz")))?;
        pending.to_brz_data(Some(14))?.write(&mut f, Some(14))?;

        hashes.insert(name.to_string(), hash_archive(&raw_path)?);
    }
    fs::write(dir.join("hashes.json"), serde_json::to_string_pretty(&hashes)?)?;

    // The nine embedded schemas as binary msgpack (the exact bytes the
    // writer embeds as *.schema files inside archives).
    for (name, schema) in [
        ("BRSavedGlobalDataSoA", schemas::global_data_schema()),
        ("BRSavedOwnerTableSoA", schemas::owners_schema()),
        ("BRSavedBrickChunkIndexSoA", schemas::bricks_chunk_index_schema()),
        ("BRSavedBrickChunkSoA", schemas::bricks_chunks_schema()),
        ("BRSavedWireChunkSoA", schemas::bricks_wires_schema()),
        ("BRSavedComponentChunkSoA", schemas::bricks_components_schema_min()),
        ("BRSavedComponentChunkSoA_max", schemas::bricks_components_schema_max()),
        ("BRSavedEntityChunkIndexSoA", schemas::entities_chunk_index_schema()),
        ("BRSavedEntityChunkSoA", schemas::entities_chunks_schema()),
    ] {
        fs::write(dir.join(format!("schemas/{name}.bin")), schema.to_bytes()?)?;
    }
    eprintln!("fixtures written to {}", dir.display());
    Ok(())
}
