# Lessons

- Start by restoring local parity infrastructure before making behavior claims. A green Rust build does not prove upstream parity when the Haskell reference tree is absent.
- Treat "complete" in project docs as evidence-scoped. Code-level implementation, upstream byte/wire evidence, and operator soak evidence are separate states.
- On Windows, byte-parity fixtures need explicit `.gitattributes` LF rules and current-checkout normalization before raw hash or `include_str!` assertions are meaningful.
- Use WSL/Linux for Haskell reference binaries, socket/operator evidence, and parity-run shell scripts; native Windows shell is only appropriate for Windows Rust gates or simple repository inspection.
- When WSL is available, run Linux-style shell work as `wsl -e bash -lc ...` and call out any native Windows exception explicitly; do not leave command provenance ambiguous.
- Do not use Git Bash or Windows-hosted Bash for repository shell helpers when WSL is available; run those helpers under WSL so the execution environment matches Linux operator/parity assumptions.
- If a command is Bash-shaped, run it through WSL by default; use native Windows only for explicit Windows gates, Git metadata operations, or simple file inspection.
- Before committing, verify repository-local `git config user.name` and `git config user.email`; commits must use `DaJo-Code` and `140243674+DaJo-Code@users.noreply.github.com`.
