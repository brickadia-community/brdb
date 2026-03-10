# Structure of Arrays (SoA)

BRDB stores brick, component, and entity data using a Structure of Arrays layout. Rather than encoding each element as a self-contained object in an array of structs, related properties are split into parallel arrays (one array per field, all the same length). This layout enables efficient iteration over a single property, cache-friendly sequential access, and selective field loading when only a subset of properties is needed.

Each chunk type has its own SoA struct that is serialized via msgpack using an embedded `.schema` file. Components and entities use a two-phase serialization: the SoA struct is written first, then per-element extra data structs are appended sequentially to the same buffer.

---

## BrickChunkSoA (`BRSavedBrickChunkSoA`)

Stores all brick data for a spatial chunk.

| Field | Type | Description |
|---|---|---|
| `ProceduralBrickStartingIndex` | `u32` | Number of basic (non-procedural) brick assets at save time. All asset indices at or above this value are procedural. |
| `BrickSizeCounters` | `Array<BrickSizeCounter>` | Run-length counters grouping bricks by asset and size. See [Type Counter Pattern](#type-counter-pattern). |
| `BrickSizes` | `FlatArray<BrickSize>` | Per-group brick dimensions. |
| `BrickTypeIndices` | `FlatArray<u32>` | Per-brick index into the global asset list. |
| `OwnerIndices` | `FlatArray<u32>` | Per-brick index into the owner table. |
| `RelativePositions` | `FlatArray<RelativePosition>` | Per-brick position relative to chunk origin. |
| `Orientations` | `FlatArray<u8>` | Per-brick orientation, encoded as a single byte. |
| `CollisionFlags_Player` | `FlatArray<u8>` | BitFlags forplayer collision per brick. |
| `CollisionFlags_Player1` | `FlatArray<u8>` | BitFlags forplayer1 collision per brick. |
| `CollisionFlags_Player2` | `FlatArray<u8>` | BitFlags forplayer2 collision per brick. |
| `CollisionFlags_Player3` | `FlatArray<u8>` | BitFlags forplayer3 collision per brick. |
| `CollisionFlags_Weapon` | `FlatArray<u8>` | BitFlags forweapon collision per brick. |
| `CollisionFlags_Interaction` | `FlatArray<u8>` | BitFlags forinteraction collision per brick. |
| `CollisionFlags_Tool` | `FlatArray<u8>` | BitFlags fortool collision per brick. |
| `CollisionFlags_Physics` | `FlatArray<u8>` | BitFlags forphysics collision per brick. |
| `VisibilityFlags` | `FlatArray<u8>` | BitFlags forvisibility per brick. |
| `MaterialIndices` | `FlatArray<u8>` | Per-brick index into the global material list. |
| `ColorsAndAlphas` | `FlatArray<SavedBrickColor>` | Per-brick RGBA color. The `a` channel encodes material intensity, not transparency. |

### Sub-types

**`BrickSizeCounter`** groups bricks sharing the same asset type and size:

```
{ AssetIndex: u32, NumSizes: u32 }
```

**`BrickSize`** stores brick dimensions in centimeter-scale units:

```
{ X: u16, Y: u16, Z: u16 }
```

**`RelativePosition`** stores position relative to chunk origin:

```
{ X: i16, Y: i16, Z: i16 }
```

**`SavedBrickColor`** is an RGBA color where `a` is material intensity (0-255), not opacity:

```
{ R: u8, G: u8, B: u8, A: u8 }
```

### Schema file

`BRSavedBrickChunkSoA.schema` (embedded; loaded via `bricks_chunks_schema()`)

---

## ComponentChunkSoA (`BRSavedComponentChunkSoA`)

Stores all component data for a chunk. Components attach behavior to bricks. Joint-type components carry additional offset and rotation data.

| Field | Type | Description |
|---|---|---|
| `ComponentTypeCounters` | `Array<ComponentTypeCounter>` | Run-length counters grouping components by type. See [Type Counter Pattern](#type-counter-pattern). |
| `ComponentBrickIndices` | `FlatArray<u32>` | Per-component index of the brick this component is attached to. |
| `JointBrickIndices` | `FlatArray<u32>` | Per-joint index of the secondary brick involved in the joint. |
| `JointEntityReferences` | `FlatArray<u32>` | Per-joint entity reference index. |
| `JointInitialRelativeOffsets` | `FlatArray<Vector3f>` | Per-joint initial relative position offset. |
| `JointInitialRelativeRotations` | `FlatArray<Quat4f>` | Per-joint initial relative rotation. |

### Sub-types

**`ComponentTypeCounter`**:

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

After the SoA msgpack block, per-component extra data structs are appended sequentially. Marker-only components (those with no associated schema struct) do not produce an extra data block.

### Schema files

- `BRSavedComponentChunkSoA.schema` (standard schema)
- `BRSavedComponentChunkSoA_max.schema` (schema used when writing at maximum capacity)

---

## EntityChunkSoA (`BRSavedEntityChunkSoA`)

Stores all entity data for a chunk. Entities are dynamic world objects with physics state and color.

| Field | Type | Description |
|---|---|---|
| `TypeCounters` | `Array<EntityTypeCounter>` | Run-length counters grouping entities by type. See [Type Counter Pattern](#type-counter-pattern). |
| `PersistentIndices` | `FlatArray<u32>` | Per-entity persistent ID used to correlate entities across saves. |
| `OwnerIndices` | `FlatArray<u32>` | Per-entity index into the owner table. |
| `Locations` | `FlatArray<Vector3f>` | Per-entity world position. |
| `Rotations` | `FlatArray<Quat4f>` | Per-entity world rotation. |
| `WeldParentFlags` | `FlatArray<u8>` | BitFlags forwhether each entity has a weld parent. |
| `PhysicsLockedFlags` | `FlatArray<u8>` | BitFlags forwhether each entity is physics-locked (frozen). |
| `PhysicsSleepingFlags` | `FlatArray<u8>` | BitFlags forwhether each entity is physics-sleeping. |
| `WeldParentIndices` | `FlatArray<u32>` | Per-entity index of the weld parent entity (only valid when `WeldParentFlags` bit is set). |
| `LinearVelocities` | `FlatArray<Vector3f>` | Per-entity linear velocity. |
| `AngularVelocities` | `FlatArray<Vector3f>` | Per-entity angular velocity. |
| `ColorsAndAlphas` | `Array<EntityColors>` | Per-entity color set containing exactly 8 `SavedBrickColor` values per entity, named `Color0` through `Color7`. |

### Sub-types

**`EntityTypeCounter`**:

```
{ TypeIndex: u32, NumEntities: u32 }
```

**`EntityColors`** is a fixed tuple of 8 `SavedBrickColor` values:

```
{ Color0, Color1, Color2, Color3, Color4, Color5, Color6, Color7 }
```

### Serialization note

After the SoA msgpack block, per-entity extra data structs are appended sequentially. Entities whose type has no associated schema struct do not produce an extra data block.

### Schema file

`BRSavedEntityChunkSoA.schema` (embedded; loaded via `entity_chunk_schema()`)

---

## Type Counter Pattern

Elements of the same type are stored consecutively and summarized with a `(type_index, count)` counter rather than repeating the type on every element. This reduces redundancy for homogeneous runs.

When building a SoA:

- If the last counter already refers to the same type, increment its count.
- Otherwise, append a new counter with count 1.

To read N elements of type T at offset O, walk the counter array accumulating counts until reaching the right group, then slice the corresponding flat arrays starting at O with length N.

---

## BitFlags

Boolean per-element flags are packed as bit vectors. N flags are stored in `ceil(N / 8)` bytes. The serialized form is a `FlatArray<u8>`.

To test bit `i`:

```
byte = array[i / 8]
set  = (byte & (1 << (i & 7))) != 0
```

`BrickChunkSoA` carries 9 BitFlags fields: the 8 collision channel flags (`CollisionFlags_Player` through `CollisionFlags_Physics`) and `VisibilityFlags`. `EntityChunkSoA` carries 3: `WeldParentFlags`, `PhysicsLockedFlags`, and `PhysicsSleepingFlags`.

---

## Serialization

SoA data is serialized in two phases:

1. **SoA phase**: The entire SoA struct is encoded as a msgpack map using the corresponding `.schema` definition, producing the primary buffer.
2. **Extra data phase** (components and entities only): For each element that has an associated typed data struct, that struct is serialized and appended sequentially to the same buffer immediately after the SoA block.

Bricks do not have an extra data phase; all brick data lives entirely in the SoA.

An empty SoA (zero elements) is valid; all flat arrays will be empty and all counter arrays will be empty.

---

## Schema Files

Each SoA type is described by an embedded `.schema` file compiled into the binary:

| Schema file | SoA type | Constant |
|---|---|---|
| `BRSavedBrickChunkSoA.schema` | `BrickChunkSoA` | `BRICK_CHUNK_SOA` |
| `BRSavedComponentChunkSoA.schema` | `ComponentChunkSoA` | `BRICK_COMPONENT_SOA` |
| `BRSavedComponentChunkSoA_max.schema` | `ComponentChunkSoA` (max capacity) | (none) |
| `BRSavedEntityChunkSoA.schema` | `EntityChunkSoA` | `ENTITY_CHUNK_SOA` |

---

## Quirks

- **`SavedBrickColor.a` is material intensity, not opacity.** Despite the field name `ColorsAndAlphas`, the alpha channel encodes how strongly the material shader is applied (0 = no material effect, 255 = full material effect). Transparency is not stored here.
- **Entity colors are always exactly 8.** `EntityColors` is a fixed-size tuple of 8 `SavedBrickColor` values. There is no variable-length encoding.
- **`BrickSizeCounter` groups by asset AND size.** Two bricks with the same asset but different sizes produce separate counter entries. The counter does not group by type alone.
- **Marker-only components skip the extra data phase.** If a component type has no associated data schema struct, no bytes are appended for it during the extra data phase.
- **An empty SoA is valid.** All arrays may have zero elements; consumers must handle this without error.
