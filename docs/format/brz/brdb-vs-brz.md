# .brdb vs .brz

## Introduction

`.brdb` is the mutable working format used during active editing and at game runtime. It is a SQLite database that tracks full revision history and supports soft-deletion. `.brz` is the read-only distribution format: a binary archive capturing a single snapshot of the active filesystem, suitable for sharing, backup, and distribution.

## Comparison Table

| Aspect | .brdb | .brz |
|--------|-------|------|
| Storage | SQLite database | Binary archive |
| Mutability | Read/write | Read-only snapshot |
| Revision tracking | Full history with timestamps | Single snapshot, no history |
| Compression | Per-blob zstd (optional) | Per-blob zstd + index zstd |
| Hashing | Per-blob BLAKE3 | Per-blob BLAKE3 + index BLAKE3 |
| File hierarchy | SQL tables with parent_id refs | Binary index with parent_id arrays |
| Soft-delete | Yes (deleted_at timestamps) | No (snapshot of active files only) |
| Integer sizes | i64 (SQLite INTEGER) | i32 (binary LE) |
| String encoding | TEXT (SQLite) | u16 length + UTF-8 bytes |

## Conversion

Both formats convert through `BrPendingFs`, an in-memory tree representation:

```
.brdb -> BrPendingFs -> .brz
.brz  -> BrPendingFs -> .brdb
```

`BrPendingFs` is a tree with three node types:

- **Root**: The top-level container. Holds named children but is not itself a folder entry.
- **Folder**: A directory with optional children. A folder with no children means it exists but is unchanged (patch semantics).
- **File**: A file with optional content bytes. A file with no content means it exists but is unchanged.

When converting to `.brz`, the tree is traversed breadth-first. Blobs are deduplicated by BLAKE3 hash and optionally compressed with zstd (falling back to uncompressed if compression doesn't reduce size). Unchanged entries (no content) are silently skipped.

## Use Cases

**Use `.brdb` when:**
- The file is being actively edited by the game runtime or tooling.
- Revision history or rollback capability is required.
- Soft-deletion semantics are needed (entries can be restored).

**Use `.brz` when:**
- Distributing a save to other players or platforms.
- Creating a point-in-time backup with a compact binary footprint.
- Read-only access is sufficient and mutability is not needed.

## Quirks

- **Size limits**: BRZ uses `i32` for all size fields (uncompressed size, compressed size, blob ranges). This imposes a ~2 GB effective limit per blob and per index. BRDB uses SQLite `INTEGER` (i64), so it has no practical per-blob size limit.
- **History loss**: Converting `.brdb` to `.brz` discards all revision history and soft-deleted entries. Only the currently active snapshot of the filesystem is written.
- **Index verification**: The BRZ index is verified with a single BLAKE3 hash covering the entire serialized index. Verification is all-or-nothing: a single corrupted byte in the index invalidates the whole archive, with no per-entry fallback.
