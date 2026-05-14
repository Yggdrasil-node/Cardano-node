//! Genesis-aware ledger-state seeding + startup tracing helpers used
//! by `run_node` at the very top of the boot sequence.
//!
//! Mirrors the genesis-loading slice of upstream `Cardano.Node.Run`:
//! `Cardano.Node.Configuration.POM.parseGenesisHash` runs the genesis
//! integrity check, then `Ouroboros.Consensus.Node.Genesis.*` seeds
//! the initial ledger state from the era-keyed genesis files. Yggdrasil
//! collapses both phases into a single binary-side helper because we
//! own the multi-era `LedgerState` directly rather than instantiating
//! it from a `ProtocolInfo` record.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Run.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side runtime startup
//! helpers — banner emission, network-magic resolution, config
//! validation, and ChainDb / mempool / governor / block-producer
//! construction wiring. Mirrors upstream
//! `Cardano.Node.Run::initNodeConfig` + `runNode`. Upstream
//! wires these inline; Yggdrasil isolates the bring-up steps
//! for testability.

use eyre::{Result, WrapErr};
use serde_json::json;

use yggdrasil_ledger::{Era, GenesisDelegationState, LedgerState, StakeCredential};
use yggdrasil_node::config::NodeConfigFile;
use yggdrasil_node::tracer::{NodeTracer, trace_fields};

/// Emit a positive-path `Node.GenesisHash.Verified` trace event after
/// [`strict_base_ledger_state`] has completed genesis-hash verification.
///
/// Each `(file, hash)` pair is reported so operators can confirm exactly
/// which files were checked. Byron is counted when both `ByronGenesisFile`
/// and `ByronGenesisHash` are present because the verifier mirrors
/// upstream canonical JSON hashing.
pub fn trace_genesis_hashes_verified(tracer: &NodeTracer, file_cfg: &NodeConfigFile) {
    let shelley_verified =
        file_cfg.shelley_genesis_file.is_some() && file_cfg.shelley_genesis_hash.is_some();
    let alonzo_verified =
        file_cfg.alonzo_genesis_file.is_some() && file_cfg.alonzo_genesis_hash.is_some();
    let conway_verified =
        file_cfg.conway_genesis_file.is_some() && file_cfg.conway_genesis_hash.is_some();
    let byron_verified =
        file_cfg.byron_genesis_file.is_some() && file_cfg.byron_genesis_hash.is_some();
    let verified_count = u64::from(byron_verified)
        + u64::from(shelley_verified)
        + u64::from(alonzo_verified)
        + u64::from(conway_verified);

    tracer.trace_runtime(
        "Node.GenesisHash.Verified",
        "Notice",
        "genesis hash integrity check passed",
        trace_fields([
            ("shelleyVerified", json!(shelley_verified)),
            ("alonzoVerified", json!(alonzo_verified)),
            ("conwayVerified", json!(conway_verified)),
            ("byronVerified", json!(byron_verified)),
            ("verifiedCount", json!(verified_count)),
        ]),
    );
}

/// Verify all declared genesis hashes, then build the initial Byron-era
/// `LedgerState` and seed it with Byron + Shelley genesis content.
///
/// Returns `Err` for any of: hash mismatch, missing file, malformed
/// genesis content. Mirrors upstream
/// `Cardano.Node.Configuration.POM.parseGenesisHash` (validation) +
/// `Ouroboros.Consensus.Node.Genesis.genesisLedgerState` (seeding).
pub fn strict_base_ledger_state(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<LedgerState> {
    // Verify the operator-declared genesis hashes BEFORE loading any
    // genesis content so a wrong genesis file aborts startup cleanly
    // rather than silently corrupting subsequent ledger state. Mirrors
    // upstream `Cardano.Node.Configuration.POM.parseGenesisHash`.
    file_cfg
        .verify_known_genesis_hashes(config_base_dir)
        .wrap_err("genesis hash verification failed")?;

    let mut state = LedgerState::new(Era::Byron);
    state.set_expected_network_id(file_cfg.expected_network_id());

    // Seed the multi-era UTxO with Byron genesis distributions so the
    // first Byron transaction that spends a genesis output can resolve
    // its inputs.  Without this seeding every Byron block beyond the
    // genesis-funded first transaction would fail with `InputNotFound`.
    //
    // Reference: `Cardano.Chain.Genesis.UTxO.genesisUtxo`.
    let byron_entries = file_cfg
        .load_byron_genesis_utxo(config_base_dir)
        .wrap_err("failed to load Byron genesis UTxO")?;
    let byron_initial_lovelace = byron_entries
        .iter()
        .fold(0u64, |sum, entry| sum.saturating_add(entry.amount));
    if !byron_entries.is_empty() {
        state.seed_byron_genesis_utxo(
            byron_entries
                .into_iter()
                .map(|entry| (entry.address, entry.amount)),
        );
    }
    if let Some(bootstrap) = file_cfg
        .load_shelley_genesis_bootstrap(config_base_dir)
        .wrap_err("failed to load Shelley genesis bootstrap")?
    {
        let shelley_initial_lovelace = bootstrap
            .initial_funds
            .iter()
            .fold(0u64, |sum, (_, txout)| sum.saturating_add(txout.amount));
        let initial_circulation = byron_initial_lovelace.saturating_add(shelley_initial_lovelace);
        state.configure_pending_shelley_genesis_utxo(bootstrap.initial_funds);
        state.configure_pending_shelley_genesis_stake(
            bootstrap
                .staking
                .into_iter()
                .map(|(credential, pool)| (StakeCredential::AddrKeyHash(credential), pool))
                .collect(),
        );
        state.configure_pending_shelley_genesis_delegs(
            bootstrap
                .gen_delegs
                .into_iter()
                .map(|(genesis_hash, parsed)| {
                    (
                        genesis_hash,
                        GenesisDelegationState {
                            delegate: parsed.delegate,
                            vrf: parsed.vrf,
                        },
                    )
                })
                .collect(),
        );
        state.set_genesis_update_quorum(bootstrap.update_quorum);
        state.set_max_lovelace_supply(bootstrap.max_lovelace_supply);
        state.accounting_mut().reserves = bootstrap
            .max_lovelace_supply
            .saturating_sub(initial_circulation);
        state.set_slots_per_epoch(bootstrap.epoch_length);
        state.set_active_slot_coeff(yggdrasil_ledger::UnitInterval {
            numerator: bootstrap.active_slots_coeff.0,
            denominator: bootstrap.active_slots_coeff.1,
        });
        // R264 — feed the Byron→Shelley boundary into the ledger so
        // PPUP / MIR / blocks_made first-slot math respects the
        // era-aware schedule on chains with a Byron prefix
        // (mainnet, preprod). Without this, those checks use
        // `(current_epoch + 1) * slots_per_epoch` (fixed-length
        // anchored at slot 0) and silently distort reward-cycle
        // accounting + protocol-update timing.
        state.set_byron_shelley_transition(file_cfg.byron_to_shelley_slot.map(|boundary| {
            (
                boundary,
                file_cfg
                    .first_shelley_epoch
                    .unwrap_or(boundary / file_cfg.byron_epoch_length.max(1)),
            )
        }));
        // Compute stability_window = 3k/f from genesis config so the
        // ledger PPUP rule can enforce the exact upstream slot-of-no-return.
        if file_cfg.active_slot_coeff > 0.0 {
            let sw = (3.0 * file_cfg.security_param_k as f64 / file_cfg.active_slot_coeff) as u64;
            state.set_stability_window(sw);
        }
    }
    if let Some(params) = file_cfg
        .load_genesis_protocol_params(config_base_dir)
        .wrap_err("failed to load genesis protocol parameters")?
    {
        state.set_protocol_params(params);
    }
    if let Some(enact) = file_cfg
        .load_genesis_enact_state(config_base_dir)
        .wrap_err("failed to load genesis enact state")?
    {
        *state.enact_state_mut() = enact;
    }
    Ok(state)
}

/// Best-effort variant of [`strict_base_ledger_state`] that swallows
/// genesis-loading errors and returns an empty Byron-era `LedgerState`.
///
/// Used by the `status` subcommand and by other inspection paths that
/// must not bail when an operator runs them against a partially-
/// configured deployment.
pub fn best_effort_base_ledger_state(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> LedgerState {
    strict_base_ledger_state(file_cfg, config_base_dir)
        .unwrap_or_else(|_| LedgerState::new(Era::Byron))
}

/// Pick the protocol-version pair `(major, minor)` to stamp on a freshly
/// forged block header. Falls back to `(max_major_protocol_version, 0)`
/// when the recovered ledger state has no `protocol_version` (e.g.
/// before the Shelley hard-fork has applied).
pub fn forged_header_protocol_version(
    base_ledger_state: &LedgerState,
    max_major_protocol_version: u64,
) -> (u64, u64) {
    base_ledger_state
        .protocol_params()
        .and_then(|params| params.protocol_version)
        .unwrap_or((max_major_protocol_version, 0))
}
