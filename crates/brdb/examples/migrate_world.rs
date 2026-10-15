//! Upgrade an EA3 world to the current save format.
//!
//! Usage: migrate_world <input.brdb|input.brz> <output.brdb|output.brz>
//!
//! A world that is already in the current format is copied unchanged.
use brdb::{Brdb, Brz, IntoReader, migrate};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = &args[..] else {
        return Err("usage: migrate_world <input> <output>".into());
    };
    let (input, output) = (Path::new(input), Path::new(output));
    if output.exists() {
        return Err(format!("{} already exists", output.display()).into());
    }
    let pending = if input.extension().is_some_and(|e| e == "brdb") {
        migrate::to_latest(&Brdb::open(input)?.into_reader())?
    } else {
        migrate::to_latest(&Brz::open(input)?.into_reader())?
    };
    if output.extension().is_some_and(|e| e == "brdb") {
        Brdb::new(output)?.write_pending("Migrated to the current format", pending)?;
    } else {
        Brz::write_pending(output, pending)?;
    }
    println!("wrote {}", output.display());
    Ok(())
}
