# Shared Schemas and Global Data

## Introduction

`BrdbSchemaGlobalData` is the central registry for a world. It collects all asset type names, entity type names, component type names, and external asset references into ordered index sets. All other data in the format refers to these registries by integer index rather than storing full strings inline, keeping per-entity and per-brick data compact.

The global data is stored in `World/0/GlobalData.{schema,mps}` and is loaded once per world open, then cached behind an `Arc<RwLock<_>>`.

---

## BrdbSchemaGlobalData Fields

| Field | Type | Serialized | Description |
|---|---|---|---|
| `entity_type_names` | `IndexSet<String>` | yes | Registry of all entity type names (e.g. `"Entity_Ball"`). |
| `entity_data_class_names` | `IndexSet<String>` | yes | Registry of entity data class (struct) names. Parallel to `entity_type_names`; populated from a legacy fallback if absent (see Quirks). |
| `basic_brick_asset_names` | `IndexSet<String>` | yes | Registry of basic (static-mesh) brick asset names. |
| `procedural_brick_asset_names` | `IndexSet<String>` | yes | Registry of procedural brick asset names. IDs are offset by `proc_brick_starting_index` at runtime (see below). |
| `material_asset_names` | `IndexSet<String>` | yes | Registry of material asset names. |
| `component_type_names` | `IndexSet<String>` | yes | Registry of component type names. Must stay synchronized with `component_data_struct_names` (same length, same insertion order). |
| `component_data_struct_names` | `Vec<String>` | yes | Struct name for each component type, parallel to `component_type_names`. Entries use the string `"None"` where a component has no data struct. |
| `component_wire_port_names` | `IndexSet<String>` | yes | Registry of wire port names used by components. |
| `external_asset_types` | `HashSet<String>` | **no** | Internal set of known external asset type strings, used only for type checking at runtime. Not written to disk. |
| `external_asset_references` | `IndexSet<(String, String)>` | yes | Ordered registry of `(asset_type, asset_name)` pairs (serialized as `BRSavedPrimaryAssetId`). Referenced by u64 index. |

---

## External Asset References

External assets (sounds, particle effects, textures, etc.) are stored as a `u64` index into `external_asset_references`. The sentinel value `-1` (interpreted as `u64::MAX`) means null / `None`.

**Reading** (index -> asset):
```
index -> external_asset_references.get_index(index) -> (asset_type, asset_name)
```

**Writing** (asset -> index):
```
(asset_type, asset_name) -> external_asset_references.get_index_of(...) -> index
```

The `external_asset_types` field is populated alongside `external_asset_references` in memory (via `add_component_meta`) for runtime type checking, but is reconstructed each time the world is loaded and is never written to the file.

---

## Component Metadata Collection

Components register their metadata into `BrdbSchemaGlobalData` via `add_component_meta(&dyn BrdbComponent)`, which calls three methods on the `BrdbComponent` trait:

| Method | Returns | Effect |
|---|---|---|
| `get_external_asset_references()` | Iterator of `(asset_type, asset_name)` pairs | Inserted into `external_asset_references` and `external_asset_types`. |
| `get_schema_struct()` | `Option<(type_name, Option<struct_name>)>` | Appends to `component_type_names` and `component_data_struct_names` (skipped if type already registered). |
| `get_wire_ports()` | Iterator of port name strings | Extended into `component_wire_port_names`. |

The `component_type_names` / `component_data_struct_names` pair is always written together. Because `component_type_names` is an `IndexSet` that deduplicates on insert, `add_component_meta` returns early for the struct/port registration step if the type name is already present.

---

## Procedural Brick Index Offset

Procedural brick asset IDs stored in chunk data are **not** stored as raw indices into `procedural_brick_asset_names`. Instead, they are offset by the length of `basic_brick_asset_names` at the time the chunk was written:

```
proc_brick_starting_index = basic_brick_asset_names.len()
```

This means a procedural brick with on-disk ID `N` maps to `procedural_brick_asset_names[N - proc_brick_starting_index]`.

The offset exists because basic and procedural brick type IDs share the same integer space in the brick chunk format. If new basic brick assets are added to the game and `basic_brick_asset_names` grows, the offset shifts. Older chunks remain loadable as long as the existing entries in `basic_brick_asset_names` are not reordered.

> **Warning:** External tools that modify `basic_brick_asset_names` (insert, remove, or reorder entries) will invalidate procedural brick references in all existing chunks.

---

## Storage and Caching

- **File path:** `World/0/GlobalData.schema` (schema descriptor) and `World/0/GlobalData.mps` (msgpack data)
- **In memory:** loaded once and stored as `Arc<BrdbSchemaGlobalData>` behind an `RwLock`. Subsequent calls to `global_data()` return a clone of the `Arc` from the cache without re-reading the file.

---

## Quirks

- **`external_asset_types` is not serialized.** It is rebuilt in memory from the data in `external_asset_references` each time `add_component_meta` is called. On read, it is reconstructed by iterating all external asset references and re-inserting their type strings.

- **`component_type_names` and `component_data_struct_names` must stay synchronized.** They are parallel arrays: index `i` in `component_type_names` corresponds to index `i` in `component_data_struct_names`. No length field ties them together at the schema level; deserialization assumes they are the same length.

- **Legacy `entity_data_class_names` fallback.** Older worlds (from around Steam Next Fest) did not serialize `EntityDataClassNames`. On read, if the key is absent from the msgpack struct, the reader falls back to `lookup_entity_struct_name(entity_type)` for each entry in `entity_type_names` to derive the class names. Unknown types map to `"Unknown"`.

- **Asset index bounds checking.** Accessor methods (`basic_brick_asset_by_index`, `procedural_brick_asset_by_index`, `material_asset_by_index`) return `BrdbSchemaError::UnknownAsset` if the index is out of range. Callers must validate indices before use. The u64 sentinel `-1` is never passed to these methods; it is handled at the call site as a null check before indexing.
