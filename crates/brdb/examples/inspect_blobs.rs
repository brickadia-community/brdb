use brdb::brz::Brz;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "world.brz".to_string()),
    );
    let brz = Brz::open(&path)?;
    let idx = &brz.index_data;

    println!("=== BRZ Blob Summary ===");
    println!("files: {}", idx.num_files);
    println!("folders: {}", idx.num_folders);
    println!("unique blobs: {}", idx.num_blobs);

    let total_uncompressed: i64 = idx.sizes_uncompressed.iter().map(|&s| s as i64).sum();
    println!("total uncompressed: {total_uncompressed} bytes ({:.1} MB)", total_uncompressed as f64 / 1_048_576.0);

    // Count references per blob
    let mut ref_count = vec![0usize; idx.num_blobs as usize];
    for &cid in &idx.file_content_ids {
        if cid >= 0 && (cid as usize) < ref_count.len() {
            ref_count[cid as usize] += 1;
        }
    }

    let shared: usize = ref_count.iter().filter(|&&c| c > 1).count();
    let unique: usize = ref_count.iter().filter(|&&c| c == 1).count();
    let empty_refs = idx.file_content_ids.iter().filter(|&&c| c < 0).count();

    println!("\nblobs referenced once: {unique}");
    println!("blobs shared (>1 ref): {shared}");
    println!("files with no content: {empty_refs}");

    // Show shared blobs sorted by total savings
    println!("\n=== Shared Blobs (biggest savings first) ===");
    let mut shared_blobs: Vec<_> = ref_count.iter().enumerate()
        .filter(|&(_, &c)| c > 1)
        .map(|(i, c)| {
            let size = idx.sizes_uncompressed.get(i).copied().unwrap_or(0) as usize;
            (i, *c, size)
        })
        .collect();
    shared_blobs.sort_by_key(|(_, c, s)| std::cmp::Reverse(*c * *s));

    for (blob_id, count, size) in shared_blobs.iter().take(20) {
        let saved = size * (count - 1);
        println!("  blob {blob_id}: {count} refs × {size} bytes = {saved} bytes saved");
    }

    let total_saved: usize = shared_blobs.iter().map(|(_, c, s)| s * (c - 1)).sum();
    let total_if_no_dedup: usize = ref_count.iter().enumerate()
        .map(|(i, c)| idx.sizes_uncompressed.get(i).copied().unwrap_or(0) as usize * c)
        .sum();
    println!("\ntotal data if no dedup: {total_if_no_dedup} bytes ({:.1} MB)", total_if_no_dedup as f64 / 1_048_576.0);
    println!("saved by dedup: {total_saved} bytes ({:.1} MB)", total_saved as f64 / 1_048_576.0);
    println!("actual stored: {total_uncompressed} bytes ({:.1} MB)", total_uncompressed as f64 / 1_048_576.0);

    Ok(())
}
