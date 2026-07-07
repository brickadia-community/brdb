# Prefab Bundles and Embedded Prefabs

A prefab (`Saved/Prefabs/*.brz`) is an ordinary BRZ archive — the container
format is identical to a world `.brz` (see [File Tree Structure](file-tree.md)).
What differs is the virtual file tree it carries. 


## Prefab file tree

```
Meta/
  Bundle.json          "type": "Prefab"
  Prefab.json          pivots, addedGlobalGridOffset, grid flags
  Thumbnail.png        always written by the game (no Screenshot.jpg)
World/0/...            identical layout to world bundles
Prefabs/               PRESENT ONLY when this bundle references other prefabs
  Uploads/
    <HASH>.brz         an embedded prefab archive (see below)
```

Compared to a world bundle, a prefab's `Meta/` has `Prefab.json` instead of
`World.json`, and no `Screenshot.jpg`. Everything under `World/0/` (grids,
chunks, components, wires, entities, schemas, global data, owners) is
unchanged.

## Bundle.json quirks

The game serializes `authors` as objects and always includes a `color`:

```json
{
  "type": "Prefab",
  "authors": [{ "iD": "<guid>", "name": "<name>" }],
  "color": { "r": 1, "g": 1, "b": 1, "a": 1 },
  ...
}
```

`color` components are floats in `0..=1` (whole numbers for white). Bundles
written by this crate before 0.7 stored `authors` as plain strings and omitted
`color`; the reader accepts both shapes.

## Embedded prefabs (`Prefabs/Uploads/`)

When a brick carries a prefab-spawning component, the referenced prefab is
**embedded byte-identically** into the referencing bundle:

- The file is stored at `Prefabs/Uploads/<HASH>.brz`, where `<HASH>` is the
  **uppercase-hex BLAKE3 hash of the complete embedded file's bytes**
  (content addressing — identical prefabs dedupe to one entry). This name is
  **required, not just a convention**: a bundle whose embedded prefab is stored
  under any other filename (verified: a readable name inside `Prefabs/Uploads/`)
  **crashes the game on load**. Always name embedded prefabs by their BLAKE3
  hash — this is exactly what `World::add_prefab` does.
- The embedded file is a complete, standalone prefab archive: parsing it
  yields another bundle with its own `Meta/`, `World/0/`, and — if it
  references further prefabs — its own `Prefabs/` folder. Recursion is by
  nesting, never by flattening.
- Worlds can carry a `Prefabs/` folder too (any save containing a prefab
  spawner); this is a bundle-level feature, not a prefab-only one.

Example, observed in a real spawner prefab: the referenced `1x1f brick.brz`
(BLAKE3 `74E5…AE3C`) is embedded unmodified at
`Prefabs/Uploads/74E546B5A2E4688A5F887B103DBF97F0FB410431943399A4177E50049D12AE3C.brz`.

## Component references (`bundle_path_ref`)

Prefab-spawning component data structs reference the embedded file through a
`Prefab` property of schema type `bundle_path_ref` — serialized as a plain
string holding the **root-relative path, no leading slash**:

```
BrickComponentData_PrefabSpawn                    { Prefab: bundle_path_ref, ... }
BrickComponentData_WireGraph_Exec_PrefabSpawner   { Prefab: bundle_path_ref, ... }
```

e.g. `"Prefabs/Uploads/74E5…AE3C.brz"`.

## Library support

Writing (`World` wrapper):

```rust
let mut outer = World::new();
outer.register_all_components();
// content-addresses, stores, and returns "Prefabs/Uploads/<HASH>.brz"
let path = outer.add_prefab_world(&inner_world)?; // or add_prefab(bytes)
outer.bricks.push(
    Brick { asset: BrickType::str("B_1x1_Gate_Exec_PrefabSpawner"), ..Default::default() }
        .with_component(
            LiteralComponent::new("BrickComponentType_WireGraph_Exec_PrefabSpawner")
                .with_data([("Prefab", Box::new(path) as Box<dyn AsBrdbValue>)]),
        ),
);
outer.make_prefab();
outer.write_brz("spawner.brz")?;
```

Reading (`BrReader` accessors, work for both `.brz` and `.brdb`):

```rust
let reader = Brz::open("spawner.brz")?.into_reader();
let bundle = reader.bundle_json()?;          // Meta/Bundle.json
let prefab = reader.prefab_json()?;          // Meta/Prefab.json (None for worlds)
let meta = reader.world_meta()?;             // assembled WorldMeta
for path in reader.prefab_paths()? {         // Prefabs/** enumeration
    let inner = reader.open_prefab(&path)?.into_reader(); // nested bundle
}
let all = reader.read_prefabs()?;            // path -> bytes, assignable to World::prefabs
```

## Quirks

- **The `Prefabs/` folder is omitted entirely when empty.** A prefab with no
  prefab-spawning references has no `Prefabs/` folder at all.
- **Root folder order is `Meta`, `World`, `Prefabs`.** Order defines archive
  ids in the BRZ index; the writers preserve it.
- **Embedding is verbatim.** The embedded bytes equal the referenced `.brz`
  file exactly (verified against game output), so the BLAKE3 filename can be
  recomputed from the embedded content.
- **`Meta/Prefab.json` `addedGlobalGridOffset`** is the grid offset the game
  recorded when the prefab was saved; generated prefabs may leave it zero.
