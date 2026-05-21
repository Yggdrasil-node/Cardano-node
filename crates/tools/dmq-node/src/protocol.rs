//! DMQ mini-protocol ports.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Module-tree parent for the upstream
//! `DMQ/Protocol/` directory. Each mini-protocol collapses its
//! upstream `{Type,Codec,Client,Server,Validate}.hs` files into one
//! Rust file, mirroring the established
//! `crates/network/src/protocols/` one-file-per-mini-protocol pattern.

pub mod local_msg_notification;
pub mod local_msg_submission;
pub mod sig_submission;
