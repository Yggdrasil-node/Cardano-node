//! EraBased stake-address run.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraBased/StakeAddress/Run.hs`.
//! R292 landed the file as an API skeleton. R508 (Phase 3.2) lands
//! `run_stake_address_key_gen_cmd` + `run_stake_address_build_cmd`,
//! mirroring upstream `runStakeAddressKeyGenCmd` /
//! `runStakeAddressBuildCmd`. Remaining stake-address subcommands
//! (`stake-address registration-certificate`, …) port over
//! subsequent rounds.

use std::path::Path;

use eyre::{Result, WrapErr};

use crate::era_independent::address::run::{
    KeyKind, generate_keypair_to_envelopes, read_verification_key_text_envelope,
};

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

/// Run `stake-address build` — construct a Shelley reward (stake)
/// address and print (or write) its Bech32 encoding.
///
/// Mirrors upstream `runStakeAddressBuildCmd` from
/// `Cardano.CLI.EraBased.StakeAddress.Run`. Hashes the stake VK with
/// Blake2b-224, assembles the reward-address bytes, and Bech32-encodes
/// them. Network selection is `--mainnet` (id 1) xor `--testnet-magic`
/// (id 0); the node binary's `cardano-cli stake-address-build`
/// wrapper is the parity reference.
pub fn run_stake_address_build_cmd(
    stake_verification_key_file: &Path,
    mainnet: bool,
    testnet_magic: Option<u32>,
    out_file: Option<&Path>,
) -> Result<()> {
    let network_id: u8 = if mainnet {
        1
    } else if testnet_magic.is_some() {
        0
    } else {
        eyre::bail!("stake-address build requires either --mainnet or --testnet-magic");
    };

    let env = std::fs::read(stake_verification_key_file).wrap_err_with(|| {
        format!(
            "failed to read --stake-verification-key-file {}",
            stake_verification_key_file.display()
        )
    })?;
    let stake_vk = read_verification_key_text_envelope(&env)?;
    let stake_hash = yggdrasil_crypto::hash_bytes_224(&stake_vk).0;

    let bech32_addr = build_shelley_reward_address_bech32(network_id, &stake_hash)?;
    match out_file {
        Some(path) => std::fs::write(path, format!("{bech32_addr}\n"))
            .wrap_err_with(|| format!("failed to write --out-file {}", path.display()))?,
        None => println!("{bech32_addr}"),
    }
    Ok(())
}

/// Assemble a Shelley reward (stake) address byte sequence and
/// Bech32-encode it.
///
/// Per `Cardano.Ledger.Address.RewardAccount`: header byte
/// `0b1110_<netid>` (address type 14 — the standard key-based
/// reward address) + 28-byte stake-key hash = 29 raw bytes.
/// Script-based reward addresses (type 15) are not yet supported.
/// The Bech32 HRP is `stake` (mainnet) or `stake_test` (testnet).
fn build_shelley_reward_address_bech32(network_id: u8, stake_hash: &[u8; 28]) -> Result<String> {
    if network_id > 0x0F {
        eyre::bail!("network_id {network_id} must fit in 4 bits (0..=15)");
    }
    let mut addr_bytes: Vec<u8> = Vec::with_capacity(29);
    addr_bytes.push(0xE0 | network_id);
    addr_bytes.extend_from_slice(stake_hash);
    let hrp_str = if network_id == 1 {
        "stake"
    } else {
        "stake_test"
    };
    let hrp = bech32::Hrp::parse(hrp_str)
        .map_err(|e| eyre::eyre!("bech32 HRP parse failed for {hrp_str:?}: {e}"))?;
    bech32::encode::<bech32::Bech32>(hrp, &addr_bytes)
        .map_err(|e| eyre::eyre!("bech32 encode failed: {e}"))
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

    /// A reward address is 29 raw bytes — header `0xE0 | netid` +
    /// 28-byte stake hash — and Bech32-encodes with the `stake` HRP
    /// on mainnet, `stake_test` on testnet.
    #[test]
    fn reward_address_header_and_hrp() {
        let stake = [0x44_u8; 28];
        let mainnet = build_shelley_reward_address_bech32(1, &stake).expect("mainnet reward");
        let testnet = build_shelley_reward_address_bech32(0, &stake).expect("testnet reward");
        assert!(
            mainnet.starts_with("stake1"),
            "mainnet reward address must use the `stake` HRP; got {mainnet}"
        );
        assert!(
            testnet.starts_with("stake_test1"),
            "testnet reward address must use the `stake_test` HRP; got {testnet}"
        );
        assert_ne!(mainnet, testnet);
    }

    /// `run_stake_address_build_cmd` reads a generated stake vkey and
    /// writes the reward address to `--out-file`.
    #[test]
    fn stake_address_build_writes_out_file() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-sab-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let vk = dir.join("stake.vkey");
        run_stake_address_key_gen_cmd(&vk, &dir.join("stake.skey")).expect("stake key-gen");
        let out = dir.join("reward.addr");
        run_stake_address_build_cmd(&vk, true, None, Some(&out)).expect("stake-address build");
        let written = std::fs::read_to_string(&out).expect("read out-file");
        assert!(
            written.trim_end().starts_with("stake1"),
            "out-file must hold a mainnet `stake1…` reward address; got {written:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `run_stake_address_build_cmd` with neither network flag bails.
    #[test]
    fn stake_address_build_requires_a_network_flag() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-sab-nf-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let vk = dir.join("stake.vkey");
        run_stake_address_key_gen_cmd(&vk, &dir.join("stake.skey")).expect("stake key-gen");
        let err = run_stake_address_build_cmd(&vk, false, None, None)
            .expect_err("no network flag must bail");
        assert!(
            err.to_string().contains("--mainnet or --testnet-magic"),
            "error must name both network flags; got {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
