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
//! dispatches `query-*` through a `&dyn LsqClient` so the binary can
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
//! - Library defines the [`NtcQuery`] enum (one variant per
//!   LocalStateQuery the library dispatches) and the [`LsqClient`]
//!   trait with a single [`LsqClient::run_query`] method taking an
//!   `NtcQuery`. Adding a new `query-*` subcommand is one enum
//!   variant + one decoder in the concrete impl — the trait surface
//!   stays a single method regardless of how many queries land.
//! - Library's [`crate::run::run_command_with`] takes
//!   `&dyn LsqClient` and dispatches each `Command::Query*` variant
//!   through it. The simpler [`crate::run::run_command`] (no client)
//!   plugs in [`DeferralLsqClient`].
//! - [`DeferralLsqClient`] is the in-crate "no concrete impl wired"
//!   sentinel: its `run_query` returns the documented eyre error
//!   pointing operators at the node binary's wrapper.
//!
//! The trait method is presentation-aware: implementations own the
//! stdout formatting. The library's job is to dispatch the variant;
//! the impl's job is to drive the wire protocol + render.

use std::path::Path;

use eyre::Result;

/// A LocalStateQuery the library can ask a running node to answer.
///
/// Each variant maps 1:1 to a `query-*` subcommand. The concrete
/// [`LsqClient`] impl owns the per-variant CBOR encode + decode; the
/// library only carries the variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NtcQuery {
    /// `query-tip` — chain point (`GetChainPoint`).
    Tip,
    /// `query-chain-block-no` — current chain block number
    /// (`GetChainBlockNo`).
    ChainBlockNo,
    /// `query-current-era` — current ledger era (`GetCurrentEra`).
    CurrentEra,
    /// `query-system-start` — system start time (`GetSystemStart`).
    SystemStart,
    /// `query-stake-distribution` — per-pool active-stake
    /// distribution.
    StakeDistribution,
    /// `query-stake-pools` — the set of registered stake-pool ids.
    StakePools,
    /// `query-protocol-parameters` — the current protocol
    /// parameters.
    ProtocolParameters,
}

impl NtcQuery {
    /// The `query-*` subcommand name this query backs — used for
    /// error messages (`DeferralLsqClient`) + the error context an
    /// impl wraps a connection failure with.
    pub fn subcommand_name(self) -> &'static str {
        match self {
            NtcQuery::Tip => "query-tip",
            NtcQuery::ChainBlockNo => "query-chain-block-no",
            NtcQuery::CurrentEra => "query-current-era",
            NtcQuery::SystemStart => "query-system-start",
            NtcQuery::StakeDistribution => "query-stake-distribution",
            NtcQuery::StakePools => "query-stake-pools",
            NtcQuery::ProtocolParameters => "query-protocol-parameters",
        }
    }
}

/// LSQ client surface the library dispatches through.
///
/// Strict mirror: none. Rust-side trait abstraction over `Cardano.Api.queryNodeLocalState`.
///
/// Concrete implementations:
///
/// - [`DeferralLsqClient`] — bails with a structured deferral error;
///   in-crate sentinel used until a real impl is wired.
/// - `TokioLsqClient` (in this crate, behind the `lsq-tokio`
///   feature) — builds a `tokio` runtime per call, opens a
///   Unix-socket NtC connection through `yggdrasil-network`, drives
///   the `LocalStateQuery` mini-protocol, prints the JSON envelope.
pub trait LsqClient {
    /// Run one [`NtcQuery`] against the node and render the result
    /// as JSON.
    ///
    /// The impl owns stdout formatting + socket-connection
    /// construction; the library only dispatches the variant.
    ///
    /// # Parameters
    ///
    /// - `socket_path` — NtC Unix domain socket path
    ///   (`$CARDANO_NODE_SOCKET_PATH`).
    /// - `network_magic` — protocol magic for the handshake
    ///   (mainnet=764_824_073 / preprod=1 / preview=2 / custom).
    /// - `query` — which [`NtcQuery`] to run.
    fn run_query(&self, socket_path: &Path, network_magic: u32, query: NtcQuery) -> Result<()>;
}

/// In-crate "no concrete LSQ impl wired" sentinel.
///
/// Used by library-side tests + by callers (e.g. the standalone
/// `yggdrasil-cardano-cli` binary's `main.rs`) that don't plug a
/// real LSQ client through. Its `run_query` returns the documented
/// deferral error pointing operators at the node binary's wrapper.
pub struct DeferralLsqClient;

impl LsqClient for DeferralLsqClient {
    fn run_query(&self, _socket_path: &Path, _network_magic: u32, query: NtcQuery) -> Result<()> {
        let subcommand = query.subcommand_name();
        eyre::bail!(
            "{subcommand}: today's library crate doesn't carry the tokio + yggdrasil-network \
             deps needed to open a NtC socket; use the node binary's \
             `yggdrasil-node cardano-cli {subcommand} --socket-path=…` subcommand for now. \
             Library-side wiring lands once a concrete `LsqClient` impl is plugged in \
             at the binary entry-point."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `DeferralLsqClient::run_query` returns the structured deferral
    /// error — naming the right subcommand — for every `NtcQuery`
    /// variant. Pins the operator-facing message.
    #[test]
    fn deferral_client_bails_with_structured_error() {
        let client = DeferralLsqClient;
        let socket = PathBuf::from("/unused.socket");
        for query in [
            NtcQuery::Tip,
            NtcQuery::ChainBlockNo,
            NtcQuery::CurrentEra,
            NtcQuery::SystemStart,
            NtcQuery::StakeDistribution,
            NtcQuery::StakePools,
            NtcQuery::ProtocolParameters,
        ] {
            let err = client
                .run_query(&socket, 764_824_073, query)
                .expect_err("DeferralLsqClient must bail");
            let msg = err.to_string();
            assert!(
                msg.contains(query.subcommand_name()) && msg.contains("LsqClient"),
                "error must name the subcommand + point at LsqClient wiring; got {msg}"
            );
        }
    }

    /// A custom `LsqClient` impl can plug in arbitrary behavior.
    /// Smoke-tests that the trait is implementable in a third-party
    /// crate and that the `NtcQuery` variant + magic are forwarded.
    #[test]
    fn custom_lsq_impl_can_be_plugged() {
        use std::cell::Cell;

        struct StubClient {
            expected_magic: u32,
            last_query: Cell<Option<NtcQuery>>,
        }
        impl LsqClient for StubClient {
            fn run_query(&self, _socket: &Path, magic: u32, query: NtcQuery) -> Result<()> {
                if magic != self.expected_magic {
                    eyre::bail!(
                        "magic mismatch: got {magic}, expected {}",
                        self.expected_magic
                    );
                }
                self.last_query.set(Some(query));
                Ok(())
            }
        }
        let client = StubClient {
            expected_magic: 1,
            last_query: Cell::new(None),
        };
        client
            .run_query(&PathBuf::from("/x"), 1, NtcQuery::CurrentEra)
            .expect("stub with matching magic succeeds");
        assert_eq!(
            client.last_query.get(),
            Some(NtcQuery::CurrentEra),
            "the NtcQuery variant must reach the impl"
        );
        let err = client
            .run_query(&PathBuf::from("/x"), 2, NtcQuery::Tip)
            .expect_err("stub with mismatched magic bails");
        assert!(err.to_string().contains("magic mismatch"));
    }
}
