# Structure of Arrays (SoA)

BRDB stores brick, component, wire, and entity data using a Structure of Arrays layout. Rather than encoding each element as a self-contained object in an array of structs, related properties are split into parallel arrays (one array per field, all the same length). This layout enables efficient iteration over a single property, cache-friendly sequential access, and selective field loading when only a subset of properties is needed.

Each chunk type has its own SoA struct that is serialized via msgpack using a `.schema` file stored beside it. Components and entities use a two-phase serialization: the SoA struct is written first, then per-element extra data structs are appended sequentially to the same buffer.

The field lists below are the current game's layouts, as in the schemas bundled with brdb (`crates/brdb/schemas/`). Older saves can lack newer fields; a reader should treat a missing field as its default. Type notation: `T[]` is a msgpack array, `T[flat]` a [flat array](flat-arrays.md).

---

## BrickChunkSoA (`BRSavedBrickChunkSoA`)

Stores all brick data for a spatial chunk.

| Field | Type | Description |
|---|---|---|
| `ProceduralBrickStartingIndex` | `u32` | Number of basic (non-procedural) brick assets at save time. All asset indices at or above this value are procedural. |
| `BrickSizeCounters` | `BRSavedBrickSizeCounter[]` | Run-length counters grouping bricks by asset and size. See [Type Counter Pattern](#type-counter-pattern). |
| `BrickSizes` | `BRSavedBrickSize[]` | Per-group brick dimensions. |
| `BrickTypeIndices` | `u32[]` | Per-brick index into the global asset list. |
| `OwnerIndices` | `u32[]` | Per-brick index into the owner table. |
| `OriginalOwnerIndices` | `u32[]` | Per-brick original owner, an index into the owner table. Missing in older saves; brdb mirrors `OwnerIndices` then. |
| `RelativePositions` | `BRSavedRelativeBrickPosition[flat]` | Per-brick position relative to chunk origin. |
| `Orientations` | `u8[flat]` | Per-brick orientation, encoded as a single byte. |
| `CollisionFlags_Player` | `BRSavedBitFlags` | Player collision per brick. |
| `CollisionFlags_Player1` | `BRSavedBitFlags` | Player1 collision per brick. |
| `CollisionFlags_Player2` | `BRSavedBitFlags` | Player2 collision per brick. |
| `CollisionFlags_Player3` | `BRSavedBitFlags` | Player3 collision per brick. |
| `CollisionFlags_Weapon` | `BRSavedBitFlags` | Weapon collision per brick. |
| `CollisionFlags_Interaction` | `BRSavedBitFlags` | Interaction collision per brick. |
| `CollisionFlags_Physics` | `BRSavedBitFlags` | Physics collision per brick. |
| `VisibilityFlags` | `BRSavedBitFlags` | Visibility per brick. |
| `MaterialIndices` | `u8[flat]` | Per-brick index into the global material list. |
| `ColorsAndAlphas` | `BRSavedBrickColor[flat]` | Per-brick RGBA color. The `A` channel encodes material intensity, not transparency. |
| `bColorsAreLinear` | `bool` | Whether `ColorsAndAlphas` RGB is linear. Current saves store sRGB (`false`); older saves without the field store linear colors. |

### Sub-types

**`BRSavedBrickSizeCounter`** groups bricks sharing the same asset type and size:

```
{ AssetIndex: u32, NumSizes: u32 }
```

**`BRSavedBrickSize`** stores brick dimensions in centimeter-scale units:

```
{ X: u16, Y: u16, Z: u16 }
```

**`BRSavedRelativeBrickPosition`** stores position relative to chunk origin:

```
{ X: i16, Y: i16, Z: i16 }
```

**`BRSavedBrickColor`** is an RGBA color where `A` is material intensity (0-255), not opacity:

```
{ R: u8, G: u8, B: u8, A: u8 }
```

### Schema file

`BRSavedBrickChunkSoA.schema` (embedded; loaded via `bricks_chunks_schema()`)

---

## ComponentChunkSoA (`BRSavedComponentChunkSoA`)

Stores all component data for a chunk. Components attach behavior to bricks. Joint-type components carry additional offset and rotation data, and microchip components reference the inner grid they host.

| Field | Type | Description |
|---|---|---|
| `ComponentTypeCounters` | `BRSavedBrickComponentTypeCounter[]` | Run-length counters grouping components by type. See [Type Counter Pattern](#type-counter-pattern). |
| `ComponentBrickIndices` | `u32[]` | Per-component index of the brick this component is attached to. |
| `JointBrickIndices` | `u32[]` | Per-joint index of the secondary brick involved in the joint. |
| `JointEntityReferences` | `u32[]` | Per-joint entity reference index. |
| `JointInitialRelativeOffsets` | `Vector3f[flat]` | Per-joint initial relative position offset. |
| `JointInitialRelativeRotations` | `Quat4f[flat]` | Per-joint initial relative rotation. |
| `MicrochipBrickIndices` | `u32[]` | Bricks in this chunk that host a microchip (`Internal_Microchip`) component, as local brick indices. Missing in older saves. |
| `MicrochipBrickGridReferences` | `u32[]` | Entity reference of each microchip's inner brick grid, parallel to `MicrochipBrickIndices`. Missing in older saves. |

### Sub-types

**`BRSavedBrickComponentTypeCounter`**:

```
{ TypeIndex: u32, NumInstances: u32 }
```

**`Vector3f`**:

```
{ X: f32, Y: f32, Z: f32 }
```

**`Quat4f`**:

```
{ X: f32, Y: f32, Z: f32, W: f32 }
```

### Serialization note

After the SoA msgpack block, per-component extra data structs are appended sequentially. Marker-only components (those with no associated schema struct) do not produce an extra data block. Data structs are named `BCSComponentVertex_*` (most wire-graph gates, for example `BrickComponentType_Expr_LogicalAND` -> `BCSComponentVertex_Expr_BoolBool_Bool`) or `BrickComponentData_*` (for example `BrickComponentType_ArrayVar` -> `BrickComponentData_ArrayVar`). Several component types can share one struct. See [entity-components.md](entity-components.md).

### Schema files

- `BRSavedComponentChunkSoA.schema`: the minimal schema, with only the SoA struct and its sub-types.
- `BRSavedComponentChunkSoA_max.schema`: every component data struct of the current game build, with the `WireValue` variant and the `types` table. brdb writes a world's `ComponentsShared.schema` as the minimal schema plus the data structs (and everything they reference) of the component types the world uses, as the game does.

---

## WireChunkSoA (`BRSavedWireChunkSoA`)

Stores the wires whose **target** is a brick in this chunk. A wire whose source is in the same grid and chunk is stored as a local wire; any other wire is a remote wire.

| Field | Type | Description |
|---|---|---|
| `RemoteWireSources` | `BRSavedRemoteWirePortSource[]` | Source port of each remote wire. |
| `LocalWireSources` | `BRSavedLocalWirePortSource[]` | Source port of each local wire. |
| `RemoteWireTargets` | `BRSavedWirePortTarget[]` | Target port of each remote wire, parallel to `RemoteWireSources`. |
| `LocalWireTargets` | `BRSavedWirePortTarget[]` | Target port of each local wire, parallel to `LocalWireSources`. |
| `PendingPropagationFlags` | `BRSavedBitFlags` | Per-wire pending propagation flags. |

### Sub-types

```
BRSavedLocalWirePortSource  { BrickIndexInChunk: u32, ComponentTypeIndex: u16, PortIndex: u16 }
BRSavedWirePortTarget       { BrickIndexInChunk: u32, ComponentTypeIndex: u16, PortIndex: u16 }
BRSavedRemoteWirePortSource { GridPersistentIndex: u32, ChunkIndex: BRSavedChunk3DIndex,
                              BrickIndexInChunk: u32, ComponentTypeIndex: u16, PortIndex: u16 }
BRSavedChunk3DIndex         { X: i16, Y: i16, Z: i16 }
```

`ComponentTypeIndex` indexes the global `ComponentTypeNames`, and `PortIndex` indexes the global `ComponentWirePortNames` (see [shared-schemas.md](shared-schemas.md)).

### Schema file

`BRSavedWireChunkSoA.schema` (embedded; loaded via `bricks_wires_schema()`)

---

## EntityChunkSoA (`BRSavedEntityChunkSoA`)

Stores all entity data for a chunk. Entities are dynamic world objects with physics state and color.

| Field | Type | Description |
|---|---|---|
| `TypeCounters` | `BRSavedEntityTypeCounter[]` | Run-length counters grouping entities by type. See [Type Counter Pattern](#type-counter-pattern). |
| `PersistentIndices` | `u32[]` | Per-entity persistent ID used to correlate entities across saves. |
| `PrefabSpawnInstanceIndices` | `u32[]` | Per-entity index of the prefab spawn instance that created it (0 for none). Latest saves only. |
| `OwnerIndices` | `u32[]` | Per-entity index into the owner table. |
| `OriginalOwnerIndices` | `u32[]` | Per-entity original owner, an index into the owner table. Missing in older saves; brdb mirrors `OwnerIndices` then. |
| `Locations` | `Vector3f[flat]` | Per-entity world position. |
| `Rotations` | `Quat4f[flat]` | Per-entity world rotation. |
| `PhysicsLockedFlags` | `BRSavedBitFlags` | Whether each entity is physics-locked (frozen). |
| `PhysicsSleepingFlags` | `BRSavedBitFlags` | Whether each entity is physics-sleeping. |
| `LinearVelocities` | `Vector3f[flat]` | Per-entity linear velocity. |
| `AngularVelocities` | `Vector3f[flat]` | Per-entity angular velocity. |
| `ColorsAndAlphas` | `BRSavedEntityColors[flat]` | Per-entity color set of exactly 8 `BRSavedBrickColor` values, `Color0` through `Color7`. |
| `RemainingLifeSpans` | `f32[flat]` | Per-entity remaining lifespan in seconds (0 = no auto-despawn). Missing in older saves. |
| `bColorsAreLinear` | `bool` | Whether `ColorsAndAlphas` RGB is linear, as for bricks. |

EA3 saves instead have `WeldParentFlags` (`BRSavedBitFlags`) and `WeldParentIndices` (`u32[]`), and no `PrefabSpawnInstanceIndices`. Current game builds removed the weld fields.

### Sub-types

**`BRSavedEntityTypeCounter`**:

```
{ TypeIndex: u32, NumEntities: u32 }
```

**`BRSavedEntityColors`** is a fixed tuple of 8 `BRSavedBrickColor` values:

```
{ Color0, Color1, Color2, Color3, Color4, Color5, Color6, Color7 }
```

### Serialization note

After the SoA msgpack block, per-entity extra data structs are appended sequentially. Entities whose type has no associated schema struct do not produce an extra data block.

### Schema file

`BRSavedEntityChunkSoA.schema` (embedded; loaded via `entities_chunks_schema()`)

---

## Type Counter Pattern

Elements of the same type are stored consecutively and summarized with a `(type_index, count)` counter rather than repeating the type on every element. This reduces redundancy for homogeneous runs.

When building a SoA:

- If the last counter already refers to the same type, increment its count.
- Otherwise, append a new counter with count 1.

To read N elements of type T at offset O, walk the counter array accumulating counts until reaching the right group, then slice the corresponding arrays starting at O with length N.

---

## BitFlags

Boolean per-element flags are packed as bit vectors in a `BRSavedBitFlags` struct:

```
struct BRSavedBitFlags { Flags: u8[flat] }
```

N flags are stored in `ceil(N / 8)` bytes. To test bit `i`:

```
byte = Flags[i / 8]
set  = (byte & (1 << (i & 7))) != 0
```

`BrickChunkSoA` carries 8 BitFlags fields: the 7 collision channel flags (`CollisionFlags_Player` through `CollisionFlags_Physics`) and `VisibilityFlags`. `EntityChunkSoA` carries 2: `PhysicsLockedFlags` and `PhysicsSleepingFlags`. `WireChunkSoA` carries `PendingPropagationFlags`.

---

## Serialization

SoA data is serialized in two phases:

1. **SoA phase**: The SoA struct is encoded with the corresponding `.schema` definition: its field values in schema order, with no struct header (see [overview.md](overview.md#structs)). This produces the primary buffer.
2. **Extra data phase** (components and entities only): For each element that has an associated typed data struct, that struct is serialized and appended sequentially to the same buffer immediately after the SoA block.

Bricks and wires do not have an extra data phase; all their data lives in the SoA.

An empty SoA (zero elements) is valid; all arrays and counter arrays will be empty.

---

## Schema Files

Each SoA type is described by an embedded `.schema` file compiled into the binary:

| Schema file | SoA type | Constant |
|---|---|---|
| `BRSavedBrickChunkSoA.schema` | `BrickChunkSoA` | `BRICK_CHUNK_SOA` |
| `BRSavedComponentChunkSoA.schema` | `ComponentChunkSoA` (minimal) | `BRICK_COMPONENT_SOA` |
| `BRSavedComponentChunkSoA_max.schema` | `ComponentChunkSoA` (every data struct) | `BRICK_COMPONENT_SOA` |
| `BRSavedWireChunkSoA.schema` | `WireChunkSoA` | `BRICK_WIRE_SOA` |
| `BRSavedEntityChunkSoA.schema` | `EntityChunkSoA` | `ENTITY_CHUNK_SOA` |

---

## Quirks

- **`BRSavedBrickColor.A` is material intensity, not opacity.** Despite the field name `ColorsAndAlphas`, the alpha channel encodes how strongly the material shader is applied (0 = no material effect, 255 = full material effect). Transparency is not stored here.
- **Entity colors are always exactly 8.** `BRSavedEntityColors` is a fixed-size tuple of 8 `BRSavedBrickColor` values. There is no variable-length encoding.
- **`BRSavedBrickSizeCounter` groups by asset AND size.** Two bricks with the same asset but different sizes produce separate counter entries. The counter does not group by type alone.
- **Marker-only components skip the extra data phase.** If a component type has no associated data schema struct, no bytes are appended for it during the extra data phase.
- **An empty SoA is valid.** All arrays may have zero elements; consumers must handle this without error.
