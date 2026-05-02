## Round 248 — TPraos overlay VRF parity

### Change

- Added Shelley-Alonzo TPraos overlay schedule handling to the verified sync
  VRF path.
- Active overlay slots now verify the selected genesis delegate cold key and
  both TPraos VRF proofs, then skip the pool stake leader threshold, matching
  upstream `pbftVrfChecks`.
- Reserved non-active overlay slots fail closed.
- Added `EpochSchedule::epoch_first_slot()` so overlay classification uses the
  era-aware equivalent of upstream `epochInfoFirst`.
- Added `verify_leader_proof_output()` for proof-only checks.

### Root Cause

The preview chain starts from Shelley genesis with `decentralisationParam = 1`.
At Alonzo slot `106220`, the TPraos overlay schedule selects an active genesis
delegate slot. Upstream validates that branch with
`Cardano.Protocol.TPraos.Rules.Overlay.pbftVrfChecks`, which checks the VRF
proofs and genesis delegation VRF key but does not run `checkLeaderValue`.

Yggdrasil was treating every TPraos block as a Praos pool-leader block. The VRF
proof verified, but the pool stake leader-threshold check failed because the
block issuer was a genesis delegate selected by the overlay schedule.

### Verification

- `cargo fmt`
- `cargo test -p yggdrasil-consensus praos::tests --lib`
- `cargo test -p yggdrasil-consensus epoch::tests --lib`
- `cargo test -p yggdrasil-node tpraos_overlay_schedule --lib`
- `cargo test -p yggdrasil-node sync:: --lib`
- `cargo build -p yggdrasil-node --release`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo test-all`
- `cargo lint`
- Live preview resume on copied DB
  `/workspaces/Cardano-node/tmp/preview-r248-overlay-20260502T110139Z`:
  - replayed through former blocker slot `106220`
  - progressed to Babbage
  - final run point before SIGTERM: slot `412896`
  - status after shutdown: `chain_tip_slot = 412896`,
    `immutable_tip = 404134`, `current_era = Babbage`, `current_epoch = 4`
  - log scan found no `VRF verification failed`, `MalformedReferenceScripts`,
    `ledger decode error`, or panic.

### Upstream References

- `cardano-ledger`:
  `libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/Overlay.hs`
  (`lookupInOverlaySchedule`, `pbftVrfChecks`)
- `cardano-ledger`:
  `libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/BHeader.hs`
  (`seedEta`, `seedL`, `mkSeed`, `checkLeaderValue`)
- `ouroboros-consensus`:
  `ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos/VRF.hs`
  (`mkInputVRF`, `vrfLeaderValue`, `vrfNonceValue`)
