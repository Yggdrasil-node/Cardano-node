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
        generated.source.contains("pub struct TransactionBody"),
        "generated source should expose a TransactionBody struct\n{}",
        generated.source
    );
    assert!(
        generated.source.contains("pub field_2: Coin"),
        "generated source should map integer-keyed coin field\n{}",
        generated.source
    );
    assert!(
        generated.source.contains("pub field_7: Option<AuxiliaryDataHash>"),
        "generated source should map optional fields\n{}",
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