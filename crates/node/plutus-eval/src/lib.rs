#![cfg_attr(test, allow(clippy::unwrap_used))]
//! CEK-machine `PlutusEvaluator` implementation for the node.
//!
//! Bridges [`yggdrasil_ledger::plutus_validation::PlutusEvaluator`] to the
//! actual [`yggdrasil_plutus`] CEK machine.
//!
//! ## Argument application
//!
//! Cardano Plutus scripts are curried functions:
//! - Spending validator:   `datum -> redeemer -> context -> result`
//! - All other validators: `redeemer -> context -> result`
//!
//! For PlutusV1/V2 the result is discarded — any non-error outcome is
//! accepted. For PlutusV3 the result must be `Constant(Bool(true))`.
//!
//! ## ScriptContext construction
//!
//! `TxInfo` is now built from a normalised ledger `TxContext` threaded through
//! the validation pipeline. Inputs, datums, redeemers, governance fields, and
//! reference-script hashes are derived from the real ledger view rather than a
//! fixed synthetic context.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core>
//! Reference: <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/PlutusScripts.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `PlutusEvaluator`
//! trait implementation that invokes the CEK machine from the
//! ledger crate. Mirrors the Phase-2 evaluator half of upstream
//! `Cardano.Ledger.Alonzo.Rules.Utxos::ledgerEvalScripts`. The
//! ledger crate cannot depend on yggdrasil-plutus directly,
//! so the trait lives in `crates/ledger/src/plutus_validation.rs`
//! and the impl + ScriptContext construction lives here.

use yggdrasil_ledger::{
    Address, CborEncode, DCert, LedgerError, Script, StakeCredential,
    plutus::PlutusData,
    plutus_validation::{
        PlutusEvaluator, PlutusScriptEval, PlutusVersion, ScriptPurpose, TxContext,
    },
};
use yggdrasil_plutus::{
    CostModel, ExBudget, MachineError, Value, decode_script_bytes,
    decode_script_bytes_allowing_remainder,
    types::{Constant, Term},
};

// ---------------------------------------------------------------------------
// CekPlutusEvaluator
// ---------------------------------------------------------------------------

/// A [`PlutusEvaluator`] backed by the `yggdrasil-plutus` CEK machine.
///
/// Decodes each script from its on-chain `PlutusBinary` bytes (upstream
/// `SerialisedScript` CBOR bytestring, then Flat), applies datum (if
/// spending), redeemer, and a version-aware ScriptContext, then evaluates
/// within the budget declared by the transaction.
///
/// When `system_start_unix_secs` and `slot_length_secs` are provided the
/// evaluator converts slot numbers in `TxContext.validity_start` /
/// `TxContext.ttl` to POSIX milliseconds before encoding them in the
/// `POSIXTimeRange` field of the `TxInfo` ScriptContext.  This matches
/// the upstream `transVITime` in `Cardano.Ledger.Alonzo.Plutus.TxInfo`.
#[derive(Clone, Debug, Default)]
pub struct CekPlutusEvaluator {
    /// Cost model to use. Defaults to `CostModel::default()`.
    pub cost_model: CostModel,
    /// Seconds since Unix epoch of the network genesis moment.
    ///
    /// Parsed from `ShelleyGenesis.system_start` (e.g. "2017-09-23T21:44:51Z").
    /// When `None`, slot numbers are passed through as-is (legacy behaviour).
    pub system_start_unix_secs: Option<f64>,
    /// Slot duration in seconds from Shelley genesis (`slotLength`).
    ///
    /// Only used when `system_start_unix_secs` is `Some`.
    pub slot_length_secs: f64,
}

impl CekPlutusEvaluator {
    /// Create an evaluator with the default cost model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an evaluator with a custom cost model.
    pub fn with_cost_model(cost_model: CostModel) -> Self {
        Self {
            cost_model,
            ..Default::default()
        }
    }

    /// Create a fully configured evaluator.
    pub fn with_time_conversion(
        cost_model: CostModel,
        system_start_unix_secs: f64,
        slot_length_secs: f64,
    ) -> Self {
        Self {
            cost_model,
            system_start_unix_secs: Some(system_start_unix_secs),
            slot_length_secs,
        }
    }

    /// Convert a slot number to POSIX milliseconds using the stored
    /// genesis parameters, or return the raw slot if unavailable.
    fn slot_to_posix_ms(&self, slot: u64) -> u64 {
        match self.system_start_unix_secs {
            Some(start) => {
                yggdrasil_node_genesis::slot_to_posix_ms(slot, start, self.slot_length_secs)
            }
            None => slot,
        }
    }
}

impl PlutusEvaluator for CekPlutusEvaluator {
    fn evaluate(&self, eval: &PlutusScriptEval, tx_ctx: &TxContext) -> Result<(), LedgerError> {
        // 1. Decode upstream PlutusBinary bytes: CBOR bytestring, then Flat.
        let program =
            decode_script_bytes_for_version(eval.version, &eval.script_bytes).map_err(|e| {
                LedgerError::PlutusScriptDecodeError {
                    hash: eval.script_hash,
                    reason: e.to_string(),
                }
            })?;

        // 2. Build Term::Constant wrappers for datum, redeemer, and context.
        let redeemer_term = data_term(eval.redeemer.clone());
        // Build the ScriptContext from the normalized ledger transaction view.
        let context_data = script_context_data(eval, tx_ctx, self)?;
        // R266c forensic instrumentation — gated on `YGG_DUMP_SCRIPT_CONTEXT=1`.
        // Logs the constructed ScriptContext as CBOR hex so a structural diff
        // against upstream's V1/V2/V3 PlutusData encoding can be done offline.
        // Used to localise the residual ~14-step CEK divergence on the Gap BP
        // failing tx (preview slot ~1,462,057). Production-safe: zero overhead
        // when unset.
        if std::env::var_os("YGG_DUMP_SCRIPT_CONTEXT").is_some() {
            let cbor = yggdrasil_ledger::CborEncode::to_cbor_bytes(&context_data);
            eprintln!(
                "YGG_DUMP_SCRIPT_CONTEXT: tx_hash={} script_hash={} version={:?} cbor_len={} cbor_hex={}",
                hex::encode(tx_ctx.tx_hash),
                hex::encode(eval.script_hash),
                eval.version,
                cbor.len(),
                hex::encode(&cbor),
            );
        }
        let context_term = Term::Constant(Constant::Data(context_data));

        // 3. Apply arguments in the order specified by the Plutus script ABI.
        //    spending validator: script datum redeemer context
        //    all others:         script redeemer context
        let applied = match &eval.datum {
            Some(datum) => Term::Apply(
                Box::new(Term::Apply(
                    Box::new(Term::Apply(
                        Box::new(program.term),
                        Box::new(data_term(datum.clone())),
                    )),
                    Box::new(redeemer_term),
                )),
                Box::new(context_term),
            ),
            None => Term::Apply(
                Box::new(Term::Apply(Box::new(program.term), Box::new(redeemer_term))),
                Box::new(context_term),
            ),
        };

        // 4. Build execution budget from the transaction's declared ExUnits.
        //    ExUnits.steps → cpu; ExUnits.mem → mem.
        let budget = ExBudget::new(eval.ex_units.steps as i64, eval.ex_units.mem as i64);
        let cost_model = match eval.cost_model.as_deref() {
            Some(values) => {
                yggdrasil_node_genesis::build_plutus_cost_model_from_protocol_values_for_protocol(
                    eval.version,
                    tx_ctx.protocol_version,
                    values,
                )
                .map_err(|err| LedgerError::PlutusScriptFailed {
                    hash: eval.script_hash,
                    reason: format!(
                        "invalid active cost model for {:?} at protocol {:?} ({} values): {err}",
                        eval.version,
                        tx_ctx.protocol_version,
                        values.len()
                    ),
                })?
            }
            None => self.cost_model.clone(),
        };

        // R266 forensic instrumentation — gated on `YGG_DUMP_PLUTUS_PV=1`.
        // Logs (per V2 eval) whether the per-tx cost model was propagated,
        // the protocol version reaching the variant selector, and the
        // builtin-semantics variant the constructed model actually carries.
        // Mirrors the upstream `machineParametersFor` selector in
        // `PlutusLedgerApi.MachineParameters`. Remove or `unset` after
        // R266 root-cause closure.
        if std::env::var_os("YGG_DUMP_PLUTUS_PV").is_some() {
            let s = &cost_model.step_costs;
            eprintln!(
                "YGG_DUMP_PLUTUS_PV: tx_hash={} script_hash={} version={:?} pv={:?} propagated={} variant={:?} \
                 startup={}/{} var={}/{} const={}/{} lam={}/{} apply={}/{} delay={}/{} force={}/{} builtin={}/{}",
                hex::encode(tx_ctx.tx_hash),
                hex::encode(eval.script_hash),
                eval.version,
                tx_ctx.protocol_version,
                eval.cost_model.is_some(),
                cost_model.builtin_semantics_variant,
                cost_model.startup_cost.cpu,
                cost_model.startup_cost.mem,
                s.var_cpu,
                s.var_mem,
                s.constant_cpu,
                s.constant_mem,
                s.lam_cpu,
                s.lam_mem,
                s.apply_cpu,
                s.apply_mem,
                s.delay_cpu,
                s.delay_mem,
                s.force_cpu,
                s.force_mem,
                s.builtin_cpu,
                s.builtin_mem,
            );
        }

        // 5. Evaluate the applied term. Keep the machine available on
        // failure so explicit Plutus `trace` breadcrumbs can be surfaced
        // during live parity investigations without changing default errors.
        let mut machine = yggdrasil_plutus::CekMachine::new(budget, cost_model);
        let result = match machine.evaluate(applied) {
            Ok(result) => result,
            Err(err) => {
                let raw_machine_err = err.to_string();
                let mut ledger_err = map_machine_error(&eval.script_hash, err);
                if std::env::var_os("YGGDRASIL_PLUTUS_TRACE_FAILURES").is_some() {
                    if let LedgerError::PlutusScriptFailed { reason, .. } = &mut ledger_err {
                        reason.push_str("; machine_error=");
                        reason.push_str(&raw_machine_err);
                    }
                    if let LedgerError::PlutusScriptFailed { reason, .. } = &mut ledger_err
                        && !machine.logs.is_empty()
                    {
                        reason.push_str("; logs=");
                        reason.push_str(&format!("{:?}", machine.logs));
                    }
                }
                return Err(ledger_err);
            }
        };

        // 6. PlutusV3 scripts must explicitly return Bool(true).
        //    PlutusV1/V2 accept any non-error result.
        if eval.version == PlutusVersion::V3 {
            match result {
                Value::Constant(Constant::Bool(true)) => Ok(()),
                other => Err(LedgerError::PlutusScriptFailed {
                    hash: eval.script_hash,
                    reason: format!("PlutusV3 script must return Bool(true), got: {:?}", other),
                }),
            }
        } else {
            Ok(())
        }
    }

    fn is_script_well_formed(
        &self,
        version: PlutusVersion,
        protocol_version: Option<(u64, u64)>,
        script_bytes: &[u8],
    ) -> bool {
        if let Some((major, _minor)) = protocol_version {
            if major < version.first_supported_protocol_major() {
                return false;
            }
        }
        decode_script_bytes_for_version(version, script_bytes).is_ok()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Wrap a [`PlutusData`] value in a `Term::Constant`.
fn data_term(data: PlutusData) -> Term {
    Term::Constant(Constant::Data(data))
}

fn decode_script_bytes_for_version(
    version: PlutusVersion,
    script_bytes: &[u8],
) -> Result<yggdrasil_plutus::Program, MachineError> {
    match version {
        PlutusVersion::V1 | PlutusVersion::V2 => {
            decode_script_bytes_allowing_remainder(script_bytes)
        }
        PlutusVersion::V3 => decode_script_bytes(script_bytes),
    }
}

fn script_context_data(
    eval: &PlutusScriptEval,
    tx_ctx: &TxContext,
    evaluator: &CekPlutusEvaluator,
) -> Result<PlutusData, LedgerError> {
    Ok(match eval.version {
        PlutusVersion::V1 | PlutusVersion::V2 => PlutusData::Constr(
            0,
            vec![
                build_tx_info(eval.version, tx_ctx, evaluator)?,
                script_purpose_data_v1v2(eval.version, &eval.purpose)?,
            ],
        ),
        PlutusVersion::V3 => PlutusData::Constr(
            0,
            vec![
                build_tx_info(eval.version, tx_ctx, evaluator)?,
                eval.redeemer.clone(),
                script_info_data_v3(&eval.purpose, eval.datum.as_ref(), tx_ctx.protocol_version)?,
            ],
        ),
    })
}

/// Build a Plutus TxInfo as PlutusData from the transaction context.
///
/// Field layout follows the upstream Haskell TxInfo constructors:
///
/// V1 (10 fields): inputs, outputs, fee, mint, dcert, wdrl, validRange,
///                 signatories, datums, id
///
/// V2 (12 fields): inputs, referenceInputs, outputs, fee, mint, dcert,
///                 wdrl, validRange, signatories, redeemers, datums, id
///   - `referenceInputs` resolved from the live UTxO set
///   - `redeemers` map is keyed by V2 `ScriptPurpose`
///
/// V3 (16 fields): inputs, referenceInputs, outputs, fee, mint, txCerts,
///                 wdrl, validRange, signatories, redeemers, datums, id,
///                 votes, proposals, currentTreasury, treasuryDonation
///   - `redeemers` use V3 `ScriptPurpose` keys
///   - `votes` and `proposalProcedures` are populated from Conway tx bodies
///   - `txCerts` uses the V3 TxCert encoding
fn build_tx_info(
    version: PlutusVersion,
    tx_ctx: &TxContext,
    evaluator: &CekPlutusEvaluator,
) -> Result<PlutusData, LedgerError> {
    // -- Shared building blocks --

    if matches!(version, PlutusVersion::V1 | PlutusVersion::V2) {
        guard_legacy_plutus_context_features(version, tx_ctx)?;
    }

    let inputs_data = PlutusData::List(
        tx_ctx
            .inputs
            .iter()
            .filter_map(|(txin, txout)| plutus_input_data(version, txin, txout))
            .collect(),
    );

    let ref_inputs_data = PlutusData::List(
        tx_ctx
            .resolved_reference_inputs
            .iter()
            .filter_map(|(txin, txout)| plutus_input_data(version, txin, txout))
            .collect(),
    );

    let outputs_data = PlutusData::List(
        tx_ctx
            .outputs
            .iter()
            .filter_map(|o| plutus_output_data(version, o))
            .collect(),
    );

    // V1/V2 fee is a full Value: Map [(CurrencySymbol "", Map [(TokenName "", Integer fee)])]
    // V3 fee is a plain Lovelace (Integer).
    // Reference: V1/V2 — `transCoinToValue (txBody ^. feeTxBodyL)`
    //            V3   — `transCoinToLovelace (txBody ^. feeTxBodyL)`
    let fee_v1v2 = plutus_value_data(&yggdrasil_ledger::eras::mary::Value::Coin(tx_ctx.fee));
    let fee_v3 = PlutusData::integer(tx_ctx.fee as i128);

    let mint_entries: Vec<(PlutusData, PlutusData)> = tx_ctx
        .mint
        .iter()
        .map(|(policy, assets)| {
            let asset_map: Vec<(PlutusData, PlutusData)> = assets
                .iter()
                .map(|(name, qty)| {
                    (
                        PlutusData::Bytes(name.clone()),
                        PlutusData::integer(*qty as i128),
                    )
                })
                .collect();
            (
                PlutusData::Bytes(policy.to_vec()),
                PlutusData::Map(asset_map),
            )
        })
        .collect();

    let wdrl_entries_v2: Vec<(PlutusData, PlutusData)> = tx_ctx
        .withdrawals
        .iter()
        .map(|(ra, amt)| {
            (
                staking_credential_data(&ra.credential),
                PlutusData::integer(*amt as i128),
            )
        })
        .collect();

    let wdrl_entries_v3: Vec<(PlutusData, PlutusData)> = tx_ctx
        .withdrawals
        .iter()
        .map(|(ra, amt)| {
            (
                credential_data(&ra.credential),
                PlutusData::integer(*amt as i128),
            )
        })
        .collect();

    let valid_range = posix_time_range(
        tx_ctx.validity_start.map(|s| evaluator.slot_to_posix_ms(s)),
        tx_ctx.ttl.map(|t| evaluator.slot_to_posix_ms(t)),
        tx_ctx.protocol_version,
    );

    let signatories = PlutusData::List(
        tx_ctx
            .required_signers
            .iter()
            .map(|h| PlutusData::Bytes(h.to_vec()))
            .collect(),
    );

    let witness_datums = sorted_witness_datums(tx_ctx);
    let datums = PlutusData::Map(
        witness_datums
            .iter()
            .map(|(hash, datum)| (PlutusData::Bytes(hash.to_vec()), (*datum).clone()))
            .collect(),
    );

    let tx_id = tx_id_data(version, &tx_ctx.tx_hash);

    match version {
        PlutusVersion::V1 => {
            // V1 mint prepends zero-ADA entry (upstream `transMintValue m = transCoinToValue zero <> transMultiAsset m`)
            let mut v1_mint = vec![(
                PlutusData::Bytes(vec![]),
                PlutusData::Map(vec![(PlutusData::Bytes(vec![]), PlutusData::integer(0))]),
            )];
            v1_mint.extend(mint_entries);
            // V1 withdrawals: [(StakingCredential, Integer)] as List of tuples
            let wdrl_list_v1 = PlutusData::List(
                wdrl_entries_v2
                    .iter()
                    .map(|(cred, amt)| PlutusData::Constr(0, vec![cred.clone(), amt.clone()]))
                    .collect(),
            );
            // V1 datums: [(DatumHash, Datum)] as List of tuples
            let datums_list_v1 = PlutusData::List(
                witness_datums
                    .iter()
                    .map(|(hash, datum)| {
                        PlutusData::Constr(
                            0,
                            vec![PlutusData::Bytes(hash.to_vec()), (*datum).clone()],
                        )
                    })
                    .collect(),
            );
            Ok(PlutusData::Constr(
                0,
                vec![
                    inputs_data,              // inputs
                    outputs_data,             // outputs
                    fee_v1v2.clone(),         // fee (Value)
                    PlutusData::Map(v1_mint), // mint (with zero-ADA)
                    PlutusData::List(
                        // dcert (legacy encoding)
                        tx_ctx
                            .certificates
                            .iter()
                            .map(legacy_dcert_data)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    wdrl_list_v1,   // withdrawals (list of tuples)
                    valid_range,    // validRange
                    signatories,    // signatories
                    datums_list_v1, // datums (list of tuples)
                    tx_id,          // txInfoId
                ],
            ))
        }

        PlutusVersion::V2 => {
            // V2 mint also prepends zero-ADA entry (upstream `transMintValue`)
            let mut v2_mint = vec![(
                PlutusData::Bytes(vec![]),
                PlutusData::Map(vec![(PlutusData::Bytes(vec![]), PlutusData::integer(0))]),
            )];
            v2_mint.extend(mint_entries);
            let redeemers_v2 = PlutusData::Map(
                tx_ctx
                    .redeemers
                    .iter()
                    .map(|(purpose, redeemer)| {
                        Ok((
                            script_purpose_data_v1v2(version, purpose)?,
                            redeemer.clone(),
                        ))
                    })
                    .collect::<Result<Vec<_>, LedgerError>>()?,
            );
            Ok(PlutusData::Constr(
                0,
                vec![
                    inputs_data,              // inputs
                    ref_inputs_data,          // referenceInputs (NEW)
                    outputs_data,             // outputs
                    fee_v1v2,                 // fee (Value)
                    PlutusData::Map(v2_mint), // mint (with zero-ADA)
                    PlutusData::List(
                        // dcert (legacy encoding)
                        tx_ctx
                            .certificates
                            .iter()
                            .map(legacy_dcert_data)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    PlutusData::Map(wdrl_entries_v2), // withdrawals (Map)
                    valid_range,                      // validRange
                    signatories,                      // signatories
                    redeemers_v2,                     // redeemers
                    datums,                           // datums (Map)
                    tx_id,                            // txInfoId
                ],
            ))
        }

        PlutusVersion::V3 => {
            let pv = tx_ctx.protocol_version;
            let redeemers_v3 = PlutusData::Map(
                tx_ctx
                    .redeemers
                    .iter()
                    .map(|(purpose, redeemer)| {
                        Ok((script_purpose_data_v3(purpose, pv)?, redeemer.clone()))
                    })
                    .collect::<Result<Vec<_>, LedgerError>>()?,
            );
            // V3 uses the richer TxCert encoding (not legacy DCert).
            let tx_certs = PlutusData::List(
                tx_ctx
                    .certificates
                    .iter()
                    .map(|c| tx_cert_data_v3(c, pv))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let current_treasury = maybe_data(
                tx_ctx
                    .current_treasury_value
                    .map(|v| PlutusData::integer(v as i128)),
            );
            let treasury_donation = maybe_data(
                tx_ctx
                    .treasury_donation
                    .map(|v| PlutusData::integer(v as i128)),
            );
            let votes = PlutusData::Map(
                tx_ctx
                    .voting_procedures
                    .as_ref()
                    .map(|voting_procedures| {
                        voting_procedures
                            .procedures
                            .iter()
                            .map(|(voter, votes)| {
                                (
                                    voter_data_v3(voter),
                                    PlutusData::Map(
                                        votes
                                            .iter()
                                            .map(|(gov_action_id, procedure)| {
                                                (
                                                    gov_action_id_data(gov_action_id),
                                                    vote_data_v3(procedure.vote),
                                                )
                                            })
                                            .collect(),
                                    ),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            );
            let proposal_procedures = PlutusData::List(
                tx_ctx
                    .proposal_procedures
                    .iter()
                    .map(proposal_procedure_data_v3)
                    .collect::<Result<Vec<_>, _>>()?,
            );
            Ok(PlutusData::Constr(
                0,
                vec![
                    inputs_data,                      // inputs
                    ref_inputs_data,                  // referenceInputs
                    outputs_data,                     // outputs
                    fee_v3,                           // fee (Lovelace = Integer)
                    PlutusData::Map(mint_entries),    // mint (V3: no zero-ADA padding)
                    tx_certs,                         // txCerts (V3 encoding)
                    PlutusData::Map(wdrl_entries_v3), // withdrawals (Map)
                    valid_range,                      // validRange
                    signatories,                      // signatories
                    redeemers_v3,                     // redeemers
                    datums,                           // datums (Map)
                    tx_id,                            // txInfoId
                    votes,                            // votes
                    proposal_procedures,              // proposalProcedures
                    current_treasury,                 // currentTreasuryAmount
                    treasury_donation,                // treasuryDonation
                ],
            ))
        }
    }
}

fn sorted_witness_datums(tx_ctx: &TxContext) -> Vec<(&[u8; 32], &PlutusData)> {
    let mut datums: Vec<_> = tx_ctx.witness_datums.iter().collect();
    datums.sort_by_key(|(hash, _)| *hash);
    datums
}

/// Encode a POSIXTimeRange as PlutusData.
///
/// `Interval (LowerBound lb inclusive) (UpperBound ub inclusive)`
/// layout: Constr(0, [lower_bound, upper_bound])
fn posix_time_range(
    start: Option<u64>,
    end: Option<u64>,
    protocol_version: Option<(u64, u64)>,
) -> PlutusData {
    let lower = match start {
        Some(s) => PlutusData::Constr(
            0,
            vec![
                PlutusData::Constr(1, vec![PlutusData::integer(s as i128)]), // Finite
                PlutusData::Constr(1, vec![]),                               // True (inclusive)
            ],
        ),
        None => PlutusData::Constr(
            0,
            vec![
                PlutusData::Constr(0, vec![]), // NegInf
                PlutusData::Constr(1, vec![]), // True
            ],
        ),
    };
    let upper = match end {
        Some(e) => {
            // Alonzo/Babbage use `PV1.to` for an upper-only interval, which
            // makes the finite upper bound inclusive. Conway switched that
            // case to `strictUpperBound`.
            let upper_is_closed =
                start.is_none() && !matches!(protocol_version, Some((major, _)) if major >= 9);
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::integer(e as i128)]), // Finite
                    bool_data(upper_is_closed),
                ],
            )
        }
        None => PlutusData::Constr(
            0,
            vec![
                PlutusData::Constr(2, vec![]), // PosInf
                PlutusData::Constr(1, vec![]), // True
            ],
        ),
    };
    PlutusData::Constr(0, vec![lower, upper])
}

fn bool_data(value: bool) -> PlutusData {
    PlutusData::Constr(if value { 1 } else { 0 }, vec![])
}

/// Encode a TxInInfo as PlutusData: Constr(0, [txOutRef, txOut]).
fn plutus_input_data(
    version: PlutusVersion,
    txin: &yggdrasil_ledger::eras::shelley::ShelleyTxIn,
    txout: &yggdrasil_ledger::utxo::MultiEraTxOut,
) -> Option<PlutusData> {
    Some(PlutusData::Constr(
        0,
        vec![
            tx_out_ref_data(version, &txin.transaction_id, u64::from(txin.index)),
            plutus_output_data(version, txout)?,
        ],
    ))
}

/// Encode a MultiEraTxOut as PlutusData.
///
/// V1 TxOut: Constr(0, [address, value, maybe_datum_hash]) — 3 fields, all eras
///   Reference: `transTxOutV1` in Alonzo.Plutus.TxInfo / Conway.TxInfo
///
/// V2/V3 TxOut: Constr(0, [address, value, datum_option, script_ref]) — 4 fields (Babbage+)
///         or   Constr(0, [address, value, maybe_datum_hash]) — 3 fields (pre-Babbage)
///   Reference: `transTxOutV2` in Babbage.TxInfo
fn plutus_output_data(
    version: PlutusVersion,
    txout: &yggdrasil_ledger::utxo::MultiEraTxOut,
) -> Option<PlutusData> {
    match txout {
        yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(o) => Some(PlutusData::Constr(
            0,
            vec![
                plutus_address_data(&o.address)?,
                plutus_value_data(&yggdrasil_ledger::eras::mary::Value::Coin(o.amount)),
                PlutusData::Constr(1, vec![]), // Nothing (no datum hash)
            ],
        )),
        yggdrasil_ledger::utxo::MultiEraTxOut::Mary(o) => Some(PlutusData::Constr(
            0,
            vec![
                plutus_address_data(&o.address)?,
                plutus_value_data(&o.amount),
                PlutusData::Constr(1, vec![]), // Nothing
            ],
        )),
        yggdrasil_ledger::utxo::MultiEraTxOut::Alonzo(o) => {
            let datum_opt = match &o.datum_hash {
                Some(h) => PlutusData::Constr(0, vec![PlutusData::Bytes(h.to_vec())]),
                None => PlutusData::Constr(1, vec![]),
            };
            Some(PlutusData::Constr(
                0,
                vec![
                    plutus_address_data(&o.address)?,
                    plutus_value_data(&o.amount),
                    datum_opt,
                ],
            ))
        }
        yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(o) => {
            if version == PlutusVersion::V1 {
                // V1: 3-element shape with Maybe DatumHash.
                // Inline datums and reference scripts are not visible to V1 scripts.
                // Reference: upstream `transTxOutV1` rejects inline datums with
                // `InlineDatumsNotSupported`, but we silently downgrade here.
                let datum_opt = match &o.datum_option {
                    Some(yggdrasil_ledger::eras::babbage::DatumOption::Hash(h)) => {
                        PlutusData::Constr(0, vec![PlutusData::Bytes(h.to_vec())])
                    }
                    _ => PlutusData::Constr(1, vec![]), // Nothing — inline datums invisible to V1
                };
                Some(PlutusData::Constr(
                    0,
                    vec![
                        plutus_address_data(&o.address)?,
                        plutus_value_data(&o.amount),
                        datum_opt,
                    ],
                ))
            } else {
                // V2/V3: 4-element shape with OutputDatum and Maybe ScriptHash.
                let datum_field = match &o.datum_option {
                    Some(yggdrasil_ledger::eras::babbage::DatumOption::Hash(h)) => {
                        PlutusData::Constr(1, vec![PlutusData::Bytes(h.to_vec())])
                    }
                    Some(yggdrasil_ledger::eras::babbage::DatumOption::Inline(d)) => {
                        PlutusData::Constr(2, vec![d.clone()])
                    }
                    None => PlutusData::Constr(0, vec![]),
                };
                let script_ref_field = match &o.script_ref {
                    Some(sref) => PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(script_hash_from_ref(sref).to_vec())],
                    ),
                    None => PlutusData::Constr(1, vec![]),
                };
                Some(PlutusData::Constr(
                    0,
                    vec![
                        plutus_address_data(&o.address)?,
                        plutus_value_data(&o.amount),
                        datum_field,
                        script_ref_field,
                    ],
                ))
            }
        }
    }
}

fn plutus_address_data(address_bytes: &[u8]) -> Option<PlutusData> {
    match Address::from_bytes(address_bytes)? {
        Address::Base(base) => Some(PlutusData::Constr(
            0,
            vec![
                credential_data(&base.payment),
                maybe_data(Some(staking_credential_data(&base.staking))),
            ],
        )),
        Address::Enterprise(enterprise) => Some(PlutusData::Constr(
            0,
            vec![credential_data(&enterprise.payment), maybe_data(None)],
        )),
        Address::Pointer(pointer) => Some(PlutusData::Constr(
            0,
            vec![
                credential_data(&pointer.payment),
                maybe_data(Some(pointer_staking_credential_data(&pointer))),
            ],
        )),
        Address::Reward(_) | Address::Byron(_) => None,
    }
}

fn pointer_staking_credential_data(pointer: &yggdrasil_ledger::PointerAddress) -> PlutusData {
    PlutusData::Constr(
        1,
        vec![
            PlutusData::integer(pointer.slot as i128),
            PlutusData::integer(pointer.tx_index as i128),
            PlutusData::integer(pointer.cert_index as i128),
        ],
    )
}

/// Encode a ledger Value as PlutusData.
///
/// Plutus V1/V2 Value = Map [CurrencySymbol -> Map [TokenName -> Integer]]
/// The ADA entry uses empty-bytes as the currency symbol and token name.
fn plutus_value_data(value: &yggdrasil_ledger::eras::mary::Value) -> PlutusData {
    let mut entries: Vec<(PlutusData, PlutusData)> = Vec::new();
    // ADA entry: "" -> ("" -> coin)
    let coin = value.coin();
    entries.push((
        PlutusData::Bytes(vec![]),
        PlutusData::Map(vec![(
            PlutusData::Bytes(vec![]),
            PlutusData::integer(coin as i128),
        )]),
    ));
    // Multi-asset entries
    if let Some(ma) = value.multi_asset() {
        for (policy, assets) in ma {
            let asset_entries: Vec<(PlutusData, PlutusData)> = assets
                .iter()
                .map(|(name, qty)| {
                    (
                        PlutusData::Bytes(name.clone()),
                        PlutusData::integer(*qty as i128),
                    )
                })
                .collect();
            entries.push((
                PlutusData::Bytes(policy.to_vec()),
                PlutusData::Map(asset_entries),
            ));
        }
    }
    PlutusData::Map(entries)
}

fn script_purpose_data_v1v2(
    version: PlutusVersion,
    purpose: &ScriptPurpose,
) -> Result<PlutusData, LedgerError> {
    Ok(match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => {
            PlutusData::Constr(1, vec![tx_out_ref_data(version, tx_id, *index)])
        }
        ScriptPurpose::Rewarding { reward_account } => {
            PlutusData::Constr(2, vec![staking_credential_data(&reward_account.credential)])
        }
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => certifying_purpose_data(*cert_index, certificate)?,
        ScriptPurpose::Voting { .. } => {
            return Err(LedgerError::UnsupportedPlutusPurpose(
                "Voting purposes require Plutus V3 ScriptContext encoding",
            ));
        }
        ScriptPurpose::Proposing { .. } => {
            return Err(LedgerError::UnsupportedPlutusPurpose(
                "Proposing purposes require Plutus V3 ScriptContext encoding",
            ));
        }
    })
}

fn script_purpose_data_v3(
    purpose: &ScriptPurpose,
    pv: Option<(u64, u64)>,
) -> Result<PlutusData, LedgerError> {
    Ok(match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => {
            PlutusData::Constr(1, vec![tx_out_ref_data(PlutusVersion::V3, tx_id, *index)])
        }
        ScriptPurpose::Rewarding { reward_account } => {
            PlutusData::Constr(2, vec![credential_data(&reward_account.credential)])
        }
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => PlutusData::Constr(
            3,
            vec![
                PlutusData::integer(*cert_index as i128),
                tx_cert_data_v3(certificate, pv)?,
            ],
        ),
        ScriptPurpose::Voting { voter } => PlutusData::Constr(4, vec![voter_data_v3(voter)]),
        ScriptPurpose::Proposing {
            proposal_index,
            proposal,
        } => PlutusData::Constr(
            5,
            vec![
                PlutusData::integer(*proposal_index as i128),
                proposal_procedure_data_v3(proposal)?,
            ],
        ),
    })
}

fn script_info_data_v3(
    purpose: &ScriptPurpose,
    datum: Option<&PlutusData>,
    pv: Option<(u64, u64)>,
) -> Result<PlutusData, LedgerError> {
    Ok(match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => PlutusData::Constr(
            1,
            vec![
                tx_out_ref_data(PlutusVersion::V3, tx_id, *index),
                maybe_data(datum.cloned()),
            ],
        ),
        ScriptPurpose::Rewarding { reward_account } => {
            PlutusData::Constr(2, vec![credential_data(&reward_account.credential)])
        }
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => PlutusData::Constr(
            3,
            vec![
                PlutusData::integer(*cert_index as i128),
                tx_cert_data_v3(certificate, pv)?,
            ],
        ),
        ScriptPurpose::Voting { voter } => PlutusData::Constr(4, vec![voter_data_v3(voter)]),
        ScriptPurpose::Proposing {
            proposal_index,
            proposal,
        } => PlutusData::Constr(
            5,
            vec![
                PlutusData::integer(*proposal_index as i128),
                proposal_procedure_data_v3(proposal)?,
            ],
        ),
    })
}

fn maybe_data(data: Option<PlutusData>) -> PlutusData {
    match data {
        Some(data) => PlutusData::Constr(0, vec![data]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn guard_legacy_plutus_context_features(
    version: PlutusVersion,
    tx_ctx: &TxContext,
) -> Result<(), LedgerError> {
    if matches!(version, PlutusVersion::V1) && !tx_ctx.resolved_reference_inputs.is_empty() {
        return Err(LedgerError::UnsupportedPlutusContext(
            "Reference inputs require Plutus V2 context support",
        ));
    }
    // B7: V1 rejects inline datums and reference scripts in any resolved output.
    // Reference: upstream `transTxOutV1` → InlineDatumsNotSupported / ReferenceScriptsNotSupported
    if matches!(version, PlutusVersion::V1) {
        let all_outputs = tx_ctx
            .inputs
            .iter()
            .map(|(_, o)| o)
            .chain(tx_ctx.outputs.iter());
        for txout in all_outputs {
            if let yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(b) = txout {
                if matches!(
                    b.datum_option,
                    Some(yggdrasil_ledger::eras::babbage::DatumOption::Inline(_))
                ) {
                    return Err(LedgerError::UnsupportedPlutusContext(
                        "Inline datums not supported in Plutus V1 context",
                    ));
                }
                if b.script_ref.is_some() {
                    return Err(LedgerError::UnsupportedPlutusContext(
                        "Reference scripts not supported in Plutus V1 context",
                    ));
                }
            }
        }
    }
    if tx_ctx.voting_procedures.is_some() {
        return Err(LedgerError::UnsupportedPlutusContext(
            "Voting procedures require Plutus V3 context support",
        ));
    }
    if !tx_ctx.proposal_procedures.is_empty() {
        return Err(LedgerError::UnsupportedPlutusContext(
            "Proposal procedures require Plutus V3 context support",
        ));
    }
    if tx_ctx.current_treasury_value.is_some() {
        return Err(LedgerError::UnsupportedPlutusContext(
            "Current treasury value requires Plutus V3 context support",
        ));
    }
    if tx_ctx.treasury_donation.is_some() {
        return Err(LedgerError::UnsupportedPlutusContext(
            "Treasury donation requires Plutus V3 context support",
        ));
    }
    Ok(())
}

fn certifying_purpose_data(
    _cert_index: u64,
    certificate: &DCert,
) -> Result<PlutusData, LedgerError> {
    let certificate_data = legacy_dcert_data(certificate)?;
    Ok(PlutusData::Constr(3, vec![certificate_data]))
}

/// Upstream `hardforkConwayBootstrapPhase`: PV major == 9.
fn is_conway_bootstrap_phase(protocol_version: Option<(u64, u64)>) -> bool {
    matches!(protocol_version, Some((9, _)))
}

fn tx_cert_data_v3(
    certificate: &DCert,
    protocol_version: Option<(u64, u64)>,
) -> Result<PlutusData, LedgerError> {
    let bootstrap = is_conway_bootstrap_phase(protocol_version);
    match certificate {
        DCert::AccountRegistration(credential) => Ok(PlutusData::Constr(
            0,
            vec![credential_data(credential), maybe_lovelace(None)],
        )),
        DCert::AccountUnregistration(credential) => Ok(PlutusData::Constr(
            1,
            vec![credential_data(credential), maybe_lovelace(None)],
        )),
        DCert::DelegationToStakePool(credential, pool_key_hash) => Ok(PlutusData::Constr(
            2,
            vec![
                credential_data(credential),
                delegatee_stake_data(pool_key_hash),
            ],
        )),
        DCert::AccountRegistrationDeposit(credential, deposit) => {
            // Upstream #4863: PV9 omits deposit (hardforkConwayBootstrapPhase).
            let dep = if bootstrap { None } else { Some(*deposit) };
            Ok(PlutusData::Constr(
                0,
                vec![credential_data(credential), maybe_lovelace(dep)],
            ))
        }
        DCert::AccountUnregistrationDeposit(credential, refund) => {
            // Upstream #4863: PV9 omits refund (hardforkConwayBootstrapPhase).
            let rf = if bootstrap { None } else { Some(*refund) };
            Ok(PlutusData::Constr(
                1,
                vec![credential_data(credential), maybe_lovelace(rf)],
            ))
        }
        DCert::DelegationToDrep(credential, drep) => Ok(PlutusData::Constr(
            2,
            vec![credential_data(credential), delegatee_vote_data(drep)],
        )),
        DCert::DelegationToStakePoolAndDrep(credential, pool_key_hash, drep) => {
            Ok(PlutusData::Constr(
                2,
                vec![
                    credential_data(credential),
                    delegatee_stake_vote_data(pool_key_hash, drep),
                ],
            ))
        }
        DCert::AccountRegistrationDelegationToStakePool(credential, pool_key_hash, deposit) => {
            Ok(PlutusData::Constr(
                3,
                vec![
                    credential_data(credential),
                    delegatee_stake_data(pool_key_hash),
                    PlutusData::integer(*deposit as i128),
                ],
            ))
        }
        DCert::AccountRegistrationDelegationToDrep(credential, drep, deposit) => {
            Ok(PlutusData::Constr(
                3,
                vec![
                    credential_data(credential),
                    delegatee_vote_data(drep),
                    PlutusData::integer(*deposit as i128),
                ],
            ))
        }
        DCert::AccountRegistrationDelegationToStakePoolAndDrep(
            credential,
            pool_key_hash,
            drep,
            deposit,
        ) => Ok(PlutusData::Constr(
            3,
            vec![
                credential_data(credential),
                delegatee_stake_vote_data(pool_key_hash, drep),
                PlutusData::integer(*deposit as i128),
            ],
        )),
        DCert::DrepRegistration(credential, deposit, _) => Ok(PlutusData::Constr(
            4,
            vec![
                drep_credential_data(credential),
                PlutusData::integer(*deposit as i128),
            ],
        )),
        DCert::DrepUpdate(credential, _) => Ok(PlutusData::Constr(
            5,
            vec![drep_credential_data(credential)],
        )),
        DCert::DrepUnregistration(credential, refund) => Ok(PlutusData::Constr(
            6,
            vec![
                drep_credential_data(credential),
                PlutusData::integer(*refund as i128),
            ],
        )),
        DCert::PoolRegistration(pool_params) => Ok(PlutusData::Constr(
            7,
            vec![
                PlutusData::Bytes(pool_params.operator.to_vec()),
                PlutusData::Bytes(pool_params.vrf_keyhash.to_vec()),
            ],
        )),
        DCert::PoolRetirement(pool_key_hash, epoch) => Ok(PlutusData::Constr(
            8,
            vec![
                PlutusData::Bytes(pool_key_hash.to_vec()),
                PlutusData::integer(epoch.0 as i128),
            ],
        )),
        DCert::CommitteeAuthorization(cold, hot) => Ok(PlutusData::Constr(
            9,
            vec![
                committee_credential_data(cold),
                committee_credential_data(hot),
            ],
        )),
        DCert::CommitteeResignation(cold, _) => Ok(PlutusData::Constr(
            10,
            vec![committee_credential_data(cold)],
        )),
        DCert::GenesisDelegation(_, _, _) => Err(LedgerError::UnsupportedCertificate(
            "GenesisDelegation has no Plutus V3 TxCert encoding",
        )),
        DCert::MoveInstantaneousReward(_, _) => Err(LedgerError::UnsupportedCertificate(
            "MoveInstantaneousReward has no Plutus V3 TxCert encoding",
        )),
    }
}

fn maybe_lovelace(value: Option<u64>) -> PlutusData {
    match value {
        Some(value) => PlutusData::Constr(0, vec![PlutusData::integer(value as i128)]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn delegatee_stake_data(pool_key_hash: &[u8; 28]) -> PlutusData {
    PlutusData::Constr(0, vec![PlutusData::Bytes(pool_key_hash.to_vec())])
}

fn delegatee_vote_data(drep: &yggdrasil_ledger::DRep) -> PlutusData {
    PlutusData::Constr(1, vec![drep_data(drep)])
}

fn delegatee_stake_vote_data(
    pool_key_hash: &[u8; 28],
    drep: &yggdrasil_ledger::DRep,
) -> PlutusData {
    PlutusData::Constr(
        2,
        vec![PlutusData::Bytes(pool_key_hash.to_vec()), drep_data(drep)],
    )
}

fn voter_data_v3(voter: &yggdrasil_ledger::Voter) -> PlutusData {
    match voter {
        yggdrasil_ledger::Voter::CommitteeKeyHash(hash) => PlutusData::Constr(
            0,
            vec![committee_credential_data(&StakeCredential::AddrKeyHash(
                *hash,
            ))],
        ),
        yggdrasil_ledger::Voter::CommitteeScript(hash) => PlutusData::Constr(
            0,
            vec![committee_credential_data(&StakeCredential::ScriptHash(
                *hash,
            ))],
        ),
        yggdrasil_ledger::Voter::DRepKeyHash(hash) => PlutusData::Constr(
            1,
            vec![drep_credential_data(&StakeCredential::AddrKeyHash(*hash))],
        ),
        yggdrasil_ledger::Voter::DRepScript(hash) => PlutusData::Constr(
            1,
            vec![drep_credential_data(&StakeCredential::ScriptHash(*hash))],
        ),
        yggdrasil_ledger::Voter::StakePool(hash) => {
            PlutusData::Constr(2, vec![PlutusData::Bytes(hash.to_vec())])
        }
    }
}

fn proposal_procedure_data_v3(
    proposal: &yggdrasil_ledger::ProposalProcedure,
) -> Result<PlutusData, LedgerError> {
    let reward_account = yggdrasil_ledger::RewardAccount::from_bytes(&proposal.reward_account)
        .ok_or_else(|| LedgerError::MalformedProposal(proposal.gov_action.clone()))?;
    Ok(PlutusData::Constr(
        0,
        vec![
            PlutusData::integer(proposal.deposit as i128),
            credential_data(&reward_account.credential),
            gov_action_data_v3(&proposal.gov_action),
        ],
    ))
}

fn gov_action_data_v3(gov_action: &yggdrasil_ledger::GovAction) -> PlutusData {
    match gov_action {
        yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id,
            protocol_param_update,
            guardrails_script_hash,
        } => PlutusData::Constr(
            0,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                PlutusData::Bytes(protocol_param_update.to_cbor_bytes()),
                maybe_script_hash_data(*guardrails_script_hash),
            ],
        ),
        yggdrasil_ledger::GovAction::HardForkInitiation {
            prev_action_id,
            protocol_version,
        } => PlutusData::Constr(
            1,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                protocol_version_data(*protocol_version),
            ],
        ),
        yggdrasil_ledger::GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash,
        } => PlutusData::Constr(
            2,
            vec![
                PlutusData::Map(
                    withdrawals
                        .iter()
                        .map(|(account, lovelace)| {
                            (
                                credential_data(&account.credential),
                                PlutusData::integer(*lovelace as i128),
                            )
                        })
                        .collect(),
                ),
                maybe_script_hash_data(*guardrails_script_hash),
            ],
        ),
        yggdrasil_ledger::GovAction::NoConfidence { prev_action_id } => {
            PlutusData::Constr(3, vec![maybe_gov_action_id_data(prev_action_id.as_ref())])
        }
        yggdrasil_ledger::GovAction::UpdateCommittee {
            prev_action_id,
            members_to_remove,
            members_to_add,
            quorum,
        } => PlutusData::Constr(
            4,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                PlutusData::List(
                    members_to_remove
                        .iter()
                        .map(committee_credential_data)
                        .collect(),
                ),
                PlutusData::Map(
                    members_to_add
                        .iter()
                        .map(|(credential, epoch)| {
                            (
                                committee_credential_data(credential),
                                PlutusData::integer(*epoch as i128),
                            )
                        })
                        .collect(),
                ),
                unit_interval_data(quorum),
            ],
        ),
        yggdrasil_ledger::GovAction::NewConstitution {
            prev_action_id,
            constitution,
        } => PlutusData::Constr(
            5,
            vec![
                maybe_gov_action_id_data(prev_action_id.as_ref()),
                constitution_data_v3(constitution),
            ],
        ),
        yggdrasil_ledger::GovAction::InfoAction => PlutusData::Constr(6, vec![]),
    }
}

fn maybe_gov_action_id_data(gov_action_id: Option<&yggdrasil_ledger::GovActionId>) -> PlutusData {
    match gov_action_id {
        Some(gov_action_id) => PlutusData::Constr(0, vec![gov_action_id_data(gov_action_id)]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn gov_action_id_data(gov_action_id: &yggdrasil_ledger::GovActionId) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::Bytes(gov_action_id.transaction_id.to_vec()),
            PlutusData::integer(gov_action_id.gov_action_index as i128),
        ],
    )
}

fn vote_data_v3(vote: yggdrasil_ledger::Vote) -> PlutusData {
    match vote {
        yggdrasil_ledger::Vote::No => PlutusData::Constr(0, vec![]),
        yggdrasil_ledger::Vote::Yes => PlutusData::Constr(1, vec![]),
        yggdrasil_ledger::Vote::Abstain => PlutusData::Constr(2, vec![]),
    }
}

fn protocol_version_data(protocol_version: (u64, u64)) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::integer(protocol_version.0 as i128),
            PlutusData::integer(protocol_version.1 as i128),
        ],
    )
}

fn maybe_script_hash_data(script_hash: Option<[u8; 28]>) -> PlutusData {
    match script_hash {
        Some(hash) => PlutusData::Constr(0, vec![PlutusData::Bytes(hash.to_vec())]),
        None => PlutusData::Constr(1, vec![]),
    }
}

fn script_hash_from_ref(script_ref: &yggdrasil_ledger::ScriptRef) -> [u8; 28] {
    match &script_ref.0 {
        Script::Native(script) => yggdrasil_ledger::native_script_hash(script),
        Script::PlutusV1(bytes) => {
            yggdrasil_ledger::plutus_validation::plutus_script_hash(PlutusVersion::V1, bytes)
        }
        Script::PlutusV2(bytes) => {
            yggdrasil_ledger::plutus_validation::plutus_script_hash(PlutusVersion::V2, bytes)
        }
        Script::PlutusV3(bytes) => {
            yggdrasil_ledger::plutus_validation::plutus_script_hash(PlutusVersion::V3, bytes)
        }
    }
}

fn unit_interval_data(unit_interval: &yggdrasil_ledger::UnitInterval) -> PlutusData {
    PlutusData::List(vec![
        PlutusData::integer(unit_interval.numerator as i128),
        PlutusData::integer(unit_interval.denominator as i128),
    ])
}

fn constitution_data_v3(constitution: &yggdrasil_ledger::Constitution) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![maybe_script_hash_data(constitution.guardrails_script_hash)],
    )
}

fn drep_data(drep: &yggdrasil_ledger::DRep) -> PlutusData {
    match drep {
        yggdrasil_ledger::DRep::KeyHash(hash) => PlutusData::Constr(
            0,
            vec![drep_credential_data(&StakeCredential::AddrKeyHash(*hash))],
        ),
        yggdrasil_ledger::DRep::ScriptHash(hash) => PlutusData::Constr(
            0,
            vec![drep_credential_data(&StakeCredential::ScriptHash(*hash))],
        ),
        yggdrasil_ledger::DRep::AlwaysAbstain => PlutusData::Constr(1, vec![]),
        yggdrasil_ledger::DRep::AlwaysNoConfidence => PlutusData::Constr(2, vec![]),
    }
}

fn drep_credential_data(credential: &StakeCredential) -> PlutusData {
    PlutusData::Constr(0, vec![credential_data(credential)])
}

fn committee_credential_data(credential: &StakeCredential) -> PlutusData {
    PlutusData::Constr(0, vec![credential_data(credential)])
}

fn legacy_dcert_data(certificate: &DCert) -> Result<PlutusData, LedgerError> {
    match certificate {
        DCert::AccountRegistration(credential) => Ok(PlutusData::Constr(
            0,
            vec![staking_credential_data(credential)],
        )),
        DCert::AccountUnregistration(credential) => Ok(PlutusData::Constr(
            1,
            vec![staking_credential_data(credential)],
        )),
        DCert::DelegationToStakePool(credential, pool_key_hash) => Ok(PlutusData::Constr(
            2,
            vec![
                staking_credential_data(credential),
                PlutusData::Bytes(pool_key_hash.to_vec()),
            ],
        )),
        DCert::AccountRegistrationDeposit(credential, _) => Ok(PlutusData::Constr(
            0,
            vec![staking_credential_data(credential)],
        )),
        DCert::AccountUnregistrationDeposit(credential, _) => Ok(PlutusData::Constr(
            1,
            vec![staking_credential_data(credential)],
        )),
        DCert::PoolRegistration(pool_params) => Ok(PlutusData::Constr(
            3,
            vec![
                PlutusData::Bytes(pool_params.operator.to_vec()),
                PlutusData::Bytes(pool_params.vrf_keyhash.to_vec()),
            ],
        )),
        DCert::PoolRetirement(pool_key_hash, epoch) => Ok(PlutusData::Constr(
            4,
            vec![
                PlutusData::Bytes(pool_key_hash.to_vec()),
                PlutusData::integer(epoch.0 as i128),
            ],
        )),
        DCert::GenesisDelegation(_, _, _) => Ok(PlutusData::Constr(5, vec![])),
        DCert::DelegationToDrep(_, _)
        | DCert::DelegationToStakePoolAndDrep(_, _, _)
        | DCert::AccountRegistrationDelegationToStakePool(_, _, _)
        | DCert::AccountRegistrationDelegationToDrep(_, _, _)
        | DCert::AccountRegistrationDelegationToStakePoolAndDrep(_, _, _, _)
        | DCert::CommitteeAuthorization(_, _)
        | DCert::CommitteeResignation(_, _)
        | DCert::DrepRegistration(_, _, _)
        | DCert::DrepUnregistration(_, _)
        | DCert::DrepUpdate(_, _) => Err(LedgerError::UnsupportedCertificate(
            "Certificate has no Plutus V1/V2 DCert encoding",
        )),
        DCert::MoveInstantaneousReward(_, _) => Err(LedgerError::UnsupportedCertificate(
            "MoveInstantaneousReward has no Plutus V1/V2 DCert encoding",
        )),
    }
}

/// Encode a TxId as PlutusData.
///
/// Plutus V1/V2 inherit the `makeIsDataSchemaIndexed ''TxId [('TxId, 0)]`
/// wrapper from `PlutusLedgerApi.V1.Tx`, while Plutus V3 derives `ToData`
/// through the newtype and therefore uses raw bytes.
fn tx_id_data(version: PlutusVersion, tx_id: &[u8; 32]) -> PlutusData {
    match version {
        PlutusVersion::V1 | PlutusVersion::V2 => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(tx_id.to_vec())])
        }
        PlutusVersion::V3 => PlutusData::Bytes(tx_id.to_vec()),
    }
}

fn tx_out_ref_data(version: PlutusVersion, tx_id: &[u8; 32], index: u64) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            tx_id_data(version, tx_id),
            PlutusData::integer(index as i128),
        ],
    )
}

fn staking_credential_data(credential: &StakeCredential) -> PlutusData {
    PlutusData::Constr(0, vec![credential_data(credential)])
}

fn credential_data(credential: &StakeCredential) -> PlutusData {
    match credential {
        StakeCredential::AddrKeyHash(hash) => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(hash.to_vec())])
        }
        StakeCredential::ScriptHash(hash) => {
            PlutusData::Constr(1, vec![PlutusData::Bytes(hash.to_vec())])
        }
    }
}

/// Convert a [`MachineError`] into a [`LedgerError::PlutusScriptFailed`].
fn map_machine_error(hash: &[u8; 28], err: MachineError) -> LedgerError {
    // Collapse operational errors to the opaque `EvaluationFailure` sentinel
    // before surfacing to the ledger.  Structural errors pass through with
    // full detail so the caller can distinguish budget exhaustion from decode
    // failures, matching upstream Plutus error semantics.
    let ledger_err = err.into_ledger_error();
    match &ledger_err {
        MachineError::FlatDecodeError(reason) => LedgerError::PlutusScriptDecodeError {
            hash: *hash,
            reason: reason.clone(),
        },
        other => LedgerError::PlutusScriptFailed {
            hash: *hash,
            reason: other.to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
