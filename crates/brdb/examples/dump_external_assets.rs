//! Print every external asset reference in a save, grouped by type.
//!
//! ```sh
//! cargo run --example dump_external_assets -- <file.brz|brdb>
//! ```

use brdb::{Brdb, Brz, IntoReader};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).expect("usage: dump_external_assets <file>"));
    let global = if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        Brdb::open(&p)?.into_reader().read_global_data()?
    } else {
        Brz::open(&p)?.into_reader().read_global_data()?
    };

    let mut by_type: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (ty, name) in global.external_asset_references.iter() {
        by_type.entry(ty.clone()).or_default().push(name.clone());
    }
    for (ty, names) in by_type {
        println!("{ty} ({}):", names.len());
        for n in names {
            println!("  {n}");
        }
    }
    Ok(())
}
