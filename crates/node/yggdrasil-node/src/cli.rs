//! Top-level `clap` subcommand definitions for the `yggdrasil-node` binary.
//!
//! Mirrors upstream `Cardano.Node.Parsers` (the `optparse-applicative`
//! shape that defines `cardano-node`'s subcommand surface). Yggdrasil's
//! variant is built on `clap` derive instead of `optparse-applicative`,
//! so the layout differs but the subcommand matrix is the same:
//!
//!  - `run` — start the node runtime
//!  - `validate-config` — operator preflight
//!  - `status` — on-disk inspection
//!  - `default-config` — emit the default JSON config
//!  - `cardano-cli` — pure-Rust subset of upstream `cardano-cli`
//!  - `query` (Unix) — NtC `LocalStateQuery` dispatcher
//!  - `tx-mempool` (Unix) — NtC `LocalTxMonitor` driver
//!  - `submit-tx` (Unix) — NtC `LocalTxSubmission` driver
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Parsers.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `clap`-based CLI parser. Upstream's `cardano-cli` carries its own optparse-applicative-based parser tree split across `cardano-cli/src/Cardano/CLI/Parser.hs` + per-cluster sub-parsers; Yggdrasil's `cli.rs` is the binary-side `clap` parser specific to the `yggdrasil-node` binary's subcommand surface (`run`, `validate-config`, `status`, `default-config`, `cardano-cli`, `query`, `submit-tx`).

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Subcommand;

use yggdrasil_node_config::NetworkPreset;

#[cfg(unix)]
use crate::commands::query::QueryCommand;
#[cfg(unix)]
use crate::commands::tx_mempool::TxMempoolCommand;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    /// Connect to a peer and sync the chain.
    Run {
        /// Path to a JSON or YAML configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
        /// Path to a P2P topology file (upstream format). Overrides topology
        /// embedded in the config file.
        #[arg(long)]
        topology: Option<PathBuf>,
        /// Peer address (host:port). Overrides config file.
        #[arg(long)]
        peer: Option<SocketAddr>,
        /// Network magic. Overrides config file.
        #[arg(long)]
        network_magic: Option<u32>,
        /// Database directory path. Overrides config `storage_dir`.
        #[arg(long)]
        database_path: Option<PathBuf>,
        /// Listen port for inbound node-to-node connections.
        #[arg(long)]
        port: Option<u16>,
        /// Listen host address for inbound node-to-node connections.
        #[arg(long)]
        host_addr: Option<String>,
        /// Disable header verification.
        #[arg(long)]
        no_verify: bool,
        /// Batch size for sync iterations.
        ///
        /// Round 166 — bumped default from 30 to 50 after the initial-sync
        /// rollback fix in `update_ledger_checkpoint_after_progress`
        /// (`node/src/sync.rs`).  The fix detects the
        /// `[RollBackward(Origin), RollForward(...)]` shape every fresh
        /// ChainSync session opens with and bypasses the heavy
        /// `recover_ledger_state_chaindb` call (which replays the volatile
        /// suffix without firing epoch boundaries), running the
        /// boundary-aware forward path directly — so a single batch can now
        /// straddle Byron→Shelley without triggering `PPUP wrong epoch`.
        /// Empirically: 50 → ~14 blocks/sec on preprod (vs ~9 at 30, ~5 at
        /// the original 10).  Values >50 plateau and start hitting upstream
        /// fetch latency.
        #[arg(long, default_value = "50")]
        batch_size: usize,
        /// Minimum slot delta between persisted ledger checkpoints.
        #[arg(long)]
        checkpoint_interval_slots: Option<u64>,
        /// Maximum number of persisted ledger checkpoints to retain.
        #[arg(long)]
        max_ledger_snapshots: Option<usize>,
        /// Maximum checkpoint trace events emitted per second. Use `0` to disable rate limiting.
        #[arg(long)]
        checkpoint_trace_max_frequency: Option<f64>,
        /// Severity override for checkpoint trace events, for example `Info` or `Silence`.
        #[arg(long)]
        checkpoint_trace_severity: Option<String>,
        /// Backend override for checkpoint trace events. Repeat the flag to route to multiple backends.
        #[arg(long, action = clap::ArgAction::Append)]
        checkpoint_trace_backend: Vec<String>,
        /// Port for Prometheus metrics HTTP endpoint. Disabled when not set.
        #[arg(long)]
        metrics_port: Option<u16>,
        /// Run as a relay/non-producing node even when block-producer
        /// credential paths are present in the config.
        #[arg(long)]
        non_producing_node: bool,
        /// Override `max_concurrent_block_fetch_peers` from the config
        /// file.  When `> 1`, the runtime promotes each warm peer's
        /// `BlockFetchClient` into a per-peer worker task and the sync
        /// loop dispatches fetch ranges in parallel via the shared
        /// `FetchWorkerPool` (mirrors upstream
        /// `Ouroboros.Network.BlockFetch.ClientRegistry`).  Default
        /// ships at `1`; the §6.5 runbook rehearsal raises this to
        /// `2` (then `4`) once parity is established.
        #[arg(long)]
        max_concurrent_block_fetch_peers: Option<u8>,
        /// Path to the NtC Unix domain socket for local client connections.
        #[arg(long)]
        socket_path: Option<PathBuf>,
        /// Path to the KES signing key file (text-envelope format).
        /// Required for block production.
        #[arg(long)]
        shelley_kes_key: Option<PathBuf>,
        /// Path to the VRF signing key file (text-envelope format).
        /// Required for block production.
        #[arg(long)]
        shelley_vrf_key: Option<PathBuf>,
        /// Path to the operational certificate file (text-envelope format).
        /// Required for block production.
        #[arg(long)]
        shelley_operational_certificate: Option<PathBuf>,
        /// Path to the issuer cold verification key file (text-envelope format).
        /// Required for strict external validation parity of forged headers.
        #[arg(long)]
        shelley_operational_certificate_issuer_vkey: Option<PathBuf>,
    },
    /// Validate config, snapshot inputs, and any existing on-disk storage state.
    ValidateConfig {
        /// Path to a JSON or YAML configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
        /// Path to a P2P topology file (upstream format). Overrides topology
        /// embedded in the config file.
        #[arg(long)]
        topology: Option<PathBuf>,
        /// Database directory path. Overrides config `storage_dir`.
        #[arg(long)]
        database_path: Option<PathBuf>,
        /// Listen port for inbound node-to-node connections.
        #[arg(long)]
        port: Option<u16>,
        /// Listen host address for inbound node-to-node connections.
        #[arg(long)]
        host_addr: Option<String>,
        /// Validate as a relay/non-producing node even when block-producer
        /// credential paths are present in the config.
        #[arg(long)]
        non_producing_node: bool,
        /// Path to the KES signing key file (text-envelope format).
        #[arg(long)]
        shelley_kes_key: Option<PathBuf>,
        /// Path to the VRF signing key file (text-envelope format).
        #[arg(long)]
        shelley_vrf_key: Option<PathBuf>,
        /// Path to the operational certificate file (text-envelope format).
        #[arg(long)]
        shelley_operational_certificate: Option<PathBuf>,
        /// Path to the issuer cold verification key file (text-envelope format).
        #[arg(long)]
        shelley_operational_certificate_issuer_vkey: Option<PathBuf>,
    },
    /// Inspect on-disk storage and report current sync status.
    Status {
        /// Path to a JSON or YAML configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
        /// Path to a P2P topology file (upstream format). Overrides topology
        /// embedded in the config file.
        #[arg(long)]
        topology: Option<PathBuf>,
        /// Database directory path. Overrides config `storage_dir`.
        #[arg(long)]
        database_path: Option<PathBuf>,
    },
    /// Print the default configuration as JSON.
    DefaultConfig,
    /// Execute selected `cardano-cli` operations via a pure Rust implementation.
    CardanoCli {
        /// Network preset used to resolve reference config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset), default_value = "preprod")]
        network: NetworkPreset,
        /// Root directory for reference configs (contains
        /// `mainnet/`, `preprod/`, `preview/`).
        #[arg(long)]
        upstream_config_root: Option<PathBuf>,
        #[command(subcommand)]
        action: CardanoCliCommand,
    },
    /// Query the running node via the NtC LocalStateQuery protocol.
    #[cfg(unix)]
    Query {
        /// Path to the NtC Unix domain socket of the running node.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Network magic used by the running node.
        #[arg(long, env = "CARDANO_NODE_NETWORK_MAGIC", default_value_t = 764824073)]
        network_magic: u32,
        /// Query tag to execute.
        #[command(subcommand)]
        query: QueryCommand,
    },
    /// Inspect the running node's mempool via the NtC LocalTxMonitor
    /// mini-protocol.  Mirrors upstream `cardano-cli query tx-mempool`.
    #[cfg(unix)]
    TxMempool {
        /// Path to the NtC Unix domain socket of the running node.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Network magic used by the running node.
        #[arg(long, env = "CARDANO_NODE_NETWORK_MAGIC", default_value_t = 764824073)]
        network_magic: u32,
        /// Mempool action to execute.
        #[command(subcommand)]
        action: TxMempoolCommand,
    },
    /// Submit a transaction to the running node via the NtC LocalTxSubmission protocol.
    #[cfg(unix)]
    SubmitTx {
        /// Path to the NtC Unix domain socket of the running node.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Network magic used by the running node.
        #[arg(long, env = "CARDANO_NODE_NETWORK_MAGIC", default_value_t = 764824073)]
        network_magic: u32,
        /// Path to a file containing the CBOR-encoded transaction.
        #[arg(long, conflicts_with = "tx_hex")]
        tx_file: Option<PathBuf>,
        /// Hex-encoded CBOR transaction bytes. Accepts an optional `0x`
        /// prefix and surrounding whitespace for terminal-paste ergonomics.
        #[arg(long, conflicts_with = "tx_file")]
        tx_hex: Option<String>,
    },
}

/// cardano-cli actions exposed through the pure Rust command implementation.
///
/// The variants below mirror upstream `cardano-cli` subcommand shapes
/// closely enough that operator muscle-memory ports cleanly:
///
/// | Yggdrasil                                          | upstream `cardano-cli`            |
/// | -------------------------------------------------- | --------------------------------- |
/// | `yggdrasil-node cardano-cli version`               | `cardano-cli version`             |
/// | `yggdrasil-node cardano-cli show-upstream-config`  | n/a (Yggdrasil-specific helper)   |
/// | `yggdrasil-node cardano-cli query-tip`             | `cardano-cli query tip`           |
/// | `yggdrasil-node cardano-cli query-utxo`            | `cardano-cli query utxo`          |
/// | `yggdrasil-node cardano-cli query-protocol-parameters` | `cardano-cli query protocol-parameters` |
/// | `yggdrasil-node cardano-cli query-stake-pools`     | `cardano-cli query stake-pools`   |
/// | `yggdrasil-node cardano-cli query-stake-distribution` | `cardano-cli query stake-distribution` |
///
/// The hyphenated single-token form (`query-tip` rather than
/// `query tip`) lets clap nest these under `cardano-cli` without an
/// additional sub-subcommand layer. A future Phase 3 round migrates
/// the surface to the upstream-shaped two-token form once the
/// in-crate `yggdrasil-cardano-cli` runtime can host the parser
/// independently.
#[derive(Subcommand)]
pub(crate) enum CardanoCliCommand {
    /// Print pure-Rust cardano-cli compatibility version info.
    Version,
    /// Show resolved reference config paths and network magic.
    ShowUpstreamConfig,
    /// Query tip against a running node socket.
    QueryTip {
        /// Path to node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using upstream reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
    /// Query UTxO entries. Either `--address` (hex-encoded address
    /// bytes; the upstream `cardano-cli` `--address` Bech32 form is
    /// not yet accepted — supply hex) or `--tx-in` (32-byte hex
    /// transaction id; pin a specific UTxO).
    QueryUtxo {
        /// Path to node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using upstream reference config.
        #[arg(long)]
        network_magic: Option<u32>,
        /// Filter UTxO set by address (hex-encoded). Mutually
        /// exclusive with `--tx-in`.
        #[arg(long, conflicts_with = "tx_in")]
        address: Option<String>,
        /// Filter UTxO set by transaction input in upstream
        /// `TX_HASH#INDEX` form (e.g. `--tx-in
        /// 0123abcd…#0`). Mutually exclusive with `--address`.
        #[arg(long, conflicts_with = "address")]
        tx_in: Option<String>,
    },
    /// Query current protocol parameters as JSON.
    QueryProtocolParameters {
        /// Path to node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using upstream reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
    /// Query the set of registered stake pools.
    QueryStakePools {
        /// Path to node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using upstream reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
    /// Query the stake distribution (pool-id → fraction of total
    /// stake delegated).
    QueryStakeDistribution {
        /// Path to node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using upstream reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
    /// Submit a previously-built transaction to the local node via
    /// `LocalTxSubmission`. The tx body is supplied either as a path
    /// to a CBOR file (`--tx-file`) or as a hex-encoded string
    /// (`--tx-hex`), matching upstream `cardano-cli transaction
    /// submit` ergonomics. Mutually exclusive.
    TransactionSubmit {
        /// Path to node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using upstream reference config.
        #[arg(long)]
        network_magic: Option<u32>,
        /// Path to a file containing the CBOR-encoded transaction.
        #[arg(long, conflicts_with = "tx_hex")]
        tx_file: Option<PathBuf>,
        /// Hex-encoded CBOR transaction bytes (with or without `0x`
        /// prefix; surrounding whitespace tolerated for terminal-
        /// paste ergonomics).
        #[arg(long, conflicts_with = "tx_file")]
        tx_hex: Option<String>,
    },
    /// Compute the transaction id (Blake2b-256 of the CBOR-encoded
    /// TxBody) of a transaction. Reads the same `--tx-file` /
    /// `--tx-hex` shape as `transaction-submit`. Offline operation;
    /// no socket needed.
    TransactionTxid {
        /// Path to a file containing the CBOR-encoded transaction.
        #[arg(long, conflicts_with = "tx_hex")]
        tx_file: Option<PathBuf>,
        /// Hex-encoded CBOR transaction bytes (with or without `0x`
        /// prefix; surrounding whitespace tolerated for terminal-
        /// paste ergonomics).
        #[arg(long, conflicts_with = "tx_file")]
        tx_hex: Option<String>,
    },
    /// Hash a payment / stake verification key. Reads the upstream
    /// TextEnvelope JSON shape (`type`, `description`, `cborHex`),
    /// CBOR-decodes the inner 32-byte key payload, and prints the
    /// 28-byte Blake2b-224 hash as 56 lowercase hex characters.
    /// Offline operation; no socket needed.
    ///
    /// Mirrors upstream `cardano-cli address key-hash --payment-
    /// verification-key-file FILE`. The same Blake2b-224 hash is
    /// the address-hash for Shelley payment / stake credentials,
    /// so this is the building block for `address build` / `stake-
    /// address build` (forthcoming).
    AddressKeyHash {
        /// Path to a TextEnvelope JSON file containing the
        /// verification key. Both `PaymentVerificationKeyShelley_ed25519`
        /// and `StakeVerificationKeyShelley_ed25519` envelopes are
        /// accepted — the wire shape is identical (32-byte VK
        /// inside a CBOR bytes envelope).
        #[arg(long)]
        payment_verification_key_file: PathBuf,
    },
}
