use super::*;

fn seed_multi_era_shelley(
    utxo: &mut MultiEraUtxo,
    tx_hash: [u8; 32],
    index: u16,
    amount: u64,
) -> ShelleyTxIn {
    let txin = ShelleyTxIn {
        transaction_id: tx_hash,
        index,
    };
    utxo.insert_shelley(
        txin.clone(),
        ShelleyTxOut {
            address: vec![0x61; 29],
            amount,
        },
    );
    txin
}

/// Helper: seed a MultiEraUtxo with a Mary output (coin + multi-asset).
fn seed_multi_era_mary(
    utxo: &mut MultiEraUtxo,
    tx_hash: [u8; 32],
    index: u16,
    coin: u64,
    policy: [u8; 28],
    asset_name: Vec<u8>,
    asset_qty: u64,
) -> ShelleyTxIn {
    use std::collections::BTreeMap;
    let txin = ShelleyTxIn {
        transaction_id: tx_hash,
        index,
    };
    let mut assets = BTreeMap::new();
    assets.insert(asset_name, asset_qty);
    let mut ma = BTreeMap::new();
    ma.insert(policy, assets);
    utxo.insert(
        txin.clone(),
        MultiEraTxOut::Mary(MaryTxOut {
            address: vec![0x61; 29],
            amount: Value::CoinAndAssets(coin, ma),
        }),
    );
    txin
}

#[test]
fn multi_era_utxo_shelley_happy_path() {
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let tx_id = [0xAA; 32];
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![
            ShelleyTxOut {
                address: vec![0x00; 57],
                amount: 8_000_000,
            },
            ShelleyTxOut {
                address: vec![0x01; 57],
                amount: 1_800_000,
            },
        ],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    utxo.apply_shelley_tx(tx_id, &body, 500)
        .expect("valid shelley tx");
    assert_eq!(utxo.len(), 2);
    assert_eq!(
        utxo.get(&ShelleyTxIn {
            transaction_id: tx_id,
            index: 0
        })
        .expect("output 0")
        .coin(),
        8_000_000,
    );
}

#[test]
fn multi_era_utxo_allegra_optional_ttl() {
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 5_000_000);

    // Allegra tx with no TTL (valid at any slot).
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 4_800_000,
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    utxo.apply_allegra_tx([0xBB; 32], &body, 999_999_999)
        .expect("no TTL means always valid");
    assert_eq!(utxo.len(), 1);
}

#[test]
fn multi_era_utxo_allegra_validity_interval_start() {
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 5_000_000);

    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 4_800_000,
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: Some(500),
    };

    // Slot 400 < start 500 → not yet valid.
    let err = utxo
        .apply_allegra_tx([0xCC; 32], &body, 400)
        .expect_err("should reject: slot < validity_interval_start");
    assert_eq!(
        err,
        LedgerError::TxNotYetValid {
            start: 500,
            slot: 400
        }
    );
    assert_eq!(utxo.len(), 1);

    // Slot 500 == start 500 → valid.
    utxo.apply_allegra_tx([0xCC; 32], &body, 500)
        .expect("slot == start should be valid");
    assert_eq!(utxo.len(), 1);
}

#[test]
fn multi_era_utxo_mary_coin_only() {
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![
            MaryTxOut {
                address: vec![0x00; 57],
                amount: Value::Coin(8_000_000),
            },
            MaryTxOut {
                address: vec![0x01; 57],
                amount: Value::Coin(1_800_000),
            },
        ],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };

    utxo.apply_mary_tx([0xDD; 32], &body, 500)
        .expect("coin-only mary tx");
    assert_eq!(utxo.len(), 2);
    assert!(matches!(
        utxo.get(&ShelleyTxIn {
            transaction_id: [0xDD; 32],
            index: 0
        })
        .expect("output"),
        MultiEraTxOut::Mary(_)
    ));
}

#[test]
fn multi_era_utxo_mary_with_mint() {
    use std::collections::BTreeMap;
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let policy = [0xAA; 28];
    let asset_name = b"Token".to_vec();

    // Mint 100 tokens and send them to an output.
    let mut output_assets = BTreeMap::new();
    output_assets.insert(asset_name.clone(), 100u64);
    let mut output_ma = BTreeMap::new();
    output_ma.insert(policy, output_assets);

    let mut mint_assets: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    mint_assets.insert(asset_name.clone(), 100);
    let mut mint: BTreeMap<[u8; 28], BTreeMap<Vec<u8>, i64>> = BTreeMap::new();
    mint.insert(policy, mint_assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x00; 57],
            amount: Value::CoinAndAssets(9_800_000, output_ma),
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
    };

    utxo.apply_mary_tx([0xEE; 32], &body, 500)
        .expect("mary tx with mint");
    assert_eq!(utxo.len(), 1);

    // Verify the output has the minted tokens.
    let out = utxo
        .get(&ShelleyTxIn {
            transaction_id: [0xEE; 32],
            index: 0,
        })
        .expect("output");
    assert_eq!(out.coin(), 9_800_000);
    let value = out.value();
    let ma = value.multi_asset().expect("should have multi-asset");
    assert_eq!(*ma.get(&policy).expect("policy").get(&asset_name).expect("asset"), 100);
}

#[test]
fn multi_era_utxo_mary_rejects_unbalanced_multi_asset() {
    use std::collections::BTreeMap;
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let policy = [0xBB; 28];
    let asset_name = b"BadToken".to_vec();

    // Output claims 100 tokens but no mint → multi-asset not preserved.
    let mut output_assets = BTreeMap::new();
    output_assets.insert(asset_name.clone(), 100u64);
    let mut output_ma = BTreeMap::new();
    output_ma.insert(policy, output_assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x00; 57],
            amount: Value::CoinAndAssets(9_800_000, output_ma),
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };

    let err = utxo
        .apply_mary_tx([0xFF; 32], &body, 500)
        .expect_err("should reject unbalanced multi-asset");
    assert!(
        matches!(err, LedgerError::MultiAssetNotPreserved { .. }),
        "expected MultiAssetNotPreserved, got {err:?}"
    );
    assert_eq!(utxo.len(), 1);
}

#[test]
fn multi_era_utxo_mary_burn_tokens() {
    use std::collections::BTreeMap;
    let policy = [0xCC; 28];
    let asset_name = b"Burn".to_vec();

    // Seed with an input that already has 200 tokens.
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_mary(&mut utxo, [0x01; 32], 0, 10_000_000, policy, asset_name.clone(), 200);

    // Burn 50 tokens: consumed=200, mint=-50 → expected=150, produced must be 150.
    let mut output_assets = BTreeMap::new();
    output_assets.insert(asset_name.clone(), 150u64);
    let mut output_ma = BTreeMap::new();
    output_ma.insert(policy, output_assets);

    let mut mint_assets: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    mint_assets.insert(asset_name.clone(), -50);
    let mut mint: BTreeMap<[u8; 28], BTreeMap<Vec<u8>, i64>> = BTreeMap::new();
    mint.insert(policy, mint_assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x00; 57],
            amount: Value::CoinAndAssets(9_800_000, output_ma),
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
    };

    utxo.apply_mary_tx([0xDD; 32], &body, 500)
        .expect("burn should succeed");
    assert_eq!(utxo.len(), 1);
    let out = utxo
        .get(&ShelleyTxIn {
            transaction_id: [0xDD; 32],
            index: 0,
        })
        .expect("output");
    let ma = out.value().multi_asset().expect("has multi-asset").clone();
    assert_eq!(*ma.get(&policy).expect("policy").get(&asset_name).expect("asset"), 150);
}

#[test]
fn multi_era_utxo_mary_transfer_existing_tokens() {
    use std::collections::BTreeMap;
    let policy = [0xDD; 28];
    let asset_name = b"Transfer".to_vec();

    // Seed: input has 500 tokens.
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_mary(&mut utxo, [0x01; 32], 0, 10_000_000, policy, asset_name.clone(), 500);

    // Transfer: split into two outputs, 300 + 200, no mint.
    let mut out1_assets = BTreeMap::new();
    out1_assets.insert(asset_name.clone(), 300u64);
    let mut out1_ma = BTreeMap::new();
    out1_ma.insert(policy, out1_assets);

    let mut out2_assets = BTreeMap::new();
    out2_assets.insert(asset_name.clone(), 200u64);
    let mut out2_ma = BTreeMap::new();
    out2_ma.insert(policy, out2_assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![
            MaryTxOut {
                address: vec![0x00; 57],
                amount: Value::CoinAndAssets(5_000_000, out1_ma),
            },
            MaryTxOut {
                address: vec![0x01; 57],
                amount: Value::CoinAndAssets(4_800_000, out2_ma),
            },
        ],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };

    utxo.apply_mary_tx([0xEE; 32], &body, 500)
        .expect("token transfer should succeed");
    assert_eq!(utxo.len(), 2);
}

#[test]
fn multi_era_utxo_alonzo_happy_path() {
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x00; 57],
            amount: Value::Coin(9_800_000),
            datum_hash: Some([0xFF; 32]),
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    utxo.apply_alonzo_tx([0xAA; 32], &body, 500)
        .expect("alonzo tx");
    assert_eq!(utxo.len(), 1);

    let out = utxo
        .get(&ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        })
        .expect("output");
    assert!(matches!(out, MultiEraTxOut::Alonzo(_)));
    assert_eq!(out.coin(), 9_800_000);
}

#[test]
fn multi_era_utxo_babbage_happy_path() {
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x00; 57],
            amount: Value::Coin(9_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    utxo.apply_babbage_tx([0xBB; 32], &body, 500)
        .expect("babbage tx");
    assert_eq!(utxo.len(), 1);
    assert!(matches!(
        utxo.get(&ShelleyTxIn {
            transaction_id: [0xBB; 32],
            index: 0
        })
        .expect("output"),
        MultiEraTxOut::Babbage(_)
    ));
}

#[test]
fn multi_era_utxo_conway_happy_path() {
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x00; 57],
            amount: Value::Coin(9_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    utxo.apply_conway_tx([0xCC; 32], &body, 500)
        .expect("conway tx");
    assert_eq!(utxo.len(), 1);
    assert!(matches!(
        utxo.get(&ShelleyTxIn {
            transaction_id: [0xCC; 32],
            index: 0
        })
        .expect("output"),
        MultiEraTxOut::Babbage(_)
    ));
}

#[test]
fn multi_era_utxo_cross_era_spending() {
    // Seed with a Shelley output, then spend it in a Mary transaction.
    let mut utxo = MultiEraUtxo::new();
    seed_multi_era_shelley(&mut utxo, [0x01; 32], 0, 10_000_000);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x00; 57],
            amount: Value::Coin(9_800_000),
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };

    utxo.apply_mary_tx([0xDD; 32], &body, 500)
        .expect("spending shelley output in mary tx");
    assert_eq!(utxo.len(), 1);
    assert!(matches!(
        utxo.get(&ShelleyTxIn {
            transaction_id: [0xDD; 32],
            index: 0,
        })
        .expect("output"),
        MultiEraTxOut::Mary(_)
    ));
}

#[test]
fn multi_era_utxo_coin_accessors() {
    let shelley = MultiEraTxOut::Shelley(ShelleyTxOut {
        address: vec![0x01],
        amount: 42,
    });
    assert_eq!(shelley.coin(), 42);
    assert_eq!(shelley.value(), Value::Coin(42));
    assert_eq!(shelley.address(), &[0x01]);

    let mary = MultiEraTxOut::Mary(MaryTxOut {
        address: vec![0x02],
        amount: Value::Coin(100),
    });
    assert_eq!(mary.coin(), 100);
    assert_eq!(mary.address(), &[0x02]);

    let alonzo = MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: vec![0x03],
        amount: Value::Coin(200),
        datum_hash: None,
    });
    assert_eq!(alonzo.coin(), 200);

    let babbage = MultiEraTxOut::Babbage(BabbageTxOut {
        address: vec![0x04],
        amount: Value::Coin(300),
        datum_option: None,
        script_ref: None,
    });
    assert_eq!(babbage.coin(), 300);
}

