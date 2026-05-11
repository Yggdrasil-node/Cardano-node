//! Analysis dispatch layer — parent shell for the analysis sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side parent shell for the `analysis/` sub-tree. The
//! upstream `Cardano.Tools.DBAnalyser.Analysis` module
//! (`Analysis.hs`, ~1057 lines) port lives at [`runner`] (R479);
//! per-analysis handlers ship across R479-R480 per the R475-R481
//! arc plan.
//!
//! Layout mapping:
//!
//! | Upstream                                                              | Yggdrasil                                |
//! |-----------------------------------------------------------------------|------------------------------------------|
//! | `Analysis/BenchmarkLedgerOps/SlotDataPoint.hs`                        | `analysis/benchmark_ledger_ops/slot_data_point.rs` |
//! | `Analysis/BenchmarkLedgerOps/Metadata.hs`                             | `analysis/benchmark_ledger_ops/metadata.rs` (pending) |
//! | `Analysis/BenchmarkLedgerOps/FileWriting.hs`                          | `analysis/benchmark_ledger_ops/file_writing.rs` (pending) |
//! | `Analysis.hs`                                                         | `analysis/runner.rs` (R479: 4 handlers + dispatch; R480: +3 handlers + deferrals) |

pub mod benchmark_ledger_ops;
pub mod runner;
