# yggdrasil-node-ntc-server — Node-to-Client mini-protocol server

## Scope

Wave 5 PR 11 — NtC side of the local-socket surface that
`cardano-cli` and other operator tools call into. Hosts the
LocalStateQuery, LocalTxSubmission, and LocalTxMonitor session
machines plus the Unix-socket accept loop.

## Rules — Non-Negotiable

- **Operator-stable API surface.** The NtC mini-protocol surface
  is part of `docs/COMPATIBILITY.md` Tier-1 stability — cardano-cli
  + sister tools depend on it. Changes require semver-major.
- **No reverse dep on runtime/server orchestration outside this crate.**

## Naming parity

Synthesis crate. The lib.rs (former node/src/local_server.rs) carries
the `## Naming parity` stanza.

## R-arc tracking

Wave 5 PR 11.
