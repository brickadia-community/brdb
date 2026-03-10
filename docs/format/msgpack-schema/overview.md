# msgpack-schema Overview

## Introduction

BRDB uses a custom schema format built on [MessagePack](https://msgpack.org/) to define data structures for `.mps` files. The format is based on [Zeblote's msgpack-schema Gist](https://gist.github.com/Zeblote/053d54cc820df3bccad57df676202895) with several undocumented extensions described in the [Deviations](#deviations-from-zeblotes-gist) section below.

A schema describes the shape of serialized game data: what fields a struct has, what variants an enum has, and how each field is encoded on the wire. The schema itself is stored as a binary `.schema` file (msgpack-encoded) alongside the `.mps` data it describes.

---

## BrdbSchema Structure

A schema contains four components:

| Component | Purpose |
|---|---|
| **Intern pool** | String deduplication. All names stored once, referenced by index |
| **Global data** | Shared asset/entity metadata (external asset references). See [shared-schemas.md](shared-schemas.md) |
| **Enums** | Named enumerations: enum name -> (variant name -> i32 value) |
| **Structs** | Named struct definitions: struct name -> (field name -> property descriptor) |

An **enum** maps variant names to i32 values. A **struct** maps field names to property descriptors (see below).

---

## Property Types

Each field in a struct has one of four property kinds:

| Kind | Wire encoding | Description |
|---|---|---|
| `Type(name)` | Single typed value | A single msgpack-encoded value of the named type |
| `Array(type)` | msgpack array length + N typed values | Variable-length sequence; length precedes elements |
| `FlatArray(type)` | Binary blob (raw little-endian bytes) | Contiguous fixed-width byte array; no per-element framing. See [flat-arrays.md](flat-arrays.md) |
| `Map(key, value)` | msgpack map length + key/value pairs | Variable-length map; length precedes entries |

### Schema file encoding of property descriptors

When writing a `.schema` file, the property kind is encoded by marker:

- `Type` is written as a msgpack string (the type name).
- `Array` is written as a 1-element msgpack array containing the type name string.
- `FlatArray` is written as a 2-element msgpack array: `[type_name, nil]`.
- `Map` is written as a 1-entry msgpack map: `{key_type: value_type}`.

---

## Supported Types

### Primitive numeric types

| Type | Size | Notes |
|---|---|---|
| `bool` | 1 byte (msgpack bool) | |
| `u8` | 1-2 bytes | msgpack uint |
| `u16` | 2-3 bytes | msgpack uint |
| `u32` | 4-5 bytes | msgpack uint |
| `u64` | 8-9 bytes | msgpack uint |
| `i8` | 1-2 bytes | msgpack int (see [i8 signedness note](#b-u8i8-signedness-ambiguity)) |
| `i16` | 2-3 bytes | msgpack int |
| `i32` | 4-5 bytes | msgpack int |
| `i64` | 8-9 bytes | msgpack int |
| `f32` | variable | msgpack float or int coerced to f32 |
| `f64` | variable | msgpack float or int coerced to f64 |

### String type

`str` is read as a msgpack string (UTF-8).

### Asset reference types

`class` and `object` are both encoded as an i64 index into `global_data.external_asset_references`.

- `-1` (or any negative value) -> `None` (null asset reference).
- `0..n` -> index into the external asset references list, which stores `(asset_type, asset_name)` tuples.

### Wire graph variant types

These types are not documented in Zeblote's gist. See [Deviations](#c-wire-types-missing-from-gist).

**`wire_graph_variant`** is a tagged union with 5 possible tags:

| Tag | Variant | Payload |
|---|---|---|
| `0` | `Number` | f64 |
| `1` | `Int` | i64 |
| `2` | `Bool` | bool |
| `3` | `Object` | (string, currently decoded as `"unknown"`) |
| `4` | `Exec` | (none) |

**`wire_graph_prim_math_variant`** is a restricted subset of `wire_graph_variant` with only 2 tags:

| Tag | Variant | Payload |
|---|---|---|
| `0` | `Number` | f64 |
| `1` | `Int` | i64 |

### Named types

Any type name not in the above list is treated as a user-defined enum or struct, looked up by name in the schema's intern pool. Unknown names produce `BrdbSchemaError::UnknownType`.

---

## String Interning

All names (type names, field names, enum names, variant names) are stored exactly once in an intern pool. References to names are integer indices into that pool rather than repeated strings. This reduces memory usage and enables fast identity comparisons.

---

## Schema Files

`.schema` files are msgpack-encoded binary metadata. The top-level encoding is a 2-element msgpack array:

```
[enums_map, structs_map]
```

- `enums_map` is a msgpack map of `enum_name -> {variant_name -> i32_value, ...}`.
- `structs_map` is a msgpack map of `struct_name -> {field_name -> property_descriptor, ...}`.

A `.schema` file can be read to reconstruct the full schema (enums, structs, intern pool), and a schema can be serialized back to `.schema` bytes.

---

## Deviations from Zeblote's Gist

### a. FixNeg Marker Encoding

Values 224-255 are encoded using MessagePack's negative fixint markers (which represent -32 to -1 in standard msgpack). When reading an unsigned integer, these markers are reinterpreted as unsigned by the formula:

```
decoded_value = 256 + signed_value
```

For example, a FixNeg byte representing -1 decodes to 255; -32 decodes to 224. Standard msgpack treats these as signed integers. BRDB's non-standard reinterpretation is not described in Zeblote's gist.

### b. u8/i8 Signedness Ambiguity

When reading wire-encoded unsigned integers, a msgpack `I8` marker (signed byte) is reinterpreted as an unsigned value by widening the signed byte directly. This means a stored value of -1 (0xFF) is read as 255.

This differs from flat array handling, where `i8` bytes are interpreted with proper signedness. The distinction matters for values 128-255: in wire encoding they are large positive unsigned integers, while in flat arrays they are negative signed integers.

### c. Wire Types Missing from Gist

`wire_graph_variant` and `wire_graph_prim_math_variant` are not documented in Zeblote's gist. Both are tag-dispatched union types:

- `wire_graph_variant` has 5 valid tag values (0-4). Tags 0 and 1 carry a numeric payload; tag 2 carries a bool; tag 3 carries a string (currently always decoded as `"unknown"`); tag 4 carries no payload.
- `wire_graph_prim_math_variant` is a restricted form with only tags 0 (Number/f64) and 1 (Int/i64). Any other tag produces `BrdbSchemaError::UnknownWireVariant`.

### d. Asset Reference Encoding

Assets (`class`, `object`) are encoded as i64 indices, not strings. The index points into `BrdbSchemaGlobalData.external_asset_references`, which is a list of `(asset_type, asset_name)` tuples. A value of -1 (or any negative i64) means no asset (null). This differs from any string-based asset reference scheme that might be expected from the gist.

---

## Error Types

Errors produced during schema reading and data decoding:

| Variant | Cause |
|---|---|
| `UnknownType(String)` | Type name not found in intern pool or built-in list |
| `UnknownWireVariant(usize)` | Tag value not valid for `wire_graph_variant` or `wire_graph_prim_math_variant` |
| `UnknownAsset(String, usize)` | Asset index out of bounds in `external_asset_references` |
| `InvalidFlatType` | Type not valid for use in a flat array |
| `InvalidFlatDataSize` | Flat array byte count not divisible by element size |
| `EnumIndexOutOfBounds` | Enum variant index outside declared range |
| `MissingStructField` | Expected struct field not present in data |
| `ExpectedType(String, String)` | Wrong msgpack marker encountered (expected, got) |
| `RmpMarkerReadError` | Low-level msgpack marker read failure |
| `InvalidUtf8` | String data is not valid UTF-8 |
