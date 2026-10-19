# Flat Arrays

## Introduction

Flat arrays are a compact binary encoding for homogeneous, fixed-size data. Regular msgpack arrays encode each element with type markers, variable-length integers, and per-element overhead. Flat arrays instead store a raw little-endian memory dump of the elements in a single msgpack binary (`bin`) blob.

The tradeoff is flexibility: flat arrays only support numeric primitives and structs composed entirely of numeric primitives. They cannot contain strings, assets, enums, booleans, nested arrays, or maps. When those constraints are met, a flat array is considerably more space-efficient and faster to decode than a regular array.

A flat array field is declared in a `.schema` file as the 2-element array `[type_name, nil]`, and in the plaintext form as `T[flat]`.

## Binary Format

A flat array is written as a msgpack `bin` value:

```
write_bin_len(total_bytes)
<raw bytes>
```

Where:

```
total_bytes = array_length * element_size
```

The `bin` header encodes the total byte count (msgpack `bin8`/`bin16`/`bin32` as appropriate). The raw bytes that follow are the elements laid out contiguously with no separators or per-element markers. All multi-byte numeric values are **little-endian**.

The array element count is not stored explicitly; it is recovered at read time by dividing the buffer length by the element size.

## Element Layout

The element layout depends on the [schema's format](older-versions.md#ea3-format):

- **Latest: C struct layout.** Each field is placed at the next offset that is a multiple of its alignment, and the struct size is rounded up to a multiple of the struct's alignment. Padding bytes are written as zero.
- **EA3: packed.** Fields are placed back to back with no padding.

A primitive's alignment is its size. A struct's alignment is the largest alignment of its fields.

| Type | Size and alignment (bytes) |
|------|-------------|
| `u8` / `i8` | 1 |
| `u16` / `i16` | 2 |
| `u32` / `i32` / `f32` | 4 |
| `u64` / `i64` / `f64` | 8 |

For example, `struct P { A: u8, B: u32 }` is:

| Format | Element bytes | Size |
|---|---|---|
| Latest | `A`, 3 padding bytes, `B` | 8 |
| EA3 | `A`, `B` | 5 |

Structs whose fields are already naturally aligned, such as `Vector3f { X, Y, Z: f32 }`, `Quat4f`, `BRSavedBrickColor { R, G, B, A: u8 }` and `BRSavedRelativeBrickPosition { X, Y, Z: i16 }`, have the same layout in both formats.

`flat_type_size(schema, ty)` returns the element size under the schema's format, or 0 when the type cannot be flat.

## Validation

At read time, after the `bin` buffer is loaded:

1. **Type validity**: the element type must have a flat layout. An unknown type, or a struct with a field that is not a flat type, is an `InvalidFlatType` error.
2. **Size alignment**: `flat_buf_len % element_size == 0`. Otherwise an `InvalidFlatDataSize` error is raised, carrying the type name, buffer length, and element size.

An **empty buffer** (`flat_buf_len == 0`) of a valid type produces an empty array.

Writers compute the layout the same way and raise `InvalidFlatType` when writing elements of a type that cannot be flat.

## Allowed Types

Types that may appear as flat array elements:

- All numeric primitives: `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`, `f32`, `f64`
- Structs whose every field is a single value (not an array, flat array or map) of an allowed flat type

## Types NOT Allowed in Flat Context

| Disallowed | Reason |
|------------|--------|
| Nested arrays (`T[]`, `T[flat]`) and maps | Not single values; the struct has no flat layout |
| Strings | Variable length; no fixed element size |
| Assets (`class`, `object`, `weak_object`) | Not a flat type |
| Enums and variants | Not a flat type |
| Booleans | Not a flat type |
| Dynamic containers | Not a flat type |

## FlatArray vs Array Comparison

| Property | `Array` (`T[]`) | `FlatArray` (`T[flat]`) |
|----------|----------------|------------------------|
| Msgpack encoding | `array` marker + per-element encoding | `bin` blob (raw bytes) |
| Byte order | N/A (msgpack handles it) | Little-endian |
| Type flexibility | Any schema type | Numeric primitives and flat structs only |
| Per-element overhead | Yes (type markers, varint lengths) | None (Latest: alignment padding inside structs) |
| Max total size | Limited by msgpack array len (u32 count) | 4 GB (u32 `bin` length) |
| Schema property variant | `BrdbSchemaStructProperty::Array` | `BrdbSchemaStructProperty::FlatArray` |
| Runtime value variant | `BrdbValue::Array` | `BrdbValue::FlatArray` |
| Schema text syntax | `T[]` | `T[flat]` |

## Quirks

- **The layout follows the schema, not the data.** The same struct can be 8 bytes in a Latest chunk and 5 bytes in an EA3 chunk of the same `.brdb`; decode each chunk with the format of the schema selected for it (see [schema-revisions.md](schema-revisions.md)).

- **Empty arrays are valid.** A `bin` of length 0 produces an empty `Vec`. Writers produce a 0-length `bin` for an empty slice.

- **4 GB maximum.** The msgpack `bin` length is a `u32`, so `total_bytes` must fit in 32 bits. This limits a flat array to at most `4,294,967,295 / element_size` elements.
