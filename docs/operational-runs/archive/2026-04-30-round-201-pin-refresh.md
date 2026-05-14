## Round 201 — Audit baseline pin refresh (Phase E.1, 4 of 5 drifted pins)

Date: 2026-04-30
Branch: main

### Goal

Phase E.1 — advance the documentary upstream pins in
`node/src/upstream_pins.rs` from the 2026-Q2 audit baseline to
the live HEAD reported by `node/scripts/check_upstream_drift.sh`.

The 6 pins are documentary (yggdrasil is a pure-Rust port with
no Cargo `git =` deps).  Each records the upstream commit at
which the corresponding repo was last audited against.  Drift
is informational — the build doesn't track upstream `main`.

### Scope

Advance 4 of 5 drifted pins.  `cardano-base` is intentionally
deferred because its SHA is mirrored by the vendored test-vector
directory name (`specs/upstream-test-vectors/cardano-base/<sha>/`)
which `crates/crypto/tests/upstream_vectors.rs::CARDANO_BASE_SHA`
consumes; advancing requires a coordinated refresh of the
vendored fixtures and re-running the full corpus drift-guard
tests — that's a separate audit slice.

### Code change

`node/src/upstream_pins.rs`:

| Pin | From | To |
|-----|------|-----|
| `UPSTREAM_CARDANO_LEDGER_COMMIT` | `9ae77d611ad8…` | `42d088ed84b799d6d980f9be6f14ad953a3c957d` |
| `UPSTREAM_OUROBOROS_CONSENSUS_COMMIT` | `91c8e1bb5d7f…` | `c368c2529f2f41196461883013f749b7ac7aa58e` |
| `UPSTREAM_PLUTUS_COMMIT` | `187c3971a34e…` | `e3eb4c76ea20cf4f90231a25bdfaab998346b406` |
| `UPSTREAM_CARDANO_NODE_COMMIT` | `60af1c23bc20…` | `799325937a4598899c8cab61f4c957662a0aeb53` |

Each constant gains an "R201 audit baseline (2026-04-30) —
advanced from … to live HEAD" rustdoc note.

`docs/UPSTREAM_PARITY.md`:

- Pinned-commits table updated with new SHAs + audit-baseline
  date `2026-04-30, R201 advance` notes for the 4 advanced
  pins.
- Drift snapshot section retitled to "2026-04-30 (post-R201
  advance)".  5 of 6 pins now show `**in-sync**`; only
  `cardano-base` remains drifted with an explanation.

### Verification

Drift detector (`bash node/scripts/check_upstream_drift.sh`):

```
[upstream-drift] timestamp=2026-04-30T10:26:51Z
  repo                    status   pinned -> live
  cardano-base            DRIFT    db52f43b38ba -> 9965336f769d
  cardano-ledger          in-sync  42d088ed84b7
  ouroboros-consensus     in-sync  c368c2529f2f
  ouroboros-network       in-sync  0e84bced45c7
  plutus                  in-sync  e3eb4c76ea20
  cardano-node            in-sync  799325937a45

[summary] drifted=1 unreachable=0 total=6
```

Drift-guard tests
(`cargo test -p yggdrasil-node --lib upstream_pins::tests`):

```
running 3 tests
test upstream_pins::tests::upstream_cardano_base_pin_matches_vendored_directory_name ... ok
test upstream_pins::tests::upstream_pins_are_40_lowercase_hex ... ok
test upstream_pins::tests::upstream_pins_cover_all_six_canonical_repos ... ok
```

All 3 invariants pass: format (40-char lowercase hex),
cardinality (6 canonical repos), and `cardano-base` ↔ vendored
directory name match.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Operational impact

R201 is purely documentary — no code surface change, no
behavioral change.  The pin advance establishes a fresh audit
baseline so future regressions against the new upstream HEAD
can be tracked from a known-good reference point.

### Open follow-ups

1. **`cardano-base` pin** — coordinated refresh of vendored
   test-vector directory + re-run full corpus drift-guard
   tests against new SHA `9965336f769d…`.  Tracked as a
   separate slice.
2. **Phase E.2** — mainnet rehearsal (24 h+ continuous sync
   from genesis) once data-plumbing arc is fully complete.
3. **Phase E.3** — parity proof report (cumulative test matrix
   + JSON byte comparison vs upstream node).
4. Phase A.6 — `GetGenesisConfig` ShelleyGenesis serialiser.
5. Phase A.7 — active stake distribution amounts.
6. Phase A.3 OMap proposals — gov-state proposal entries.
7. Phase C.2 — pipelined fetch+apply.
8. Phase D.1 — deep cross-epoch rollback recovery.
9. Phase D.2 — multi-session peer accounting.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code: [`node/src/upstream_pins.rs`](node/src/upstream_pins.rs).
- Drift detector:
  [`node/scripts/check_upstream_drift.sh`](node/scripts/check_upstream_drift.sh).
- Documentation:
  [`docs/UPSTREAM_PARITY.md`](docs/UPSTREAM_PARITY.md).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-199-200-multipeer-verified-and-apply-histogram.md`](2026-04-30-round-199-200-multipeer-verified-and-apply-histogram.md).
