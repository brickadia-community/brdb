use brdb::{BrFsReader, BrReader, Brdb, Brz, IntoReader, schema::BrdbSchema};
use std::path::PathBuf;

/// Find every *.schema file, read its raw bytes, round-trip through
/// BrdbSchema::read + write, and report any that don't re-emit byte-identically.
fn run<T: BrFsReader>(db: &BrReader<T>) -> Result<(), Box<dyn std::error::Error>> {
    let mut schemas = vec![];
    db.get_fs()?.for_each(&mut |f| {
        if f.is_file() && f.name().ends_with(".schema") {
            if let Ok(buf) = f.read(&**db) {
                schemas.push((f.name().to_string(), buf));
            }
        }
    });

    for (name, original) in &schemas {
        match BrdbSchema::read(original.as_slice()) {
            Ok(schema) => {
                let reemit = schema.to_bytes()?;
                if &reemit == original {
                    println!("OK   {name} ({} bytes)", original.len());
                } else {
                    println!(
                        "DIFF {name}: original {} bytes, reemit {} bytes",
                        original.len(),
                        reemit.len()
                    );
                    // Find first differing byte
                    let first = original
                        .iter()
                        .zip(reemit.iter())
                        .position(|(a, b)| a != b);
                    if let Some(i) = first {
                        let lo = i.saturating_sub(8);
                        println!(
                            "  first diff at byte {i}\n    orig: {:02x?}\n    new : {:02x?}",
                            &original[lo..(i + 8).min(original.len())],
                            &reemit[lo..(i + 8).min(reemit.len())],
                        );
                    }
                }
            }
            Err(e) => println!("ERR  {name}: {e}"),
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
