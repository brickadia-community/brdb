//! Named value conversions a migration step can apply (`values` in
//! `migrations.json`).
use std::{collections::HashMap, sync::Arc, sync::LazyLock};

use indexmap::IndexMap;

use super::steps::ValueRule;
use crate::{
    errors::BrdbSchemaError,
    schema::{
        BrdbDynamicArray, BrdbDynamicMap, BrdbInterned, BrdbSchema, BrdbStruct, BrdbValue,
        EA3WireVariant,
    },
    schemas,
};

/// Apply one conversion to a decoded value, recursively.
pub(super) fn apply(rule: ValueRule, value: &BrdbValue) -> Result<BrdbValue, BrdbSchemaError> {
    match rule {
        ValueRule::Ea3WireGraph => ea3_wire_graph(value),
    }
}

/// The current component schema, which the structs built here are interned
/// in.
fn current_schema() -> &'static Arc<BrdbSchema> {
    static SCHEMA: LazyLock<Arc<BrdbSchema>> =
        LazyLock::new(|| Arc::new(schemas::bricks_components_schema_max().clone()));
    &SCHEMA
}

/// A struct value of the current component schema.
pub(super) fn current_struct(name: &str, fields: Vec<(&str, BrdbValue)>) -> BrdbValue {
    let schema = current_schema();
    let mut properties = HashMap::new();
    for (k, v) in fields {
        if let Some(id) = schema.intern.get(k) {
            properties.insert(id, v);
        }
    }
    BrdbValue::Struct(Box::new(BrdbStruct {
        schema: schema.clone(),
        name: schema.intern.get(name).unwrap_or(BrdbInterned(0)),
        properties,
    }))
}

/// The current `types` entry for an EA3 wire graph element type name (from a
/// `WireGraph<T>Array` or `WireGraphMap_<K>_<V>` struct name).
fn element_type(ea3: &str) -> String {
    match ea3 {
        "Double" | "double" => "f64",
        "Int64" | "int64" => "i64",
        "Bool" | "bool" => "Bool",
        "Object" | "FWeakObjectPtr" => "weak_object",
        "Vector" | "FVector" => "Vector",
        "Rotator" | "FRotator" => "Rotator",
        "Quat" | "FQuat" => "Quat",
        "String" | "FWireGraphString" => "str",
        "LinearColor" | "FLinearColor" => "LinearColor",
        other => other,
    }
    .to_owned()
}

/// A value of the EA3 `wire_graph_variant` union as the plain member value
/// the current `WireValue` variant holds.
fn wire_variant_value(v: &EA3WireVariant) -> BrdbValue {
    match v {
        EA3WireVariant::Number(n) => BrdbValue::F64(*n),
        EA3WireVariant::Int(i) => BrdbValue::I64(*i),
        EA3WireVariant::Bool(b) => BrdbValue::Bool(*b),
        EA3WireVariant::Str(s) => BrdbValue::String(s.clone()),
        EA3WireVariant::Object(_) => BrdbValue::Asset(None),
        EA3WireVariant::Exec => current_struct("WireGraphExec", vec![]),
        EA3WireVariant::Vector(v) => current_struct(
            "Vector",
            vec![
                ("X", BrdbValue::F64(v.x as f64)),
                ("Y", BrdbValue::F64(v.y as f64)),
                ("Z", BrdbValue::F64(v.z as f64)),
            ],
        ),
        EA3WireVariant::Rotator { pitch, yaw, roll } => current_struct(
            "Rotator",
            vec![
                ("Pitch", BrdbValue::F64(*pitch)),
                ("Yaw", BrdbValue::F64(*yaw)),
                ("Roll", BrdbValue::F64(*roll)),
            ],
        ),
        EA3WireVariant::Quat { x, y, z, w } => current_struct(
            "Quat",
            vec![
                ("X", BrdbValue::F64(*x)),
                ("Y", BrdbValue::F64(*y)),
                ("Z", BrdbValue::F64(*z)),
                ("W", BrdbValue::F64(*w)),
            ],
        ),
        EA3WireVariant::LinearColor { r, g, b, a } => current_struct(
            "LinearColor",
            vec![
                ("R", BrdbValue::F32(*r)),
                ("G", BrdbValue::F32(*g)),
                ("B", BrdbValue::F32(*b)),
                ("A", BrdbValue::F32(*a)),
            ],
        ),
    }
}

/// `ea3WireGraph`: wire graph arrays become dynamic arrays, wire graph maps
/// become dynamic maps, wrapped enums become dynamic enums and
/// `wire_graph_variant` values become plain values. Everything else is kept
/// (the writer matches struct fields by name).
fn ea3_wire_graph(value: &BrdbValue) -> Result<BrdbValue, BrdbSchemaError> {
    Ok(match value {
        BrdbValue::EA3WireVar(v) => wire_variant_value(v),
        BrdbValue::Array(items) => BrdbValue::Array(all(items)?),
        BrdbValue::FlatArray(items) => BrdbValue::FlatArray(all(items)?),
        BrdbValue::Map(map) => BrdbValue::Map(map_entries(map)?),
        BrdbValue::Struct(s) => {
            let name = s.get_name();
            if let Some(element) = name
                .strip_prefix("WireGraphEnumArray_")
                .or_else(|| name.strip_prefix("WireGraph").and_then(|n| n.strip_suffix("Array")))
            {
                let values = s.prop("Values")?.as_array()?;
                BrdbValue::DynamicArray(BrdbDynamicArray::new(element_type(element), all(values)?))
            } else if name == "WireGraphEnumWrapper" {
                // The enum value itself, written as a `dynamic_enum`.
                ea3_wire_graph(s.prop("Enum")?)?
            } else if name.starts_with("WireGraphMapKeyWrapper_") {
                let map = s.prop("Map")?.as_struct()?;
                let (key, value) = map
                    .get_name()
                    .strip_prefix("WireGraphMap_")
                    .and_then(|kv| {
                        ["int64", "FWireGraphString", "FWeakObjectPtr"]
                            .into_iter()
                            .find_map(|k| Some((k, kv.strip_prefix(k)?.strip_prefix('_')?)))
                    })
                    .ok_or_else(|| {
                        BrdbSchemaError::ExpectedType(
                            "WireGraphMap_<key>_<value>".to_owned(),
                            map.get_name().to_owned(),
                        )
                    })?;
                let BrdbValue::Map(entries) = map.prop("Values")? else {
                    return Err(BrdbSchemaError::ExpectedType(
                        "map".to_owned(),
                        map.prop("Values")?.get_type().to_owned(),
                    ));
                };
                BrdbValue::DynamicMap(BrdbDynamicMap::new(
                    element_type(key),
                    element_type(value),
                    map_entries(entries)?,
                ))
            } else {
                let mut out = (**s).clone();
                for v in out.properties.values_mut() {
                    *v = ea3_wire_graph(v)?;
                }
                BrdbValue::Struct(Box::new(out))
            }
        }
        other => other.clone(),
    })
}

fn all(items: &[BrdbValue]) -> Result<Vec<BrdbValue>, BrdbSchemaError> {
    items.iter().map(ea3_wire_graph).collect()
}

fn map_entries(
    map: &IndexMap<BrdbValue, BrdbValue>,
) -> Result<IndexMap<BrdbValue, BrdbValue>, BrdbSchemaError> {
    map.iter()
        .map(|(k, v)| Ok((ea3_wire_graph(k)?, ea3_wire_graph(v)?)))
        .collect()
}
