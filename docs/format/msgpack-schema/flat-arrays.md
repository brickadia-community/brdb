# Flat Arrays

## Introduction

Flat arrays are a compact binary encoding for homogeneous, fixed-size data. Regular msgpack arrays encode each element with type markers, variable-length integers, and per-element overhead. Flat arrays instead store raw little-endian bytes packed into a single msgpack binary (`bin`) blob.

The tradeoff is flexibility: flat arrays only support numeric primitives and structs composed entirely of numeric primitives. They cannot contain strings, assets, enums, booleans, nested arrays, or maps. When those constraints are met, a flat array is considerably more space-efficient and faster to decode than a regular array.

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

The `bin` header encodes the total byte count as a `u32` (msgpack `bin8`/`bin16`/`bin32` as appropriate). The raw bytes that follow are the elements laid out contiguously with no separators or per-element markers. All multi-byte numeric values are **little-endian**.

The array element count is not stored explicitly; it is recovered at read time by dividing the buffer length by the element size.

## Element Size Computation

`flat_type_size(schema, ty)` recursively computes the byte size of a flat-compatible type:

| Type | Size (bytes) |
|------|-------------|
| `u8` / `i8` | 1 |
| `u16` / `i16` | 2 |
| `u32` / `i32` / `f32` | 4 |
| `u64` / `i64` / `f64` | 8 |
| struct | sum of sizes of all `Type` properties (recursively) |

For structs, the size is computed recursively: look up the struct in the schema, sum the sizes of all scalar (`Type`) properties. Properties declared as `Array`, `FlatArray`, or `Map` contribute **0**, effectively making such a struct invalid for flat use (see Quirks below). If the type name is not found in the schema at all, the size is 0.

## Validation

At read time, after the `bin` buffer is loaded, the runtime checks:

1. **Type validity**: `flat_type_size` must return a non-zero value. If it returns 0 (unknown type or struct with non-`Type` properties), an `InvalidFlatType` error is raised.
2. **Size alignment**: `flat_buf_len % element_size == 0`. If the buffer length is not an exact multiple of the element size, an `InvalidFlatDataSize` error is raised, carrying the type name, buffer length, and element size for diagnostics.

An **empty buffer** (`flat_buf_len == 0`) always passes both checks and produces an empty array.

## Allowed Types

Types that may appear as flat array elements:

- All numeric primitives: `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`, `f32`, `f64`
- Structs whose every property is declared as `Type` and recursively resolves to an allowed flat type

## Types NOT Allowed in Flat Context

The following may not be flat array element types:

| Disallowed | Reason |
|------------|--------|
| Nested arrays (`T[]`, `T[flat]`) | Only `Type` properties contribute to size |
| Maps | Only `Type` properties contribute to size |
| Strings | Variable length; no fixed element size |
| Assets | Stored as u64 indices in normal context but not handled in flat path |
| Enums | Not matched in `read_flat_type` / `write_flat_type` |
| Booleans | Not matched in `read_flat_type` / `write_flat_type` |

A struct that contains any of these property kinds will have a computed size equal only to the sum of its `Type` properties, which may be 0 or simply wrong, causing an `InvalidFlatType` or `InvalidFlatDataSize` error at read time.

## FlatArray vs Array Comparison

| Property | `Array` (`T[]`) | `FlatArray` (`T[flat]`) |
|----------|----------------|------------------------|
| Msgpack encoding | `array` marker + per-element encoding | `bin` blob (raw bytes) |
| Byte order | N/A (msgpack handles it) | Little-endian |
| Type flexibility | Any schema type | Numeric primitives and flat structs only |
| Per-element overhead | Yes (type markers, varint lengths) | None |
| Max total size | Limited by msgpack array len (u32 count) | 4 GB (u32 `bin` length) |
| Schema property variant | `BrdbSchemaStructProperty::Array` | `BrdbSchemaStructProperty::FlatArray` |
| Runtime value variant | `BrdbValue::Array` | `BrdbValue::FlatArray` |
| Schema text syntax | `T[]` | `T[flat]` |

## Quirks

- **`flat_type_size` returns 0 for invalid types.** The function does not return an error; a 0 result is the signal that a type is unsupported. The error is only raised later, during `read_flat_type`, when the 0 is detected. Write-side code uses the same function but does not independently validate, so writing a flat array of an invalid type will silently write a zero-byte `bin` unless the caller ensures the type is valid.

- **Empty arrays are valid.** A `bin` of length 0 divides evenly by any non-zero element size and produces an empty `Vec`. Writers produce a 0-length `bin` for an empty slice.

- **No alignment or padding.** Struct fields are packed with no inter-field padding and no alignment requirements. A struct `{ x: u8, y: u32 }` occupies exactly 5 bytes per element.

- **4 GB maximum.** The msgpack `bin` length is a `u32`, so `total_bytes` must fit in 32 bits. This limits a flat array to at most `4,294,967,295 / element_size` elements.
