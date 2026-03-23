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
    Address,
    DCert,
    LedgerError,
    Script,
    plutus::PlutusData,
    plutus_validation::{PlutusEvaluator, PlutusScriptEval, PlutusVersion, ScriptPurpose, TxContext},
    StakeCredential,
};
use yggdrasil_plutus::{
    decode_script_bytes,
    types::{Constant, Term},
    CostModel, ExBudget, MachineError, Value,
};

// ---------------------------------------------------------------------------
// CekPlutusEvaluator
// ---------------------------------------------------------------------------

/// A [`PlutusEvaluator`] backed by the `yggdrasil-plutus` CEK machine.
///
/// Decodes each script from its on-chain Flat bytes, applies datum (if
/// spending), redeemer, and a version-aware ScriptContext, then evaluates
/// within the budget declared by the transaction.
#[derive(Clone, Debug, Default)]
pub struct CekPlutusEvaluator {
    /// Cost model to use. Defaults to `CostModel::default()`.
    pub cost_model: CostModel,
}

impl CekPlutusEvaluator {
    /// Create an evaluator with the default cost model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an evaluator with a custom cost model.
    pub fn with_cost_model(cost_model: CostModel) -> Self {
        Self { cost_model }
    }
}

impl PlutusEvaluator for CekPlutusEvaluator {
    fn evaluate(&self, eval: &PlutusScriptEval, tx_ctx: &TxContext) -> Result<(), LedgerError> {
        // 1. Decode the on-chain script bytes (Flat / CBOR-unwrap).
        let program = decode_script_bytes(&eval.script_bytes).map_err(|e| {
            LedgerError::PlutusScriptDecodeError {
                hash: eval.script_hash,
                reason: e.to_string(),
            }
        })?;

        // 2. Build Term::Constant wrappers for datum, redeemer, and context.
        let redeemer_term = data_term(eval.redeemer.clone());
        // Build the ScriptContext from the normalized ledger transaction view.
        let context_term = Term::Constant(Constant::Data(script_context_data(eval, tx_ctx)?));

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
                Box::new(Term::Apply(
                    Box::new(program.term),
                    Box::new(redeemer_term),
                )),
                Box::new(context_term),
            ),
        };

        // 4. Build execution budget from the transaction's declared ExUnits.
        //    ExUnits.steps → cpu; ExUnits.mem → mem.
        let budget = ExBudget::new(
            eval.ex_units.steps as i64,
            eval.ex_units.mem as i64,
        );

        // 5. Evaluate the applied term.
        let (result, _logs) =
            yggdrasil_plutus::evaluate_term(applied, budget, self.cost_model.clone())
                .map_err(|e| map_machine_error(&eval.script_hash, e))?;

        // 6. PlutusV3 scripts must explicitly return Bool(true).
        //    PlutusV1/V2 accept any non-error result.
        if eval.version == PlutusVersion::V3 {
            match result {
                Value::Constant(Constant::Bool(true)) => Ok(()),
                other => Err(LedgerError::PlutusScriptFailed {
                    hash: eval.script_hash,
                    reason: format!(
                        "PlutusV3 script must return Bool(true), got: {:?}",
                        other
                    ),
                }),
            }
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Wrap a [`PlutusData`] value in a `Term::Constant`.
fn data_term(data: PlutusData) -> Term {
    Term::Constant(Constant::Data(data))
}

fn script_context_data(
    eval: &PlutusScriptEval,
    tx_ctx: &TxContext,
) -> Result<PlutusData, LedgerError> {
    Ok(match eval.version {
        PlutusVersion::V1 | PlutusVersion::V2 => PlutusData::Constr(
            0,
            vec![build_tx_info(eval.version, tx_ctx)?, script_purpose_data_v1v2(&eval.purpose)],
        ),
        PlutusVersion::V3 => PlutusData::Constr(
            0,
            vec![
                build_tx_info(eval.version, tx_ctx)?,
                eval.redeemer.clone(),
                script_info_data_v3(&eval.purpose, eval.datum.as_ref())?,
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
fn build_tx_info(version: PlutusVersion, tx_ctx: &TxContext) -> Result<PlutusData, LedgerError> {
    // -- Shared building blocks --

    let inputs_data = PlutusData::List(
        tx_ctx
            .inputs
            .iter()
            .filter_map(|(txin, txout)| plutus_input_data(txin, txout))
            .collect(),
    );

    let ref_inputs_data = PlutusData::List(
        tx_ctx
            .resolved_reference_inputs
            .iter()
            .filter_map(|(txin, txout)| plutus_input_data(txin, txout))
            .collect(),
    );

    let outputs_data = PlutusData::List(
        tx_ctx.outputs.iter().filter_map(plutus_output_data).collect(),
    );

    // Fee as lovelace-only Value map: "" -> ("" -> coin)
    let fee = PlutusData::Map(vec![
        (PlutusData::Bytes(vec![]), PlutusData::Integer(tx_ctx.fee as i128)),
    ]);

    let mint_entries: Vec<(PlutusData, PlutusData)> = tx_ctx
        .mint
        .iter()
        .map(|(policy, assets)| {
            let asset_map: Vec<(PlutusData, PlutusData)> = assets
                .iter()
                .map(|(name, qty)| {
                    (PlutusData::Bytes(name.clone()), PlutusData::Integer(*qty as i128))
                })
                .collect();
            (PlutusData::Bytes(policy.to_vec()), PlutusData::Map(asset_map))
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

    let valid_range = posix_time_range(tx_ctx.validity_start, tx_ctx.ttl);

    let signatories = PlutusData::List(
        tx_ctx
            .required_signers
            .iter()
            .map(|h| PlutusData::Bytes(h.to_vec()))
            .collect(),
    );

    let datums = PlutusData::Map(
        tx_ctx
            .witness_datums
            .iter()
            .map(|(hash, datum)| (PlutusData::Bytes(hash.to_vec()), datum.clone()))
            .collect(),
    );

    let tx_id = PlutusData::Constr(0, vec![PlutusData::Bytes(tx_ctx.tx_hash.to_vec())]);
    let redeemers_v2 = PlutusData::Map(
        tx_ctx
            .redeemers
            .iter()
            .map(|(purpose, redeemer)| (script_purpose_data_v1v2(purpose), redeemer.clone()))
            .collect(),
    );
    let redeemers_v3 = PlutusData::Map(
        tx_ctx
            .redeemers
            .iter()
            .map(|(purpose, redeemer)| Ok((script_purpose_data_v3(purpose)?, redeemer.clone())))
            .collect::<Result<Vec<_>, LedgerError>>()?,
    );

    match version {
        PlutusVersion::V1 => Ok(PlutusData::Constr(
            0,
            vec![
                inputs_data,                                          // inputs
                outputs_data,                                         // outputs
                fee,                                                  // fee
                PlutusData::Map(mint_entries),                        // mint
                PlutusData::List(                                      // dcert (legacy encoding)
                    tx_ctx.certificates.iter().filter_map(legacy_dcert_data).collect(),
                ),
                PlutusData::Map(wdrl_entries_v2),                     // withdrawals
                valid_range,                                          // validRange
                signatories,                                          // signatories
                datums,                                               // datums
                tx_id,                                                // txInfoId
            ],
        )),

        PlutusVersion::V2 => Ok(PlutusData::Constr(
            0,
            vec![
                inputs_data,                                          // inputs
                ref_inputs_data,                                      // referenceInputs (NEW)
                outputs_data,                                         // outputs
                fee,                                                  // fee
                PlutusData::Map(mint_entries),                        // mint
                PlutusData::List(                                      // dcert (legacy encoding)
                    tx_ctx.certificates.iter().filter_map(legacy_dcert_data).collect(),
                ),
                PlutusData::Map(wdrl_entries_v2),                     // withdrawals
                valid_range,                                          // validRange
                signatories,                                          // signatories
                redeemers_v2,                                         // redeemers
                datums,                                               // datums
                tx_id,                                                // txInfoId
            ],
        )),

        PlutusVersion::V3 => {
            // V3 uses the richer TxCert encoding (not legacy DCert).
            let tx_certs = PlutusData::List(
                tx_ctx
                    .certificates
                    .iter()
                    .map(tx_cert_data_v3)
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let current_treasury = maybe_data(
                tx_ctx.current_treasury_value.map(|v| PlutusData::Integer(v as i128)),
            );
            let treasury_donation = maybe_data(
                tx_ctx.treasury_donation.map(|v| PlutusData::Integer(v as i128)),
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
                                                (gov_action_id_data(gov_action_id), vote_data_v3(procedure.vote))
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
                    inputs_data,                                      // inputs
                    ref_inputs_data,                                  // referenceInputs
                    outputs_data,                                     // outputs
                    fee,                                              // fee
                    PlutusData::Map(mint_entries),                    // mint
                    tx_certs,                                         // txCerts (V3 encoding)
                    PlutusData::Map(wdrl_entries_v3),                 // withdrawals
                    valid_range,                                      // validRange
                    signatories,                                      // signatories
                    redeemers_v3,                                     // redeemers
                    datums,                                           // datums
                    tx_id,                                            // txInfoId
                    votes,                                            // votes
                    proposal_procedures,                              // proposalProcedures
                    current_treasury,                                 // currentTreasuryAmount
                    treasury_donation,                                // treasuryDonation
                ],
            ))
        }
    }
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
                PlutusData::Constr(1, vec![]),                              // True (inclusive)
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
                PlutusData::Constr(1, vec![]),                              // True (inclusive)
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
    txin: &yggdrasil_ledger::eras::shelley::ShelleyTxIn,
    txout: &yggdrasil_ledger::utxo::MultiEraTxOut,
) -> Option<PlutusData> {
    Some(PlutusData::Constr(
        0,
        vec![plutus_txin_data(txin), plutus_output_data(txout)?],
    ))
}

/// Encode a TxOutRef as PlutusData: Constr(0, [Constr(0, [tx_hash]), index]).
fn plutus_txin_data(txin: &yggdrasil_ledger::eras::shelley::ShelleyTxIn) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::Constr(0, vec![PlutusData::Bytes(txin.transaction_id.to_vec())]),
            PlutusData::Integer(txin.index as i128),
        ],
    )
}

/// Encode a MultiEraTxOut as PlutusData.
///
/// V1/V2 TxOut: Constr(0, [address, value, datum_hash_option])
/// where address = Constr(0, [credential, maybe_staking_credential])
///       value   = Map[(policy, Map[(asset, qty)])]  or just lovelace
fn plutus_output_data(txout: &yggdrasil_ledger::utxo::MultiEraTxOut) -> Option<PlutusData> {
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
            let datum_field = match &o.datum_option {
                Some(yggdrasil_ledger::eras::babbage::DatumOption::Hash(h)) => {
                    PlutusData::Constr(1, vec![PlutusData::Bytes(h.to_vec())])
                }
                Some(yggdrasil_ledger::eras::babbage::DatumOption::Inline(d)) => {
                    PlutusData::Constr(2, vec![d.clone()])
                }
                None => PlutusData::Constr(0, vec![]),
            };
            // For V2 TxOut shape: Constr(0, [address, value, datum_option, script_ref_option])
            let script_ref_field = match &o.script_ref {
                Some(sref) => PlutusData::Constr(0, vec![PlutusData::Bytes(script_hash_from_ref(sref).to_vec())]),
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
                    (PlutusData::Bytes(name.clone()), PlutusData::Integer(*qty as i128))
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

fn script_purpose_data_v1v2(purpose: &ScriptPurpose) -> PlutusData {
    match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => {
            PlutusData::Constr(1, vec![tx_out_ref_data(tx_id, *index)])
        }
        ScriptPurpose::Rewarding { reward_account } => PlutusData::Constr(
            2,
            vec![staking_credential_data(&reward_account.credential)],
        ),
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => certifying_purpose_data(*cert_index, certificate),
        ScriptPurpose::Voting { voter } => {
            PlutusData::Constr(4, vec![voter_data_v3(voter)])
        }
        ScriptPurpose::Proposing {
            proposal_index,
            proposal,
        } => PlutusData::Constr(
            5,
            vec![
                PlutusData::Integer(*proposal_index as i128),
                proposal_procedure_data_v3(proposal)
                    .unwrap_or_else(|| PlutusData::Integer(*proposal_index as i128)),
            ],
        ),
    }
}

fn script_purpose_data_v3(purpose: &ScriptPurpose) -> Result<PlutusData, LedgerError> {
    Ok(match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => {
            PlutusData::Constr(1, vec![tx_out_ref_data(tx_id, *index)])
        }
        ScriptPurpose::Rewarding { reward_account } => PlutusData::Constr(
            2,
            vec![credential_data(&reward_account.credential)],
        ),
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => PlutusData::Constr(
            3,
            vec![
                PlutusData::Integer(*cert_index as i128),
                tx_cert_data_v3(certificate)?,
            ],
        ),
        ScriptPurpose::Voting { voter } => {
            PlutusData::Constr(4, vec![voter_data_v3(voter)])
        }
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
) -> Result<PlutusData, LedgerError> {
    Ok(match purpose {
        ScriptPurpose::Minting { policy_id } => {
            PlutusData::Constr(0, vec![PlutusData::Bytes(policy_id.to_vec())])
        }
        ScriptPurpose::Spending { tx_id, index } => PlutusData::Constr(
            1,
            vec![tx_out_ref_data(tx_id, *index), maybe_data(datum.cloned())],
        ),
        ScriptPurpose::Rewarding { reward_account } => PlutusData::Constr(
            2,
            vec![credential_data(&reward_account.credential)],
        ),
        ScriptPurpose::Certifying {
            cert_index,
            certificate,
        } => PlutusData::Constr(3, vec![
            PlutusData::Integer(*cert_index as i128),
            tx_cert_data_v3(certificate)?,
        ]),
        ScriptPurpose::Voting { voter } => {
            PlutusData::Constr(4, vec![voter_data_v3(voter)])
        }
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

fn certifying_purpose_data(cert_index: u64, certificate: &DCert) -> PlutusData {
    let certificate_data = legacy_dcert_data(certificate)
        .unwrap_or_else(|| PlutusData::Integer(cert_index as i128));
    PlutusData::Constr(3, vec![certificate_data])
}

fn tx_cert_data_v3(certificate: &DCert) -> Result<PlutusData, LedgerError> {
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
            vec![credential_data(credential), delegatee_stake_data(pool_key_hash)],
        )),
        DCert::AccountRegistrationDeposit(credential, deposit) => Ok(PlutusData::Constr(
            0,
            vec![credential_data(credential), maybe_lovelace(Some(*deposit))],
        )),
        DCert::AccountUnregistrationDeposit(credential, refund) => Ok(PlutusData::Constr(
            1,
            vec![credential_data(credential), maybe_lovelace(Some(*refund))],
        )),
        DCert::DelegationToDrep(credential, drep) => Ok(PlutusData::Constr(
            2,
            vec![credential_data(credential), delegatee_vote_data(drep)],
        )),
        DCert::DelegationToStakePoolAndDrep(credential, pool_key_hash, drep) => Ok(
            PlutusData::Constr(
                2,
                vec![
                    credential_data(credential),
                    delegatee_stake_vote_data(pool_key_hash, drep),
                ],
            ),
        ),
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
        DCert::AccountRegistrationDelegationToDrep(credential, drep, deposit) => Ok(
            PlutusData::Constr(
                3,
                vec![
                    credential_data(credential),
                    delegatee_vote_data(drep),
                    PlutusData::Integer(*deposit as i128),
                ],
            ),
        ),
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
            vec![drep_credential_data(credential), PlutusData::Integer(*deposit as i128)],
        )),
        DCert::DrepUpdate(credential, _) => {
            Ok(PlutusData::Constr(5, vec![drep_credential_data(credential)]))
        }
        DCert::DrepUnregistration(credential, refund) => Ok(PlutusData::Constr(
            6,
            vec![drep_credential_data(credential), PlutusData::Integer(*refund as i128)],
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

fn delegatee_stake_vote_data(pool_key_hash: &[u8; 28], drep: &yggdrasil_ledger::DRep) -> PlutusData {
    PlutusData::Constr(
        2,
        vec![PlutusData::Bytes(pool_key_hash.to_vec()), drep_data(drep)],
    )
}

fn voter_data_v3(voter: &yggdrasil_ledger::Voter) -> PlutusData {
    match voter {
        yggdrasil_ledger::Voter::CommitteeKeyHash(hash) => {
            PlutusData::Constr(0, vec![committee_credential_data(&StakeCredential::AddrKeyHash(*hash))])
        }
        yggdrasil_ledger::Voter::CommitteeScript(hash) => {
            PlutusData::Constr(0, vec![committee_credential_data(&StakeCredential::ScriptHash(*hash))])
        }
        yggdrasil_ledger::Voter::DRepKeyHash(hash) => {
            PlutusData::Constr(1, vec![drep_credential_data(&StakeCredential::AddrKeyHash(*hash))])
        }
        yggdrasil_ledger::Voter::DRepScript(hash) => {
            PlutusData::Constr(1, vec![drep_credential_data(&StakeCredential::ScriptHash(*hash))])
        }
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
        yggdrasil_ledger::DRep::KeyHash(hash) => {
            PlutusData::Constr(0, vec![drep_credential_data(&StakeCredential::AddrKeyHash(*hash))])
        }
        yggdrasil_ledger::DRep::ScriptHash(hash) => {
            PlutusData::Constr(0, vec![drep_credential_data(&StakeCredential::ScriptHash(*hash))])
        }
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

fn legacy_dcert_data(certificate: &DCert) -> Option<PlutusData> {
    match certificate {
        DCert::AccountRegistration(credential) => {
            Some(PlutusData::Constr(0, vec![staking_credential_data(credential)]))
        }
        DCert::AccountUnregistration(credential) => {
            Some(PlutusData::Constr(1, vec![staking_credential_data(credential)]))
        }
        DCert::DelegationToStakePool(credential, pool_key_hash) => Some(PlutusData::Constr(
            2,
            vec![
                staking_credential_data(credential),
                PlutusData::Bytes(pool_key_hash.to_vec()),
            ],
        )),
        DCert::PoolRegistration(pool_params) => Some(PlutusData::Constr(
            3,
            vec![
                PlutusData::Bytes(pool_params.operator.to_vec()),
                PlutusData::Bytes(pool_params.vrf_keyhash.to_vec()),
            ],
        )),
        DCert::PoolRetirement(pool_key_hash, epoch) => Some(PlutusData::Constr(
            4,
            vec![
                PlutusData::Bytes(pool_key_hash.to_vec()),
                PlutusData::Integer(epoch.0 as i128),
            ],
        )),
        DCert::GenesisDelegation(_, _, _) => Some(PlutusData::Constr(5, vec![])),
        DCert::AccountRegistrationDeposit(_, _)
        | DCert::AccountUnregistrationDeposit(_, _)
        | DCert::DelegationToDrep(_, _)
        | DCert::DelegationToStakePoolAndDrep(_, _, _)
        | DCert::AccountRegistrationDelegationToStakePool(_, _, _)
        | DCert::AccountRegistrationDelegationToDrep(_, _, _)
        | DCert::AccountRegistrationDelegationToStakePoolAndDrep(_, _, _, _)
        | DCert::CommitteeAuthorization(_, _)
        | DCert::CommitteeResignation(_, _)
        | DCert::DrepRegistration(_, _, _)
        | DCert::DrepUnregistration(_, _)
        | DCert::DrepUpdate(_, _) => None,
    }
}

fn tx_out_ref_data(tx_id: &[u8; 32], index: u64) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::Bytes(tx_id.to_vec()),
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
    LedgerError::PlutusScriptFailed {
        hash: *hash,
        reason: err.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::plutus_validation::{PlutusScriptEval, PlutusVersion, ScriptPurpose, TxContext};
    use yggdrasil_ledger::{
        Address,
        BaseAddress,
        BabbageTxOut,
        DatumOption,
        EnterpriseAddress,
        PointerAddress,
        RewardAccount, StakeCredential,
        types::Anchor,
        eras::alonzo::ExUnits,
        plutus::{PlutusData, ScriptRef},
        Value,
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
            other => panic!("expected Constr(0, [tx_info, redeemer, script_info]), got: {:?}", other),
        }
    }

    fn expect_script_context_data(eval: &PlutusScriptEval, tx_ctx: &TxContext) -> PlutusData {
        script_context_data(eval, tx_ctx).expect("script context should encode")
    }

    fn expect_tx_info(version: PlutusVersion, tx_ctx: &TxContext) -> PlutusData {
        build_tx_info(version, tx_ctx).expect("tx info should encode")
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
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Spending {
                tx_id: [0x11; 32],
                index: 7,
            },
            None,
            PlutusData::Integer(0),
        ), &test_tx_ctx());

        // Only check the purpose field (index 1); TxInfo is at index 0.
        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                1,
                vec![PlutusData::Constr(
                    0,
                    vec![
                        PlutusData::Bytes(vec![0x11; 32]),
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

        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Rewarding { reward_account },
            None,
            PlutusData::Integer(0),
        ), &test_tx_ctx());

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
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Minting {
                policy_id: [0x33; 28],
            },
            None,
            PlutusData::Integer(0),
        ), &test_tx_ctx());

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x33; 28])])
        );
    }

    #[test]
    fn script_context_data_encodes_legacy_certifying_certificate_shape() {
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V2,
            ScriptPurpose::Certifying {
                cert_index: 0,
                certificate: DCert::PoolRetirement([0x44; 28], yggdrasil_ledger::EpochNo(9)),
            },
            None,
            PlutusData::Integer(0),
        ), &test_tx_ctx());

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(
                3,
                vec![PlutusData::Constr(
                    4,
                    vec![
                        PlutusData::Bytes(vec![0x44; 28]),
                        PlutusData::Integer(9),
                    ],
                )],
            )
        );
    }

    #[test]
    fn script_context_data_falls_back_for_conway_only_certifying_certificate() {
        let data = expect_script_context_data(&test_eval(
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
        ), &test_tx_ctx());

        assert_eq!(
            extract_purpose_v1v2(&data),
            PlutusData::Constr(3, vec![PlutusData::Integer(2)])
        );
    }

    #[test]
    fn script_context_data_uses_v3_three_field_shape_for_spending() {
        let datum = PlutusData::Integer(12);
        let redeemer = PlutusData::Integer(34);
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Spending {
                tx_id: [0x55; 32],
                index: 4,
            },
            Some(datum.clone()),
            redeemer.clone(),
        ), &test_tx_ctx());

        // V3 ScriptContext = Constr(0, [tx_info, redeemer, script_info])
        let PlutusData::Constr(0, ref fields) = data else { panic!("expected outer Constr(0, ...)") };
        assert_eq!(fields.len(), 3, "V3 ScriptContext must have 3 fields");
        assert_eq!(fields[1], redeemer);
        assert_eq!(
            extract_script_info_v3(&data),
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::Constr(
                        0,
                        vec![
                            PlutusData::Bytes(vec![0x55; 32]),
                            PlutusData::Integer(4),
                        ],
                    ),
                    PlutusData::Constr(0, vec![datum]),
                ],
            )
        );
    }

    #[test]
    fn script_context_data_uses_v3_certifying_txcert_shape() {
        let data = expect_script_context_data(&test_eval(
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
        ), &test_tx_ctx());

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
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Voting {
                voter: yggdrasil_ledger::Voter::DRepScript([0x77; 28]),
            },
            None,
            PlutusData::Integer(88),
        ), &test_tx_ctx());

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
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Proposing {
                proposal_index: 2,
                proposal,
            },
            None,
            PlutusData::Integer(101),
        ), &test_tx_ctx());

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
        let PlutusData::Constr(0, fields) = tx_info else { panic!("TxInfo must be Constr(0, ...)") };
        assert_eq!(fields.len(), 10, "V1 TxInfo must have exactly 10 fields");
    }

    #[test]
    fn tx_info_v2_has_12_fields_with_reference_inputs() {
        let tx_info = expect_tx_info(PlutusVersion::V2, &test_tx_ctx());
        let PlutusData::Constr(0, fields) = tx_info else { panic!("TxInfo must be Constr(0, ...)") };
        assert_eq!(fields.len(), 12, "V2 TxInfo must have exactly 12 fields");
        // field 1 = referenceInputs, should be an empty list when no ref inputs provided
        assert_eq!(fields[1], PlutusData::List(vec![]));
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
        let PlutusData::Constr(0, fields) = tx_info else { panic!("TxInfo must be Constr(0, ...)") };
        assert_eq!(
            fields[9],
            PlutusData::Map(vec![(
                PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0x22; 28])]),
                PlutusData::Integer(5),
            )])
        );
    }

    #[test]
    fn tx_info_v3_has_16_fields() {
        let tx_info = expect_tx_info(PlutusVersion::V3, &test_tx_ctx());
        let PlutusData::Constr(0, fields) = tx_info else { panic!("TxInfo must be Constr(0, ...)") };
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
        let PlutusData::Constr(0, fields) = tx_info else { panic!("TxInfo must be Constr(0, ...)") };

        assert_eq!(
            fields[9],
            PlutusData::Map(vec![(
                PlutusData::Constr(4, vec![PlutusData::Constr(2, vec![PlutusData::Bytes(vec![0x33; 28])])]),
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
                        vec![
                            PlutusData::Bytes(vec![0x44; 32]),
                            PlutusData::Integer(3),
                        ],
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
        tx_ctx.withdrawals = vec![(
            RewardAccount {
                network: 1,
                credential: StakeCredential::ScriptHash([0x24; 28]),
            },
            11,
        )];

        let tx_info = expect_tx_info(PlutusVersion::V3, &tx_ctx);
        let PlutusData::Constr(0, fields) = tx_info else { panic!("TxInfo must be Constr(0, ...)") };

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

        let err = build_tx_info(PlutusVersion::V3, &tx_ctx)
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

        let err = build_tx_info(PlutusVersion::V3, &tx_ctx)
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
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(yggdrasil_ledger::ShelleyTxOut {
            address,
            amount: 5,
        });

        assert_eq!(
            plutus_output_data(&txout),
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
            plutus_output_data(&txout),
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
                                    vec![PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0x32; 28])])],
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
        let txout = yggdrasil_ledger::utxo::MultiEraTxOut::Shelley(yggdrasil_ledger::ShelleyTxOut {
            address: vec![0x80],
            amount: 1,
        });

        assert_eq!(plutus_output_data(&txout), None);
    }

    #[test]
    fn script_context_v1v2_has_two_field_outer_shape() {
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V1,
            ScriptPurpose::Minting { policy_id: [0; 28] },
            None,
            PlutusData::Integer(0),
        ), &test_tx_ctx());
        let PlutusData::Constr(0, ref fields) = data else { panic!() };
        assert_eq!(fields.len(), 2, "V1 ScriptContext must have 2 fields: [tx_info, purpose]");
    }

    #[test]
    fn script_context_v3_has_three_field_outer_shape() {
        let data = expect_script_context_data(&test_eval(
            PlutusVersion::V3,
            ScriptPurpose::Minting { policy_id: [0; 28] },
            None,
            PlutusData::Integer(0),
        ), &test_tx_ctx());
        let PlutusData::Constr(0, ref fields) = data else { panic!() };
        assert_eq!(fields.len(), 3, "V3 ScriptContext must have 3 fields: [tx_info, redeemer, script_info]");
    }
}
