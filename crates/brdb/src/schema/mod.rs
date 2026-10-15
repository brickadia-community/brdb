use std::{
    fmt::Display,
    io::{Read, Write},
    sync::Arc,
};

use indexmap::IndexMap;
use lalrpop_util::lalrpop_mod;
use rmp::{Marker, decode::RmpRead};
use rustc_hash::FxBuildHasher;

mod global_data;
mod intern;
pub mod read;
mod value;
mod wire_value;
pub mod write;
pub use global_data::*;
pub use intern::*;
pub use value::*;
pub use wire_value::*;
pub mod as_brdb;
#[cfg(test)]
mod format_tests;

pub(crate) type PlaintextEnumBody = Vec<(String, i32)>;
pub(crate) type PlaintextStructBody = Vec<(String, BrdbStructPropRaw)>;
pub(crate) type PlaintextVariantBody = Vec<String>;

/// One top-level declaration of the plaintext `.schema` format.
pub(crate) enum PlaintextDecl {
    Enum(String, PlaintextEnumBody),
    Variant(String, PlaintextVariantBody),
    Struct(String, PlaintextStructBody),
    Types(Vec<String>),
}

lalrpop_mod!(plaintext);

use crate::{
    errors::BrdbSchemaError,
    schema::{
        as_brdb::AsBrdbValue,
        read::{read_owned_str, read_str_from_len},
    },
};

/// Field type whose element type is chosen per instance from the schema's
/// `types` table.
pub const DYNAMIC_ARRAY: &str = "dynamic_array";
/// Field type whose key and value types are chosen per instance from the
/// schema's `types` table.
pub const DYNAMIC_MAP: &str = "dynamic_map";
/// An enum value tagged with its enum type: a `types` index, then the value.
/// Written by the game but not described by the draft spec.
pub const DYNAMIC_ENUM: &str = "dynamic_enum";

/// Type names the game writes that are not declared in the schema and are not
/// in the draft spec. `Bool` and `Str` are read and written like `bool` and
/// `str` (assumed: a recent game update writes them, but no saved instance
/// has been observed). The handle types have no known payload encoding, so
/// reading or writing a value of those types is an error; containers of them
/// that are empty still round-trip.
pub const BRICK_HANDLE: &str = "brick_handle";
pub const COMPONENT_HANDLE: &str = "component_handle";

/// EA3 wire graph union used directly as a field type, without a variant
/// declaration: selectors `[f64, i64, bool, weak object, exec]`.
pub const EA3_WIRE_GRAPH_VARIANT: &str = "wire_graph_variant";
/// EA3 numeric wire graph union: selectors `[f64, i64]`.
pub const EA3_WIRE_GRAPH_PRIM_MATH_VARIANT: &str = "wire_graph_prim_math_variant";

/// The save format a schema belongs to: which game builds can read it, its
/// container form, and the value encodings that differ between builds.
///
/// | | `EA3` | `Latest` |
/// |---|---|---|
/// | container | `[enums, variants, structs]` array (`[enums, structs]` also read) | `{enums, variants, structs, types}` map, empty sections omitted |
/// | `types` table, `dynamic_*` | not expressible | supported |
/// | `weak_object` | the asset index is read and written | written as -1, read as null |
/// | `wire_graph_variant` selector 3 | carries an i64 index | no payload (spec layout `[f64, i64, bool, nil, nil]`) |
/// | `u8` 128..=255 | written as -128..=-1 (the only form EA3 builds read) | written unsigned |
/// | flat array elements | fields packed back to back | C layout (fields aligned, size padded) |
///
/// Both formats decode integers leniently (any integer tag, cast to the
/// field type) and accept any integer or float tag for `f32`/`f64`.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum BrdbSchemaFormat {
    /// The schema's own format: the one it was read in, otherwise `Latest`
    /// when it has a `types` table, otherwise `EA3`. When writing a schema,
    /// anything that needs the `Latest` container (a `types` table or
    /// dynamic containers) is always written as `Latest`.
    #[default]
    Auto,
    /// The format of retail EA3 builds (and every save written before the
    /// map form). Cannot express a `types` table.
    EA3,
    /// The current spec format.
    Latest,
}

/// Canonical simple type for a type name that is an alias of one. Only used
/// when the name is not defined by the schema itself.
pub(crate) fn builtin_alias(ty: &str) -> Option<&'static str> {
    match ty {
        "Bool" => Some("bool"),
        "Str" => Some("str"),
        _ => None,
    }
}

/// Whether two type names describe the same payload type for the purpose of
/// matching variant alternatives and `types` entries by type.
pub(crate) fn same_type(a: &str, b: &str) -> bool {
    a == b || builtin_alias(a).unwrap_or(a) == builtin_alias(b).unwrap_or(b)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrdbSchemaStructProperty {
    Type(BrdbInterned),
    Array(BrdbInterned),
    FlatArray(BrdbInterned),
    Map(BrdbInterned, BrdbInterned),
}
pub type BrdbSchemaStruct = IndexMap<BrdbInterned, BrdbSchemaStructProperty, FxBuildHasher>;
pub type BrdbSchemaEnum = IndexMap<BrdbInterned, i32, FxBuildHasher>;

pub type BrdbSchemaMetaEnum = (String, Vec<(String, i32)>);
pub type BrdbSchemaMetaStruct = (String, Vec<(String, BrdbStructPropRaw)>);
/// Enums, variants, structs and the ordered `types` table.
pub type BrdbSchemaMeta = (
    Vec<BrdbSchemaMetaEnum>,
    Vec<BrdbSchemaMetaVariant>,
    Vec<BrdbSchemaMetaStruct>,
    Vec<String>,
);
/// A tagged-union/variant: a name and the ordered list of member type names it
/// can hold. The encoded value is a uint selector (the member index in the
/// saved list) followed by the value of that member type (e.g. `WireValue`).
pub type BrdbSchemaMetaVariant = (String, Vec<String>);

impl BrdbSchemaStructProperty {
    pub fn as_string(&self, schema: &BrdbSchema) -> String {
        match self {
            BrdbSchemaStructProperty::Type(t) => {
                schema.intern.lookup(*t).unwrap_or("UnknownType".to_owned())
            }
            BrdbSchemaStructProperty::Array(t) => {
                format!(
                    "{}[]",
                    schema
                        .intern
                        .lookup(*t)
                        .unwrap_or("UnknownArrayType".to_owned())
                )
            }
            BrdbSchemaStructProperty::FlatArray(t) => format!(
                "{}[flat]",
                schema
                    .intern
                    .lookup(*t)
                    .unwrap_or("UnknownFlatArrayType".to_owned())
            ),
            BrdbSchemaStructProperty::Map(k, v) => {
                let key = schema
                    .intern
                    .lookup(*k)
                    .unwrap_or("UnknownMapKeyType".to_owned());
                let value = schema
                    .intern
                    .lookup(*v)
                    .unwrap_or("UnknownMapValueType".to_owned());
                format!("{{{key}: {value}}}")
            }
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct BrdbSchema {
    pub intern: BrdbIntern,
    pub(crate) global_data: Arc<BrdbSchemaGlobalData>,
    pub enums: IndexMap<BrdbInterned, BrdbSchemaEnum, FxBuildHasher>,
    pub structs: IndexMap<BrdbInterned, BrdbSchemaStruct, FxBuildHasher>,
    /// Tagged-union/variant definitions: name -> ordered member types.
    pub variants: IndexMap<BrdbInterned, Vec<BrdbInterned>, FxBuildHasher>,
    /// The ordered `types` table that dynamic containers and dynamic enums
    /// index into. Append-only; empty for schemas that do not use it.
    pub types: Vec<BrdbInterned>,
    /// The format this schema was read in (`EA3` for the array container,
    /// `Latest` for the map), when it was read from binary.
    pub(crate) source_format: Option<BrdbSchemaFormat>,
    /// An explicit format for encoding values, overriding the inferred one.
    pub(crate) format_override: Option<BrdbSchemaFormat>,
}

impl Display for BrdbSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (name, values) in &self.enums {
            let name = self
                .intern
                .lookup(*name)
                .unwrap_or("UnknownEnum".to_owned());
            writeln!(f, "enum {name} {{")?;
            for (key, value) in values {
                let key = self.intern.lookup(*key).unwrap_or("UnknownKey".to_owned());
                writeln!(f, "    {key} = {value},")?;
            }
            writeln!(f, "}}")?;
        }
        for (name, members) in &self.variants {
            let name = self
                .intern
                .lookup(*name)
                .unwrap_or("UnknownVariant".to_owned());
            writeln!(f, "variant {name} {{")?;
            for member in members {
                let member = self
                    .intern
                    .lookup(*member)
                    .unwrap_or("UnknownMember".to_owned());
                writeln!(f, "    {member},")?;
            }
            writeln!(f, "}}")?;
        }
        for (name, properties) in &self.structs {
            let name = self
                .intern
                .lookup(*name)
                .unwrap_or("UnknownStruct".to_owned());
            writeln!(f, "struct {name} {{")?;
            for (prop_name, prop_type) in properties {
                let prop_name = self
                    .intern
                    .lookup(*prop_name)
                    .unwrap_or("UnknownProperty".to_owned());
                writeln!(f, "    {prop_name}: {},", prop_type.as_string(self))?;
            }
            writeln!(f, "}}")?;
        }
        if !self.types.is_empty() {
            writeln!(f, "types {{")?;
            for ty in &self.types {
                let ty = self.intern.lookup(*ty).unwrap_or("UnknownType".to_owned());
                writeln!(f, "    {ty},")?;
            }
            writeln!(f, "}}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrdbStructPropRaw {
    Type(String),
    Array(String),
    FlatArray(String),
    Map(String, String),
}

impl BrdbSchema {
    pub fn get_struct(&self, name: &str) -> Option<&BrdbSchemaStruct> {
        self.structs.get(&self.intern.get(name)?)
    }

    pub fn get_struct_interned(&self, id: BrdbInterned) -> Option<&BrdbSchemaStruct> {
        self.structs.get(&id)
    }

    pub fn get_enum(&self, name: &str) -> Option<&BrdbSchemaEnum> {
        self.enums.get(&self.intern.get(name)?)
    }

    pub fn get_enum_interned(&self, id: BrdbInterned) -> Option<&BrdbSchemaEnum> {
        self.enums.get(&id)
    }

    pub fn get_variant(&self, name: &str) -> Option<&Vec<BrdbInterned>> {
        self.variants.get(&self.intern.get(name)?)
    }

    pub fn get_variant_interned(&self, id: BrdbInterned) -> Option<&Vec<BrdbInterned>> {
        self.variants.get(&id)
    }

    pub fn parse_to_meta(input: &str) -> Result<BrdbSchemaMeta, String> {
        let decls = plaintext::MetaParser::new()
            .parse(input)
            .map_err(|e| e.to_string())?;
        let mut meta: BrdbSchemaMeta = Default::default();
        for decl in decls {
            match decl {
                PlaintextDecl::Enum(name, body) => meta.0.push((name, body)),
                PlaintextDecl::Variant(name, body) => meta.1.push((name, body)),
                PlaintextDecl::Struct(name, body) => meta.2.push((name, body)),
                PlaintextDecl::Types(types) => meta.3.extend(types),
            }
        }
        Ok(meta)
    }

    /// Parse a schema from a plaintext input string into a `BrdbSchema`
    pub fn new_parsed(input: &str) -> Result<BrdbSchema, BrdbSchemaError> {
        let (enums, variants, structs, types) =
            Self::parse_to_meta(input).map_err(BrdbSchemaError::ParseError)?;
        Ok(Self::from_meta(enums, variants, structs, types))
    }

    /// Read a schema from a msgpack .schema file into a human readable format
    pub fn read_to_meta(buf: impl Read) -> Result<BrdbSchemaMeta, BrdbSchemaError> {
        Self::read_to_meta_with_format(buf).map(|(meta, _)| meta)
    }

    /// Like [`Self::read_to_meta`], also reporting the container form found:
    /// the spec map `{enums, variants, structs, types}` (any subset, any
    /// order) or a legacy `[enums, structs]` / `[enums, variants, structs]`
    /// array.
    pub fn read_to_meta_with_format(
        mut buf: impl Read,
    ) -> Result<(BrdbSchemaMeta, BrdbSchemaFormat), BrdbSchemaError> {
        let marker = rmp::decode::read_marker(&mut buf)
            .map_err(|e| BrdbSchemaError::RmpMarkerReadError(e.0))?;
        let mut meta: BrdbSchemaMeta = Default::default();
        match marker {
            Marker::FixArray(_) | Marker::Array16 | Marker::Array32 => {
                let len = container_len(&mut buf, marker)?;
                if len != 2 && len != 3 {
                    return Err(BrdbSchemaError::InvalidHeader(len));
                }
                meta.0 = read_enum_section(&mut buf)?;
                if len == 3 {
                    meta.1 = read_variant_section(&mut buf)?;
                }
                meta.2 = read_struct_section(&mut buf)?;
                Ok((meta, BrdbSchemaFormat::EA3))
            }
            Marker::FixMap(_) | Marker::Map16 | Marker::Map32 => {
                let len = container_len(&mut buf, marker)?;
                let mut seen: Vec<String> = Vec::with_capacity(4);
                for _ in 0..len {
                    let section = read_owned_str(&mut buf)?;
                    if seen.contains(&section) {
                        return Err(BrdbSchemaError::InvalidSchema(format!(
                            "duplicate schema section '{section}'"
                        )));
                    }
                    match section.as_str() {
                        "enums" => meta.0 = read_enum_section(&mut buf)?,
                        "variants" => meta.1 = read_variant_section(&mut buf)?,
                        "structs" => meta.2 = read_struct_section(&mut buf)?,
                        "types" => meta.3 = read_types_section(&mut buf)?,
                        other => {
                            return Err(BrdbSchemaError::InvalidSchema(format!(
                                "unknown schema section '{other}'"
                            )));
                        }
                    }
                    seen.push(section);
                }
                Ok((meta, BrdbSchemaFormat::Latest))
            }
            marker => Err(BrdbSchemaError::InvalidSchema(format!(
                "schema must be a map or an array, found {marker:?}"
            ))),
        }
    }

    pub fn from_meta(
        enums: impl IntoIterator<Item = (String, Vec<(String, i32)>)>,
        variants: impl IntoIterator<Item = (String, Vec<String>)>,
        structs: impl IntoIterator<Item = (String, Vec<(String, BrdbStructPropRaw)>)>,
        types: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut schema = BrdbSchema::default();
        schema.add_meta(enums, structs);
        schema.add_variants(variants);
        schema.add_types(types);
        schema
    }

    /// Bulk insert enums and structs from metadata into this schema
    pub fn add_meta(
        &mut self,
        enums: impl IntoIterator<Item = (String, Vec<(String, i32)>)>,
        structs: impl IntoIterator<Item = (String, Vec<(String, BrdbStructPropRaw)>)>,
    ) {
        for (name, values) in enums {
            self.add_enum(name, values);
        }
        for (name, props) in structs {
            self.add_struct(name, props);
        }
    }

    /// Merge every section of `meta` into this schema.
    pub fn add_schema_meta(&mut self, meta: BrdbSchemaMeta) {
        let (enums, variants, structs, types) = meta;
        self.add_meta(enums, structs);
        self.add_variants(variants);
        self.add_types(types);
    }

    /// Read from a msgpack .schema buffer into a populated `BrdbSchema` struct
    pub fn read(buf: impl Read) -> Result<BrdbSchema, BrdbSchemaError> {
        let ((enums, variants, structs, types), format) = Self::read_to_meta_with_format(buf)?;
        let mut schema = Self::from_meta(enums, variants, structs, types);
        schema.source_format = Some(format);
        Ok(schema)
    }

    /// The format this schema was read in (`EA3` or `Latest`), or `None`
    /// for a schema that was built or parsed from text.
    pub fn source_format(&self) -> Option<BrdbSchemaFormat> {
        self.source_format
    }

    /// The format (`EA3` or `Latest`, never `Auto`) whose value encodings
    /// this schema reads and writes: an explicit [`Self::set_format`], else
    /// the format it was read in, else `Latest` when it has a `types` table,
    /// else `EA3`.
    pub fn format(&self) -> BrdbSchemaFormat {
        self.format_override
            .or(self.source_format)
            .unwrap_or(if self.types.is_empty() {
                BrdbSchemaFormat::EA3
            } else {
                BrdbSchemaFormat::Latest
            })
    }

    /// Whether values use the `Latest` encodings (see [`BrdbSchemaFormat`]).
    pub fn is_latest(&self) -> bool {
        self.format() == BrdbSchemaFormat::Latest
    }

    /// Choose the format for this schema's value encodings; `Auto` restores
    /// the inferred one.
    pub fn set_format(&mut self, format: BrdbSchemaFormat) {
        self.format_override = (format != BrdbSchemaFormat::Auto).then_some(format);
    }

    /// Append entries to the `types` table. The table is append-only: names
    /// already present keep their index and are not duplicated.
    pub fn add_types(&mut self, types: impl IntoIterator<Item = String>) {
        for ty in types {
            let id = self.intern.get_or_insert(ty);
            if !self.types.contains(&id) {
                self.types.push(id);
            }
        }
    }

    /// The type name at `index` in the `types` table.
    pub fn type_at(&self, index: usize) -> Option<&str> {
        self.types
            .get(index)
            .and_then(|id| self.intern.lookup_ref(*id))
    }

    /// Index of a type in the `types` table, matched by name, falling back to
    /// an equivalent built-in alias (`bool`/`Bool`, `str`/`Str`).
    pub fn type_index(&self, name: &str) -> Option<usize> {
        let names = || self.types.iter().map(|id| self.intern.lookup_ref(*id));
        names()
            .position(|n| n == Some(name))
            .or_else(|| names().position(|n| n.is_some_and(|n| same_type(n, name))))
    }

    /// Whether this schema can only be written in the `Latest` form: it has a
    /// `types` table, or something references a dynamic type.
    pub fn needs_map_form(&self) -> bool {
        if !self.types.is_empty() {
            return true;
        }
        let dynamic = [DYNAMIC_ARRAY, DYNAMIC_MAP, DYNAMIC_ENUM].map(|name| self.intern.get(name));
        let is_dynamic = |id: &BrdbInterned| dynamic.contains(&Some(*id));
        self.structs
            .values()
            .flat_map(|props| props.values())
            .any(|prop| prop_type_refs(prop).iter().flatten().any(is_dynamic))
            || self.variants.values().flatten().any(is_dynamic)
    }

    /// Add a single enum to the schema
    pub fn add_enum(&mut self, name: String, values: Vec<(String, i32)>) {
        let name = self.intern.get_or_insert(name);
        let values = values
            .into_iter()
            .map(|(k, v)| (self.intern.get_or_insert(k), v))
            .collect::<BrdbSchemaEnum>();
        self.enums.insert(name, values);
    }

    /// Add a single struct to the schema
    pub fn add_struct(&mut self, name: String, props: Vec<(String, BrdbStructPropRaw)>) {
        let name = self.intern.get_or_insert(name);
        let props = props
            .into_iter()
            .map(|(prop, prop_ty)| {
                (
                    self.intern.get_or_insert(prop),
                    match prop_ty {
                        BrdbStructPropRaw::Type(ty) => {
                            BrdbSchemaStructProperty::Type(self.intern.get_or_insert(ty))
                        }
                        BrdbStructPropRaw::Array(ty) => {
                            BrdbSchemaStructProperty::Array(self.intern.get_or_insert(ty))
                        }
                        BrdbStructPropRaw::FlatArray(ty) => {
                            BrdbSchemaStructProperty::FlatArray(self.intern.get_or_insert(ty))
                        }
                        BrdbStructPropRaw::Map(k_ty, v_ty) => BrdbSchemaStructProperty::Map(
                            self.intern.get_or_insert(k_ty),
                            self.intern.get_or_insert(v_ty),
                        ),
                    },
                )
            })
            .collect::<BrdbSchemaStruct>();
        self.structs.insert(name, props);
    }

    /// Add tagged-union/variant definitions to the schema. Each entry maps a
    /// variant name to the ordered list of member type names it can hold.
    pub fn add_variants(&mut self, variants: impl IntoIterator<Item = (String, Vec<String>)>) {
        for (name, members) in variants {
            let name = self.intern.get_or_insert(name);
            let members = members
                .into_iter()
                .map(|m| self.intern.get_or_insert(m))
                .collect::<Vec<_>>();
            self.variants.insert(name, members);
        }
    }

    /// Iterate all struct names in this schema.
    pub fn struct_names(&self) -> Vec<String> {
        self.structs
            .keys()
            .filter_map(|k| self.intern.lookup(*k))
            .collect()
    }

    /// Extract a single struct's metadata (and any variant types it references)
    /// back into `BrdbSchemaMeta` form so it can be inserted into another
    /// schema via `add_meta`/`add_variants`.
    pub fn extract_struct_meta(&self, struct_name: &str) -> Option<BrdbSchemaMeta> {
        let id = self.intern.get(struct_name)?;
        let props = self.structs.get(&id)?;
        let enums = Vec::new();
        let mut variants: Vec<BrdbSchemaMetaVariant> = Vec::new();
        let mut structs = Vec::new();

        // Pull in any variant definitions referenced by this struct's
        // properties so the extracted meta is self-contained.
        for prop in props.values() {
            let referenced = match prop {
                BrdbSchemaStructProperty::Type(t)
                | BrdbSchemaStructProperty::Array(t)
                | BrdbSchemaStructProperty::FlatArray(t) => *t,
                BrdbSchemaStructProperty::Map(_, v) => *v,
            };
            if let Some(members) = self.get_variant_interned(referenced) {
                let name = self.intern.lookup(referenced).unwrap();
                if !variants.iter().any(|(n, _)| n == &name) {
                    let member_names = members
                        .iter()
                        .map(|m| self.intern.lookup(*m).unwrap())
                        .collect();
                    variants.push((name, member_names));
                }
            }
        }

        let raw_props: Vec<(String, BrdbStructPropRaw)> = props
            .iter()
            .map(|(k, v)| {
                let key = self.intern.lookup(*k).unwrap();
                let raw = match v {
                    BrdbSchemaStructProperty::Type(t) => {
                        BrdbStructPropRaw::Type(self.intern.lookup(*t).unwrap())
                    }
                    BrdbSchemaStructProperty::Array(t) => {
                        BrdbStructPropRaw::Array(self.intern.lookup(*t).unwrap())
                    }
                    BrdbSchemaStructProperty::FlatArray(t) => {
                        BrdbStructPropRaw::FlatArray(self.intern.lookup(*t).unwrap())
                    }
                    BrdbSchemaStructProperty::Map(k, v) => BrdbStructPropRaw::Map(
                        self.intern.lookup(*k).unwrap(),
                        self.intern.lookup(*v).unwrap(),
                    ),
                };
                (key, raw)
            })
            .collect();
        structs.push((struct_name.to_owned(), raw_props));
        let mut meta = (enums, variants, structs, Vec::new());
        if self.meta_uses_dynamic_types(&meta) {
            meta.3 = self.type_names();
        }
        Some(meta)
    }

    /// Names of the `types` table entries, in order.
    pub fn type_names(&self) -> Vec<String> {
        self.types
            .iter()
            .filter_map(|id| self.intern.lookup(*id))
            .collect()
    }

    fn meta_uses_dynamic_types(&self, meta: &BrdbSchemaMeta) -> bool {
        let dynamic = |n: &str| matches!(n, DYNAMIC_ARRAY | DYNAMIC_MAP | DYNAMIC_ENUM);
        meta.1.iter().flat_map(|(_, m)| m).any(|m| dynamic(m))
            || meta.2.iter().flat_map(|(_, p)| p).any(|(_, p)| match p {
                BrdbStructPropRaw::Type(t)
                | BrdbStructPropRaw::Array(t)
                | BrdbStructPropRaw::FlatArray(t) => dynamic(t),
                BrdbStructPropRaw::Map(k, v) => dynamic(k) || dynamic(v),
            })
    }

    /// Extract the named structs plus every type they transitively reference
    /// (sub-structs, enums, variants, and variant member types) into
    /// self-contained meta. Primitive/unknown type names are skipped. When
    /// anything extracted uses a dynamic type, the whole `types` table (and
    /// every type it names) comes along, so type indices stay valid.
    ///
    /// Used to build a minimal component schema that embeds only what a world
    /// actually uses, mirroring how the game writes `ComponentsShared.schema`
    /// (only the data structs that appear, not the full catalog).
    pub fn extract_structs_transitive<'a>(
        &self,
        seeds: impl IntoIterator<Item = &'a str>,
    ) -> BrdbSchemaMeta {
        let mut enums: Vec<BrdbSchemaMetaEnum> = Vec::new();
        let mut variants: Vec<BrdbSchemaMetaVariant> = Vec::new();
        let mut structs: Vec<BrdbSchemaMetaStruct> = Vec::new();
        let mut types: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut work: Vec<String> = seeds.into_iter().map(str::to_owned).collect();

        while let Some(name) = work.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if matches!(name.as_str(), DYNAMIC_ARRAY | DYNAMIC_MAP | DYNAMIC_ENUM)
                && types.is_empty()
                && !self.types.is_empty()
            {
                types = self.type_names();
                // LIFO worklist: push in reverse so the table is visited in order.
                work.extend(types.iter().rev().cloned());
                continue;
            }
            if let Some(members) = self.get_variant(&name) {
                let member_names: Vec<String> = members
                    .iter()
                    .filter_map(|m| self.intern.lookup(*m))
                    .collect();
                work.extend(member_names.iter().cloned());
                variants.push((name, member_names));
            } else if let Some(values) = self.get_enum(&name) {
                let vals = values
                    .iter()
                    .filter_map(|(k, v)| self.intern.lookup(*k).map(|k| (k, *v)))
                    .collect();
                enums.push((name, vals));
            } else if let Some(props) = self.get_struct(&name) {
                let mut raw_props = Vec::with_capacity(props.len());
                for (k, v) in props {
                    let key = self.intern.lookup(*k).unwrap();
                    let raw = match v {
                        BrdbSchemaStructProperty::Type(t) => {
                            let n = self.intern.lookup(*t).unwrap();
                            work.push(n.clone());
                            BrdbStructPropRaw::Type(n)
                        }
                        BrdbSchemaStructProperty::Array(t) => {
                            let n = self.intern.lookup(*t).unwrap();
                            work.push(n.clone());
                            BrdbStructPropRaw::Array(n)
                        }
                        BrdbSchemaStructProperty::FlatArray(t) => {
                            let n = self.intern.lookup(*t).unwrap();
                            work.push(n.clone());
                            BrdbStructPropRaw::FlatArray(n)
                        }
                        BrdbSchemaStructProperty::Map(k2, v2) => {
                            let kn = self.intern.lookup(*k2).unwrap();
                            let vn = self.intern.lookup(*v2).unwrap();
                            work.push(kn.clone());
                            work.push(vn.clone());
                            BrdbStructPropRaw::Map(kn, vn)
                        }
                    };
                    raw_props.push((key, raw));
                }
                structs.push((name, raw_props));
            }
            // else: primitive/unknown type — nothing to extract
        }
        (enums, variants, structs, types)
    }

    /// Struct keys in dependency-first order: a struct is emitted only after
    /// every struct it references through its fields. The game's schema
    /// deserializer builds serializers top-to-bottom and requires a referenced
    /// struct to be defined "higher up" than the struct using it, so the
    /// written struct order must be topological. Stable where unconstrained
    /// (preserves insertion order); cycles (shouldn't occur) break arbitrarily.
    ///
    /// `O(structs + references)`. Runs once per schema serialization on a small
    /// struct set, so it's not a hot path. Recursion depth is the longest
    /// dependency chain (a few levels in practice).
    pub(crate) fn topo_struct_order(&self) -> Vec<BrdbInterned> {
        let mut visited = std::collections::HashSet::with_capacity(self.structs.len());
        let mut order = Vec::with_capacity(self.structs.len());
        for &root in self.structs.keys() {
            self.visit_struct_deps(root, &mut visited, &mut order);
        }
        order
    }

    /// Depth-first post-order: append `id`'s struct deps before `id` itself.
    /// The `visited` set makes each struct land once and breaks any cycle.
    fn visit_struct_deps(
        &self,
        id: BrdbInterned,
        visited: &mut std::collections::HashSet<BrdbInterned>,
        order: &mut Vec<BrdbInterned>,
    ) {
        if !visited.insert(id) {
            return;
        }
        if let Some(props) = self.structs.get(&id) {
            for prop in props.values() {
                for dep in prop_type_refs(prop).into_iter().flatten() {
                    if self.structs.contains_key(&dep) {
                        self.visit_struct_deps(dep, visited, order);
                    }
                }
            }
        }
        order.push(id);
    }

    /// Attach global data to the schema
    pub fn with_global_data(mut self, global_data: Arc<BrdbSchemaGlobalData>) -> Self {
        self.global_data = global_data;
        self
    }

    /// Attach global data to the schema
    pub fn set_global_data(&mut self, global_data: Arc<BrdbSchemaGlobalData>) {
        self.global_data = global_data;
    }

    /// Serialize the schema as msgpack in its own format ([`BrdbSchemaFormat::Auto`]).
    pub fn write(&self, buf: impl Write) -> Result<(), BrdbSchemaError> {
        self.write_as(buf, BrdbSchemaFormat::Auto)
    }

    /// The concrete format (`EA3` or `Latest`) a write with `format` uses
    /// for this schema: `Auto` is the schema's own [`Self::format`], or
    /// `Latest` when the schema cannot be expressed as `EA3`.
    pub fn resolve_format(&self, format: BrdbSchemaFormat) -> BrdbSchemaFormat {
        match format {
            BrdbSchemaFormat::Auto if self.needs_map_form() => BrdbSchemaFormat::Latest,
            BrdbSchemaFormat::Auto => self.format(),
            other => other,
        }
    }

    /// Serialize the schema as msgpack in the given format.
    ///
    /// `EA3` is the 3-element `[enums, variants, structs]` array (an empty
    /// variants map is valid); it fails for a schema that
    /// [needs the `Latest` form](Self::needs_map_form). `Latest` is the spec
    /// map, which omits empty sections and writes the legacy
    /// `bundle_path_ref` alias as `str`.
    pub fn write_as(
        &self,
        mut buf: impl Write,
        format: BrdbSchemaFormat,
    ) -> Result<(), BrdbSchemaError> {
        let format = self.resolve_format(format);
        let map_form = format == BrdbSchemaFormat::Latest;
        if !map_form && self.needs_map_form() {
            return Err(BrdbSchemaError::InvalidSchema(
                "the EA3 format cannot express a types table or dynamic containers".to_owned(),
            ));
        }

        let lookup = |interned: BrdbInterned| {
            let name = interned
                .get(self)
                .ok_or(BrdbSchemaError::StringNotInterned(interned.0))?;
            Ok::<_, BrdbSchemaError>(if map_form && name == "bundle_path_ref" {
                "str"
            } else {
                name
            })
        };

        if map_form {
            let sections = [
                !self.enums.is_empty(),
                !self.variants.is_empty(),
                !self.structs.is_empty(),
                !self.types.is_empty(),
            ];
            let count = sections.iter().filter(|s| **s).count();
            rmp::encode::write_map_len(&mut buf, count as u32)?;
        } else {
            rmp::encode::write_array_len(&mut buf, 3)?;
        }

        if !map_form || !self.enums.is_empty() {
            if map_form {
                rmp::encode::write_str(&mut buf, "enums")?;
            }
            rmp::encode::write_map_len(&mut buf, self.enums.len() as u32)?;
            for (enum_name, values) in &self.enums {
                rmp::encode::write_str(&mut buf, lookup(*enum_name)?)?;
                rmp::encode::write_map_len(&mut buf, values.len() as u32)?;
                for (key, value) in values {
                    rmp::encode::write_str(&mut buf, lookup(*key)?)?;
                    write::write_int(&mut buf, *value as i64)?;
                }
            }
        }

        if !map_form || !self.variants.is_empty() {
            if map_form {
                rmp::encode::write_str(&mut buf, "variants")?;
            }
            rmp::encode::write_map_len(&mut buf, self.variants.len() as u32)?;
            for (variant_name, members) in &self.variants {
                rmp::encode::write_str(&mut buf, lookup(*variant_name)?)?;
                rmp::encode::write_array_len(&mut buf, members.len() as u32)?;
                for member in members {
                    rmp::encode::write_str(&mut buf, lookup(*member)?)?;
                }
            }
        }

        if !map_form || !self.structs.is_empty() {
            if map_form {
                rmp::encode::write_str(&mut buf, "structs")?;
            }
            // Structs must be written in dependency-first order (the game
            // requires a referenced struct to appear before the struct that
            // uses it).
            let struct_order = self.topo_struct_order();
            rmp::encode::write_map_len(&mut buf, struct_order.len() as u32)?;
            for struct_name in struct_order {
                let properties = self
                    .structs
                    .get(&struct_name)
                    .expect("topo order only yields known struct keys");
                rmp::encode::write_str(&mut buf, lookup(struct_name)?)?;
                rmp::encode::write_map_len(&mut buf, properties.len() as u32)?;
                for (prop_name, prop_type) in properties {
                    rmp::encode::write_str(&mut buf, lookup(*prop_name)?)?;
                    match prop_type {
                        BrdbSchemaStructProperty::Type(t) => {
                            rmp::encode::write_str(&mut buf, lookup(*t)?)?
                        }
                        BrdbSchemaStructProperty::Array(t) => {
                            rmp::encode::write_array_len(&mut buf, 1)?;
                            rmp::encode::write_str(&mut buf, lookup(*t)?)?;
                        }
                        BrdbSchemaStructProperty::FlatArray(t) => {
                            rmp::encode::write_array_len(&mut buf, 2)?;
                            rmp::encode::write_str(&mut buf, lookup(*t)?)?;
                            rmp::encode::write_nil(&mut buf)?;
                        }
                        BrdbSchemaStructProperty::Map(key_type, value_type) => {
                            rmp::encode::write_map_len(&mut buf, 1)?;
                            rmp::encode::write_str(&mut buf, lookup(*key_type)?)?;
                            rmp::encode::write_str(&mut buf, lookup(*value_type)?)?;
                        }
                    }
                }
            }
        }

        if map_form && !self.types.is_empty() {
            rmp::encode::write_str(&mut buf, "types")?;
            rmp::encode::write_array_len(&mut buf, self.types.len() as u32)?;
            for ty in &self.types {
                rmp::encode::write_str(&mut buf, lookup(*ty)?)?;
            }
        }

        Ok(())
    }

    /// Serialize the schema to a vector of bytes in its own format
    /// ([`BrdbSchemaFormat::Auto`]).
    pub fn to_bytes(&self) -> Result<Vec<u8>, BrdbSchemaError> {
        self.to_bytes_as(BrdbSchemaFormat::Auto)
    }

    /// Serialize the schema to a vector of bytes in the given format.
    pub fn to_bytes_as(&self, format: BrdbSchemaFormat) -> Result<Vec<u8>, BrdbSchemaError> {
        let mut buf = Vec::new();
        self.write_as(&mut buf, format)?;
        Ok(buf)
    }

    pub fn write_brdb(
        &self,
        ty: &str,
        value: &impl AsBrdbValue,
    ) -> Result<Vec<u8>, BrdbSchemaError> {
        let mut buf = Vec::new();
        let mut cache = write::PlanCache::new(self);
        write::write_brdb_cached(self, &mut cache, &mut buf, ty, value)?;
        Ok(buf)
    }
}

/// The type names a struct property references (a map references two).
pub(crate) fn prop_type_refs(prop: &BrdbSchemaStructProperty) -> [Option<BrdbInterned>; 2] {
    match prop {
        BrdbSchemaStructProperty::Type(t)
        | BrdbSchemaStructProperty::Array(t)
        | BrdbSchemaStructProperty::FlatArray(t) => [Some(*t), None],
        BrdbSchemaStructProperty::Map(k, v) => [Some(*k), Some(*v)],
    }
}

/// The element/pair count following an already-read array or map marker.
fn container_len(buf: &mut impl Read, marker: Marker) -> Result<u32, BrdbSchemaError> {
    Ok(match marker {
        Marker::FixArray(n) | Marker::FixMap(n) => n as u32,
        Marker::Array16 | Marker::Map16 => buf.read_data_u16()? as u32,
        Marker::Array32 | Marker::Map32 => buf.read_data_u32()?,
        other => {
            return Err(BrdbSchemaError::InvalidSchema(format!(
                "expected an array or map, found {other:?}"
            )));
        }
    })
}

fn read_enum_section(buf: &mut impl Read) -> Result<Vec<BrdbSchemaMetaEnum>, BrdbSchemaError> {
    let num_enums = rmp::decode::read_map_len(buf)? as usize;
    let mut enums = Vec::with_capacity(num_enums);
    for _ in 0..num_enums {
        let enum_name = read_owned_str(buf)?;
        let value_count = rmp::decode::read_map_len(buf)? as usize;
        let mut values = Vec::with_capacity(value_count);
        for _ in 0..value_count {
            let key = read_owned_str(buf)?;
            let value = read::read_int(buf)? as i32;
            values.push((key, value));
        }
        enums.push((enum_name, values));
    }
    Ok(enums)
}

fn read_variant_section(
    buf: &mut impl Read,
) -> Result<Vec<BrdbSchemaMetaVariant>, BrdbSchemaError> {
    let num_variants = rmp::decode::read_map_len(buf)? as usize;
    let mut variants = Vec::with_capacity(num_variants);
    for _ in 0..num_variants {
        let variant_name = read_owned_str(buf)?;
        let member_count = rmp::decode::read_array_len(buf)? as usize;
        let mut members = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            members.push(read_owned_str(buf)?);
        }
        variants.push((variant_name, members));
    }
    Ok(variants)
}

fn read_types_section(buf: &mut impl Read) -> Result<Vec<String>, BrdbSchemaError> {
    let count = rmp::decode::read_array_len(buf)? as usize;
    let mut types = Vec::with_capacity(count);
    for _ in 0..count {
        let ty = read_owned_str(buf)?;
        if ty == DYNAMIC_ARRAY || ty == DYNAMIC_MAP {
            return Err(BrdbSchemaError::InvalidSchema(format!(
                "'{ty}' cannot appear in the types table"
            )));
        }
        types.push(ty);
    }
    Ok(types)
}

fn read_struct_section(buf: &mut impl Read) -> Result<Vec<BrdbSchemaMetaStruct>, BrdbSchemaError> {
    let num_structs = rmp::decode::read_map_len(buf)? as usize;
    let mut structs = Vec::with_capacity(num_structs);
    for _ in 0..num_structs {
        let struct_name = read_owned_str(buf)?;

        let num_props = rmp::decode::read_map_len(buf)? as usize;
        let mut properties = Vec::with_capacity(num_props);
        for _ in 0..num_props {
            let prop_name = read_owned_str(buf)?;
            let prop_type_marker = rmp::decode::read_marker(buf)
                .map_err(|e| BrdbSchemaError::RmpMarkerReadError(e.0))?;
            let property = match prop_type_marker {
                // Basic types
                Marker::FixStr(size) => {
                    BrdbStructPropRaw::Type(read_str_from_len(buf, size as usize)?)
                }
                Marker::Str8 => {
                    let len = buf.read_data_u8()? as usize;
                    BrdbStructPropRaw::Type(read_str_from_len(buf, len)?)
                }
                Marker::Str16 => {
                    let len = buf.read_data_u16()? as usize;
                    BrdbStructPropRaw::Type(read_str_from_len(buf, len)?)
                }

                Marker::FixArray(len) if len == 0 => {
                    return Err(BrdbSchemaError::InvalidSchema(
                        "0 length FixArray marker not supported".to_string(),
                    ));
                }
                // Array type
                Marker::FixArray(len) if len == 1 => {
                    let array_type = read_owned_str(buf)?;
                    BrdbStructPropRaw::Array(array_type)
                }
                // Flat array has a specific format: [type, nil]
                Marker::FixArray(len) if len == 2 => {
                    let array_type = read_owned_str(buf)?;
                    // Ensure the second element is nil
                    rmp::decode::read_nil(buf).map_err(BrdbSchemaError::RmpValueReadError)?;
                    BrdbStructPropRaw::FlatArray(array_type)
                }

                Marker::FixMap(len) if len != 1 => {
                    return Err(BrdbSchemaError::InvalidSchema(
                        "FixMap with length != 1 is not supported".to_string(),
                    ));
                }
                Marker::FixMap(len) if len == 1 => {
                    let key_type = read_owned_str(buf)?;
                    let value_type = read_owned_str(buf)?;
                    BrdbStructPropRaw::Map(key_type, value_type)
                }
                marker => {
                    return Err(BrdbSchemaError::InvalidSchema(format!(
                        "Unsupported property type marker: {marker:?}",
                    )));
                }
            };

            properties.push((prop_name, property));
        }
        structs.push((struct_name, properties));
    }
    Ok(structs)
}

pub trait ReadBrdbSchema {
    fn read_brdb_schema(&mut self) -> Result<Arc<BrdbSchema>, BrdbSchemaError>;
    fn read_brdb_schema_with_data(
        &mut self,
        data: Arc<BrdbSchemaGlobalData>,
    ) -> Result<Arc<BrdbSchema>, BrdbSchemaError>;
    fn read_brdb(
        &mut self,
        schema: &Arc<BrdbSchema>,
        ty: &str,
    ) -> Result<BrdbValue, BrdbSchemaError>;
}

impl<R: Read> ReadBrdbSchema for R {
    fn read_brdb_schema(&mut self) -> Result<Arc<BrdbSchema>, BrdbSchemaError> {
        BrdbSchema::read(self).map(Arc::new)
    }
    fn read_brdb_schema_with_data(
        &mut self,
        data: Arc<BrdbSchemaGlobalData>,
    ) -> Result<Arc<BrdbSchema>, BrdbSchemaError> {
        BrdbSchema::read(self).map(|schema| Arc::new(schema.with_global_data(data)))
    }
    fn read_brdb(
        &mut self,
        schema: &Arc<BrdbSchema>,
        ty: &str,
    ) -> Result<BrdbValue, BrdbSchemaError> {
        read::read_type(schema, ty, self)
    }
}

#[cfg(test)]
mod schema_tests {
    #[test]
    fn test_plaintext() {
        let input = "enum Foo {
    A = 0,
    B = 1,
}
struct Bar {
    value: i32,
    arr: str[],
    flat_arr: str[flat],
    map: {str: i32},
}
";
        let (enums, _variants, structs, _types) = super::BrdbSchema::parse_to_meta(input).unwrap();

        // When inserting all the enums and structs into a schema it should
        // produce the same displayed output as the input
        let mut schema = super::BrdbSchema::default();
        schema.add_meta(enums.clone(), structs.clone());
        assert_eq!(schema.to_string(), input,);
    }

    #[test]
    fn test_plaintext_variants() {
        // Display order is enums, then variants, then structs.
        let input = "enum Foo {
    A = 0,
}
variant WireGraphVariant {
    f64,
    i64,
    bool,
    Vector,
}
struct Bar {
    value: WireGraphVariant,
}
";
        let schema = super::BrdbSchema::new_parsed(input).unwrap();

        // The variant table should be populated and resolvable by name.
        let members = schema.get_variant("WireGraphVariant").unwrap();
        assert_eq!(members.len(), 4);

        // Parsing then displaying should round-trip exactly.
        assert_eq!(schema.to_string(), input);
    }
}
