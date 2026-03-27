//! Integration tests for output-level predicate failures:
//!
//! - `OutputTooBigUTxO`: serialized Value exceeds `max_val_size`.
//! - `OutputBootAddrAttrsTooBig`: Byron address attributes exceed 64 bytes.
//!
//! Upstream references:
//! - `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateOutputTooBigUTxO`
//! - `Cardano.Ledger.Shelley.Rules.Utxo` — `validateOutputBootAddrAttrsTooBig`

use super::*;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn alonzo_params() -> ProtocolParameters {
    ProtocolParameters::alonzo_defaults()
}

fn enterprise_addr() -> Vec<u8> {
    let mut addr = vec![0x61]; // enterprise keyhash, network 1
    addr.extend_from_slice(&[0xAA; 28]);
    addr
}

/// Build a multi-asset value with `n_policies × n_assets` entries.
/// Each asset name is ~10 bytes and each quantity is a large u64.
fn large_multi_asset(n_policies: usize, n_assets: usize) -> BTreeMap<[u8; 28], BTreeMap<Vec<u8>, u64>> {
    let mut ma = BTreeMap::new();
    for p in 0..n_policies {
        let mut pid = [0u8; 28];
        pid[0] = (p & 0xFF) as u8;
        pid[1] = ((p >> 8) & 0xFF) as u8;
        let mut assets = BTreeMap::new();
        for a in 0..n_assets {
            let name = format!("tkn_{:05}", a).into_bytes();
            assets.insert(name, 999_999_999_999u64);
        }
        ma.insert(pid, assets);
    }
    ma
}

/// Construct a valid Byron address with a given attributes-blob size.
///
/// Format: `array(2)` `tag(24)` `bstr(payload)` `uint(checksum)`
/// where payload = `array(3)` `tag(24) bstr(root)` `<attrs_bytes>` `uint(0)`.
fn byron_address_with_attrs_size(attrs_size: usize) -> Vec<u8> {
    // Build the inner payload first
    let root = [0xBB; 28]; // dummy root hash

    // attributes: a CBOR map with enough padding bytes.
    // We'll use a definite-length map with byte string entries to reach the target size.
    let attrs_blob = if attrs_size <= 1 {
        // Empty map is 0xa0 (1 byte)
        vec![0xa0]
    } else {
        // Build a map with a single entry: { 0x01: bstr(padding) }
        // map(1)=0xa1, key=uint(1)=0x01, value=bstr(N)
        // overhead: 1(map) + 1(key) + header(bstr) = 2 + bstr_header
        // For bstr_header: if length < 24 => 1 byte => total overhead=3
        //   if length < 256 => 2 bytes => total overhead=4
        //   if length < 65536 => 3 bytes => total overhead=5
        // We want total size = attrs_size
        let overhead = if attrs_size <= 3 + 23 {
            3
        } else if attrs_size <= 4 + 255 {
            4
        } else {
            5
        };
        let padding_len = if attrs_size > overhead { attrs_size - overhead } else { 0 };

        let mut enc = Encoder::new();
        enc.map(1);
        enc.unsigned(1);
        let padding = vec![0u8; padding_len];
        enc.bytes(&padding);
        enc.into_bytes()
    };

    // Build payload: array(3) [tag(24) bstr(root), attrs_blob, uint(0)]
    let mut payload_enc = Encoder::new();
    payload_enc.array(3);
    payload_enc.tag(24);
    payload_enc.bytes(&root);
    // Write attrs_blob raw (it's already valid CBOR)
    payload_enc.raw(&attrs_blob);
    payload_enc.unsigned(0); // address type
    let payload = payload_enc.into_bytes();

    // Byron outer: array(2) [tag(24) bstr(payload), uint(checksum)]
    let mut outer_enc = Encoder::new();
    outer_enc.array(2);
    outer_enc.tag(24);
    outer_enc.bytes(&payload);
    outer_enc.unsigned(0); // dummy checksum
    outer_enc.into_bytes()
}

// ---------------------------------------------------------------------------
// OutputTooBigUTxO tests
// ---------------------------------------------------------------------------

/// A normal-sized output should pass the max_val_size check.
#[test]
fn output_value_within_max_val_size() {
    let params = alonzo_params(); // max_val_size = Some(5000)
    let outputs = vec![MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: enterprise_addr(),
        amount: Value::Coin(2_000_000),
        datum_hash: None,
    })];
    assert!(yggdrasil_ledger::validate_output_not_too_big(&params, &outputs).is_ok());
}

/// A multi-asset value that serializes beyond max_val_size should be rejected.
#[test]
fn output_value_exceeds_max_val_size() {
    let params = alonzo_params(); // max_val_size = 5000
    // 20 policies × 20 assets ≈ well over 5000 CBOR bytes
    let huge = Value::CoinAndAssets(2_000_000, large_multi_asset(20, 20));
    let outputs = vec![MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: enterprise_addr(),
        amount: huge,
        datum_hash: None,
    })];
    let result = yggdrasil_ledger::validate_output_not_too_big(&params, &outputs);
    assert!(
        matches!(result, Err(LedgerError::OutputTooBig { .. })),
        "Expected OutputTooBig, got: {:?}",
        result,
    );
}

/// When max_val_size is None (pre-Alonzo), the check is a no-op.
#[test]
fn output_value_no_limit_when_max_val_size_none() {
    let mut params = alonzo_params();
    params.max_val_size = None;
    let huge = Value::CoinAndAssets(2_000_000, large_multi_asset(20, 20));
    let outputs = vec![MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: enterprise_addr(),
        amount: huge,
        datum_hash: None,
    })];
    assert!(yggdrasil_ledger::validate_output_not_too_big(&params, &outputs).is_ok());
}

/// Multiple outputs: first passes, second is too big → rejects.
#[test]
fn output_value_second_output_too_big() {
    let params = alonzo_params();
    let ok_output = MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: enterprise_addr(),
        amount: Value::Coin(2_000_000),
        datum_hash: None,
    });
    let big_output = MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: enterprise_addr(),
        amount: Value::CoinAndAssets(2_000_000, large_multi_asset(20, 20)),
        datum_hash: None,
    });
    let outputs = vec![ok_output, big_output];
    let result = yggdrasil_ledger::validate_output_not_too_big(&params, &outputs);
    assert!(matches!(result, Err(LedgerError::OutputTooBig { .. })));
}

/// Babbage output with large value should also be caught.
#[test]
fn babbage_output_value_too_big() {
    let params = alonzo_params();
    let outputs = vec![MultiEraTxOut::Babbage(BabbageTxOut {
        address: enterprise_addr(),
        amount: Value::CoinAndAssets(2_000_000, large_multi_asset(20, 20)),
        datum_option: None,
        script_ref: None,
    })];
    let result = yggdrasil_ledger::validate_output_not_too_big(&params, &outputs);
    assert!(matches!(result, Err(LedgerError::OutputTooBig { .. })));
}

// ---------------------------------------------------------------------------
// OutputBootAddrAttrsTooBig tests
// ---------------------------------------------------------------------------

/// A non-Byron (Shelley enterprise) address should pass (not checked).
#[test]
fn boot_addr_check_ignores_shelley_addr() {
    let outputs = vec![MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: enterprise_addr(),
        amount: Value::Coin(2_000_000),
        datum_hash: None,
    })];
    assert!(yggdrasil_ledger::validate_output_boot_addr_attrs(&outputs).is_ok());
}

/// A Byron address with small attributes (empty map = 1 byte) should pass.
#[test]
fn boot_addr_small_attrs_pass() {
    let byron = byron_address_with_attrs_size(1); // empty map
    let outputs = vec![MultiEraTxOut::Babbage(BabbageTxOut {
        address: byron,
        amount: Value::Coin(2_000_000),
        datum_option: None,
        script_ref: None,
    })];
    assert!(yggdrasil_ledger::validate_output_boot_addr_attrs(&outputs).is_ok());
}

/// A Byron address with attributes exactly at the 64-byte limit should pass.
#[test]
fn boot_addr_attrs_at_limit_pass() {
    let byron = byron_address_with_attrs_size(64);
    let outputs = vec![MultiEraTxOut::Babbage(BabbageTxOut {
        address: byron,
        amount: Value::Coin(2_000_000),
        datum_option: None,
        script_ref: None,
    })];
    assert!(yggdrasil_ledger::validate_output_boot_addr_attrs(&outputs).is_ok());
}

/// A Byron address with attributes at 65 bytes (one over limit) should fail.
#[test]
fn boot_addr_attrs_one_over_limit_rejected() {
    let byron = byron_address_with_attrs_size(65);
    let outputs = vec![MultiEraTxOut::Babbage(BabbageTxOut {
        address: byron,
        amount: Value::Coin(2_000_000),
        datum_option: None,
        script_ref: None,
    })];
    let result = yggdrasil_ledger::validate_output_boot_addr_attrs(&outputs);
    assert!(
        matches!(result, Err(LedgerError::OutputBootAddrAttrsTooBig { .. })),
        "Expected OutputBootAddrAttrsTooBig, got: {:?}",
        result,
    );
}

/// A Byron address with very large attributes (200 bytes) should fail.
#[test]
fn boot_addr_attrs_very_large_rejected() {
    let byron = byron_address_with_attrs_size(200);
    let outputs = vec![MultiEraTxOut::Babbage(BabbageTxOut {
        address: byron,
        amount: Value::Coin(2_000_000),
        datum_option: None,
        script_ref: None,
    })];
    let result = yggdrasil_ledger::validate_output_boot_addr_attrs(&outputs);
    assert!(
        matches!(result, Err(LedgerError::OutputBootAddrAttrsTooBig { size }) if size == 200),
        "Expected OutputBootAddrAttrsTooBig with size 200, got: {:?}",
        result,
    );
}

/// Mixed outputs: Byron ok + Shelley ok → pass.
#[test]
fn boot_addr_mixed_outputs_all_ok() {
    let byron = byron_address_with_attrs_size(30);
    let outputs = vec![
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: byron,
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: enterprise_addr(),
            amount: Value::Coin(3_000_000),
            datum_hash: None,
        }),
    ];
    assert!(yggdrasil_ledger::validate_output_boot_addr_attrs(&outputs).is_ok());
}

/// Mixed outputs: Shelley ok + Byron too big → fail on second.
#[test]
fn boot_addr_mixed_outputs_second_fails() {
    let byron = byron_address_with_attrs_size(100);
    let outputs = vec![
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: enterprise_addr(),
            amount: Value::Coin(3_000_000),
            datum_hash: None,
        }),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: byron,
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }),
    ];
    let result = yggdrasil_ledger::validate_output_boot_addr_attrs(&outputs);
    assert!(matches!(result, Err(LedgerError::OutputBootAddrAttrsTooBig { .. })));
}
