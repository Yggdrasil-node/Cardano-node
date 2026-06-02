# Guidance for the pure-Rust port of upstream `bech32`.

**Status (R334 closeout):** **deployment-ready**. Drop-in
byte-equivalent to upstream `IntersectMBO/bech32 1.1.10` for every
documented CLI surface. Parity-matrix entry status:
`verified_11_0_1`.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate: `python3 dev/test/check-strict-mirror.py`.

| Rust path | Upstream `.hs` |
|---|---|
| `src/lib.rs` | `Codec/Binary/Bech32.hs` (public API) |
| `src/internal.rs` | `Codec/Binary/Bech32/Internal.hs` (CHARSET + EncodingSpec) |
| `src/main.rs` | `bech32/app/Main.hs` (binary entry) |
| `src/parser.rs` | none — Yggdrasil-side CLI parser shell with byte-equivalent `--help`/`--version` fixtures |

Upstream's `bech32-th/src/Codec/Binary/Bech32/TH.hs` (Template
Haskell helpers) has no Rust analog — Rust uses `macro_rules!` /
proc-macros directly. No `crates/tools/bech32/src/th.rs` exists; the
strict-mirror policy supports this absence per the `Setup.hs` /
`Orphans.hs` precedents.

## Upstream source

`.reference-haskell-cardano-node/deps/bech32/bech32/`

## Build + run

```bash
# Production deploy: build all sister-tool binaries.
cargo build --release --workspace

# Run via the universal launcher (R329):
dev/scripts/run-tools.sh bech32 --help        # byte-equivalent to upstream
dev/scripts/run-tools.sh bech32 --version     # byte-equivalent to upstream
echo "706174617465" | dev/scripts/run-tools.sh bech32 base16_
# → base16_1wpshgct5v5r5mxh0

# Or invoke the binary directly:
target/release/bech32 --help
echo "base16_1wpshgct5v5r5mxh0" | target/release/bech32  # decode → 706174617465
```

The binary is at `target/release/bech32` (NOT `target/release/yggdrasil-bech32`)
so the upstream-compatible name is what operators see.

## Functional surface (R333 — fully shipped)

The CLI surface mirrors upstream exactly:

```text
Usage: bech32 [PREFIX]

  Convert to and from bech32 strings. Data are read from standard input.

Available options:
  -h,--help                Show this help text
  PREFIX                   An optional human-readable prefix (e.g. 'addr').
                             - When provided, the input text is decoded from
                               various encoding formats and re-encoded to
                               bech32 using the given prefix.
                             - When omitted, the input text is decoded from
                               bech32 to base16.
  -v,--version             output version information and exit

Supported encoding formats: Base16, Bech32 & Base58.
```

Encoding detection (`detect_encoding`) order matches upstream:
Base16 (all-hex + even length) → Bech32 (separator '1' + valid HRP/data
chars + consistent letter case) → Base58 (Bitcoin alphabet).

## Pure-Rust crate dependencies

| Crate | Version | Role | License |
|---|---|---|---|
| `bech32` | 0.11 | BIP-0173 / BIP-0350 codec | MIT |
| `bs58` | 0.5 | Bitcoin Base58 alphabet | MIT/Apache-2.0 |
| `hex` | 0.4 | Base16 codec | MIT/Apache-2.0 |
| `clap` | 4.6 | (workspace) — currently unused; manual parsing in `parser.rs` is simpler |
| `eyre` | 0.6 | Binary error reporting |
| `thiserror` | 2.0 | Library error derive |

All pure Rust; no FFI; no native build requirements.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `bech32` is the
  acceptance gate. Operators must be able to swap the upstream
  binary for the yggdrasil binary without a script change. R333
  closure verifies this for every documented example.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new bech32 release with different help output, refresh
  the fixtures + bump `UPSTREAM_BECH32_COMMIT` in
  `crates/node/config/src/upstream_pins.rs` as a coordinated round.

## Round roadmap (Phase A.1 — bech32, COMPLETE)

| Round | Status | Deliverable |
|---|---|---|
| R331 | ✅ shipped | File-mirror skeleton (lib.rs / internal.rs / main.rs) |
| R332 | ✅ shipped | CLI parser + byte-equivalent --help/--version |
| R333 | ✅ shipped | Concrete encode/decode + drop-in deployment proof |
| R334 | ✅ closeout (this round) | CHANGELOG, AGENTS.md, parity-matrix → verified_11_0_1 |

Phase A.1 is **closed**. Tier 1 next: Phase A.2 — cardano-submit-api
(R335-R343, 9 rounds, MEDIUM).

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only needed when bumping bech32 version).
bash dev/reference/setup-reference.sh

# 2. Run cargo test for the bech32 crate.
cargo test -p yggdrasil-bech32

# 3. Spot-check the documented examples.
for input in 706174617465 Ae2tdPwUPEYy old_prefix1wpshgcg2s33x3; do
  echo -n "$input" | .reference-haskell-cardano-node/install/bin/bech32 base16_ > /tmp/up
  echo -n "$input" | target/debug/bech32 base16_ > /tmp/yg
  diff /tmp/up /tmp/yg && echo "MATCH: $input" || echo "DRIFT: $input"
done

# 4. Compare --help output.
diff <(.reference-haskell-cardano-node/install/bin/bech32 --help) \
     <(target/debug/bech32 --help)
# (empty diff expected — byte-equivalent)
```

## Maintenance Guidance

- If upstream bumps the `bech32` package version: refresh the
  vendored source via `bash dev/reference/setup-reference.sh`, re-capture
  the help/version fixtures into `tests/fixtures/`, advance
  `UPSTREAM_BECH32_COMMIT` in `crates/node/config/src/upstream_pins.rs`, and run
  the full cargo gate.
- If a new subcommand or flag is added upstream: extend
  `parser::Args` + `parser::parse_args` to handle it; capture the
  new `--help` output into the fixture; add a golden test in
  `tests/cli_help_golden.rs`.
- If encoding-detection logic drifts (e.g. upstream adds a new
  encoding): mirror the change in `detect_encoding()` + add a
  `detect_*` unit test pinning the new variant.
