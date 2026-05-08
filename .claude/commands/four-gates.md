---
description: Run Yggdrasil's four required verification gates — fmt, check, lint, test.
allowed-tools: Bash(cargo fmt*), Bash(cargo check*), Bash(cargo check-all), Bash(cargo lint), Bash(cargo clippy*), Bash(cargo test*), Bash(cargo test-all)
---

Run the four required verification gates in order and report each
outcome:

```bash
cargo fmt --all -- --check       # rustfmt gate
cargo check-all                  # cargo check --workspace --all-targets
cargo lint                       # cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test-all                   # cargo test --workspace --all-features
```

If `cargo fmt --all -- --check` fails, run `cargo fmt --all` to fix
in-place, then re-check.

If `cargo lint` flags warnings, the workspace enforces `-D warnings`
— diagnose and fix; do not `#[allow(...)]` your way out.

For `cargo test-all`, report the aggregate `passed / failed / ignored`
count. The expected baseline as of R273h is **4,855 passed, 0 failed**.
A drop in test count requires diagnosis, not a commit.

Do not declare a round done until all four gates pass.
