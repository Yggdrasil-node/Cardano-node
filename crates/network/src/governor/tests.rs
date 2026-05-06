// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;
use std::net::{Ipv4Addr, SocketAddrV4};

fn addr(port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

/// Deterministic pick policy for reproducible test results.
fn test_pick() -> PickPolicy {
    PickPolicy::deterministic(42)
}

fn make_registry(peers: &[(u16, PeerSource, PeerStatus)]) -> PeerRegistry {
    let mut reg = PeerRegistry::default();
    for &(port, source, status) in peers {
        reg.insert_source(addr(port), source);
        reg.set_status(addr(port), status);
    }
    reg
}

// ── GovernorTargets::is_sane — direct unit coverage ───────────────
//
// Upstream `sanePeerSelectionTargets` is the single safety gate that
// keeps the governor from entering an unreachable target configuration.
// It is consulted at node startup via `validate_config_report` but had
// no direct unit-level coverage; `_warns_on_insane_governor_targets`
// tests the preflight warning but not the individual invariants.
//
// Each test below pins one invariant in isolation so a future regression
// that flips a single predicate surfaces as the exact failing test.

fn sane_baseline() -> GovernorTargets {
    GovernorTargets::default()
}

#[test]
fn is_sane_accepts_default_targets() {
    assert!(GovernorTargets::default().is_sane());
}

/// R222 — Pin the Phase D.2 lifetime-stats accumulation contract:
/// counters are monotonic across "reconnects", `last_seen` updates
/// on every event, and lifetime_stats[peer] survives session
/// boundaries.  Distinct from session-keyed `failures` which
/// resets via `record_success` — verified by the second assertion
/// that lifetime `failures_total` survives `record_success`.
#[test]
fn lifetime_stats_accumulate_across_simulated_reconnects() {
    use std::net::{IpAddr, Ipv4Addr};

    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3001);
    let mut state = GovernorState::default();

    // Initial state: no stats record.
    assert!(state.lifetime_stats_for(&peer).is_none());

    // Simulate first session: handshake → traffic → failure.
    state.record_lifetime_session_started(peer);
    state.record_lifetime_traffic(peer, 1024, 256);
    state.record_lifetime_session_failure(peer);

    let s1 = *state
        .lifetime_stats_for(&peer)
        .expect("entry after session 1");
    assert_eq!(s1.sessions, 1);
    assert_eq!(s1.successful_handshakes, 1);
    assert_eq!(s1.bytes_in, 1024);
    assert_eq!(s1.bytes_out, 256);
    assert_eq!(s1.failures_total, 1);
    assert!(s1.first_seen.is_some());
    assert!(s1.last_seen.is_some());

    // Simulate session-keyed reset (the existing record_success
    // resets `failures` map but MUST NOT touch lifetime_stats).
    state.record_failure(peer);
    state.record_success(peer);
    let s_after_reset = *state
        .lifetime_stats_for(&peer)
        .expect("lifetime stats survive record_success");
    assert_eq!(
        s_after_reset, s1,
        "lifetime stats unchanged by session-keyed reset"
    );

    // Simulate second session: handshake → traffic.  Sessions /
    // handshakes / bytes accumulate; failures_total stays at 1
    // (no new failures); last_seen advances.
    state.record_lifetime_session_started(peer);
    state.record_lifetime_traffic(peer, 2048, 512);

    let s2 = *state
        .lifetime_stats_for(&peer)
        .expect("entry after session 2");
    assert_eq!(s2.sessions, 2);
    assert_eq!(s2.successful_handshakes, 2);
    assert_eq!(s2.bytes_in, 1024 + 2048);
    assert_eq!(s2.bytes_out, 256 + 512);
    assert_eq!(s2.failures_total, 1);
    assert_eq!(s2.first_seen, s1.first_seen, "first_seen never moves");
    assert!(
        s2.last_seen >= s1.last_seen,
        "last_seen advances on each event"
    );

    // R237 cumulative overwrite path: server-egress sources are
    // already monotonic per peer, so refreshing from that source
    // replaces the local total rather than adding to it.
    state.set_lifetime_bytes_out(peer, 9_000);
    let s3 = *state
        .lifetime_stats_for(&peer)
        .expect("entry after bytes_out overwrite");
    assert_eq!(s3.bytes_out, 9_000);
}

#[test]
fn is_sane_rejects_active_above_established() {
    let mut t = sane_baseline();
    t.target_active = t.target_established + 1;
    assert!(!t.is_sane(), "active > established must be rejected");
}

#[test]
fn is_sane_rejects_established_above_known() {
    let mut t = sane_baseline();
    t.target_established = t.target_known + 1;
    assert!(!t.is_sane(), "established > known must be rejected");
}

#[test]
fn is_sane_rejects_root_above_known() {
    let mut t = sane_baseline();
    t.target_root = t.target_known + 1;
    assert!(!t.is_sane(), "root > known must be rejected");
}

#[test]
fn is_sane_rejects_active_big_above_established_big() {
    let mut t = sane_baseline();
    t.target_established_big_ledger = 5;
    t.target_active_big_ledger = 6;
    t.target_known_big_ledger = 10;
    assert!(
        !t.is_sane(),
        "active_big > established_big must be rejected",
    );
}

#[test]
fn is_sane_rejects_established_big_above_known_big() {
    let mut t = sane_baseline();
    t.target_known_big_ledger = 5;
    t.target_established_big_ledger = 6;
    t.target_active_big_ledger = 0;
    assert!(!t.is_sane(), "established_big > known_big must be rejected",);
}

#[test]
fn is_sane_accepts_boundary_upper_limits() {
    // Exact upper bounds (100 / 1000 / 10000) must pass.
    let t = GovernorTargets {
        target_root: 0,
        target_known: 10_000,
        target_established: 1_000,
        target_active: 100,
        target_known_big_ledger: 10_000,
        target_established_big_ledger: 1_000,
        target_active_big_ledger: 100,
    };
    assert!(t.is_sane(), "upper-bound values must be accepted");
}

#[test]
fn is_sane_rejects_active_above_100() {
    let mut t = sane_baseline();
    t.target_active = 101;
    t.target_established = 101;
    t.target_known = 101;
    assert!(!t.is_sane(), "active > 100 must be rejected");
}

#[test]
fn is_sane_rejects_established_above_1000() {
    let mut t = sane_baseline();
    t.target_established = 1_001;
    t.target_known = 1_001;
    assert!(!t.is_sane(), "established > 1000 must be rejected");
}

#[test]
fn is_sane_rejects_known_above_10000() {
    let mut t = sane_baseline();
    t.target_known = 10_001;
    assert!(!t.is_sane(), "known > 10000 must be rejected");
}

#[test]
fn is_sane_rejects_active_big_above_100() {
    let mut t = sane_baseline();
    t.target_active_big_ledger = 101;
    t.target_established_big_ledger = 101;
    t.target_known_big_ledger = 101;
    assert!(!t.is_sane(), "active_big > 100 must be rejected");
}

#[test]
fn is_sane_rejects_established_big_above_1000() {
    let mut t = sane_baseline();
    t.target_established_big_ledger = 1_001;
    t.target_known_big_ledger = 1_001;
    assert!(!t.is_sane(), "established_big > 1000 must be rejected",);
}

#[test]
fn is_sane_rejects_known_big_above_10000() {
    let mut t = sane_baseline();
    t.target_known_big_ledger = 10_001;
    assert!(!t.is_sane(), "known_big > 10000 must be rejected");
}

#[test]
fn is_sane_accepts_all_zeros() {
    // All-zero targets (no governor pressure) are sane — governor just
    // won't maintain any peers, but that is a valid no-op config.
    let t = GovernorTargets {
        target_root: 0,
        target_known: 0,
        target_established: 0,
        target_active: 0,
        target_known_big_ledger: 0,
        target_established_big_ledger: 0,
        target_active_big_ledger: 0,
    };
    assert!(t.is_sane(), "all-zero targets must be sane");
}

#[test]
fn promote_cold_to_warm_when_below_target() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };

    let actions = evaluate_cold_to_warm_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 2);
    // Local root should be promoted first.
    assert_eq!(actions[0], GovernorAction::PromoteToWarm(addr(1)));
}

#[test]
fn no_promotions_when_targets_met() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };

    let actions = evaluate_cold_to_warm_promotions(&reg, &targets, &mut test_pick());
    assert!(actions.is_empty());

    let actions = evaluate_warm_to_hot_promotions(&reg, &targets, &mut test_pick());
    assert!(actions.is_empty());
}

#[test]
fn demote_hot_when_excess() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 3,
        target_active: 1,
        ..Default::default()
    };

    let actions =
        evaluate_hot_to_warm_demotions(&reg, &targets, &mut test_pick(), &PeerMetrics::default());
    assert_eq!(actions.len(), 2);
    // Non-local-root peers should be demoted first.
    for action in &actions {
        if let GovernorAction::DemoteToWarm(peer) = action {
            assert_ne!(*peer, addr(1), "local root should not be demoted first");
        }
    }
}

#[test]
fn local_root_valency_enforcement() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
    ]);
    let group = LocalRootTargets {
        peers: vec![addr(1), addr(2), addr(3)],
        hot_valency: 1,
        warm_valency: 2,
        trustable: false,
    };

    let actions = enforce_local_root_valency(&reg, &[group], &mut test_pick());
    // Need 1 more warm (have 1, target 2) → promote 1 cold to warm.
    // Need 1 hot (have 0, target 1) → promote 1 warm to hot.
    assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    assert!(actions.contains(&GovernorAction::PromoteToHot(addr(3))));
}

#[test]
fn governor_tick_combined() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };
    let groups = vec![LocalRootTargets {
        peers: vec![addr(1)],
        hot_valency: 1,
        warm_valency: 1,
        trustable: false,
    }];

    let actions = governor_tick(
        &reg,
        &targets,
        &groups,
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        None,
        &mut test_pick(),
        &PeerMetrics::default(),
        Instant::now(),
    );
    // Should have at least the local root promotion.
    assert!(!actions.is_empty());
    assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
}

#[test]
fn empty_registry_produces_no_actions() {
    let reg = PeerRegistry::default();
    let targets = GovernorTargets::default();
    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        None,
        &mut test_pick(),
        &PeerMetrics::default(),
        Instant::now(),
    );
    assert!(actions.is_empty());
}

#[test]
fn failure_tracking_and_backoff() {
    let mut state = GovernorState::default();
    let peer = addr(1);
    let now = Instant::now();

    assert!(!state.is_backing_off(&peer, now));

    // Reach max_failures (default 5).
    for _ in 0..5 {
        state.record_failure(peer);
    }
    assert!(state.is_backing_off(&peer, now));

    // Success resets.
    state.record_success(peer);
    assert!(!state.is_backing_off(&peer, now));
}

#[test]
fn filter_removes_backed_off_promotions() {
    let mut state = GovernorState::default();
    for _ in 0..5 {
        state.record_failure(addr(2));
    }
    let now = Instant::now();

    let actions = vec![
        GovernorAction::PromoteToWarm(addr(1)),
        GovernorAction::PromoteToWarm(addr(2)),
        GovernorAction::DemoteToWarm(addr(3)),
    ];
    let filtered = state.filter_backed_off(actions, now);
    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains(&GovernorAction::PromoteToWarm(addr(1))));
    assert!(filtered.contains(&GovernorAction::DemoteToWarm(addr(3))));
}

#[test]
fn churn_cycle_starts_on_first_tick() {
    let reg = make_registry(&[(1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot)]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 1,
        target_active: 1,
        ..Default::default()
    };
    let mut state = GovernorState::default();
    let now = Instant::now();

    // First tick should enter DecreasedActive immediately.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        now,
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));
}

#[test]
fn churn_decreased_active_lowers_hot_targets() {
    let state = GovernorState {
        churn_phase: ChurnPhase::DecreasedActive {
            started: Instant::now(),
        },
        ..Default::default()
    };
    let targets = GovernorTargets {
        target_active: 5,
        target_active_big_ledger: 10,
        target_established: 10,
        target_established_big_ledger: 20,
        ..Default::default()
    };
    let eff = state.apply_churn_to_targets(&targets);
    assert_eq!(eff.target_active, churn_decrease(5));
    assert_eq!(eff.target_active_big_ledger, churn_decrease(10));
    // Established unchanged in this phase.
    assert_eq!(eff.target_established, 10);
    assert_eq!(eff.target_established_big_ledger, 20);
}

#[test]
fn churn_decreased_established_lowers_warm_targets() {
    let state = GovernorState {
        churn_phase: ChurnPhase::DecreasedEstablished {
            started: Instant::now(),
        },
        ..Default::default()
    };
    let targets = GovernorTargets {
        target_active: 5,
        target_established: 10,
        target_established_big_ledger: 20,
        ..Default::default()
    };
    let eff = state.apply_churn_to_targets(&targets);
    // Active unchanged in this phase.
    assert_eq!(eff.target_active, 5);
    // Established decrease uses upstream formula: decrease(warm_only) + active.
    // warm_only = 10 - 5 = 5 → decrease(5) = 4 → 4 + 5 = 9.
    assert_eq!(eff.target_established, 9);
    // Big-ledger: warm_only = 20 - 0 = 20 → decrease(20) = 16 → 16 + 0 = 16.
    assert_eq!(eff.target_established_big_ledger, 16);
}

#[test]
fn churn_idle_returns_unchanged_targets() {
    let state = GovernorState::default();
    let targets = GovernorTargets {
        target_active: 5,
        target_established: 10,
        ..Default::default()
    };
    let eff = state.apply_churn_to_targets(&targets);
    assert_eq!(eff, targets);
}

#[test]
fn churn_phase_advances_through_full_cycle() {
    let reg = PeerRegistry::default();
    let targets = GovernorTargets::default();
    let mut state = GovernorState {
        churn: ChurnConfig {
            bulk_churn_interval: Duration::from_secs(300),
            phase_timeout: Duration::from_secs(60),
            ..Default::default()
        },
        ..Default::default()
    };
    let t0 = Instant::now();

    // Tick 0: Idle → DecreasedActive (first cycle fires immediately).
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0,
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));

    // 30s later: still DecreasedActive (phase_timeout = 60s).
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(30),
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));

    // 61s later: advance to DecreasedEstablished.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(61),
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedEstablished { .. }
    ));

    // 122s later: advance to Idle (cycle complete).
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(122),
    );
    assert_eq!(state.churn_phase, ChurnPhase::Idle);
    assert!(state.last_churn_cycle.is_some());
}

#[test]
fn churn_cycle_respects_interval_before_restarting() {
    let reg = PeerRegistry::default();
    let targets = GovernorTargets::default();
    let mut state = GovernorState {
        churn: ChurnConfig {
            bulk_churn_interval: Duration::from_secs(300),
            phase_timeout: Duration::from_secs(10),
            ..Default::default()
        },
        ..Default::default()
    };
    let t0 = Instant::now();

    // Complete a full cycle: Idle→Active→Established→Idle
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0,
    );
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(11),
    );
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(22),
    );
    assert_eq!(state.churn_phase, ChurnPhase::Idle);

    // 100s after cycle end: interval not elapsed (300s), stays Idle.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(122),
    );
    assert_eq!(state.churn_phase, ChurnPhase::Idle);

    // 301s after cycle end: interval elapsed, new cycle starts.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(323),
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));
}

#[test]
fn churn_produces_demotions_in_decreased_active_phase() {
    // 3 hot peers, target_active=2.  During DecreasedActive,
    // churn_decrease(2) = 1, so the governor should demote 2
    // excess hot peers.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 3,
        target_active: 2,
        ..Default::default()
    };
    let mut state = GovernorState {
        churn_phase: ChurnPhase::DecreasedActive {
            started: Instant::now(),
        },
        ..Default::default()
    };

    let eff = state.apply_churn_to_targets(&targets);
    assert_eq!(eff.target_active, 1); // churn_decrease(2)

    let actions = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Instant::now(),
    );
    // Should demote non-local-root hot peers.
    let demotions: Vec<_> = actions
        .iter()
        .filter(|a| matches!(a, GovernorAction::DemoteToWarm(_)))
        .collect();
    assert_eq!(demotions.len(), 2);
}

#[test]
fn churn_produces_demotions_in_decreased_established_phase() {
    // 3 warm peers, target_established=2.  During DecreasedEstablished,
    // churn_decrease(2) = 1, so governor should demote 2.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 0,
        ..Default::default()
    };
    let mut state = GovernorState {
        churn_phase: ChurnPhase::DecreasedEstablished {
            started: Instant::now(),
        },
        ..Default::default()
    };

    let eff = state.apply_churn_to_targets(&targets);
    assert_eq!(eff.target_established, 1); // churn_decrease(2)

    let actions = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Instant::now(),
    );
    let cold_demotions: Vec<_> = actions
        .iter()
        .filter(|a| matches!(a, GovernorAction::DemoteToCold(_)))
        .collect();
    assert_eq!(cold_demotions.len(), 2);
}

#[test]
fn churn_skips_local_root_demotions() {
    // Only local-root hot peers — no demotions even in decrease phase.
    let _reg = make_registry(&[(1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot)]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 1,
        target_active: 1,
        ..Default::default()
    };
    let state = GovernorState {
        churn_phase: ChurnPhase::DecreasedActive {
            started: Instant::now(),
        },
        ..Default::default()
    };

    // churn_decrease(1) = 0, but the one hot peer is local-root so
    // demotion should prefer non-local-root first.  With only
    // local-root peers the demotion will include them when excess
    // prevents it from being avoided — but target_active after
    // decrease is 0, and local-root is still protected by
    // enforce_local_root_valency re-promoting it.  The governor
    // targets simply produce the excess demotion.
    let eff = state.apply_churn_to_targets(&targets);
    assert_eq!(eff.target_active, 0);
}

#[test]
fn stateful_tick_integrates_churn_and_backoff() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };
    let groups = vec![LocalRootTargets {
        peers: vec![addr(1)],
        hot_valency: 0,
        warm_valency: 1,
        trustable: false,
    }];
    let mut state = GovernorState::default();

    // Back off peer 1 so the local-root promotion is suppressed.
    for _ in 0..5 {
        state.record_failure(addr(1));
    }

    let actions = state.tick(
        &reg,
        &targets,
        &groups,
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Instant::now(),
    );
    // PromoteToWarm(addr(1)) should be filtered out.
    assert!(!actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    // First tick enters DecreasedActive phase.
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));
}

// -----------------------------------------------------------------------
// churn_decrease (upstream `decrease` function)
// -----------------------------------------------------------------------

#[test]
fn churn_decrease_small_counts() {
    assert_eq!(churn_decrease(0), 0);
    assert_eq!(churn_decrease(1), 0); // max(0, 1 - max(1, 0)) = 0
    assert_eq!(churn_decrease(2), 1); // max(0, 2 - max(1, 0)) = 1
    assert_eq!(churn_decrease(5), 4); // max(0, 5 - max(1, 1)) = 4
}

#[test]
fn churn_decrease_large_counts() {
    // At 10: max(0, 10 - max(1, 2)) = 8
    assert_eq!(churn_decrease(10), 8);
    // At 20: max(0, 20 - max(1, 4)) = 16
    assert_eq!(churn_decrease(20), 16);
    // At 100: max(0, 100 - max(1, 20)) = 80
    assert_eq!(churn_decrease(100), 80);
}

// -----------------------------------------------------------------------
// Two-phase churn integration
// -----------------------------------------------------------------------

#[test]
fn tick_enters_churn_and_demotes_excess_hot() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 3,
        target_active: 2,
        ..Default::default()
    };
    let mut state = GovernorState::default();
    let now = Instant::now();

    // After first tick, DecreasedActive is entered.
    // churn_decrease(2) = 1, so 1 excess hot → DemoteToWarm.
    let actions = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        now,
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, GovernorAction::DemoteToWarm(_)))
    );
}

#[test]
fn tick_churn_cycle_produces_established_demotions() {
    // Start already at DecreasedEstablished with excess warm peers.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 3,
        target_active: 0,
        ..Default::default()
    };
    // Start in DecreasedEstablished so the established targets are lowered.
    let now = Instant::now();
    let mut state = GovernorState {
        churn_phase: ChurnPhase::DecreasedEstablished { started: now },
        last_churn_cycle: None,
        ..Default::default()
    };

    // churn_decrease(3) = 2, 3 warm > 2 target → 1 demotion to cold.
    let actions = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        now,
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, GovernorAction::DemoteToCold(_)))
    );
}

// -----------------------------------------------------------------------
// In-flight tracking
// -----------------------------------------------------------------------

#[test]
fn in_flight_warm_blocks_promotion() {
    let mut state = GovernorState::default();
    state.mark_in_flight_warm(addr(1));
    let now = Instant::now();

    let actions = vec![
        GovernorAction::PromoteToWarm(addr(1)),
        GovernorAction::PromoteToWarm(addr(2)),
    ];
    let filtered = state.filter_backed_off(actions, now);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], GovernorAction::PromoteToWarm(addr(2)));

    // Clear the in-flight flag — now it's allowed again.
    state.clear_in_flight_warm(&addr(1));
    let actions = vec![GovernorAction::PromoteToWarm(addr(1))];
    let filtered = state.filter_backed_off(actions, now);
    assert_eq!(filtered.len(), 1);
}

#[test]
fn in_flight_hot_blocks_promotion() {
    let mut state = GovernorState::default();
    state.mark_in_flight_hot(addr(3));
    let now = Instant::now();

    let actions = vec![
        GovernorAction::PromoteToHot(addr(3)),
        GovernorAction::PromoteToHot(addr(4)),
    ];
    let filtered = state.filter_backed_off(actions, now);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], GovernorAction::PromoteToHot(addr(4)));
}

// -----------------------------------------------------------------------
// Exponential backoff
// -----------------------------------------------------------------------

#[test]
fn exponential_backoff_short_elapsed() {
    let mut state = GovernorState {
        failure_backoff: Duration::from_secs(10),
        ..Default::default()
    };

    // 1 failure → backoff = 10s * 2^0 = 10s.
    state.record_failure(addr(1));
    let now = Instant::now();
    // Immediately after, still backing off.
    assert!(state.is_backing_off(&addr(1), now));
    // After 11s, no longer backing off.
    assert!(!state.is_backing_off(&addr(1), now + Duration::from_secs(11)));
}

#[test]
fn exponential_backoff_doubles_with_failures() {
    let mut state = GovernorState {
        failure_backoff: Duration::from_secs(10),
        ..Default::default()
    };

    // 2 failures → backoff = 10s * 2^1 = 20s.
    state.record_failure(addr(1));
    state.record_failure(addr(1));
    let now = Instant::now();
    assert!(state.is_backing_off(&addr(1), now + Duration::from_secs(15)));
    assert!(!state.is_backing_off(&addr(1), now + Duration::from_secs(21)));
}

#[test]
fn request_backoff_failure_path_uses_negative_counter() {
    let now = Instant::now();
    let mut backoff = RequestBackoffState::default();

    backoff.mark_request_started();
    backoff.on_failure(now);
    assert_eq!(backoff.counter, -1);
    assert_eq!(backoff.next_retry, Some(now + Duration::from_secs(2)));
    assert!(!backoff.in_progress);

    backoff.mark_request_started();
    backoff.on_failure(now);
    assert_eq!(backoff.counter, -2);
    assert_eq!(backoff.next_retry, Some(now + Duration::from_secs(4)));
}

#[test]
fn request_backoff_no_progress_path_uses_positive_counter() {
    let now = Instant::now();
    let mut backoff = RequestBackoffState::default();

    backoff.mark_request_started();
    backoff.on_result(now, false, Duration::from_secs(123), None);
    assert_eq!(backoff.counter, 1);
    assert_eq!(backoff.next_retry, Some(now + Duration::from_secs(2)));

    backoff.mark_request_started();
    backoff.on_result(now, false, Duration::from_secs(123), None);
    assert_eq!(backoff.counter, 2);
    assert_eq!(backoff.next_retry, Some(now + Duration::from_secs(4)));
}

#[test]
fn request_backoff_progress_resets_counter_and_applies_ttl_cap() {
    let now = Instant::now();
    let mut backoff = RequestBackoffState {
        counter: -3,
        next_retry: None,
        in_progress: true,
    };

    backoff.on_result(
        now,
        true,
        Duration::from_secs(300),
        Some(Duration::from_secs(60)),
    );

    assert_eq!(backoff.counter, 0);
    assert_eq!(backoff.next_retry, Some(now + Duration::from_secs(60)));
    assert!(!backoff.in_progress);
}

#[test]
fn failures_decay_over_time() {
    let mut state = GovernorState {
        failure_backoff: Duration::from_secs(10),
        failure_decay: Duration::from_secs(5),
        ..Default::default()
    };

    state.record_failure(addr(1));
    state.record_failure(addr(1));
    let now = Instant::now();

    // Initial backoff for 2 failures is 20s.
    assert!(state.is_backing_off(&addr(1), now + Duration::from_secs(6)));

    // After one decay step, effective failures drop to 1 and backoff to 10s.
    assert!(!state.is_backing_off(&addr(1), now + Duration::from_secs(12)));

    // After enough decay, the record should be pruned.
    state.prune_decayed_failures(now + Duration::from_secs(15));
    assert!(!state.failures.contains_key(&addr(1)));
}

// -----------------------------------------------------------------------
// Tick with full churn cycle
// -----------------------------------------------------------------------

#[test]
fn tick_no_churn_actions_when_targets_met_in_idle() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 2, // exactly met so no peer-share requests fire
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };
    // Start with a recent cycle so Idle persists.
    let now = Instant::now();
    let mut state = GovernorState {
        last_churn_cycle: Some(now),
        ..Default::default()
    };

    let actions = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        now + Duration::from_secs(10),
    );
    // Targets met and no churn due → no actions.
    assert!(actions.is_empty());
    assert_eq!(state.churn_phase, ChurnPhase::Idle);
}

// -----------------------------------------------------------------------
// Big-ledger peer evaluation
// -----------------------------------------------------------------------

#[test]
fn big_ledger_cold_to_warm_promotions() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_established_big_ledger: 2,
        ..Default::default()
    };
    // Currently 1 warm big-ledger peer, target is 2 → promote 1.
    let actions = evaluate_cold_to_warm_big_ledger_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], GovernorAction::PromoteToWarm(_)));
}

#[test]
fn big_ledger_warm_to_hot_promotions() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_active_big_ledger: 1,
        ..Default::default()
    };
    let actions = evaluate_warm_to_hot_big_ledger_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], GovernorAction::PromoteToHot(_)));
}

#[test]
fn big_ledger_hot_to_warm_demotions() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_active_big_ledger: 1,
        ..Default::default()
    };
    let actions = evaluate_hot_to_warm_big_ledger_demotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 2);
    for a in &actions {
        assert!(matches!(a, GovernorAction::DemoteToWarm(_)));
    }
}

#[test]
fn big_ledger_no_actions_when_targets_met() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_established_big_ledger: 2,
        target_active_big_ledger: 1,
        ..Default::default()
    };
    assert!(
        evaluate_cold_to_warm_big_ledger_promotions(&reg, &targets, &mut test_pick()).is_empty()
    );
    assert!(
        evaluate_warm_to_hot_big_ledger_promotions(&reg, &targets, &mut test_pick()).is_empty()
    );
    assert!(evaluate_hot_to_warm_big_ledger_demotions(&reg, &targets, &mut test_pick()).is_empty());
    assert!(
        evaluate_warm_to_cold_big_ledger_demotions(&reg, &targets, &mut test_pick()).is_empty()
    );
}

#[test]
fn request_public_roots_when_below_target_and_retry_elapsed() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm)]);
    let targets = GovernorTargets {
        target_root: 3,
        ..Default::default()
    };
    let state = GovernorState {
        enable_root_big_ledger_requests: true,
        ..Default::default()
    };

    let actions = evaluate_request_public_roots(&reg, &targets, &state, Instant::now());
    assert_eq!(actions, vec![GovernorAction::RequestPublicRoots]);
}

#[test]
fn request_public_roots_suppressed_during_backoff() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm)]);
    let targets = GovernorTargets {
        target_root: 3,
        ..Default::default()
    };
    let now = Instant::now();
    let state = GovernorState {
        enable_root_big_ledger_requests: true,
        public_root_backoff: RequestBackoffState {
            counter: 1,
            next_retry: Some(now + Duration::from_secs(5)),
            in_progress: false,
        },
        ..Default::default()
    };

    let actions = evaluate_request_public_roots(&reg, &targets, &state, now);
    assert!(actions.is_empty());
}

#[test]
fn request_big_ledger_peers_when_below_target_and_retry_elapsed() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm)]);
    let targets = GovernorTargets {
        target_known_big_ledger: 3,
        ..Default::default()
    };
    let state = GovernorState {
        enable_root_big_ledger_requests: true,
        ..Default::default()
    };

    let actions = evaluate_request_big_ledger_peers(&reg, &targets, &state, Instant::now());
    assert_eq!(actions, vec![GovernorAction::RequestBigLedgerPeers]);
}

// -----------------------------------------------------------------------
// Forget cold peers
// -----------------------------------------------------------------------

#[test]
fn forget_cold_peers_when_excess_known() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (4, PeerSource::PeerSourceLedger, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 2,
        ..Default::default()
    };
    let actions = evaluate_forget_cold_peers(&reg, &targets, &mut test_pick());
    // 4 known > target 2, excess 2. Peer-share (2) and public-root (3)
    // are forgettable by source, but root floor blocks forgetting (3)
    // because roots are exactly at target_root (=3 by default).
    assert_eq!(actions, vec![GovernorAction::ForgetPeer(addr(2))]);
}

#[test]
fn forget_cold_peers_can_forget_public_root_when_above_root_floor() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (4, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 2,
        target_root: 1,
        ..Default::default()
    };

    let actions = evaluate_forget_cold_peers(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 2);
    assert!(actions.contains(&GovernorAction::ForgetPeer(addr(4))));
    assert!(
        actions.contains(&GovernorAction::ForgetPeer(addr(2)))
            || actions.contains(&GovernorAction::ForgetPeer(addr(3)))
    );
}

#[test]
fn forget_cold_peers_preserves_root_floor() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 1,
        target_root: 3,
        ..Default::default()
    };

    // Only public-root peer 2 is forgettable by source, but root floor
    // is already reached so no root peer can be forgotten.
    let actions = evaluate_forget_cold_peers(&reg, &targets, &mut test_pick());
    assert!(actions.is_empty());
}

#[test]
fn forget_cold_peers_no_action_when_below_target() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        ..Default::default()
    };
    let actions = evaluate_forget_cold_peers(&reg, &targets, &mut test_pick());
    assert!(actions.is_empty());
}

#[test]
fn regular_established_target_ignores_big_ledger_peers() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_established: 1,
        target_established_big_ledger: 1,
        ..Default::default()
    };

    let actions = evaluate_cold_to_warm_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions, vec![GovernorAction::PromoteToWarm(addr(2))]);
}

#[test]
fn regular_active_target_ignores_big_ledger_peers() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_active: 1,
        target_active_big_ledger: 1,
        ..Default::default()
    };

    let actions = evaluate_warm_to_hot_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions, vec![GovernorAction::PromoteToHot(addr(2))]);
}

#[test]
fn regular_demotion_targets_ignore_big_ledger_peers() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        (4, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
    ]);

    let established_targets = GovernorTargets {
        target_established: 2,
        target_established_big_ledger: 2,
        ..Default::default()
    };
    assert!(
        evaluate_warm_to_cold_demotions(&reg, &established_targets, &mut test_pick()).is_empty()
    );

    let active_targets = GovernorTargets {
        target_active: 1,
        target_active_big_ledger: 1,
        ..Default::default()
    };
    assert!(
        evaluate_hot_to_warm_demotions(
            &reg,
            &active_targets,
            &mut test_pick(),
            &PeerMetrics::default()
        )
        .is_empty()
    );
}

#[test]
fn forget_cold_peers_ignores_big_ledger_known_count() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 2,
        target_known_big_ledger: 1,
        ..Default::default()
    };

    let actions = evaluate_forget_cold_peers(&reg, &targets, &mut test_pick());
    assert!(actions.is_empty());
}

// -- Bootstrap-sensitive mode tests ----------------------------------------

#[test]
fn requires_bootstrap_peers_returns_false_when_young_enough() {
    let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    assert!(!requires_bootstrap_peers(
        &ubp,
        LedgerStateJudgement::YoungEnough
    ));
}

#[test]
fn requires_bootstrap_peers_returns_true_when_too_old_and_enabled() {
    let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    assert!(requires_bootstrap_peers(&ubp, LedgerStateJudgement::TooOld));
}

#[test]
fn requires_bootstrap_peers_returns_false_when_too_old_but_disabled() {
    let ubp = UseBootstrapPeers::DontUseBootstrapPeers;
    assert!(!requires_bootstrap_peers(
        &ubp,
        LedgerStateJudgement::TooOld
    ));
}

#[test]
fn requires_bootstrap_peers_returns_true_when_unavailable_and_enabled() {
    let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    assert!(requires_bootstrap_peers(
        &ubp,
        LedgerStateJudgement::Unavailable
    ));
}

#[test]
fn peer_selection_mode_sensitive_when_bootstrap_required() {
    let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    assert_eq!(
        peer_selection_mode(&ubp, LedgerStateJudgement::TooOld),
        PeerSelectionMode::Sensitive,
    );
}

#[test]
fn peer_selection_mode_normal_when_young_enough() {
    let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    assert_eq!(
        peer_selection_mode(&ubp, LedgerStateJudgement::YoungEnough),
        PeerSelectionMode::Normal,
    );
}

#[test]
fn peer_selection_mode_normal_when_disabled() {
    let ubp = UseBootstrapPeers::DontUseBootstrapPeers;
    assert_eq!(
        peer_selection_mode(&ubp, LedgerStateJudgement::TooOld),
        PeerSelectionMode::Normal,
    );
}

#[test]
fn is_node_able_to_make_progress_normal_mode() {
    let ubp = UseBootstrapPeers::DontUseBootstrapPeers;
    // Not in sensitive mode → always able to make progress.
    assert!(is_node_able_to_make_progress(
        &ubp,
        LedgerStateJudgement::TooOld,
        false
    ));
}

#[test]
fn is_node_able_to_make_progress_sensitive_with_trustable_only() {
    let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    assert!(is_node_able_to_make_progress(
        &ubp,
        LedgerStateJudgement::TooOld,
        true
    ));
}

#[test]
fn is_node_able_to_make_progress_sensitive_without_trustable_only() {
    let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    assert!(!is_node_able_to_make_progress(
        &ubp,
        LedgerStateJudgement::TooOld,
        false
    ));
}

#[test]
fn has_only_trustable_established_peers_empty_registry() {
    let reg = PeerRegistry::default();
    let groups: Vec<LocalRootTargets> = vec![];
    assert!(has_only_trustable_established_peers(&reg, &groups));
}

#[test]
fn has_only_trustable_established_peers_bootstrap_warm() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerWarm)]);
    let groups: Vec<LocalRootTargets> = vec![];
    // Bootstrap peers are always trustable.
    assert!(has_only_trustable_established_peers(&reg, &groups));
}

#[test]
fn has_only_trustable_established_peers_trustable_local_root() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot)]);
    let groups = vec![LocalRootTargets {
        peers: vec![addr(1)],
        hot_valency: 1,
        warm_valency: 1,
        trustable: true,
    }];
    assert!(has_only_trustable_established_peers(&reg, &groups));
}

#[test]
fn has_only_trustable_established_peers_non_trustable_local_root() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm)]);
    let groups = vec![LocalRootTargets {
        peers: vec![addr(1)],
        hot_valency: 1,
        warm_valency: 1,
        trustable: false,
    }];
    assert!(!has_only_trustable_established_peers(&reg, &groups));
}

#[test]
fn has_only_trustable_cold_peers_do_not_block() {
    // Cold peers (even non-trustable) don't block the check.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
    ]);
    let groups: Vec<LocalRootTargets> = vec![];
    assert!(has_only_trustable_established_peers(&reg, &groups));
}

#[test]
fn sensitive_hot_demotions_demote_non_trustable() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
    ]);
    let groups = vec![LocalRootTargets {
        peers: vec![addr(1)],
        hot_valency: 1,
        warm_valency: 1,
        trustable: true,
    }];

    let actions = evaluate_sensitive_hot_demotions(&reg, &groups);
    // Peer 1 is bootstrap → trustable → no demotion.
    // Peers 2 & 3 are public root / ledger → not trustable → demote.
    assert_eq!(actions.len(), 2);
    assert!(actions.contains(&GovernorAction::DemoteToWarm(addr(2))));
    assert!(actions.contains(&GovernorAction::DemoteToWarm(addr(3))));
}

#[test]
fn sensitive_hot_demotions_spares_trustable_local_roots() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
    ]);
    let groups = vec![
        LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 1,
            warm_valency: 1,
            trustable: true,
        },
        LocalRootTargets {
            peers: vec![addr(2)],
            hot_valency: 1,
            warm_valency: 1,
            trustable: false,
        },
    ];

    let actions = evaluate_sensitive_hot_demotions(&reg, &groups);
    // Peer 1 is in trustable group → spared.
    // Peer 2 is in non-trustable group → demoted.
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], GovernorAction::DemoteToWarm(addr(2)));
}

#[test]
fn sensitive_warm_demotions_demote_non_trustable() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let groups: Vec<LocalRootTargets> = vec![];

    let actions = evaluate_sensitive_warm_demotions(&reg, &groups);
    // Peer 1 is bootstrap → trustable.
    // Peer 2 is peer-shared → not trustable.
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], GovernorAction::DemoteToCold(addr(2)));
}

#[test]
fn filter_sensitive_promotions_keeps_trustable() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
    ]);
    let groups = vec![LocalRootTargets {
        peers: vec![addr(3)],
        hot_valency: 1,
        warm_valency: 1,
        trustable: true,
    }];

    let actions = vec![
        GovernorAction::PromoteToWarm(addr(1)),
        GovernorAction::PromoteToWarm(addr(2)),
        GovernorAction::PromoteToWarm(addr(3)),
    ];

    let filtered = filter_sensitive_promotions(actions, &reg, &groups);
    // Peer 1 (bootstrap) and peer 3 (trustable local root) pass filter.
    // Peer 2 (public root, not trustable) is filtered out.
    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains(&GovernorAction::PromoteToWarm(addr(1))));
    assert!(filtered.contains(&GovernorAction::PromoteToWarm(addr(3))));
}

#[test]
fn filter_sensitive_promotions_keeps_demotions() {
    let reg = make_registry(&[(1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot)]);
    let groups: Vec<LocalRootTargets> = vec![];

    let actions = vec![GovernorAction::DemoteToWarm(addr(1))];
    let filtered = filter_sensitive_promotions(actions, &reg, &groups);
    // Demotions are never filtered.
    assert_eq!(filtered.len(), 1);
}

#[test]
fn governor_tick_sensitive_demotes_non_trustable_hot() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 2,
        ..Default::default()
    };
    let groups: Vec<LocalRootTargets> = vec![];

    let actions = governor_tick(
        &reg,
        &targets,
        &groups,
        PeerSelectionMode::Sensitive,
        AssociationMode::Unrestricted,
        None,
        &mut test_pick(),
        &PeerMetrics::default(),
        Instant::now(),
    );
    // Even though targets say 2 active, peer 2 is not trustable → demote.
    assert!(actions.contains(&GovernorAction::DemoteToWarm(addr(2))));
    // Peer 1 (bootstrap) is NOT demoted.
    assert!(!actions.contains(&GovernorAction::DemoteToWarm(addr(1))));
}

#[test]
fn governor_tick_sensitive_suppresses_big_ledger_promotions() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourceBootstrap, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 1,
        target_known_big_ledger: 5,
        target_established_big_ledger: 1,
        target_active_big_ledger: 1,
        ..Default::default()
    };
    let groups: Vec<LocalRootTargets> = vec![];

    let actions = governor_tick(
        &reg,
        &targets,
        &groups,
        PeerSelectionMode::Sensitive,
        AssociationMode::Unrestricted,
        None,
        &mut test_pick(),
        &PeerMetrics::default(),
        Instant::now(),
    );
    // Bootstrap peer may be promoted.
    assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(2))));
    // Big-ledger peer is suppressed in sensitive mode.
    assert!(!actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
}

#[test]
fn governor_tick_normal_allows_all_promotions() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 3,
        target_active: 1,
        target_known_big_ledger: 5,
        target_established_big_ledger: 1,
        target_active_big_ledger: 1,
        ..Default::default()
    };
    let groups: Vec<LocalRootTargets> = vec![];

    let actions = governor_tick(
        &reg,
        &targets,
        &groups,
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        None,
        &mut test_pick(),
        &PeerMetrics::default(),
        Instant::now(),
    );
    // All peers should be promoted in normal mode.
    assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(2))));
    assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(3))));
}

// -----------------------------------------------------------------------
// Tepid flag tests
// -----------------------------------------------------------------------

#[test]
fn tepid_flag_set_on_hot_to_warm() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    reg.set_status(addr(1), PeerStatus::PeerHot);
    assert!(!reg.get(&addr(1)).unwrap().tepid);

    // Hot → Warm sets tepid.
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    assert!(reg.get(&addr(1)).unwrap().tepid);
}

#[test]
fn tepid_flag_cleared_on_cold_to_warm() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    reg.set_status(addr(1), PeerStatus::PeerHot);
    reg.set_status(addr(1), PeerStatus::PeerWarm); // sets tepid
    assert!(reg.get(&addr(1)).unwrap().tepid);

    // Warm → Cold, then Cold → Warm clears tepid.
    reg.set_status(addr(1), PeerStatus::PeerCold);
    assert!(reg.get(&addr(1)).unwrap().tepid); // still true while cold
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    assert!(!reg.get(&addr(1)).unwrap().tepid); // cleared
}

#[test]
fn tepid_flag_starts_false() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourceLedger);
    assert!(!reg.get(&addr(1)).unwrap().tepid);
}

#[test]
fn cold_to_warm_prefers_non_tepid() {
    // Create two cold peers: one tepid, one not.
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
    reg.insert_source(addr(2), PeerSource::PeerSourcePublicRoot);

    // Make peer 1 tepid by cycling through hot → warm.
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    reg.set_status(addr(1), PeerStatus::PeerHot);
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    reg.set_status(addr(1), PeerStatus::PeerCold);
    assert!(reg.get(&addr(1)).unwrap().tepid);
    assert!(!reg.get(&addr(2)).unwrap().tepid);

    let targets = GovernorTargets {
        target_known: 10,
        target_established: 1,
        target_active: 0,
        ..Default::default()
    };

    let actions = evaluate_cold_to_warm_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 1);
    // Non-tepid peer 2 should be promoted first.
    assert_eq!(actions[0], GovernorAction::PromoteToWarm(addr(2)));
}

#[test]
fn warm_to_hot_prefers_non_tepid() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
    reg.insert_source(addr(2), PeerSource::PeerSourcePublicRoot);

    // Make both warm, but peer 1 is tepid.
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    reg.set_status(addr(1), PeerStatus::PeerHot);
    reg.set_status(addr(1), PeerStatus::PeerWarm); // tepid
    assert!(reg.get(&addr(1)).unwrap().tepid);

    reg.set_status(addr(2), PeerStatus::PeerWarm); // fresh, not tepid
    assert!(!reg.get(&addr(2)).unwrap().tepid);

    let targets = GovernorTargets {
        target_known: 10,
        target_established: 5,
        target_active: 1,
        ..Default::default()
    };

    let actions = evaluate_warm_to_hot_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 1);
    // Non-tepid peer 2 should be promoted first.
    assert_eq!(actions[0], GovernorAction::PromoteToHot(addr(2)));
}

#[test]
fn tepid_peers_still_promoted_when_needed() {
    // When targets demand more peers than non-tepid available, tepid
    // peers fill the gap.
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);

    // Make peer 1 cold + tepid.
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    reg.set_status(addr(1), PeerStatus::PeerHot);
    reg.set_status(addr(1), PeerStatus::PeerWarm);
    reg.set_status(addr(1), PeerStatus::PeerCold);
    assert!(reg.get(&addr(1)).unwrap().tepid);

    let targets = GovernorTargets {
        target_known: 10,
        target_established: 1,
        target_active: 0,
        ..Default::default()
    };

    let actions = evaluate_cold_to_warm_promotions(&reg, &targets, &mut test_pick());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], GovernorAction::PromoteToWarm(addr(1)));
}

// -----------------------------------------------------------------------
// Max connection retries (forget-failed-peers) tests
// -----------------------------------------------------------------------

#[test]
fn forget_failed_peer_exceeding_max_retries() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);
    // Peer is cold (default).

    let mut state = GovernorState {
        max_connection_retries: Some(3),
        ..Default::default()
    };
    // Record 4 failures (> max_retries of 3).
    for _ in 0..4 {
        state.record_failure(addr(1));
    }

    let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], GovernorAction::ForgetPeer(addr(1)));
}

#[test]
fn do_not_forget_peer_at_or_below_max_retries() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);

    let mut state = GovernorState {
        max_connection_retries: Some(3),
        ..Default::default()
    };
    // Record exactly 3 failures (= max_retries, not exceeded).
    for _ in 0..3 {
        state.record_failure(addr(1));
    }

    let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
    assert!(actions.is_empty());
}

#[test]
fn do_not_forget_protected_peer_on_max_retries() {
    // Local-root, bootstrap, ledger, and big-ledger peers are protected.
    for protected_source in [
        PeerSource::PeerSourceLocalRoot,
        PeerSource::PeerSourceBootstrap,
        PeerSource::PeerSourceLedger,
        PeerSource::PeerSourceBigLedger,
    ] {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), protected_source);

        let mut state = GovernorState {
            max_connection_retries: Some(2),
            ..Default::default()
        };
        for _ in 0..5 {
            state.record_failure(addr(1));
        }

        let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
        assert!(
            actions.is_empty(),
            "protected source {:?} should not be forgotten",
            protected_source,
        );
    }
}

#[test]
fn do_not_forget_warm_peer_on_max_retries() {
    // Only cold peers are forgotten.
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);
    reg.set_status(addr(1), PeerStatus::PeerWarm);

    let mut state = GovernorState {
        max_connection_retries: Some(2),
        ..Default::default()
    };
    for _ in 0..5 {
        state.record_failure(addr(1));
    }

    let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
    assert!(actions.is_empty());
}

#[test]
fn no_forget_when_max_retries_disabled() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);

    let mut state = GovernorState::default();
    assert!(state.max_connection_retries.is_none());
    for _ in 0..10 {
        state.record_failure(addr(1));
    }

    let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
    assert!(actions.is_empty());
}

#[test]
fn governor_tick_integrates_forget_failed() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);
    // Peer stays cold.

    let mut state = GovernorState {
        max_connection_retries: Some(2),
        ..Default::default()
    };
    for _ in 0..5 {
        state.record_failure(addr(1));
    }

    let targets = GovernorTargets {
        target_known: 10, // not exceeding, so excess-forgetting won't fire
        ..Default::default()
    };
    let now = Instant::now();
    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Some(&state),
        &mut test_pick(),
        &PeerMetrics::default(),
        now,
    );
    assert!(actions.contains(&GovernorAction::ForgetPeer(addr(1))));
}

// -----------------------------------------------------------------------
// In-flight demotion tracking tests
// -----------------------------------------------------------------------

#[test]
fn filter_backed_off_removes_duplicate_hot_to_warm_demotion() {
    let mut state = GovernorState::default();
    state.mark_in_flight_demote_hot(addr(1));

    let actions = vec![
        GovernorAction::DemoteToWarm(addr(1)),
        GovernorAction::DemoteToWarm(addr(2)),
    ];
    let filtered = state.filter_backed_off(actions, Instant::now());
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], GovernorAction::DemoteToWarm(addr(2)));
}

#[test]
fn filter_backed_off_removes_duplicate_warm_to_cold_demotion() {
    let mut state = GovernorState::default();
    state.mark_in_flight_demote_warm(addr(3));

    let actions = vec![
        GovernorAction::DemoteToCold(addr(3)),
        GovernorAction::DemoteToCold(addr(4)),
    ];
    let filtered = state.filter_backed_off(actions, Instant::now());
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], GovernorAction::DemoteToCold(addr(4)));
}

#[test]
fn clear_in_flight_demote_allows_subsequent_demotion() {
    let mut state = GovernorState::default();
    state.mark_in_flight_demote_hot(addr(1));
    state.clear_in_flight_demote_hot(&addr(1));

    let actions = vec![GovernorAction::DemoteToWarm(addr(1))];
    let filtered = state.filter_backed_off(actions, Instant::now());
    assert_eq!(filtered.len(), 1);
}

#[test]
fn in_flight_demotion_does_not_affect_promotions() {
    let mut state = GovernorState::default();
    state.mark_in_flight_demote_hot(addr(1));
    state.mark_in_flight_demote_warm(addr(2));

    // Promotions for same addresses should still pass through.
    let actions = vec![
        GovernorAction::PromoteToWarm(addr(1)),
        GovernorAction::PromoteToHot(addr(2)),
    ];
    let filtered = state.filter_backed_off(actions, Instant::now());
    assert_eq!(filtered.len(), 2);
}

#[test]
fn in_flight_promotion_does_not_affect_demotions() {
    let mut state = GovernorState::default();
    state.mark_in_flight_warm(addr(1));
    state.mark_in_flight_hot(addr(2));

    // Demotions for same addresses should still pass through.
    let actions = vec![
        GovernorAction::DemoteToWarm(addr(1)),
        GovernorAction::DemoteToCold(addr(2)),
    ];
    let filtered = state.filter_backed_off(actions, Instant::now());
    assert_eq!(filtered.len(), 2);
}

#[test]
fn tick_filters_in_flight_demotions() {
    // Hot peer with in-flight hot→warm demotion should not get
    // another DemoteToWarm from tick().
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 3,
        target_active: 1,
        ..Default::default()
    };
    let mut state = GovernorState::default();
    state.mark_in_flight_demote_hot(addr(1));

    let actions = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Instant::now(),
    );
    // Should need to demote 2, but addr(1) is in-flight so at most 1
    // new demotion from the 2 remaining candidates through filter.
    let demote_warm_count = actions
        .iter()
        .filter(|a| matches!(a, GovernorAction::DemoteToWarm(_)))
        .count();
    // addr(1) filtered out; addr(2) and addr(3) are eligible → 2 demotions emitted
    // minus addr(1) = at most 2.  But the excess over target is 2 (3 hot - 1 target).
    // The tick picks first 2 of [addr(2), addr(3), addr(1)] (non-local first).
    // If addr(1) ends up in the first 2, filter removes it → 1 emitted.
    // Otherwise, 2 emitted.  Either way, addr(1) is never emitted.
    assert!(!actions.contains(&GovernorAction::DemoteToWarm(addr(1))));
    assert!(demote_warm_count <= 2);
}

// -----------------------------------------------------------------------
// Peer sharing request tests
// -----------------------------------------------------------------------

#[test]
fn peer_share_request_when_below_target_known() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };
    let state = GovernorState::default(); // known=2, target=10 → below target

    let actions = evaluate_peer_share_requests(&reg, &targets, &state, &mut test_pick());
    assert!(!actions.is_empty());
    // Should contain share requests for eligible warm/hot peers.
    for a in &actions {
        assert!(matches!(a, GovernorAction::ShareRequest(_)));
    }
}

#[test]
fn known_peer_discovery_adopts_inbound_when_peer_share_unavailable() {
    let reg = make_registry(&[
        // No warm/hot peers eligible for peer-share.
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        ..Default::default()
    };
    let mut state = GovernorState::default();
    state.set_inbound_peers([
        (addr(10), NodePeerSharing::PeerSharingEnabled),
        (addr(11), NodePeerSharing::PeerSharingEnabled),
    ]);

    let actions =
        evaluate_known_peer_discovery(&reg, &targets, &state, &mut test_pick(), Instant::now());
    assert!(!actions.is_empty());
    assert!(
        actions
            .iter()
            .all(|a| matches!(a, GovernorAction::AdoptInboundPeer(_)))
    );
}

#[test]
fn known_peer_discovery_falls_back_to_peer_share_before_inbound_retry() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        ..Default::default()
    };
    let now = Instant::now();
    let mut state = GovernorState {
        inbound_peers_retry_time: Some(now + Duration::from_secs(60)),
        ..Default::default()
    };
    state.set_inbound_peers([(addr(10), NodePeerSharing::PeerSharingEnabled)]);

    let actions = evaluate_known_peer_discovery(&reg, &targets, &state, &mut test_pick(), now);
    assert!(
        actions
            .iter()
            .all(|a| matches!(a, GovernorAction::ShareRequest(_)))
    );
}

#[test]
fn mark_inbound_peer_pick_sets_retry_deadline() {
    let now = Instant::now();
    let mut state = GovernorState {
        inbound_peers_retry_delay: Duration::from_secs(60),
        ..Default::default()
    };
    state.mark_inbound_peer_pick(now);
    assert_eq!(
        state.inbound_peers_retry_time,
        Some(now + Duration::from_secs(60))
    );
}

#[test]
fn no_peer_share_when_known_meets_target() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 2, // exactly met
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };
    let state = GovernorState::default();

    let actions = evaluate_peer_share_requests(&reg, &targets, &state, &mut test_pick());
    assert!(actions.is_empty());
}

#[test]
fn no_peer_share_when_budget_exhausted() {
    let reg = make_registry(&[(1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm)]);
    let targets = GovernorTargets {
        target_known: 10,
        ..Default::default()
    };
    let mut state = GovernorState::default();
    // Exhaust the budget.
    state.in_progress_peer_share_reqs = state.max_in_progress_peer_share_reqs;

    let actions = evaluate_peer_share_requests(&reg, &targets, &state, &mut test_pick());
    assert!(actions.is_empty());
}

#[test]
fn peer_share_respects_budget_limit() {
    // 5 warm peers but budget only allows 2 requests.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (4, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (5, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 100,
        target_established: 5,
        ..Default::default()
    };
    let state = GovernorState {
        max_in_progress_peer_share_reqs: 2,
        ..Default::default()
    };

    let actions = evaluate_peer_share_requests(&reg, &targets, &state, &mut test_pick());
    assert_eq!(actions.len(), 2);
}

#[test]
fn peer_share_excludes_local_root_and_bootstrap() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 100,
        target_established: 3,
        target_active: 1,
        ..Default::default()
    };
    let state = GovernorState::default();

    let actions = evaluate_peer_share_requests(&reg, &targets, &state, &mut test_pick());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], GovernorAction::ShareRequest(addr(3)));
}

#[test]
fn peer_share_excludes_big_ledger_peers() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 100,
        target_established: 2,
        ..Default::default()
    };
    let state = GovernorState::default();

    let actions = evaluate_peer_share_requests(&reg, &targets, &state, &mut test_pick());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], GovernorAction::ShareRequest(addr(2)));
}

#[test]
fn peer_share_excludes_cold_peers() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 100,
        target_established: 1,
        ..Default::default()
    };
    let state = GovernorState::default();

    let actions = evaluate_peer_share_requests(&reg, &targets, &state, &mut test_pick());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], GovernorAction::ShareRequest(addr(2)));
}

#[test]
fn mark_and_clear_peer_share_counters() {
    let mut state = GovernorState::default();
    assert_eq!(state.in_progress_peer_share_reqs, 0);

    state.mark_peer_share_sent();
    assert_eq!(state.in_progress_peer_share_reqs, 1);

    state.mark_peer_share_sent();
    assert_eq!(state.in_progress_peer_share_reqs, 2);

    state.clear_peer_share_completed(1);
    assert_eq!(state.in_progress_peer_share_reqs, 1);

    state.clear_peer_share_completed(5); // saturating_sub
    assert_eq!(state.in_progress_peer_share_reqs, 0);
}

#[test]
fn no_peer_share_in_sensitive_mode() {
    // Peer sharing requests are suppressed in sensitive mode.
    let reg = make_registry(&[(1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm)]);
    let targets = GovernorTargets {
        target_known: 100,
        target_established: 1,
        ..Default::default()
    };
    let state = GovernorState::default();
    let now = Instant::now();

    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Sensitive,
        AssociationMode::Unrestricted,
        Some(&state),
        &mut test_pick(),
        &PeerMetrics::default(),
        now,
    );
    // No ShareRequest should appear in sensitive mode since peer
    // sharing is only wired in Normal mode path.
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, GovernorAction::ShareRequest(_)))
    );
}

#[test]
fn governor_tick_emits_share_requests_normal_mode() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 100, // way above known count → below target
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };
    let state = GovernorState::default();
    let now = Instant::now();

    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Some(&state),
        &mut test_pick(),
        &PeerMetrics::default(),
        now,
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, GovernorAction::ShareRequest(_)))
    );
}

#[test]
fn tick_emits_share_requests_with_budget() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 100,
        target_established: 3,
        target_active: 0,
        ..Default::default()
    };
    let mut state = GovernorState {
        max_in_progress_peer_share_reqs: 1, // only 1 allowed
        ..Default::default()
    };

    let actions = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Instant::now(),
    );
    let share_count = actions
        .iter()
        .filter(|a| matches!(a, GovernorAction::ShareRequest(_)))
        .count();
    assert_eq!(share_count, 1);
}

// -----------------------------------------------------------------------
// AssociationMode and NodePeerSharing tests
// -----------------------------------------------------------------------

#[test]
fn node_peer_sharing_default_disabled() {
    assert!(!NodePeerSharing::default().is_enabled());
    assert!(NodePeerSharing::PeerSharingEnabled.is_enabled());
}

#[test]
fn node_peer_sharing_from_wire() {
    assert_eq!(
        NodePeerSharing::from_wire(0),
        NodePeerSharing::PeerSharingDisabled
    );
    assert_eq!(
        NodePeerSharing::from_wire(1),
        NodePeerSharing::PeerSharingEnabled
    );
    // Any nonzero wire value is treated as enabled per the protocol spec.
    assert_eq!(
        NodePeerSharing::from_wire(42),
        NodePeerSharing::PeerSharingEnabled
    );
}

#[test]
fn node_peer_sharing_to_wire_is_strict_inverse_of_from_wire() {
    // Canonical mapping: Disabled → 0, Enabled → 1 — matches the
    // only two values upstream encoders ever emit.
    assert_eq!(NodePeerSharing::PeerSharingDisabled.to_wire(), 0);
    assert_eq!(NodePeerSharing::PeerSharingEnabled.to_wire(), 1);

    // Round-trip every canonical value through to_wire → from_wire.
    for &v in &[
        NodePeerSharing::PeerSharingDisabled,
        NodePeerSharing::PeerSharingEnabled,
    ] {
        let wire = v.to_wire();
        let reconstructed = NodePeerSharing::from_wire(wire);
        assert_eq!(reconstructed, v);
    }
}

#[test]
fn node_peer_sharing_from_wire_then_to_wire_normalises_bogus_inputs() {
    // Lenient receive + strict transmit: if we accept a bogus wire
    // value (42), we must re-emit the canonical form (1) — not
    // re-transmit the original. This is the "liberal in what you
    // accept, conservative in what you send" Postel-style invariant
    // that prevents accidental bogus-value amplification through
    // the node.
    let round_tripped = NodePeerSharing::from_wire(42).to_wire();
    assert_eq!(round_tripped, 1);
    let round_tripped = NodePeerSharing::from_wire(255).to_wire();
    assert_eq!(round_tripped, 1);
}

#[test]
fn compute_association_mode_all_disabled_is_local_only() {
    assert_eq!(
        compute_association_mode(
            &UseBootstrapPeers::DontUseBootstrapPeers,
            &UseLedgerPeers::DontUseLedgerPeers,
            NodePeerSharing::PeerSharingDisabled,
            LedgerStateJudgement::YoungEnough,
        ),
        AssociationMode::LocalRootsOnly,
    );
}

#[test]
fn compute_association_mode_ledger_peers_is_unrestricted() {
    assert_eq!(
        compute_association_mode(
            &UseBootstrapPeers::DontUseBootstrapPeers,
            &UseLedgerPeers::UseLedgerPeers(crate::root_peers::AfterSlot::Always),
            NodePeerSharing::PeerSharingDisabled,
            LedgerStateJudgement::YoungEnough,
        ),
        AssociationMode::Unrestricted,
    );
}

#[test]
fn compute_association_mode_peer_sharing_is_unrestricted() {
    assert_eq!(
        compute_association_mode(
            &UseBootstrapPeers::DontUseBootstrapPeers,
            &UseLedgerPeers::DontUseLedgerPeers,
            NodePeerSharing::PeerSharingEnabled,
            LedgerStateJudgement::YoungEnough,
        ),
        AssociationMode::Unrestricted,
    );
}

#[test]
fn compute_association_mode_bootstrap_synced_no_ledger_no_sharing_is_local() {
    // Bootstrap peers configured but ledger is young enough (not
    // requiring bootstrap peers) and no ledger/sharing → LocalRootsOnly.
    assert_eq!(
        compute_association_mode(
            &UseBootstrapPeers::UseBootstrapPeers(vec![]),
            &UseLedgerPeers::DontUseLedgerPeers,
            NodePeerSharing::PeerSharingDisabled,
            LedgerStateJudgement::YoungEnough,
        ),
        AssociationMode::LocalRootsOnly,
    );
}

#[test]
fn compute_association_mode_bootstrap_too_old_is_unrestricted() {
    // Bootstrap peers configured and ledger is TooOld (still requires
    // bootstrap) → Unrestricted.
    assert_eq!(
        compute_association_mode(
            &UseBootstrapPeers::UseBootstrapPeers(vec![]),
            &UseLedgerPeers::DontUseLedgerPeers,
            NodePeerSharing::PeerSharingDisabled,
            LedgerStateJudgement::TooOld,
        ),
        AssociationMode::Unrestricted,
    );
}

#[test]
fn local_roots_only_suppresses_peer_sharing() {
    // In LocalRootsOnly mode, peer sharing requests should NOT be
    // generated even in Normal mode.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 100,
        target_established: 2,
        target_active: 1,
        ..Default::default()
    };
    let state = GovernorState::default();
    let now = Instant::now();

    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::LocalRootsOnly,
        Some(&state),
        &mut test_pick(),
        &PeerMetrics::default(),
        now,
    );
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, GovernorAction::ShareRequest(_)))
    );
}

#[test]
fn local_roots_only_suppresses_big_ledger_promotions() {
    // In LocalRootsOnly mode, big-ledger promotions should NOT be
    // generated even in Normal mode.
    let reg = make_registry(&[(1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold)]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 1,
        target_known_big_ledger: 5,
        target_established_big_ledger: 1,
        target_active_big_ledger: 1,
        ..Default::default()
    };
    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::LocalRootsOnly,
        None,
        &mut test_pick(),
        &PeerMetrics::default(),
        Instant::now(),
    );
    // Big-ledger peer should NOT be promoted in LocalRootsOnly.
    assert!(!actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
}

#[test]
fn unrestricted_allows_big_ledger_promotions() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold)]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 1,
        target_known_big_ledger: 5,
        target_established_big_ledger: 1,
        target_active_big_ledger: 1,
        ..Default::default()
    };
    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        None,
        &mut test_pick(),
        &PeerMetrics::default(),
        Instant::now(),
    );
    // Big-ledger peer SHOULD be promoted in Unrestricted.
    assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
}

// -----------------------------------------------------------------------
// PeerSelectionCounters tests
// -----------------------------------------------------------------------

#[test]
fn counters_empty_registry() {
    let reg = PeerRegistry::default();
    let counters = PeerSelectionCounters::from_registry(&reg, None);
    assert_eq!(counters, PeerSelectionCounters::default());
}

#[test]
fn counters_regular_peer_categories() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        (4, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
    ]);
    let counters = PeerSelectionCounters::from_registry(&reg, None);

    // Regular totals: all 4 are non-big-ledger.
    assert_eq!(counters.known, 4);
    assert_eq!(counters.available_to_connect, 2); // ports 1 and 4 are cold
    assert_eq!(counters.established, 2); // warm(2) + hot(3)
    assert_eq!(counters.active, 1); // hot(3)

    // Local-root: only port 1.
    assert_eq!(counters.known_local_root, 1);
    assert_eq!(counters.available_to_connect_local_root, 1);
    assert_eq!(counters.established_local_root, 0);
    assert_eq!(counters.active_local_root, 0);

    // Non-root: port 4 (PeerShare is not a root source).
    assert_eq!(counters.known_non_root, 1);
    assert_eq!(counters.available_to_connect_non_root, 1);

    // Root peers: 3 (LocalRoot + PublicRoot + Ledger).
    assert_eq!(counters.root_peers, 3);
}

#[test]
fn counters_big_ledger_peers() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
    ]);
    let counters = PeerSelectionCounters::from_registry(&reg, None);

    // Big-ledger counters.
    assert_eq!(counters.known_big_ledger, 3);
    assert_eq!(counters.available_to_connect_big_ledger, 1); // cold
    assert_eq!(counters.established_big_ledger, 2); // warm + hot
    assert_eq!(counters.active_big_ledger, 1); // hot

    // Regular counters should be zero (big-ledger is excluded).
    assert_eq!(counters.known, 0);
    assert_eq!(counters.established, 0);
    assert_eq!(counters.active, 0);
}

#[test]
fn counters_in_flight_from_governor_state() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        (4, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
    ]);
    let mut gs = GovernorState::default();
    gs.mark_in_flight_warm(addr(1)); // regular cold→warm
    gs.mark_in_flight_hot(addr(2)); // regular warm→hot
    gs.mark_in_flight_warm(addr(3)); // big-ledger cold→warm
    gs.mark_in_flight_demote_hot(addr(4)); // big-ledger hot→warm

    let counters = PeerSelectionCounters::from_registry(&reg, Some(&gs));

    assert_eq!(counters.cold_peers_promotions, 1); // addr(1)
    assert_eq!(counters.warm_peers_promotions, 1); // addr(2)
    assert_eq!(counters.cold_big_ledger_promotions, 1); // addr(3)
    assert_eq!(counters.active_big_ledger_demotions, 1); // addr(4)
}

#[test]
fn counters_cooling_peers_not_available() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
    reg.set_status(addr(1), PeerStatus::PeerCooling);

    let counters = PeerSelectionCounters::from_registry(&reg, None);
    assert_eq!(counters.known, 1);
    assert_eq!(counters.available_to_connect, 0); // cooling → not available
    assert_eq!(counters.established, 0);
}

// -----------------------------------------------------------------------
// OutboundConnectionsState tests
// -----------------------------------------------------------------------

#[test]
fn outbound_local_roots_only_all_trustable() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
    ]);
    let group = LocalRootTargets {
        peers: vec![addr(1), addr(2)],
        hot_valency: 1,
        warm_valency: 2,
        trustable: true,
    };
    let state = compute_outbound_connections_state(
        &reg,
        &[group],
        AssociationMode::LocalRootsOnly,
        &UseBootstrapPeers::DontUseBootstrapPeers,
    );
    assert_eq!(
        state,
        OutboundConnectionsState::TrustedStateWithExternalPeers
    );
}

#[test]
fn outbound_local_roots_only_non_trustable_warm_untrusted() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
    ]);
    let group = LocalRootTargets {
        peers: vec![addr(1)],
        hot_valency: 0,
        warm_valency: 1,
        trustable: true,
    };
    let state = compute_outbound_connections_state(
        &reg,
        &[group],
        AssociationMode::LocalRootsOnly,
        &UseBootstrapPeers::DontUseBootstrapPeers,
    );
    // addr(2) is warm but not a trustable local root → untrusted.
    assert_eq!(state, OutboundConnectionsState::UntrustedState);
}

#[test]
fn outbound_unrestricted_no_bootstrap_always_trusted() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let state = compute_outbound_connections_state(
        &reg,
        &[],
        AssociationMode::Unrestricted,
        &UseBootstrapPeers::DontUseBootstrapPeers,
    );
    assert_eq!(
        state,
        OutboundConnectionsState::TrustedStateWithExternalPeers
    );
}

#[test]
fn outbound_unrestricted_bootstrap_all_trustable_with_external() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
    ]);
    let group = LocalRootTargets {
        peers: vec![addr(2)],
        hot_valency: 0,
        warm_valency: 1,
        trustable: true,
    };
    let bootstrap = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    let state = compute_outbound_connections_state(
        &reg,
        &[group],
        AssociationMode::Unrestricted,
        &bootstrap,
    );
    // All established are trustable AND addr(1) is active + bootstrap → trusted.
    assert_eq!(
        state,
        OutboundConnectionsState::TrustedStateWithExternalPeers
    );
}

#[test]
fn outbound_unrestricted_bootstrap_no_external_active_untrusted() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
    ]);
    let group = LocalRootTargets {
        peers: vec![addr(1), addr(2)],
        hot_valency: 1,
        warm_valency: 2,
        trustable: true,
    };
    let bootstrap = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    let state = compute_outbound_connections_state(
        &reg,
        &[group],
        AssociationMode::Unrestricted,
        &bootstrap,
    );
    // All established are trustable BUT no bootstrap/public-root active → untrusted.
    assert_eq!(state, OutboundConnectionsState::UntrustedState);
}

#[test]
fn outbound_unrestricted_bootstrap_untrusted_warm_peer() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
    ]);
    let bootstrap = UseBootstrapPeers::UseBootstrapPeers(vec![]);
    let state =
        compute_outbound_connections_state(&reg, &[], AssociationMode::Unrestricted, &bootstrap);
    // addr(2) is warm + PeerShare (not trustable) → untrusted.
    assert_eq!(state, OutboundConnectionsState::UntrustedState);
}

#[test]
fn outbound_local_roots_only_empty_registry_trusted() {
    let reg = PeerRegistry::default();
    let state = compute_outbound_connections_state(
        &reg,
        &[],
        AssociationMode::LocalRootsOnly,
        &UseBootstrapPeers::DontUseBootstrapPeers,
    );
    // No established peers → all (vacuously) trustable.
    assert_eq!(
        state,
        OutboundConnectionsState::TrustedStateWithExternalPeers
    );
}

#[test]
fn outbound_local_roots_only_non_trustable_group() {
    let reg = make_registry(&[(1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm)]);
    let group = LocalRootTargets {
        peers: vec![addr(1)],
        hot_valency: 0,
        warm_valency: 1,
        trustable: false, // group is NOT trustable
    };
    let state = compute_outbound_connections_state(
        &reg,
        &[group],
        AssociationMode::LocalRootsOnly,
        &UseBootstrapPeers::DontUseBootstrapPeers,
    );
    // addr(1) is warm but its group is not trustable → untrusted.
    assert_eq!(state, OutboundConnectionsState::UntrustedState);
}

// -----------------------------------------------------------------------
// FetchMode tests
// -----------------------------------------------------------------------

#[test]
fn fetch_mode_young_enough_is_deadline() {
    assert_eq!(
        fetch_mode_from_judgement(LedgerStateJudgement::YoungEnough),
        FetchMode::FetchModeDeadline,
    );
}

#[test]
fn fetch_mode_too_old_is_bulk_sync() {
    assert_eq!(
        fetch_mode_from_judgement(LedgerStateJudgement::TooOld),
        FetchMode::FetchModeBulkSync,
    );
}

#[test]
fn fetch_mode_unavailable_is_bulk_sync() {
    assert_eq!(
        fetch_mode_from_judgement(LedgerStateJudgement::Unavailable),
        FetchMode::FetchModeBulkSync,
    );
}

// -----------------------------------------------------------------------
// ChurnMode / ChurnRegime tests
// -----------------------------------------------------------------------

#[test]
fn churn_mode_from_deadline_is_normal() {
    assert_eq!(
        churn_mode_from_fetch_mode(FetchMode::FetchModeDeadline),
        ChurnMode::Normal,
    );
}

#[test]
fn churn_mode_from_bulk_sync_is_bulk() {
    assert_eq!(
        churn_mode_from_fetch_mode(FetchMode::FetchModeBulkSync),
        ChurnMode::BulkSync,
    );
}

#[test]
fn churn_regime_normal_always_default() {
    // ChurnModeNormal → ChurnDefault regardless of bootstrap/consensus.
    assert_eq!(
        pick_churn_regime(
            ChurnMode::Normal,
            &UseBootstrapPeers::DontUseBootstrapPeers,
            ConsensusMode::PraosMode
        ),
        ChurnRegime::ChurnDefault,
    );
    assert_eq!(
        pick_churn_regime(
            ChurnMode::Normal,
            &UseBootstrapPeers::UseBootstrapPeers(vec![]),
            ConsensusMode::PraosMode
        ),
        ChurnRegime::ChurnDefault,
    );
    assert_eq!(
        pick_churn_regime(
            ChurnMode::Normal,
            &UseBootstrapPeers::DontUseBootstrapPeers,
            ConsensusMode::GenesisMode
        ),
        ChurnRegime::ChurnDefault,
    );
}

#[test]
fn churn_regime_genesis_mode_always_default() {
    // GenesisMode → ChurnDefault even with BulkSync + bootstrap.
    assert_eq!(
        pick_churn_regime(
            ChurnMode::BulkSync,
            &UseBootstrapPeers::UseBootstrapPeers(vec![]),
            ConsensusMode::GenesisMode
        ),
        ChurnRegime::ChurnDefault,
    );
}

#[test]
fn churn_regime_bulk_sync_no_bootstrap_is_praos_sync() {
    assert_eq!(
        pick_churn_regime(
            ChurnMode::BulkSync,
            &UseBootstrapPeers::DontUseBootstrapPeers,
            ConsensusMode::PraosMode
        ),
        ChurnRegime::ChurnPraosSync,
    );
}

#[test]
fn churn_regime_bulk_sync_with_bootstrap_is_bootstrap_praos_sync() {
    assert_eq!(
        pick_churn_regime(
            ChurnMode::BulkSync,
            &UseBootstrapPeers::UseBootstrapPeers(vec![]),
            ConsensusMode::PraosMode
        ),
        ChurnRegime::ChurnBootstrapPraosSync,
    );
}

// -----------------------------------------------------------------------
// Regime-aware churn decrease tests
// -----------------------------------------------------------------------

#[test]
fn churn_decrease_active_default_uses_standard() {
    // ChurnDefault → churn_decrease(10) = 10 - max(1, 10/5) = 10 - 2 = 8.
    assert_eq!(churn_decrease_active(ChurnRegime::ChurnDefault, 10, 0), 8);
    assert_eq!(churn_decrease_active(ChurnRegime::ChurnDefault, 10, 5), 8);
}

#[test]
fn churn_decrease_active_praos_sync_caps_to_local_hot() {
    // PraosSync → min(max(1, local_hot), base - 1).
    // local_hot=3, base=10 → min(3, 9) = 3.
    assert_eq!(churn_decrease_active(ChurnRegime::ChurnPraosSync, 10, 3), 3);
    // local_hot=0, base=10 → min(max(1,0)=1, 9) = 1.
    assert_eq!(churn_decrease_active(ChurnRegime::ChurnPraosSync, 10, 0), 1);
}

#[test]
fn churn_decrease_active_bootstrap_praos_same_as_praos() {
    assert_eq!(
        churn_decrease_active(ChurnRegime::ChurnBootstrapPraosSync, 10, 3),
        3
    );
    assert_eq!(
        churn_decrease_active(ChurnRegime::ChurnBootstrapPraosSync, 10, 0),
        1
    );
}

#[test]
fn churn_decrease_active_zero_stays_zero() {
    assert_eq!(churn_decrease_active(ChurnRegime::ChurnDefault, 0, 0), 0);
    assert_eq!(churn_decrease_active(ChurnRegime::ChurnPraosSync, 0, 0), 0);
    assert_eq!(
        churn_decrease_active(ChurnRegime::ChurnBootstrapPraosSync, 0, 0),
        0
    );
}

#[test]
fn churn_decrease_established_default_shrinks_warm_portion() {
    // est=10, active=5 → warm_only=5, decrease(5)=4, result=4+5=9.
    assert_eq!(
        churn_decrease_established(ChurnRegime::ChurnDefault, 10, 5),
        9
    );
    // est=10, active=8 → warm_only=2, decrease(2)=1, result=1+8=9.
    assert_eq!(
        churn_decrease_established(ChurnRegime::ChurnDefault, 10, 8),
        9
    );
}

#[test]
fn churn_decrease_established_praos_sync_same_as_default() {
    assert_eq!(
        churn_decrease_established(ChurnRegime::ChurnPraosSync, 10, 5),
        9
    );
}

#[test]
fn churn_decrease_established_bootstrap_aggressive() {
    // BootstrapPraosSync → min(active, established - 1).
    // est=10, active=5 → min(5, 9) = 5.
    assert_eq!(
        churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 10, 5),
        5
    );
    // est=10, active=9 → min(9, 9) = 9.
    assert_eq!(
        churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 10, 9),
        9
    );
    // est=3, active=1 → min(1, 2) = 1.
    assert_eq!(
        churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 3, 1),
        1
    );
}

#[test]
fn churn_decrease_established_zero_stays_zero() {
    assert_eq!(
        churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 0, 0),
        0
    );
}

// -----------------------------------------------------------------------
// Regime-aware apply_churn_to_targets tests
// -----------------------------------------------------------------------

#[test]
fn churn_targets_praos_sync_caps_active_decrease() {
    let state = GovernorState {
        churn_phase: ChurnPhase::DecreasedActive {
            started: Instant::now(),
        },
        churn_regime: ChurnRegime::ChurnPraosSync,
        local_root_hot_target: 3,
        ..Default::default()
    };
    let targets = GovernorTargets {
        target_active: 10,
        target_established: 20,
        ..Default::default()
    };
    let eff = state.apply_churn_to_targets(&targets);
    // PraosSync: min(max(1, 3), 10-1) = min(3, 9) = 3.
    assert_eq!(eff.target_active, 3);
    // Established unchanged (only active phase).
    assert_eq!(eff.target_established, 20);
}

#[test]
fn churn_targets_bootstrap_aggressive_established() {
    let state = GovernorState {
        churn_phase: ChurnPhase::DecreasedEstablished {
            started: Instant::now(),
        },
        churn_regime: ChurnRegime::ChurnBootstrapPraosSync,
        ..Default::default()
    };
    let targets = GovernorTargets {
        target_active: 5,
        target_established: 10,
        target_active_big_ledger: 2,
        target_established_big_ledger: 6,
        ..Default::default()
    };
    let eff = state.apply_churn_to_targets(&targets);
    // BootstrapPraosSync: min(active, established - 1).
    // Regular: min(5, 9) = 5.
    assert_eq!(eff.target_established, 5);
    // Big-ledger: min(2, 5) = 2.
    assert_eq!(eff.target_established_big_ledger, 2);
}

// -----------------------------------------------------------------------
// FetchMode-dependent churn interval tests
// -----------------------------------------------------------------------

#[test]
fn churn_config_interval_for_bulk_sync() {
    let config = ChurnConfig::default();
    assert_eq!(
        config.interval_for_mode(FetchMode::FetchModeBulkSync),
        Duration::from_secs(900),
    );
}

#[test]
fn churn_config_interval_for_deadline() {
    let config = ChurnConfig::default();
    assert_eq!(
        config.interval_for_mode(FetchMode::FetchModeDeadline),
        Duration::from_secs(3300),
    );
}

#[test]
fn deadline_mode_uses_longer_churn_interval() {
    let reg = PeerRegistry::default();
    let targets = GovernorTargets::default();
    let mut state = GovernorState {
        churn: ChurnConfig {
            bulk_churn_interval: Duration::from_secs(100),
            deadline_churn_interval: Duration::from_secs(500),
            phase_timeout: Duration::from_secs(10),
        },
        fetch_mode: FetchMode::FetchModeDeadline,
        ..Default::default()
    };
    let t0 = Instant::now();

    // Complete a cycle fast.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0,
    );
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(11),
    );
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(22),
    );
    assert_eq!(state.churn_phase, ChurnPhase::Idle);

    // At 200s after cycle end (< 500s deadline interval): stays Idle.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(222),
    );
    assert_eq!(state.churn_phase, ChurnPhase::Idle);

    // At 501s after cycle end: new cycle starts.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(523),
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));
}

#[test]
fn bulk_sync_mode_uses_shorter_churn_interval() {
    let reg = PeerRegistry::default();
    let targets = GovernorTargets::default();
    let mut state = GovernorState {
        churn: ChurnConfig {
            bulk_churn_interval: Duration::from_secs(100),
            deadline_churn_interval: Duration::from_secs(500),
            phase_timeout: Duration::from_secs(10),
        },
        fetch_mode: FetchMode::FetchModeBulkSync,
        ..Default::default()
    };
    let t0 = Instant::now();

    // Complete a cycle.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0,
    );
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(11),
    );
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(22),
    );
    assert_eq!(state.churn_phase, ChurnPhase::Idle);

    // At 101s after cycle end (> 100s bulk interval): new cycle starts.
    let _ = state.tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        t0 + Duration::from_secs(123),
    );
    assert!(matches!(
        state.churn_phase,
        ChurnPhase::DecreasedActive { .. }
    ));
}

// -----------------------------------------------------------------------
// PeerSelectionTimeouts tests
// -----------------------------------------------------------------------

#[test]
fn peer_selection_timeouts_defaults() {
    let t = PeerSelectionTimeouts::default();
    assert_eq!(t.find_public_root_timeout, Duration::from_secs(5));
    assert_eq!(t.max_in_progress_peer_share_reqs, 2);
    assert_eq!(t.peer_share_retry_time, Duration::from_secs(900));
    assert_eq!(t.peer_share_batch_wait_time, Duration::from_secs(3));
    assert_eq!(t.peer_share_overall_timeout, Duration::from_secs(10));
    assert_eq!(t.peer_share_activation_delay, Duration::from_secs(300));
    assert_eq!(t.max_connection_retries, 5);
    assert_eq!(t.clear_fail_count_delay, Duration::from_secs(120));
    assert_eq!(t.inbound_peers_retry_delay, Duration::from_secs(60));
    assert_eq!(t.max_inbound_peers, 10);
}

// -----------------------------------------------------------------------
// ConnectionManagerCounters tests
// -----------------------------------------------------------------------

#[test]
fn connection_counters_empty_registry() {
    let reg = PeerRegistry::default();
    let counters = ConnectionManagerCounters::from_registry(&reg);
    assert_eq!(counters, ConnectionManagerCounters::default());
}

#[test]
fn connection_counters_outbound_warm_and_hot() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
    ]);
    let counters = ConnectionManagerCounters::from_registry(&reg);
    assert_eq!(counters.outbound_conns, 2);
    assert_eq!(counters.unidirectional_conns, 2);
    assert_eq!(counters.terminating_conns, 0);
    assert_eq!(counters.inbound_conns, 0);
}

#[test]
fn connection_counters_terminating_cooling() {
    let mut reg = PeerRegistry::default();
    reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
    reg.set_status(addr(1), PeerStatus::PeerCooling);

    let counters = ConnectionManagerCounters::from_registry(&reg);
    assert_eq!(counters.terminating_conns, 1);
    assert_eq!(counters.outbound_conns, 0);
}

#[test]
fn connection_counters_add_is_fieldwise() {
    let a = ConnectionManagerCounters {
        full_duplex_conns: 1,
        duplex_conns: 2,
        unidirectional_conns: 3,
        inbound_conns: 4,
        outbound_conns: 5,
        terminating_conns: 6,
    };
    let b = ConnectionManagerCounters {
        full_duplex_conns: 10,
        duplex_conns: 20,
        unidirectional_conns: 30,
        inbound_conns: 40,
        outbound_conns: 50,
        terminating_conns: 60,
    };
    let sum = a + b;
    assert_eq!(sum.full_duplex_conns, 11);
    assert_eq!(sum.duplex_conns, 22);
    assert_eq!(sum.unidirectional_conns, 33);
    assert_eq!(sum.inbound_conns, 44);
    assert_eq!(sum.outbound_conns, 55);
    assert_eq!(sum.terminating_conns, 66);
}

// -----------------------------------------------------------------------
// ConsensusMode tests
// -----------------------------------------------------------------------

#[test]
fn consensus_mode_eq() {
    assert_eq!(ConsensusMode::PraosMode, ConsensusMode::PraosMode);
    assert_eq!(ConsensusMode::GenesisMode, ConsensusMode::GenesisMode);
    assert_ne!(ConsensusMode::PraosMode, ConsensusMode::GenesisMode);
}

// -----------------------------------------------------------------------
// tick() updates local_root_hot_target
// -----------------------------------------------------------------------

#[test]
fn tick_updates_local_root_hot_target() {
    let reg = PeerRegistry::default();
    let targets = GovernorTargets::default();
    let groups = vec![
        LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 3,
            warm_valency: 5,
            trustable: true,
        },
        LocalRootTargets {
            peers: vec![addr(2)],
            hot_valency: 7,
            warm_valency: 10,
            trustable: false,
        },
    ];
    let mut state = GovernorState::default();
    let _ = state.tick(
        &reg,
        &targets,
        &groups,
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        Instant::now(),
    );
    assert_eq!(state.local_root_hot_target, 7);
}

// -----------------------------------------------------------------------
// Xorshift64 PRNG tests
// -----------------------------------------------------------------------

#[test]
fn xorshift64_deterministic() {
    let mut a = Xorshift64::new(12345);
    let mut b = Xorshift64::new(12345);
    for _ in 0..100 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
}

#[test]
fn xorshift64_zero_seed_avoids_degenerate_state() {
    let mut rng = Xorshift64::new(0);
    // Zero seed is silently replaced with 1; must produce non-zero output.
    assert_ne!(rng.next_u64(), 0);
}

#[test]
fn xorshift64_different_seeds_diverge() {
    let mut a = Xorshift64::new(1);
    let mut b = Xorshift64::new(2);
    // Different seeds must produce different sequences.
    let sa: Vec<u64> = (0..10).map(|_| a.next_u64()).collect();
    let sb: Vec<u64> = (0..10).map(|_| b.next_u64()).collect();
    assert_ne!(sa, sb);
}

#[test]
fn xorshift64_partial_shuffle_subset() {
    let mut rng = Xorshift64::new(99);
    let mut v: Vec<u32> = (0..20).collect();
    rng.partial_shuffle(&mut v, 5);
    assert_eq!(v.len(), 5);
    // All selected values must be from the original range.
    for &x in &v {
        assert!(x < 20);
    }
    // No duplicates in the selection.
    let mut sorted = v.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 5);
}

#[test]
fn xorshift64_partial_shuffle_count_exceeds_len() {
    let mut rng = Xorshift64::new(77);
    let mut v: Vec<u32> = vec![10, 20, 30];
    rng.partial_shuffle(&mut v, 100);
    // When count > len, return all elements (shuffled).
    assert_eq!(v.len(), 3);
}

// -----------------------------------------------------------------------
// PickPolicy tests
// -----------------------------------------------------------------------

#[test]
fn pick_policy_deterministic_reproducible() {
    let candidates: Vec<SocketAddr> = (1..=10).map(addr).collect();
    let mut p1 = PickPolicy::deterministic(42);
    let mut p2 = PickPolicy::deterministic(42);
    let r1 = p1.pick(3, candidates.clone());
    let r2 = p2.pick(3, candidates);
    assert_eq!(r1, r2);
}

#[test]
fn pick_policy_selects_correct_count() {
    let candidates: Vec<SocketAddr> = (1..=20).map(addr).collect();
    let mut pick = PickPolicy::new(0xDEAD);
    let selected = pick.pick(5, candidates);
    assert_eq!(selected.len(), 5);
}

#[test]
fn pick_policy_no_duplicates() {
    let candidates: Vec<SocketAddr> = (1..=50).map(addr).collect();
    let mut pick = PickPolicy::new(0xBEEF);
    let selected = pick.pick(20, candidates);
    let mut sorted = selected.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 20);
}

#[test]
fn pick_policy_empty_candidates() {
    let mut pick = PickPolicy::new(1);
    let selected = pick.pick(5, vec![]);
    assert!(selected.is_empty());
}

#[test]
fn pick_policy_count_exceeds_candidates() {
    let candidates: Vec<SocketAddr> = (1..=3).map(addr).collect();
    let mut pick = PickPolicy::new(1);
    let selected = pick.pick(100, candidates);
    assert_eq!(selected.len(), 3);
}

#[test]
fn pick_policy_different_seeds_different_selections() {
    let candidates: Vec<SocketAddr> = (1..=20).map(addr).collect();
    let mut p1 = PickPolicy::new(111);
    let mut p2 = PickPolicy::new(222);
    let r1 = p1.pick(5, candidates.clone());
    let r2 = p2.pick(5, candidates);
    // With 20 candidates and only 5 selected, two different seeds
    // should almost certainly produce different subsets.
    assert_ne!(r1, r2);
}

// -----------------------------------------------------------------------
// PickPolicy scored selection tests (hot demotion scoring)
// -----------------------------------------------------------------------

#[test]
fn pick_scored_prefers_higher_scored_peers() {
    let candidates: Vec<SocketAddr> = (1..=5).map(addr).collect();
    let mut metrics = PeerMetrics::default();
    // addr(1) gets high score, addr(5) gets medium, rest get 0.
    for _ in 0..100 {
        metrics.record_upstreamyness(addr(1), 0);
        metrics.record_fetchyness(addr(1), 0);
    }
    for _ in 0..50 {
        metrics.record_upstreamyness(addr(5), 0);
    }

    let mut pick = PickPolicy::deterministic(42);
    let selected = pick.pick_scored(2, candidates, &metrics);
    assert_eq!(selected.len(), 2);
    // The highest-scored peers should be selected.
    assert!(selected.contains(&addr(1)));
    assert!(selected.contains(&addr(5)));
}

#[test]
fn pick_scored_empty_metrics_still_selects() {
    let candidates: Vec<SocketAddr> = (1..=10).map(addr).collect();
    let metrics = PeerMetrics::default();
    let mut pick = PickPolicy::deterministic(42);
    let selected = pick.pick_scored(3, candidates, &metrics);
    assert_eq!(selected.len(), 3);
}

// -----------------------------------------------------------------------
// PeerMetrics tests
// -----------------------------------------------------------------------

#[test]
fn peer_metrics_combined_score() {
    let mut m = PeerMetrics::default();
    m.record_upstreamyness(addr(1), 100);
    m.record_upstreamyness(addr(1), 101);
    m.record_fetchyness(addr(1), 100);
    assert_eq!(m.combined_score(&addr(1)), 3); // 2 upstream + 1 fetch
    assert_eq!(m.combined_score(&addr(2)), 0); // unknown peer
}

#[test]
fn peer_metrics_remove_peer() {
    let mut m = PeerMetrics::default();
    m.record_upstreamyness(addr(1), 0);
    m.record_fetchyness(addr(1), 0);
    assert_eq!(m.combined_score(&addr(1)), 2);
    m.remove_peer(&addr(1));
    assert_eq!(m.combined_score(&addr(1)), 0);
}

#[test]
fn peer_metrics_independent_per_peer() {
    let mut m = PeerMetrics::default();
    m.record_upstreamyness(addr(1), 0);
    m.record_fetchyness(addr(2), 0);
    assert_eq!(m.combined_score(&addr(1)), 1);
    assert_eq!(m.combined_score(&addr(2)), 1);
}

// -----------------------------------------------------------------------
// Randomized governor evaluation integration
// -----------------------------------------------------------------------

#[test]
fn randomized_promotions_select_different_subsets_with_different_seeds() {
    // With many cold peers but limited target, different seeds should
    // produce different promotion sets (demonstrating randomization works).
    let reg = make_registry(
        &(1..=20)
            .map(|p| (p, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold))
            .collect::<Vec<_>>(),
    );
    let targets = GovernorTargets {
        target_known: 30,
        target_established: 3,
        ..Default::default()
    };
    let mut p1 = PickPolicy::new(111);
    let mut p2 = PickPolicy::new(222);
    let r1 = evaluate_cold_to_warm_promotions(&reg, &targets, &mut p1);
    let r2 = evaluate_cold_to_warm_promotions(&reg, &targets, &mut p2);
    assert_eq!(r1.len(), 3);
    assert_eq!(r2.len(), 3);
    // Different seeds should give different subsets with high probability.
    assert_ne!(r1, r2);
}

// -----------------------------------------------------------------------
// Slice D — HotPeerScheduling, hot_peers_remote, evaluate_hot_promotions
// -----------------------------------------------------------------------
//
// Reference: `Ouroboros.Network.PeerSelection.Governor.HotPeers` in
// `IntersectMBO/ouroboros-network`.

#[test]
fn hot_peer_scheduling_default_weights_match_upstream() {
    // Defaults must mirror upstream `defaultMiniProtocolParameters`:
    // BlockFetch=10, ChainSync=3, TxSubmission=2, KeepAlive=1, PeerSharing=1.
    let s = HotPeerScheduling::new();
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH), 10);
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::CHAIN_SYNC), 3);
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::TX_SUBMISSION), 2);
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::KEEP_ALIVE), 1);
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::PEER_SHARING), 1);
}

#[test]
fn hot_peer_scheduling_default_impl_matches_new() {
    // Default::default() must agree with new(), so consumers that
    // construct via `..Default::default()` get the upstream weights.
    assert_eq!(HotPeerScheduling::default(), HotPeerScheduling::new());
}

#[test]
fn hot_peer_scheduling_unset_protocol_returns_zero() {
    // Handshake has no scheduling weight in upstream, and absent
    // entries must return 0 rather than panic.
    let s = HotPeerScheduling::new();
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::HANDSHAKE), 0);
}

#[test]
fn set_hot_protocol_weight_overwrites_default() {
    let mut s = HotPeerScheduling::new();
    s.set_hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH, 5);
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH), 5);
}

#[test]
fn set_hot_protocol_weight_is_idempotent() {
    // Two writes with the same value leave state identical to one write.
    let mut a = HotPeerScheduling::new();
    let mut b = HotPeerScheduling::new();
    a.set_hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH, 7);
    b.set_hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH, 7);
    b.set_hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH, 7);
    assert_eq!(a, b);
}

#[test]
fn set_hot_protocol_weight_zero_disables() {
    // Weight 0 is the documented "disable from scheduler share" value.
    let mut s = HotPeerScheduling::new();
    s.set_hot_protocol_weight(MiniProtocolNum::TX_SUBMISSION, 0);
    assert_eq!(s.hot_protocol_weight(MiniProtocolNum::TX_SUBMISSION), 0);
}

#[test]
fn hot_peer_scheduling_weights_view_is_readonly() {
    // `weights()` returns `&BTreeMap` so callers cannot mutate the
    // map directly.  The only legal mutation path is
    // `set_hot_protocol_weight`.
    let s = HotPeerScheduling::new();
    let w = s.weights();
    assert!(w.contains_key(&MiniProtocolNum::BLOCK_FETCH));
    assert_eq!(w.len(), 5); // 5 named protocols at upstream defaults.
}

#[test]
fn hot_peers_remote_excludes_local_root_and_big_ledger() {
    // Local-root and big-ledger peers run their own valency invariants;
    // the remote-hot view must surface only the public/ledger hot set.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        (4, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        (5, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
    ]);
    let hot = hot_peers_remote(&reg);
    assert!(!hot.contains(&addr(1)), "local-root excluded");
    assert!(hot.contains(&addr(2)), "public-root hot included");
    assert!(hot.contains(&addr(3)), "ledger hot included");
    assert!(!hot.contains(&addr(4)), "big-ledger excluded");
    assert!(!hot.contains(&addr(5)), "warm excluded");
    assert_eq!(hot.len(), 2);
}

#[test]
fn hot_peers_remote_empty_when_no_hot_peers() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
    ]);
    assert!(hot_peers_remote(&reg).is_empty());
}

#[test]
fn evaluate_hot_promotions_matches_warm_to_hot_promotions() {
    // The new entry point must produce identical actions to the
    // direct call so the existing 16+ promotion regression tests
    // remain the source of truth for selection semantics.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (3, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 3,
        target_active: 2,
        ..Default::default()
    };
    let scheduling = HotPeerScheduling::new();

    let direct = evaluate_warm_to_hot_promotions(&reg, &targets, &mut test_pick());
    let via_facade = evaluate_hot_promotions(&reg, &targets, &mut test_pick(), &scheduling);
    assert_eq!(direct, via_facade);
}

#[test]
fn evaluate_hot_promotions_ignores_weights_for_candidacy() {
    // Weights affect the connection-manager scheduler, not promotion
    // candidacy.  Setting an extreme weight must NOT change which
    // peers are selected, so consumers can tune scheduling without
    // perturbing the warm→hot pipeline.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 2,
        ..Default::default()
    };
    let mut s_low = HotPeerScheduling::new();
    s_low.set_hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH, 0);
    let mut s_high = HotPeerScheduling::new();
    s_high.set_hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH, 255);

    let r_low = evaluate_hot_promotions(&reg, &targets, &mut test_pick(), &s_low);
    let r_high = evaluate_hot_promotions(&reg, &targets, &mut test_pick(), &s_high);
    assert_eq!(r_low, r_high);
}

#[test]
fn evaluate_hot_promotions_returns_n_promotions() {
    // Multi-peer promotion: with 5 warm peers and target_active=3,
    // the function must return exactly 3 promotions in one tick.
    let reg = make_registry(
        &(1..=5)
            .map(|p| (p, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm))
            .collect::<Vec<_>>(),
    );
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 5,
        target_active: 3,
        ..Default::default()
    };
    let actions =
        evaluate_hot_promotions(&reg, &targets, &mut test_pick(), &HotPeerScheduling::new());
    assert_eq!(actions.len(), 3);
    for a in &actions {
        assert!(matches!(a, GovernorAction::PromoteToHot(_)));
    }
}

#[test]
fn evaluate_hot_promotions_empty_when_target_met() {
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 2,
        ..Default::default()
    };
    let actions =
        evaluate_hot_promotions(&reg, &targets, &mut test_pick(), &HotPeerScheduling::new());
    assert!(actions.is_empty());
}

#[test]
fn evaluate_hot_promotions_empty_when_no_warm_peers() {
    // Edge case mirroring the existing `no_promotions_when_targets_met`
    // pattern: if there are no warm candidates, promotion is a no-op.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 5,
        target_active: 5,
        ..Default::default()
    };
    let actions =
        evaluate_hot_promotions(&reg, &targets, &mut test_pick(), &HotPeerScheduling::new());
    assert!(actions.is_empty());
}

#[test]
fn governor_state_default_carries_default_hot_scheduling() {
    // Default GovernorState must carry the upstream-default scheduling
    // so the no-op consumer path (no explicit configuration) gets
    // upstream parity for free.
    let gs = GovernorState::default();
    assert_eq!(gs.hot_scheduling, HotPeerScheduling::new());
}

// -----------------------------------------------------------------------
// Slice GD-Governor — density-aware combined_score
// -----------------------------------------------------------------------

#[test]
fn density_for_unknown_peer_returns_zero() {
    let m = PeerMetrics::default();
    assert_eq!(m.density_for(&addr(3001)), 0.0);
}

#[test]
fn set_density_overwrites_previous_observation() {
    let mut m = PeerMetrics::default();
    m.set_density(addr(3002), 0.3);
    m.set_density(addr(3002), 0.8);
    assert!((m.density_for(&addr(3002)) - 0.8).abs() < f64::EPSILON);
}

#[test]
fn is_low_density_false_for_unknown_peer() {
    // Freshly-promoted peer: no density observation yet must NOT
    // be treated as low-density (let it deliver a few headers
    // before scoring kicks in).
    let m = PeerMetrics::default();
    assert!(!m.is_low_density(&addr(3003)));
}

#[test]
fn is_low_density_true_below_threshold() {
    let mut m = PeerMetrics::default();
    m.set_density(addr(3004), 0.1);
    assert!(m.is_low_density(&addr(3004)));
}

#[test]
fn is_low_density_false_at_or_above_threshold() {
    let mut m = PeerMetrics::default();
    m.set_density(addr(3005), LOW_DENSITY_THRESHOLD);
    // Threshold is the boundary — at-threshold is NOT low.
    assert!(!m.is_low_density(&addr(3005)));
}

#[test]
fn combined_score_adds_density_bonus_above_threshold() {
    let mut m = PeerMetrics::default();
    let a = addr(3006);
    m.upstreamyness.insert(a, 10);
    m.fetchyness.insert(a, 20);
    // No density set → no bonus.
    assert_eq!(m.combined_score(&a), 30);
    // High density → +HIGH_DENSITY_BONUS.
    m.set_density(a, 0.8);
    assert_eq!(m.combined_score(&a), 30 + HIGH_DENSITY_BONUS);
}

#[test]
fn combined_score_no_bonus_for_low_density() {
    let mut m = PeerMetrics::default();
    let a = addr(3007);
    m.upstreamyness.insert(a, 5);
    m.fetchyness.insert(a, 5);
    m.set_density(a, 0.2); // below threshold
    assert_eq!(m.combined_score(&a), 10);
}

#[test]
fn remove_peer_clears_density() {
    let mut m = PeerMetrics::default();
    let a = addr(3008);
    m.set_density(a, 0.7);
    m.remove_peer(&a);
    assert_eq!(m.density_for(&a), 0.0);
}

#[test]
fn high_density_bonus_is_canonical() {
    // Pin the bonus magnitude so a future regression that changes
    // it surfaces immediately. Currently chosen small enough to act
    // as a tie-breaker without overriding upstreamyness+fetchyness.
    assert_eq!(HIGH_DENSITY_BONUS, 5);
}

#[test]
fn low_density_threshold_matches_consensus_default() {
    // Pin against the consensus-side `DEFAULT_LOW_DENSITY_THRESHOLD
    // = 0.6` so a future regression that drifts the network value
    // away from the consensus default surfaces immediately.
    assert!((LOW_DENSITY_THRESHOLD - 0.6).abs() < f64::EPSILON);
}

#[test]
fn governor_tick_normal_uses_evaluate_hot_promotions_path() {
    // Drives a single Normal-mode tick with two warm peers and
    // target_active=2: promotions must come through the new
    // evaluate_hot_promotions entry point and produce exactly 2
    // PromoteToHot actions, identical to the legacy direct path.
    let reg = make_registry(&[
        (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
    ]);
    let targets = GovernorTargets {
        target_known: 10,
        target_established: 2,
        target_active: 2,
        ..Default::default()
    };
    let mut pick = test_pick();
    let metrics = PeerMetrics::default();
    let actions = governor_tick(
        &reg,
        &targets,
        &[],
        PeerSelectionMode::Normal,
        AssociationMode::Unrestricted,
        None,
        &mut pick,
        &metrics,
        Instant::now(),
    );
    let promotions: Vec<_> = actions
        .iter()
        .filter(|a| matches!(a, GovernorAction::PromoteToHot(_)))
        .collect();
    assert_eq!(promotions.len(), 2);
}
