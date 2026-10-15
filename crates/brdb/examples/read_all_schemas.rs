use std::path::PathBuf;

use brdb::{BrFsReader, Brdb, IntoReader, schema::BrdbSchema};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // world file from argv
    let filename = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "world.brdb".to_string());
    let path = PathBuf::from(filename);
    if !path.exists() {
        eprintln!("File does not exist: {}", path.display());
        std::process::exit(1);
    }

    let db = Brdb::open(&path)?.into_reader();

    let schemas_dir = path.with_extension("schemas");
    if schemas_dir.exists() {
        std::fs::remove_dir_all(&schemas_dir)?;
    }
    std::fs::create_dir_all(&schemas_dir)?;

    db.get_fs()?.for_each(&mut |f| {
        if !f.is_file() || !f.name().ends_with(".schema") {
            return;
        }
        println!("Reading schema file: {}... ", f.name());

        let Ok(buf) = f.read(&*db) else {
            eprintln!("Failed to read schema file: {}", f.name());
            return;
        };

        let schema = match BrdbSchema::read(buf.as_slice()) {
            Ok(schema) => schema,
            Err(e) => {
                eprintln!("Failed to parse schema file: {}: {e}", f.name());
                return;
            }
        };
        println!(
            "  {:?} format: {} enums, {} variants, {} structs, {} types",
            schema.format(),
            schema.enums.len(),
            schema.variants.len(),
            schema.structs.len(),
            schema.types.len()
        );
        let soa = schema
            .structs
            .last()
            .unwrap()
            .0
            .get(&schema)
            .unwrap()
            .to_string();

        if let Err(e) = std::fs::write(
            schemas_dir.join(soa).with_extension("schema"),
            schema.to_string(),
        ) {
            eprintln!("Failed to write schema file: {}: {}", f.name(), e);
        }
    });

    Ok(())
}
