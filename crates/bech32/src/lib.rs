//! Pure-Rust port of upstream `Codec.Binary.Bech32` — BIP-0173
//! Bech32 + BIP-0350 Bech32m address-format encoding and decoding.
//!
//! Foundation: `bech32` v0.11+ (rust-bitcoin/rust-bech32, MIT,
//! workspace dep at R330) + `bs58` v0.5+ (Nullus157/bs58-rs,
//! MIT/Apache-2.0, workspace dep at R333) + `hex` v0.4 (workspace).
//!
//! ## Naming parity
//!
//! **Strict mirror:** Codec/Binary/Bech32.hs. The Rust crate root
//! (`lib.rs`) is the canonical 1:1 mirror of upstream
//! `bech32/bech32/src/Codec/Binary/Bech32.hs` — the public library
//! API surface. The internal helper module lives in `internal.rs`
//! (mirroring upstream `Codec/Binary/Bech32/Internal.hs`); the
//! binary entry point lives in `main.rs` (mirroring upstream
//! `bech32/bech32/app/Main.hs`); the CLI parser shell lives in
//! `parser.rs` (Yggdrasil-side synthesis with byte-equivalent
//! `--help` / `--version` fixture pinning).
//!
//! Upstream's `bech32-th/src/Codec/Binary/Bech32/TH.hs` (Template
//! Haskell helpers) has no Rust analog — Rust uses `macro_rules!`
//! and proc-macros directly. No corresponding `crates/bech32/src/th.rs`
//! is needed; the strict-mirror policy supports this absence per the
//! `Setup.hs` / `Orphans.hs` precedents.

use std::io::{Read, Write};

pub mod internal;
pub mod parser;

// ---------------------------------------------------------------------------
// Public API surface — strict mirror of upstream `Codec.Binary.Bech32`
// exports. The R331 placeholder types (DataPart/HumanReadablePart/
// EncodingError/DecodingError) are now backed by the rust-bitcoin/rust-bech32
// types. Yggdrasil-side wrappers preserve the upstream symbol names for
// downstream consumer compatibility.
// ---------------------------------------------------------------------------

/// Human-readable part of a Bech32 string (the prefix before the `1`
/// separator). Wraps `bech32::Hrp` for upstream-compatible naming.
///
/// Upstream: `Codec.Binary.Bech32::HumanReadablePart`.
pub type HumanReadablePart = bech32::Hrp;

/// Data payload of a Bech32 string (the bytes encoded after the `1`
/// separator). Yggdrasil represents this as a raw byte buffer; the
/// 5-bit-word interface upstream exposes via `Word5` is internal to
/// the bech32 crate's encoder.
///
/// Upstream: `Codec.Binary.Bech32::DataPart`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataPart {
    /// Raw decoded bytes (post-checksum-strip).
    pub bytes: Vec<u8>,
}

impl DataPart {
    /// Build a `DataPart` from raw bytes.
    ///
    /// Upstream: `Codec.Binary.Bech32::dataPartFromBytes`.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Extract the raw bytes from a `DataPart`.
    ///
    /// Upstream: `Codec.Binary.Bech32::dataPartToBytes`.
    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Top-level error type for Bech32 binary operations.
///
/// Upstream: combines `Codec.Binary.Bech32::EncodingError`,
/// `DecodingError`, and `HumanReadablePartError` into a single
/// surface for the CLI binary's error path.
#[derive(Debug, thiserror::Error)]
pub enum Bech32Error {
    /// HRP failed to parse (invalid characters, too long, too short).
    #[error("invalid prefix: {0}")]
    InvalidPrefix(String),

    /// Bech32 / Bech32m string failed to decode (bad checksum, invalid
    /// characters, separator missing).
    #[error("invalid bech32 string: {0}")]
    InvalidBech32(String),

    /// Failed to encode bytes into a Bech32 string (typically because
    /// the encoded result exceeds the protocol's length cap).
    #[error("bech32 encoding failed: {0}")]
    EncodeFailed(String),

    /// Input string didn't match any of the supported encodings
    /// (Base16 / Bech32 / Base58).
    #[error("Unable to detect input encoding. Neither Base16, Bech32 nor Base58.")]
    UnknownEncoding,

    /// Input is too short for the encoding-detection heuristic to be
    /// reliable. Mirrors upstream's `StringToDecodeTooShort` error
    /// emitted when stdin is empty / whitespace-only.
    #[error("StringToDecodeTooShort")]
    StringToDecodeTooShort,

    /// Standard I/O failure (stdin/stdout).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Hex-decode failure for Base16 input path.
    #[error("invalid base16: {0}")]
    Base16(#[from] hex::FromHexError),

    /// Base58-decode failure for the Bitcoin-alphabet input path.
    #[error("invalid base58: {0}")]
    Base58(String),
}

/// Detected input encoding for the encode path.
///
/// Upstream: `Encoding` ADT in `bech32/app/Main.hs` (Base16, Bech32,
/// Base58).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEncoding {
    /// Hex / base16.
    Base16,
    /// Bech32 / Bech32m.
    Bech32,
    /// Base58 with the Bitcoin alphabet (Cardano legacy Byron path).
    Base58,
}

/// Minimum string length below which encoding-detection is
/// unreliable. Mirrors upstream's `minimalSizeForDetection = 8`.
pub const MINIMAL_SIZE_FOR_DETECTION: usize = 8;

/// Bech32 5-bit-word data character set (BIP-0173):
/// `qpzry9x8gf2tvdw0s3jn54khce6mua7l`. Re-exported from
/// [`internal::CHARSET`] for upstream-compatible naming.
///
/// Upstream: `Codec.Binary.Bech32.Internal::charset`.
pub const DATA_CHAR_LIST: &str = internal::CHARSET;

/// Bech32 separator character (`'1'`). Mirrors upstream's
/// `Codec.Binary.Bech32.Internal::separatorChar`.
pub const SEPARATOR_CHAR: char = '1';

/// Bech32 checksum length in characters (6). Mirrors upstream's
/// `Codec.Binary.Bech32.Internal::checksumLength`.
pub const CHECKSUM_LENGTH: usize = 6;

// ---------------------------------------------------------------------------
// CLI dispatch — strict mirror of upstream `bech32/app/Main.hs::run`.
// ---------------------------------------------------------------------------

/// Legacy run-loop entry. Reads stdin, dispatches via [`run_with`].
pub fn run() -> eyre::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parser::parse_args(&argv).map_err(|e| eyre::eyre!("{e}"))?;
    run_with(args)
}

/// Concrete run-loop entry called by `main` after argument parsing.
///
/// Mirrors upstream `bech32/app/Main.hs::run`: reads stdin, trims
/// whitespace, then either decodes (no prefix) or encodes (with
/// prefix) the input.
pub fn run_with(args: parser::Args) -> eyre::Result<()> {
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    let source = stdin.trim();

    if source.is_empty() {
        return Err(Bech32Error::StringToDecodeTooShort.into());
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match args.prefix {
        None => {
            let decoded = run_decode(source)?;
            writeln!(out, "{decoded}")?;
        }
        Some(prefix) => {
            let encoded = run_encode(&prefix, source)?;
            writeln!(out, "{encoded}")?;
        }
    }
    Ok(())
}

/// Decode a Bech32-encoded `source` into hex.
///
/// Mirrors upstream `bech32/app/Main.hs::runDecode`: input is
/// `Bech32.decodeLenient`'d and the data part is hex-encoded for
/// stdout.
pub fn run_decode(source: &str) -> Result<String, Bech32Error> {
    let (_hrp, bytes) =
        bech32::decode(source).map_err(|e| Bech32Error::InvalidBech32(format!("{e}")))?;
    Ok(hex::encode(bytes))
}

/// Encode a `source` (auto-detected as Base16 / Bech32 / Base58)
/// under the given `prefix` HRP into a Bech32 string.
///
/// Mirrors upstream `bech32/app/Main.hs::runEncode`: detects input
/// encoding, decodes to raw bytes, then re-encodes via
/// `Bech32.encodeLenient` with the supplied HRP.
pub fn run_encode(prefix: &str, source: &str) -> Result<String, Bech32Error> {
    let hrp =
        HumanReadablePart::parse(prefix).map_err(|e| Bech32Error::InvalidPrefix(format!("{e}")))?;

    let bytes: Vec<u8> = match detect_encoding(source) {
        Some(InputEncoding::Base16) => hex::decode(source)?,
        Some(InputEncoding::Bech32) => {
            let (_hrp, b) =
                bech32::decode(source).map_err(|e| Bech32Error::InvalidBech32(format!("{e}")))?;
            b
        }
        Some(InputEncoding::Base58) => bs58::decode(source)
            .with_alphabet(bs58::Alphabet::BITCOIN)
            .into_vec()
            .map_err(|e| Bech32Error::Base58(format!("{e}")))?,
        None => return Err(Bech32Error::UnknownEncoding),
    };

    bech32::encode::<bech32::Bech32>(hrp, &bytes)
        .map_err(|e| Bech32Error::EncodeFailed(format!("{e}")))
}

/// Try detecting the encoding of a given string.
///
/// Mirrors upstream `bech32/app/Main.hs::detectEncoding`. Returns
/// `None` if the string is shorter than [`MINIMAL_SIZE_FOR_DETECTION`]
/// (8 chars) or doesn't match any of the three supported encodings.
///
/// Detection order: Base16 → Bech32 → Base58.
pub fn detect_encoding(s: &str) -> Option<InputEncoding> {
    if s.len() < MINIMAL_SIZE_FOR_DETECTION {
        return None;
    }
    if resemble_base16(s) {
        return Some(InputEncoding::Base16);
    }
    if resemble_bech32(s) {
        return Some(InputEncoding::Bech32);
    }
    if resemble_base58(s) {
        return Some(InputEncoding::Base58);
    }
    None
}

/// Return true if `s` resembles a Base16 string (all hex chars + even
/// length).
fn resemble_base16(s: &str) -> bool {
    s.len().is_multiple_of(2)
        && s.chars()
            .all(|c| c.to_ascii_lowercase().is_ascii_hexdigit())
}

/// Return true if `s` resembles a Bech32 string per upstream's
/// detection heuristic: contains a `1` separator, non-empty HRP,
/// data part >= checksum length, all data chars are valid bech32,
/// and consistent letter case (all upper or all lower).
fn resemble_bech32(s: &str) -> bool {
    if !s.contains(SEPARATOR_CHAR) {
        return false;
    }
    // Split on the LAST separator (since HRP can contain '1' after
    // the prefix; upstream uses `reverse . takeWhile (/= sep) . reverse`).
    let last_sep = s.rfind(SEPARATOR_CHAR).expect("contains sep");
    let humanpart = &s[..last_sep];
    let datapart = &s[last_sep + 1..];

    if humanpart.is_empty() {
        return false;
    }
    if !humanpart.chars().all(human_readable_char_is_valid) {
        return false;
    }
    if datapart.len() < CHECKSUM_LENGTH {
        return false;
    }
    if !datapart.chars().all(is_data_char) {
        return false;
    }

    // Letter-case consistency over the WHOLE string.
    let alpha: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    let all_upper = alpha.iter().all(|c| c.is_uppercase());
    let all_lower = alpha.iter().all(|c| c.is_lowercase());
    all_upper || all_lower
}

/// Return true if `s` resembles a Base58 (Bitcoin alphabet) string:
/// every character is in the alphabet.
fn resemble_base58(s: &str) -> bool {
    s.chars().all(is_base58_bitcoin_digit)
}

/// Test whether `c` is a valid character in the human-readable part
/// of a Bech32 string. Mirrors upstream
/// `Codec.Binary.Bech32.Internal::humanReadableCharIsValid`:
/// printable ASCII range 33..=126.
pub fn human_readable_char_is_valid(c: char) -> bool {
    let code = c as u32;
    (33..=126).contains(&code)
}

/// Test whether `c` is in the bech32 data character set
/// `qpzry9x8gf2tvdw0s3jn54khce6mua7l` (case-insensitive).
fn is_data_char(c: char) -> bool {
    DATA_CHAR_LIST.contains(c.to_ascii_lowercase())
}

/// Test whether `c` is a Bitcoin Base58 alphabet character.
///
/// Bitcoin alphabet:
/// `123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz`
/// (no `0`, `O`, `I`, `l` to avoid visually-confusable digits).
fn is_base58_bitcoin_digit(c: char) -> bool {
    const BITCOIN: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    BITCOIN.contains(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the upstream-help-text examples as round-trip tests.
    /// These are documented in the help output and form the primary
    /// canonical fixture.
    #[test]
    fn upstream_example_base16_to_bech32() {
        // From bech32 --help: "$ bech32 base16_ <<< 706174617465"
        // expected stdout: "base16_1wpshgct5v5r5mxh0"
        assert_eq!(
            run_encode("base16_", "706174617465").expect("encode"),
            "base16_1wpshgct5v5r5mxh0",
        );
    }

    #[test]
    fn upstream_example_base58_to_bech32() {
        // From bech32 --help: "$ bech32 base58_ <<< Ae2tdPwUPEYy"
        // expected stdout: "base58_1p58rejhd9592uusa8pzj2"
        assert_eq!(
            run_encode("base58_", "Ae2tdPwUPEYy").expect("encode"),
            "base58_1p58rejhd9592uusa8pzj2",
        );
    }

    #[test]
    fn upstream_example_reencode_to_new_prefix() {
        // From bech32 --help: "$ bech32 new_prefix <<< old_prefix1wpshgcg2s33x3"
        // expected stdout: "new_prefix1wpshgcgeak9mv"
        assert_eq!(
            run_encode("new_prefix", "old_prefix1wpshgcg2s33x3").expect("encode"),
            "new_prefix1wpshgcgeak9mv",
        );
    }

    #[test]
    fn upstream_example_bech32_to_base16() {
        // From bech32 --help: "$ bech32 <<< base16_1wpshgct5v5r5mxh0"
        // expected stdout: "706174617465"
        assert_eq!(
            run_decode("base16_1wpshgct5v5r5mxh0").expect("decode"),
            "706174617465",
        );
    }

    #[test]
    fn detect_base16() {
        assert_eq!(detect_encoding("706174617465"), Some(InputEncoding::Base16),);
    }

    #[test]
    fn detect_bech32() {
        assert_eq!(
            detect_encoding("base16_1wpshgct5v5r5mxh0"),
            Some(InputEncoding::Bech32),
        );
    }

    #[test]
    fn detect_base58() {
        // 12-char Bitcoin alphabet string that's not valid base16
        // (contains 'A' but mixed case so not bech32-resembling).
        assert_eq!(detect_encoding("Ae2tdPwUPEYy"), Some(InputEncoding::Base58));
    }

    #[test]
    fn rejects_too_short_input() {
        assert_eq!(detect_encoding("abc"), None);
        assert_eq!(detect_encoding("1234567"), None);
    }

    #[test]
    fn round_trip_via_bech32() {
        // Encode then decode should give back the original bytes.
        let original = b"hello, world";
        let hex_input = hex::encode(original);
        let encoded = run_encode("test", &hex_input).expect("encode");
        let decoded = run_decode(&encoded).expect("decode");
        assert_eq!(decoded, hex_input);
    }

    #[test]
    fn rejects_invalid_prefix() {
        // HRP with chars outside printable ASCII range fails parse.
        assert!(run_encode("\x00bad", "706174617465").is_err());
    }

    #[test]
    fn rejects_unknown_encoding() {
        // String with chars that aren't base16, bech32, or base58 chars.
        let result = run_encode("test", "!@#$%^&*()");
        assert!(matches!(result, Err(Bech32Error::UnknownEncoding)));
    }
}
