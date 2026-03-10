# Revision Mechanism

BRDB tracks changes to the file system over time using a lightweight revision system. Each write operation produces a named, timestamped revision record. Files and folders are never physically removed; instead, they carry a `deleted_at` timestamp that marks when they ceased to be active. Temporal queries reconstruct the state of the file system at any point in time by filtering on these timestamps.

---

## Revisions Table

```sql
CREATE TABLE revisions (
    revision_id INTEGER PRIMARY KEY,
    description TEXT,
    created_at  INTEGER
);
```

| Column        | Type    | Notes                                              |
|---------------|---------|----------------------------------------------------|
| `revision_id` | INTEGER | Auto-increment primary key (SQLite rowid alias).   |
| `description` | TEXT    | Human-readable label; no uniqueness constraint.    |
| `created_at`  | INTEGER | Unix timestamp in whole seconds (UTC).             |

There is no `UNIQUE` constraint on either `description` or `created_at`. Two revisions committed within the same second will share an identical `created_at` value.

---

## Timestamps

All timestamps in BRDB (in `revisions`, `files`, and `folders`) are Unix timestamps with 1-second granularity, stored as signed 64-bit integers. The value represents whole seconds since the Unix epoch (1970-01-01 00:00:00 UTC).

---

## Soft-Delete Pattern

Neither `files` nor `folders` rows are ever physically removed from the database. Both tables carry a nullable `deleted_at INTEGER` column:

```sql
CREATE TABLE folders (
    folder_id  INTEGER PRIMARY KEY,
    parent_id  INTEGER REFERENCES folders(folder_id),
    name       TEXT,
    created_at INTEGER,
    deleted_at INTEGER          -- NULL = active
);

CREATE TABLE files (
    file_id    INTEGER PRIMARY KEY,
    parent_id  INTEGER REFERENCES folders(folder_id),
    name       TEXT,
    content_id INTEGER REFERENCES blobs(blob_id),
    created_at INTEGER,
    deleted_at INTEGER          -- NULL = active
);
```

When a file or folder is removed, its `deleted_at` is set to the revision's timestamp. The row is never deleted:

```sql
UPDATE files   SET deleted_at = :timestamp WHERE file_id   = :id;
UPDATE folders SET deleted_at = :timestamp WHERE folder_id = :id;
```

The `deleted_at` value is the same Unix-second timestamp as the enclosing revision's `created_at`. All current-state queries filter on `deleted_at IS NULL`:

```sql
WHERE parent_id = :parent AND deleted_at IS NULL
```

Because rows are never removed, the full history of every file and folder is always present in the database.

---

## Temporal Queries

Revisions are **not** linked to files or folders via a foreign key. The `revisions` table records when a snapshot was taken, but individual file/folder rows carry their own `created_at` and `deleted_at` timestamps.

To reconstruct the file system state at a given revision, look up the revision's `created_at` value and apply this filter:

```sql
WHERE created_at <= :revision_time
  AND (deleted_at IS NULL OR deleted_at > :revision_time)
```

This selects rows that existed by the revision time and had not yet been deleted at that point.

---

## Revision Lifecycle

### Creation

A revision is created by inserting a row into `revisions` with a description and `created_at` timestamp. This is the only write operation for revisions. There are no update or delete operations. Revisions are write-once.

### Initial Revision

When a new `.brdb` database is created, an initial revision with the description `"Initial Revision"` is inserted automatically. Every valid `.brdb` database has at least one revision.

### Write Transactions

Revisions are created as part of an atomic transaction:

1. Obtain a single `created_at` timestamp.
2. Open a SQLite transaction.
3. Insert the revision record.
4. Apply all pending file/folder insertions and deletions, each stamped with the same `created_at`.
5. Commit the transaction. Any error causes a full rollback.

Because a single timestamp is shared across the revision and all file/folder changes, every change within one revision is indistinguishable by timestamp. Changes in different revisions are distinguishable (assuming they don't occur within the same second).

---

## Quirks and Limitations

- **No uniqueness on `created_at`.** Two writes within the same second produce revisions with identical timestamps. Temporal queries over such revisions may match rows from both simultaneously.

- **No revision metadata.** Revisions carry only a free-text description and a timestamp. There is no author field, no parent-revision pointer, no file count, and no content checksum.

- **No diff API.** There is no built-in way to enumerate what changed between two revisions. The only approach is to query both timestamps independently and compare the resulting row sets.

- **No cascade on folder deletion.** Deleting a folder only sets `deleted_at` on that folder row. Child folders and files are not automatically marked deleted, so each descendant must be deleted individually.

- **Write-once revisions.** Once created, neither the description nor the timestamp can be modified.
