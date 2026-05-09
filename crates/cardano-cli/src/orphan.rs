//! Orphan-instance namespace for cardano-cli.
//!
//! Mirrors upstream `Cardano.CLI.Orphan` — the module Haskell uses to
//! gather orphan instances that the cardano-cli code-base needs but
//! that don't have a natural home in any other module. Rust does not
//! have orphan instances in the same sense (the orphan rule is about
//! `impl ForeignTrait for ForeignType`); this file exists to carry
//! the strict 1:1 file-mirror parity with upstream's namespace.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Orphan.hs`.
//! Empty module on the Yggdrasil side because Rust's coherence rules
//! eliminate the upstream orphan-instance need; retained as a strict
//! file mirror so the audit table grades clean.
