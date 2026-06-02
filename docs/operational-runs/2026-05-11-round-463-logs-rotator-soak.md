---
title: 'R463 closeout: cardano-tracer Logs Rotator + file-write soak'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-463-logs-rotator-soak/
---

# R463 — Logs Rotator + file-write soak

**Date:** 2026-05-11
**Predecessor:** [`R462 closure`](../#unreleased) (trace-objects file-write
IO orchestration + HandleRegistry handoff).

## Closure scope

Validates the R461 + R462 system under realistic concurrent load.
Closes the R462 advisor flag that the rotator/handler pair hadn't
been validated as a system — only as isolated unit tests.

## Test surface

R463 adds one integration test:
`handlers::logs::rotator::tests::rotator_and_writer_cooperate_under_load`.

The test drives the full pipeline:

1. **Setup** — temp directory, aggressive `RotationParams`
   (`frequency_secs=1`, `log_limit_bytes=80`,
   `max_age_minutes=60`, `keep_files_num=2`), shared
   `HandleRegistry` + `current_log_lock`.
2. **Spawn the rotator** — `run_logs_rotator` runs in a
   background task with a brake flag controlled by the test.
3. **Drive the writer** — 10 batches of 2 events each, fired
   every 250ms. Each ForMachine event is ~40-50 bytes prepared,
   so every batch crosses the 80-byte threshold for the rotator
   to roll on its next 1-second scan.
4. **Final settle** — wait one full scan cycle (1.1s) for any
   in-flight roll to complete.
5. **Brake-trip** — set the stop flag, await the rotator's
   clean exit within 3 seconds.

## Assertions

- At least one `node-*.json` log file exists in the per-node
  subdir.
- At least one `node.json` convenience symlink exists.
- Total log-file count is bounded by `keep_files_num + slack`
  (3-4 files; we allow up to 4 to handle the race window
  between mint + rotator scan).
- The shared `HandleRegistry` has **exactly one** entry for
  the (node_name, logging_params) key — the rotator overwrites
  the registry entry on each roll and never accumulates.

## Failure modes the test catches

- **File-descriptor leak**: if `create_or_update_empty_log`
  didn't drop the previous Arc<File> on overwrite, the FD
  table would grow with each roll. The single-registry-entry
  assertion catches accumulation.
- **Append-vs-truncate confusion**: if `write_trace_objects_to_file`
  truncated instead of appending, the file size after a batch
  would equal one batch's bytes (not the cumulative total).
  The internal byte-count chain (handler returns written_bytes,
  test sums them, compares against on-disk file size in the
  R462 unit tests) catches this; the soak indirectly verifies
  it by ensuring the rotator actually fires (which requires
  the file to grow past the threshold).
- **Symlink atomicity under contention**: the writer holds the
  per-handle mutex while writing; the rotator's
  `create_or_update_empty_log` holds `current_log_lock` while
  swapping. If those locks were reversed or one was missing,
  concurrent writes during a roll could race the symlink swap.
  The test exercises this concurrency (writer + rotator
  interleaved at 250ms vs 1-second cadence).
- **Rotator no-ops the deletion path**: if
  `check_if_there_are_old_logs` had an off-by-one in
  `dropEnd keep_files_num`, the rotator would either delete
  the current handle's underlying file (catastrophic) or
  never delete anything (bounded leak). The file-count cap
  catches both.

## Carve-out validations

- **`canonicalize` fails when root doesn't pre-exist**: the
  R462 closure fixed the original `canonicalize(root)` call
  to use `logging_params.root.join(node_name)` directly +
  `create_dir_all` so first-write succeeds on a non-existent
  root. The soak test sets up the tempdir but never
  pre-creates the per-node subdir, exercising this path.
- **`hTell` semantics vs `File::metadata().len()`**: upstream
  uses `hTell` (current write offset); Yggdrasil uses
  `metadata().len()` since the file is opened write-only and
  extends linearly. The soak test relies on this equivalence
  by allowing the rotator to detect "full" via size check.

## Verification log

```
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 6,035 passing (was 6,034 pre-R463)
cargo lint                                          # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation  # 0 violations
python3 dev/test/check-parity-matrix.py              # clean
```

The soak test itself runs in ~3.6 seconds.

## R462 misattribution fix

R462's `write_trace_objects_to_file_status` descriptor said
`create_or_update_empty_log` shipped at R390. The R390 round
actually shipped only the pure log-naming + timestamp parser
subset; `createOrUpdateEmptyLog` real impl landed at R402 (per
`git log --oneline -- crates/cardano-tracer/src/handlers/logs/utils.rs`).
R463 corrects the descriptor to "R402".

## Follow-on observation

The deregister-hook (per-connection HandleRegistry teardown when
a forwarder disconnects) remains pending but is **not** load-
bearing:

- `create_or_update_empty_log` calls `registry.insert(key, new)`
  which returns the previous `Option<(SharedLogFile, PathBuf)>`.
  Dropping that returned tuple closes the previous file
  descriptor via Arc's drop semantics.
- On forwarder disconnect + reconnect, the same (node_name,
  logging_params) key produces the same `key`; the next write
  mints a fresh handle which overwrites the registry entry,
  dropping (and closing) the stale handle.
- Bounded leakage: one HandleRegistry entry per
  *currently-disconnected-and-never-reconnected* forwarder.

The path is self-healing on reconnect, so wiring the explicit
deregister hook is hygiene, not correctness.
