use yggdrasil_cddl_codegen::{GeneratedModule, ParsedType, generate_module, parse_schema};

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
    let parsed = parse_schema(
        "tx-body = {\n  fee: uint,\n  metadata-hash: bytes,\n}\n",
    )
        .expect("map definitions should parse");

    assert_eq!(
        parsed,
        vec![ParsedType {
            name: String::from("tx-body"),
            definition: yggdrasil_cddl_codegen::parser::TypeDefinition::Map(vec![
                yggdrasil_cddl_codegen::parser::ParsedField {
                    name: String::from("fee"),
                    ty: String::from("uint"),
                },
                yggdrasil_cddl_codegen::parser::ParsedField {
                    name: String::from("metadata-hash"),
                    ty: String::from("bytes"),
                },
            ]),
        }]
    );
}

fn assert_module_contains(module: &GeneratedModule, needle: &str) {
    assert!(
        module.source.contains(needle),
        "generated module missing expected content: {needle}\n{}",
        module.source
    );
}
