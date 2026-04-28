use super::*;

/// Helper: build a Conway-era block from Conway tx bodies.
fn make_conway_committee_block(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<ConwayTxBody>,
) -> Block {
    let tx_list: Vec<Tx> = txs
        .iter()
        .map(|body| {
            let raw = body.to_cbor_bytes();
            let id_hash = yggdrasil_crypto::hash_bytes_256(&raw);
            Tx {
                id: TxId(id_hash.0),
                body: raw,
                witnesses: None,
                auxiliary_data: None,
                is_valid: None,
            }
        })
        .collect();

    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: tx_list,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

/// Helper: build a minimal ConwayTxBody with given inputs, outputs, fee,
/// and optional certificates (mirrors the Shelley helper shape).
fn conway_tx(
    inputs: Vec<ShelleyTxIn>,
    outputs: Vec<BabbageTxOut>,
    fee: u64,
    certificates: Option<Vec<DCert>>,
) -> ConwayTxBody {
    ConwayTxBody {
        inputs,
        outputs,
        fee,
        ttl: None,
        certificates,
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
    }
}

#[test]
fn ledger_state_authorizes_known_committee_member_hot_key() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x68; 28]);
    let hot_credential = StakeCredential::ScriptHash([0x69; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_credential, 200);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let block = make_conway_committee_block(
        12,
        1,
        0xAB,
        vec![conway_tx(
            vec![ShelleyTxIn {
                transaction_id: [0xAA; 32],
                index: 0,
            }],
            vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            100_000,
            Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
        )],
    );

    state
        .apply_block(&block)
        .expect("committee authorization block");
    assert_eq!(
        state
            .committee_member_state(&cold_credential)
            .expect("committee member")
            .authorization(),
        Some(&CommitteeAuthorization::CommitteeHotCredential(
            hot_credential
        ))
    );
}

#[test]
fn ledger_state_resigns_known_committee_member() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x6A; 28]);
    let hot_credential = StakeCredential::AddrKeyHash([0x6B; 28]);
    let anchor = Anchor {
        url: "https://example.com/committee-resignation.json".to_string(),
        data_hash: [0x6C; 32],
    };
    state
        .committee_state_mut()
        .register_with_term(cold_credential, 200);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xAC; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xAD; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let authorization_block = make_conway_committee_block(
        12,
        1,
        0xAE,
        vec![conway_tx(
            vec![ShelleyTxIn {
                transaction_id: [0xAC; 32],
                index: 0,
            }],
            vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            100_000,
            Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
        )],
    );

    state
        .apply_block(&authorization_block)
        .expect("committee authorization block");

    let block = make_conway_committee_block(
        13,
        2,
        0xAF,
        vec![conway_tx(
            vec![ShelleyTxIn {
                transaction_id: [0xAD; 32],
                index: 0,
            }],
            vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            100_000,
            Some(vec![DCert::CommitteeResignation(
                cold_credential,
                Some(anchor.clone()),
            )]),
        )],
    );

    state
        .apply_block(&block)
        .expect("committee resignation block");
    assert_eq!(
        state
            .committee_member_state(&cold_credential)
            .expect("committee member")
            .authorization(),
        Some(&CommitteeAuthorization::CommitteeMemberResigned(Some(
            anchor
        )))
    );
}

#[test]
fn ledger_state_rejects_unknown_committee_member_authorization() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x6D; 28]);
    let hot_credential = StakeCredential::ScriptHash([0x6E; 28]);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xAE; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let block = make_conway_committee_block(
        12,
        1,
        0xAF,
        vec![conway_tx(
            vec![ShelleyTxIn {
                transaction_id: [0xAE; 32],
                index: 0,
            }],
            vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            100_000,
            Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
        )],
    );

    let err = state
        .apply_block(&block)
        .expect_err("unknown committee member");
    assert_eq!(err, LedgerError::CommitteeIsUnknown(cold_credential));
}

#[test]
fn ledger_state_rejects_reauthorizing_resigned_committee_member() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x70; 28]);
    let hot_credential = StakeCredential::ScriptHash([0x71; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_credential, 200);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xB0; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xB1; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let resignation_block = make_conway_committee_block(
        12,
        1,
        0xB2,
        vec![conway_tx(
            vec![ShelleyTxIn {
                transaction_id: [0xB0; 32],
                index: 0,
            }],
            vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            100_000,
            Some(vec![DCert::CommitteeResignation(cold_credential, None)]),
        )],
    );

    state
        .apply_block(&resignation_block)
        .expect("committee resignation block");

    let block = make_conway_committee_block(
        13,
        2,
        0xB3,
        vec![conway_tx(
            vec![ShelleyTxIn {
                transaction_id: [0xB1; 32],
                index: 0,
            }],
            vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            100_000,
            Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
        )],
    );

    let err = state
        .apply_block(&block)
        .expect_err("resigned committee member cannot reauthorize");
    assert_eq!(
        err,
        LedgerError::CommitteeHasPreviouslyResigned(cold_credential)
    );
}
