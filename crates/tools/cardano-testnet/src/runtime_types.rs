//! cardano-testnet runtime and key-file types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side port of upstream
//! `cardano-testnet/src/Testnet/Types.hs` — the basename `types.rs`
//! is already taken by the `Testnet/Start/Types.hs` mirror, and the
//! `lib.rs` layout table maps `Testnet/Types.hs` to this
//! `runtime_types.rs`.
//!
//! Ports the portable types: `KeyPair` and the key-kind markers, the
//! SPO / payment / delegator key records, `LeadershipSlot`, the
//! default testnet IPv4, and the `TestnetRuntime` / `TestnetNode` /
//! `TestnetKesAgent` process-handle record carriers. Upstream's `VKey`
//! / `SKey` are `File`-tag
//! phantoms with no Rust counterpart — yggdrasil's `KeyPair` stores
//! `PathBuf` directly rather than a typed `File`.

use crate::filepath::Sprocket;

use std::fmt;
use std::marker::PhantomData;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin};

/// The hard-coded testnet IPv4 address — the local host.
///
/// Mirror of upstream `testnetDefaultIpv4Address =
/// tupleToHostAddress (127, 0, 0, 1)`. Upstream's separate
/// `showIpv4Address` renderer has no Rust counterpart — `Ipv4Addr`'s
/// `Display` already produces the dotted `127.0.0.1` form.
pub const TESTNET_DEFAULT_IPV4_ADDRESS: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

/// One slot in a stake pool's leadership schedule.
///
/// Mirror of upstream `data LeadershipSlot` (`Testnet/Types.hs`) —
/// parsed from a `cardano-cli query leadership-schedule` JSON record.
/// Upstream derives Aeson `FromJSON`, which keys on the record-field
/// names (`slotNumber` / `slotTime`); `serde(rename_all = camelCase)`
/// reproduces those keys.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadershipSlot {
    /// The absolute slot number.
    pub slot_number: i64,
    /// The wall-clock time of the slot, as an ISO-8601 string.
    pub slot_time: String,
}

/// Key-kind marker — a VRF key. Mirror of upstream `data VrfKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct VrfKey;

/// Key-kind marker — a stake-pool cold key. Mirror of upstream
/// `data StakePoolKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StakePoolKey;

/// Key-kind marker — a stake key. Mirror of upstream `data StakeKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StakeKey;

/// Key-kind marker — a payment key. Mirror of upstream
/// `data PaymentKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PaymentKey;

/// Key-kind marker — a KES (key-evolving-signature) key. Mirror of
/// upstream `data KesKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct KesKey;

/// Key-kind marker — a DRep (delegated-representative) key. Mirror of
/// upstream `data DRepKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DRepKey;

/// A verification + signing key-file pair, phantom-typed by key kind.
///
/// Mirror of upstream `data KeyPair k` (`Testnet/Types.hs`) — the
/// `k` parameter is one of the key-kind markers above, giving
/// compile-time safety against mixing, say, a `KeyPair<PaymentKey>`
/// with a `KeyPair<StakeKey>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyPair<K> {
    /// Path to the verification (public) key file.
    pub verification_key: PathBuf,
    /// Path to the signing (private) key file.
    pub signing_key: PathBuf,
    _kind: PhantomData<K>,
}

impl<K> KeyPair<K> {
    /// Construct a key pair from its verification- and signing-key
    /// file paths.
    pub fn new(
        verification_key: impl Into<PathBuf>,
        signing_key: impl Into<PathBuf>,
    ) -> KeyPair<K> {
        KeyPair {
            verification_key: verification_key.into(),
            signing_key: signing_key.into(),
            _kind: PhantomData,
        }
    }

    /// The verification-key file path. Mirror of upstream
    /// `verificationKeyFp`.
    pub fn verification_key_fp(&self) -> &Path {
        &self.verification_key
    }

    /// The signing-key file path. Mirror of upstream `signingKeyFp`.
    pub fn signing_key_fp(&self) -> &Path {
        &self.signing_key
    }
}

/// The cold, VRF, and staking key pairs of a stake-pool-operator
/// (SPO) node.
///
/// Mirror of upstream `data SpoNodeKeys` (`Testnet/Types.hs`).
/// Upstream's `MonoFunctor` instance — a typeclass for mapping a
/// function over the contained file paths — is Haskell-specific
/// machinery with no Rust counterpart; only the record is ported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpoNodeKeys {
    /// The pool's cold (operator) key pair.
    pub pool_node_keys_cold: KeyPair<StakePoolKey>,
    /// The pool's VRF key pair.
    pub pool_node_keys_vrf: KeyPair<VrfKey>,
    /// The pool's staking key pair.
    pub pool_node_keys_staking: KeyPair<StakeKey>,
}

/// A payment key pair together with its derived address.
///
/// Mirror of upstream `data PaymentKeyInfo` (`Testnet/Types.hs`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentKeyInfo {
    /// The payment key pair.
    pub payment_key_info_pair: KeyPair<PaymentKey>,
    /// The address derived from the payment key.
    pub payment_key_info_addr: String,
}

/// A stake delegator — a payment key pair and the staking key pair it
/// delegates with.
///
/// Mirror of upstream `data Delegator` (`Testnet/Types.hs`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Delegator {
    /// The delegator's payment key pair.
    pub payment_key_pair: KeyPair<PaymentKey>,
    /// The delegator's staking key pair.
    pub staking_key_pair: KeyPair<StakeKey>,
}

/// Upstream `CardanoModeParams $ EpochSlots 21600` used by
/// `nodeConnectionInfo`.
pub const CARDANO_MODE_EPOCH_SLOTS: u64 = 21_600;

const DEFAULT_RPC_SOCKET_NAME: &str = "rpc.sock";

/// Network magic used by a local Cardano testnet.
///
/// Mirror of upstream `NetworkMagic` when `nodeConnectionInfo` builds
/// `Testnet (NetworkMagic ...)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NetworkMagic(pub u32);

/// Network identifier for local node-to-client calls.
///
/// Mirror of upstream `NetworkId`; `cardano-testnet` runtime records
/// use the `Testnet` constructor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NetworkId {
    /// The mainnet network.
    Mainnet,
    /// A testnet network with its magic.
    Testnet(NetworkMagic),
}

/// Cardano-mode consensus parameters.
///
/// Mirror of upstream `CardanoModeParams (EpochSlots 21600)` at the
/// `cardano-testnet` runtime boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CardanoModeParams {
    /// Byron epoch slots in Cardano mode.
    pub epoch_slots: u64,
}

/// Rust carrier for upstream `LocalNodeConnectInfo`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalNodeConnectInfo {
    /// Node socket path.
    pub local_node_socket_path: PathBuf,
    /// Network identifier.
    pub local_node_network_id: NetworkId,
    /// Consensus mode parameters.
    pub local_consensus_mode_params: CardanoModeParams,
}

/// Errors constructing [`LocalNodeConnectInfo`] from a runtime.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NodeConnectionInfoError {
    /// No node exists at the requested zero-based index.
    #[error("there is no node in the testnet with index {index}; number of nodes: {nodes}")]
    NoNodeWithIndex {
        /// Requested zero-based index.
        index: usize,
        /// Number of nodes in the runtime.
        nodes: usize,
    },
    /// Runtime magic cannot fit in upstream's `NetworkMagic Word32`.
    #[error("testnet magic {0} is outside the NetworkMagic Word32 range")]
    InvalidNetworkMagic(i64),
}

/// Handle to a spawned node or KES-agent stdin stream.
///
/// Mirror of upstream `IO.Handle` fields. The `Placeholder` variant lets
/// deterministic unit tests and pre-spawn planning code carry the same
/// record shape without launching a subprocess.
pub enum TestnetStdinHandle {
    /// No stdin handle has been attached yet.
    Placeholder,
    /// Stdin of a spawned child process.
    Child(Box<ChildStdin>),
}

impl TestnetStdinHandle {
    /// Construct a placeholder handle.
    pub fn placeholder() -> Self {
        TestnetStdinHandle::Placeholder
    }

    /// Wrap a spawned child stdin handle.
    pub fn from_child(stdin: ChildStdin) -> Self {
        TestnetStdinHandle::Child(Box::new(stdin))
    }

    /// Whether this is a placeholder.
    pub fn is_placeholder(&self) -> bool {
        matches!(self, TestnetStdinHandle::Placeholder)
    }
}

impl fmt::Debug for TestnetStdinHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestnetStdinHandle::Placeholder => f.write_str("TestnetStdinHandle::Placeholder"),
            TestnetStdinHandle::Child(_) => f.write_str("TestnetStdinHandle::Child(..)"),
        }
    }
}

/// Handle to a spawned node or KES-agent process.
///
/// Mirror of upstream `IO.ProcessHandle` fields.
pub enum TestnetProcessHandle {
    /// No process has been spawned yet.
    Placeholder,
    /// Spawned child process.
    Child(Box<Child>),
}

impl TestnetProcessHandle {
    /// Construct a placeholder process handle.
    pub fn placeholder() -> Self {
        TestnetProcessHandle::Placeholder
    }

    /// Wrap a spawned child process.
    pub fn from_child(child: Child) -> Self {
        TestnetProcessHandle::Child(Box::new(child))
    }

    /// Return the child process id when a real child is attached.
    pub fn child_id(&self) -> Option<u32> {
        match self {
            TestnetProcessHandle::Placeholder => None,
            TestnetProcessHandle::Child(child) => Some(child.id()),
        }
    }
}

impl fmt::Debug for TestnetProcessHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestnetProcessHandle::Placeholder => f.write_str("TestnetProcessHandle::Placeholder"),
            TestnetProcessHandle::Child(child) => f
                .debug_struct("TestnetProcessHandle::Child")
                .field("id", &child.id())
                .finish(),
        }
    }
}

/// Runtime state for a running testnet.
///
/// Mirror of upstream `data TestnetRuntime`.
#[derive(Debug)]
pub struct TestnetRuntime {
    /// Node configuration file.
    pub configuration_file: PathBuf,
    /// Shelley genesis file.
    pub shelley_genesis_file: PathBuf,
    /// Testnet magic.
    pub testnet_magic: i64,
    /// Spawned testnet nodes.
    pub testnet_nodes: Vec<TestnetNode>,
    /// Generated wallets.
    pub wallets: Vec<PaymentKeyInfo>,
    /// Generated delegators.
    pub delegators: Vec<Delegator>,
}

/// Runtime state for one spawned node.
///
/// Mirror of upstream `data TestnetNode`.
#[derive(Debug)]
pub struct TestnetNode {
    /// Node name.
    pub node_name: String,
    /// SPO keys; present only for block-producing nodes.
    pub pool_keys: Option<SpoNodeKeys>,
    /// IPv4 bind address.
    pub node_ipv4: Ipv4Addr,
    /// Node port.
    pub node_port: u16,
    /// Node local socket.
    pub node_sprocket: Sprocket,
    /// Child stdin handle.
    pub node_stdin_handle: TestnetStdinHandle,
    /// Path to the stdout log file.
    pub node_stdout: PathBuf,
    /// Path to the stderr log file.
    pub node_stderr: PathBuf,
    /// Child process handle.
    pub node_process_handle: TestnetProcessHandle,
}

/// Runtime state for one spawned KES agent.
///
/// Mirror of upstream `data TestnetKesAgent`.
#[derive(Debug)]
pub struct TestnetKesAgent {
    /// KES agent name.
    pub kes_agent_name: String,
    /// SPO keys; present only for block-producing nodes.
    pub kes_agent_pool_keys: Option<SpoNodeKeys>,
    /// Service socket.
    pub kes_agent_service_sprocket: Sprocket,
    /// Control socket.
    pub kes_agent_control_sprocket: Sprocket,
    /// Child stdin handle.
    pub kes_agent_stdin_handle: TestnetStdinHandle,
    /// Path to the stdout log file.
    pub kes_agent_stdout: PathBuf,
    /// Path to the stderr log file.
    pub kes_agent_stderr: PathBuf,
    /// Child process handle.
    pub kes_agent_process_handle: TestnetProcessHandle,
}

/// All node sprockets in runtime order.
///
/// Mirror of upstream `testnetSprockets`.
pub fn testnet_sprockets(runtime: &TestnetRuntime) -> Vec<Sprocket> {
    runtime
        .testnet_nodes
        .iter()
        .map(|node| node.node_sprocket.clone())
        .collect()
}

/// SPO nodes in runtime order.
///
/// Mirror of upstream `spoNodes`.
pub fn spo_nodes(runtime: &TestnetRuntime) -> Vec<&TestnetNode> {
    runtime
        .testnet_nodes
        .iter()
        .filter(|node| is_testnet_node_spo(node))
        .collect()
}

/// Relay nodes in runtime order.
///
/// Mirror of upstream `relayNodes`.
pub fn relay_nodes(runtime: &TestnetRuntime) -> Vec<&TestnetNode> {
    runtime
        .testnet_nodes
        .iter()
        .filter(|node| !is_testnet_node_spo(node))
        .collect()
}

/// Whether the node is an SPO node.
///
/// Mirror of upstream `isTestnetNodeSpo = isJust . poolKeys`.
pub fn is_testnet_node_spo(node: &TestnetNode) -> bool {
    node.pool_keys.is_some()
}

/// The node socket path.
///
/// Mirror of upstream `nodeSocketPath = File . sprocketSystemName . nodeSprocket`.
pub fn node_socket_path(node: &TestnetNode) -> PathBuf {
    PathBuf::from(node.node_sprocket.system_name())
}

/// The node RPC socket path.
///
/// Mirror of upstream `nodeRpcSocketPath = nodeSocketPathToRpcSocketPath
/// . nodeSocketPath`. `cardano-node`'s parser documents the default as
/// `rpc.sock` in the same directory as the node socket.
pub fn node_rpc_socket_path(node: &TestnetNode) -> PathBuf {
    let socket_path = node.node_sprocket.system_name();
    match socket_path.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => {
            PathBuf::from(format!("{dir}/{DEFAULT_RPC_SOCKET_NAME}"))
        }
        _ => PathBuf::from(DEFAULT_RPC_SOCKET_NAME),
    }
}

/// Connection data for one node in a runtime.
///
/// Mirror of upstream `nodeConnectionInfo`.
pub fn node_connection_info(
    runtime: &TestnetRuntime,
    index: usize,
) -> Result<LocalNodeConnectInfo, NodeConnectionInfoError> {
    let node =
        runtime
            .testnet_nodes
            .get(index)
            .ok_or(NodeConnectionInfoError::NoNodeWithIndex {
                index,
                nodes: runtime.testnet_nodes.len(),
            })?;
    let magic = u32::try_from(runtime.testnet_magic)
        .map_err(|_| NodeConnectionInfoError::InvalidNetworkMagic(runtime.testnet_magic))?;
    Ok(LocalNodeConnectInfo {
        local_node_socket_path: node_socket_path(node),
        local_node_network_id: NetworkId::Testnet(NetworkMagic(magic)),
        local_consensus_mode_params: CardanoModeParams {
            epoch_slots: CARDANO_MODE_EPOCH_SLOTS,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_pair_accessors_return_the_paths() {
        let kp: KeyPair<PaymentKey> = KeyPair::new("/keys/pay.vkey", "/keys/pay.skey");
        assert_eq!(kp.verification_key_fp().to_str(), Some("/keys/pay.vkey"));
        assert_eq!(kp.signing_key_fp().to_str(), Some("/keys/pay.skey"));
    }

    #[test]
    fn key_pair_equality_is_by_path() {
        let a: KeyPair<StakeKey> = KeyPair::new("/k/v", "/k/s");
        let b: KeyPair<StakeKey> = KeyPair::new("/k/v", "/k/s");
        let c: KeyPair<StakeKey> = KeyPair::new("/k/v", "/k/other");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn key_pair_is_phantom_typed_per_kind() {
        // Each key kind yields a distinct `KeyPair` type — exercised
        // here by constructing one of every kind.
        let _vrf: KeyPair<VrfKey> = KeyPair::new("v", "s");
        let _spo: KeyPair<StakePoolKey> = KeyPair::new("v", "s");
        let _stake: KeyPair<StakeKey> = KeyPair::new("v", "s");
        let _pay: KeyPair<PaymentKey> = KeyPair::new("v", "s");
        let _kes: KeyPair<KesKey> = KeyPair::new("v", "s");
        let _drep: KeyPair<DRepKey> = KeyPair::new("v", "s");
    }

    #[test]
    fn testnet_default_ipv4_is_localhost() {
        assert_eq!(TESTNET_DEFAULT_IPV4_ADDRESS, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(TESTNET_DEFAULT_IPV4_ADDRESS.to_string(), "127.0.0.1");
    }

    #[test]
    fn leadership_slot_parses_upstream_json_keys() {
        let json = r#"{"slotNumber": 4492800, "slotTime": "2021-03-01T21:47:51Z"}"#;
        let slot: LeadershipSlot = serde_json::from_str(json).expect("parses");
        assert_eq!(slot.slot_number, 4_492_800);
        assert_eq!(slot.slot_time, "2021-03-01T21:47:51Z");
    }

    #[test]
    fn spo_node_keys_holds_the_three_kinded_pairs() {
        let keys = SpoNodeKeys {
            pool_node_keys_cold: KeyPair::new("cold.vkey", "cold.skey"),
            pool_node_keys_vrf: KeyPair::new("vrf.vkey", "vrf.skey"),
            pool_node_keys_staking: KeyPair::new("stake.vkey", "stake.skey"),
        };
        assert_eq!(
            keys.pool_node_keys_cold.verification_key_fp().to_str(),
            Some("cold.vkey")
        );
        assert_eq!(keys.clone(), keys);
    }

    #[test]
    fn payment_key_info_carries_pair_and_address() {
        let info = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("pay.vkey", "pay.skey"),
            payment_key_info_addr: "addr_test1abc".to_string(),
        };
        assert_eq!(info.payment_key_info_addr, "addr_test1abc");
    }

    #[test]
    fn delegator_pairs_payment_and_staking_keys() {
        let a = Delegator {
            payment_key_pair: KeyPair::new("p.vkey", "p.skey"),
            staking_key_pair: KeyPair::new("s.vkey", "s.skey"),
        };
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.staking_key_pair.signing_key_fp().to_str(), Some("s.skey"));
    }

    fn test_node(name: &str, pool_keys: Option<SpoNodeKeys>) -> TestnetNode {
        TestnetNode {
            node_name: name.to_string(),
            pool_keys,
            node_ipv4: TESTNET_DEFAULT_IPV4_ADDRESS,
            node_port: 30_000,
            node_sprocket: crate::filepath::Sprocket {
                base: "/tmp/ygg-testnet/".to_string(),
                name: format!("run/socket/{name}"),
            },
            node_stdin_handle: TestnetStdinHandle::placeholder(),
            node_stdout: format!("{name}.stdout.log").into(),
            node_stderr: format!("{name}.stderr.log").into(),
            node_process_handle: TestnetProcessHandle::placeholder(),
        }
    }

    fn test_pool_keys() -> SpoNodeKeys {
        SpoNodeKeys {
            pool_node_keys_cold: KeyPair::new("cold.vkey", "cold.skey"),
            pool_node_keys_vrf: KeyPair::new("vrf.vkey", "vrf.skey"),
            pool_node_keys_staking: KeyPair::new("stake.vkey", "stake.skey"),
        }
    }

    #[test]
    fn testnet_node_spo_predicate_and_runtime_filters_match_upstream() {
        let runtime = TestnetRuntime {
            configuration_file: "configuration.json".into(),
            shelley_genesis_file: "shelley-genesis.json".into(),
            testnet_magic: 42,
            testnet_nodes: vec![
                test_node("node-spo1", Some(test_pool_keys())),
                test_node("node-relay1", None),
            ],
            wallets: Vec::new(),
            delegators: Vec::new(),
        };

        assert!(is_testnet_node_spo(&runtime.testnet_nodes[0]));
        assert!(!is_testnet_node_spo(&runtime.testnet_nodes[1]));
        assert_eq!(spo_nodes(&runtime)[0].node_name, "node-spo1");
        assert_eq!(relay_nodes(&runtime)[0].node_name, "node-relay1");
        assert_eq!(
            testnet_sprockets(&runtime)
                .into_iter()
                .map(|s| s.system_name())
                .collect::<Vec<_>>(),
            vec![
                "/tmp/ygg-testnet/run/socket/node-spo1".to_string(),
                "/tmp/ygg-testnet/run/socket/node-relay1".to_string(),
            ]
        );
    }

    #[test]
    fn node_socket_paths_derive_from_sprocket_and_rpc_default() {
        let node = test_node("node-spo1", Some(test_pool_keys()));

        assert_eq!(
            node_socket_path(&node),
            std::path::PathBuf::from("/tmp/ygg-testnet/run/socket/node-spo1")
        );
        assert_eq!(
            node_rpc_socket_path(&node),
            std::path::PathBuf::from("/tmp/ygg-testnet/run/socket/rpc.sock")
        );
    }

    #[test]
    fn node_connection_info_reports_socket_magic_and_cardano_epoch_slots() {
        let runtime = TestnetRuntime {
            configuration_file: "configuration.json".into(),
            shelley_genesis_file: "shelley-genesis.json".into(),
            testnet_magic: 42,
            testnet_nodes: vec![test_node("node-spo1", Some(test_pool_keys()))],
            wallets: Vec::new(),
            delegators: Vec::new(),
        };

        let info = node_connection_info(&runtime, 0).expect("node exists");
        assert_eq!(
            info.local_node_socket_path,
            std::path::PathBuf::from("/tmp/ygg-testnet/run/socket/node-spo1")
        );
        assert_eq!(
            info.local_node_network_id,
            NetworkId::Testnet(NetworkMagic(42))
        );
        assert_eq!(
            info.local_consensus_mode_params,
            CardanoModeParams {
                epoch_slots: CARDANO_MODE_EPOCH_SLOTS,
            }
        );

        assert_eq!(
            node_connection_info(&runtime, 1),
            Err(NodeConnectionInfoError::NoNodeWithIndex { index: 1, nodes: 1 })
        );
    }

    #[test]
    fn kes_agent_carries_service_and_control_sprockets() {
        let agent = TestnetKesAgent {
            kes_agent_name: "kes-agent-node1".to_string(),
            kes_agent_pool_keys: Some(test_pool_keys()),
            kes_agent_service_sprocket: crate::filepath::Sprocket {
                base: "/tmp/ygg-testnet/".to_string(),
                name: "run/socket/kes-agent-node1".to_string(),
            },
            kes_agent_control_sprocket: crate::filepath::Sprocket {
                base: "/tmp/ygg-testnet/".to_string(),
                name: "run/socket/kes-agent-node1-control".to_string(),
            },
            kes_agent_stdin_handle: TestnetStdinHandle::placeholder(),
            kes_agent_stdout: "kes-agent-node1.stdout.log".into(),
            kes_agent_stderr: "kes-agent-node1.stderr.log".into(),
            kes_agent_process_handle: TestnetProcessHandle::placeholder(),
        };

        assert!(agent.kes_agent_pool_keys.is_some());
        assert_eq!(
            agent.kes_agent_service_sprocket.system_name(),
            "/tmp/ygg-testnet/run/socket/kes-agent-node1"
        );
        assert_eq!(
            agent.kes_agent_control_sprocket.system_name(),
            "/tmp/ygg-testnet/run/socket/kes-agent-node1-control"
        );
    }
}
