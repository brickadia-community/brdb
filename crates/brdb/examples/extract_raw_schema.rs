use brdb::{BrFsReader, BrReader, Brdb, Brz, IntoReader};
use std::{io::Write, path::PathBuf};

/// Extract the raw bytes of the first *.schema whose name contains the filter,
/// writing them to the given output path. Usage:
///   extract_raw_schema <file> <schema_substr> <out_path>
fn run<T: BrFsReader>(
    db: &BrReader<T>,
    filter: &str,
    out: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut found: Option<Vec<u8>> = None;
    db.get_fs()?.for_each(&mut |f| {
        if found.is_none()
            && f.is_file()
            && f.name().ends_with(".schema")
            && f.name().contains(filter)
        {
            if let Ok(buf) = f.read(&**db) {
                found = Some(buf);
            }
        }
    });
    let bytes = found.ok_or("schema not found")?;
    std::fs::File::create(out)?.write_all(&bytes)?;
    println!("wrote {} bytes to {out}", bytes.len());
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).unwrap());
    let filter = std::env::args().nth(2).unwrap();
    let out = std::env::args().nth(3).unwrap();
    if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        run(&Brdb::open(&p)?.into_reader(), &filter, &out)
    } else {
        run(&Brz::open(&p)?.into_reader(), &filter, &out)
    }
}
