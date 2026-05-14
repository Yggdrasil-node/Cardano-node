## Round 271c — `runtime.rs` per-domain split: third slice (LedgerJudgementSettings)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 third slice — micro-slice)

### Slice scope

Extracted 28 source lines from `runtime.rs::LedgerJudgementSettings`
(struct + Default impl) into a new `node/src/runtime/ledger_judgement.rs`
(45 lines). `runtime.rs` keeps a `pub mod ledger_judgement;`
declaration plus a `pub use ledger_judgement::LedgerJudgementSettings;`
re-export.

This is a focused, single-type slice — exactly what its upstream
analogue (`mkLedgerStateJudgement` configuration in
`Cardano.Node.Diffusion.Configuration`) is at upstream. Bundles the
three genesis-derived inputs (system start, slot length, max age) that
drive the live `LedgerStateJudgement` computation in
`ChainDbConsensusLedgerSource`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/ledger_judgement.rs::LedgerJudgementSettings` | upstream `mkLedgerStateJudgement` configuration record in `Cardano.Node.Diffusion.Configuration` (driven from `Ouroboros.Consensus.Genesis.Governor` thresholds) |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 7,020 | 6,995 | −25 |
| `node/src/runtime/ledger_judgement.rs` | (new) | 45 | +45 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

Clean first-try extraction. No fixup passes needed.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271b closure: `2026-05-06-round-271b-runtime-block-producer-config-extraction.md`
- Upstream mkLedgerStateJudgement: `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/Diffusion/Configuration.hs`
