use std::{fs, path::PathBuf};
use yggdrasil_cddl_codegen::{generate_module, parse_schema};

#[test]
fn ledger_can_intake_foundation_schema_fixture() {
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("specs")
        .join("mini-ledger.cddl");
    let schema = fs::read_to_string(&schema_path)
        .expect("foundation schema fixture should be readable from the repository specs directory");

    let parsed = parse_schema(&schema)
        .expect("foundation schema fixture should parse as supported CDDL subset");
    let generated = generate_module(&parsed);

    assert!(
        generated.source.contains("pub struct TxBody"),
        "generated source should expose a TxBody struct\n{}",
        generated.source
    );
    assert!(
        generated.source.contains("pub fee: u64"),
        "generated source should map uint to u64\n{}",
        generated.source
    );
    assert!(
        generated.source.contains("pub metadata_hash: Vec<u8>"),
        "generated source should normalize hyphenated field names\n{}",
        generated.source
    );
    assert!(
        generated.source.contains("pub struct BlockHeader"),
        "generated source should expose a BlockHeader struct\n{}",
        generated.source
    );
    assert!(
        generated.source.contains("pub struct TxSeq"),
        "generated source should generate an array-backed struct for TxSeq\n{}",
        generated.source
    );
}