use std::io::Write;

mod cache;

use cache::{FieldKind, FlatOp, PlanOp, StructPlan, VariantPlan};
pub(crate) use cache::{PlanCache, TopLevelMemo};

use crate::{
    errors::BrdbSchemaError,
    schema::{
        BrdbEnum, BrdbInterned, BrdbSchema, BrdbSchemaEnum, BrdbSchemaStruct,
        BrdbSchemaStructProperty, BrdbStruct, BrdbValue, DYNAMIC_ARRAY, DYNAMIC_ENUM, DYNAMIC_MAP,
        EA3_WIRE_GRAPH_PRIM_MATH_VARIANT, EA3_WIRE_GRAPH_VARIANT, EA3WireVariant,
        as_brdb::AsBrdbValue,
        builtin_alias, handle_struct,
        read::{FlatKind, FlatLayout, flat_layout},
        same_type,
    },
};

/// Type names the unplanned writers handle before consulting the schema's
/// struct/enum/variant tables.
fn is_builtin_type(ty: &str) -> bool {
    matches!(
        ty,
        "nil"
            | "bool"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
            | "str"
            | "wire_graph_variant"
            | "wire_graph_prim_math_variant"
            | "bundle_path_ref"
            | "class"
            | "object"
            | "weak_object"
            | DYNAMIC_ARRAY
            | DYNAMIC_MAP
    )
}

/// Whether `ty` names a struct, enum or variant defined by the schema.
fn is_schema_type(schema: &BrdbSchema, ty: &str) -> bool {
    schema.intern.get(ty).is_some_and(|id| {
        schema.get_struct_interned(id).is_some()
            || schema.get_enum_interned(id).is_some()
            || schema.get_variant_interned(id).is_some()
    })
}

/// Selector and saved type name of the first alternative of `variant_ty`
/// whose type matches one of `candidates` (in preference order). Alternatives
/// are matched by type, not position, so `bool` finds a `Bool` alternative.
fn find_alternative<'s>(
    schema: &'s BrdbSchema,
    variant_ty: &str,
    candidates: &[&str],
) -> Result<(usize, &'s str), BrdbSchemaError> {
    let alternatives = schema
        .get_variant(variant_ty)
        .ok_or_else(|| BrdbSchemaError::UnknownType(variant_ty.to_owned()))?;
    for candidate in candidates {
        for (i, alt) in alternatives.iter().enumerate() {
            if let Some(name) = schema.intern.lookup_ref(*alt)
                && same_type(name, candidate)
            {
                return Ok((i, name));
            }
        }
    }
    Err(BrdbSchemaError::ExpectedType(
        format!("{variant_ty} member"),
        candidates.first().copied().unwrap_or("value").to_owned(),
    ))
}

/// Candidate alternative types for a decoded value, most specific first.
fn value_alternative_candidates(value: &BrdbValue) -> Result<Vec<&str>, BrdbSchemaError> {
    Ok(match value {
        BrdbValue::Nil => vec!["nil"],
        BrdbValue::Bool(_) => vec!["bool"],
        BrdbValue::U8(_) => vec!["u8"],
        BrdbValue::U16(_) => vec!["u16"],
        BrdbValue::U32(_) => vec!["u32"],
        BrdbValue::U64(_) => vec!["u64"],
        BrdbValue::I8(_) => vec!["i8"],
        BrdbValue::I16(_) => vec!["i16"],
        BrdbValue::I32(_) => vec!["i32"],
        BrdbValue::I64(_) => vec!["i64"],
        BrdbValue::F32(_) => vec!["f32"],
        BrdbValue::F64(_) => vec!["f64"],
        BrdbValue::String(_) => vec!["str", "bundle_path_ref"],
        BrdbValue::Asset(_) => vec!["weak_object", "object", "class"],
        BrdbValue::Enum(e) => vec![e.get_name(), DYNAMIC_ENUM],
        BrdbValue::Struct(s) => vec![s.get_name()],
        other => {
            return Err(BrdbSchemaError::ExpectedType(
                "variant alternative".to_owned(),
                other.get_type().to_owned(),
            ));
        }
    })
}

/// Index of a `types` entry for a value's selected type.
fn dynamic_type_index(schema: &BrdbSchema, ty: &str) -> Result<usize, BrdbSchemaError> {
    if ty == DYNAMIC_ARRAY || ty == DYNAMIC_MAP {
        return Err(BrdbSchemaError::InvalidSchema(format!(
            "dynamic containers cannot contain '{ty}'"
        )));
    }
    schema.type_index(ty).ok_or_else(|| {
        BrdbSchemaError::ExpectedType("a type in the types table".to_owned(), ty.to_owned())
    })
}

/// The `types` entry used for an unset dynamic container: the first of
/// `preferred` present (matching the game's own defaults for new array/map
/// variables), else the first entry.
fn default_type_index(schema: &BrdbSchema, preferred: &str) -> Result<usize, BrdbSchemaError> {
    if schema.types.is_empty() {
        return Err(BrdbSchemaError::InvalidTypeIndex { index: 0, len: 0 });
    }
    Ok(schema.type_index(preferred).unwrap_or(0))
}

/// Write a `dynamic_array` value: element type index, then the array.
fn write_dynamic_array(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    let (element_type, items) = value.as_brdb_dynamic_array()?;
    let index = dynamic_type_index(schema, element_type)?;
    let element_type = schema.type_at(index).expect("index from the types table");
    write_uint(buf, index as u64)?;
    rmp::encode::write_array_len(buf, items.len() as u32)?;
    for (i, item) in items.enumerate() {
        write_brdb(schema, buf, element_type, item).map_err(|e| e.wrap(i))?;
    }
    Ok(())
}

/// Write a `dynamic_map` value: key type index, value type index, then the map.
fn write_dynamic_map(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    let (key_type, value_type, entries) = value.as_brdb_dynamic_map()?;
    let key_index = dynamic_type_index(schema, key_type)?;
    let value_index = dynamic_type_index(schema, value_type)?;
    let key_type = schema
        .type_at(key_index)
        .expect("index from the types table");
    let value_type = schema
        .type_at(value_index)
        .expect("index from the types table");
    write_uint(buf, key_index as u64)?;
    write_uint(buf, value_index as u64)?;
    rmp::encode::write_map_len(buf, entries.len() as u32)?;
    for (k, v) in entries {
        write_brdb(schema, buf, key_type, k).map_err(|e| e.wrap("map key"))?;
        write_brdb(schema, buf, value_type, v).map_err(|e| e.wrap("map value"))?;
    }
    Ok(())
}

/// Write a `dynamic_enum` value: the enum's `types` index, then the value.
fn write_dynamic_enum(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    let (enum_name, enum_value) = value.as_brdb_dynamic_enum()?;
    if schema.get_enum(enum_name).is_none() {
        return Err(BrdbSchemaError::ExpectedType(
            "enum in types table".to_owned(),
            enum_name.to_owned(),
        ));
    }
    write_uint(buf, dynamic_type_index(schema, enum_name)? as u64)?;
    write_int(buf, enum_value)
}

/// Types the unplanned writers resolve after the schema's own definitions:
/// built-in aliases, `dynamic_enum`, and the handle types.
fn write_extra_type(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    if let Some(alias) = builtin_alias(ty) {
        write_brdb(schema, buf, alias, value)
    } else if ty == DYNAMIC_ENUM {
        write_dynamic_enum(schema, buf, value)
    } else if let Some(payload) = handle_struct(ty) {
        if schema.get_struct(payload).is_none() {
            return Err(BrdbSchemaError::UnsupportedType(ty.to_owned()));
        }
        write_brdb(schema, buf, payload, value)
    } else {
        Err(BrdbSchemaError::UnknownType(ty.to_owned()))
    }
}

pub fn write_bool(buf: &mut impl Write, value: bool) -> Result<(), BrdbSchemaError> {
    rmp::encode::write_bool(buf, value)?;
    Ok(())
}

pub fn write_str(buf: &mut impl Write, value: &str) -> Result<(), BrdbSchemaError> {
    rmp::encode::write_str(buf, value)?;
    Ok(())
}

pub fn write_type(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
    value: &BrdbValue,
) -> Result<(), BrdbSchemaError> {
    Ok(match (ty, value) {
        // `nil` has no payload.
        ("nil", _) => {}
        ("bool", BrdbValue::Bool(v)) => write_bool(buf, *v)?,
        ("u8", BrdbValue::U8(v)) => write_u8_for(schema, buf, *v)?,
        ("u16", BrdbValue::U16(v)) => write_uint(buf, *v as u64)?,
        ("u32", BrdbValue::U32(v)) => write_uint(buf, *v as u64)?,
        ("u64", BrdbValue::U64(v)) => write_uint(buf, *v)?,
        ("i8", BrdbValue::I8(v)) => write_int(buf, *v as i64)?,
        ("i16", BrdbValue::I16(v)) => write_int(buf, *v as i64)?,
        ("i32", BrdbValue::I32(v)) => write_int(buf, *v as i64)?,
        ("i64", BrdbValue::I64(v)) => write_int(buf, *v)?,
        ("f32", BrdbValue::F32(v)) => write_float32(buf, *v)?,
        ("f64", BrdbValue::F64(v)) => write_float64(buf, *v)?,
        ("str" | "bundle_path_ref", BrdbValue::String(v)) => write_str(buf, &v)?,
        // Latest: non-persistent references are always written as -1; EA3
        // keeps the asset index it was read with.
        ("weak_object", BrdbValue::Asset(_) | BrdbValue::Nil) if schema.is_latest() => {
            write_int(buf, -1)?
        }
        ("weak_object", BrdbValue::Asset(opt)) => {
            write_int(buf, weak_object_payload(schema, *opt))?
        }
        (DYNAMIC_ARRAY | DYNAMIC_MAP, value) => write_brdb(schema, buf, ty, value)?,
        (EA3_WIRE_GRAPH_VARIANT, BrdbValue::EA3WireVar(v)) => {
            write_ea3_wire_graph_variant(buf, v, schema.is_latest())?
        }
        (EA3_WIRE_GRAPH_PRIM_MATH_VARIANT, BrdbValue::EA3WireVar(v)) => match v {
            EA3WireVariant::Number(v) => {
                write_uint(buf, 0)?;
                write_float64(buf, *v)?;
            }
            EA3WireVariant::Int(v) => {
                write_uint(buf, 1)?;
                write_int(buf, *v)?;
            }
            other => {
                return Err(BrdbSchemaError::ExpectedType(
                    "wire_graph_prim_math_variant".to_owned(),
                    other.to_string(),
                ));
            }
        },

        // Named variant types (e.g. WireGraphVariant) encode as uint(tag) +
        // member value. Must come before the asset/struct arms below, whose
        // `_` patterns would otherwise swallow it.
        (variant_ty, _) if schema.get_variant(variant_ty).is_some() => {
            write_variant_value(schema, buf, variant_ty, value)?
        }
        ("class" | "object" | _, BrdbValue::Asset(None)) => {
            // None is -1
            write_int(buf, -1)?;
        }
        ("class" | "object" | _, BrdbValue::Asset(Some(s))) => {
            if let Some((asset_ty, _)) = schema.global_data.external_asset_references.get_index(*s)
            {
                if asset_ty != ty {
                    return Err(BrdbSchemaError::UnknownAsset(ty.to_owned(), *s));
                }
                // Assets are stored as u64 indices
                write_uint(buf, *s as u64)?;
            } else {
                return Err(BrdbSchemaError::UnknownAsset(ty.to_owned(), *s));
            }
        }
        (other, BrdbValue::Struct(_) | BrdbValue::Enum(_)) if is_schema_type(schema, other) => {
            write_named_type(schema, buf, other, value)?
        }
        (other, value) if !is_builtin_type(other) && !is_schema_type(schema, other) => {
            match builtin_alias(other) {
                Some(alias) => write_type(schema, buf, alias, value)?,
                None => write_extra_type(schema, buf, other, value)?,
            }
        }
        (expected, found) => {
            return Err(BrdbSchemaError::ExpectedType(
                expected.to_owned(),
                found.get_type().to_string(),
            ));
        }
    })
}

/// Write the EA3 `wire_graph_variant` union: selectors `[f64, i64, bool,
/// weak object, exec]`. EA3 stores an i64 asset index after selector 3;
/// `Latest` follows the spec layout, where selectors 3 and 4 have no payload.
fn write_ea3_wire_graph_variant(
    buf: &mut impl Write,
    v: &EA3WireVariant,
    latest: bool,
) -> Result<(), BrdbSchemaError> {
    match v {
        EA3WireVariant::Number(v) => {
            write_uint(buf, 0)?;
            write_float64(buf, *v)?;
        }
        EA3WireVariant::Int(v) => {
            write_uint(buf, 1)?;
            write_int(buf, *v)?;
        }
        EA3WireVariant::Bool(v) => {
            write_uint(buf, 2)?;
            write_bool(buf, *v)?;
        }
        EA3WireVariant::Object(opt) => {
            write_uint(buf, 3)?;
            if !latest {
                write_int(buf, opt.map(|i| i as i64).unwrap_or(-1))?;
            }
        }
        EA3WireVariant::Exec => {
            write_uint(buf, 4)?;
        }
        // The legacy union has no string/vector/rotator/quat/color members
        // (those are only in the newer named `WireGraphVariant` table).
        EA3WireVariant::Str(_)
        | EA3WireVariant::Vector(_)
        | EA3WireVariant::Rotator { .. }
        | EA3WireVariant::Quat { .. }
        | EA3WireVariant::LinearColor { .. } => {
            return Err(BrdbSchemaError::ExpectedType(
                "wire_graph_variant".to_owned(),
                v.to_string(),
            ));
        }
    }
    Ok(())
}

/// Find the tag (member index) of `member_name` within a named variant,
/// matching by type (so `bool` also finds a `Bool` alternative).
fn variant_member_tag(
    schema: &BrdbSchema,
    variant_ty: &str,
    member_name: &str,
) -> Result<usize, BrdbSchemaError> {
    find_alternative(schema, variant_ty, &[member_name]).map(|(tag, _)| tag)
}

/// Write a `EA3WireVariant` into a named variant type (e.g. `WireGraphVariant`,
/// `WireGraphPrimMathVariant`) as `uint(tag) + member value`, where the tag is
/// the member index resolved from the schema's variant table.
fn write_ea3_wire_variant(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    variant_ty: &str,
    v: &EA3WireVariant,
) -> Result<(), BrdbSchemaError> {
    let member_name = match v {
        EA3WireVariant::Number(_) => "f64",
        EA3WireVariant::Int(_) => "i64",
        EA3WireVariant::Bool(_) => "bool",
        EA3WireVariant::Object(_) => "weak_object",
        EA3WireVariant::Exec => "WireGraphExec",
        EA3WireVariant::Vector(..) => "Vector",
        EA3WireVariant::Rotator { .. } => "Rotator",
        EA3WireVariant::Quat { .. } => "Quat",
        EA3WireVariant::Str(_) => "str",
        EA3WireVariant::LinearColor { .. } => "LinearColor",
    };
    let tag = variant_member_tag(schema, variant_ty, member_name)?;
    write_uint(buf, tag as u64)?;
    match v {
        EA3WireVariant::Number(n) => write_float64(buf, *n)?,
        EA3WireVariant::Int(i) => write_int(buf, *i)?,
        EA3WireVariant::Bool(b) => write_bool(buf, *b)?,
        // weak_object: an asset-reference index (-1 is null); Latest writes
        // -1 for every non-persistent reference.
        EA3WireVariant::Object(opt) => write_int(buf, weak_object_payload(schema, *opt))?,
        EA3WireVariant::Exec => {} // WireGraphExec: empty struct, no payload
        // Vector is a struct {X, Y, Z: f64}; structs are written field-by-field.
        EA3WireVariant::Vector(v) => {
            write_float64(buf, v.x as f64)?;
            write_float64(buf, v.y as f64)?;
            write_float64(buf, v.z as f64)?;
        }
        // Rotator is a struct {Pitch, Yaw, Roll: f64}.
        EA3WireVariant::Rotator { pitch, yaw, roll } => {
            write_float64(buf, *pitch)?;
            write_float64(buf, *yaw)?;
            write_float64(buf, *roll)?;
        }
        // Quat is a struct {X, Y, Z, W: f64}.
        EA3WireVariant::Quat { x, y, z, w } => {
            write_float64(buf, *x)?;
            write_float64(buf, *y)?;
            write_float64(buf, *z)?;
            write_float64(buf, *w)?;
        }
        EA3WireVariant::Str(s) => write_str(buf, s)?,
        // LinearColor is a struct {R, G, B, A: f32}.
        EA3WireVariant::LinearColor { r, g, b, a } => {
            write_float32(buf, *r)?;
            write_float32(buf, *g)?;
            write_float32(buf, *b)?;
            write_float32(buf, *a)?;
        }
    }
    Ok(())
}

/// Write an arbitrary `BrdbValue` into a named variant type. A `EA3WireVar` maps to
/// a member by its variant kind; a raw value (from a round-tripped read) maps to
/// a member by its concrete type.
fn write_variant_value(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    variant_ty: &str,
    val: &BrdbValue,
) -> Result<(), BrdbSchemaError> {
    if let BrdbValue::EA3WireVar(v) = val {
        return write_ea3_wire_variant(schema, buf, variant_ty, v);
    }
    // A decoded variant carries its alternative's value, not the selector:
    // pick the alternative of this variant whose type matches the value.
    let candidates = value_alternative_candidates(val)
        .map_err(|e| e.wrap(format!("{variant_ty} alternative")))?;
    let (tag, alternative) = find_alternative(schema, variant_ty, &candidates)?;
    write_uint(buf, tag as u64)?;
    match (alternative, val) {
        // A weak_object alternative's payload is always a signed integer, as
        // for EA3 wire values.
        ("weak_object", BrdbValue::Asset(opt)) => write_int(buf, weak_object_payload(schema, *opt)),
        _ => write_brdb(schema, buf, alternative, val),
    }
}

fn write_named_type(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty_str: &str,
    value: &BrdbValue,
) -> Result<(), BrdbSchemaError> {
    match (
        schema.intern.get(ty_str),
        schema.get_struct(ty_str),
        schema.get_enum(ty_str),
        value,
    ) {
        (Some(intern_ty), Some(struct_ty), _, BrdbValue::Struct(s)) => {
            if intern_ty != s.name {
                return Err(BrdbSchemaError::ExpectedType(
                    ty_str.to_owned(),
                    schema
                        .intern
                        .lookup(s.name)
                        .unwrap_or_else(|| "unknown struct".to_owned()),
                ));
            }
            write_struct(schema, buf, struct_ty, s)
        }
        (Some(intern_ty), _, Some(enum_ty), BrdbValue::Enum(e)) => {
            if intern_ty != e.name {
                return Err(BrdbSchemaError::ExpectedType(
                    ty_str.to_owned(),
                    schema
                        .intern
                        .lookup(e.name)
                        .unwrap_or_else(|| "unknown enum".to_owned()),
                ));
            }
            write_enum(schema, buf, enum_ty, e)
        }
        _ => {
            return Err(BrdbSchemaError::UnknownType(ty_str.to_owned()));
        }
    }
}

fn write_struct(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &BrdbSchemaStruct,
    value: &BrdbStruct,
) -> Result<(), BrdbSchemaError> {
    // Write the struct properties
    for (k, prop_schema) in ty {
        let prop_val = value.properties.get(k).ok_or_else(|| {
            BrdbSchemaError::MissingStructField(
                value
                    .name
                    .get_or_else(schema, || "unknown struct".to_owned()),
                k.get_or_else(schema, || "unknown property".to_owned()),
            )
        })?;
        write_struct_property(schema, buf, prop_schema, prop_val)?;
    }
    Ok(())
}

fn write_struct_property(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    prop_schema: &BrdbSchemaStructProperty,
    value: &BrdbValue,
) -> Result<(), BrdbSchemaError> {
    let lookup = |ty: BrdbInterned| {
        ty.get_ok(schema, || {
            BrdbSchemaError::UnknownStructPropertyType(ty.0.to_string())
        })
    };

    match (prop_schema, value) {
        (BrdbSchemaStructProperty::Type(ty), value) => {
            write_type(schema, buf, &lookup(*ty)?, value)?
        }
        (BrdbSchemaStructProperty::Array(ty), BrdbValue::Array(arr)) => {
            rmp::encode::write_array_len(buf, arr.len() as u32)?;
            // Write each item in the array
            let item_ty = &lookup(*ty)?;
            for item in arr {
                write_type(schema, buf, item_ty, item)?;
            }
        }
        (BrdbSchemaStructProperty::FlatArray(ty), BrdbValue::FlatArray(arr_data)) => {
            let item_ty = lookup(*ty)?;
            let layout = flat_layout(schema, item_ty)
                .filter(|l| l.size > 0)
                .ok_or_else(|| BrdbSchemaError::InvalidFlatType(item_ty.to_owned()))?;
            // The bin length is the byte count of the whole memory dump.
            rmp::encode::write_bin_len(buf, (arr_data.len() * layout.size) as u32)?;
            for item in arr_data {
                write_flat_value(schema, buf, &layout, item)?;
            }
        }
        (BrdbSchemaStructProperty::Map(k_ty, v_ty), BrdbValue::Map(map)) => {
            // Write the number of items in the map
            rmp::encode::write_map_len(buf, map.len() as u32)?;
            // Write each key-value pair
            for (key, val) in map {
                write_type(schema, buf, &lookup(*k_ty)?, key)?;
                write_type(schema, buf, &lookup(*v_ty)?, val)?;
            }
        }
        (ty, val) => {
            return Err(BrdbSchemaError::ExpectedType(
                ty.as_string(schema),
                val.get_type().to_string(),
            ));
        }
    }
    Ok(())
}

fn write_enum(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &BrdbSchemaEnum,
    e: &BrdbEnum,
) -> Result<(), BrdbSchemaError> {
    if e.value >= ty.len() as u64 {
        return Err(BrdbSchemaError::EnumIndexOutOfBounds {
            // Unwrap safety: e.name matches a known enum sourced from the schema
            enum_name: schema.intern.lookup(e.name).unwrap(),
            index: e.value,
        });
    }
    // Write the enum index
    write_uint(buf, e.value)
}

/// Write the smallest possible integer representation of `value` to the buffer.
pub fn write_int(buf: &mut impl Write, value: i64) -> Result<(), BrdbSchemaError> {
    if value >= 0 {
        if value < 128 {
            rmp::encode::write_pfix(buf, value as u8)?;
        } else if value <= i8::MAX as i64 {
            rmp::encode::write_i8(buf, value as i8)?;
        } else if value <= i16::MAX as i64 {
            rmp::encode::write_i16(buf, value as i16)?;
        } else if value <= i32::MAX as i64 {
            rmp::encode::write_i32(buf, value as i32)?;
        } else {
            rmp::encode::write_i64(buf, value)?;
        }
    } else {
        if value > -32 {
            rmp::encode::write_nfix(buf, value as i8)?;
        } else if value >= i8::MIN as i64 {
            rmp::encode::write_i8(buf, value as i8)?;
        } else if value >= i16::MIN as i64 {
            rmp::encode::write_i16(buf, value as i16)?;
        } else if value >= i32::MIN as i64 {
            rmp::encode::write_i32(buf, value as i32)?;
        } else {
            rmp::encode::write_i64(buf, value)?;
        }
    }
    Ok(())
}

/// Write the smallest possible unsigned integer representation of `value` to the buffer.
pub fn write_u8(buf: &mut impl Write, value: u64) -> Result<(), BrdbSchemaError> {
    // The game serializes u8 as a *signed* value: positive fixint for 0..=127,
    // negative fixint for 224..=255, and int8 (marker 0xd0) for 128..=223. It
    // never emits uint8 (marker 0xcc), and its component reader rejects that
    // marker (`mpack_error_type`). Reinterpret as i8 and let msgpack pick the
    // smallest signed encoding so mid-range bytes (e.g. color channels) match.
    rmp::encode::write_sint(buf, value as u8 as i8 as i64)?;
    Ok(())
}

/// Write the smallest possible unsigned integer representation of `value` to the buffer.
pub fn write_uint(buf: &mut impl Write, value: u64) -> Result<(), BrdbSchemaError> {
    if value <= 127 {
        rmp::encode::write_pfix(buf, value as u8)?;
    } else if value <= u8::MAX as u64 {
        rmp::encode::write_u8(buf, value as u8)?;
    } else if value <= u16::MAX as u64 {
        rmp::encode::write_u16(buf, value as u16)?;
    } else if value <= u32::MAX as u64 {
        rmp::encode::write_u32(buf, value as u32)?;
    } else {
        rmp::encode::write_u64(buf, value)?;
    }
    Ok(())
}

/// Write a `u8` in the schema's format: EA3 builds only read the signed form
/// ([`write_u8`]); `Latest` writes the unsigned value.
fn write_u8_for(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    value: u8,
) -> Result<(), BrdbSchemaError> {
    if schema.is_latest() {
        write_uint(buf, value as u64)
    } else {
        write_u8(buf, value as u64)
    }
}

/// The integer a `weak_object` payload carries: the asset index (-1 for
/// null) in EA3, always -1 in `Latest`.
fn weak_object_payload(schema: &BrdbSchema, index: Option<usize>) -> i64 {
    match index {
        Some(i) if !schema.is_latest() => i as i64,
        _ => -1,
    }
}

pub fn write_float32(buf: &mut impl Write, value: f32) -> Result<(), BrdbSchemaError> {
    // Attempt to write as ints on whole numbers less than 8 or 16 bits
    if value.eq(&value.round()) && (value as u16) < u16::MAX && (value as i16) > i16::MIN {
        write_int(buf, value as i64)?;
    } else {
        rmp::encode::write_f32(buf, value)?;
    }
    Ok(())
}

pub fn write_float64(buf: &mut impl Write, value: f64) -> Result<(), BrdbSchemaError> {
    // Attempt to write as ints on whole numbers less than 8, 16, or 32 bits
    if value.eq(&value.round()) && (value as u32) < u32::MAX && (value as i32) > i32::MIN {
        write_int(buf, value as i64)?;
    } else {
        rmp::encode::write_f64(buf, value)?;
    }
    Ok(())
}

pub fn write_flat_type(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
    value: &BrdbValue,
) -> Result<(), BrdbSchemaError> {
    let layout = flat_layout(schema, ty)
        .filter(|l| l.size > 0)
        .ok_or_else(|| BrdbSchemaError::InvalidFlatType(ty.to_owned()))?;
    write_flat_value(schema, buf, &layout, value)
}

fn write_padding(buf: &mut impl Write, len: usize) -> Result<(), BrdbSchemaError> {
    const ZEROS: [u8; 16] = [0; 16];
    let mut left = len;
    while left > 0 {
        let n = left.min(ZEROS.len());
        buf.write_all(&ZEROS[..n])?;
        left -= n;
    }
    Ok(())
}

/// Write one flat element in its C layout: each field at its aligned offset,
/// zero padding between fields and up to the struct size.
pub(crate) fn write_flat_value(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    layout: &FlatLayout,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    match &layout.kind {
        FlatKind::U8 => write_flat_u8(buf, value.as_brdb_u8()?),
        FlatKind::U16 => write_flat_u16(buf, value.as_brdb_u16()?),
        FlatKind::U32 => write_flat_u32(buf, value.as_brdb_u32()?),
        FlatKind::U64 => write_flat_u64(buf, value.as_brdb_u64()?),
        FlatKind::I8 => write_flat_i8(buf, value.as_brdb_i8()?),
        FlatKind::I16 => write_flat_i16(buf, value.as_brdb_i16()?),
        FlatKind::I32 => write_flat_i32(buf, value.as_brdb_i32()?),
        FlatKind::I64 => write_flat_i64(buf, value.as_brdb_i64()?),
        FlatKind::F32 => write_flat_f32(buf, value.as_brdb_f32()?),
        FlatKind::F64 => write_flat_f64(buf, value.as_brdb_f64()?),
        FlatKind::Struct(s_id, fields) => {
            if let Some(BrdbValue::Struct(s)) = value.as_brdb_value()
                && s.name != *s_id
            {
                return Err(BrdbSchemaError::ExpectedType(
                    s_id.get_or(schema, "unknown struct").to_owned(),
                    s.get_name().to_owned(),
                ));
            }
            let mut pos = 0;
            for (prop_id, offset, field) in fields {
                write_padding(buf, offset - pos)?;
                let prop_val = value.as_brdb_struct_prop_value(schema, *s_id, *prop_id)?;
                write_flat_value(schema, buf, field, prop_val)?;
                pos = offset + field.size;
            }
            write_padding(buf, layout.size - pos)
        }
    }
}

fn write_flat_u8(buf: &mut impl Write, value: u8) -> Result<(), BrdbSchemaError> {
    buf.write_all(&[value])?;
    Ok(())
}
fn write_flat_u16(buf: &mut impl Write, value: u16) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_flat_u32(buf: &mut impl Write, value: u32) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_flat_u64(buf: &mut impl Write, value: u64) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_flat_i8(buf: &mut impl Write, value: i8) -> Result<(), BrdbSchemaError> {
    buf.write_all(&[value as u8])?;
    Ok(())
}
fn write_flat_i16(buf: &mut impl Write, value: i16) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_flat_i32(buf: &mut impl Write, value: i32) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_flat_i64(buf: &mut impl Write, value: i64) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_flat_f32(buf: &mut impl Write, value: f32) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_flat_f64(buf: &mut impl Write, value: f64) -> Result<(), BrdbSchemaError> {
    buf.write_all(&value.to_le_bytes())?;
    Ok(())
}

type DefaultsMap = rustc_hash::FxHashMap<
    &'static str,
    rustc_hash::FxHashMap<&'static str, &'static dyn AsBrdbValue>,
>;

type DefaultsTable =
    std::sync::LazyLock<Vec<(&'static str, Vec<(&'static str, Box<dyn AsBrdbValue>)>)>>;

fn defaults_map(table: &'static DefaultsTable) -> DefaultsMap {
    std::sync::LazyLock::force(table)
        .iter()
        .map(|(name, fields)| {
            (
                *name,
                fields
                    .iter()
                    .map(|(n, v)| (*n, v.as_ref() as &'static dyn AsBrdbValue))
                    .collect(),
            )
        })
        .collect()
}

/// O(1) lookup into the generated `STRUCT_DEFAULTS` table, replacing a
/// linear scan over all structs (and their fields) per unset field. Entity data structs fall back to the hand-kept
/// `assets::entities::entity_field_default`.
fn struct_field_default(
    _schema: &BrdbSchema,
    struct_name: &str,
    field: &str,
) -> Option<&'static dyn AsBrdbValue> {
    use std::sync::LazyLock;
    static CURRENT: LazyLock<DefaultsMap> =
        LazyLock::new(|| defaults_map(&crate::wrapper::component_db::STRUCT_DEFAULTS));
    CURRENT
        .get(struct_name)
        .and_then(|m| m.get(field))
        .copied()
        .or_else(|| crate::assets::entities::entity_field_default(struct_name, field))
}

/// Write the default (or zero) value for an unset struct field.
fn write_struct_field_default(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    struct_name: &str,
    field_name: &str,
    ty_str: &str,
) -> Result<(), BrdbSchemaError> {
    if let Some(default_val) = struct_field_default(schema, struct_name, field_name) {
        write_brdb(schema, buf, ty_str, default_val)
    } else {
        write_brdb_zero(schema, buf, ty_str)
    }
}

/// Resolve `ty` to a schema struct only when `write_brdb` would take the
/// struct path for it: not a primitive/special name and not a variant.
/// Lets array loops resolve their element type once instead of per element.
fn resolve_plain_struct<'a>(
    schema: &'a BrdbSchema,
    ty: &str,
) -> Option<(BrdbInterned, &'a BrdbSchemaStruct)> {
    match ty {
        ty if is_builtin_type(ty) => None,
        _ => {
            let id = schema.intern.get(ty)?;
            if schema.get_variant_interned(id).is_some() {
                return None;
            }
            Some((id, schema.get_struct_interned(id)?))
        }
    }
}

/// Write the fields of a schema struct value (the struct arm of `write_brdb`,
/// with the struct already resolved to its interned id and property table).
fn write_brdb_struct_fields(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    s_id: BrdbInterned,
    s_ty: &BrdbSchemaStruct,
    ty_name: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    let lookup = |ty: BrdbInterned| {
        schema
            .intern
            .lookup_ref(ty)
            .ok_or(BrdbSchemaError::UnknownType(ty.0.to_string()))
    };

    for (prop_id, prop_schema) in s_ty {
        match prop_schema {
            BrdbSchemaStructProperty::Type(ty_id) => {
                let ty_str = lookup(*ty_id)?;
                match value.as_brdb_struct_prop_opt(schema, s_id, *prop_id)? {
                    Some(prop_value) => {
                        write_brdb(schema, buf, ty_str, prop_value)?;
                    }
                    None => {
                        write_struct_field_default(
                            schema,
                            buf,
                            ty_name,
                            lookup(*prop_id)?,
                            ty_str,
                        )?;
                    }
                }
            }
            BrdbSchemaStructProperty::Array(ty_id) => {
                if !value.has_brdb_struct_prop(schema, s_id, *prop_id) {
                    rmp::encode::write_array_len(buf, 0)?;
                    continue;
                }
                match value.as_brdb_struct_prop_array(schema, s_id, *prop_id) {
                    Ok(prop_values) => {
                        let ty = lookup(*ty_id)?;
                        rmp::encode::write_array_len(buf, prop_values.len() as u32)?;
                        if let Some((elem_id, elem_ty)) = resolve_plain_struct(schema, ty) {
                            for prop_value in prop_values {
                                write_brdb_struct_fields(
                                    schema,
                                    buf,
                                    elem_id,
                                    elem_ty,
                                    ty,
                                    &*prop_value,
                                )?;
                            }
                        } else {
                            for prop_value in prop_values {
                                write_brdb(schema, buf, ty, &*prop_value)?;
                            }
                        }
                    }
                    Err(BrdbSchemaError::MissingStructField(..)) => {
                        rmp::encode::write_array_len(buf, 0)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            BrdbSchemaStructProperty::FlatArray(ty_id) => {
                if !value.has_brdb_struct_prop(schema, s_id, *prop_id) {
                    rmp::encode::write_bin_len(buf, 0)?;
                    continue;
                }
                match value.as_brdb_struct_prop_array(schema, s_id, *prop_id) {
                    Ok(prop_values) => {
                        let ty = lookup(*ty_id)?;
                        let layout = flat_layout(schema, ty);
                        let size = layout.as_ref().map_or(0, |l| l.size);
                        rmp::encode::write_bin_len(buf, (prop_values.len() * size) as u32)?;
                        for prop_value in prop_values {
                            let Some(layout) = &layout else {
                                return Err(BrdbSchemaError::InvalidFlatType(ty.to_owned()));
                            };
                            write_flat_value(schema, buf, layout, &*prop_value)?;
                        }
                    }
                    Err(BrdbSchemaError::MissingStructField(..)) => {
                        rmp::encode::write_bin_len(buf, 0)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            BrdbSchemaStructProperty::Map(k_ty_id, v_ty_id) => {
                if !value.has_brdb_struct_prop(schema, s_id, *prop_id) {
                    rmp::encode::write_map_len(buf, 0)?;
                    continue;
                }
                match value.as_brdb_struct_prop_map(schema, s_id, *prop_id) {
                    Ok(prop_values) => {
                        let k_ty = lookup(*k_ty_id)?;
                        let v_ty = lookup(*v_ty_id)?;
                        rmp::encode::write_map_len(buf, prop_values.len() as u32)?;
                        for (key, val) in prop_values {
                            write_brdb(schema, buf, k_ty, &*key)?;
                            write_brdb(schema, buf, v_ty, &*val)?;
                        }
                    }
                    Err(BrdbSchemaError::MissingStructField(..)) => {
                        rmp::encode::write_map_len(buf, 0)?;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(())
}

pub fn write_brdb(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    match ty {
        // `nil` has no payload.
        "nil" => {}
        "bool" => write_bool(buf, value.as_brdb_bool()?)?,
        "u8" => write_u8_for(schema, buf, value.as_brdb_u8()?)?,
        "u16" => write_uint(buf, value.as_brdb_u16()? as u64)?,
        "u32" => write_uint(buf, value.as_brdb_u32()? as u64)?,
        "u64" => write_uint(buf, value.as_brdb_u64()?)?,
        "i8" => write_int(buf, value.as_brdb_i8()? as i64)?,
        "i16" => write_int(buf, value.as_brdb_i16()? as i64)?,
        "i32" => write_int(buf, value.as_brdb_i32()? as i64)?,
        "i64" => write_int(buf, value.as_brdb_i64()?)?,
        "f32" => write_float32(buf, value.as_brdb_f32()?)?,
        "f64" => write_float64(buf, value.as_brdb_f64()?)?,
        "str" => write_str(buf, value.as_brdb_str()?)?,
        EA3_WIRE_GRAPH_VARIANT => write_ea3_wire_graph_variant(
            buf,
            &value.as_brdb_ea3_wire_variant()?,
            schema.is_latest(),
        )?,
        EA3_WIRE_GRAPH_PRIM_MATH_VARIANT => match value.as_brdb_ea3_wire_variant()? {
            EA3WireVariant::Number(v) => {
                write_uint(buf, 0)?;
                write_float64(buf, v)?;
            }
            EA3WireVariant::Int(v) => {
                write_uint(buf, 1)?;
                write_int(buf, v)?;
            }
            other => {
                return Err(BrdbSchemaError::ExpectedType(
                    EA3_WIRE_GRAPH_PRIM_MATH_VARIANT.to_owned(),
                    other.to_string(),
                ));
            }
        },
        "bundle_path_ref" => write_str(buf, value.as_brdb_str().unwrap_or(""))?,
        // Latest: non-persistent references are always written as -1.
        "weak_object" if schema.is_latest() => write_int(buf, -1)?,
        "class" | "object" | "weak_object" => {
            let asset_index = value.as_brdb_asset(schema, ty)?;
            if let Some(asset_index) = asset_index {
                write_uint(buf, asset_index as u64)?;
            } else {
                write_int(buf, -1)?;
            }
        }
        DYNAMIC_ARRAY => write_dynamic_array(schema, buf, value)?,
        DYNAMIC_MAP => write_dynamic_map(schema, buf, value)?,
        other => {
            // Resolve the type name through the interner once; variant, struct
            // and enum tables are then probed by interned id.
            let Some(s_id) = schema.intern.get(other) else {
                return write_extra_type(schema, buf, other, value);
            };
            if schema.get_variant_interned(s_id).is_some() {
                // Struct-member variants (e.g. BRInventoryEntryVariant, or a
                // `WireValue`) self-identify by member name; a decoded value
                // picks the alternative matching its own type; everything
                // else is an EA3 scalar wire variant.
                if let Some(member) = value.as_brdb_variant_member() {
                    let (tag, alternative) = find_alternative(schema, other, &[member])?;
                    write_uint(buf, tag as u64)?;
                    // `value` re-encodes as its member (e.g. its struct fields).
                    write_brdb(schema, buf, alternative, value)?;
                } else if let Some(raw) = value
                    .as_brdb_value()
                    .filter(|v| !matches!(v, BrdbValue::EA3WireVar(_)))
                {
                    write_variant_value(schema, buf, other, raw)?;
                } else {
                    write_ea3_wire_variant(schema, buf, other, &value.as_brdb_ea3_wire_variant()?)?;
                }
            } else if let Some(s_ty) = schema.get_struct_interned(s_id) {
                write_brdb_struct_fields(schema, buf, s_id, s_ty, other, value)?;
            } else if let Some(enum_ty) = schema.get_enum_interned(s_id) {
                let enum_value = value.as_brdb_enum(schema, enum_ty)?;
                if enum_value >= enum_ty.len() as i32 {
                    return Err(BrdbSchemaError::EnumIndexOutOfBounds {
                        enum_name: other.to_owned(),
                        index: enum_value as u64,
                    });
                }
                write_uint(buf, enum_value as u64)?;
            } else {
                write_extra_type(schema, buf, other, value)?;
            }
        }
    }
    Ok(())
}

/// Write component data structs. Same as `write_brdb` but encodes `str`
/// fields as `uint(len) + raw bytes` ("wire graph str") instead of
/// standard msgpack str — the game's component reader expects this format.
pub fn write_brdb_component(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    // Override str to use wire graph str encoding
    if ty == "str" {
        let s = value.as_brdb_str()?;
        write_uint(buf, s.len() as u64)?;
        buf.write_all(s.as_bytes())?;
        return Ok(());
    }
    // For struct types, recurse with component encoding for nested fields
    let lookup = |ty: BrdbInterned| {
        schema
            .intern
            .lookup_ref(ty)
            .ok_or(BrdbSchemaError::UnknownType(ty.0.to_string()))
    };
    if let (Some(s_id), Some(s_ty)) = (schema.intern.get(ty), schema.get_struct(ty)) {
        for (prop_id, prop_schema) in s_ty {
            match prop_schema {
                BrdbSchemaStructProperty::Type(ty_id) => {
                    let prop_value = value.as_brdb_struct_prop_value(schema, s_id, *prop_id)?;
                    write_brdb_component(schema, buf, &lookup(*ty_id)?, &*prop_value)?;
                }
                BrdbSchemaStructProperty::Array(ty_id) => {
                    let ty = &lookup(*ty_id)?;
                    let prop_values = value.as_brdb_struct_prop_array(schema, s_id, *prop_id)?;
                    rmp::encode::write_array_len(buf, prop_values.len() as u32)?;
                    for prop_value in prop_values {
                        write_brdb_component(schema, buf, ty, &*prop_value)?;
                    }
                }
                _ => {
                    // Fall back to standard write for other field types
                    let prop_value = value.as_brdb_struct_prop_value(schema, s_id, *prop_id)?;
                    write_brdb(schema, buf, ty, &*prop_value)?;
                }
            }
        }
        return Ok(());
    }
    // Fall back to standard write for non-str, non-struct types
    write_brdb(schema, buf, ty, value)
}

pub fn write_brdb_flat(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    let layout =
        flat_layout(schema, ty).ok_or_else(|| BrdbSchemaError::InvalidFlatType(ty.to_owned()))?;
    write_flat_value(schema, buf, &layout, value)
}

/// Write a zero/default value for any type. Used when a struct field is
/// missing from the component data — the engine zero-initialises fields
/// before loading, so writing zeros is safe.
pub fn write_brdb_zero(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
) -> Result<(), BrdbSchemaError> {
    match ty {
        "nil" => {}
        "bool" => write_bool(buf, false)?,
        "u8" => write_u8(buf, 0)?,
        "u16" | "u32" | "u64" => write_uint(buf, 0)?,
        "i8" | "i16" | "i32" | "i64" => write_int(buf, 0)?,
        "f32" => write_float32(buf, 0.0)?,
        "f64" => write_float64(buf, 0.0)?,
        "str" => write_str(buf, "")?,
        EA3_WIRE_GRAPH_VARIANT => {
            write_uint(buf, 0)?; // Number
            write_float64(buf, 0.0)?;
        }
        EA3_WIRE_GRAPH_PRIM_MATH_VARIANT => {
            write_uint(buf, 0)?; // Number
            write_float64(buf, 0.0)?;
        }
        "bundle_path_ref" => write_str(buf, "")?,
        "class" | "object" | "weak_object" => write_int(buf, -1)?,
        // An unset dynamic container is empty, with the element (key/value)
        // types the game gives a new array (map) variable.
        DYNAMIC_ARRAY => {
            write_uint(buf, default_type_index(schema, "f64")? as u64)?;
            rmp::encode::write_array_len(buf, 0)?;
        }
        DYNAMIC_MAP => {
            write_uint(buf, default_type_index(schema, "i64")? as u64)?;
            write_uint(buf, default_type_index(schema, "f64")? as u64)?;
            rmp::encode::write_map_len(buf, 0)?;
        }
        other if schema.get_variant(other).is_some() => {
            // Zero a named variant as tag 0 + the zero value of its first member.
            let members = schema.get_variant(other).unwrap();
            write_uint(buf, 0)?;
            if let Some(first) = members.first() {
                let inner = schema
                    .intern
                    .lookup_ref(*first)
                    .ok_or_else(|| BrdbSchemaError::UnknownType(first.0.to_string()))?;
                write_brdb_zero(schema, buf, inner)?;
            }
        }
        other => {
            if let Some(s_ty) = schema.get_struct(other) {
                for (_, prop_schema) in s_ty {
                    match prop_schema {
                        BrdbSchemaStructProperty::Type(ty_id) => {
                            let inner = schema
                                .intern
                                .lookup_ref(*ty_id)
                                .ok_or_else(|| BrdbSchemaError::UnknownType(ty_id.0.to_string()))?;
                            write_brdb_zero(schema, buf, &inner)?;
                        }
                        BrdbSchemaStructProperty::Array(_) => {
                            rmp::encode::write_array_len(buf, 0)?;
                        }
                        BrdbSchemaStructProperty::FlatArray(_) => {
                            rmp::encode::write_bin_len(buf, 0)?;
                        }
                        BrdbSchemaStructProperty::Map(_, _) => {
                            rmp::encode::write_map_len(buf, 0)?;
                        }
                    }
                }
            } else if schema.get_enum(other).is_some() {
                write_uint(buf, 0)?;
            } else if let Some(alias) = builtin_alias(other) {
                write_brdb_zero(schema, buf, alias)?;
            } else if other == DYNAMIC_ENUM {
                // The first enum in the types table, value 0.
                let index = (0..schema.types.len())
                    .find(|i| {
                        schema
                            .type_at(*i)
                            .is_some_and(|t| schema.get_enum(t).is_some())
                    })
                    .ok_or_else(|| {
                        BrdbSchemaError::ExpectedType(
                            "an enum in the types table".to_owned(),
                            DYNAMIC_ENUM.to_owned(),
                        )
                    })?;
                write_uint(buf, index as u64)?;
                write_int(buf, 0)?;
            } else if let Some(payload) = handle_struct(other) {
                if schema.get_struct(payload).is_none() {
                    return Err(BrdbSchemaError::UnsupportedType(other.to_owned()));
                }
                write_brdb_zero(schema, buf, payload)?;
            } else {
                return Err(BrdbSchemaError::UnknownType(other.to_owned()));
            }
        }
    }
    Ok(())
}

/// Expected shape of one struct property, for `struct_shape_matches`.
pub(crate) enum ShapeProp {
    Ty(&'static str),
    Arr(&'static str),
    Flat(&'static str),
}

/// Whether `name` resolves to a struct whose properties exactly match
/// `expect` in order, name and shape. Specialized SoA writers use this to
/// verify the loaded schema still has the shape they hardcode, falling back
/// to the generic writer otherwise.
pub(crate) fn struct_shape_matches(
    schema: &BrdbSchema,
    name: &str,
    expect: &[(&str, ShapeProp)],
) -> bool {
    let Some(s) = schema.get_struct(name) else {
        return false;
    };
    if s.len() != expect.len() {
        return false;
    }
    for ((prop_id, prop), (e_name, e_shape)) in s.iter().zip(expect) {
        if schema.intern.lookup_ref(*prop_id) != Some(*e_name) {
            return false;
        }
        let ok = match (prop, e_shape) {
            (BrdbSchemaStructProperty::Type(t), ShapeProp::Ty(en)) => {
                schema.intern.lookup_ref(*t) == Some(*en)
            }
            (BrdbSchemaStructProperty::Array(t), ShapeProp::Arr(en)) => {
                schema.intern.lookup_ref(*t) == Some(*en)
            }
            (BrdbSchemaStructProperty::FlatArray(t), ShapeProp::Flat(en)) => {
                schema.intern.lookup_ref(*t) == Some(*en)
            }
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Plan-cached counterpart of `write_brdb`: identical bytes and errors, but
/// per-type resolution (field types, variant tags, defaults) happens once per
/// cache instead of per instance.
pub(crate) fn write_brdb_cached<'a>(
    schema: &'a BrdbSchema,
    cache: &mut PlanCache<'a>,
    buf: &mut impl Write,
    ty: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    match ty {
        // Primitive and special names take the unplanned writer directly;
        // they must be checked before the interned tables to keep
        // `write_brdb`'s dispatch precedence.
        ty if is_builtin_type(ty) => write_brdb(schema, buf, ty, value),
        other => {
            let Some(s_id) = schema.intern.get(other) else {
                return Err(BrdbSchemaError::UnknownType(other.to_owned()));
            };
            let Some(name) = schema.intern.lookup_ref(s_id) else {
                return write_brdb(schema, buf, other, value);
            };
            let op = cache.resolve_named(s_id, name);
            write_op(schema, cache, buf, &op, value)
        }
    }
}

/// `write_brdb_cached` with a one-entry memo over `ty`. Component and entity
/// data is grouped by type, so consecutive instances resolve for free.
pub(crate) fn write_brdb_memoized<'a>(
    schema: &'a BrdbSchema,
    cache: &mut PlanCache<'a>,
    memo: &mut TopLevelMemo<'a>,
    buf: &mut impl Write,
    ty: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    if let Some((p, l, op)) = &memo.last {
        if *p == ty.as_ptr() && *l == ty.len() {
            let op = op.clone();
            return write_op(schema, cache, buf, &op, value);
        }
    }
    match ty {
        ty if is_builtin_type(ty) => write_brdb(schema, buf, ty, value),
        other => {
            let Some(s_id) = schema.intern.get(other) else {
                return Err(BrdbSchemaError::UnknownType(other.to_owned()));
            };
            let Some(name) = schema.intern.lookup_ref(s_id) else {
                return write_brdb(schema, buf, other, value);
            };
            let op = cache.resolve_named(s_id, name);
            memo.last = Some((ty.as_ptr(), ty.len(), op.clone()));
            write_op(schema, cache, buf, &op, value)
        }
    }
}

fn write_op<'a>(
    schema: &'a BrdbSchema,
    cache: &mut PlanCache<'a>,
    buf: &mut impl Write,
    op: &PlanOp<'a>,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    match op {
        PlanOp::Nil => Ok(()),
        PlanOp::Bool => write_bool(buf, value.as_brdb_bool()?),
        PlanOp::U8 => write_u8_for(schema, buf, value.as_brdb_u8()?),
        PlanOp::U16 => write_uint(buf, value.as_brdb_u16()? as u64),
        PlanOp::U32 => write_uint(buf, value.as_brdb_u32()? as u64),
        PlanOp::U64 => write_uint(buf, value.as_brdb_u64()?),
        PlanOp::I8 => write_int(buf, value.as_brdb_i8()? as i64),
        PlanOp::I16 => write_int(buf, value.as_brdb_i16()? as i64),
        PlanOp::I32 => write_int(buf, value.as_brdb_i32()? as i64),
        PlanOp::I64 => write_int(buf, value.as_brdb_i64()?),
        PlanOp::F32 => write_float32(buf, value.as_brdb_f32()?),
        PlanOp::F64 => write_float64(buf, value.as_brdb_f64()?),
        PlanOp::Str => write_str(buf, value.as_brdb_str()?),
        PlanOp::EA3WireGraphVariant => match value.as_brdb_ea3_wire_variant_ref() {
            Some(v) => write_ea3_wire_graph_variant(buf, v, schema.is_latest()),
            None => write_ea3_wire_graph_variant(
                buf,
                &value.as_brdb_ea3_wire_variant()?,
                schema.is_latest(),
            ),
        },
        PlanOp::EA3WireGraphPrimMathVariant => {
            let owned;
            let v = match value.as_brdb_ea3_wire_variant_ref() {
                Some(v) => v,
                None => {
                    owned = value.as_brdb_ea3_wire_variant()?;
                    &owned
                }
            };
            match v {
                EA3WireVariant::Number(v) => {
                    write_uint(buf, 0)?;
                    write_float64(buf, *v)
                }
                EA3WireVariant::Int(v) => {
                    write_uint(buf, 1)?;
                    write_int(buf, *v)
                }
                other => Err(BrdbSchemaError::ExpectedType(
                    "wire_graph_prim_math_variant".to_owned(),
                    other.to_string(),
                )),
            }
        }
        PlanOp::BundlePathRef => write_str(buf, value.as_brdb_str().unwrap_or("")),
        PlanOp::Asset("weak_object") if schema.is_latest() => write_int(buf, -1),
        PlanOp::Asset(ty) => match value.as_brdb_asset(schema, ty)? {
            Some(asset_index) => write_uint(buf, asset_index as u64),
            None => write_int(buf, -1),
        },
        PlanOp::Enum { name, def } => {
            let enum_value = value.as_brdb_enum(schema, def)?;
            if enum_value >= def.len() as i32 {
                return Err(BrdbSchemaError::EnumIndexOutOfBounds {
                    enum_name: (*name).to_owned(),
                    index: enum_value as u64,
                });
            }
            write_uint(buf, enum_value as u64)
        }
        PlanOp::Struct(plan) => {
            let plan = plan.clone();
            write_struct_plan(schema, cache, buf, &plan, value)
        }
        PlanOp::Deferred(id, name) => match cache.struct_plan(*id, name) {
            Some(plan) => write_struct_plan(schema, cache, buf, &plan, value),
            None => write_brdb(schema, buf, name, value),
        },
        PlanOp::Variant(id, name) => {
            let plan = cache.variant_plan(*id, name);
            write_variant_plan(schema, cache, buf, &plan, value)
        }
        PlanOp::Fallback(ty) => write_brdb(schema, buf, ty, value),
    }
}

fn write_struct_plan<'a>(
    schema: &'a BrdbSchema,
    cache: &mut PlanCache<'a>,
    buf: &mut impl Write,
    plan: &StructPlan<'a>,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    // An instance with no set fields writes a constant byte stream; skip the
    // per-field probes entirely when it was precomputable.
    if let Some(all) = &plan.all_default_bytes {
        if !value.has_brdb_struct_props() {
            buf.write_all(all)?;
            return Ok(());
        }
    }
    for field in &plan.fields {
        match &field.kind {
            FieldKind::Scalar {
                op,
                ty_str,
                default_bytes,
            } => match value.as_brdb_struct_prop_opt(schema, plan.s_id, field.prop_id)? {
                Some(prop_value) => write_op(schema, cache, buf, op, prop_value)?,
                None => match default_bytes {
                    Some(bytes) => buf.write_all(bytes)?,
                    None => {
                        write_struct_field_default(schema, buf, plan.name, field.prop_name, ty_str)?
                    }
                },
            },
            FieldKind::Array { elem } => {
                if !value.has_brdb_struct_prop(schema, plan.s_id, field.prop_id) {
                    rmp::encode::write_array_len(buf, 0)?;
                    continue;
                }
                match value.as_brdb_struct_prop_array(schema, plan.s_id, field.prop_id) {
                    Ok(prop_values) => {
                        rmp::encode::write_array_len(buf, prop_values.len() as u32)?;
                        for prop_value in prop_values {
                            write_op(schema, cache, buf, elem, &*prop_value)?;
                        }
                    }
                    Err(BrdbSchemaError::MissingStructField(..)) => {
                        rmp::encode::write_array_len(buf, 0)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            FieldKind::FlatArray { elem, elem_size } => {
                if !value.has_brdb_struct_prop(schema, plan.s_id, field.prop_id) {
                    rmp::encode::write_bin_len(buf, 0)?;
                    continue;
                }
                match value.as_brdb_struct_prop_array(schema, plan.s_id, field.prop_id) {
                    Ok(prop_values) => {
                        rmp::encode::write_bin_len(buf, (prop_values.len() * elem_size) as u32)?;
                        for prop_value in prop_values {
                            write_flat_op(schema, buf, elem, &*prop_value)?;
                        }
                    }
                    Err(BrdbSchemaError::MissingStructField(..)) => {
                        rmp::encode::write_bin_len(buf, 0)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            FieldKind::Map { key, value: val_op } => {
                if !value.has_brdb_struct_prop(schema, plan.s_id, field.prop_id) {
                    rmp::encode::write_map_len(buf, 0)?;
                    continue;
                }
                match value.as_brdb_struct_prop_map(schema, plan.s_id, field.prop_id) {
                    Ok(prop_values) => {
                        rmp::encode::write_map_len(buf, prop_values.len() as u32)?;
                        for (k, v) in prop_values {
                            write_op(schema, cache, buf, key, &*k)?;
                            write_op(schema, cache, buf, val_op, &*v)?;
                        }
                    }
                    Err(BrdbSchemaError::MissingStructField(..)) => {
                        rmp::encode::write_map_len(buf, 0)?;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(())
}

fn write_variant_plan<'a>(
    schema: &'a BrdbSchema,
    cache: &mut PlanCache<'a>,
    buf: &mut impl Write,
    plan: &VariantPlan<'a>,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    if let Some(member) = value.as_brdb_variant_member() {
        // Alternatives match by type, so `bool` also finds `Bool`.
        let Some(tag) = plan
            .members
            .iter()
            .position(|(name, _)| *name == Some(member))
            .or_else(|| {
                plan.members
                    .iter()
                    .position(|(name, _)| name.is_some_and(|n| same_type(n, member)))
            })
        else {
            return Err(BrdbSchemaError::ExpectedType(
                format!("{} member", plan.name),
                member.to_owned(),
            ));
        };
        write_uint(buf, tag as u64)?;
        let op = plan.members[tag].1.clone();
        write_op(schema, cache, buf, &op, value)
    } else if let Some(raw) = value
        .as_brdb_value()
        .filter(|v| !matches!(v, BrdbValue::EA3WireVar(_)))
    {
        write_variant_value(schema, buf, plan.name, raw)
    } else if let Some(v) = value.as_brdb_ea3_wire_variant_ref() {
        write_ea3_wire_variant_planned(schema, buf, plan, v)
    } else {
        write_ea3_wire_variant_planned(schema, buf, plan, &value.as_brdb_ea3_wire_variant()?)
    }
}

fn write_ea3_wire_variant_planned(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    plan: &VariantPlan,
    v: &EA3WireVariant,
) -> Result<(), BrdbSchemaError> {
    let (tag, member_name) = match v {
        EA3WireVariant::Number(_) => (plan.ea3_wire.number, "f64"),
        EA3WireVariant::Int(_) => (plan.ea3_wire.int, "i64"),
        EA3WireVariant::Bool(_) => (plan.ea3_wire.boolean, "bool"),
        EA3WireVariant::Object(_) => (plan.ea3_wire.object, "weak_object"),
        EA3WireVariant::Exec => (plan.ea3_wire.exec, "WireGraphExec"),
        EA3WireVariant::Vector(..) => (plan.ea3_wire.vector, "Vector"),
        EA3WireVariant::Rotator { .. } => (plan.ea3_wire.rotator, "Rotator"),
        EA3WireVariant::Quat { .. } => (plan.ea3_wire.quat, "Quat"),
        EA3WireVariant::Str(_) => (plan.ea3_wire.string, "str"),
        EA3WireVariant::LinearColor { .. } => (plan.ea3_wire.linear_color, "LinearColor"),
    };
    let Some(tag) = tag else {
        return Err(BrdbSchemaError::ExpectedType(
            format!("{} member", plan.name),
            member_name.to_owned(),
        ));
    };
    write_uint(buf, tag)?;
    match v {
        EA3WireVariant::Number(n) => write_float64(buf, *n)?,
        EA3WireVariant::Int(i) => write_int(buf, *i)?,
        EA3WireVariant::Bool(b) => write_bool(buf, *b)?,
        EA3WireVariant::Object(opt) => write_int(buf, weak_object_payload(schema, *opt))?,
        EA3WireVariant::Exec => {}
        EA3WireVariant::Vector(v) => {
            write_float64(buf, v.x as f64)?;
            write_float64(buf, v.y as f64)?;
            write_float64(buf, v.z as f64)?;
        }
        EA3WireVariant::Rotator { pitch, yaw, roll } => {
            write_float64(buf, *pitch)?;
            write_float64(buf, *yaw)?;
            write_float64(buf, *roll)?;
        }
        EA3WireVariant::Quat { x, y, z, w } => {
            write_float64(buf, *x)?;
            write_float64(buf, *y)?;
            write_float64(buf, *z)?;
            write_float64(buf, *w)?;
        }
        EA3WireVariant::Str(s) => write_str(buf, s)?,
        EA3WireVariant::LinearColor { r, g, b, a } => {
            write_float32(buf, *r)?;
            write_float32(buf, *g)?;
            write_float32(buf, *b)?;
            write_float32(buf, *a)?;
        }
    }
    Ok(())
}

fn write_flat_op(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    op: &FlatOp,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    match op {
        FlatOp::U8 => write_flat_u8(buf, value.as_brdb_u8()?),
        FlatOp::U16 => write_flat_u16(buf, value.as_brdb_u16()?),
        FlatOp::U32 => write_flat_u32(buf, value.as_brdb_u32()?),
        FlatOp::U64 => write_flat_u64(buf, value.as_brdb_u64()?),
        FlatOp::I8 => write_flat_i8(buf, value.as_brdb_i8()?),
        FlatOp::I16 => write_flat_i16(buf, value.as_brdb_i16()?),
        FlatOp::I32 => write_flat_i32(buf, value.as_brdb_i32()?),
        FlatOp::I64 => write_flat_i64(buf, value.as_brdb_i64()?),
        FlatOp::F32 => write_flat_f32(buf, value.as_brdb_f32()?),
        FlatOp::F64 => write_flat_f64(buf, value.as_brdb_f64()?),
        FlatOp::Struct(plan) => {
            for (prop_id, prop_op, pad) in &plan.props {
                write_padding(buf, *pad)?;
                let prop_val = value.as_brdb_struct_prop_value(schema, plan.s_id, *prop_id)?;
                write_flat_op(schema, buf, prop_op, &*prop_val)?;
            }
            write_padding(buf, plan.tail_pad)
        }
        FlatOp::Fallback(ty) => write_brdb_flat(schema, buf, ty, value),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_write_uint() {
        // write ints from 0 to 512
        let mut buf = Vec::new();
        for i in 0..512 {
            buf.clear();
            super::write_uint(&mut buf, i).unwrap();
        }
    }

    #[test]
    fn test_write_u8() {
        // write ints from 0 to 512
        let mut buf = Vec::new();
        for i in 0..256 {
            buf.clear();
            super::write_u8(&mut buf, i).unwrap();
        }
    }

    #[test]
    fn test_map_property_reads_msgpack_map_header() {
        use crate::schema::{BrdbSchema, BrdbValue, read::read_type};
        use std::sync::Arc;

        let schema = Arc::new(
            BrdbSchema::new_parsed(
                "struct Holder {
    M: {i64: f64},
}
",
            )
            .unwrap(),
        );

        // An empty map is a msgpack map header FixMap(0) = 0x80 — what the game
        // (and this crate's writer) emit. The reader must accept the map header,
        // not demand a plain uint (which rejected the marker and broke reading
        // any MapVar component, e.g. `read_components` on a real world).
        let val = read_type(&schema, "Holder", &mut [0x80u8].as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::Struct(_)));
    }

    #[test]
    fn test_named_variant_round_trip() {
        use crate::schema::{BrdbSchema, BrdbValue, EA3WireVariant, read::read_type};
        use std::sync::Arc;

        let schema = Arc::new(
            BrdbSchema::new_parsed(
                "variant WireGraphVariant {
    f64,
    i64,
    bool,
    Vector,
}
struct Vector {
    X: f64,
    Y: f64,
    Z: f64,
}
",
            )
            .unwrap(),
        );

        // BrdbValue path (write_type): Number -> f64 member (tag 0).
        let mut buf = Vec::new();
        super::write_type(
            &schema,
            &mut buf,
            "WireGraphVariant",
            &BrdbValue::EA3WireVar(EA3WireVariant::Number(3.5)),
        )
        .unwrap();
        assert_eq!(buf[0], 0); // tag 0
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::F64(n) if n == 3.5));

        // BrdbValue path: Bool -> bool member (tag 2).
        let mut buf = Vec::new();
        super::write_type(
            &schema,
            &mut buf,
            "WireGraphVariant",
            &BrdbValue::EA3WireVar(EA3WireVariant::Bool(true)),
        )
        .unwrap();
        assert_eq!(buf[0], 2); // tag 2
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::Bool(true)));

        // AsBrdbValue path (write_brdb, the authoring path): Int -> i64 member (tag 1).
        let mut buf = Vec::new();
        super::write_brdb(
            &schema,
            &mut buf,
            "WireGraphVariant",
            &EA3WireVariant::Int(7),
        )
        .unwrap();
        assert_eq!(buf[0], 1); // tag 1
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::I64(7)));

        // Zero value: tag 0 + zero of first member (f64 0.0).
        let mut buf = Vec::new();
        super::write_brdb_zero(&schema, &mut buf, "WireGraphVariant").unwrap();
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::F64(n) if n == 0.0));
    }

    #[test]
    fn test_legacy_wire_graph_variant_weak_object() {
        // The legacy literal `wire_graph_variant` tag 3 is a `weak_object`
        // (object/asset reference), an i64 index that must round-trip (it was
        // previously written/read as nothing, causing misalignment).
        use crate::schema::{BrdbSchema, BrdbValue, EA3WireVariant, read::read_type};
        use std::sync::Arc;

        let schema = Arc::new(BrdbSchema::default());

        for obj in [Some(5usize), None] {
            let mut buf = Vec::new();
            super::write_type(
                &schema,
                &mut buf,
                "wire_graph_variant",
                &BrdbValue::EA3WireVar(EA3WireVariant::Object(obj)),
            )
            .unwrap();
            assert_eq!(buf[0], 3); // tag 3
            let val = read_type(&schema, "wire_graph_variant", &mut buf.as_slice()).unwrap();
            match val {
                BrdbValue::EA3WireVar(EA3WireVariant::Object(got)) => assert_eq!(got, obj),
                other => panic!("expected Object, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_named_variant_vector_and_str_round_trip() {
        use crate::schema::{BrdbSchema, BrdbValue, EA3WireVariant, read::read_type};
        use crate::wrapper::Vector3f;
        use std::sync::Arc;

        let schema = Arc::new(
            BrdbSchema::new_parsed(
                "variant WireGraphVariant {
    f64,
    i64,
    bool,
    weak_object,
    WireGraphExec,
    Vector,
    str,
}
struct Vector {
    X: f64,
    Y: f64,
    Z: f64,
}
struct WireGraphExec {
}
",
            )
            .unwrap(),
        );

        // Str -> str member (tag 6).
        let mut buf = Vec::new();
        super::write_brdb(
            &schema,
            &mut buf,
            "WireGraphVariant",
            &EA3WireVariant::Str("hi".into()),
        )
        .unwrap();
        assert_eq!(buf[0], 6); // tag 6
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::String(s) if s == "hi"));

        // Vector -> Vector member (tag 5), read back as a struct.
        let mut buf = Vec::new();
        super::write_brdb(
            &schema,
            &mut buf,
            "WireGraphVariant",
            &EA3WireVariant::Vector(Vector3f {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
        )
        .unwrap();
        assert_eq!(buf[0], 5); // tag 5
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::Struct(_)));

        // A plain `String` authors via `as_brdb_ea3_wire_variant` -> str (tag 6).
        let mut buf = Vec::new();
        super::write_brdb(&schema, &mut buf, "WireGraphVariant", &String::from("yo")).unwrap();
        assert_eq!(buf[0], 6);
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::String(s) if s == "yo"));

        // A plain `Vector3f` authors via `as_brdb_ea3_wire_variant` -> Vector (tag 5).
        let mut buf = Vec::new();
        super::write_brdb(
            &schema,
            &mut buf,
            "WireGraphVariant",
            &Vector3f {
                x: 4.0,
                y: 5.0,
                z: 6.0,
            },
        )
        .unwrap();
        assert_eq!(buf[0], 5);
    }
}
