# .brz File Format

A `.brz` file is a read-only binary archive format for Brickadia worlds. It stores the same virtual filesystem as `.brdb` but as a single binary blob optimized for distribution. The format is specified in [Zeblote's brz Gist](https://gist.github.com/Zeblote/0fc682b9df1a3e82942b613ab70d8a04).

The archive contains three logical regions written contiguously:

```
[Magic:      3 bytes  "BRZ" (0x42, 0x52, 0x5A)]
[Header:     variable]
[Index Data: variable, optionally zstd-compressed]
[Blob Data:  variable, contiguous raw/compressed blob bytes]
```

All integers are little-endian.

---

## Archive Header

Immediately after the magic bytes:

| Field                    | Type        | Description                                              |
|--------------------------|-------------|----------------------------------------------------------|
| `version`                | `u8`        | Format version. Only `0` (`Initial`) is valid.           |
| `index_compression`      | `u8`        | `0 = None`, `1 = GenericZstd`                            |
| `index_size_uncompressed`| `i32 LE`    | Byte length of index data after decompression. Must be >= 0. |
| `index_size_compressed`  | `i32 LE`    | Byte length of index data on disk. Must be >= 0.         |
| `index_hash`             | `[u8; 32]`  | BLAKE3 hash of the **decompressed** index data.          |

The header is 42 bytes total (2 + 4 + 4 + 32).

---

## Index Data

After the header, `index_size_compressed` bytes (or `index_size_uncompressed` bytes if uncompressed) of index data follow. If `index_compression = GenericZstd`, decompress first before parsing. The BLAKE3 hash of the decompressed bytes must match `index_hash`.

The index is a custom little-endian binary format (not msgpack). It uses parallel arrays throughout: all arrays for a given entity type are written in full before the next array begins.

### Counts Header

```
i32  num_folders
i32  num_files
i32  num_blobs
```

All three counts must be >= 0.

### Folder Arrays (parallel, `num_folders` elements each)

```
i32  * num_folders    folder_parent_ids     (-1 = root folder)
u16  * num_folders    folder_name_lengths
utf8 * num_folders    folder_names          (variable length; folder_name_lengths[i] bytes each)
```

### File Arrays (parallel, `num_files` elements each)

```
i32  * num_files      file_parent_ids       (-1 = root file)
i32  * num_files      file_content_ids      (-1 = empty file, no blob)
u16  * num_files      file_name_lengths
utf8 * num_files      file_names            (variable length; file_name_lengths[i] bytes each)
```

### Blob Arrays (parallel, `num_blobs` elements each)

```
u8        * num_blobs    compression_methods    (0 = None, 1 = GenericZstd)
i32       * num_blobs    sizes_uncompressed     (must be >= 0)
i32       * num_blobs    sizes_compressed       (must be >= 0)
[u8; 32]  * num_blobs    blob_hashes            (BLAKE3, one per blob)
```

---

## Blob Data

After the index region, all blob bytes are written as one contiguous binary block. Blob byte ranges are **not stored**; they are computed during deserialization by accumulating sizes:

```
range_size[i] = sizes_uncompressed[i]  if compression_methods[i] == None
              = sizes_compressed[i]    if compression_methods[i] == GenericZstd

start[0] = 0
start[i] = sum of range_size[0..i-1]
end[i]   = start[i] + range_size[i]
```

Blob `i` occupies `blob_data[start[i]..end[i]]`. If the blob is zstd-compressed, decompress it to `sizes_uncompressed[i]` bytes before use.

Multiple files can reference the same blob via their `file_content_ids`.

---

## Index Compression

The index itself may be zstd-compressed. When writing, compression is applied only if it reduces the index size (default compression level: 14). The `index_hash` field always contains the hash of the decompressed index, regardless of whether compression was applied.

---

## Helper Functions

All integers are little-endian:

| Function              | Implementation                        |
|-----------------------|---------------------------------------|
| `read_u8(r)`          | Read 1 byte                           |
| `read_u16(r)`         | Read 2 bytes, `u16::from_le_bytes`    |
| `read_i32(r)`         | Read 4 bytes, `i32::from_le_bytes`    |
| `read_string(r, len)` | Read `len` bytes, validate as UTF-8   |

---

## Error Cases

| Error                            | Condition                                                          |
|----------------------------------|--------------------------------------------------------------------|
| `InvalidMagic([u8; 3])`          | First 3 bytes are not `b"BRZ"`                                     |
| `InvalidFormat(u8)`              | `version` byte is not `0`                                          |
| `InvalidIndexDecompressedLength` | `index_size_uncompressed` < 0                                      |
| `InvalidIndexCompressedLength`   | `index_size_compressed` < 0                                        |
| `InvalidIndexHash`               | Computed BLAKE3 of decompressed index does not match `index_hash`  |
| `InvalidNumFolders`              | `num_folders` < 0                                                  |
| `InvalidNumFiles`                | `num_files` < 0                                                    |
| `InvalidNumBlobs`                | `num_blobs` < 0                                                    |
| `InvalidCompressionMethod(u8)`   | Compression byte is not `0` or `1`                                 |
| `InvalidBlobDecompressedLength`  | A blob `sizes_uncompressed` value < 0                              |
| `InvalidBlobCompressedLength`    | A blob `sizes_compressed` value < 0                                |
| `InvalidUtf8`                    | A name field fails UTF-8 validation                                |

---

## Quirks

- **Blob ranges are computed, not stored.** During deserialization, `blob_ranges` and `blob_total_size` are derived by accumulating range sizes in order. There is no seek table in the file.
- **`parent_id = -1` means root.** Both folders and files use `-1` in their parent ID field to indicate they live at the filesystem root.
- **`content_id = -1` means empty file.** A file with `file_content_ids[i] = -1` has no associated blob and is treated as zero-length.
- **Blobs are deduplicated.** Multiple files can share the same blob by referencing the same blob index. The blob is stored once in the blob data region.
- **All names must be valid UTF-8.** Folder and file names are decoded with strict UTF-8 validation; invalid byte sequences are rejected.
- **Index compression is optional.** The `index_compression` field determines whether the on-disk index bytes need decompression before parsing. The hash always covers the uncompressed form.
