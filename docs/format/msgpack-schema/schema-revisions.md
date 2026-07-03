# Schema Selection by Revision

A recurring point of confusion: *which schema decodes a `.mps` chunk, and how is it chosen "by
date"?* This document answers that at the format level.

**There is no date → schema-version table.** BRDB is a versioned filesystem (see
[revisions.md](../brdb/revisions.md)): both data files (`.mps`) and schema files (`.schema`)
carry `created_at` / `deleted_at` timestamps. A chunk is decoded with the sibling `.schema`
file that was **live at the chunk's own `created_at`**. `.brz` archives are single-revision and
ignore this entirely.

---

## Why per-revision schemas exist

A `.schema` describes the exact field layout of the `.mps` files beside it. Schemas evolve over
a world's edit history — new fields are added to component data structs, new component or entity
types appear. Because every write is a new revision (old file rows soft-deleted, new rows
inserted), a world can simultaneously hold a chunk written under an older schema and the newer
`.schema` that later superseded it. Decoding that chunk with the newer schema would misread its
fields. The chunk must be paired with the schema that was active when the chunk was written.

---

## The lookup

Data and schema files sit side by side, e.g. `World/0/Bricks/ChunksShared.schema` alongside
`World/0/Bricks/Grids/1/Chunks/0_0_0.mps`. To decode `0_0_0.mps`:

1. Resolve the chunk file and read its `created_at`.
2. Resolve the sibling schema **as of that timestamp**, using the half-open interval predicate:

   ```sql
   SELECT content_id, created_at FROM files
   WHERE parent_id = :parent AND name = :schema_name
     AND created_at <= :chunk_created_at
     AND (deleted_at IS NULL OR deleted_at > :chunk_created_at)
   ORDER BY created_at ASC LIMIT 1;
   ```

   This selects the schema whose lifetime `[created_at, deleted_at)` contains the chunk's
   timestamp. The interval predicate yields at most one row; the `ORDER BY … LIMIT 1` is
   defensive.
3. Parse that schema blob and decode the chunk with it — including any per-instance
   component/entity structs appended after the SoA, which use the same revision-selected schema.

Parent folders are treated as stable across revisions; only the leaf `.schema` file is resolved
at the revision. Timestamps are Unix epoch **seconds**.

---

## Per-category schema files

Each data category has one shared `.schema` file and a root SoA struct. A chunk of a given
category is always decoded with that category's schema, revision-selected as above:

| Data category | Schema path | Root SoA struct |
|---|---|---|
| Owner table | `World/0/Owners.schema` | `BRSavedOwnerTableSoA` |
| Global data | `World/0/GlobalData.schema` | `BRSavedGlobalDataSoA` |
| Brick chunks | `World/0/Bricks/ChunksShared.schema` | `BRSavedBrickChunkSoA` |
| Brick chunk index | `World/0/Bricks/ChunkIndexShared.schema` | `BRSavedBrickChunkIndexSoA` |
| Component chunks | `World/0/Bricks/ComponentsShared.schema` | `BRSavedComponentChunkSoA` (+ per-type data structs) |
| Wire chunks | `World/0/Bricks/WiresShared.schema` | `BRSavedWireChunkSoA` |
| Entity chunks | `World/0/Entities/ChunksShared.schema` | `BRSavedEntityChunkSoA` (+ entity data structs) |
| Entity chunk index | `World/0/Entities/ChunkIndex.schema` | `BRSavedEntityChunkIndexSoA` |

A chunk and its inline per-instance structs always resolve to the **same** schema version, so a
`BRSavedComponentChunkSoA` never disagrees with the `BrickComponentData_*` structs appended
after it.

---

## `GlobalData` is the exception

`World/0/GlobalData.{schema,mps}` is decoded with the **current** schema, not a
revision-selected one. Older saves may omit some fields (`EntityDataClassNames`,
`GlobalGridEntityTypeIndex`); these are handled by presence checks rather than by schema
versioning.

---

## `.brz` ignores revisions

A `.brz` archive is a single snapshot with one `.schema` per category and no timestamps (all
files behave as `created_at = 0`). There is exactly one schema version, so the revision
selection is a no-op — the sibling schema is always the one used.

---

## Implications for writers

A writer never performs the interval lookup: it emits exactly one current schema per category
(always the 3-element form, structs in dependency order — see [overview.md](overview.md)). Only
reading a multi-revision `.brdb` requires revision selection; a `.brz` reader and any writer can
ignore it.

---

## Quirks

- **No date table.** "Schema by date" is an emergent property of the versioned filesystem, not
  a lookup of format versions. There are no hardcoded timestamp → version mappings.
- **Folders are timeless.** Only leaf schema files are resolved at a revision; folder paths are
  assumed stable across revisions.
- **Second granularity.** `created_at` is whole seconds. In practice a chunk and the schema it
  was written against are committed in the same revision (same second), so the `<=` / `>`
  boundaries resolve to that schema.
