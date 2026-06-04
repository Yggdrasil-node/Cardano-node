//! EraIndependent run.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Key/Run.hs`.
//! R293 landed the file with the API skeleton. R520 ports
//! `runKeyCmds` / `runVerificationKeyCmd` for normal Ed25519
//! signing-key TextEnvelope families.

use std::path::Path;

use eyre::{Result, WrapErr};

use crate::era_independent::address::run::write_text_envelope;
use crate::era_independent::key::command::{KeyCmds, KeyVerificationKeyCmdArgs};

/// Run a [`KeyCmds`] value.
///
/// Mirrors upstream `runKeyCmds` from
/// `Cardano.CLI.EraIndependent.Key.Run`.
pub fn run_key_cmds(command: KeyCmds) -> Result<()> {
    match command {
        KeyCmds::KeyVerificationKeyCmd(args) => run_verification_key_cmd(args),
    }
}

/// Run `key verification-key`.
///
/// Mirrors upstream `runVerificationKeyCmd`. The bounded Rust port
/// accepts the normal Ed25519 signing-key TextEnvelope families used
/// by payment, stake, governance, genesis, and stake-pool operators,
/// derives the verification key, and writes the corresponding
/// verification-key TextEnvelope.
pub fn run_verification_key_cmd(args: KeyVerificationKeyCmdArgs) -> Result<()> {
    let signing = read_signing_key_text_envelope(&args.signing_key_file)?;
    let sk = yggdrasil_crypto::SigningKey::from_bytes(signing.key_bytes);
    let vk = sk
        .verification_key()
        .map_err(|e| eyre::eyre!("failed to derive verification key: {e}"))?;
    write_text_envelope(
        &args.verification_key_file,
        signing.verification_envelope_type,
        signing.verification_description,
        &vk.to_bytes(),
        false,
    )
}

struct SigningKeyEnvelope {
    key_bytes: [u8; 32],
    verification_envelope_type: &'static str,
    verification_description: &'static str,
}

fn read_signing_key_text_envelope(path: &Path) -> Result<SigningKeyEnvelope> {
    let envelope_bytes = std::fs::read(path)
        .wrap_err_with(|| format!("failed to read --signing-key-file {}", path.display()))?;
    let envelope: serde_json::Value = serde_json::from_slice(&envelope_bytes)
        .map_err(|e| eyre::eyre!("TextEnvelope is not valid JSON: {e}"))?;
    let envelope_type = envelope
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| eyre::eyre!("TextEnvelope is missing the required `type` string field"))?;
    let (verification_envelope_type, verification_description) = match envelope_type {
        "PaymentSigningKeyShelley_ed25519" => (
            "PaymentVerificationKeyShelley_ed25519",
            "Payment Verification Key",
        ),
        "StakeSigningKeyShelley_ed25519" => (
            "StakeVerificationKeyShelley_ed25519",
            "Stake Verification Key",
        ),
        "DRepSigningKey_ed25519" => (
            "DRepVerificationKey_ed25519",
            "Delegated Representative Verification Key",
        ),
        "ConstitutionalCommitteeColdSigningKey_ed25519" => (
            "ConstitutionalCommitteeColdVerificationKey_ed25519",
            "Constitutional Committee Cold Verification Key",
        ),
        "ConstitutionalCommitteeHotSigningKey_ed25519" => (
            "ConstitutionalCommitteeHotVerificationKey_ed25519",
            "Constitutional Committee Hot Verification Key",
        ),
        "GenesisSigningKey_ed25519" => {
            ("GenesisVerificationKey_ed25519", "Genesis Verification Key")
        }
        "GenesisDelegateSigningKey_ed25519" => (
            "GenesisDelegateVerificationKey_ed25519",
            "Genesis delegate operator key",
        ),
        "GenesisUTxOSigningKey_ed25519" => (
            "GenesisUTxOVerificationKey_ed25519",
            "Genesis initial UTxO key",
        ),
        "StakePoolSigningKey_ed25519" => (
            "StakePoolVerificationKey_ed25519",
            "Stake Pool Operator Verification Key",
        ),
        other => {
            eyre::bail!(
                "unsupported signing key TextEnvelope type {other:?}; \
                 supported types are normal Ed25519 payment, stake, DRep, \
                 constitutional committee, genesis, genesis delegate, genesis UTxO, \
                 and stake-pool signing keys"
            );
        }
    };
    let cbor_hex = envelope
        .get("cborHex")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            eyre::eyre!("TextEnvelope is missing the required `cborHex` string field")
        })?;
    let cbor_bytes = hex::decode(cbor_hex.trim())
        .map_err(|e| eyre::eyre!("TextEnvelope cborHex is not valid hex: {e}"))?;
    if cbor_bytes.len() != 34 {
        eyre::bail!(
            "expected 34 bytes of cborHex (2-byte CBOR prefix + 32-byte key), got {}",
            cbor_bytes.len()
        );
    }
    if cbor_bytes[0] != 0x58 || cbor_bytes[1] != 0x20 {
        eyre::bail!(
            "expected CBOR prefix 0x5820 (bytes-string of length 32), got 0x{:02x}{:02x}",
            cbor_bytes[0],
            cbor_bytes[1]
        );
    }
    let mut key_bytes = [0_u8; 32];
    key_bytes.copy_from_slice(&cbor_bytes[2..]);
    Ok(SigningKeyEnvelope {
        key_bytes,
        verification_envelope_type,
        verification_description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_key_cmd_derives_payment_vkey() {
        let dir = temp_dir("yggdrasil-cli-key-vk");
        let skey = dir.join("payment.skey");
        let vkey = dir.join("payment.vkey");
        let out = dir.join("derived.vkey");
        crate::era_independent::address::run::run_address_key_gen_cmd(&vkey, &skey)
            .expect("address key-gen");

        run_verification_key_cmd(KeyVerificationKeyCmdArgs {
            signing_key_file: skey,
            verification_key_file: out.clone(),
        })
        .expect("key verification-key");

        let expected = std::fs::read_to_string(&vkey).expect("read original vkey");
        let actual = std::fs::read_to_string(&out).expect("read derived vkey");
        assert_eq!(actual, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verification_key_cmd_derives_stake_vkey_metadata() {
        let dir = temp_dir("yggdrasil-cli-key-stake-vk");
        let skey = dir.join("stake.skey");
        let vkey = dir.join("stake.vkey");
        let out = dir.join("derived-stake.vkey");
        crate::era_based::stake_address::run::run_stake_address_key_gen_cmd(&vkey, &skey)
            .expect("stake-address key-gen");

        run_verification_key_cmd(KeyVerificationKeyCmdArgs {
            signing_key_file: skey,
            verification_key_file: out.clone(),
        })
        .expect("key verification-key");

        let expected = std::fs::read_to_string(&vkey).expect("read original vkey");
        let actual_text = std::fs::read_to_string(&out).expect("read derived vkey");
        assert_eq!(actual_text, expected);
        let actual: serde_json::Value =
            serde_json::from_str(&actual_text).expect("derived vkey JSON");
        assert_eq!(actual["type"], "StakeVerificationKeyShelley_ed25519");
        assert_eq!(actual["description"], "Stake Verification Key");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verification_key_cmd_rejects_unknown_signing_key_type() {
        let dir = temp_dir("yggdrasil-cli-key-bad-type");
        let skey = dir.join("bad.skey");
        let out = dir.join("out.vkey");
        std::fs::write(
            &skey,
            r#"{"type":"BogusSigningKey","description":"bad","cborHex":"58200000000000000000000000000000000000000000000000000000000000000000"}"#,
        )
        .expect("write bad skey");

        let err = run_verification_key_cmd(KeyVerificationKeyCmdArgs {
            signing_key_file: skey,
            verification_key_file: out,
        })
        .expect_err("unknown signing key type must fail");
        assert!(
            err.to_string()
                .contains("unsupported signing key TextEnvelope type"),
            "error must explain unsupported type; got {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verification_key_cmd_supports_upstream_normal_ed25519_families() {
        let cases = [
            (
                "DRepSigningKey_ed25519",
                "DRepVerificationKey_ed25519",
                "Delegated Representative Verification Key",
            ),
            (
                "ConstitutionalCommitteeColdSigningKey_ed25519",
                "ConstitutionalCommitteeColdVerificationKey_ed25519",
                "Constitutional Committee Cold Verification Key",
            ),
            (
                "ConstitutionalCommitteeHotSigningKey_ed25519",
                "ConstitutionalCommitteeHotVerificationKey_ed25519",
                "Constitutional Committee Hot Verification Key",
            ),
            (
                "GenesisSigningKey_ed25519",
                "GenesisVerificationKey_ed25519",
                "Genesis Verification Key",
            ),
            (
                "GenesisDelegateSigningKey_ed25519",
                "GenesisDelegateVerificationKey_ed25519",
                "Genesis delegate operator key",
            ),
            (
                "GenesisUTxOSigningKey_ed25519",
                "GenesisUTxOVerificationKey_ed25519",
                "Genesis initial UTxO key",
            ),
            (
                "StakePoolSigningKey_ed25519",
                "StakePoolVerificationKey_ed25519",
                "Stake Pool Operator Verification Key",
            ),
        ];

        for (idx, (skey_type, expected_vkey_type, expected_description)) in
            cases.into_iter().enumerate()
        {
            let dir = temp_dir(&format!("yggdrasil-cli-key-family-{idx}"));
            let skey = dir.join("input.skey");
            let out = dir.join("output.vkey");
            let seed = [idx as u8 + 1; 32];
            let signing = yggdrasil_crypto::SigningKey::from_bytes(seed);
            let expected_vk = signing
                .verification_key()
                .expect("test signing seed derives verification key")
                .to_bytes();
            crate::era_independent::address::run::write_text_envelope(
                &skey,
                skey_type,
                "test signing key",
                &signing.to_bytes(),
                true,
            )
            .expect("write signing envelope");

            run_verification_key_cmd(KeyVerificationKeyCmdArgs {
                signing_key_file: skey,
                verification_key_file: out.clone(),
            })
            .expect("key verification-key");

            let actual: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&out).expect("read output vkey"))
                    .expect("output vkey JSON");
            assert_eq!(actual["type"], expected_vkey_type);
            assert_eq!(actual["description"], expected_description);
            assert_eq!(
                actual["cborHex"],
                format!("5820{}", hex::encode(expected_vk)),
                "{skey_type} should derive the expected verification key bytes",
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
