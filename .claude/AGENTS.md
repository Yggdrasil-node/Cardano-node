# Guidance for `.claude/` — Claude Code harness configuration.

## Scope
- `.claude/settings.json` — project-scoped Claude Code settings (committed). Loads after `~/.claude/settings.json` and is overridden by `.claude/settings.local.json` (gitignored, not present in this repo).
- `.claude/hooks/session-start.sh` — `SessionStart` hook that provisions the toolchain and pre-fetches workspace deps in Claude Code on the web.
- `.claude/agents/*.md` — sub-agent definitions (Markdown with YAML frontmatter `name`, `description`, optional `tools`).
- `.claude/skills/<name>/SKILL.md` — skill definitions (YAML frontmatter `name`, `description`, optional `allowed-tools`).
- `.claude/commands/*.md` — slash-command definitions for parity / filetree gates.
- `.claude/scripts/filetree.py` — stdlib-only filetree manifest tool (mirrors the Codex example; reads `.claude/filetree/manifest.json` + writes `.claude/filetree/FILETREE.md`).
- `.claude/filetree/{manifest.json,FILETREE.md}` — generated filetree state (regenerate via the script; do not hand-edit `FILETREE.md`).

This directory is **harness configuration, not source code.** It is not a Rust crate, has no `Cargo.toml`, and is excluded from `cargo *` aliases.

## Subagents

Defined as Markdown with YAML frontmatter under `.claude/agents/`:

- [`haskell-reference-auditor.md`](agents/haskell-reference-auditor.md) — read-heavy specialist for mapping Yggdrasil's Rust implementation to the IntersectMBO/cardano-node Haskell reference. Delegate before claiming parity, before recommending implementation, or when a fix needs upstream evidence cited.
- [`round-extractor.md`](agents/round-extractor.md) — filename-mirror extraction specialist for one R-arc round (R271-style runtime split, R273-style subsystem split). Reads `round-extraction` skill for the recipe; will not invent sub-module names that don't mirror upstream.
- [`filetree-reviewer.md`](agents/filetree-reviewer.md) — focused maintainer for stale `.claude/filetree` descriptions. Restricts writes to the manifest + rendered output.

## Skills

Defined as `.claude/skills/<name>/SKILL.md`:

- [`continuous-agent-loop/SKILL.md`](skills/continuous-agent-loop/SKILL.md) — Yggdrasil's R-arc round-by-round development rhythm (one slice per round, four cargo gates green between rounds, one operational-runs doc, "proceed" cadence).
- [`round-extraction/SKILL.md`](skills/round-extraction/SKILL.md) — recipe for splitting an oversized `.rs` file into upstream-aligned sub-modules. Encodes empirically-confirmed patterns from R271a–s and R273a–h.
- [`parity-plan/SKILL.md`](skills/parity-plan/SKILL.md) — author a parity plan before substantive Yggdrasil code edits (CBOR shape, hash input, signature domain, ledger predicate, Plutus budget, network framing).
- [`cardano-filetree-maintainer/SKILL.md`](skills/cardano-filetree-maintainer/SKILL.md) — invoke for filetree maintenance work flagged by `python3 .claude/scripts/filetree.py check`.

- [`cardano-haskell-node/SKILL.md`](skills/cardano-haskell-node/SKILL.md) — operator reference for upstream Haskell `cardano-node` stake-pool administration. Use for Haskell-node operations, not Yggdrasil Rust implementation or file-mirror parity work.

## Slash commands

Under `.claude/commands/`:

- `/four-gates` — runs `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo test-all` and reports outcomes.
- `/parity-check` — runs `python3 scripts/check-parity-matrix.py`.
- `/filetree-check` — runs `python3 .claude/scripts/filetree.py check`.
- `/parity-plan <feature>` — authors a parity plan before substantive code edits.
- `/round-doc <round-id> <slug>` — authors `docs/operational-runs/YYYY-MM-DD-round-NNN-<slug>.md` for the just-completed round.
- `/setup-reference` — runs `bash scripts/setup-reference.sh` to refresh `.reference-haskell-cardano-node/` to the policy IntersectMBO tag.

## Rules *Non-Negotiable*
- The four required verification gates (`cargo fmt --all -- --check`, `cargo check-all`, `cargo test-all`, `cargo lint`) MUST stay runnable from a fresh web session without manual setup. Any change here that breaks the cold-start path for those gates is a regression.
- `session-start.sh` MUST stay gated on `$CLAUDE_CODE_REMOTE` so local sessions are unaffected. If the gate is removed, contributors with their own toolchain managers (e.g. `direnv`, `nix`) will have their environment clobbered.
- `session-start.sh` MUST be idempotent and non-interactive — it can be re-run on resume/clear/compact and runs without a TTY.
- The hook MUST NOT run cargo commands that mutate the source tree, write into `target/`, or invoke a network-dependent compile. Pre-fetching the registry via `cargo fetch --locked` is fine; `cargo build` / `cargo test` here would race with the agent loop.
- Permissions added to `permissions.allow` MUST be **read-only or build-related** patterns. Never allow-list `Bash(git push *)`, `Bash(git reset *)`, `Bash(git checkout *)`, `Bash(rm *)`, or anything that mutates remote/shared state — those should remain prompted per the harness's "executing actions with care" rules.
- Hook commands that emit JSON to stdout (e.g. the `Stop` hook) MUST emit a single well-formed JSON object on the first non-empty line. Garbled JSON silently disables the hook.

## Conventions
- **Async preamble**: `session-start.sh` opts into async via `echo '{"async": true, "asyncTimeout": 300000}'` as its first executable line, NOT via `"async": true` in `settings.json`. The script-controlled form lets us tune `asyncTimeout` (5 minutes covers cold-start `cargo fetch` on a slow registry mirror).
- **Allow-list scope**: `Bash(cargo *)` patterns cover the four verification gates and common read-only diagnostics (`metadata`, `tree`). `Bash(git status*)` / `git diff*` / `git log*` / `git show*` / `git branch` are the only git rules — write-side git commands stay prompted.
- **Env vars**: `CARGO_TERM_COLOR=always` and `RUST_BACKTRACE=1` are session-scoped via `env` in `settings.json`. Persisted shell exports go through `$CLAUDE_ENV_FILE` inside the hook script, not the `env` block.
- **Stop hook**: re-prints the four-gate verification reminder on every stop. It is informational only — it does NOT set `continue: false` and does NOT block the agent.
- New hooks SHOULD be added under `.claude/hooks/` with one script per hook, registered by path in `settings.json`. Inline commands in `settings.json` are fine for one-liners (like the current `Stop` hook); anything multi-line becomes a script.

## When to update this folder
- Adding a new system dependency the workspace requires → update `session-start.sh` (and document why in the comment block at the top).
- Adding a new commonly-used cargo subcommand (e.g. `cargo nextest`, `cargo deny`) → consider adding to `permissions.allow`.
- Adding a new hook → add a script under `hooks/`, register in `settings.json`, validate with `jq -e '.hooks.<event>[] | .hooks[] | .command' .claude/settings.json`, and document the hook's contract in the section above.
- After any change here, re-run `CLAUDE_CODE_REMOTE=true ./.claude/hooks/session-start.sh` from a clean shell to confirm the cold-start path still works.

## Parity-flow gates

The parity-flow gates live outside this directory but are surfaced by it:

- `python3 scripts/check-parity-matrix.py` — validates `docs/parity-matrix.json` schema, on-disk paths, and `.reference-haskell-cardano-node/REFERENCE_TAG`. The reference tag (`reference.tag`) MUST match the latest IntersectMBO/cardano-node release; bump in lockstep with `scripts/setup-reference.sh` (`CARDANO_NODE_VERSION`) and `scripts/check-parity-matrix.py` (`REFERENCE_TAG`).
- `python3 .claude/scripts/filetree.py check` — flags stale or missing description entries.
- `bash scripts/setup-reference.sh [--force]` — refreshes `.reference-haskell-cardano-node/` to the policy tag.

When a new upstream release ships:

1. Update `CARDANO_NODE_VERSION` in `scripts/setup-reference.sh`.
2. Update `REFERENCE_TAG` and `ALLOWED_STATUS` in `scripts/check-parity-matrix.py`.
3. Update `reference.tag` in `docs/parity-matrix.json` and re-validate every `haskell_reference.path` (paths can move across releases).
4. Update prose mentions in `CLAUDE.md`, root `AGENTS.md`, and the policy memory at `~/.claude/projects/.../memory/intersectmbo_version_policy.md`.
5. Run `bash scripts/setup-reference.sh --force` to re-fetch the reference tree.

## Out of scope
- The four required verification gates themselves live in `Cargo.toml` workspace aliases and `.github/workflows/ci.yml`. Don't reimplement them as hooks here.
- The rolling parity journal lives in the root `AGENTS.md`. Don't mirror status updates into this file.
- Other AI-assistant configurations (e.g. `referance-codex-examples/`, `.github/copilot-instructions.md`) are out of scope here — each tool's directory owns its own conventions.
