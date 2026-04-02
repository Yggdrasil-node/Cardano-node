//! SHA3-256 hashing.
//!
//! Used for Byron address root reconstruction (ADDRHASH = Blake2b-224 of
//! SHA3-256 of serialized address spending data).
//!
//! Reference: `Cardano.Crypto.Hashing` — `abstractHash` uses SHA3-256 for
//! Byron-era address roots.

use sha3::{Digest, Sha3_256};

/// A 32-byte SHA3-256 digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sha3_256Hash(pub [u8; 32]);

/// Hashes arbitrary bytes with SHA3-256.
pub fn sha3_256(bytes: &[u8]) -> Sha3_256Hash {
    let digest = Sha3_256::digest(bytes);
    let mut hash = [0_u8; 32];
    hash.copy_from_slice(&digest);
    Sha3_256Hash(hash)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sha3_256_output_is_32_bytes() {
        let h = sha3_256(b"hello");
        assert_eq!(h.0.len(), 32);
    }

    #[test]
    fn sha3_256_deterministic() {
        let a = sha3_256(b"test");
        let b = sha3_256(b"test");
        assert_eq!(a.0, b.0);
    }

    #[test]
    fn sha3_256_different_inputs() {
        let a = sha3_256(b"hello");
        let b = sha3_256(b"world");
        assert_ne!(a.0, b.0);
    }
}
