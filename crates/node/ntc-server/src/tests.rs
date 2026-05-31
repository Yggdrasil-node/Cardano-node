// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;
use std::sync::{Mutex, MutexGuard};
use yggdrasil_consensus::{NonceEvolutionState, OcertCounters};
use yggdrasil_ledger::{Era, LedgerState};
use yggdrasil_network::MiniProtocolNum;

/// Serialises every test that reads or mutates `YGG_LSQ_ERA_FLOOR`. Rust's
/// test runner runs unit tests in parallel by default, so without this
/// lock the floor-promotion test could leak a value into the table-pinning
/// test running concurrently and force every PV to era_index 6.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const LSQ_ERA_FLOOR_ENV: &str = "YGG_LSQ_ERA_FLOOR";

fn clear_lsq_era_floor(_guard: &MutexGuard<'_, ()>) {
    // SAFETY: callers must hold ENV_LOCK, passed as `_guard`, so the
    // process-wide environment is not mutated concurrently by another
    // test in this module.
    unsafe {
        std::env::remove_var(LSQ_ERA_FLOOR_ENV);
    }
}

fn set_lsq_era_floor(_guard: &MutexGuard<'_, ()>, value: &str) {
    // SAFETY: callers must hold ENV_LOCK, passed as `_guard`, so the
    // process-wide environment is not mutated concurrently by another
    // test in this module.
    unsafe {
        std::env::set_var(LSQ_ERA_FLOOR_ENV, value);
    }
}

#[test]
fn test_ntc_protocol_numbers() {
    assert_eq!(MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION, MiniProtocolNum(5));
    assert_eq!(MiniProtocolNum::NTC_LOCAL_STATE_QUERY, MiniProtocolNum(7));
    assert_eq!(MiniProtocolNum::NTC_LOCAL_TX_MONITOR, MiniProtocolNum(9));
}

#[test]
fn test_encode_rejection_reason_is_non_empty() {
    let bytes = encode_rejection_reason("tx too large");
    assert!(!bytes.is_empty());
}

#[test]
fn protocol_state_uses_exact_chain_dep_sidecar_and_ignores_latest_mirrors() {
    use yggdrasil_ledger::{
        Block, BlockHeader, BlockNo, CborEncode, Decoder, HeaderHash, Nonce, Point, SlotNo,
    };
    use yggdrasil_node_sync::{LedgerCheckpointUpdateOutcome, persist_chain_dep_state_sidecar};

    let dir = tempfile::tempdir().expect("temp sidecar dir");
    let point = Point::BlockPoint(SlotNo(7), HeaderHash([7; 32]));
    let block = Block {
        era: Era::Byron,
        header: BlockHeader {
            hash: HeaderHash([7; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(7),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: Vec::new(),
        raw_cbor: None,
        header_cbor_size: None,
    };
    let mut ledger = LedgerState::new(Era::Byron);
    ledger.apply_block(&block).expect("advance ledger tip");
    let snapshot = ledger.snapshot();

    let pool = [0xBE; 28];
    let exact_nonce = NonceEvolutionState::new(Nonce::Hash([0x55; 32]));
    let mut exact_counters = OcertCounters::new();
    for seq in 0..=4 {
        exact_counters
            .validate_and_update(pool, seq, true)
            .expect("advance exact counter");
    }
    let outcome = Some(LedgerCheckpointUpdateOutcome::Persisted {
        slot: SlotNo(7),
        retained_snapshots: 1,
        pruned_snapshots: 0,
        rollback_count: 0,
    });
    persist_chain_dep_state_sidecar(
        &outcome,
        Some(dir.path()),
        point,
        Some(&exact_nonce),
        Some(&exact_counters),
        1,
    )
    .expect("persist exact chain-dep sidecar");

    let stale_nonce = NonceEvolutionState::new(Nonce::Hash([0x99; 32]));
    let mut nonce_enc = yggdrasil_ledger::Encoder::new();
    stale_nonce.encode_cbor(&mut nonce_enc);
    std::fs::write(dir.path().join("nonce_state.cbor"), nonce_enc.into_bytes())
        .expect("write stale nonce mirror");
    let stale_counters = OcertCounters::new();
    std::fs::write(
        dir.path().join("ocert_counters.cbor"),
        stale_counters.to_cbor_bytes(),
    )
    .expect("write stale counter mirror");

    let snapshot = attach_chain_dep_state_from_sidecar(snapshot, Some(dir.path()));
    let ctx = snapshot.chain_dep_state().expect("chain-dep context");
    assert_eq!(ctx.evolving_nonce, exact_nonce.evolving_nonce);
    assert_eq!(ctx.opcert_counters.get(&pool).copied(), Some(4));

    let encoded = encode_praos_state_versioned(&snapshot);
    let mut dec = Decoder::new(&encoded);
    assert_eq!(dec.array().expect("versioned wrapper"), 2);
    assert_eq!(dec.unsigned().expect("version"), 0);
    assert_eq!(dec.array().expect("praos fields"), 8);
    dec.skip().expect("last slot");
    assert_eq!(dec.map().expect("ocert map"), 1);
    assert_eq!(dec.bytes().expect("pool key"), pool);
    assert_eq!(dec.unsigned().expect("counter"), 4);
    assert_eq!(dec.array().expect("nonce constructor"), 2);
    assert_eq!(dec.unsigned().expect("nonce tag"), 1);
    assert_eq!(dec.bytes().expect("nonce hash"), [0x55; 32]);
}

/// R214 — pin upstream's 15-element shape for
/// `Cardano.Ledger.Shelley.Genesis.encCBOR`.  Drift here means
/// `cardano-cli`-side decoders that consume `GetGenesisConfig`
/// (era-specific tag 11) silently mis-parse or reject the
/// response.  See `crates/network/src/protocols/local_state_query_upstream.rs`
/// for the Shelley PP `encode_shelley_pparams_for_lsq` 17-element
/// shape that field 12 reuses.
#[test]
fn shelley_genesis_encoder_emits_15_element_list() {
    use std::collections::BTreeMap;
    use yggdrasil_ledger::{Decoder, ProtocolParameters};
    use yggdrasil_node_genesis::{
        GenesisProtocolVersion, GenesisRational, ShelleyGenesis, ShelleyGenesisDelegation,
        ShelleyGenesisProtocolParams,
    };

    // Mainnet-like genesis (subset; any non-default value would do).
    let mut gen_delegs = BTreeMap::new();
    gen_delegs.insert(
        // 28-byte hex string → 28 bytes after decode
        "ad5463153dc3d24b9ff133e46136028bdc1edbb897f5a7cf1b37950c".to_string(),
        ShelleyGenesisDelegation {
            delegate: "d9e5c76ad5ee778960804094a389f0b546b5c2b140a62f8ec43ea54d".to_string(),
            vrf: "64fa87e8b29a5b7bfbd6795677e3e878c505bc4a3649485d366b50abadec92d7".to_string(),
        },
    );

    let mut initial_funds = BTreeMap::new();
    initial_funds.insert(
        "82d818582183581ce6e08c8c6e1aa9a40b7e15bdb5dac739b1c10f5d6a9203a8b3a3aaa0a0021af3afdfba"
            .to_string(),
        462_146_000_000u64,
    );

    let genesis = ShelleyGenesis {
        active_slots_coeff: 0.05,
        epoch_length: 432_000,
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        security_param: 2160,
        slot_length: 1.0,
        network_id: Some("Mainnet".to_string()),
        network_magic: Some(764_824_073),
        gen_delegs,
        initial_funds,
        staking: Default::default(),
        protocol_params: ShelleyGenesisProtocolParams {
            min_fee_a: 44,
            min_fee_b: 155_381,
            max_block_body_size: 65_536,
            max_tx_size: 16_384,
            max_block_header_size: 1_100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 150,
            a0: GenesisRational {
                numerator: 3,
                denominator: 10,
            },
            rho: GenesisRational {
                numerator: 3,
                denominator: 1_000,
            },
            tau: GenesisRational {
                numerator: 1,
                denominator: 5,
            },
            decentralisation_param: Some(1.0),
            extra_entropy: None,
            protocol_version: GenesisProtocolVersion { major: 2, minor: 0 },
            min_utxo_value: 1_000_000,
            min_pool_cost: 340_000_000,
        },
        update_quorum: 5,
        system_start: Some("2017-09-23T21:44:51Z".to_string()),
        max_lovelace_supply: 45_000_000_000_000_000,
    };

    let pp = ProtocolParameters::default();
    let bytes = encode_shelley_genesis_for_lsq(&genesis, &pp, 1_506_203_091.0);

    // Top-level must be a 15-element CBOR list.
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("expected outer array");
    assert_eq!(len, 15, "ShelleyGenesis must encode as a 15-element list");

    // Field 1 (systemStart) is itself a 3-element list `[mjd, picos, 0]`.
    let mjd_arr_len = dec.array().expect("systemStart array");
    assert_eq!(
        mjd_arr_len, 3,
        "systemStart must be 3-element [mjd, picos, attos]"
    );
    let mjd = dec.unsigned().expect("systemStart MJD");
    let picos = dec.unsigned().expect("systemStart picos-of-day");
    let attos = dec.unsigned().expect("systemStart attoseconds (always 0)");
    // 2017-09-23 ↔ MJD 58019; pin within ±2 days against floating-point drift.
    assert!(
        (58_017..=58_021).contains(&mjd),
        "MJD for 2017-09-23 must be ~58019, got {mjd}"
    );
    assert!(
        picos < 86_400u64 * 1_000_000_000_000,
        "picos < 1 day in picos"
    );
    assert_eq!(attos, 0, "attoseconds must be 0 per upstream convention");
}

/// Round 161 — pin `effective_era_index_for_lsq`'s PV major →
/// era_index mapping per upstream
/// `Ouroboros.Consensus.Cardano.CanHardFork`'s `*Transition`
/// `ProtVer` table.  When this drifts, cardano-cli's per-era
/// query gating misclassifies the chain's active era and
/// queries silently fail or run against the wrong codec.
#[test]
fn effective_era_index_pv_table_matches_upstream() {
    use yggdrasil_ledger::ProtocolParameters;

    let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Clear any value a concurrent floor-promotion test may have left set
    // so the PV→era mapping is exercised on its own.
    clear_lsq_era_floor(&guard);

    let cases = [
        // (block_pv, expected_era_index)
        (Some((1u64, 0u64)), 0), // Byron
        (Some((2, 0)), 1),       // Shelley
        (Some((3, 0)), 2),       // Allegra (signal in Shelley codec)
        (Some((4, 0)), 3),       // Mary
        (Some((5, 0)), 4),       // Alonzo intra-era
        (Some((6, 0)), 4),       // Alonzo intra-era (post-bump)
        (Some((7, 0)), 5),       // Babbage transition signal
        (Some((8, 0)), 5),       // Babbage intra-era
        (Some((9, 0)), 6),       // Conway transition signal
        (Some((10, 0)), 6),      // Conway intra-era
        (Some((100, 0)), 6),     // Future PV bumps stay at Conway
    ];

    for (pv, expected) in cases {
        let mut state = LedgerState::new(Era::Byron);
        state.latest_block_protocol_version = pv;
        // Leave protocol_params=None so the test exercises the
        // block_pv path exclusively, not the params fallback.
        let _ = ProtocolParameters::default;
        let snapshot = state.snapshot();
        let actual = effective_era_index_for_lsq(&snapshot);
        assert_eq!(
            actual, expected,
            "PV {pv:?} should map to era_index {expected}, got {actual}",
        );
    }
}

/// Round 178 — `YGG_LSQ_ERA_FLOOR=N` raises the reported era
/// to at least `N` so operators can bypass cardano-cli's
/// Babbage+ gate on partial-sync chains stuck at PV=(6,0)
/// (Alonzo).  Same-era and lower-floor settings are no-ops.
///
/// Env-var manipulation is serialised via a static `Mutex` so
/// concurrent test execution doesn't race on the process-wide
/// env table (Rust's test runner runs unit tests in parallel
/// by default).
#[test]
fn era_floor_env_var_promotes_reported_era() {
    let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Build a snapshot whose chain is at PV=(6,0) (Alonzo).
    let mut state = LedgerState::new(Era::Byron);
    state.latest_block_protocol_version = Some((6, 0));
    let snapshot = state.snapshot();

    // Sanity: with no env var set, era is Alonzo (4).
    clear_lsq_era_floor(&guard);
    assert_eq!(effective_era_index_for_lsq(&snapshot), 4);

    // With YGG_LSQ_ERA_FLOOR=5, era promotes to Babbage (5).
    set_lsq_era_floor(&guard, "5");
    assert_eq!(effective_era_index_for_lsq(&snapshot), 5);

    // With YGG_LSQ_ERA_FLOOR=6, era promotes to Conway (6).
    set_lsq_era_floor(&guard, "6");
    assert_eq!(effective_era_index_for_lsq(&snapshot), 6);

    // With YGG_LSQ_ERA_FLOOR=2 (lower than derived Alonzo=4),
    // it's a no-op — never demote.
    set_lsq_era_floor(&guard, "2");
    assert_eq!(effective_era_index_for_lsq(&snapshot), 4);

    // With YGG_LSQ_ERA_FLOOR=99 (out of range), it's a no-op.
    set_lsq_era_floor(&guard, "99");
    assert_eq!(effective_era_index_for_lsq(&snapshot), 4);

    // With YGG_LSQ_ERA_FLOOR=garbage, it's a no-op.
    set_lsq_era_floor(&guard, "not-a-number");
    assert_eq!(effective_era_index_for_lsq(&snapshot), 4);

    // Cleanup.
    clear_lsq_era_floor(&guard);
}

/// Round 161 — when block_pv is `None` (no block applied yet)
/// the helper falls back to `protocol_params.protocol_version`.
#[test]
fn effective_era_index_falls_back_to_params_pv_when_no_block() {
    use yggdrasil_ledger::ProtocolParameters;
    let mut state = LedgerState::new(Era::Byron);
    state.latest_block_protocol_version = None;
    let pp = ProtocolParameters {
        protocol_version: Some((9, 0)),
        ..ProtocolParameters::default()
    };
    *state.protocol_params_mut() = Some(pp);
    let snapshot = state.snapshot();
    assert_eq!(
        effective_era_index_for_lsq(&snapshot),
        6,
        "params_pv major=9 should map to Conway (6) when no block PV is set",
    );
}

/// Round 163 — `GetStakePools` against an empty snapshot
/// returns the empty CBOR set `tag(258) [<>]` which cardano-cli
/// renders as `[]`.  Pins the upstream-faithful encoding shape
/// for the empty case.
#[test]
fn get_stake_pools_empty_snapshot_emits_tag_258_empty_set() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let bytes = encode_stake_pools_set(&snapshot);
    // CBOR tag 258 = `0xd9 0x01 0x02`, then `0x80` (empty array).
    assert_eq!(bytes, [0xd9, 0x01, 0x02, 0x80]);
}

/// Round 179 — `GetStakeDistribution` / `GetStakeDistribution2`
/// against an empty snapshot returns the canonical 2-element
/// `[map, total]` PoolDistr envelope: `0x82 0xa0 0x01`
/// (1-lovelace placeholder for `NonZero Coin pdTotalStake`).
#[test]
fn get_stake_distribution_empty_snapshot_emits_pool_distr_envelope() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let bytes = encode_stake_distribution_map(&snapshot);
    // 0x82 = list-2, 0xa0 = empty map (unPoolDistr),
    // 0x01 = 1 coin (pdTotalStake placeholder, NonZero).
    assert_eq!(bytes, [0x82, 0xa0, 0x01]);
}

/// Round 236 — `GetStakeDistribution` against a snapshot with
/// live `StakeSnapshots` attached emits the upstream PoolDistr
/// shape with real per-pool `IndividualPoolStake` entries
/// (3-tuple of stake share, total pool stake, VRF key) and a
/// `NonZero Coin` pdTotalActiveStake matching the sum of active
/// stake.  Pins the wire shape that `cardano-cli query
/// stake-distribution` consumes once the snapshot rotation
/// container is plumbed through (R203 path).
#[test]
fn get_stake_distribution_with_live_snapshot_emits_individual_pool_stakes() {
    use yggdrasil_ledger::cbor::Decoder;
    use yggdrasil_ledger::stake::{StakeSnapshot, StakeSnapshots};
    use yggdrasil_ledger::{
        PoolMetadata, PoolParams, RewardAccount, StakeCredential, UnitInterval,
    };

    let state = LedgerState::new(Era::Conway);
    let mut snapshot = state.snapshot();

    let pool_a: [u8; 28] = [0xa0; 28];
    let pool_b: [u8; 28] = [0xb1; 28];
    let cred_a = StakeCredential::AddrKeyHash([0x10; 28]);
    let cred_b = StakeCredential::AddrKeyHash([0x20; 28]);
    let cred_c = StakeCredential::AddrKeyHash([0x30; 28]);

    let mk_pool = |op: [u8; 28], vrf: [u8; 32]| PoolParams {
        operator: op,
        vrf_keyhash: vrf,
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0; 28]),
        },
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None as Option<PoolMetadata>,
    };

    let mut set_snap = StakeSnapshot::empty();
    set_snap.stake.add(cred_a, 600);
    set_snap.stake.add(cred_b, 300);
    set_snap.stake.add(cred_c, 100);
    set_snap.delegations.insert(cred_a, pool_a);
    set_snap.delegations.insert(cred_b, pool_a);
    set_snap.delegations.insert(cred_c, pool_b);
    set_snap
        .pool_params
        .insert(pool_a, mk_pool(pool_a, [0x55; 32]));
    set_snap
        .pool_params
        .insert(pool_b, mk_pool(pool_b, [0x66; 32]));

    let snapshots = StakeSnapshots {
        mark: StakeSnapshot::empty(),
        set: set_snap,
        go: StakeSnapshot::empty(),
        fee_pot: 0,
        previous_fee_pot: 0,
    };
    snapshot = snapshot.with_stake_snapshots(snapshots);

    let bytes = encode_stake_distribution_map(&snapshot);
    let mut dec = Decoder::new(&bytes);
    let outer = dec.array().expect("outer array");
    assert_eq!(outer, 2, "PoolDistr envelope is a 2-element list");

    let map_len = dec.map().expect("inner map");
    assert_eq!(map_len, 2, "two pools registered with non-zero stake");

    let mut seen_a = false;
    let mut seen_b = false;
    for _ in 0..map_len {
        let key = dec.bytes().expect("pool key").to_vec();
        let inner = dec.array().expect("IndividualPoolStake list");
        assert_eq!(inner, 3, "IndividualPoolStake is a 3-tuple");
        let tag = dec.tag().expect("tag for Rational");
        assert_eq!(tag, 30, "Rational uses CBOR tag 30");
        let pair = dec.array().expect("[num, den] pair");
        assert_eq!(pair, 2);
        let numerator = dec.unsigned().expect("rational numerator");
        let denominator = dec.unsigned().expect("rational denominator");
        assert_eq!(denominator, 1000, "denominator is total active stake");
        let pool_stake = dec.unsigned().expect("CompactCoin total");
        let vrf_bytes = dec.bytes().expect("VRF key bytes");
        assert_eq!(vrf_bytes.len(), 32);

        if key == pool_a {
            assert_eq!(numerator, 900);
            assert_eq!(pool_stake, 900);
            assert_eq!(vrf_bytes, [0x55; 32]);
            seen_a = true;
        } else if key == pool_b {
            assert_eq!(numerator, 100);
            assert_eq!(pool_stake, 100);
            assert_eq!(vrf_bytes, [0x66; 32]);
            seen_b = true;
        }
    }
    assert!(seen_a && seen_b, "both pools surfaced in the map");

    let total = dec.unsigned().expect("pdTotalActiveStake");
    assert_eq!(total, 1000, "total active stake matches sum of inputs");
}

/// R237 — Conway `GetPoolDistr2` applies the optional pool-key
/// filter server-side while preserving the full active-stake
/// denominator from the source `PoolDistr`.
#[test]
fn get_pool_distr2_with_filter_emits_requested_pool_only() {
    use std::collections::HashSet;
    use yggdrasil_ledger::cbor::Decoder;
    use yggdrasil_ledger::stake::{StakeSnapshot, StakeSnapshots};
    use yggdrasil_ledger::{
        PoolMetadata, PoolParams, RewardAccount, StakeCredential, UnitInterval,
    };

    let state = LedgerState::new(Era::Conway);
    let pool_a: [u8; 28] = [0xa0; 28];
    let pool_b: [u8; 28] = [0xb1; 28];
    let cred_a = StakeCredential::AddrKeyHash([0x10; 28]);
    let cred_b = StakeCredential::AddrKeyHash([0x20; 28]);

    let mk_pool = |op: [u8; 28], vrf: [u8; 32]| PoolParams {
        operator: op,
        vrf_keyhash: vrf,
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0; 28]),
        },
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None as Option<PoolMetadata>,
    };

    let mut set_snap = StakeSnapshot::empty();
    set_snap.stake.add(cred_a, 900);
    set_snap.stake.add(cred_b, 100);
    set_snap.delegations.insert(cred_a, pool_a);
    set_snap.delegations.insert(cred_b, pool_b);
    set_snap
        .pool_params
        .insert(pool_a, mk_pool(pool_a, [0x55; 32]));
    set_snap
        .pool_params
        .insert(pool_b, mk_pool(pool_b, [0x66; 32]));

    let snapshot = state.snapshot().with_stake_snapshots(StakeSnapshots {
        mark: StakeSnapshot::empty(),
        set: set_snap,
        go: StakeSnapshot::empty(),
        fee_pot: 0,
        previous_fee_pot: 0,
    });
    let filter = HashSet::from([pool_b]);
    let bytes = encode_pool_distr_for_lsq(&snapshot, Some(&filter));

    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.array().expect("PoolDistr envelope"), 2);
    assert_eq!(dec.map().expect("filtered map"), 1);
    assert_eq!(dec.bytes().expect("pool key"), pool_b);
    assert_eq!(dec.array().expect("IndividualPoolStake"), 3);
    assert_eq!(dec.tag().expect("Rational tag"), 30);
    assert_eq!(dec.array().expect("Rational pair"), 2);
    assert_eq!(dec.unsigned().expect("numerator"), 100);
    assert_eq!(
        dec.unsigned().expect("denominator"),
        1000,
        "filtered entry remains a share of the full active stake"
    );
    assert_eq!(dec.unsigned().expect("CompactCoin pool stake"), 100);
    assert_eq!(dec.bytes().expect("VRF key"), [0x66; 32]);
    assert_eq!(
        dec.unsigned().expect("pdTotalActiveStake"),
        1000,
        "PoolDistr total remains the full active stake"
    );
}

#[test]
fn get_pool_distr2_empty_snapshot_with_filter_preserves_nonzero_total() {
    use std::collections::HashSet;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let filter = HashSet::from([[0xa0; 28]]);
    let bytes = encode_pool_distr_for_lsq(&snapshot, Some(&filter));

    assert_eq!(bytes, [0x82, 0xa0, 0x01]);
}

/// Round 237 — `GetLedgerPeerSnapshot` against a snapshot with
/// live `StakeSnapshots` + registered pools (with relays)
/// emits the upstream `LedgerPeerSnapshotV2` shape with
/// per-pool `AccPoolStake` (cumulative) + `PoolStake` rationals
/// derived from the active stake distribution, sorted
/// descending by stake.  Pins the wire shape consumed by
/// `cardano-cli ... query ledger-peer-snapshot`.
#[test]
fn get_ledger_peer_snapshot_with_live_stake_emits_cdf_rationals() {
    use yggdrasil_ledger::cbor::Decoder;
    use yggdrasil_ledger::stake::{StakeSnapshot, StakeSnapshots};
    use yggdrasil_ledger::{
        PoolMetadata, PoolParams, Relay, RewardAccount, StakeCredential, UnitInterval,
    };

    let pool_a: [u8; 28] = [0xa0; 28];
    let pool_b: [u8; 28] = [0xb1; 28];

    let mk_pool = |op: [u8; 28], vrf: [u8; 32], port: u16, ipv4: [u8; 4]| PoolParams {
        operator: op,
        vrf_keyhash: vrf,
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0; 28]),
        },
        pool_owners: vec![],
        relays: vec![Relay::SingleHostAddr(Some(port), Some(ipv4), None)],
        pool_metadata: None as Option<PoolMetadata>,
    };

    // Register pool A and pool B with relays.
    let mut state = LedgerState::new(Era::Conway);
    state.pool_state_mut().register_with_deposit(
        mk_pool(pool_a, [0x55; 32], 3001, [192, 0, 2, 1]),
        500_000_000,
    );
    state.pool_state_mut().register_with_deposit(
        mk_pool(pool_b, [0x66; 32], 3002, [192, 0, 2, 2]),
        500_000_000,
    );

    // Build the live `set` snapshot: pool A holds 900 / 1000,
    // pool B holds 100 / 1000.  Stake credentials don't matter
    // for the test — only the aggregate per-pool stake.
    let cred_a = StakeCredential::AddrKeyHash([0x10; 28]);
    let cred_b = StakeCredential::AddrKeyHash([0x20; 28]);
    let mut set_snap = StakeSnapshot::empty();
    set_snap.stake.add(cred_a, 900);
    set_snap.stake.add(cred_b, 100);
    set_snap.delegations.insert(cred_a, pool_a);
    set_snap.delegations.insert(cred_b, pool_b);
    set_snap
        .pool_params
        .insert(pool_a, mk_pool(pool_a, [0x55; 32], 3001, [192, 0, 2, 1]));
    set_snap
        .pool_params
        .insert(pool_b, mk_pool(pool_b, [0x66; 32], 3002, [192, 0, 2, 2]));

    let snapshots = StakeSnapshots {
        mark: StakeSnapshot::empty(),
        set: set_snap,
        go: StakeSnapshot::empty(),
        fee_pot: 0,
        previous_fee_pot: 0,
    };

    let snapshot = state.snapshot().with_stake_snapshots(snapshots);
    let bytes = encode_ledger_peer_snapshot_v2_for_lsq(&snapshot);

    let mut dec = Decoder::new(&bytes);
    let outer = dec.array().expect("outer 2-elem");
    assert_eq!(outer, 2);
    let v = dec.unsigned().expect("V2 discriminator");
    assert_eq!(v, 1);
    let inner = dec.array().expect("inner 2-elem");
    assert_eq!(inner, 2);

    // WithOrigin SlotNo: snapshot tip is Origin → [0].
    let wo_len = dec.array().expect("WithOrigin");
    assert_eq!(wo_len, 1);
    let origin_disc = dec.unsigned().expect("Origin discriminator");
    assert_eq!(origin_disc, 0);

    // Pools — indef-length list: peek for 0x9f (indef start),
    // then iterate until 0xff.  The CBOR Decoder helper handles
    // indef arrays implicitly via `array()` returning u64::MAX
    // for indefinite, but we just walk the bytes manually here
    // since `Decoder` may differ.  Use `array_indef`-aware path
    // if exposed; otherwise loop on raw bytes.
    //
    // For brevity, skip indef parsing and just verify the raw
    // bytes contain the expected sentinels.
    let remaining = &bytes[bytes.len() - {
        // Find offset of pool list — after the WithOrigin [1, 0]
        // bytes we should hit 0x9f.
        let pos = bytes.iter().position(|&b| b == 0x9f).expect("indef start");
        bytes.len() - pos
    }..];
    assert_eq!(remaining[0], 0x9f, "indef-list start sentinel");

    // Verify the first pool entry begins immediately after 0x9f
    // with `0x82` (2-element array `[AccPoolStake, [PoolStake,
    // Relays]]`).  Pool A (stake 900) should be first
    // (descending order).
    assert_eq!(remaining[1], 0x82, "first entry [AccPoolStake, ...]");
    // AccPoolStake = [num=900, den=1000] → array(2), 0x19 0x03 0x84,
    // 0x19 0x03 0xe8.
    assert_eq!(remaining[2], 0x82, "AccPoolStake list-2");
    // 0x19 = unsigned 2-byte, 0x03 0x84 = 900.
    assert_eq!(&remaining[3..6], &[0x19, 0x03, 0x84]);
    assert_eq!(&remaining[6..9], &[0x19, 0x03, 0xe8]); // 1000.

    // The bytes end with 0xff 0xff (close NonEmpty Relays
    // indef + close pool list indef) — at minimum two sentinels
    // are present in the encoded output.
    let last2 = &bytes[bytes.len() - 2..];
    assert_eq!(last2, &[0xff, 0xff]);
}

/// Round 163 — `GetFilteredDelegationsAndRewardAccounts` against
/// an empty snapshot returns `[empty_map, empty_map]` = the
/// 2-element list `0x82 0xa0 0xa0`.
#[test]
fn get_filtered_delegations_empty_snapshot_emits_two_empty_maps() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let creds = std::collections::HashSet::new();
    let bytes = encode_filtered_delegations_and_rewards(&snapshot, &creds);
    assert_eq!(bytes, [0x82, 0xa0, 0xa0]);
}

/// Round 171 — `GetStakePoolParams` against an empty snapshot or
/// an empty hash-filter set returns the empty CBOR map `0xa0`,
/// matching upstream `Map.intersection` of an empty registered
/// set with any filter.
#[test]
fn get_stake_pool_params_empty_filter_emits_empty_map() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let filter = std::collections::HashSet::new();
    let bytes = encode_filtered_stake_pool_params(&snapshot, &filter);
    assert_eq!(bytes, [0xa0]);
}

/// Round 171 — `GetStakePoolParams` filter for a non-existent
/// pool against a populated snapshot still returns the empty CBOR
/// map (intersection drops unknown pools).
#[test]
fn get_stake_pool_params_unknown_filter_emits_empty_map() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let mut filter = std::collections::HashSet::new();
    filter.insert([0xff; 28]);
    let bytes = encode_filtered_stake_pool_params(&snapshot, &filter);
    assert_eq!(bytes, [0xa0]);
}

/// Round 171 — `decode_pool_hash_set` accepts the canonical
/// `tag(258) [* bytes(28)]` shape upstream cardano-cli sends.
#[test]
fn decode_pool_hash_set_accepts_tagged_set_form() {
    // tag 258 + array(2) + 2 × bytes(28)
    let mut payload = vec![0xd9, 0x01, 0x02, 0x82];
    payload.extend_from_slice(&[0x58, 0x1c]);
    payload.extend_from_slice(&[0xaa; 28]);
    payload.extend_from_slice(&[0x58, 0x1c]);
    payload.extend_from_slice(&[0xbb; 28]);
    let set = decode_pool_hash_set(&payload).expect("decode");
    assert_eq!(set.len(), 2);
    assert!(set.contains(&[0xaa; 28]));
    assert!(set.contains(&[0xbb; 28]));
}

/// Round 171 — `decode_pool_hash_set` also accepts the legacy
/// untagged-array shape for forward-compatibility.
#[test]
fn decode_pool_hash_set_accepts_untagged_array_form() {
    let mut payload = vec![0x81]; // 1-element array, no tag
    payload.extend_from_slice(&[0x58, 0x1c]);
    payload.extend_from_slice(&[0x33; 28]);
    let set = decode_pool_hash_set(&payload).expect("decode");
    assert_eq!(set.len(), 1);
    assert!(set.contains(&[0x33; 28]));
}

/// Round 172 — `GetPoolState` against an empty snapshot with no
/// filter emits the canonical 4-element PState list of empty
/// maps `0x84 0xa0 0xa0 0xa0 0xa0`.
#[test]
fn get_pool_state_empty_snapshot_no_filter_emits_four_empty_maps() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let bytes = encode_pool_state(&snapshot, None);
    assert_eq!(bytes, [0x84, 0xa0, 0xa0, 0xa0, 0xa0]);
}

/// Round 172 — `GetPoolState` against an empty snapshot with a
/// non-matching filter still emits four empty maps (filter
/// applies to every component).
#[test]
fn get_pool_state_empty_snapshot_with_filter_emits_four_empty_maps() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let mut filter = std::collections::HashSet::new();
    filter.insert([0x77; 28]);
    let bytes = encode_pool_state(&snapshot, Some(&filter));
    assert_eq!(bytes, [0x84, 0xa0, 0xa0, 0xa0, 0xa0]);
}

/// Round 172 — `decode_maybe_pool_hash_set` accepts the
/// canonical `[0]` shape for `Nothing`.
#[test]
fn decode_maybe_pool_hash_set_accepts_zero_discriminator() {
    let payload = vec![0x81, 0x00];
    let result = decode_maybe_pool_hash_set(&payload).expect("decode");
    assert!(result.is_none());
}

/// Round 172 — `decode_maybe_pool_hash_set` accepts the
/// `[1, set]` shape for `Just <set>`.
#[test]
fn decode_maybe_pool_hash_set_accepts_one_discriminator_with_set() {
    // [1, tag(258)[bytes(28)]]
    let mut payload = vec![0x82, 0x01, 0xd9, 0x01, 0x02, 0x81, 0x58, 0x1c];
    payload.extend_from_slice(&[0x99; 28]);
    let result = decode_maybe_pool_hash_set(&payload).expect("decode");
    let set = result.expect("Just");
    assert_eq!(set.len(), 1);
    assert!(set.contains(&[0x99; 28]));
}

/// Round 172 — `decode_maybe_pool_hash_set` also accepts a bare
/// `null` (CBOR major 7 / value 22) as `Nothing` for
/// forward-compatibility with upstream encoders that skip the
/// list wrapper.
#[test]
fn decode_maybe_pool_hash_set_accepts_null_as_nothing() {
    let payload = vec![0xf6]; // CBOR null
    let result = decode_maybe_pool_hash_set(&payload).expect("decode");
    assert!(result.is_none());
}

/// Round 173 — `GetStakeSnapshots` against an empty snapshot
/// with no filter emits the canonical 4-element envelope
/// `[empty_map, 0, 0, 0]` = `0x84 0xa0 0x00 0x00 0x00`.
#[test]
fn get_stake_snapshots_empty_snapshot_no_filter_emits_envelope() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let bytes = encode_stake_snapshots(&snapshot, None);
    // R179: NonZero placeholders (1) for mark/set/go totals.
    assert_eq!(bytes, [0x84, 0xa0, 0x01, 0x01, 0x01]);
}

/// Round 173 — `GetStakeSnapshots` against an empty snapshot
/// with a non-matching filter still emits four-element envelope
/// (per-pool map empty, totals zero).
#[test]
fn get_stake_snapshots_empty_snapshot_with_filter_emits_envelope() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let mut filter = std::collections::HashSet::new();
    filter.insert([0xee; 28]);
    let bytes = encode_stake_snapshots(&snapshot, Some(&filter));
    // R179: NonZero placeholders (1) for mark/set/go totals.
    assert_eq!(bytes, [0x84, 0xa0, 0x01, 0x01, 0x01]);
}

/// Round 174 — `decode_pool_hash_set` rejects a non-258 tag
/// (e.g. tag 30 = UnitInterval) instead of silently stripping
/// it.  Pre-R174 the decoder consumed any tag, then tried to
/// parse the next byte as an array length — masking malformed
/// payloads.
#[test]
fn decode_pool_hash_set_rejects_non_258_tag() {
    // tag 30 (UnitInterval) + 0 (the rational num)
    let payload = vec![0xd8, 0x1e, 0x00];
    let result = decode_pool_hash_set(&payload);
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("expected tag 258"),
        "expected error to mention tag 258, got: {msg}"
    );
}

/// Round 174 — `decode_stake_credential_set` rejects a non-258
/// tag for parity with `decode_pool_hash_set`.
#[test]
fn decode_stake_credential_set_rejects_non_258_tag() {
    // tag 30 + 0
    let payload = vec![0xd8, 0x1e, 0x00];
    let result = decode_stake_credential_set(&payload);
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("expected tag 258"),
        "expected error to mention tag 258, got: {msg}"
    );
}

/// Round 174 — `decode_maybe_pool_hash_set` no longer
/// accidentally accepts CBOR `undefined` (`0xf7`) as `Nothing`.
/// Pre-R174 the major-7 check matched undefined/floats/break;
/// post-R174 only `0xf6` (null) shortcuts to `Nothing`.
#[test]
fn decode_maybe_pool_hash_set_rejects_undefined() {
    let payload = vec![0xf7]; // CBOR undefined
    let result = decode_maybe_pool_hash_set(&payload);
    // Should now error rather than silently treating as Nothing.
    assert!(
        result.is_err(),
        "expected err on CBOR undefined, got: {result:?}"
    );
}

/// Round 176 — `decode_address_set` rejects a non-258 tag
/// (parity with R174's tightening of pool/credential set
/// decoders).
#[test]
fn decode_address_set_rejects_non_258_tag() {
    // tag 30 (UnitInterval) + 0
    let payload = vec![0xd8, 0x1e, 0x00];
    let result = decode_address_set(&payload);
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("expected tag 258"),
        "expected error to mention tag 258, got: {msg}"
    );
}

/// Round 176 — `decode_address_set` accepts the canonical
/// `tag(258) [* bytes]` shape (positive case stays working).
#[test]
fn decode_address_set_accepts_tagged_set_form() {
    // tag 258 + array(1) + bytes(3) "abc"
    let payload = vec![0xd9, 0x01, 0x02, 0x81, 0x43, b'a', b'b', b'c'];
    let result = decode_address_set(&payload).expect("decode");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], b"abc");
}

/// Round 176 — `decode_address_set` also accepts the legacy
/// untagged-array shape for forward-compatibility.
#[test]
fn decode_address_set_accepts_untagged_array_form() {
    let payload = vec![0x81, 0x43, b'a', b'b', b'c'];
    let result = decode_address_set(&payload).expect("decode");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], b"abc");
}

/// Round 176 — `decode_txin_set` rejects a non-258 tag
/// (parity with R174's tightening).
#[test]
fn decode_txin_set_rejects_non_258_tag() {
    // tag 30 + 0
    let payload = vec![0xd8, 0x1e, 0x00];
    let result = decode_txin_set(&payload);
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("expected tag 258"),
        "expected error to mention tag 258, got: {msg}"
    );
}

/// Round 177 — `encode_filtered_delegations_and_rewards` emits
/// CBOR map entries in deterministic ascending-key order
/// regardless of the input `HashSet`'s internal iteration
/// order.  Pre-R177 the function iterated `credentials.iter()`
/// directly, producing different byte streams across runs for
/// the same logical input.
///
/// We can't easily build a populated snapshot in a unit test,
/// but we CAN verify that two calls with the same filter set
/// (constructed via different insertion orders) produce
/// byte-identical outputs.  Empty-snapshot baseline output is
/// `[empty_map, empty_map] = 0x82 0xa0 0xa0`.
#[test]
fn encode_filtered_delegations_and_rewards_is_deterministic() {
    use yggdrasil_ledger::StakeCredential;
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Build two HashSets with the same credentials but different
    // insertion orders — `HashSet` iteration order may differ.
    let mut a = std::collections::HashSet::new();
    a.insert(StakeCredential::AddrKeyHash([0x11; 28]));
    a.insert(StakeCredential::AddrKeyHash([0x22; 28]));
    a.insert(StakeCredential::ScriptHash([0x33; 28]));

    let mut b = std::collections::HashSet::new();
    b.insert(StakeCredential::ScriptHash([0x33; 28]));
    b.insert(StakeCredential::AddrKeyHash([0x22; 28]));
    b.insert(StakeCredential::AddrKeyHash([0x11; 28]));

    let bytes_a = encode_filtered_delegations_and_rewards(&snapshot, &a);
    let bytes_b = encode_filtered_delegations_and_rewards(&snapshot, &b);
    assert_eq!(
        bytes_a, bytes_b,
        "filtered-delegations encoding must be order-independent of \
             the input HashSet's iteration order"
    );

    // Empty snapshot still produces empty maps — none of the
    // credentials match a registered delegator/reward account,
    // so both maps are empty, top-level `[map(0), map(0)]`.
    assert_eq!(bytes_a, [0x82, 0xa0, 0xa0]);
}

/// Round 161 — yggdrasil never DEMOTES the era.  When the wire
/// era_tag (e.g. block came in as Conway-codec, era_tag=6) is
/// higher than the PV-derived era (e.g. PV major=5 = Alonzo),
/// we keep the wire era to avoid confusing cardano-cli with
/// regressing era progression.
#[test]
fn effective_era_index_never_demotes_below_wire_era() {
    let mut state = LedgerState::new(Era::Conway);
    state.latest_block_protocol_version = Some((5, 0));
    let snapshot = state.snapshot();
    let actual = effective_era_index_for_lsq(&snapshot);
    assert_eq!(
        actual,
        Era::Conway.era_ordinal() as u32,
        "must keep wire era_tag (Conway=6) when PV-derived would demote",
    );
}

#[test]
fn test_basic_dispatcher_current_era() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Build a [0] query — QueryCurrentEra.
    let mut enc = Encoder::new();
    enc.array(1).unsigned(0u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(
        !result.is_empty(),
        "QueryCurrentEra should return a non-empty response"
    );
}

/// Round 148 — operator-captured upstream `cardano-cli query tip
/// --testnet-magic 1` payloads now route through the upstream
/// codec dispatch and return upstream-shaped responses.
/// `BlockQuery (QueryHardFork GetCurrentEra)` returns
/// `encode_era_index(era_ordinal)` (a 1-element CBOR array
/// `[era_index]`); `BlockQuery (QueryHardFork GetInterpreter)`
/// returns CBOR `null` (`0xf6`) because the full upstream
/// `Interpreter` era-history codec is the Phase-2 follow-up.
/// Pre-fix, the dispatcher returned a 1-byte era ordinal against
/// an upstream client expecting an `EraMismatch`-wrapped result
/// envelope, tearing down the bearer.  Round 147 introduced a
/// defensive null-on-collision guard; Round 148 supersedes it
/// with the actual codec.
#[test]
fn upstream_hardforkblock_query_dispatches_to_typed_responses() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // [0, [2, [1]]] → GetCurrentEra → era_index of Conway = 6.
    // Round 149 — V_23 emits `EraIndex` as bare CBOR uint per the
    // 2026-04-27 socat-proxy capture from `cardano-node 10.7.1`.
    let get_current_era: &[u8] = &[0x82, 0x00, 0x82, 0x02, 0x81, 0x01];
    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, get_current_era);
    assert_eq!(
        result,
        vec![0x06],
        "GetCurrentEra in Conway era must return bare uint 6 at NtC V_23",
    );

    // [0, [2, [0]]] → GetInterpreter → minimal Interpreter shape.
    let get_interpreter: &[u8] = &[0x82, 0x00, 0x82, 0x02, 0x81, 0x00];
    let result_int =
        BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, get_interpreter);
    // Indefinite-length array start `0x9f`, single 3-elem
    // EraSummary, then break `0xff`.
    assert_eq!(result_int[0], 0x9f, "indefinite-length Summary outer");
    assert_eq!(*result_int.last().unwrap(), 0xff, "indef-array break");

    // Sanity: yggdrasil's own flat-table `[0]` (no inner array)
    // continues to work — `UpstreamQuery::decode` rejects
    // length-1 arrays at the top level, so this falls through
    // cleanly to the flat-table dispatcher's `Some(0) =>
    // CurrentEra` branch and returns the era ordinal as a bare
    // unsigned (different shape from the upstream `[era_index]`).
    let yggdrasil_native = [0x81, 0x00];
    let native_result =
        BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &yggdrasil_native);
    assert_eq!(
        native_result,
        vec![0x06],
        "yggdrasil flat-table CurrentEra returns bare unsigned (era ordinal) \
             — distinct from upstream's [era_index] array shape",
    );
}

/// R178 follow-up: Conway `QueryIfCurrent` response envelopes
/// use the HFC `Right` envelope `[body]` on an era match, while era
/// mismatches use the HFC `Left` envelope `[requestedEra, ledgerEra]`.
#[test]
fn conway_query_if_current_uses_hfc_match_and_mismatch_envelopes() {
    use yggdrasil_ledger::{CborEncode, Encoder};
    use yggdrasil_network::protocols::{
        LocalStateQueryMessage, encode_query_if_current_match, encode_query_if_current_mismatch,
    };

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut constitution = Encoder::new();
    snapshot
        .enact_state()
        .constitution()
        .encode_cbor(&mut constitution);
    let conway_cases: [(&str, &[u8], Vec<u8>); 3] = [
        (
            "gov-state",
            &[0x82, 0x00, 0x82, 0x00, 0x82, 0x06, 0x81, 0x18, 0x18],
            encode_conway_gov_state_for_lsq(&snapshot),
        ),
        (
            "constitution",
            &[0x82, 0x00, 0x82, 0x00, 0x82, 0x06, 0x81, 0x17],
            constitution.into_bytes(),
        ),
        (
            "committee-state",
            &[
                0x82, 0x00, 0x82, 0x00, 0x82, 0x06, 0x84, 0x18, 0x1b, 0xd9, 0x01, 0x02, 0x80, 0xd9,
                0x01, 0x02, 0x80, 0xd9, 0x01, 0x02, 0x80,
            ],
            encode_committee_members_state_for_lsq(&snapshot),
        ),
    ];

    for (label, query, body) in conway_cases {
        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, query);
        let expected = encode_query_if_current_match(&body);
        assert_eq!(
            result, expected,
            "{label} QueryIfCurrent match must be the HFC Right envelope [body]",
        );
        assert_eq!(
            LocalStateQueryMessage::MsgResult {
                result: result.clone(),
            }
            .to_cbor(),
            {
                let mut bytes = vec![0x82, 0x04];
                bytes.extend_from_slice(&expected);
                bytes
            },
            "{label} MsgResult frame must inline the QueryIfCurrent envelope",
        );
    }

    let get_gov_state_babbage: &[u8] = &[0x82, 0x00, 0x82, 0x00, 0x82, 0x05, 0x81, 0x18, 0x18];
    let mismatch =
        BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, get_gov_state_babbage);
    assert_eq!(
        mismatch,
        encode_query_if_current_mismatch(lsq_era_index::CONWAY, lsq_era_index::BABBAGE),
        "QueryIfCurrent mismatch must be the HFC Left envelope [requested, ledger]",
    );
    assert_eq!(
        mismatch,
        vec![
            0x82, 0x82, 0x05, 0x67, b'B', b'a', b'b', b'b', b'a', b'g', b'e', 0x82, 0x06, 0x66,
            b'C', b'o', b'n', b'w', b'a', b'y',
        ],
        "mismatch envelope must be [NS Babbage, NS Conway]",
    );
    assert_eq!(
        LocalStateQueryMessage::MsgResult {
            result: mismatch.clone(),
        }
        .to_cbor(),
        {
            let mut bytes = vec![0x82, 0x04];
            bytes.extend_from_slice(&mismatch);
            bytes
        },
        "MsgResult frame must inline the QueryIfCurrent mismatch envelope",
    );
}

/// Round 148: `[3]` is upstream `GetChainPoint`. In yggdrasil's
/// flat table `[3]` is `ProtocolParameters`, so the upstream codec wins.
#[test]
fn upstream_get_chain_point_returns_encoded_tip_point() {
    use yggdrasil_ledger::{HeaderHash, SlotNo};
    let mut state = LedgerState::new(Era::Conway);
    state.tip = yggdrasil_ledger::Point::BlockPoint(SlotNo(42), HeaderHash([0xab; 32]));
    let snapshot = state.snapshot();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &[0x81, 0x03]);
    // Round 149 — V_23 `encodePoint` shape: BlockPoint = [slot, hash]
    // (no constructor tag); Origin = [].  Captured from
    // `cardano-node 10.7.1` socat proxy.
    assert_eq!(result[0], 0x82, "array length 2 for BlockPoint");
    assert_eq!(result[1], 0x18, "uint8 escape for slot 42");
    assert_eq!(result[2], 0x2a, "slot 42");
    assert_eq!(result[3], 0x58, "byte string uint8 length follows");
    assert_eq!(result[4], 0x20, "hash length 32");
}

/// `[2]` is upstream `GetChainBlockNo`.  At chain genesis the
/// snapshot's `tip_block_no` is `None`, so we encode `Origin`
/// (`[0]`).  Once the first block has been applied,
/// `apply_block_validated` populates the field and the dispatcher
/// returns `[1, blockNo]` — see
/// `upstream_get_chain_block_no_returns_block_no_after_apply`
/// below.
#[test]
fn upstream_get_chain_block_no_returns_origin_at_chain_genesis() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();
    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &[0x81, 0x02]);
    assert_eq!(
        result,
        vec![0x81, 0x00],
        "GetChainBlockNo at chain genesis must encode `Origin` (`[0]`)",
    );
}

/// Once `apply_block_validated` has populated `tip_block_no`,
/// `GetChainBlockNo` returns `[1, blockNo]` matching upstream
/// `Ouroboros.Network.Block.Tip.tipBlockNo` — NOT the slot value
/// (which historically leaked through under this label and
/// produced wrong answers for any caller computing chain density
/// from `cardano-cli query tip`).
#[test]
fn upstream_get_chain_block_no_returns_block_no_after_apply() {
    use yggdrasil_ledger::{BlockNo, HeaderHash, SlotNo};
    let mut state = LedgerState::new(Era::Conway);
    state.tip = yggdrasil_ledger::Point::BlockPoint(SlotNo(123_456), HeaderHash([0xcd; 32]));
    state.tip_block_no = Some(BlockNo(7_890));
    let snapshot = state.snapshot();
    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &[0x81, 0x02]);
    // Upstream `encodeChainBlockNo (At b) = encodeListLen 2 <> encodeWord 1 <> encode b`
    // → CBOR `[1, 7890]`.  7890 fits a uint16 escape (`0x19 1ed2`).
    assert_eq!(
        result,
        vec![0x82, 0x01, 0x19, 0x1e, 0xd2],
        "GetChainBlockNo after apply must encode `[1, blockNo]` with the actual \
             block number, not the slot",
    );
}

#[test]
fn test_basic_dispatcher_chain_tip() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Round 148 — `Tip` migrates to upstream `[3]` (`GetChainPoint`).
    let mut enc = Encoder::new();
    enc.array(1).unsigned(3u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(
        !result.is_empty(),
        "GetChainPoint should return a non-empty response"
    );
    // Round 149 — V_23 `encodePoint` shape: Origin is `[]` (empty
    // CBOR array, single byte `0x80`), per
    // `cardano-node 10.7.1` capture.
    assert_eq!(result, vec![0x80]);
}

#[test]
fn test_basic_dispatcher_current_epoch() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Round 148 — yggdrasil-extension `[101]` for `CurrentEpoch`
    // (upstream `[2]` is `GetChainBlockNo`).
    let mut enc = Encoder::new();
    enc.array(1).unsigned(101u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(
        !result.is_empty(),
        "yggdrasil CurrentEpoch ([101]) should return a non-empty response"
    );
}

#[test]
fn test_basic_dispatcher_unknown_tag_returns_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Build a [99] query — unknown tag.
    let mut enc = Encoder::new();
    enc.array(1).unsigned(99u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(
        result.is_empty(),
        "unknown query tag should return empty bytes"
    );
}

#[test]
fn test_basic_dispatcher_empty_query_returns_empty() {
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &[]);
    assert!(
        result.is_empty(),
        "empty query bytes should return empty bytes"
    );
}

#[test]
fn test_basic_dispatcher_protocol_params_null_when_absent() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Round 148 — yggdrasil-extension `[102]` for
    // `ProtocolParameters` (upstream `[3]` is `GetChainPoint`).
    let mut enc = Encoder::new();
    enc.array(1).unsigned(102u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(
        !result.is_empty(),
        "yggdrasil ProtocolParameters ([102]) should return CBOR null"
    );
    // CBOR null is 0xf6
    assert_eq!(result, vec![0xf6]);
}

#[test]
fn test_basic_dispatcher_utxo_by_address_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query [4, address_bytes] — with a dummy address that has no UTxOs.
    let mut enc = Encoder::new();
    // Enterprise address: header 0x61 (type 6, network 1) + 28-byte keyhash
    let mut addr = vec![0x61u8];
    addr.extend_from_slice(&[0xAA; 28]);
    enc.array(2).unsigned(4u64).bytes(&addr);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Should return empty CBOR map: 0xa0
    assert_eq!(result, vec![0xa0]);
}

#[test]
fn test_basic_dispatcher_stake_distribution_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(5u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Should return empty CBOR map: 0xa0
    assert_eq!(result, vec![0xa0]);
}

#[test]
fn test_basic_dispatcher_reward_balance_zero() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Reward account: header 0xe1 (type 14, network 1) + 28-byte keyhash
    let mut acct = vec![0xe1u8];
    acct.extend_from_slice(&[0xBB; 28]);
    let mut enc = Encoder::new();
    enc.array(2).unsigned(6u64).bytes(&acct);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Should return CBOR unsigned 0: 0x00
    assert_eq!(result, vec![0x00]);
}

#[test]
fn test_basic_dispatcher_treasury_and_reserves() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(7u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Should return [treasury, reserves] = [0, 0] on fresh state.
    assert!(!result.is_empty());
    // CBOR [0, 0] is 0x82 0x00 0x00
    assert_eq!(result, vec![0x82, 0x00, 0x00]);
}

#[test]
fn test_basic_dispatcher_get_constitution() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(8u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(
        !result.is_empty(),
        "GetConstitution should return a non-empty CBOR response"
    );
}

#[test]
fn test_basic_dispatcher_get_gov_state_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(9u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Should return empty CBOR map: 0xa0
    assert_eq!(result, vec![0xa0]);
}

#[test]
fn test_basic_dispatcher_get_drep_state_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(10u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // DrepState encodes as a CBOR array; empty = 0x80
    assert_eq!(result, vec![0x80]);
}

#[test]
fn test_basic_dispatcher_get_committee_members_state_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(11u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // CommitteeState encodes as CBOR array; empty = 0x80
    assert_eq!(result, vec![0x80]);
}

#[test]
fn test_basic_dispatcher_get_stake_pool_params_null() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query [12, pool_hash_bytes] with a non-existent pool.
    let mut enc = Encoder::new();
    enc.array(2).unsigned(12u64).bytes(&[0xCC; 28]);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Non-existent pool returns CBOR null: 0xf6
    assert_eq!(result, vec![0xf6]);
}

#[test]
fn test_basic_dispatcher_get_stake_pool_params_no_param() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query [12] with missing parameter.
    let mut enc = Encoder::new();
    enc.array(1).unsigned(12u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Missing param returns CBOR null: 0xf6
    assert_eq!(result, vec![0xf6]);
}

#[test]
fn test_basic_dispatcher_get_account_state() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(13u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Should return [treasury, reserves, total_deposits] = [0, 0, 0] on fresh state.
    assert!(!result.is_empty());
    // CBOR [0, 0, 0] is 0x83 0x00 0x00 0x00
    assert_eq!(result, vec![0x83, 0x00, 0x00, 0x00]);
}

#[test]
fn test_basic_dispatcher_get_utxo_by_txin_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query format: [14, [TxIn, ...]] — send an empty input set.
    let mut enc = Encoder::new();
    enc.array(2).unsigned(14u64);
    enc.array(0); // no inputs
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(!result.is_empty());
    // Empty CBOR map is 0xa0.
    assert_eq!(result, vec![0xa0]);
}

#[test]
fn test_basic_dispatcher_get_utxo_by_txin_nonexistent() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query for a non-existent TxIn.
    let fake_tx_id = [0xab; 32];
    let mut enc = Encoder::new();
    enc.array(2).unsigned(14u64);
    enc.array(1);
    enc.array(2).bytes(&fake_tx_id).unsigned(0u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(!result.is_empty());
    // Should return empty map.
    assert_eq!(result, vec![0xa0]);
}

#[test]
fn test_basic_dispatcher_get_stake_pools_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query format: [15]
    let mut enc = Encoder::new();
    enc.array(1).unsigned(15u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(!result.is_empty());
    // Empty CBOR array is 0x80.
    assert_eq!(result, vec![0x80]);
}

#[test]
fn test_basic_dispatcher_get_delegations_and_rewards_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query format: [16, [credential, ...]] — send empty credential set.
    let mut enc = Encoder::new();
    enc.array(2).unsigned(16u64);
    enc.array(0); // no credentials
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(!result.is_empty());
    // Empty CBOR array is 0x80.
    assert_eq!(result, vec![0x80]);
}

#[test]
fn test_basic_dispatcher_get_delegations_and_rewards_unregistered() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query for an unregistered credential.
    let fake_hash = [0xcc; 28];
    let mut enc = Encoder::new();
    enc.array(2).unsigned(16u64);
    enc.array(1);
    // StakeCredential::AddrKeyHash(fake_hash) — CBOR [0, hash]
    enc.array(2).unsigned(0u64).bytes(&fake_hash);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(!result.is_empty());
    // Unregistered credential returns empty array.
    assert_eq!(result, vec![0x80]);
}

#[test]
fn test_basic_dispatcher_get_drep_stake_distr_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query format: [17]
    let mut enc = Encoder::new();
    enc.array(1).unsigned(17u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    assert!(!result.is_empty());
    // Empty CBOR map is 0xa0.
    assert_eq!(result, vec![0xa0]);
}

#[test]
fn test_basic_dispatcher_get_genesis_delegations_empty() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query [18] — GetGenesisDelegations.
    let mut enc = Encoder::new();
    enc.array(1).unsigned(18u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // Empty CBOR map is 0xa0.
    assert_eq!(result, vec![0xa0]);
}

#[test]
fn test_basic_dispatcher_get_stability_window_unset() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query [19] — GetStabilityWindow.
    let mut enc = Encoder::new();
    enc.array(1).unsigned(19u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // CBOR null is 0xf6.
    assert_eq!(result, vec![0xf6]);
}

#[test]
fn test_basic_dispatcher_get_num_dormant_epochs_zero() {
    use yggdrasil_ledger::Encoder;

    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    // Query [20] — GetNumDormantEpochs.
    let mut enc = Encoder::new();
    enc.array(1).unsigned(20u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // CBOR unsigned 0 is 0x00.
    assert_eq!(result, vec![0x00]);
}

#[test]
fn test_basic_dispatcher_get_expected_network_id_returns_null_when_unset() {
    use yggdrasil_ledger::Encoder;

    // Default `LedgerState::new(Era::Conway)` does not set an expected
    // network id; the dispatcher should surface that as CBOR null so
    // clients can distinguish "unset" from a real id.
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(21u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // CBOR null is 0xf6.
    assert_eq!(result, vec![0xf6]);
}

#[test]
fn test_basic_dispatcher_get_deposit_pot_default_is_all_zeros() {
    use yggdrasil_ledger::Encoder;

    // Fresh ledger has no deposits; all four buckets zero.
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(22u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // 4-element array of four CBOR zeros.
    assert_eq!(result, vec![0x84, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn test_basic_dispatcher_get_deposit_pot_preserves_bucket_order() {
    use yggdrasil_ledger::{Decoder, Encoder};

    // Populate each bucket with a distinct value and verify the wire
    // encoding preserves `[key, pool, drep, proposal]` ordering.
    let mut state = LedgerState::new(Era::Conway);
    state.deposit_pot_mut().add_key_deposit(2_000_000);
    state.deposit_pot_mut().add_pool_deposit(500_000_000);
    state.deposit_pot_mut().add_drep_deposit(500_000_000);
    state
        .deposit_pot_mut()
        .add_proposal_deposit(100_000_000_000);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(22u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);

    let mut dec = Decoder::new(&result);
    assert_eq!(dec.array().unwrap(), 4);
    assert_eq!(dec.unsigned().unwrap(), 2_000_000);
    assert_eq!(dec.unsigned().unwrap(), 500_000_000);
    assert_eq!(dec.unsigned().unwrap(), 500_000_000);
    assert_eq!(dec.unsigned().unwrap(), 100_000_000_000);
}

#[test]
fn test_basic_dispatcher_get_ledger_counts_default_is_all_zero() {
    use yggdrasil_ledger::Encoder;

    // Fresh ledger has zero registered credentials / pools / DReps /
    // committee members / governance actions / gen_delegs.
    let state = LedgerState::new(Era::Conway);
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(23u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // 6-element array of six CBOR zeros.
    assert_eq!(result, vec![0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn test_basic_dispatcher_get_expected_network_id_returns_mainnet_id() {
    use yggdrasil_ledger::Encoder;

    let mut state = LedgerState::new(Era::Conway);
    state.set_expected_network_id(1); // mainnet
    let snapshot = state.snapshot();

    let mut enc = Encoder::new();
    enc.array(1).unsigned(21u64);
    let query = enc.into_bytes();

    let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
    // CBOR unsigned 1 is 0x01.
    assert_eq!(result, vec![0x01]);
}
