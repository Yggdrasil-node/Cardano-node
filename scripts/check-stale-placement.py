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
three-command node-wrapper cardano-cli subset wording, and the closed
workspace-member gap.

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
import tempfile
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REFERENCE_TREE = ".reference-haskell-cardano-node"

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

    with tempfile.TemporaryDirectory() as temp_dir:
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

    with tempfile.TemporaryDirectory() as temp_dir:
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

    with tempfile.TemporaryDirectory() as temp_dir:
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

    with tempfile.TemporaryDirectory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / REFERENCE_TREE / "deps/cardano-ledger").mkdir(parents=True)
        if reference_git_metadata_failures(temp_root):
            failures.append("reference source tree without Git metadata should be allowed")

    with tempfile.TemporaryDirectory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / ".gitignore").write_text(
            f"**/{REFERENCE_TREE}/\n",
            encoding="utf-8",
        )
        if reference_gitignore_failures(temp_root, verify_with_git=False):
            failures.append("reference gitignore rule should be accepted")

    with tempfile.TemporaryDirectory() as temp_dir:
        temp_root = Path(temp_dir)
        (temp_root / ".gitignore").write_text("/target/\n", encoding="utf-8")
        if not reference_gitignore_failures(temp_root, verify_with_git=False):
            failures.append("missing reference gitignore rule should be rejected")

    with tempfile.TemporaryDirectory() as temp_dir:
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

    with tempfile.TemporaryDirectory() as temp_dir:
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
