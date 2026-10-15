//! Synthetic tests for the EA3 and Latest schema/value formats.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::{
    errors::BrdbSchemaError,
    schema::{
        BrdbDynamicArray, BrdbDynamicEnum, BrdbDynamicMap, BrdbSchema, BrdbSchemaFormat,
        BrdbSchemaGlobalData, BrdbValue, EA3WireVariant, SavedBrickRef, SavedComponentRef,
        WireValue, as_brdb::AsBrdbValue, read::read_type, write,
    },
    wrapper::ChunkIndex,
};

fn parsed(text: &str) -> BrdbSchema {
    BrdbSchema::new_parsed(text).unwrap()
}

fn with_format(text: &str, format: BrdbSchemaFormat) -> Arc<BrdbSchema> {
    let mut schema = parsed(text);
    schema.set_format(format);
    Arc::new(schema)
}

fn encode(schema: &BrdbSchema, ty: &str, value: &dyn AsBrdbValue) -> Vec<u8> {
    let mut buf = Vec::new();
    write::write_brdb(schema, &mut buf, ty, value).unwrap();
    buf
}

/// Decode `ty` and require that every byte was consumed.
fn decode(schema: &Arc<BrdbSchema>, ty: &str, bytes: &[u8]) -> BrdbValue {
    let mut rest = bytes;
    let value = read_type(schema, ty, &mut rest).unwrap();
    assert!(rest.is_empty(), "{} trailing bytes after {ty}", rest.len());
    value
}

fn mp_str(buf: &mut Vec<u8>, s: &str) {
    rmp::encode::write_str(buf, s).unwrap();
}

/// `{ "S": { "A": "i64" } }` as a struct section.
fn struct_section(buf: &mut Vec<u8>) {
    rmp::encode::write_map_len(buf, 1).unwrap();
    mp_str(buf, "S");
    rmp::encode::write_map_len(buf, 1).unwrap();
    mp_str(buf, "A");
    mp_str(buf, "i64");
}

fn enum_section(buf: &mut Vec<u8>) {
    rmp::encode::write_map_len(buf, 1).unwrap();
    mp_str(buf, "E");
    rmp::encode::write_map_len(buf, 2).unwrap();
    mp_str(buf, "E::A");
    rmp::encode::write_pfix(buf, 0).unwrap();
    mp_str(buf, "E::B");
    rmp::encode::write_pfix(buf, 1).unwrap();
}

#[test]
fn reads_legacy_array_containers() {
    // [enums, structs]
    let mut two = Vec::new();
    rmp::encode::write_array_len(&mut two, 2).unwrap();
    enum_section(&mut two);
    struct_section(&mut two);
    let schema = BrdbSchema::read(two.as_slice()).unwrap();
    assert_eq!(schema.source_format(), Some(BrdbSchemaFormat::EA3));
    assert!(schema.get_enum("E").is_some() && schema.get_struct("S").is_some());

    // [enums, variants, structs]
    let mut three = Vec::new();
    rmp::encode::write_array_len(&mut three, 3).unwrap();
    enum_section(&mut three);
    rmp::encode::write_map_len(&mut three, 1).unwrap();
    mp_str(&mut three, "V");
    rmp::encode::write_array_len(&mut three, 2).unwrap();
    mp_str(&mut three, "f64");
    mp_str(&mut three, "S");
    struct_section(&mut three);
    let schema = BrdbSchema::read(three.as_slice()).unwrap();
    assert_eq!(schema.format(), BrdbSchemaFormat::EA3);
    assert_eq!(schema.get_variant("V").unwrap().len(), 2);
    // EA3 schemas re-emit as the same array bytes.
    assert_eq!(schema.to_bytes().unwrap(), three);
}

#[test]
fn reads_map_container_in_any_section_order() {
    let mut bytes = Vec::new();
    rmp::encode::write_map_len(&mut bytes, 3).unwrap();
    mp_str(&mut bytes, "types");
    rmp::encode::write_array_len(&mut bytes, 2).unwrap();
    mp_str(&mut bytes, "E");
    mp_str(&mut bytes, "i64");
    mp_str(&mut bytes, "structs");
    struct_section(&mut bytes);
    mp_str(&mut bytes, "enums");
    enum_section(&mut bytes);
    let schema = BrdbSchema::read(bytes.as_slice()).unwrap();
    assert_eq!(schema.source_format(), Some(BrdbSchemaFormat::Latest));
    assert_eq!(schema.type_names(), ["E", "i64"]);
    assert_eq!(schema.type_index("i64"), Some(1));
    assert!(schema.get_struct("S").is_some());

    // `{}` is a valid, empty schema.
    let empty = BrdbSchema::read([0x80u8].as_slice()).unwrap();
    assert!(empty.structs.is_empty() && empty.types.is_empty());
    assert_eq!(empty.source_format(), Some(BrdbSchemaFormat::Latest));
    assert_eq!(empty.to_bytes().unwrap(), [0x80]);
}

#[test]
fn rejects_duplicate_unknown_and_invalid_sections() {
    let mut dup = Vec::new();
    rmp::encode::write_map_len(&mut dup, 2).unwrap();
    mp_str(&mut dup, "structs");
    struct_section(&mut dup);
    mp_str(&mut dup, "structs");
    struct_section(&mut dup);
    assert!(matches!(
        BrdbSchema::read(dup.as_slice()),
        Err(BrdbSchemaError::InvalidSchema(m)) if m.contains("duplicate")
    ));

    let mut unknown = Vec::new();
    rmp::encode::write_map_len(&mut unknown, 1).unwrap();
    mp_str(&mut unknown, "aliases");
    rmp::encode::write_map_len(&mut unknown, 0).unwrap();
    assert!(matches!(
        BrdbSchema::read(unknown.as_slice()),
        Err(BrdbSchemaError::InvalidSchema(m)) if m.contains("unknown")
    ));

    let mut dynamic_type = Vec::new();
    rmp::encode::write_map_len(&mut dynamic_type, 1).unwrap();
    mp_str(&mut dynamic_type, "types");
    rmp::encode::write_array_len(&mut dynamic_type, 1).unwrap();
    mp_str(&mut dynamic_type, "dynamic_array");
    assert!(BrdbSchema::read(dynamic_type.as_slice()).is_err());

    let mut four = Vec::new();
    rmp::encode::write_array_len(&mut four, 4).unwrap();
    assert!(matches!(
        BrdbSchema::read(four.as_slice()),
        Err(BrdbSchemaError::InvalidHeader(4))
    ));
}

const DYNAMIC_SCHEMA: &str = "enum EColor {
    EColor::Red = 0,
    EColor::Green = 1,
}
variant WireValue {
    Bool,
    i64,
    f64,
    Str,
    dynamic_enum,
    weak_object,
    BRSavedComponentRef,
    WireGraphExec,
}
struct BRSavedChunk3DIndex {
    X: i16,
    Y: i16,
    Z: i16,
}
struct BRSavedBrickRef {
    GridPersistentIndex: u32,
    ChunkIndex: BRSavedChunk3DIndex,
    BrickIndexInChunk: u32,
}
struct BRSavedComponentRef {
    Brick: BRSavedBrickRef,
    ComponentTypeIndex: u16,
}
struct WireGraphExec {
}
struct Holder {
    Values: dynamic_array,
    Lookup: dynamic_map,
    Value: WireValue,
    Tail: u8,
}
types {
    EColor,
    i64,
    Str,
    f64,
    Bool,
    brick_handle,
}
";

#[test]
fn schema_formats_select_container_and_round_trip() {
    let schema = parsed(DYNAMIC_SCHEMA);
    assert!(schema.needs_map_form());
    assert_eq!(schema.format(), BrdbSchemaFormat::Latest);
    // Auto picks the map container, and EA3 cannot express the types table.
    let bytes = schema.to_bytes().unwrap();
    assert_eq!(bytes[0] & 0xf0, 0x80);
    assert!(schema.to_bytes_as(BrdbSchemaFormat::EA3).is_err());
    let back = BrdbSchema::read(bytes.as_slice()).unwrap();
    assert_eq!(back.to_string(), schema.to_string());
    assert_eq!(back.to_bytes().unwrap(), bytes);

    // The text form round-trips the types table.
    assert_eq!(parsed(&schema.to_string()).to_string(), schema.to_string());

    // A schema without types stays in the EA3 array container under Auto,
    // and the Latest map omits every empty section.
    let plain = parsed("struct S {\n    A: i64,\n}\n");
    assert_eq!(plain.format(), BrdbSchemaFormat::EA3);
    assert_eq!(plain.to_bytes().unwrap()[0], 0x93);
    let latest = plain.to_bytes_as(BrdbSchemaFormat::Latest).unwrap();
    let mut expected = Vec::new();
    rmp::encode::write_map_len(&mut expected, 1).unwrap();
    mp_str(&mut expected, "structs");
    struct_section(&mut expected);
    assert_eq!(latest, expected);
    // A schema read in the map form keeps it under Auto.
    let reread = BrdbSchema::read(latest.as_slice()).unwrap();
    assert_eq!(reread.to_bytes().unwrap(), latest);
}

#[test]
fn dynamic_containers_round_trip_including_empty() {
    let schema = Arc::new(parsed(DYNAMIC_SCHEMA));
    let mut lookup = IndexMap::new();
    lookup.insert(BrdbValue::I64(12), BrdbValue::F64(3.5));
    let holder = |values, lookup| {
        let mut s = crate::schema::BrdbStruct {
            schema: schema.clone(),
            name: schema.intern.get("Holder").unwrap(),
            properties: Default::default(),
        };
        let prop = |n: &str| schema.intern.get(n).unwrap();
        s.properties
            .insert(prop("Values"), BrdbValue::DynamicArray(values));
        s.properties
            .insert(prop("Lookup"), BrdbValue::DynamicMap(lookup));
        s.properties
            .insert(prop("Value"), BrdbValue::String("hi".into()));
        s.properties.insert(prop("Tail"), BrdbValue::U8(7));
        BrdbValue::Struct(Box::new(s))
    };

    let value = holder(
        BrdbDynamicArray::new("i64", vec![BrdbValue::I64(10), BrdbValue::I64(20)]),
        BrdbDynamicMap::new("i64", "f64", lookup),
    );
    let bytes = encode(&schema, "Holder", &value);
    // Values: type 1 (i64), [10, 20]; Lookup: types 1 (i64) and 3 (f64), {12: 3.5}
    assert_eq!(&bytes[..4], [0x01, 0x92, 10, 20]);
    assert_eq!(&bytes[4..7], [0x01, 0x03, 0x81]);
    let back = decode(&schema, "Holder", &bytes);
    assert_eq!(back, value);

    // Empty containers still carry their selected types.
    let empty = holder(
        BrdbDynamicArray::empty("Str"),
        BrdbDynamicMap::empty("EColor", "Bool"),
    );
    let bytes = encode(&schema, "Holder", &empty);
    assert_eq!(&bytes[..5], [0x02, 0x90, 0x00, 0x04, 0x80]);
    assert_eq!(decode(&schema, "Holder", &bytes), empty);

    // An unset field writes an empty container: f64 elements, i64 -> f64 pairs.
    let mut zero = Vec::new();
    write::write_brdb_zero(&schema, &mut zero, "dynamic_array").unwrap();
    write::write_brdb_zero(&schema, &mut zero, "dynamic_map").unwrap();
    assert_eq!(zero, [0x03, 0x90, 0x01, 0x03, 0x80]);
}

#[test]
fn dynamic_type_references_are_validated() {
    let schema = Arc::new(parsed(DYNAMIC_SCHEMA));
    // Index 9 is outside the six-entry types table.
    let err = read_type(&schema, "dynamic_array", &mut [0x09u8, 0x90].as_slice()).unwrap_err();
    assert!(matches!(
        err,
        BrdbSchemaError::InvalidTypeIndex { index: 9, len: 6 }
    ));
    // Writing a type that is not in the table fails.
    let mut buf = Vec::new();
    let err = write::write_brdb(
        &schema,
        &mut buf,
        "dynamic_array",
        &BrdbDynamicArray::empty("Vector"),
    );
    assert!(err.is_err());
}

#[test]
fn handle_types_are_unsupported_but_empty_containers_round_trip() {
    let schema = Arc::new(parsed(DYNAMIC_SCHEMA));
    // An empty array of brick handles: type index 5, no elements.
    let empty = decode(&schema, "dynamic_array", &[0x05, 0x90]);
    assert_eq!(
        empty,
        BrdbValue::DynamicArray(BrdbDynamicArray::empty("brick_handle"))
    );
    assert_eq!(encode(&schema, "dynamic_array", &empty), [0x05, 0x90]);
    // A populated one cannot be decoded: the element encoding is unknown.
    let err = read_type(
        &schema,
        "dynamic_array",
        &mut [0x05u8, 0x91, 0x00].as_slice(),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("brick_handle"));
}

#[test]
fn wire_value_alternatives_and_extra_type_strings() {
    let schema = Arc::new(parsed(DYNAMIC_SCHEMA));
    let brick = SavedBrickRef {
        grid_persistent_index: 1,
        chunk_index: ChunkIndex { x: -1, y: 0, z: 2 },
        brick_index_in_chunk: 42,
    };
    let cases = [
        (WireValue::Bool(true), vec![0x00, 0xc3]),
        (WireValue::Int(-5), vec![0x01, 0xfb]),
        (WireValue::Number(2.5), {
            let mut v = vec![0x02];
            write::write_float64(&mut v, 2.5).unwrap();
            v
        }),
        (WireValue::Str("ok".into()), vec![0x03, 0xa2, b'o', b'k']),
        // dynamic_enum: the enum's types index (0 = EColor), then the value.
        (
            WireValue::Enum(BrdbDynamicEnum::new("EColor", 1)),
            vec![0x04, 0x00, 0x01],
        ),
        (WireValue::Object, vec![0x05, 0xff]),
        (
            WireValue::ComponentRef(SavedComponentRef {
                brick,
                component_type_index: 3,
            }),
            vec![0x06, 0x01, 0xff, 0x00, 0x02, 42, 0x03],
        ),
        (WireValue::Exec, vec![0x07]),
    ];
    for (value, expected) in cases {
        let bytes = encode(&schema, "WireValue", &value);
        assert_eq!(bytes, expected, "{value:?}");
        let decoded = decode(&schema, "WireValue", &bytes);
        assert_eq!(WireValue::try_from(&decoded).unwrap(), value);
        // A decoded value writes back to the same alternative.
        assert_eq!(encode(&schema, "WireValue", &decoded), expected);
    }
}

#[test]
fn variant_alternatives_are_matched_by_type_not_position() {
    // The saved schema lists [f64, i64, bool]; the current one reorders,
    // renames bool to Bool and adds Str.
    let saved = with_format(
        "variant V {\n    f64,\n    i64,\n    bool,\n}\nstruct S {\n    Value: V,\n    Tail: u8,\n}\n",
        BrdbSchemaFormat::EA3,
    );
    let current = with_format(
        "variant V {\n    Str,\n    Bool,\n    i64,\n    f64,\n}\nstruct S {\n    Value: V,\n    Tail: u8,\n}\n",
        BrdbSchemaFormat::Latest,
    );
    // Saved selector 2 (bool), then Tail.
    let value = decode(&saved, "S", &[0x02, 0xc3, 0x09]);
    let bytes = encode(&current, "S", &value);
    // Current selector for Bool is 1.
    assert_eq!(bytes, [0x01, 0xc3, 0x09]);
    let back = decode(&current, "S", &bytes);
    assert_eq!(back.prop("Value").unwrap(), &BrdbValue::Bool(true));
    // The EA3 wire value type maps the same way.
    assert_eq!(encode(&current, "V", &EA3WireVariant::Int(4)), [0x02, 0x04]);
}

#[test]
fn removed_alternative_is_consumed_with_its_saved_type() {
    let saved = with_format(
        "variant V {\n    f64,\n    Old,\n}\nstruct Old {\n    A: i64,\n    B: str,\n}\nstruct S {\n    Value: V,\n    Tail: u8,\n}\n",
        BrdbSchemaFormat::EA3,
    );
    let current = with_format(
        "variant V {\n    f64,\n}\nstruct S {\n    Value: V,\n    Tail: u8,\n}\n",
        BrdbSchemaFormat::Latest,
    );
    // Selector 1 (Old {A: 300, B: "x"}) then Tail = 5: the saved type
    // consumes the whole payload, so Tail stays aligned.
    let mut bytes = vec![0x01];
    write::write_int(&mut bytes, 300).unwrap();
    bytes.extend([0xa1, b'x', 0x05]);
    let value = decode(&saved, "S", &bytes);
    assert_eq!(value.prop("Tail").unwrap(), &BrdbValue::U8(5));
    // The current schema has no such alternative.
    let mut buf = Vec::new();
    assert!(write::write_brdb(&current, &mut buf, "S", &value).is_err());
}

#[test]
fn nil_fields_consume_no_bytes() {
    let schema = Arc::new(parsed("struct S {\n    A: nil,\n    B: u8,\n}\n"));
    let value = decode(&schema, "S", &[0x07]);
    assert_eq!(value.prop("A").unwrap(), &BrdbValue::Nil);
    assert_eq!(encode(&schema, "S", &value), [0x07]);
}

#[test]
fn flat_arrays_are_c_layout_in_latest_and_packed_in_ea3() {
    let text = "struct P {\n    A: u8,\n    B: u32,\n}\nstruct S {\n    Items: P[flat],\n}\n";
    let ea3 = with_format(text, BrdbSchemaFormat::EA3);
    let latest = with_format(text, BrdbSchemaFormat::Latest);
    assert_eq!(super::read::flat_type_size(&ea3, "P"), 5);
    assert_eq!(super::read::flat_type_size(&latest, "P"), 8);

    // Latest: A, 3 padding bytes, B (little endian).
    let latest_bytes = [0xc4, 8, 0x01, 0, 0, 0, 0x02, 0, 0, 0];
    let value = decode(&latest, "S", &latest_bytes);
    let item = value.prop("Items").unwrap().index_unwrap(0).unwrap();
    assert_eq!(item.prop("A").unwrap(), &BrdbValue::U8(1));
    assert_eq!(item.prop("B").unwrap(), &BrdbValue::U32(2));
    assert_eq!(encode(&latest, "S", &value), latest_bytes);

    // EA3: fields back to back.
    let ea3_bytes = [0xc4, 5, 0x01, 0x02, 0, 0, 0];
    let value = decode(&ea3, "S", &ea3_bytes);
    assert_eq!(encode(&ea3, "S", &value), ea3_bytes);

    // Naturally aligned structs are identical in both.
    let vec3 = "struct V {\n    X: f32,\n    Y: f32,\n    Z: f32,\n}\n";
    assert_eq!(
        super::read::flat_type_size(&with_format(vec3, BrdbSchemaFormat::EA3), "V"),
        super::read::flat_type_size(&with_format(vec3, BrdbSchemaFormat::Latest), "V"),
    );
}

#[test]
fn u8_reads_legacy_negatives_and_writes_per_format() {
    let ea3 = with_format("struct S {\n    A: u8,\n}\n", BrdbSchemaFormat::EA3);
    let latest = with_format("struct S {\n    A: u8,\n}\n", BrdbSchemaFormat::Latest);
    for schema in [&ea3, &latest] {
        // -128 (int 8) and -1 (negative fixint) are 128 and 255.
        assert_eq!(decode(schema, "u8", &[0xd0, 0x80]), BrdbValue::U8(128));
        assert_eq!(decode(schema, "u8", &[0xff]), BrdbValue::U8(255));
        assert_eq!(decode(schema, "u8", &[0xcc, 200]), BrdbValue::U8(200));
    }
    assert_eq!(encode(&ea3, "u8", &200u8), [0xd0, 200]);
    assert_eq!(encode(&latest, "u8", &200u8), [0xcc, 200]);
    assert_eq!(encode(&latest, "u8", &7u8), [7]);
}

#[test]
fn floats_accept_any_numeric_tag() {
    let schema = Arc::new(BrdbSchema::default());
    let mut f64_bytes = vec![0xcb];
    f64_bytes.extend(1.5f64.to_be_bytes());
    assert_eq!(decode(&schema, "f32", &f64_bytes), BrdbValue::F32(1.5));
    let mut i64_bytes = vec![0xd3];
    i64_bytes.extend((-7i64).to_be_bytes());
    assert_eq!(decode(&schema, "f32", &i64_bytes), BrdbValue::F32(-7.0));
    let mut u64_bytes = vec![0xcf];
    u64_bytes.extend(9u64.to_be_bytes());
    assert_eq!(decode(&schema, "f64", &u64_bytes), BrdbValue::F64(9.0));
}

fn asset_schema(text: &str, format: BrdbSchemaFormat) -> Arc<BrdbSchema> {
    let mut data = BrdbSchemaGlobalData::default();
    for i in 0..4 {
        data.external_asset_references
            .insert(("Asset".to_owned(), format!("A{i}")));
    }
    let mut schema = parsed(text).with_global_data(Arc::new(data));
    schema.set_format(format);
    Arc::new(schema)
}

/// Regression: EA3 `wire_graph_variant` selector 3 carries an i64 index that
/// must be consumed, and `weak_object` keeps its index.
#[test]
fn ea3_weak_object_and_wire_graph_variant_selector_3_keep_their_index() {
    let text = "struct S {\n    W: wire_graph_variant,\n    O: weak_object,\n    Tail: u8,\n}\n";
    let schema = asset_schema(text, BrdbSchemaFormat::EA3);
    // W = selector 3 + index 2, O = index 3, Tail = 9
    let bytes = [0x03, 0x02, 0x03, 0x09];
    let value = decode(&schema, "S", &bytes);
    assert!(matches!(
        value.prop("W").unwrap(),
        BrdbValue::EA3WireVar(EA3WireVariant::Object(Some(2)))
    ));
    assert_eq!(value.prop("O").unwrap(), &BrdbValue::Asset(Some(3)));
    assert_eq!(value.prop("Tail").unwrap(), &BrdbValue::U8(9));
    // Byte-identical re-encode.
    assert_eq!(encode(&schema, "S", &value), bytes);
    let mut via_value = Vec::new();
    write::write_type(&schema, &mut via_value, "S", &value).unwrap();
    assert_eq!(via_value, bytes);
}

#[test]
fn latest_weak_object_and_wire_graph_variant_follow_the_spec() {
    let text = "struct S {\n    W: wire_graph_variant,\n    O: weak_object,\n    Tail: u8,\n}\n";
    let schema = asset_schema(text, BrdbSchemaFormat::Latest);
    // W = selector 3 (no payload), O = 3 (discarded), Tail = 9
    let value = decode(&schema, "S", &[0x03, 0x03, 0x09]);
    assert!(matches!(
        value.prop("W").unwrap(),
        BrdbValue::EA3WireVar(EA3WireVariant::Object(None))
    ));
    assert_eq!(value.prop("O").unwrap(), &BrdbValue::Asset(None));
    // weak_object is written as -1.
    assert_eq!(encode(&schema, "S", &value), [0x03, 0xff, 0x09]);
}

#[test]
fn extracted_structs_carry_the_types_table() {
    let schema = parsed(DYNAMIC_SCHEMA);
    let (enums, _, structs, types) = schema.extract_structs_transitive(["Holder"]);
    assert_eq!(types, schema.type_names());
    assert!(enums.iter().any(|(n, _)| n == "EColor"));
    assert!(structs.iter().any(|(n, _)| n == "BRSavedComponentRef"));

    let mut small = parsed("struct Base {\n}\n");
    small.add_schema_meta(schema.extract_structs_transitive(["Holder"]));
    assert_eq!(small.format(), BrdbSchemaFormat::Latest);
    assert_eq!(small.type_names(), schema.type_names());

    // Nothing dynamic: no types table.
    let plain = parsed("struct A {\n    X: i64,\n}\ntypes {\n    i64,\n}\n");
    assert!(plain.extract_structs_transitive(["A"]).3.is_empty());
}
