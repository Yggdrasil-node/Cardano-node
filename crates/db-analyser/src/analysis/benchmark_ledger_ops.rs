//! BenchmarkLedgerOps analysis sub-tree — parent shell for the three
//! upstream leaves under `Analysis/BenchmarkLedgerOps/`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side parent shell. The upstream
//! `Cardano.Tools.DBAnalyser.Analysis.BenchmarkLedgerOps` namespace
//! is *not* a single Haskell module — the three `.hs` leaves
//! (`SlotDataPoint.hs`, `Metadata.hs`, `FileWriting.hs`) live as
//! peers under it without an aggregate module. This file therefore
//! has no upstream `.hs` counterpart; it exists as a directory-shell
//! to declare the leaves below it.
//!
//! Layout mapping:
//!
//! | Upstream                                                  | Yggdrasil                |
//! |-----------------------------------------------------------|--------------------------|
//! | `Analysis/BenchmarkLedgerOps/SlotDataPoint.hs`            | `slot_data_point.rs`     |
//! | `Analysis/BenchmarkLedgerOps/Metadata.hs`                 | `metadata.rs`            |
//! | `Analysis/BenchmarkLedgerOps/FileWriting.hs`              | `file_writing.rs`        |

pub mod file_writing;
pub mod metadata;
pub mod slot_data_point;
