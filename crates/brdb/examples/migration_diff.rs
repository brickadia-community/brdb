//! Compare the bricks, components and wires of two worlds brick by brick.
//!
//! Usage: migration_diff <expected.brdb> <actual.brdb> [--verbose] [--renames]
//!                       [--allow <file>] [--list]
//!
//! `--allow` names a file of known differences, one `kind<TAB>item` per line
//! (`#` starts a comment). Matching differences are counted separately and
//! do not fail the run. `--list` prints every remaining difference in that
//! format.
//!
//! Bricks are paired by grid, chunk and position in the chunk (the game may
//! reorder bricks within a chunk when it saves). For every pair the
//! tool compares the brick asset, the component types on the brick, each
//! component's data struct and the wires between ports. It prints a summary
//! per kind of difference and exits non-zero when any are found.
use brdb::{BrFsReader, BrReader, Brdb, Brz, IntoReader, WireChunkSoA, schema::BrdbValue};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// grid, chunk, "x,y,z" (with a "#n" suffix for bricks sharing a position).
type BrickKey = (usize, String, String);

#[derive(Default)]
struct WorldDump {
    bricks: BTreeMap<BrickKey, String>,
    /// Per brick: component type -> rendered data (fields only).
    components: BTreeMap<BrickKey, BTreeMap<String, String>>,
    /// Per component type: data struct name.
    structs: BTreeMap<String, String>,
    wires: BTreeSet<String>,
}

/// The fields of a rendered struct, without its leading struct name.
fn fields_of(rendered: &str) -> String {
    rendered
        .split_once('{')
        .map(|(_, rest)| rest.to_owned())
        .unwrap_or_default()
}

fn dump<T: BrFsReader>(db: &BrReader<T>) -> Result<WorldDump, Box<dyn std::error::Error>> {
    let data = db.global_data()?;
    let mut out = WorldDump::default();
    let type_name = |i: u16| {
        data.component_type_names
            .get_index(i as usize)
            .cloned()
            .unwrap_or_else(|| format!("<type {i}>"))
    };
    let port_name = |i: u16| {
        data.component_wire_port_names
            .get_index(i as usize)
            .cloned()
            .unwrap_or_else(|| format!("<port {i}>"))
    };
    for (i, ty) in data.component_type_names.iter().enumerate() {
        let st = data.component_data_struct_names.get(i).cloned().unwrap_or_default();
        out.structs.insert(ty.clone(), st);
    }

    // Pass 1: a stable key for every brick, by (grid, chunk) and index.
    let mut grids: BTreeMap<(usize, String), (brdb::ChunkMeta, Vec<BrickKey>)> = BTreeMap::new();
    for gid in 1..512 {
        let Ok(chunks) = db.brick_chunk_index(gid) else {
            continue;
        };
        for chunk in chunks {
            let ck = chunk.index.to_string();
            let soa = db.brick_chunk_soa(gid, chunk.index)?;
            let mut seen: BTreeMap<String, usize> = BTreeMap::new();
            let mut keys = Vec::with_capacity(soa.brick_type_indices.len());
            let bricks: Vec<_> = soa.iter_bricks(chunk.index, data.clone()).collect();
            for (i, brick) in bricks.into_iter().enumerate() {
                let asset = match brick?.asset {
                    brdb::BrickType::Basic(name) => name.to_string(),
                    brdb::BrickType::Procedural { asset, size } => {
                        format!("{asset} {}x{}x{}", size.x, size.y, size.z)
                    }
                };
                let p = &soa.relative_positions[i];
                let mut id = format!("{},{},{}", p.x, p.y, p.z);
                let n = seen.entry(id.clone()).or_default();
                if *n > 0 {
                    id = format!("{id}#{n}");
                }
                *n += 1;
                let key = (gid, ck.clone(), id);
                out.bricks.insert(key.clone(), asset);
                keys.push(key);
            }
            grids.insert((gid, ck), (chunk, keys));
        }
    }
    let brick_key = |gid: usize, ck: &str, index: u32| -> String {
        grids
            .get(&(gid, ck.to_owned()))
            .and_then(|(_, keys)| keys.get(index as usize))
            .map(|k| format!("{}/{}/{}", k.0, k.1, k.2))
            .unwrap_or_else(|| format!("{gid}/{ck}/<brick {index}>"))
    };

    // Pass 2: components and wires.
    for ((gid, ck), (chunk, keys)) in &grids {
        let gid = *gid;
        if chunk.num_components > 0 {
            let (csoa, structs) = db.component_chunk_soa(gid, chunk.index)?;
            let mut structs = structs.into_iter();
            let mut stream = 0usize;
            for counter in &csoa.component_type_counters {
                let ty = type_name(counter.type_index as u16);
                let has_data = out.structs.get(&ty).is_some_and(|s| s != "None");
                for _ in 0..counter.num_instances {
                    let brick = csoa.component_brick_indices[stream];
                    stream += 1;
                    let rendered = if has_data {
                        structs
                            .next()
                            .map(|s| fields_of(&s.to_string()))
                            .unwrap_or_else(|| "<missing data>".to_owned())
                    } else {
                        String::new()
                    };
                    let key = keys
                        .get(brick as usize)
                        .cloned()
                        .unwrap_or_else(|| (gid, ck.clone(), format!("<brick {brick}>")));
                    out.components.entry(key).or_default().insert(ty.clone(), rendered);
                }
            }
        }
        if chunk.num_wires > 0 {
            let soa = WireChunkSoA::try_from(&BrdbValue::Struct(Box::new(
                db.wire_chunk_soa(gid, chunk.index)?,
            )))?;
            let target = |t: &brdb::WirePortTarget| {
                format!(
                    "{}:{}.{}",
                    brick_key(gid, ck, t.brick_index_in_chunk),
                    type_name(t.component_type_index),
                    port_name(t.port_index)
                )
            };
            for (s, t) in soa.local_wire_sources.iter().zip(&soa.local_wire_targets) {
                out.wires.insert(format!(
                    "{}:{}.{} -> {}",
                    brick_key(gid, ck, s.brick_index_in_chunk),
                    type_name(s.component_type_index),
                    port_name(s.port_index),
                    target(t)
                ));
            }
            for (s, t) in soa.remote_wire_sources.iter().zip(&soa.remote_wire_targets) {
                out.wires.insert(format!(
                    "{}:{}.{} -> {}",
                    brick_key(
                        s.grid_persistent_index as usize,
                        &s.chunk_index.to_string(),
                        s.brick_index_in_chunk
                    ),
                    type_name(s.component_type_index),
                    port_name(s.port_index),
                    target(t)
                ));
            }
        }
    }
    Ok(out)
}

fn open_and_dump(path: &Path) -> Result<WorldDump, Box<dyn std::error::Error>> {
    if path.extension().is_some_and(|e| e == "brdb") {
        dump(&Brdb::open(path)?.into_reader())
    } else {
        dump(&Brz::open(path)?.into_reader())
    }
}

/// The lines only one side has, marked `-` (expected) and `+` (actual).
fn line_diff(expected: &str, actual: &str) -> String {
    let e: BTreeSet<&str> = expected.lines().collect();
    let a: BTreeSet<&str> = actual.lines().collect();
    let mut out = Vec::new();
    for l in e.difference(&a) {
        out.push(format!("            - {l}"));
    }
    for l in a.difference(&e) {
        out.push(format!("            + {l}"));
    }
    out.join("
")
}

/// Differences per category.
#[derive(Default)]
struct Report {
    kinds: BTreeMap<String, Vec<(String, String)>>,
}

impl Report {
    /// Record a difference: `item` identifies it, `detail` explains it.
    fn add(&mut self, kind: impl Into<String>, item: impl Into<String>, detail: String) {
        self.kinds
            .entry(kind.into())
            .or_default()
            .push((item.into(), detail));
    }
    fn total(&self) -> usize {
        self.kinds.values().map(Vec::len).sum()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let allow_file = args.iter().position(|a| a == "--allow").map(|i| i + 1);
    let paths: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && Some(*i) != allow_file)
        .map(|(_, a)| a)
        .collect();
    let verbose = args.iter().any(|a| a == "--verbose");
    let [expected, actual] = paths[..] else {
        return Err("usage: migration_diff <expected.brdb> <actual.brdb> [--verbose]".into());
    };
    let expected = open_and_dump(Path::new(expected))?;
    let actual = open_and_dump(Path::new(actual))?;
    let mut report = Report::default();

    for (key, asset) in &expected.bricks {
        match actual.bricks.get(key) {
            None => report.add("brick missing", format!("{key:?}"), asset.clone()),
            Some(a) if a != asset => {
                report.add("brick asset", format!("{key:?}"), format!("{asset} vs {a}"))
            }
            _ => {}
        }
    }
    for key in actual.bricks.keys().filter(|k| !expected.bricks.contains_key(*k)) {
        report.add("brick extra", format!("{key:?}"), actual.bricks[key].clone());
    }

    let empty = BTreeMap::new();
    let keys: BTreeSet<&BrickKey> =
        expected.components.keys().chain(actual.components.keys()).collect();
    for key in keys {
        let e = expected.components.get(key).unwrap_or(&empty);
        let a = actual.components.get(key).unwrap_or(&empty);
        for (ty, data) in e {
            match a.get(ty) {
                None => report.add(
                    format!("component missing: {ty}"),
                    format!("{key:?}"),
                    String::new(),
                ),
                Some(ad) if ad != data => report.add(
                    format!("component data: {ty}"),
                    format!("{key:?}"),
                    line_diff(data, ad),
                ),
                _ => {}
            }
        }
        for ty in a.keys().filter(|t| !e.contains_key(*t)) {
            report.add(format!("component extra: {ty}"), format!("{key:?}"), String::new());
        }
    }

    // Inferred renames: a brick whose asset differs, and a brick where
    // exactly one component type is missing and one is extra.
    let mut renames: BTreeMap<String, usize> = BTreeMap::new();
    for (key, asset) in &expected.bricks {
        if let Some(a) = actual.bricks.get(key).filter(|a| *a != asset) {
            *renames.entry(format!("brick {a} -> {asset}")).or_default() += 1;
            if args.iter().any(|a| a == "--bricks") {
                println!("brick {key:?}: {a} -> {asset}");
            }
        }
    }
    for (key, e) in &expected.components {
        let a = actual.components.get(key).unwrap_or(&empty);
        let missing: Vec<_> = e.keys().filter(|t| !a.contains_key(*t)).collect();
        let extra: Vec<_> = a.keys().filter(|t| !e.contains_key(*t)).collect();
        if let ([m], [x]) = (&missing[..], &extra[..]) {
            *renames.entry(format!("component {x} -> {m}")).or_default() += 1;
        }
    }
    if args.iter().any(|a| a == "--renames") {
        for (r, n) in &renames {
            println!("rename {n:5}  {r}");
        }
    }

    for (ty, st) in &expected.structs {
        if let Some(ast) = actual.structs.get(ty) {
            if ast != st {
                report.add("struct name", ty.clone(), format!("{st} vs {ast}"));
            }
        }
    }

    for w in expected.wires.difference(&actual.wires) {
        report.add("wire missing", w.clone(), String::new());
    }
    for w in actual.wires.difference(&expected.wires) {
        report.add("wire extra", w.clone(), String::new());
    }

    println!(
        "bricks {} / {}, component bricks {} / {}, wires {} / {}",
        expected.bricks.len(),
        actual.bricks.len(),
        expected.components.len(),
        actual.components.len(),
        expected.wires.len(),
        actual.wires.len()
    );
    // Known differences.
    let mut allowed: BTreeSet<(String, String)> = BTreeSet::new();
    if let Some(i) = args.iter().position(|a| a == "--allow") {
        let path = args.get(i + 1).ok_or("--allow needs a file")?;
        for line in std::fs::read_to_string(path)?.lines() {
            let line = line.split('#').next().unwrap_or("").trim_end();
            if let Some((kind, item)) = line.split_once('\t') {
                allowed.insert((kind.to_owned(), item.to_owned()));
            }
        }
    }
    let mut known = 0;
    let mut unused = allowed.clone();
    for (kind, items) in report.kinds.iter_mut() {
        items.retain(|(item, _)| {
            let key = (kind.clone(), item.clone());
            let hit = allowed.contains(&key);
            if hit {
                known += 1;
                unused.remove(&key);
            }
            !hit
        });
    }
    report.kinds.retain(|_, items| !items.is_empty());

    let list = args.iter().any(|a| a == "--list");
    for (kind, items) in &report.kinds {
        if list {
            for (item, _) in items {
                println!("{kind}\t{item}");
            }
            continue;
        }
        println!("{:6}  {kind}", items.len());
        for (item, detail) in items.iter().take(if verbose { usize::MAX } else { 3 }) {
            println!("          {item}");
            if verbose && !detail.is_empty() {
                println!("{detail}");
            }
        }
    }
    if !allowed.is_empty() {
        println!("known differences: {known} (allow list entries not seen: {})", unused.len());
        for (kind, item) in &unused {
            println!("  unseen: {kind}\t{item}");
        }
    }
    println!("total differences: {}", report.total());
    if report.total() > 0 || !unused.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
