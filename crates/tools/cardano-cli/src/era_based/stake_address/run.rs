//! EraBased stake-address run.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraBased/StakeAddress/Run.hs`.
//! R292 landed the file as an API skeleton. R508 (Phase 3.2) lands
//! the first concrete subcommand — `run_stake_address_key_gen_cmd`,
//! mirroring upstream `runStakeAddressKeyGenCmd`. Remaining
//! stake-address subcommands (`stake-address build`,
//! `stake-address registration-certificate`, …) port over
//! subsequent rounds.

use std::path::Path;

use eyre::Result;

use crate::era_independent::address::run::{KeyKind, generate_keypair_to_envelopes};

/// Run `stake-address key-gen` — generate a fresh Ed25519 stake
/// keypair and write both keys as TextEnvelope JSON files.
///
/// Mirrors upstream `runStakeAddressKeyGenCmd` from
/// `Cardano.CLI.EraBased.StakeAddress.Run`. The entropy + on-wire
/// key shape are identical to `address key-gen` — only the
/// TextEnvelope `type` / `description` metadata differs
/// (`StakeSigningKeyShelley_ed25519` /
/// `StakeVerificationKeyShelley_ed25519`), so the keypair generation
/// reuses the shared [`generate_keypair_to_envelopes`] helper with
/// [`KeyKind::Stake`].
pub fn run_stake_address_key_gen_cmd(
    verification_key_file: &Path,
    signing_key_file: &Path,
) -> Result<()> {
    generate_keypair_to_envelopes(verification_key_file, signing_key_file, KeyKind::Stake)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `run_stake_address_key_gen_cmd` writes two TextEnvelope files
    /// carrying the **stake** key metadata — distinguishing them
    /// from the payment envelopes `address key-gen` produces.
    #[test]
    fn stake_key_gen_writes_stake_typed_envelopes() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-stakekeygen-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let vk_path = dir.join("stake.vkey");
        let sk_path = dir.join("stake.skey");

        run_stake_address_key_gen_cmd(&vk_path, &sk_path).expect("stake key-gen must succeed");

        let vk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&vk_path).expect("read vkey"))
                .expect("vkey is JSON");
        let sk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&sk_path).expect("read skey"))
                .expect("skey is JSON");

        assert_eq!(vk["type"], "StakeVerificationKeyShelley_ed25519");
        assert_eq!(sk["type"], "StakeSigningKeyShelley_ed25519");
        assert_eq!(vk["description"], "Stake Verification Key");
        assert_eq!(sk["description"], "Stake Signing Key");
        // `5820` (2 bytes) + 32-byte key = 34 bytes = 68 hex chars.
        assert_eq!(vk["cborHex"].as_str().expect("vk cborHex").len(), 68);
        assert_eq!(sk["cborHex"].as_str().expect("sk cborHex").len(), 68);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
