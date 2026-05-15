//! LSQ-client abstraction for library-side `query-*` dispatch.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Rust-idiomatic indirection that upstream
//! Haskell's monomorphic call-graph doesn't need: upstream
//! `Cardano.CLI.*.Run.*` modules call
//! `Cardano.Api.queryNodeLocalState` inline, threading the network-
//! magic + socket-path through `LocalNodeConnectInfo`. The Rust port
//! needs a trait here because `yggdrasil-cardano-cli` keeps its
//! dependency footprint deliberately small (no `tokio`, no
//! `yggdrasil-network`) — the actual LSQ wire-protocol driver lives
//! in the binary crate that hosts the runtime, and the library
//! dispatches QueryTip through a `&dyn LsqClient` so the binary can
//! plug its concrete impl in at `main` time without bringing those
//! transitive deps into the library surface.
//!
//! The trait is intentionally **synchronous-facing** at the library
//! boundary even though concrete impls are tokio-async internally —
//! the impl is responsible for constructing its own runtime + driving
//! the future to completion. That keeps the library `run_command`
//! signature plain `fn(...) -> Result<()>` rather than `async fn`.
//!
//! ## Wiring shape
//!
//! - Library defines [`LsqClient`] with one method per LSQ-backed
//!   subcommand the library currently dispatches. R504 ships
//!   [`LsqClient::query_tip`] (the one library-side LSQ subcommand
//!   today); follow-on rounds grow the trait alongside subcommand
//!   migrations (`query utxo`, `query protocol-parameters`, …).
//! - Library's [`crate::run::run_command_with`] takes
//!   `&dyn LsqClient` and dispatches `Command::QueryTip` through it.
//!   The simpler [`crate::run::run_command`] (no client) stays
//!   operational for the `Version` / `ShowUpstreamConfig` arms and
//!   bails on `QueryTip` with a deferral pointer at
//!   `run_command_with` — preserving the public API the standalone
//!   binary's `main.rs` already calls.
//! - [`DeferralLsqClient`] is the in-crate "no concrete impl wired"
//!   sentinel: its `query_tip` returns the documented eyre error
//!   pointing operators at the node binary's wrapper. Library-side
//!   tests use it to pin the deferral message and library-side
//!   consumers (e.g. `main.rs`) can pass it through when a real impl
//!   isn't available yet.
//!
//! The trait method signature is presentation-aware: implementations
//! own the stdout formatting (matches upstream `cardano-cli query
//! tip --out-file` behavior where output formatting is the impl's
//! job, not the dispatcher's). The library's job is to dispatch the
//! variant; the impl's job is to drive the wire protocol + render.

use std::path::Path;

use eyre::Result;

/// LSQ client surface the library dispatches through.
///
/// Strict mirror: none. Rust-side trait abstraction over `Cardano.Api.queryNodeLocalState`.
///
/// Concrete implementations:
///
/// - [`DeferralLsqClient`] — bails with a structured deferral error;
///   in-crate stub used until the binary wires a real impl.
/// - (future) `TokioYggdrasilLsqClient` in the binary crate — builds
///   a `tokio::runtime::Runtime` per call, opens a Unix-socket NtC
///   connection through `yggdrasil-network`, drives the
///   `LocalStateQuery` mini-protocol to retrieve the tip + chain
///   point + block number, prints the JSON envelope upstream
///   `cardano-cli query tip` emits.
pub trait LsqClient {
    /// Query the running node for tip / chain-point / block-no and
    /// render the result.
    ///
    /// Mirrors the inline call in upstream
    /// `Cardano.CLI.Compatible.Run.Tip.runTipCmd`. The impl owns
    /// stdout formatting + socket connection construction; the
    /// library only dispatches.
    ///
    /// # Parameters
    ///
    /// - `socket_path` — NtC Unix domain socket path
    ///   (`$CARDANO_NODE_SOCKET_PATH`).
    /// - `network_magic` — protocol magic for the handshake
    ///   (mainnet=764_824_073 / preprod=1 / preview=2 / custom).
    fn query_tip(&self, socket_path: &Path, network_magic: u32) -> Result<()>;
}

/// In-crate "no concrete LSQ impl wired" sentinel.
///
/// Used by library-side tests + by callers (e.g. the standalone
/// `yggdrasil-cardano-cli` binary's `main.rs`) that don't yet plug
/// a real LSQ client through. Its `query_tip` returns the documented
/// deferral error pointing operators at the node binary's wrapper.
/// When the concrete tokio + yggdrasil-network impl lands in the
/// binary, that impl displaces this sentinel in `main.rs`; the
/// sentinel stays in the crate as the test fixture.
pub struct DeferralLsqClient;

impl LsqClient for DeferralLsqClient {
    fn query_tip(&self, _socket_path: &Path, _network_magic: u32) -> Result<()> {
        eyre::bail!(
            "query-tip: today's library crate doesn't carry the tokio + yggdrasil-network \
             deps needed to open a NtC socket; use the node binary's \
             `yggdrasil-node cardano-cli query-tip --socket-path=…` subcommand for now. \
             Library-side wiring lands once a concrete `LsqClient` impl is plugged in \
             at the binary entry-point."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `DeferralLsqClient::query_tip` returns the structured deferral
    /// error rather than panicking or printing. Pins the error string
    /// so the operator-facing message stays stable.
    #[test]
    fn deferral_client_bails_with_structured_error() {
        let client = DeferralLsqClient;
        let result = client.query_tip(&PathBuf::from("/unused.socket"), 764_824_073);
        let err = result.expect_err("DeferralLsqClient must bail");
        let msg = err.to_string();
        assert!(
            msg.contains("query-tip") && msg.contains("LsqClient"),
            "error must point at LsqClient wiring; got {msg}"
        );
    }

    /// A custom `LsqClient` impl can plug in arbitrary behavior.
    /// Smoke-tests that the trait can actually be implemented in a
    /// third-party crate (the binary crate's eventual concrete impl
    /// will look like this — minus the unit-test scaffolding).
    #[test]
    fn custom_lsq_impl_can_be_plugged() {
        struct StubClient {
            expected_magic: u32,
        }
        impl LsqClient for StubClient {
            fn query_tip(&self, _socket: &Path, magic: u32) -> Result<()> {
                if magic != self.expected_magic {
                    eyre::bail!(
                        "magic mismatch: got {magic}, expected {}",
                        self.expected_magic
                    );
                }
                Ok(())
            }
        }
        let client = StubClient { expected_magic: 1 };
        client
            .query_tip(&PathBuf::from("/x"), 1)
            .expect("stub with matching magic succeeds");
        let err = client
            .query_tip(&PathBuf::from("/x"), 2)
            .expect_err("stub with mismatched magic bails");
        assert!(err.to_string().contains("magic mismatch"));
    }
}
