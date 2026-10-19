# msgpack-schema Overview

## Introduction

BRDB uses a schema format built on [MessagePack](https://msgpack.org/) to describe the data in `.mps` files. Each `.mps` file is decoded with a `.schema` file that sits beside it. The schema says what fields a struct has, what values an enum has, what alternatives a variant has, and how each field is encoded.

The format follows [Zeblote's msgpack-schema draft spec](https://gist.github.com/Zeblote/053d54cc820df3bccad57df676202895). The game writes a few things the spec does not describe; see [Differences from the spec](#differences-from-the-spec).

There are two generations of the format:

| | **Latest** | **EA3** |
|---|---|---|
| Written by | current game builds | retail EA3 builds, and every save before the wire and component rewrite |
| Schema container | a map of named sections | a 2- or 3-element array |
| `types` table and dynamic containers | yes | no |
| Value encodings | as described below | a few differ; see [older-versions.md](older-versions.md#ea3-format) |

brdb calls these `BrdbSchemaFormat::Latest` and `BrdbSchemaFormat::EA3`. A schema's format is the container it was read from, and it decides how the values that schema decodes are encoded. This page describes Latest; [older-versions.md](older-versions.md) covers EA3 and what differs.

---

## Schema contents

A schema has four sections:

| Section | Contents |
|---|---|
| `enums` | enum name -> (value name -> integer value) |
| `variants` | variant name -> ordered list of alternative type names. See [Variants](#variants) |
| `structs` | struct name -> ordered (field name -> field descriptor) |
| `types` | an ordered list of type names, indexed by dynamic containers. See [The types table](#the-types-table) |

Enum value names are written fully qualified, e.g. `EBrickAxis::X`.

In memory, brdb also interns every name in a string pool and attaches the world's global data (external asset references; see [shared-schemas.md](shared-schemas.md)). Neither is part of the `.schema` file.

---

## Schema files

### Latest: the map container

A Latest `.schema` file is a msgpack map from section name to section:

```
{ "enums": {...}, "variants": {...}, "structs": {...}, "types": [...] }
```

- Every section is optional. Writers leave out empty sections, and readers treat a missing section as empty. `{}` (the single byte `0x80`) is a valid, empty schema.
- Section order does not matter.
- A duplicate section or a section with any other name is an error.

Section encodings:

- `enums`: a map of `enum_name -> { value_name -> int }`.
- `variants`: a map of `variant_name -> [type_name, ...]`.
- `structs`: a map of `struct_name -> { field_name -> field_descriptor }`. A struct must appear after every struct it uses; brdb writes structs in dependency order.
- `types`: an array of type name strings. `dynamic_array` and `dynamic_map` may not appear in it.

### Field descriptors

Each struct field is one of four kinds, written as:

| Kind | Descriptor in the `.schema` file | Value encoding |
|---|---|---|
| `Type(T)` | the string `T` | one value of type `T` |
| `Array(T)` | `[T]` (1-element array) | msgpack array header, then each element |
| `FlatArray(T)` | `[T, nil]` (2-element array) | one msgpack `bin` blob; see [flat-arrays.md](flat-arrays.md) |
| `Map(K, V)` | `{K: V}` (1-entry map) | msgpack map header, then each key and value |

### Plaintext form

brdb also reads and writes a plaintext form of schemas (`crates/brdb/schemas/*.schema` are stored this way):

```
enum EColor {
    EColor::Red = 0,
    EColor::Green = 1,
}
variant WireValue {
    Bool,
    i64,
    f64,
}
struct Holder {
    Scalar: i64,
    List: i64[],
    Packed: u8[flat],
    Lookup: {str: i64},
    Values: dynamic_array,
}
types {
    EColor,
    i64,
    f64,
}
```

---

## Value encoding

Values are written back to back with no separators. Their order and types come entirely from the schema.

### Structs

A struct value is its field values in schema order, **with no header**: no map, no array length and no field names. For example, a `BRSavedBrickRef { GridPersistentIndex: u32, ChunkIndex: BRSavedChunk3DIndex, BrickIndexInChunk: u32 }` holding grid 0, chunk (0,0,0) and brick 297 is the six bytes `00 00 00 00 cd 01 29`.

### Enums

An enum value is an unsigned integer: the enum value itself. brdb rejects a value that is not less than the number of values the enum declares.

### Built-in types

| Type | Encoding |
|---|---|
| `nil` | nothing: the field takes zero bytes |
| `bool` | msgpack bool |
| `u8`, `u16`, `u32`, `u64` | msgpack integer |
| `i8`, `i16`, `i32`, `i64` | msgpack integer |
| `f32`, `f64` | msgpack float or integer |
| `str` | msgpack string (UTF-8) |
| `class`, `object` | integer index into the world's external asset references; negative means none. See [shared-schemas.md](shared-schemas.md) |
| `weak_object` | integer, always written as `-1`; the reader discards it and the reference loads as null |
| `bundle_path_ref` | legacy alias of `str`. Latest schemas are written with `str` in its place |
| `dynamic_array`, `dynamic_map` | see [Dynamic containers](#dynamic-containers) |
| `dynamic_enum` | see [`dynamic_enum`](#dynamic_enum) |

Two more names are not declared in schemas but appear in them:

| Type | Encoding |
|---|---|
| `Bool`, `Str` | aliases of `bool` and `str`, used in `WireValue` and the `types` table. brdb assumes they encode the same way; no saved instance has been seen |
| `brick_handle`, `component_handle` | an inline `BRSavedBrickRef` / `BRSavedComponentRef` struct with no tag. See [Handles](#handles) |

Any other type name must be an enum, variant or struct in the schema. An unknown name is an `UnknownType` error.

### Numbers

- **Integers** are written in the smallest msgpack form that holds the value. Readers accept any integer tag and cast it to the field type.
- **Floats:** readers accept any integer or float tag for `f32` and `f64`. brdb writes a whole-number float as an integer when it fits (`f32`: within the 16-bit range, `f64`: within the 32-bit range) and as a msgpack float otherwise.
- **`u8` 128..=255:** older writers stored these as -128..=-1. Readers add 256 to a negative `u8`, so `0xff` (-1) reads as 255 and `d0 80` (-128) reads as 128. Latest writers store the unsigned value (`cc c8` for 200).

---

## Variants

A variant is a tagged union. Its schema entry is an ordered list of alternative type names.

```
uint(selector) + value(alternatives[selector])
```

The selector is the 0-based index of the alternative in the **saved** schema's list. The payload is encoded as that alternative's type on its own, so a primitive, asset index, struct, enum or another variant. A struct alternative with no fields, such as `WireGraphExec`, has no payload.

### Matching alternatives by type

The saved and current schemas can list different alternatives in a different order. A reader decodes the payload with the saved alternative's type, then matches that alternative to the current schema's list **by type, not by position** (`Bool` matches `bool`, `Str` matches `str`). An alternative that the current schema has removed is still decoded with its saved type, so the fields after it stay aligned. Writing such a value with the current schema is an error.

### `WireValue`

Wire values (gate inputs, outputs and stored values) are the `WireValue` variant. Current game builds declare it as:

| Selector | Alternative | Payload |
|---|---|---|
| 0 | `Bool` | bool |
| 1 | `i64` | integer |
| 2 | `f64` | float |
| 3 | `LinearColor` | struct `{R, G, B, A: f32}` |
| 4 | `Vector` | struct `{X, Y, Z: f64}` |
| 5 | `Rotator` | struct `{Pitch, Yaw, Roll: f64}` |
| 6 | `Quat` | struct `{X, Y, Z, W: f64}` |
| 7 | `str` | string |
| 8 | `dynamic_enum` | types index, then the enum value |
| 9 | `weak_object` | integer (`-1`) |
| 10 | `BRSavedBrickRef` | struct, see [Handles](#handles) |
| 11 | `BRSavedComponentRef` | struct, see [Handles](#handles) |
| 12 | `WireGraphExec` | none (empty struct) |

Selectors are only meaningful against the schema that wrote them. Read the alternative list from the save's own schema, not from this table.

Arrays and maps of wire values are not variants any more. They are `dynamic_array` and `dynamic_map` fields, for example `BrickComponentData_ArrayVar.Value` and `BrickComponentData_MapVar.Value`.

Other variants in current saves include `BRInventoryEntryVariant` (`BRInventoryEntryNothing`, `BRInventoryEntryBrick`, `BRInventoryEntryEntity`, `BRInventoryEntryItem`).

---

## The types table

The `types` section is an ordered list of type names: built-in types, enums, variants or structs. Dynamic containers refer to its entries by 0-based index.

- An index is an unsigned msgpack integer from 0 to 4,294,967,295. An index past the end of the table is an `InvalidTypeIndex` error.
- brdb treats the table as append-only: `add_types` only appends, and a name already in the table keeps its index.
- `dynamic_array` and `dynamic_map` cannot be entries, so dynamic containers cannot nest. `WireValue` can be an entry, so a container can hold arbitrary wire values.

Current game builds write a table of the wire-graph enums (`EBrickDirection`, `EBrickAxis`, …, `EWireKind`) followed by `f64, i64, Bool, Vector, Rotator, Quat, LinearColor, str, weak_object, brick_handle, component_handle, WireValue`.

### Dynamic containers

`dynamic_array` and `dynamic_map` are field types whose element (or key and value) types are chosen per value:

```
dynamic_array = uint(element_type_index) + array_header(n) + n * value(element_type)
dynamic_map   = uint(key_type_index) + uint(value_type_index) + map_header(n) + n * (value(key_type) + value(value_type))
```

- The type indices are written even when the container is empty. An empty `Str` array with `Str` at index 2 is `02 90`.
- A later map entry with the same key replaces the earlier one.
- brdb writes an unset dynamic field as an empty container: `f64` elements for an array, `i64 -> f64` for a map.

### `dynamic_enum`

An enum value tagged with its enum type. The game writes it and the spec does not describe it:

```
dynamic_enum = uint(enum_type_index) + uint(value)
```

The indexed entry must be an enum. `dynamic_enum` is a `WireValue` alternative, and fields such as `BCSComponentVertex_Expr_IntegerToEnum.TargetType` use it.

### Handles

`brick_handle` and `component_handle` are reference types used as dynamic container element, key and value types. Their payload is an inline struct with no tag:

| Handle | Payload struct |
|---|---|
| `brick_handle` | `BRSavedBrickRef { GridPersistentIndex: u32, ChunkIndex: BRSavedChunk3DIndex, BrickIndexInChunk: u32 }` |
| `component_handle` | `BRSavedComponentRef { Brick: BRSavedBrickRef, ComponentTypeIndex: u16 }` |

`BRSavedChunk3DIndex` is `{X, Y, Z: i16}`. An array of arrays is a `component_handle` array whose elements point at other `ArrayVar` components.

The game writes grid `0` with chunk `(0, 0, 0)` to mean the array's own chunk. To reach a brick in another chunk, write the absolute form: the grid's persistent index (1 for the main grid), the brick's own chunk, and its index in that chunk.

---

## Older versions

EA3 saves, from retail EA3 builds and every build before the wire and component rewrite, use an array container, no `types` table, and a few different value encodings. brdb reads them and can migrate them to Latest, but it no longer writes EA3 worlds. [older-versions.md](older-versions.md) covers each older version: how to recognise it, how its values are encoded, the names the rewrite changed, and how brdb migrates it.

---

## Differences from the spec

What the game writes and brdb handles beyond the draft spec:

- **`dynamic_enum`**, an enum value tagged with a `types` index.
- **`Bool` and `Str`**, aliases of `bool` and `str`.
- **`brick_handle` and `component_handle`**, inline `BRSavedBrickRef` / `BRSavedComponentRef` payloads.
- **EA3 encodings** ([older-versions.md](older-versions.md#ea3-format)) that break the spec's rules: the asset index after `wire_graph_variant` selector 3, the stored `weak_object` index, and packed (unaligned) flat array structs.

The spec describes the `types` section and dynamic containers as planned. Current game builds write them.

---

## Error types

Errors produced while reading schemas and decoding values (`BrdbSchemaError`):

| Variant | Cause |
|---|---|
| `InvalidHeader(n)` | an array-form schema whose length is not 2 or 3 |
| `InvalidSchema(msg)` | a duplicate or unknown section, a schema that is neither a map nor an array, a dynamic container in the `types` table, or an EA3 write of a schema that needs the map form |
| `InvalidTypeIndex { index, len }` | a dynamic container's type index is past the end of the `types` table |
| `UnknownType(name)` | a type name that is neither built in nor in the schema |
| `UnknownSchemaType(name)` | a name in the schema that is not a struct, enum or variant |
| `UnknownWireVariant(selector)` | a variant selector past the end of the alternative list, or an invalid built-in wire union selector |
| `UnsupportedType(name)` | a handle type whose payload struct is missing from the schema |
| `UnknownAsset(type, index)` | an asset index past the end of the external asset references |
| `EnumIndexOutOfBounds` | an enum value not less than the number of declared values |
| `InvalidFlatType` | a type that cannot be a flat array element |
| `InvalidFlatDataSize` | a flat array byte count that is not a multiple of the element size |
| `MissingStructField` | a struct value without a field the schema requires |
| `ExpectedType(expected, got)` | a value of the wrong type or msgpack marker |
| `RmpMarkerReadError` | a low-level msgpack marker read failure |
| `InvalidUtf8` | string data that is not valid UTF-8 |
