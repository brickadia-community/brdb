# Entity and Component .mps Files

`.mps` (MessagePack Schema) files store world chunk data as msgpack-encoded structs. Each file begins with a Structure-of-Arrays (SoA) msgpack blob. Files that carry per-instance typed data append additional msgpack structs after the SoA with no padding or separators between them.

## Binary Structure

```
[SoA msgpack data] [struct_1 msgpack] [struct_2 msgpack] ... [struct_N msgpack]
```

The SoA is read first. For files that carry extra per-instance data (entities, components), the remaining bytes in the buffer are consumed sequentially according to type counters embedded in the SoA. There is no alignment, no length prefix for the extra section, and no separators between structs.

The buffer is consumed sequentially. After reading the SoA, each extra struct is read in order from the remaining bytes.

## File Paths

| Path | Root struct name | Extra data |
|---|---|---|
| `World/0/Bricks/Grids/{grid_id}/Chunks/{x}_{y}_{z}.mps` | `BRSavedBrickChunkSoA` | None |
| `World/0/Bricks/Grids/{grid_id}/Components/{x}_{y}_{z}.mps` | `BRSavedComponentChunkSoA` | Per-component structs |
| `World/0/Bricks/Grids/{grid_id}/Wires/{x}_{y}_{z}.mps` | `BRSavedWireChunkSoA` | None |
| `World/0/Entities/Chunks/{x}_{y}_{z}.mps` | `BRSavedEntityChunkSoA` | Per-entity structs |
| `World/0/Bricks/Grids/{grid_id}/ChunkIndex.mps` | `BRSavedBrickChunkIndexSoA` | None |
| `World/0/Entities/ChunkIndex.mps` | `BRSavedEntityChunkIndexSoA` | None |
| `World/0/GlobalData.mps` | `BRSavedGlobalDataSoA` | None |
| `World/0/Owners.mps` | `BRSavedOwnerTableSoA` | None |

The `{x}_{y}_{z}` segment in chunk paths is the `Display` formatting of a `ChunkIndex`: three i16 values joined by underscores.

## Reading Extra Data

### Entities

Entity chunks embed a `type_counters` array in the SoA. Each counter has a `type_index: u32` and `num_entities: u32`. The reading algorithm is:

1. Read the SoA with `buf.read_brdb(&schema, ENTITY_CHUNK_SOA)`.
2. Iterate `soa.type_counters`. For each counter:
   - Look up `global_data.entity_data_class_names[counter.type_index]` to get `struct_name`.
   - If no entry exists, or `struct_name == "None"`, skip (push `None` entries or continue).
   - Otherwise read `counter.num_entities` consecutive structs from `buf` using `buf.read_brdb(&schema, struct_name)`.

The schema is chosen by the file's creation revision (`entities_schema_rev(found.created_at)`), not the latest.

### Components

Component chunks embed `component_type_counters`. Each counter has `type_index: u32` and `num_instances: u32`. The reading algorithm is:

1. Read the SoA with `buf.read_brdb(&schema, BRICK_COMPONENT_SOA)`.
2. Iterate `soa.component_type_counters`. For each counter:
   - Look up `global_data.component_data_struct_names[counter.type_index]` to get `struct_name`.
   - If `struct_name == "None"`, skip the entire counter (no structs follow for it).
   - Otherwise read `counter.num_instances` consecutive structs from `buf`.

`component_data_struct_names` is a `Vec` (not an `IndexSet`), so it is indexed by position directly.

### Bricks and Wires

Brick and wire chunk files contain only a SoA. There are no type counters and no extra structs after the SoA.

## Writing Extra Data

Writing is the reverse of reading:

1. Serialize the SoA first: `schema.write_brdb(ENTITY_CHUNK_SOA, &self)` returns `Vec<u8>`.
2. Iterate `unwritten_struct_data`. For each `entity_data`:
   - Call `entity_data.get_schema_struct()`. This returns `Option<(type_name, Option<struct_ty>)>`.
   - If the result is `None` or the inner `struct_ty` is `None`, skip. No bytes are written.
   - Otherwise call `write_brdb(&schema, &mut buf, struct_ty.as_ref(), &**entity_data)` to append the struct to the buffer.

Components follow the same pattern. A component whose `get_schema_struct()` returns `Some(_, None)` (a marker-only component with no data struct) is skipped entirely. It contributes to the type counter in the SoA but appends no bytes.

## ChunkIndex

A `ChunkIndex` is a 3D coordinate identifying a chunk in chunk space:

| Field | Type |
|-------|------|
| `x` | i16 |
| `y` | i16 |
| `z` | i16 |

One chunk unit equals 2048 world units (`CHUNK_SIZE = 2048`). The chunk index is formatted as `{x}_{y}_{z}` in file paths (e.g., `0_0_0.mps`).

## ChunkMeta

`ChunkMeta` represents a single entry from a `BRSavedBrickChunkIndexSoA`, read from `ChunkIndex.mps` files.

| Field | Type | Description |
|---|---|---|
| `index` | `ChunkIndex` | The chunk's 3D coordinate |
| `chunk_offset` | `IntVector` (i32,i32,i32) | World-space origin of the chunk |
| `chunk_size` | `i32` | Size of the chunk in world units (typically 2048) |
| `num_bricks` | `u32` | Number of bricks in this chunk |
| `num_components` | `u32` | Number of component instances in this chunk |
| `num_wires` | `u32` | Number of wires in this chunk |

`chunk_offset` and `chunk_size` default to `IntVector::new(1024, 1024, 1024)` and `2048` respectively for old worlds that do not store these fields.

The parallel arrays stored in the SoA are `Chunk3DIndices`, `ChunkOffsets`, `ChunkSizes`, `NumBricks`, `NumComponents`, and `NumWires`.

## Schema Relationship

Each `.mps` file is read using the schema that was current at the time the file was written, identified by the file's creation revision timestamp. Schemas are loaded lazily and cached. The shared schema files for each data category are:

| Data type | Schema path |
|---|---|
| Brick chunks | `World/0/Bricks/ChunksShared.schema` |
| Brick chunk index | `World/0/Bricks/ChunkIndexShared.schema` |
| Component chunks | `World/0/Bricks/ComponentsShared.schema` |
| Wire chunks | `World/0/Bricks/WiresShared.schema` |
| Entity chunks | `World/0/Entities/ChunksShared.schema` |
| Entity chunk index | `World/0/Entities/ChunkIndex.schema` |

## Quirks and Edge Cases

**Legacy entity name fallback.** Older worlds do not store `EntityDataClassNames` in `GlobalData.mps`. When that field is absent, the reader falls back to `lookup_entity_struct_name(entity_type_name)`, which is a hardcoded table mapping a small set of entity type names from Steam Next Fest to their corresponding struct names. New worlds always include `EntityDataClassNames` directly.

**`SavedBrickColor` alpha field.** The alpha channel of the `SavedBrickColor` struct stores the brick's `material_intensity` (a `u8`), not transparency. The field name is historical.

**Entity colors are fixed at 8.** Each entity carries exactly 8 color slots (`Color0`..`Color7`) regardless of entity type, stored as a single `EntityColors` tuple struct.

**Marker-only components.** Some components carry no serializable data struct. Their `get_schema_struct()` returns `Some(type_name, None)`. These components affect type counters in the SoA but contribute zero bytes to the extra data section. On the reading side, `struct_name == "None"` signals this case and the reader skips that counter without consuming any bytes.

**`component_data_struct_names` is positional.** Unlike most name tables in global data which are sets, `component_data_struct_names` is an ordered list indexed by `type_index` position, not looked up by name.

**Sequential buffer consumption.** Both the SoA and extra structs are read from the same byte stream. The decoder advances the position on each read, so extra structs are consumed in strict left-to-right order with no seeking.
