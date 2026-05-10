//! cardano-cli envelope-file reader.
//!
//! Mirrors upstream `Cardano.CLI.Read` plus its `Read/*` sub-tree
//! (envelope-format readers for governance, DRep, and committee
//! key types).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Read.hs`.
//! The sub-modules below mirror
//! `Cardano/CLI/Read/{Committee,DRep,GovernanceActionId}.hs`.
//! R295 sweeper corrected the docstring (R294's auto-generated
//! parent shell missed the top-level `Read.hs` alongside the
//! `Read/` directory and declared an incorrect synthesis verdict;
//! re-graded to (a) DIRECT_MIRROR here).

pub mod committee;
pub mod d_rep;
pub mod governance_action_id;
