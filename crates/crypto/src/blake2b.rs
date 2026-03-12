use blake2::{Blake2b512, Digest};

/// A 64-byte Blake2b digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blake2bHash(pub [u8; 64]);

/// Hashes arbitrary bytes with Blake2b-512.
pub fn hash_bytes(bytes: &[u8]) -> Blake2bHash {
    let digest = Blake2b512::digest(bytes);
    let mut hash = [0_u8; 64];
    hash.copy_from_slice(&digest);
    Blake2bHash(hash)
}
