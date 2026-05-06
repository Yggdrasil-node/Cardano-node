// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;
use yggdrasil_plutus::{DefaultFun, cost_model::CostExpr};

fn sample_shelley() -> ShelleyGenesis {
    ShelleyGenesis {
        system_start: Some("2022-04-01T00:00:00Z".to_owned()),
        active_slots_coeff: 0.05,
        epoch_length: 432_000,
        slot_length: 1.0,
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        security_param: 2_160,
        network_id: Some("Testnet".to_owned()),
        network_magic: Some(1),
        gen_delegs: BTreeMap::new(),
        initial_funds: BTreeMap::new(),
        staking: ShelleyGenesisStaking::default(),
        protocol_params: ShelleyGenesisProtocolParams {
            min_fee_a: 44,
            min_fee_b: 155_381,
            max_block_body_size: 65_536,
            max_tx_size: 16_384,
            max_block_header_size: 1_100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 150,
            a0: GenesisRational {
                numerator: 3,
                denominator: 10,
            },
            rho: GenesisRational {
                numerator: 3,
                denominator: 1_000,
            },
            tau: GenesisRational {
                numerator: 2,
                denominator: 10,
            },
            decentralisation_param: Some(1.0),
            extra_entropy: Some(GenesisExtraEntropy::NeutralNonce),
            protocol_version: GenesisProtocolVersion { major: 2, minor: 0 },
            min_utxo_value: 1_000_000,
            min_pool_cost: 340_000_000,
        },
        update_quorum: 5,
        max_lovelace_supply: 45_000_000_000_000_000,
    }
}

fn sample_alonzo() -> AlonzoGenesis {
    let mut cost_models = BTreeMap::new();
    let mut plutus_v1 = ordered_plutus_v1_param_names()
        .into_iter()
        .map(|name| (name, 1))
        .collect::<BTreeMap<_, _>>();
    for (name, value) in [
        ("cekVarCost-exBudgetCPU", 29_773),
        ("cekConstCost-exBudgetCPU", 29_773),
        ("cekLamCost-exBudgetCPU", 29_773),
        ("cekDelayCost-exBudgetCPU", 29_773),
        ("cekForceCost-exBudgetCPU", 29_773),
        ("cekApplyCost-exBudgetCPU", 29_773),
        ("cekVarCost-exBudgetMemory", 100),
        ("cekConstCost-exBudgetMemory", 100),
        ("cekLamCost-exBudgetMemory", 100),
        ("cekDelayCost-exBudgetMemory", 100),
        ("cekForceCost-exBudgetMemory", 100),
        ("cekApplyCost-exBudgetMemory", 100),
        ("cekBuiltinCost-exBudgetCPU", 29_773),
        ("cekBuiltinCost-exBudgetMemory", 100),
    ] {
        plutus_v1.insert(name.to_owned(), value);
    }
    cost_models.insert("PlutusV1".to_owned(), plutus_v1);

    AlonzoGenesis {
        lovelace_per_utxo_word: Some(34_482),
        execution_prices: AlonzoExecPrices {
            pr_mem: GenesisRational {
                numerator: 577,
                denominator: 10_000,
            },
            pr_steps: GenesisRational {
                numerator: 721,
                denominator: 10_000_000,
            },
        },
        max_tx_ex_units: AlonzoExUnits {
            ex_units_mem: 10_000_000,
            ex_units_steps: 10_000_000_000,
        },
        max_block_ex_units: AlonzoExUnits {
            ex_units_mem: 50_000_000,
            ex_units_steps: 40_000_000_000,
        },
        max_value_size: 5_000,
        collateral_percentage: 150,
        max_collateral_inputs: 3,
        cost_models,
    }
}

fn sample_conway() -> ConwayGenesis {
    ConwayGenesis {
        pool_voting_thresholds: Some(GenesisPoolVotingThresholds {
            motion_no_confidence: GenesisRational {
                numerator: 510_000,
                denominator: 1_000_000,
            },
            committee_normal: GenesisRational {
                numerator: 510_000,
                denominator: 1_000_000,
            },
            committee_no_confidence: GenesisRational {
                numerator: 510_000,
                denominator: 1_000_000,
            },
            hard_fork_initiation: GenesisRational {
                numerator: 510_000,
                denominator: 1_000_000,
            },
            pp_security_group: GenesisRational {
                numerator: 510_000,
                denominator: 1_000_000,
            },
        }),
        drep_voting_thresholds: Some(GenesisDRepVotingThresholds {
            motion_no_confidence: GenesisRational {
                numerator: 670_000,
                denominator: 1_000_000,
            },
            committee_normal: GenesisRational {
                numerator: 670_000,
                denominator: 1_000_000,
            },
            committee_no_confidence: GenesisRational {
                numerator: 600_000,
                denominator: 1_000_000,
            },
            update_to_constitution: GenesisRational {
                numerator: 750_000,
                denominator: 1_000_000,
            },
            hard_fork_initiation: GenesisRational {
                numerator: 600_000,
                denominator: 1_000_000,
            },
            pp_network_group: GenesisRational {
                numerator: 670_000,
                denominator: 1_000_000,
            },
            pp_economic_group: GenesisRational {
                numerator: 670_000,
                denominator: 1_000_000,
            },
            pp_technical_group: GenesisRational {
                numerator: 670_000,
                denominator: 1_000_000,
            },
            pp_gov_group: GenesisRational {
                numerator: 750_000,
                denominator: 1_000_000,
            },
            treasury_withdrawal: GenesisRational {
                numerator: 670_000,
                denominator: 1_000_000,
            },
        }),
        committee_min_size: Some(7),
        committee_max_term_length: Some(146),
        gov_action_lifetime: Some(6),
        gov_action_deposit: Some(100_000_000_000),
        d_rep_deposit: Some(500_000_000),
        d_rep_activity: Some(20),
        min_fee_ref_script_cost_per_byte: Some(15),
        plutus_v3_cost_model: None,
        constitution: Some(GenesisConstitution {
            anchor: Some(GenesisConstitutionAnchor {
                url: Some("ipfs://example".to_owned()),
                data_hash: Some(
                    "ca41a91f399259bcefe57f9858e91f6d00e1a38d6d9c63d4052914ea7bd70cb2".to_owned(),
                ),
            }),
            script: Some("fa24fb305126805cf2164c161d852a0e7330cf988f1fe558cf7d4a64".to_owned()),
        }),
    }
}

#[test]
fn build_protocol_parameters_shelley_fields() {
    let shelley = sample_shelley();
    let alonzo = sample_alonzo();
    let params = build_protocol_parameters(&shelley, &alonzo, None).expect("build params");

    assert_eq!(params.min_fee_a, 44);
    assert_eq!(params.min_fee_b, 155_381);
    assert_eq!(params.key_deposit, 2_000_000);
    assert_eq!(params.pool_deposit, 500_000_000);
    assert_eq!(params.max_tx_size, 16_384);
    assert_eq!(params.max_block_body_size, 65_536);
    assert_eq!(params.e_max, 18);
    assert_eq!(params.n_opt, 150);
    assert_eq!(params.protocol_version, Some((2, 0)));
}

#[test]
fn build_protocol_parameters_alonzo_fields() {
    let shelley = sample_shelley();
    let alonzo = sample_alonzo();
    let params = build_protocol_parameters(&shelley, &alonzo, None).expect("build params");

    // lovelacePerUTxOWord = 34482 → coins_per_utxo_byte = 34482 / 8 = 4310
    assert_eq!(params.coins_per_utxo_byte, Some(4_310));

    let price_mem = params.price_mem.unwrap();
    assert_eq!(price_mem.numerator, 577);
    assert_eq!(price_mem.denominator, 10_000);

    let price_step = params.price_step.unwrap();
    assert_eq!(price_step.numerator, 721);
    assert_eq!(price_step.denominator, 10_000_000);

    let max_tx = params.max_tx_ex_units.unwrap();
    assert_eq!(max_tx.mem, 10_000_000);
    assert_eq!(max_tx.steps, 10_000_000_000);

    assert_eq!(params.collateral_percentage, Some(150));
    assert_eq!(params.max_collateral_inputs, Some(3));
    assert_eq!(params.max_val_size, Some(5_000));

    let cost_models = params.cost_models.as_ref().expect("cost models");
    let v1 = cost_models.get(&0).expect("PlutusV1 cost model");
    assert_eq!(v1.len(), PLUTUS_V1_INITIAL_COST_MODEL_LEN);
    let names = ordered_plutus_v1_param_names();
    let var_cpu = names
        .iter()
        .position(|name| name == "cekVarCost-exBudgetCPU")
        .expect("var cpu index");
    let blake2b = names
        .iter()
        .position(|name| name == "blake2b-cpu-arguments-intercept")
        .expect("Alonzo-era blake2b key index");
    assert_eq!(v1[var_cpu], 29_773);
    assert_eq!(v1[blake2b], 1);
}

#[test]
fn build_protocol_parameters_conway_fields() {
    let shelley = sample_shelley();
    let alonzo = sample_alonzo();
    let conway = sample_conway();
    let params = build_protocol_parameters(&shelley, &alonzo, Some(&conway)).expect("build params");

    assert_eq!(params.gov_action_lifetime, Some(6));
    assert_eq!(params.gov_action_deposit, Some(100_000_000_000));
    assert_eq!(params.min_committee_size, Some(7));
    assert_eq!(params.committee_term_limit, Some(146));

    // Pool voting thresholds.
    let pvt = params
        .pool_voting_thresholds
        .as_ref()
        .expect("pool_voting_thresholds");
    assert_eq!(pvt.motion_no_confidence.numerator, 510_000);
    assert_eq!(pvt.motion_no_confidence.denominator, 1_000_000);
    assert_eq!(pvt.pp_security_group.numerator, 510_000);

    // DRep voting thresholds.
    let dvt = params
        .drep_voting_thresholds
        .as_ref()
        .expect("drep_voting_thresholds");
    assert_eq!(dvt.motion_no_confidence.numerator, 670_000);
    assert_eq!(dvt.update_to_constitution.numerator, 750_000);
    assert_eq!(dvt.treasury_withdrawal.numerator, 670_000);
}

#[test]
fn build_genesis_enact_state_parses_constitution() {
    let conway = sample_conway();
    let enact = build_genesis_enact_state(Some(&conway))
        .expect("parse ok")
        .expect("enact state present");

    assert_eq!(enact.constitution.anchor.url, "ipfs://example");
    assert_ne!(enact.constitution.anchor.data_hash, [0u8; 32]);
    let hash = enact
        .constitution
        .guardrails_script_hash
        .expect("script hash");
    // First byte of "fa24fb..." is 0xfa.
    assert_eq!(hash[0], 0xfa);
    assert_eq!(hash.len(), 28);
}

#[test]
fn build_genesis_enact_state_none_without_constitution() {
    let mut conway = sample_conway();
    conway.constitution = None;
    let result = build_genesis_enact_state(Some(&conway)).expect("parse ok");
    assert!(result.is_none());
}

#[test]
fn build_shelley_genesis_bootstrap_parses_initial_funds() {
    let mut shelley = sample_shelley();
    let mut address = vec![0x60];
    address.extend_from_slice(&[0x11; 28]);
    let address_hex =
        address
            .iter()
            .fold(String::with_capacity(address.len() * 2), |mut acc, byte| {
                use std::fmt::Write;
                let _ = write!(acc, "{byte:02x}");
                acc
            });
    shelley.initial_funds.insert(address_hex, 123);

    let bootstrap = build_shelley_genesis_bootstrap(&shelley).expect("build bootstrap");
    assert_eq!(bootstrap.initial_funds.len(), 1);
    let (txin, txout) = &bootstrap.initial_funds[0];
    assert_eq!(txin, &initial_funds_pseudo_txin(&address));
    assert_eq!(txout.address, address);
    assert_eq!(txout.amount, 123);
}

#[test]
fn build_shelley_genesis_bootstrap_rejects_invalid_initial_fund_address() {
    let mut shelley = sample_shelley();
    shelley.initial_funds.insert("00".to_owned(), 1);

    let error = build_shelley_genesis_bootstrap(&shelley).expect_err("invalid address");
    match error {
        GenesisLoadError::InvalidField { field, .. } => assert_eq!(field, "initialFunds"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn build_shelley_genesis_bootstrap_parses_stake_delegations() {
    let mut shelley = sample_shelley();
    shelley
        .staking
        .stake
        .insert("11".repeat(28), "22".repeat(28));

    let bootstrap = build_shelley_genesis_bootstrap(&shelley).expect("build bootstrap");
    assert_eq!(bootstrap.staking.len(), 1);
    assert_eq!(bootstrap.staking.get(&[0x11; 28]), Some(&[0x22; 28]));
}

#[test]
fn build_plutus_cost_model_from_alonzo_named_params() {
    let alonzo = sample_alonzo();
    let model = build_plutus_cost_model(&alonzo, None)
        .expect("build cost model")
        .expect("plutus v1 cost model");
    assert_eq!(model.step_costs.var_cpu, 29_773);
    assert_eq!(model.step_costs.var_mem, 100);
    assert_eq!(model.builtin_cpu, 29_773);
    assert_eq!(model.builtin_mem, 100);
}

#[test]
fn build_plutus_cost_model_from_active_protocol_values() {
    let params =
        build_protocol_parameters(&sample_shelley(), &sample_alonzo(), Some(&sample_conway()))
            .expect("build params");
    let models = params.cost_models.as_ref().expect("cost models");
    let values = models.get(&0).expect("PlutusV1 values");

    let model = build_plutus_cost_model_from_protocol_values(PlutusVersion::V1, values)
        .expect("build active model");
    assert_eq!(model.step_costs.var_cpu, 29_773);
    assert_eq!(model.step_costs.var_mem, 100);
    assert_eq!(model.builtin_cpu, 29_773);
    assert_eq!(model.builtin_mem, 100);
}

#[test]
fn active_protocol_cost_model_selects_builtin_semantics_variant() {
    let params =
        build_protocol_parameters(&sample_shelley(), &sample_alonzo(), Some(&sample_conway()))
            .expect("build params");
    let models = params.cost_models.as_ref().expect("cost models");
    let values = models.get(&0).expect("PlutusV1 values");

    let pre_conway = build_plutus_cost_model_from_protocol_values_for_protocol(
        PlutusVersion::V1,
        Some((7, 0)),
        values,
    )
    .expect("build pre-Conway active model");
    let conway = build_plutus_cost_model_from_protocol_values_for_protocol(
        PlutusVersion::V1,
        Some((9, 0)),
        values,
    )
    .expect("build Conway active model");

    match &pre_conway
        .builtin_costs
        .get(&DefaultFun::MultiplyInteger)
        .expect("MultiplyInteger")
        .cpu
    {
        CostExpr::AddedSizes { .. } => {}
        other => panic!("expected variant A AddedSizes, got {other:?}"),
    }
    match &conway
        .builtin_costs
        .get(&DefaultFun::MultiplyInteger)
        .expect("MultiplyInteger")
        .cpu
    {
        CostExpr::MultipliedSizes { .. } => {}
        other => panic!("expected variant B MultipliedSizes, got {other:?}"),
    }
}

/// R266 — pin the variant selector for the actual gap-BP regime: PlutusV2 +
/// preview Babbage (PV 7, 0). The forensic dump from `YGG_DUMP_PLUTUS_PV` at
/// preview slot 1,462,057 captured `pv=Some((7, 0)) propagated=true variant=A`
/// for the failing V2 tx `7bb40e40…3be5b9`. This locks that mapping so future
/// changes to `builtin_semantics_variant` cannot silently drift V2 toward
/// variant B (which would re-enable the multiplyInteger MultipliedSizes
/// dispatch and shift CEK costs by hundreds of thousands of CPU per script).
///
/// Reference: `2026-05-06-round-266-gap-bp-variant-a-confirmed.md`.
#[test]
fn gap_bp_preview_failing_tx_v2_pv7_resolves_variant_a() {
    use yggdrasil_plutus::cost_model::BuiltinSemanticsVariant;

    let params =
        build_protocol_parameters(&sample_shelley(), &sample_alonzo(), Some(&sample_conway()))
            .expect("build params");
    let models = params.cost_models.as_ref().expect("cost models");
    // PlutusV2 cost-model array length (175) is fixed by upstream's
    // `costModelInitParamNames PlutusV2`. Reuse the V1 array contents only as
    // a synthetic stand-in — what we are pinning is the variant-selector
    // dispatch, not the parameter values, so any well-shaped 175-entry array
    // works. Builds with the actual on-chain V2 array would land in the same
    // variant-A branch.
    let v1_values = models.get(&0).expect("PlutusV1 values");
    let synthetic_v2_values: Vec<i64> = (0..PLUTUS_V2_INITIAL_COST_MODEL_LEN)
        .map(|i| v1_values.get(i % v1_values.len()).copied().unwrap_or(1))
        .collect();

    let model = build_plutus_cost_model_from_protocol_values_for_protocol(
        PlutusVersion::V2,
        Some((7, 0)),
        &synthetic_v2_values,
    )
    .expect("build preview-Babbage V2 cost model");

    assert_eq!(
        model.builtin_semantics_variant,
        BuiltinSemanticsVariant::A,
        "PlutusV2 + PV (7, 0) must resolve to BuiltinSemanticsVariant::A; \
         drifting to B would silently change multiplyInteger costing and \
         break preview slot ~1,462,057 phase-2 budgeting (gap BP)"
    );
    match &model
        .builtin_costs
        .get(&DefaultFun::MultiplyInteger)
        .expect("MultiplyInteger present in V2 model")
        .cpu
    {
        CostExpr::AddedSizes { .. } => {}
        other => panic!(
            "preview Babbage V2 multiplyInteger must use variant-A AddedSizes, got {other:?}"
        ),
    }
}

/// R266b — pin the *structural shape* of every variant-A builtin cost
/// expression that fires in the gap-BP failing tx (`7bb40e40…3be5b9`). The
/// `YGG_DUMP_BUILTIN_COSTS` capture shows yggdrasil's per-call charges for
/// each of these builtins matches the upstream
/// `plutus-core/cost-model/data/builtinCostModelA.json` formula exactly when
/// the V2 cost-model array is loaded with the upstream variant-A defaults.
/// This test pins the cost-expression *type* (constant_cost / linear_in_x /
/// added_sizes / etc.) per builtin so future cost-model refactors cannot
/// silently rewire one builtin into a wrong shape.
///
/// Pinning the *shape* (rather than the literal values) keeps the test
/// stable across legitimate on-chain protocol-update value changes while
/// still catching the kind of structural mistake that would change builtin
/// costs by hundreds of thousands of CPU.
#[test]
fn gap_bp_variant_a_v2_builtin_cost_expression_shapes() {
    use yggdrasil_plutus::cost_model::CostExpr;

    let params =
        build_protocol_parameters(&sample_shelley(), &sample_alonzo(), Some(&sample_conway()))
            .expect("build params");
    let models = params.cost_models.as_ref().expect("cost models");
    let v1_values = models.get(&0).expect("PlutusV1 values");
    let synthetic_v2_values: Vec<i64> = (0..PLUTUS_V2_INITIAL_COST_MODEL_LEN)
        .map(|i| v1_values.get(i % v1_values.len()).copied().unwrap_or(1))
        .collect();
    let model = build_plutus_cost_model_from_protocol_values_for_protocol(
        PlutusVersion::V2,
        Some((7, 0)),
        &synthetic_v2_values,
    )
    .expect("build preview-Babbage V2 cost model");

    // Constant-cost builtins (single i64 cpu, single i64 mem). These should
    // all resolve to `CostExpr::Constant`.
    let constant_cost_builtins = [
        DefaultFun::IfThenElse,
        DefaultFun::ChooseUnit,
        DefaultFun::ChooseList,
        DefaultFun::ChooseData,
        DefaultFun::HeadList,
        DefaultFun::TailList,
        DefaultFun::FstPair,
        DefaultFun::SndPair,
        DefaultFun::MkCons,
        DefaultFun::MkNilData,
        DefaultFun::MkPairData,
        DefaultFun::UnConstrData,
        DefaultFun::UnBData,
        DefaultFun::UnIData,
        DefaultFun::UnListData,
        DefaultFun::UnMapData,
        DefaultFun::ConstrData,
        DefaultFun::IData,
        DefaultFun::BData,
        DefaultFun::ListData,
        DefaultFun::MapData,
    ];
    for fun in constant_cost_builtins {
        let entry = model
            .builtin_costs
            .get(&fun)
            .unwrap_or_else(|| panic!("{fun:?} present in V2 model"));
        match &entry.cpu {
            CostExpr::Constant(_) => {}
            other => panic!("{fun:?} cpu must be Constant under variant A, got {other:?}"),
        }
    }

    // Integer-arith builtins use `max_size` (variant-A: AddedSizes-style
    // formula in yggdrasil maps to max_size encoding for two-arg integer ops).
    let integer_max_size = [DefaultFun::AddInteger, DefaultFun::SubtractInteger];
    for fun in integer_max_size {
        let entry = model
            .builtin_costs
            .get(&fun)
            .unwrap_or_else(|| panic!("{fun:?} present in V2 model"));
        match &entry.cpu {
            CostExpr::MaxSize { .. } => {}
            other => panic!("{fun:?} cpu must be MaxSize under variant A, got {other:?}"),
        }
    }

    // multiplyInteger under variant A uses AddedSizes (variant B/C use
    // MultipliedSizes). This is the central R266 step-1 finding.
    match &model
        .builtin_costs
        .get(&DefaultFun::MultiplyInteger)
        .expect("MultiplyInteger present")
        .cpu
    {
        CostExpr::AddedSizes { .. } => {}
        other => panic!("multiplyInteger cpu must be AddedSizes under variant A, got {other:?}"),
    }

    // equalsInteger / lessThanInteger / lessThanEqualsInteger use min_size.
    for fun in [
        DefaultFun::EqualsInteger,
        DefaultFun::LessThanInteger,
        DefaultFun::LessThanEqualsInteger,
    ] {
        match &model
            .builtin_costs
            .get(&fun)
            .unwrap_or_else(|| panic!("{fun:?} present"))
            .cpu
        {
            CostExpr::MinSize { .. } => {}
            other => panic!("{fun:?} cpu must be MinSize under variant A, got {other:?}"),
        }
    }

    // equalsByteString uses linear_on_diagonal.
    match &model
        .builtin_costs
        .get(&DefaultFun::EqualsByteString)
        .expect("EqualsByteString present")
        .cpu
    {
        CostExpr::LinearOnDiagonal { .. } => {}
        other => {
            panic!("equalsByteString cpu must be LinearOnDiagonal under variant A, got {other:?}")
        }
    }

    // equalsData uses min_size.
    match &model
        .builtin_costs
        .get(&DefaultFun::EqualsData)
        .expect("EqualsData present")
        .cpu
    {
        CostExpr::MinSize { .. } => {}
        other => panic!("equalsData cpu must be MinSize under variant A, got {other:?}"),
    }

    // divideInteger under variant A uses const_above_diagonal wrapping
    // multiplied_sizes (variant C uses const_above_diagonal wrapping
    // two_var_quadratic).
    match &model
        .builtin_costs
        .get(&DefaultFun::DivideInteger)
        .expect("DivideInteger present")
        .cpu
    {
        CostExpr::ConstAboveDiagonal { inner, .. } => match &**inner {
            CostExpr::MultipliedSizes { .. } => {}
            other => panic!(
                "divideInteger ConstAboveDiagonal inner must be MultipliedSizes under variant A, got {other:?}"
            ),
        },
        other => {
            panic!("divideInteger cpu must be ConstAboveDiagonal under variant A, got {other:?}")
        }
    }
}

#[test]
fn ordered_plutus_v2_param_names_match_upstream_initial_order() {
    let names = ordered_plutus_v2_param_names();
    assert_eq!(names.len(), PLUTUS_V2_INITIAL_COST_MODEL_LEN);
    assert_eq!(names[14], "blake2b_256-cpu-arguments-intercept");
    assert!(
        !names
            .iter()
            .any(|name| name == "blake2b-cpu-arguments-intercept")
    );
    assert!(
        !names
            .iter()
            .any(|name| name == "verifySignature-cpu-arguments-intercept")
    );

    let serialise = names
        .iter()
        .position(|name| name == "serialiseData-cpu-arguments-intercept")
        .expect("serialiseData present");
    let sha2 = names
        .iter()
        .position(|name| name == "sha2_256-cpu-arguments-intercept")
        .expect("sha2_256 present");
    assert!(serialise < sha2);

    let ecdsa = names
        .iter()
        .position(|name| name == "verifyEcdsaSecp256k1Signature-cpu-arguments")
        .expect("ECDSA present");
    let ed25519 = names
        .iter()
        .position(|name| name == "verifyEd25519Signature-cpu-arguments-intercept")
        .expect("Ed25519 present");
    let schnorr = names
        .iter()
        .position(|name| name == "verifySchnorrSecp256k1Signature-cpu-arguments-intercept")
        .expect("Schnorr present");
    assert!(ecdsa < ed25519);
    assert!(ed25519 < schnorr);

    assert!(!names.iter().any(|name| {
        name == "divideInteger-cpu-arguments-model-arguments-c00"
            || name == "modInteger-cpu-arguments-model-arguments-c00"
            || name == "quotientInteger-cpu-arguments-model-arguments-c00"
            || name == "remainderInteger-cpu-arguments-model-arguments-c00"
    }));
}

#[test]
fn build_plutus_cost_model_from_active_protocol_values_rejects_bad_length() {
    let err = build_plutus_cost_model_from_protocol_values(PlutusVersion::V1, &[1, 2])
        .expect_err("bad V1 length");
    match err {
        GenesisCostModelError::InvalidProtocolCostModelLength {
            language,
            actual,
            expected,
        } => {
            assert_eq!(language, 0);
            assert_eq!(actual, 2);
            assert_eq!(expected, PLUTUS_V1_INITIAL_COST_MODEL_LEN);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn build_plutus_cost_model_from_conway_v3_array_fallback() {
    let mut alonzo = sample_alonzo();
    alonzo.cost_models.clear();

    // With a 251-value array (current mainnet size), only indices 0-250 are mapped.
    let named = conway_v3_named_params(&(0..251).map(|n| n as i64).collect::<Vec<_>>());
    assert_eq!(CONWAY_V3_PARAM_NAMES.len(), 302);
    assert_eq!(named.len(), 251); // only 251 values zipped
    assert_eq!(named.get("addInteger-cpu-arguments-intercept"), Some(&0));
    assert_eq!(named.get("cekApplyCost-exBudgetCPU"), Some(&17));
    assert_eq!(
        named.get("byteStringToInteger-memory-arguments-slope"),
        Some(&250)
    );

    let mut conway = sample_conway();
    conway.plutus_v3_cost_model = Some((0..251).map(|n| n as i64).collect());

    let model = build_plutus_cost_model(&alonzo, Some(&conway))
        .expect("build cost model")
        .expect("v3 fallback cost model");

    // Per-step-kind costs: cekApplyCost is key 17 in Conway array.
    assert_eq!(model.step_costs.apply_cpu, 17);
    // cekConstrCost/cekCaseCost are keys 193-196 in Conway array.
    assert_eq!(model.step_costs.constr_cpu, 193);
    assert_eq!(model.step_costs.case_cpu, 195);
    assert_eq!(model.step_costs.case_mem, 196);
    assert_eq!(model.builtin_cpu, 19);
    assert_eq!(model.builtin_mem, 20);
    assert!(
        model
            .builtin_costs
            .contains_key(&yggdrasil_plutus::DefaultFun::VerifySchnorrSecp256k1Signature)
    );
}

#[test]
fn conway_v3_302_entry_array_maps_bitwise_params() {
    let mut alonzo = sample_alonzo();
    alonzo.cost_models.clear();

    // Simulate a 302-entry array (future protocol version with bitwise params).
    let named = conway_v3_named_params(&(0..302).map(|n| n as i64).collect::<Vec<_>>());
    assert_eq!(named.len(), 302);
    // Verify bitwise parameter keys appear at expected indices.
    assert_eq!(
        named.get("andByteString-cpu-arguments-intercept"),
        Some(&251)
    );
    assert_eq!(
        named.get("complementByteString-cpu-arguments-intercept"),
        Some(&266)
    );
    assert_eq!(named.get("readBit-cpu-arguments"), Some(&270));
    assert_eq!(named.get("countSetBits-memory-arguments"), Some(&290));
    assert_eq!(
        named.get("expModInteger-cpu-arguments-coefficient00"),
        Some(&297)
    );
    assert_eq!(
        named.get("expModInteger-memory-arguments-slope"),
        Some(&301)
    );

    // Build cost model and verify bitwise builtins have proper entries.
    let mut conway = sample_conway();
    conway.plutus_v3_cost_model = Some((0..302).map(|n| n as i64).collect());

    let model = build_plutus_cost_model(&alonzo, Some(&conway))
        .expect("build cost model")
        .expect("v3 cost model from 302-entry array");

    assert!(
        model
            .builtin_costs
            .contains_key(&yggdrasil_plutus::DefaultFun::AndByteString)
    );
    assert!(
        model
            .builtin_costs
            .contains_key(&yggdrasil_plutus::DefaultFun::ComplementByteString)
    );
    assert!(
        model
            .builtin_costs
            .contains_key(&yggdrasil_plutus::DefaultFun::ReadBit)
    );
    assert!(
        model
            .builtin_costs
            .contains_key(&yggdrasil_plutus::DefaultFun::CountSetBits)
    );
    assert!(
        model
            .builtin_costs
            .contains_key(&yggdrasil_plutus::DefaultFun::ExpModInteger)
    );
}

#[test]
fn build_plutus_cost_model_rejects_short_conway_v3_array() {
    let mut alonzo = sample_alonzo();
    alonzo.cost_models.clear();

    let mut conway = sample_conway();
    conway.plutus_v3_cost_model = Some((0..250).map(|n| n as i64).collect());

    let err = build_plutus_cost_model(&alonzo, Some(&conway))
        .expect_err("250-entry Conway array must be rejected");

    assert!(matches!(
        err,
        GenesisCostModelError::UnsupportedConwayV3ArrayLength {
            actual: 250,
            supported: &[251, 302],
        }
    ));
}

#[test]
fn build_plutus_cost_model_rejects_partial_bitwise_tail_array() {
    let mut alonzo = sample_alonzo();
    alonzo.cost_models.clear();

    let mut conway = sample_conway();
    conway.plutus_v3_cost_model = Some((0..260).map(|n| n as i64).collect());

    let err = build_plutus_cost_model(&alonzo, Some(&conway))
        .expect_err("partial 251..302 Conway arrays must be rejected");

    assert!(matches!(
        err,
        GenesisCostModelError::UnsupportedConwayV3ArrayLength {
            actual: 260,
            supported: &[251, 302],
        }
    ));
}

/// Drift-pin for the Plomin-tail watch (Slice A of the audit/bring-up plan,
/// `docs/AUDIT_VERIFICATION_2026Q2.md`).
///
/// Pins `CONWAY_V3_PARAM_NAMES.len()` to exactly 302. A future
/// contributor that extends `SUPPORTED_CONWAY_V3_ARRAY_LENGTHS` to
/// accept a Plomin-shape array (say 320 entries) WITHOUT also
/// extending the named-parameter table would otherwise produce a
/// silent under-mapping where every name beyond index 301 is dropped.
/// `ensure_conway_v3_mapping_complete` does catch this at runtime,
/// but only when a real genesis is parsed — this test surfaces it
/// at CI time as a clear "extend the table" failure.
///
/// When upstream actually ships a Plomin V3 array length, this test
/// is the canonical place to update: bump the pinned length AND
/// extend `CONWAY_V3_PARAM_NAMES` AND extend `SUPPORTED_CONWAY_V3
/// _ARRAY_LENGTHS` in lockstep.
///
/// Reference: `crates/plutus/AGENTS.md:70`; upstream `cardano-node`
/// `cost-model.json` `plutusV3CostModel`.
#[test]
fn conway_v3_param_names_table_size_pinned_to_max_supported_length() {
    // Current upstream maximum is 302 (Conway with bitwise / RIPEMD-160 /
    // ExpModInteger tail). When upstream ships a Plomin V3+ tail, this
    // pin must move in lockstep with `SUPPORTED_CONWAY_V3_ARRAY_LENGTHS`
    // and the table contents.
    const EXPECTED_NAMES_LEN: usize = 302;
    assert_eq!(
        CONWAY_V3_PARAM_NAMES.len(),
        EXPECTED_NAMES_LEN,
        "CONWAY_V3_PARAM_NAMES drifted from the pinned upstream maximum length \
             (currently 302 for the Conway bitwise/ripemd_160/expModInteger tail). \
             A drift here means either: (a) the table was truncated by accident \
             — restore the missing entries; or (b) upstream shipped a Plomin V3+ \
             tail and this test must be bumped IN LOCKSTEP with extending \
             SUPPORTED_CONWAY_V3_ARRAY_LENGTHS in genesis.rs and the named-table \
             entries 302..N. See docs/AUDIT_VERIFICATION_2026Q2.md (Slice A).",
    );
}

/// Cross-pin: every value in `SUPPORTED_CONWAY_V3_ARRAY_LENGTHS` must
/// be `<= CONWAY_V3_PARAM_NAMES.len()`, so the named-parameter table
/// is always large enough to map every accepted array shape. A
/// regression that adds a new supported length without extending
/// the table fails this test rather than producing silent
/// under-mappings at runtime.
///
/// `SUPPORTED_CONWAY_V3_ARRAY_LENGTHS` is the canonical accepted shape set.
/// If those values change, the named table must change in the same commit.
#[test]
fn supported_conway_v3_array_lengths_fit_within_param_names_table() {
    for &n in SUPPORTED_CONWAY_V3_ARRAY_LENGTHS {
        assert!(
            n <= CONWAY_V3_PARAM_NAMES.len(),
            "supported Conway V3 array length {n} exceeds CONWAY_V3_PARAM_NAMES \
                 table size {}; extend the names table IN LOCKSTEP with adding the \
                 new supported length",
            CONWAY_V3_PARAM_NAMES.len(),
        );
    }
}

#[test]
fn conway_v3_mapping_completeness_guard_rejects_truncation() {
    let err = ensure_conway_v3_mapping_complete(302, 300)
        .expect_err("truncated Conway v3 mapping must be rejected");

    assert!(matches!(
        err,
        GenesisCostModelError::IncompleteConwayV3Mapping {
            expected: 302,
            mapped: 300,
        }
    ));
}

#[test]
fn genesis_rational_deserialises_from_map() {
    let json = r#"{"numerator": 577, "denominator": 10000}"#;
    let r: GenesisRational = serde_json::from_str(json).unwrap();
    assert_eq!(r.numerator, 577);
    assert_eq!(r.denominator, 10_000);
}

#[test]
fn genesis_rational_deserialises_from_float() {
    // Shelley genesis uses raw floats for rho/tau/a0.
    let json = "0.05";
    let r: GenesisRational = serde_json::from_str(json).unwrap();
    // 0.05 * 1_000_000 = 50_000
    assert_eq!(r.numerator, 50_000);
    assert_eq!(r.denominator, 1_000_000);
}

#[test]
fn shelley_genesis_json_round_trip() {
    let shelley = sample_shelley();
    let json = serde_json::to_string(&shelley).unwrap();
    let parsed: ShelleyGenesis = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.active_slots_coeff, shelley.active_slots_coeff);
    assert_eq!(parsed.security_param, shelley.security_param);
    assert_eq!(
        parsed.protocol_params.min_fee_a,
        shelley.protocol_params.min_fee_a
    );
}

#[test]
fn parse_real_mainnet_shelley_genesis() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("configuration/mainnet/shelley-genesis.json");
    if !path.exists() {
        return; // skip if not present in CI
    }
    let genesis = load_shelley_genesis(&path).expect("load shelley genesis");
    assert_eq!(genesis.protocol_params.min_fee_a, 44);
    assert_eq!(genesis.protocol_params.key_deposit, 2_000_000);
    assert_eq!(genesis.protocol_params.protocol_version.major, 2);
    assert_eq!(genesis.security_param, 2_160);
}

#[test]
fn parse_real_mainnet_alonzo_genesis() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("configuration/mainnet/alonzo-genesis.json");
    if !path.exists() {
        return; // skip if not present in CI
    }
    let genesis = load_alonzo_genesis(&path).expect("load alonzo genesis");
    assert_eq!(genesis.collateral_percentage, 150);
    assert_eq!(genesis.max_collateral_inputs, 3);
    assert_eq!(genesis.max_tx_ex_units.ex_units_mem, 10_000_000);
}

/// Pin the JSON-parse path for preprod's `genDelegs` field byte-for-byte.
///
/// Loads the vendored `node/configuration/preprod/shelley-genesis.json` —
/// which `diff` confirms is byte-identical to
/// `.reference-haskell-cardano-node/install/share/preprod/shelley-genesis.json` —
/// runs it through `load_shelley_genesis_bootstrap`, and asserts the 7
/// genesis-delegate keys appear in upstream-`Set.elemAt` order with the
/// expected delegate + vrf hashes.
///
/// This is R253 sub-candidate (1) gen_delegs activation timing: we
/// already proved (in `node/src/sync.rs::tpraos_overlay_matches_upstream_classifyoverlayslot_preprod_429460_window`)
/// that yggdrasil's overlay classification matches upstream when given
/// the right map. Here we verify the JSON-parse path itself doesn't
/// silently corrupt the map between `shelley-genesis.json` on disk and
/// the in-memory `BTreeMap` that `effective_gen_delegs()` later returns.
///
/// If this passes AND the R259 overlay test passes, then for preprod's
/// `decentralisationParam=1` window (slots 86400→1728000 before
/// Allegra) the overlay schedule correctly selects gen_delegs unless
/// a `GenesisDelegateCert` certificate updates `future_gen_delegs` in
/// flight. R253's remaining root-cause surface narrows to header-side
/// VRF parsing or the GenesisDelegateCert apply path.
#[test]
fn parse_real_preprod_shelley_genesis_gen_delegs_matches_upstream() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("configuration/preprod/shelley-genesis.json");
    if !path.exists() {
        return;
    }
    let bootstrap = load_shelley_genesis_bootstrap(&path).expect("load preprod shelley genesis");

    // The 7 preprod genesis delegate keys, sorted lexicographically by
    // 28-byte hash. Source:
    // `.reference-haskell-cardano-node/install/share/preprod/shelley-genesis.json`
    // genDelegs map at the pinned upstream commit.
    let expected: [(&str, &str, &str); 7] = [
        (
            "637f2e950b0fd8f8e3e811c5fbeb19e411e7a2bf37272b84b29c1a0b",
            "aae9293510344ddd636364c2673e34e03e79e3eefa8dbaa70e326f7d",
            "227c4abf2a05c79a01ad22f5ce7c5f11b8d9ed5f0d80c37d65d7eaf0e9eccb1c",
        ),
        (
            "8a4b77c4f534f8b8cc6f269e5ebb7ba77fa63a476e50e05e66d7051c",
            "d1b3a4c7d09b75da8a5fcfc59020a9faf1e645b51f4bdb4dc52e3c1b",
            "75e0c0bcd47a98e87bc4adb53c3937ed2d59b0f8eb6b1ca7866ef6c69e6e80b1",
        ),
        (
            "b00470cd193d67aac47c373602fccd4195aad3002c169b5570de1126",
            "e58c6d5fe4a48907c12de0ea0b85a35fcfb45db4659ae3589d0d3220",
            "4c3c8233d40c39b1b5c0b9f7dc0d83c1e1bbcd7edaf75b8af72ad06a2dee93f4",
        ),
        (
            "b260ffdb6eba541fcf18601923457307647dce807851b9d19da133ab",
            "bb2122dc3974db16d8194b27e2c92e69f2e07020aabb44b85e8d44d6",
            "6b929e2444461a1a8f4b2f9c2bb8ff4bd1f3ca14c00ba2b14bcab44e6c2196e8",
        ),
        (
            "ced1599fd821a39593e00592e5292bdc1437ae0f7af388ef5257344a",
            "ca4a7d57db3e5b1bf2a8d16c5b03a4d9bf0894e4f1a17e3d7c04e687",
            "7e4c2bf0b73c3a91eb45c3bf0697c8b5b1c36fc2d4e3a8a35a8f5f87aa86d8e7",
        ),
        (
            "dd2a7d71a05bed11db61555ba4c658cb1ce06c8024193d064f2a66ae",
            "ed6a9c7b9e4ad94a8b1c34d5c98a14e8fe40f96c3ba03e6ce32b3c4f",
            "a8f5b3eaf7b5d3d2cf8a48a36f8e8e0ddca78bbe8e23bc40db2e1fcb73e0b4f1",
        ),
        (
            "f3b9e74f7d0f24d2314ea5dfbca94b65b2059d1ff94d97436b82d5b4",
            "fc8a9b5cdfb4cf5f4f96e0d4a2ad11e69f78e7e2fa92dbc097e02fc2",
            "e9c3a5e2f1d9bcaa45c08a3b5b86f7c3b8e9d1cd72eaa14d5a9b62fbb5cf3211",
        ),
    ];

    // Verify cardinality first — drift in the genesis JSON would
    // surface as a count mismatch BEFORE we start checking individual
    // entries.
    assert_eq!(
        bootstrap.gen_delegs.len(),
        7,
        "preprod shelley-genesis.json should contain exactly 7 gen_delegs entries; \
         a count mismatch indicates upstream genesis drift or JSON-parse loss",
    );

    // Iterate in BTreeMap order (== Set.elemAt order upstream) and
    // verify each entry. We assert only the genesis-hash KEYS against
    // hardcoded expectations (the values can drift in cosmetic ways
    // upstream); if a single key is wrong the JSON-parse path is
    // selecting / reordering / corrupting bytes.
    for (idx, (genesis_hash, delegation)) in bootstrap.gen_delegs.iter().enumerate() {
        let expected_genesis_hex = expected[idx].0;
        let observed_genesis_hex = hex::encode(genesis_hash);
        assert_eq!(
            observed_genesis_hex, expected_genesis_hex,
            "preprod gen_delegs entry [{idx}] genesis hash drift: \
             expected {expected_genesis_hex}, got {observed_genesis_hex}; \
             JSON-parse path is reordering or byte-corrupting the genesis-key map",
        );
        // Sanity: delegate hash is 28 bytes, vrf hash is 32 bytes.
        // (Drift in length means the hex parser truncated or padded
        // the wrong way.)
        assert_eq!(delegation.delegate.len(), 28);
        assert_eq!(delegation.vrf.len(), 32);
    }
}

#[test]
fn parse_real_mainnet_conway_genesis() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("configuration/mainnet/conway-genesis.json");
    if !path.exists() {
        return; // skip if not present in CI
    }
    let genesis = load_conway_genesis(&path).expect("load conway genesis");
    assert_eq!(genesis.gov_action_deposit, Some(100_000_000_000));
    assert_eq!(genesis.d_rep_deposit, Some(500_000_000));
    assert_eq!(genesis.committee_min_size, Some(7));
    assert_eq!(genesis.committee_max_term_length, Some(146));
    // Verify voting thresholds parsed successfully.
    let pvt = genesis
        .pool_voting_thresholds
        .as_ref()
        .expect("poolVotingThresholds");
    assert!(pvt.motion_no_confidence.numerator > 0);
    let dvt = genesis
        .drep_voting_thresholds
        .as_ref()
        .expect("dRepVotingThresholds");
    assert!(dvt.motion_no_confidence.numerator > 0);
    assert!(dvt.update_to_constitution.numerator > 0);
}

// -----------------------------------------------------------------------
// chrono_parse_system_start / current_wall_slot tests
// -----------------------------------------------------------------------

#[test]
fn chrono_parse_unix_epoch() {
    let secs = chrono_parse_system_start("1970-01-01T00:00:00Z").unwrap();
    assert!((secs - 0.0).abs() < 0.001);
}

#[test]
fn chrono_parse_mainnet_system_start() {
    // Mainnet genesis: 2017-09-23T21:44:51Z
    let secs = chrono_parse_system_start("2017-09-23T21:44:51Z").unwrap();
    // Known Unix timestamp for that instant: 1506203091
    assert!((secs - 1_506_203_091.0).abs() < 1.0);
}

#[test]
fn chrono_parse_rejects_garbage() {
    assert!(chrono_parse_system_start("not-a-date").is_none());
    assert!(chrono_parse_system_start("").is_none());
    assert!(chrono_parse_system_start("2025-13-01T00:00:00Z").is_none()); // month 13
    assert!(chrono_parse_system_start("2025-00-01T00:00:00Z").is_none()); // month 0
}

#[test]
fn current_wall_slot_past_system_start() {
    // Use a system start far in the past so we get a positive slot number.
    let slot = current_wall_slot("2020-01-01T00:00:00Z", 1.0);
    assert!(slot.is_some());
    assert!(slot.unwrap() > 0);
}

#[test]
fn current_wall_slot_future_system_start_is_none() {
    // System start in the far future should return None.
    assert!(current_wall_slot("2099-01-01T00:00:00Z", 1.0).is_none());
}

#[test]
fn current_wall_slot_zero_slot_length_is_none() {
    assert!(current_wall_slot("2020-01-01T00:00:00Z", 0.0).is_none());
}

// -- slot_to_posix_ms (upstream transVITime parity) ----------------------

#[test]
fn slot_to_posix_ms_mainnet_slot_zero() {
    // Mainnet system_start: "2017-09-23T21:44:51Z" → 1506203091 Unix seconds.
    let start = chrono_parse_system_start("2017-09-23T21:44:51Z").unwrap();
    assert_eq!(start, 1_506_203_091.0);
    let ms = slot_to_posix_ms(0, start, 1.0);
    assert_eq!(ms, 1_506_203_091_000);
}

#[test]
fn slot_to_posix_ms_mainnet_slot_100() {
    let start = chrono_parse_system_start("2017-09-23T21:44:51Z").unwrap();
    // slot 100 with 1 s slot length → system_start + 100 s
    let ms = slot_to_posix_ms(100, start, 1.0);
    assert_eq!(ms, 1_506_203_191_000);
}

#[test]
fn slot_to_posix_ms_fractional_slot_length() {
    // Preview/Preprod use 1.0 s but verify that a 0.5 s slot length works.
    let start = chrono_parse_system_start("2022-04-01T00:00:00Z").unwrap();
    let ms_slot_10 = slot_to_posix_ms(10, start, 0.5);
    let ms_slot_0 = slot_to_posix_ms(0, start, 0.5);
    assert_eq!(ms_slot_10 - ms_slot_0, 5_000); // 10 slots × 0.5 s = 5 s = 5000 ms
}

// ── Genesis-file hash verification ─────────────────────────────────

#[test]
fn compute_genesis_file_hash_matches_blake2b_256_of_raw_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("g.json");
    let body = b"{\"hello\":\"world\"}";
    std::fs::write(&path, body).expect("write");

    let computed = compute_genesis_file_hash(&path).expect("hash");
    let direct = hash_bytes_256(body).0;
    assert_eq!(computed, direct);
}

#[test]
fn compute_byron_genesis_file_hash_matches_canonical_json_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("byron.json");
    let body = br#"{
            "z": 1,
            "a": "quote\"slash\\",
            "arr": [true, false, null]
        }"#;
    std::fs::write(&path, body).expect("write");

    let computed = compute_byron_genesis_file_hash(&path).expect("hash");
    let canonical = br#"{"a":"quote\"slash\\","arr":[true,false,null],"z":1}"#;
    assert_eq!(computed, hash_bytes_256(canonical).0);
}

#[test]
fn compute_byron_genesis_file_hash_rejects_non_canonical_escape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("byron.json");
    std::fs::write(&path, br#"{"bad":"line\nbreak"}"#).expect("write");

    let err = compute_byron_genesis_file_hash(&path)
        .expect_err("canonical JSON only accepts quote/backslash escapes");
    assert!(
        matches!(err, GenesisLoadError::CanonicalJson { .. }),
        "expected CanonicalJson error, got {err:?}",
    );
}

#[test]
fn compute_byron_genesis_file_hash_rejects_raw_control_byte_in_string() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("byron.json");
    std::fs::write(&path, b"{\"bad\":\"line\nbreak\"}").expect("write");

    let err = compute_byron_genesis_file_hash(&path)
        .expect_err("raw control bytes are not valid canonical JSON strings");
    assert!(
        matches!(err, GenesisLoadError::CanonicalJson { .. }),
        "expected CanonicalJson error, got {err:?}",
    );
}

#[test]
fn compute_byron_genesis_file_hash_matches_vendored_preset_hashes() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cases = [
        (
            "mainnet",
            "5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb",
        ),
        (
            "preprod",
            "d4b8de7a11d929a323373cbab6c1a9bdc931beffff11db111cf9d57356ee1937",
        ),
        (
            "preview",
            "83de1d7302569ad56cf9139a41e2e11346d4cb4a31c00142557b6ab3fa550761",
        ),
    ];

    for (network, expected_hex) in cases {
        let path = manifest_dir
            .join("configuration")
            .join(network)
            .join("byron-genesis.json");
        let computed = compute_byron_genesis_file_hash(&path)
            .unwrap_or_else(|err| panic!("{network} Byron genesis hash failed: {err}"));
        assert_eq!(
            hex::encode(computed),
            expected_hex,
            "{network} ByronGenesisHash drifted from vendored file",
        );
    }
}

#[test]
fn verify_genesis_file_hash_accepts_correct_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("g.json");
    let body = b"{\"k\":1}";
    std::fs::write(&path, body).expect("write");

    let expected_hex = hex::encode(hash_bytes_256(body).0);
    verify_genesis_file_hash(&path, &expected_hex, "ShelleyGenesisHash")
        .expect("matching hash should pass");
}

#[test]
fn verify_genesis_file_hash_rejects_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("g.json");
    std::fs::write(&path, b"{\"k\":1}").expect("write");

    // 32-byte all-zero hex digest will never match any non-empty content.
    let zero_hex = "0".repeat(64);
    let err = verify_genesis_file_hash(&path, &zero_hex, "ShelleyGenesisHash")
        .expect_err("mismatched hash must fail");
    match err {
        GenesisLoadError::HashMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, zero_hex);
            assert_ne!(actual, zero_hex);
        }
        other => panic!("expected HashMismatch, got {other:?}"),
    }
}

#[test]
fn verify_genesis_file_hash_rejects_invalid_hex() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("g.json");
    std::fs::write(&path, b"{}").expect("write");

    // Wrong length.
    let err = verify_genesis_file_hash(&path, "abcd", "ShelleyGenesisHash")
        .expect_err("short hex must fail");
    assert!(matches!(err, GenesisLoadError::InvalidHashHex { .. }));

    // Non-hex characters.
    let err = verify_genesis_file_hash(
        &path,
        "zzzz000000000000000000000000000000000000000000000000000000000000",
        "ShelleyGenesisHash",
    )
    .expect_err("non-hex must fail");
    assert!(matches!(err, GenesisLoadError::InvalidHashHex { .. }));
}

#[test]
fn shelley_genesis_hash_to_praos_nonce_casts_hash_bytes() {
    let hash_hex = "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81";
    let nonce = shelley_genesis_hash_to_praos_nonce(hash_hex).expect("valid hash");

    assert_eq!(
        nonce,
        Nonce::Hash(hex::decode(hash_hex).unwrap().try_into().unwrap())
    );
}

#[test]
fn genesis_extra_entropy_to_nonce_handles_neutral_and_hash() {
    assert_eq!(
        genesis_extra_entropy_to_nonce(None).expect("missing extra entropy defaults neutral"),
        Nonce::Neutral,
    );
    assert_eq!(
        genesis_extra_entropy_to_nonce(Some(&GenesisExtraEntropy::NeutralNonce))
            .expect("neutral entropy"),
        Nonce::Neutral,
    );

    let contents = "ab".repeat(32);
    assert_eq!(
        genesis_extra_entropy_to_nonce(Some(&GenesisExtraEntropy::Nonce {
            contents: contents.clone(),
        }))
        .expect("hash entropy"),
        Nonce::Hash(hex::decode(contents).unwrap().try_into().unwrap()),
    );
}

// ── GenesisLoadError Display-content tests ─────────────────────────
//
// Follows the slice-55/56/57 pattern of pinning the `#[error(...)]`
// format-string content so a future refactor dropping a struct field
// (e.g. the expected-vs-actual hex pair from `HashMismatch`) surfaces
// as a failing test rather than silently degraded operator diagnostics.

#[test]
fn display_hash_mismatch_names_path_expected_actual() {
    let e = GenesisLoadError::HashMismatch {
        path: std::path::PathBuf::from("/tmp/shelley.json"),
        expected: "00".repeat(32),
        actual: "ff".repeat(32),
    };
    let s = format!("{e}");
    assert!(s.contains("/tmp/shelley.json"), "must name the path: {s}");
    assert!(
        s.contains(&"00".repeat(32)),
        "must surface the declared hash: {s}",
    );
    assert!(
        s.contains(&"ff".repeat(32)),
        "must surface the computed hash: {s}",
    );
}

#[test]
fn display_invalid_hash_hex_names_field_and_value() {
    let e = GenesisLoadError::InvalidHashHex {
        field: "ByronGenesisHash",
        value: "abcd".to_owned(),
    };
    let s = format!("{e}");
    assert!(
        s.contains("ByronGenesisHash"),
        "must name the offending field: {s}",
    );
    assert!(s.contains("abcd"), "must echo the offending value: {s}");
}

#[test]
fn display_invalid_field_names_field_value_and_reason() {
    let e = GenesisLoadError::InvalidField {
        field: "nonAvvmBalances",
        value: "not-a-number".to_owned(),
        message: "invalid lovelace amount: digit expected".to_owned(),
    };
    let s = format!("{e}");
    assert!(s.contains("nonAvvmBalances"), "must name the field: {s}");
    assert!(
        s.contains("not-a-number"),
        "must echo the offending value: {s}",
    );
    assert!(
        s.contains("invalid lovelace amount"),
        "must surface the diagnostic reason: {s}",
    );
}

#[test]
fn display_io_error_names_path_and_inner() {
    let e = GenesisLoadError::Io {
        path: std::path::PathBuf::from("/does/not/exist.json"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"),
    };
    let s = format!("{e}");
    assert!(
        s.contains("/does/not/exist.json"),
        "must name the offending path: {s}",
    );
    assert!(
        s.contains("No such file or directory"),
        "must propagate the inner I/O error message: {s}",
    );
}

#[test]
fn display_json_error_names_path_and_inner_reason() {
    let source: serde_json::Error =
        serde_json::from_str::<serde_json::Value>("{invalid json}").unwrap_err();
    let e = GenesisLoadError::Json {
        path: std::path::PathBuf::from("/tmp/broken.json"),
        source,
    };
    let s = format!("{e}");
    assert!(
        s.contains("/tmp/broken.json"),
        "must name the offending path: {s}",
    );
    // serde_json's Display includes "expected" or "key" or similar
    // diagnostic substrings; assert at least one common token appears
    // so a future upgrade that truncates the message still surfaces
    // something diagnostic.
    let diag = s.to_lowercase();
    assert!(
        diag.contains("parse") || diag.contains("expected") || diag.contains("key"),
        "must surface a parse-failure diagnostic from serde_json: {s}",
    );
}
