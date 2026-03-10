# .brdb File Tree Structure

A `.brdb` file is a SQLite database that stores a virtual filesystem. Four tables (`blobs`, `revisions`, `folders`, and `files`) form a tree of named folders and files, with file content stored as deduplicated, optionally compressed blobs.

---

## SQLite Tables

### `blobs`

Stores file content, optionally compressed and/or delta-encoded.

```sql
CREATE TABLE blobs (
    blob_id             INTEGER PRIMARY KEY,
    compression         INTEGER,
    size_uncompressed   INTEGER,
    size_compressed     INTEGER,
    delta_base_id       INTEGER REFERENCES blobs(blob_id),
    hash                BLOB,
    content             BLOB
);
```

| Column | Description |
|--------|-------------|
| `blob_id` | Auto-assigned primary key |
| `compression` | Compression method identifier (0 = none, other values are format-defined) |
| `size_uncompressed` | Original byte size before compression |
| `size_compressed` | Stored byte size (may equal `size_uncompressed` if uncompressed) |
| `delta_base_id` | Reserved for delta compression (always NULL, never used) |
| `hash` | 32-byte BLAKE3 hash of the uncompressed content; used for deduplication |
| `content` | The stored (possibly compressed) bytes |

---

### `revisions`

Records write history. Each successful commit to the database creates a revision.

```sql
CREATE TABLE revisions (
    revision_id   INTEGER PRIMARY KEY,
    description   TEXT,
    created_at    INTEGER
);
```

| Column | Description |
|--------|-------------|
| `revision_id` | Auto-assigned primary key |
| `description` | Human-readable description of the revision |
| `created_at` | Unix timestamp (seconds) |

---

### `folders`

Represents directories in the virtual filesystem.

```sql
CREATE TABLE folders (
    folder_id   INTEGER PRIMARY KEY,
    parent_id   INTEGER REFERENCES folders(folder_id),
    name        TEXT,
    created_at  INTEGER,
    deleted_at  INTEGER
);
```

| Column | Description |
|--------|-------------|
| `folder_id` | Auto-assigned primary key |
| `parent_id` | Parent folder; `NULL` indicates a root-level folder |
| `name` | Directory name (not a full path) |
| `created_at` | Unix timestamp when the folder was created |
| `deleted_at` | Unix timestamp when the folder was logically deleted; `NULL` if active |

---

### `files`

Represents files in the virtual filesystem.

```sql
CREATE TABLE files (
    file_id     INTEGER PRIMARY KEY,
    parent_id   INTEGER REFERENCES folders(folder_id),
    name        TEXT,
    content_id  INTEGER REFERENCES blobs(blob_id),
    created_at  INTEGER,
    deleted_at  INTEGER
);
```

| Column | Description |
|--------|-------------|
| `file_id` | Auto-assigned primary key |
| `parent_id` | Containing folder |
| `name` | File name (not a full path) |
| `content_id` | References the blob holding this file's content |
| `created_at` | Unix timestamp when the file was created |
| `deleted_at` | Unix timestamp when the file was logically deleted; `NULL` if active |

---

## Indexes

```sql
CREATE INDEX blobs_size_hash             ON blobs(size_uncompressed, hash);
CREATE INDEX folders_parent_name_deleted ON folders(parent_id, name, deleted_at);
CREATE INDEX files_parent_name_deleted   ON files(parent_id, name, deleted_at);
```

- **`blobs_size_hash`**: Used for content deduplication. Before inserting a new blob, a lookup on `(size_uncompressed, hash)` determines whether identical content is already stored.
- **`folders_parent_name_deleted`**: Accelerates hierarchy traversal. Given a parent folder and a name, quickly resolve to the active (non-deleted) folder.
- **`files_parent_name_deleted`**: Same pattern for files. Resolves a named file within a parent folder while filtering deleted entries.

---

## Virtual Filesystem Hierarchy

`parent_id` references in `folders` and `files` form a tree. A `NULL` `parent_id` on a folder means it sits directly under the conceptual root. Files reference their content via `content_id` into the `blobs` table.

The well-known path tree written by the brdb crate is:

```
(root)
+-- Meta/
|   +-- Bundle.json
|   +-- World.json
|   +-- Screenshot.jpg          (optional, omitted if no screenshot)
|   +-- Thumbnail.png           (optional, omitted if no thumbnail)
+-- World/
    +-- 0/
        +-- GlobalData.schema
        +-- GlobalData.mps
        +-- Owners.schema
        +-- Owners.mps
        +-- Minigame.bp         (placeholder, not yet serialized)
        +-- Environment.bp      (placeholder, not yet serialized)
        +-- Bricks/
        |   +-- Grids/
        |       +-- ChunkIndexShared.schema
        |       +-- ChunksShared.schema
        |       +-- WiresShared.schema
        |       +-- ComponentsShared.schema
        |       +-- {grid_id}/
        |           +-- ChunkIndex.mps
        |           +-- Chunks/
        |           |   +-- {x}_{y}_{z}.mps
        |           +-- Components/
        |           |   +-- {x}_{y}_{z}.mps
        |           +-- Wires/
        |               +-- {x}_{y}_{z}.mps
        +-- Entities/
            +-- ChunkIndex.schema
            +-- ChunkIndex.mps
            +-- ChunksShared.schema
            +-- Chunks/          (only present if entities exist)
                +-- {x}_{y}_{z}.mps
```

The `World/` folder is keyed by world ID (always `0` for the primary world). `{grid_id}`, `{x}`, `{y}`, and `{z}` are numeric strings resolved at write time.

---

## Quirks and Notes

**Grid IDs**
Grid `1` is always the main brick grid. Grid IDs greater than `1` are dynamic grids created by entities. The shared schema files (`ChunkIndexShared.schema`, `ChunksShared.schema`, `WiresShared.schema`, `ComponentsShared.schema`) live at the `Grids/` level and are shared across all grids. They are not duplicated into each `{grid_id}/` folder.

**Unimplemented placeholders**
`Minigame.bp` and `Environment.bp` are reserved paths but their serialization is not yet implemented. The files are not written even when the corresponding data is present; the paths are placeholders for a future format revision.

**Entities `Chunks/` directory**
The `Entities/Chunks/` folder is only created when there is at least one entity chunk to write. If no entities exist, the folder is absent entirely.

**Blob deduplication**
Before writing a file's content, the writer computes a hash of the uncompressed bytes and looks up `(size_uncompressed, hash)` in the `blobs` table. If a matching blob already exists, the file's `content_id` points to the existing blob rather than inserting a duplicate. This means multiple files across revisions or grids may share a single `blob_id`.

**Logical deletion**
Both `folders` and `files` use `deleted_at` rather than hard deletes. Overwritten files are marked deleted at the timestamp of the new revision; queries for the active tree filter `WHERE deleted_at IS NULL`.
