use indexmap::{IndexMap, IndexSet};

use crate::{
    errors::BrdbSchemaError,
    schema::{
        BrdbDynamicArray, BrdbDynamicEnum, BrdbDynamicMap, BrdbInterned, BrdbSchema,
        BrdbSchemaEnum, BrdbStruct, BrdbValue, EA3WireVariant,
    },
};

pub type BrdbArrayIter<'a> = Box<dyn ExactSizeIterator<Item = &'a dyn AsBrdbValue> + 'a>;
pub type BrdbMapIter<'a> =
    Box<dyn ExactSizeIterator<Item = (&'a dyn AsBrdbValue, &'a dyn AsBrdbValue)> + 'a>;

/// A helper trait to allow serializing implementing types to msgpack schema format
pub trait AsBrdbValue: Send + Sync {
    fn as_brdb_bool(&self) -> Result<bool, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "bool",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_u8(&self) -> Result<u8, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "u8",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_u16(&self) -> Result<u16, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "u16",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_u32(&self) -> Result<u32, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "u32",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_u64(&self) -> Result<u64, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "u64",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_i8(&self) -> Result<i8, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "i8",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_i16(&self) -> Result<i16, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "i16",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_i32(&self) -> Result<i32, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "i32",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_i64(&self) -> Result<i64, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "i64",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_f32(&self) -> Result<f32, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "f32",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_f64(&self) -> Result<f64, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "f64",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_str(&self) -> Result<&str, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "str",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_asset(
        &self,
        _schema: &BrdbSchema,
        _ty: &str,
    ) -> Result<Option<usize>, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "asset",
            std::any::type_name::<Self>(),
        ))
    }
    fn as_brdb_enum(
        &self,
        _schema: &BrdbSchema,
        _def: &BrdbSchemaEnum,
    ) -> Result<i32, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "enum",
            std::any::type_name::<Self>(),
        ))
    }

    fn as_brdb_ea3_wire_variant(
        &self,
    ) -> Result<crate::schema::value::EA3WireVariant, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "wire variant",
            std::any::type_name::<Self>(),
        ))
    }

    /// Borrowed fast path for values that ARE wire variants. Must return
    /// `Some` only when the value encodes purely as a wire variant (its
    /// member/map/array probes all decline); the writer then skips those
    /// probes and the clone `as_brdb_ea3_wire_variant` would make.
    fn as_brdb_ea3_wire_variant_ref(&self) -> Option<&crate::schema::value::EA3WireVariant> {
        None
    }

    /// A `dynamic_array` value: the element type name (a `types` entry) and
    /// the elements.
    fn as_brdb_dynamic_array(&self) -> Result<(&str, BrdbArrayIter<'_>), BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "dynamic array",
            std::any::type_name::<Self>(),
        ))
    }

    /// A `dynamic_map` value: the key and value type names (`types` entries)
    /// and the pairs.
    fn as_brdb_dynamic_map(&self) -> Result<(&str, &str, BrdbMapIter<'_>), BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "dynamic map",
            std::any::type_name::<Self>(),
        ))
    }

    /// A `dynamic_enum` value: the enum type name (a `types` entry) and the
    /// enum value.
    fn as_brdb_dynamic_enum(&self) -> Result<(&str, i64), BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "dynamic enum",
            std::any::type_name::<Self>(),
        ))
    }

    /// The decoded value itself, for values that are a [`BrdbValue`]. Lets
    /// the variant writer pick an alternative by the value's own type (a
    /// round-tripped read carries its member's value, not a tag).
    fn as_brdb_value(&self) -> Option<&BrdbValue> {
        None
    }

    /// When this value should encode as a named member of a schema `variant`
    /// (a tagged union whose members are structs, e.g. `BRInventoryEntryVariant`
    /// -> `BRInventoryEntryNothing`), return the member struct's name. The
    /// writer emits `uint(tag) + <that struct>`. `None` (the default) means
    /// "not a struct-member variant" and the writer falls back to the wire
    /// map/array/scalar variant paths.
    fn as_brdb_variant_member(&self) -> Option<&str> {
        None
    }

    /// Whether ANY struct property is set on this value. When this returns
    /// `false`, the planned writer emits the type's precomputed all-default
    /// byte stream in one copy instead of probing every field. The default
    /// `true` keeps the per-field path; only implementations that can answer
    /// exactly (e.g. a keyed store checking emptiness) should override.
    fn has_brdb_struct_props(&self) -> bool {
        true
    }

    /// Cheap presence probe for a struct property. When this returns
    /// `false`, the schema writer takes the default/zero path directly
    /// instead of paying for a `MissingStructField` error (two `String`
    /// allocations) per unset field. Implementations that can't answer
    /// cheaply keep the default `true`; the writer then falls back to
    /// the erroring accessors below.
    fn has_brdb_struct_prop(
        &self,
        _schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        _prop_name: BrdbInterned,
    ) -> bool {
        true
    }

    /// Read a struct property value, or `None` when the field is unset.
    /// The default composes `has_brdb_struct_prop` with
    /// `as_brdb_struct_prop_value`; implementations backed by a keyed
    /// lookup override it to probe their storage once.
    fn as_brdb_struct_prop_opt(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<Option<&dyn AsBrdbValue>, BrdbSchemaError> {
        if !self.has_brdb_struct_prop(schema, struct_name, prop_name) {
            return Ok(None);
        }
        match self.as_brdb_struct_prop_value(schema, struct_name, prop_name) {
            Ok(v) => Ok(Some(v)),
            Err(BrdbSchemaError::MissingStructField(..)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read a specific struct property value from the schema.
    fn as_brdb_struct_prop_value(
        &self,
        _schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        _prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "struct property",
            std::any::type_name::<Self>(),
        ))
    }

    /// Get the the number of entries in a struct property.
    fn as_brdb_struct_prop_array(
        &self,
        _schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        _prop_name: BrdbInterned,
    ) -> Result<BrdbArrayIter<'_>, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "struct property array",
            std::any::type_name::<Self>(),
        ))
    }

    /// Get the the number of entries in a struct property.
    fn as_brdb_struct_prop_map(
        &self,
        _schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        _prop_name: BrdbInterned,
    ) -> Result<BrdbMapIter<'_>, BrdbSchemaError> {
        Err(BrdbSchemaError::UnimplementedCast(
            "struct property map",
            std::any::type_name::<Self>(),
        ))
    }
}

impl AsBrdbValue for () {}

/// A nested-struct default value: `field name -> boxed value`. Emitted by the
/// `extract_defaults` example so struct-typed component defaults (e.g. `Vector2D` `{X, Y}`,
/// `LinearColor` `{R, G, B, A}`) survive into `STRUCT_DEFAULTS` and get written for unset
/// struct fields instead of falling back to zeros. Answers `as_brdb_struct_prop_value` by
/// field-name lookup, mirroring [`BrdbStruct`]. Values may themselves be nested.
pub struct NestedStructDefault(pub Vec<(&'static str, Box<dyn AsBrdbValue>)>);

impl AsBrdbValue for NestedStructDefault {
    fn has_brdb_struct_prop(
        &self,
        schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> bool {
        schema
            .intern
            .lookup_ref(prop_name)
            .is_some_and(|name| self.0.iter().any(|(k, _)| *k == name))
    }

    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        if let Some(name) = schema.intern.lookup_ref(prop_name) {
            if let Some((_, v)) = self.0.iter().find(|(k, _)| *k == name) {
                return Ok(v.as_ref());
            }
        }
        Err(BrdbSchemaError::MissingStructField(
            schema
                .intern
                .lookup(struct_name)
                .unwrap_or_else(|| "nested struct".to_owned()),
            schema
                .intern
                .lookup(prop_name)
                .unwrap_or_else(|| "unknown property".to_owned()),
        ))
    }
}

macro_rules! as_brdb_fn {
    ($fn_name:ident, $ty:ty, $method:ident) => {
        fn $fn_name(&self) -> Result<$ty, BrdbSchemaError> {
            if let BrdbValue::$method(v) = self {
                Ok(*v as $ty)
            } else {
                Err(BrdbSchemaError::ExpectedType(
                    stringify!($ty).to_owned(),
                    self.get_type().to_string(),
                ))
            }
        }
    };
}

/// A default impl for `BrdbValue`.
impl AsBrdbValue for BrdbValue {
    as_brdb_fn!(as_brdb_bool, bool, Bool);
    as_brdb_fn!(as_brdb_u8, u8, U8);
    as_brdb_fn!(as_brdb_u16, u16, U16);
    as_brdb_fn!(as_brdb_u32, u32, U32);
    as_brdb_fn!(as_brdb_u64, u64, U64);
    as_brdb_fn!(as_brdb_i8, i8, I8);
    as_brdb_fn!(as_brdb_i16, i16, I16);
    as_brdb_fn!(as_brdb_i32, i32, I32);
    as_brdb_fn!(as_brdb_i64, i64, I64);
    as_brdb_fn!(as_brdb_f32, f32, F32);
    as_brdb_fn!(as_brdb_f64, f64, F64);
    fn as_brdb_str(&self) -> Result<&str, BrdbSchemaError> {
        if let BrdbValue::String(s) = self {
            Ok(s)
        } else {
            Err(BrdbSchemaError::ExpectedType(
                "str".to_owned(),
                self.get_type().to_string(),
            ))
        }
    }
    fn as_brdb_asset(
        &self,
        _schema: &BrdbSchema,
        _ty: &str,
    ) -> Result<Option<usize>, BrdbSchemaError> {
        if let BrdbValue::Asset(index) = self {
            Ok(*index)
        } else {
            Err(BrdbSchemaError::ExpectedType(
                "asset".to_owned(),
                self.get_type().to_string(),
            ))
        }
    }
    fn as_brdb_enum(
        &self,
        _schema: &BrdbSchema,
        _def: &BrdbSchemaEnum,
    ) -> Result<i32, BrdbSchemaError> {
        if let BrdbValue::Enum(e) = self {
            Ok(e.value as i32)
        } else {
            Err(BrdbSchemaError::ExpectedType(
                "enum".to_owned(),
                self.get_type().to_string(),
            ))
        }
    }
    fn as_brdb_ea3_wire_variant(&self) -> Result<EA3WireVariant, BrdbSchemaError> {
        if let BrdbValue::EA3WireVar(wire) = self {
            Ok(wire.to_owned())
        } else {
            Err(BrdbSchemaError::ExpectedType(
                "wire variant".to_owned(),
                self.get_type().to_string(),
            ))
        }
    }
    fn as_brdb_value(&self) -> Option<&BrdbValue> {
        Some(self)
    }
    fn as_brdb_dynamic_array(&self) -> Result<(&str, BrdbArrayIter<'_>), BrdbSchemaError> {
        match self {
            BrdbValue::DynamicArray(a) => a.as_brdb_dynamic_array(),
            _ => Err(BrdbSchemaError::ExpectedType(
                "dynamic array".to_owned(),
                self.get_type().to_string(),
            )),
        }
    }
    fn as_brdb_dynamic_map(&self) -> Result<(&str, &str, BrdbMapIter<'_>), BrdbSchemaError> {
        match self {
            BrdbValue::DynamicMap(m) => m.as_brdb_dynamic_map(),
            _ => Err(BrdbSchemaError::ExpectedType(
                "dynamic map".to_owned(),
                self.get_type().to_string(),
            )),
        }
    }
    fn as_brdb_dynamic_enum(&self) -> Result<(&str, i64), BrdbSchemaError> {
        match self {
            BrdbValue::Enum(e) => Ok((e.get_name(), e.value as i64)),
            _ => Err(BrdbSchemaError::ExpectedType(
                "dynamic enum".to_owned(),
                self.get_type().to_string(),
            )),
        }
    }
    fn as_brdb_ea3_wire_variant_ref(&self) -> Option<&EA3WireVariant> {
        if let BrdbValue::EA3WireVar(wire) = self {
            Some(wire)
        } else {
            None
        }
    }
    fn has_brdb_struct_prop(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> bool {
        match self {
            BrdbValue::Struct(s) => s.has_brdb_struct_prop(schema, struct_name, prop_name),
            _ => true,
        }
    }
    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        let BrdbValue::Struct(s) = self else {
            return Err(BrdbSchemaError::ExpectedType(
                "struct".to_owned(),
                self.get_type().to_string(),
            ));
        };

        s.as_brdb_struct_prop_value(schema, struct_name, prop_name)
    }
    fn as_brdb_struct_prop_array(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<BrdbArrayIter<'_>, BrdbSchemaError> {
        let BrdbValue::Struct(s) = self else {
            return Err(BrdbSchemaError::ExpectedType(
                "struct".to_owned(),
                self.get_type().to_string(),
            ));
        };
        s.as_brdb_struct_prop_array(schema, struct_name, prop_name)
    }
    fn as_brdb_struct_prop_map(
        &self,
        schema: &BrdbSchema,
        struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<BrdbMapIter<'_>, BrdbSchemaError> {
        let BrdbValue::Struct(s) = self else {
            return Err(BrdbSchemaError::ExpectedType(
                "struct".to_owned(),
                self.get_type().to_string(),
            ));
        };
        s.as_brdb_struct_prop_map(schema, struct_name, prop_name)
    }
}

impl BrdbStruct {
    /// The property for a field id of `schema`. Fields are keyed by this
    /// struct's own schema; when writing with another schema (e.g. a value
    /// read from a save, written with the built-in schema), the field is
    /// matched by name.
    fn prop_for(&self, schema: &BrdbSchema, prop_name: BrdbInterned) -> Option<&BrdbValue> {
        if std::ptr::eq(&*self.schema, schema) {
            return self.properties.get(&prop_name);
        }
        let name = schema.intern.lookup_ref(prop_name)?;
        self.properties.get(&self.schema.intern.get(name)?)
    }
}

impl AsBrdbValue for BrdbStruct {
    fn has_brdb_struct_prop(
        &self,
        schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> bool {
        self.prop_for(schema, prop_name).is_some()
    }

    fn as_brdb_struct_prop_value(
        &self,
        schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
        if let Some(prop) = self.prop_for(schema, prop_name) {
            Ok(prop)
        } else {
            Err(BrdbSchemaError::MissingStructField(
                schema
                    .intern
                    .lookup(self.name)
                    .unwrap_or_else(|| "unknown struct".to_owned()),
                schema
                    .intern
                    .lookup(prop_name)
                    .unwrap_or_else(|| "unknown property".to_owned()),
            ))
        }
    }

    fn as_brdb_struct_prop_array(
        &self,
        schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<BrdbArrayIter<'_>, BrdbSchemaError> {
        match self.prop_for(schema, prop_name) {
            Some(BrdbValue::Array(vec)) | Some(BrdbValue::FlatArray(vec)) => Ok(vec.as_brdb_iter()),
            _ => Err(BrdbSchemaError::MissingStructField(
                schema
                    .intern
                    .lookup(self.name)
                    .unwrap_or_else(|| "unknown struct".to_owned()),
                schema
                    .intern
                    .lookup(prop_name)
                    .unwrap_or_else(|| "unknown property".to_owned()),
            )),
        }
    }

    fn as_brdb_struct_prop_map(
        &self,
        schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        prop_name: BrdbInterned,
    ) -> Result<BrdbMapIter<'_>, BrdbSchemaError> {
        if let Some(BrdbValue::Map(map)) = self.prop_for(schema, prop_name) {
            Ok(Box::new(map.iter().map(|(k, v)| {
                (k as &dyn AsBrdbValue, v as &dyn AsBrdbValue)
            })))
        } else {
            Err(BrdbSchemaError::MissingStructField(
                schema
                    .intern
                    .lookup(self.name)
                    .unwrap_or_else(|| "unknown struct".to_owned()),
                schema
                    .intern
                    .lookup(prop_name)
                    .unwrap_or_else(|| "unknown property".to_owned()),
            ))
        }
    }
}

impl AsBrdbValue for BrdbDynamicArray {
    fn as_brdb_dynamic_array(&self) -> Result<(&str, BrdbArrayIter<'_>), BrdbSchemaError> {
        Ok((&self.element_type, self.values.as_brdb_iter()))
    }
}

impl AsBrdbValue for BrdbDynamicMap {
    fn as_brdb_dynamic_map(&self) -> Result<(&str, &str, BrdbMapIter<'_>), BrdbSchemaError> {
        Ok((
            &self.key_type,
            &self.value_type,
            self.entries.as_brdb_map_iter(),
        ))
    }
}

impl AsBrdbValue for BrdbDynamicEnum {
    fn as_brdb_dynamic_enum(&self) -> Result<(&str, i64), BrdbSchemaError> {
        Ok((&self.enum_name, self.value))
    }
}

impl AsBrdbValue for EA3WireVariant {
    fn as_brdb_ea3_wire_variant(&self) -> Result<EA3WireVariant, BrdbSchemaError> {
        Ok(self.clone())
    }
    fn as_brdb_ea3_wire_variant_ref(&self) -> Option<&EA3WireVariant> {
        Some(self)
    }
}

macro_rules! as_brdb_int(
    ($ty:ty, $($rest:ty),*) => {
        as_brdb_int!($ty);
        as_brdb_int!($($rest),*);
    };
    ($ty:ty) => {
        impl AsBrdbValue for $ty {
            fn as_brdb_bool(&self) -> Result<bool, BrdbSchemaError> {
                Ok(*self != 0)
            }
            fn as_brdb_u8(&self) -> Result<u8, BrdbSchemaError> {
                Ok(*self as u8)
            }
            fn as_brdb_u16(&self) -> Result<u16, BrdbSchemaError> {
                Ok(*self as u16)
            }
            fn as_brdb_u32(&self) -> Result<u32, BrdbSchemaError> {
                Ok(*self as u32)
            }
            fn as_brdb_u64(&self) -> Result<u64, BrdbSchemaError> {
                Ok(*self as u64)
            }
            fn as_brdb_i8(&self) -> Result<i8, BrdbSchemaError> {
                Ok(*self as i8)
            }
            fn as_brdb_i16(&self) -> Result<i16, BrdbSchemaError> {
                Ok(*self as i16)
            }
            fn as_brdb_i32(&self) -> Result<i32, BrdbSchemaError> {
                Ok(*self as i32)
            }
            fn as_brdb_i64(&self) -> Result<i64, BrdbSchemaError> {
                Ok(*self as i64)
            }
            fn as_brdb_f32(&self) -> Result<f32, BrdbSchemaError> {
                Ok(*self as f32)
            }
            fn as_brdb_f64(&self) -> Result<f64, BrdbSchemaError> {
                Ok(*self as f64)
            }
            fn as_brdb_ea3_wire_variant(
                &self,
            ) -> Result<EA3WireVariant, BrdbSchemaError> {
                Ok((*self).into())
            }
            fn as_brdb_enum(
                &self,
                _schema: &BrdbSchema,
                _def: &BrdbSchemaEnum,
            ) -> Result<i32, BrdbSchemaError> {
                Ok(*self as i32)
            }
        }
    }
);
as_brdb_int!(u8, u16, u32, u64, i8, i16, i32, i64);

macro_rules! as_brdb_float {
    ($ty:ty, $($rest:ty),*) => {
        as_brdb_float!($ty);
        as_brdb_float!($($rest),*);
    };
    ($ty:ty) => {
        impl AsBrdbValue for $ty {
            fn as_brdb_f32(&self) -> Result<f32, BrdbSchemaError> {
                Ok(*self as f32)
            }
            fn as_brdb_f64(&self) -> Result<f64, BrdbSchemaError> {
                Ok(*self as f64)
            }
            fn as_brdb_ea3_wire_variant(
                &self,
            ) -> Result<EA3WireVariant, BrdbSchemaError> {
                Ok(EA3WireVariant::Number(*self as f64))
            }
        }
    };
}
as_brdb_float!(f32, f64);

impl AsBrdbValue for bool {
    fn as_brdb_bool(&self) -> Result<bool, BrdbSchemaError> {
        Ok(*self)
    }
    fn as_brdb_ea3_wire_variant(
        &self,
    ) -> Result<crate::schema::value::EA3WireVariant, BrdbSchemaError> {
        Ok(EA3WireVariant::Bool(*self))
    }
}
impl AsBrdbValue for String {
    fn as_brdb_str(&self) -> Result<&str, BrdbSchemaError> {
        Ok(self)
    }
    fn as_brdb_ea3_wire_variant(&self) -> Result<EA3WireVariant, BrdbSchemaError> {
        Ok(EA3WireVariant::Str(self.clone()))
    }
}
impl AsBrdbValue for &str {
    fn as_brdb_str(&self) -> Result<&str, BrdbSchemaError> {
        Ok(self)
    }
    fn as_brdb_ea3_wire_variant(&self) -> Result<EA3WireVariant, BrdbSchemaError> {
        Ok(EA3WireVariant::Str(self.to_string()))
    }
}

pub trait AsBrdbIter {
    fn as_brdb_iter(&self) -> BrdbArrayIter<'_>;
}
pub trait AsBrdbMapIter {
    fn as_brdb_map_iter(&self) -> BrdbMapIter<'_>;
}

impl<T: AsBrdbValue> AsBrdbIter for Vec<T> {
    fn as_brdb_iter(&self) -> BrdbArrayIter<'_> {
        Box::new(self.iter().map(|v| v as &dyn AsBrdbValue))
    }
}
impl<T: AsBrdbValue, S> AsBrdbIter for IndexSet<T, S> {
    fn as_brdb_iter(&self) -> BrdbArrayIter<'_> {
        Box::new(self.iter().map(|v| v as &dyn AsBrdbValue))
    }
}
impl<K: AsBrdbValue, V: AsBrdbValue, S> AsBrdbMapIter for IndexMap<K, V, S> {
    fn as_brdb_map_iter(&self) -> BrdbMapIter<'_> {
        Box::new(
            self.iter()
                .map(|(k, v)| (k as &dyn AsBrdbValue, v as &dyn AsBrdbValue)),
        )
    }
}

// Automatically implement `AsBrdbValue` for tuples of any number of elements
// and treat them as struct properties.
macro_rules! as_brdb_tuple {
    ($($name:ident $index:tt),*) => {
        impl<$($name: AsBrdbValue),*> AsBrdbValue for ($($name),*) {
            fn as_brdb_struct_prop_value(
                &self,
                schema: &BrdbSchema,
                struct_name: BrdbInterned,
                prop_name: BrdbInterned,
            ) -> Result<&dyn AsBrdbValue, BrdbSchemaError> {
                let s_ty = schema.get_struct_interned(struct_name).unwrap();
                let prop_index = s_ty.get_index_of(&prop_name);
                match prop_index {
                    $(
                        Some($index) => Ok(&self.$index),
                    )*
                    _ => Err(BrdbSchemaError::MissingStructField(
                        struct_name.get_or_else(schema, || "unknown struct".to_owned()),
                        prop_name.get_or_else(schema, || "unknown property".to_owned()),
                    )),
                }
            }
        }
    };
}

as_brdb_tuple!(A 0, B 1);
as_brdb_tuple!(A 0, B 1, C 2);
as_brdb_tuple!(A 0, B 1, C 2, D 3);
as_brdb_tuple!(A 0, B 1, C 2, D 3, E 4);
