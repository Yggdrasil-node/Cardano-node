//! EraBased text view sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `era_based/text_view/*` sub-modules. Upstream has no
//! `Cardano/CLI/EraBased/TextView.hs` top-level file; the surface
//! lives under `Cardano/CLI/EraBased/TextView/*.hs`.

pub mod command;
pub mod option;
pub mod run;
