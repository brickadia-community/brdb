use std::{collections::HashMap, fmt::Display};

use crate::{
    BrFsError, Wrap,
    errors::BrError,
    wrapper::{
        UnsavedFs,
        schemas::{self, ENTITY_CHUNK_INDEX_SOA, GLOBAL_DATA_SOA, OWNER_TABLE_SOA},
    },
};

/// Describes an entire filesystem tree that needs to be written
/// Any `None` values indicate unchanged files or folders
/// Any absent entries will be deleted
/// All files will be hashed and checked for existing blobs
/// Any overwritten files will be marked as deleted
///
/// A revision will be created along with all of the pending
#[derive(Debug, Clone)]
pub enum BrPendingFs {
    Root(Vec<(String, BrPendingFs)>),
    Folder(Option<Vec<(String, BrPendingFs)>>),
    File(Option<Vec<u8>>),
}

/// Insert `content` at the nested `segments` path, creating folders as
/// needed and reusing existing ones (insertion order is preserved — it
/// defines archive ids).
fn insert_path(tree: &mut Vec<(String, BrPendingFs)>, segments: &[&str], content: Vec<u8>) {
    use BrPendingFs::*;
    let name = segments[0].to_string();
    if segments.len() == 1 {
        tree.push((name, File(Some(content))));
        return;
    }
    let idx = match tree
        .iter()
        .position(|(n, e)| *n == name && matches!(e, Folder(_)))
    {
        Some(i) => i,
        None => {
            tree.push((name, Folder(Some(Vec::new()))));
            tree.len() - 1
        }
    };
    let Folder(Some(children)) = &mut tree[idx].1 else {
        // A Folder(None) (patch placeholder) can't come from from_unsaved's
        // freshly-built tree.
        unreachable!("insert_path only appends Folder(Some) entries");
    };
    insert_path(children, &segments[1..], content);
}

impl BrPendingFs {
    pub fn is_root(&self) -> bool {
        matches!(self, BrPendingFs::Root(_))
    }

    pub fn from_unsaved(fs: UnsavedFs) -> Result<Self, BrError> {
        use BrPendingFs::*;
        let mut worlds = vec![];

        for (world_id, mut world) in fs.worlds {
            // An explicit format pins the value encodings of the data as well.
            let builtin = schemas::BuiltinSchemas::get();
            let pin = |schema: &'static crate::schema::BrdbSchema| {
                schemas::pinned(schema, world.schema_format)
            };
            let global_data_schema = &*pin(builtin.global_data);
            let owners_schema = &*pin(builtin.owners);
            let brick_chunk_index_schema = pin(builtin.bricks_chunk_index);
            let brick_chunk_schema = pin(builtin.bricks_chunks);
            let wires_schema = pin(builtin.bricks_wires);
            let entity_chunk_index_schema = &*pin(builtin.entities_chunk_index);
            let global_data = std::sync::Arc::new(world.global_data.clone());
            world.component_schema.set_global_data(global_data.clone());
            world.entity_schema.set_global_data(global_data);
            let proc_brick_starting_index = world.global_data.proc_brick_starting_index();
            let schema_format = world.schema_format;

            let mut world_dir = vec![
                // Write GlobalData
                (
                    "GlobalData.schema".to_owned(),
                    File(Some(
                        global_data_schema
                            .to_bytes_as(schema_format)
                            .about("GlobalData.schema")?,
                    )),
                ),
                (
                    "GlobalData.mps".to_owned(),
                    File(Some(
                        global_data_schema
                            .write_brdb(GLOBAL_DATA_SOA, &world.global_data)
                            .about("GlobalData.mps")?,
                    )),
                ),
                // Write Owners
                (
                    "Owners.schema".to_owned(),
                    File(Some(
                        owners_schema
                            .to_bytes_as(schema_format)
                            .about("Owners.schema")?,
                    )),
                ),
                (
                    "Owners.mps".to_owned(),
                    File(Some(
                        owners_schema
                            .write_brdb(OWNER_TABLE_SOA, &world.owners)
                            .about("Owners.mps")?,
                    )),
                ),
            ];

            if let Some(_env) = world.environment.as_ref() {
                // TODO: Write Environment.bp
            }
            if let Some(_minigame) = world.minigame.as_ref() {
                // TODO: Write Minigame.bp
            }

            let mut bricks_dir = vec![
                // Shared schemas
                (
                    "ChunkIndexShared.schema".to_owned(),
                    File(Some(
                        brick_chunk_index_schema
                            .to_bytes_as(schema_format)
                            .about("ChunkIndexShared.schema")?,
                    )),
                ),
                (
                    "ChunksShared.schema".to_owned(),
                    File(Some(
                        brick_chunk_schema
                            .to_bytes_as(schema_format)
                            .about("ChunksShared.schema")?,
                    )),
                ),
                (
                    "WiresShared.schema".to_owned(),
                    File(Some(
                        wires_schema
                            .to_bytes_as(schema_format)
                            .about("WiresShared.schema")?,
                    )),
                ),
                // Component schema
                (
                    "ComponentsShared.schema".to_owned(),
                    File(Some(
                        world
                            .component_schema
                            .to_bytes_as(schema_format)
                            .about("ComponentsShared.schema")?,
                    )),
                ),
            ];
            let mut grids_dir = vec![];

            for (grid_id, grid) in world.grids {
                grids_dir.push((
                    grid_id.to_string(),
                    grid.to_pending_with(
                        proc_brick_starting_index,
                        &world.component_schema,
                        &brick_chunk_index_schema,
                        &brick_chunk_schema,
                        &wires_schema,
                    )
                    .about_f(|| format!("Grids/{grid_id}"))?,
                ));
            }

            let mut entities_dir = vec![
                (
                    "ChunkIndex.schema".to_owned(),
                    File(Some(
                        entity_chunk_index_schema
                            .to_bytes_as(schema_format)
                            .about("ChunkIndex.schema")?,
                    )),
                ),
                (
                    "ChunkIndex.mps".to_owned(),
                    File(Some(
                        entity_chunk_index_schema
                            .write_brdb(ENTITY_CHUNK_INDEX_SOA, &world.entity_chunk_index)
                            .about("ChunkIndex.mps")?,
                    )),
                ),
                (
                    "ChunksShared.schema".to_owned(),
                    File(Some(
                        world
                            .entity_schema
                            .to_bytes_as(schema_format)
                            .about("ChunksShared.schema")?,
                    )),
                ),
            ];

            // Entities/Chunks/*
            let entities_chunks_dir = world
                .entity_chunks
                .into_iter()
                .map(|(chunk, entities)| {
                    let buf = entities
                        .to_bytes(&world.entity_schema)
                        .about_f(|| format!("Entities/Chunks/{chunk}.mps"))?;

                    Ok((format!("{chunk}.mps"), File(Some(buf))))
                })
                .collect::<Result<Vec<_>, BrError>>()?;

            // Only add the Chunks directory if there are any chunks
            if !entities_chunks_dir.is_empty() {
                entities_dir.push(("Chunks".to_owned(), Folder(Some(entities_chunks_dir))));
            }
            bricks_dir.push(("Grids".to_owned(), Folder(Some(grids_dir))));
            world_dir.push(("Bricks".to_owned(), Folder(Some(bricks_dir))));
            world_dir.push(("Entities".to_owned(), Folder(Some(entities_dir))));
            worlds.push((world_id.to_string(), Folder(Some(world_dir))));
        }

        // Bundle.json is always present. Prefabs additionally write Prefab.json
        // and omit World.json; worlds write World.json. Both may carry the
        // (optional) Screenshot/Thumbnail — the game reads a prefab's
        // Screenshot.jpg for previews when one is present.
        let mut meta_files = vec![(
            "Bundle.json".to_owned(),
            File(Some(
                serde_json::to_vec(&fs.meta.bundle).about("Bundle.json")?,
            )),
        )];
        if let Some(prefab) = &fs.meta.prefab {
            meta_files.push((
                "Prefab.json".to_owned(),
                File(Some(serde_json::to_vec(prefab).about("Prefab.json")?)),
            ));
            meta_files.push((
                "Screenshot.jpg".to_owned(),
                File(fs.meta.screenshot.clone()),
            ));
            meta_files.push(("Thumbnail.png".to_owned(), File(fs.meta.thumbnail.clone())));
        } else {
            meta_files.push((
                "Screenshot.jpg".to_owned(),
                File(fs.meta.screenshot.clone()),
            ));
            meta_files.push(("Thumbnail.png".to_owned(), File(fs.meta.thumbnail.clone())));
            meta_files.push((
                "World.json".to_owned(),
                File(Some(
                    serde_json::to_vec(&fs.meta.world).about("World.json")?,
                )),
            ));
        }

        let meta_dir = ("Meta".to_owned(), Folder(Some(meta_files)));

        let world_dir = ("World".to_owned(), Folder(Some(worlds)));

        let mut root = vec![meta_dir, world_dir];
        // Embedded prefabs (root `Prefabs/` folder), only when present —
        // bundles with no prefab references have no Prefabs folder at all.
        // Paths nest generically so future subpaths beyond Uploads/ survive.
        for (path, bytes) in fs.prefabs {
            let segments: Vec<&str> = path.split('/').collect();
            insert_path(&mut root, &segments, bytes);
        }
        Ok(Root(root))
    }

    #[cfg(feature = "brz")]
    /// Convert this pending FS into a BRZ archive
    pub fn to_brz_data(self, zstd_level: Option<i32>) -> Result<crate::brz::Brz, BrError> {
        use std::collections::{HashMap, VecDeque};

        use crate::{
            brz::{Brz, BrzIndexData, CompressionMethod},
            errors::BrFsError,
            tables::BrBlob,
        };

        let mut queue = VecDeque::new();
        queue.push_front((None, "Root".to_owned(), self));

        let mut index = BrzIndexData::default();
        // Unique blob contents in blob-id order; compressed after the walk.
        let mut blobs: Vec<Vec<u8>> = Vec::new();
        let mut hash_to_blob_index: HashMap<[u8; 32], i32, BuildDigestHasher> =
            HashMap::with_hasher(BuildDigestHasher::default());

        while let Some((parent_id, name, fs)) = queue.pop_front() {
            match fs {
                BrPendingFs::Root(items) => {
                    for (name, item) in items {
                        queue.push_back((None, name, item));
                    }
                }

                // Insert the folder, then all of its children
                BrPendingFs::Folder(Some(items)) => {
                    let folder_id = index.num_folders;
                    // Add this folder
                    index.num_folders += 1;
                    index.folder_parent_ids.push(parent_id.unwrap_or(-1));
                    index.folder_names.push(name);

                    // Queue the folder's children
                    for (item_name, item_fs) in items {
                        queue.push_back((Some(folder_id), item_name, item_fs));
                    }
                }

                // Insert the file, and its content if it was not already inserted
                BrPendingFs::File(Some(content)) => {
                    index.num_files += 1;
                    index.file_parent_ids.push(parent_id.unwrap_or(-1));
                    index.file_names.push(name);
                    let hash = BrBlob::hash(&content);

                    let content_id = if content.is_empty() {
                        -1
                    } else if let Some(i) = hash_to_blob_index.get(&hash) {
                        *i
                    } else {
                        let blob_id = index.num_blobs;
                        index.num_blobs += 1;

                        hash_to_blob_index.insert(hash, blob_id);
                        index.blob_hashes.push(hash);
                        index.sizes_uncompressed.push(content.len() as i32);
                        blobs.push(content);
                        blob_id
                    };

                    index.file_content_ids.push(content_id)
                }
                BrPendingFs::File(None) | BrPendingFs::Folder(None) => {
                    // Noop - these files are ignored.
                }
            }
        }

        // Compress every unique blob at the same level (in parallel when the
        // workload warrants it), then pick per blob whichever of the
        // compressed/raw forms is smaller, exactly as the sequential loop did.
        let compressed = match zstd_level {
            Some(level) if !blobs.is_empty() => {
                Some(compress_blobs(&blobs, level).map_err(BrFsError::Compress)?)
            }
            _ => None,
        };

        let total_size: usize = blobs
            .iter()
            .enumerate()
            .map(|(i, content)| match &compressed {
                Some(z) if z[i].len() < content.len() => z[i].len(),
                _ => content.len(),
            })
            .sum();

        let mut blob_data = Vec::with_capacity(total_size);
        for (i, content) in blobs.iter().enumerate() {
            let start = blob_data.len();
            match &compressed {
                Some(z) if z[i].len() < content.len() => {
                    index.sizes_compressed.push(z[i].len() as i32);
                    index
                        .compression_methods
                        .push(CompressionMethod::GenericZstd);
                    blob_data.extend_from_slice(&z[i]);
                }
                // Store raw when compression is off or does not shrink the blob
                _ => {
                    index.sizes_compressed.push(content.len() as i32);
                    index.compression_methods.push(CompressionMethod::None);
                    blob_data.extend_from_slice(content);
                }
            }
            index.blob_ranges.push((start, blob_data.len()));
        }
        index.blob_total_size = blob_data.len();

        Ok(Brz {
            index_data: index,
            blob_data,
        })
    }

    // Get the children of the root if this is a root
    pub fn to_root(self) -> Option<Vec<(String, BrPendingFs)>> {
        match self {
            BrPendingFs::Root(items) => Some(items),
            _ => None,
        }
    }

    // Get the children of this folder if this is a folder
    pub fn to_folder(self) -> Option<Vec<(String, BrPendingFs)>> {
        match self {
            BrPendingFs::Folder(items) => items,
            _ => None,
        }
    }

    // Get the file content of this pending FS this is a file
    pub fn to_file(self) -> Option<Vec<u8>> {
        match self {
            BrPendingFs::File(items) => items,
            _ => None,
        }
    }

    /// Apply a PendingFs as a patch to this FS
    pub fn with_patch(mut self, patch: BrPendingFs) -> Result<Self, BrFsError> {
        self.patch(patch)?;
        Ok(self)
    }

    /// Apply a PendingFs as a patch to this FS
    pub fn patch(&mut self, patch: BrPendingFs) -> Result<(), BrFsError> {
        match (self, patch) {
            // Root and folders apply patches to existing children and insert new files
            (BrPendingFs::Folder(Some(children)), BrPendingFs::Folder(Some(patch)))
            | (BrPendingFs::Root(children), BrPendingFs::Root(patch)) => {
                let mut patch_map = patch.into_iter().collect::<HashMap<_, _>>();
                // Patch anything that already exists in the folder
                for (name, fs) in children.iter_mut() {
                    let Some(patch) = patch_map.remove(name) else {
                        continue;
                    };
                    fs.patch(patch)?;
                }
                // Append the other patches
                children.extend(patch_map);
            }
            // If the folder is empty, insert the patched folder
            (BrPendingFs::Folder(source @ None), BrPendingFs::Folder(Some(patch))) => {
                *source = Some(patch);
            }
            (BrPendingFs::File(data), BrPendingFs::File(Some(patch))) => {
                *data = Some(patch);
            }
            (BrPendingFs::Folder(_), BrPendingFs::Folder(None))
            | (BrPendingFs::File(_), BrPendingFs::File(None)) => {
                // None means no changes for this patch
            }
            (left, right) => {
                return Err(BrFsError::InvalidStructure(
                    left.to_string(),
                    right.to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Navigate a pending brdb filesystem to a specific path.
    pub fn cd(&self, path: impl AsRef<str>) -> Result<&BrPendingFs, BrFsError> {
        let mut components = path
            .as_ref()
            .split('/')
            .filter(|s| !s.is_empty())
            .peekable();

        let mut curr = self;

        while let Some(name) = components.next() {
            let children = match curr {
                BrPendingFs::Root(items) => items,
                BrPendingFs::Folder(Some(items)) => items,
                BrPendingFs::Folder(None) => Err(BrFsError::NotFound(name.to_string()))?,
                BrPendingFs::File(_) => Err(if components.peek().is_some() {
                    BrFsError::ExpectedDirectory(name.to_string())
                } else {
                    BrFsError::NotFound(name.to_string())
                })?,
            };

            let Some((_, next)) = children.iter().find(|(n, _)| n == name) else {
                return Err(BrFsError::NotFound(name.to_string()));
            };
            curr = next;
        }

        Ok(curr)
    }

    /// Navigate a pending brdb filesystem to a specific path.
    pub fn cd_mut(&mut self, path: impl AsRef<str>) -> Result<&mut BrPendingFs, BrFsError> {
        let mut components = path
            .as_ref()
            .split('/')
            .filter(|s| !s.is_empty())
            .peekable();

        let mut curr = self;

        while let Some(name) = components.next() {
            let children = match curr {
                BrPendingFs::Root(items) => items,
                BrPendingFs::Folder(Some(items)) => items,
                BrPendingFs::Folder(None) => Err(BrFsError::NotFound(name.to_string()))?,
                BrPendingFs::File(_) => Err(if components.peek().is_some() {
                    BrFsError::ExpectedDirectory(name.to_string())
                } else {
                    BrFsError::NotFound(name.to_string())
                })?,
            };

            let Some((_, next)) = children.iter_mut().find(|(n, _)| n == name) else {
                return Err(BrFsError::NotFound(name.to_string()));
            };
            curr = next;
        }

        Ok(curr)
    }

    /// Navigate a pending brdb filesystem to a specific path.
    pub fn cd_owned(self, path: impl AsRef<str>) -> Result<BrPendingFs, BrFsError> {
        let mut components = path
            .as_ref()
            .split('/')
            .filter(|s| !s.is_empty())
            .peekable();

        let mut curr = self;

        while let Some(name) = components.next() {
            let children = match curr {
                BrPendingFs::Root(items) => items,
                BrPendingFs::Folder(Some(items)) => items,
                BrPendingFs::Folder(None) => Err(BrFsError::NotFound(name.to_string()))?,
                BrPendingFs::File(_) => Err(if components.peek().is_some() {
                    BrFsError::ExpectedDirectory(name.to_string())
                } else {
                    BrFsError::NotFound(name.to_string())
                })?,
            };

            let Some((_, next)) = children.into_iter().find(|(n, _)| n == name) else {
                return Err(BrFsError::NotFound(name.to_string()));
            };
            curr = next;
        }

        Ok(curr)
    }
}

/// Hasher for keys that are already uniform digests (blake3 blob hashes):
/// the first 8 bytes of the digest become the hash value directly.
#[cfg(feature = "brz")]
#[derive(Default)]
struct DigestHasher(u64);

#[cfg(feature = "brz")]
type BuildDigestHasher = std::hash::BuildHasherDefault<DigestHasher>;

#[cfg(feature = "brz")]
impl std::hash::Hasher for DigestHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut buf = [0u8; 8];
        let n = bytes.len().min(8);
        buf[..n].copy_from_slice(&bytes[..n]);
        self.0 = u64::from_le_bytes(buf);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Below this many total bytes the blobs are compressed on the calling
/// thread; spawning workers costs more than it saves.
#[cfg(feature = "brz")]
const PARALLEL_COMPRESS_MIN_BYTES: usize = 128 * 1024;

/// Compress each blob independently at the same zstd level, returning the
/// results in input order. Output bytes are identical to compressing the
/// blobs one by one.
#[cfg(feature = "brz")]
fn compress_blobs(blobs: &[Vec<u8>], level: i32) -> Result<Vec<Vec<u8>>, std::io::Error> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::compression::compress;

    let total: usize = blobs.iter().map(|b| b.len()).sum();
    // available_parallelism is Err on targets without threads (e.g. wasm),
    // which routes those to the sequential path.
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(blobs.len())
        .min(total / (32 * 1024));
    if workers <= 1 || total < PARALLEL_COMPRESS_MIN_BYTES {
        return blobs.iter().map(|b| compress(b, level)).collect();
    }

    // Hand out the largest blobs first so no worker is left finishing a
    // large blob alone after the others drain the queue.
    let mut order: Vec<usize> = (0..blobs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(blobs[i].len()));
    let next = AtomicUsize::new(0);

    let chunks = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let slot = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&i) = order.get(slot) else { break };
                        out.push((i, compress(&blobs[i], level)));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect::<Vec<_>>()
    });

    let mut results: Vec<Option<Vec<u8>>> = Vec::new();
    results.resize_with(blobs.len(), || None);
    for chunk in chunks {
        for (i, res) in chunk {
            results[i] = Some(res?);
        }
    }
    Ok(results
        .into_iter()
        .map(|r| r.expect("every blob index is scheduled exactly once"))
        .collect())
}

impl Display for BrPendingFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrPendingFs::Root(items) => write!(
                f,
                "[{}]",
                items
                    .iter()
                    .map(|(n, i)| format!("{n} {i}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            BrPendingFs::Folder(items) => write!(
                f,
                "[{}]",
                items
                    .as_ref()
                    .map(|v| v
                        .iter()
                        .map(|(n, i)| format!("{n} {i}"))
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_else(|| "empty".to_string())
            ),
            BrPendingFs::File(content) => write!(
                f,
                "({})",
                content
                    .as_ref()
                    .map(|v| v.len().to_string())
                    .unwrap_or_default()
            ),
        }
    }
}
