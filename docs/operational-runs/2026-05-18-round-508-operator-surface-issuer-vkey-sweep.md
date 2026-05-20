# Round 508 — operator-surface sweep: purge the removed issuer-vkey flag

**Date:** 2026-05-18
**Area:** operator scripts + manual — `scripts/`, `docs/`
**Upstream reference:** follow-up to round 507 (A3 R3a slice 3).

## Summary

Round 507 removed the `--shelley-operational-certificate-issuer-vkey` CLI
flag and the `ShelleyOperationalCertificateIssuerVkey` config key. This
round purges every remaining operator-facing reference to them. Three
producer scripts still passed the now-nonexistent flag — the node's
`clap` parser rejects an unknown flag, so the producer harness could no
longer start a node. Seven docs still instructed operators to use the
removed flag / config key. All are corrected.

## Scope

Found via repo-wide grep for `shelley-operational-certificate-issuer-vkey`,
`ShelleyOperationalCertificateIssuerVkey`, and `ISSUER_VKEY_PATH`.

Scripts (functionally broken by R507 — `clap` rejects the unknown flag):

- `run_preprod_real_pool_producer.sh`, `run_mainnet_real_pool_producer.sh`
  — dropped the `ISSUER_VKEY_PATH` env var, its `require_file` guard, its
  usage-block line, and the `--shelley-operational-certificate-issuer-vkey`
  invocation argument.
- `preview_producer_harness.sh` — dropped the `--arg issuer` jq binding
  and the `ShelleyOperationalCertificateIssuerVkey` config-emit line.

Docs (stale operator instructions):

- `docs/manual/{configuration,cli-reference,block-production,docker,
  troubleshooting}.md` — removed the config-key and CLI-flag table rows,
  the config-JSON and CLI examples, and the docker-compose arg; corrected
  "four credentials" → "three"; rewrote the `block-production.md`
  startup-verification step and the `troubleshooting.md` OpCert section to
  describe the new behavior (the issuer key is the opcert's embedded cold
  vkey; the signature check is an opcert internal-consistency check).
- `docs/MANUAL_TEST_RUNBOOK.md`, `docs/REAL_PREPROD_POOL_VERIFICATION.md`
  — removed the `ISSUER_VKEY_PATH` env-var lines and the cold-vkey
  prerequisite mentions.

`docs/COMPLETION_ROADMAP.md`'s A3 R3a section retains its `issuer_vkey_path`
mentions — those correctly describe completed work, not stale instructions.

## Verification

- Repo-wide grep: zero residual `shelley-operational-certificate-issuer-vkey`
  / `ShelleyOperationalCertificateIssuerVkey` / `ISSUER_VKEY_PATH` references
  outside the roadmap's plan context.
- `bash -n` syntax check passes on all three edited scripts.
- Doc/script-only round — no cargo gates apply; the Rust workspace was not
  touched, so round 507's four-gate baseline (6,529 pass / 0 fail) stands.

## Notes

Round 507 framed this follow-up as "doc-only"; grounding showed it also had
to fix the three producer scripts — a functional break, not just doc drift.
A3 R3a remains complete (slices 1–3); the next protocol round is R3b
(consensus config — `Run.initProtocol` / `mkConsensusProtocolCardano`).
