use yggdrasil_cddl_codegen::{
    FieldKey, GeneratedModule, ParsedField, ParsedType, TypeDefinition, TypeExpr, generate_module,
    parse_schema,
};

#[test]
fn parses_and_generates_basic_module() {
    let parsed = parse_schema("Tx = uint\nBlock = { slot: uint, issuer: bytes }\n")
        .expect("non-empty schema should parse successfully");
    let generated = generate_module(&parsed);

    assert!(generated.source.contains("pub type Tx = u64;"));
    assert!(generated.source.contains("pub struct Block"));
    assert!(generated.source.contains("pub slot: u64"));
    assert!(generated.source.contains("pub issuer: Vec<u8>"));
}

#[test]
fn parses_comments_arrays_and_aliases() {
    let parsed = parse_schema(
        " ; comment only\nheader = [uint, bytes]\ncertificate = header ; trailing comment\n",
    )
    .expect("schema with comments, arrays, and aliases should parse");

    assert_eq!(parsed.len(), 2);

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub struct Header");
    assert_module_contains(&generated, "pub item_0: u64");
    assert_module_contains(&generated, "pub item_1: Vec<u8>");
    assert_module_contains(&generated, "pub type Certificate = Header;");
}

#[test]
fn parses_definition_shapes() {
    let parsed = parse_schema("tx-body = {\n  fee: uint,\n  metadata-hash: bytes,\n}\n")
        .expect("map definitions should parse");

    assert_eq!(
        parsed,
        vec![ParsedType {
            name: String::from("tx-body"),
            definition: TypeDefinition::Map(vec![
                ParsedField {
                    key: FieldKey::Label(String::from("fee")),
                    ty: TypeExpr::Named(String::from("uint")),
                    optional: false,
                },
                ParsedField {
                    key: FieldKey::Label(String::from("metadata-hash")),
                    ty: TypeExpr::Named(String::from("bytes")),
                    optional: false,
                },
            ]),
        }]
    );
}

#[test]
fn parses_size_constrained_aliases() {
    let parsed = parse_schema("block_number = uint .size 8\nhash32 = bytes .size 32\n")
        .expect("size-constrained aliases should parse");

    assert_eq!(parsed.len(), 2);
    assert_eq!(
        parsed[0].definition,
        TypeDefinition::Alias(TypeExpr::Sized(String::from("uint"), 8))
    );
    assert_eq!(
        parsed[1].definition,
        TypeDefinition::Alias(TypeExpr::Sized(String::from("bytes"), 32))
    );

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub type BlockNumber = u64;");
    assert_module_contains(&generated, "pub type Hash32 = [u8; 32];");
}

#[test]
fn parses_integer_keyed_map_with_optional_fields() {
    let schema = "tx = {\n  0: uint,\n  1: bytes,\n  ? 2: uint,\n}\n";
    let parsed = parse_schema(schema).expect("integer-keyed map should parse");

    assert_eq!(parsed.len(), 1);
    let TypeDefinition::Map(fields) = &parsed[0].definition else {
        panic!("expected map definition");
    };
    assert_eq!(fields.len(), 3);

    assert_eq!(fields[0].key, FieldKey::Index(0));
    assert!(!fields[0].optional);
    assert_eq!(fields[1].key, FieldKey::Index(1));
    assert!(!fields[1].optional);
    assert_eq!(fields[2].key, FieldKey::Index(2));
    assert!(fields[2].optional);

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub field_0: u64");
    assert_module_contains(&generated, "pub field_1: Vec<u8>");
    assert_module_contains(&generated, "pub field_2: Option<u64>");
}

#[test]
fn parses_var_array_type_expr() {
    let schema = "inputs = [* uint]\n";
    let parsed = parse_schema(schema).expect("var-array alias should parse");
    assert_eq!(
        parsed[0].definition,
        TypeDefinition::Alias(TypeExpr::VarArray(Box::new(TypeExpr::Named(
            String::from("uint")
        ))))
    );

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub type Inputs = Vec<u64>;");
}

#[test]
fn parses_var_array_in_map_fields() {
    let schema = "body = {\n  0: [* uint],\n  1: [* bytes],\n}\n";
    let parsed = parse_schema(schema).expect("map with var-array fields should parse");

    let TypeDefinition::Map(fields) = &parsed[0].definition else {
        panic!("expected map");
    };
    assert_eq!(
        fields[0].ty,
        TypeExpr::VarArray(Box::new(TypeExpr::Named(String::from("uint"))))
    );

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub field_0: Vec<u64>");
    assert_module_contains(&generated, "pub field_1: Vec<Vec<u8>>");
}

#[test]
fn parses_named_array_fields() {
    let schema = "input = [id: hash32, index: uint .size 2]\n";
    let parsed = parse_schema(schema).expect("named array fields should parse");

    let TypeDefinition::Array(items) = &parsed[0].definition else {
        panic!("expected array");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].name.as_deref(), Some("id"));
    assert_eq!(items[0].ty, TypeExpr::Named(String::from("hash32")));
    assert_eq!(items[1].name.as_deref(), Some("index"));
    assert_eq!(items[1].ty, TypeExpr::Sized(String::from("uint"), 2));

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub id: Hash32");
    assert_module_contains(&generated, "pub index: u16");
}

#[test]
fn generates_sized_uint_variants() {
    let schema =
        "a = uint .size 1\nb = uint .size 2\nc = uint .size 4\nd = uint .size 8\n";
    let parsed = parse_schema(schema).expect("sized uint variants should parse");
    let generated = generate_module(&parsed);

    assert_module_contains(&generated, "pub type A = u8;");
    assert_module_contains(&generated, "pub type B = u16;");
    assert_module_contains(&generated, "pub type C = u32;");
    assert_module_contains(&generated, "pub type D = u64;");
}

#[test]
fn parses_shelley_fixture_subset() {
    let fixture = std::fs::read_to_string("../../specs/mini-ledger.cddl")
        .expect("pinned Shelley fixture should exist");
    let parsed = parse_schema(&fixture).expect("fixture should parse without errors");

    // Verify key definitions were parsed.
    let names: Vec<&str> = parsed.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"block_number"));
    assert!(names.contains(&"slot"));
    assert!(names.contains(&"hash32"));
    assert!(names.contains(&"vkey"));
    assert!(names.contains(&"transaction_id"));
    assert!(names.contains(&"coin"));
    assert!(names.contains(&"transaction_input"));
    assert!(names.contains(&"transaction_body"));
    assert!(names.contains(&"block-header"));
    assert!(names.contains(&"tx-seq"));

    let generated = generate_module(&parsed);

    // Size-constrained aliases
    assert_module_contains(&generated, "pub type BlockNumber = u64;");
    assert_module_contains(&generated, "pub type Slot = u64;");
    assert_module_contains(&generated, "pub type Hash32 = [u8; 32];");
    assert_module_contains(&generated, "pub type Vkey = [u8; 32];");
    assert_module_contains(&generated, "pub type Signature = [u8; 64];");
    assert_module_contains(&generated, "pub type KesSignature = [u8; 448];");

    // Plain aliases
    assert_module_contains(&generated, "pub type TransactionId = Hash32;");
    assert_module_contains(&generated, "pub type Coin = u64;");

    // Named array with size-constrained field
    assert_module_contains(&generated, "pub struct TransactionInput");
    assert_module_contains(&generated, "pub id: TransactionId");
    assert_module_contains(&generated, "pub index: u16");

    // Integer-keyed map with optional fields
    assert_module_contains(&generated, "pub struct TransactionBody");
    assert_module_contains(&generated, "pub field_0: Vec<TransactionInput>");
    assert_module_contains(&generated, "pub field_4: Option<Vec<Vkeywitness>>");
    assert_module_contains(&generated, "pub field_7: Option<AuxiliaryDataHash>");
}

fn assert_module_contains(module: &GeneratedModule, needle: &str) {
    assert!(
        module.source.contains(needle),
        "generated module missing expected content: {needle}\n{}",
        module.source
    );
}
