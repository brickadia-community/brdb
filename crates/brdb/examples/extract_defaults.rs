use brdb::schema::{BrdbSchema, BrdbSchemaStructProperty, BrdbValue, EA3WireVariant, WireValue};
use brdb::{AsBrdbValue, Brdb, IntoReader};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Upper bound for the sparse grid-id probe (grid 1 = main, microchip planes are higher).
const MAX_GRID_ID: usize = 512;

/// An EA3 wire graph value as a Rust literal.
fn ea3_wire_literal(wv: EA3WireVariant) -> String {
    match wv {
        EA3WireVariant::Number(n) => format!("EA3WireVariant::Number({n:?})"),
        EA3WireVariant::Int(n) => format!("EA3WireVariant::Int({n})"),
        EA3WireVariant::Bool(b) => format!("EA3WireVariant::Bool({b})"),
        EA3WireVariant::Object(o) => format!("EA3WireVariant::Object({o:?})"),
        EA3WireVariant::Exec => "EA3WireVariant::Exec".into(),
        EA3WireVariant::Vector(v) => format!(
            "EA3WireVariant::Vector(Vector3f {{ x: {:?}, y: {:?}, z: {:?} }})",
            v.x, v.y, v.z
        ),
        EA3WireVariant::Str(s) => format!("EA3WireVariant::Str({s:?}.into())"),
        EA3WireVariant::Rotator { pitch, yaw, roll } => {
            format!("EA3WireVariant::Rotator {{ pitch: {pitch:?}, yaw: {yaw:?}, roll: {roll:?} }}")
        }
        EA3WireVariant::Quat { x, y, z, w } => {
            format!("EA3WireVariant::Quat {{ x: {x:?}, y: {y:?}, z: {z:?}, w: {w:?} }}")
        }
        EA3WireVariant::LinearColor { r, g, b, a } => format!(
            "EA3WireVariant::LinearColor {{ r: {r:?}f32, g: {g:?}f32, b: {b:?}f32, a: {a:?}f32 }}"
        ),
    }
}

/// A `WireValue` as a Rust literal; `None` for brick/component references,
/// whose default is the zero value written for an unset field anyway.
fn wire_value_literal(wv: WireValue) -> Option<String> {
    Some(match wv {
        WireValue::Bool(b) => format!("WireValue::Bool({b})"),
        WireValue::Int(n) => format!("WireValue::Int({n})"),
        WireValue::Number(n) => format!("WireValue::Number({n:?})"),
        WireValue::LinearColor { r, g, b, a } => format!(
            "WireValue::LinearColor {{ r: {r:?}f32, g: {g:?}f32, b: {b:?}f32, a: {a:?}f32 }}"
        ),
        WireValue::Vector { x, y, z } => {
            format!("WireValue::Vector {{ x: {x:?}, y: {y:?}, z: {z:?} }}")
        }
        WireValue::Rotator { pitch, yaw, roll } => {
            format!("WireValue::Rotator {{ pitch: {pitch:?}, yaw: {yaw:?}, roll: {roll:?} }}")
        }
        WireValue::Quat { x, y, z, w } => {
            format!("WireValue::Quat {{ x: {x:?}, y: {y:?}, z: {z:?}, w: {w:?} }}")
        }
        WireValue::Str(s) => format!("WireValue::Str({s:?}.into())"),
        WireValue::Enum(e) => format!(
            "WireValue::Enum(BrdbDynamicEnum::new({:?}, {}))",
            e.enum_name, e.value
        ),
        WireValue::Object => "WireValue::Object".into(),
        WireValue::Exec => "WireValue::Exec".into(),
        WireValue::BrickRef(_) | WireValue::ComponentRef(_) => return None,
    })
}

fn format_value(schema: &BrdbSchema, ty_str: &str, val: &dyn AsBrdbValue) -> Option<String> {
    let b = |s: String| format!("Box::new({s}) as Box<dyn AsBrdbValue>");
    // Types the Latest format writes without declaring them.
    let ty_str = match ty_str {
        "Bool" if schema.get_struct(ty_str).is_none() => "bool",
        "Str" if schema.get_struct(ty_str).is_none() => "str",
        other => other,
    };
    match ty_str {
        "bool" => Some(b(format!("{}", val.as_brdb_bool().ok()?))),
        "u8" => Some(b(format!("{}u8", val.as_brdb_u8().ok()?))),
        "u16" => Some(b(format!("{}u16", val.as_brdb_u16().ok()?))),
        "u32" => Some(b(format!("{}u32", val.as_brdb_u32().ok()?))),
        "u64" => Some(b(format!("{}u64", val.as_brdb_u64().ok()?))),
        "i8" => Some(b(format!("{}i8", val.as_brdb_i8().ok()?))),
        "i16" => Some(b(format!("{}i16", val.as_brdb_i16().ok()?))),
        "i32" => Some(b(format!("{}i32", val.as_brdb_i32().ok()?))),
        "i64" => Some(b(format!("{}i64", val.as_brdb_i64().ok()?))),
        "f32" => Some(b(format!("{:?}f32", val.as_brdb_f32().ok()?))),
        "f64" => Some(b(format!("{:?}f64", val.as_brdb_f64().ok()?))),
        "str" => Some(b(format!("String::from({:?})", val.as_brdb_str().ok()?))),
        "bundle_path_ref" => Some(b(format!(
            "String::from({:?})",
            val.as_brdb_str().unwrap_or("")
        ))),
        "wire_graph_variant" | "wire_graph_prim_math_variant" => {
            Some(b(ea3_wire_literal(val.as_brdb_ea3_wire_variant().ok()?)))
        }
        "class" | "object" | "weak_object" => None, // asset refs are context-dependent
        "dynamic_array" => {
            // Only empty containers have a literal form here.
            let (element, mut items) = val.as_brdb_dynamic_array().ok()?;
            items
                .next()
                .is_none()
                .then(|| b(format!("BrdbDynamicArray::empty({element:?})")))
        }
        "dynamic_map" => {
            let (key, value, mut entries) = val.as_brdb_dynamic_map().ok()?;
            entries
                .next()
                .is_none()
                .then(|| b(format!("BrdbDynamicMap::empty({key:?}, {value:?})")))
        }
        "dynamic_enum" => {
            let (name, value) = val.as_brdb_dynamic_enum().ok()?;
            Some(b(format!("BrdbDynamicEnum::new({name:?}, {value})")))
        }
        other => {
            if schema.get_variant(other).is_some() {
                // A decoded variant is the raw value of its alternative.
                let raw = val.as_brdb_value()?;
                if schema.is_latest() {
                    Some(b(wire_value_literal(WireValue::try_from(raw).ok()?)?))
                } else {
                    Some(b(ea3_wire_literal(EA3WireVariant::try_from(raw).ok()?)))
                }
            } else if schema.get_enum(other).is_some() {
                let value = match val.as_brdb_value() {
                    Some(BrdbValue::Enum(e)) => e.value,
                    _ => val.as_brdb_u8().unwrap_or(0) as u64,
                };
                Some(b(format!("{value}u8")))
            } else if other == "Color" {
                let s_id = schema.intern.get(other)?;
                let get_u8 = |field: &str| -> u8 {
                    let fid = schema.intern.get(field).unwrap();
                    val.as_brdb_struct_prop_value(schema, s_id, fid)
                        .ok()
                        .and_then(|v| v.as_brdb_u8().ok())
                        .unwrap_or(0)
                };
                let r = get_u8("R");
                let g = get_u8("G");
                let b_ = get_u8("B");
                let a = get_u8("A");
                Some(b(format!(
                    "SavedBrickColor {{ r: {r}, g: {g}, b: {b_}, a: {a} }}"
                )))
            } else if let Some(s_ty) = schema.get_struct(other) {
                // Generic struct — recurse into fields and emit a NestedStructDefault so nested
                // struct values (Vector2D {X,Y}, LinearColor {R,G,B,A}, ...) survive into
                // STRUCT_DEFAULTS instead of being written as zeros for unset fields.
                let s_id = schema.intern.get(other)?;
                let mut parts = Vec::new();
                for (field_id, prop_ty) in s_ty {
                    let field_name = field_id.get(schema)?;
                    let inner_ty = match prop_ty {
                        BrdbSchemaStructProperty::Type(t) => schema.intern.lookup_ref(*t)?,
                        _ => continue,
                    };
                    let prop_val = val
                        .as_brdb_struct_prop_value(schema, s_id, *field_id)
                        .ok()?;
                    // Skip a sub-field we can't format (e.g. an object/class ref) but keep the
                    // rest — a missing field falls back to zero at write time.
                    let Some(formatted) = format_value(schema, &inner_ty, prop_val) else {
                        continue;
                    };
                    // First field keeps the `as Box<dyn AsBrdbValue>` cast to fix the vec's
                    // element type; the rest coerce to it.
                    if parts.is_empty() {
                        parts.push(format!("(\"{field_name}\", {formatted})"));
                    } else {
                        let short = formatted.trim_end_matches(" as Box<dyn AsBrdbValue>");
                        parts.push(format!("(\"{field_name}\", {short})"));
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(b(format!(
                        "NestedStructDefault(vec![{}])",
                        parts.join(", ")
                    )))
                }
            } else {
                None
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(
        args.get(1)
            .expect("usage: extract_defaults <dump.brdb> [output.rs]"),
    );
    let output_path = args.get(2).map(PathBuf::from);
    let db = Brdb::open(path)?.into_reader();
    let global_data = db.global_data()?;
    let schema = db.components_schema()?;

    let mut type_to_struct: BTreeMap<String, String> = BTreeMap::new();
    let mut wire_ports: BTreeSet<String> = BTreeSet::new();
    let mut struct_defaults: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();

    for (i, type_name) in global_data.component_type_names.iter().enumerate() {
        if let Some(struct_name) = global_data.component_data_struct_names.get(i) {
            if struct_name != "None" {
                type_to_struct.insert(type_name.clone(), struct_name.clone());
            }
        }
    }

    for port in &global_data.component_wire_port_names {
        wire_ports.insert(port.clone());
    }

    // Grid 1 is the main grid; microchip planes are separate grids whose ids are
    // sparse and can start well above 1 (a zoo save had 1 then 48..=71), so probe a
    // wide range and skip gaps instead of stopping at the first missing id. Reading
    // only grid 1 loses every plane-resident component (MicrochipInput, the
    // CallableEntry_* gates, FunctionInput) -- they place only inside a plane.
    for gid in 1..MAX_GRID_ID {
        let Ok(chunks) = db.brick_chunk_index(gid) else {
            continue;
        };
        for chunk in chunks {
            if chunk.num_components == 0 {
                continue;
            }
            let Ok((_soa, components)) = db.component_chunk(gid, *chunk) else {
                continue;
            };
            for s in components {
                let name = s.get_name().to_owned();
                if struct_defaults.contains_key(&name) {
                    continue;
                }

                let Some(struct_def) = schema.get_struct(&name) else {
                    continue;
                };
                let s_id = match schema.intern.get(&name) {
                    Some(id) => id,
                    None => continue,
                };

                let mut fields = Vec::new();
                for (field_id, prop_ty) in struct_def {
                    let field_name = match field_id.get(&schema) {
                        Some(n) => n.to_owned(),
                        None => continue,
                    };
                    let ty_str = match prop_ty {
                        BrdbSchemaStructProperty::Type(t) => match schema.intern.lookup_ref(*t) {
                            Some(s) => s.to_owned(),
                            None => continue,
                        },
                        _ => continue,
                    };
                    let val = match s.as_brdb_struct_prop_value(&schema, s_id, *field_id) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let formatted = match format_value(&schema, &ty_str, val) {
                        Some(f) => f,
                        None => continue,
                    };
                    fields.push((field_name, ty_str, formatted));
                }
                struct_defaults.insert(name, fields);
            }
        }
    }

    use std::fmt::Write;
    let mut out = String::new();
    macro_rules! w { ($($t:tt)*) => { writeln!(out, $($t)*).unwrap() } }

    w!("// Autogenerated from: cargo run --example extract_defaults -- path/to/dump.brdb");
    w!();

    w!("pub static COMPONENT_TYPE_STRUCT_PAIRS: &[(&str, &str)] = &[");
    for (type_name, struct_name) in &type_to_struct {
        w!("    (\"{type_name}\", \"{struct_name}\"),");
    }
    w!("];");
    w!();

    w!("pub static WIRE_PORT_NAMES: &[&str] = &[");
    for port in &wire_ports {
        w!("    \"{port}\",");
    }
    w!("];");
    w!();

    w!("pub static ENTITY_TYPE_STRUCT_PAIRS: &[(&str, &str)] = &[");
    for (i, type_name) in global_data.entity_type_names.iter().enumerate() {
        if let Some(class_name) = global_data.entity_data_class_names.get_index(i) {
            w!("    (\"{type_name}\", \"{class_name}\"),");
        }
    }
    w!("];");
    w!();

    w!("use std::sync::LazyLock;");
    // EA3WireVariant is only referenced when a dump carries wire-variant defaults;
    // allow it to be unused so a dump without any still compiles clean.
    w!("#[allow(unused_imports)]");
    if schema.is_latest() {
        w!("use crate::schema::{{BrdbDynamicArray, BrdbDynamicEnum, BrdbDynamicMap, WireValue}};");
    } else {
        w!("use crate::schema::EA3WireVariant;");
    }
    w!("use crate::schema::as_brdb::AsBrdbValue;");
    // NestedStructDefault carries struct-typed defaults (Vector2D/LinearColor/...); allow it
    // to be unused so a dump without any nested-struct defaults still compiles clean.
    w!("#[allow(unused_imports)]");
    w!("use crate::schema::as_brdb::NestedStructDefault;");
    w!("use crate::SavedBrickColor;");
    w!();
    w!("/// Default field values for every component data struct.");
    w!(
        "pub static STRUCT_DEFAULTS: LazyLock<Vec<(&'static str, Vec<(&'static str, Box<dyn AsBrdbValue>)>)>> ="
    );
    w!("    LazyLock::new(|| vec![");
    let mut first_entry = true;
    for (name, fields) in &struct_defaults {
        if fields.is_empty() {
            continue;
        }
        w!("        (\"{name}\", vec![");
        for (field_name, _ty, val) in fields {
            if first_entry {
                w!("            (\"{field_name}\", {val}),");
                first_entry = false;
            } else {
                let short = val.trim_end_matches(" as Box<dyn AsBrdbValue>");
                w!("            (\"{field_name}\", {short}),");
            }
        }
        w!("        ]),");
    }
    w!("    ]);");

    if let Some(ref p) = output_path {
        std::fs::write(p, &out)?;
        eprintln!("Wrote {}", p.display());
    } else {
        print!("{out}");
    }

    eprintln!(
        "Extracted: {} type mappings, {} wire ports, {} struct defaults, {} entity types",
        type_to_struct.len(),
        wire_ports.len(),
        struct_defaults.len(),
        global_data.entity_type_names.len(),
    );

    Ok(())
}
