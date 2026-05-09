//! EraIndependent cardano-cli command surface.
//!
//! Mirrors upstream `Cardano.CLI.EraIndependent.*`. EraIndependent is
//! the surface that does not vary across eras — node identity, key
//! generation/conversion, address derivation, hash computation, and
//! other operator-tooling commands whose semantics are independent
//! of the protocol era.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! EraIndependent sub-tree. Upstream has no top-level
//! `Cardano/CLI/EraIndependent.hs`; the surface lives entirely under
//! `Cardano/CLI/EraIndependent/*.hs`.

pub mod address;
pub mod cip;
pub mod debug;
pub mod hash;
pub mod key;
pub mod node;
pub mod ping;
