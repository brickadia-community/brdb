use brdb::{Brdb, IntoReader};
use std::collections::BTreeMap;
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).unwrap());
    let db = Brdb::open(&p)?.into_reader();
    let gd = db.global_data()?;
    let mut by_type: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (ty, name) in &gd.external_asset_references {
        by_type.entry(ty.clone()).or_default().push(name.clone());
    }
    println!("=== external_asset_references: {} total, {} types ===",
        gd.external_asset_references.len(), by_type.len());
    for (ty, names) in &by_type {
        println!("\n[{}] ({})", ty, names.len());
        for n in names { println!("  {}", n); }
    }
    println!("\n=== external_asset_types ===");
    let mut ts: Vec<_> = gd.external_asset_types.iter().collect();
    ts.sort();
    for t in ts { println!("  {}", t); }
    Ok(())
}
