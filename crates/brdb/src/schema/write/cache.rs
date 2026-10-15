use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::schema::{
    BrdbInterned, BrdbSchema, BrdbSchemaEnum, BrdbSchemaStructProperty, DYNAMIC_ARRAY, DYNAMIC_MAP,
    EA3_WIRE_GRAPH_PRIM_MATH_VARIANT, EA3_WIRE_GRAPH_VARIANT,
    as_brdb::AsBrdbValue,
    builtin_alias,
    read::{FlatKind, flat_layout, flat_type_size},
};

/// A compiled per-type "write plan": one schema struct's field list with
/// types, variant member tags, enum tables and unset-field default bytes
/// resolved once, then executed per instance. The emitted bytes are identical
/// to the unplanned `write_brdb` path; anything that cannot be pre-resolved
/// becomes a `PlanOp::Fallback` that runs the unplanned writer.
pub(crate) struct StructPlan<'a> {
    pub(crate) s_id: BrdbInterned,
    pub(crate) name: &'a str,
    pub(crate) fields: Vec<FieldPlan<'a>>,
    /// The full byte stream for an instance with no set fields (every field
    /// takes its default/zero or empty-container path), precomputed by
    /// executing the plan against an empty value. `None` when any field's
    /// unset path is not a constant.
    pub(crate) all_default_bytes: Option<Box<[u8]>>,
}

/// A value with no set struct properties; used to precompute
/// `all_default_bytes` through the exact per-field write path.
struct EmptyProps;

impl AsBrdbValue for EmptyProps {
    fn has_brdb_struct_prop(
        &self,
        _schema: &BrdbSchema,
        _struct_name: BrdbInterned,
        _prop_name: BrdbInterned,
    ) -> bool {
        false
    }
}

pub(crate) struct FieldPlan<'a> {
    pub(crate) prop_id: BrdbInterned,
    pub(crate) prop_name: &'a str,
    pub(crate) kind: FieldKind<'a>,
}

pub(crate) enum FieldKind<'a> {
    Scalar {
        op: PlanOp<'a>,
        ty_str: &'a str,
        /// Bytes emitted when the field is unset, precomputed with
        /// `write_struct_field_default`. `None`: the precompute failed and
        /// the unplanned default path runs instead.
        default_bytes: Option<Box<[u8]>>,
    },
    Array {
        elem: PlanOp<'a>,
    },
    FlatArray {
        elem: FlatOp<'a>,
        elem_size: usize,
    },
    Map {
        key: PlanOp<'a>,
        value: PlanOp<'a>,
    },
}

#[derive(Clone)]
pub(crate) enum PlanOp<'a> {
    Bool,
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
    Str,
    /// `nil`: no payload.
    Nil,
    EA3WireGraphVariant,
    EA3WireGraphPrimMathVariant,
    BundlePathRef,
    Asset(&'a str),
    Enum {
        name: &'a str,
        def: &'a BrdbSchemaEnum,
    },
    Struct(Rc<StructPlan<'a>>),
    /// A struct referenced while its own plan was still being built
    /// (self-reference through an array/map field); resolved through the
    /// cache at write time.
    Deferred(BrdbInterned, &'a str),
    /// Named variants resolve lazily: a member struct may reference the
    /// variant again, so eager planning could recurse forever.
    Variant(BrdbInterned, &'a str),
    /// Not plannable; the unplanned writer runs (and errors) as before.
    Fallback(&'a str),
}

#[derive(Clone)]
pub(crate) enum FlatOp<'a> {
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
    Struct(Rc<FlatStructPlan<'a>>),
    Fallback(&'a str),
}

pub(crate) struct FlatStructPlan<'a> {
    pub(crate) s_id: BrdbInterned,
    /// Field, its op, and the zero padding written before it.
    pub(crate) props: Vec<(BrdbInterned, FlatOp<'a>, usize)>,
    /// Zero padding after the last field, up to the struct size.
    pub(crate) tail_pad: usize,
}

/// Tags of the EA3 wire-variant members within one named variant type, resolved
/// once per variant. `None` = the variant has no such member; the unplanned
/// writer's `ExpectedType` error is reproduced at write time.
pub(crate) struct EA3WireTags {
    pub(crate) number: Option<u64>,
    pub(crate) int: Option<u64>,
    pub(crate) boolean: Option<u64>,
    pub(crate) object: Option<u64>,
    pub(crate) exec: Option<u64>,
    pub(crate) vector: Option<u64>,
    pub(crate) rotator: Option<u64>,
    pub(crate) quat: Option<u64>,
    pub(crate) string: Option<u64>,
    pub(crate) linear_color: Option<u64>,
}

pub(crate) struct VariantPlan<'a> {
    pub(crate) name: &'a str,
    /// Ordered members (tag = index): member type name (`None` when the
    /// interned id cannot be resolved) and its write op.
    pub(crate) members: Vec<(Option<&'a str>, PlanOp<'a>)>,
    pub(crate) ea3_wire: EA3WireTags,
}

pub(crate) struct PlanCache<'a> {
    schema: &'a BrdbSchema,
    structs: FxHashMap<BrdbInterned, Option<Rc<StructPlan<'a>>>>,
    variants: FxHashMap<BrdbInterned, Rc<VariantPlan<'a>>>,
    building: FxHashSet<BrdbInterned>,
}

impl<'a> PlanCache<'a> {
    pub(crate) fn new(schema: &'a BrdbSchema) -> Self {
        Self {
            schema,
            structs: FxHashMap::default(),
            variants: FxHashMap::default(),
            building: FxHashSet::default(),
        }
    }

    /// Resolve a type name to a write op, mirroring `write_brdb`'s dispatch
    /// order exactly: primitives and special names first, then variant,
    /// struct, enum; anything else is a fallback.
    fn resolve_op(&mut self, ty: &'a str) -> PlanOp<'a> {
        match ty {
            "bool" => PlanOp::Bool,
            "u8" => PlanOp::U8,
            "u16" => PlanOp::U16,
            "u32" => PlanOp::U32,
            "u64" => PlanOp::U64,
            "i8" => PlanOp::I8,
            "i16" => PlanOp::I16,
            "i32" => PlanOp::I32,
            "i64" => PlanOp::I64,
            "f32" => PlanOp::F32,
            "f64" => PlanOp::F64,
            "str" => PlanOp::Str,
            "nil" => PlanOp::Nil,
            EA3_WIRE_GRAPH_VARIANT => PlanOp::EA3WireGraphVariant,
            EA3_WIRE_GRAPH_PRIM_MATH_VARIANT => PlanOp::EA3WireGraphPrimMathVariant,
            "bundle_path_ref" => PlanOp::BundlePathRef,
            "class" | "object" | "weak_object" => PlanOp::Asset(ty),
            DYNAMIC_ARRAY | DYNAMIC_MAP => PlanOp::Fallback(ty),
            other => match self.schema.intern.get(other) {
                Some(id) => self.resolve_named(id, other),
                None => PlanOp::Fallback(other),
            },
        }
    }

    pub(crate) fn resolve_named(&mut self, id: BrdbInterned, name: &'a str) -> PlanOp<'a> {
        let schema = self.schema;
        if schema.get_variant_interned(id).is_some() {
            PlanOp::Variant(id, name)
        } else if schema.get_struct_interned(id).is_some() {
            if self.building.contains(&id) {
                return PlanOp::Deferred(id, name);
            }
            match self.struct_plan(id, name) {
                Some(plan) => PlanOp::Struct(plan),
                None => PlanOp::Fallback(name),
            }
        } else if let Some(def) = schema.get_enum_interned(id) {
            PlanOp::Enum { name, def }
        } else if let Some(alias) = builtin_alias(name) {
            self.resolve_op(alias)
        } else {
            PlanOp::Fallback(name)
        }
    }

    /// Build (or fetch) the plan for a schema struct. `None`: unplannable
    /// (an interned name failed to resolve); callers fall back to the
    /// unplanned writer for the whole struct.
    pub(crate) fn struct_plan(
        &mut self,
        s_id: BrdbInterned,
        name: &'a str,
    ) -> Option<Rc<StructPlan<'a>>> {
        if let Some(cached) = self.structs.get(&s_id) {
            return cached.clone();
        }
        if self.building.contains(&s_id) {
            // In-flight self-reference reached during a plan's own
            // default-bytes precompute; the caller falls back.
            return None;
        }
        self.building.insert(s_id);
        let built = self.build_struct_plan(s_id, name);
        self.building.remove(&s_id);
        self.structs.insert(s_id, built.clone());
        built
    }

    fn build_struct_plan(
        &mut self,
        s_id: BrdbInterned,
        name: &'a str,
    ) -> Option<Rc<StructPlan<'a>>> {
        let schema = self.schema;
        let s_ty = schema.get_struct_interned(s_id)?;
        let mut fields = Vec::with_capacity(s_ty.len());
        for (prop_id, prop_schema) in s_ty {
            let prop_name = schema.intern.lookup_ref(*prop_id)?;
            let kind = match prop_schema {
                BrdbSchemaStructProperty::Type(ty_id) => {
                    let ty_str = schema.intern.lookup_ref(*ty_id)?;
                    let op = self.resolve_op(ty_str);
                    let mut dbuf = Vec::new();
                    let default_bytes = super::write_struct_field_default(
                        schema, &mut dbuf, name, prop_name, ty_str,
                    )
                    .ok()
                    .map(|_| dbuf.into_boxed_slice());
                    FieldKind::Scalar {
                        op,
                        ty_str,
                        default_bytes,
                    }
                }
                BrdbSchemaStructProperty::Array(ty_id) => {
                    let ty_str = schema.intern.lookup_ref(*ty_id)?;
                    FieldKind::Array {
                        elem: self.resolve_op(ty_str),
                    }
                }
                BrdbSchemaStructProperty::FlatArray(ty_id) => {
                    let ty_str = schema.intern.lookup_ref(*ty_id)?;
                    FieldKind::FlatArray {
                        elem: self.resolve_flat_op(ty_str),
                        elem_size: flat_type_size(schema, ty_str),
                    }
                }
                BrdbSchemaStructProperty::Map(k_ty_id, v_ty_id) => {
                    let k_ty = schema.intern.lookup_ref(*k_ty_id)?;
                    let v_ty = schema.intern.lookup_ref(*v_ty_id)?;
                    FieldKind::Map {
                        key: self.resolve_op(k_ty),
                        value: self.resolve_op(v_ty),
                    }
                }
            };
            fields.push(FieldPlan {
                prop_id: *prop_id,
                prop_name,
                kind,
            });
        }
        let mut plan = StructPlan {
            s_id,
            name,
            fields,
            all_default_bytes: None,
        };
        let mut dbuf = Vec::new();
        if super::write_struct_plan(schema, self, &mut dbuf, &plan, &EmptyProps).is_ok() {
            plan.all_default_bytes = Some(dbuf.into_boxed_slice());
        }
        Some(Rc::new(plan))
    }

    fn resolve_flat_op(&mut self, ty: &'a str) -> FlatOp<'a> {
        match ty {
            "u8" => FlatOp::U8,
            "u16" => FlatOp::U16,
            "u32" => FlatOp::U32,
            "u64" => FlatOp::U64,
            "i8" => FlatOp::I8,
            "i16" => FlatOp::I16,
            "i32" => FlatOp::I32,
            "i64" => FlatOp::I64,
            "f32" => FlatOp::F32,
            "f64" => FlatOp::F64,
            other => match self.flat_struct_plan(other) {
                Some(plan) => FlatOp::Struct(Rc::new(plan)),
                None => FlatOp::Fallback(other),
            },
        }
    }

    /// `None`: not a plannable flat struct (unknown type, a non-`Type`
    /// property, or an unresolvable name); the caller falls back to
    /// `write_brdb_flat`, which reproduces the mid-stream error behavior.
    fn flat_struct_plan(&mut self, name: &'a str) -> Option<FlatStructPlan<'a>> {
        let schema = self.schema;
        let s_id = schema.intern.get(name)?;
        let s_ty = schema.get_struct_interned(s_id)?;
        let layout = flat_layout(schema, name)?;
        let FlatKind::Struct(_, fields) = &layout.kind else {
            return None;
        };
        let mut props = Vec::with_capacity(s_ty.len());
        let mut pos = 0;
        for ((prop_id, prop_schema), (_, offset, field)) in s_ty.iter().zip(fields) {
            let BrdbSchemaStructProperty::Type(prop_ty_id) = prop_schema else {
                return None;
            };
            let prop_ty = schema.intern.lookup_ref(*prop_ty_id)?;
            props.push((*prop_id, self.resolve_flat_op(prop_ty), offset - pos));
            pos = offset + field.size;
        }
        Some(FlatStructPlan {
            s_id,
            props,
            tail_pad: layout.size - pos,
        })
    }

    pub(crate) fn variant_plan(
        &mut self,
        v_id: BrdbInterned,
        name: &'a str,
    ) -> Rc<VariantPlan<'a>> {
        if let Some(plan) = self.variants.get(&v_id) {
            return plan.clone();
        }
        let schema = self.schema;
        let member_ids = schema
            .get_variant_interned(v_id)
            .cloned()
            .unwrap_or_default();
        let mut members = Vec::with_capacity(member_ids.len());
        for m_id in &member_ids {
            match schema.intern.lookup_ref(*m_id) {
                Some(m_name) => {
                    let op = self.resolve_op(m_name);
                    members.push((Some(m_name), op));
                }
                None => members.push((None, PlanOp::Fallback(name))),
            }
        }
        let find = |n: &str| {
            members
                .iter()
                .position(|(m, _)| *m == Some(n))
                .map(|i| i as u64)
        };
        let ea3_wire = EA3WireTags {
            number: find("f64"),
            int: find("i64"),
            boolean: find("bool").or_else(|| find("Bool")),
            object: find("weak_object"),
            exec: find("WireGraphExec"),
            vector: find("Vector"),
            rotator: find("Rotator"),
            quat: find("Quat"),
            string: find("str").or_else(|| find("Str")),
            linear_color: find("LinearColor"),
        };
        let plan = Rc::new(VariantPlan {
            name,
            members,
            ea3_wire,
        });
        self.variants.insert(v_id, plan.clone());
        plan
    }
}

/// One-entry memo for consecutive `write_brdb_cached` calls with the same
/// type-name reference, keyed by the `&str`'s address and length. Sound only
/// when the caller's type-name strings live in storage that outlives the memo
/// (e.g. names borrowed from the schema's global data), so an address is
/// never reused for different content.
#[derive(Default)]
pub(crate) struct TopLevelMemo<'a> {
    pub(crate) last: Option<(*const u8, usize, PlanOp<'a>)>,
}
