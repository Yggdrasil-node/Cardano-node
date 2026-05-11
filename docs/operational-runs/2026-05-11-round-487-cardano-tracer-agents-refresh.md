---
title: 'R487: cardano-tracer AGENTS.md refresh post-R474'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-487-cardano-tracer-agents-refresh/
---

# R487 — cardano-tracer AGENTS.md refresh

**Date:** 2026-05-11
**Scope:** documentation-only round.

## Slice scope

Closes documentation drift in `crates/tools/cardano-tracer/AGENTS.md`
left behind by the R460-R474 follow-on arc. The file's
**Status** field, **Current functional surface** matrix, and
**Round roadmap** all still reflected pre-R460 state.

Key updates:

1. **Status field**: `post-R411-R459 arc` → `post-R474 closeout`.
2. **Surface matrix entries flipped from ❌ to ✅:**
   - DataPoint sub-protocol forwarder side (R471-R473).
   - TLS termination via axum-server-rustls (R468).
   - Logs Rotator full IO orchestration (R461-R463).
   - runMetricsServers aggregator (R464).
   - per-connection HandleRegistry deregister hook (R465).
   - DataPointRequestors registry plumbing (R469-R470).
3. **Surviving carve-outs preserved**:
   - EKG ReqResp (synthesis carve-out — Hackage package not vendored).
   - Trace-forwarder handshake-over-socket codec (RemoteSocket TCP path).
   - TraceObject CBOR upstream-byte-equivalence (cardano-logging
     Hackage source not vendored).
   - RTView web UI (permanent — no Rust analog for ThreePenny GUI).
4. **Round roadmap** gains an R460-R474 follow-on arc bullet
   listing each delivered round.
5. **Build + run** paragraph drops the stale "concrete dispatch
   lands at R361+" language; replaces with a clear statement
   that the R411-R474 arc closure ships the operational binary +
   the TraceObject-CBOR caveat for upstream interop.

No source code touched.

## Tests delivered

None — documentation round. Test count unchanged at 6,181.

## Verification log

```
cargo fmt --all -- --check                                  clean
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Stop point

Three AGENTS.md files refreshed across this autonomous push:
db-analyser (R483) + db-truncater (R484) + cardano-tracer (R487).
The remaining sister-tool AGENTS.md files
(tx-generator, db-synthesizer, cardano-testnet, snapshot-converter,
kes-agent, kes-agent-control, dmq-node) still carry pre-implementation
language but that language is *accurate* about their state —
refreshing them requires the underlying mini-arc implementation
to ship first.
