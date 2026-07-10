use std::io::Write;

use crate::{
    errors::BrdbSchemaError,
    schema::{
        BrdbEnum, BrdbInterned, BrdbSchema, BrdbSchemaEnum, BrdbSchemaStruct,
        BrdbSchemaStructProperty, BrdbStruct, BrdbValue, WireArrayVariant, WireVariant,
        as_brdb::AsBrdbValue, read::flat_type_size,
    },
};

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
        ("bool", BrdbValue::Bool(v)) => write_bool(buf, *v)?,
        ("u8", BrdbValue::U8(v)) => write_u8(buf, *v as u64)?,
        ("u16", BrdbValue::U16(v)) => write_uint(buf, *v as u64)?,
        ("u32", BrdbValue::U32(v)) => write_uint(buf, *v as u64)?,
        ("u64", BrdbValue::U64(v)) => write_uint(buf, *v)?,
        ("i8", BrdbValue::I8(v)) => write_int(buf, *v as i64)?,
        ("i16", BrdbValue::I16(v)) => write_int(buf, *v as i64)?,
        ("i32", BrdbValue::I32(v)) => write_int(buf, *v as i64)?,
        ("i64", BrdbValue::I64(v)) => write_int(buf, *v)?,
        ("f32", BrdbValue::F32(v)) => write_float32(buf, *v)?,
        ("f64", BrdbValue::F64(v)) => write_float64(buf, *v)?,
        ("str", BrdbValue::String(v)) => write_str(buf, &v)?,
        ("wire_graph_variant", BrdbValue::WireVar(v)) => write_wire_var(buf, v)?,
        ("wire_graph_prim_math_variant", BrdbValue::WireVar(v)) => match v {
            WireVariant::Number(v) => {
                write_uint(buf, 0)?;
                write_float64(buf, *v)?;
            }
            WireVariant::Int(v) => {
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
        (other, BrdbValue::Struct(_) | BrdbValue::Enum(_)) => {
            write_named_type(schema, buf, other, value)?
        }
        (expected, found) => {
            return Err(BrdbSchemaError::ExpectedType(
                expected.to_owned(),
                found.get_type().to_string(),
            ));
        }
    })
}

fn write_wire_var(buf: &mut impl Write, v: &WireVariant) -> Result<(), BrdbSchemaError> {
    match v {
        WireVariant::Number(v) => {
            write_uint(buf, 0)?;
            write_float64(buf, *v)?;
        }
        WireVariant::Int(v) => {
            write_uint(buf, 1)?;
            write_int(buf, *v)?;
        }
        WireVariant::Bool(v) => {
            write_uint(buf, 2)?;
            write_bool(buf, *v)?;
        }
        // Tag 3 is a `weak_object` (object/asset reference): an i64 index.
        WireVariant::Object(opt) => {
            write_uint(buf, 3)?;
            write_int(buf, opt.map(|i| i as i64).unwrap_or(-1))?;
        }
        WireVariant::Exec => {
            write_uint(buf, 4)?;
        }
        // The legacy union has no string/vector/rotator/quat/color members
        // (those are only in the newer named `WireGraphVariant` table).
        WireVariant::Str(_)
        | WireVariant::Vector(_)
        | WireVariant::Rotator { .. }
        | WireVariant::Quat { .. }
        | WireVariant::LinearColor { .. } => {
            return Err(BrdbSchemaError::ExpectedType(
                "wire_graph_variant".to_owned(),
                v.to_string(),
            ));
        }
    }
    Ok(())
}

/// Find the tag (member index) of `member_name` within a named variant.
fn variant_member_tag(
    schema: &BrdbSchema,
    variant_ty: &str,
    member_name: &str,
) -> Result<usize, BrdbSchemaError> {
    let members = schema
        .get_variant(variant_ty)
        .ok_or_else(|| BrdbSchemaError::UnknownType(variant_ty.to_owned()))?;
    members
        .iter()
        .position(|m| schema.intern.lookup_ref(*m) == Some(member_name))
        .ok_or_else(|| {
            BrdbSchemaError::ExpectedType(format!("{variant_ty} member"), member_name.to_owned())
        })
}

/// Write a `WireVariant` into a named variant type (e.g. `WireGraphVariant`,
/// `WireGraphPrimMathVariant`) as `uint(tag) + member value`, where the tag is
/// the member index resolved from the schema's variant table.
fn write_named_wire_variant(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    variant_ty: &str,
    v: &WireVariant,
) -> Result<(), BrdbSchemaError> {
    let member_name = match v {
        WireVariant::Number(_) => "f64",
        WireVariant::Int(_) => "i64",
        WireVariant::Bool(_) => "bool",
        WireVariant::Object(_) => "weak_object",
        WireVariant::Exec => "WireGraphExec",
        WireVariant::Vector(..) => "Vector",
        WireVariant::Rotator { .. } => "Rotator",
        WireVariant::Quat { .. } => "Quat",
        WireVariant::Str(_) => "str",
        WireVariant::LinearColor { .. } => "LinearColor",
    };
    let tag = variant_member_tag(schema, variant_ty, member_name)?;
    write_uint(buf, tag as u64)?;
    match v {
        WireVariant::Number(n) => write_float64(buf, *n)?,
        WireVariant::Int(i) => write_int(buf, *i)?,
        WireVariant::Bool(b) => write_bool(buf, *b)?,
        // weak_object: an asset-reference index (-1 is null).
        WireVariant::Object(opt) => write_int(buf, opt.map(|i| i as i64).unwrap_or(-1))?,
        WireVariant::Exec => {} // WireGraphExec: empty struct, no payload
        // Vector is a struct {X, Y, Z: f64}; structs are written field-by-field.
        WireVariant::Vector(v) => {
            write_float64(buf, v.x as f64)?;
            write_float64(buf, v.y as f64)?;
            write_float64(buf, v.z as f64)?;
        }
        // Rotator is a struct {Pitch, Yaw, Roll: f64}.
        WireVariant::Rotator { pitch, yaw, roll } => {
            write_float64(buf, *pitch)?;
            write_float64(buf, *yaw)?;
            write_float64(buf, *roll)?;
        }
        // Quat is a struct {X, Y, Z, W: f64}.
        WireVariant::Quat { x, y, z, w } => {
            write_float64(buf, *x)?;
            write_float64(buf, *y)?;
            write_float64(buf, *z)?;
            write_float64(buf, *w)?;
        }
        WireVariant::Str(s) => write_str(buf, s)?,
        // LinearColor is a struct {R, G, B, A: f32}.
        WireVariant::LinearColor { r, g, b, a } => {
            write_float32(buf, *r)?;
            write_float32(buf, *g)?;
            write_float32(buf, *b)?;
            write_float32(buf, *a)?;
        }
    }
    Ok(())
}

/// Write a `WireArrayVariant` into a named variant type (e.g.
/// `WireGraphArrayVariant`) as `uint(tag) + array`. Each member is a
/// `WireGraph*Array` struct holding a single `Values` array, which (having one
/// field) encodes as just that array.
fn write_named_array_variant(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    variant_ty: &str,
    arr: &WireArrayVariant,
) -> Result<(), BrdbSchemaError> {
    let tag = variant_member_tag(schema, variant_ty, arr.member_type())?;
    write_uint(buf, tag as u64)?;
    match arr {
        WireArrayVariant::DoubleArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for x in v {
                write_float64(buf, *x)?;
            }
        }
        WireArrayVariant::Int64Array(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for x in v {
                write_int(buf, *x)?;
            }
        }
        WireArrayVariant::BoolArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for x in v {
                write_bool(buf, *x)?;
            }
        }
        WireArrayVariant::ObjectArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for o in v {
                // weak_object: an asset-reference index (-1 is null).
                write_int(buf, o.map(|i| i as i64).unwrap_or(-1))?;
            }
        }
        WireArrayVariant::VectorArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for vec in v {
                write_float64(buf, vec.x as f64)?;
                write_float64(buf, vec.y as f64)?;
                write_float64(buf, vec.z as f64)?;
            }
        }
        WireArrayVariant::RotatorArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for (pitch, yaw, roll) in v {
                write_float64(buf, *pitch)?;
                write_float64(buf, *yaw)?;
                write_float64(buf, *roll)?;
            }
        }
        WireArrayVariant::QuatArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for (x, y, z, w) in v {
                write_float64(buf, *x)?;
                write_float64(buf, *y)?;
                write_float64(buf, *z)?;
                write_float64(buf, *w)?;
            }
        }
        WireArrayVariant::StringArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for s in v {
                write_str(buf, s)?;
            }
        }
        WireArrayVariant::LinearColorArray(v) => {
            rmp::encode::write_array_len(buf, v.len() as u32)?;
            for (r, g, b, a) in v {
                write_float32(buf, *r)?;
                write_float32(buf, *g)?;
                write_float32(buf, *b)?;
                write_float32(buf, *a)?;
            }
        }
    }
    Ok(())
}

/// Write an arbitrary `BrdbValue` into a named variant type. A `WireVar` maps to
/// a member by its variant kind; a raw value (from a round-tripped read) maps to
/// a member by its concrete type.
fn write_variant_value(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    variant_ty: &str,
    val: &BrdbValue,
) -> Result<(), BrdbSchemaError> {
    if let BrdbValue::WireVar(v) = val {
        return write_named_wire_variant(schema, buf, variant_ty, v);
    }
    let member_name = match val {
        BrdbValue::F64(_) => "f64",
        BrdbValue::I64(_) => "i64",
        BrdbValue::Bool(_) => "bool",
        BrdbValue::String(_) => "str",
        BrdbValue::Asset(_) => "weak_object",
        BrdbValue::Struct(s) => schema
            .intern
            .lookup_ref(s.name)
            .ok_or_else(|| BrdbSchemaError::UnknownType(s.name.0.to_string()))?,
        other => {
            return Err(BrdbSchemaError::ExpectedType(
                variant_ty.to_owned(),
                other.get_type().to_owned(),
            ));
        }
    };
    let tag = variant_member_tag(schema, variant_ty, member_name)?;
    write_uint(buf, tag as u64)?;
    write_type(schema, buf, member_name, val)
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
            write_named_type(schema, buf, &lookup(*ty)?, value)?
        }
        (BrdbSchemaStructProperty::Array(ty), BrdbValue::Array(arr)) => {
            rmp::encode::write_array_len(buf, arr.len() as u32)?;
            // Write each item in the array
            let item_ty = &lookup(*ty)?;
            for item in arr {
                write_named_type(schema, buf, item_ty, item)?;
            }
        }
        (BrdbSchemaStructProperty::FlatArray(ty), BrdbValue::FlatArray(arr_data)) => {
            // Write the length of the buffer that will be allocated
            let type_size = flat_type_size(schema, &lookup(*ty)?);
            rmp::encode::write_bin_len(buf, (arr_data.len() * type_size) as u32)?;

            let item_ty = &lookup(*ty)?;
            for item in arr_data {
                write_flat_type(schema, buf, item_ty, item)?;
            }
        }
        (BrdbSchemaStructProperty::Map(k_ty, v_ty), BrdbValue::Map(map)) => {
            // Write the number of items in the map
            rmp::encode::write_map_len(buf, map.len() as u32)?;
            // Write each key-value pair
            for (key, val) in map {
                write_named_type(schema, buf, &lookup(*k_ty)?, key)?;
                write_named_type(schema, buf, &lookup(*v_ty)?, val)?;
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
    if value <= 127 {
        rmp::encode::write_pfix(buf, value as u8)?;
    } else if value > 256 - 32 && value <= u8::MAX as u64 {
        rmp::encode::write_nfix(buf, value as i8)?;
    } else {
        rmp::encode::write_u8(buf, value as u8)?;
    }
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
    match (ty, value) {
        ("u8", BrdbValue::U8(v)) => write_flat_u8(buf, *v)?,
        ("u16", BrdbValue::U16(v)) => write_flat_u16(buf, *v)?,
        ("u32", BrdbValue::U32(v)) => write_flat_u32(buf, *v)?,
        ("u64", BrdbValue::U64(v)) => write_flat_u64(buf, *v)?,
        ("i8", BrdbValue::I8(v)) => write_flat_i8(buf, *v)?,
        ("i16", BrdbValue::I16(v)) => write_flat_i16(buf, *v)?,
        ("i32", BrdbValue::I32(v)) => write_flat_i32(buf, *v)?,
        ("i64", BrdbValue::I64(v)) => write_flat_i64(buf, *v)?,
        ("f32", BrdbValue::F32(v)) => write_flat_f32(buf, *v)?,
        ("f64", BrdbValue::F64(v)) => write_flat_f64(buf, *v)?,
        (other, BrdbValue::Struct(s)) => {
            if let Some((intern, s_ty)) = schema
                .intern
                .get(other)
                .and_then(|i| schema.structs.get(&i).map(|s| (i, s)))
            {
                if s.name != intern {
                    return Err(BrdbSchemaError::ExpectedType(
                        other.to_owned(),
                        schema
                            .intern
                            .lookup(s.name)
                            .unwrap_or_else(|| "unknown struct".to_owned()),
                    ));
                }

                for (k, prop_schema) in s_ty {
                    let prop_val = s.properties.get(k).ok_or_else(|| {
                        BrdbSchemaError::MissingStructField(
                            schema
                                .intern
                                .lookup(s.name)
                                .unwrap_or_else(|| "unknown struct".to_owned()),
                            schema
                                .intern
                                .lookup(*k)
                                .unwrap_or_else(|| "unknown property".to_owned()),
                        )
                    })?;

                    // Flat types can only write properties of type `Type`
                    match prop_schema {
                        BrdbSchemaStructProperty::Type(ty) => write_flat_type(
                            schema,
                            buf,
                            schema.intern.lookup_ref(*ty).ok_or(
                                BrdbSchemaError::UnknownStructPropertyType(ty.0.to_string()),
                            )?,
                            prop_val,
                        )?,
                        other => {
                            return Err(BrdbSchemaError::InvalidFlatType(other.as_string(schema)));
                        }
                    }
                }
            } else {
                return Err(BrdbSchemaError::UnknownType(other.to_owned()));
            }
        }
        (other, _) => return Err(BrdbSchemaError::InvalidFlatType(other.to_owned())),
    }
    Ok(())
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

pub fn write_brdb(
    schema: &BrdbSchema,
    buf: &mut impl Write,
    ty: &str,
    value: &dyn AsBrdbValue,
) -> Result<(), BrdbSchemaError> {
    let lookup = |ty: BrdbInterned| {
        schema
            .intern
            .lookup_ref(ty)
            .ok_or(BrdbSchemaError::UnknownType(ty.0.to_string()))
    };

    match ty {
        "bool" => write_bool(buf, value.as_brdb_bool()?)?,
        "u8" => write_u8(buf, value.as_brdb_u8()? as u64)?,
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
        "wire_graph_variant" => write_wire_var(buf, &value.as_brdb_wire_variant()?)?,
        "wire_graph_prim_math_variant" => match value.as_brdb_wire_variant()? {
            WireVariant::Number(v) => {
                write_uint(buf, 0)?;
                write_float64(buf, v)?;
            }
            WireVariant::Int(v) => {
                write_uint(buf, 1)?;
                write_int(buf, v)?;
            }
            other => {
                return Err(BrdbSchemaError::ExpectedType(
                    "wire_graph_prim_math_variant".to_owned(),
                    other.to_string(),
                ));
            }
        },
        "bundle_path_ref" => write_str(buf, value.as_brdb_str().unwrap_or(""))?,
        "class" | "object" => {
            let asset_index = value.as_brdb_asset(schema, ty)?;
            if let Some(asset_index) = asset_index {
                write_uint(buf, asset_index as u64)?;
            } else {
                write_int(buf, -1)?;
            }
        }
        other if schema.get_variant(other).is_some() => {
            // Array-valued variants (WireGraphArrayVariant) self-identify via
            // `as_brdb_wire_array_variant`; everything else is a scalar variant.
            if let Ok(arr) = value.as_brdb_wire_array_variant() {
                write_named_array_variant(schema, buf, other, &arr)?;
            } else {
                write_named_wire_variant(schema, buf, other, &value.as_brdb_wire_variant()?)?;
            }
        }
        other => {
            if let (Some(s_id), Some(s_ty)) = (schema.intern.get(other), schema.get_struct(other)) {
                for (prop_id, prop_schema) in s_ty {
                    match prop_schema {
                        BrdbSchemaStructProperty::Type(ty_id) => {
                            let ty_str = lookup(*ty_id)?;
                            match value.as_brdb_struct_prop_value(schema, s_id, *prop_id) {
                                Ok(prop_value) => {
                                    write_brdb(schema, buf, &ty_str, &*prop_value)?;
                                }
                                Err(BrdbSchemaError::MissingStructField(
                                    ref _sn,
                                    ref field_name,
                                )) => {
                                    let defaults = &*crate::wrapper::component_db::STRUCT_DEFAULTS;
                                    let found = defaults
                                        .iter()
                                        .find(|(name, _)| *name == other)
                                        .and_then(|(_, fields)| {
                                            fields
                                                .iter()
                                                .find(|(n, _)| *n == field_name.as_str())
                                                .map(|(_, v)| v)
                                        });
                                    if let Some(default_val) = found {
                                        write_brdb(schema, buf, &ty_str, default_val.as_ref())?;
                                    } else {
                                        write_brdb_zero(schema, buf, &ty_str)?;
                                    }
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        BrdbSchemaStructProperty::Array(ty_id) => {
                            match value.as_brdb_struct_prop_array(schema, s_id, *prop_id) {
                                Ok(prop_values) => {
                                    let ty = &lookup(*ty_id)?;
                                    rmp::encode::write_array_len(buf, prop_values.len() as u32)?;
                                    for prop_value in prop_values {
                                        write_brdb(schema, buf, ty, &*prop_value)?;
                                    }
                                }
                                Err(BrdbSchemaError::MissingStructField(..)) => {
                                    rmp::encode::write_array_len(buf, 0)?;
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        BrdbSchemaStructProperty::FlatArray(ty_id) => {
                            match value.as_brdb_struct_prop_array(schema, s_id, *prop_id) {
                                Ok(prop_values) => {
                                    let ty = &lookup(*ty_id)?;
                                    let type_size = flat_type_size(schema, ty);
                                    rmp::encode::write_bin_len(
                                        buf,
                                        (prop_values.len() * type_size) as u32,
                                    )?;
                                    for prop_value in prop_values {
                                        write_brdb_flat(schema, buf, ty, &*prop_value)?;
                                    }
                                }
                                Err(BrdbSchemaError::MissingStructField(..)) => {
                                    rmp::encode::write_bin_len(buf, 0)?;
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        BrdbSchemaStructProperty::Map(k_ty_id, v_ty_id) => {
                            match value.as_brdb_struct_prop_map(schema, s_id, *prop_id) {
                                Ok(prop_values) => {
                                    let k_ty = &lookup(*k_ty_id)?;
                                    let v_ty = &lookup(*v_ty_id)?;
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
            } else if let Some(enum_ty) = schema.get_enum(other) {
                let enum_value = value.as_brdb_enum(schema, enum_ty)?;
                if enum_value >= enum_ty.len() as i32 {
                    return Err(BrdbSchemaError::EnumIndexOutOfBounds {
                        enum_name: other.to_owned(),
                        index: enum_value as u64,
                    });
                }
                write_uint(buf, enum_value as u64)?;
            } else {
                return Err(BrdbSchemaError::UnknownType(other.to_owned()));
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
    match ty {
        "u8" => write_flat_u8(buf, value.as_brdb_u8()?)?,
        "u16" => write_flat_u16(buf, value.as_brdb_u16()?)?,
        "u32" => write_flat_u32(buf, value.as_brdb_u32()?)?,
        "u64" => write_flat_u64(buf, value.as_brdb_u64()?)?,
        "i8" => write_flat_i8(buf, value.as_brdb_i8()?)?,
        "i16" => write_flat_i16(buf, value.as_brdb_i16()?)?,
        "i32" => write_flat_i32(buf, value.as_brdb_i32()?)?,
        "i64" => write_flat_i64(buf, value.as_brdb_i64()?)?,
        "f32" => write_flat_f32(buf, value.as_brdb_f32()?)?,
        "f64" => write_flat_f64(buf, value.as_brdb_f64()?)?,
        other => {
            let (Some(s_id), Some(s_ty)) = (schema.intern.get(other), schema.get_struct(other))
            else {
                return Err(BrdbSchemaError::InvalidFlatType(other.to_owned()));
            };
            for (prop_id, prop_schema) in s_ty {
                let BrdbSchemaStructProperty::Type(prop_ty) = prop_schema else {
                    return Err(BrdbSchemaError::InvalidFlatType(format!(
                        "flat {other} struct property"
                    )));
                };
                let prop_ty = prop_ty.get_ok(schema, || {
                    BrdbSchemaError::UnknownStructPropertyType(prop_ty.0.to_string())
                })?;
                let prop_val = value.as_brdb_struct_prop_value(schema, s_id, *prop_id)?;
                write_brdb_flat(schema, buf, prop_ty, &*prop_val)?;
            }
        }
    }
    Ok(())
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
        "bool" => write_bool(buf, false)?,
        "u8" => write_u8(buf, 0)?,
        "u16" | "u32" | "u64" => write_uint(buf, 0)?,
        "i8" | "i16" | "i32" | "i64" => write_int(buf, 0)?,
        "f32" => write_float32(buf, 0.0)?,
        "f64" => write_float64(buf, 0.0)?,
        "str" => write_str(buf, "")?,
        "wire_graph_variant" => {
            write_uint(buf, 0)?; // Number
            write_float64(buf, 0.0)?;
        }
        "wire_graph_prim_math_variant" => {
            write_uint(buf, 0)?; // Number
            write_float64(buf, 0.0)?;
        }
        "bundle_path_ref" => write_str(buf, "")?,
        "class" | "object" => write_int(buf, -1)?,
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
            } else {
                return Err(BrdbSchemaError::UnknownType(other.to_owned()));
            }
        }
    }
    Ok(())
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
    fn test_named_variant_round_trip() {
        use crate::schema::{BrdbSchema, BrdbValue, WireVariant, read::read_type};
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
            &BrdbValue::WireVar(WireVariant::Number(3.5)),
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
            &BrdbValue::WireVar(WireVariant::Bool(true)),
        )
        .unwrap();
        assert_eq!(buf[0], 2); // tag 2
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::Bool(true)));

        // AsBrdbValue path (write_brdb, the authoring path): Int -> i64 member (tag 1).
        let mut buf = Vec::new();
        super::write_brdb(&schema, &mut buf, "WireGraphVariant", &WireVariant::Int(7)).unwrap();
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
        use crate::schema::{BrdbSchema, BrdbValue, WireVariant, read::read_type};
        use std::sync::Arc;

        let schema = Arc::new(BrdbSchema::default());

        for obj in [Some(5usize), None] {
            let mut buf = Vec::new();
            super::write_type(
                &schema,
                &mut buf,
                "wire_graph_variant",
                &BrdbValue::WireVar(WireVariant::Object(obj)),
            )
            .unwrap();
            assert_eq!(buf[0], 3); // tag 3
            let val = read_type(&schema, "wire_graph_variant", &mut buf.as_slice()).unwrap();
            match val {
                BrdbValue::WireVar(WireVariant::Object(got)) => assert_eq!(got, obj),
                other => panic!("expected Object, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_named_variant_vector_and_str_round_trip() {
        use crate::schema::{BrdbSchema, BrdbValue, WireVariant, read::read_type};
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
            &WireVariant::Str("hi".into()),
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
            &WireVariant::Vector(Vector3f {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
        )
        .unwrap();
        assert_eq!(buf[0], 5); // tag 5
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::Struct(_)));

        // A plain `String` authors via `as_brdb_wire_variant` -> str (tag 6).
        let mut buf = Vec::new();
        super::write_brdb(&schema, &mut buf, "WireGraphVariant", &String::from("yo")).unwrap();
        assert_eq!(buf[0], 6);
        let val = read_type(&schema, "WireGraphVariant", &mut buf.as_slice()).unwrap();
        assert!(matches!(val, BrdbValue::String(s) if s == "yo"));

        // A plain `Vector3f` authors via `as_brdb_wire_variant` -> Vector (tag 5).
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

    #[test]
    fn test_named_array_variant_round_trip() {
        use crate::schema::{BrdbSchema, BrdbValue, WireArrayVariant, read::read_type};
        use std::sync::Arc;

        let schema = Arc::new(
            BrdbSchema::new_parsed(
                "variant WireGraphArrayVariant {
    WireGraphDoubleArray,
    WireGraphStringArray,
}
struct WireGraphDoubleArray {
    Values: f64[],
}
struct WireGraphStringArray {
    Values: str[],
}
",
            )
            .unwrap(),
        );

        // DoubleArray -> WireGraphDoubleArray member (tag 0).
        let mut buf = Vec::new();
        super::write_brdb(
            &schema,
            &mut buf,
            "WireGraphArrayVariant",
            &WireArrayVariant::DoubleArray(vec![1.0, 2.0, 3.0]),
        )
        .unwrap();
        assert_eq!(buf[0], 0); // tag 0
        let val = read_type(&schema, "WireGraphArrayVariant", &mut buf.as_slice()).unwrap();
        // Read back into a WireArrayVariant via TryFrom.
        let back = WireArrayVariant::try_from(&val).unwrap();
        assert_eq!(back, WireArrayVariant::DoubleArray(vec![1.0, 2.0, 3.0]));

        // StringArray -> WireGraphStringArray member (tag 1), full round-trip.
        let mut buf = Vec::new();
        super::write_brdb(
            &schema,
            &mut buf,
            "WireGraphArrayVariant",
            &WireArrayVariant::StringArray(vec!["a".into(), "b".into()]),
        )
        .unwrap();
        assert_eq!(buf[0], 1); // tag 1
        let val = read_type(&schema, "WireGraphArrayVariant", &mut buf.as_slice()).unwrap();
        let back = WireArrayVariant::try_from(&val).unwrap();
        assert_eq!(
            back,
            WireArrayVariant::StringArray(vec!["a".into(), "b".into()])
        );
    }
}
