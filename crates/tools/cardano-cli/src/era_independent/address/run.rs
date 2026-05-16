//! EraIndependent address run.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Address/Run.hs`.
//! R293 landed the file as an API skeleton. R507 (Phase 3.2) lands
//! the first concrete subcommand — `run_address_key_gen_cmd`,
//! mirroring upstream `runAddressKeyGenCmd`. Remaining
//! EraIndependent address subcommands (`address build`,
//! `address key-hash`, `address info`) port over subsequent rounds.

use std::path::Path;

use eyre::{Result, WrapErr};

/// Run `address key-gen` — generate a fresh Ed25519 payment keypair
/// and write both keys as TextEnvelope JSON files.
///
/// Mirrors upstream `runAddressKeyGenCmd` from
/// `Cardano.CLI.EraIndependent.Address.Run`. Yggdrasil's surface is
/// the simplified payment-key form (the node binary's
/// `cardano-cli address-key-gen` wrapper is the parity reference):
/// no `--key-output-format` selector, key type fixed to the normal
/// (non-extended) Ed25519 form.
///
/// The signing-key file is written `0o600` on Unix so the private
/// key is not world-readable; the verification-key file uses the
/// default mode so it can be shared / checked in freely.
pub fn run_address_key_gen_cmd(
    verification_key_file: &Path,
    signing_key_file: &Path,
) -> Result<()> {
    generate_keypair_to_envelopes(verification_key_file, signing_key_file, KeyKind::Payment)
}

/// Kind of key being generated — selects the TextEnvelope `type` +
/// `description` metadata. The on-wire bytes are identical for both
/// kinds (32-byte Ed25519 SK / VK); only the metadata changes so
/// upstream `cardano-cli` can tell payment from stake at file-load
/// time.
///
/// `Stake` is unused by `address key-gen` itself but kept here so
/// the forthcoming `stake-address key-gen` port (a sibling round)
/// reuses [`generate_keypair_to_envelopes`] verbatim.
#[derive(Clone, Copy)]
pub enum KeyKind {
    /// Payment credential keypair.
    Payment,
    /// Stake (delegation / reward-account) credential keypair.
    Stake,
}

impl KeyKind {
    fn signing_envelope_type(self) -> &'static str {
        match self {
            KeyKind::Payment => "PaymentSigningKeyShelley_ed25519",
            KeyKind::Stake => "StakeSigningKeyShelley_ed25519",
        }
    }
    fn signing_description(self) -> &'static str {
        match self {
            KeyKind::Payment => "Payment Signing Key",
            KeyKind::Stake => "Stake Signing Key",
        }
    }
    fn verification_envelope_type(self) -> &'static str {
        match self {
            KeyKind::Payment => "PaymentVerificationKeyShelley_ed25519",
            KeyKind::Stake => "StakeVerificationKeyShelley_ed25519",
        }
    }
    fn verification_description(self) -> &'static str {
        match self {
            KeyKind::Payment => "Payment Verification Key",
            KeyKind::Stake => "Stake Verification Key",
        }
    }
}

/// Shared keypair generator: read 32 bytes of OS entropy, derive the
/// VK, and write both TextEnvelope files with the metadata for
/// `kind`. Used by `address key-gen` and (forthcoming)
/// `stake-address key-gen`.
pub fn generate_keypair_to_envelopes(
    verification_key_file: &Path,
    signing_key_file: &Path,
    kind: KeyKind,
) -> Result<()> {
    let seed = read_os_entropy_32_bytes()?;
    let sk = yggdrasil_crypto::SigningKey::from_bytes(seed);
    let vk = sk
        .verification_key()
        .map_err(|e| eyre::eyre!("failed to derive VK from generated SK: {e}"))?;
    write_text_envelope(
        signing_key_file,
        kind.signing_envelope_type(),
        kind.signing_description(),
        &sk.to_bytes(),
        /* private = */ true,
    )?;
    write_text_envelope(
        verification_key_file,
        kind.verification_envelope_type(),
        kind.verification_description(),
        &vk.to_bytes(),
        /* private = */ false,
    )?;
    Ok(())
}

/// Read 32 cryptographically-secure random bytes from the OS.
///
/// Uses `/dev/urandom` directly rather than pulling a `getrandom` /
/// `rand` dep into the cardano-cli crate — every supported Yggdrasil
/// platform provides the kernel entropy device. Non-Unix errors out
/// cleanly rather than silently downgrading.
fn read_os_entropy_32_bytes() -> Result<[u8; 32]> {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut buf = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .wrap_err("open /dev/urandom failed")?
            .read_exact(&mut buf)
            .wrap_err("read 32 bytes from /dev/urandom failed")?;
        Ok(buf)
    }
    #[cfg(not(unix))]
    {
        eyre::bail!(
            "address key-gen needs /dev/urandom for entropy; not supported on this platform"
        )
    }
}

/// Write a TextEnvelope JSON file (`{type, description, cborHex}`)
/// matching upstream `cardano-cli`'s output shape for a 32-byte key.
///
/// `cborHex` is `5820 || payload` — a CBOR major-type-2 byte string
/// of length 32. When `private = true` on Unix the file is created
/// `0o600` so the signing key is not world-readable.
fn write_text_envelope(
    path: &Path,
    envelope_type: &str,
    description: &str,
    payload: &[u8; 32],
    private: bool,
) -> Result<()> {
    let mut cbor = Vec::with_capacity(34);
    cbor.push(0x58);
    cbor.push(0x20);
    cbor.extend_from_slice(payload);
    let envelope = serde_json::json!({
        "type": envelope_type,
        "description": description,
        "cborHex": hex::encode(&cbor),
    });
    let json =
        serde_json::to_string_pretty(&envelope).wrap_err("failed to serialise TextEnvelope")?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mode = if private { 0o600 } else { 0o644 };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(mode)
            .open(path)
            .wrap_err_with(|| format!("open {} failed", path.display()))?;
        f.write_all(json.as_bytes())
            .wrap_err_with(|| format!("write {} failed", path.display()))?;
        f.write_all(b"\n")
            .wrap_err_with(|| format!("write {} trailing newline failed", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = private;
        std::fs::write(path, json + "\n")
            .wrap_err_with(|| format!("write {} failed", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `run_address_key_gen_cmd` produces two well-formed TextEnvelope
    /// files: the SK typed `PaymentSigningKeyShelley_ed25519` and the
    /// VK typed `PaymentVerificationKeyShelley_ed25519`, each with a
    /// 68-hex-char `cborHex` (`5820` prefix + 64 hex of the 32-byte
    /// key).
    #[test]
    fn key_gen_writes_two_payment_text_envelopes() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-keygen-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let vk_path = dir.join("payment.vkey");
        let sk_path = dir.join("payment.skey");

        run_address_key_gen_cmd(&vk_path, &sk_path).expect("key-gen must succeed");

        let vk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&vk_path).expect("read vkey"))
                .expect("vkey is JSON");
        let sk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&sk_path).expect("read skey"))
                .expect("skey is JSON");

        assert_eq!(vk["type"], "PaymentVerificationKeyShelley_ed25519");
        assert_eq!(sk["type"], "PaymentSigningKeyShelley_ed25519");
        assert_eq!(vk["description"], "Payment Verification Key");
        assert_eq!(sk["description"], "Payment Signing Key");
        // `5820` (2 bytes) + 32-byte key = 34 bytes = 68 hex chars.
        assert_eq!(vk["cborHex"].as_str().expect("vk cborHex string").len(), 68);
        assert_eq!(sk["cborHex"].as_str().expect("sk cborHex string").len(), 68);
        assert!(
            vk["cborHex"]
                .as_str()
                .expect("vk cborHex")
                .starts_with("5820"),
            "cborHex must carry the CBOR bytes-32 prefix 5820"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two successive `key-gen` runs produce different keys —
    /// confirms real entropy is consumed, not a fixed seed.
    #[test]
    fn key_gen_is_non_deterministic() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-keygen-nd-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        run_address_key_gen_cmd(&dir.join("a.vkey"), &dir.join("a.skey")).expect("key-gen a");
        run_address_key_gen_cmd(&dir.join("b.vkey"), &dir.join("b.skey")).expect("key-gen b");
        let a = std::fs::read_to_string(dir.join("a.skey")).expect("read a");
        let b = std::fs::read_to_string(dir.join("b.skey")).expect("read b");
        assert_ne!(a, b, "two key-gen runs must produce different signing keys");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
