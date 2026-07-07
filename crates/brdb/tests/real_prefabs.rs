//! Ground-truth checks against game-written prefab archives.
//! The files live in the gitignored fixtures/real/ dir; the tests are a
//! no-op when they're absent (e.g. CI) — refresh them from
//! `%LOCALAPPDATA%/Brickadia/Saved/Prefabs/`.
use brdb::{BrFsReader, Brz, IntoReader};
use std::path::PathBuf;

fn fixture(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/real")
        .join(name);
    p.exists().then_some(p)
}

#[test]
fn reads_real_spawner_prefab() {
    let Some(path) = fixture("1x1f spawner.brz") else {
        eprintln!("skipping: fixtures/real/1x1f spawner.brz not present");
        return;
    };
    let reader = Brz::open(path).unwrap().into_reader();

    let bundle = reader.bundle_json().unwrap();
    assert_eq!(bundle.level_type, "Prefab");
    assert_eq!(bundle.name, "1x1f spawner");
    // Don't assert on author identity — the fixture is a personal game file.
    assert!(!bundle.authors.is_empty());
    assert!(bundle.color.is_some());

    let prefab = reader.prefab_json().unwrap().expect("Meta/Prefab.json");
    assert!(!prefab.is_microchip_prefab);
    assert!(reader.world_json().unwrap().is_none());
    assert!(reader.thumbnail().unwrap().is_some());

    // Exactly one embedded prefab, content-addressed by BLAKE3.
    let paths = reader.prefab_paths().unwrap();
    assert_eq!(paths.len(), 1);
    let bytes = reader.read_file(&paths[0]).unwrap();
    let hash = blake3::hash(&bytes).to_hex().to_string().to_uppercase();
    assert_eq!(paths[0], format!("Prefabs/Uploads/{hash}.brz"));

    // The spawner component references that exact path.
    let chunks = reader.brick_chunk_index(1).unwrap();
    let mut found = false;
    for chunk in chunks {
        if chunk.num_components == 0 {
            continue;
        }
        let (_, components) = reader.component_chunk_soa(1, chunk.index).unwrap();
        for c in components {
            found |= c.to_string().contains(&paths[0]);
        }
    }
    assert!(found, "no component references {}", paths[0]);

    // The embedded archive is itself a readable prefab bundle.
    let inner = reader.open_prefab(&paths[0]).unwrap().into_reader();
    let inner_bundle = inner.bundle_json().unwrap();
    assert_eq!(inner_bundle.name, "1x1f brick");
    assert!(inner.prefab_json().unwrap().is_some());
    assert!(inner.prefab_paths().unwrap().is_empty());
}

#[test]
fn reads_real_brick_prefab_and_matches_embedded_copy() {
    let (Some(spawner), Some(brick)) = (fixture("1x1f spawner.brz"), fixture("1x1f brick.brz"))
    else {
        eprintln!("skipping: real fixtures not present");
        return;
    };
    let reader = Brz::open(spawner).unwrap().into_reader();
    let path = reader.prefab_paths().unwrap().remove(0);
    // The game embeds referenced prefabs byte-identically.
    assert_eq!(
        reader.read_file(&path).unwrap(),
        std::fs::read(brick).unwrap()
    );
}
