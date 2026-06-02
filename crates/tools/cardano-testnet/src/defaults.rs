//! cardano-testnet default genesis / script values.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Defaults.hs.
//!
//! This slice ports the era-free script values — the `simpleScript`
//! builder and the always-succeeds Plutus test scripts — plus the
//! pure node-configuration and default key/path helpers used by
//! `create-env` (`defaultYamlConfig`, `defaultYamlHardforkViaConfig`,
//! `defaultGenesisFilepath`, `defaultSpoKeys`, DRep / committee key
//! paths, delegator stake keys, UTxO key pairs, and the default P2P
//! topology records). Upstream `Defaults.hs` is otherwise era /
//! ledger-coupled (per-era default genesis records); those land once
//! yggdrasil-ledger's era surface is exposed at crate boundaries.
//! The two large Plutus blobs (`plutusV3SupplementalDatumScript`,
//! `plutusV2StakeScript`) land in a follow-up round.

use serde_json::{Map, Number, Value, json};
use std::path::PathBuf;

use crate::paths::{default_utxo_skey_path, default_utxo_vkey_path};
use crate::runtime_types::{KeyPair, PaymentKey, SpoNodeKeys, StakeKey, StakePoolKey};
use crate::types::{CardanoEra, ShelleyBasedEra};
use yggdrasil_network::{
    AfterSlot, LocalRootConfig, PeerAccessPoint, PeerDiffusionMode, PublicRootConfig,
    TopologyConfig, UseBootstrapPeers, UseLedgerPeers,
};

/// Build a "simple script" (native-script) JSON envelope requiring a
/// single signer.
///
/// Mirror of upstream `simpleScript :: Text -> Text`.
pub fn simple_script(signer_required: &str) -> String {
    format!(
        "{{ \"scripts\": [ {{ \"keyHash\": \"{signer_required}\", \"type\": \"sig\" }} ], \"type\": \"all\" }}"
    )
}

/// An always-succeeds Plutus V2 test script (text-envelope JSON).
///
/// Mirror of upstream `plutusV2Script`.
pub const PLUTUS_V2_SCRIPT: &str = r#"{ "type": "PlutusScriptV2", "description": "", "cborHex": "5822582001000022325333573466e1ccde5251333792945200000100111200116375a005" }"#;

/// An always-succeeds Plutus V3 test script (text-envelope JSON).
///
/// Mirror of upstream `plutusV3Script`.
pub const PLUTUS_V3_SCRIPT: &str =
    r#"{ "type": "PlutusScriptV3", "description": "", "cborHex": "46450101002499" }"#;

/// Relative genesis-file name for an era.
///
/// Mirror of upstream `defaultGenesisFilepath era =
/// eraToString era <> "-genesis.json"`.
pub fn default_genesis_filepath(era: CardanoEra) -> String {
    format!("{}-genesis.json", era.era_to_string())
}

/// Relative committee verification-key path.
///
/// Mirror of upstream `defaultCommitteeVkeyFp`.
pub fn default_committee_vkey_fp(n: i32) -> PathBuf {
    PathBuf::from("committee-keys").join(format!("committee{n}.vkey"))
}

/// Relative committee signing-key path.
///
/// Mirror of upstream `defaultCommitteeSkeyFp`.
pub fn default_committee_skey_fp(n: i32) -> PathBuf {
    PathBuf::from("committee-keys").join(format!("committee{n}.skey"))
}

/// Default DRep verification + signing key paths.
///
/// Mirror of upstream `defaultDRepKeyPair`. Upstream types this as a
/// `KeyPair PaymentKey`; yggdrasil keeps that exact surface instead of
/// substituting the separate `DRepKey` marker.
pub fn default_drep_key_pair(n: i32) -> KeyPair<PaymentKey> {
    KeyPair::new(
        PathBuf::from("drep-keys")
            .join(format!("drep{n}"))
            .join("drep.vkey"),
        default_drep_skey_fp(n),
    )
}

/// Relative DRep signing-key path.
///
/// Mirror of upstream `defaultDRepSkeyFp`.
pub fn default_drep_skey_fp(n: i32) -> PathBuf {
    PathBuf::from("drep-keys")
        .join(format!("drep{n}"))
        .join("drep.skey")
}

/// Default committee verification + signing key paths.
///
/// Mirror of upstream `defaultCommitteeKeyPair`. Upstream types this
/// as a `KeyPair PaymentKey`.
pub fn default_committee_key_pair(n: i32) -> KeyPair<PaymentKey> {
    KeyPair::new(default_committee_vkey_fp(n), default_committee_skey_fp(n))
}

/// Relative SPO cold verification-key path.
///
/// Mirror of upstream `defaultSpoColdVKeyFp`.
pub fn default_spo_cold_vkey_fp(n: i32) -> PathBuf {
    default_spo_keys_dir(n).join("cold.vkey")
}

/// Relative SPO cold signing-key path.
///
/// Mirror of upstream `defaultSpoColdSKeyFp`.
pub fn default_spo_cold_skey_fp(n: i32) -> PathBuf {
    default_spo_keys_dir(n).join("cold.skey")
}

/// Default SPO name — `pool<n>`.
///
/// Mirror of upstream `defaultSpoName`.
pub fn default_spo_name(n: i32) -> String {
    format!("pool{n}")
}

/// Default SPO key directory — `pools-keys/pool<n>`.
///
/// Mirror of upstream `defaultSpoKeysDir`.
pub fn default_spo_keys_dir(n: i32) -> PathBuf {
    PathBuf::from("pools-keys").join(default_spo_name(n))
}

/// Default SPO cold verification + signing key paths.
///
/// Mirror of upstream `defaultSpoColdKeyPair`.
pub fn default_spo_cold_key_pair(n: i32) -> KeyPair<StakePoolKey> {
    KeyPair::new(default_spo_cold_vkey_fp(n), default_spo_cold_skey_fp(n))
}

/// Default cold, VRF, and staking key paths for one SPO.
///
/// Mirror of upstream `defaultSpoKeys`.
pub fn default_spo_keys(n: i32) -> SpoNodeKeys {
    let dir = default_spo_keys_dir(n);
    SpoNodeKeys {
        pool_node_keys_cold: default_spo_cold_key_pair(n),
        pool_node_keys_vrf: KeyPair::new(dir.join("vrf.vkey"), dir.join("vrf.skey")),
        pool_node_keys_staking: KeyPair::new(
            dir.join("staking-reward.vkey"),
            dir.join("staking-reward.skey"),
        ),
    }
}

/// Default stake-delegator verification + signing key paths.
///
/// Mirror of upstream `defaultDelegatorStakeKeyPair`.
pub fn default_delegator_stake_key_pair(n: i32) -> KeyPair<StakeKey> {
    let dir = PathBuf::from("stake-delegators").join(format!("delegator{n}"));
    KeyPair::new(dir.join("staking.vkey"), dir.join("staking.skey"))
}

/// Default UTxO verification + signing key paths.
///
/// Mirror of upstream `defaultUtxoKeys`.
pub fn default_utxo_keys(n: i32) -> KeyPair<PaymentKey> {
    KeyPair::new(default_utxo_vkey_path(n), default_utxo_skey_path(n))
}

/// Default mainnet P2P topology used by `cardano-testnet`.
///
/// Mirror of upstream `defaultMainnetTopology`.
pub fn default_mainnet_topology() -> TopologyConfig {
    TopologyConfig {
        local_roots: vec![LocalRootConfig {
            access_points: vec![PeerAccessPoint {
                address: "relays-new.cardano-mainnet.iohk.io".to_string(),
                port: 3_001,
            }],
            advertise: true,
            trustable: true,
            hot_valency: 2,
            warm_valency: Some(2),
            diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
        }],
        bootstrap_peers: UseBootstrapPeers::DontUseBootstrapPeers,
        public_roots: Vec::new(),
        use_ledger_peers: UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
        peer_snapshot_file: None,
    }
}

/// Default P2P topology for local producers.
///
/// Mirror of upstream `defaultP2PTopology`.
pub fn default_p2p_topology(addresses: Vec<PeerAccessPoint>) -> TopologyConfig {
    let valency =
        u16::try_from(addresses.len()).expect("defaultP2PTopology address count exceeds u16");
    TopologyConfig {
        local_roots: vec![LocalRootConfig {
            access_points: addresses,
            advertise: false,
            trustable: true,
            hot_valency: valency,
            warm_valency: Some(valency),
            diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
        }],
        bootstrap_peers: UseBootstrapPeers::DontUseBootstrapPeers,
        public_roots: vec![PublicRootConfig {
            access_points: Vec::new(),
            advertise: false,
        }],
        use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
        peer_snapshot_file: None,
    }
}

fn number(n: i64) -> Value {
    Value::Number(Number::from(n))
}

fn insert_entries(
    map: &mut Map<String, Value>,
    entries: impl IntoIterator<Item = (&'static str, Value)>,
) {
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
}

/// Base node configuration generated by `cardano-testnet`.
///
/// Mirror of upstream `defaultYamlConfig`. This is the era-independent
/// portion; use [`default_yaml_hardfork_via_config`] for the complete
/// config used when starting directly in a Shelley-based era.
pub fn default_yaml_config() -> Map<String, Value> {
    let mut config = Map::new();
    insert_entries(
        &mut config,
        [
            ("Protocol", json!("Cardano")),
            ("SocketPath", json!("db/node.socket")),
            ("PBftSignatureThreshold", json!(0.6)),
            ("minSeverity", json!("Debug")),
            ("EnableLogMetrics", json!(false)),
            ("TurnOnLogMetrics", json!(false)),
            ("MaxConcurrencyBulkSync", number(1)),
            ("MaxConcurrencyDeadline", number(2)),
            ("EnableLogging", json!(true)),
            (
                "ByronGenesisFile",
                json!(default_genesis_filepath(CardanoEra::Byron)),
            ),
            (
                "ShelleyGenesisFile",
                json!(default_genesis_filepath(CardanoEra::Shelley)),
            ),
            (
                "AlonzoGenesisFile",
                json!(default_genesis_filepath(CardanoEra::Alonzo)),
            ),
            (
                "ConwayGenesisFile",
                json!(default_genesis_filepath(CardanoEra::Conway)),
            ),
            ("DijkstraGenesisFile", json!("dijkstra-genesis.json")),
            ("RequiresNetworkMagic", json!("RequiresMagic")),
            ("PeerSharing", json!(false)),
            (
                "setupScribes",
                json!([
                    {
                        "scKind": "FileSK",
                        "scName": "logs/node.log",
                        "scFormat": "ScJson"
                    },
                    {
                        "scKind": "StdoutSK",
                        "scName": "stdout",
                        "scFormat": "ScJson"
                    }
                ]),
            ),
            (
                "rotation",
                json!({
                    "rpLogLimitBytes": 5_000_000,
                    "rpKeepFilesNum": 3,
                    "rpMaxAgeHours": 24
                }),
            ),
            (
                "defaultScribes",
                json!([["FileSK", "logs/mainnet.log"], ["StdoutSK", "stdout"]]),
            ),
            ("setupBackends", json!(["KatipBK"])),
            ("defaultBackends", json!(["KatipBK"])),
            ("options", Value::Object(Map::new())),
        ],
    );
    config
}

fn last_known_block_version_major(era: ShelleyBasedEra) -> i64 {
    match era {
        ShelleyBasedEra::Shelley => 2,
        ShelleyBasedEra::Allegra => 3,
        ShelleyBasedEra::Mary => 4,
        ShelleyBasedEra::Alonzo => 5,
        ShelleyBasedEra::Babbage => 8,
        ShelleyBasedEra::Conway => 9,
    }
}

fn hardfork_keys_through(era: ShelleyBasedEra) -> &'static [&'static str] {
    match era {
        ShelleyBasedEra::Shelley => &["TestShelleyHardForkAtEpoch"],
        ShelleyBasedEra::Allegra => &["TestShelleyHardForkAtEpoch", "TestAllegraHardForkAtEpoch"],
        ShelleyBasedEra::Mary => &[
            "TestShelleyHardForkAtEpoch",
            "TestAllegraHardForkAtEpoch",
            "TestMaryHardForkAtEpoch",
        ],
        ShelleyBasedEra::Alonzo => &[
            "TestShelleyHardForkAtEpoch",
            "TestAllegraHardForkAtEpoch",
            "TestMaryHardForkAtEpoch",
            "TestAlonzoHardForkAtEpoch",
        ],
        ShelleyBasedEra::Babbage => &[
            "TestShelleyHardForkAtEpoch",
            "TestAllegraHardForkAtEpoch",
            "TestMaryHardForkAtEpoch",
            "TestAlonzoHardForkAtEpoch",
            "TestBabbageHardForkAtEpoch",
        ],
        ShelleyBasedEra::Conway => &[
            "TestShelleyHardForkAtEpoch",
            "TestAllegraHardForkAtEpoch",
            "TestMaryHardForkAtEpoch",
            "TestAlonzoHardForkAtEpoch",
            "TestBabbageHardForkAtEpoch",
            "TestConwayHardForkAtEpoch",
        ],
    }
}

fn tracing_overrides() -> Map<String, Value> {
    let mut tracers = Map::new();
    insert_entries(
        &mut tracers,
        [
            ("TraceBlockFetchClient", json!(false)),
            ("TraceBlockFetchDecisions", json!(false)),
            ("TraceBlockFetchProtocol", json!(false)),
            ("TraceBlockFetchProtocolSerialised", json!(false)),
            ("TraceBlockFetchServer", json!(false)),
            ("TraceBlockchainTime", json!(true)),
            ("TraceChainDB", json!(true)),
            ("TraceChainSyncClient", json!(false)),
            ("TraceChainSyncBlockServer", json!(false)),
            ("TraceChainSyncHeaderServer", json!(false)),
            ("TraceChainSyncProtocol", json!(false)),
            ("TraceDnsResolver", json!(true)),
            ("TraceDnsSubscription", json!(true)),
            ("TraceErrorPolicy", json!(true)),
            ("TraceLocalErrorPolicy", json!(true)),
            ("TraceForge", json!(true)),
            ("TraceHandshake", json!(false)),
            ("TraceIpSubscription", json!(true)),
            ("TraceLocalRootPeers", json!(true)),
            ("TracePublicRootPeers", json!(true)),
            ("TracePeerSelection", json!(true)),
            ("TracePeerSelectionActions", json!(true)),
            ("TraceConnectionManager", json!(true)),
            ("TraceServer", json!(true)),
            ("TraceLocalConnectionManager", json!(false)),
            ("TraceLocalServer", json!(false)),
            ("TraceLocalChainSyncProtocol", json!(false)),
            ("TraceLocalHandshake", json!(false)),
            ("TraceLocalTxSubmissionProtocol", json!(false)),
            ("TraceLocalTxSubmissionServer", json!(false)),
            ("TraceMempool", json!(true)),
            ("TraceMux", json!(false)),
            ("TraceTxInbound", json!(false)),
            ("TraceTxOutbound", json!(false)),
            ("TraceTxSubmissionProtocol", json!(false)),
        ],
    );
    tracers
}

/// Complete YAML/JSON node configuration for a direct hardfork into
/// the selected Shelley-based era.
///
/// Mirror of upstream `defaultYamlHardforkViaConfig`: base config,
/// tracer switches, empty `TraceOptions`, last-known block-version
/// fields, `ExperimentalProtocolsEnabled`, and cumulative
/// `Test*HardForkAtEpoch = 0` switches through the selected era.
pub fn default_yaml_hardfork_via_config(era: ShelleyBasedEra) -> Map<String, Value> {
    let mut config = default_yaml_config();
    config.extend(tracing_overrides());
    config.insert("TraceOptions".to_string(), Value::Object(Map::new()));
    config.insert(
        "LastKnownBlockVersion-Major".to_string(),
        number(last_known_block_version_major(era)),
    );
    config.insert("LastKnownBlockVersion-Minor".to_string(), number(0));
    config.insert("LastKnownBlockVersion-Alt".to_string(), number(0));
    config.insert("ExperimentalProtocolsEnabled".to_string(), json!(true));
    for key in hardfork_keys_through(era) {
        config.insert((*key).to_string(), number(0));
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_script_embeds_the_signer_hash() {
        let s = simple_script("deadbeef");
        assert!(s.contains(r#""keyHash": "deadbeef""#), "got: {s}");
        assert!(s.contains(r#""type": "sig""#));
        assert!(s.contains(r#""type": "all""#));
    }

    #[test]
    fn plutus_scripts_are_text_envelopes() {
        assert!(PLUTUS_V2_SCRIPT.contains(r#""type": "PlutusScriptV2""#));
        assert!(PLUTUS_V2_SCRIPT.contains("cborHex"));
        assert!(PLUTUS_V3_SCRIPT.contains(r#""type": "PlutusScriptV3""#));
        assert!(PLUTUS_V3_SCRIPT.contains(r#""cborHex": "46450101002499""#));
    }

    #[test]
    fn plutus_scripts_parse_as_json() {
        let v2: serde_json::Value = serde_json::from_str(PLUTUS_V2_SCRIPT).expect("V2 is JSON");
        assert_eq!(v2["type"], "PlutusScriptV2");
        let v3: serde_json::Value = serde_json::from_str(PLUTUS_V3_SCRIPT).expect("V3 is JSON");
        assert_eq!(v3["type"], "PlutusScriptV3");
    }

    #[test]
    fn default_genesis_filepath_matches_era_to_string() {
        assert_eq!(
            default_genesis_filepath(CardanoEra::Byron),
            "byron-genesis.json"
        );
        assert_eq!(
            default_genesis_filepath(CardanoEra::Conway),
            "conway-genesis.json"
        );
    }

    #[test]
    fn committee_key_paths_match_upstream_defaults() {
        assert_eq!(
            default_committee_vkey_fp(2),
            PathBuf::from("committee-keys/committee2.vkey")
        );
        assert_eq!(
            default_committee_skey_fp(2),
            PathBuf::from("committee-keys/committee2.skey")
        );

        let pair = default_committee_key_pair(2);
        assert_eq!(
            pair.verification_key_fp(),
            PathBuf::from("committee-keys/committee2.vkey").as_path()
        );
        assert_eq!(
            pair.signing_key_fp(),
            PathBuf::from("committee-keys/committee2.skey").as_path()
        );
    }

    #[test]
    fn drep_key_paths_match_upstream_defaults() {
        assert_eq!(
            default_drep_skey_fp(3),
            PathBuf::from("drep-keys/drep3/drep.skey")
        );

        let pair = default_drep_key_pair(3);
        assert_eq!(
            pair.verification_key_fp(),
            PathBuf::from("drep-keys/drep3/drep.vkey").as_path()
        );
        assert_eq!(
            pair.signing_key_fp(),
            PathBuf::from("drep-keys/drep3/drep.skey").as_path()
        );
    }

    #[test]
    fn spo_key_paths_match_upstream_defaults() {
        assert_eq!(default_spo_name(4), "pool4");
        assert_eq!(default_spo_keys_dir(4), PathBuf::from("pools-keys/pool4"));
        assert_eq!(
            default_spo_cold_vkey_fp(4),
            PathBuf::from("pools-keys/pool4/cold.vkey")
        );
        assert_eq!(
            default_spo_cold_skey_fp(4),
            PathBuf::from("pools-keys/pool4/cold.skey")
        );

        let cold = default_spo_cold_key_pair(4);
        assert_eq!(
            cold.verification_key_fp(),
            PathBuf::from("pools-keys/pool4/cold.vkey").as_path()
        );
        assert_eq!(
            cold.signing_key_fp(),
            PathBuf::from("pools-keys/pool4/cold.skey").as_path()
        );

        let keys = default_spo_keys(4);
        assert_eq!(
            keys.pool_node_keys_cold.verification_key_fp(),
            PathBuf::from("pools-keys/pool4/cold.vkey").as_path()
        );
        assert_eq!(
            keys.pool_node_keys_vrf.verification_key_fp(),
            PathBuf::from("pools-keys/pool4/vrf.vkey").as_path()
        );
        assert_eq!(
            keys.pool_node_keys_vrf.signing_key_fp(),
            PathBuf::from("pools-keys/pool4/vrf.skey").as_path()
        );
        assert_eq!(
            keys.pool_node_keys_staking.verification_key_fp(),
            PathBuf::from("pools-keys/pool4/staking-reward.vkey").as_path()
        );
        assert_eq!(
            keys.pool_node_keys_staking.signing_key_fp(),
            PathBuf::from("pools-keys/pool4/staking-reward.skey").as_path()
        );
    }

    #[test]
    fn delegator_and_utxo_key_paths_match_upstream_defaults() {
        let delegator = default_delegator_stake_key_pair(5);
        assert_eq!(
            delegator.verification_key_fp(),
            PathBuf::from("stake-delegators/delegator5/staking.vkey").as_path()
        );
        assert_eq!(
            delegator.signing_key_fp(),
            PathBuf::from("stake-delegators/delegator5/staking.skey").as_path()
        );

        let utxo = default_utxo_keys(6);
        assert_eq!(
            utxo.verification_key_fp(),
            PathBuf::from("utxo-keys/utxo6/utxo.vkey").as_path()
        );
        assert_eq!(
            utxo.signing_key_fp(),
            PathBuf::from("utxo-keys/utxo6/utxo.skey").as_path()
        );
    }

    #[test]
    fn default_mainnet_topology_matches_upstream_defaults() {
        let topology = default_mainnet_topology();
        assert!(matches!(
            topology.bootstrap_peers,
            UseBootstrapPeers::DontUseBootstrapPeers
        ));
        assert!(topology.public_roots.is_empty());
        assert_eq!(
            topology.use_ledger_peers,
            UseLedgerPeers::UseLedgerPeers(AfterSlot::Always)
        );
        assert!(topology.peer_snapshot_file.is_none());

        let [local_root] = topology.local_roots.as_slice() else {
            panic!("expected one local root group");
        };
        assert_eq!(local_root.access_points.len(), 1);
        assert_eq!(
            local_root.access_points[0],
            PeerAccessPoint {
                address: "relays-new.cardano-mainnet.iohk.io".to_string(),
                port: 3_001
            }
        );
        assert!(local_root.advertise);
        assert!(local_root.trustable);
        assert_eq!(local_root.hot_valency, 2);
        assert_eq!(local_root.warm_valency, Some(2));
        assert_eq!(
            local_root.diffusion_mode,
            PeerDiffusionMode::InitiatorAndResponderDiffusionMode
        );
    }

    #[test]
    fn default_p2p_topology_matches_upstream_defaults() {
        let producers = vec![
            PeerAccessPoint {
                address: "127.0.0.1".to_string(),
                port: 30_001,
            },
            PeerAccessPoint {
                address: "127.0.0.1".to_string(),
                port: 30_002,
            },
        ];

        let topology = default_p2p_topology(producers.clone());
        assert!(matches!(
            topology.bootstrap_peers,
            UseBootstrapPeers::DontUseBootstrapPeers
        ));
        assert_eq!(
            topology.use_ledger_peers,
            UseLedgerPeers::DontUseLedgerPeers
        );
        assert!(topology.peer_snapshot_file.is_none());

        let [local_root] = topology.local_roots.as_slice() else {
            panic!("expected one local root group");
        };
        assert_eq!(local_root.access_points, producers);
        assert!(!local_root.advertise);
        assert!(local_root.trustable);
        assert_eq!(local_root.hot_valency, 2);
        assert_eq!(local_root.warm_valency, Some(2));
        assert_eq!(
            local_root.diffusion_mode,
            PeerDiffusionMode::InitiatorAndResponderDiffusionMode
        );

        let [public_root] = topology.public_roots.as_slice() else {
            panic!("expected one empty public root group");
        };
        assert!(public_root.access_points.is_empty());
        assert!(!public_root.advertise);
    }

    #[test]
    fn default_topologies_serialize_with_upstream_optional_field_omissions() {
        let mainnet = serde_json::to_value(default_mainnet_topology()).expect("serialize mainnet");
        let mainnet = mainnet.as_object().expect("mainnet topology object");
        assert!(!mainnet.contains_key("bootstrapPeers"));
        assert!(!mainnet.contains_key("peerSnapshotFile"));
        assert_eq!(mainnet["useLedgerAfterSlot"], json!(0));

        let p2p = serde_json::to_value(default_p2p_topology(Vec::new())).expect("serialize p2p");
        let p2p = p2p.as_object().expect("p2p topology object");
        assert!(!p2p.contains_key("bootstrapPeers"));
        assert!(!p2p.contains_key("peerSnapshotFile"));
        assert_eq!(p2p["useLedgerAfterSlot"], json!(-1));
    }

    #[test]
    fn default_yaml_config_contains_upstream_base_keys() {
        let config = default_yaml_config();
        assert_eq!(config["Protocol"], json!("Cardano"));
        assert_eq!(config["SocketPath"], json!("db/node.socket"));
        assert_eq!(config["RequiresNetworkMagic"], json!("RequiresMagic"));
        assert_eq!(config["PeerSharing"], json!(false));
        assert_eq!(config["ByronGenesisFile"], json!("byron-genesis.json"));
        assert_eq!(config["ShelleyGenesisFile"], json!("shelley-genesis.json"));
        assert_eq!(config["AlonzoGenesisFile"], json!("alonzo-genesis.json"));
        assert_eq!(config["ConwayGenesisFile"], json!("conway-genesis.json"));
        assert_eq!(
            config["DijkstraGenesisFile"],
            json!("dijkstra-genesis.json")
        );
        assert_eq!(config["rotation"]["rpLogLimitBytes"], json!(5_000_000));
        assert_eq!(config["setupBackends"], json!(["KatipBK"]));
    }

    #[test]
    fn default_yaml_hardfork_via_config_sets_protocol_version_by_era() {
        let cases = [
            (ShelleyBasedEra::Shelley, 2),
            (ShelleyBasedEra::Allegra, 3),
            (ShelleyBasedEra::Mary, 4),
            (ShelleyBasedEra::Alonzo, 5),
            (ShelleyBasedEra::Babbage, 8),
            (ShelleyBasedEra::Conway, 9),
        ];
        for (era, major) in cases {
            let config = default_yaml_hardfork_via_config(era);
            assert_eq!(config["LastKnownBlockVersion-Major"], json!(major));
            assert_eq!(config["LastKnownBlockVersion-Minor"], json!(0));
            assert_eq!(config["LastKnownBlockVersion-Alt"], json!(0));
            assert_eq!(config["ExperimentalProtocolsEnabled"], json!(true));
            assert_eq!(config["TraceOptions"], Value::Object(Map::new()));
        }
    }

    #[test]
    fn default_yaml_hardfork_via_config_enables_cumulative_hardfork_keys() {
        let conway = default_yaml_hardfork_via_config(ShelleyBasedEra::Conway);
        for key in [
            "TestShelleyHardForkAtEpoch",
            "TestAllegraHardForkAtEpoch",
            "TestMaryHardForkAtEpoch",
            "TestAlonzoHardForkAtEpoch",
            "TestBabbageHardForkAtEpoch",
            "TestConwayHardForkAtEpoch",
        ] {
            assert_eq!(conway[key], json!(0), "missing {key}");
        }

        let mary = default_yaml_hardfork_via_config(ShelleyBasedEra::Mary);
        assert!(mary.contains_key("TestMaryHardForkAtEpoch"));
        assert!(!mary.contains_key("TestAlonzoHardForkAtEpoch"));
    }

    #[test]
    fn default_yaml_hardfork_via_config_carries_upstream_tracer_switches() {
        let config = default_yaml_hardfork_via_config(ShelleyBasedEra::Conway);
        assert_eq!(config["TraceBlockchainTime"], json!(true));
        assert_eq!(config["TraceChainDB"], json!(true));
        assert_eq!(config["TraceBlockFetchClient"], json!(false));
        assert_eq!(config["TraceLocalTxSubmissionServer"], json!(false));
        assert_eq!(config["TraceMempool"], json!(true));
    }
}
