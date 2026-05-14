# Archived operational-run logs

Historical engineering-iteration logs from rounds **R151–R298** of the
Yggdrasil port effort, retained for forensic reference.

## Archive policy

Logs land here under either of these conditions:

- **R-number ≤ 298** AND no remaining production-source reference
  (`.rs` / `.sh` / `.py` / `.yml` / `.toml`). Markdown back-references
  from `AGENTS.md`, `docs/ARCHITECTURE.md`, and `docs/PARITY_PROOF.md`
  have been rewritten to point here.
- Or a deliberate later-round retire (rare; logs that proved a one-shot
  experiment with no carry-over signal).

Logs **stay at `docs/operational-runs/*.md`** (top level) when:

- R-number > 298 (recent rounds), OR
- A production source file (`.rs`/`.sh`/`.py`/`.yml`/`.toml`) cites the
  filename verbatim — usually in a `//!` doc-comment naming the
  forensic-evidence round behind a particular invariant.

Total archived in this batch: **161 files** across rounds R151–R298.

## Index by R-number range

### R151–R174 (2026-04-27 → 2026-04-28, 24 files)

- [R151](./2026-04-27-round-151-chainsync-pool-wiring.md) — chainsync pool wiring
- [R152](./2026-04-27-round-152-cardano-cli-tip-parity.md) — cardano cli tip parity
- [R153](./2026-04-27-round-153-network-aware-interpreter.md) — network aware interpreter
- [R154](./2026-04-27-round-154-era-pv-transition-signal.md) — era pv transition signal
- [R155](./2026-04-27-round-155-tx-size-fee-parity.md) — tx size fee parity
- [R156](./2026-04-27-round-156-pparams-query-parity.md) — pparams query parity
- [R157](./2026-04-28-round-157-utxo-query-parity.md) — utxo query parity
- [R158](./2026-04-28-round-158-tx-mempool-parity.md) — tx mempool parity
- [R159](./2026-04-28-round-159-alonzo-pparams.md) — alonzo pparams
- [R160](./2026-04-28-round-160-babbage-pparams-pv-era.md) — babbage pparams pv era
- [R161](./2026-04-28-round-161-conway-pparams.md) — conway pparams
- [R162](./2026-04-28-round-162-era-history-coverage.md) — era history coverage
- [R163](./2026-04-28-round-163-stake-query-dispatchers.md) — stake query dispatchers
- [R164](./2026-04-28-round-164-cumulative-parity-sweep.md) — cumulative parity sweep
- [R165](./2026-04-28-round-165-sync-speed.md) — sync speed
- [R166](./2026-04-28-round-166-rollback-recovery-fix.md) — rollback recovery fix
- [R167](./2026-04-28-round-167-mid-sync-rollback-epoch-fixup.md) — mid sync rollback epoch fixup
- [R168](./2026-04-28-round-168-bootstrap-peer-metric.md) — bootstrap peer metric
- [R169](./2026-04-28-round-169-current-era-metric.md) — current era metric
- [R170](./2026-04-28-round-170-per-era-block-counters.md) — per era block counters
- [R171](./2026-04-28-round-171-stake-pool-params-tag14.md) — stake pool params tag14
- [R172](./2026-04-28-round-172-pool-state-tag17.md) — pool state tag17
- [R173](./2026-04-28-round-173-stake-snapshots-tag18.md) — stake snapshots tag18
- [R174](./2026-04-28-round-174-decoder-strictness-fixes.md) — decoder strictness fixes

### R175–R199 (2026-04-28 → 2026-04-30, 25 files)

- [R175](./2026-04-28-round-175-registry-cooling-completeness.md) — registry cooling completeness
- [R176](./2026-04-28-round-176-decoder-strictness-cleanup.md) — decoder strictness cleanup
- [R177](./2026-04-28-round-177-filtered-delegations-fixes.md) — filtered delegations fixes
- [R178](./2026-04-28-round-178-era-floor-env-var.md) — era floor env var
- [R179](./2026-04-29-round-179-era-blockage-end-to-end.md) — era blockage end to end
- [R180](./2026-04-29-round-180-conway-governance-queries.md) — conway governance queries
- [R181](./2026-04-30-round-181-drep-state-map-shape.md) — drep state map shape
- [R182](./2026-04-30-round-182-committee-members-state.md) — committee members state
- [R183](./2026-04-30-round-183-future-pparams.md) — future pparams
- [R184](./2026-04-30-round-184-drep-spo-stake-distr.md) — drep spo stake distr
- [R185](./2026-04-30-round-185-proposals-default-vote.md) — proposals default vote
- [R186](./2026-04-30-round-186-stake-deleg-deposits-pool-distr2.md) — stake deleg deposits pool distr2
- [R187](./2026-04-30-round-187-ratify-state.md) — ratify state
- [R188](./2026-04-30-round-188-gov-state.md) — gov state
- [R189](./2026-04-30-round-189-ledger-peer-snapshot.md) — ledger peer snapshot
- [R190](./2026-04-30-round-190-comprehensive-audit.md) — comprehensive audit
- [R191](./2026-04-30-round-191-live-tip-slot-plumbing.md) — live tip slot plumbing
- [R192](./2026-04-30-round-192-chain-dep-state-context.md) — chain dep state context
- [R193](./2026-04-30-round-193-gov-relation-live.md) — gov relation live
- [R194](./2026-04-30-round-194-stake-distributions-live.md) — stake distributions live
- [R195](./2026-04-30-round-195-ledger-peer-pools-live.md) — ledger peer pools live
- [R196](./2026-04-30-round-196-ocert-sidecar-load.md) — ocert sidecar load
- [R197](./2026-04-30-round-197-nonce-sidecar-codec.md) — nonce sidecar codec
- [R198](./2026-04-30-round-198-nonce-sidecar-persist.md) — nonce sidecar persist
- [R199](./2026-04-30-round-199-200-multipeer-verified-and-apply-histogram.md) — 200 multipeer verified and apply histogram

### R201–R224 (2026-04-30 → 2026-04-30, 20 files)

- [R201](./2026-04-30-round-201-pin-refresh.md) — pin refresh
- [R202](./2026-04-30-round-202-stake-snapshots-infra.md) — stake snapshots infra
- [R203](./2026-04-30-round-203-stake-snapshots-sidecar.md) — stake snapshots sidecar
- [R204](./2026-04-30-round-204-gov-action-state-shape-adapter.md) — gov action state shape adapter
- [R205](./2026-04-30-round-205-comprehensive-verification.md) — comprehensive verification
- [R208](./2026-04-30-round-208-mainnet-boot-smoke.md) — mainnet boot smoke
- [R210](./2026-04-30-round-210-mainnet-stall-diagnostic.md) — mainnet stall diagnostic
- [R211](./2026-04-30-round-211-mainnet-byron-ebb-hash-fix.md) — mainnet byron ebb hash fix
- [R212](./2026-04-30-round-212-mainnet-cardano-cli-verification.md) — mainnet cardano cli verification
- [R213](./2026-04-30-round-213-mux-egress-singlemsg-allow.md) — mux egress singlemsg allow
- [R214](./2026-04-30-round-214-getgenesisconfig-encoder.md) — getgenesisconfig encoder
- [R215](./2026-04-30-round-215-multinetwork-post-r214-regression.md) — multinetwork post r214 regression
- [R216](./2026-04-30-round-216-pin-refresh-r2.md) — pin refresh r2
- [R217](./2026-04-30-round-217-fetch-batch-histogram.md) — fetch batch histogram
- [R218](./2026-04-30-round-218-mainnet-multipeer-fetch-rate.md) — mainnet multipeer fetch rate
- [R220](./2026-04-30-round-220-server-tip-envelope-fix.md) — server tip envelope fix
- [R221](./2026-04-30-round-221-chainprovider-tip-point-split.md) — chainprovider tip point split
- [R222](./2026-04-30-round-222-peer-lifetime-stats-foundation.md) — peer lifetime stats foundation
- [R223](./2026-04-30-round-223-peer-lifetime-stats-wiring.md) — peer lifetime stats wiring
- [R224](./2026-04-30-round-224-peer-lifetime-bytes-in.md) — peer lifetime bytes in

### R225–R249 (2026-05-01 → 2026-05-05, 17 files)

- [R225](./2026-05-01-round-225-rollback-depth-histogram.md) — rollback depth histogram
- [R226](./2026-05-01-round-226-peer-lifetime-unique-handshakes.md) — peer lifetime unique handshakes
- [R234](./2026-05-01-round-234-blockfetch-server-bytes-out.md) — blockfetch server bytes out
- [R236](./2026-05-01-round-236-stake-distribution-live-pooldistr.md) — stake distribution live pooldistr
- [R237](./2026-05-01-round-237-pooldistr2-egress-rollback.md) — pooldistr2 egress rollback
- [R238](./2026-05-01-round-238-rollback-sidecar-hardening.md) — rollback sidecar hardening
- [R239](./2026-05-01-round-239-cardano-base-fixture-refresh.md) — cardano base fixture refresh
- [R240](./2026-05-01-round-240-parallel-blockfetch-soak-automation.md) — parallel blockfetch soak automation
- [R241](./2026-05-01-round-241-devcontainer-preprod-blockfetch-smoke.md) — devcontainer preprod blockfetch smoke
- [R242](./2026-05-01-round-242-upstream-cardano-node-tests-harness.md) — upstream cardano node tests harness
- [R243](./2026-05-01-round-243-cardano-ledger-pin-refresh.md) — cardano ledger pin refresh
- [R244](./2026-05-01-round-244-byron-genesis-hash.md) — byron genesis hash
- [R245](./2026-05-01-round-245-cardano-ledger-bbody-gov-refresh.md) — cardano ledger bbody gov refresh
- [R246](./2026-05-02-round-246-preview-plutus-well-formedness-parity.md) — preview plutus well formedness parity
- [R247](./2026-05-02-round-247-origin-blockfetch-prefix.md) — origin blockfetch prefix
- [R248](./2026-05-02-round-248-tpraos-overlay-vrf.md) — tpraos overlay vrf
- [R249](./2026-05-05-round-249-cumulative-pin-refresh.md) — cumulative pin refresh

### R258–R274 (2026-05-06 → 2026-05-09, 53 files)

- [R258](./2026-05-06-round-258-multipeer-default-graduation.md) — multipeer default graduation
- [R259](./2026-05-06-round-259-tpraos-overlay-vrf-diagnostics.md) — tpraos overlay vrf diagnostics
- [R260](./2026-05-06-round-260-cddl-codegen-removal.md) — cddl codegen removal
- [R261](./2026-05-06-round-261-r253-narrowing.md) — r253 narrowing
- [R262](./2026-05-06-round-262-r253-final-narrowing-nonce-evolution.md) — r253 final narrowing nonce evolution
- [R265](./2026-05-06-round-265-gap-bp-confirmed-fresh-capture.md) — gap bp confirmed fresh capture
- [R266b](./2026-05-06-round-266b-gap-bp-builtin-trace-narrowing.md) — gap bp builtin trace narrowing
- [R266c](./2026-05-06-round-266c-gap-bp-script-context-shape.md) — gap bp script context shape
- [R266d](./2026-05-06-round-266d-gap-bp-cost-model-loading-fixture.md) — gap bp cost model loading fixture
- [R269j](./2026-05-06-round-269j-state-governance-and-committee-extraction.md) — state governance and committee extraction
- [R269q](./2026-05-06-round-269q-state-eras-byron-extraction.md) — state eras byron extraction
- [R269r](./2026-05-06-round-269r-state-eras-shelley-extraction.md) — state eras shelley extraction
- [R269s](./2026-05-06-round-269s-state-eras-allegra-extraction.md) — state eras allegra extraction
- [R269t](./2026-05-06-round-269t-state-eras-mary-extraction.md) — state eras mary extraction
- [R269u](./2026-05-06-round-269u-state-eras-alonzo-extraction.md) — state eras alonzo extraction
- [R269v](./2026-05-06-round-269v-state-eras-babbage-extraction.md) — state eras babbage extraction
- [R269w](./2026-05-06-round-269w-state-eras-conway-extraction.md) — state eras conway extraction
- [R270a](./2026-05-06-round-270a-governor-types-extraction.md) — governor types extraction
- [R270b](./2026-05-06-round-270b-governor-churn-extraction.md) — governor churn extraction
- [R270c](./2026-05-06-round-270c-governor-peer-metric-extraction.md) — governor peer metric extraction
- [R270d](./2026-05-06-round-270d-governor-state-extraction.md) — governor state extraction
- [R270e](./2026-05-06-round-270e-governor-counters-extraction.md) — governor counters extraction
- [R271a](./2026-05-06-round-271a-runtime-governor-config-extraction.md) — runtime governor config extraction
- [R271b](./2026-05-06-round-271b-runtime-block-producer-config-extraction.md) — runtime block producer config extraction
- [R271c](./2026-05-06-round-271c-runtime-ledger-judgement-extraction.md) — runtime ledger judgement extraction
- [R271d](./2026-05-06-round-271d-runtime-mempool-helpers-extraction.md) — runtime mempool helpers extraction
- [R271e](./2026-05-06-round-271e-runtime-tx-submission-service-extraction.md) — runtime tx submission service extraction
- [R271f](./2026-05-06-round-271f-runtime-peer-session-extraction.md) — runtime peer session extraction
- [R271g](./2026-05-06-round-271g-runtime-bootstrap-extraction.md) — runtime bootstrap extraction
- [R271h](./2026-05-06-round-271h-runtime-keep-alive-extraction.md) — runtime keep alive extraction
- [R271i](./2026-05-06-round-271i-runtime-reconnecting-extraction-rollback.md) — runtime reconnecting extraction rollback
- [R271i](./2026-05-07-round-271i-revised-runtime-tracing-extraction.md) — revised runtime tracing extraction
- [R271j](./2026-05-07-round-271j-runtime-reconnecting-extraction.md) — runtime reconnecting extraction
- [R271k](./2026-05-07-round-271k-runtime-block-producer-loop-extraction.md) — runtime block producer loop extraction
- [R271l](./2026-05-07-round-271l-runtime-governor-loop-extraction.md) — runtime governor loop extraction
- [R271m](./2026-05-07-round-271m-runtime-reconnecting-sync-extraction.md) — runtime reconnecting sync extraction
- [R271n](./2026-05-07-round-271n-runtime-peer-management-extraction.md) — runtime peer management extraction
- [R271o](./2026-05-07-round-271o-runtime-cm-actions-extraction.md) — runtime cm actions extraction
- [R271p](./2026-05-07-round-271p-runtime-forge-extraction.md) — runtime forge extraction
- [R271q](./2026-05-07-round-271q-runtime-ledger-peer-source-extraction.md) — runtime ledger peer source extraction
- [R271r](./2026-05-07-round-271r-runtime-sync-session-extraction.md) — runtime sync session extraction
- [R271s](./2026-05-07-round-271s-runtime-final-folds.md) — runtime final folds
- [R273](./2026-05-09-round-273-rename-strict-naming-parity.md) — rename strict naming parity
- [R273a](./2026-05-07-round-273a-praos-split.md) — praos split
- [R273b](./2026-05-07-round-273b-nonce-split.md) — nonce split
- [R273c](./2026-05-07-round-273c-opcert-split.md) — opcert split
- [R273d](./2026-05-08-round-273d-mempool-queue-split.md) — mempool queue split
- [R273e](./2026-05-08-round-273e-mempool-tx-state-split.md) — mempool tx state split
- [R273f](./2026-05-08-round-273f-diffusion-pipelining-split.md) — diffusion pipelining split
- [R273g](./2026-05-08-round-273g-plutus-types-split.md) — plutus types split
- [R273h](./2026-05-08-round-273h-plutus-cost-model-split.md) — plutus cost model split
- [R273i](./2026-05-09-round-273i-plutus-flat-split.md) — plutus flat split
- [R274](./2026-05-09-round-274-strict-mirror-discovery.md) — strict mirror discovery

### R275–R298 (2026-05-09 → 2026-05-09, 22 files)

- [R275](./2026-05-09-round-275-strict-mirror-drift-guard.md) — strict mirror drift guard
- [R276](./2026-05-09-round-276-state-naming-parity.md) — state naming parity
- [R277](./2026-05-09-round-277-consensus-cluster-naming-parity.md) — consensus cluster naming parity
- [R278](./2026-05-09-round-278-mempool-naming-parity.md) — mempool naming parity
- [R279](./2026-05-09-round-279-runtime-naming-parity.md) — runtime naming parity
- [R280](./2026-05-09-round-280-network-governor-naming-parity.md) — network governor naming parity
- [R281](./2026-05-09-round-281-sweeper-naming-parity.md) — sweeper naming parity
- [R282](./2026-05-09-round-282-block-producer-serde-field.md) — block producer serde field
- [R283](./2026-05-09-round-283-era-tag-wiring.md) — era tag wiring
- [R284](./2026-05-09-round-284-lsq-todo-resolution.md) — lsq todo resolution
- [R285](./2026-05-09-round-285-phase-6-allow-cleanup.md) — phase 6 allow cleanup
- [R286](./2026-05-09-round-286-marker-and-helper-cleanup.md) — marker and helper cleanup
- [R287](./2026-05-09-round-287-doc-regrade.md) — doc regrade
- [R288](./2026-05-09-round-288-drift-guard-fail-build.md) — drift guard fail build
- [R289](./2026-05-09-round-289-phase-f-bootstrap.md) — phase f bootstrap
- [R290](./2026-05-09-round-290-cardano-cli-byron-cluster.md) — cardano cli byron cluster
- [R291](./2026-05-09-round-291-cardano-cli-compatible-cluster.md) — cardano cli compatible cluster
- [R292](./2026-05-09-round-292-cardano-cli-era-based-cluster.md) — cardano cli era based cluster
- [R294](./2026-05-09-round-294-295-cardano-cli-sweeper.md) — 295 cardano cli sweeper
- [R296](./2026-05-09-round-296-cardano-cli-version-wiring.md) — cardano cli version wiring
- [R297](./2026-05-09-round-297-cardano-cli-show-upstream-config.md) — cardano cli show upstream config
- [R298](./2026-05-09-round-298-docs-cleanup-phase-1.md) — docs cleanup phase 1

## Restoring a file

If a future round needs to re-cite an archived log from production code,
`git mv` the file back into `docs/operational-runs/` and update
`AGENTS.md` / `docs/ARCHITECTURE.md` / `docs/PARITY_PROOF.md` link paths
accordingly. The archive is plain `git mv` — no metadata is lost.
