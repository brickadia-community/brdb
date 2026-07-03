use brdb::{BrFsReader, BrReader, Brdb, Brz, IntoReader, schema::BrdbSchema};
use std::path::PathBuf;

/// Print the variant tables (name -> ordered members) of every *.schema file.
fn run<T: BrFsReader>(db: &BrReader<T>) -> Result<(), Box<dyn std::error::Error>> {
    let mut schemas = vec![];
    db.get_fs()?.for_each(&mut |f| {
        if f.is_file() && f.name().ends_with(".schema") {
            if let Ok(buf) = f.read(&**db) {
                schemas.push((f.name().to_string(), buf));
            }
        }
    });

    for (name, bytes) in &schemas {
        let (_enums, variants, _structs) = BrdbSchema::read_to_meta(bytes.as_slice())?;
        if variants.is_empty() {
            continue;
        }
        println!("--- {name} ---");
        for (vname, members) in &variants {
            println!("  {vname}: {members:?}");
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).unwrap());
    if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        run(&Brdb::open(&p)?.into_reader())
    } else {
        run(&Brz::open(&p)?.into_reader())
    }
}
