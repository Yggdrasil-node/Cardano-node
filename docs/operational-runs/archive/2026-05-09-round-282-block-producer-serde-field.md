# Round 282 — `block_producer.rs` serde-required field

**Date:** 2026-05-09
**Phase:** C (tech-debt purge)
**Predecessor:** R281 (`docs/operational-runs/2026-05-09-round-281-sweeper-naming-parity.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Resolve the `#[allow(dead_code)]` annotation on `TextEnvelope::description`
in `node/src/block_producer.rs`. Phase C first round.

## Investigation

The `TextEnvelope` struct mirrors upstream
`Cardano.Api.SerialiseTextEnvelope::TextEnvelope`. Upstream's
`FromJSON` instance reads three required fields:

```haskell
instance FromJSON TextEnvelope where
  parseJSON = withObject "TextEnvelope" $ \v ->
    TextEnvelope
      <$> (v .: "type")
      <*> (v .: "description")
      <*> (parseJSONBase16 =<< v .: "cborHex")
```

Yggdrasil's block-producer reads the file via `serde_json::from_str`
and uses only the `type` (validated against expected key tag) and
`cborHex` (decoded into the actual key bytes). The free-text
`description` is purely informational upstream — operator-supplied
annotation like "Stake pool operator key" — and Yggdrasil has no
production use for it.

`grep "envelope.description\|.description" node/src/block_producer.rs`
returned 0 matches. The field was deserialized purely to satisfy a
schema-completeness intuition that turns out not to apply.

## Resolution

Drop the `description: String` field from the Rust struct entirely.
Serde's default behavior is to silently ignore unknown JSON keys, so
upstream-produced text envelopes (which always carry `description`)
continue to deserialize cleanly with the trimmed two-field struct.

The struct's documentation comment now explicitly notes that the
field is intentionally absent and explains why.

```rust
/// Standard Cardano text-envelope format used for signing key and
/// certificate files.
///
/// Reference: `Cardano.Api.SerialiseTextEnvelope`. Upstream's
/// `TextEnvelope` carries three fields: `type` / `description` /
/// `cborHex`. Yggdrasil's block-producer never inspects the operator-
/// supplied free-text `description` (it's informational), so the
/// field is intentionally absent here. Serde silently ignores the
/// JSON key during deserialization, preserving wire-format compatibility
/// with upstream-produced envelopes.
#[derive(serde::Deserialize)]
struct TextEnvelope {
    #[serde(rename = "type")]
    type_tag: String,
    #[serde(rename = "cborHex")]
    cbor_hex: String,
}
```

The `#[allow(dead_code)]` annotation is gone. No callers needed updates
(the field had no readers).

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 3.64s)
cargo lint                          clean (Finished `dev` profile in 8.55s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
```

The text-envelope test fixture in `make_text_envelope` continues to
pass — it generates a JSON envelope with a `description` field that
Yggdrasil's deserializer correctly skips.

## Production `#[allow(dead_code)]` site count

| Site | Pre-R282 | Post-R282 | Round |
|---|---|---|---|
| `block_producer.rs::TextEnvelope::description` | 1 | 0 | R282 ✅ |
| `sync.rs::mod era_tag` | 1 | 1 | R283 |
| `reconnecting.rs::_runstate_impl_marker` | 1 | 1 | R286 |
| `peer_management.rs` × 5 (Phase 6 scaffolding) | 5 | 5 | R285 |
| `shelley.rs::mk_txout` test helper | 1 | 1 | R286 |
| **TOTAL production** | 9 | 8 | |

R282 reduces the production-side `#[allow(dead_code)]` count from 9
to 8. R283/R285/R286 close the remainder.

## Diff stat

```text
node/src/block_producer.rs  -3 lines (drop field + allow + clarify docstring)
docs/operational-runs/2026-05-09-round-282-... (new)
```

## Stop point — Phase C started

| Round | Site | Status |
|---|---|---|
| **R282** | `block_producer.rs::description` | ✅ closed |
| R283 | `sync.rs::mod era_tag` wiring | next |
| R284 | `local_server.rs:713` LSQ TODO | pending |
| R285 | `peer_management.rs` Phase 6 wiring | pending |
| R286 | `reconnecting.rs` marker + `shelley.rs` test helper | pending |
| R287 | `code-audit.md` + `REFACTOR_BLUEPRINT.md` re-grade | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R281 (`docs/operational-runs/2026-05-09-round-281-sweeper-naming-parity.md`)
- Upstream `TextEnvelope`:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Api/SerialiseTextEnvelope.hs`
