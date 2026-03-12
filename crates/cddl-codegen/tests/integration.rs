use yggdrasil_cddl_codegen::{generate_module, parse_schema};

#[test]
fn parses_and_generates_basic_module() {
    let parsed = parse_schema("Tx = {}\nBlock = {}\n")
        .expect("non-empty schema should parse successfully");
    let generated = generate_module(&parsed);

    assert!(generated.source.contains("pub struct Tx"));
    assert!(generated.source.contains("pub struct Block"));
}
