//! Log-handler subsystem — parent shell for the
//! `handlers/logs/` sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side parent shell for the
//! `Cardano.Tracer.Handlers.Logs.*` namespace. The upstream namespace
//! has no aggregate `Logs.hs` file — it's a directory of peer leaves
//! (`File.hs`, `Rotator.hs`, `Journal.hs`, `Utils.hs`,
//! `TraceObjects.hs`, plus the `Journal/` subdirectory) — so this
//! file exists only to declare the sub-modules below it.
//!
//! Layout mapping:
//!
//! | Upstream                                      | Yggdrasil                |
//! |-----------------------------------------------|--------------------------|
//! | `Logs/File.hs`                                | `file.rs` (pending)      |
//! | `Logs/Rotator.hs`                             | `rotator.rs` (pending)   |
//! | `Logs/Journal.hs`                             | `journal.rs`             |
//! | `Logs/Journal/Systemd.hs`                     | (carve-out — Yggdrasil's  policy bans systemd-specific deps; see `journal::no_systemd`) |
//! | `Logs/Journal/NoSystemd.hs`                   | `journal/no_systemd.rs`  |
//! | `Logs/TraceObjects.hs`                        | `trace_objects.rs` (pending) |
//! | `Logs/Utils.hs`                               | `utils.rs` (pending)     |

pub mod journal;
