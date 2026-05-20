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

use yggdrasil_ledger::{Era, LedgerState};
use yggdrasil_node_config::NodeConfigFile;
use yggdrasil_node_tracer::{NodeTracer, trace_fields};

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

    // Load every genesis piece, then hand them to the shared
    // `build_base_ledger_state` builder. Keeping the construction in a
    // library crate lets the db-synthesizer seed a byte-identical
    // initial ledger state (A3 R3c-1a).
    let inputs = yggdrasil_node_genesis::BaseLedgerStateInputs {
        expected_network_id: file_cfg.expected_network_id(),
        byron_entries: file_cfg
            .load_byron_genesis_utxo(config_base_dir)
            .wrap_err("failed to load Byron genesis UTxO")?,
        shelley_bootstrap: file_cfg
            .load_shelley_genesis_bootstrap(config_base_dir)
            .wrap_err("failed to load Shelley genesis bootstrap")?,
        protocol_params: file_cfg
            .load_genesis_protocol_params(config_base_dir)
            .wrap_err("failed to load genesis protocol parameters")?,
        enact_state: file_cfg
            .load_genesis_enact_state(config_base_dir)
            .wrap_err("failed to load genesis enact state")?,
        byron_to_shelley_slot: file_cfg.byron_to_shelley_slot,
        first_shelley_epoch: file_cfg.first_shelley_epoch,
        byron_epoch_length: file_cfg.byron_epoch_length,
        active_slot_coeff: file_cfg.active_slot_coeff,
        security_param_k: file_cfg.security_param_k,
    };
    Ok(yggdrasil_node_genesis::build_base_ledger_state(inputs))
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
#[cfg(feature = "forge")]
pub fn forged_header_protocol_version(
    base_ledger_state: &LedgerState,
    max_major_protocol_version: u64,
) -> (u64, u64) {
    base_ledger_state
        .protocol_params()
        .and_then(|params| params.protocol_version)
        .unwrap_or((max_major_protocol_version, 0))
}
