use brdb::{BrFsReader, BrReader, Brdb, Brz, IntoReader, schema::BrdbSchema};
use std::path::PathBuf;

/// Print every *.schema's enums/variants/structs (names + ordered props) so two
/// files can be diffed with `diff`. Usage: dump_schema_meta <file> [schema_substr]
fn run<T: BrFsReader>(db: &BrReader<T>, filter: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut schemas = vec![];
    db.get_fs()?.for_each(&mut |f| {
        if f.is_file() && f.name().ends_with(".schema") && f.name().contains(filter) {
            if let Ok(buf) = f.read(&**db) {
                schemas.push((format!("{} ({} bytes)", f.name(), buf.len()), buf));
            }
        }
    });
    schemas.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, bytes) in &schemas {
        let (enums, variants, structs) = BrdbSchema::read_to_meta(bytes.as_slice())?;
        println!("######## {path} ########");
        for (n, vals) in &enums {
            let names: Vec<_> = vals.iter().map(|(k, v)| format!("{k}={v}")).collect();
            println!("enum {n}: {}", names.join(", "));
        }
        for (n, members) in &variants {
            println!("variant {n}: {members:?}");
        }
        for (n, props) in &structs {
            let p: Vec<_> = props.iter().map(|(k, v)| format!("{k}:{v:?}")).collect();
            println!("struct {n} {{ {} }}", p.join(", "));
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).unwrap());
    let filter = std::env::args().nth(2).unwrap_or_default();
    if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        run(&Brdb::open(&p)?.into_reader(), &filter)
    } else {
        run(&Brz::open(&p)?.into_reader(), &filter)
    }
}
