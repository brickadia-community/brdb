# Older Format Versions

Brickadia has changed its save format several times during development. brdb reads every version on this page, but it writes only the current one, described in [overview.md](overview.md). This page covers how to tell the versions apart, what each one encodes differently, and what brdb does with a save in an older version.

---

## At a glance

From oldest to newest:

| Version | Schema container | Wire values | Dynamic containers |
|---|---|---|---|
| Before EA3.2 | `[enums, structs]` | built-in `wire_graph_variant` and `wire_graph_prim_math_variant` | no |
| EA3.2 to the rewrite | `[enums, variants, structs]` | named variants (`WireGraphVariant`, ...) declared in the schema | no |
| Latest | `{enums, variants, structs, types}` map | `WireValue` | yes |

brdb calls both array versions `BrdbSchemaFormat::EA3`, since they share their value encodings and differ only in whether the schema has a variant table. `BrdbSchemaFormat::Latest` is the last row.

EA3 is the format of retail EA3 builds and of every save written before the game rewrote its wire and component system. The rewrite introduced the Latest format, and it also renamed component types, data structs and gate bricks (see [Names](#names)).

---

## Telling versions apart

The schema container decides the version, so the first byte of any `.schema` file is enough:

| First byte | Container | Version |
|---|---|---|
| `0x92` | 2-element array | before EA3.2 |
| `0x93` | 3-element array | EA3.2 to the rewrite |
| `0x80`..`0x8f`, `0xde`, `0xdf` | map | Latest |

Any other array length is an `InvalidHeader` error. In brdb, `BrdbSchema::read_to_meta_with_format` reports the container it read, and `migrate::needs_migration(&reader)` says whether a whole world is in an older format.

A `.brdb` keeps old revisions of its files (see [schema-revisions.md](schema-revisions.md)), so one `.brdb` can hold chunks in both formats. Decode each chunk with the schema selected for its own revision, and use that schema's format for its values.

---

## EA3 format

EA3 schemas are recognised by their container, and the values they decode follow EA3 rules.

### Container

An EA3 `.schema` file is a msgpack array:

```
[enums, structs]              # before EA3.2: no variant table
[enums, variants, structs]
```

Each element is encoded like the matching Latest section. The array form cannot hold a `types` table, so EA3 schemas have no dynamic containers and no `dynamic_enum`.

### Value differences

| | EA3 | Latest |
|---|---|---|
| `weak_object` | the asset index is stored and read back | written as -1, read as null |
| `wire_graph_variant` selector 3 | followed by an i64 asset index | no payload |
| `u8` 128..=255 | written as -128..=-1 (the only form EA3 builds read) | written unsigned |
| Flat array structs | fields packed back to back | C layout; see [flat-arrays.md](flat-arrays.md) |

### Built-in wire unions

The oldest saves use two unions that are not declared in the schema. Their selector layout is fixed:

| Type | Selectors |
|---|---|
| `wire_graph_variant` | 0 `f64`, 1 `i64`, 2 `bool`, 3 object, 4 exec (no payload) |
| `wire_graph_prim_math_variant` | 0 `f64`, 1 `i64` |

The spec freezes `wire_graph_variant` as `[f64, i64, bool, nil, nil]`, so selector 3 has no payload. EA3 data stores an i64 asset index after selector 3 anyway, and a reader must consume it or the rest of the struct misaligns. Any other selector is an `UnknownWireVariant` error.

### Named wire unions

Saves from EA3.2 on declare their wire unions in the variant table. The last EA3 builds declare:

| Variant | Alternatives |
|---|---|
| `WireGraphVariant` | `f64, i64, bool, weak_object, WireGraphExec, Vector, Rotator, Quat, str, LinearColor, WireGraphEnumWrapper` |
| `WireGraphPrimMathVariant` | `f64, i64, Vector, Rotator, Quat, LinearColor` |
| `WireGraphArrayVariant` | `WireGraphDoubleArray, WireGraphInt64Array, WireGraphBoolArray, WireGraphObjectArray, WireGraphVectorArray, WireGraphRotatorArray, WireGraphQuatArray, WireGraphStringArray, WireGraphLinearColorArray, WireGraphEnumArrayWrapper` |

They are joined by per-enum array and map families (`WireGraphEnumArrayVariant`, `WireGraphEnumVariant`, `WireGraphMapVariant`, `WireGraphMapVariant_{int64,FWeakObjectPtr,FWireGraphString}`, `WireGraphEnumMapVariant_*`). Each array alternative is a struct with a single `Values` array field, for example `WireGraphEnumArray_EBrickAxis { Values: EBrickAxis[] }`. Earlier EA3 builds declared shorter lists, which is why selectors must always be read against the save's own schema.

In the Latest format these become `WireValue` (single values), `dynamic_array` (arrays), `dynamic_map` (maps) and `dynamic_enum` (wrapped enums).

---

## Names

The rewrite that introduced the Latest format also renamed much of what a save refers to by name:

- **Component types and data structs**, for example `BrickComponentType_WireGraph_Expr_LogicalAND` became `BrickComponentType_Expr_LogicalAND`, and `WireGraphPseudo_Timer` became `Stopwatch`.
- **Gate bricks** moved to new assets. Gates that became procedural (queues, custom events, function calls and others) use a `PB_Gate_*` brick.
- **Data fields** were renamed or removed on some components.

An older save still uses its old names, so a reader that looks up current names finds nothing. The full lists are data, in `crates/brdb/migrations.json` (see [migrations.md](../migrations.md)).

---

## Changes within one version

Game builds also change the format without changing its version. They add components, add or remove fields on data structs, and add alternatives to variants. For example, a later Latest build made change and edge detectors store `InputLast` and `bSeeded` instead of their outputs.

Every save carries the schema it was written with, so a reader that decodes with the save's own schema handles these changes: a field added later is missing from an older save, and a removed field is still there. A reader that assumes the current layout misreads them.

Smaller encodings that older writers used, and that readers still accept:

- **`bundle_path_ref`**, an alias of `str`. Latest writers write `str` in its place.
- **Negative `u8`**: values 128..=255 stored as -128..=-1, as in the EA3 table above. Readers add 256.
- **No `EntityDataClassNames`**: worlds from around Steam Next Fest leave this field out of `GlobalData`. Readers derive the class names from the entity type names instead; see [shared-schemas.md](shared-schemas.md). `GlobalGridEntityTypeIndex` can be missing too; readers check whether it is present.

---

## Migrating older saves

brdb reads a save in an older version as it is stored, and can upgrade it to the Latest format with `migrate::open_latest` or `migrate::to_latest`. It writes the Latest format only. [migrations.md](../migrations.md) covers the upgrade: the entry points, the `migrations.json` step format, what the EA3 step changes, what an upgraded world keeps, and writing older versions.
