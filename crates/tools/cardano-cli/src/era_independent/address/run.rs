//! EraIndependent address run.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Address/Run.hs`.
//! R293 landed the file as an API skeleton. R507–R508 (Phase 3.2)
//! land `run_address_key_gen_cmd` / `run_address_key_hash_cmd` /
//! `run_address_build_cmd`, mirroring upstream `runAddressKeyGenCmd`
//! / `runAddressKeyHashCmd` / `runAddressBuildCmd`. Remaining
//! EraIndependent address subcommands (`address info`) port over
//! subsequent rounds.

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

/// Run `address key-hash` — print the Blake2b-224 hash of a
/// verification key as lowercase hex.
///
/// Mirrors upstream `runAddressKeyHashCmd` from
/// `Cardano.CLI.EraIndependent.Address.Run`. Yggdrasil's surface
/// (the node binary's `cardano-cli address-key-hash` wrapper is the
/// parity reference) reads the VK from a TextEnvelope file and
/// prints the 56-hex-char hash to stdout — no `--out-file` selector.
/// Both `Payment…` and `Stake…` verification-key envelopes are
/// accepted: the wire shape is identical (32-byte VK).
pub fn run_address_key_hash_cmd(payment_verification_key_file: &Path) -> Result<()> {
    let envelope_bytes = std::fs::read(payment_verification_key_file).wrap_err_with(|| {
        format!(
            "failed to read --payment-verification-key-file {}",
            payment_verification_key_file.display()
        )
    })?;
    let key_bytes = read_verification_key_text_envelope(&envelope_bytes)?;
    let hash = yggdrasil_crypto::hash_bytes_224(&key_bytes);
    // Upstream prints lowercase hex, no `0x` prefix (28 bytes = 56 chars).
    println!("{}", hex::encode(hash.0));
    Ok(())
}

/// Run `address build` — construct a Shelley payment address and
/// print (or write) its Bech32 encoding.
///
/// Mirrors upstream `runAddressBuildCmd` from
/// `Cardano.CLI.EraIndependent.Address.Run`. Hashes the payment VK
/// (and the optional stake VK) with Blake2b-224, assembles the
/// Shelley address bytes, and Bech32-encodes them. Network selection
/// is `--mainnet` (id 1) xor `--testnet-magic` (id 0); the node
/// binary's `cardano-cli address-build` wrapper is the parity
/// reference.
pub fn run_address_build_cmd(
    payment_verification_key_file: &Path,
    stake_verification_key_file: Option<&Path>,
    mainnet: bool,
    testnet_magic: Option<u32>,
    out_file: Option<&Path>,
) -> Result<()> {
    let network_id: u8 = if mainnet {
        1
    } else if testnet_magic.is_some() {
        0
    } else {
        eyre::bail!("address build requires either --mainnet or --testnet-magic");
    };

    let pay_env = std::fs::read(payment_verification_key_file).wrap_err_with(|| {
        format!(
            "failed to read --payment-verification-key-file {}",
            payment_verification_key_file.display()
        )
    })?;
    let pay_vk = read_verification_key_text_envelope(&pay_env)?;
    let pay_hash = yggdrasil_crypto::hash_bytes_224(&pay_vk).0;

    let stake_hash: Option<[u8; 28]> = match stake_verification_key_file {
        Some(p) => {
            let env = std::fs::read(p).wrap_err_with(|| {
                format!(
                    "failed to read --stake-verification-key-file {}",
                    p.display()
                )
            })?;
            let vk = read_verification_key_text_envelope(&env)?;
            Some(yggdrasil_crypto::hash_bytes_224(&vk).0)
        }
        None => None,
    };

    let bech32_addr = build_shelley_address_bech32(network_id, &pay_hash, stake_hash.as_ref())?;
    match out_file {
        Some(path) => std::fs::write(path, format!("{bech32_addr}\n"))
            .wrap_err_with(|| format!("failed to write --out-file {}", path.display()))?,
        None => println!("{bech32_addr}"),
    }
    Ok(())
}

/// Assemble a Shelley address byte sequence and Bech32-encode it.
///
/// Two cases, per `Cardano.Ledger.Address`:
///
/// - Enterprise (`stake_hash == None`): header `0b0110_<netid>`
///   (address type 6 = key-payment, no stake) + 28-byte payment
///   hash = 29 raw bytes.
/// - Base (`stake_hash == Some(h)`): header `0b0000_<netid>`
///   (type 0 = key-payment, key-stake) + 28-byte payment hash +
///   28-byte stake hash = 57 raw bytes.
///
/// The Bech32 HRP is `addr` (mainnet, `network_id == 1`) or
/// `addr_test` (any other network) per
/// `Cardano.Ledger.Address.serialiseAddrBech32`.
fn build_shelley_address_bech32(
    network_id: u8,
    payment_hash: &[u8; 28],
    stake_hash: Option<&[u8; 28]>,
) -> Result<String> {
    if network_id > 0x0F {
        eyre::bail!("network_id {network_id} must fit in 4 bits (0..=15)");
    }
    let header = match stake_hash {
        Some(_) => network_id,     // base address (type 0)
        None => 0x60 | network_id, // enterprise address (type 6)
    };
    let mut addr_bytes: Vec<u8> = Vec::with_capacity(57);
    addr_bytes.push(header);
    addr_bytes.extend_from_slice(payment_hash);
    if let Some(sh) = stake_hash {
        addr_bytes.extend_from_slice(sh);
    }
    let hrp_str = if network_id == 1 { "addr" } else { "addr_test" };
    let hrp = bech32::Hrp::parse(hrp_str)
        .map_err(|e| eyre::eyre!("bech32 HRP parse failed for {hrp_str:?}: {e}"))?;
    bech32::encode::<bech32::Bech32>(hrp, &addr_bytes)
        .map_err(|e| eyre::eyre!("bech32 encode failed: {e}"))
}

/// Decode a 32-byte verification key out of a TextEnvelope JSON
/// blob (`{type, description, cborHex}`).
///
/// The `cborHex` field must be exactly 34 bytes: the 2-byte CBOR
/// bytes-string-of-length-32 prefix `0x5820` followed by the
/// 32-byte key. Mirrors the inverse of [`write_text_envelope`].
fn read_verification_key_text_envelope(envelope_bytes: &[u8]) -> Result<[u8; 32]> {
    let envelope: serde_json::Value = serde_json::from_slice(envelope_bytes)
        .map_err(|e| eyre::eyre!("TextEnvelope is not valid JSON: {e}"))?;
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
    // CBOR bytes-string of length 32 = major-type-2 | length-32 = 0x58 0x20.
    if cbor_bytes[0] != 0x58 || cbor_bytes[1] != 0x20 {
        eyre::bail!(
            "expected CBOR prefix 0x5820 (bytes-string of length 32), got 0x{:02x}{:02x}",
            cbor_bytes[0],
            cbor_bytes[1]
        );
    }
    let mut out = [0_u8; 32];
    out.copy_from_slice(&cbor_bytes[2..]);
    Ok(out)
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

    /// `read_verification_key_text_envelope` round-trips a key that
    /// `write_text_envelope` produced — the 32 key bytes survive the
    /// `5820`-prefixed CBOR envelope and back.
    #[test]
    fn verification_key_envelope_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-vk-rt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let key = [0x42_u8; 32];
        let path = dir.join("k.vkey");
        write_text_envelope(
            &path,
            "PaymentVerificationKeyShelley_ed25519",
            "Payment Verification Key",
            &key,
            false,
        )
        .expect("write envelope");
        let decoded =
            read_verification_key_text_envelope(&std::fs::read(&path).expect("read envelope"))
                .expect("decode envelope");
        assert_eq!(
            decoded, key,
            "32 key bytes must survive the envelope round-trip"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `read_verification_key_text_envelope` rejects a `cborHex`
    /// whose decoded length is not the required 34 bytes.
    #[test]
    fn verification_key_envelope_rejects_wrong_length() {
        // `cborHex` = "5820" + only 4 hex (2 bytes) of payload → 4 bytes total.
        let bad = br#"{"type":"x","description":"y","cborHex":"5820abcd"}"#;
        let err = read_verification_key_text_envelope(bad).expect_err("short cborHex must bail");
        assert!(
            err.to_string().contains("expected 34 bytes"),
            "error must explain the length requirement; got {err}"
        );
    }

    /// `run_address_key_hash_cmd` succeeds on a freshly-generated
    /// verification key — confirms the key-gen → key-hash pipeline
    /// composes end-to-end.
    #[test]
    fn key_hash_succeeds_on_generated_vkey() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-kh-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let vk_path = dir.join("p.vkey");
        run_address_key_gen_cmd(&vk_path, &dir.join("p.skey")).expect("key-gen");
        run_address_key_hash_cmd(&vk_path).expect("key-hash must succeed on a valid vkey");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An enterprise address (no stake key) is 29 raw bytes — header
    /// `0x60 | netid` + 28-byte payment hash — and Bech32-encodes
    /// with the `addr` HRP on mainnet, `addr_test` on testnet.
    #[test]
    fn enterprise_address_header_and_hrp() {
        let pay = [0x11_u8; 28];
        let mainnet = build_shelley_address_bech32(1, &pay, None).expect("mainnet enterprise");
        let testnet = build_shelley_address_bech32(0, &pay, None).expect("testnet enterprise");
        assert!(
            mainnet.starts_with("addr1"),
            "mainnet enterprise address must use the `addr` HRP; got {mainnet}"
        );
        assert!(
            testnet.starts_with("addr_test1"),
            "testnet enterprise address must use the `addr_test` HRP; got {testnet}"
        );
        // Mainnet and testnet differ only in the header's low nibble,
        // so the encodings must not collide.
        assert_ne!(mainnet, testnet);
    }

    /// A base address (payment + stake) is 57 raw bytes — header
    /// `0x00 | netid` + payment hash + stake hash — and is distinct
    /// from the enterprise address built from the same payment key.
    #[test]
    fn base_address_differs_from_enterprise() {
        let pay = [0x22_u8; 28];
        let stake = [0x33_u8; 28];
        let base = build_shelley_address_bech32(1, &pay, Some(&stake)).expect("base address");
        let enterprise = build_shelley_address_bech32(1, &pay, None).expect("enterprise address");
        assert!(base.starts_with("addr1"));
        assert_ne!(
            base, enterprise,
            "a base address must differ from the enterprise address with the same payment key"
        );
    }

    /// `run_address_build_cmd` with neither `--mainnet` nor
    /// `--testnet-magic` bails with the documented message.
    #[test]
    fn address_build_requires_a_network_flag() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-ab-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let vk = dir.join("p.vkey");
        run_address_key_gen_cmd(&vk, &dir.join("p.skey")).expect("key-gen");
        let err = run_address_build_cmd(&vk, None, false, None, None)
            .expect_err("no network flag must bail");
        assert!(
            err.to_string().contains("--mainnet or --testnet-magic"),
            "error must name both network flags; got {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `run_address_build_cmd` writes the Bech32 address to
    /// `--out-file` when one is supplied.
    #[test]
    fn address_build_writes_out_file() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-cli-abo-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let vk = dir.join("p.vkey");
        run_address_key_gen_cmd(&vk, &dir.join("p.skey")).expect("key-gen");
        let out = dir.join("addr.txt");
        run_address_build_cmd(&vk, None, true, None, Some(&out)).expect("address build");
        let written = std::fs::read_to_string(&out).expect("read out-file");
        assert!(
            written.trim_end().starts_with("addr1"),
            "out-file must hold a mainnet `addr1…` address; got {written:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
