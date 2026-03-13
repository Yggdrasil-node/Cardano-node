# Contributing

## Required Checks
- `cargo check-all`
- `cargo test-all`
- `cargo lint`

## Expectations
- Preserve crate boundaries and avoid reaching across them casually.
- Keep error types explicit in library crates.
- Do not use `unwrap`, `dbg!`, or `todo!` in committed code.
- Add tests for new domain logic, especially in consensus, ledger, and crypto code.
- Keep `AGENTS.md`, `README.md`, and docs in sync with implemented behavior whenever milestones shift.

## Generated Code
- Treat generated files as outputs of `cddl-codegen`.
- Do not edit generated code by hand.
- Document the upstream spec revision used for regeneration.

## Unsafe Code
- Unsafe code is not expected during the foundation phase.
- Any future unsafe code must be isolated, justified with `SAFETY:` comments, and reviewed explicitly.
