// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;

/// Round 152 — pin the preprod Interpreter Byron prefix
/// byte-for-byte against upstream `cardano-node 10.7.1`'s
/// captured wire bytes.  When this regresses, cardano-cli's
/// `query tip` against yggdrasil silently falls back to
/// displaying origin (`slot=0/epoch=0/syncProgress=0.00`) and
/// operator visibility into chain progress is lost.  Capture
/// source: `/tmp/ygg-runbook/haskell-traffic.bin` (socat -x -v
/// proxy of `cardano-cli 10.16.0.0 query tip --testnet-magic 1`
/// against upstream Haskell preprod).
#[test]
fn preprod_interpreter_byron_prefix_matches_upstream_capture() {
    let bytes = encode_interpreter_minimal(21_600, 1);
    // Wire shape (37 bytes for Byron summary):
    //   9f                                 — indef-array opener
    //   83                                 — Byron summary [eraStart, eraEnd, params]
    //     83 00 00 00                      — eraStart [relativeTime=0, slot=0, epoch=0]
    //     83 1b 17fb16d83be00000 1a 00015180 04
    //                                      — eraEnd [1.728e18 ps, 86400, 4]
    //     84 195460 194e20 83 00 1910e0 81 00 1910e0
    //                                      — params [21600, 20000, [0,4320,[0]], 4320]
    let expected_byron_prefix: [u8; 39] = [
        0x9f, // indef-array opener
        0x83, // Byron summary header
        0x83, 0x00, 0x00, 0x00, // eraStart [0,0,0]
        0x83, // eraEnd opener
        0x1b, 0x17, 0xfb, 0x16, 0xd8, 0x3b, 0xe0, 0x00, 0x00, // relativeTime u64 (NOT bignum)
        0x1a, 0x00, 0x01, 0x51, 0x80, // slot 86400
        0x04, // epoch 4
        0x84, // eraParams opener (4 fields)
        0x19, 0x54, 0x60, // epochSize=21600
        0x19, 0x4e, 0x20, // slotLength=20000ms
        0x83, 0x00, 0x19, 0x10, 0xe0, 0x81, 0x00, // safeZone=[0,4320,[0]]
        0x19, 0x10, 0xe0, // genesisWindow=4320
    ];
    assert!(
        bytes.starts_with(&expected_byron_prefix),
        "Byron summary prefix must match upstream capture verbatim — \
             relativeTime is CBOR uint (0x1b prefix), NOT bignum (0xc2 0x48 …); \
             when this drifts, cardano-cli silently falls back to origin tip",
    );
}

/// Round 152 — pin the Shelley summary's `epochSize=432000` and
/// `slotLength=1000ms` against the socat capture.  Earlier
/// drafts used Shelley `epochSize=21600` (Byron-shape) which
/// caused cardano-cli to compute the wrong epoch boundaries
/// (and ultimately fall back to origin display because the
/// Shelley summary failed downstream validation).
#[test]
fn preprod_interpreter_shelley_uses_captured_epoch_size_and_genesis_window() {
    let bytes = encode_interpreter_minimal(21_600, 1);
    // Shelley summary's params block: locate by walking past
    // the Byron summary (38 bytes) then past the Shelley
    // start+end Bound headers.  The Shelley `eraParams` starts
    // with `84 1a 00069780 1903e8 …` — the `0x69780` (432000)
    // is the load-bearing value.
    let shelley_params_marker = [0x84u8, 0x1a, 0x00, 0x06, 0x97, 0x80, 0x19, 0x03, 0xe8];
    assert!(
        bytes
            .windows(shelley_params_marker.len())
            .any(|w| w == shelley_params_marker),
        "Shelley eraParams must start with `84 1a 00069780 1903e8` \
             (epochSize=432000, slotLength=1000ms) — captured from \
             upstream `cardano-node 10.7.1`; using Byron-shape values \
             (21600/20000) here breaks cardano-cli's slot↔epoch \
             conversion",
    );
    // Shelley genesisWindow=129600 (0x1fa40) and
    // safeZone=[0, 129600, [0]] both reuse the same 4-byte literal.
    let shelley_genesis_window = [0x1au8, 0x00, 0x01, 0xfa, 0x40];
    let occurrences = bytes
        .windows(shelley_genesis_window.len())
        .filter(|w| *w == shelley_genesis_window)
        .count();
    assert!(
        occurrences >= 2,
        "Shelley summary must encode 0x1fa40 (=129600) for both \
             safeZone-slots and genesisWindow",
    );
}

/// Round 153 — preview testnet's vendored `shelley-genesis.json`
/// pins `epochLength=86_400` (1-day epochs at 1s/slot) and
/// `config.json` sets every `Test*HardForkAtEpoch=0` (no Byron
/// blocks).  This test pins the resulting wire shape so a future
/// drift in either constant fails CI rather than silently
/// regressing operator-visible cardano-cli output.
#[test]
fn preview_interpreter_emits_single_shelley_summary_with_1day_epochs() {
    let bytes = encode_interpreter_for_network(NetworkKind::Preview);
    // Indef-array opener
    assert_eq!(bytes[0], 0x9f, "Summary indef-array opener");
    assert_eq!(bytes[1], 0x83, "Single EraSummary header (array len 3)");
    // eraStart [0, 0, 0]
    assert_eq!(
        &bytes[2..6],
        &[0x83, 0x00, 0x00, 0x00],
        "Preview eraStart=[0,0,0]"
    );
    // Critical: Preview eraParams use epochSize=86_400 (NOT
    // 432_000 as preprod), encoded as `1a 00 01 51 80`.
    let expected_preview_params_marker = [0x84u8, 0x1a, 0x00, 0x01, 0x51, 0x80, 0x19, 0x03, 0xe8];
    assert!(
        bytes
            .windows(expected_preview_params_marker.len())
            .any(|w| w == expected_preview_params_marker),
        "Preview eraParams must start with `84 1a 00015180 1903e8` \
             (epochSize=86_400, slotLength=1000ms) — preprod's `0x69780` \
             (=432_000) must NOT appear in preview output",
    );
    // Confirm preprod's signature `0x69780` (=432_000) is NOT in
    // the preview output — guards against accidentally falling
    // through to the preprod encoder.
    let preprod_marker = [0x1au8, 0x00, 0x06, 0x97, 0x80];
    assert!(
        !bytes
            .windows(preprod_marker.len())
            .any(|w| w == preprod_marker),
        "Preview must NOT emit preprod's epochSize=432_000",
    );
}

/// Round 153 — preview's `systemStart` is 2022-10-25 (day-of-year
/// 298).  Pin the encoding so a regression in the date constant
/// fails CI cleanly.
#[test]
fn preview_system_start_is_2022_day_298() {
    let bytes = encode_system_start_for_network(NetworkKind::Preview);
    // [year=2022, dayOfYear=298, picosecondsOfDay=0]
    // 2022 = uint16 0x07e6, 298 = uint16 0x012a.
    assert_eq!(bytes, [0x83, 0x19, 0x07, 0xe6, 0x19, 0x01, 0x2a, 0x00]);
}

/// Round 153 — preprod `systemStart` baseline pinned alongside
/// the per-network selector to guard against accidental swap.
#[test]
fn preprod_system_start_is_2022_day_152() {
    let bytes = encode_system_start_for_network(NetworkKind::Preprod);
    // 2022 = 0x07e6, 152 = uint8 0x18 0x98.
    assert_eq!(bytes, [0x83, 0x19, 0x07, 0xe6, 0x18, 0x98, 0x00]);
}

/// Round 156 — captured upstream `cardano-cli 10.16.0.0 query
/// protocol-parameters --testnet-magic 1` payload:
/// `82 03 82 00 82 00 82 01 81 03` =
/// `MsgQuery [BlockQuery [QueryIfCurrent [era_index=1, [GetCurrentPParams=3]]]]`.
/// Pin the decoder so a future drift in any layer fails CI cleanly.
#[test]
fn decode_real_cardano_cli_get_current_pparams_payload() {
    // The full MsgQuery wraps the UpstreamQuery; extract the
    // UpstreamQuery payload (skip the leading `82 03` MsgQuery
    // wrapper which is the LSQ codec's responsibility).
    let upstream_query_bytes = [0x82, 0x00, 0x82, 0x00, 0x82, 0x01, 0x81, 0x03];
    let q = UpstreamQuery::decode(&upstream_query_bytes).expect("must decode");
    let inner = match q {
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent { inner_cbor }) => inner_cbor,
        other => panic!("expected QueryIfCurrent, got {other:?}"),
    };
    let (era_idx, era_query) = decode_query_if_current(&inner).expect("inner decode must succeed");
    assert_eq!(era_idx, 1, "era_index must be Shelley=1");
    assert!(matches!(era_query, EraSpecificQuery::GetCurrentPParams));
}

/// Round 157 — pin the captured `query utxo --whole-utxo`
/// payload `82 00 82 00 82 01 81 07` so a future drift in
/// QueryIfCurrent or `GetWholeUTxO` (era-specific tag 7)
/// fails CI cleanly.
#[test]
fn decode_real_cardano_cli_get_whole_utxo_payload() {
    let bytes = [0x82, 0x00, 0x82, 0x00, 0x82, 0x01, 0x81, 0x07];
    let q = UpstreamQuery::decode(&bytes).expect("must decode");
    let inner = match q {
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent { inner_cbor }) => inner_cbor,
        other => panic!("expected QueryIfCurrent, got {other:?}"),
    };
    let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
    assert_eq!(era_idx, 1);
    assert!(matches!(era_query, EraSpecificQuery::GetWholeUTxO));
}

/// Round 157 — pin the captured `query utxo --tx-in` payload
/// shape: `[era_idx=1, [15, txin_set]]`.  Tag **15** (NOT 14) is
/// the load-bearing fact captured from the 2026-04-28
/// cardano-cli rehearsal.
#[test]
fn decode_real_cardano_cli_get_utxo_by_tx_in_payload() {
    // Inner: `82 01 82 0f 81 82 58 20 <32 bytes txid> 00`.
    let mut inner = vec![0x82, 0x01, 0x82, 0x0f, 0x81, 0x82, 0x58, 0x20];
    inner.extend_from_slice(&[0xa0u8; 32]);
    inner.push(0x00); // index 0
    let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
    assert_eq!(era_idx, 1);
    match era_query {
        EraSpecificQuery::GetUTxOByTxIn { txin_set_cbor } => {
            // First byte must be the array length-1 marker (0x81).
            assert_eq!(txin_set_cbor[0], 0x81, "txin_set is array len 1");
        }
        other => panic!("expected GetUTxOByTxIn, got {other:?}"),
    }
}

/// Round 157 — `GetUTxOByAddress` is era-specific tag 6.  Pin
/// the decoder so a future drift in tag assignment fails CI.
#[test]
fn decode_get_utxo_by_address_recognises_tag_6() {
    // Inner: `[1, [6, [<addr_bytes>]]]`.
    let mut inner = vec![0x82, 0x01, 0x82, 0x06, 0x81, 0x58, 0x1d];
    inner.extend_from_slice(&[0xab; 29]); // 29-byte addr
    let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
    assert_eq!(era_idx, 1);
    assert!(matches!(
        era_query,
        EraSpecificQuery::GetUTxOByAddress { .. }
    ));
}

/// Round 173 (corrected R179) — pin the era-specific tag table
/// addition for `GetStakeSnapshots` (tag 20 per upstream).
/// Wire form mirrors `GetPoolState` (Maybe payload) but with
/// tag 20.
#[test]
fn decode_recognises_stake_snapshots_tag_with_just_filter() {
    // [1, [20, [1, tag(258)[bytes(28)]]]] = era 1, GetStakeSnapshots
    // (Just {single_pool_keyhash})
    let mut payload = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x14, // [tag=20, maybe_payload]
        0x82, 0x01, // [discriminator=1 (Just), set]
        0xd9, 0x01, 0x02, // tag 258
        0x81, // 1-element array
        0x58, 0x1c, // bytes(28)
    ];
    payload.extend_from_slice(&[0x77; 28]);
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetStakeSnapshots { .. }));
}

/// Round 173 (corrected R179) — pin the `Nothing` shape for
/// `GetStakeSnapshots` at tag 20.
#[test]
fn decode_recognises_stake_snapshots_tag_with_nothing_filter() {
    // [1, [20, [0]]] = era 1, GetStakeSnapshots Nothing
    let payload = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x14, // [tag=20, maybe_payload]
        0x81, 0x00, // [0] = Nothing
    ];
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetStakeSnapshots { .. }));
}

/// Round 172 (corrected R179) — pin the era-specific tag table
/// addition for `GetPoolState` (tag 19 per upstream) with
/// `Just <set>` payload.
#[test]
fn decode_recognises_pool_state_tag_with_just_filter() {
    // [1, [19, [1, tag(258)[bytes(28)]]]] = era 1, GetPoolState
    // (Just {single_pool_keyhash})
    // Wire form: 82 01 82 13 82 01 d9 0102 81 581c <28 bytes>
    let mut payload = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x13, // [tag=19, maybe_payload]
        0x82, 0x01, // [discriminator=1 (Just), set]
        0xd9, 0x01, 0x02, // tag 258
        0x81, // 1-element array
        0x58, 0x1c, // bytes(28)
    ];
    payload.extend_from_slice(&[0x55; 28]);
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetPoolState { .. }));
}

/// Round 172 (corrected R179) — pin the `Nothing` shape for
/// `GetPoolState` at tag 19.
#[test]
fn decode_recognises_pool_state_tag_with_nothing_filter() {
    // [1, [19, [0]]] = era 1, GetPoolState Nothing
    // Wire form: 82 01 82 13 81 00
    let payload = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x13, // [tag=19, maybe_payload]
        0x81, 0x00, // [0] = Nothing
    ];
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetPoolState { .. }));
}

/// Round 171 (corrected R179) — pin the era-specific tag table
/// addition for `GetStakePoolParams` (tag 17 per upstream).
#[test]
fn decode_recognises_stake_pool_params_tag() {
    // [1, [17, tag(258)[bytes(28)]]] = era 1, GetStakePoolParams
    // with a single 28-byte pool keyhash in a CIP-21 tagged set.
    // Wire form: 82 01 82 11 d9 0102 81 581c <28 bytes>
    let mut payload = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x11, // [tag=17, set]
        0xd9, 0x01, 0x02, // tag 258
        0x81, // 1-element array
        0x58, 0x1c, // bytes(28)
    ];
    payload.extend_from_slice(&[0xcd; 28]);
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetStakePoolParams { .. }));
}

/// Round 179 — pin upstream tag 37 (`GetStakeDistribution2`,
/// post-Conway no-VRF variant) decoded as `GetStakeDistribution`.
/// `cardano-cli query stake-distribution` sends tag 37 since
/// cardano-node 10.x.
#[test]
fn decode_recognises_stake_distribution2_tag_37() {
    let payload = vec![0x82, 0x01, 0x81, 0x18, 0x25]; // [1, [37]]
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetStakeDistribution));
}

/// Round 189 — pin upstream tag 34 `GetLedgerPeerSnapshot'`.
/// cardano-cli 10.16 sends the v15+ 2-element form with a
/// `peer_kind` byte (`0` = BigLedgerPeers, `1` =
/// AllLedgerPeers).  Older clients may send the 1-element
/// singleton form.
#[test]
fn decode_recognises_ledger_peer_snapshot_tag_34() {
    // [1, [34, 1]] = era 1, GetLedgerPeerSnapshot AllLedgerPeers
    let payload_v15 = vec![0x82, 0x01, 0x82, 0x18, 0x22, 0x01];
    let (era_idx, q) = decode_query_if_current(&payload_v15).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(
        q,
        EraSpecificQuery::GetLedgerPeerSnapshot { peer_kind: Some(1) }
    ));

    // [1, [34]] = era 1, GetLedgerPeerSnapshot pre-v15
    let payload_legacy = vec![0x82, 0x01, 0x81, 0x18, 0x22];
    let (era_idx, q) = decode_query_if_current(&payload_legacy).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(
        q,
        EraSpecificQuery::GetLedgerPeerSnapshot { peer_kind: None }
    ));
}

/// Round 187 — pin upstream tag 32 `GetRatifyState`
/// (singleton query — no parameters).
#[test]
fn decode_recognises_ratify_state_tag_32() {
    // [1, [32]] = era 1, GetRatifyState
    let payload = vec![0x82, 0x01, 0x81, 0x18, 0x20];
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetRatifyState));
}

/// Round 183 — pin upstream tag 33 `GetFuturePParams`
/// (singleton, no parameters).
#[test]
fn decode_recognises_future_pparams_tag_33() {
    // [1, [33]] = era 1, GetFuturePParams
    let payload = vec![0x82, 0x01, 0x81, 0x18, 0x21];
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetFuturePParams));
}

/// Round 184 — pin upstream tags 26 `GetDRepStakeDistr`,
/// 28 `GetFilteredVoteDelegatees`, and 30 `GetSPOStakeDistr`
/// (each 2-element query carrying a filter set).
#[test]
fn decode_recognises_drep_and_spo_stake_distr_tags() {
    // [1, [26, tag(258) [empty]]] = GetDRepStakeDistr
    let drep_stake = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x18, 0x1a, // 2-elem list, tag 26
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
    ];
    let (era_idx, q) = decode_query_if_current(&drep_stake).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetDRepStakeDistr { .. }));

    // [1, [28, tag(258) [empty]]] = GetFilteredVoteDelegatees
    let vote_delegatees = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x18, 0x1c, // 2-elem list, tag 28
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
    ];
    let (era_idx, q) = decode_query_if_current(&vote_delegatees).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(
        q,
        EraSpecificQuery::GetFilteredVoteDelegatees { .. }
    ));

    // [1, [30, tag(258) [empty]]] = GetSPOStakeDistr
    let spo_stake = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x18, 0x1e, // 2-elem list, tag 30
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
    ];
    let (era_idx, q) = decode_query_if_current(&spo_stake).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetSPOStakeDistr { .. }));
}

/// Round 186 — pin upstream tags 22 `GetStakeDelegDeposits`
/// (Map Credential Coin) and 36 `GetPoolDistr2` (PoolDistr
/// with optional pool-id filter).
#[test]
fn decode_recognises_stake_deleg_deposits_and_pool_distr2_tags() {
    // [1, [22, tag(258) [empty]]] = GetStakeDelegDeposits
    let stake_deleg_deposits = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x16, // 2-elem list, tag 22
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
    ];
    let (era_idx, q) = decode_query_if_current(&stake_deleg_deposits).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetStakeDelegDeposits { .. }));

    // [1, [36, []]] = GetPoolDistr2 with `Nothing` filter
    let pool_distr2 = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x18, 0x24, // 2-elem list, tag 36
        0x80, // empty list (Maybe Nothing)
    ];
    let (era_idx, q) = decode_query_if_current(&pool_distr2).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetPoolDistr2 { .. }));
}

/// Round 185 — pin upstream tags 31 `GetProposals` (Seq
/// of GovActionState filtered by gov-action-id set) and
/// 35 `QueryStakePoolDefaultVote` (per-pool default-vote
/// query).
#[test]
fn decode_recognises_proposals_and_default_vote_tags() {
    // [1, [31, tag(258) [empty]]] = GetProposals
    let proposals = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x18, 0x1f, // 2-elem list, tag 31
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
    ];
    let (era_idx, q) = decode_query_if_current(&proposals).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(q, EraSpecificQuery::GetProposals { .. }));

    // [1, [35, bytes(28)]] = QueryStakePoolDefaultVote
    let mut default_vote = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x18, 0x23, // 2-elem list, tag 35
        0x58, 0x1c, // bytes(28)
    ];
    default_vote.extend_from_slice(&[0u8; 28]);
    let (era_idx, q) = decode_query_if_current(&default_vote).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(
        q,
        EraSpecificQuery::QueryStakePoolDefaultVote { .. }
    ));
}

/// Round 182 — pin upstream tag 27 `GetCommitteeMembersState`
/// (4-element query with cold creds + hot creds + statuses
/// filter sets).
#[test]
fn decode_recognises_committee_members_state_tag_27() {
    // [1, [27, set_cold, set_hot, set_status]]
    // = [1, [4-elem [27, tag(258)[empty], tag(258)[empty], tag(258)[empty]]]]
    let payload = vec![
        0x82, 0x01, // [era=1, ...]
        0x84, 0x18, 0x1b, // 4-elem list, tag 27
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array (cold)
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array (hot)
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array (statuses)
    ];
    let (era_idx, q) = decode_query_if_current(&payload).unwrap();
    assert_eq!(era_idx, 1);
    assert!(matches!(
        q,
        EraSpecificQuery::GetCommitteeMembersState { .. }
    ));
}

/// Round 180 — pin upstream Conway-only governance query tags:
/// 23 GetConstitution, 24 GetGovState, 25 GetDRepState (with
/// credential-set parameter), 29 GetAccountState.
#[test]
fn decode_recognises_conway_governance_tags() {
    // [1, [23]] = GetConstitution
    let constitution = vec![0x82, 0x01, 0x81, 0x17];
    let (_, q) = decode_query_if_current(&constitution).unwrap();
    assert!(matches!(q, EraSpecificQuery::GetConstitution));

    // [1, [24]] = GetGovState
    let gov_state = vec![0x82, 0x01, 0x81, 0x18, 0x18];
    let (_, q) = decode_query_if_current(&gov_state).unwrap();
    assert!(matches!(q, EraSpecificQuery::GetGovState));

    // [1, [25, <credential_set>]] = GetDRepState
    let drep_state = vec![
        0x82, 0x01, // [era=1, ...]
        0x82, 0x18, 0x19, // [tag=25, set]
        0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
    ];
    let (_, q) = decode_query_if_current(&drep_state).unwrap();
    assert!(matches!(q, EraSpecificQuery::GetDRepState { .. }));

    // [1, [29]] = GetAccountState
    let account_state = vec![0x82, 0x01, 0x81, 0x18, 0x1d];
    let (_, q) = decode_query_if_current(&account_state).unwrap();
    assert!(matches!(q, EraSpecificQuery::GetAccountState));
}

/// Round 163 (corrected R179) — pin the era-specific tag table
/// for `GetStakeDistribution` (5), `GetFilteredDelegationsAndRewardAccounts`
/// (10), `GetGenesisConfig` (11), `GetStakePools` (tag 16 per
/// upstream — was 13 in R163, off by 3).
#[test]
fn decode_recognises_stake_pool_distribution_genesis_tags() {
    // [1, [5]] = era 1, GetStakeDistribution
    let stake_dist = vec![0x82, 0x01, 0x81, 0x05];
    let (_, q) = decode_query_if_current(&stake_dist).unwrap();
    assert!(matches!(q, EraSpecificQuery::GetStakeDistribution));

    // [1, [11]] = era 1, GetGenesisConfig
    let gen_cfg = vec![0x82, 0x01, 0x81, 0x0b];
    let (_, q) = decode_query_if_current(&gen_cfg).unwrap();
    assert!(matches!(q, EraSpecificQuery::GetGenesisConfig));

    // [1, [16]] = era 1, GetStakePools
    let stake_pools = vec![0x82, 0x01, 0x81, 0x10];
    let (_, q) = decode_query_if_current(&stake_pools).unwrap();
    assert!(matches!(q, EraSpecificQuery::GetStakePools));

    // [1, [10, [<credentials>]]] = GetFilteredDelegationsAndRewardAccounts
    let mut delegs = vec![0x82, 0x01, 0x82, 0x0a, 0x81, 0x82, 0x00, 0x58, 0x1c];
    delegs.extend_from_slice(&[0xab; 28]);
    let (_, q) = decode_query_if_current(&delegs).unwrap();
    assert!(matches!(
        q,
        EraSpecificQuery::GetFilteredDelegationsAndRewardAccounts { .. }
    ));
}

/// Round 156 — encode_query_if_current_match must produce a
/// **1-element** CBOR list (not 2-element with tag) per upstream
/// `encodeEitherMismatch`.  This is the load-bearing wire-shape
/// fact: cardano-cli's decoder uses list-len discrimination
/// between Right (len=1) and Left (len=2) — there is NO leading
/// variant tag for Right.
#[test]
fn encode_query_if_current_match_is_one_element_list_no_tag() {
    let result_payload = [0x91u8, 0x01]; // sentinel inner result
    let envelope = encode_query_if_current_match(&result_payload);
    // 0x81 = array(1), then the inner result bytes verbatim.
    assert_eq!(envelope, [0x81, 0x91, 0x01]);
    assert_ne!(
        envelope[0], 0x82,
        "must NOT be 2-element list — that's the Left/mismatch shape, \
             not Right/match",
    );
}

/// Round 156 — encode_query_if_current_mismatch must produce a
/// 2-element CBOR list of NS-encoded era names per upstream
/// `encodeEitherMismatch` `Left` case.  The order matches
/// upstream: `era1` (the query's requested era) first, then
/// `era2` (the ledger's actual era).
#[test]
fn encode_query_if_current_mismatch_is_two_element_ns_list() {
    // ledger=Shelley(1), query=Babbage(5)
    let bytes = encode_query_if_current_mismatch(1, 5);
    // 0x82 array(2), then `[5, "Babbage"]`, then `[1, "Shelley"]`.
    assert_eq!(bytes[0], 0x82, "outer list len 2");
    assert_eq!(bytes[1], 0x82, "first NS-era is a 2-element list");
    assert_eq!(bytes[2], 0x05, "first NS-era index = 5 (Babbage)");
}

/// Round 156 — encode_shelley_pparams_for_lsq emits the upstream
/// 17-element PParams list with preprod-genesis-shape values.
#[test]
fn shelley_pparams_emit_17_element_list_with_preprod_values() {
    use yggdrasil_ledger::ProtocolParameters;
    let params = ProtocolParameters {
        min_fee_a: 44,
        min_fee_b: 155381,
        max_block_body_size: 65536,
        max_tx_size: 16384,
        max_block_header_size: 1100,
        key_deposit: 2_000_000,
        pool_deposit: 500_000_000,
        e_max: 18,
        n_opt: 150,
        min_utxo_value: Some(1_000_000),
        min_pool_cost: 340_000_000,
        protocol_version: Some((2, 0)),
        ..ProtocolParameters::default()
    };
    let bytes = encode_shelley_pparams_for_lsq(&params);
    // 0x91 = array(17).
    assert_eq!(bytes[0], 0x91, "must be 17-element list");
    // First element: minFeeA = 44 = 0x18 0x2c.
    assert_eq!(&bytes[1..3], &[0x18, 0x2c]);
    // Second: minFeeB = 155381 = 0x1a 0x00 0x02 0x5e 0xf5.
    assert_eq!(&bytes[3..8], &[0x1a, 0x00, 0x02, 0x5e, 0xf5]);
}

/// Round 159 — pin `encode_alonzo_pparams_for_lsq` produces a
/// 24-element CBOR list (Alonzo's `[minfeeA, minfeeB, maxBBSize,
/// maxTxSize, maxBHSize, keyDeposit, poolDeposit, eMax, nOpt,
/// a0, rho, tau, d, extraEntropy, protocolVersion, minPoolCost,
/// coinsPerUtxoWord, costModels, prices, maxTxExUnits,
/// maxBlockExUnits, maxValSize, collateralPercentage,
/// maxCollateralInputs]`).  This is what
/// `cardano-cli 10.16.0.0 query protocol-parameters` against
/// preview's Alonzo era expects, captured during the Round 159
/// operational rehearsal.
#[test]
fn alonzo_pparams_emit_24_element_list() {
    use yggdrasil_ledger::ProtocolParameters;
    let params = ProtocolParameters {
        min_fee_a: 44,
        min_fee_b: 155381,
        max_block_body_size: 65536,
        max_tx_size: 16384,
        max_block_header_size: 1100,
        key_deposit: 2_000_000,
        pool_deposit: 500_000_000,
        e_max: 18,
        n_opt: 150,
        min_utxo_value: None,
        min_pool_cost: 340_000_000,
        coins_per_utxo_byte: Some(34_482 / 8),
        collateral_percentage: Some(150),
        max_collateral_inputs: Some(3),
        max_val_size: Some(5000),
        protocol_version: Some((6, 0)),
        ..ProtocolParameters::default()
    };
    let bytes = encode_alonzo_pparams_for_lsq(&params);
    // 0x98 = uint8-prefix array length follows for len ≥ 24.
    assert_eq!(bytes[0], 0x98, "must be array(N) with N≥24 prefix");
    assert_eq!(bytes[1], 24, "Alonzo PP has 24 fields");
    // Fields 1+2: minFeeA=44, minFeeB=155381 — same as Shelley prefix.
    assert_eq!(&bytes[2..4], &[0x18, 0x2c]);
    assert_eq!(&bytes[4..9], &[0x1a, 0x00, 0x02, 0x5e, 0xf5]);
}

/// Round 160 — pin Babbage PP shape: 22-element list (drops
/// `d` and `extraEntropy` from Alonzo, renames
/// `coinsPerUtxoWord` → `coinsPerUtxoByte`).
#[test]
fn babbage_pparams_emit_22_element_list() {
    use yggdrasil_ledger::ProtocolParameters;
    let params = ProtocolParameters {
        min_fee_a: 44,
        min_fee_b: 155381,
        max_block_body_size: 90112,
        max_tx_size: 16384,
        max_block_header_size: 1100,
        key_deposit: 2_000_000,
        pool_deposit: 500_000_000,
        e_max: 18,
        n_opt: 500,
        min_pool_cost: 340_000_000,
        coins_per_utxo_byte: Some(4_310),
        collateral_percentage: Some(150),
        max_collateral_inputs: Some(3),
        max_val_size: Some(5000),
        protocol_version: Some((8, 0)),
        ..ProtocolParameters::default()
    };
    let bytes = encode_babbage_pparams_for_lsq(&params);
    // 0x96 = array(22) (uint5-inlined since 22 < 24).
    assert_eq!(bytes[0], 0x96, "Babbage PP has 22 fields");
    assert_eq!(&bytes[1..3], &[0x18, 0x2c], "minFeeA=44");
    assert_eq!(
        &bytes[3..8],
        &[0x1a, 0x00, 0x02, 0x5e, 0xf5],
        "minFeeB=155381",
    );
}

/// Round 161 — pin Conway PP shape: 31-element list adding
/// governance fields (pool/DRep voting thresholds, committee
/// params, gov-action lifetime/deposit, DRep deposit/activity,
/// minFeeRefScriptCostPerByte).
#[test]
fn conway_pparams_emit_31_element_list() {
    use yggdrasil_ledger::ProtocolParameters;
    let params = ProtocolParameters {
        min_fee_a: 44,
        min_fee_b: 155381,
        max_block_body_size: 90112,
        max_tx_size: 16384,
        max_block_header_size: 1100,
        key_deposit: 2_000_000,
        pool_deposit: 500_000_000,
        e_max: 18,
        n_opt: 500,
        min_pool_cost: 340_000_000,
        coins_per_utxo_byte: Some(4_310),
        collateral_percentage: Some(150),
        max_collateral_inputs: Some(3),
        max_val_size: Some(5000),
        protocol_version: Some((10, 0)),
        ..ProtocolParameters::default()
    };
    let bytes = encode_conway_pparams_for_lsq(&params);
    // 0x98 = uint8-len-prefix array, 0x1f = 31.
    assert_eq!(bytes[0], 0x98, "must be array(N) with N≥24 prefix");
    assert_eq!(bytes[1], 0x1f, "Conway PP has 31 fields");
    // Field 1: minFeeA=44 = 0x18 0x2c.
    assert_eq!(&bytes[2..4], &[0x18, 0x2c]);
    // Field 2: minFeeB=155381.
    assert_eq!(&bytes[4..9], &[0x1a, 0x00, 0x02, 0x5e, 0xf5]);
}

/// Captured upstream `cardano-cli 10.16.0.0 query tip --testnet-magic 1`
/// payload — `BlockQuery (QueryHardFork GetCurrentEra)`.  Operator
/// rehearsal record in
/// `docs/operational-runs/2026-04-27-runbook-pass.md`.
#[test]
fn decode_real_cardano_cli_get_current_era_payload() {
    let bytes = [0x82, 0x00, 0x82, 0x02, 0x81, 0x01];
    let q = UpstreamQuery::decode(&bytes).expect("must decode");
    assert_eq!(
        q,
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(
            QueryHardFork::GetCurrentEra
        ))
    );
    // Round-trip
    assert_eq!(q.encode(), bytes);
}

#[test]
fn decode_real_cardano_cli_get_interpreter_payload() {
    let bytes = [0x82, 0x00, 0x82, 0x02, 0x81, 0x00];
    let q = UpstreamQuery::decode(&bytes).expect("must decode");
    assert_eq!(
        q,
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(
            QueryHardFork::GetInterpreter
        ))
    );
    assert_eq!(q.encode(), bytes);
}

#[test]
fn decode_get_chain_point_top_level() {
    let bytes = [0x81, 0x03];
    let q = UpstreamQuery::decode(&bytes).expect("must decode");
    assert_eq!(q, UpstreamQuery::GetChainPoint);
    assert_eq!(q.encode(), bytes);
}

#[test]
fn decode_get_chain_block_no_top_level() {
    let bytes = [0x81, 0x02];
    let q = UpstreamQuery::decode(&bytes).expect("must decode");
    assert_eq!(q, UpstreamQuery::GetChainBlockNo);
    assert_eq!(q.encode(), bytes);
}

#[test]
fn decode_get_system_start_top_level() {
    let bytes = [0x81, 0x01];
    let q = UpstreamQuery::decode(&bytes).expect("must decode");
    assert_eq!(q, UpstreamQuery::GetSystemStart);
    assert_eq!(q.encode(), bytes);
}

#[test]
fn decode_query_anytime_get_era_start() {
    // [0, [1, [0], 3]] — BlockQuery (QueryAnytime GetEraStart era=3)
    let mut enc = Encoder::new();
    enc.array(2);
    enc.unsigned(0);
    enc.array(3);
    enc.unsigned(1);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(3);
    let bytes = enc.into_bytes();
    let q = UpstreamQuery::decode(&bytes).expect("must decode");
    assert_eq!(
        q,
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryAnytime {
            kind: QueryAnytimeKind::GetEraStart,
            era_index: 3,
        })
    );
    assert_eq!(q.encode(), bytes);
}

#[test]
fn unrecognised_top_level_tag_is_rejected_cleanly() {
    // `[42]` — invalid top-level tag.
    let bytes = [0x81, 0x18, 0x2a];
    UpstreamQuery::decode(&bytes).expect_err("must reject");
}

#[test]
fn unrecognised_hfc_block_query_tag_rejected() {
    // [0, [99, [0]]]
    let bytes = [0x82, 0x00, 0x82, 0x18, 0x63, 0x81, 0x00];
    UpstreamQuery::decode(&bytes).expect_err("must reject");
}

#[test]
fn query_hardfork_round_trip() {
    for q in [QueryHardFork::GetInterpreter, QueryHardFork::GetCurrentEra] {
        let bytes = q.encode();
        let decoded = QueryHardFork::decode(&bytes).expect("round-trip");
        assert_eq!(decoded, q);
    }
}

#[test]
fn encode_chain_point_origin_is_empty_array() {
    let bytes = encode_chain_point(&Point::Origin);
    assert_eq!(bytes, vec![0x80]);
}

#[test]
fn encode_chain_point_block_point_is_slot_hash_pair() {
    use yggdrasil_ledger::{HeaderHash, SlotNo};
    let p = Point::BlockPoint(SlotNo(42), HeaderHash([0xab; 32]));
    let bytes = encode_chain_point(&p);
    // [42, h'<32 bytes>'] — length 2, no constructor tag.
    assert_eq!(bytes[0], 0x82); // array len 2
    assert_eq!(bytes[1], 0x18); // CBOR uint8 escape
    assert_eq!(bytes[2], 0x2a); // 42
    assert_eq!(bytes[3], 0x58); // bytes uint8 length follows
    assert_eq!(bytes[4], 0x20); // length 32
    // Remaining 32 bytes are the hash payload.
}

/// Operator capture from `cardano-node 10.7.1` at NtC V_23 — Allegra era,
/// slot 610040, hash `ec4a816d...12`.  Inner MsgResult after the [4, ...]
/// wrapper is `82 1a 00 09 4e f8 58 20 ec 4a 81 6d ...` =
/// `[610040, h'<32-byte hash>']`.
#[test]
fn encode_chain_point_matches_real_haskell_capture_block_point() {
    use yggdrasil_ledger::{HeaderHash, SlotNo};
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(
        &hex::decode("ec4a816d939b1999386ffcda5d0df3d96a535c282c59edefdec20a9cd841cf12")
            .expect("valid hex"),
    );
    let p = Point::BlockPoint(SlotNo(610040), HeaderHash(hash_bytes));
    let bytes = encode_chain_point(&p);
    let expected = hex::decode(
        "821a00094ef85820\
             ec4a816d939b1999386ffcda5d0df3d96a535c282c59edefdec20a9cd841cf12",
    )
    .expect("valid hex");
    assert_eq!(bytes, expected);
}

#[test]
fn encode_chain_block_no_origin_and_at() {
    assert_eq!(encode_chain_block_no(None), vec![0x81, 0x00]);
    let at = encode_chain_block_no(Some(100));
    assert_eq!(at[0], 0x82); // array len 2
    assert_eq!(at[1], 0x01); // tag 1 (At)
    assert_eq!(at[2], 0x18); // uint8
    assert_eq!(at[3], 0x64); // 100
}

#[test]
fn encode_era_index_bare_unsigned_v23_shape() {
    // NtC V_23 (negotiated against modern upstream cardano-cli)
    // emits EraIndex as bare CBOR uint, per the 2026-04-27
    // socat-proxy capture from `cardano-node 10.7.1`.
    assert_eq!(encode_era_index(7), vec![0x07]);
    assert_eq!(encode_era_index(0), vec![0x00]);
    assert_eq!(encode_era_index(23), vec![0x17]); // boundary: still 1 byte
    assert_eq!(encode_era_index(24), vec![0x18, 0x18]); // CBOR uint8 escape
}

/// The exact bytes captured from `cardano-node 10.7.1` at NtC V_23 in
/// response to `BlockQuery (QueryHardFork GetCurrentEra)` while in
/// Allegra era — `MsgResult [4, 2]` = `82 04 02`.  `encode_era_index(2)`
/// must match the inner result `02`.
#[test]
fn encode_era_index_matches_real_haskell_capture() {
    assert_eq!(encode_era_index(2), vec![0x02]);
}
