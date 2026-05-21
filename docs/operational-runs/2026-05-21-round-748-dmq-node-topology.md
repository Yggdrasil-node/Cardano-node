---
title: "Round 748 dmq-node Topology configuration (dmq-node arc, slice 30)"
parent: Reference
---

# Round 748 dmq-node Topology configuration (dmq-node arc, slice 30)

Date: 2026-05-21

## Scope

Slice 30 of the dmq-node arc — the topology-file reader (strict
mirror of `DMQ/Configuration/Topology.hs`).

## What shipped

`crates/tools/dmq-node/src/topology.rs` — new file, strict mirror of
`DMQ/Configuration/Topology.hs`:

- `read_topology_file` — mirror of upstream `readTopologyFile`: reads
  the topology file and JSON-decodes it.
- `TopologyError` — the I/O / parse failure modes of upstream's
  `Either Text` result.

Upstream parses a `NetworkTopology NoExtraConfig NoExtraFlags` — the
standard `ouroboros-network` topology with the "no extra"
instantiation. yggdrasil reuses `crates/network`'s concrete
`TopologyConfig` (a complete, `serde`-backed port of that topology
schema); the `NoExtraConfig` / `NoExtraFlags` type parameters carry
no data and are dropped. New workspace-internal dependency
`yggdrasil-network` — needed here, and by the still-pending
runtime / diffusion integration layer.

`lib.rs` gains `pub mod topology;`.

3 unit tests: a missing file (I/O error), a valid topology, and
malformed JSON (parse error).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 118 lib (+3 vs R747's 115) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The client / server protocol drivers; the `NodeKernel` /
  `Diffusion/*` run-loop wiring; the NtN / NtC protocol bundles;
  `Tracer.hs`; the `SigSubmissionV2` protocol sub-tree.
