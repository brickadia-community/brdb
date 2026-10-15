//! Read every schema and data file of a world, then re-encode each decoded
//! value with the schema it was read with and decode it again.
//!
//! Usage: read_all <world.brdb|world.brz>
//!
//! Fails on the first file that cannot be decoded or whose re-encoded value
//! decodes differently. Byte-identical re-encodes are counted separately:
//! the game may pick different (equally valid) tags than this crate.
use std::path::PathBuf;
use std::sync::Arc;

use brdb::schema::{BrdbSchema, BrdbValue, ReadBrdbSchema, write::write_brdb};
use brdb::{AsBrdbValue, BrFsReader, BrReader, Brdb, Brz, IntoReader};

#[derive(Default)]
struct Stats {
    files: usize,
    values: usize,
    identical_files: usize,
}

/// Decode `types` in order from `bytes`, requiring every byte to be used.
fn decode_all(
    schema: &Arc<BrdbSchema>,
    bytes: &[u8],
    types: impl IntoIterator<Item = String>,
) -> Result<Vec<(String, BrdbValue)>, Box<dyn std::error::Error>> {
    let mut buf = bytes;
    let mut out = Vec::new();
    for ty in types {
        let value = buf.read_brdb(schema, &ty)?;
        out.push((ty, value));
    }
    if !buf.is_empty() {
        return Err(format!("{} trailing bytes", buf.len()).into());
    }
    Ok(out)
}

/// Re-encode decoded values and check they decode to the same values.
fn round_trip(
    stats: &mut Stats,
    label: &str,
    schema: &Arc<BrdbSchema>,
    original: &[u8],
    values: &[(String, BrdbValue)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut encoded = Vec::new();
    for (ty, value) in values {
        write_brdb(schema, &mut encoded, ty, value)
            .map_err(|e| format!("{label}: re-encode {ty}: {e}"))?;
    }
    let again = decode_all(schema, &encoded, values.iter().map(|(t, _)| t.clone()))
        .map_err(|e| format!("{label}: decode re-encoded: {e}"))?;
    for ((ty, a), (_, b)) in values.iter().zip(&again) {
        if a != b {
            return Err(format!("{label}: {ty} changed in a round trip").into());
        }
    }
    stats.files += 1;
    stats.values += values.len();
    if encoded == original {
        stats.identical_files += 1;
    } else {
        let at = encoded
            .iter()
            .zip(original)
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let near = |b: &[u8]| b[at.saturating_sub(4)..(at + 8).min(b.len())].to_vec();
        println!(
            "re-encode differs from the original bytes: {label} at {at}: {:02x?} vs {:02x?}",
            near(&encoded),
            near(original)
        );
    }
    Ok(())
}

fn read_file<T: BrFsReader>(
    db: &BrReader<T>,
    path: &str,
) -> Result<Option<(i64, Vec<u8>)>, Box<dyn std::error::Error>> {
    let Some(found) = db.find_file_by_path(path)? else {
        return Ok(None);
    };
    Ok(Some((
        found.created_at,
        db.find_blob(found.blob_id)?.read()?,
    )))
}

fn struct_names_of(
    soa: &BrdbValue,
    counters: &str,
    count: &str,
    names: &[String],
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for counter in soa.prop(counters)?.as_array()? {
        let index = counter.prop("TypeIndex")?.as_brdb_u32()? as usize;
        let n = counter.prop(count)?.as_brdb_u32()?;
        let name = names.get(index).cloned().unwrap_or_else(|| "None".into());
        if name != "None" {
            out.extend(std::iter::repeat_n(name, n as usize));
        }
    }
    Ok(out)
}

fn run<T: BrFsReader>(db: &BrReader<T>) -> Result<(), Box<dyn std::error::Error>> {
    let global = db.global_data()?;
    let mut stats = Stats::default();

    // Every schema, with its format.
    let mut schemas = Vec::new();
    db.get_fs()?.for_each(&mut |f| {
        if f.is_file() && f.name().ends_with(".schema") {
            schemas.push((f.name().to_string(), f.read(&**db)));
        }
    });
    for (name, bytes) in schemas {
        let schema = BrdbSchema::read(bytes?.as_slice())?;
        println!(
            "schema {name}: {:?}, {} structs, {} types",
            schema.format(),
            schema.structs.len(),
            schema.types.len()
        );
    }

    // Owners
    let owners = db.owners_soa()?;
    println!("owners: {} rows", owners.prop("UserIds")?.as_array()?.len());

    let (mut bricks, mut components, mut wires, mut grids) = (0usize, 0usize, 0usize, 0usize);
    for gid in 1..512usize {
        let Ok(chunks) = db.brick_chunk_index(gid) else {
            continue;
        };
        grids += 1;
        for chunk in chunks {
            let base = format!("World/0/Bricks/Grids/{gid}");
            // Bricks
            let path = format!("{base}/Chunks/{}.mps", chunk.index);
            if let Some((rev, bytes)) = read_file(db, &path)? {
                let schema = db.bricks_schema_rev(rev)?;
                let values = decode_all(&schema, &bytes, ["BRSavedBrickChunkSoA".to_owned()])
                    .map_err(|e| format!("{path}: {e}"))?;
                let soa = db.brick_chunk_soa(gid, chunk.index)?;
                bricks += soa.relative_positions.len();
                round_trip(&mut stats, &path, &schema, &bytes, &values)?;
            }
            // Components
            if chunk.num_components > 0 {
                let path = format!("{base}/Components/{}.mps", chunk.index);
                let (rev, bytes) = read_file(db, &path)?.ok_or(path.clone())?;
                let schema = db.components_schema_rev(rev)?;
                let mut first = &bytes[..];
                let soa = first.read_brdb(&schema, "BRSavedComponentChunkSoA")?;
                let mut types = vec!["BRSavedComponentChunkSoA".to_owned()];
                let names = struct_names_of(
                    &soa,
                    "ComponentTypeCounters",
                    "NumInstances",
                    &global.component_data_struct_names,
                )?;
                components += names.len();
                types.extend(names);
                let values =
                    decode_all(&schema, &bytes, types).map_err(|e| format!("{path}: {e}"))?;
                round_trip(&mut stats, &path, &schema, &bytes, &values)?;
            }
            // Wires
            if chunk.num_wires > 0 {
                let path = format!("{base}/Wires/{}.mps", chunk.index);
                let (rev, bytes) = read_file(db, &path)?.ok_or(path.clone())?;
                let schema = db.wires_schema_rev(rev)?;
                let values = decode_all(&schema, &bytes, ["BRSavedWireChunkSoA".to_owned()])
                    .map_err(|e| format!("{path}: {e}"))?;
                let soa = &values[0].1;
                wires += soa.prop("LocalWireSources")?.as_array()?.len()
                    + soa.prop("RemoteWireSources")?.as_array()?.len();
                round_trip(&mut stats, &path, &schema, &bytes, &values)?;
            }
        }
    }

    // Entities
    let mut entities = 0;
    let class_names: Vec<String> = global.entity_data_class_names.iter().cloned().collect();
    for chunk in db.entity_chunk_index()? {
        let path = format!("World/0/Entities/Chunks/{chunk}.mps");
        let (rev, bytes) = read_file(db, &path)?.ok_or(path.clone())?;
        let schema = db.entities_schema_rev(rev)?;
        let mut first = &bytes[..];
        let soa = first.read_brdb(&schema, "BRSavedEntityChunkSoA")?;
        let names = struct_names_of(&soa, "TypeCounters", "NumEntities", &class_names)?;
        entities += names.len();
        let mut types = vec!["BRSavedEntityChunkSoA".to_owned()];
        types.extend(names);
        let values = decode_all(&schema, &bytes, types).map_err(|e| format!("{path}: {e}"))?;
        round_trip(&mut stats, &path, &schema, &bytes, &values)?;
        // The typed entity reader agrees.
        db.entity_chunk(chunk)?;
    }

    println!(
        "grids {grids}, bricks {bricks}, components {components}, wires {wires}, entities {entities}"
    );
    println!(
        "round trip: {} files, {} values, {} byte-identical",
        stats.files, stats.values, stats.identical_files
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).expect("usage: read_all <world>"));
    if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        run(&Brdb::open(&p)?.into_reader())
    } else {
        run(&Brz::open(&p)?.into_reader())
    }
}
