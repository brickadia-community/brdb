use brdb::{BrFsReader, Brdb, Brz, IntoReader};
use std::{ops::Deref, path::PathBuf};

fn dump<R>(db: &R) -> Result<(), Box<dyn std::error::Error>>
where
    R: Deref,
    R::Target: BrFsReader + Sized,
{
    let inner: &R::Target = db;
    println!("=== FS ===\n{}", inner.get_fs()?.render());
    inner.get_fs()?.for_each(&mut |f| {
        if f.is_file() && f.name().ends_with(".json") {
            if let Ok(buf) = f.read(inner) {
                println!("\n=== {} ===\n{}", f.name(), String::from_utf8_lossy(&buf));
            }
        }
    });
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = PathBuf::from(std::env::args().nth(1).unwrap());
    if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        dump(&Brdb::open(&p)?.into_reader())
    } else {
        dump(&Brz::open(&p)?.into_reader())
    }
}
