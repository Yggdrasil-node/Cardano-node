#!/usr/bin/env python3
"""Fail on stale pre-reorganization placements and status claims.

This is intentionally narrower than a full documentation rewrite. Historical
run records and tagged changelog entries may mention old paths as evidence of
when a file landed, but current code, generated navigation, CI, commands,
living docs, and the unreleased changelog must point at the post-reorganization
layout:

  - node binary crate: crates/node/cardano-node/
  - node diagnostics/tools: crates/tools/<tool>/
  - operator configuration: configuration/
  - operator scripts: scripts/

It also catches old root-node or yggdrasil-node shorthand metadata/test paths
such as node/AGENTS.md, yggdrasil-node/Cargo.toml, and node/tests/... when
they appear in current-facing files. Operator artifacts are rejected under any
`crates/node/*` package so they cannot drift from one stale node-local home to
another. A small set of stale current-status claims is guarded here too:
obsolete node-local LSQ wording, old cardano-cli counts and gate wording, old
three-command node-wrapper cardano-cli subset wording, stale active-migration
wording for the closed cardano-cli C-arc, obsolete parity-summary baselines,
obsolete proof/upstream verification baselines, old README/docs-site test
baselines, stale cardano-submit-api structured-decoder/R345-R346 evidence
wording, stale kes-agent/kes-agent-control early-mini-arc status wording,
stale root-manifest sister-tool labels, stale dmq-node pre-R816 current-status
wording, stale cardano-testnet pre-R823 current-status wording, the closed
cardano-testnet Command-payload, Process/Cli/Keys, Transaction
sign/submit/txid, DRep pure-builder, SPO pure-builder, and Process/Run
flexible-wrapper, RunIO plan-json, RunIO execution, Property/Util, and
Property/Assert pure-helper/CLI-wrapper, and Property/Run pure-helper gaps,
the closed bech32 pre-verified current-status wording, closed cardano-submit-api
accepted-response OK wording, and the closed workspace-member gap.

The vendored Haskell reference snapshot is allowed to contain source files and
reference artifacts, but it must not retain nested Git metadata. It is a
reference corpus in this workspace, not a nested checkout or submodule.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import uuid
from contextlib import contextmanager
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REFERENCE_TREE = ".reference-haskell-cardano-node"
SELF_TEST_TMP_ROOT = ROOT / "target" / "check-stale-placement-self-test"

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

EXCLUDED_PREFIXES = (
    ".git/",
    ".reference-haskell-cardano-node/",
    "target/",
    "docs/archive/",
    "docs/operational-runs/archive/",
)
EXCLUDED_PATHS = {
    "scripts/check-stale-placement.py",
}

# R505+ is the cleanup window where operational-run notes are current-facing
# evidence for the post-reorganization layout. Older run records remain
# historical evidence and may legitimately mention retired paths.
CURRENT_OPERATIONAL_RUN_CUTOFF = date(2026, 5, 18)

STALE_DIRECTORIES = {
    "old node crate directory": "crates/node/yggdrasil-node",
    "old root node directory": "node",
    "old yggdrasil-node shorthand directory": "yggdrasil-node",
    "configuration nested under cardano-node crate directory": (
        "crates/node/cardano-node/configuration"
    ),
    "scripts nested under cardano-node crate directory": "crates/node/cardano-node/scripts",
    "developer diagnostics nested under cardano-node crate directory": (
        "crates/node/cardano-node/src/bin"
    ),
    "old top-level bech32 crate directory": "crates/bech32",
    "old top-level cardano-cli crate directory": "crates/cardano-cli",
    "old top-level cardano-submit-api crate directory": "crates/cardano-submit-api",
    "old top-level cardano-testnet crate directory": "crates/cardano-testnet",
    "old top-level cardano-tracer crate directory": "crates/cardano-tracer",
    "old top-level db-analyser crate directory": "crates/db-analyser",
    "old top-level db-synthesizer crate directory": "crates/db-synthesizer",
    "old top-level db-truncater crate directory": "crates/db-truncater",
    "old top-level dmq-node crate directory": "crates/dmq-node",
    "old top-level kes-agent crate directory": "crates/kes-agent",
    "old top-level kes-agent-control crate directory": "crates/kes-agent-control",
    "old top-level snapshot-converter crate directory": "crates/snapshot-converter",
    "old top-level tx-generator crate directory": "crates/tx-generator",
    "old Claude skill directory": ".claude/skills/cardano-node",
}

NETWORK_PRESETS = ("mainnet", "preprod", "preview")
REQUIRED_OPERATOR_CONFIG_FILES = (
    "alonzo-genesis.json",
    "byron-genesis.json",
    "config-legacy.json",
    "config.json",
    "conway-genesis.json",
    "peer-snapshot.json",
    "shelley-genesis.json",
    "submit-api-config.json",
    "topology.json",
    "tracer-config.json",
)
REQUIRED_NETWORK_SPECIFIC_CONFIG_FILES = {
    "mainnet": ("checkpoints.json",),
    "preview": ("checkpoints.json",),
}

BASE_REQUIRED_CURRENT_PLACEMENTS = {
    "node binary crate manifest": "crates/node/cardano-node/Cargo.toml",
    "node binary crate entrypoint": "crates/node/cardano-node/src/main.rs",
    "operator metadata sample": "configuration/poolMetaData.json",
    "root sister-tool launcher": "scripts/run-tools.sh",
    "root reference setup helper": "scripts/setup-reference.sh",
    "root Haskell install helper": "scripts/install_haskell_cardano_node.sh",
    "root Haskell tip comparison harness": "scripts/compare_tip_to_haskell.sh",
    "root parallel blockfetch soak harness": "scripts/parallel_blockfetch_soak.sh",
    "root preview producer harness": "scripts/preview_producer_harness.sh",
    "root systemd unit template": "scripts/yggdrasil-node.service",
    "db-analyser forensic dump helper": "crates/tools/db-analyser/src/bin/dump_block.rs",
    "active Cardano Haskell reference skill": (
        ".claude/skills/cardano-haskell-node/SKILL.md"
    ),
}

REQUIRED_CURRENT_PLACEMENTS = {
    **BASE_REQUIRED_CURRENT_PLACEMENTS,
    **{
        f"{network} operator preset {filename}": (
            f"configuration/{network}/{filename}"
        )
        for network in NETWORK_PRESETS
        for filename in REQUIRED_OPERATOR_CONFIG_FILES
    },
    **{
        f"{network} operator preset {filename}": (
            f"configuration/{network}/{filename}"
        )
        for network, filenames in REQUIRED_NETWORK_SPECIFIC_CONFIG_FILES.items()
        for filename in filenames
    },
}

REQUIRED_TEXT_SNIPPETS = (
    (
        ".github/workflows/release.yml",
        "release workflow stages root configuration bundle",
        'cp -r configuration "$dir/configuration"',
    ),
    (
        ".github/workflows/release.yml",
        "release workflow stages root scripts bundle",
        'cp -r scripts "$dir/scripts"',
    ),
    (
        ".github/workflows/repro-check.yml",
        "repro workflow stages root configuration bundle",
        'cp -r configuration "$dir/configuration"',
    ),
    (
        ".github/workflows/repro-check.yml",
        "repro workflow stages root scripts bundle",
        'cp -r scripts "$dir/scripts"',
    ),
    (
        "Dockerfile",
        "Docker image copies root configuration bundle",
        "COPY --from=builder /src/configuration /usr/share/yggdrasil/configuration",
    ),
    (
        "Dockerfile",
        "Docker image pins preset resolution to copied configuration bundle",
        "YGGDRASIL_CONFIG_ROOT=/usr/share/yggdrasil/configuration",
    ),
    (
        "Dockerfile",
        "Docker image copies root upstream-drift helper",
        "COPY --from=builder /src/scripts/check_upstream_drift.sh",
    ),
    (
        "Dockerfile",
        "Docker image copies root restart-resilience helper",
        "COPY --from=builder /src/scripts/restart_resilience.sh",
    ),
    (
        "scripts/install_from_release.sh",
        "release installer requires bundled configuration",
        '[ -d "${extracted_dir}/configuration" ] || err "extracted archive missing configuration/ bundle"',
    ),
    (
        "scripts/install_from_release.sh",
        "release installer requires bundled scripts",
        '[ -d "${extracted_dir}/scripts" ] || err "extracted archive missing scripts/ bundle"',
    ),
    (
        "scripts/install_from_release.sh",
        "release installer installs root configuration bundle",
        'run_install cp -R "${extracted_dir}/configuration" "${share_target}/"',
    ),
    (
        "scripts/install_from_release.sh",
        "release installer installs root scripts bundle",
        'run_install cp -R "${extracted_dir}/scripts" "${share_target}/"',
    ),
    (
        "crates/node/cardano-node/src/commands/configuration.rs",
        "network preset resolver accepts installed config root override",
        'const CONFIG_ROOT_ENV_VAR: &str = "YGGDRASIL_CONFIG_ROOT";',
    ),
    (
        "crates/node/cardano-node/src/commands/configuration.rs",
        "network preset resolver probes installed share config root",
        "fn yggdrasil_share_config_root(prefix: &Path) -> PathBuf",
    ),
    (
        "crates/node/cardano-node/src/commands/configuration.rs",
        "network preset resolver retains source checkout fallback",
        "fn source_config_root() -> PathBuf",
    ),
    (
        "crates/node/cardano-node/src/commands/submit_tx.rs",
        "node submit-tx delegates to shared cardano-cli LocalTxSubmission client",
        "TokioLsqClient.submit_tx(&socket_path, network_magic, &tx_bytes)",
    ),
    (
        "crates/node/cardano-node/src/commands/query.rs",
        "node current-epoch query delegates to shared cardano-cli LSQ plan",
        "QueryCommand::CurrentEpoch => NtcQuery::CurrentEpoch",
    ),
    (
        "crates/node/cardano-node/src/commands/query.rs",
        "node era-history query delegates to shared cardano-cli LSQ plan",
        "QueryCommand::EraHistory => NtcQuery::EraHistory",
    ),
    (
        "crates/node/cardano-node/src/commands/query.rs",
        "node utxo-by-address query delegates to shared cardano-cli LSQ plan",
        "QueryCommand::UtxoByAddress { address } => NtcQuery::UtxoByAddress",
    ),
    (
        "crates/node/cardano-node/src/commands/query.rs",
        "node reward-balance query delegates to shared cardano-cli LSQ plan",
        "QueryCommand::RewardBalance { account } => NtcQuery::RewardBalance",
    ),
    (
        "crates/node/cardano-node/src/commands/query.rs",
        "node utxo-by-tx-in query delegates to shared cardano-cli LSQ plan",
        "QueryCommand::UtxoByTxIn { tx_id, index } => NtcQuery::UtxoByTxIn",
    ),
    (
        "crates/node/cardano-node/src/commands/query.rs",
        "node delegations-and-rewards query delegates to shared cardano-cli LSQ plan",
        "QueryCommand::DelegationsAndRewards",
    ),
    (
        "crates/node/cardano-node/src/commands/query.rs",
        "node stake-pool-params query delegates to shared cardano-cli LSQ plan",
        "QueryCommand::StakePoolParams { pool_hash } => NtcQuery::StakePoolParams",
    ),
    (
        "crates/tools/cardano-cli/AGENTS.md",
        "cardano-cli docs record parameterized LSQ ownership in shared crate",
        "R529 moved the remaining parameterized query envelopes into `lsq.rs`",
    ),
    (
        "crates/tools/cardano-cli/AGENTS.md",
        "cardano-cli docs include migrated transaction-build surface",
        "| `transaction-build` (offline)               | `transaction build` (offline subset)       |",
    ),
    (
        "crates/tools/cardano-cli/src/lib.rs",
        "cardano-cli crate docs describe node wrapper as thin adapter",
        "should remain a thin parser adapter",
    ),
    (
        "scripts/yggdrasil-node.service",
        "systemd unit pins preset resolution to installed configuration bundle",
        "Environment=YGGDRASIL_CONFIG_ROOT=/usr/local/share/yggdrasil/configuration",
    ),
)

NODE_BINARY_PACKAGE = "yggdrasil-node"
NODE_BINARY_MANIFEST = "crates/node/cardano-node/Cargo.toml"
NODE_SUPPORT_PACKAGE_PREFIX = "yggdrasil-node-"
SISTER_TOOL_PACKAGES = {
    "yggdrasil-bech32",
    "yggdrasil-cardano-cli",
    "yggdrasil-cardano-submit-api",
    "yggdrasil-cardano-testnet",
    "yggdrasil-cardano-tracer",
    "yggdrasil-db-analyser",
    "yggdrasil-db-synthesizer",
    "yggdrasil-db-truncater",
    "yggdrasil-dmq-node",
    "yggdrasil-kes-agent",
    "yggdrasil-kes-agent-control",
    "yggdrasil-snapshot-converter",
    "yggdrasil-tx-generator",
}

STALE_PATTERNS = {
    "old node crate path": re.compile(
        r"crates[/\\]node[/\\]yggdrasil-node"
    ),
    "old node src shorthand": re.compile(
        r"(?<![A-Za-z0-9_.-])yggdrasil-node[/\\]src(?=$|[/\\])"
    ),
    "old yggdrasil-node shorthand path": re.compile(
        r"(?<![A-Za-z0-9_.-])yggdrasil-node[/\\](?:AGENTS\.md|"
        r"Cargo\.toml|configuration|scripts|tests)(?=$|[/\\])"
    ),
    "old node src path": re.compile(
        r"(?<![A-Za-z0-9_.-])node[/\\]src(?=$|[/\\])"
    ),
    "old node tests path": re.compile(
        r"(?<![A-Za-z0-9_.-])node[/\\]tests(?=$|[/\\])"
    ),
    "old node metadata path": re.compile(
        r"(?<![A-Za-z0-9_.-])node[/\\](?:AGENTS\.md|Cargo\.toml)(?=$|[/\\])"
    ),
    "old top-level cardano-cli crate path": re.compile(
        r"crates[/\\]cardano-cli(?=$|[/\\])"
    ),
    "old top-level cardano-submit-api crate path": re.compile(
        r"crates[/\\]cardano-submit-api(?=$|[/\\])"
    ),
    "old top-level cardano-tracer crate path": re.compile(
        r"crates[/\\]cardano-tracer(?=$|[/\\])"
    ),
    "old top-level sister-tool crate path": re.compile(
        r"crates[/\\](?:bech32|cardano-cli|cardano-submit-api|"
        r"cardano-testnet|cardano-tracer|db-analyser|db-synthesizer|"
        r"db-truncater|dmq-node|kes-agent|kes-agent-control|"
        r"snapshot-converter|tx-generator)(?=$|[/\\])"
    ),
    "config nested under node": re.compile(
        r"(?<![A-Za-z0-9_.-])node[/\\]configuration(?=$|[/\\])"
    ),
    "scripts nested under node": re.compile(
        r"(?<![A-Za-z0-9_.-])node[/\\]scripts(?=$|[/\\])"
    ),
    "config nested under cardano-node crate": re.compile(
        r"crates[/\\]node[/\\]cardano-node[/\\]configuration"
    ),
    "scripts nested under cardano-node crate": re.compile(
        r"crates[/\\]node[/\\]cardano-node[/\\]scripts"
    ),
    "operator configuration nested under node crate": re.compile(
        r"crates[/\\]node[/\\][^/\\]+[/\\]configuration(?=$|[/\\])"
    ),
    "operator scripts nested under node crate": re.compile(
        r"crates[/\\]node[/\\][^/\\]+[/\\]scripts(?=$|[/\\])"
    ),
    "developer diagnostics nested under cardano-node crate": re.compile(
        r"crates[/\\]node[/\\]cardano-node[/\\]src[/\\]bin(?=$|[/\\])"
    ),
    "old Claude skill path": re.compile(r"\.claude[/\\]skills[/\\]cardano-node"),
    "stale node-local parameterized LSQ wording": re.compile(
        r"node-only parameterized LocalStateQuery variants|"
        r"parameterized or not-yet-migrated query tags local"
    ),
    "stale cardano-cli migration count": re.compile(
        r"3/35 subcommands|3-subcommand surface|current 33-subcommand C-arc surface"
    ),
    "stale cardano-cli subset wording": re.compile(
        r"pure-Rust subset \(`version`, `show-upstream-config`, `query-tip`\)|"
        r"pure-Rust subset of upstream `cardano-cli`|"
        r"future Phase 3 round migrates\s+the surface to the upstream-shaped "
        r"two-token form once the\s+in-crate `yggdrasil-cardano-cli` runtime "
        r"can host the parser\s+independently"
    ),
    "stale active cardano-cli migration wording": re.compile(
        r"plus the `cardano-cli` subcommand migration|"
        r"33 `Command` variants wired"
    ),
    "stale parity summary baseline wording": re.compile(
        r"394\+ parity rounds completed|"
        r"Workspace tests: \*\*5,638 passing|"
        r"post-R394|"
        r"249 parity rounds covering"
    ),
    "stale parity proof baseline wording": re.compile(
        r"Cumulative arc\*\*: R1\s*(?:→|->)\s*(?:R320|R529)\+|"
        r"Workspace tests\*\*: 4,982 passing|"
        r'canonical reference for "what works end-to-end" today|'
        r"check-parity-matrix\.py` over 8 entries"
    ),
    "stale upstream parity baseline wording": re.compile(
        r"Last updated: 2026-05-05; header|"
        r"Five-gate snapshot \(post-R311|"
        r"`cargo test --workspace --all-features`: 4,982 passing / 0 failing|"
        r"`check-parity-matrix\.py` \(8 entries|"
        r"Post-R529 focused cleanup gates"
    ),
    "stale living docs verification baseline": re.compile(
        r"6,519 tests passing|"
        r"7,295 tests passing|"
        r"7%2C295%20passing|"
        r"7,298 listed tests|"
        r"7,210 tests passing|"
        r"7,213 listed tests|"
        r"6\.5K\+.*passing|"
        r"4\.7K\+ tests|"
        r"4\.7K\+ passing|"
        r"Live workspace coverage is 4\.7K\+|"
        r"As of R248|"
        r"R211.*R248 arc"
    ),
    "stale blockfetch default flip wording": re.compile(
        r"flip the default .*max_concurrent_block_fetch_peers.*from `1` to `2`|"
        r"flip(?:ping)? the default .*max_concurrent_block_fetch_peers.*from 1 to 2|"
        r"before changing the default `max_concurrent_block_fetch_peers`|"
        r"shipped default \(`max_concurrent_block_fetch_peers = 1`\)"
    ),
    "stale tx-generator cardano-cli prerequisite gate": re.compile(
        r"cardano-cli C-arc CLI-MVS|"
        r"tx-generator.*cardano-cli CLI-MVS|"
        r"Phase C entry at R408\+|"
        r"Concrete subcommand dispatch lands at \*\*R435\+\*\*|"
        r"once concrete dispatch lands at `R435\+`"
    ),
    "stale workspace member gap wording": re.compile(
        r"the 9 `crates/node/\*` sub-crates|"
        r"are not in the root `Cargo\.toml` `\[workspace\]\s*$|"
        r"skip their own\s*$|"
        r"`cargo metadata` lists 24 members"
    ),
    "stale cardano-cli downstream gate wording": re.compile(
        r"gates Phase C of the sister-tools plan|"
        r"Hard-gated on CLI-MVS|"
        r"R424 drives CLI-MVS|"
        r"C-arc CLI-MVS gate at R408|"
        r"once concrete dispatch lands at `R417\+`"
    ),
    "stale cardano-cli bootstrap follow-up wording": re.compile(
        r"Phase F bootstrap state|"
        r"R298\+ migration roadmap|"
        r"concrete implementations port over multi-week R296\+ follow-up work"
    ),
    "stale cardano-submit-api structured-decoder debt": re.compile(
        r"structured-enum decoder, deferred|"
        r"Yggdrasil's variant is era-opaque at the Rust-type level pending|"
        r"raw-bytes carrier with the full per-era predicate-failure sum types"
    ),
    "stale cardano-submit-api R345/R346 evidence wording": re.compile(
        r"R345 milestone of the cardano-|"
        r"cardano-submit-api integration soak \(R345\)|"
        r"validation at R345|"
        r"Phase A\.2 closeout \(R346\)|"
        r"before R346 closeout|"
        r"lands at R345|"
        r"scheduled \(gated on R345\)|"
        r"cardano-submit-api\",\s*# partial\s*(?:—|-)\s*R338-R345 implementation arc"
    ),
    "stale cardano-submit-api accepted-response OK wording": re.compile(
        r"TxId in 202 success body\s*(?:—|-)\s*upstream returns `TxId` from|"
        r"Yggdrasil still returns `?\\?\"OK\\?\"`?|"
        r"Yggdrasil currently returns (?:an )?empty success body \(`?\\?\"OK\\?\"`?\)|"
        r"empty `?\\?\"OK\\?\"` response body|"
        r"Accepted-response parity: upstream `cardano-submit-api` returns the submitted transaction `TxId`"
    ),
    "stale kes-agent R344/R345 current-status wording": re.compile(
        r"subcommand dispatch lands at \*\*R345\+\*\*|"
        r"once concrete dispatch lands at `R345\+`|"
        r"Next:\s*\*\*R345\*\*|"
        r"awaiting Phase A\.3 entry at R344\+|"
        r"Skeleton round at R344: file-tree mirror \+ CLI parser scaffolding\.|"
        r"R344-R354(?: mini-arc)?"
    ),
    "stale kes-agent-control pre-R444 current-status wording": re.compile(
        r"subcommand dispatch lands at \*\*R356\+\*\*|"
        r"once concrete dispatch lands at `R356\+`|"
        r"Next:\s*\*\*R356\*\*"
    ),
    "stale sister-tool root manifest status label": re.compile(
        r'"crates/tools/bech32",\s*#\s*R334 verified_11_0_1|'
        r'"crates/tools/kes-agent",\s*#\s*skeleton\b.*R443 deferral|'
        r'"crates/tools/cardano-tracer",\s*#\s*partial\b.*R411-R438|'
        r'"crates/tools/db-truncater",\s*#\s*partial\b.*R347-R350 implementation arc|'
        r'"crates/tools/db-analyser",\s*#\s*partial\b.*R442 structured deferral surface|'
        r'"crates/tools/snapshot-converter",\s*#\s*partial\b.*R446 format-version scaffolding|'
        r'"crates/tools/db-synthesizer",\s*#\s*partial\b.*R441 structured deferral surface|'
        r'"crates/tools/cardano-testnet",\s*#\s*partial\b.*R445 structured deferral surface|'
        r'"crates/tools/cardano-testnet",\s*#\s*partial\b.*R534 era-aware runtime pending|'
        r'"crates/tools/tx-generator",\s*#\s*skeleton\b.*tx-generator parser/submission arc pending|'
        r'"crates/tools/dmq-node",\s*#\s*partial\b.*R444 structured deferral surface'
    ),
    "stale dmq-node pre-R816 current-status wording": re.compile(
        r"Diffusion \+ NodeToNode \+ NodeToClient \+ NodeKernel implementation pending\.|"
        r"R357\+ for Diffusion/NodeKernel/PeerSelection wiring|"
        r"Diffusion/NodeKernel/PeerSelection wiring lands at R357\+|"
        r"once concrete dispatch lands at `R451\+`\.|"
        r"Next:\s*\*\*R451\*\*"
    ),
    "stale cardano-testnet pre-R823 current-status wording": re.compile(
        r"R445 surfaces the era-aware-dispatch \+ Process/Property carve-outs as a `\*_status\(\)` helper\.|"
        r"cardano-testnet mini-arc per .*R416-R433|"
        r"R367 lands argv (?:→|->) \[`parser::Command`\] subcommand dispatch\.|"
        r"`?parse_runtime_options`,\s*//! `parse_genesis_options` so far"
    ),
    "stale cardano-testnet Command-payload gap wording": re.compile(
        r"Command payload/runtime pending|"
        r"Command payload wiring pending|"
        r"Remaining work is Command payload wiring|"
        r"parse_args still carries PassthroughArgs until the next wiring slice|"
        r"Wire parse_args Command::Cardano / Command::CreateEnv to carry the typed CardanoTestnetCliOptions / CardanoTestnetCreateEnvOptions records instead of PassthroughArgs\.|"
        r"Pending: thread those typed records into `Command::Cardano` / `Command::CreateEnv`"
    ),
    "stale cardano-testnet process-handle type gap wording": re.compile(
        r"process-handle runtime types \(`TestnetNode`,\s*`TestnetRuntime`,\s*`TestnetKesAgent`[^)]*hold OS process / stdio handles|"
        r"Port Testnet/Types\.hs process-handle runtime types \(TestnetRuntime / TestnetNode / TestnetKesAgent\) with local process supervision\.|"
        r"`Testnet/Types\.hs` \(portable runtime/key types\)\s*\|\s*`runtime_types\.rs`"
    ),
    "stale cardano-testnet process-cli keys gap wording": re.compile(
        r"`Testnet/Process/Cli/\*\.hs` \(SPO/Tx/Keys/DRep dispatch\)\s*\|\s*`process/cli/\*\.rs` \(pending\)|"
        r"Process/Cli/Keys\.hs .*pending|"
        r"Port Testnet/Process/Cli/Keys\.hs"
    ),
    "stale cardano-testnet transaction sign-submit-txid gap wording": re.compile(
        r"Process/Cli/\{SPO,Transaction,DRep\}\.hs helpers|"
        r"Port the remaining Process/Cli SPO, Transaction, and DRep helpers|"
        r"signTx/submitTx/retrieveTransactionId pending|"
        r"Transaction sign/submit/txid builders pending"
    ),
    "stale cardano-testnet drep pure-builder gap wording": re.compile(
        r"Process/Cli SPO/DRep helpers|"
        r"Process/Cli/\{SPO,DRep\}\.hs helpers|"
        r"DRep key/cert/vote builders pending|"
        r"Port Testnet/Process/Cli/DRep\.hs"
    ),
    "stale cardano-testnet spo pure-builder gap wording": re.compile(
        r"Process/Cli SPO helpers|"
        r"Process/Cli/SPO\.hs helpers pending|"
        r"SPO cert(?:ificate)?/vote builders pending|"
        r"Port Testnet/Process/Cli/SPO\.hs"
    ),
    "stale cardano-testnet process-run gap wording": re.compile(
        r"Process/Run\.hs .*pending|"
        r"Process execution wrappers pending|"
        r"process execution wrappers,\s*node/KES spawning|"
        r"without process execution wrappers"
    ),
    "stale cardano-testnet runio plan-json gap wording": re.compile(
        r"RunIO.*plan-json.*pending|"
        r"plan-json binary-resolution helpers pending|"
        r"without RunIO plan-json"
    ),
    "stale cardano-testnet runio execution gap wording": re.compile(
        r"RunIO.*execution.*pending|"
        r"RunIO execution/liftIO helpers pending|"
        r"without RunIO execution"
    ),
    "stale cardano-testnet property-util pure-helper gap wording": re.compile(
        r"Property/Util\.hs .*pending|"
        r"integrationRetryWorkspace pending|"
        r"aesonObjectLookUp pending|"
        r"without Property/Util"
    ),
    "stale cardano-testnet property-assert pure-helper gap wording": re.compile(
        r"Property/Assert\.hs .*pending|"
        r"readJsonLines pending|"
        r"getRelevantSlots pending|"
        r"assertErasEqual pending|"
        r"without Property/Assert|"
        r"CLI-backed property assertions|"
        r"remaining CLI-backed assertion wrappers|"
        r"assertExpectedSposInLedgerState .*pending"
    ),
    "stale cardano-testnet property-run pure-helper gap wording": re.compile(
        r"Property/Run\.hs pure .*pending|"
        r"testnetProperty planning pending|"
        r"UserProvidedEnv pending|"
        r"ignoreOn(?:Windows|Mac|MacAndWindows)? pending|"
        r"without Property/Run helpers"
    ),
    "stale bech32 pre-verified current-status wording": re.compile(
        r"Yggdrasil pure-Rust port\s*(?:—|-)\s*R327 skeleton; concrete implementation lands across the A\.1 sub-arc per the R326(?:–|-)R459 sister-tools port plan\."
    ),
    "stale R178 LSQ closure wording": re.compile(
        r"The Conway-era LSQ wire-protocol gap is fully closed|"
        r"core node parity closure remains intact"
    ),
}

SELF_TEST_STALE_CASES = (
    ("crates/node/yggdrasil-node/src/main.rs", "old node crate path"),
    ("yggdrasil-node/src/main.rs", "old node src shorthand"),
    ("yggdrasil-node/scripts/run-tools.sh", "old yggdrasil-node shorthand path"),
    ("yggdrasil-node/configuration/mainnet/config.json", "old yggdrasil-node shorthand path"),
    ("node/src/main.rs", "old node src path"),
    ("node/tests/runtime.rs", "old node tests path"),
    ("node/AGENTS.md", "old node metadata path"),
    ("crates/cardano-cli/src/lib.rs", "old top-level cardano-cli crate path"),
    ("crates/cardano-submit-api/src/lib.rs", "old top-level cardano-submit-api crate path"),
    ("crates/cardano-tracer/src/lib.rs", "old top-level cardano-tracer crate path"),
    ("crates/db-truncater/src/lib.rs", "old top-level sister-tool crate path"),
    (
        "The Conway-era LSQ wire-protocol gap is fully closed",
        "stale R178 LSQ closure wording",
    ),
    ("core node parity closure remains intact", "stale R178 LSQ closure wording"),
    ("node/configuration/preview/config.json", "config nested under node"),
    ("node/scripts/run-tools.sh", "scripts nested under node"),
    (
        "crates/node/cardano-node/configuration/mainnet/config.json",
        "config nested under cardano-node crate",
    ),
    ("crates/node/cardano-node/scripts/run-tools.sh", "scripts nested under cardano-node crate"),
    (
        "crates/node/runtime/configuration/mainnet/config.json",
        "operator configuration nested under node crate",
    ),
    ("crates/node/runtime/scripts/run-tools.sh", "operator scripts nested under node crate"),
    (
        "crates/node/cardano-node/src/bin/dump_block.rs",
        "developer diagnostics nested under cardano-node crate",
    ),
    (".claude/skills/cardano-node/SKILL.md", "old Claude skill path"),
    (
        "node-only parameterized LocalStateQuery variants",
        "stale node-local parameterized LSQ wording",
    ),
    ("3/35 subcommands", "stale cardano-cli migration count"),
    (
        "cardano-cli - pure-Rust subset (`version`, `show-upstream-config`, `query-tip`)",
        "stale cardano-cli subset wording",
    ),
    (
        "plus the `cardano-cli` subcommand migration",
        "stale active cardano-cli migration wording",
    ),
    ("33 `Command` variants wired", "stale active cardano-cli migration wording"),
    ("394+ parity rounds completed", "stale parity summary baseline wording"),
    (
        "Workspace tests: **5,638 passing, 0 failing**",
        "stale parity summary baseline wording",
    ),
    ("post-R394", "stale parity summary baseline wording"),
    ("249 parity rounds covering", "stale parity summary baseline wording"),
    ("**Cumulative arc**: R1 -> R320+", "stale parity proof baseline wording"),
    ("**Cumulative arc**: R1 -> R529+", "stale parity proof baseline wording"),
    (
        "**Workspace tests**: 4,982 passing, 0 failing",
        "stale parity proof baseline wording",
    ),
    (
        'canonical reference for "what works end-to-end" today',
        "stale parity proof baseline wording",
    ),
    (
        "check-parity-matrix.py` over 8 entries",
        "stale parity proof baseline wording",
    ),
    (
        "Last updated: 2026-05-05; header + verification-baseline refreshed",
        "stale upstream parity baseline wording",
    ),
    (
        "### Five-gate snapshot (post-R311, 2026-05-09)",
        "stale upstream parity baseline wording",
    ),
    (
        "`cargo test --workspace --all-features`: 4,982 passing / 0 failing",
        "stale upstream parity baseline wording",
    ),
    (
        "`check-parity-matrix.py` (8 entries against tag `11.0.1`)",
        "stale upstream parity baseline wording",
    ),
    (
        "Post-R529 focused cleanup gates",
        "stale upstream parity baseline wording",
    ),
    ("6,519 tests passing, 0 failing", "stale living docs verification baseline"),
    ("7,295 tests passing, 0 failing", "stale living docs verification baseline"),
    ("tests-7%2C295%20passing-brightgreen", "stale living docs verification baseline"),
    ("7,298 listed tests total", "stale living docs verification baseline"),
    ("7,210 tests passing, 0 failing", "stale living docs verification baseline"),
    ("7,213 listed tests total", "stale living docs verification baseline"),
    ("The full workspace runs **4.7K+ tests**", "stale living docs verification baseline"),
    ("Live workspace coverage is 4.7K+ passing tests", "stale living docs verification baseline"),
    ("### Remaining gates (R211→R248 arc)", "stale living docs verification baseline"),
    (
        "flip the default in `crates/node/config/src/lib.rs::default_max_concurrent_block_fetch_peers` from `1` to `2`",
        "stale blockfetch default flip wording",
    ),
    (
        "before flipping the default `max_concurrent_block_fetch_peers` from 1 to 2",
        "stale blockfetch default flip wording",
    ),
    (
        "before changing the default `max_concurrent_block_fetch_peers`",
        "stale blockfetch default flip wording",
    ),
    (
        "shipped default (`max_concurrent_block_fetch_peers = 1`)",
        "stale blockfetch default flip wording",
    ),
    (
        "tx-generator on the cardano-cli CLI-MVS (A2)",
        "stale tx-generator cardano-cli prerequisite gate",
    ),
    (
        "Phase C entry at R408+ depends on the cardano-cli C-arc CLI-MVS",
        "stale tx-generator cardano-cli prerequisite gate",
    ),
    (
        "the 9 `crates/node/*` sub-crates are not in the root `Cargo.toml`",
        "stale workspace member gap wording",
    ),
    ("`cargo metadata` lists 24 members", "stale workspace member gap wording"),
    (
        "own R-numbering and gates Phase C of the sister-tools plan",
        "stale cardano-cli downstream gate wording",
    ),
    (
        "Hard-gated on CLI-MVS (keys/tx/query/genesis/governance)",
        "stale cardano-cli downstream gate wording",
    ),
    (
        "concrete implementations port over multi-week R296+ follow-up work",
        "stale cardano-cli bootstrap follow-up wording",
    ),
    (
        "**Remaining work** (Phase 2 - structured-enum decoder, deferred)",
        "stale cardano-submit-api structured-decoder debt",
    ),
    (
        "All endpoints byte-identical. Phase A.2 closeout (R346) can",
        "stale cardano-submit-api R345/R346 evidence wording",
    ),
    (
        "subject to live-rehearsal validation at R345.",
        "stale cardano-submit-api R345/R346 evidence wording",
    ),
    (
        "### B2 - cardano-submit-api integration soak (R345)",
        "stale cardano-submit-api R345/R346 evidence wording",
    ),
    (
        '"crates/tools/cardano-submit-api",    # partial — R338-R345 implementation arc',
        "stale cardano-submit-api R345/R346 evidence wording",
    ),
    (
        "subcommand dispatch lands at **R345+** per the R326-R459 sister-tools port arc plan",
        "stale kes-agent R344/R345 current-status wording",
    ),
    (
        "SKELETON STUB awaiting Phase A.3 entry at R344+",
        "stale kes-agent R344/R345 current-status wording",
    ),
    (
        "Skeleton round at R344: file-tree mirror + CLI parser scaffolding.",
        "stale kes-agent R344/R345 current-status wording",
    ),
    (
        "subcommand dispatch lands at **R356+** per the R326-R459 sister-tools port arc plan",
        "stale kes-agent-control pre-R444 current-status wording",
    ),
    (
        "once concrete dispatch lands at `R356+`",
        "stale kes-agent-control pre-R444 current-status wording",
    ),
    (
        "Next: **R356** — first concrete-impl round of the mini-arc.",
        "stale kes-agent-control pre-R444 current-status wording",
    ),
    (
        '"crates/tools/bech32",                # R334 verified_11_0_1',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/kes-agent",             # skeleton — R443 deferral; R444+ daemon/socket follow-on',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/cardano-tracer",        # partial — R411-R438 cardano-tracer named arc closed + R431-R437 follow-ons',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/db-truncater",          # partial — R347-R350 implementation arc',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/db-analyser",           # partial — R442 structured deferral surface',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/snapshot-converter",    # partial — R446 format-version scaffolding',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/db-synthesizer",        # partial — R441 structured deferral surface',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/cardano-testnet",       # partial — R445 structured deferral surface',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/tx-generator",          # skeleton — cardano-cli prerequisite closed; tx-generator parser/submission arc pending',
        "stale sister-tool root manifest status label",
    ),
    (
        '"crates/tools/dmq-node",              # partial — R444 structured deferral surface',
        "stale sister-tool root manifest status label",
    ),
    (
        "Diffusion + NodeToNode + NodeToClient + NodeKernel implementation pending.",
        "stale dmq-node pre-R816 current-status wording",
    ),
    (
        "R361 lands the parser → resolve → run() chain; the actual Diffusion/NodeKernel/PeerSelection wiring lands at R357+",
        "stale dmq-node pre-R816 current-status wording",
    ),
    (
        "once concrete dispatch lands at `R451+`.",
        "stale dmq-node pre-R816 current-status wording",
    ),
    (
        "Next: **R451** — first concrete-impl round of the mini-arc.",
        "stale dmq-node pre-R816 current-status wording",
    ),
    (
        "R445 surfaces the era-aware-dispatch + Process/Property carve-outs as a `*_status()` helper.",
        "stale cardano-testnet pre-R823 current-status wording",
    ),
    (
        "the cardano-testnet mini-arc per the playful-tickling-plum.md plan (R416-R433",
        "stale cardano-testnet pre-R823 current-status wording",
    ),
    (
        "R367 lands argv → [`parser::Command`] subcommand dispatch.",
        "stale cardano-testnet pre-R823 current-status wording",
    ),
    (
        "parse_runtime_options`,\n//! `parse_genesis_options` so far",
        "stale cardano-testnet pre-R823 current-status wording",
    ),
    (
        '"crates/tools/cardano-testnet",       # partial — R823 parser composition; Command payload/runtime pending',
        "stale cardano-testnet Command-payload gap wording",
    ),
    (
        "`Parsers/{Run,Cardano}.hs` | `parser.rs` (Command payload wiring pending)",
        "stale cardano-testnet Command-payload gap wording",
    ),
    (
        "The typed CardanoTestnetCliOptions / CardanoTestnetCreateEnvOptions records are produced by helper parsers; parse_args still carries PassthroughArgs until the next wiring slice.",
        "stale cardano-testnet Command-payload gap wording",
    ),
    (
        "The process-handle runtime types (`TestnetNode`, `TestnetRuntime`, `TestnetKesAgent` - they hold OS process / stdio handles) remain pending.",
        "stale cardano-testnet process-handle type gap wording",
    ),
    (
        "Port Testnet/Types.hs process-handle runtime types (TestnetRuntime / TestnetNode / TestnetKesAgent) with local process supervision.",
        "stale cardano-testnet process-handle type gap wording",
    ),
    (
        "| `Testnet/Types.hs` (portable runtime/key types) | `runtime_types.rs` |",
        "stale cardano-testnet process-handle type gap wording",
    ),
    (
        "| `Testnet/Process/Cli/*.hs` (SPO/Tx/Keys/DRep dispatch) | `process/cli/*.rs` (pending) |",
        "stale cardano-testnet process-cli keys gap wording",
    ),
    (
        "Process/Cli/Keys.hs remains pending until the next process-wrapper round.",
        "stale cardano-testnet process-cli keys gap wording",
    ),
    (
        "Port Testnet/Process/Cli/Keys.hs before node spawning.",
        "stale cardano-testnet process-cli keys gap wording",
    ),
    (
        "Port the remaining Process/Cli SPO, Transaction, and DRep helpers.",
        "stale cardano-testnet transaction sign-submit-txid gap wording",
    ),
    (
        "Transaction sign/submit/txid builders pending until the next process-wrapper round.",
        "stale cardano-testnet transaction sign-submit-txid gap wording",
    ),
    (
        "Process/Cli SPO/DRep helpers remain pending.",
        "stale cardano-testnet drep pure-builder gap wording",
    ),
    (
        "Process/Cli SPO helpers remain pending.",
        "stale cardano-testnet spo pure-builder gap wording",
    ),
    (
        "SPO certificate/vote builders pending until the next process-wrapper round.",
        "stale cardano-testnet spo pure-builder gap wording",
    ),
    (
        "Port Testnet/Process/Cli/SPO.hs before the next cardano-testnet slice.",
        "stale cardano-testnet spo pure-builder gap wording",
    ),
    (
        "DRep key/cert/vote builders pending until the next process-wrapper round.",
        "stale cardano-testnet drep pure-builder gap wording",
    ),
    (
        '"crates/tools/cardano-testnet",       # partial — R534 era-aware runtime pending',
        "stale sister-tool root manifest status label",
    ),
    (
        "Process/Run.hs execution helpers remain pending until the next process-wrapper round.",
        "stale cardano-testnet process-run gap wording",
    ),
    (
        "Process execution wrappers pending until node startup lands.",
        "stale cardano-testnet process-run gap wording",
    ),
    (
        "The current testnet crate still runs without process execution wrappers.",
        "stale cardano-testnet process-run gap wording",
    ),
    (
        "RunIO plan-json process planning remains pending.",
        "stale cardano-testnet runio plan-json gap wording",
    ),
    (
        "plan-json binary-resolution helpers pending until the next process slice.",
        "stale cardano-testnet runio plan-json gap wording",
    ),
    (
        "The current testnet crate still runs without RunIO plan-json lookup.",
        "stale cardano-testnet runio plan-json gap wording",
    ),
    (
        "RunIO execution helpers remain pending.",
        "stale cardano-testnet runio execution gap wording",
    ),
    (
        "RunIO execution/liftIO helpers pending until the next process slice.",
        "stale cardano-testnet runio execution gap wording",
    ),
    (
        "The current testnet crate still runs without RunIO execution wrappers.",
        "stale cardano-testnet runio execution gap wording",
    ),
    (
        "Property/Util.hs pure helper port remains pending.",
        "stale cardano-testnet property-util pure-helper gap wording",
    ),
    (
        "integrationRetryWorkspace pending until Process/Property lands.",
        "stale cardano-testnet property-util pure-helper gap wording",
    ),
    (
        "The current testnet crate still runs without Property/Util helpers.",
        "stale cardano-testnet property-util pure-helper gap wording",
    ),
    (
        "Property/Assert.hs pure helper port remains pending.",
        "stale cardano-testnet property-assert pure-helper gap wording",
    ),
    (
        "readJsonLines pending until Process/Property lands.",
        "stale cardano-testnet property-assert pure-helper gap wording",
    ),
    (
        "The current testnet crate still runs without Property/Assert helpers.",
        "stale cardano-testnet property-assert pure-helper gap wording",
    ),
    (
        "CLI-backed property assertions remain pending.",
        "stale cardano-testnet property-assert pure-helper gap wording",
    ),
    (
        "assertExpectedSposInLedgerState wrapper pending until Process/Property lands.",
        "stale cardano-testnet property-assert pure-helper gap wording",
    ),
    (
        "Property/Run.hs pure helper port remains pending.",
        "stale cardano-testnet property-run pure-helper gap wording",
    ),
    (
        "testnetProperty planning pending until the next Property/Run slice.",
        "stale cardano-testnet property-run pure-helper gap wording",
    ),
    (
        "ignoreOnWindows pending until Process/Property lands.",
        "stale cardano-testnet property-run pure-helper gap wording",
    ),
    (
        "The current testnet crate still runs without Property/Run helpers.",
        "stale cardano-testnet property-run pure-helper gap wording",
    ),
    (
        "Yggdrasil pure-Rust port — R327 skeleton; concrete implementation lands across the A.1 sub-arc per the R326-R459 sister-tools port plan.",
        "stale bech32 pre-verified current-status wording",
    ),
    (
        'Accepted-response parity: upstream `cardano-submit-api` returns the submitted transaction `TxId` in the 202 Accepted JSON body (`Handler TxId`); Yggdrasil still returns `"OK"` until multi-era TxId derivation is wired into `tx_submit_post`.',
        "stale cardano-submit-api accepted-response OK wording",
    ),
)

SELF_TEST_ALLOWED_CASES = (
    "crates/node/cardano-node/src/main.rs",
    "crates/node/cardano-node/tests/runtime.rs",
    "crates/node/config/src/lib.rs",
    "crates/tools/cardano-cli/src/lib.rs",
    "crates/tools/db-analyser/src/bin/dump_block.rs",
    "configuration/preview/config.json",
    "scripts/run-tools.sh",
    "cargo build --bin yggdrasil-node",
    "https://github.com/yggdrasil-node/Cardano-node",
    ".reference-haskell-cardano-node/configuration/cardano",
)


def git_paths() -> list[str]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
    )
    paths = [
        p.decode("utf-8", errors="replace").replace("\\", "/")
        for p in result.stdout.split(b"\0")
        if p
    ]
    paths = [path for path in paths if (ROOT / path).exists()]
    return sorted(set(paths))


def should_scan(path: str) -> bool:
    if path in EXCLUDED_PATHS:
        return False
    if any(path.startswith(prefix) for prefix in EXCLUDED_PREFIXES):
        return False
    if path.startswith("docs/operational-runs/"):
        return is_current_operational_run_note(path)
    return True


def is_current_operational_run_note(path: str) -> bool:
    """Scan current run notes while preserving older run logs as evidence."""

    filename = Path(path).name
    if not filename.endswith(".md"):
        return False
    prefix = filename[:10]
    try:
        run_date = date.fromisoformat(prefix)
    except ValueError:
        return False
    return run_date >= CURRENT_OPERATIONAL_RUN_CUTOFF


def scan_file(path: str) -> list[tuple[int, str, str]]:
    full = ROOT / path
    try:
        text = full.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return []

    start_line = 1
    if path == "CHANGELOG.md":
        text, start_line = unreleased_changelog_text(text)

    hits: list[tuple[int, str, str]] = []
    for offset, line in enumerate(text.splitlines()):
        line_no = start_line + offset
        for label, pattern in STALE_PATTERNS.items():
            if pattern.search(line):
                hits.append((line_no, label, line.strip()))
    return hits


def matching_labels(text: str) -> set[str]:
    return {
        label
        for label, pattern in STALE_PATTERNS.items()
        if pattern.search(text)
    }


def stale_directory_failures(root: Path = ROOT) -> list[str]:
    failures = []
    for label, relative_path in STALE_DIRECTORIES.items():
        full_path = root / relative_path
        if full_path.exists():
            failures.append(f"{relative_path}:0: stale filesystem path: {label}")
    return failures


def required_current_placement_failures(
    root: Path = ROOT,
    placements: dict[str, str] = REQUIRED_CURRENT_PLACEMENTS,
) -> list[str]:
    failures = []
    for label, relative_path in placements.items():
        full_path = root / relative_path
        if not full_path.exists():
            failures.append(
                f"{relative_path}:0: required current placement missing: {label}"
            )
    return failures


def required_text_snippet_failures(
    root: Path = ROOT,
    snippets: tuple[tuple[str, str, str], ...] = REQUIRED_TEXT_SNIPPETS,
) -> list[str]:
    failures = []
    for relative_path, label, snippet in snippets:
        full_path = root / relative_path
        try:
            text = full_path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            failures.append(
                f"{relative_path}:0: required packaging surface missing: {label}"
            )
            continue
        if snippet not in text:
            failures.append(
                f"{relative_path}:0: required packaging surface missing: {label}"
            )
    return failures


def root_shell_script_mode_failures(
    root: Path = ROOT,
    *,
    stage_output: bytes | None = None,
) -> list[str]:
    """Require tracked root shell helpers to retain executable file mode."""

    if stage_output is None:
        try:
            result = subprocess.run(
                ["git", "ls-files", "--stage", "-z", "--", "scripts/*.sh"],
                cwd=root,
                check=True,
                stdout=subprocess.PIPE,
            )
        except (OSError, subprocess.CalledProcessError) as exc:
            return [f"scripts/*.sh:0: failed to inspect executable modes: {exc}"]
        stage_output = result.stdout

    failures = []
    tracked_shell_scripts = 0
    for entry in stage_output.split(b"\0"):
        if not entry:
            continue
        text = entry.decode("utf-8", errors="replace")
        if "\t" not in text:
            continue
        metadata, path = text.split("\t", 1)
        normalized_path = path.replace("\\", "/")
        if not normalized_path.startswith("scripts/") or not normalized_path.endswith(
            ".sh"
        ):
            continue
        tracked_shell_scripts += 1
        mode = metadata.split(" ", 1)[0]
        if mode != "100755":
            failures.append(
                f"{normalized_path}:0: root operator shell script must be "
                f"tracked executable (100755); got {mode}"
            )

    if tracked_shell_scripts == 0:
        failures.append("scripts/*.sh:0: no tracked root shell scripts found")

    return failures


def node_crate_operator_artifact_directory_failures(root: Path = ROOT) -> list[str]:
    """Reject node-crate-local operator artifact directories.

    The accepted layout keeps operator presets and helper scripts at repository
    root. This catches the same stale placement class if it reappears under a
    different `crates/node/*` package instead of only under the binary crate.
    """

    node_root = root / "crates/node"
    if not node_root.exists():
        return []

    failures = []
    for crate_dir in node_root.iterdir():
        if not crate_dir.is_dir():
            continue
        for artifact_dir in ("configuration", "scripts"):
            full_path = crate_dir / artifact_dir
            if not full_path.exists():
                continue
            relative_path = full_path.relative_to(root).as_posix()
            failures.append(
                f"{relative_path}:0: operator artifact directory must stay at "
                "repository root, not under a node crate"
            )
    return failures


def cargo_binary() -> str | None:
    cargo = shutil.which("cargo")
    if cargo:
        return cargo

    suffix = ".exe" if sys.platform == "win32" else ""
    rustup_cargo = Path.home() / ".cargo" / "bin" / f"cargo{suffix}"
    if rustup_cargo.exists():
        return str(rustup_cargo)
    return None


def cargo_metadata_failures(root: Path = ROOT) -> list[str]:
    cargo = cargo_binary()
    if cargo is None:
        return ["Cargo metadata:0: cargo executable not found"]

    try:
        result = subprocess.run(
            [cargo, "metadata", "--no-deps", "--format-version", "1"],
            cwd=root,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        return [f"Cargo metadata:0: failed to inspect workspace paths: {exc}"]

    try:
        metadata = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        return [f"Cargo metadata:0: invalid JSON: {exc}"]

    return cargo_metadata_path_failures(metadata)


def cargo_metadata_path_failures(metadata: object) -> list[str]:
    failures: list[str] = []
    if not isinstance(metadata, dict):
        return ["Cargo metadata:0: expected metadata object"]

    for member in metadata.get("workspace_members", []):
        failures.extend(stale_metadata_path_failures("workspace member", member))

    packages = metadata.get("packages", [])
    if not isinstance(packages, list):
        return failures

    for package in packages:
        if not isinstance(package, dict):
            continue
        package_name = package.get("name", "<unknown>")
        failures.extend(
            stale_metadata_path_failures(
                f"package {package_name} manifest",
                package.get("manifest_path"),
            )
        )
        targets = package.get("targets", [])
        if not isinstance(targets, list):
            continue
        for target in targets:
            if not isinstance(target, dict):
                continue
            target_name = target.get("name", "<unknown>")
            failures.extend(
                stale_metadata_path_failures(
                    f"package {package_name} target {target_name}",
                    target.get("src_path"),
                )
            )

    failures.extend(cargo_workspace_bucket_failures(metadata))
    return failures


def stale_metadata_path_failures(context: str, value: object) -> list[str]:
    if not isinstance(value, str):
        return []
    normalized = value.replace("\\", "/")
    return [
        f"Cargo metadata:0: {label}: {context}: {value}"
        for label in matching_labels(normalized)
    ]


def cargo_workspace_bucket_failures(metadata: dict[str, object]) -> list[str]:
    packages = metadata.get("packages", [])
    if not isinstance(packages, list):
        return []

    failures: list[str] = []
    for package in packages:
        if not isinstance(package, dict):
            continue
        package_name = package.get("name")
        manifest = canonical_metadata_path(package.get("manifest_path"))
        if not isinstance(package_name, str) or manifest is None:
            continue

        if package_name == NODE_BINARY_PACKAGE:
            if manifest != NODE_BINARY_MANIFEST:
                failures.append(
                    "Cargo metadata:0: workspace package bucket: "
                    f"{NODE_BINARY_PACKAGE} manifest must stay at "
                    f"{NODE_BINARY_MANIFEST}; got {manifest}"
                )
        elif package_name.startswith(NODE_SUPPORT_PACKAGE_PREFIX):
            if not manifest.startswith("crates/node/"):
                failures.append(
                    "Cargo metadata:0: workspace package bucket: "
                    f"{package_name} must live under crates/node/; got {manifest}"
                )
        elif package_name in SISTER_TOOL_PACKAGES:
            if not manifest.startswith("crates/tools/"):
                failures.append(
                    "Cargo metadata:0: workspace package bucket: "
                    f"{package_name} must live under crates/tools/; got {manifest}"
                )

    return failures


def canonical_metadata_path(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    normalized = value.replace("\\", "/")
    if normalized.startswith("path+file://"):
        normalized = normalized.removeprefix("path+file://").split("#", 1)[0]
    if "/crates/" in normalized:
        return "crates/" + normalized.split("/crates/", 1)[1]
    if normalized.startswith("crates/"):
        return normalized
    if "/xtask/" in normalized:
        return "xtask/" + normalized.split("/xtask/", 1)[1]
    if normalized.startswith("xtask/"):
        return normalized
    return normalized


def reference_git_metadata_failures(root: Path = ROOT) -> list[str]:
    reference_root = root / REFERENCE_TREE
    if not reference_root.exists():
        return []

    failures = []
    for git_metadata in reference_root.rglob(".git"):
        if not git_metadata.exists():
            continue
        relative_path = git_metadata.relative_to(root).as_posix()
        failures.append(
            f"{relative_path}:0: nested git metadata in reference snapshot"
        )
    return failures


@contextmanager
def self_test_temp_directory():
    """Create self-test scratch space under the workspace.

    Some sandboxed local environments deny writes to the process-wide temp
    directory. CI still gets isolated throwaway directories because
    `target/` is untracked and job-local.
    """

    SELF_TEST_TMP_ROOT.mkdir(parents=True, exist_ok=True)
    temp_root = SELF_TEST_TMP_ROOT / f"case-{uuid.uuid4().hex}"
    temp_root.mkdir(parents=True)
    try:
        yield temp_root
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def reference_gitignore_failures(
    root: Path = ROOT,
    *,
    verify_with_git: bool = True,
) -> list[str]:
    """Require the upstream reference snapshot to stay ignored by Git."""

    failures = []
    gitignore = root / ".gitignore"
    try:
        gitignore_text = gitignore.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        gitignore_text = ""

    if REFERENCE_TREE not in gitignore_text:
        failures.append(
            ".gitignore:0: reference snapshot must be ignored as a local-only corpus"
        )
        return failures

    if not verify_with_git:
        return failures

    for path in (
        REFERENCE_TREE,
        f"{REFERENCE_TREE}/deps/cardano-node/README.md",
    ):
        try:
            result = subprocess.run(
                ["git", "check-ignore", "-q", "--", path],
                cwd=root,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        except OSError:
            failures.append(
                f"{path}:0: failed to verify reference snapshot ignore rule"
            )
            continue
        if result.returncode != 0:
            failures.append(
                f"{path}:0: reference snapshot must be ignored by Git"
            )

    return failures


def reference_submodule_failures(
    root: Path = ROOT,
    *,
    inspect_git_index: bool = True,
) -> list[str]:
    failures = []

    gitmodules = root / ".gitmodules"
    if gitmodules.exists():
        try:
            text = gitmodules.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            text = ""
        if REFERENCE_TREE in text:
            failures.append(
                ".gitmodules:0: reference snapshot must not be declared as a submodule"
            )

    if not inspect_git_index:
        return failures

    try:
        result = subprocess.run(
            ["git", "ls-files", "--stage", "-z", "--", REFERENCE_TREE],
            cwd=root,
            check=True,
            stdout=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError):
        return failures

    for entry in result.stdout.split(b"\0"):
        if not entry:
            continue
        text = entry.decode("utf-8", errors="replace")
        failures.extend(reference_index_entry_failures(text))

    return failures


def reference_index_entry_failures(entry: str) -> list[str]:
    if "\t" in entry:
        metadata, path = entry.split("\t", 1)
    else:
        metadata, path = entry, REFERENCE_TREE

    normalized_path = path.replace("\\", "/")
    if normalized_path != REFERENCE_TREE and not normalized_path.startswith(
        f"{REFERENCE_TREE}/"
    ):
        return []

    mode = metadata.split(" ", 1)[0]
    if mode == "160000":
        return [
            f"{REFERENCE_TREE}:0: reference snapshot must not be tracked as a submodule"
        ]

    return [
        f"{normalized_path}:0: reference snapshot must not enter the Git index"
    ]


def unreleased_changelog_text(text: str) -> tuple[str, int]:
    """Return only the current changelog section and its 1-based start line."""

    lines = text.splitlines()
    start = None
    for index, line in enumerate(lines):
        if line == "## [Unreleased]":
            start = index
            break
    if start is None:
        return "", 1

    end = len(lines)
    for index in range(start + 1, len(lines)):
        if lines[index].startswith("## "):
            end = index
            break

    return "\n".join(lines[start:end]), start + 1


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        return self_test()
    if len(sys.argv) != 1:
        print("usage: check-stale-placement.py [--self-test]", file=sys.stderr)
        return 2

    failures: list[str] = stale_directory_failures()
    failures.extend(required_current_placement_failures())
    failures.extend(required_text_snippet_failures())
    failures.extend(root_shell_script_mode_failures())
    failures.extend(node_crate_operator_artifact_directory_failures())
    failures.extend(reference_git_metadata_failures())
    failures.extend(reference_gitignore_failures())
    failures.extend(reference_submodule_failures())
    failures.extend(cargo_metadata_failures())
    for path in git_paths():
        if not should_scan(path):
            continue
        for label, pattern in STALE_PATTERNS.items():
            if pattern.search(path):
                failures.append(f"{path}:0: stale file path: {label}")
        for line_no, label, line in scan_file(path):
            failures.append(f"{path}:{line_no}: {label}: {line}")

    if failures:
        print("stale placement references found:")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("stale placement check clean")
    return 0


def self_test() -> int:
    failures: list[str] = []

    for text, expected_label in SELF_TEST_STALE_CASES:
        labels = matching_labels(text)
        if expected_label not in labels:
            failures.append(
                f"expected {expected_label!r} to match {text!r}; got {sorted(labels)!r}"
            )

    for text in SELF_TEST_ALLOWED_CASES:
        labels = matching_labels(text)
        if labels:
            failures.append(f"expected {text!r} to be allowed; got {sorted(labels)!r}")

    if should_scan("docs/operational-runs/2026-05-17-old.md"):
        failures.append("old operational-run records should stay excluded")
    if not should_scan("docs/operational-runs/2026-05-18-current.md"):
        failures.append("current operational-run records should be scanned")
    if should_scan("docs/operational-runs/2026-05-18-current.log"):
        failures.append("operational-run logs should stay excluded as artifacts")
    if should_scan("docs/archive/old.md"):
        failures.append("docs/archive should stay excluded")
    if should_scan("scripts/check-stale-placement.py"):
        failures.append("guard source should stay excluded from content scanning")

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        for relative_path in STALE_DIRECTORIES.values():
            (temp_root / relative_path).mkdir(parents=True)
        (temp_root / "crates/node/cardano-node").mkdir(parents=True, exist_ok=True)
        (temp_root / "crates/node/runtime/scripts").mkdir(parents=True, exist_ok=True)
        (temp_root / "crates/node/sync/configuration").mkdir(
            parents=True,
            exist_ok=True,
        )
        (temp_root / "crates/tools/cardano-cli").mkdir(parents=True, exist_ok=True)
        (temp_root / REFERENCE_TREE / ".git").mkdir(parents=True)
        (temp_root / REFERENCE_TREE / "deps/cardano-ledger/.git").mkdir(parents=True)
        (temp_root / REFERENCE_TREE / "deps/ouroboros-network").mkdir(parents=True)
        (
            temp_root / REFERENCE_TREE / "deps/ouroboros-network/.git"
        ).write_text("gitdir: ../../.git/modules/ouroboros-network\n", encoding="utf-8")
        directory_failures = stale_directory_failures(temp_root)
        for label in STALE_DIRECTORIES:
            if not any(label in failure for failure in directory_failures):
                failures.append(f"expected stale directory detector to flag {label!r}")
        if any(
            failure.startswith("crates/node/cardano-node:") for failure in directory_failures
        ):
            failures.append("current cardano-node crate directory should be allowed")
        if any(
            failure.startswith("crates/tools/cardano-cli:") for failure in directory_failures
        ):
            failures.append("current tools/cardano-cli directory should be allowed")
        node_artifact_failures = node_crate_operator_artifact_directory_failures(
            temp_root
        )
        for expected_path in (
            "crates/node/cardano-node/configuration",
            "crates/node/cardano-node/scripts",
            "crates/node/runtime/scripts",
            "crates/node/sync/configuration",
        ):
            if not any(
                failure.startswith(f"{expected_path}:")
                for failure in node_artifact_failures
            ):
                failures.append(
                    f"expected node-crate operator artifact detector to flag {expected_path!r}"
                )
        reference_failures = reference_git_metadata_failures(temp_root)
        if len(reference_failures) != 3:
            failures.append(
                "expected reference metadata detector to flag .git directories and files"
            )

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        sample_placements = {
            "node binary crate manifest": "crates/node/cardano-node/Cargo.toml",
            "mainnet operator preset": "configuration/mainnet/config.json",
            "root sister-tool launcher": "scripts/run-tools.sh",
            "root reference setup helper": "scripts/setup-reference.sh",
        }
        if len(required_current_placement_failures(temp_root, sample_placements)) != len(
            sample_placements
        ):
            failures.append("missing required current placements should be rejected")
        for relative_path in sample_placements.values():
            full_path = temp_root / relative_path
            full_path.parent.mkdir(parents=True, exist_ok=True)
            full_path.write_text("", encoding="utf-8")
        if required_current_placement_failures(temp_root, sample_placements):
            failures.append("present required current placements should be accepted")

    executable_mode_failures = root_shell_script_mode_failures(
        stage_output=(
            f"100644 {'0' * 40} 0\tscripts/run-tools.sh\0"
            f"100755 {'0' * 40} 0\tscripts/setup-reference.sh\0"
            f"100644 {'0' * 40} 0\tscripts/yggdrasil-node.service\0"
            f"100644 {'0' * 40} 0\tdocs/example.sh\0"
        ).encode("utf-8")
    )
    if not any("scripts/run-tools.sh" in failure for failure in executable_mode_failures):
        failures.append("non-executable root shell scripts should be rejected")
    if any("scripts/setup-reference.sh" in failure for failure in executable_mode_failures):
        failures.append("executable root shell scripts should be accepted")
    if any("scripts/yggdrasil-node.service" in failure for failure in executable_mode_failures):
        failures.append("non-shell root script artifacts should not require executable mode")
    if any("docs/example.sh" in failure for failure in executable_mode_failures):
        failures.append("non-root shell scripts should not be handled by placement guard")

    if root_shell_script_mode_failures(
        stage_output=(f"100755 {'0' * 40} 0\tscripts/run-tools.sh\0").encode(
            "utf-8"
        )
    ):
        failures.append("all-executable root shell script stage output should be accepted")

    if not root_shell_script_mode_failures(stage_output=b""):
        failures.append("missing tracked root shell scripts should be rejected")

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        sample_snippets = (
            (
                ".github/workflows/release.yml",
                "release workflow stages root configuration bundle",
                'cp -r configuration "$dir/configuration"',
            ),
            (
                "Dockerfile",
                "Docker image copies root configuration bundle",
                "COPY --from=builder /src/configuration /usr/share/yggdrasil/configuration",
            ),
        )
        if len(required_text_snippet_failures(temp_root, sample_snippets)) != len(
            sample_snippets
        ):
            failures.append("missing required packaging snippets should be rejected")
        for relative_path, _label, snippet in sample_snippets:
            full_path = temp_root / relative_path
            full_path.parent.mkdir(parents=True, exist_ok=True)
            full_path.write_text(f"{snippet}\n", encoding="utf-8")
        if required_text_snippet_failures(temp_root, sample_snippets):
            failures.append("present required packaging snippets should be accepted")

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / REFERENCE_TREE / "deps/cardano-ledger").mkdir(parents=True)
        if reference_git_metadata_failures(temp_root):
            failures.append("reference source tree without Git metadata should be allowed")

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / ".gitignore").write_text(
            f"**/{REFERENCE_TREE}/\n",
            encoding="utf-8",
        )
        if reference_gitignore_failures(temp_root, verify_with_git=False):
            failures.append("reference gitignore rule should be accepted")

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / ".gitignore").write_text("/target/\n", encoding="utf-8")
        if not reference_gitignore_failures(temp_root, verify_with_git=False):
            failures.append("missing reference gitignore rule should be rejected")

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / ".gitmodules").write_text(
            "\n".join(
                [
                    '[submodule ".reference-haskell-cardano-node"]',
                    f"\tpath = {REFERENCE_TREE}",
                    "\turl = https://github.com/IntersectMBO/cardano-node.git",
                ]
            ),
            encoding="utf-8",
        )
        if not reference_submodule_failures(temp_root, inspect_git_index=False):
            failures.append("reference submodule declaration should be rejected")

    reference_index_failures = reference_index_entry_failures(
        f"100644 {'0' * 40} 0\t{REFERENCE_TREE}/README.md"
    )
    if not any("must not enter the Git index" in failure for failure in reference_index_failures):
        failures.append("reference regular index entries should be rejected")

    reference_submodule_index_failures = reference_index_entry_failures(
        f"160000 {'0' * 40} 0\t{REFERENCE_TREE}"
    )
    if not any(
        "must not be tracked as a submodule" in failure
        for failure in reference_submodule_index_failures
    ):
        failures.append("reference submodule index entries should be rejected")

    unrelated_index_failures = reference_index_entry_failures(
        f"100644 {'0' * 40} 0\tdocs/external/README.md"
    )
    if unrelated_index_failures:
        failures.append("unrelated Git index entries should be allowed")

    with self_test_temp_directory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / ".gitmodules").write_text(
            "\n".join(
                [
                    '[submodule "docs"]',
                    "\tpath = docs/external",
                    "\turl = https://example.invalid/docs.git",
                ]
            ),
            encoding="utf-8",
        )
        if reference_submodule_failures(temp_root, inspect_git_index=False):
            failures.append("unrelated submodule declarations should be allowed")

    stale_metadata = {
        "workspace_members": [
            "path+file:///repo/crates/node/yggdrasil-node#yggdrasil-node@0.2.0",
        ],
        "packages": [
            {
                "name": "yggdrasil-node",
                "manifest_path": "/repo/crates/node/runtime/Cargo.toml",
                "targets": [],
            },
            {
                "name": "yggdrasil-node-config",
                "manifest_path": "/repo/crates/tools/config/Cargo.toml",
                "targets": [],
            },
            {
                "name": "yggdrasil-cardano-cli",
                "manifest_path": "/repo/crates/node/cardano-cli/Cargo.toml",
                "targets": [],
            },
            {
                "name": "old-tool",
                "manifest_path": "/repo/crates/cardano-cli/Cargo.toml",
                "targets": [
                    {"name": "old-node-main", "src_path": "/repo/node/src/main.rs"},
                ],
            },
        ],
    }
    metadata_failures = cargo_metadata_path_failures(stale_metadata)
    for expected_label in (
        "old node crate path",
        "old top-level cardano-cli crate path",
        "old top-level sister-tool crate path",
        "old node src path",
    ):
        if not any(expected_label in failure for failure in metadata_failures):
            failures.append(f"expected Cargo metadata detector to flag {expected_label!r}")
    for expected_bucket_failure in (
        "yggdrasil-node manifest must stay at",
        "yggdrasil-node-config must live under crates/node/",
        "yggdrasil-cardano-cli must live under crates/tools/",
    ):
        if not any(expected_bucket_failure in failure for failure in metadata_failures):
            failures.append(
                f"expected Cargo metadata bucket detector to flag {expected_bucket_failure!r}"
            )

    current_metadata = {
        "workspace_members": [
            "path+file:///repo/crates/node/cardano-node#yggdrasil-node@0.2.0",
            "path+file:///repo/crates/tools/cardano-cli#yggdrasil-cardano-cli@0.2.0",
        ],
        "packages": [
            {
                "name": "yggdrasil-node",
                "manifest_path": "/repo/crates/node/cardano-node/Cargo.toml",
                "targets": [
                    {
                        "name": "yggdrasil-node",
                        "src_path": "/repo/crates/node/cardano-node/src/main.rs",
                    },
                ],
            },
            {
                "name": "yggdrasil-node-config",
                "manifest_path": "/repo/crates/node/config/Cargo.toml",
                "targets": [
                    {
                        "name": "yggdrasil_node_config",
                        "src_path": "/repo/crates/node/config/src/lib.rs",
                    },
                ],
            },
            {
                "name": "yggdrasil-cardano-cli",
                "manifest_path": "/repo/crates/tools/cardano-cli/Cargo.toml",
                "targets": [
                    {
                        "name": "cardano-cli",
                        "src_path": "/repo/crates/tools/cardano-cli/src/main.rs",
                    },
                ],
            },
        ],
    }
    if cargo_metadata_path_failures(current_metadata):
        failures.append("current Cargo metadata paths should be allowed")

    unreleased = "\n".join(
        [
            "# Changelog",
            "",
            "## [Unreleased]",
            "- Current entry.",
            "",
            "## [0.2.0]",
            "- Historical entry.",
        ]
    )
    text, start_line = unreleased_changelog_text(unreleased)
    if "Current entry" not in text or "Historical entry" in text or start_line != 3:
        failures.append("unreleased changelog extraction did not isolate the current section")

    if failures:
        print("stale placement self-test failed:")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("stale placement self-test clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
