//! Canonical per-file payload dump for cross-language verification.
//! Usage: cargo run -q --example dump_canonical -- <file.brz|.brdb> [--mode=hashes]
//! Prints JSON: { "<path>": { "blake3": "<hex>", "len": <decompressed len> } }
//! sorted by path. Hashes are over DECOMPRESSED payloads, so the output is
//! independent of container framing and zstd implementation.
use std::collections::BTreeMap;
use std::path::PathBuf;

use brdb::fs::BrFs;
use brdb::{Brdb, BrFsReader, Brz, IntoReader};

fn collect(fs: &BrFs, prefix: &str, out: &mut Vec<(String, Option<i64>)>) {
    match fs {
        BrFs::Root(children) => {
            for (name, child) in children {
                collect(child, name, out);
            }
        }
        BrFs::Folder(_, children) => {
            for (name, child) in children {
                let path = format!("{prefix}/{name}");
                collect(child, &path, out);
            }
        }
        BrFs::File(file) => {
            out.push((prefix.to_string(), file.content_id));
        }
    }
}

fn dump(reader: &impl BrFsReader) -> Result<(), Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    collect(&reader.get_fs()?, "", &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = BTreeMap::new();
    for (path, content_id) in files {
        let content = match content_id {
            Some(id) => reader.find_blob(id)?.read()?,
            None => Vec::new(),
        };
        out.insert(
            path,
            serde_json::json!({
                "blake3": blake3::hash(&content).to_hex().to_string(),
                "len": content.len(),
            }),
        );
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let p = PathBuf::from(args.next().expect("usage: dump_canonical <file> [--mode=hashes]"));
    if let Some(mode) = args.next() {
        if mode != "--mode=hashes" {
            // --mode=canonical is deliberately not implemented: the hashes gate
            // (byte-identical payloads) plus the brs-js read-side tests subsume it.
            return Err(format!("unsupported mode {mode}").into());
        }
    }
    if p.extension().and_then(|e| e.to_str()) == Some("brdb") {
        dump(&*Brdb::open(&p)?.into_reader())
    } else {
        dump(&*Brz::open(&p)?.into_reader())
    }
}
