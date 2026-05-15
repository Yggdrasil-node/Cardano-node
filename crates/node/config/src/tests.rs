// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;

#[test]
fn mainnet_network_magic_constant_matches_upstream() {
    // Upstream `cardano-node` `Cardano.Chain.Genesis.Data`
    // `protocolMagicId = 764824073`. This constant MUST NOT drift —
    // every NtN / NtC handshake verifies it byte-for-byte, so any
    // other value produces a silently-incompatible client.
    assert_eq!(MAINNET_NETWORK_MAGIC, 764_824_073);
}

#[test]
fn preprod_network_magic_constant_matches_upstream() {
    // Upstream `cardano-configurations` preprod
    // `shelley-genesis.json` `networkMagic = 1`.
    assert_eq!(PREPROD_NETWORK_MAGIC, 1);
}

#[test]
fn preview_network_magic_constant_matches_upstream() {
    // Upstream `cardano-configurations` preview
    // `shelley-genesis.json` `networkMagic = 2`.
    assert_eq!(PREVIEW_NETWORK_MAGIC, 2);
}

#[test]
fn all_three_network_magics_are_distinct() {
    // Defensive invariant: if any two preset magics ever collide,
    // handshake disambiguation breaks. Upstream guarantees all three
    // are distinct; pin it locally so a typo on any constant fails.
    assert_ne!(MAINNET_NETWORK_MAGIC, PREPROD_NETWORK_MAGIC);
    assert_ne!(MAINNET_NETWORK_MAGIC, PREVIEW_NETWORK_MAGIC);
    assert_ne!(PREPROD_NETWORK_MAGIC, PREVIEW_NETWORK_MAGIC);
}

#[test]
fn preset_configs_use_canonical_magic_constants() {
    // Pin `preprod_config()` / `preview_config()` → their canonical
    // constants so a future refactor re-inlining the literals fails CI.
    assert_eq!(preprod_config().network_magic, PREPROD_NETWORK_MAGIC);
    assert_eq!(preview_config().network_magic, PREVIEW_NETWORK_MAGIC);
}

#[test]
fn network_preset_network_magic_matches_to_config_for_all_presets() {
    // Cheap accessor (`network_magic()`) and full constructor
    // (`to_config().network_magic`) MUST agree for every preset.
    // A drift here would mean preflight code (which uses the cheap
    // accessor) and the actual node startup (which uses `to_config`)
    // disagree on the network — silently producing handshake
    // failures on real connections.
    for &preset in NetworkPreset::all() {
        assert_eq!(
            preset.network_magic(),
            preset.to_config().network_magic,
            "preset {preset:?}: cheap accessor disagrees with to_config()",
        );
    }
}

#[test]
fn default_governor_target_fns_match_governor_targets_default() {
    // The six `default_governor_target_*` serde-default functions
    // MUST agree with the corresponding fields of
    // `GovernorTargets::default()`. Drift here would mean a freshly
    // parsed config (uses serde defaults) and a hand-constructed
    // `GovernorTargets::default()` (used internally by the governor)
    // disagree on peer-selection targets — silently producing
    // different peer-governor behavior. Pinning both sides here
    // turns the drift into a CI failure.
    use yggdrasil_network::GovernorTargets;

    let defaults = GovernorTargets::default();
    assert_eq!(default_governor_target_known(), defaults.target_known);
    assert_eq!(
        default_governor_target_established(),
        defaults.target_established,
    );
    assert_eq!(default_governor_target_active(), defaults.target_active);
    assert_eq!(
        default_governor_target_known_big_ledger(),
        defaults.target_known_big_ledger,
    );
    assert_eq!(
        default_governor_target_established_big_ledger(),
        defaults.target_established_big_ledger,
    );
    assert_eq!(
        default_governor_target_active_big_ledger(),
        defaults.target_active_big_ledger,
    );
}

#[test]
fn default_governor_targets_are_sane() {
    // Pins that the config defaults themselves satisfy
    // `sanePeerSelectionTargets` — a preflight failure here would
    // mean every fresh install hits the slice-40 `insane governor
    // targets` warning right out of the box. Belt-and-braces next
    // to slice 60's direct unit coverage of `is_sane`.
    use yggdrasil_network::GovernorTargets;
    let defaults = GovernorTargets {
        target_known: default_governor_target_known(),
        target_established: default_governor_target_established(),
        target_active: default_governor_target_active(),
        target_known_big_ledger: default_governor_target_known_big_ledger(),
        target_established_big_ledger: default_governor_target_established_big_ledger(),
        target_active_big_ledger: default_governor_target_active_big_ledger(),
        ..Default::default()
    };
    assert!(
        defaults.is_sane(),
        "default governor targets must be sane — they are the fresh-install baseline",
    );
}

#[test]
fn conway_major_protocol_version_constant_matches_upstream_default() {
    // Pin the Conway-era default `MaxMajorProtVer` against the
    // upstream value. Also pin the relationship to
    // `default_max_major_protocol_version()` so a refactor that
    // inlines the value in one place but not the other fails CI.
    assert_eq!(CONWAY_MAJOR_PROTOCOL_VERSION, 10);
    assert_eq!(
        default_max_major_protocol_version(),
        CONWAY_MAJOR_PROTOCOL_VERSION
    );
}

#[test]
fn preset_configs_use_conway_major_protocol_version() {
    // All three presets default to Conway-era; each `max_major` must
    // route through the named constant.
    assert_eq!(
        mainnet_config().max_major_protocol_version,
        CONWAY_MAJOR_PROTOCOL_VERSION,
    );
    assert_eq!(
        preprod_config().max_major_protocol_version,
        CONWAY_MAJOR_PROTOCOL_VERSION,
    );
    assert_eq!(
        preview_config().max_major_protocol_version,
        CONWAY_MAJOR_PROTOCOL_VERSION,
    );
}

/// Pin the canonical default for `max_concurrent_block_fetch_peers`
/// across every preset.
///
/// The default `2` matches upstream
/// `Ouroboros.Network.BlockFetch.Decision`'s `bfcMaxConcurrencyBulkSync`
/// — the canonical initial-sync concurrency cap. R218 verified the
/// multi-peer runtime path on mainnet (67% throughput gain). Drift
/// here without a corresponding upstream change would silently shift
/// every preset off the parity-aligned default; iterates
/// `NetworkPreset::all()` to catch the case where someone bumps
/// mainnet but forgets preprod/preview.
#[test]
fn preset_configs_share_canonical_max_concurrent_block_fetch_peers() {
    for &preset in NetworkPreset::all() {
        let cfg = preset.to_config();
        assert_eq!(
            cfg.max_concurrent_block_fetch_peers, 2,
            "preset {preset:?}: max_concurrent_block_fetch_peers must default to 2 \
                 (matches upstream bfcMaxConcurrencyBulkSync; R218 evidence)",
        );
    }
}

/// Pin that the `default_max_concurrent_block_fetch_peers()`
/// fallback used by serde matches the explicit per-preset default.
/// Drift between serde-default (parsed-from-disk path) and the
/// preset constructors (in-process default path) would silently
/// produce different runtime behaviour for the same nominal preset.
#[test]
fn default_max_concurrent_block_fetch_peers_matches_preset_value() {
    assert_eq!(default_max_concurrent_block_fetch_peers(), 2);
    for &preset in NetworkPreset::all() {
        assert_eq!(
            preset.to_config().max_concurrent_block_fetch_peers,
            default_max_concurrent_block_fetch_peers(),
            "preset {preset:?}: in-process default must match serde-default",
        );
    }
}

#[test]
fn preset_configs_share_canonical_protocol_versions() {
    // The three preset constructors (`mainnet_config`, `preprod_config`,
    // `preview_config`) each independently hand-code
    // `protocol_versions: vec![13, 14]`. Drift between them would mean
    // a freshly bootstrapped mainnet relay proposes a different NtN
    // version range than a preprod relay built from the same binary
    // — silently producing handshake mismatches that look like
    // peer-misbehaviour at the operator level. This test iterates
    // every preset via `NetworkPreset::all()` (slice 82) and asserts
    // each preset's `protocol_versions` is identical to mainnet's.
    // A divergent edit to ANY single preset fails CI naming the
    // offending preset.
    let mainnet = mainnet_config().protocol_versions.clone();
    for &preset in NetworkPreset::all() {
        let cfg = preset.to_config();
        assert_eq!(
            cfg.protocol_versions, mainnet,
            "preset {preset:?}: protocol_versions {:?} drifted from mainnet {:?}",
            cfg.protocol_versions, mainnet,
        );
    }
}

#[test]
fn preset_configs_protocol_versions_match_named_handshake_constants() {
    // Every entry in the canonical `protocol_versions` vector MUST
    // correspond to one of the named NtN handshake constants
    // (`HandshakeVersion::V13`, `V14`, `V15` — see slice 88). A
    // typo like `vec![13, 41]` (transposed digits) would otherwise
    // pass the slice-82 cross-preset check (since all three would
    // share the same typo) but break real handshake negotiation
    // because tag 41 is not a known NtN protocol version.
    //
    // Pinning the literal `[13, 14]` AND cross-asserting against the
    // named constants composes the two slice-88 named constants with
    // the canonical preset content, so a future bump of the proposed
    // version range (e.g. adding V15 once Conway+1 is live) is a
    // single coordinated edit: update the preset constructors,
    // update this test's `expected` array, and the named constants
    // already exist.
    use yggdrasil_network::HandshakeVersion;

    let expected: Vec<u32> = vec![
        HandshakeVersion::V13.0 as u32,
        HandshakeVersion::V14.0 as u32,
    ];
    assert_eq!(
        expected,
        vec![13_u32, 14_u32],
        "named NtN constants must correspond to literal protocol versions 13/14",
    );
    assert_eq!(
        mainnet_config().protocol_versions,
        expected,
        "mainnet preset must propose exactly the named NtN constants",
    );
}

#[test]
fn mainnet_network_id_constant_matches_upstream() {
    // Upstream `Cardano.Ledger.Api.Tx.Address`: `Network = Mainnet`
    // encodes to `1` in the high nibble of every reward / Shelley
    // address. Drift would silently misclassify mainnet addresses
    // as testnet (or vice versa) at value-preservation time.
    assert_eq!(MAINNET_NETWORK_ID, 1);
    assert_eq!(TESTNET_NETWORK_ID, 0);
}

#[test]
fn expected_network_id_uses_named_constants_consistently() {
    // Mainnet config → MAINNET_NETWORK_ID, every test net → TESTNET_NETWORK_ID.
    // Pin both branches so a refactor that flips one fails CI.
    assert_eq!(mainnet_config().expected_network_id(), MAINNET_NETWORK_ID);
    assert_eq!(preprod_config().expected_network_id(), TESTNET_NETWORK_ID);
    assert_eq!(preview_config().expected_network_id(), TESTNET_NETWORK_ID);

    // Custom magic → testnet (defensive: any non-mainnet magic
    // including the canonical preprod/preview values must classify
    // as testnet).
    let mut cfg = mainnet_config();
    cfg.network_magic = 99_999;
    assert_eq!(cfg.expected_network_id(), TESTNET_NETWORK_ID);
}

#[test]
fn network_preset_network_magic_returns_named_constants() {
    // Pin the cheap accessor's output to the named constants so a
    // refactor that inlines the literal in one branch (and not the
    // others) fails CI.
    assert_eq!(
        NetworkPreset::Mainnet.network_magic(),
        MAINNET_NETWORK_MAGIC
    );
    assert_eq!(
        NetworkPreset::Preprod.network_magic(),
        PREPROD_NETWORK_MAGIC
    );
    assert_eq!(
        NetworkPreset::Preview.network_magic(),
        PREVIEW_NETWORK_MAGIC
    );
}

#[test]
fn mainnet_config_uses_canonical_magic_constant() {
    // Pin the `mainnet_config()` → `MAINNET_NETWORK_MAGIC` link so a
    // future refactor that accidentally hard-codes a different value
    // inline in the constructor fails CI.
    let cfg = mainnet_config();
    assert_eq!(cfg.network_magic, MAINNET_NETWORK_MAGIC);
}

#[test]
fn requires_network_magic_default_pins_constant() {
    // Mainnet → RequiresNoMagic, anything else → RequiresMagic. Pin
    // both branches so a regression flipping the constant silently
    // loses mainnet Byron-header-decode compatibility.
    assert_eq!(
        RequiresNetworkMagic::default_for_magic(MAINNET_NETWORK_MAGIC),
        RequiresNetworkMagic::RequiresNoMagic,
    );
    assert_eq!(
        RequiresNetworkMagic::default_for_magic(MAINNET_NETWORK_MAGIC + 1),
        RequiresNetworkMagic::RequiresMagic,
    );
    assert_eq!(
        RequiresNetworkMagic::default_for_magic(2),
        RequiresNetworkMagic::RequiresMagic,
    );
}

#[test]
fn default_config_round_trips_json() {
    let cfg = default_config();
    let json = serde_json::to_string_pretty(&cfg).expect("serialize");
    let parsed: NodeConfigFile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.network_magic, cfg.network_magic);
    assert_eq!(parsed.peer_addr, cfg.peer_addr);
    assert_eq!(parsed.bootstrap_peers, cfg.bootstrap_peers);
    assert_eq!(parsed.local_roots, cfg.local_roots);
    assert_eq!(parsed.public_roots, cfg.public_roots);
    assert_eq!(parsed.use_ledger_after_slot, cfg.use_ledger_after_slot);
    assert_eq!(parsed.peer_snapshot_file, cfg.peer_snapshot_file);
    assert_eq!(parsed.storage_dir, cfg.storage_dir);
    assert_eq!(
        parsed.checkpoint_interval_slots,
        cfg.checkpoint_interval_slots
    );
    assert_eq!(parsed.max_ledger_snapshots, cfg.max_ledger_snapshots);
    assert_eq!(
        parsed.governor_tick_interval_secs,
        cfg.governor_tick_interval_secs
    );
    assert_eq!(parsed.governor_target_known, cfg.governor_target_known);
    assert_eq!(
        parsed.governor_target_established,
        cfg.governor_target_established
    );
    assert_eq!(parsed.governor_target_active, cfg.governor_target_active);
    assert_eq!(
        parsed.governor_target_known_big_ledger,
        cfg.governor_target_known_big_ledger
    );
    assert_eq!(
        parsed.governor_target_established_big_ledger,
        cfg.governor_target_established_big_ledger
    );
    assert_eq!(
        parsed.governor_target_active_big_ledger,
        cfg.governor_target_active_big_ledger
    );
    assert_eq!(parsed.peer_sharing, cfg.peer_sharing);
    assert_eq!(parsed.consensus_mode, cfg.consensus_mode);
    assert_eq!(parsed.turn_on_logging, cfg.turn_on_logging);
    assert_eq!(parsed.use_trace_dispatcher, cfg.use_trace_dispatcher);
    assert_eq!(parsed.trace_option_node_name, cfg.trace_option_node_name);
    assert_eq!(parsed.trace_options, cfg.trace_options);
    assert_eq!(parsed.shelley_genesis_file, cfg.shelley_genesis_file);
    assert_eq!(parsed.alonzo_genesis_file, cfg.alonzo_genesis_file);
    assert_eq!(parsed.conway_genesis_file, cfg.conway_genesis_file);
    assert_eq!(
        parsed.shelley_operational_certificate_issuer_vkey,
        cfg.shelley_operational_certificate_issuer_vkey
    );
}

#[test]
fn minimal_config_uses_defaults() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert!(cfg.bootstrap_peers.is_empty());
    assert!(cfg.local_roots.is_empty());
    assert!(cfg.public_roots.is_empty());
    assert!(cfg.use_ledger_after_slot.is_none());
    assert!(cfg.peer_snapshot_file.is_none());
    assert_eq!(cfg.storage_dir, PathBuf::from("data"));
    assert_eq!(cfg.checkpoint_interval_slots, 2160);
    assert_eq!(cfg.max_ledger_snapshots, 8);
    assert_eq!(cfg.slots_per_kes_period, 129_600);
    assert_eq!(cfg.max_kes_evolutions, 62);
    assert_eq!(cfg.epoch_length, 432_000);
    assert_eq!(cfg.security_param_k, 2160);
    assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
    assert!(cfg.keepalive_interval_secs.is_none());
    assert_eq!(cfg.peer_sharing, 1);
    assert_eq!(cfg.consensus_mode, ConsensusModeConfig::PraosMode);
    assert_eq!(cfg.governor_tick_interval_secs, 5);
    assert_eq!(cfg.governor_target_known, 20);
    assert_eq!(cfg.governor_target_established, 10);
    assert_eq!(cfg.governor_target_active, 5);
    assert_eq!(cfg.governor_target_known_big_ledger, 0);
    assert_eq!(cfg.governor_target_established_big_ledger, 0);
    assert_eq!(cfg.governor_target_active_big_ledger, 0);
    assert!(cfg.turn_on_logging);
    assert!(cfg.use_trace_dispatcher);
    assert!(cfg.turn_on_log_metrics);
    assert!(cfg.trace_option_node_name.is_none());
    assert!(cfg.shelley_genesis_file.is_none());
    assert!(cfg.alonzo_genesis_file.is_none());
    assert!(cfg.conway_genesis_file.is_none());
    assert!(cfg.shelley_operational_certificate_issuer_vkey.is_none());
    assert!(cfg.trace_options.contains_key(""));
    assert!(cfg.trace_options.contains_key("Node.Recovery.Checkpoint"));
    assert_eq!(
        cfg.trace_options
            .get("Node.Recovery.Checkpoint")
            .expect("checkpoint trace options")
            .max_frequency,
        Some(1.0)
    );
}

#[test]
fn config_parses_big_ledger_governor_targets() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "governor_target_known_big_ledger": 8,
            "governor_target_established_big_ledger": 3,
            "governor_target_active_big_ledger": 1
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");

    assert_eq!(cfg.governor_target_known_big_ledger, 8);
    assert_eq!(cfg.governor_target_established_big_ledger, 3);
    assert_eq!(cfg.governor_target_active_big_ledger, 1);
}

#[test]
fn config_parses_upstream_genesis_hash_aliases() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "ShelleyGenesisFile": "shelley-genesis.json",
            "ShelleyGenesisHash": "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81",
            "AlonzoGenesisFile": "alonzo-genesis.json",
            "AlonzoGenesisHash": "7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874",
            "ConwayGenesisFile": "conway-genesis.json",
            "ConwayGenesisHash": "15a199f895e461ec0ffc6dd4e4028af28a492ab4e806d39cb674c88f7643ef62",
            "ByronGenesisFile": "byron-genesis.json",
            "ByronGenesisHash": "5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb"
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert_eq!(
        cfg.shelley_genesis_hash.as_deref(),
        Some("1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81")
    );
    assert_eq!(
        cfg.alonzo_genesis_hash.as_deref(),
        Some("7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874")
    );
    assert_eq!(
        cfg.conway_genesis_hash.as_deref(),
        Some("15a199f895e461ec0ffc6dd4e4028af28a492ab4e806d39cb674c88f7643ef62")
    );
    assert_eq!(
        cfg.byron_genesis_hash.as_deref(),
        Some("5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb")
    );
}

#[test]
fn verify_known_genesis_hashes_passes_when_files_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"{\"k\":1}";
    let byron_body = br#"{ "z": 1, "a": "b" }"#;
    std::fs::write(dir.path().join("shelley.json"), body).expect("write");
    std::fs::write(dir.path().join("alonzo.json"), body).expect("write");
    std::fs::write(dir.path().join("conway.json"), body).expect("write");
    std::fs::write(dir.path().join("byron.json"), byron_body).expect("write");
    let expected_hex = hex::encode(yggdrasil_crypto::blake2b::hash_bytes_256(body).0);
    let expected_byron_hex =
        hex::encode(yggdrasil_crypto::blake2b::hash_bytes_256(br#"{"a":"b","z":1}"#).0);

    let mut cfg = mainnet_config();
    cfg.byron_genesis_file = Some("byron.json".to_owned());
    cfg.byron_genesis_hash = Some(expected_byron_hex);
    cfg.shelley_genesis_file = Some("shelley.json".to_owned());
    cfg.shelley_genesis_hash = Some(expected_hex.clone());
    cfg.alonzo_genesis_file = Some("alonzo.json".to_owned());
    cfg.alonzo_genesis_hash = Some(expected_hex.clone());
    cfg.conway_genesis_file = Some("conway.json".to_owned());
    cfg.conway_genesis_hash = Some(expected_hex);

    cfg.verify_known_genesis_hashes(Some(dir.path()))
        .expect("matching hashes should pass");
}

#[test]
fn vendored_preset_hashes_match_vendored_genesis_files_end_to_end() {
    // Exercises the full path that runs on every `--network <preset>`
    // startup: each preset's preset constructor declares the
    // canonical *GenesisHash values, and `verify_known_genesis_hashes`
    // reads the vendored genesis files from
    // `crates/node/yggdrasil-node/configuration/<network>/` and
    // compares Blake2b-256 of the file bytes. If a vendored file is
    // updated without bumping the in-code hash (or vice versa), this
    // test fails immediately so the drift is caught at CI time.
    //
    // Wave 5 PR 7+8: the `configuration/` tree stayed with the binary
    // crate; remap CARGO_MANIFEST_DIR (which is `crates/node/config/`
    // now) up one level and into `yggdrasil-node/`.
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("config crate manifest dir has a parent")
        .join("yggdrasil-node");
    for &preset in NetworkPreset::all() {
        let cfg = preset.to_config();
        let base = manifest_dir.join("configuration").join(preset.to_string());
        cfg.verify_known_genesis_hashes(Some(&base))
            .unwrap_or_else(|err| {
                panic!(
                    "preset {preset:?} hashes drifted from vendored files at {}: {err}",
                    base.display(),
                );
            });
    }
}

#[test]
fn verify_known_genesis_hashes_short_circuits_on_first_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let byron_body = br#"{"a":1}"#;
    std::fs::write(dir.path().join("byron.json"), byron_body).expect("write");
    std::fs::write(dir.path().join("shelley.json"), b"{}").expect("write");
    let byron_expected_hex =
        hex::encode(yggdrasil_crypto::blake2b::hash_bytes_256(br#"{"a":1}"#).0);

    let mut cfg = mainnet_config();
    cfg.byron_genesis_file = Some("byron.json".to_owned());
    cfg.byron_genesis_hash = Some(byron_expected_hex);
    cfg.shelley_genesis_file = Some("shelley.json".to_owned());
    cfg.shelley_genesis_hash = Some("0".repeat(64));
    // Other genesis paths intentionally point at non-existent files
    // so we can prove short-circuit: if the Shelley check did not fire
    // first, the loader for the next file would surface a different
    // error variant.
    cfg.alonzo_genesis_file = Some("missing-alonzo.json".to_owned());
    cfg.alonzo_genesis_hash = Some("0".repeat(64));
    cfg.conway_genesis_file = Some("missing-conway.json".to_owned());
    cfg.conway_genesis_hash = Some("0".repeat(64));

    let err = cfg
        .verify_known_genesis_hashes(Some(dir.path()))
        .expect_err("Shelley mismatch must surface");
    assert!(
        matches!(
            err,
            yggdrasil_node_genesis::GenesisLoadError::HashMismatch { .. }
        ),
        "expected HashMismatch first, got {err:?}",
    );
}

#[test]
fn config_parses_requires_network_magic_and_min_node_version() {
    // Mainnet uses RequiresNoMagic; preprod/preview use RequiresMagic.
    // Both keys parse into our typed fields and the operator-supplied
    // MinNodeVersion string round-trips verbatim.
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 764824073,
            "protocol_versions": [13],
            "RequiresNetworkMagic": "RequiresNoMagic",
            "MinNodeVersion": "10.6.2"
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert_eq!(
        cfg.requires_network_magic,
        Some(RequiresNetworkMagic::RequiresNoMagic)
    );
    assert_eq!(cfg.min_node_version.as_deref(), Some("10.6.2"));

    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 1,
            "protocol_versions": [13],
            "RequiresNetworkMagic": "RequiresMagic"
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert_eq!(
        cfg.requires_network_magic,
        Some(RequiresNetworkMagic::RequiresMagic)
    );
    assert_eq!(cfg.min_node_version, None);
}

#[test]
fn requires_network_magic_default_for_magic_matches_upstream() {
    // Canonical mainnet magic → RequiresNoMagic.
    assert_eq!(
        RequiresNetworkMagic::default_for_magic(764_824_073),
        RequiresNetworkMagic::RequiresNoMagic,
    );
    // Anything else → RequiresMagic (preprod is 1, preview is 2,
    // sancho/scratchpad networks have arbitrary magics).
    assert_eq!(
        RequiresNetworkMagic::default_for_magic(1),
        RequiresNetworkMagic::RequiresMagic,
    );
    assert_eq!(
        RequiresNetworkMagic::default_for_magic(2),
        RequiresNetworkMagic::RequiresMagic,
    );
    assert_eq!(
        RequiresNetworkMagic::default_for_magic(0),
        RequiresNetworkMagic::RequiresMagic,
    );
}

#[test]
fn config_parses_checkpoints_file_upstream_keys() {
    // Vendored mainnet config ships these alongside the genesis hash
    // declarations. We currently parse them for byte-for-byte
    // upstream-config compat; the underlying checkpoint-pinning
    // feature is a separate slice.
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 764824073,
            "protocol_versions": [13],
            "CheckpointsFile": "checkpoints.json",
            "CheckpointsFileHash": "3e6dee5bae7acc6d870187e72674b37c929be8c66e62a552cf6a876b1af31ade"
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert_eq!(cfg.checkpoints_file.as_deref(), Some("checkpoints.json"));
    assert_eq!(
        cfg.checkpoints_file_hash.as_deref(),
        Some("3e6dee5bae7acc6d870187e72674b37c929be8c66e62a552cf6a876b1af31ade")
    );
}

#[test]
fn config_parses_last_known_block_version_and_protocol_upstream_keys() {
    // The hyphenated `LastKnownBlockVersion-*` keys round-trip into
    // distinct typed fields and the literal `Protocol` string is
    // preserved, matching upstream `cardano-node`'s mainnet config.
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 764824073,
            "protocol_versions": [13],
            "Protocol": "Cardano",
            "LastKnownBlockVersion-Major": 3,
            "LastKnownBlockVersion-Minor": 0,
            "LastKnownBlockVersion-Alt": 0
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert_eq!(cfg.protocol.as_deref(), Some("Cardano"));
    assert_eq!(cfg.last_known_block_version_major, Some(3));
    assert_eq!(cfg.last_known_block_version_minor, Some(0));
    assert_eq!(cfg.last_known_block_version_alt, Some(0));
}

#[test]
fn config_parses_max_known_major_protocol_version_upstream_alias() {
    // Upstream `cardano-node` ships `MaxKnownMajorProtocolVersion` in
    // `config.json`; vendored configs that use this key must parse
    // straight into our `max_major_protocol_version` field.
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "MaxKnownMajorProtocolVersion": 11
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert_eq!(cfg.max_major_protocol_version, 11);
}

#[test]
fn config_parses_upstream_target_peer_count_aliases() {
    // The official cardano-node config uses PascalCase keys
    // `TargetNumberOfKnownPeers` etc.; vendored / operator-supplied
    // configs that use those names must parse without translation.
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "TargetNumberOfKnownPeers": 150,
            "TargetNumberOfEstablishedPeers": 60,
            "TargetNumberOfActivePeers": 30,
            "TargetNumberOfKnownBigLedgerPeers": 20,
            "TargetNumberOfEstablishedBigLedgerPeers": 10,
            "TargetNumberOfActiveBigLedgerPeers": 4
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");

    assert_eq!(cfg.governor_target_known, 150);
    assert_eq!(cfg.governor_target_established, 60);
    assert_eq!(cfg.governor_target_active, 30);
    assert_eq!(cfg.governor_target_known_big_ledger, 20);
    assert_eq!(cfg.governor_target_established_big_ledger, 10);
    assert_eq!(cfg.governor_target_active_big_ledger, 4);
}

#[test]
fn config_parses_peer_sharing_and_consensus_mode_aliases() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "PeerSharing": 0,
            "ConsensusMode": "GenesisMode"
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");

    assert_eq!(cfg.peer_sharing, 0);
    assert_eq!(cfg.consensus_mode, ConsensusModeConfig::GenesisMode);
}

#[test]
fn tracing_config_parses_with_upstream_field_names() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "TurnOnLogging": true,
            "UseTraceDispatcher": true,
            "TurnOnLogMetrics": false,
            "TraceOptionNodeName": "yggdrasil-local",
            "TraceOptionMetricsPrefix": "cardano.node.metrics.",
            "TraceOptionResourceFrequency": 500,
            "TraceOptionForwarder": {
                "connQueueSize": 16,
                "disconnQueueSize": 32,
                "maxReconnectDelay": 5
            },
            "TraceOptions": {
                "": {
                    "severity": "Notice",
                    "detail": "DNormal",
                    "backends": ["Stdout MachineFormat"]
                },
                "Net.PeerSelection": {
                    "severity": "Info"
                }
            }
        }"#;

    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert!(cfg.turn_on_logging);
    assert!(cfg.use_trace_dispatcher);
    assert!(!cfg.turn_on_log_metrics);
    assert_eq!(
        cfg.trace_option_node_name.as_deref(),
        Some("yggdrasil-local")
    );
    assert_eq!(cfg.trace_option_resource_frequency, 500);
    assert_eq!(cfg.trace_option_forwarder.conn_queue_size, 16);
    assert_eq!(
        cfg.trace_options
            .get("")
            .expect("root trace options")
            .backends,
        vec!["Stdout MachineFormat".to_owned()]
    );
    assert_eq!(
        cfg.trace_options
            .get("Net.PeerSelection")
            .expect("peer selection trace options")
            .severity
            .as_deref(),
        Some("Info")
    );
}

#[test]
fn mainnet_stability_window() {
    let cfg = default_config();
    // stability_window = 3k/f = 3 * 2160 / 0.05 = 129600
    let stability_window = (3.0 * cfg.security_param_k as f64 / cfg.active_slot_coeff) as u64;
    assert_eq!(stability_window, 129_600);
}

#[test]
fn mainnet_preset_matches_genesis() {
    let cfg = NetworkPreset::Mainnet.to_config();
    let mut candidates = vec![cfg.peer_addr];
    candidates.extend(cfg.bootstrap_peers.iter().copied());
    assert_eq!(cfg.network_magic, 764_824_073);
    assert_eq!(cfg.epoch_length, 432_000);
    assert_eq!(cfg.security_param_k, 2160);
    assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
    assert_eq!(cfg.slots_per_kes_period, 129_600);
    assert_eq!(cfg.max_kes_evolutions, 62);
    assert_eq!(cfg.use_ledger_after_slot, Some(182_044_807));
    assert_eq!(
        cfg.peer_snapshot_file.as_deref(),
        Some("peer-snapshot.json")
    );
    assert_eq!(cfg.storage_dir, PathBuf::from("data/mainnet"));
    assert_eq!(cfg.expected_network_id(), 1);
    assert_eq!(cfg.checkpoint_interval_slots, 2160);
    assert_eq!(cfg.max_ledger_snapshots, 8);
    assert_eq!(
        cfg.shelley_genesis_file.as_deref(),
        Some("shelley-genesis.json")
    );
    assert_eq!(
        cfg.alonzo_genesis_file.as_deref(),
        Some("alonzo-genesis.json")
    );
    assert_eq!(
        cfg.conway_genesis_file.as_deref(),
        Some("conway-genesis.json")
    );
    assert!(!candidates.is_empty());
    assert!(candidates.len() <= 3);
}

#[test]
fn mainnet_preset_loads_plutus_cost_model() {
    let cfg = NetworkPreset::Mainnet.to_config();
    // Wave 5 PR 7+8: the `configuration/` tree lives with the binary
    // crate; reach it relative to this crate's manifest dir.
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("config crate manifest dir has a parent")
        .join("yggdrasil-node/configuration/mainnet");
    let model = cfg
        .load_plutus_cost_model(Some(base_dir.as_path()))
        .expect("load plutus cost model")
        .expect("mainnet plutus cost model");
    assert_eq!(model.step_costs.var_cpu, 29_773);
    assert_eq!(model.step_costs.var_mem, 100);
    assert_eq!(model.builtin_cpu, 29_773);
    assert_eq!(model.builtin_mem, 100);
}

#[test]
fn preprod_preset_matches_genesis() {
    let cfg = NetworkPreset::Preprod.to_config();
    assert_eq!(cfg.network_magic, 1);
    assert_eq!(cfg.expected_network_id(), 0);
    assert_eq!(cfg.epoch_length, 432_000);
    assert_eq!(cfg.security_param_k, 2160);
    assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
    assert_eq!(cfg.slots_per_kes_period, 129_600);
    assert_eq!(cfg.max_kes_evolutions, 62);
    assert_eq!(cfg.use_ledger_after_slot, Some(118_022_427));
    assert_eq!(
        cfg.peer_snapshot_file.as_deref(),
        Some("peer-snapshot.json")
    );
    assert_eq!(cfg.storage_dir, PathBuf::from("data/preprod"));
    assert_eq!(cfg.checkpoint_interval_slots, 2160);
    assert_eq!(cfg.max_ledger_snapshots, 8);
    assert!(cfg.bootstrap_peers.is_empty());
}

#[test]
fn preview_preset_matches_genesis() {
    let cfg = NetworkPreset::Preview.to_config();
    assert_eq!(cfg.network_magic, 2);
    assert_eq!(cfg.expected_network_id(), 0);
    assert_eq!(cfg.epoch_length, 86_400);
    assert_eq!(cfg.security_param_k, 432);
    assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
    assert_eq!(cfg.slots_per_kes_period, 129_600);
    assert_eq!(cfg.max_kes_evolutions, 62);
    assert_eq!(cfg.use_ledger_after_slot, Some(107_222_465));
    assert_eq!(
        cfg.peer_snapshot_file.as_deref(),
        Some("peer-snapshot.json")
    );
    assert_eq!(cfg.storage_dir, PathBuf::from("data/preview"));
    // stability_window = 3*432/0.05 = 25920
    let stability_window = (3.0 * cfg.security_param_k as f64 / cfg.active_slot_coeff) as u64;
    assert_eq!(stability_window, 25_920);
    assert!(cfg.bootstrap_peers.is_empty());
}

#[test]
fn explicit_bootstrap_peers_parse_from_json() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "bootstrap_peers": ["127.0.0.2:3001", "127.0.0.3:3001"],
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse with bootstrap peers");
    assert_eq!(cfg.peer_addr, "127.0.0.1:3001".parse().expect("addr"));
    assert_eq!(cfg.bootstrap_peers.len(), 2);
}

#[test]
fn topology_parser_reads_bootstrap_peers() {
    let peers = parse_topology_bootstrap_peers(include_str!(
        "../../yggdrasil-node/configuration/mainnet/topology.json"
    ));
    assert_eq!(peers.len(), 3);
    assert_eq!(peers[0].0, "backbone.cardano.iog.io");
    assert_eq!(peers[0].1, 3001);
}

#[test]
fn topology_resolution_falls_back_when_json_has_no_bootstrap_peers() {
    let fallback: SocketAddr = "127.0.0.1:3001".parse().expect("fallback");
    let topology = resolve_topology_peers("{\"bootstrapPeers\":[]}", fallback);
    assert_eq!(topology.primary_peer, fallback);
    assert!(topology.fallback_peers.is_empty());
}

#[test]
fn topology_resolution_prefers_bootstrap_then_trustable_local_then_public_roots() {
    let fallback: SocketAddr = "127.0.0.99:3001".parse().expect("fallback");
    let topology = resolve_topology_peers(
        r#"{
                "bootstrapPeers": [
                    { "address": "127.0.0.10", "port": 3001 },
                    { "address": "127.0.0.11", "port": 3001 }
                ],
                "localRoots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.12", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": false,
                        "valency": 1
                    },
                    {
                        "accessPoints": [
                            { "address": "127.0.0.13", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": true,
                        "valency": 1
                    }
                ],
                "publicRoots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.14", "port": 3001 }
                        ],
                        "advertise": false
                    }
                ]
            }"#,
        fallback,
    );

    assert_eq!(
        topology.primary_peer,
        "127.0.0.10:3001".parse().expect("addr")
    );
    assert_eq!(
        topology.fallback_peers,
        vec![
            "127.0.0.11:3001".parse().expect("addr"),
            "127.0.0.13:3001".parse().expect("addr"),
            "127.0.0.12:3001".parse().expect("addr"),
            "127.0.0.14:3001".parse().expect("addr"),
        ]
    );
}

#[test]
fn ordered_fallback_peers_include_resolved_topology_groups() {
    let cfg: NodeConfigFile = serde_json::from_str(
        r#"{
                "peer_addr": "127.0.0.10:3001",
                "bootstrap_peers": ["127.0.0.11:3001"],
                "local_roots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.13", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": true,
                        "valency": 1
                    },
                    {
                        "accessPoints": [
                            { "address": "127.0.0.12", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": false,
                        "valency": 1
                    }
                ],
                "public_roots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.14", "port": 3001 }
                        ],
                        "advertise": false
                    }
                ],
                "network_magic": 42,
                "protocol_versions": [13]
            }"#,
    )
    .expect("parse with topology groups");

    assert_eq!(
        cfg.ordered_fallback_peers(),
        vec![
            "127.0.0.11:3001".parse().expect("addr"),
            "127.0.0.13:3001".parse().expect("addr"),
            "127.0.0.12:3001".parse().expect("addr"),
            "127.0.0.14:3001".parse().expect("addr"),
        ]
    );
}

#[test]
fn use_ledger_peers_policy_preserves_legacy_option_semantics() {
    let mut cfg = default_config();

    cfg.use_ledger_after_slot = None;
    assert_eq!(
        cfg.use_ledger_peers_policy(),
        UseLedgerPeers::DontUseLedgerPeers
    );

    cfg.use_ledger_after_slot = Some(0);
    assert_eq!(
        cfg.use_ledger_peers_policy(),
        UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::Always)
    );

    cfg.use_ledger_after_slot = Some(42);
    assert_eq!(
        cfg.use_ledger_peers_policy(),
        UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::After(42))
    );
}

#[test]
fn topology_config_round_trips_network_owned_fields() {
    let cfg = NetworkPreset::Mainnet.to_config();
    let topology = cfg.topology_config();

    assert_eq!(topology.local_roots, cfg.local_roots);
    assert_eq!(topology.public_roots, cfg.public_roots);
    assert_eq!(topology.use_ledger_peers, cfg.use_ledger_peers_policy());
    assert_eq!(topology.peer_snapshot_file, cfg.peer_snapshot_file);
}

#[test]
fn eligible_ledger_fallback_peers_returns_empty_when_policy_blocks_use() {
    let mut cfg = default_config();
    cfg.use_ledger_after_slot = Some(100);

    let snapshot = LedgerPeerSnapshot::new(
        ["127.0.0.20:3001".parse().expect("ledger")],
        ["127.0.0.21:3001".parse().expect("big")],
    );

    let (decision, peers) = cfg.eligible_ledger_fallback_peers(
        &snapshot,
        Some(99),
        LedgerStateJudgement::YoungEnough,
        PeerSnapshotFreshness::Fresh,
    );

    assert_eq!(
        decision,
        LedgerPeerUseDecision::BeforeUseLedgerAfterSlot {
            after_slot: 100,
            latest_slot: 99,
        }
    );
    assert!(peers.is_empty());
}

#[test]
fn eligible_ledger_fallback_peers_filters_primary_and_static_fallbacks() {
    let cfg: NodeConfigFile = serde_json::from_str(
        r#"{
                "peer_addr": "127.0.0.1:3001",
                "bootstrap_peers": ["127.0.0.2:3001"],
                "public_roots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.3", "port": 3001 }
                        ],
                        "advertise": false
                    }
                ],
                "use_ledger_after_slot": 0,
                "peer_snapshot_file": "peer-snapshot.json",
                "network_magic": 42,
                "protocol_versions": [13]
            }"#,
    )
    .expect("parse config");

    let snapshot = LedgerPeerSnapshot::new(
        [
            "127.0.0.1:3001".parse().expect("primary overlap"),
            "127.0.0.2:3001".parse().expect("bootstrap overlap"),
            "127.0.0.4:3001".parse().expect("new ledger"),
        ],
        [
            "127.0.0.3:3001".parse().expect("public overlap"),
            "127.0.0.5:3001".parse().expect("new big ledger"),
        ],
    );

    let (decision, peers) = cfg.eligible_ledger_fallback_peers(
        &snapshot,
        Some(1),
        LedgerStateJudgement::YoungEnough,
        PeerSnapshotFreshness::Fresh,
    );

    assert_eq!(decision, LedgerPeerUseDecision::Eligible);
    assert_eq!(
        peers,
        vec![
            "127.0.0.4:3001".parse().expect("ledger fallback"),
            "127.0.0.5:3001".parse().expect("big ledger fallback"),
        ]
    );
}

#[test]
fn eligible_ledger_fallback_peers_returns_empty_when_snapshot_is_not_fresh() {
    let mut cfg = default_config();
    cfg.use_ledger_after_slot = Some(0);
    cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

    let snapshot = LedgerPeerSnapshot::new(
        ["127.0.0.20:3001".parse().expect("ledger")],
        ["127.0.0.21:3001".parse().expect("big")],
    );

    let (decision, peers) = cfg.eligible_ledger_fallback_peers(
        &snapshot,
        Some(100),
        LedgerStateJudgement::YoungEnough,
        PeerSnapshotFreshness::Stale,
    );

    assert_eq!(
        decision,
        LedgerPeerUseDecision::BlockedByPeerSnapshot {
            freshness: PeerSnapshotFreshness::Stale,
        }
    );
    assert!(peers.is_empty());
}

#[test]
fn peer_snapshot_freshness_waits_for_latest_slot_before_gate() {
    let mut cfg = default_config();
    cfg.use_ledger_after_slot = Some(100);
    cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

    assert_eq!(
        cfg.peer_snapshot_freshness(Some(100), None, true),
        PeerSnapshotFreshness::Awaiting
    );
    assert_eq!(
        cfg.peer_snapshot_freshness(Some(100), Some(99), true),
        PeerSnapshotFreshness::Awaiting
    );
}

#[test]
fn peer_snapshot_freshness_marks_old_snapshot_stale_after_gate() {
    let mut cfg = default_config();
    cfg.use_ledger_after_slot = Some(100);
    cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

    assert_eq!(
        cfg.peer_snapshot_freshness(Some(99), Some(100), true),
        PeerSnapshotFreshness::Stale
    );
    assert_eq!(
        cfg.peer_snapshot_freshness(Some(100), Some(100), true),
        PeerSnapshotFreshness::Fresh
    );
}

#[test]
fn derive_peer_snapshot_freshness_matches_node_config_helper() {
    let mut cfg = default_config();
    cfg.use_ledger_after_slot = Some(100);
    cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

    assert_eq!(
        derive_peer_snapshot_freshness(
            cfg.use_ledger_peers_policy(),
            true,
            Some(100),
            Some(100),
            true,
        ),
        cfg.peer_snapshot_freshness(Some(100), Some(100), true)
    );
}

#[test]
fn parse_peer_snapshot_json_supports_v2_big_ledger_snapshots() {
    let loaded = parse_peer_snapshot_json(
        r#"{
                "version": 2,
                "slotNo": 42,
                "bigLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.20", "port": 3001 },
                            { "address": "127.0.0.21", "port": 3001 }
                        ]
                    }
                ]
            }"#,
    )
    .expect("parse v2 snapshot");

    assert_eq!(loaded.slot, Some(42));
    assert!(loaded.snapshot.ledger_peers.is_empty());
    assert_eq!(
        loaded.snapshot.big_ledger_peers,
        vec![
            "127.0.0.20:3001".parse().expect("peer"),
            "127.0.0.21:3001".parse().expect("peer"),
        ]
    );
}

#[test]
fn parse_peer_snapshot_json_supports_v23_all_ledger_snapshots() {
    let loaded = parse_peer_snapshot_json(
        r#"{
                "NodeToClientVersion": 23,
                "Point": {
                    "slot": 84,
                    "hash": "00"
                },
                "NetworkMagic": 1,
                "allLedgerPools": [
                    {
                        "relativeStake": 0.25,
                        "relays": [
                            { "address": "127.0.0.30", "port": 3001 }
                        ]
                    }
                ]
            }"#,
    )
    .expect("parse v23 snapshot");

    assert_eq!(loaded.slot, Some(84));
    assert_eq!(
        loaded.snapshot.ledger_peers,
        vec!["127.0.0.30:3001".parse().expect("peer")]
    );
    assert!(loaded.snapshot.big_ledger_peers.is_empty());
}

#[test]
fn network_preset_from_str() {
    assert_eq!(
        "mainnet".parse::<NetworkPreset>().expect("mainnet"),
        NetworkPreset::Mainnet
    );
    assert_eq!(
        "Preprod".parse::<NetworkPreset>().expect("preprod"),
        NetworkPreset::Preprod
    );
    assert_eq!(
        "PREVIEW".parse::<NetworkPreset>().expect("preview"),
        NetworkPreset::Preview
    );
    assert!("unknown".parse::<NetworkPreset>().is_err());
}

#[test]
fn network_preset_all_returns_every_variant_exactly_once() {
    // Pins `NetworkPreset::all()` content: exactly the three variants
    // in canonical declaration order. Extends the slice-80-era guard
    // that every preset's constants match upstream by ensuring the
    // iteration helper's set itself can't silently drift — a new
    // enum variant must be explicitly added to `all()` or this test
    // (and the downstream `.all()` callers) fail immediately.
    let all = NetworkPreset::all();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0], NetworkPreset::Mainnet);
    assert_eq!(all[1], NetworkPreset::Preprod);
    assert_eq!(all[2], NetworkPreset::Preview);

    // And every variant must be distinct — catches a duplicated
    // entry from a copy-paste refactor.
    assert_ne!(all[0], all[1]);
    assert_ne!(all[0], all[2]);
    assert_ne!(all[1], all[2]);
}

#[test]
fn network_preset_display_round_trips() {
    for &preset in NetworkPreset::all() {
        let s = preset.to_string();
        let parsed: NetworkPreset = s.parse().expect("display should round-trip");
        assert_eq!(parsed, preset);
    }
}

#[test]
fn default_config_is_mainnet() {
    let def = default_config();
    let mainnet = mainnet_config();
    assert_eq!(def.network_magic, mainnet.network_magic);
    assert_eq!(def.epoch_length, mainnet.epoch_length);
    assert_eq!(def.security_param_k, mainnet.security_param_k);
    assert_eq!(def.expected_network_id(), 1);
}

#[test]
fn topology_file_path_config_parses() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "TopologyFilePath": "topology.json"
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert_eq!(cfg.topology_file_path.as_deref(), Some("topology.json"));
}

#[test]
fn topology_file_path_absent_defaults_to_none() {
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
    let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
    assert!(cfg.topology_file_path.is_none());
}

#[test]
fn load_topology_file_reads_upstream_format() {
    let dir = std::env::temp_dir().join(format!(
        "yggdrasil-topology-load-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let topo_path = dir.join("topology.json");
    std::fs::write(
        &topo_path,
        r#"{
                "bootstrapPeers": [
                    {"address": "127.0.0.20", "port": 3001}
                ],
                "localRoots": [
                    {
                        "accessPoints": [
                            {"address": "127.0.0.21", "port": 3002}
                        ],
                        "advertise": false,
                        "valency": 1,
                        "trustable": true
                    }
                ],
                "publicRoots": [
                    {
                        "accessPoints": [
                            {"address": "127.0.0.22", "port": 3003}
                        ],
                        "advertise": false
                    }
                ],
                "useLedgerAfterSlot": 42000,
                "peerSnapshotFile": "snap.json"
            }"#,
    )
    .expect("write topology file");

    let topology = load_topology_file(&topo_path).expect("load topology");
    assert_eq!(topology.local_roots.len(), 1);
    assert_eq!(topology.public_roots.len(), 1);
    assert_eq!(topology.use_ledger_peers.to_after_slot(), Some(42000));
    assert_eq!(topology.peer_snapshot_file.as_deref(), Some("snap.json"));

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn load_topology_file_returns_error_on_missing_file() {
    let result = load_topology_file(std::path::Path::new("/tmp/nonexistent-topology.json"));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, TopologyFileError::Io { .. }));
}

#[test]
fn apply_topology_to_config_overrides_inline_topology() {
    use yggdrasil_network::TopologyConfig;
    let mut cfg = default_config();
    cfg.local_roots = Vec::new();
    cfg.public_roots = Vec::new();
    cfg.use_ledger_after_slot = None;
    cfg.peer_snapshot_file = None;

    let topology = TopologyConfig {
        local_roots: vec![yggdrasil_network::LocalRootConfig {
            access_points: vec![yggdrasil_network::PeerAccessPoint {
                address: "127.0.0.30".to_owned(),
                port: 3001,
            }],
            advertise: false,
            trustable: true,
            hot_valency: 1,
            warm_valency: None,
            diffusion_mode: Default::default(),
        }],
        public_roots: vec![yggdrasil_network::PublicRootConfig {
            access_points: vec![yggdrasil_network::PeerAccessPoint {
                address: "127.0.0.31".to_owned(),
                port: 3002,
            }],
            advertise: false,
        }],
        use_ledger_peers: yggdrasil_network::UseLedgerPeers::UseLedgerPeers(
            yggdrasil_network::AfterSlot::After(99000),
        ),
        peer_snapshot_file: Some("my-snap.json".to_owned()),
        ..Default::default()
    };

    apply_topology_to_config(&mut cfg, &topology);

    assert_eq!(cfg.local_roots.len(), 1);
    assert_eq!(cfg.public_roots.len(), 1);
    assert_eq!(cfg.use_ledger_after_slot, Some(99000));
    assert_eq!(cfg.peer_snapshot_file.as_deref(), Some("my-snap.json"));
}

#[test]
fn topology_file_path_round_trips_json() {
    let mut cfg = default_config();
    cfg.topology_file_path = Some("my-topology.json".to_owned());
    let json = serde_json::to_string_pretty(&cfg).expect("serialize");
    let parsed: NodeConfigFile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        parsed.topology_file_path.as_deref(),
        Some("my-topology.json")
    );
}

/// Default `max_major_protocol_version` matches Conway-era `MaxMajorProtVer`
/// (upstream value: 10).
#[test]
fn max_major_protocol_version_default_is_conway_era() {
    let cfg = default_config();
    assert_eq!(cfg.max_major_protocol_version, 10);
}

/// `max_major_protocol_version` round-trips through JSON serialization and
/// deserializes to the default when absent from the input.
#[test]
fn max_major_protocol_version_round_trips_and_defaults() {
    // Explicit value round-trips.
    let mut cfg = default_config();
    cfg.max_major_protocol_version = 12;
    let json = serde_json::to_string(&cfg).expect("serialize");
    let parsed: NodeConfigFile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.max_major_protocol_version, 12);

    // Missing from JSON → defaults to 10.
    let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
    let parsed: NodeConfigFile = serde_json::from_str(json).expect("deserialize");
    assert_eq!(parsed.max_major_protocol_version, 10);
}
