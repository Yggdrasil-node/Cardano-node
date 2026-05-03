# Guidance for `.claude/` — Claude Code harness configuration.

## Scope
- `.claude/settings.json` — project-scoped Claude Code settings (committed). Loads after `~/.claude/settings.json` and is overridden by `.claude/settings.local.json` (gitignored, not present in this repo).
- `.claude/hooks/session-start.sh` — `SessionStart` hook that provisions the toolchain and pre-fetches workspace deps in Claude Code on the web.

This directory is **harness configuration, not source code.** It is not a Rust crate, has no `Cargo.toml`, and is excluded from `cargo *` aliases.

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

## Out of scope
- The four required verification gates themselves live in `Cargo.toml` workspace aliases and `.github/workflows/ci.yml`. Don't reimplement them as hooks here.
- The rolling parity journal lives in the root `AGENTS.md`. Don't mirror status updates into this file.
- Other AI-assistant configurations (`.codex/`, `.github/copilot-instructions.md`, etc.) are out of scope here — each tool's directory owns its own conventions.
