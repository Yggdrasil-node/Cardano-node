use yggdrasil_cddl_codegen::{
    FieldKey, GeneratedModule, ParsedField, ParsedType, TypeDefinition, TypeExpr,
    generate_module, generate_module_with_codecs, parse_schema,
};
use yggdrasil_cddl_codegen::parser::ParseError;

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

// ===========================================================================
// CBOR tag annotations (#6.N)
// ===========================================================================

#[test]
fn parses_cbor_tag_annotation() {
    let schema = "tagged_set = #6.258([* uint])\n";
    let parsed = parse_schema(schema).expect("CBOR tag annotation should parse");

    assert_eq!(
        parsed[0].definition,
        TypeDefinition::Alias(TypeExpr::Tagged(
            258,
            Box::new(TypeExpr::VarArray(Box::new(TypeExpr::Named(
                String::from("uint")
            ))))
        ))
    );

    let generated = generate_module(&parsed);
    // Tagged types unwrap to the inner type in generated code.
    assert_module_contains(&generated, "pub type TaggedSet = Vec<u64>;");
}

#[test]
fn parses_cbor_tag_with_named_type() {
    let schema = "encoded = #6.24(bytes)\n";
    let parsed = parse_schema(schema).expect("tag 24 should parse");
    assert_eq!(
        parsed[0].definition,
        TypeDefinition::Alias(TypeExpr::Tagged(
            24,
            Box::new(TypeExpr::Named(String::from("bytes")))
        ))
    );

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub type Encoded = Vec<u8>;");
}

// ===========================================================================
// Group choices (//)
// ===========================================================================

#[test]
fn parses_group_choice() {
    let schema = "cert = [tag: uint, key: bytes] // [tag: uint, pool: bytes, vrf: bytes]\n";
    let parsed = parse_schema(schema).expect("group choice should parse");

    let TypeDefinition::GroupChoice(variants) = &parsed[0].definition else {
        panic!("expected GroupChoice, got {:?}", parsed[0].definition);
    };
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].len(), 2);
    assert_eq!(variants[1].len(), 3);
    assert_eq!(variants[0][0].name.as_deref(), Some("tag"));
    assert_eq!(variants[1][2].name.as_deref(), Some("vrf"));
}

#[test]
fn generates_enum_from_group_choice() {
    let schema = "cert = [reg: uint, cred: bytes] // [dereg: uint, cred: bytes]\n";
    let parsed = parse_schema(schema).expect("group choice should parse");
    let generated = generate_module(&parsed);

    assert_module_contains(&generated, "pub enum Cert {");
    assert_module_contains(&generated, "Reg {");
    assert_module_contains(&generated, "Dereg {");
}

#[test]
fn generates_variant_index_fallback() {
    let schema = "choice = [uint, bytes] // [uint, uint, bytes]\n";
    let parsed = parse_schema(schema).expect("unnamed group choice should parse");
    let generated = generate_module(&parsed);

    assert_module_contains(&generated, "pub enum Choice {");
    assert_module_contains(&generated, "Variant0 {");
    assert_module_contains(&generated, "Variant1 {");
}

// ===========================================================================
// Shelley fixture — extended definitions
// ===========================================================================

#[test]
fn parses_shelley_fixture_tagged_and_choice() {
    let fixture = std::fs::read_to_string("../../specs/mini-ledger.cddl")
        .expect("pinned Shelley fixture should exist");
    let parsed = parse_schema(&fixture).expect("fixture should parse without errors");

    let names: Vec<&str> = parsed.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"set_of_inputs"));
    assert!(names.contains(&"certificate"));

    let generated = generate_module(&parsed);

    // Tagged set → Vec<TransactionInput>
    assert_module_contains(&generated, "pub type SetOfInputs = Vec<TransactionInput>;");

    // Group choice → enum Certificate
    assert_module_contains(&generated, "pub enum Certificate {");
    assert_module_contains(&generated, "Reg {");
    assert_module_contains(&generated, "Dereg {");
    assert_module_contains(&generated, "Delegate {");
}

// ===========================================================================
// CBOR codec generation
// ===========================================================================

#[test]
fn codec_gen_array_struct() {
    let schema = "input = [id: hash32, index: uint .size 2]\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // CborEncode
    assert_module_contains(&generated, "impl CborEncode for Input {");
    assert_module_contains(&generated, "enc.array(2)");
    assert_module_contains(&generated, "self.id.encode_cbor(enc)");
    assert_module_contains(&generated, "enc.unsigned(self.index as u64)");

    // CborDecode
    assert_module_contains(&generated, "impl CborDecode for Input {");
    assert_module_contains(&generated, "let _len = dec.array()?");
    assert_module_contains(&generated, "let id = Hash32::decode_cbor(dec)?");
    assert_module_contains(&generated, "let index = dec.unsigned()? as u16");
    assert_module_contains(&generated, "Ok(Self {");
}

#[test]
fn codec_gen_map_with_integer_keys() {
    let schema = "body = {\n  0: [* uint],\n  1: [* bytes],\n  2: uint,\n}\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // CborEncode
    assert_module_contains(&generated, "impl CborEncode for Body {");
    assert_module_contains(&generated, "enc.map(3)");
    assert_module_contains(&generated, "enc.unsigned(0)");
    assert_module_contains(&generated, "enc.unsigned(1)");
    assert_module_contains(&generated, "enc.unsigned(2)");

    // CborDecode
    assert_module_contains(&generated, "impl CborDecode for Body {");
    assert_module_contains(&generated, "let map_len = dec.map()?");
    assert_module_contains(&generated, "let key = dec.unsigned()?");
    assert_module_contains(&generated, "match key {");
    assert_module_contains(&generated, "0 => {");
    assert_module_contains(&generated, "1 => {");
    assert_module_contains(&generated, "2 => {");
    assert_module_contains(&generated, "_ => { dec.skip()?; }");
}

#[test]
fn codec_gen_map_with_optional_fields() {
    let schema = "tx = {\n  0: uint,\n  1: bytes,\n  ? 2: uint,\n}\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // Encode: conditional count and optional field encoding.
    assert_module_contains(&generated, "let mut count: u64 = 2");
    assert_module_contains(&generated, "if self.field_2.is_some() { count += 1; }");
    assert_module_contains(&generated, "if let Some(ref val) = self.field_2 {");
    assert_module_contains(&generated, "enc.unsigned(2)");

    // Decode: optional fields are None when missing.
    assert_module_contains(&generated, "let mut field_2: Option<u64> = None");
    assert_module_contains(&generated, "field_2,");
}

#[test]
fn codec_gen_map_with_string_keys() {
    let schema = "header = { slot: uint, issuer: bytes }\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // CborEncode
    assert_module_contains(&generated, "impl CborEncode for Header {");
    assert_module_contains(&generated, "enc.text(\"slot\")");
    assert_module_contains(&generated, "enc.text(\"issuer\")");

    // CborDecode
    assert_module_contains(&generated, "let key = dec.text()?");
    assert_module_contains(&generated, "\"slot\" => {");
    assert_module_contains(&generated, "\"issuer\" => {");
}

#[test]
fn codec_gen_group_choice() {
    let schema = "cert = [reg: uint, cred: bytes] // [dereg: uint, cred: bytes]\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // CborEncode
    assert_module_contains(&generated, "impl CborEncode for Cert {");
    assert_module_contains(&generated, "match self {");
    assert_module_contains(&generated, "Self::Reg { reg, cred } => {");
    assert_module_contains(&generated, "enc.array(2)");

    // CborDecode
    assert_module_contains(&generated, "impl CborDecode for Cert {");
    assert_module_contains(&generated, "let len = dec.array()?");
}

#[test]
fn codec_gen_alias_no_impl() {
    let schema = "coin = uint\nhash = bytes .size 32\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // Aliases should NOT get CborEncode/CborDecode impls.
    assert!(
        !generated.source.contains("impl CborEncode for Coin"),
        "alias should not generate codec impl"
    );
    assert!(
        !generated.source.contains("impl CborDecode for Hash"),
        "alias should not generate codec impl"
    );
}

#[test]
fn codec_gen_tagged_type_in_array() {
    let schema = "tagged = [val: #6.258([* uint])]\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // Encode should emit tag then inner array.
    assert_module_contains(&generated, "enc.tag(258)");
    // Decode should consume tag.
    assert_module_contains(&generated, "let t = dec.tag()?");
}

#[test]
fn codec_gen_var_array_in_map() {
    let schema = "body = { 0: [* uint] }\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // Encode: array of items inside map.
    assert_module_contains(&generated, "enc.array(self.field_0.len() as u64)");
    assert_module_contains(&generated, "for item in &self.field_0 {");

    // Decode: array inside map key dispatch.
    assert_module_contains(&generated, "let count = dec.array()?");
    assert_module_contains(&generated, "v.push(dec.unsigned()?)");
}

#[test]
fn codec_gen_shelley_fixture_transaction_body() {
    let fixture = std::fs::read_to_string("../../specs/mini-ledger.cddl")
        .expect("pinned Shelley fixture should exist");
    let parsed = parse_schema(&fixture).expect("fixture should parse");
    let generated = generate_module_with_codecs(&parsed);

    // TransactionBody should get integer-keyed map codec.
    assert_module_contains(&generated, "impl CborEncode for TransactionBody {");
    assert_module_contains(&generated, "impl CborDecode for TransactionBody {");

    // TransactionInput should get array codec.
    assert_module_contains(&generated, "impl CborEncode for TransactionInput {");
    assert_module_contains(&generated, "impl CborDecode for TransactionInput {");

    // TransactionOutput should get array codec.
    assert_module_contains(&generated, "impl CborEncode for TransactionOutput {");
    assert_module_contains(&generated, "impl CborDecode for TransactionOutput {");

    // Certificate should get group-choice codec.
    assert_module_contains(&generated, "impl CborEncode for Certificate {");
    assert_module_contains(&generated, "impl CborDecode for Certificate {");
}

#[test]
fn codec_gen_fixed_bytes_decode() {
    let schema = "key = [vkey: bytes .size 32, sig: bytes .size 64]\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    // Fixed bytes decode should use try_into with error.
    assert_module_contains(&generated, "let b = dec.bytes()?");
    assert_module_contains(&generated, "try_into().map_err");
    assert_module_contains(&generated, "LedgerError::CborInvalidLength");
}

// ===========================================================================
// Parser error cases
// ===========================================================================

#[test]
fn error_empty_schema() {
    assert_eq!(parse_schema("").unwrap_err(), ParseError::Empty);
}

#[test]
fn error_comments_only_is_empty() {
    assert_eq!(parse_schema("; just a comment\n").unwrap_err(), ParseError::Empty);
}

#[test]
fn error_missing_assignment() {
    let err = parse_schema("no_equals_sign\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingAssignment(_)));
}

#[test]
fn error_invalid_type_name_leading_digit() {
    let err = parse_schema("123bad = uint\n").unwrap_err();
    assert!(matches!(err, ParseError::InvalidTypeName(_)));
}

#[test]
fn error_empty_definition() {
    let err = parse_schema("foo = \n").unwrap_err();
    assert!(matches!(err, ParseError::EmptyDefinition(_)));
}

#[test]
fn error_invalid_map_field_no_colon() {
    let err = parse_schema("m = { badfield }\n").unwrap_err();
    assert!(matches!(err, ParseError::InvalidField(_)));
}

#[test]
fn error_invalid_size_non_numeric() {
    let err = parse_schema("h = bytes .size abc\n").unwrap_err();
    assert!(matches!(err, ParseError::InvalidSize(_)));
}

#[test]
fn error_invalid_tag_no_parens() {
    let err = parse_schema("t = #6.258\n").unwrap_err();
    assert!(matches!(err, ParseError::InvalidSize(_)));
}

// ===========================================================================
// Nil alternative (type / nil)
// ===========================================================================

#[test]
fn parses_nil_alternative() {
    let parsed = parse_schema("maybe_coin = uint / nil\n").expect("nil alternative should parse");
    assert_eq!(
        parsed[0].definition,
        TypeDefinition::Alias(TypeExpr::Optional(Box::new(TypeExpr::Named(
            String::from("uint")
        ))))
    );

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub type MaybeCoin = Option<u64>;");
}

#[test]
fn parses_nil_alternative_in_map() {
    let schema = "body = { 0: uint, 1: bytes / nil }\n";
    let parsed = parse_schema(schema).expect("nil alt in map should parse");

    let TypeDefinition::Map(fields) = &parsed[0].definition else {
        panic!("expected map");
    };
    assert_eq!(
        fields[1].ty,
        TypeExpr::Optional(Box::new(TypeExpr::Named(String::from("bytes"))))
    );

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub field_1: Option<Vec<u8>>");
}

// ===========================================================================
// Nested var-array
// ===========================================================================

#[test]
fn parses_nested_var_array() {
    let schema = "matrix = [* [* uint]]\n";
    let parsed = parse_schema(schema).expect("nested var-array should parse");
    assert_eq!(
        parsed[0].definition,
        TypeDefinition::Alias(TypeExpr::VarArray(Box::new(TypeExpr::VarArray(
            Box::new(TypeExpr::Named(String::from("uint")))
        ))))
    );

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub type Matrix = Vec<Vec<u64>>;");
}

// ===========================================================================
// Group choice with equal-length variants
// ===========================================================================

#[test]
fn group_choice_unnamed_equal_length() {
    let schema = "op = [uint, bytes] // [uint, uint]\n";
    let parsed = parse_schema(schema).expect("equal-length group choice should parse");

    let TypeDefinition::GroupChoice(variants) = &parsed[0].definition else {
        panic!("expected GroupChoice");
    };
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].len(), 2);
    assert_eq!(variants[1].len(), 2);

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub enum Op {");
    assert_module_contains(&generated, "Variant0 {");
    assert_module_contains(&generated, "Variant1 {");
}

// ===========================================================================
// Tagged type in map field
// ===========================================================================

#[test]
fn codec_gen_tagged_in_map_field() {
    let schema = "body = { 0: #6.258([* uint]) }\n";
    let parsed = parse_schema(schema).expect("should parse");
    let generated = generate_module_with_codecs(&parsed);

    assert_module_contains(&generated, "enc.tag(258)");
    assert_module_contains(&generated, "let t = dec.tag()?");
}

// ===========================================================================
// Multiple definitions in single schema
// ===========================================================================

#[test]
fn parses_multiple_interdependent_types() {
    let schema = "hash32 = bytes .size 32\ninput = [id: hash32, index: uint .size 2]\nbody = { 0: [* input], 2: uint }\n";
    let parsed = parse_schema(schema).expect("multi-def schema should parse");
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].name, "hash32");
    assert_eq!(parsed[1].name, "input");
    assert_eq!(parsed[2].name, "body");

    let generated = generate_module(&parsed);
    assert_module_contains(&generated, "pub type Hash32 = [u8; 32];");
    assert_module_contains(&generated, "pub struct Input");
    assert_module_contains(&generated, "pub struct Body");
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn parses_type_name_with_underscores_and_hyphens() {
    let parsed = parse_schema("my_type-name = uint\n").expect("hyphens/underscores in names");
    assert_eq!(parsed[0].name, "my_type-name");
}

#[test]
fn parses_empty_map() {
    let parsed = parse_schema("empty = {}\n").expect("empty map should parse");
    let TypeDefinition::Map(fields) = &parsed[0].definition else {
        panic!("expected map");
    };
    assert!(fields.is_empty());
}

#[test]
fn codec_gen_empty_map() {
    let parsed = parse_schema("empty = {}\n").expect("should parse");
    let generated = generate_module_with_codecs(&parsed);
    assert_module_contains(&generated, "impl CborEncode for Empty {");
    assert_module_contains(&generated, "enc.map(0)");
}
