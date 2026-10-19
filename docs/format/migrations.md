# Migrating Older Versions

Breaking game updates rename component types, data structs, data fields and gate bricks, and sometimes change how values are stored. A save written before such an update is in an **older version** of the format: it still holds the old names and encodings. [older-versions.md](msgpack-schema/older-versions.md) describes each older version and how to recognise it; this page covers how brdb and brs-js upgrade them.

brdb reads a save in an older version as it is stored, and can upgrade it to the Latest format the same way the game upgrades an old world when it loads it. Code that works on the upgraded world only has to understand the current vocabulary.

---

## Upgrading a world

| brdb | Purpose |
|---|---|
| `migrate::open_latest(&reader)` | A reader over the same world in the Latest format. A Latest world is copied unchanged, so it is safe to call on every world |
| `migrate::to_latest(&reader)` | The upgraded world as a `BrPendingFs` file tree, ready to write with `Brdb::write_pending` or `Brz::write_pending`. The `migrate_world` example converts a save file this way |
| `migrate::needs_migration(&reader)` | Whether a world is in an older version |
| `migrate::pending_steps(&reader)` | The steps a world needs |
| `migrate::steps()` | Every step, oldest first |
| `migrate::component_type(name)` / `migrate::brick_asset(name)` | Map a single older name to its current one |
| `migrate::migrate_value(&value)` | Reshape a single decoded older value |

```rust
use brdb::{Brdb, IntoReader, migrate};

let world = migrate::open_latest(&Brdb::open("old.brdb")?.into_reader())?;
let data = world.global_data()?; // current component type names
```

The plain reader methods (`global_data`, `component_chunk`, …) still return a save exactly as stored, in its own version.

---

## Steps

Each breaking game update is one **step** in `crates/brdb/migrations.json`. brs-js reads the same file (synced into `src/brdb/migrations.ts`), so both libraries always migrate the same way.

A step says how to recognise a save from before its update, and lists the renames and value conversions it needs. Steps are listed oldest first. A world runs the first step that recognises it and every step after it, so a save from several updates ago upgrades in one pass.

| Field | Meaning |
|---|---|
| `name` | Short identifier, e.g. `"ea3"` |
| `description` | Which older version the step upgrades |
| `detect` | How to recognise a save from before the step. Every condition given must hold for the save's `ComponentsShared.schema`: `componentSchemaFormat` (`"EA3"` or `"Latest"`, the schema's [container](msgpack-schema/older-versions.md#telling-versions-apart)) and/or `componentSchemaHasStruct` (a struct only saves from before the step declare) |
| `componentTypes` | `renames` (old -> new component type), then `prefixes` (`[from, to]` prefix replacements, tried in order) |
| `bricks` | `renames` (old -> new brick asset, basic or procedural) and `toGate` (basic gate brick -> `PB_Gate_*` procedural gate brick) |
| `fields` | Renamed data struct fields, as `{component, from, to}`; `component` is the type's name after the step |
| `values` | Named value conversions, implemented under the same name in brdb (`src/migrate/values.rs`) and brs-js (`src/brdb/migrate.ts`) |

Detection never uses a game build number.

---

## The `ea3` step

The only step so far upgrades the **EA3** version: saves from retail EA3 builds and every save written before the wire and component rewrite. It recognises them by an EA3 (array-container) component schema.

- **Component types:** the `BrickComponentType_WireGraphPseudo_` and `BrickComponentType_WireGraph_` prefixes become `BrickComponentType_` (for example `BrickComponentType_WireGraph_Expr_LogicalAND` -> `BrickComponentType_Expr_LogicalAND`), plus explicit renames such as `BrickComponentType_WireGraphPseudo_Timer` -> `BrickComponentType_Stopwatch`.
- **Bricks:** gate bricks are renamed to their current assets (for example `B_1x1_Gate_DeltaTime` -> `B_1x1_Gate_Expr_DeltaTime`). Gates that became procedural become their `PB_Gate_*` brick at its largest size, the one exposing every port.
- **Values (`ea3WireGraph`):** EA3 wire arrays become `dynamic_array` values, wire maps become `dynamic_map` values, wrapped enums become `dynamic_enum` values, and `wire_graph_variant` values become plain values. See [older-versions.md](msgpack-schema/older-versions.md#ea3-format) for the EA3 encodings.

---

## What an upgraded world contains

Files whose meaning did not change keep their original encoding, since every game build reads the older versions:

- `GlobalData.mps`: component type, data struct and brick asset names are renamed; gates that became procedural bricks add their `PB_Gate_*` asset.
- `ComponentsShared.schema` and the component chunks: data is re-encoded into the current structs.
- Brick chunks: rewritten only when they hold a brick whose asset changed index.

The upgrade matches what the game does when it loads an older world, with one difference: it is lossless where the game drops data on load. It keeps wires to ports that no longer exist, components on bricks that no longer allow them, and gate output values. Data with no place in the current format is dropped, as the game drops it: data fields a step removes, and the data of component types the game no longer has (their components are kept, without data).

---

## Writing older versions

brdb writes worlds in the Latest format only. Writing a world as EA3 (`World::schema_format = BrdbSchemaFormat::EA3`) fails with `BrdbWorldError::Ea3WriteUnsupported`; to write worlds for retail EA3 builds, use brdb 0.11. A single schema can still be written in the EA3 container with `write_as(BrdbSchemaFormat::EA3)`, as long as it has no `types` table and uses no dynamic container.
