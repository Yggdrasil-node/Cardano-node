# Round 243 - cardano-ledger import-only pin refresh

Date: 2026-05-01
Phase: E.1 upstream pin maintenance
Scope: documentary upstream pin refresh, no ledger/runtime behavior change

### Summary

`node/scripts/check_upstream_drift.sh` found one new drift after the
R239 fixture refresh:

```text
cardano-ledger          DRIFT    42d088ed84b7 -> 110b30e7abd8
```

The official IntersectMBO comparison from
`42d088ed84b799d6d980f9be6f14ad953a3c957d` to
`110b30e7abd8f507ea21625f8ac06fb6c8b66768` contains two commits and
one file change. The merge commit is PR #5787, "Remove redudant
import", and the only source delta removes an unused
`Cardano.Ledger.Shelley.Rules ()` import from
`eras/shelley/impl/src/Cardano/Ledger/Shelley/API/Mempool.hs`.

No ledger rule, CDDL schema, binary codec, or exported behavior changed
in the Yggdrasil ported subset. This round therefore advances only the
documentary pin and refreshes the living parity docs.

### Changes

- `UPSTREAM_CARDANO_LEDGER_COMMIT` now points at
  `110b30e7abd8f507ea21625f8ac06fb6c8b66768`.
- `docs/UPSTREAM_PARITY.md`, `docs/PARITY_PROOF.md`,
  `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `README.md`, and
  root `AGENTS.md` now describe the R243 refresh.

### Verification

Commands run during this slice:

```sh
cargo check-all
cargo test-all
cargo lint
node/scripts/check_upstream_drift.sh
```

Drift result after the pin advance:

```text
[upstream-drift] timestamp=2026-05-01T11:57:15Z
  repo                    status   pinned -> live
  cardano-base            in-sync  7a8a991945d4
  cardano-ledger          in-sync  110b30e7abd8
  ouroboros-consensus     in-sync  b047aca4a731
  ouroboros-network       in-sync  0e84bced45c7
  plutus                  in-sync  4cd40a14e364
  cardano-node            in-sync  799325937a45

drifted=0 unreachable=0 total=6
```

### Upstream references

- Compare:
  <https://github.com/IntersectMBO/cardano-ledger/compare/42d088ed84b799d6d980f9be6f14ad953a3c957d...110b30e7abd8f507ea21625f8ac06fb6c8b66768>
- Merge commit:
  <https://github.com/IntersectMBO/cardano-ledger/commit/110b30e7abd8f507ea21625f8ac06fb6c8b66768>

### Status impact

- All 6 canonical upstream pins are back in sync with live HEAD.
- No code-level parity blocker is opened by this upstream delta.
- Operator-time gates remain unchanged: runbook §6.5 BlockFetch
  sign-off and the §2-9 mainnet endurance rehearsal before changing
  production defaults.
