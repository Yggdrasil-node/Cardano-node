<!--
Thanks for contributing to Yggdrasil. Fill in the sections below.
Strike out (with ~~tildes~~) any sections that don't apply.
-->

## Summary

<!-- One paragraph: what does this PR change, and why? -->

## Upstream parity reference

<!--
If this PR touches consensus / network / ledger / Plutus / cryptography,
link to the upstream IntersectMBO module(s) it mirrors. If parity is not
applicable (operator UX, docs, scripts), say so.
-->

## Type of change

- [ ] Bug fix (non-breaking, fixes an issue)
- [ ] Feature (non-breaking, adds functionality)
- [ ] Breaking change (existing operators must adjust config or recompile)
- [ ] Documentation only
- [ ] Tooling / CI / release infrastructure

## Verification

Required gates:

- [ ] `cargo check-all` passes
- [ ] `cargo test-all` passes (note any test count delta)
- [ ] `cargo lint` clean
- [ ] `cargo doc --workspace --no-deps` warning-free

If touching docs:

- [ ] `docs/manual/` chapters still match the live binary surface (CLI flags, config keys, metric names)
- [ ] Site builds locally (`cd docs && bundle exec jekyll build`) — or the Pages workflow run is green

If touching the network / consensus / ledger surface:

- [ ] No new entries in workspace-denied lints (`unwrap_used`, `todo!`, `dbg_macro`)
- [ ] No new dependencies, OR each new dep justified in `docs/DEPENDENCIES.md`
- [ ] `node/scripts/check_upstream_drift.sh` still clean
- [ ] Upstream pinned SHAs in `node/src/upstream_pins.rs` updated if relevant

## AGENTS.md updates

- [ ] Updated relevant `**/AGENTS.md` files to reflect changes (per the workspace contract)

## Test plan

<!--
What did you do to convince yourself this works?
For consensus / wire-format changes, mention any cross-implementation testing.
-->

## Related issues / PRs

<!-- Closes #..., relates to #..., supersedes #... -->
