---
title: Contributing
layout: default
parent: Reference
nav_order: 11
---

# Contributing

## Required Checks
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo test-all`
- `cargo lint`

CI ([.github/workflows/ci.yml](https://github.com/yggdrasil-node/Cardano-node/blob/main/.github/workflows/ci.yml)) runs all four; format drift is now a hard gate.

## Devcontainer Networking
- NtN/P2P uses TCP. Bind listener rehearsals to `0.0.0.0:<port>` when another container or host process must connect; `127.0.0.1:<port>` is only for in-container loopback tests.
- NtC uses a Unix socket, not a TCP port. The devcontainer exports `CARDANO_NODE_SOCKET_PATH=/workspaces/Cardano-node/tmp/preview-producer/run/preview-producer.sock` so `cardano-cli` and `yggdrasil-node query` can target the preview producer by default.
- Forwarded development ports are `3001` for the standard NtN peer port, `13001` for the local preview relay P2P rehearsal, `12798` for docker-compose metrics, `19002` for the preview producer metrics, and the runbook metrics ports `9001`, `9099`, and `9101`.
- Keep socket paths under the workspace or `/tmp` so they are accessible to tools launched inside the container. For host-side Cardano tools, prefer running them inside the devcontainer unless the host OS reliably supports Unix sockets on the workspace mount.

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
