//! cardano-testnet default genesis / script values.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Defaults.hs.
//!
//! This slice ports the era-free script values — the `simpleScript`
//! builder and the always-succeeds Plutus test scripts. Upstream
//! `Defaults.hs` is otherwise era / ledger-coupled (per-era default
//! genesis records, default key pairs, topology); those land once
//! yggdrasil-ledger's era surface is exposed at crate boundaries.
//! The two large Plutus blobs (`plutusV3SupplementalDatumScript`,
//! `plutusV2StakeScript`) land in a follow-up round.

/// Build a "simple script" (native-script) JSON envelope requiring a
/// single signer.
///
/// Mirror of upstream `simpleScript :: Text -> Text`.
pub fn simple_script(signer_required: &str) -> String {
    format!(
        "{{ \"scripts\": [ {{ \"keyHash\": \"{signer_required}\", \"type\": \"sig\" }} ], \"type\": \"all\" }}"
    )
}

/// An always-succeeds Plutus V2 test script (text-envelope JSON).
///
/// Mirror of upstream `plutusV2Script`.
pub const PLUTUS_V2_SCRIPT: &str = r#"{ "type": "PlutusScriptV2", "description": "", "cborHex": "5822582001000022325333573466e1ccde5251333792945200000100111200116375a005" }"#;

/// An always-succeeds Plutus V3 test script (text-envelope JSON).
///
/// Mirror of upstream `plutusV3Script`.
pub const PLUTUS_V3_SCRIPT: &str =
    r#"{ "type": "PlutusScriptV3", "description": "", "cborHex": "46450101002499" }"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_script_embeds_the_signer_hash() {
        let s = simple_script("deadbeef");
        assert!(s.contains(r#""keyHash": "deadbeef""#), "got: {s}");
        assert!(s.contains(r#""type": "sig""#));
        assert!(s.contains(r#""type": "all""#));
    }

    #[test]
    fn plutus_scripts_are_text_envelopes() {
        assert!(PLUTUS_V2_SCRIPT.contains(r#""type": "PlutusScriptV2""#));
        assert!(PLUTUS_V2_SCRIPT.contains("cborHex"));
        assert!(PLUTUS_V3_SCRIPT.contains(r#""type": "PlutusScriptV3""#));
        assert!(PLUTUS_V3_SCRIPT.contains(r#""cborHex": "46450101002499""#));
    }

    #[test]
    fn plutus_scripts_parse_as_json() {
        let v2: serde_json::Value = serde_json::from_str(PLUTUS_V2_SCRIPT).expect("V2 is JSON");
        assert_eq!(v2["type"], "PlutusScriptV2");
        let v3: serde_json::Value = serde_json::from_str(PLUTUS_V3_SCRIPT).expect("V3 is JSON");
        assert_eq!(v3["type"], "PlutusScriptV3");
    }
}
