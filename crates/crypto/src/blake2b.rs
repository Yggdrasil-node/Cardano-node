use blake2::{Blake2b512, Digest};
use blake2::digest::consts::{U28, U32};

type Blake2b256 = blake2::Blake2b<U32>;
type Blake2b224 = blake2::Blake2b<U28>;

/// A 64-byte Blake2b-512 digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blake2bHash(pub [u8; 64]);

/// A 32-byte Blake2b-256 digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blake2b256Hash(pub [u8; 32]);

/// A 28-byte Blake2b-224 digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blake2b224Hash(pub [u8; 28]);

/// Hashes arbitrary bytes with Blake2b-512.
pub fn hash_bytes(bytes: &[u8]) -> Blake2bHash {
    let digest = Blake2b512::digest(bytes);
    let mut hash = [0_u8; 64];
    hash.copy_from_slice(&digest);
    Blake2bHash(hash)
}

/// Hashes arbitrary bytes with Blake2b-256.
///
/// Used for KES verification key pairing and header hashing.
pub fn hash_bytes_256(bytes: &[u8]) -> Blake2b256Hash {
    let digest = Blake2b256::digest(bytes);
    let mut hash = [0_u8; 32];
    hash.copy_from_slice(&digest);
    Blake2b256Hash(hash)
}

/// Hashes arbitrary bytes with Blake2b-224.
///
/// Used for credential hashes (verification key hashes, script hashes).
pub fn hash_bytes_224(bytes: &[u8]) -> Blake2b224Hash {
    let digest = Blake2b224::digest(bytes);
    let mut hash = [0_u8; 28];
    hash.copy_from_slice(&digest);
    Blake2b224Hash(hash)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ── Blake2b-512 ──────────────────────────────────────────────────────

    #[test]
    fn blake2b_512_output_is_64_bytes() {
        let h = hash_bytes(b"hello");
        assert_eq!(h.0.len(), 64);
    }

    #[test]
    fn blake2b_512_deterministic() {
        assert_eq!(hash_bytes(b"cardano"), hash_bytes(b"cardano"));
    }

    #[test]
    fn blake2b_512_different_inputs_differ() {
        assert_ne!(hash_bytes(b"a"), hash_bytes(b"b"));
    }

    #[test]
    fn blake2b_512_empty_input() {
        let h = hash_bytes(b"");
        // Must produce a valid 64-byte digest even for empty input.
        assert_eq!(h.0.len(), 64);
        assert_ne!(h, hash_bytes(b"notempty"));
    }

    #[test]
    fn blake2b_512_known_vector() {
        // Blake2b-512("abc") — reference vector (unkeyed, 64-byte digest).
        let h = hash_bytes(b"abc");
        let hex: String = h.0.iter().fold(String::new(), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{:02x}", b);
            acc
        });
        assert_eq!(
            hex,
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    // ── Blake2b-256 ──────────────────────────────────────────────────────

    #[test]
    fn blake2b_256_output_is_32_bytes() {
        let h = hash_bytes_256(b"test");
        assert_eq!(h.0.len(), 32);
    }

    #[test]
    fn blake2b_256_deterministic() {
        assert_eq!(hash_bytes_256(b"test"), hash_bytes_256(b"test"));
    }

    #[test]
    fn blake2b_256_different_inputs_differ() {
        assert_ne!(hash_bytes_256(b"x"), hash_bytes_256(b"y"));
    }

    #[test]
    fn blake2b_256_empty_input() {
        let h = hash_bytes_256(b"");
        assert_eq!(h.0.len(), 32);
        assert_ne!(h, hash_bytes_256(b"notempty"));
    }

    #[test]
    fn blake2b_256_known_vector() {
        // Blake2b-256("abc") — well-known test vector.
        let h = hash_bytes_256(b"abc");
        let hex: String = h.0.iter().fold(String::new(), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{:02x}", b);
            acc
        });
        assert_eq!(
            hex,
            "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319"
        );
    }

    // ── Blake2b-224 ──────────────────────────────────────────────────────

    #[test]
    fn blake2b_224_output_is_28_bytes() {
        let h = hash_bytes_224(b"test");
        assert_eq!(h.0.len(), 28);
    }

    #[test]
    fn blake2b_224_deterministic() {
        assert_eq!(hash_bytes_224(b"test"), hash_bytes_224(b"test"));
    }

    #[test]
    fn blake2b_224_different_inputs_differ() {
        assert_ne!(hash_bytes_224(b"x"), hash_bytes_224(b"y"));
    }

    #[test]
    fn blake2b_224_empty_input() {
        let h = hash_bytes_224(b"");
        assert_eq!(h.0.len(), 28);
        assert_ne!(h, hash_bytes_224(b"notempty"));
    }

    // ── Cross-variant ────────────────────────────────────────────────────

    #[test]
    fn different_output_lengths_for_same_input() {
        let h512 = hash_bytes(b"same");
        let h256 = hash_bytes_256(b"same");
        let h224 = hash_bytes_224(b"same");
        // Different lengths => structurally different.
        assert_eq!(h512.0.len(), 64);
        assert_eq!(h256.0.len(), 32);
        assert_eq!(h224.0.len(), 28);
    }

    #[test]
    fn hash_types_clone_and_copy() {
        let h = hash_bytes(b"x");
        let h2 = h;
        assert_eq!(h, h2);

        let h256 = hash_bytes_256(b"x");
        let h256_2 = h256;
        assert_eq!(h256, h256_2);

        let h224 = hash_bytes_224(b"x");
        let h224_2 = h224;
        assert_eq!(h224, h224_2);
    }

    #[test]
    fn hash_types_debug_format() {
        let h = hash_bytes(b"x");
        let dbg = format!("{:?}", h);
        assert!(dbg.starts_with("Blake2bHash("));

        let h256 = hash_bytes_256(b"x");
        let dbg256 = format!("{:?}", h256);
        assert!(dbg256.starts_with("Blake2b256Hash("));

        let h224 = hash_bytes_224(b"x");
        let dbg224 = format!("{:?}", h224);
        assert!(dbg224.starts_with("Blake2b224Hash("));
    }

    #[test]
    fn large_input_hashes_successfully() {
        let data = vec![0xAB_u8; 1_000_000];
        let h = hash_bytes(&data);
        assert_eq!(h.0.len(), 64);
        // Deterministic for the same large input.
        assert_eq!(h, hash_bytes(&data));
    }
}
