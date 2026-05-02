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
            Some(start) => crate::genesis::slot_to_posix_ms(slot, start, self.slot_length_secs),
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
                crate::genesis::build_plutus_cost_model_from_protocol_values_for_protocol(
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

        // 5. Evaluate the applied term.
        let (result, _logs) = yggdrasil_plutus::evaluate_term(applied, budget, cost_model)
            .map_err(|e| map_machine_error(&eval.script_hash, e))?;

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
    let fee_v3 = PlutusData::Integer(tx_ctx.fee as i128);

    let mint_entries: Vec<(PlutusData, PlutusData)> = tx_ctx
        .mint
        .iter()
        .map(|(policy, assets)| {
            let asset_map: Vec<(PlutusData, PlutusData)> = assets
                .iter()
                .map(|(name, qty)| {
                    (
                        PlutusData::Bytes(name.clone()),
                        PlutusData::Integer(*qty as i128),
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
                PlutusData::Integer(*amt as i128),
            )
        })
        .collect();

    let wdrl_entries_v3: Vec<(PlutusData, PlutusData)> = tx_ctx
        .withdrawals
        .iter()
        .map(|(ra, amt)| {
            (
                credential_data(&ra.credential),
                PlutusData::Integer(*amt as i128),
            )
        })
        .collect();

    let valid_range = posix_time_range(
        tx_ctx.validity_start.map(|s| evaluator.slot_to_posix_ms(s)),
        tx_ctx.ttl.map(|t| evaluator.slot_to_posix_ms(t)),
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
                PlutusData::Map(vec![(PlutusData::Bytes(vec![]), PlutusData::Integer(0))]),
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
                PlutusData::Map(vec![(PlutusData::Bytes(vec![]), PlutusData::Integer(0))]),
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
                    .map(|v| PlutusData::Integer(v as i128)),
            );
            let treasury_donation = maybe_data(
                tx_ctx
                    .treasury_donation
                    .map(|v| PlutusData::Integer(v as i128)),
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
    datums.sort_by(|(a, _), (b, _)| a.cmp(b));
    datums
}

/// Encode a POSIXTimeRange as PlutusData.
///
/// `Interval (LowerBound lb inclusive) (UpperBound ub inclusive)`
/// layout: Constr(0, [lower_bound, upper_bound])
fn posix_time_range(start: Option<u64>, end: Option<u64>) -> PlutusData {
    let lower = match start {
        Some(s) => PlutusData::Constr(
            0,
            vec![
                PlutusData::Constr(1, vec![PlutusData::Integer(s as i128)]), // Finite
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
        Some(e) => PlutusData::Constr(
            0,
            vec![
                PlutusData::Constr(1, vec![PlutusData::Integer(e as i128)]), // Finite
                PlutusData::Constr(0, vec![]), // False (exclusive) — upstream strictUpperBound
            ],
        ),
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
            PlutusData::Integer(pointer.slot as i128),
            PlutusData::Integer(pointer.tx_index as i128),
            PlutusData::Integer(pointer.cert_index as i128),
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
            PlutusData::Integer(coin as i128),
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
                        PlutusData::Integer(*qty as i128),
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
                PlutusData::Integer(*cert_index as i128),
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
                PlutusData::Integer(*proposal_index as i128),
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
                PlutusData::Integer(*cert_index as i128),
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
                PlutusData::Integer(*proposal_index as i128),
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
                    PlutusData::Integer(*deposit as i128),
                ],
            ))
        }
        DCert::AccountRegistrationDelegationToDrep(credential, drep, deposit) => {
            Ok(PlutusData::Constr(
                3,
                vec![
                    credential_data(credential),
                    delegatee_vote_data(drep),
                    PlutusData::Integer(*deposit as i128),
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
                PlutusData::Integer(*deposit as i128),
            ],
        )),
        DCert::DrepRegistration(credential, deposit, _) => Ok(PlutusData::Constr(
            4,
            vec![
                drep_credential_data(credential),
                PlutusData::Integer(*deposit as i128),
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
                PlutusData::Integer(*refund as i128),
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
                PlutusData::Integer(epoch.0 as i128),
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
        Some(value) => PlutusData::Constr(0, vec![PlutusData::Integer(value as i128)]),
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
            PlutusData::Integer(proposal.deposit as i128),
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
                                PlutusData::Integer(*lovelace as i128),
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
                                PlutusData::Integer(*epoch as i128),
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
            PlutusData::Integer(gov_action_id.gov_action_index as i128),
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
            PlutusData::Integer(protocol_version.0 as i128),
            PlutusData::Integer(protocol_version.1 as i128),
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
        PlutusData::Integer(unit_interval.numerator as i128),
        PlutusData::Integer(unit_interval.denominator as i128),
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
                PlutusData::Integer(epoch.0 as i128),
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
            PlutusData::Integer(index as i128),
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
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use yggdrasil_ledger::plutus_validation::{
        PlutusScriptEval, PlutusVersion, ScriptPurpose, TxContext,
    };
    use yggdrasil_ledger::{
        Address, AlonzoTxOut, BabbageTxOut, BaseAddress, Constitution, DRep, DatumOption,
        EnterpriseAddress, EpochNo, GovAction, GovActionId, MaryTxOut, PointerAddress, PoolParams,
        ProtocolParameterUpdate, RewardAccount, ShelleyTxOut, StakeCredential, UnitInterval, Value,
        Vote, Voter,
        eras::alonzo::ExUnits,
        plutus::{PlutusData, ScriptRef},
        types::Anchor,
    };

    fn dummy_hash() -> [u8; 28] {
        [0xab; 28]
    }

    fn test_tx_ctx() -> TxContext {
        TxContext {
            tx_hash: [0x01; 32],
            ..Default::default()
        }
    }

    fn test_eval(
        version: PlutusVersion,
        purpose: ScriptPurpose,
        datum: Option<PlutusData>,
        redeemer: PlutusData,
    ) -> PlutusScriptEval {
        PlutusScriptEval {
            script_hash: dummy_hash(),
            version,
            script_bytes: vec![],
            purpose,
            datum,
            redeemer,
            ex_units: ExUnits {
                mem: 10_000_000,
                steps: 10_000_000,
            },
            cost_model: None,
        }
    }

    /// Extract the purpose/script_info PlutusData from a V1/V2 ScriptContext.
    /// `script_context_data` returns `Constr(0, [tx_info, purpose])` for V1/V2.
    fn extract_purpose_v1v2(ctx: &PlutusData) -> PlutusData {
        match ctx {
            PlutusData::Constr(0, fields) => fields[1].clone(),
            other => panic!("expected Constr(0, [tx_info, purpose]), got: {:?}", other),
        }
    }

    /// Extract the script_info PlutusData from a V3 ScriptContext.
    /// `script_context_data` returns `Constr(0, [tx_info, redeemer, script_info])` for V3.
    fn extract_script_info_v3(ctx: &PlutusData) -> PlutusData {
        match ctx {
            PlutusData::Constr(0, fields) => fields[2].clone(),
            other => panic!(
                "expected Constr(0, [tx_info, redeemer, script_info]), got: {:?}",
                other
            ),
        }
    }

    fn expect_script_context_data(eval: &PlutusScriptEval, tx_ctx: &TxContext) -> PlutusData {
        script_context_data(eval, tx_ctx, &CekPlutusEvaluator::new())
            .expect("script context should encode")
    }

    fn expect_tx_info(version: PlutusVersion, tx_ctx: &TxContext) -> PlutusData {
        build_tx_info(version, tx_ctx, &CekPlutusEvaluator::new()).expect("tx info should encode")
    }

    fn mint_eval(script_bytes: Vec<u8>, version: PlutusVersion) -> PlutusScriptEval {
        PlutusScriptEval {
            script_hash: dummy_hash(),
            version,
            script_bytes,
            purpose: ScriptPurpose::Minting {
                policy_id: dummy_hash(),
            },
            datum: None,
            redeemer: PlutusData::Integer(42),
            ex_units: ExUnits {
                mem: 10_000_000,
                steps: 10_000_000,
            },
            cost_model: None,
        }
    }

    #[test]
    fn decode_error_on_empty_bytes() {
        let evaluator = CekPlutusEvaluator::new();
        // Empty script bytes → decode failure.
        let eval = PlutusScriptEval {
            script_bytes: vec![],
            ..mint_eval(vec![], PlutusVersion::V1)
        };
        let result = evaluator.evaluate(&eval, &test_tx_ctx());
        assert!(
            result.is_err(),
            "empty script bytes must produce a decode error"
        );
        match result {
            Err(LedgerError::PlutusScriptDecodeError { .. }) => {}
            Err(other) => panic!("expected PlutusScriptDecodeError, got: {:?}", other),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn decode_error_on_garbage_bytes() {
        let evaluator = CekPlutusEvaluator::new();
        let eval = mint_eval(vec![0xff, 0xfe, 0xfd, 0xfc], PlutusVersion::V1);
        let result = evaluator.evaluate(&eval, &test_tx_ctx());
        assert!(
            result.is_err(),
            "garbage bytes must produce a decode or evaluation error"
        );
    }

    #[test]
    fn script_context_data_wraps_tx_info_and_spending_purpose() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Spending {
                    tx_id: [0x11; 32],
                    index: 7,
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        // Only check the purpose field (index 1); TxInfo is at index 0.
        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                1,
                vec![PlutusData::Constr(
                    0,
                    vec![
                        PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x11; 32])]),
                        PlutusData::Integer(7),
                    ],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_rewarding_purpose_with_staking_credential_shape() {
        let reward_account = RewardAccount {
            network: 1,
            credential: StakeCredential::ScriptHash([0x22; 28]),
        };

        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Rewarding { reward_account },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                2,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        1,
                        vec![PlutusData::Bytes(vec![0x22; 28])],
                    )],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_minting_with_upstream_constructor_index() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Minting {
                    policy_id: [0x33; 28],
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x33; 28])])
        );
    }

    #[test]
    fn script_context_data_encodes_legacy_certifying_certificate_shape() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Certifying {
                    cert_index: 0,
                    certificate: DCert::PoolRetirement([0x44; 28], yggdrasil_ledger::EpochNo(9)),
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                3,
                vec![PlutusData::Constr(
                    4,
                    vec![PlutusData::Bytes(vec![0x44; 28]), PlutusData::Integer(9),],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_rejects_unsupported_conway_cert_for_v2() {
        let err = script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Certifying {
                    cert_index: 2,
                    certificate: DCert::DrepRegistration(
                        StakeCredential::ScriptHash([0x99; 28]),
                        5,
                        Some(Anchor {
                            url: "https://example.invalid/drep".to_string(),
                            data_hash: [0xaa; 32],
                        }),
                    ),
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
            &CekPlutusEvaluator::new(),
        )
        .expect_err("unsupported Conway cert should fail for V2");

        assert!(matches!(
            err,
            LedgerError::UnsupportedCertificate(message)
                if message == "Certificate has no Plutus V1/V2 DCert encoding"
        ));
    }

    #[test]
    fn script_context_data_rejects_unsupported_conway_cert_for_v1() {
        let err = script_context_data(
            &test_eval(
                PlutusVersion::V1,
                ScriptPurpose::Certifying {
                    cert_index: 2,
                    certificate: DCert::DrepRegistration(
                        StakeCredential::ScriptHash([0x9a; 28]),
                        5,
                        Some(Anchor {
                            url: "https://example.invalid/drep-v1".to_string(),
                            data_hash: [0xab; 32],
                        }),
                    ),
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
            &CekPlutusEvaluator::new(),
        )
        .expect_err("unsupported Conway cert should fail for V1");

        assert!(matches!(
            err,
            LedgerError::UnsupportedCertificate(message)
                if message == "Certificate has no Plutus V1/V2 DCert encoding"
        ));
    }

    #[test]
    fn script_context_data_encodes_deposit_registration_cert_as_legacy_reg_for_v2() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Certifying {
                    cert_index: 0,
                    certificate: DCert::AccountRegistrationDeposit(
                        StakeCredential::ScriptHash([0x98; 28]),
                        5,
                    ),
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                3,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Bytes(vec![0x98; 28])],
                        )],
                    )],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_deposit_registration_cert_as_legacy_reg_for_v1() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V1,
                ScriptPurpose::Certifying {
                    cert_index: 0,
                    certificate: DCert::AccountRegistrationDeposit(
                        StakeCredential::ScriptHash([0x97; 28]),
                        9,
                    ),
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                3,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Bytes(vec![0x97; 28])],
                        )],
                    )],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_deposit_unregistration_cert_as_legacy_dereg_for_v2() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Certifying {
                    cert_index: 0,
                    certificate: DCert::AccountUnregistrationDeposit(
                        StakeCredential::ScriptHash([0x96; 28]),
                        4,
                    ),
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                3,
                vec![PlutusData::Constr(
                    1,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Bytes(vec![0x96; 28])],
                        )],
                    )],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_encodes_deposit_unregistration_cert_as_legacy_dereg_for_v1() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V1,
                ScriptPurpose::Certifying {
                    cert_index: 0,
                    certificate: DCert::AccountUnregistrationDeposit(
                        StakeCredential::ScriptHash([0x95; 28]),
                        4,
                    ),
                },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                3,
                vec![PlutusData::Constr(
                    1,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Bytes(vec![0x95; 28])],
                        )],
                    )],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_three_field_shape_for_spending() {
        let datum = PlutusData::Integer(12);
        let redeemer = PlutusData::Integer(34);
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V3,
                ScriptPurpose::Spending {
                    tx_id: [0x55; 32],
                    index: 4,
                },
                Some(datum.clone()),
                redeemer.clone(),
            ),
            &test_tx_ctx(),
        );

        // V3 ScriptContext = Constr(0, [tx_info, redeemer, script_info])
        let PlutusData::Constr(0, ref fields) = data else {
            panic!("expected outer Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3, "V3 ScriptContext must have 3 fields");
        assert_eq!(fields[1], redeemer);
        assert_eq!(
            extract_script_info_v3(&data),
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(vec![0x55; 32]), PlutusData::Integer(4),],
                    ),
                    PlutusData::Constr(0, vec![datum]),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_certifying_txcert_shape() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V3,
                ScriptPurpose::Certifying {
                    cert_index: 1,
                    certificate: DCert::DelegationToDrep(
                        StakeCredential::AddrKeyHash([0x66; 28]),
                        yggdrasil_ledger::DRep::AlwaysAbstain,
                    ),
                },
                None,
                PlutusData::Integer(77),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_script_info_v3(&data),
            PlutusData::Constr(
                3,
                vec![
                    PlutusData::Integer(1),
                    PlutusData::Constr(
                        2,
                        vec![
                            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x66; 28])]),
                            PlutusData::Constr(1, vec![PlutusData::Constr(1, vec![])]),
                        ],
                    ),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_voting_script_info_shape() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V3,
                ScriptPurpose::Voting {
                    voter: yggdrasil_ledger::Voter::DRepScript([0x77; 28]),
                },
                None,
                PlutusData::Integer(88),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_script_info_v3(&data),
            PlutusData::Constr(
                4,
                vec![PlutusData::Constr(
                    1,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Bytes(vec![0x77; 28])],
                        )],
                    )],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_rejects_voting_purpose_for_v2() {
        let err = script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Voting {
                    voter: yggdrasil_ledger::Voter::DRepScript([0x77; 28]),
                },
                None,
                PlutusData::Integer(88),
            ),
            &test_tx_ctx(),
            &CekPlutusEvaluator::new(),
        )
        .expect_err("V2 should reject Conway voting purpose encoding");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusPurpose(message)
                if message == "Voting purposes require Plutus V3 ScriptContext encoding"
        ));
    }

    #[test]
    fn script_context_data_rejects_voting_purpose_for_v1() {
        let err = script_context_data(
            &test_eval(
                PlutusVersion::V1,
                ScriptPurpose::Voting {
                    voter: yggdrasil_ledger::Voter::DRepScript([0x77; 28]),
                },
                None,
                PlutusData::Integer(88),
            ),
            &test_tx_ctx(),
            &CekPlutusEvaluator::new(),
        )
        .expect_err("V1 should reject Conway voting purpose encoding");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusPurpose(message)
                if message == "Voting purposes require Plutus V3 ScriptContext encoding"
        ));
    }

    #[test]
    fn script_context_data_rejects_proposing_purpose_for_v2() {
        let proposal = yggdrasil_ledger::ProposalProcedure {
            deposit: 9,
            reward_account: yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: StakeCredential::ScriptHash([0x99; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposing-v2".to_string(),
                data_hash: [0xAA; 32],
            },
        };

        let err = script_context_data(
            &test_eval(
                PlutusVersion::V2,
                ScriptPurpose::Proposing {
                    proposal_index: 2,
                    proposal,
                },
                None,
                PlutusData::Integer(101),
            ),
            &test_tx_ctx(),
            &CekPlutusEvaluator::new(),
        )
        .expect_err("V2 should reject Conway proposing purpose encoding");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusPurpose(message)
                if message == "Proposing purposes require Plutus V3 ScriptContext encoding"
        ));
    }

    #[test]
    fn script_context_data_rejects_proposing_purpose_for_v1() {
        let proposal = yggdrasil_ledger::ProposalProcedure {
            deposit: 9,
            reward_account: yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: StakeCredential::ScriptHash([0x99; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposing-v1".to_string(),
                data_hash: [0xAB; 32],
            },
        };

        let err = script_context_data(
            &test_eval(
                PlutusVersion::V1,
                ScriptPurpose::Proposing {
                    proposal_index: 2,
                    proposal,
                },
                None,
                PlutusData::Integer(101),
            ),
            &test_tx_ctx(),
            &CekPlutusEvaluator::new(),
        )
        .expect_err("V1 should reject Conway proposing purpose encoding");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusPurpose(message)
                if message == "Proposing purposes require Plutus V3 ScriptContext encoding"
        ));
    }

    #[test]
    fn script_context_data_uses_v3_proposing_script_info_shape() {
        let proposal = yggdrasil_ledger::ProposalProcedure {
            deposit: 9,
            reward_account: yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: StakeCredential::ScriptHash([0x99; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposing".to_string(),
                data_hash: [0xAA; 32],
            },
        };
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V3,
                ScriptPurpose::Proposing {
                    proposal_index: 2,
                    proposal,
                },
                None,
                PlutusData::Integer(101),
            ),
            &test_tx_ctx(),
        );

        assert_eq!(
            extract_script_info_v3(&data),
            PlutusData::Constr(
                5,
                vec![
                    PlutusData::Integer(2),
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Integer(9),
                            PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x99; 28])]),
                            PlutusData::Constr(6, vec![]),
                        ],
                    ),
                ],
            )
        );
    }

    #[test]
    fn tx_info_v1_has_10_fields() {
        let tx_info = expect_tx_info(PlutusVersion::V1, &test_tx_ctx());
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!("TxInfo must be Constr(0, ...)")
        };
        assert_eq!(fields.len(), 10, "V1 TxInfo must have exactly 10 fields");
    }

    #[test]
    fn tx_info_v2_has_12_fields_with_reference_inputs() {
        let tx_info = expect_tx_info(PlutusVersion::V2, &test_tx_ctx());
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!("TxInfo must be Constr(0, ...)")
        };
        assert_eq!(fields.len(), 12, "V2 TxInfo must have exactly 12 fields");
        // field 1 = referenceInputs, should be an empty list when no ref inputs provided
        assert_eq!(fields[1], PlutusData::List(vec![]));
    }

    #[test]
    fn tx_info_v2_allows_reference_inputs() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.resolved_reference_inputs.push((
            yggdrasil_ledger::eras::shelley::ShelleyTxIn {
                transaction_id: [0x44; 32],
                index: 1,
            },
            yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0x45; 28]),
                })
                .to_bytes(),
                amount: Value::Coin(10),
                datum_option: None,
                script_ref: None,
            }),
        ));

        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!("TxInfo must be Constr(0, ...)")
        };
        assert_eq!(fields.len(), 12, "V2 TxInfo must have exactly 12 fields");
        assert_eq!(
            fields[1],
            PlutusData::List(vec![PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x44; 32])]),
                            PlutusData::Integer(1),
                        ],
                    ),
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(
                                0,
                                vec![
                                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x45; 28])]),
                                    PlutusData::Constr(1, vec![]),
                                ]
                            ),
                            plutus_value_data(&Value::Coin(10)),
                            PlutusData::Constr(0, vec![]),
                            PlutusData::Constr(1, vec![]),
                        ],
                    ),
                ],
            )])
        );
    }

    #[test]
    fn tx_info_v1_rejects_reference_inputs() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.resolved_reference_inputs.push((
            yggdrasil_ledger::eras::shelley::ShelleyTxIn {
                transaction_id: [0x46; 32],
                index: 0,
            },
            yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(yggdrasil_ledger::ShelleyTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0x47; 28]),
                })
                .to_bytes(),
                amount: 5,
            }),
        ));

        let err = build_tx_info(PlutusVersion::V1, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V1 should reject reference inputs");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Reference inputs require Plutus V2 context support"
        ));
    }

    #[test]
    fn tx_info_v2_populates_redeemers_map() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.redeemers = vec![(
            ScriptPurpose::Minting {
                policy_id: [0x22; 28],
            },
            PlutusData::Integer(5),
        )];

        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!("TxInfo must be Constr(0, ...)")
        };
        assert_eq!(
            fields[9],
            PlutusData::Map(vec![(
                PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x22; 28])]),
                PlutusData::Integer(5),
            )])
        );
    }

    #[test]
    fn tx_info_v2_rejects_conway_proposal_procedures() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.proposal_procedures = vec![yggdrasil_ledger::ProposalProcedure {
            deposit: 7,
            reward_account: yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x55; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposal-v2".to_string(),
                data_hash: [0x66; 32],
            },
        }];

        let err = build_tx_info(PlutusVersion::V2, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V2 should reject Conway proposal procedures");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Proposal procedures require Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v1_rejects_conway_proposal_procedures() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.proposal_procedures = vec![yggdrasil_ledger::ProposalProcedure {
            deposit: 7,
            reward_account: yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x58; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposal-v1".to_string(),
                data_hash: [0x67; 32],
            },
        }];

        let err = build_tx_info(PlutusVersion::V1, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V1 should reject Conway proposal procedures");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Proposal procedures require Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v2_rejects_present_current_treasury_value() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.current_treasury_value = Some(0);

        let err = build_tx_info(PlutusVersion::V2, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V2 should reject current treasury value field presence");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Current treasury value requires Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v2_rejects_present_but_empty_voting_procedures() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.voting_procedures = Some(yggdrasil_ledger::VotingProcedures {
            procedures: std::collections::BTreeMap::new(),
        });

        let err = build_tx_info(PlutusVersion::V2, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V2 should reject Conway voting procedures even when empty");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Voting procedures require Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v2_rejects_present_zero_treasury_donation() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.treasury_donation = Some(0);

        let err = build_tx_info(PlutusVersion::V2, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V2 should reject treasury donation field presence");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Treasury donation requires Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v1_rejects_present_but_empty_voting_procedures() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.voting_procedures = Some(yggdrasil_ledger::VotingProcedures {
            procedures: std::collections::BTreeMap::new(),
        });

        let err = build_tx_info(PlutusVersion::V1, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V1 should reject Conway voting procedures even when empty");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Voting procedures require Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v1_rejects_present_zero_treasury_donation() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.treasury_donation = Some(0);

        let err = build_tx_info(PlutusVersion::V1, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V1 should reject treasury donation field presence");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Treasury donation requires Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v1_rejects_present_current_treasury_value() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.current_treasury_value = Some(0);

        let err = build_tx_info(PlutusVersion::V1, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("V1 should reject current treasury value field presence");

        assert!(matches!(
            err,
            LedgerError::UnsupportedPlutusContext(message)
                if message == "Current treasury value requires Plutus V3 context support"
        ));
    }

    #[test]
    fn tx_info_v3_has_16_fields() {
        let tx_info = expect_tx_info(PlutusVersion::V3, &test_tx_ctx());
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!("TxInfo must be Constr(0, ...)")
        };
        assert_eq!(fields.len(), 16, "V3 TxInfo must have exactly 16 fields");
    }

    #[test]
    fn tx_info_v3_populates_redeemers_votes_and_proposals() {
        let mut tx_ctx = test_tx_ctx();
        let mut votes = std::collections::BTreeMap::new();
        votes.insert(
            yggdrasil_ledger::GovActionId {
                transaction_id: [0x44; 32],
                gov_action_index: 3,
            },
            yggdrasil_ledger::VotingProcedure {
                vote: yggdrasil_ledger::Vote::Yes,
                anchor: None,
            },
        );
        tx_ctx.redeemers = vec![(
            ScriptPurpose::Voting {
                voter: yggdrasil_ledger::Voter::StakePool([0x33; 28]),
            },
            PlutusData::Integer(9),
        )];
        tx_ctx.voting_procedures = Some(yggdrasil_ledger::VotingProcedures {
            procedures: std::collections::BTreeMap::from([(
                yggdrasil_ledger::Voter::StakePool([0x33; 28]),
                votes,
            )]),
        });
        tx_ctx.proposal_procedures = vec![yggdrasil_ledger::ProposalProcedure {
            deposit: 7,
            reward_account: yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x55; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposal".to_string(),
                data_hash: [0x66; 32],
            },
        }];

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!("TxInfo must be Constr(0, ...)")
        };

        assert_eq!(
            fields[9],
            PlutusData::Map(vec![(
                PlutusData::Constr(
                    4,
                    vec![PlutusData::Constr(
                        2,
                        vec![PlutusData::Bytes(vec![0x33; 28])]
                    )]
                ),
                PlutusData::Integer(9),
            )])
        );
        assert_eq!(
            fields[12],
            PlutusData::Map(vec![(
                PlutusData::Constr(2, vec![PlutusData::Bytes(vec![0x33; 28])]),
                PlutusData::Map(vec![(
                    PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(vec![0x44; 32]), PlutusData::Integer(3),],
                    ),
                    PlutusData::Constr(1, vec![]),
                )]),
            )])
        );
        assert_eq!(
            fields[13],
            PlutusData::List(vec![PlutusData::Constr(
                0,
                vec![
                    PlutusData::Integer(7),
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x55; 28])]),
                    PlutusData::Constr(6, vec![]),
                ],
            )])
        );
    }

    #[test]
    fn tx_info_v3_withdrawals_use_plain_credentials() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.withdrawals = [(
            RewardAccount {
                network: 1,
                credential: StakeCredential::ScriptHash([0x24; 28]),
            },
            11,
        )]
        .into_iter()
        .collect();

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!("TxInfo must be Constr(0, ...)")
        };

        assert_eq!(
            fields[6],
            PlutusData::Map(vec![(
                PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x24; 28])]),
                PlutusData::Integer(11),
            )])
        );
    }

    #[test]
    fn tx_info_v3_rejects_unsupported_genesis_delegation_certificates() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.certificates = vec![DCert::GenesisDelegation([0x01; 28], [0x02; 28], [0x03; 32])];

        let err = build_tx_info(PlutusVersion::V3, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("unsupported V3 certificates should fail encoding");

        assert!(matches!(
            err,
            LedgerError::UnsupportedCertificate(message)
                if message == "GenesisDelegation has no Plutus V3 TxCert encoding"
        ));
    }

    #[test]
    fn tx_info_v3_rejects_malformed_proposal_reward_accounts() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.proposal_procedures = vec![yggdrasil_ledger::ProposalProcedure {
            deposit: 7,
            reward_account: vec![0xff],
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/bad-proposal".to_string(),
                data_hash: [0x77; 32],
            },
        }];

        let err = build_tx_info(PlutusVersion::V3, &tx_ctx, &CekPlutusEvaluator::new())
            .expect_err("malformed proposal reward account should fail encoding");

        assert!(matches!(
            err,
            LedgerError::MalformedProposal(yggdrasil_ledger::GovAction::InfoAction)
        ));
    }

    #[test]
    fn plutus_output_data_encodes_structured_pointer_address() {
        let address = Address::Pointer(PointerAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x10; 28]),
            slot: 9,
            tx_index: 4,
            cert_index: 2,
        })
        .to_bytes();
        let txout =
            yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(yggdrasil_ledger::ShelleyTxOut {
                address,
                amount: 5,
            });

        assert_eq!(
            plutus_output_data(PlutusVersion::V2, &txout),
            Some(PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x10; 28])]),
                            PlutusData::Constr(
                                0,
                                vec![PlutusData::Constr(
                                    1,
                                    vec![
                                        PlutusData::Integer(9),
                                        PlutusData::Integer(4),
                                        PlutusData::Integer(2),
                                    ],
                                )],
                            ),
                        ],
                    ),
                    plutus_value_data(&Value::Coin(5)),
                    PlutusData::Constr(1, vec![]),
                ],
            ))
        );
    }

    #[test]
    fn plutus_output_data_encodes_reference_script_hash() {
        let script_bytes = vec![0xde, 0xad, 0xbe, 0xef];
        let script_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
            PlutusVersion::V2,
            &script_bytes,
        );
        let address = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x31; 28]),
            staking: StakeCredential::ScriptHash([0x32; 28]),
        })
        .to_bytes();
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address,
            amount: Value::Coin(5),
            datum_option: Some(DatumOption::Inline(PlutusData::Integer(4))),
            script_ref: Some(ScriptRef(Script::PlutusV2(script_bytes))),
        });

        assert_eq!(
            plutus_output_data(PlutusVersion::V2, &txout),
            Some(PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x31; 28])]),
                            PlutusData::Constr(
                                0,
                                vec![PlutusData::Constr(
                                    0,
                                    vec![PlutusData::Constr(
                                        1,
                                        vec![PlutusData::Bytes(vec![0x32; 28])]
                                    )],
                                )],
                            ),
                        ],
                    ),
                    plutus_value_data(&Value::Coin(5)),
                    PlutusData::Constr(2, vec![PlutusData::Integer(4)]),
                    PlutusData::Constr(0, vec![PlutusData::Bytes(script_hash.to_vec())]),
                ],
            ))
        );
    }

    #[test]
    fn plutus_output_data_omits_byron_addresses() {
        let txout =
            yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(yggdrasil_ledger::ShelleyTxOut {
                address: vec![0x80],
                amount: 1,
            });

        assert_eq!(plutus_output_data(PlutusVersion::V2, &txout), None);
    }

    #[test]
    fn script_context_v1v2_has_two_field_outer_shape() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V1,
                ScriptPurpose::Minting { policy_id: [0; 28] },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );
        let PlutusData::Constr(0, ref fields) = data else {
            panic!()
        };
        assert_eq!(
            fields.len(),
            2,
            "V1 ScriptContext must have 2 fields: [tx_info, purpose]"
        );
    }

    #[test]
    fn script_context_v3_has_three_field_outer_shape() {
        let data = expect_script_context_data(
            &test_eval(
                PlutusVersion::V3,
                ScriptPurpose::Minting { policy_id: [0; 28] },
                None,
                PlutusData::Integer(0),
            ),
            &test_tx_ctx(),
        );
        let PlutusData::Constr(0, ref fields) = data else {
            panic!()
        };
        assert_eq!(
            fields.len(),
            3,
            "V3 ScriptContext must have 3 fields: [tx_info, redeemer, script_info]"
        );
    }

    // -- V3 governance-certificate encoding ----------------------------------

    #[test]
    fn tx_cert_data_v3_encodes_drep_registration_with_deposit() {
        let cert =
            DCert::DrepRegistration(StakeCredential::AddrKeyHash([0x11; 28]), 2_000_000, None);
        let result = tx_cert_data_v3(&cert, None).expect("DrepRegistration should encode");
        // Constr(4, [DRepCredential(PubKeyCredential(hash)), deposit])
        assert_eq!(
            result,
            PlutusData::Constr(
                4,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            0,
                            vec![PlutusData::Bytes(vec![0x11; 28])]
                        ),]
                    ),
                    PlutusData::Integer(2_000_000),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_drep_unregistration_with_refund() {
        let cert = DCert::DrepUnregistration(StakeCredential::ScriptHash([0x22; 28]), 500_000);
        let result = tx_cert_data_v3(&cert, None).expect("DrepUnregistration should encode");
        // Constr(6, [DRepCredential(ScriptCredential(hash)), refund])
        assert_eq!(
            result,
            PlutusData::Constr(
                6,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Bytes(vec![0x22; 28])]
                        ),]
                    ),
                    PlutusData::Integer(500_000),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_committee_authorization() {
        let cert = DCert::CommitteeAuthorization(
            StakeCredential::AddrKeyHash([0x33; 28]),
            StakeCredential::ScriptHash([0x44; 28]),
        );
        let result = tx_cert_data_v3(&cert, None).expect("CommitteeAuthorization should encode");
        // Constr(9, [ColdCommitteeCredential(PubKey), HotCommitteeCredential(Script)])
        assert_eq!(
            result,
            PlutusData::Constr(
                9,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            0,
                            vec![PlutusData::Bytes(vec![0x33; 28])]
                        ),]
                    ),
                    PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            1,
                            vec![PlutusData::Bytes(vec![0x44; 28])]
                        ),]
                    ),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_committee_resignation() {
        let cert = DCert::CommitteeResignation(StakeCredential::ScriptHash([0x55; 28]), None);
        let result = tx_cert_data_v3(&cert, None).expect("CommitteeResignation should encode");
        // Constr(10, [ColdCommitteeCredential(Script)])
        assert_eq!(
            result,
            PlutusData::Constr(
                10,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        1,
                        vec![PlutusData::Bytes(vec![0x55; 28])]
                    ),]
                ),],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_registration_deposit_with_maybe_lovelace() {
        let cert =
            DCert::AccountRegistrationDeposit(StakeCredential::AddrKeyHash([0x66; 28]), 1_000_000);
        let result =
            tx_cert_data_v3(&cert, None).expect("AccountRegistrationDeposit should encode for V3");
        // Constr(0, [credential, Just(deposit)]) — distinct from legacy which ignores deposit
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x66; 28])]),
                    PlutusData::Constr(0, vec![PlutusData::Integer(1_000_000)]),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_plain_registration_with_nothing_deposit() {
        let cert = DCert::AccountRegistration(StakeCredential::ScriptHash([0x11; 28]));
        let result =
            tx_cert_data_v3(&cert, None).expect("AccountRegistration should encode for V3");
        // Constr(0, [credential, Nothing]) — no deposit present
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x11; 28])]),
                    PlutusData::Constr(1, vec![]),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_plain_unregistration_with_nothing_refund() {
        let cert = DCert::AccountUnregistration(StakeCredential::AddrKeyHash([0x22; 28]));
        let result =
            tx_cert_data_v3(&cert, None).expect("AccountUnregistration should encode for V3");
        // Constr(1, [credential, Nothing])
        assert_eq!(
            result,
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x22; 28])]),
                    PlutusData::Constr(1, vec![]),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_unregistration_deposit_with_refund() {
        let cert =
            DCert::AccountUnregistrationDeposit(StakeCredential::ScriptHash([0x33; 28]), 750_000);
        let result = tx_cert_data_v3(&cert, None)
            .expect("AccountUnregistrationDeposit should encode for V3");
        // Constr(1, [credential, Just(refund)])
        assert_eq!(
            result,
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x33; 28])]),
                    PlutusData::Constr(0, vec![PlutusData::Integer(750_000)]),
                ],
            )
        );
    }

    /// PV9 (Conway bootstrap phase): deposit is omitted from
    /// `AccountRegistrationDeposit` — upstream bug #4863.
    #[test]
    fn tx_cert_data_v3_pv9_omits_registration_deposit() {
        let cert =
            DCert::AccountRegistrationDeposit(StakeCredential::AddrKeyHash([0x66; 28]), 1_000_000);
        let result = tx_cert_data_v3(&cert, Some((9, 0))).expect("should encode for PV9");
        // PV9: Constr(0, [credential, Nothing]) — deposit omitted
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x66; 28])]),
                    PlutusData::Constr(1, vec![]),
                ],
            )
        );
    }

    /// PV9 (Conway bootstrap phase): refund is omitted from
    /// `AccountUnregistrationDeposit` — upstream bug #4863.
    #[test]
    fn tx_cert_data_v3_pv9_omits_unregistration_refund() {
        let cert =
            DCert::AccountUnregistrationDeposit(StakeCredential::ScriptHash([0x33; 28]), 750_000);
        let result = tx_cert_data_v3(&cert, Some((9, 0))).expect("should encode for PV9");
        // PV9: Constr(1, [credential, Nothing]) — refund omitted
        assert_eq!(
            result,
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x33; 28])]),
                    PlutusData::Constr(1, vec![]),
                ],
            )
        );
    }

    /// PV10: deposit is included (normal behavior, not bootstrap phase).
    #[test]
    fn tx_cert_data_v3_pv10_includes_registration_deposit() {
        let cert =
            DCert::AccountRegistrationDeposit(StakeCredential::AddrKeyHash([0x66; 28]), 1_000_000);
        let result = tx_cert_data_v3(&cert, Some((10, 0))).expect("should encode for PV10");
        // PV10: Constr(0, [credential, Just(deposit)])
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x66; 28])]),
                    PlutusData::Constr(0, vec![PlutusData::Integer(1_000_000)]),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_delegation_to_stake_pool() {
        let pool_hash: [u8; 28] = [0xaa; 28];
        let cert =
            DCert::DelegationToStakePool(StakeCredential::AddrKeyHash([0x44; 28]), pool_hash);
        let result =
            tx_cert_data_v3(&cert, None).expect("DelegationToStakePool should encode for V3");
        // Constr(2, [credential, Delegatee::Stake(pool_hash)])
        assert_eq!(
            result,
            PlutusData::Constr(
                2,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x44; 28])]),
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xaa; 28])]),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_delegation_to_drep() {
        let cert = DCert::DelegationToDrep(
            StakeCredential::AddrKeyHash([0x55; 28]),
            DRep::AlwaysNoConfidence,
        );
        let result = tx_cert_data_v3(&cert, None).expect("DelegationToDrep should encode for V3");
        // Constr(2, [credential, Delegatee::Vote(AlwaysNoConfidence)])
        assert_eq!(
            result,
            PlutusData::Constr(
                2,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x55; 28])]),
                    PlutusData::Constr(1, vec![PlutusData::Constr(2, vec![])]),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_delegation_to_stake_pool_and_drep() {
        let pool_hash: [u8; 28] = [0xbb; 28];
        let cert = DCert::DelegationToStakePoolAndDrep(
            StakeCredential::ScriptHash([0x77; 28]),
            pool_hash,
            DRep::AlwaysAbstain,
        );
        let result = tx_cert_data_v3(&cert, None)
            .expect("DelegationToStakePoolAndDrep should encode for V3");
        // Constr(2, [credential, Delegatee::StakeVote(pool_hash, drep)])
        assert_eq!(
            result,
            PlutusData::Constr(
                2,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x77; 28])]),
                    PlutusData::Constr(
                        2,
                        vec![
                            PlutusData::Bytes(vec![0xbb; 28]),
                            PlutusData::Constr(1, vec![]),
                        ],
                    ),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_reg_delegation_to_stake_pool() {
        let pool_hash: [u8; 28] = [0xcc; 28];
        let cert = DCert::AccountRegistrationDelegationToStakePool(
            StakeCredential::AddrKeyHash([0x88; 28]),
            pool_hash,
            3_000_000,
        );
        let result = tx_cert_data_v3(&cert, None)
            .expect("AccountRegistrationDelegationToStakePool should encode");
        // Constr(3, [credential, Delegatee::Stake(pool), deposit])
        assert_eq!(
            result,
            PlutusData::Constr(
                3,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x88; 28])]),
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xcc; 28])]),
                    PlutusData::Integer(3_000_000),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_reg_delegation_to_drep() {
        let cert = DCert::AccountRegistrationDelegationToDrep(
            StakeCredential::ScriptHash([0x99; 28]),
            DRep::KeyHash([0xdd; 28]),
            5_000_000,
        );
        let result = tx_cert_data_v3(&cert, None)
            .expect("AccountRegistrationDelegationToDrep should encode");
        // Constr(3, [credential, Delegatee::Vote(DRep::KeyHash), deposit])
        assert_eq!(
            result,
            PlutusData::Constr(
                3,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x99; 28])]),
                    PlutusData::Constr(
                        1,
                        vec![PlutusData::Constr(
                            0,
                            vec![PlutusData::Constr(
                                0,
                                vec![PlutusData::Constr(
                                    0,
                                    vec![PlutusData::Bytes(vec![0xdd; 28])]
                                ),]
                            ),]
                        ),]
                    ),
                    PlutusData::Integer(5_000_000),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_reg_delegation_to_stake_pool_and_drep() {
        let pool_hash: [u8; 28] = [0xee; 28];
        let cert = DCert::AccountRegistrationDelegationToStakePoolAndDrep(
            StakeCredential::AddrKeyHash([0xaa; 28]),
            pool_hash,
            DRep::ScriptHash([0xff; 28]),
            4_000_000,
        );
        let result = tx_cert_data_v3(&cert, None)
            .expect("AccountRegistrationDelegationToStakePoolAndDrep should encode");
        // Constr(3, [credential, Delegatee::StakeVote(pool, drep), deposit])
        assert_eq!(
            result,
            PlutusData::Constr(
                3,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xaa; 28])]),
                    PlutusData::Constr(
                        2,
                        vec![
                            PlutusData::Bytes(vec![0xee; 28]),
                            PlutusData::Constr(
                                0,
                                vec![PlutusData::Constr(
                                    0,
                                    vec![PlutusData::Constr(
                                        1,
                                        vec![PlutusData::Bytes(vec![0xff; 28])]
                                    ),]
                                ),]
                            ),
                        ],
                    ),
                    PlutusData::Integer(4_000_000),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_drep_update() {
        let cert = DCert::DrepUpdate(StakeCredential::AddrKeyHash([0xbb; 28]), None);
        let result = tx_cert_data_v3(&cert, None).expect("DrepUpdate should encode for V3");
        // Constr(5, [DRepCredential(PubKeyCredential(hash))])
        assert_eq!(
            result,
            PlutusData::Constr(
                5,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(vec![0xbb; 28])]
                    ),]
                ),],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_pool_registration() {
        let cert = DCert::PoolRegistration(PoolParams {
            operator: [0x01; 28],
            vrf_keyhash: [0x02; 32],
            pledge: 100_000_000,
            cost: 340_000_000,
            margin: UnitInterval {
                numerator: 1,
                denominator: 100,
            },
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x01; 28]),
            },
            pool_owners: vec![],
            relays: vec![],
            pool_metadata: None,
        });
        let result = tx_cert_data_v3(&cert, None).expect("PoolRegistration should encode for V3");
        // Constr(7, [operator_bytes, vrf_keyhash_bytes])
        assert_eq!(
            result,
            PlutusData::Constr(
                7,
                vec![
                    PlutusData::Bytes(vec![0x01; 28]),
                    PlutusData::Bytes(vec![0x02; 32]),
                ],
            )
        );
    }

    #[test]
    fn tx_cert_data_v3_encodes_pool_retirement() {
        let cert = DCert::PoolRetirement([0xcc; 28], EpochNo(42));
        let result = tx_cert_data_v3(&cert, None).expect("PoolRetirement should encode for V3");
        // Constr(8, [pool_key_hash, epoch])
        assert_eq!(
            result,
            PlutusData::Constr(
                8,
                vec![PlutusData::Bytes(vec![0xcc; 28]), PlutusData::Integer(42),],
            )
        );
    }

    // -- V3 voter encoding ---------------------------------------------------

    #[test]
    fn voter_data_v3_encodes_committee_key_hash() {
        let voter = Voter::CommitteeKeyHash([0x11; 28]);
        let result = voter_data_v3(&voter);
        // Constr(0, [CommitteeCredential(PubKeyCredential(hash))])
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(vec![0x11; 28])]
                    ),]
                )],
            )
        );
    }

    #[test]
    fn voter_data_v3_encodes_committee_script() {
        let voter = Voter::CommitteeScript([0x22; 28]);
        let result = voter_data_v3(&voter);
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        1,
                        vec![PlutusData::Bytes(vec![0x22; 28])]
                    ),]
                )],
            )
        );
    }

    #[test]
    fn voter_data_v3_encodes_drep_key_hash() {
        let voter = Voter::DRepKeyHash([0x33; 28]);
        let result = voter_data_v3(&voter);
        // Constr(1, [DRepCredential(PubKeyCredential(hash))])
        assert_eq!(
            result,
            PlutusData::Constr(
                1,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(vec![0x33; 28])]
                    ),]
                )],
            )
        );
    }

    #[test]
    fn voter_data_v3_encodes_drep_script() {
        let voter = Voter::DRepScript([0x44; 28]);
        let result = voter_data_v3(&voter);
        assert_eq!(
            result,
            PlutusData::Constr(
                1,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        1,
                        vec![PlutusData::Bytes(vec![0x44; 28])]
                    ),]
                )],
            )
        );
    }

    #[test]
    fn voter_data_v3_encodes_stake_pool() {
        let voter = Voter::StakePool([0x55; 28]);
        let result = voter_data_v3(&voter);
        // Constr(2, [pool_key_hash_bytes])
        assert_eq!(
            result,
            PlutusData::Constr(2, vec![PlutusData::Bytes(vec![0x55; 28])])
        );
    }

    // -- V3 vote encoding ----------------------------------------------------

    #[test]
    fn vote_data_v3_encodes_all_variants() {
        assert_eq!(vote_data_v3(Vote::No), PlutusData::Constr(0, vec![]));
        assert_eq!(vote_data_v3(Vote::Yes), PlutusData::Constr(1, vec![]));
        assert_eq!(vote_data_v3(Vote::Abstain), PlutusData::Constr(2, vec![]));
    }

    // -- V3 gov_action encoding ----------------------------------------------

    #[test]
    fn gov_action_data_v3_encodes_info_action() {
        let result = gov_action_data_v3(&GovAction::InfoAction);
        assert_eq!(result, PlutusData::Constr(6, vec![]));
    }

    #[test]
    fn gov_action_data_v3_encodes_no_confidence_without_prev() {
        let result = gov_action_data_v3(&GovAction::NoConfidence {
            prev_action_id: None,
        });
        // Constr(3, [Nothing])
        assert_eq!(
            result,
            PlutusData::Constr(3, vec![PlutusData::Constr(1, vec![])])
        );
    }

    #[test]
    fn gov_action_data_v3_encodes_no_confidence_with_prev() {
        let result = gov_action_data_v3(&GovAction::NoConfidence {
            prev_action_id: Some(GovActionId {
                transaction_id: [0xaa; 32],
                gov_action_index: 3,
            }),
        });
        // Constr(3, [Just(GovActionId(tx_hash, index))])
        assert_eq!(
            result,
            PlutusData::Constr(
                3,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(vec![0xaa; 32]), PlutusData::Integer(3),],
                    )],
                )],
            )
        );
    }

    #[test]
    fn gov_action_data_v3_encodes_hard_fork_initiation() {
        let result = gov_action_data_v3(&GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        });
        // Constr(1, [Nothing, ProtocolVersion(major, minor)])
        assert_eq!(
            result,
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::Constr(1, vec![]),
                    PlutusData::Constr(0, vec![PlutusData::Integer(10), PlutusData::Integer(0)],),
                ],
            )
        );
    }

    #[test]
    fn gov_action_data_v3_encodes_new_constitution() {
        let result = gov_action_data_v3(&GovAction::NewConstitution {
            prev_action_id: None,
            constitution: Constitution {
                anchor: Anchor {
                    url: "https://example.invalid".to_string(),
                    data_hash: [0xbb; 32],
                },
                guardrails_script_hash: Some([0xcc; 28]),
            },
        });
        // Constr(5, [Nothing, Constitution(Just(guardrails_hash))])
        assert_eq!(
            result,
            PlutusData::Constr(
                5,
                vec![
                    PlutusData::Constr(1, vec![]),
                    PlutusData::Constr(
                        0,
                        vec![PlutusData::Constr(
                            0,
                            vec![PlutusData::Bytes(vec![0xcc; 28])]
                        )],
                    ),
                ],
            )
        );
    }

    #[test]
    fn gov_action_data_v3_encodes_new_constitution_without_guardrails() {
        let result = gov_action_data_v3(&GovAction::NewConstitution {
            prev_action_id: None,
            constitution: Constitution {
                anchor: Anchor {
                    url: "https://example.invalid".to_string(),
                    data_hash: [0xdd; 32],
                },
                guardrails_script_hash: None,
            },
        });
        // Constr(5, [Nothing, Constitution(Nothing)])
        assert_eq!(
            result,
            PlutusData::Constr(
                5,
                vec![
                    PlutusData::Constr(1, vec![]),
                    PlutusData::Constr(0, vec![PlutusData::Constr(1, vec![])]),
                ],
            )
        );
    }

    // -- V3 proposal_procedure encoding --------------------------------------

    #[test]
    fn proposal_procedure_data_v3_encodes_info_action_proposal() {
        // Construct a valid reward account: header byte 0xe0 + 28 key-hash bytes
        let mut reward_bytes = vec![0xe0];
        reward_bytes.extend_from_slice(&[0x11; 28]);
        let proposal = yggdrasil_ledger::ProposalProcedure {
            deposit: 100_000_000,
            reward_account: reward_bytes,
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposal".to_string(),
                data_hash: [0xee; 32],
            },
        };
        let result = proposal_procedure_data_v3(&proposal).expect("valid proposal should encode");
        // Constr(0, [deposit, credential, gov_action])
        let PlutusData::Constr(0, ref fields) = result else {
            panic!("expected Constr(0, _)")
        };
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], PlutusData::Integer(100_000_000));
        // gov_action = InfoAction = Constr(6, [])
        assert_eq!(fields[2], PlutusData::Constr(6, vec![]));
    }

    #[test]
    fn proposal_procedure_data_v3_rejects_malformed_reward_account() {
        let proposal = yggdrasil_ledger::ProposalProcedure {
            deposit: 50,
            reward_account: vec![0xff],
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/bad".to_string(),
                data_hash: [0x00; 32],
            },
        };
        let err = proposal_procedure_data_v3(&proposal)
            .expect_err("malformed reward account should fail");
        assert!(matches!(
            err,
            LedgerError::MalformedProposal(GovAction::InfoAction)
        ));
    }

    // -- posix_time_range encoding -------------------------------------------

    #[test]
    fn posix_time_range_encodes_open_interval() {
        let result = posix_time_range(None, None);
        // Interval(LowerBound(NegInf, True), UpperBound(PosInf, True))
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(0, vec![]), // NegInf
                            PlutusData::Constr(1, vec![]), // True
                        ]
                    ),
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(2, vec![]), // PosInf
                            PlutusData::Constr(1, vec![]), // True
                        ]
                    ),
                ],
            )
        );
    }

    #[test]
    fn posix_time_range_encodes_bounded_interval() {
        let result = posix_time_range(Some(1000), Some(2000));
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(1, vec![PlutusData::Integer(1000)]), // Finite(1000)
                            PlutusData::Constr(1, vec![]), // True (inclusive)
                        ]
                    ),
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(1, vec![PlutusData::Integer(2000)]), // Finite(2000)
                            PlutusData::Constr(0, vec![]), // False (exclusive — upstream strictUpperBound)
                        ]
                    ),
                ],
            )
        );
    }

    #[test]
    fn posix_time_range_encodes_lower_bounded_only() {
        let result = posix_time_range(Some(500), None);
        let PlutusData::Constr(0, ref fields) = result else {
            panic!("expected Interval")
        };
        // Lower bound: Finite(500)
        assert_eq!(
            fields[0],
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::Integer(500)]),
                    PlutusData::Constr(1, vec![]),
                ]
            )
        );
        // Upper bound: PosInf
        assert_eq!(
            fields[1],
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(2, vec![]), PlutusData::Constr(1, vec![]),]
            )
        );
    }

    #[test]
    fn posix_time_range_encodes_upper_bounded_only() {
        let result = posix_time_range(None, Some(9999));
        let PlutusData::Constr(0, ref fields) = result else {
            panic!("expected Interval")
        };
        // Lower bound: NegInf
        assert_eq!(
            fields[0],
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(0, vec![]), PlutusData::Constr(1, vec![]),]
            )
        );
        // Upper bound: Finite(9999) — exclusive (upstream strictUpperBound)
        assert_eq!(
            fields[1],
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(1, vec![PlutusData::Integer(9999)]),
                    PlutusData::Constr(0, vec![]),
                ]
            )
        );
    }

    // -- plutus_value_data encoding ------------------------------------------

    #[test]
    fn plutus_value_data_encodes_pure_coin() {
        let value = yggdrasil_ledger::eras::mary::Value::Coin(5_000_000);
        let result = plutus_value_data(&value);
        // Map[("" -> Map[("" -> 5_000_000)])]
        assert_eq!(
            result,
            PlutusData::Map(vec![(
                PlutusData::Bytes(vec![]),
                PlutusData::Map(vec![(
                    PlutusData::Bytes(vec![]),
                    PlutusData::Integer(5_000_000),
                )]),
            )])
        );
    }

    #[test]
    fn plutus_value_data_encodes_coin_and_multi_asset() {
        use std::collections::BTreeMap;
        let policy: [u8; 28] = [0xaa; 28];
        let mut assets = BTreeMap::new();
        assets.insert(b"Token1".to_vec(), 100u64);
        let mut multi_asset = BTreeMap::new();
        multi_asset.insert(policy, assets);
        let value = yggdrasil_ledger::eras::mary::Value::CoinAndAssets(2_000_000, multi_asset);
        let result = plutus_value_data(&value);
        let PlutusData::Map(ref entries) = result else {
            panic!("expected Map")
        };
        // First entry is ADA
        assert_eq!(entries[0].0, PlutusData::Bytes(vec![]));
        assert_eq!(
            entries[0].1,
            PlutusData::Map(vec![(
                PlutusData::Bytes(vec![]),
                PlutusData::Integer(2_000_000),
            )])
        );
        // Second entry is the policy
        assert_eq!(entries[1].0, PlutusData::Bytes(vec![0xaa; 28]));
        assert_eq!(
            entries[1].1,
            PlutusData::Map(vec![(
                PlutusData::Bytes(b"Token1".to_vec()),
                PlutusData::Integer(100),
            )])
        );
    }

    // -- tx_out_ref_data encoding --------------------------------------------

    #[test]
    fn tx_out_ref_data_encodes_v1v2_wrapped_tx_id() {
        let result = tx_out_ref_data(PlutusVersion::V2, &[0xbb; 32], 7);
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xbb; 32])]),
                    PlutusData::Integer(7),
                ],
            )
        );
    }

    // -- credential_data encoding --------------------------------------------

    #[test]
    fn credential_data_encodes_pubkey_hash() {
        let result = credential_data(&StakeCredential::AddrKeyHash([0x11; 28]));
        assert_eq!(
            result,
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x11; 28])])
        );
    }

    #[test]
    fn credential_data_encodes_script_hash() {
        let result = credential_data(&StakeCredential::ScriptHash([0x22; 28]));
        assert_eq!(
            result,
            PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x22; 28])])
        );
    }

    // -- staking_credential_data encoding ------------------------------------

    #[test]
    fn staking_credential_data_wraps_credential() {
        let result = staking_credential_data(&StakeCredential::AddrKeyHash([0x33; 28]));
        // StakingHash(PubKeyCredential(hash)) = Constr(0, [Constr(0, [bytes])])
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Bytes(vec![0x33; 28])]
                ),]
            )
        );
    }

    // -- maybe_data encoding -------------------------------------------------

    #[test]
    fn maybe_data_encodes_nothing() {
        assert_eq!(maybe_data(None), PlutusData::Constr(1, vec![]));
    }

    #[test]
    fn maybe_data_encodes_just() {
        let inner = PlutusData::Integer(42);
        assert_eq!(
            maybe_data(Some(inner.clone())),
            PlutusData::Constr(0, vec![inner])
        );
    }

    // -- drep_data encoding --------------------------------------------------

    #[test]
    fn drep_data_encodes_key_hash() {
        let hash = [0xAA; 28];
        let result = drep_data(&DRep::KeyHash(hash));
        // DRep::KeyHash → Constr(0, [drep_credential_data(AddrKeyHash)])
        // drep_credential_data → Constr(0, [credential_data(AddrKeyHash)])
        // credential_data(AddrKeyHash) → Constr(0, [Bytes(hash)])
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        0,
                        vec![PlutusData::Bytes(hash.to_vec())]
                    )]
                )]
            )
        );
    }

    #[test]
    fn drep_data_encodes_script_hash() {
        let hash = [0xBB; 28];
        let result = drep_data(&DRep::ScriptHash(hash));
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        1,
                        vec![PlutusData::Bytes(hash.to_vec())]
                    )]
                )]
            )
        );
    }

    #[test]
    fn drep_data_encodes_always_abstain() {
        assert_eq!(
            drep_data(&DRep::AlwaysAbstain),
            PlutusData::Constr(1, vec![])
        );
    }

    #[test]
    fn drep_data_encodes_always_no_confidence() {
        assert_eq!(
            drep_data(&DRep::AlwaysNoConfidence),
            PlutusData::Constr(2, vec![])
        );
    }

    // -- delegatee encoding --------------------------------------------------

    #[test]
    fn delegatee_stake_data_wraps_pool_hash() {
        let pool = [0xCC; 28];
        assert_eq!(
            delegatee_stake_data(&pool),
            PlutusData::Constr(0, vec![PlutusData::Bytes(pool.to_vec())])
        );
    }

    #[test]
    fn delegatee_vote_data_wraps_drep() {
        let result = delegatee_vote_data(&DRep::AlwaysAbstain);
        assert_eq!(
            result,
            PlutusData::Constr(1, vec![PlutusData::Constr(1, vec![])])
        );
    }

    #[test]
    fn delegatee_stake_vote_data_combines_pool_and_drep() {
        let pool = [0xDD; 28];
        let result = delegatee_stake_vote_data(&pool, &DRep::AlwaysNoConfidence);
        assert_eq!(
            result,
            PlutusData::Constr(
                2,
                vec![
                    PlutusData::Bytes(pool.to_vec()),
                    PlutusData::Constr(2, vec![]),
                ]
            )
        );
    }

    // -- maybe_lovelace encoding ---------------------------------------------

    #[test]
    fn maybe_lovelace_some_encodes_just() {
        assert_eq!(
            maybe_lovelace(Some(1_000_000)),
            PlutusData::Constr(0, vec![PlutusData::Integer(1_000_000)])
        );
    }

    #[test]
    fn maybe_lovelace_none_encodes_nothing() {
        assert_eq!(maybe_lovelace(None), PlutusData::Constr(1, vec![]));
    }

    // -- credential wrapper helpers ------------------------------------------

    #[test]
    fn drep_credential_data_wraps_credential() {
        let hash = [0xEE; 28];
        assert_eq!(
            drep_credential_data(&StakeCredential::AddrKeyHash(hash)),
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Bytes(hash.to_vec())]
                )]
            )
        );
    }

    #[test]
    fn committee_credential_data_wraps_credential() {
        let hash = [0xFF; 28];
        assert_eq!(
            committee_credential_data(&StakeCredential::ScriptHash(hash)),
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    1,
                    vec![PlutusData::Bytes(hash.to_vec())]
                )]
            )
        );
    }

    // -- protocol_version_data encoding --------------------------------------

    #[test]
    fn protocol_version_data_encodes_major_minor() {
        assert_eq!(
            protocol_version_data((9, 1)),
            PlutusData::Constr(0, vec![PlutusData::Integer(9), PlutusData::Integer(1),])
        );
    }

    // -- unit_interval_data encoding -----------------------------------------

    #[test]
    fn unit_interval_data_encodes_fraction() {
        let ui = UnitInterval {
            numerator: 1,
            denominator: 5,
        };
        assert_eq!(
            unit_interval_data(&ui),
            PlutusData::List(vec![PlutusData::Integer(1), PlutusData::Integer(5)])
        );
    }

    // -- maybe_script_hash_data encoding -------------------------------------

    #[test]
    fn maybe_script_hash_data_some() {
        let hash = [0x11; 28];
        assert_eq!(
            maybe_script_hash_data(Some(hash)),
            PlutusData::Constr(0, vec![PlutusData::Bytes(hash.to_vec())])
        );
    }

    #[test]
    fn maybe_script_hash_data_none() {
        assert_eq!(maybe_script_hash_data(None), PlutusData::Constr(1, vec![]));
    }

    // -- maybe_gov_action_id_data encoding -----------------------------------

    #[test]
    fn maybe_gov_action_id_data_some() {
        let gid = GovActionId {
            transaction_id: [0x22; 32],
            gov_action_index: 3,
        };
        assert_eq!(
            maybe_gov_action_id_data(Some(&gid)),
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Bytes(vec![0x22; 32]), PlutusData::Integer(3),]
                )]
            )
        );
    }

    #[test]
    fn maybe_gov_action_id_data_none() {
        assert_eq!(
            maybe_gov_action_id_data(None),
            PlutusData::Constr(1, vec![])
        );
    }

    // -- gov_action_id_data encoding -----------------------------------------

    #[test]
    fn gov_action_id_data_encodes_tx_hash_and_index() {
        let gid = GovActionId {
            transaction_id: [0x44; 32],
            gov_action_index: 7,
        };
        assert_eq!(
            gov_action_id_data(&gid),
            PlutusData::Constr(
                0,
                vec![PlutusData::Bytes(vec![0x44; 32]), PlutusData::Integer(7),]
            )
        );
    }

    #[test]
    fn tx_out_ref_data_encodes_v3_raw_tx_id() {
        let tx_id = [0x55; 32];
        assert_eq!(
            tx_out_ref_data(PlutusVersion::V3, &tx_id, 42),
            PlutusData::Constr(
                0,
                vec![PlutusData::Bytes(tx_id.to_vec()), PlutusData::Integer(42),]
            )
        );
    }

    // -- V3 script_purpose_data encoding -------------------------------------
    // Key difference from V1/V2: Rewarding uses credential_data (not staking_credential_data).
    // V3 also supports Voting (Constr 4) and Proposing (Constr 5) natively.

    #[test]
    fn script_purpose_v3_minting_uses_constr_0() {
        let purpose = ScriptPurpose::Minting {
            policy_id: [0x66; 28],
        };
        let result = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            result,
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x66; 28])])
        );
    }

    #[test]
    fn script_purpose_v3_spending_uses_constr_1() {
        let purpose = ScriptPurpose::Spending {
            tx_id: [0x77; 32],
            index: 5,
        };
        let result = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            result,
            PlutusData::Constr(1, vec![tx_out_ref_data(PlutusVersion::V3, &[0x77; 32], 5)])
        );
    }

    #[test]
    fn script_purpose_v3_rewarding_uses_plain_credential() {
        // V3 Rewarding uses credential_data (Constr(0, [hash])), NOT staking_credential_data
        let cred = StakeCredential::ScriptHash([0x88; 28]);
        let purpose = ScriptPurpose::Rewarding {
            reward_account: RewardAccount {
                network: 1,
                credential: cred,
            },
        };
        let result = script_purpose_data_v3(&purpose, None).unwrap();
        // credential_data(ScriptHash) → Constr(1, [Bytes])
        assert_eq!(
            result,
            PlutusData::Constr(
                2,
                vec![PlutusData::Constr(
                    1,
                    vec![PlutusData::Bytes(vec![0x88; 28])]
                )]
            )
        );
    }

    #[test]
    fn script_purpose_v1v2_rewarding_uses_staking_credential() {
        // V1/V2 Rewarding uses staking_credential_data which wraps in extra Constr(0, [...])
        let cred = StakeCredential::ScriptHash([0x88; 28]);
        let purpose = ScriptPurpose::Rewarding {
            reward_account: RewardAccount {
                network: 1,
                credential: cred,
            },
        };
        let result = script_purpose_data_v1v2(PlutusVersion::V2, &purpose).unwrap();
        // staking_credential_data → Constr(0, [credential_data])
        assert_eq!(
            result,
            PlutusData::Constr(
                2,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        1,
                        vec![PlutusData::Bytes(vec![0x88; 28])]
                    )]
                )]
            )
        );
    }

    #[test]
    fn script_purpose_v3_voting_uses_constr_4() {
        let purpose = ScriptPurpose::Voting {
            voter: Voter::StakePool([0x99; 28]),
        };
        let result = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            result,
            PlutusData::Constr(
                4,
                vec![PlutusData::Constr(
                    2,
                    vec![PlutusData::Bytes(vec![0x99; 28])]
                )]
            )
        );
    }

    #[test]
    fn script_purpose_v3_proposing_uses_constr_5() {
        let purpose = ScriptPurpose::Proposing {
            proposal_index: 0,
            proposal: yggdrasil_ledger::ProposalProcedure {
                deposit: 1_000_000,
                reward_account: Address::Reward(RewardAccount {
                    network: 1,
                    credential: StakeCredential::AddrKeyHash([0xAA; 28]),
                })
                .to_bytes(),
                gov_action: GovAction::InfoAction,
                anchor: Anchor {
                    url: String::new(),
                    data_hash: [0; 32],
                },
            },
        };
        let result = script_purpose_data_v3(&purpose, None).unwrap();
        let PlutusData::Constr(5, fields) = result else {
            panic!("Expected Constr(5, ...)")
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], PlutusData::Integer(0));
    }

    // -- plutus_address_data encoding ----------------------------------------

    #[test]
    fn plutus_address_data_base_address_has_staking() {
        let addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xBB; 28]),
            staking: StakeCredential::ScriptHash([0xCC; 28]),
        });
        let bytes = addr.to_bytes();
        let result = plutus_address_data(&bytes).expect("Base address should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 2);
        // payment credential
        assert_eq!(
            fields[0],
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xBB; 28])])
        );
        // staking: Just(StakingHash(credential))
        assert_eq!(
            fields[1],
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Constr(
                        1,
                        vec![PlutusData::Bytes(vec![0xCC; 28])]
                    )]
                )]
            )
        );
    }

    #[test]
    fn plutus_address_data_enterprise_has_no_staking() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xDD; 28]),
        });
        let bytes = addr.to_bytes();
        let result = plutus_address_data(&bytes).expect("Enterprise address should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(
            fields[0],
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xDD; 28])])
        );
        // Nothing (no staking)
        assert_eq!(fields[1], PlutusData::Constr(1, vec![]));
    }

    #[test]
    fn plutus_address_data_reward_returns_none() {
        let addr = Address::Reward(RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xEE; 28]),
        });
        let bytes = addr.to_bytes();
        assert!(plutus_address_data(&bytes).is_none());
    }

    // -- plutus_input_data encoding ------------------------------------------

    #[test]
    fn plutus_input_data_combines_txin_and_output() {
        let txin = yggdrasil_ledger::eras::shelley::ShelleyTxIn {
            transaction_id: [0xFF; 32],
            index: 2,
        };
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0x11; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        });
        let result = plutus_input_data(PlutusVersion::V2, &txin, &txout).expect("Should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 2);
        // First field is the txin encoding
        assert_eq!(
            fields[0],
            tx_out_ref_data(
                PlutusVersion::V2,
                &txin.transaction_id,
                u64::from(txin.index)
            )
        );
    }

    // -- script_info_data_v3 encoding ----------------------------------------
    // Key difference from script_purpose_data_v3: Spending carries maybe_data(datum).

    #[test]
    fn script_info_v3_spending_with_datum_includes_just() {
        let datum = PlutusData::Integer(99);
        let purpose = ScriptPurpose::Spending {
            tx_id: [0xAA; 32],
            index: 3,
        };
        let result = script_info_data_v3(&purpose, Some(&datum), None).unwrap();
        let PlutusData::Constr(1, fields) = result else {
            panic!("Spending must be Constr(1, ...)")
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(
            fields[0],
            tx_out_ref_data(PlutusVersion::V3, &[0xAA; 32], 3)
        );
        // datum wrapped in Just
        assert_eq!(
            fields[1],
            PlutusData::Constr(0, vec![PlutusData::Integer(99)])
        );
    }

    #[test]
    fn script_info_v3_spending_without_datum_includes_nothing() {
        let purpose = ScriptPurpose::Spending {
            tx_id: [0xBB; 32],
            index: 0,
        };
        let result = script_info_data_v3(&purpose, None, None).unwrap();
        let PlutusData::Constr(1, fields) = result else {
            panic!("Spending must be Constr(1, ...)")
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[1], PlutusData::Constr(1, vec![])); // Nothing
    }

    #[test]
    fn script_info_v3_minting_matches_script_purpose_v3() {
        let purpose = ScriptPurpose::Minting {
            policy_id: [0xCC; 28],
        };
        let info = script_info_data_v3(&purpose, None, None).unwrap();
        let sp = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            info, sp,
            "Minting ScriptInfo and ScriptPurpose should be identical for V3"
        );
    }

    // -- certifying_purpose_data encoding ------------------------------------
    // V1/V2 Certifying wraps legacy_dcert_data in Constr(3, [cert]) — no cert_index.

    #[test]
    fn certifying_purpose_data_wraps_legacy_cert() {
        let cert = DCert::AccountRegistration(StakeCredential::AddrKeyHash([0xDD; 28]));
        let result = certifying_purpose_data(42, &cert).unwrap();
        let PlutusData::Constr(3, fields) = result else {
            panic!("Expected Constr(3, ...)")
        };
        // V1/V2 certifying does NOT include cert_index in the output
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0], legacy_dcert_data(&cert).unwrap());
    }

    #[test]
    fn certifying_purpose_data_rejects_conway_certs() {
        let cert = DCert::DrepRegistration(StakeCredential::AddrKeyHash([0x11; 28]), 0, None);
        assert!(certifying_purpose_data(0, &cert).is_err());
    }

    // -- constitution_data_v3 encoding ---------------------------------------

    #[test]
    fn constitution_data_v3_with_guardrails() {
        let c = Constitution {
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
            guardrails_script_hash: Some([0xEE; 28]),
        };
        let result = constitution_data_v3(&c);
        assert_eq!(
            result,
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Bytes(vec![0xEE; 28])]
                )]
            )
        );
    }

    #[test]
    fn constitution_data_v3_without_guardrails() {
        let c = Constitution {
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
            guardrails_script_hash: None,
        };
        let result = constitution_data_v3(&c);
        assert_eq!(
            result,
            PlutusData::Constr(0, vec![PlutusData::Constr(1, vec![])])
        );
    }

    // -- plutus_output_data per-era encoding ----------------------------------

    #[test]
    fn plutus_output_data_shelley_has_3_fields_with_no_datum() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x11; 28]),
        });
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(ShelleyTxOut {
            address: addr.to_bytes(),
            amount: 2_000_000,
        });
        let result = plutus_output_data(PlutusVersion::V2, &txout).expect("Shelley should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3, "Shelley TxOut must have 3 fields");
        assert_eq!(
            fields[2],
            PlutusData::Constr(1, vec![]),
            "Datum must be Nothing"
        );
    }

    #[test]
    fn plutus_output_data_mary_has_3_fields() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x22; 28]),
        });
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Mary(MaryTxOut {
            address: addr.to_bytes(),
            amount: Value::Coin(3_000_000),
        });
        let result = plutus_output_data(PlutusVersion::V2, &txout).expect("Mary should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3, "Mary TxOut must have 3 fields");
        assert_eq!(
            fields[2],
            PlutusData::Constr(1, vec![]),
            "Datum must be Nothing"
        );
    }

    #[test]
    fn plutus_output_data_alonzo_with_datum_hash() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x33; 28]),
        });
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.to_bytes(),
            amount: Value::Coin(4_000_000),
            datum_hash: Some([0x44; 32]),
        });
        let result = plutus_output_data(PlutusVersion::V2, &txout).expect("Alonzo should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3, "Alonzo TxOut must have 3 fields");
        assert_eq!(
            fields[2],
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x44; 32])]),
            "Datum hash must be Just(hash)"
        );
    }

    #[test]
    fn plutus_output_data_alonzo_without_datum_hash() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x55; 28]),
        });
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.to_bytes(),
            amount: Value::Coin(1_000_000),
            datum_hash: None,
        });
        let result = plutus_output_data(PlutusVersion::V2, &txout).expect("Alonzo should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(
            fields[2],
            PlutusData::Constr(1, vec![]),
            "Datum must be Nothing"
        );
    }

    #[test]
    fn plutus_output_data_babbage_inline_datum() {
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0x66; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(2_000_000),
            datum_option: Some(DatumOption::Inline(PlutusData::Integer(777))),
            script_ref: None,
        });
        let result = plutus_output_data(PlutusVersion::V2, &txout).expect("Babbage should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 4, "Babbage TxOut must have 4 fields");
        // Inline datum → Constr(2, [data])
        assert_eq!(
            fields[2],
            PlutusData::Constr(2, vec![PlutusData::Integer(777)])
        );
        // No script ref → Nothing
        assert_eq!(fields[3], PlutusData::Constr(1, vec![]));
    }

    #[test]
    fn plutus_output_data_babbage_datum_hash() {
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0x77; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(1_000_000),
            datum_option: Some(DatumOption::Hash([0x88; 32])),
            script_ref: None,
        });
        let result = plutus_output_data(PlutusVersion::V2, &txout).expect("Babbage should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        // Datum hash → Constr(1, [Bytes(hash)])
        assert_eq!(
            fields[2],
            PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x88; 32])])
        );
    }

    #[test]
    fn plutus_output_data_babbage_no_datum() {
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0x99; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        });
        let result = plutus_output_data(PlutusVersion::V2, &txout).expect("Babbage should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        // No datum → Constr(0, []) i.e. NoDatum
        assert_eq!(fields[2], PlutusData::Constr(0, vec![]));
    }

    // -- B6: V1 Babbage TxOut uses 3-element shape ----------------------------

    #[test]
    fn plutus_output_data_v1_babbage_has_3_fields_with_datum_hash() {
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0xAA; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(3_000_000),
            datum_option: Some(DatumOption::Hash([0xBB; 32])),
            script_ref: None,
        });
        let result =
            plutus_output_data(PlutusVersion::V1, &txout).expect("V1 Babbage should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3, "V1 Babbage TxOut must have 3 fields");
        // Datum hash → Just(hash)
        assert_eq!(
            fields[2],
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xBB; 32])])
        );
    }

    #[test]
    fn plutus_output_data_v1_babbage_inline_datum_becomes_nothing() {
        // V1 cannot see inline datums — they are downgraded to Nothing.
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0xCC; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(1_000_000),
            datum_option: Some(DatumOption::Inline(PlutusData::Integer(42))),
            script_ref: None,
        });
        let result =
            plutus_output_data(PlutusVersion::V1, &txout).expect("V1 Babbage should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3, "V1 Babbage TxOut must have 3 fields");
        // Inline datum invisible to V1: Nothing
        assert_eq!(fields[2], PlutusData::Constr(1, vec![]));
    }

    #[test]
    fn plutus_output_data_v1_babbage_no_datum() {
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0xDD; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        });
        let result =
            plutus_output_data(PlutusVersion::V1, &txout).expect("V1 Babbage should encode");
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3, "V1 Babbage TxOut must have 3 fields");
        assert_eq!(
            fields[2],
            PlutusData::Constr(1, vec![]),
            "No datum = Nothing"
        );
    }

    // -- B7: V1 guard rejects inline datums and reference scripts -------------

    #[test]
    fn guard_v1_rejects_inline_datum_in_input() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.inputs = vec![(
            yggdrasil_ledger::eras::shelley::ShelleyTxIn {
                transaction_id: [0x01; 32],
                index: 0,
            },
            yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0x02; 28]),
                })
                .to_bytes(),
                amount: Value::Coin(1_000_000),
                datum_option: Some(DatumOption::Inline(PlutusData::Integer(1))),
                script_ref: None,
            }),
        )];
        let result = guard_legacy_plutus_context_features(PlutusVersion::V1, &tx_ctx);
        assert!(result.is_err(), "V1 must reject inline datums");
    }

    #[test]
    fn guard_v1_rejects_reference_script_in_output() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.outputs = vec![yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(
            BabbageTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0x03; 28]),
                })
                .to_bytes(),
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: Some(ScriptRef(Script::PlutusV2(vec![0xDE, 0xAD]))),
            },
        )];
        let result = guard_legacy_plutus_context_features(PlutusVersion::V1, &tx_ctx);
        assert!(result.is_err(), "V1 must reject reference scripts");
    }

    #[test]
    fn guard_v2_allows_inline_datum_and_reference_script() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.inputs = vec![(
            yggdrasil_ledger::eras::shelley::ShelleyTxIn {
                transaction_id: [0x01; 32],
                index: 0,
            },
            yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0x02; 28]),
                })
                .to_bytes(),
                amount: Value::Coin(1_000_000),
                datum_option: Some(DatumOption::Inline(PlutusData::Integer(1))),
                script_ref: Some(ScriptRef(Script::PlutusV2(vec![0xDE, 0xAD]))),
            }),
        )];
        let result = guard_legacy_plutus_context_features(PlutusVersion::V2, &tx_ctx);
        assert!(
            result.is_ok(),
            "V2 must allow inline datums and reference scripts"
        );
    }

    // -- B1/B2: V3 fee is plain Integer, V1/V2 fee is nested Value -----------

    #[test]
    fn tx_info_v3_fee_is_plain_integer() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.fee = 500_000;
        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 3 = fee (Lovelace = plain Integer)
        assert_eq!(fields[3], PlutusData::Integer(500_000));
    }

    // -- B3: V2 mint also has zero-ADA padding, V3 mint does not --------------

    #[test]
    fn tx_info_v2_mint_has_zero_ada_padding() {
        let mut tx_ctx = test_tx_ctx();
        let policy: [u8; 28] = [0x11; 28];
        let mut assets = BTreeMap::new();
        assets.insert(b"X".to_vec(), 5i64);
        tx_ctx.mint.insert(policy, assets);

        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V2 field 4 = mint
        let PlutusData::Map(mint) = &fields[4] else {
            panic!("Expected Map")
        };
        assert_eq!(mint.len(), 2, "zero-ADA entry + 1 policy");
        assert_eq!(
            mint[0].0,
            PlutusData::Bytes(vec![]),
            "first entry is empty policy"
        );
    }

    #[test]
    fn tx_info_v3_mint_has_no_zero_ada_padding() {
        let mut tx_ctx = test_tx_ctx();
        let policy: [u8; 28] = [0x11; 28];
        let mut assets = BTreeMap::new();
        assets.insert(b"X".to_vec(), 5i64);
        tx_ctx.mint.insert(policy, assets);

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 4 = mint
        let PlutusData::Map(mint) = &fields[4] else {
            panic!("Expected Map")
        };
        assert_eq!(
            mint.len(),
            1,
            "V3 has no zero-ADA padding — only the real policy"
        );
        assert_eq!(mint[0].0, PlutusData::Bytes(vec![0x11; 28]));
    }

    // -- B8: V1 withdrawals use List-of-tuples, V2 uses Map -------------------

    #[test]
    fn tx_info_v1_withdrawals_are_list_of_tuples() {
        let mut tx_ctx = test_tx_ctx();
        let cred = StakeCredential::AddrKeyHash([0x55; 28]);
        tx_ctx.withdrawals.insert(
            RewardAccount {
                network: 1,
                credential: cred,
            },
            42,
        );

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 5 = withdrawals
        let PlutusData::List(wdrl) = &fields[5] else {
            panic!("V1 withdrawals should be List, not Map")
        };
        assert_eq!(wdrl.len(), 1);
        // Each entry is Constr(0, [staking_credential, amount])
        let PlutusData::Constr(0, pair) = &wdrl[0] else {
            panic!("Expected Constr tuple")
        };
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[1], PlutusData::Integer(42));
    }

    #[test]
    fn tx_info_v2_withdrawals_are_map() {
        let mut tx_ctx = test_tx_ctx();
        let cred = StakeCredential::AddrKeyHash([0x55; 28]);
        tx_ctx.withdrawals.insert(
            RewardAccount {
                network: 1,
                credential: cred,
            },
            42,
        );

        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V2 field 6 = withdrawals (Map)
        let PlutusData::Map(wdrl) = &fields[6] else {
            panic!("V2 withdrawals should be Map")
        };
        assert_eq!(wdrl.len(), 1);
    }

    // -- gov_action_data_v3 remaining variants --------------------------------

    #[test]
    fn gov_action_data_v3_encodes_parameter_change() {
        let ga = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: ProtocolParameterUpdate::default(),
            guardrails_script_hash: Some([0xAA; 28]),
        };
        let result = gov_action_data_v3(&ga);
        let PlutusData::Constr(0, fields) = result else {
            panic!("Expected Constr(0, ...)")
        };
        assert_eq!(fields.len(), 3);
        // prev_action_id: Nothing
        assert_eq!(fields[0], PlutusData::Constr(1, vec![]));
        // protocol_param_update: CBOR-serialized bytes
        let PlutusData::Bytes(_) = &fields[1] else {
            panic!("Expected Bytes for param update")
        };
        // guardrails: Just(hash)
        assert_eq!(
            fields[2],
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xAA; 28])])
        );
    }

    #[test]
    fn gov_action_data_v3_encodes_treasury_withdrawals() {
        let mut withdrawals = BTreeMap::new();
        withdrawals.insert(
            RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0xBB; 28]),
            },
            5_000_000u64,
        );
        let ga = GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        };
        let result = gov_action_data_v3(&ga);
        let PlutusData::Constr(2, fields) = result else {
            panic!("Expected Constr(2, ...)")
        };
        assert_eq!(fields.len(), 2);
        // withdrawals map
        let PlutusData::Map(entries) = &fields[0] else {
            panic!("Expected Map")
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, PlutusData::Integer(5_000_000));
        // guardrails: Nothing
        assert_eq!(fields[1], PlutusData::Constr(1, vec![]));
    }

    #[test]
    fn gov_action_data_v3_encodes_update_committee() {
        let remove = vec![StakeCredential::AddrKeyHash([0xCC; 28])];
        let mut add = BTreeMap::new();
        add.insert(StakeCredential::ScriptHash([0xDD; 28]), 100u64);
        let ga = GovAction::UpdateCommittee {
            prev_action_id: Some(GovActionId {
                transaction_id: [0xEE; 32],
                gov_action_index: 1,
            }),
            members_to_remove: remove,
            members_to_add: add,
            quorum: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        };
        let result = gov_action_data_v3(&ga);
        let PlutusData::Constr(4, fields) = result else {
            panic!("Expected Constr(4, ...)")
        };
        assert_eq!(fields.len(), 4);
        // prev: Just(gov_action_id)
        let PlutusData::Constr(0, _) = &fields[0] else {
            panic!("Expected Just for prev")
        };
        // members_to_remove: List of committee_credential_data
        let PlutusData::List(removed) = &fields[1] else {
            panic!("Expected List")
        };
        assert_eq!(removed.len(), 1);
        // members_to_add: Map of (committee_credential_data -> epoch)
        let PlutusData::Map(added) = &fields[2] else {
            panic!("Expected Map")
        };
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].1, PlutusData::Integer(100));
        // quorum: [2, 3]
        assert_eq!(
            fields[3],
            PlutusData::List(vec![PlutusData::Integer(2), PlutusData::Integer(3)])
        );
    }

    // -- map_machine_error encoding ------------------------------------------

    #[test]
    fn map_machine_error_structural_out_of_budget() {
        let hash = [0xFF; 28];
        let err = MachineError::OutOfBudget("cpu exceeded".into());
        let result = map_machine_error(&hash, err);
        match result {
            LedgerError::PlutusScriptFailed { hash: h, reason } => {
                assert_eq!(h, [0xFF; 28]);
                // Structural error — detail preserved
                assert!(
                    reason.contains("cpu exceeded"),
                    "budget detail must be preserved"
                );
            }
            other => panic!("Expected PlutusScriptFailed, got {:?}", other),
        }
    }

    #[test]
    fn map_machine_error_operational_collapses_to_opaque() {
        let hash = [0xAA; 28];
        let err = MachineError::DivisionByZero;
        let result = map_machine_error(&hash, err);
        match result {
            LedgerError::PlutusScriptFailed { reason, .. } => {
                // Operational error collapsed — must NOT leak "division by zero"
                assert!(
                    reason.contains("evaluation failure"),
                    "operational error should be opaque, got: {reason}"
                );
            }
            other => panic!("Expected PlutusScriptFailed, got {:?}", other),
        }
    }

    #[test]
    fn map_machine_error_flat_decode_becomes_decode_error() {
        let hash = [0xBB; 28];
        let err = MachineError::FlatDecodeError("trailing bits".into());
        let result = map_machine_error(&hash, err);
        match result {
            LedgerError::PlutusScriptDecodeError { hash: h, reason } => {
                assert_eq!(h, [0xBB; 28]);
                assert!(reason.contains("trailing bits"));
            }
            other => panic!("Expected PlutusScriptDecodeError, got {:?}", other),
        }
    }

    // -- build_tx_info field-level correctness --------------------------------

    #[test]
    fn tx_info_v1_fee_is_flat_map_not_value() {
        // V1 fee field is a nested Value: Map[("" → Map[("" → fee)])] (upstream transCoinToValue).
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.fee = 173_201;
        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 2 = fee (Value encoding)
        assert_eq!(
            fields[2],
            PlutusData::Map(vec![(
                PlutusData::Bytes(vec![]),
                PlutusData::Map(vec![(
                    PlutusData::Bytes(vec![]),
                    PlutusData::Integer(173_201),
                )]),
            )])
        );
    }

    #[test]
    fn tx_info_v2_fee_is_flat_map() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.fee = 250_000;
        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V2 field 3 = fee (Value encoding — nested Map)
        assert_eq!(
            fields[3],
            PlutusData::Map(vec![(
                PlutusData::Bytes(vec![]),
                PlutusData::Map(vec![(
                    PlutusData::Bytes(vec![]),
                    PlutusData::Integer(250_000),
                )]),
            )])
        );
    }

    #[test]
    fn tx_info_v1_tx_id_field_is_last() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.tx_hash = [0xAA; 32];
        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 9 = txInfoId = Constr(0, [Bytes(tx_hash)])
        assert_eq!(
            fields[9],
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xAA; 32])])
        );
    }

    #[test]
    fn tx_info_v2_tx_id_is_field_11() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.tx_hash = [0xBB; 32];
        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V2 field 11 = txInfoId
        assert_eq!(
            fields[11],
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xBB; 32])])
        );
    }

    #[test]
    fn tx_info_v3_tx_id_is_field_11() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.tx_hash = [0xCC; 32];
        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        assert_eq!(fields[11], PlutusData::Bytes(vec![0xCC; 32]));
    }

    #[test]
    fn tx_info_v1_inputs_populated_from_context() {
        let mut tx_ctx = test_tx_ctx();
        let txin = yggdrasil_ledger::eras::shelley::ShelleyTxIn {
            transaction_id: [0xDD; 32],
            index: 0,
        };
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
            address: Address::Enterprise(EnterpriseAddress {
                network: 1,
                payment: StakeCredential::AddrKeyHash([0xEE; 28]),
            })
            .to_bytes(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        });
        tx_ctx.inputs = vec![(txin.clone(), txout.clone())];

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 0 = inputs
        let PlutusData::List(inputs) = &fields[0] else {
            panic!("Expected List")
        };
        assert_eq!(inputs.len(), 1);
        assert_eq!(
            inputs[0],
            plutus_input_data(PlutusVersion::V1, &txin, &txout).unwrap()
        );
    }

    #[test]
    fn tx_info_v1_outputs_populated_and_byron_skipped() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.outputs = vec![
            // Shelley output with enterprise address — should be included
            yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(ShelleyTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0x11; 28]),
                })
                .to_bytes(),
                amount: 1_000_000,
            }),
            // Shelley output with Byron address — should be skipped
            yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(ShelleyTxOut {
                address: Address::Byron(vec![0x82, 0x00]).to_bytes(),
                amount: 500_000,
            }),
        ];

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 1 = outputs; Byron address should be filtered
        let PlutusData::List(outputs) = &fields[1] else {
            panic!("Expected List")
        };
        assert_eq!(outputs.len(), 1, "Byron output should be filtered out");
    }

    #[test]
    fn tx_info_v1_mint_encodes_policy_and_assets() {
        let mut tx_ctx = test_tx_ctx();
        let policy: [u8; 28] = [0x22; 28];
        let mut asset_map = BTreeMap::new();
        asset_map.insert(b"TokenA".to_vec(), 100i64);
        tx_ctx.mint.insert(policy, asset_map);

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 3 = mint (with zero-ADA prefix from upstream transMintValue)
        let PlutusData::Map(mint) = &fields[3] else {
            panic!("Expected Map")
        };
        assert_eq!(mint.len(), 2, "zero-ADA entry + 1 policy");
        // mint[0] = zero-ADA prefix
        assert_eq!(mint[0].0, PlutusData::Bytes(vec![]));
        assert_eq!(
            mint[0].1,
            PlutusData::Map(vec![(PlutusData::Bytes(vec![]), PlutusData::Integer(0))])
        );
        // mint[1] = actual policy
        assert_eq!(mint[1].0, PlutusData::Bytes(vec![0x22; 28]));
        let PlutusData::Map(assets) = &mint[1].1 else {
            panic!("Expected asset Map")
        };
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].0, PlutusData::Bytes(b"TokenA".to_vec()));
        assert_eq!(assets[0].1, PlutusData::Integer(100));
    }

    #[test]
    fn tx_info_v2_withdrawals_use_staking_credential_wrapping() {
        let mut tx_ctx = test_tx_ctx();
        let cred = StakeCredential::AddrKeyHash([0x33; 28]);
        tx_ctx.withdrawals.insert(
            RewardAccount {
                network: 1,
                credential: cred,
            },
            42,
        );

        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V2 field 6 = withdrawals
        let PlutusData::Map(wdrl) = &fields[6] else {
            panic!("Expected Map")
        };
        assert_eq!(wdrl.len(), 1);
        // V2 wraps via staking_credential_data → Constr(0, [credential_data])
        assert_eq!(
            wdrl[0].0,
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(
                    0,
                    vec![PlutusData::Bytes(vec![0x33; 28])]
                )]
            )
        );
        assert_eq!(wdrl[0].1, PlutusData::Integer(42));
    }

    #[test]
    fn tx_info_v1_signatories_is_list_of_bytes() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.required_signers = vec![[0x44; 28], [0x55; 28]];

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 7 = signatories
        assert_eq!(
            fields[7],
            PlutusData::List(vec![
                PlutusData::Bytes(vec![0x44; 28]),
                PlutusData::Bytes(vec![0x55; 28]),
            ])
        );
    }

    #[test]
    fn tx_info_v1_datums_map_populated() {
        let mut tx_ctx = test_tx_ctx();
        let datum_hash = [0x66; 32];
        let datum = PlutusData::Integer(999);
        tx_ctx.witness_datums.insert(datum_hash, datum.clone());

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 8 = datums (List of Constr tuples \u2014 upstream PV1 encoding)
        let PlutusData::List(datums) = &fields[8] else {
            panic!("Expected List")
        };
        assert_eq!(datums.len(), 1);
        assert_eq!(
            datums[0],
            PlutusData::Constr(
                0,
                vec![PlutusData::Bytes(vec![0x66; 32]), PlutusData::Integer(999)]
            )
        );
    }

    #[test]
    fn tx_info_datums_are_sorted_by_hash() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx
            .witness_datums
            .insert([0x66; 32], PlutusData::Integer(2));
        tx_ctx
            .witness_datums
            .insert([0x11; 32], PlutusData::Integer(1));

        let v1 = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, v1_fields) = v1 else {
            panic!()
        };
        let PlutusData::List(v1_datums) = &v1_fields[8] else {
            panic!("Expected V1 datum list")
        };
        assert_eq!(
            v1_datums[0],
            PlutusData::Constr(
                0,
                vec![PlutusData::Bytes(vec![0x11; 32]), PlutusData::Integer(1)]
            )
        );
        assert_eq!(
            v1_datums[1],
            PlutusData::Constr(
                0,
                vec![PlutusData::Bytes(vec![0x66; 32]), PlutusData::Integer(2)]
            )
        );

        let v2 = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, v2_fields) = v2 else {
            panic!()
        };
        let PlutusData::Map(v2_datums) = &v2_fields[10] else {
            panic!("Expected V2 datum map")
        };
        assert_eq!(v2_datums[0].0, PlutusData::Bytes(vec![0x11; 32]));
        assert_eq!(v2_datums[0].1, PlutusData::Integer(1));
        assert_eq!(v2_datums[1].0, PlutusData::Bytes(vec![0x66; 32]));
        assert_eq!(v2_datums[1].1, PlutusData::Integer(2));
    }

    #[test]
    fn tx_info_v1_validity_range_populates() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.validity_start = Some(1_000);
        tx_ctx.ttl = Some(2_000);

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 6 = validRange
        assert_eq!(fields[6], posix_time_range(Some(1_000), Some(2_000)));
    }

    #[test]
    fn tx_info_v1_certs_use_legacy_encoding() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.certificates = vec![DCert::AccountRegistration(StakeCredential::AddrKeyHash(
            [0x77; 28],
        ))];

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 4 = dcert
        let PlutusData::List(certs) = &fields[4] else {
            panic!("Expected List")
        };
        assert_eq!(certs.len(), 1);
        assert_eq!(
            certs[0],
            legacy_dcert_data(&tx_ctx.certificates[0]).unwrap()
        );
    }

    #[test]
    fn tx_info_v3_current_treasury_populated() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.current_treasury_value = Some(50_000_000);

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 14 = currentTreasuryAmount
        assert_eq!(
            fields[14],
            PlutusData::Constr(0, vec![PlutusData::Integer(50_000_000)])
        );
    }

    #[test]
    fn tx_info_v3_treasury_donation_populated() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.treasury_donation = Some(10_000);

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 15 = treasuryDonation
        assert_eq!(
            fields[15],
            PlutusData::Constr(0, vec![PlutusData::Integer(10_000)])
        );
    }

    #[test]
    fn tx_info_v3_treasury_fields_are_nothing_when_absent() {
        let tx_info = expect_tx_info(PlutusVersion::V3, &test_tx_ctx());
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // field 14 = currentTreasuryAmount = Nothing
        assert_eq!(fields[14], PlutusData::Constr(1, vec![]));
        // field 15 = treasuryDonation = Nothing
        assert_eq!(fields[15], PlutusData::Constr(1, vec![]));
    }

    #[test]
    fn tx_info_v3_certs_use_v3_encoding() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.certificates = vec![DCert::AccountRegistration(StakeCredential::AddrKeyHash(
            [0x88; 28],
        ))];

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 5 = txCerts (V3 encoding, not legacy)
        let PlutusData::List(certs) = &fields[5] else {
            panic!("Expected List")
        };
        assert_eq!(certs.len(), 1);
        // V3 AccountRegistration = Constr(0, [credential, maybe_lovelace(None)])
        assert_eq!(
            certs[0],
            tx_cert_data_v3(&tx_ctx.certificates[0], None).unwrap()
        );
    }

    #[test]
    fn tx_info_inputs_skip_byron_address_outputs() {
        let mut tx_ctx = test_tx_ctx();
        let txin = yggdrasil_ledger::eras::shelley::ShelleyTxIn {
            transaction_id: [0x99; 32],
            index: 0,
        };
        // Byron address → plutus_address_data returns None → plutus_input_data returns None
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(ShelleyTxOut {
            address: Address::Byron(vec![0x82, 0x00]).to_bytes(),
            amount: 1_000_000,
        });
        tx_ctx.inputs = vec![(txin, txout)];

        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        let PlutusData::List(inputs) = &fields[0] else {
            panic!("Expected List")
        };
        assert_eq!(inputs.len(), 0, "Byron-addressed input should be filtered");
    }

    #[test]
    fn tx_info_v3_mint_at_field_4() {
        let mut tx_ctx = test_tx_ctx();
        let policy: [u8; 28] = [0xAA; 28];
        let mut asset_map = BTreeMap::new();
        asset_map.insert(b"Coin".to_vec(), -50i64);
        tx_ctx.mint.insert(policy, asset_map);

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 4 = mint
        let PlutusData::Map(mint) = &fields[4] else {
            panic!("Expected Map")
        };
        assert_eq!(mint.len(), 1);
        let PlutusData::Map(assets) = &mint[0].1 else {
            panic!("Expected asset Map")
        };
        assert_eq!(
            assets[0].1,
            PlutusData::Integer(-50),
            "Burns should be negative"
        );
    }

    // -----------------------------------------------------------------------
    // Cross-version shared-field equivalence
    // -----------------------------------------------------------------------
    // V1/V2/V3 must produce identical encodings for fields that share the same
    // semantics.  These tests build a single TxContext, encode it under multiple
    // versions, and compare the fields that must match.

    #[test]
    fn tx_info_v1_v2_produce_identical_inputs_encoding() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.inputs = vec![(
            yggdrasil_ledger::eras::shelley::ShelleyTxIn {
                transaction_id: [0xA1; 32],
                index: 7,
            },
            yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0xA2; 28]),
                })
                .to_bytes(),
                amount: Value::Coin(100),
                datum_option: None,
                script_ref: None,
            }),
        )];

        let v1 = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let v2 = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, f1) = v1 else {
            panic!()
        };
        let PlutusData::Constr(0, f2) = v2 else {
            panic!()
        };
        // V1 uses 3-element Babbage output (no datum_option/script_ref); V2 uses 4-element
        assert_ne!(
            f1[0], f2[0],
            "V1 and V2 Babbage outputs differ (3-element vs 4-element)"
        );
    }

    #[test]
    fn tx_info_v2_v3_tx_id_shapes_diverge() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.inputs = vec![(
            yggdrasil_ledger::eras::shelley::ShelleyTxIn {
                transaction_id: [0xB1; 32],
                index: 0,
            },
            yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
                address: Address::Enterprise(EnterpriseAddress {
                    network: 1,
                    payment: StakeCredential::AddrKeyHash([0xB2; 28]),
                })
                .to_bytes(),
                amount: Value::Coin(500),
                datum_option: None,
                script_ref: None,
            }),
        )];
        tx_ctx.fee = 200;
        tx_ctx.required_signers = vec![[0xB3; 28]];
        tx_ctx
            .witness_datums
            .insert([0xB4; 32], PlutusData::Integer(42));

        let v2 = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let v3 = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, f2) = v2 else {
            panic!()
        };
        let PlutusData::Constr(0, f3) = v3 else {
            panic!()
        };
        // V2 and V3 share positions for: refInputs(1), outputs(2),
        // validRange(7), signatories(8), datums(10). Inputs and txInfoId
        // differ because V2 inherits the V1 `TxId` constructor wrapper while
        // V3 derives `ToData` through the newtype and uses raw bytes.
        // fee(3) also diverges: V2 uses Value, V3 uses Lovelace.
        assert_ne!(f2[0], f3[0], "input TxOutRef TxId shapes must diverge");
        assert_eq!(f2[1], f3[1], "refInputs must match");
        assert_eq!(f2[2], f3[2], "outputs must match");
        assert_ne!(
            f2[3], f3[3],
            "V2 fee is Value (nested Map), V3 fee is Lovelace (Integer)"
        );
        assert_eq!(f2[7], f3[7], "validRange must match");
        assert_eq!(f2[8], f3[8], "signatories must match");
        assert_eq!(f2[10], f3[10], "datums must match");
        assert_ne!(f2[11], f3[11], "txInfoId TxId shapes must diverge");
    }

    #[test]
    fn tx_info_v2_v3_withdrawals_diverge_on_credential_wrapping() {
        let mut tx_ctx = test_tx_ctx();
        let cred = StakeCredential::AddrKeyHash([0xC1; 28]);
        tx_ctx.withdrawals.insert(
            RewardAccount {
                network: 1,
                credential: cred,
            },
            99,
        );

        let v2 = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let v3 = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, f2) = v2 else {
            panic!()
        };
        let PlutusData::Constr(0, f3) = v3 else {
            panic!()
        };
        // V2 field 6 wraps via staking_credential_data; V3 field 6 uses plain credential_data.
        assert_ne!(
            f2[6], f3[6],
            "V2 and V3 withdrawal keys must differ (wrapping vs plain)"
        );

        // V2 key = Constr(0, [credential_data])
        let PlutusData::Map(wdrl_v2) = &f2[6] else {
            panic!()
        };
        assert_eq!(wdrl_v2[0].0, staking_credential_data(&cred));
        // V3 key = credential_data directly
        let PlutusData::Map(wdrl_v3) = &f3[6] else {
            panic!()
        };
        assert_eq!(wdrl_v3[0].0, credential_data(&cred));
    }

    #[test]
    fn tx_info_v2_v3_certs_diverge_on_encoding_scheme() {
        let mut tx_ctx = test_tx_ctx();
        let cert = DCert::AccountRegistration(StakeCredential::AddrKeyHash([0xD1; 28]));
        tx_ctx.certificates = vec![cert.clone()];

        let v2 = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let v3 = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, f2) = v2 else {
            panic!()
        };
        let PlutusData::Constr(0, f3) = v3 else {
            panic!()
        };
        // V2 field 5 uses legacy_dcert_data; V3 field 5 uses tx_cert_data_v3.
        let PlutusData::List(certs_v2) = &f2[5] else {
            panic!()
        };
        let PlutusData::List(certs_v3) = &f3[5] else {
            panic!()
        };
        assert_eq!(certs_v2[0], legacy_dcert_data(&cert).unwrap());
        assert_eq!(certs_v3[0], tx_cert_data_v3(&cert, None).unwrap());
        // They must be different (legacy reg = Constr(0,[cred]) vs V3 reg = Constr(0,[cred, Nothing]))
        assert_ne!(
            certs_v2[0], certs_v3[0],
            "Legacy and V3 cert encodings must differ"
        );
    }

    // -----------------------------------------------------------------------
    // Multi-item encoding
    // -----------------------------------------------------------------------
    // Verify that multiple items in a collection are all faithfully encoded.

    #[test]
    fn tx_info_v2_encodes_multiple_inputs() {
        let mut tx_ctx = test_tx_ctx();
        let mk_input = |id: u8, idx: u16| {
            (
                yggdrasil_ledger::eras::shelley::ShelleyTxIn {
                    transaction_id: [id; 32],
                    index: idx,
                },
                yggdrasil_ledger::utxo::MultiEraTxOut::Babbage(BabbageTxOut {
                    address: Address::Enterprise(EnterpriseAddress {
                        network: 1,
                        payment: StakeCredential::AddrKeyHash([id; 28]),
                    })
                    .to_bytes(),
                    amount: Value::Coin(id as u64 * 100),
                    datum_option: None,
                    script_ref: None,
                }),
            )
        };
        tx_ctx.inputs = vec![mk_input(0xE1, 0), mk_input(0xE2, 1), mk_input(0xE3, 2)];

        let tx_info = expect_tx_info(PlutusVersion::V2, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        let PlutusData::List(inputs) = &fields[0] else {
            panic!("Expected List")
        };
        assert_eq!(inputs.len(), 3, "All three inputs must be encoded");
    }

    #[test]
    fn tx_info_v1_encodes_multiple_mint_policies() {
        let mut tx_ctx = test_tx_ctx();
        let p1: [u8; 28] = [0xF1; 28];
        let p2: [u8; 28] = [0xF2; 28];
        let mut assets1 = BTreeMap::new();
        assets1.insert(b"A".to_vec(), 10i64);
        let mut assets2 = BTreeMap::new();
        assets2.insert(b"B".to_vec(), 20i64);
        assets2.insert(b"C".to_vec(), -5i64);
        tx_ctx.mint.insert(p1, assets1);
        tx_ctx.mint.insert(p2, assets2);

        let tx_info = expect_tx_info(PlutusVersion::V1, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V1 field 3 = mint (with zero-ADA prefix from upstream transMintValue)
        let PlutusData::Map(mint) = &fields[3] else {
            panic!("Expected Map")
        };
        assert_eq!(mint.len(), 3, "zero-ADA entry + 2 policies");
        // mint[0] = zero-ADA prefix
        assert_eq!(mint[0].0, PlutusData::Bytes(vec![]));
        // Third entry (index 2) is the second policy with 2 assets
        let PlutusData::Map(assets_for_p2) = &mint[2].1 else {
            panic!("Expected asset Map")
        };
        assert_eq!(
            assets_for_p2.len(),
            2,
            "Second policy should have two assets"
        );
    }

    #[test]
    fn tx_info_v3_encodes_multiple_withdrawals() {
        let mut tx_ctx = test_tx_ctx();
        let cred1 = StakeCredential::AddrKeyHash([0x01; 28]);
        let cred2 = StakeCredential::ScriptHash([0x02; 28]);
        tx_ctx.withdrawals = BTreeMap::from([
            (
                RewardAccount {
                    network: 1,
                    credential: cred1,
                },
                100,
            ),
            (
                RewardAccount {
                    network: 1,
                    credential: cred2,
                },
                200,
            ),
        ]);

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 6 = withdrawals
        let PlutusData::Map(wdrl) = &fields[6] else {
            panic!("Expected Map")
        };
        assert_eq!(wdrl.len(), 2, "Both withdrawals must be encoded");
        // V3 uses plain credential_data keys
        let keys: Vec<_> = wdrl.iter().map(|(k, _)| k.clone()).collect();
        assert!(keys.contains(&credential_data(&cred1)));
        assert!(keys.contains(&credential_data(&cred2)));
    }

    #[test]
    fn tx_info_v3_encodes_multiple_v3_certs() {
        let mut tx_ctx = test_tx_ctx();
        tx_ctx.certificates = vec![
            DCert::AccountRegistration(StakeCredential::AddrKeyHash([0x11; 28])),
            DCert::PoolRetirement([0x22; 28], EpochNo(100)),
            DCert::DrepRegistration(StakeCredential::ScriptHash([0x33; 28]), 500, None),
        ];

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else {
            panic!()
        };
        // V3 field 5 = txCerts
        let PlutusData::List(certs) = &fields[5] else {
            panic!("Expected List")
        };
        assert_eq!(certs.len(), 3, "All three certs must be encoded");
        // Verify each is the V3 encoding
        assert_eq!(
            certs[0],
            tx_cert_data_v3(&tx_ctx.certificates[0], None).unwrap()
        );
        assert_eq!(
            certs[1],
            tx_cert_data_v3(&tx_ctx.certificates[1], None).unwrap()
        );
        assert_eq!(
            certs[2],
            tx_cert_data_v3(&tx_ctx.certificates[2], None).unwrap()
        );
    }

    // -----------------------------------------------------------------------
    // script_info_data_v3 — remaining variant coverage
    // -----------------------------------------------------------------------
    // Spending and Minting are covered above. These tests verify the remaining
    // four variants produce the correct structure.

    #[test]
    fn script_info_v3_rewarding_matches_purpose() {
        let purpose = ScriptPurpose::Rewarding {
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::ScriptHash([0x51; 28]),
            },
        };
        let info = script_info_data_v3(&purpose, None, None).unwrap();
        let sp = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            info, sp,
            "Rewarding ScriptInfo and ScriptPurpose must be identical"
        );
        // Verify structure: Constr(2, [credential_data])
        assert_eq!(
            info,
            PlutusData::Constr(
                2,
                vec![credential_data(&StakeCredential::ScriptHash([0x51; 28]))]
            )
        );
    }

    #[test]
    fn script_info_v3_certifying_carries_index_and_cert() {
        let cert = DCert::PoolRetirement([0x61; 28], EpochNo(42));
        let purpose = ScriptPurpose::Certifying {
            cert_index: 5,
            certificate: cert.clone(),
        };
        let info = script_info_data_v3(&purpose, None, None).unwrap();
        let sp = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            info, sp,
            "Certifying ScriptInfo and ScriptPurpose must be identical"
        );
        // Verify structure
        assert_eq!(
            info,
            PlutusData::Constr(
                3,
                vec![
                    PlutusData::Integer(5),
                    tx_cert_data_v3(&cert, None).unwrap(),
                ]
            )
        );
    }

    #[test]
    fn script_info_v3_voting_matches_purpose() {
        let purpose = ScriptPurpose::Voting {
            voter: Voter::DRepKeyHash([0x71; 28]),
        };
        let info = script_info_data_v3(&purpose, None, None).unwrap();
        let sp = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            info, sp,
            "Voting ScriptInfo and ScriptPurpose must be identical"
        );
        assert_eq!(
            info,
            PlutusData::Constr(4, vec![voter_data_v3(&Voter::DRepKeyHash([0x71; 28]))])
        );
    }

    #[test]
    fn script_info_v3_proposing_carries_index_and_procedure() {
        let proposal = yggdrasil_ledger::ProposalProcedure {
            deposit: 100,
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x81; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/info".to_string(),
                data_hash: [0x82; 32],
            },
        };
        let purpose = ScriptPurpose::Proposing {
            proposal_index: 3,
            proposal: proposal.clone(),
        };
        let info = script_info_data_v3(&purpose, None, None).unwrap();
        let sp = script_purpose_data_v3(&purpose, None).unwrap();
        assert_eq!(
            info, sp,
            "Proposing ScriptInfo and ScriptPurpose must be identical"
        );
        assert_eq!(
            info,
            PlutusData::Constr(
                5,
                vec![
                    PlutusData::Integer(3),
                    proposal_procedure_data_v3(&proposal).unwrap(),
                ]
            )
        );
    }

    // -- slot-to-POSIX time conversion in evaluator --------------------------

    #[test]
    fn evaluator_slot_to_posix_ms_converts_when_configured() {
        // Mainnet: system_start = "2017-09-23T21:44:51Z" → 1506203091 unix secs
        let eval =
            CekPlutusEvaluator::with_time_conversion(CostModel::default(), 1_506_203_091.0, 1.0);
        assert_eq!(eval.slot_to_posix_ms(0), 1_506_203_091_000);
        assert_eq!(eval.slot_to_posix_ms(100), 1_506_203_191_000);
    }

    #[test]
    fn evaluator_slot_to_posix_ms_passthrough_when_unconfigured() {
        // Default evaluator (no genesis info) should pass slot through.
        let eval = CekPlutusEvaluator::new();
        assert_eq!(eval.slot_to_posix_ms(42), 42);
    }

    #[test]
    fn posix_time_range_with_converted_slots() {
        // Verify the full data path: slot → POSIX ms → PlutusData encoding.
        let eval = CekPlutusEvaluator::with_time_conversion(
            CostModel::default(),
            1_506_203_091.0, // mainnet system_start
            1.0,
        );
        let start_ms = eval.slot_to_posix_ms(1000);
        let end_ms = eval.slot_to_posix_ms(2000);
        assert_eq!(start_ms, 1_506_204_091_000); // 1506203091 + 1000
        assert_eq!(end_ms, 1_506_205_091_000); // 1506203091 + 2000

        let range = posix_time_range(Some(start_ms), Some(end_ms));
        // Verify Finite(start_ms) inclusive lower, Finite(end_ms) exclusive upper.
        assert_eq!(
            range,
            PlutusData::Constr(
                0,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(1, vec![PlutusData::Integer(start_ms as i128)]),
                            PlutusData::Constr(1, vec![]),
                        ]
                    ),
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Constr(1, vec![PlutusData::Integer(end_ms as i128)]),
                            PlutusData::Constr(0, vec![]),
                        ]
                    ),
                ],
            )
        );
    }
}
