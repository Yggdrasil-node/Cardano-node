//! EraBased genesis key delegation certificate sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/governance/genesis_key_delegation_certificate/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/Governance/GenesisKeyDelegationCertificate.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/Governance/GenesisKeyDelegationCertificate/*.hs`.

pub mod run;
