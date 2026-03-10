# Blobs

Blobs are the content-addressed storage layer of BRDB. Every file's data is stored as a blob, identified by a BLAKE3 hash, and optionally compressed with zstd. Blobs are write-once and immutable.

---

## Blob Table Schema

```sql
CREATE TABLE blobs (
    blob_id           INTEGER PRIMARY KEY,
    compression       INTEGER,
    size_uncompressed INTEGER,
    size_compressed   INTEGER,
    delta_base_id     INTEGER REFERENCES blobs(blob_id),
    hash              BLOB,
    content           BLOB
);

CREATE INDEX blobs_size_hash ON blobs(size_uncompressed, hash);
```

| Column             | Type    | Description                                              |
|--------------------|---------|----------------------------------------------------------|
| `blob_id`          | INTEGER | Auto-assigned primary key.                               |
| `compression`      | INTEGER | `0` = uncompressed, `1` = zstd-compressed.              |
| `size_uncompressed`| INTEGER | Size of the original content in bytes.                   |
| `size_compressed`  | INTEGER | Size of stored content in bytes (equals `size_uncompressed` when `compression = 0`). |
| `delta_base_id`    | INTEGER | FK to `blobs(blob_id)`. Always NULL (reserved, never used). |
| `hash`             | BLOB    | 32-byte BLAKE3 hash of the **uncompressed** content.     |
| `content`          | BLOB    | Raw stored bytes (compressed or uncompressed).           |

---

## Compression

### Values

- `compression = 0`: content is stored verbatim (uncompressed).
- `compression = 1`: content is zstd-compressed.

### Encoding Behavior

When writing a blob with compression enabled:

1. Attempt zstd compression at the specified level (integer, valid range 1-22).
2. If the compressed output is **strictly smaller** than the original, store the compressed bytes and set `compression = 1`.
3. If compression does not reduce size (compressed >= original), discard the compressed output and store the original bytes with `compression = 0`.

No zstd dictionary is used. The entire content is buffered in memory with no streaming compression.

### Decoding

When `compression = 1`, the stored content is decompressed using zstd with `size_uncompressed` as the expected output length.

---

## BLAKE3 Hashing

- **Algorithm:** BLAKE3
- **Output:** 32 bytes (256 bits)
- **Input:** Always the **uncompressed** content, never the compressed bytes.
- **When computed:** Before insertion. The hash is provided by the caller, not computed during the insert operation.
- **Verification:** On every read, the decompressed content is re-hashed and compared byte-for-byte against the stored hash.

---

## Deduplication

The compound index `blobs_size_hash` on `(size_uncompressed, hash)` supports efficient deduplication lookups. Before inserting a new blob, the writer can query for an existing blob with the same size and hash. If a match exists, the file's `content_id` points to the existing blob.

The index is non-unique. Deduplication is enforced by the writer, not by a database constraint.

---

## Read Validation

When reading a blob, the following validation steps are performed:

**When `compression = 1` (compressed):**

1. Verify `content.length == size_compressed`. Error on mismatch (corruption).
2. Decompress via zstd using `size_uncompressed` as the expected output length. Error on decompression failure.
3. Verify `decompressed.length == size_uncompressed`. Error on mismatch.
4. Compute BLAKE3 hash of decompressed content and compare to stored `hash`. Error on mismatch.

**When `compression = 0` (uncompressed):**

Steps 1-2 are skipped. Validation starts at step 3 (size check) then step 4 (hash check).

---

## Blob Lifecycle

| Operation     | Description                                                    |
|---------------|----------------------------------------------------------------|
| **Create**    | Insert content with hash and optional zstd level -> `blob_id`  |
| **Read**      | Fetch by `blob_id`, decompress if needed, validate size + hash |
| **Find**      | Look up by `(size_uncompressed, hash)` for deduplication       |
| **Update**    | Not supported. Blobs are immutable.                             |
| **Delete**    | Not supported. Blobs are never removed.                         |

---

## Quirks

- **`delta_base_id` is dead.** The column and FK constraint exist in the schema but are never populated. It was reserved for delta-compression support that was never implemented. Always NULL.

- **No size limits.** All size columns are SQLite `INTEGER` (signed 64-bit). No maximum blob size is enforced; practical limits are available RAM and SQLite's BLOB size limit.

- **No chunking or streaming.** The entire blob is loaded into memory at once, both on write and on read.

- **Compression is optional and smart.** When compression is requested but doesn't reduce size, the blob is silently stored uncompressed. There is no default compression level; compression must be explicitly requested.
