use std::path::{Path, PathBuf};
use clap::Parser;
use anyhow::{Context, Result, bail};
use brdb::{
    Brdb, Brz, IntoReader,
    pending::BrPendingFs,
    BrReader, BrFsReader,
    schema::{BrdbSchema, BrdbValue, ReadBrdbSchema, BrdbSchemaGlobalData},
    schemas,
};
use std::sync::Arc;
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "brzip")]
#[command(about = "A tool to unpack .brdb and .brz files")]
struct Cli {
    /// The input file to unpack
    input: PathBuf,

    /// The output directory (optional, defaults to input filename without extension)
    output: Option<PathBuf>,

    /// Convert .schema and .mps files to JSON
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let args = Cli::parse();

    let input_path = args.input.clone();
    if !input_path.exists() {
        bail!("Input file does not exist: {}", input_path.display());
    }

    let output_path = match args.output.clone() {
        Some(p) => p,
        None => {
            let file_stem = input_path.file_stem().context("Invalid input filename")?;
            let mut p = input_path.parent().unwrap_or(Path::new("")).to_path_buf();
            p.push(file_stem);
            p
        }
    };

    println!("Unpacking {} to {}", input_path.display(), output_path.display());

    let extension = input_path.extension().and_then(|s| s.to_str()).unwrap_or("");

    if extension.eq_ignore_ascii_case("brdb") {
        let db = Brdb::open(&input_path).context("Failed to open .brdb file")?;
        let reader = BrReader::new(db);
        let pending_fs = reader.to_pending().context("Failed to read filesystem from .brdb")?;
        unpack_pending_fs(&reader, &pending_fs, &output_path, args.json).context("Failed to unpack filesystem")?;
    } else if extension.eq_ignore_ascii_case("brz") {
        let brz = Brz::open(&input_path).context("Failed to open .brz file")?;
        let reader = brz.into_reader();
        let pending_fs = reader.to_pending().context("Failed to convert .brz to pending fs")?;
        unpack_pending_fs(&reader, &pending_fs, &output_path, args.json).context("Failed to unpack filesystem")?;
    } else {
         bail!("Unknown file extension: {}. Supported: .brdb, .brz", extension);
    };

    println!("Done.");

    Ok(())
}

fn unpack_pending_fs<T: BrFsReader>(reader: &BrReader<T>, fs: &BrPendingFs, path: &Path, convert_json: bool) -> Result<()> {
    unpack_recursive(reader, fs, path, PathBuf::new(), convert_json)
}

fn unpack_recursive<T: BrFsReader>(reader: &BrReader<T>, fs: &BrPendingFs, output_root: &Path, rel_path: PathBuf, convert_json: bool) -> Result<()> {
    use std::fs;

    match fs {
        BrPendingFs::Root(children) => {
            fs::create_dir_all(output_root.join(&rel_path))?;
            for (name, child) in children {
                unpack_recursive(reader, child, output_root, rel_path.join(name), convert_json)?;
            }
        }
        BrPendingFs::Folder(Some(children)) => {
            fs::create_dir_all(output_root.join(&rel_path))?;
            for (name, child) in children {
                unpack_recursive(reader, child, output_root, rel_path.join(name), convert_json)?;
            }
        }
        BrPendingFs::Folder(None) => {
             fs::create_dir_all(output_root.join(&rel_path))?;
        }
        BrPendingFs::File(Some(content)) => {
            let final_path = output_root.join(&rel_path);
            if let Some(parent) = final_path.parent() {
                 fs::create_dir_all(parent)?;
            }
            fs::write(&final_path, content)?;

            if convert_json {
                if let Err(e) = convert_to_json(reader, &rel_path, content, &final_path) {
                    eprintln!("Warning: Failed to convert {} to JSON: {}", rel_path.display(), e);
                }
            }
        }
        BrPendingFs::File(None) => {
        }
    }
    Ok(())
}

fn convert_to_json<T: BrFsReader>(reader: &BrReader<T>, rel_path: &Path, content: &[u8], output_path: &Path) -> Result<()> {
    let ext = rel_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    // Normalize Windows separators so the forward-slash patterns in
    // resolve_mps_info match.
    let path_str = rel_path.to_string_lossy().replace('\\', "/");


    let mut cursor = content;

    if ext == "schema" {

        let global_data = match reader.global_data() {
            Ok(gd) => gd,
            Err(_) => {
                Arc::new(BrdbSchemaGlobalData::default())
            }
        };

        let schema = match cursor.read_brdb_schema_with_data(global_data) {
            Ok(s) => s,
            Err(_) => {

                 let mut cursor2 = content;
                 cursor2.read_brdb_schema()?
            }
        };
        
        let json_val = schema_to_json(&schema);
        let mut json_path = output_path.to_path_buf();
        json_path.set_extension("schema.json");
        let file = std::fs::File::create(json_path)?;
        serde_json::to_writer_pretty(file, &json_val)?;

    } else if ext == "mps" {

        let (schema_path, type_name) = resolve_mps_info(&path_str).context("Unknown MPS file type")?;
        

        let schema_bytes = reader.read_file(schema_path)?;
        
        let global_data = reader.global_data().unwrap_or_else(|_| Arc::new(BrdbSchemaGlobalData::default()));
        let mut schema_cursor = schema_bytes.as_slice();
        let schema = schema_cursor.read_brdb_schema_with_data(global_data.clone())?;


        let val = cursor.read_brdb(&schema, type_name)?;
        
        let json_val = value_to_json(&val, &schema, &global_data)?;
        let mut json_path = output_path.to_path_buf();
        json_path.set_extension("mps.json");
        let file = std::fs::File::create(json_path)?;
        serde_json::to_writer_pretty(file, &json_val)?;
    }

    Ok(())
}

fn resolve_mps_info(path: &str) -> Option<(&'static str, &'static str)> {
    if path.contains("GlobalData.mps") {
        return Some(("World/0/GlobalData.schema", schemas::GLOBAL_DATA_SOA));
    }
    if path.contains("Owners.mps") {
        return Some(("World/0/Owners.schema", schemas::OWNER_TABLE_SOA));
    }
    if path.contains("Bricks/Grids/") && path.contains("ChunkIndex.mps") {
        return Some(("World/0/Bricks/ChunkIndexShared.schema", schemas::BRICK_CHUNK_INDEX_SOA));
    }
    if path.contains("Bricks/Grids/") && path.contains("/Chunks/") {
        return Some(("World/0/Bricks/ChunksShared.schema", schemas::BRICK_CHUNK_SOA));
    }
    if path.contains("Bricks/Grids/") && path.contains("/Components/") {
        return Some(("World/0/Bricks/ComponentsShared.schema", schemas::BRICK_COMPONENT_SOA));
    }
    if path.contains("Bricks/Grids/") && path.contains("/Wires/") {
        return Some(("World/0/Bricks/WiresShared.schema", schemas::BRICK_WIRE_SOA));
    }
    if path.contains("Entities/ChunkIndex.mps") {
        return Some(("World/0/Entities/ChunkIndex.schema", schemas::ENTITY_CHUNK_INDEX_SOA));
    }
    if path.contains("Entities/Chunks/") {
        return Some(("World/0/Entities/ChunksShared.schema", schemas::ENTITY_CHUNK_SOA));
    }
    None
}

fn schema_to_json(schema: &BrdbSchema) -> Value {
    let mut enums = serde_json::Map::new();
    for (name, vals) in &schema.enums {
        let name_str = schema.intern.lookup(*name).unwrap_or("?".to_string());
        let mut variants = serde_json::Map::new();
        for (k, v) in vals {
             let k_str = schema.intern.lookup(*k).unwrap_or("?".to_string());
             variants.insert(k_str, json!(v));
        }
        enums.insert(name_str, Value::Object(variants));
    }

    let mut structs = serde_json::Map::new();
    for (name, props) in &schema.structs {
        let name_str = schema.intern.lookup(*name).unwrap_or("?".to_string());
        let mut prop_map = serde_json::Map::new();
        for (k, v) in props {
            let k_str = schema.intern.lookup(*k).unwrap_or("?".to_string());
            prop_map.insert(k_str, json!(v.as_string(schema)));
        }
        structs.insert(name_str, Value::Object(prop_map));
    }

    json!({
        "enums": enums,
        "structs": structs
    })
}

fn value_to_json(val: &BrdbValue, schema: &BrdbSchema, global_data: &BrdbSchemaGlobalData) -> Result<Value> {
    Ok(match val {
        BrdbValue::Nil => Value::Null,
        BrdbValue::Bool(b) => json!(b),
        BrdbValue::U8(n) => json!(n),
        BrdbValue::U16(n) => json!(n),
        BrdbValue::U32(n) => json!(n),
        BrdbValue::U64(n) => json!(n),
        BrdbValue::I8(n) => json!(n),
        BrdbValue::I16(n) => json!(n),
        BrdbValue::I32(n) => json!(n),
        BrdbValue::I64(n) => json!(n),
        BrdbValue::F32(n) => json!(n),
        BrdbValue::F64(n) => json!(n),
        BrdbValue::String(s) => json!(s),
        BrdbValue::Asset(opt_idx) => {
             if let Some(idx) = opt_idx {
                 if let Some((ty, name)) = global_data.external_asset_references.get_index(*idx) {
                     json!(format!("{}/{}", ty, name))
                 } else {
                     json!(format!("AssetIndex({})", idx))
                 }
             } else {
                 Value::Null
             }
        },
        BrdbValue::Enum(e) => {
             json!(e.get_value())
        },
        BrdbValue::Struct(s) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &s.properties {
                let key_str = schema.intern.lookup(*k).unwrap_or("?".to_string());
                map.insert(key_str, value_to_json(v, schema, global_data)?);
            }
            Value::Object(map)
        },
        BrdbValue::Array(arr) | BrdbValue::FlatArray(arr) => {
            let mut vec = Vec::new();
            for v in arr {
                vec.push(value_to_json(v, schema, global_data)?);
            }
            Value::Array(vec)
        },
        BrdbValue::Map(m) => {

             let mut map = serde_json::Map::new();
             for (k, v) in m {
                 let k_json = value_to_json(k, schema, global_data)?;
                 let k_str = match k_json {
                     Value::String(s) => s,
                     _ => k_json.to_string(),
                 };
                 map.insert(k_str, value_to_json(v, schema, global_data)?);
             }
             Value::Object(map)
        },
        BrdbValue::WireVar(w) => json!(w.to_string()),
    })
}
