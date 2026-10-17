# Changelog

All notable changes to the `brdb` crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/) from 1.0.0 on.

## 1.0.1 - 2026-10-15

### Added

- `schema::{BrickRefArray, ComponentRefArray}`: brick and component ref
  arrays, for an `ArrayVar`'s `Value`. An array of arrays is a
  `ComponentRefArray` of refs to other `ArrayVar`s.
- `TryFrom<&BrdbValue>` for `SavedBrickRef` and `SavedComponentRef`.
- `schema::handle_struct`, the struct a handle type is stored as.

### Fixed

- `brick_handle` and `component_handle` values are now read and written, so
  saves with populated component arrays decode. Refs are stored as given.
  To reach a brick in another chunk, write the absolute form: the grid's
  persistent index (1 for the main grid), the brick's own chunk, and its
  index in that chunk. Grid 0 with chunk (0,0,0), which the game writes,
  only reaches the array's own chunk.

## 1.0.0 - 2026-10-15

Brickadia's current game builds rewrote the wire and component system. Saves
written by those builds use a new schema container, a new wire value type and
renamed component types. This release targets those builds:

- **brdb now speaks only the current ("Latest") vocabulary.** `COMPONENTS`,
  `LogicGate`, `BufferTicks` and the bundled schemas all describe current game
  builds, and wire values are `WireValue`.
- **EA3 saves are migrated on read.** EA3 is the format of retail EA3 builds
  and of every save written before the rewrite. The new `migrate` module
  upgrades an EA3 world into a Latest world the same way the game does when
  it loads one, so your code only has to handle one format.
- **Writing EA3 worlds is no longer supported.** To write worlds for retail
  EA3 builds, keep using brdb 0.11.

### Reading EA3 saves

`migrate::open_latest` takes any reader and returns a reader over the same
world in the Latest format. A Latest world is copied as is, so you can call
it on every world you open:

```rust
use brdb::{Brdb, IntoReader, migrate};

let world = migrate::open_latest(&Brdb::open("old.brdb")?.into_reader())?;
let data = world.global_data()?; // current component type names
```

What the migration changes:

- **Names:** component types and data structs get their current names (for
  example `BrickComponentType_WireGraph_Expr_LogicalAND` →
  `BrickComponentType_Expr_LogicalAND`, `WireGraphPseudo_Timer` →
  `Stopwatch`).
- **Gate bricks:** gate bricks are renamed to their current assets. Gates that
  became procedural (queues, custom events, function calls, …) become their
  `PB_Gate_*` brick at the largest size, which exposes every port.
- **Wire values:** EA3 wire arrays become `dynamic_array` values, wire maps
  become `dynamic_map` values, and wrapped enums become `dynamic_enum` values.
- **Everything else** keeps its EA3 encoding, which every game build reads.

It was checked against the game itself: migrating an EA3 world containing every
component gives the same bricks, components and wires as the game's own
upgrade of that world. A few things differ on purpose, because the migration
is lossless where the game drops data on load. It keeps:

- wires to ports that no longer exist;
- components on bricks that no longer allow them;
- gate output values, which the game resets to their typed default;
- the bot `bColorsAreLinear` flag, which the game normalises.

Data with no place in the current format is dropped, as the game drops it:
data fields a step removes, and the data of component types the game no
longer has (their components are kept, without data).

Lower-level pieces, if you need them:

- `migrate::to_latest(&reader)` returns the migrated world as a
  `BrPendingFs` file tree, ready to write with `Brdb::write_pending` or
  `Brz::write_pending`. The `migrate_world` example converts a save file this
  way.
- `migrate::needs_migration(&reader)` tells you whether a world is in an
  older format, and `migrate::pending_steps(&reader)` lists the steps it
  needs.
- `migrate::component_type(name)` and `migrate::brick_asset(name)` map
  single older names.
- `migrate::migrate_value(&value)` reshapes a single decoded older value.

Migrations are data. Each breaking game update is one step in the crate's
`migrations.json`. A step says how to recognise a save from before that
update, and lists the component types, bricks and data fields it renames,
plus any value conversions it needs. A world runs the first step that
recognises it and every step after it, so a save from several updates ago
upgrades in one go. brs-js reads the same file, so both libraries always
migrate the same way. `migrate::steps()` lists the steps.

The plain reader methods (`global_data`, `component_chunk`, …) still return
a save exactly as stored. Tools that edit a save in place keep working on EA3
saves.

### Migrating from 0.11

| 0.11 | 1.0 |
|---|---|
| `schema::WireVariant` | `WireValue` for wire values. The EA3 codec type is now `EA3WireVariant`. |
| `WireArrayVariant`, `WireMapVariant`, `WireMapKey`, `WireMapValue`, `WireMapKeyData`, `WireMapValueData` | removed; Latest saves use `BrdbValue::DynamicArray` / `BrdbValue::DynamicMap` |
| `BrdbValue::WireVar` | `BrdbValue::EA3WireVar`. Only the oldest saves produce it; convert with `WireValue::try_from`. |
| `AsBrdbValue::as_brdb_wire_variant`, `as_brdb_wire_variant_ref` | `as_brdb_ea3_wire_variant`, `as_brdb_ea3_wire_variant_ref` |
| `AsBrdbValue::as_brdb_wire_array_variant`, `as_brdb_wire_map_variant` | removed; implement `as_brdb_dynamic_array` / `as_brdb_dynamic_map` |
| `LogicGate` | same name, now `BrickComponentType_Expr_*` types with `WireValue` inputs |
| `BufferTicks`, `BufferSeconds` | same names, current types, and new fields: `zero_*_to_wait`, `buffered`, `has_queued`, `is_off_timer` |
| `COMPONENTS`, `component`, `component_db::*`, `schemas::*`, `assets::external::*` | same names, now the current build's. External audio constants are renamed `BOSA_*` → `OSA_*`, and the smoke grenade assets are gone. |
| writing for retail EA3 | not supported; keep brdb 0.11 for that |

### Other breaking changes

- `BrdbSchemaMeta` gained a fourth element, the `types` table. This affects
  `BrdbSchema::from_meta` (new `types` argument), `parse_to_meta`,
  `read_to_meta`, `extract_struct_meta` and `extract_structs_transitive`. To
  merge meta, use the new `add_schema_meta` instead of
  `add_meta` + `add_variants`.
- `BrdbValue` gained `DynamicArray` and `DynamicMap`, and `WireVar` was
  renamed to `EA3WireVar`; exhaustive matches need updating.
- `BrdbSchemaError` gained `InvalidTypeIndex`, `IntegerOutOfRange` and
  `UnsupportedType`. `BrdbWorldError` gained `Ea3WriteUnsupported`,
  `MigrationDuplicateName`, `MigrationMergesComponentType` and
  `BrickSizeCountersExceedSizes`.
- `World` writes every schema in the map form by default, as the game
  writes them: `World::schema_format` defaults to `Latest`. Before, worlds
  used the array form wherever a schema allowed it. `BrdbSchemaFormat::Auto`
  still writes that way, but a world written so without components reads as
  an EA3 save to `migrate::needs_migration`.
- New public fields: `World::schema_format`, `UnsavedWorld::schema_format`,
  `BrdbSchemaGlobalData::{prefab_spawn_instance_indices, prefab_spawn_instance_parent_indices}`
  and `EntityChunkSoA::prefab_spawn_instance_indices`. Struct literals without
  `..Default::default()` need the new fields.
- `BrdbSchema::write` / `to_bytes` now write a schema in the container it
  needs: the map form when it has a `types` table or dynamic containers,
  otherwise the array form. Before, they always wrote the array. Use
  `write_as` / `to_bytes_as` to choose.

### Added

- The `migrate` module and its `migrations.json` steps (see above), plus the
  `migrate_world` and `migration_diff` examples. `migration_diff --renames`
  infers renames between an old world and the game's re-save of it, which is
  how a new step's renames are gathered.
- `schema::BrdbSchemaFormat` (`Auto`, `EA3`, `Latest`), plus the
  `BrdbSchema` methods `format`, `source_format`, `set_format`, `is_latest`,
  `resolve_format`, `needs_map_form`, `write_as` and `to_bytes_as`.
  `World::schema_format` picks the container worlds are written in (default
  `Latest`); `EA3` returns `Ea3WriteUnsupported`.
- The schema `types` table: `BrdbSchema::types`, `add_types`, `type_at`,
  `type_index` and `type_names`. Plaintext `.schema` files accept a
  `types { … }` block.
- `schema::{WireValue, SavedBrickRef, SavedComponentRef}`. `WireValue`
  converts from decoded values of either format with `WireValue::try_from`.
- `schema::{BrdbDynamicArray, BrdbDynamicMap, BrdbDynamicEnum}` and the
  `DYNAMIC_ARRAY`, `DYNAMIC_MAP` and `DYNAMIC_ENUM` type-name constants.
- Procedural gate bricks (`PB_Gate_*`, whose brick size selects which ports
  are on the brick): `assets::components::{PROCEDURAL_GATES, ProceduralGate,
  GateVariant, procedural_gate, procedural_gate_for_component}`.
- `UnsavedGrid::to_pending_with`, which takes explicit brick, chunk-index and
  wire schemas.
- `BrzIndex` is now public; it is the reader type of an in-memory `.brz`.
- Latest-only save data is read and written: the GlobalData
  `PrefabSpawnInstanceIndices` / `PrefabSpawnInstanceParentIndices` tables and
  the per-entity `PrefabSpawnInstanceIndices`.
- The `read_all` example decodes every file in a world and checks that each
  one round-trips through the writer.
- Half-extents for the bricks added in current builds.

### Changed

- Component catalogs, component tables, external assets and the bundled
  `schemas/*.schema` files are regenerated from a current game build.
- Number decoding is more lenient. Integer fields accept any integer tag and
  cast it to the field type. `f32` and `f64` fields accept any integer or
  float tag. Before this release, `f32` only accepted small integers and
  `float 32`.
- When a struct value read with one schema is written with another (for
  example, copying data from a save into a world that uses the built-in
  schema), its fields are matched by name instead of by intern id.

### Fixed

- The `write_wire` example used gate types that did not match the component
  tables it registered.

### Known gaps

- The `Bool` and `Str` wire value members are assumed to encode like `bool`
  and `str`; no saved instance has been seen yet.
- `brick_handle` and `component_handle` values have no known encoding.
  Reading or writing one is an error, but empty containers of them round-trip.
- The format docs in `docs/format/msgpack-schema/` still describe only the EA3
  container.

## Earlier releases

Releases up to 0.11.0 were not tracked in a changelog; see `git log`.
