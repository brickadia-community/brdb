//! Typed values of the `WireValue` variant that `Latest` saves use for wire
//! graph data (gate inputs/outputs, variables, queues).

use crate::{
    errors::BrdbSchemaError,
    schema::{
        BRICK_HANDLE, BrdbDynamicEnum, BrdbInterned, BrdbSchema, BrdbValue, COMPONENT_HANDLE,
        EA3WireVariant,
        as_brdb::{AsBrdbValue, BrdbArrayIter},
    },
    wrapper::ChunkIndex,
};

/// A `BRSavedBrickRef`: a brick addressed by grid, chunk and index in chunk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SavedBrickRef {
    pub grid_persistent_index: u32,
    pub chunk_index: ChunkIndex,
    pub brick_index_in_chunk: u32,
}

/// A `BRSavedComponentRef`: a component on a brick, by its type index in the
/// save's component type table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SavedComponentRef {
    pub brick: SavedBrickRef,
    pub component_type_index: u16,
}

/// One value of the `WireValue` variant. Each case encodes as the
/// alternative named in its doc; alternatives are matched by type, so the
/// selector comes from the target schema's own alternative list.
#[derive(Clone, Debug, PartialEq)]
pub enum WireValue {
    /// `Bool`
    Bool(bool),
    /// `i64`
    Int(i64),
    /// `f64`
    Number(f64),
    /// `LinearColor` (linear 0-1)
    LinearColor { r: f32, g: f32, b: f32, a: f32 },
    /// `Vector`
    Vector { x: f64, y: f64, z: f64 },
    /// `Rotator` (degrees)
    Rotator { pitch: f64, yaw: f64, roll: f64 },
    /// `Quat`
    Quat { x: f64, y: f64, z: f64, w: f64 },
    /// `Str`
    Str(String),
    /// `dynamic_enum`: an enum value tagged with its enum type.
    Enum(BrdbDynamicEnum),
    /// `weak_object`: non-persistent, always null in a save.
    Object,
    /// `BRSavedBrickRef`
    BrickRef(SavedBrickRef),
    /// `BRSavedComponentRef`
    ComponentRef(SavedComponentRef),
    /// `WireGraphExec`: no payload.
    Exec,
}

impl Default for WireValue {
    fn default() -> Self {
        WireValue::Number(0.0)
    }
}

impl WireValue {
    /// The `WireValue` alternative type this value encodes as.
    pub fn alternative(&self) -> &'static str {
        match self {
            WireValue::Bool(_) => "Bool",
            WireValue::Int(_) => "i64",
            WireValue::Number(_) => "f64",
            WireValue::LinearColor { .. } => "LinearColor",
            WireValue::Vector { .. } => "Vector",
            WireValue::Rotator { .. } => "Rotator",
            WireValue::Quat { .. } => "Quat",
            WireValue::Str(_) => "Str",
            WireValue::Enum(_) => "dynamic_enum",
            WireValue::Object => "weak_object",
            WireValue::BrickRef(_) => "BRSavedBrickRef",
            WireValue::ComponentRef(_) => "BRSavedComponentRef",
            WireValue::Exec => "WireGraphExec",
        }
    }
}

impl From<bool> for WireValue {
    fn from(value: bool) -> Self {
        WireValue::Bool(value)
    }
}
impl From<i64> for WireValue {
    fn from(value: i64) -> Self {
        WireValue::Int(value)
    }
}
impl From<f64> for WireValue {
    fn from(value: f64) -> Self {
        WireValue::Number(value)
    }
}
impl From<String> for WireValue {
    fn from(value: String) -> Self {
        WireValue::Str(value)
    }
}
impl From<&str> for WireValue {
    fn from(value: &str) -> Self {
        WireValue::Str(value.to_owned())
    }
}

/// The equivalent `WireValue` of an EA3 wire graph value.
impl From<EA3WireVariant> for WireValue {
    fn from(value: EA3WireVariant) -> Self {
        match value {
            EA3WireVariant::Number(n) => WireValue::Number(n),
            EA3WireVariant::Int(i) => WireValue::Int(i),
            EA3WireVariant::Bool(b) => WireValue::Bool(b),
            EA3WireVariant::Object(_) => WireValue::Object,
            EA3WireVariant::Exec => WireValue::Exec,
            EA3WireVariant::Vector(v) => WireValue::Vector {
                x: v.x as f64,
                y: v.y as f64,
                z: v.z as f64,
            },
            EA3WireVariant::Rotator { pitch, yaw, roll } => WireValue::Rotator { pitch, yaw, roll },
            EA3WireVariant::Quat { x, y, z, w } => WireValue::Quat { x, y, z, w },
            EA3WireVariant::Str(s) => WireValue::Str(s),
            EA3WireVariant::LinearColor { r, g, b, a } => WireValue::LinearColor { r, g, b, a },
        }
    }
}

fn prop_mismatch(schema: &BrdbSchema, s: BrdbInterned, p: BrdbInterned) -> BrdbSchemaError {
    BrdbSchemaError::MissingStructField(
        s.get_or_else(schema, || "wire value struct".to_owned()),
        p.get_or_else(schema, || "unknown property".to_owned()),
    )
}

impl AsBrdbValue for SavedBrickRef {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        Ok(match prop_name.get(schema) {
            Some("GridPersistentIndex") => &self.grid_persistent_index,
            Some("ChunkIndex") => &self.chunk_index,
            Some("BrickIndexInChunk") => &self.brick_index_in_chunk,
            _ => return Err(prop_mismatch(schema, struct_name, prop_name)),
        })
    }
}

impl AsBrdbValue for SavedComponentRef {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        Ok(match prop_name.get(schema) {
            Some("Brick") => &self.brick,
            Some("ComponentTypeIndex") => &self.component_type_index,
            _ => return Err(prop_mismatch(schema, struct_name, prop_name)),
        })
    }
}

impl AsBrdbValue for WireValue {
    fn as_brdb_variant_member(&self) -> Option<&str> {
        Some(self.alternative())
    }
    fn as_brdb_bool(&self) -> Result<bool, BrdbSchemaError> {
        match self {
            WireValue::Bool(b) => Ok(*b),
            _ => Err(BrdbSchemaError::ExpectedType(
                "Bool".into(),
                self.alternative().into(),
            )),
        }
    }
    fn as_brdb_i64(&self) -> Result<i64, BrdbSchemaError> {
        match self {
            WireValue::Int(i) => Ok(*i),
            _ => Err(BrdbSchemaError::ExpectedType(
                "i64".into(),
                self.alternative().into(),
            )),
        }
    }
    fn as_brdb_f64(&self) -> Result<f64, BrdbSchemaError> {
        match self {
            WireValue::Number(n) => Ok(*n),
            _ => Err(BrdbSchemaError::ExpectedType(
                "f64".into(),
                self.alternative().into(),
            )),
        }
    }
    fn as_brdb_str(&self) -> Result<&str, BrdbSchemaError> {
        match self {
            WireValue::Str(s) => Ok(s),
            _ => Err(BrdbSchemaError::ExpectedType(
                "Str".into(),
                self.alternative().into(),
            )),
        }
    }
    fn as_brdb_asset(
        &self,
        _schema: &BrdbSchema,
        _ty: &str,
    ) -> Result<Option<usize>, BrdbSchemaError> {
        match self {
            WireValue::Object => Ok(None),
            _ => Err(BrdbSchemaError::ExpectedType(
                "weak_object".into(),
                self.alternative().into(),
            )),
        }
    }
    fn as_brdb_dynamic_enum(&self) -> Result<(&str, i64), BrdbSchemaError> {
        match self {
            WireValue::Enum(e) => Ok((&e.enum_name, e.value)),
            _ => Err(BrdbSchemaError::ExpectedType(
                "dynamic_enum".into(),
                self.alternative().into(),
            )),
        }
    }
    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        let prop = prop_name.get(schema).unwrap_or("");
        Ok(match (self, prop) {
            (WireValue::LinearColor { r, .. }, "R") => r,
            (WireValue::LinearColor { g, .. }, "G") => g,
            (WireValue::LinearColor { b, .. }, "B") => b,
            (WireValue::LinearColor { a, .. }, "A") => a,
            (WireValue::Vector { x, .. } | WireValue::Quat { x, .. }, "X") => x,
            (WireValue::Vector { y, .. } | WireValue::Quat { y, .. }, "Y") => y,
            (WireValue::Vector { z, .. } | WireValue::Quat { z, .. }, "Z") => z,
            (WireValue::Quat { w, .. }, "W") => w,
            (WireValue::Rotator { pitch, .. }, "Pitch") => pitch,
            (WireValue::Rotator { yaw, .. }, "Yaw") => yaw,
            (WireValue::Rotator { roll, .. }, "Roll") => roll,
            (WireValue::BrickRef(r), _) => {
                return r.as_brdb_struct_prop_value(schema, struct_name, prop_name);
            }
            (WireValue::ComponentRef(r), _) => {
                return r.as_brdb_struct_prop_value(schema, struct_name, prop_name);
            }
            _ => return Err(prop_mismatch(schema, struct_name, prop_name)),
        })
    }
}

fn saved_brick_ref(value: &BrdbValue) -> Result<SavedBrickRef, BrdbSchemaError> {
    Ok(SavedBrickRef {
        grid_persistent_index: value.prop("GridPersistentIndex")?.as_brdb_u32()?,
        chunk_index: ChunkIndex::try_from(value.prop("ChunkIndex")?)?,
        brick_index_in_chunk: value.prop("BrickIndexInChunk")?.as_brdb_u32()?,
    })
}

impl TryFrom<&BrdbValue> for SavedBrickRef {
    type Error = BrdbSchemaError;
    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        saved_brick_ref(value)
    }
}

impl TryFrom<&BrdbValue> for SavedComponentRef {
    type Error = BrdbSchemaError;
    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(SavedComponentRef {
            brick: saved_brick_ref(value.prop("Brick")?)?,
            component_type_index: value.prop("ComponentTypeIndex")?.as_brdb_u16()?,
        })
    }
}

/// A `dynamic_array` value of brick refs (`brick_handle` elements), such as
/// the `Value` of an `ArrayVar` holding bricks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrickRefArray(pub Vec<SavedBrickRef>);

impl AsBrdbValue for BrickRefArray {
    fn as_brdb_dynamic_array(&self) -> Result<(&str, BrdbArrayIter<'_>), BrdbSchemaError> {
        Ok((
            BRICK_HANDLE,
            Box::new(self.0.iter().map(|r| r as &dyn AsBrdbValue)),
        ))
    }
}

/// A `dynamic_array` value of component refs (`component_handle` elements).
/// An array of arrays is one of these holding refs to other `ArrayVar`s.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentRefArray(pub Vec<SavedComponentRef>);

impl AsBrdbValue for ComponentRefArray {
    fn as_brdb_dynamic_array(&self) -> Result<(&str, BrdbArrayIter<'_>), BrdbSchemaError> {
        Ok((
            COMPONENT_HANDLE,
            Box::new(self.0.iter().map(|r| r as &dyn AsBrdbValue)),
        ))
    }
}

/// Convert a decoded `WireValue` alternative (the raw member value a variant
/// read produces) back into a `WireValue`. EA3 wire values convert too.
impl TryFrom<&BrdbValue> for WireValue {
    type Error = BrdbSchemaError;
    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        let f64_of = |name: &str| value.prop(name)?.as_brdb_f64();
        let f32_of = |name: &str| value.prop(name)?.as_brdb_f32();
        Ok(match value {
            BrdbValue::Bool(b) => WireValue::Bool(*b),
            BrdbValue::I64(i) => WireValue::Int(*i),
            BrdbValue::F64(n) => WireValue::Number(*n),
            BrdbValue::String(s) => WireValue::Str(s.clone()),
            BrdbValue::Asset(_) => WireValue::Object,
            BrdbValue::Enum(e) => {
                WireValue::Enum(BrdbDynamicEnum::new(e.get_name(), e.value as i64))
            }
            BrdbValue::EA3WireVar(v) => v.clone().into(),
            BrdbValue::Struct(s) => match s.get_name() {
                "LinearColor" => WireValue::LinearColor {
                    r: f32_of("R")?,
                    g: f32_of("G")?,
                    b: f32_of("B")?,
                    a: f32_of("A")?,
                },
                "Vector" => WireValue::Vector {
                    x: f64_of("X")?,
                    y: f64_of("Y")?,
                    z: f64_of("Z")?,
                },
                "Rotator" => WireValue::Rotator {
                    pitch: f64_of("Pitch")?,
                    yaw: f64_of("Yaw")?,
                    roll: f64_of("Roll")?,
                },
                "Quat" => WireValue::Quat {
                    x: f64_of("X")?,
                    y: f64_of("Y")?,
                    z: f64_of("Z")?,
                    w: f64_of("W")?,
                },
                "BRSavedBrickRef" => WireValue::BrickRef(saved_brick_ref(value)?),
                "BRSavedComponentRef" => WireValue::ComponentRef(value.try_into()?),
                "WireGraphExec" => WireValue::Exec,
                other => {
                    return Err(BrdbSchemaError::ExpectedType(
                        "WireValue alternative".to_owned(),
                        other.to_owned(),
                    ));
                }
            },
            other => {
                return Err(BrdbSchemaError::ExpectedType(
                    "WireValue alternative".to_owned(),
                    other.get_type().to_owned(),
                ));
            }
        })
    }
}
