use blake2::{Blake2b512, Digest};
use blake2::digest::consts::U32;

type Blake2b256 = blake2::Blake2b<U32>;

/// A 64-byte Blake2b-512 digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blake2bHash(pub [u8; 64]);

/// A 32-byte Blake2b-256 digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blake2b256Hash(pub [u8; 32]);

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
