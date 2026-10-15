use std::{collections::HashMap, io::Read, sync::Arc};

use indexmap::IndexMap;
use rmp::{Marker, decode::RmpRead};

use crate::{
    errors::BrdbSchemaError,
    schema::{
        BRICK_HANDLE, BrdbDynamicArray, BrdbDynamicMap, BrdbEnum, BrdbInterned, BrdbSchema,
        BrdbSchemaEnum, BrdbSchemaStruct, BrdbSchemaStructProperty, BrdbStruct, BrdbValue,
        COMPONENT_HANDLE, DYNAMIC_ARRAY, DYNAMIC_ENUM, DYNAMIC_MAP,
        EA3_WIRE_GRAPH_PRIM_MATH_VARIANT, EA3_WIRE_GRAPH_VARIANT, EA3WireVariant, builtin_alias,
    },
};

/// Read a message pack string from the stream and return it as an owned String.
pub fn read_owned_str(bytes: &mut impl Read) -> Result<String, BrdbSchemaError> {
    let len = rmp::decode::read_str_len(bytes)? as usize;
    read_str_from_len(bytes, len)
}

/// Read a message pack string of a specific length from the stream and return it as an owned String.
pub fn read_str_from_len(bytes: &mut impl Read, len: usize) -> Result<String, BrdbSchemaError> {
    let mut buf = vec![0; len as usize];
    bytes
        .read_exact(&mut buf)
        .map_err(BrdbSchemaError::ReadError)?;
    String::from_utf8(buf).map_err(BrdbSchemaError::InvalidUtf8)
}

pub fn read_type(
    schema: &Arc<BrdbSchema>,
    ty: &str,
    buf: &mut impl Read,
) -> Result<BrdbValue, BrdbSchemaError> {
    Ok(match ty {
        // No payload: a `nil` field consumes zero bytes.
        "nil" => BrdbValue::Nil,
        "bool" => BrdbValue::Bool(read_bool(buf)?),
        // Integers decode leniently in every format: any integer tag, cast to
        // the field type (older writers encoded u8 128..=255 as -128..=-1).
        "u8" => BrdbValue::U8(read_uint(buf)? as u8),
        "u16" => BrdbValue::U16(read_uint(buf)? as u16),
        "u32" => BrdbValue::U32(read_uint(buf)? as u32),
        "u64" => BrdbValue::U64(read_uint(buf)?),
        "i8" => BrdbValue::I8(read_int(buf)? as i8),
        "i16" => BrdbValue::I16(read_int(buf)? as i16),
        "i32" => BrdbValue::I32(read_int(buf)? as i32),
        "i64" => BrdbValue::I64(read_int(buf)?),
        "f32" => BrdbValue::F32(read_float32(buf)?),
        "f64" => BrdbValue::F64(read_float64(buf)?),
        // `bundle_path_ref` is a legacy alias for `str`.
        "str" | "bundle_path_ref" => BrdbValue::String(read_str(buf)?),
        EA3_WIRE_GRAPH_VARIANT => BrdbValue::EA3WireVar(match read_uint(buf)? {
            0 => EA3WireVariant::Number(read_float64(buf)?),
            1 => EA3WireVariant::Int(read_int(buf)?),
            2 => EA3WireVariant::Bool(read_bool(buf)?),
            // EA3 data stores an i64 asset index after selector 3; it must be
            // consumed or the rest of the struct misaligns. The spec's frozen
            // layout `[f64, i64, bool, nil, nil]` (no payload) applies to
            // `Latest` schemas only.
            3 if schema.is_latest() => EA3WireVariant::Object(None),
            3 => {
                let id = read_int(buf)?;
                EA3WireVariant::Object(if id < 0 { None } else { Some(id as usize) })
            }
            4 => EA3WireVariant::Exec,
            other => return Err(BrdbSchemaError::UnknownWireVariant(other as usize)),
        }),
        EA3_WIRE_GRAPH_PRIM_MATH_VARIANT => BrdbValue::EA3WireVar(match read_uint(buf)? {
            0 => EA3WireVariant::Number(read_float64(buf)?),
            1 => EA3WireVariant::Int(read_int(buf)?),
            other => return Err(BrdbSchemaError::UnknownWireVariant(other as usize)),
        }),
        // Latest: a non-persistent reference; the integer is discarded and the
        // reference loads as null.
        "weak_object" if schema.is_latest() => {
            read_int(buf)?;
            BrdbValue::Asset(None)
        }
        "class" | "object" | "weak_object" => {
            // Assets are stored as indices into the external asset table.
            let id = read_int(buf)?;
            if id < 0 {
                BrdbValue::Asset(None)
            } else {
                let id = id as usize;
                if schema.global_data.external_asset_references.len() <= id {
                    return Err(BrdbSchemaError::UnknownAsset(ty.to_owned(), id));
                }
                BrdbValue::Asset(Some(id))
            }
        }
        DYNAMIC_ARRAY => {
            let element_type = read_type_ref(schema, buf)?;
            let len = rmp::decode::read_array_len(buf)? as usize;
            let mut values = Vec::with_capacity(len.min(1 << 16));
            for i in 0..len {
                values.push(read_type(schema, element_type, buf).map_err(|e| e.wrap(i))?);
            }
            BrdbValue::DynamicArray(BrdbDynamicArray::new(element_type, values))
        }
        DYNAMIC_MAP => {
            let key_type = read_type_ref(schema, buf)?;
            let value_type = read_type_ref(schema, buf)?;
            let len = rmp::decode::read_map_len(buf)? as usize;
            let mut entries = IndexMap::with_capacity(len.min(1 << 16));
            for _ in 0..len {
                let key = read_type(schema, key_type, buf).map_err(|e| e.wrap("map key"))?;
                let value = read_type(schema, value_type, buf).map_err(|e| e.wrap("map value"))?;
                // A later entry with the same key updates the existing value.
                entries.insert(key, value);
            }
            BrdbValue::DynamicMap(BrdbDynamicMap::new(key_type, value_type, entries))
        }
        other => {
            let id = schema.intern.get(other);
            if let Some(alternatives) = id.and_then(|id| schema.get_variant_interned(id)) {
                // The selector indexes the SAVED alternative list, and the
                // payload is decoded with that alternative's saved type.
                let selector = read_uint(buf)? as usize;
                let Some(alternative) = alternatives.get(selector).copied() else {
                    return Err(BrdbSchemaError::UnknownWireVariant(selector));
                };
                let alternative_ty = schema.intern.lookup_ref(alternative).ok_or(
                    BrdbSchemaError::UnknownStructPropertyType(alternative.0.to_string()),
                )?;
                read_type(schema, alternative_ty, buf)?
            } else if let Some(id) = id.filter(|id| {
                schema.get_struct_interned(*id).is_some() || schema.get_enum_interned(*id).is_some()
            }) {
                read_named_type(schema, buf, id)?
            } else if let Some(alias) = builtin_alias(other) {
                read_type(schema, alias, buf)?
            } else if other == DYNAMIC_ENUM {
                read_dynamic_enum(schema, buf)?
            } else if other == BRICK_HANDLE || other == COMPONENT_HANDLE {
                return Err(BrdbSchemaError::UnsupportedType(other.to_owned()));
            } else if let Some(id) = id {
                return Err(BrdbSchemaError::UnknownSchemaType(
                    schema
                        .intern
                        .lookup(id)
                        .unwrap_or_else(|| format!("unknown ({})", id.0)),
                ));
            } else {
                return Err(BrdbSchemaError::UnknownType(other.to_string()));
            }
        }
    })
}

/// Read a `types` table reference: an unsigned index that must be in range
/// and must not name another dynamic container.
fn read_type_ref<'a>(
    schema: &'a BrdbSchema,
    buf: &mut impl Read,
) -> Result<&'a str, BrdbSchemaError> {
    let index = read_uint(buf)?;
    let ty = usize::try_from(index)
        .ok()
        .filter(|i| *i as u64 <= u32::MAX as u64)
        .and_then(|i| schema.type_at(i))
        .ok_or(BrdbSchemaError::InvalidTypeIndex {
            index: index as usize,
            len: schema.types.len(),
        })?;
    if ty == DYNAMIC_ARRAY || ty == DYNAMIC_MAP {
        return Err(BrdbSchemaError::InvalidSchema(format!(
            "dynamic containers cannot contain '{ty}'"
        )));
    }
    Ok(ty)
}

/// `dynamic_enum`: a `types` index naming the enum, then the enum value.
fn read_dynamic_enum(
    schema: &Arc<BrdbSchema>,
    buf: &mut impl Read,
) -> Result<BrdbValue, BrdbSchemaError> {
    let enum_name = read_type_ref(schema, buf)?;
    let value = read_int(buf)?;
    let name = schema
        .intern
        .get(enum_name)
        .filter(|id| schema.get_enum_interned(*id).is_some())
        .ok_or_else(|| {
            BrdbSchemaError::ExpectedType("enum in types table".to_owned(), enum_name.to_owned())
        })?;
    Ok(BrdbValue::Enum(BrdbEnum {
        schema: Arc::clone(schema),
        name,
        value: value as u64,
    }))
}

fn read_named_type(
    schema: &Arc<BrdbSchema>,
    buf: &mut impl Read,
    ty: BrdbInterned,
) -> Result<BrdbValue, BrdbSchemaError> {
    if let Some(s) = schema.get_struct_interned(ty) {
        read_struct(&schema, buf, ty, s).map_err(|e| e.wrap(ty.get_or(schema, "unknown struct")))
    } else if let Some(e) = schema.get_enum_interned(ty) {
        read_enum(&schema, buf, ty, e).map_err(|e| e.wrap(ty.get_or(schema, "unknown enum")))
    } else {
        return Err(BrdbSchemaError::UnknownSchemaType(
            schema
                .intern
                .lookup(ty)
                .unwrap_or_else(|| format!("unknown ({})", ty.0)),
        ));
    }
}

fn read_struct(
    schema: &Arc<BrdbSchema>,
    buf: &mut impl Read,
    name: BrdbInterned,
    s: &BrdbSchemaStruct,
) -> Result<BrdbValue, BrdbSchemaError> {
    let mut properties = HashMap::with_capacity(s.len());
    for (k, v) in s.iter() {
        properties.insert(
            *k,
            read_struct_property(schema, buf, v)
                .map_err(|e| e.wrap(k.get_or(schema, "unknown prop")))?,
        );
    }
    Ok(BrdbValue::Struct(Box::new(BrdbStruct {
        schema: Arc::clone(schema),
        name,
        properties,
    })))
}

fn read_struct_property(
    schema: &Arc<BrdbSchema>,
    buf: &mut impl Read,
    prop: &BrdbSchemaStructProperty,
) -> Result<BrdbValue, BrdbSchemaError> {
    let lookup = |ty: BrdbInterned| {
        schema
            .intern
            .lookup_ref(ty)
            .ok_or(BrdbSchemaError::UnknownStructPropertyType(ty.0.to_string()))
    };

    match prop {
        BrdbSchemaStructProperty::Type(ty) => read_type(schema, &lookup(*ty)?, buf),
        BrdbSchemaStructProperty::Array(ty) => {
            let len = rmp::decode::read_array_len(buf)? as usize;
            let item_ty = lookup(*ty)?;
            let mut values = Vec::with_capacity(len);
            for i in 0..len {
                values.push(read_type(schema, item_ty, buf).map_err(|e| e.wrap(i))?);
            }
            Ok(BrdbValue::Array(values))
        }
        BrdbSchemaStructProperty::FlatArray(ty) => {
            // A flat array is a bin blob holding a little endian memory dump
            // of the elements.
            let flat_buf_len = rmp::decode::read_bin_len(buf)? as usize;
            let mut flat_buf = vec![0; flat_buf_len];
            buf.read_exact(&mut flat_buf)
                .map_err(BrdbSchemaError::ReadError)?;

            let flat_ty = lookup(*ty)?;
            let layout = flat_layout(schema, flat_ty)
                .filter(|l| l.size > 0)
                .ok_or_else(|| BrdbSchemaError::InvalidFlatType(flat_ty.to_string()))?;
            if flat_buf_len % layout.size != 0 {
                return Err(BrdbSchemaError::InvalidFlatDataSize(
                    flat_ty.to_string(),
                    flat_buf_len,
                    layout.size,
                ));
            }

            let items = flat_buf
                .chunks_exact(layout.size)
                .map(|elem| decode_flat(schema, &layout, elem))
                .collect();
            Ok(BrdbValue::FlatArray(items))
        }
        BrdbSchemaStructProperty::Map(k_ty, v_ty) => {
            let mut map = IndexMap::new();
            // A map is a msgpack map header (the pair count) then that many
            // key/value pairs — matching write_map_len in the writer.
            let len = rmp::decode::read_map_len(buf)? as usize;
            for _ in 0..len {
                // Dispatch key/value through the full `read_type` (primitives,
                // structs, enums, variants, weak_object).
                let key = read_type(schema, &lookup(*k_ty)?, buf)
                    .map_err(|e| e.wrap(k_ty.get_or(schema, "unknown map key")))?;
                let value = read_type(schema, &lookup(*v_ty)?, buf)
                    .map_err(|e| e.wrap(k_ty.get_or(schema, "unknown map value")))?;
                // A later entry with the same key updates the existing value.
                map.insert(key, value);
            }
            Ok(BrdbValue::Map(map))
        }
    }
}

fn read_enum(
    schema: &Arc<BrdbSchema>,
    buf: &mut impl Read,
    name: BrdbInterned,
    e: &BrdbSchemaEnum,
) -> Result<BrdbValue, BrdbSchemaError> {
    let value = read_uint(buf)?;
    if e.len() <= value as usize {
        return Err(BrdbSchemaError::EnumIndexOutOfBounds {
            enum_name: schema
                .intern
                .lookup(name)
                .unwrap_or_else(|| format!("unknown ({})", name.0)),
            index: value,
        });
    }
    Ok(BrdbValue::Enum(BrdbEnum {
        schema: Arc::clone(schema),
        name,
        value,
    }))
}

fn read_bool(buf: &mut impl Read) -> Result<bool, BrdbSchemaError> {
    rmp::decode::read_bool(buf).map_err(BrdbSchemaError::from)
}

fn read_str(buf: &mut impl Read) -> Result<String, BrdbSchemaError> {
    let len = rmp::decode::read_str_len(buf)?;
    read_str_from_len(buf, len as usize)
}

/// Read an ambiguously encoded signed integer from the buffer: any integer
/// tag (a `u64` above `i64::MAX` wraps).
pub(crate) fn read_int(buf: &mut impl Read) -> Result<i64, BrdbSchemaError> {
    let marker =
        rmp::decode::read_marker(buf).map_err(|e| BrdbSchemaError::RmpMarkerReadError(e.0))?;
    Ok(match marker {
        Marker::FixPos(value) => value as i64,
        Marker::U8 => buf
            .read_u8()
            .map_err(rmp::decode::ValueReadError::InvalidDataRead)? as i64,
        Marker::U16 => buf.read_data_u16()? as i64,
        Marker::U32 => buf.read_data_u32()? as i64,
        Marker::U64 => buf.read_data_u64()? as i64,
        Marker::FixNeg(value) => value as i64,
        Marker::I8 => buf.read_data_i8()? as i64,
        Marker::I16 => buf.read_data_i16()? as i64,
        Marker::I32 => buf.read_data_i32()? as i64,
        Marker::I64 => buf.read_data_i64()? as i64,
        _ => {
            return Err(BrdbSchemaError::RmpMarkerReadError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unexpected marker for integer",
            )));
        }
    })
}

/// Read an ambiguously encoded unsigned integer from the buffer. Signed tags
/// are sign-extended, and negative fixints read as 224..=255: older writers
/// encoded u8 128..=255 as -128..=-1, which the `u8` cast restores.
fn read_uint(buf: &mut impl Read) -> Result<u64, BrdbSchemaError> {
    let marker =
        rmp::decode::read_marker(buf).map_err(|e| BrdbSchemaError::RmpMarkerReadError(e.0))?;
    Ok(match marker {
        Marker::FixPos(value) => value as u64,
        Marker::U8 => buf
            .read_u8()
            .map_err(rmp::decode::ValueReadError::InvalidDataRead)? as u64,
        Marker::I8 => buf.read_data_i8()? as u64,
        Marker::I16 => buf.read_data_i16()? as u64,
        Marker::I32 => buf.read_data_i32()? as u64,
        Marker::I64 => buf.read_data_i64()? as u64,
        Marker::U16 => buf.read_data_u16()? as u64,
        Marker::U32 => buf.read_data_u32()? as u64,
        Marker::U64 => buf.read_data_u64()? as u64,
        // It's very sneaky making values 224 thru 255 use FixNeg markers...
        Marker::FixNeg(value) => (256 + (value as i16)) as u64,
        m => {
            return Err(BrdbSchemaError::ExpectedType(
                "uint".to_string(),
                format!("marker {m:?}"),
            ));
        }
    })
}

/// Any integer or floating point tag, converted to f64.
fn read_number(buf: &mut impl Read, ty: &str) -> Result<f64, BrdbSchemaError> {
    let marker =
        rmp::decode::read_marker(buf).map_err(|e| BrdbSchemaError::RmpMarkerReadError(e.0))?;
    Ok(match marker {
        Marker::FixPos(value) => value as f64,
        Marker::FixNeg(value) => value as f64,
        Marker::U8 => buf.read_data_u8()? as f64,
        Marker::U16 => buf.read_data_u16()? as f64,
        Marker::U32 => buf.read_data_u32()? as f64,
        Marker::U64 => buf.read_data_u64()? as f64,
        Marker::I8 => buf.read_data_i8()? as f64,
        Marker::I16 => buf.read_data_i16()? as f64,
        Marker::I32 => buf.read_data_i32()? as f64,
        Marker::I64 => buf.read_data_i64()? as f64,
        Marker::F32 => buf.read_data_f32()? as f64,
        Marker::F64 => buf.read_data_f64()?,
        _ => {
            return Err(BrdbSchemaError::RmpMarkerReadError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unexpected marker {marker:?} for {ty}"),
            )));
        }
    })
}

/// f32 accepts any integer or float tag. A `float 32` payload is returned
/// exactly; anything else is converted.
fn read_float32(buf: &mut impl Read) -> Result<f32, BrdbSchemaError> {
    read_number(buf, "float32").map(|v| v as f32)
}

fn read_float64(buf: &mut impl Read) -> Result<f64, BrdbSchemaError> {
    read_number(buf, "float64")
}

/// The layout of a flat (memory dump) type: size and alignment, plus the
/// field offsets of a struct. `EA3` packs struct fields back to back;
/// `Latest` uses C layout (each field at its alignment, the size padded to
/// the largest field alignment). The two agree for every struct whose fields
/// are already naturally aligned.
#[derive(Clone, Debug)]
pub(crate) struct FlatLayout {
    pub(crate) kind: FlatKind,
    pub(crate) size: usize,
    pub(crate) align: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum FlatKind {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    /// Struct name and `(field name, offset, layout)` per field.
    Struct(BrdbInterned, Vec<(BrdbInterned, usize, FlatLayout)>),
}

const fn align_up(offset: usize, align: usize) -> usize {
    offset.div_ceil(align) * align
}

/// The layout of a flat type under the schema's format, or `None` when it
/// cannot be flat (unknown type, or a struct with a non-scalar field).
pub(crate) fn flat_layout(schema: &BrdbSchema, ty: &str) -> Option<FlatLayout> {
    flat_layout_with(schema, ty, schema.is_latest())
}

fn flat_layout_with(schema: &BrdbSchema, ty: &str, c_layout: bool) -> Option<FlatLayout> {
    let prim = |kind, size| {
        Some(FlatLayout {
            kind,
            size,
            align: size,
        })
    };
    match ty {
        "u8" => prim(FlatKind::U8, 1),
        "u16" => prim(FlatKind::U16, 2),
        "u32" => prim(FlatKind::U32, 4),
        "u64" => prim(FlatKind::U64, 8),
        "i8" => prim(FlatKind::I8, 1),
        "i16" => prim(FlatKind::I16, 2),
        "i32" => prim(FlatKind::I32, 4),
        "i64" => prim(FlatKind::I64, 8),
        "f32" => prim(FlatKind::F32, 4),
        "f64" => prim(FlatKind::F64, 8),
        other => {
            let id = schema.intern.get(other)?;
            let s = schema.get_struct_interned(id)?;
            let mut fields = Vec::with_capacity(s.len());
            let (mut offset, mut align) = (0, 1);
            for (prop_id, prop) in s {
                let BrdbSchemaStructProperty::Type(field_ty) = prop else {
                    return None;
                };
                let field =
                    flat_layout_with(schema, schema.intern.lookup_ref(*field_ty)?, c_layout)?;
                if c_layout {
                    offset = align_up(offset, field.align);
                    align = align.max(field.align);
                }
                let size = field.size;
                fields.push((*prop_id, offset, field));
                offset += size;
            }
            Some(FlatLayout {
                kind: FlatKind::Struct(id, fields),
                size: align_up(offset, align),
                align,
            })
        }
    }
}

/// Determine the byte size of a flat type in the schema (0 if it cannot be
/// flat).
pub fn flat_type_size(schema: &BrdbSchema, ty: &str) -> usize {
    flat_layout(schema, ty).map(|l| l.size).unwrap_or(0)
}

/// Decode one flat element from exactly `layout.size` bytes.
fn decode_flat(schema: &Arc<BrdbSchema>, layout: &FlatLayout, bytes: &[u8]) -> BrdbValue {
    fn le<const N: usize>(bytes: &[u8]) -> [u8; N] {
        bytes[..N]
            .try_into()
            .expect("flat element sized by its layout")
    }
    match &layout.kind {
        FlatKind::U8 => BrdbValue::U8(bytes[0]),
        FlatKind::U16 => BrdbValue::U16(u16::from_le_bytes(le(bytes))),
        FlatKind::U32 => BrdbValue::U32(u32::from_le_bytes(le(bytes))),
        FlatKind::U64 => BrdbValue::U64(u64::from_le_bytes(le(bytes))),
        FlatKind::I8 => BrdbValue::I8(bytes[0] as i8),
        FlatKind::I16 => BrdbValue::I16(i16::from_le_bytes(le(bytes))),
        FlatKind::I32 => BrdbValue::I32(i32::from_le_bytes(le(bytes))),
        FlatKind::I64 => BrdbValue::I64(i64::from_le_bytes(le(bytes))),
        FlatKind::F32 => BrdbValue::F32(f32::from_le_bytes(le(bytes))),
        FlatKind::F64 => BrdbValue::F64(f64::from_le_bytes(le(bytes))),
        FlatKind::Struct(name, fields) => {
            let properties = fields
                .iter()
                .map(|(prop, offset, field)| {
                    (
                        *prop,
                        decode_flat(schema, field, &bytes[*offset..*offset + field.size]),
                    )
                })
                .collect();
            BrdbValue::Struct(Box::new(BrdbStruct {
                schema: Arc::clone(schema),
                name: *name,
                properties,
            }))
        }
    }
}

/// Read a flat type from a buffer. Flat types are memory dumps of structs or
/// numeric types and are not encoded in a message pack format.
pub fn read_flat_type(
    schema: &Arc<BrdbSchema>,
    ty: &str,
    buf: &mut impl Read,
) -> Result<BrdbValue, BrdbSchemaError> {
    let layout = flat_layout(schema, ty)
        .filter(|l| l.size > 0)
        .ok_or_else(|| BrdbSchemaError::InvalidFlatType(ty.to_owned()))?;
    let mut bytes = vec![0; layout.size];
    buf.read_exact(&mut bytes)
        .map_err(BrdbSchemaError::ReadError)?;
    Ok(decode_flat(schema, &layout, &bytes))
}
