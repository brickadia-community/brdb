use super::*;
use crate::schema::{
    BrdbDynamicArray, BrdbDynamicMap, BrdbEnum, BrdbSchemaFormat, BrdbStruct, read::read_type,
};
use indexmap::IndexMap;

/// A small EA3 component schema with every reshaped wire graph form.
const EA3_SCHEMA: &str = r#"
    enum EBrickAxis { EBrickAxis::X = 0, EBrickAxis::Y = 1, EBrickAxis::Z = 2 }
    variant WireGraphEnumVariant { EBrickAxis }
    struct WireGraphEnumWrapper { Enum: WireGraphEnumVariant }
    variant WireGraphVariant { f64, i64, bool, WireGraphEnumWrapper }
    struct WireGraphDoubleArray { Values: f64[] }
    struct WireGraphStringArray { Values: str[] }
    variant WireGraphArrayVariant { WireGraphDoubleArray, WireGraphStringArray }
    struct WireGraphMap_int64_double { Values: {i64: f64} }
    variant WireGraphMapVariant_int64 { WireGraphMap_int64_double }
    struct WireGraphMapKeyWrapper_int64 { Map: WireGraphMapVariant_int64 }
    variant WireGraphMapVariant { WireGraphMapKeyWrapper_int64 }
    struct BrickComponentData_WireGraphPseudo_Var { Value: WireGraphVariant }
    struct BrickComponentData_WireGraphPseudo_ArrayVar { Value: WireGraphArrayVariant }
    struct BrickComponentData_WireGraphPseudo_MapVar { Value: WireGraphMapVariant }
"#;

fn ea3_schema() -> Arc<BrdbSchema> {
    let mut schema = BrdbSchema::new_parsed(EA3_SCHEMA).unwrap();
    schema.set_format(BrdbSchemaFormat::EA3);
    Arc::new(schema)
}

fn struct_value(schema: &Arc<BrdbSchema>, name: &str, fields: Vec<(&str, BrdbValue)>) -> BrdbValue {
    BrdbValue::Struct(Box::new(BrdbStruct {
        schema: schema.clone(),
        name: schema.intern.get(name).unwrap(),
        properties: fields
            .into_iter()
            .map(|(k, v)| (schema.intern.get(k).unwrap(), v))
            .collect(),
    }))
}

/// Encode `value` as EA3 `ea3_ty`, decode it, migrate it, encode it as
/// current `current_ty` and decode the result.
fn migrate_through(ea3_ty: &str, value: BrdbValue, current_ty: &str) -> BrdbValue {
    let ea3 = ea3_schema();
    let bytes = ea3.write_brdb(ea3_ty, &value).unwrap();
    let decoded = read_type(&ea3, ea3_ty, &mut bytes.as_slice()).unwrap();
    let migrated = migrate_value(&decoded).unwrap();
    let current = Arc::new(schemas::bricks_components_schema_max().clone());
    let bytes = current.write_brdb(current_ty, &migrated).unwrap();
    let mut rest = bytes.as_slice();
    let out = read_type(&current, current_ty, &mut rest).unwrap();
    assert!(rest.is_empty());
    out
}

/// Two synthetic steps: `old` renames types by prefix and bricks, `new`
/// renames one of the results again and renames a field of its data.
const CHAINED_STEPS: &str = r#"{ "steps": [
    {
        "name": "old",
        "description": "",
        "detect": { "componentSchemaFormat": "EA3" },
        "componentTypes": { "prefixes": [["Old_", "Mid_"]] },
        "bricks": { "renames": { "B_A": "B_B" }, "toGate": { "B_Gate": "PB_Gate_QueueTicks" } }
    },
    {
        "name": "new",
        "description": "",
        "detect": { "componentSchemaHasStruct": "OnlyBeforeNew" },
        "componentTypes": { "renames": { "Mid_Thing": "New_Thing" } },
        "bricks": { "renames": { "B_B": "B_C" } },
        "fields": [
            { "component": "New_Thing", "from": "Value", "to": "Amount" },
            { "component": "New_Thing", "from": "Amount", "to": "Total" }
        ]
    }
] }"#;

#[test]
fn the_shared_steps_file_parses() {
    let all = steps();
    assert_eq!(all[0].name, "ea3");
    for step in all {
        for to in step.component_types.renames.values() {
            assert!(current_struct(to).is_some(), "{to} is not a current component type");
        }
        for to in step.bricks.to_gate.values() {
            assert!(procedural_gate(to).is_some(), "{to} is not a procedural gate");
        }
    }
}

#[test]
fn component_types_follow_the_prefix_rule_and_exceptions() {
    assert_eq!(
        component_type("BrickComponentType_WireGraph_Expr_LogicalAND"),
        "BrickComponentType_Expr_LogicalAND"
    );
    assert_eq!(
        component_type("BrickComponentType_WireGraphPseudo_BufferTicks"),
        "BrickComponentType_BufferTicks"
    );
    assert_eq!(
        component_type("BrickComponentType_WireGraphPseudo_Timer"),
        "BrickComponentType_Stopwatch"
    );
    assert_eq!(
        component_type("BrickComponentType_WireGraph_Fake_Gamemode_RoundEndEvent"),
        "BrickComponentType_Event_Gamemode_RoundEnd"
    );
    assert_eq!(component_type("Component_Internal_Seat"), "Component_Internal_Seat");
}

#[test]
fn removed_gate_bricks_become_their_largest_procedural_gate() {
    assert_eq!(
        brick_asset("B_1x1_Gate_Pseudo_QueueTicks"),
        Some(BrickMigration::Procedural(
            "PB_Gate_QueueTicks".to_owned(),
            BrickSize::new(10, 5, 2)
        ))
    );
    assert_eq!(
        brick_asset("B_1x1_Gate_Pseudo_Timer"),
        Some(BrickMigration::Basic("B_1x1_Gate_Stopwatch".to_owned()))
    );
    assert_eq!(brick_asset("PB_DefaultBrick"), None);
}

#[test]
fn steps_chain_renames() {
    let steps = steps::parse(CHAINED_STEPS).unwrap();
    assert_eq!(component_type_through(&steps, "Old_Thing"), "New_Thing");
    assert_eq!(component_type_through(&steps, "Old_Other"), "Mid_Other");
    // A save from between the steps only runs the second one.
    assert_eq!(component_type_through(&steps[1..], "Mid_Thing"), "New_Thing");
    assert_eq!(
        brick_through(&steps, "B_A"),
        Some(BrickMigration::Basic("B_C".to_owned()))
    );
    assert!(matches!(
        brick_through(&steps, "B_Gate"),
        Some(BrickMigration::Procedural(asset, _)) if asset == "PB_Gate_QueueTicks"
    ));
    assert_eq!(brick_through(&steps, "B_Other"), None);
    // Field renames compose: Total <- Amount <- Value.
    assert_eq!(
        field_aliases(&steps, "Old_Thing"),
        HashMap::from([("Total".to_owned(), "Value".to_owned())])
    );
    assert!(field_aliases(&steps, "Old_Other").is_empty());
}

#[test]
fn steps_detect_saves_by_container_and_struct() {
    let steps = steps::parse(CHAINED_STEPS).unwrap();
    let read_back = |text: &str, format| {
        let schema = BrdbSchema::new_parsed(text).unwrap();
        BrdbSchema::read(schema.to_bytes_as(format).unwrap().as_slice()).unwrap()
    };
    let ea3 = read_back("struct S { A: i64 }", BrdbSchemaFormat::EA3);
    assert!(steps[0].detects(&ea3));
    assert!(!steps[1].detects(&ea3));
    let between = read_back("struct OnlyBeforeNew { A: i64 }", BrdbSchemaFormat::Latest);
    assert!(!steps[0].detects(&between));
    assert!(steps[1].detects(&between));
}

#[test]
fn renamed_fields_are_written_under_their_new_name() {
    let stored = Arc::new(BrdbSchema::new_parsed("struct Old { Value: i64 }").unwrap());
    let current = BrdbSchema::new_parsed("struct New { Total: i64 }").unwrap();
    let component = MigratedComponent {
        ty: "New_Thing".to_owned(),
        data: struct_value(&stored, "Old", vec![("Value", BrdbValue::I64(42))]),
        aliases: Arc::new(HashMap::from([("Total".to_owned(), "Value".to_owned())])),
    };
    let bytes = current.write_brdb("New", &component).unwrap();
    let current = Arc::new(current);
    let out = read_type(&current, "New", &mut bytes.as_slice()).unwrap();
    assert_eq!(out.prop("Total").unwrap(), &BrdbValue::I64(42));
}

#[test]
fn wire_graph_arrays_become_dynamic_arrays() {
    let ea3 = ea3_schema();
    let values = vec![BrdbValue::F64(1.5), BrdbValue::F64(-2.0)];
    let array = struct_value(
        &ea3,
        "WireGraphDoubleArray",
        vec![("Values", BrdbValue::Array(values.clone()))],
    );
    let var = struct_value(
        &ea3,
        "BrickComponentData_WireGraphPseudo_ArrayVar",
        vec![("Value", array)],
    );
    let out = migrate_through(
        "BrickComponentData_WireGraphPseudo_ArrayVar",
        var,
        "BrickComponentData_ArrayVar",
    );
    assert_eq!(
        out.prop("Value").unwrap(),
        &BrdbValue::DynamicArray(BrdbDynamicArray::new("f64", values))
    );
}

#[test]
fn empty_string_arrays_keep_their_element_type() {
    let ea3 = ea3_schema();
    let array = struct_value(
        &ea3,
        "WireGraphStringArray",
        vec![("Values", BrdbValue::Array(vec![]))],
    );
    let var = struct_value(
        &ea3,
        "BrickComponentData_WireGraphPseudo_ArrayVar",
        vec![("Value", array)],
    );
    let out = migrate_through(
        "BrickComponentData_WireGraphPseudo_ArrayVar",
        var,
        "BrickComponentData_ArrayVar",
    );
    assert_eq!(
        out.prop("Value").unwrap(),
        &BrdbValue::DynamicArray(BrdbDynamicArray::empty("str"))
    );
}

#[test]
fn wire_graph_maps_become_dynamic_maps() {
    let ea3 = ea3_schema();
    let mut entries = IndexMap::new();
    entries.insert(BrdbValue::I64(7), BrdbValue::F64(0.25));
    let map = struct_value(
        &ea3,
        "WireGraphMap_int64_double",
        vec![("Values", BrdbValue::Map(entries.clone()))],
    );
    let wrapper = struct_value(&ea3, "WireGraphMapKeyWrapper_int64", vec![("Map", map)]);
    let var = struct_value(
        &ea3,
        "BrickComponentData_WireGraphPseudo_MapVar",
        vec![("Value", wrapper)],
    );
    let out = migrate_through(
        "BrickComponentData_WireGraphPseudo_MapVar",
        var,
        "BrickComponentData_MapVar",
    );
    assert_eq!(
        out.prop("Value").unwrap(),
        &BrdbValue::DynamicMap(BrdbDynamicMap::new("i64", "f64", entries))
    );
}

#[test]
fn wrapped_enums_become_dynamic_enums() {
    let ea3 = ea3_schema();
    let axis = BrdbValue::Enum(BrdbEnum {
        schema: ea3.clone(),
        name: ea3.intern.get("EBrickAxis").unwrap(),
        value: 2,
    });
    let wrapper = struct_value(&ea3, "WireGraphEnumWrapper", vec![("Enum", axis)]);
    let var = struct_value(
        &ea3,
        "BrickComponentData_WireGraphPseudo_Var",
        vec![("Value", wrapper)],
    );
    let out = migrate_through(
        "BrickComponentData_WireGraphPseudo_Var",
        var,
        "BrickComponentData_Var",
    );
    let BrdbValue::Enum(e) = out.prop("Value").unwrap() else {
        panic!("expected an enum, got {:?}", out.prop("Value"));
    };
    assert_eq!((e.get_name(), e.value), ("EBrickAxis", 2));
}

#[test]
fn plain_wire_values_pass_through() {
    let ea3 = ea3_schema();
    let var = struct_value(
        &ea3,
        "BrickComponentData_WireGraphPseudo_Var",
        vec![("Value", BrdbValue::I64(-9))],
    );
    let out = migrate_through(
        "BrickComponentData_WireGraphPseudo_Var",
        var,
        "BrickComponentData_Var",
    );
    assert_eq!(out.prop("Value").unwrap(), &BrdbValue::I64(-9));
}

#[cfg(feature = "brz")]
#[test]
fn a_world_written_without_components_is_current() -> Result<(), BrError> {
    use crate::{Brick, Brz, IntoReader, World};
    let mut world = World::new();
    world.bricks.push(Brick::default());
    let reader = Brz::read_slice(&world.to_brz_vec()?)?.into_reader();
    assert!(!needs_migration(&reader)?);

    // `Auto` keeps the EA3 container, which reads as EA3.
    world.schema_format = BrdbSchemaFormat::Auto;
    let reader = Brz::read_slice(&world.to_brz_vec()?)?.into_reader();
    assert!(needs_migration(&reader)?);
    Ok(())
}

fn table(names: &[&str]) -> indexmap::IndexSet<String, rustc_hash::FxBuildHasher> {
    names.iter().map(|n| n.to_string()).collect()
}

fn renames<'a>(map: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |n| map.iter().find(|(from, _)| *from == n).map(|(_, to)| to.to_string())
}

#[test]
fn rename_table_keeps_indices_and_remaps_collisions() {
    let mut t = table(&["A", "B", "C"]);
    assert!(rename_table(&mut t, renames(&[("B", "X")])).unwrap().is_empty());
    assert_eq!(t, table(&["A", "X", "C"]));

    // A new name the table already has remaps to its entry.
    let mut t = table(&["P", "U"]);
    let remap = rename_table(&mut t, renames(&[("P", "U")])).unwrap();
    assert_eq!(t, table(&["P", "U"]));
    assert_eq!(remap, HashMap::from([(0, 1)]));

    // Two entries renamed to one new name share the first.
    let mut t = table(&["A", "B", "C"]);
    let remap = rename_table(&mut t, renames(&[("A", "X"), ("C", "X")])).unwrap();
    assert_eq!(t, table(&["X", "B", "C"]));
    assert_eq!(remap, HashMap::from([(2, 0)]));

    // Chained renames keep every name unique.
    let mut t = table(&["A", "B"]);
    rename_table(&mut t, renames(&[("A", "B"), ("B", "C")])).unwrap();
    assert_eq!(t, table(&["B", "C"]));

    // C stays (remapped onto X's entry) while D is renamed to C.
    let mut t = table(&["A", "C", "D"]);
    let err = rename_table(&mut t, renames(&[("A", "X"), ("C", "X"), ("D", "C")]));
    assert!(matches!(err, Err(BrdbWorldError::MigrationDuplicateName(n)) if n == "C"));
}

fn chunk(start: u32, types: &[u32], counters: &[(u32, u32)], sizes: &[BrickSize]) -> BrickChunkSoA {
    let mut soa = BrickChunkSoA::default();
    soa.procedural_brick_starting_index = start;
    soa.brick_type_indices = types.to_vec();
    soa.brick_size_counters = counters
        .iter()
        .map(|&(asset_index, num_sizes)| BrickSizeCounter {
            asset_index,
            num_sizes,
        })
        .collect();
    soa.brick_sizes = sizes.to_vec();
    soa
}

fn counters(soa: &BrickChunkSoA) -> Vec<(u32, u32)> {
    soa.brick_size_counters
        .iter()
        .map(|c| (c.asset_index, c.num_sizes))
        .collect()
}

#[test]
fn gate_bricks_join_the_procedural_slots() {
    let s = |x, y, z| BrickSize::new(x, y, z);
    let gate = s(10, 5, 2);
    let to_gate = HashMap::from([(0usize, (7usize, gate))]);
    let none = HashMap::new();

    // A new asset starts a run after the counted slots; the unreferenced
    // trailing size stays unreferenced.
    let mut soa = chunk(2, &[0, 2, 1], &[(3, 1)], &[s(1, 1, 1), s(9, 9, 9)]);
    assert!(migrate_brick_columns(&mut soa, &none, &none, &to_gate).unwrap());
    assert_eq!(soa.brick_type_indices, [3, 2, 1]);
    assert_eq!(counters(&soa), [(3, 1), (7, 1)]);
    assert_eq!(soa.brick_sizes, [s(1, 1, 1), gate, s(9, 9, 9)]);

    // An asset the chunk has extends its run and shifts later slots.
    let mut soa = chunk(2, &[0, 2, 3], &[(7, 1), (3, 1)], &[s(4, 4, 4), s(1, 1, 1)]);
    migrate_brick_columns(&mut soa, &none, &none, &to_gate).unwrap();
    assert_eq!(counters(&soa), [(7, 2), (3, 1)]);
    assert_eq!(soa.brick_sizes, [s(4, 4, 4), gate, s(1, 1, 1)]);
    assert_eq!(soa.brick_type_indices, [3, 2, 4]);

    // An existing (asset, size) slot is reused.
    let mut soa = chunk(2, &[0, 0, 2], &[(7, 1)], &[gate]);
    migrate_brick_columns(&mut soa, &none, &none, &to_gate).unwrap();
    assert_eq!(soa.brick_type_indices, [2, 2, 2]);
    assert_eq!(counters(&soa), [(7, 1)]);
    assert_eq!(soa.brick_sizes, [gate]);

    // Counters past the size table are rejected.
    let mut soa = chunk(0, &[], &[(0, 2)], &[s(1, 1, 1)]);
    assert!(matches!(
        migrate_brick_columns(&mut soa, &none, &none, &to_gate),
        Err(BrdbWorldError::BrickSizeCountersExceedSizes { counted: 2, sizes: 1 })
    ));
}
