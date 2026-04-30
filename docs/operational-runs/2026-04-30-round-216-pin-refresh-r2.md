## Round 216 — Phase E.1 pin refresh (round 2)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: E.1 (cardano-base coordinated fixture refresh remains
deferred separately)

### Goal

Advance `ouroboros-consensus` and `plutus` documentary pins to
their current live HEAD per `node/scripts/check_upstream_drift.sh`.
R201 (~16 rounds ago) refreshed these pins; both have drifted
again as upstream continues to merge changes.  This round is the
documentary-pin-only refresh; the `cardano-base` advance remains
deferred because it requires a coordinated vendored test-vector
fixture refresh.

### Pre-state (drift report)

```
[upstream-drift] timestamp=2026-04-30T21:17:22Z
  repo                    status   pinned -> live
  cardano-base            DRIFT    db52f43b38ba -> 7a8a991945d4
  cardano-ledger          in-sync  42d088ed84b7
  ouroboros-consensus     DRIFT    c368c2529f2f -> b047aca4a731
  ouroboros-network       in-sync  0e84bced45c7
  plutus                  DRIFT    e3eb4c76ea20 -> 4cd40a14e364
  cardano-node            in-sync  799325937a45

[summary] drifted=3 unreachable=0 total=6
```

### Pin advances

`node/src/upstream_pins.rs`:

- `UPSTREAM_OUROBOROS_CONSENSUS_COMMIT`:
  `c368c2529f2f41196461883013f749b7ac7aa58e` →
  `b047aca4a731d3282b1dab012d3669e9395328cc`
- `UPSTREAM_PLUTUS_COMMIT`:
  `e3eb4c76ea20cf4f90231a25bdfaab998346b406` →
  `4cd40a14e36431019414fad519c1a6d426a55509`

Each constant's doc comment now records both the R201 advance and
the R216 advance with its rationale.  No upstream-only changes
during the drift window affect the ported subset; the cumulative
multi-network operational evidence (R215) confirms the existing
port still passes against the new audit baseline.

### Post-state (drift report)

```
[upstream-drift] timestamp=2026-04-30T21:18:57Z
  repo                    status   pinned -> live
  cardano-base            DRIFT    db52f43b38ba -> 7a8a991945d4
  cardano-ledger          in-sync  42d088ed84b7
  ouroboros-consensus     in-sync  b047aca4a731
  ouroboros-network       in-sync  0e84bced45c7
  plutus                  in-sync  4cd40a14e364
  cardano-node            in-sync  799325937a45

[summary] drifted=1 unreachable=0 total=6
```

5 of 6 pins now in-sync.  Only `cardano-base` remains drifted —
intentionally, pending a coordinated vendored-fixture refresh
(`specs/upstream-test-vectors/cardano-base/<sha>/` directory
rename + `crates/crypto/tests/upstream_vectors.rs::CARDANO_BASE_SHA`
constant update + corpus re-run).

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 745 passed / 0 failed / 1 ignored
node/scripts/check_upstream_drift.sh   # 5 in-sync, 1 drift = cardano-base
```

`upstream_pins::tests::upstream_pins_are_40_lowercase_hex`,
`upstream_pins_cover_all_six_canonical_repos`, and
`upstream_cardano_base_pin_matches_vendored_directory_name` all
pass against the new SHAs.

### Companion doc updates

- `docs/UPSTREAM_PARITY.md` — pinning table updated with new SHAs
  and "R216 advance" annotations; drift snapshot section refreshed
  with new live-HEAD comparison.

### Strategic significance

R216 closes the Phase E.1 refresh slice for the two documentary
pins that had drifted since R201.  The cumulative pin-advance
cadence (R201 → R216, 15 rounds apart) demonstrates the
audit-baseline is being actively maintained against upstream.

### Open follow-ups

Phase E.1 cardano-base — deferred to a future coordinated audit
slice.  This requires:

1. Fetching upstream test vectors at the new SHA
   (`7a8a991945d4...`).
2. `git mv specs/upstream-test-vectors/cardano-base/db52f43b38ba…
   /7a8a991945d4…/`.
3. Updating `CARDANO_BASE_SHA` in
   `crates/crypto/tests/upstream_vectors.rs`.
4. Updating `UPSTREAM_CARDANO_BASE_COMMIT` in
   `node/src/upstream_pins.rs`.
5. Re-running the full crypto-vectors corpus to confirm no fixture
   drift.

Plus the unchanged Phase C.2 (pipelined fetch+apply), Phase D.1
(deep cross-epoch rollback), Phase D.2 (multi-session peer
accounting), Phase E.2 (24h+ mainnet rehearsal).

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §5
  (upstream alignment row).
- Pinning detail: [`docs/UPSTREAM_PARITY.md`](../UPSTREAM_PARITY.md)
  (pinned-commits section + drift snapshot).
- Previous round: [R215](2026-04-30-round-215-multinetwork-post-r214-regression.md).
- Drift detector: [`node/scripts/check_upstream_drift.sh`](../../node/scripts/check_upstream_drift.sh).
- Upstream references:
  - <https://github.com/IntersectMBO/ouroboros-consensus/commit/b047aca4a731d3282b1dab012d3669e9395328cc>
  - <https://github.com/IntersectMBO/plutus/commit/4cd40a14e36431019414fad519c1a6d426a55509>
