# Guidance for the pure-Rust port of upstream `kes-agent`.

**Status:** `partial` (R335 help/version skeleton + typed
`AgentMain.hs` parser/env surface + common protocol type vocabulary +
version-handshake + Control V0/V1/V2/V3 and Service V0/V1/V2
protocol/peer/trace surfaces + BearerUtil helper vocabulary + R443
structured deferral). Next milestone: **R444+** socket-protocol
fixture capture and daemon dispatch follow-on. Scope band: **MEDIUM**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 dev/test/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/kes-agent/kes-agent/` (68 `.hs` files).

## Mini-arc scope

**HIGHEST-STAKES** sister tool. KES key custody + period-rotation daemon.
Socket protocol byte-equivalence is mandatory or live SPO setups break.
The R444+ follow-on captures upstream socket traces as fixtures before
daemon/socket code lands, then wires server-side socket protocol,
`crates/crypto/src/kes.rs` + `crates/crypto/src/sum_kes.rs`, and live
rehearsal vs upstream.

## Current functional surface (post-R443)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::ProgramOptions` dispatch for
  `start|stop|restart|status|run`, `-F/--config/--config-file`, and
  the upstream `run` mode flags.
- ✅ Environment-derived normal/service option overlays matching
  upstream `nmoFromEnv`, `smoFromEnv`, and `splitBy ':'`.
- ✅ Common protocol vocabulary mirrors for upstream
  `Protocols/AgentInfo.hs`, `Protocols/RecvResult.hs`, and
  `Protocols/Types.hs`, including enum ordinal tests and selected
  pretty-rendering contracts.
- ✅ Version-handshake protocol mirrors for upstream
  `Protocols/VersionHandshake/{Protocol,Peers,Driver}.hs`, including
  the typed version identifier, peer negotiation choice, and driver
  trace pretty contracts.
- ✅ Control V0/V1/V2/V3 protocol mirrors for upstream
  `Protocols/Control/V0/{Protocol,Peers,Driver}.hs`,
  `Protocols/Control/V1/{Protocol,Peers,Driver}.hs`,
  `Protocols/Control/V2/{Protocol,Peers,Driver}.hs` and
  `Protocols/Control/V3/{Protocol,Peers,Driver}.hs`, including typed
  control messages, `Control:<crypto>:0.5`, `Control:1.0`,
  `Control:2.0`, and `Control:3.0`, pure control command peer
  choices, command discriminators, and read-error trace mapping.
- ✅ Service V0/V1/V2 protocol mirrors for upstream
  `Protocols/Service/V0/{Protocol,Peers,Driver}.hs`,
  `Protocols/Service/V1/{Protocol,Peers,Driver}.hs`, and
  `Protocols/Service/V2/{Protocol,Peers,Driver}.hs`, including typed
  service messages, V0 `Service:<crypto>:0.4`, V2 key/drop
  discriminants, pure receiver/pusher choices, and read-error trace
  mapping.
- ✅ BearerUtil helper mirror for upstream `Protocols/BearerUtil.hs`,
  including `BearerConnectionClosed`, `withDuplexBearer`, the
  one-byte-at-a-time receive buffering model, and the 1024-byte
  receiver buffer constant.
- ❌ Daemon dispatch — returns `RunError::DaemonDispatchDeferred`
  (R443 structured deferral). See **Carve-out inventory** below.
- ❌ End-to-end behavioral tests against upstream binary — pending
  the R444+ daemon/socket follow-on.

## Carve-out inventory (R443 structured deferral surface)

`crates/tools/kes-agent/src/status.rs` ships `daemon_status()`
returning a `DaemonStatus` descriptor.

| Carve-out                            | Status helper             | Deferral rationale (one-liner)                                            |
|--------------------------------------|---------------------------|---------------------------------------------------------------------------|
| Daemon dispatch (socket server + KES key lifecycle + start/stop/run/restart/status subcommands) | `status::daemon_status()` | HIGHEST-STAKES parity per the R326-R459 plan: socket protocol must be byte-equivalent or live SPO setups break. Depends on `crates/crypto/src/kes.rs` + `crates/crypto/src/sum_kes.rs` (shipped) + the R444+ byte-equivalent server-side socket protocol follow-on. |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-kes-agent

# Run via the universal launcher (recommended).
dev/scripts/run-tools.sh kes-agent --help
dev/scripts/run-tools.sh kes-agent --version

# Or invoke the binary directly:
target/release/kes-agent --help
```

The binary is named `kes-agent` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
after the R444+ daemon/socket follow-on closes.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `kes-agent` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `crates/node/config/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

Per the R326-R459 plan, this crate's full implementation lands across
the named mini-arc rounds:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- ✅ Typed `AgentMain.hs` parser/options surface shipped.
- ✅ `AgentMain.hs` env-option overlay surface shipped.
- ✅ Common protocol type vocabulary shipped for
  `Protocols/{AgentInfo,RecvResult,Types}.hs`.
- ✅ Version-handshake protocol/peer/trace surface shipped for
  `Protocols/VersionHandshake/{Protocol,Peers,Driver}.hs`.
- ✅ Control V0 protocol/peer/trace surface shipped for
  `Protocols/Control/V0/{Protocol,Peers,Driver}.hs`.
- ✅ Control V1 protocol/peer/trace surface shipped for
  `Protocols/Control/V1/{Protocol,Peers,Driver}.hs`.
- ✅ Control V2 protocol/peer/trace surface shipped for
  `Protocols/Control/V2/{Protocol,Peers,Driver}.hs`.
- ✅ Control V3 protocol/peer/trace surface shipped for
  `Protocols/Control/V3/{Protocol,Peers,Driver}.hs`.
- ✅ Service V0 protocol/peer/trace surface shipped for
  `Protocols/Service/V0/{Protocol,Peers,Driver}.hs`.
- ✅ Service V1 protocol/peer/trace surface shipped for
  `Protocols/Service/V1/{Protocol,Peers,Driver}.hs`.
- ✅ Service V2 protocol/peer/trace surface shipped for
  `Protocols/Service/V2/{Protocol,Peers,Driver}.hs`.
- ✅ BearerUtil helper vocabulary shipped for `Protocols/BearerUtil.hs`.
- ✅ Structured daemon/socket deferral surfaced at R443.
- 🟡 Next: **R444+** — upstream socket-protocol fixture capture and
  daemon dispatch follow-on.
- 🟡 Closeout — when all subcommands are functional, parity-matrix
  entry advances `partial → verified_11_0_1`. Operators can then
  swap upstream binary for the yggdrasil binary without script
  changes.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash dev/reference/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-kes-agent

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/kes-agent --help) \
     <(target/debug/kes-agent --help)
diff <(.reference-haskell-cardano-node/install/bin/kes-agent --version) \
     <(target/debug/kes-agent --version)
# (empty diffs expected — byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete subcommand implementations
  land (replace `❌ not yet implemented` rows with `✅ shipped` +
  round number).
- Keep the per-tool migration round numbers in sync with
  `docs/COMPLETION_ROADMAP.md` and `CHANGELOG.md`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
