use super::*;

pub(super) fn sample_hash28() -> [u8; 28] {
    let mut h = [0u8; 28];
    for (i, b) in h.iter_mut().enumerate() {
        *b = (i as u8) + 1;
    }
    h
}

pub(super) fn sample_hash28_alt() -> [u8; 28] {
    let mut h = [0u8; 28];
    for (i, b) in h.iter_mut().enumerate() {
        *b = (i as u8) + 0xa0;
    }
    h
}

pub(super) fn sample_hash32() -> [u8; 32] {
    let mut h = [0u8; 32];
    for (i, b) in h.iter_mut().enumerate() {
        *b = (i as u8) + 0x10;
    }
    h
}

// -- StakeCredential tests --

#[test]
fn stake_credential_key_hash_accessors() {
    let h = sample_hash28();
    let cred = StakeCredential::AddrKeyHash(h);
    assert!(cred.is_key_hash());
    assert!(!cred.is_script_hash());
    assert_eq!(cred.hash(), &h);
}

#[test]
fn stake_credential_script_hash_accessors() {
    let h = sample_hash28();
    let cred = StakeCredential::ScriptHash(h);
    assert!(!cred.is_key_hash());
    assert!(cred.is_script_hash());
    assert_eq!(cred.hash(), &h);
}

#[test]
fn stake_credential_key_hash_cbor_round_trip() {
    let cred = StakeCredential::AddrKeyHash(sample_hash28());
    let bytes = cred.to_cbor_bytes();
    let decoded = StakeCredential::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cred, decoded);
}

#[test]
fn stake_credential_script_hash_cbor_round_trip() {
    let cred = StakeCredential::ScriptHash(sample_hash28());
    let bytes = cred.to_cbor_bytes();
    let decoded = StakeCredential::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cred, decoded);
}

#[test]
fn stake_credential_cbor_encoding_structure() {
    let h = sample_hash28();
    let cred = StakeCredential::AddrKeyHash(h);
    let bytes = cred.to_cbor_bytes();
    // Should start with array(2), then unsigned(0), then bytes(28)
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("array");
    assert_eq!(len, 2);
    let tag = dec.unsigned().expect("tag");
    assert_eq!(tag, 0);
    let raw = dec.bytes().expect("hash");
    assert_eq!(raw, &h[..]);

    // Script hash should have tag 1
    let cred2 = StakeCredential::ScriptHash(h);
    let bytes2 = cred2.to_cbor_bytes();
    let mut dec2 = Decoder::new(&bytes2);
    let _ = dec2.array().expect("array");
    let tag2 = dec2.unsigned().expect("tag");
    assert_eq!(tag2, 1);
}

#[test]
fn stake_credential_decode_invalid_tag() {
    // Construct CBOR: [2, hash28] — invalid tag
    let mut enc = Encoder::new();
    enc.array(2).unsigned(2).bytes(&sample_hash28());
    let result = StakeCredential::from_cbor_bytes(&enc.into_bytes());
    assert!(result.is_err());
}

#[test]
fn stake_credential_decode_wrong_hash_length() {
    // Construct CBOR: [0, hash16] — wrong length
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0).bytes(&[0u8; 16]);
    let result = StakeCredential::from_cbor_bytes(&enc.into_bytes());
    assert!(result.is_err());
}

#[test]
fn stake_credential_ordering() {
    let a = StakeCredential::AddrKeyHash([0u8; 28]);
    let b = StakeCredential::AddrKeyHash([1u8; 28]);
    let c = StakeCredential::ScriptHash([0u8; 28]);
    assert!(a < b);
    assert!(a < c); // AddrKeyHash < ScriptHash in enum order
}

// -- RewardAccount tests --

#[test]
fn reward_account_key_hash_round_trip() {
    let ra = RewardAccount {
        network: 1,
        credential: StakeCredential::AddrKeyHash(sample_hash28()),
    };
    let bytes = ra.to_bytes();
    assert_eq!(bytes.len(), 29);
    assert_eq!(bytes[0], 0xe1); // 0xe0 | 1
    let decoded = RewardAccount::from_bytes(&bytes).expect("decode");
    assert_eq!(ra, decoded);
}

#[test]
fn reward_account_script_hash_round_trip() {
    let ra = RewardAccount {
        network: 0,
        credential: StakeCredential::ScriptHash(sample_hash28()),
    };
    let bytes = ra.to_bytes();
    assert_eq!(bytes.len(), 29);
    assert_eq!(bytes[0], 0xf0); // 0xf0 | 0
    let decoded = RewardAccount::from_bytes(&bytes).expect("decode");
    assert_eq!(ra, decoded);
}

#[test]
fn reward_account_from_bytes_invalid_length() {
    assert!(RewardAccount::from_bytes(&[0xe1; 28]).is_none());
    assert!(RewardAccount::from_bytes(&[0xe1; 30]).is_none());
    assert!(RewardAccount::from_bytes(&[]).is_none());
}

#[test]
fn reward_account_from_bytes_invalid_type() {
    // Header byte 0x01 — type nibble 0x0, not 0xe or 0xf
    let mut bytes = [0u8; 29];
    bytes[0] = 0x01;
    assert!(RewardAccount::from_bytes(&bytes).is_none());
}

#[test]
fn reward_account_cbor_round_trip() {
    let ra = RewardAccount {
        network: 1,
        credential: StakeCredential::AddrKeyHash(sample_hash28()),
    };
    let cbor = ra.to_cbor_bytes();
    let decoded = RewardAccount::from_cbor_bytes(&cbor).expect("decode");
    assert_eq!(ra, decoded);
}

#[test]
fn reward_account_cbor_script_round_trip() {
    let ra = RewardAccount {
        network: 0,
        credential: StakeCredential::ScriptHash(sample_hash28()),
    };
    let cbor = ra.to_cbor_bytes();
    let decoded = RewardAccount::from_cbor_bytes(&cbor).expect("decode");
    assert_eq!(ra, decoded);
}

// -- Address tests --

#[test]
fn base_address_key_key_round_trip() {
    let addr = Address::Base(BaseAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash(sample_hash28()),
        staking: StakeCredential::AddrKeyHash(sample_hash28_alt()),
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes.len(), 57);
    assert_eq!(bytes[0] >> 4, 0x0); // key/key
    assert_eq!(bytes[0] & 0x0f, 1); // network
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn base_address_script_key_round_trip() {
    let addr = Address::Base(BaseAddress {
        network: 0,
        payment: StakeCredential::ScriptHash(sample_hash28()),
        staking: StakeCredential::AddrKeyHash(sample_hash28_alt()),
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes[0] >> 4, 0x1); // script/key
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn base_address_key_script_round_trip() {
    let addr = Address::Base(BaseAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash(sample_hash28()),
        staking: StakeCredential::ScriptHash(sample_hash28_alt()),
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes[0] >> 4, 0x2); // key/script
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn base_address_script_script_round_trip() {
    let addr = Address::Base(BaseAddress {
        network: 0,
        payment: StakeCredential::ScriptHash(sample_hash28()),
        staking: StakeCredential::ScriptHash(sample_hash28_alt()),
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes[0] >> 4, 0x3); // script/script
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn enterprise_address_key_round_trip() {
    let addr = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash(sample_hash28()),
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes.len(), 29);
    assert_eq!(bytes[0] >> 4, 0x6);
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn enterprise_address_script_round_trip() {
    let addr = Address::Enterprise(EnterpriseAddress {
        network: 0,
        payment: StakeCredential::ScriptHash(sample_hash28()),
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes[0] >> 4, 0x7);
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn pointer_address_round_trip() {
    let addr = Address::Pointer(PointerAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash(sample_hash28()),
        slot: 100,
        tx_index: 2,
        cert_index: 0,
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes[0] >> 4, 0x4);
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn pointer_address_script_large_values() {
    let addr = Address::Pointer(PointerAddress {
        network: 0,
        payment: StakeCredential::ScriptHash(sample_hash28()),
        slot: 1_000_000,
        tx_index: 127,
        cert_index: 255,
    });
    let bytes = addr.to_bytes();
    assert_eq!(bytes[0] >> 4, 0x5);
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn reward_address_via_address_round_trip() {
    let ra = RewardAccount {
        network: 1,
        credential: StakeCredential::AddrKeyHash(sample_hash28()),
    };
    let addr = Address::Reward(ra);
    let bytes = addr.to_bytes();
    assert_eq!(bytes.len(), 29);
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

#[test]
fn byron_address_passthrough() {
    // Byron addresses start with type nibble 0x8
    let mut raw = vec![0x82]; // 0x8 << 4 | 0x2 = 0x82
    raw.extend_from_slice(&[0xaa; 56]);
    let addr = Address::from_bytes(&raw).expect("decode");
    match &addr {
        Address::Byron(b) => assert_eq!(b, &raw),
        other => panic!("expected Byron, got {other:?}"),
    }
    assert_eq!(addr.to_bytes(), raw);
}

#[test]
fn address_from_empty_bytes_returns_none() {
    assert!(Address::from_bytes(&[]).is_none());
}

#[test]
fn address_from_invalid_type_nibble_returns_none() {
    // Type nibble 0x9 is not assigned
    let mut bytes = [0u8; 29];
    bytes[0] = 0x91;
    assert!(Address::from_bytes(&bytes).is_none());
}

#[test]
fn address_network_accessor() {
    let base = Address::Base(BaseAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash(sample_hash28()),
        staking: StakeCredential::AddrKeyHash(sample_hash28_alt()),
    });
    assert_eq!(base.network(), Some(1));

    let enterprise = Address::Enterprise(EnterpriseAddress {
        network: 0,
        payment: StakeCredential::AddrKeyHash(sample_hash28()),
    });
    assert_eq!(enterprise.network(), Some(0));

    let byron = Address::Byron(vec![0x82, 0x00]);
    assert_eq!(byron.network(), None);
}

#[test]
fn base_address_wrong_length_returns_none() {
    // Type nibble 0x0 (base) but only 29 bytes — needs 57
    let mut bytes = [0u8; 29];
    bytes[0] = 0x01;
    assert!(Address::from_bytes(&bytes).is_none());
}

#[test]
fn enterprise_address_wrong_length_returns_none() {
    // Type nibble 0x6 but 57 bytes — needs 29
    let mut bytes = [0u8; 57];
    bytes[0] = 0x61;
    assert!(Address::from_bytes(&bytes).is_none());
}

#[test]
fn pointer_address_zero_values() {
    let addr = Address::Pointer(PointerAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash(sample_hash28()),
        slot: 0,
        tx_index: 0,
        cert_index: 0,
    });
    let bytes = addr.to_bytes();
    // header(1) + hash(28) + 3 zero-encoded varints(3) = 32 bytes
    assert_eq!(bytes.len(), 32);
    let decoded = Address::from_bytes(&bytes).expect("decode");
    assert_eq!(addr, decoded);
}

// =========================================================================
// Phase 49 — Certificate Hierarchy Tests
// =========================================================================

// -- UnitInterval ----------------------------------------------------------

#[test]
fn unit_interval_cbor_round_trip() {
    let ui = UnitInterval {
        numerator: 1,
        denominator: 3,
    };
    let bytes = ui.to_cbor_bytes();
    let decoded = UnitInterval::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ui, decoded);
}

#[test]
fn unit_interval_cbor_tag_30() {
    let ui = UnitInterval {
        numerator: 0,
        denominator: 1,
    };
    let bytes = ui.to_cbor_bytes();
    // Should start with tag 30 (0xd8 0x1e)
    assert_eq!(bytes[0], 0xd8);
    assert_eq!(bytes[1], 0x1e);
}

#[test]
fn unit_interval_large_values() {
    let ui = UnitInterval {
        numerator: 999_999,
        denominator: 1_000_000,
    };
    let bytes = ui.to_cbor_bytes();
    let decoded = UnitInterval::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ui, decoded);
}

// -- Relay -----------------------------------------------------------------

#[test]
fn relay_single_host_addr_full_round_trip() {
    let relay = Relay::SingleHostAddr(
        Some(3001),
        Some([127, 0, 0, 1]),
        Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
    );
    let bytes = relay.to_cbor_bytes();
    let decoded = Relay::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(relay, decoded);
}

#[test]
fn relay_single_host_addr_all_null_round_trip() {
    let relay = Relay::SingleHostAddr(None, None, None);
    let bytes = relay.to_cbor_bytes();
    let decoded = Relay::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(relay, decoded);
}

#[test]
fn relay_single_host_name_round_trip() {
    let relay = Relay::SingleHostName(Some(6000), "relay1.example.com".to_string());
    let bytes = relay.to_cbor_bytes();
    let decoded = Relay::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(relay, decoded);
}

#[test]
fn relay_single_host_name_no_port_round_trip() {
    let relay = Relay::SingleHostName(None, "relay.example.com".to_string());
    let bytes = relay.to_cbor_bytes();
    let decoded = Relay::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(relay, decoded);
}

#[test]
fn relay_multi_host_name_round_trip() {
    let relay = Relay::MultiHostName("pool.example.com".to_string());
    let bytes = relay.to_cbor_bytes();
    let decoded = Relay::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(relay, decoded);
}

// -- PoolMetadata ----------------------------------------------------------

#[test]
fn pool_metadata_cbor_round_trip() {
    let pm = PoolMetadata {
        url: "https://example.com/pool.json".to_string(),
        metadata_hash: sample_hash32(),
    };
    let bytes = pm.to_cbor_bytes();
    let decoded = PoolMetadata::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pm, decoded);
}

// -- DRep ------------------------------------------------------------------

#[test]
fn drep_key_hash_round_trip() {
    let drep = DRep::KeyHash(sample_hash28());
    let bytes = drep.to_cbor_bytes();
    let decoded = DRep::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(drep, decoded);
}

#[test]
fn drep_script_hash_round_trip() {
    let drep = DRep::ScriptHash(sample_hash28());
    let bytes = drep.to_cbor_bytes();
    let decoded = DRep::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(drep, decoded);
}

#[test]
fn drep_always_abstain_round_trip() {
    let drep = DRep::AlwaysAbstain;
    let bytes = drep.to_cbor_bytes();
    let decoded = DRep::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(drep, decoded);
}

#[test]
fn drep_always_no_confidence_round_trip() {
    let drep = DRep::AlwaysNoConfidence;
    let bytes = drep.to_cbor_bytes();
    let decoded = DRep::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(drep, decoded);
}

#[test]
fn drep_abstain_array_length_1() {
    let drep = DRep::AlwaysAbstain;
    let bytes = drep.to_cbor_bytes();
    // Should be [1-element array, uint(2)] = 0x81 0x02
    assert_eq!(bytes[0], 0x81);
    assert_eq!(bytes[1], 0x02);
}

#[test]
fn drep_key_hash_array_length_2() {
    let drep = DRep::KeyHash(sample_hash28());
    let bytes = drep.to_cbor_bytes();
    // Should be [2-element array, uint(0), bytes(28)] = 0x82 0x00 ...
    assert_eq!(bytes[0], 0x82);
    assert_eq!(bytes[1], 0x00);
}

// -- DCert (Shelley tags 0–5) ----------------------------------------------

pub(super) fn sample_pool_params() -> PoolParams {
    PoolParams {
        operator: sample_hash28(),
        vrf_keyhash: sample_hash32(),
        pledge: 500_000_000,
        cost: 340_000_000,
        margin: UnitInterval {
            numerator: 1,
            denominator: 100,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash(sample_hash28()),
        },
        pool_owners: vec![sample_hash28()],
        relays: vec![Relay::SingleHostName(
            Some(3001),
            "relay1.example.com".to_string(),
        )],
        pool_metadata: Some(PoolMetadata {
            url: "https://example.com/pool.json".to_string(),
            metadata_hash: sample_hash32(),
        }),
    }
}

#[test]
fn dcert_stake_registration_round_trip() {
    let cert = DCert::AccountRegistration(StakeCredential::AddrKeyHash(sample_hash28()));
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_stake_deregistration_round_trip() {
    let cert = DCert::AccountUnregistration(StakeCredential::ScriptHash(sample_hash28()));
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_stake_delegation_round_trip() {
    let cert =
        DCert::DelegationToStakePool(StakeCredential::AddrKeyHash(sample_hash28()), sample_hash28());
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_pool_registration_round_trip() {
    let cert = DCert::PoolRegistration(sample_pool_params());
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_pool_registration_no_metadata_round_trip() {
    let mut params = sample_pool_params();
    params.pool_metadata = None;
    params.relays = vec![];
    let cert = DCert::PoolRegistration(params);
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_pool_retirement_round_trip() {
    let cert = DCert::PoolRetirement(sample_hash28(), EpochNo(300));
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_genesis_key_delegation_round_trip() {
    let cert = DCert::GenesisDelegation(sample_hash28(), sample_hash28(), sample_hash32());
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

// -- DCert (Conway tags 7–18) ---------------------------------------------

#[test]
fn dcert_reg_cert_round_trip() {
    let cert = DCert::AccountRegistrationDeposit(StakeCredential::AddrKeyHash(sample_hash28()), 2_000_000);
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_unreg_cert_round_trip() {
    let cert = DCert::AccountUnregistrationDeposit(StakeCredential::ScriptHash(sample_hash28()), 2_000_000);
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_vote_deleg_cert_round_trip() {
    let cert = DCert::DelegationToDrep(
        StakeCredential::AddrKeyHash(sample_hash28()),
        DRep::KeyHash(sample_hash28()),
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_stake_vote_deleg_cert_round_trip() {
    let cert = DCert::DelegationToStakePoolAndDrep(
        StakeCredential::AddrKeyHash(sample_hash28()),
        sample_hash28(),
        DRep::AlwaysAbstain,
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_stake_reg_deleg_cert_round_trip() {
    let cert = DCert::AccountRegistrationDelegationToStakePool(
        StakeCredential::AddrKeyHash(sample_hash28()),
        sample_hash28(),
        2_000_000,
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_vote_reg_deleg_cert_round_trip() {
    let cert = DCert::AccountRegistrationDelegationToDrep(
        StakeCredential::AddrKeyHash(sample_hash28()),
        DRep::ScriptHash(sample_hash28()),
        2_000_000,
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_stake_vote_reg_deleg_cert_round_trip() {
    let cert = DCert::AccountRegistrationDelegationToStakePoolAndDrep(
        StakeCredential::AddrKeyHash(sample_hash28()),
        sample_hash28(),
        DRep::AlwaysNoConfidence,
        2_000_000,
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_auth_committee_hot_round_trip() {
    let cert = DCert::CommitteeAuthorization(
        StakeCredential::AddrKeyHash(sample_hash28()),
        StakeCredential::ScriptHash(sample_hash28()),
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_resign_committee_cold_with_anchor_round_trip() {
    let cert = DCert::CommitteeResignation(
        StakeCredential::AddrKeyHash(sample_hash28()),
        Some(Anchor {
            url: "https://example.com/resign.json".to_string(),
            data_hash: sample_hash32(),
        }),
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_resign_committee_cold_no_anchor_round_trip() {
    let cert = DCert::CommitteeResignation(
        StakeCredential::ScriptHash(sample_hash28()),
        None,
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_reg_drep_with_anchor_round_trip() {
    let cert = DCert::DrepRegistration(
        StakeCredential::AddrKeyHash(sample_hash28()),
        500_000_000,
        Some(Anchor {
            url: "https://example.com/drep.json".to_string(),
            data_hash: sample_hash32(),
        }),
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_reg_drep_no_anchor_round_trip() {
    let cert = DCert::DrepRegistration(
        StakeCredential::AddrKeyHash(sample_hash28()),
        500_000_000,
        None,
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_unreg_drep_round_trip() {
    let cert = DCert::DrepUnregistration(StakeCredential::AddrKeyHash(sample_hash28()), 500_000_000);
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_update_drep_with_anchor_round_trip() {
    let cert = DCert::DrepUpdate(
        StakeCredential::ScriptHash(sample_hash28()),
        Some(Anchor {
            url: "https://example.com/drep-update.json".to_string(),
            data_hash: sample_hash32(),
        }),
    );
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

#[test]
fn dcert_update_drep_no_anchor_round_trip() {
    let cert = DCert::DrepUpdate(StakeCredential::AddrKeyHash(sample_hash28()), None);
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

// -- DCert structural checks -----------------------------------------------

#[test]
fn dcert_stake_registration_starts_with_tag_0() {
    let cert = DCert::AccountRegistration(StakeCredential::AddrKeyHash(sample_hash28()));
    let bytes = cert.to_cbor_bytes();
    // array(2) = 0x82, uint(0) = 0x00
    assert_eq!(bytes[0], 0x82);
    assert_eq!(bytes[1], 0x00);
}

#[test]
fn dcert_pool_registration_starts_with_tag_3() {
    let cert = DCert::PoolRegistration(sample_pool_params());
    let bytes = cert.to_cbor_bytes();
    // array(10) = 0x8a, uint(3) = 0x03
    assert_eq!(bytes[0], 0x8a);
    assert_eq!(bytes[1], 0x03);
}

#[test]
fn dcert_reg_cert_conway_starts_with_tag_7() {
    let cert = DCert::AccountRegistrationDeposit(StakeCredential::AddrKeyHash(sample_hash28()), 2_000_000);
    let bytes = cert.to_cbor_bytes();
    // array(3) = 0x83, uint(7) = 0x07
    assert_eq!(bytes[0], 0x83);
    assert_eq!(bytes[1], 0x07);
}

// -- PoolParams with multiple relays and owners ----------------------------

#[test]
fn pool_params_multiple_relays_and_owners_round_trip() {
    let params = PoolParams {
        operator: sample_hash28(),
        vrf_keyhash: sample_hash32(),
        pledge: 1_000_000_000,
        cost: 340_000_000,
        margin: UnitInterval {
            numerator: 3,
            denominator: 100,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash(sample_hash28()),
        },
        pool_owners: vec![sample_hash28(), [0xbb; 28]],
        relays: vec![
            Relay::SingleHostAddr(Some(3001), Some([1, 2, 3, 4]), None),
            Relay::SingleHostName(None, "relay2.example.com".to_string()),
            Relay::MultiHostName("pool.example.com".to_string()),
        ],
        pool_metadata: None,
    };
    let cert = DCert::PoolRegistration(params);
    let bytes = cert.to_cbor_bytes();
    let decoded = DCert::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(cert, decoded);
}

// -- Moved Anchor: verify existing anchor tests still work -----------------

// ---------------------------------------------------------------------------
