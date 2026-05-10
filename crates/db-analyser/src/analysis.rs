//! Analysis dispatch layer — parent shell for the analysis sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side parent shell for the `analysis/` sub-tree. The
//! upstream `Cardano.Tools.DBAnalyser.Analysis` module
//! (`Analysis.hs`, ~1057 lines) ports the 13-variant analysis
//! dispatch arm and lands in a future round (per the
//! `sister-tool.db-analyser` parity-matrix `remaining_work[3]`
//! entry). This file currently only declares the `benchmark_ledger_ops`
//! sub-module.
//!
//! Layout mapping:
//!
//! | Upstream                                                              | Yggdrasil                                |
//! |-----------------------------------------------------------------------|------------------------------------------|
//! | `Analysis/BenchmarkLedgerOps/SlotDataPoint.hs`                        | `analysis/benchmark_ledger_ops/slot_data_point.rs` |
//! | `Analysis/BenchmarkLedgerOps/Metadata.hs`                             | `analysis/benchmark_ledger_ops/metadata.rs` (pending) |
//! | `Analysis/BenchmarkLedgerOps/FileWriting.hs`                          | `analysis/benchmark_ledger_ops/file_writing.rs` (pending) |
//! | `Analysis.hs`                                                         | `analysis.rs` body (pending)             |

pub mod benchmark_ledger_ops;
