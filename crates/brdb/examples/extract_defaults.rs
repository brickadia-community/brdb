use brdb::{AsBrdbValue, Brdb, IntoReader};
use brdb::schema::{BrdbSchema, BrdbSchemaStructProperty, WireVariant};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn format_value(
    schema: &BrdbSchema,
    ty_str: &str,
    val: &dyn AsBrdbValue,
) -> Option<String> {
    let b = |s: String| format!("Box::new({s}) as Box<dyn AsBrdbValue>");
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
        "bundle_path_ref" => Some(b(format!("String::from({:?})", val.as_brdb_str().unwrap_or("")))),
        "wire_graph_variant" | "wire_graph_prim_math_variant" => {
            let wv = match val.as_brdb_wire_variant().ok()? {
                WireVariant::Number(n) => format!("WireVariant::Number({n:?})"),
                WireVariant::Int(n) => format!("WireVariant::Int({n})"),
                WireVariant::Bool(b) => format!("WireVariant::Bool({b})"),
                WireVariant::Object(o) => format!("WireVariant::Object({o:?})"),
                WireVariant::Exec => "WireVariant::Exec".into(),
                WireVariant::Vector(v) => format!(
                    "WireVariant::Vector(Vector3f {{ x: {:?}, y: {:?}, z: {:?} }})",
                    v.x, v.y, v.z
                ),
                WireVariant::Str(s) => format!("WireVariant::Str({s:?}.into())"),
            };
            Some(b(wv))
        }
        "class" | "object" => None, // asset refs are context-dependent
        other => {
            if schema.get_enum(other).is_some() {
                Some(b(format!("{}u8", val.as_brdb_u8().unwrap_or(0))))
            } else if other == "Color" {
                let s_id = schema.intern.get(other)?;
                let get_u8 = |field: &str| -> u8 {
                    let fid = schema.intern.get(field).unwrap();
                    val.as_brdb_struct_prop_value(schema, s_id, fid)
                        .ok()
                        .and_then(|v| v.as_brdb_u8().ok())
                        .unwrap_or(0)
                };
                let r = get_u8("R"); let g = get_u8("G"); let b_ = get_u8("B"); let a = get_u8("A");
                Some(b(format!("SavedBrickColor {{ r: {r}, g: {g}, b: {b_}, a: {a} }}")))
            } else if let Some(s_ty) = schema.get_struct(other) {
                // Generic struct — recurse into fields
                let s_id = schema.intern.get(other)?;
                let mut parts = Vec::new();
                for (field_id, prop_ty) in s_ty {
                    let field_name = field_id.get(schema)?;
                    let inner_ty = match prop_ty {
                        BrdbSchemaStructProperty::Type(t) => schema.intern.lookup_ref(*t)?,
                        _ => continue,
                    };
                    let prop_val = val.as_brdb_struct_prop_value(schema, s_id, *field_id).ok()?;
                    let formatted = format_value(schema, &inner_ty, prop_val)?;
                    parts.push(format!("(\"{field_name}\", {formatted})"));
                }
                // Can't easily box a nested struct — skip
                None
            } else {
                None
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(args.get(1).expect("usage: extract_defaults <dump.brdb> [output.rs]"));
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

    for chunk in db.brick_chunk_index(1)? {
        if chunk.num_components == 0 {
            continue;
        }
        let Ok((_soa, components)) = db.component_chunk(1, *chunk) else {
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
                    BrdbSchemaStructProperty::Type(t) => {
                        match schema.intern.lookup_ref(*t) {
                            Some(s) => s.to_owned(),
                            None => continue,
                        }
                    }
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
    // WireVariant is only referenced when a dump carries wire-variant defaults;
    // allow it to be unused so a dump without any still compiles clean.
    w!("#[allow(unused_imports)]");
    w!("use crate::schema::WireVariant;");
    w!("use crate::schema::as_brdb::AsBrdbValue;");
    w!("use crate::SavedBrickColor;");
    w!();
    w!("/// Default field values for every component data struct.");
    w!("pub static STRUCT_DEFAULTS: LazyLock<Vec<(&'static str, Vec<(&'static str, Box<dyn AsBrdbValue>)>)>> =");
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
