use std::{collections::HashMap, fmt::Display, hash::Hash, sync::Arc};

use indexmap::IndexMap;

use crate::{
    errors::BrdbSchemaError,
    schema::{BrdbInterned, BrdbSchema, as_brdb::AsBrdbValue},
    wrapper::Vector3f,
};

#[derive(Clone, Debug)]
pub struct BrdbEnum {
    pub(crate) schema: Arc<BrdbSchema>,
    pub name: BrdbInterned,
    pub value: u64,
}

#[derive(Clone, Debug)]
pub struct BrdbStruct {
    pub(crate) schema: Arc<BrdbSchema>,
    pub name: BrdbInterned,
    pub properties: HashMap<BrdbInterned, BrdbValue>,
}

impl BrdbStruct {
    pub fn get(&self, prop: impl AsRef<str>) -> Option<&BrdbValue> {
        let key = self.schema.intern.get(prop.as_ref())?;
        self.properties.get(&key)
    }

    pub fn contains_key(&self, prop: impl AsRef<str>) -> bool {
        let Some(key) = self.schema.intern.get(prop.as_ref()) else {
            return false;
        };
        self.properties.contains_key(&key)
    }

    pub fn get_name(&self) -> &str {
        self.schema
            .intern
            .lookup_ref(self.name)
            .unwrap_or("unknown")
    }

    pub fn prop(&self, prop: impl AsRef<str>) -> Result<&BrdbValue, BrdbSchemaError> {
        let prop = prop.as_ref();
        self.get(prop).ok_or_else(|| {
            BrdbSchemaError::MissingStructField(
                self.schema
                    .intern
                    .lookup(self.name)
                    .unwrap_or_else(|| "unknown struct".to_string()),
                prop.to_owned(),
            )
        })
    }

    pub fn set_prop(
        &mut self,
        prop: impl AsRef<str>,
        value: BrdbValue,
    ) -> Result<(), BrdbSchemaError> {
        let prop = prop.as_ref();
        let key = self.schema.intern.get(prop).ok_or_else(|| {
            BrdbSchemaError::MissingStructField(
                self.schema
                    .intern
                    .lookup(self.name)
                    .unwrap_or_else(|| "unknown struct".to_string()),
                prop.to_owned(),
            )
        })?;
        // ensure prop exists
        if !self.properties.contains_key(&key) {
            return Err(BrdbSchemaError::MissingStructField(
                self.schema
                    .intern
                    .lookup(self.name)
                    .unwrap_or_else(|| "unknown struct".to_string()),
                prop.to_owned(),
            ));
        }
        self.properties.insert(key, value);
        Ok(())
    }

    pub fn as_hashmap(&self) -> Result<HashMap<String, Box<dyn AsBrdbValue>>, BrdbSchemaError> {
        let mut map = HashMap::new();
        for (k, v) in &self.properties {
            let key = k
                .get_ok(&self.schema, || {
                    BrdbSchemaError::MissingStructField(
                        self.name.get_or(&self.schema, "unknown struct").to_string(),
                        "unknown_prop".to_string(),
                    )
                })?
                .to_string();
            map.insert(key, Box::new(v.clone()) as Box<dyn AsBrdbValue>);
        }
        Ok(map)
    }

    pub fn to_value(self) -> BrdbValue {
        BrdbValue::Struct(Box::new(self.clone()))
    }
}

impl From<BrdbStruct> for BrdbValue {
    fn from(value: BrdbStruct) -> Self {
        BrdbValue::Struct(Box::new(value))
    }
}

impl Display for BrdbStruct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut props = self
            .properties
            .iter()
            .map(|(k, v)| {
                format!(
                    "  {}: {},\n",
                    k.get_or(&self.schema, "unknown_prop"),
                    v.display_inner(&self.schema, 1)
                )
            })
            .collect::<Vec<_>>();
        props.sort();
        write!(
            f,
            "{} {{\n{}}}",
            self.name.get_or(&self.schema, "unknown struct"),
            props.join("")
        )
    }
}

impl BrdbEnum {
    pub fn get_value_raw(&self) -> u64 {
        self.value
    }

    pub fn get_name(&self) -> &str {
        self.schema
            .intern
            .lookup_ref(self.name)
            .unwrap_or("unknown")
    }

    pub fn get_value(&self) -> String {
        self.schema
            .intern
            .lookup(self.name)
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Clone, Debug)]
pub enum BrdbValue {
    Nil,
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    String(String),
    Asset(Option<usize>),
    Enum(BrdbEnum),
    Struct(Box<BrdbStruct>),
    Array(Vec<BrdbValue>),
    FlatArray(Vec<BrdbValue>),
    Map(IndexMap<BrdbValue, BrdbValue>),
    WireVar(WireVariant),
}

/// A wire graph variant value. The members mirror the engine's
/// `WireGraphVariant` union (`f64`, `i64`, `bool`, `weak_object`,
/// `WireGraphExec`, `Vector`, `Rotator`, `Quat`, `str`, `LinearColor`);
/// `WireGraphPrimMathVariant` uses the numeric/spatial subset.
#[derive(Clone, Debug)]
pub enum WireVariant {
    Number(f64),
    Int(i64),
    Bool(bool),
    /// `weak_object` member: an object/asset reference, stored as an index into
    /// `global_data.external_asset_references` (`None` is a null reference).
    Object(Option<usize>),
    Exec,
    /// `Vector` member (X, Y, Z).
    Vector(Vector3f),
    /// `Rotator` member (Pitch, Yaw, Roll — f64 degrees).
    Rotator { pitch: f64, yaw: f64, roll: f64 },
    /// `Quat` member (X, Y, Z, W — f64).
    Quat { x: f64, y: f64, z: f64, w: f64 },
    /// `str` member: a string value.
    Str(String),
    /// `LinearColor` member (R, G, B, A — f32, linear 0–1).
    LinearColor { r: f32, g: f32, b: f32, a: f32 },
}
impl Default for WireVariant {
    fn default() -> Self {
        WireVariant::Number(0.0)
    }
}
impl Display for WireVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireVariant::Number(n) => write!(f, "{n}"),
            WireVariant::Int(i) => write!(f, "{i}"),
            WireVariant::Bool(b) => write!(f, "{b}"),
            WireVariant::Object(Some(i)) => write!(f, "obj#{i}"),
            WireVariant::Object(None) => write!(f, "obj#null"),
            WireVariant::Exec => write!(f, "exec"),
            WireVariant::Vector(v) => write!(f, "({}, {}, {})", v.x, v.y, v.z),
            WireVariant::Rotator { pitch, yaw, roll } => {
                write!(f, "rot({pitch}, {yaw}, {roll})")
            }
            WireVariant::Quat { x, y, z, w } => write!(f, "quat({x}, {y}, {z}, {w})"),
            WireVariant::Str(s) => write!(f, "{s:?}"),
            WireVariant::LinearColor { r, g, b, a } => {
                write!(f, "color({r}, {g}, {b}, {a})")
            }
        }
    }
}
impl From<f64> for WireVariant {
    fn from(value: f64) -> Self {
        WireVariant::Number(value)
    }
}
impl From<f32> for WireVariant {
    fn from(value: f32) -> Self {
        WireVariant::Number(value as f64)
    }
}

macro_rules! wire_var_int {
    ($ty:ty) => {
        impl From<$ty> for WireVariant {
            fn from(value: $ty) -> Self {
                WireVariant::Int(value as i64)
            }
        }
    };
    ($ty:ty, $($rest:ty),*) => {
        wire_var_int!($ty);
        wire_var_int!($($rest),*);
    };
}
wire_var_int!(i8, i16, i32, i64, u8, u16, u32, u64);

impl From<bool> for WireVariant {
    fn from(value: bool) -> Self {
        WireVariant::Bool(value)
    }
}
impl From<String> for WireVariant {
    fn from(value: String) -> Self {
        WireVariant::Str(value)
    }
}
impl From<&str> for WireVariant {
    fn from(value: &str) -> Self {
        WireVariant::Str(value.to_string())
    }
}
impl From<Vector3f> for WireVariant {
    fn from(value: Vector3f) -> Self {
        WireVariant::Vector(value)
    }
}

/// Convert a decoded value back into a `WireVariant`. Named variants decode to
/// raw `BrdbValue`s (the tag is consumed on read); legacy variants decode to a
/// `WireVar`. Both round-trip here.
impl TryFrom<&BrdbValue> for WireVariant {
    type Error = BrdbSchemaError;
    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(match value {
            BrdbValue::WireVar(v) => v.clone(),
            BrdbValue::F64(n) => WireVariant::Number(*n),
            BrdbValue::F32(n) => WireVariant::Number(*n as f64),
            BrdbValue::I64(i) => WireVariant::Int(*i),
            BrdbValue::I32(i) => WireVariant::Int(*i as i64),
            BrdbValue::Bool(b) => WireVariant::Bool(*b),
            BrdbValue::String(s) => WireVariant::Str(s.clone()),
            BrdbValue::Asset(opt) => WireVariant::Object(*opt),
            BrdbValue::Struct(s) if s.get_name() == "Vector" => {
                WireVariant::Vector(Vector3f::try_from(value)?)
            }
            other => {
                return Err(BrdbSchemaError::ExpectedType(
                    "wire variant".to_owned(),
                    other.get_type().to_owned(),
                ));
            }
        })
    }
}

/// A wire graph array variant value, mirroring the engine's
/// `WireGraphArrayVariant` union (one array member per element type). Each
/// member is a `WireGraph*Array` struct holding a single `Values` array.
#[derive(Clone, Debug, PartialEq)]
pub enum WireArrayVariant {
    /// `WireGraphDoubleArray`
    DoubleArray(Vec<f64>),
    /// `WireGraphInt64Array`
    Int64Array(Vec<i64>),
    /// `WireGraphBoolArray`
    BoolArray(Vec<bool>),
    /// `WireGraphObjectArray` (weak_object references, as asset indices)
    ObjectArray(Vec<Option<usize>>),
    /// `WireGraphVectorArray`
    VectorArray(Vec<Vector3f>),
    /// `WireGraphRotatorArray` — (Pitch, Yaw, Roll) f64 elements.
    RotatorArray(Vec<(f64, f64, f64)>),
    /// `WireGraphQuatArray` — (X, Y, Z, W) f64 elements.
    QuatArray(Vec<(f64, f64, f64, f64)>),
    /// `WireGraphStringArray`
    StringArray(Vec<String>),
    /// `WireGraphLinearColorArray` — (R, G, B, A) f32 elements, linear 0–1.
    LinearColorArray(Vec<(f32, f32, f32, f32)>),
}

impl WireArrayVariant {
    /// The `WireGraph*Array` struct name this array maps to.
    pub fn member_type(&self) -> &'static str {
        match self {
            WireArrayVariant::DoubleArray(_) => "WireGraphDoubleArray",
            WireArrayVariant::Int64Array(_) => "WireGraphInt64Array",
            WireArrayVariant::BoolArray(_) => "WireGraphBoolArray",
            WireArrayVariant::ObjectArray(_) => "WireGraphObjectArray",
            WireArrayVariant::VectorArray(_) => "WireGraphVectorArray",
            WireArrayVariant::RotatorArray(_) => "WireGraphRotatorArray",
            WireArrayVariant::QuatArray(_) => "WireGraphQuatArray",
            WireArrayVariant::StringArray(_) => "WireGraphStringArray",
            WireArrayVariant::LinearColorArray(_) => "WireGraphLinearColorArray",
        }
    }
}

impl From<Vec<f64>> for WireArrayVariant {
    fn from(v: Vec<f64>) -> Self {
        WireArrayVariant::DoubleArray(v)
    }
}
impl From<Vec<i64>> for WireArrayVariant {
    fn from(v: Vec<i64>) -> Self {
        WireArrayVariant::Int64Array(v)
    }
}
impl From<Vec<bool>> for WireArrayVariant {
    fn from(v: Vec<bool>) -> Self {
        WireArrayVariant::BoolArray(v)
    }
}
impl From<Vec<Vector3f>> for WireArrayVariant {
    fn from(v: Vec<Vector3f>) -> Self {
        WireArrayVariant::VectorArray(v)
    }
}
impl From<Vec<String>> for WireArrayVariant {
    fn from(v: Vec<String>) -> Self {
        WireArrayVariant::StringArray(v)
    }
}

/// Convert a decoded `WireGraph*Array` struct back into a `WireArrayVariant`.
impl TryFrom<&BrdbValue> for WireArrayVariant {
    type Error = BrdbSchemaError;
    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        let s = value.as_struct()?;
        let values = s.prop("Values")?.as_array()?;
        Ok(match s.get_name() {
            "WireGraphDoubleArray" => WireArrayVariant::DoubleArray(
                values.iter().map(|v| v.as_brdb_f64()).collect::<Result<_, _>>()?,
            ),
            "WireGraphInt64Array" => WireArrayVariant::Int64Array(
                values.iter().map(|v| v.as_brdb_i64()).collect::<Result<_, _>>()?,
            ),
            "WireGraphBoolArray" => WireArrayVariant::BoolArray(
                values.iter().map(|v| v.as_brdb_bool()).collect::<Result<_, _>>()?,
            ),
            "WireGraphStringArray" => WireArrayVariant::StringArray(
                values
                    .iter()
                    .map(|v| v.as_brdb_str().map(str::to_owned))
                    .collect::<Result<_, _>>()?,
            ),
            "WireGraphVectorArray" => WireArrayVariant::VectorArray(
                values.iter().map(Vector3f::try_from).collect::<Result<_, _>>()?,
            ),
            "WireGraphRotatorArray" => WireArrayVariant::RotatorArray(
                values
                    .iter()
                    .map(|v| {
                        let s = v.as_struct()?;
                        Ok((
                            s.prop("Pitch")?.as_brdb_f64()?,
                            s.prop("Yaw")?.as_brdb_f64()?,
                            s.prop("Roll")?.as_brdb_f64()?,
                        ))
                    })
                    .collect::<Result<_, BrdbSchemaError>>()?,
            ),
            "WireGraphQuatArray" => WireArrayVariant::QuatArray(
                values
                    .iter()
                    .map(|v| {
                        let s = v.as_struct()?;
                        Ok((
                            s.prop("X")?.as_brdb_f64()?,
                            s.prop("Y")?.as_brdb_f64()?,
                            s.prop("Z")?.as_brdb_f64()?,
                            s.prop("W")?.as_brdb_f64()?,
                        ))
                    })
                    .collect::<Result<_, BrdbSchemaError>>()?,
            ),
            "WireGraphLinearColorArray" => WireArrayVariant::LinearColorArray(
                values
                    .iter()
                    .map(|v| {
                        let s = v.as_struct()?;
                        Ok((
                            s.prop("R")?.as_brdb_f32()?,
                            s.prop("G")?.as_brdb_f32()?,
                            s.prop("B")?.as_brdb_f32()?,
                            s.prop("A")?.as_brdb_f32()?,
                        ))
                    })
                    .collect::<Result<_, BrdbSchemaError>>()?,
            ),
            "WireGraphObjectArray" => WireArrayVariant::ObjectArray(
                values
                    .iter()
                    .map(|v| match v {
                        BrdbValue::Asset(opt) => Ok(*opt),
                        other => Err(BrdbSchemaError::ExpectedType(
                            "weak_object".to_owned(),
                            other.get_type().to_owned(),
                        )),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            other => return Err(BrdbSchemaError::UnknownType(other.to_owned())),
        })
    }
}

impl BrdbValue {
    pub fn get_type(&self) -> &'static str {
        match self {
            BrdbValue::Nil => "nil",
            BrdbValue::Bool(_) => "bool",
            BrdbValue::U8(_) => "u8",
            BrdbValue::U16(_) => "u16",
            BrdbValue::U32(_) => "u32",
            BrdbValue::U64(_) => "u64",
            BrdbValue::I8(_) => "i8",
            BrdbValue::I16(_) => "i16",
            BrdbValue::I32(_) => "i32",
            BrdbValue::I64(_) => "i64",
            BrdbValue::F32(_) => "f32",
            BrdbValue::F64(_) => "f64",
            BrdbValue::String(_) => "string",
            BrdbValue::Asset(_) => "asset",
            BrdbValue::Enum(_) => "enum",
            BrdbValue::Struct(_) => "struct",
            BrdbValue::Array(_) => "array",
            BrdbValue::FlatArray(_) => "flatarray",
            BrdbValue::Map(_) => "map",
            BrdbValue::WireVar(_) => "wire_variant",
        }
    }
    pub fn as_struct(&self) -> Result<&BrdbStruct, BrdbSchemaError> {
        if let Self::Struct(v) = self {
            Ok(v)
        } else {
            Err(BrdbSchemaError::ExpectedType(
                "struct".to_owned(),
                self.get_type().to_string(),
            ))
        }
    }

    pub fn prop(&self, prop: impl AsRef<str>) -> Result<&BrdbValue, BrdbSchemaError> {
        let prop = prop.as_ref();
        let s = self.as_struct()?;
        s.get(prop).ok_or_else(|| {
            BrdbSchemaError::MissingStructField(
                s.schema
                    .intern
                    .lookup(s.name)
                    .unwrap_or_else(|| "unknown struct".to_string()),
                prop.to_owned(),
            )
        })
    }

    pub fn contains_key(&self, prop: impl AsRef<str>) -> bool {
        let Some(s) = self.as_struct().ok() else {
            return false;
        };
        s.contains_key(prop)
    }

    pub fn as_array(&self) -> Result<&Vec<BrdbValue>, BrdbSchemaError> {
        match self {
            Self::Array(v) | Self::FlatArray(v) => Ok(v),
            _ => Err(BrdbSchemaError::ExpectedType(
                "array".to_owned(),
                self.get_type().to_string(),
            )),
        }
    }

    pub fn index(&self, index: usize) -> Result<Option<&BrdbValue>, BrdbSchemaError> {
        Ok(self.as_array()?.get(index))
    }

    pub fn index_unwrap(&self, index: usize) -> Result<&BrdbValue, BrdbSchemaError> {
        let vec = self.as_array()?;
        Ok(vec
            .get(index)
            .ok_or_else(|| BrdbSchemaError::ArrayIndexOutOfBounds {
                len: vec.len(),
                index,
            })?)
    }

    pub fn as_str(&self) -> Result<&str, BrdbSchemaError> {
        if let Self::String(v) = self {
            Ok(v)
        } else {
            Err(BrdbSchemaError::ExpectedType(
                "string".to_owned(),
                self.get_type().to_string(),
            ))
        }
    }

    pub fn display(&self, schema: &BrdbSchema) -> String {
        self.display_inner(schema, 0)
    }

    fn display_inner(&self, schema: &BrdbSchema, depth: usize) -> String {
        match self {
            BrdbValue::Nil => "nil".to_string(),
            BrdbValue::Bool(v) => format!("{v}"),
            BrdbValue::U8(v) => format!("{v}u8"),
            BrdbValue::U16(v) => format!("{v}u16"),
            BrdbValue::U32(v) => format!("{v}u32"),
            BrdbValue::U64(v) => format!("{v}u64"),
            BrdbValue::I8(v) => format!("{v}i8"),
            BrdbValue::I16(v) => format!("{v}i16"),
            BrdbValue::I32(v) => format!("{v}i32"),
            BrdbValue::I64(v) => format!("{v}i64"),
            BrdbValue::F32(v) => format!("{v}f32"),
            BrdbValue::F64(v) => format!("{v}f64"),
            BrdbValue::WireVar(v) => match v {
                WireVariant::Number(n) => format!("wire {n}f64"),
                WireVariant::Int(i) => format!("wire {i}i64"),
                WireVariant::Bool(b) => format!("wire {b}"),
                WireVariant::Object(o) => format!("wire obj#{o:?}"),
                WireVariant::Exec => "w exec".to_string(),
                WireVariant::Vector(v) => format!("wire ({}, {}, {})", v.x, v.y, v.z),
                WireVariant::Rotator { pitch, yaw, roll } => {
                    format!("wire rot({pitch}, {yaw}, {roll})")
                }
                WireVariant::Quat { x, y, z, w } => format!("wire quat({x}, {y}, {z}, {w})"),
                WireVariant::Str(s) => format!("wire {s:?}"),
                WireVariant::LinearColor { r, g, b, a } => {
                    format!("wire color({r}, {g}, {b}, {a})")
                }
            },
            BrdbValue::String(v) => format!("\"{v}\""),
            BrdbValue::Asset(None) => "none".to_string(),
            BrdbValue::Asset(Some(v)) => {
                if let Some((asset_ty, asset_name)) =
                    schema.global_data.external_asset_references.get_index(*v)
                {
                    format!("{asset_ty}/{asset_name}")
                } else {
                    format!("unknown asset {v}")
                }
            }
            BrdbValue::Enum(e) => format!("{}::{}", e.get_name(), e.get_value()),
            BrdbValue::Struct(s) => {
                let pad = "  ".repeat(depth);
                let mut props = s
                    .properties
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{pad}  {}: {},\n",
                            schema.intern.lookup_ref(*k).unwrap_or("unknown prop"),
                            v.display_inner(schema, depth + 1)
                        )
                    })
                    .collect::<Vec<_>>();
                props.sort();
                format!(
                    "{} {{\n{}{pad}}}",
                    schema.intern.lookup_ref(s.name).unwrap_or("unknown struct"),
                    props.join("")
                )
            }
            BrdbValue::Array(v) => {
                let pad = "  ".repeat(depth);
                let elems = v
                    .iter()
                    .map(|e| format!("{pad}  {},\n", e.display_inner(schema, depth + 1)))
                    .collect::<Vec<_>>();
                format!("[\n{}{}]", elems.join(""), "  ".repeat(depth))
            }
            BrdbValue::FlatArray(v) => {
                let pad = "  ".repeat(depth);
                let elems = v
                    .iter()
                    .map(|e| format!("{pad}  {},\n", e.display_inner(schema, depth + 1)))
                    .collect::<Vec<_>>();
                format!("flat[\n{}{}]", elems.join(""), "  ".repeat(depth))
            }
            BrdbValue::Map(map) => {
                let pad = "  ".repeat(depth);
                let mut entries = map
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{pad}  {}: {},\n",
                            k.display_inner(schema, depth + 1),
                            v.display_inner(schema, depth + 1)
                        )
                    })
                    .collect::<Vec<_>>();
                entries.sort();
                format!("{{\n{}\n{pad}}}", entries.join(""))
            }
        }
    }
}

impl Hash for BrdbValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            BrdbValue::Nil => {}
            BrdbValue::Bool(v) => v.hash(state),
            BrdbValue::U8(v) => v.hash(state),
            BrdbValue::U16(v) => v.hash(state),
            BrdbValue::U32(v) => v.hash(state),
            BrdbValue::U64(v) => v.hash(state),
            BrdbValue::I8(v) => v.hash(state),
            BrdbValue::I16(v) => v.hash(state),
            BrdbValue::I32(v) => v.hash(state),
            BrdbValue::I64(v) => v.hash(state),
            BrdbValue::F32(v) => v.to_bits().hash(state),
            BrdbValue::F64(v) => v.to_bits().hash(state),
            BrdbValue::String(v) => v.hash(state),
            BrdbValue::Asset(v) => v.hash(state),
            BrdbValue::Enum(e) => {
                e.name.hash(state);
                e.value.hash(state);
            }
            BrdbValue::Struct(s) => {
                s.name.hash(state);
                for (k, v) in &s.properties {
                    k.hash(state);
                    v.hash(state);
                }
            }
            BrdbValue::Array(v) => v.hash(state),
            BrdbValue::FlatArray(v) => v.hash(state),
            BrdbValue::Map(map) => map.iter().for_each(|(k, v)| {
                k.hash(state);
                v.hash(state);
            }),
            BrdbValue::WireVar(w) => match w {
                WireVariant::Number(n) => n.to_bits().hash(state),
                WireVariant::Int(i) => i.hash(state),
                WireVariant::Bool(b) => b.hash(state),
                WireVariant::Object(o) => o.hash(state),
                WireVariant::Exec => {}
                WireVariant::Vector(v) => {
                    v.x.to_bits().hash(state);
                    v.y.to_bits().hash(state);
                    v.z.to_bits().hash(state);
                }
                WireVariant::Rotator { pitch, yaw, roll } => {
                    pitch.to_bits().hash(state);
                    yaw.to_bits().hash(state);
                    roll.to_bits().hash(state);
                }
                WireVariant::Quat { x, y, z, w } => {
                    x.to_bits().hash(state);
                    y.to_bits().hash(state);
                    z.to_bits().hash(state);
                    w.to_bits().hash(state);
                }
                WireVariant::Str(s) => s.hash(state),
                WireVariant::LinearColor { r, g, b, a } => {
                    r.to_bits().hash(state);
                    g.to_bits().hash(state);
                    b.to_bits().hash(state);
                    a.to_bits().hash(state);
                }
            },
        }
    }
}

impl Eq for BrdbValue {}

impl PartialEq for BrdbValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(l0), Self::Bool(r0)) => l0 == r0,
            (Self::U8(l0), Self::U8(r0)) => l0 == r0,
            (Self::U16(l0), Self::U16(r0)) => l0 == r0,
            (Self::U32(l0), Self::U32(r0)) => l0 == r0,
            (Self::U64(l0), Self::U64(r0)) => l0 == r0,
            (Self::I8(l0), Self::I8(r0)) => l0 == r0,
            (Self::I16(l0), Self::I16(r0)) => l0 == r0,
            (Self::I32(l0), Self::I32(r0)) => l0 == r0,
            (Self::I64(l0), Self::I64(r0)) => l0 == r0,
            (Self::F32(l0), Self::F32(r0)) => l0 == r0,
            (Self::F64(l0), Self::F64(r0)) => l0 == r0,
            (Self::String(l0), Self::String(r0)) => l0 == r0,
            (Self::Asset(l0), Self::Asset(r0)) => l0 == r0,
            (Self::Enum(l0), Self::Enum(r0)) => l0.name == r0.name && l0.value == r0.value,
            (Self::Struct(l0), Self::Struct(r0)) => {
                if l0.name != r0.name {
                    return false;
                }
                // Compare all properties
                for (k, lv) in &l0.properties {
                    let Some(kv) = r0.properties.get(k) else {
                        return false;
                    };
                    if lv != kv {
                        return false;
                    }
                }
                return true;
            }
            (Self::Array(l0), Self::Array(r0)) => l0 == r0,
            (Self::FlatArray(l0), Self::FlatArray(r0)) => l0 == r0,
            (Self::Map(l0), Self::Map(r0)) => l0 == r0,
            (Self::WireVar(l0), Self::WireVar(r0)) => match (l0, r0) {
                (WireVariant::Number(l), WireVariant::Number(r)) => l == r,
                (WireVariant::Int(l), WireVariant::Int(r)) => l == r,
                (WireVariant::Bool(l), WireVariant::Bool(r)) => l == r,
                (WireVariant::Object(l), WireVariant::Object(r)) => l == r,
                (WireVariant::Exec, WireVariant::Exec) => false,
                (WireVariant::Vector(l), WireVariant::Vector(r)) => l == r,
                (WireVariant::Str(l), WireVariant::Str(r)) => l == r,
                _ => false,
            },
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

impl TryFrom<&BrdbValue> for String {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        value.as_str().map(|s| s.to_string())
    }
}
impl TryFrom<BrdbValue> for String {
    type Error = BrdbSchemaError;

    fn try_from(value: BrdbValue) -> Result<Self, Self::Error> {
        match value {
            BrdbValue::String(s) => Ok(s),
            _ => Err(BrdbSchemaError::ExpectedType(
                "string".to_owned(),
                value.get_type().to_string(),
            )),
        }
    }
}

impl<'a> TryFrom<&'a BrdbValue> for &'a str {
    type Error = BrdbSchemaError;

    fn try_from(value: &'a BrdbValue) -> Result<&'a str, Self::Error> {
        if let BrdbValue::String(v) = value {
            Ok(v.as_ref())
        } else {
            Err(BrdbSchemaError::ExpectedType(
                "string".to_owned(),
                value.get_type().to_string(),
            ))
        }
    }
}

impl<'a, T: TryFrom<&'a BrdbValue, Error = BrdbSchemaError>> TryFrom<&'a BrdbValue> for Vec<T> {
    type Error = BrdbSchemaError;

    fn try_from(value: &'a BrdbValue) -> Result<Self, Self::Error> {
        let array = value.as_array()?;
        let mut vec = Vec::with_capacity(array.len());
        for item in array {
            vec.push(T::try_from(item)?);
        }
        Ok(vec)
    }
}

impl<T: TryFrom<BrdbValue, Error = BrdbSchemaError>> TryFrom<BrdbValue> for Vec<T> {
    type Error = BrdbSchemaError;

    fn try_from(value: BrdbValue) -> Result<Self, Self::Error> {
        let array = match value {
            BrdbValue::Array(v) => v,
            BrdbValue::FlatArray(v) => v,
            _ => {
                return Err(BrdbSchemaError::ExpectedType(
                    "array".to_owned(),
                    value.get_type().to_string(),
                ));
            }
        };
        let mut vec = Vec::with_capacity(array.len());
        for item in array {
            vec.push(T::try_from(item)?);
        }
        Ok(vec)
    }
}

macro_rules! try_from_impl(
    ($id:ident @ $ty:ty) => {
        impl TryFrom<&BrdbValue> for $ty {
            type Error = BrdbSchemaError;

            fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
                value.$id()
            }
        }
        impl TryFrom<BrdbValue> for $ty {
            type Error = BrdbSchemaError;

            fn try_from(value: BrdbValue) -> Result<Self, Self::Error> {
                value.$id()
            }
        }

    }
);

try_from_impl!(as_brdb_bool @ bool);
try_from_impl!(as_brdb_u8 @ u8);
try_from_impl!(as_brdb_u16 @ u16);
try_from_impl!(as_brdb_u32 @ u32);
try_from_impl!(as_brdb_u64 @ u64);
try_from_impl!(as_brdb_i8 @ i8);
try_from_impl!(as_brdb_i16 @ i16);
try_from_impl!(as_brdb_i32 @ i32);
try_from_impl!(as_brdb_i64 @ i64);
try_from_impl!(as_brdb_f32 @ f32);
try_from_impl!(as_brdb_f64 @ f64);
