use yggdrasil_ledger::{
    Address, AllegraTxBody, AlonzoCompatibleSubmittedTx, AlonzoTxBody, AlonzoTxOut, Anchor,
    BYRON_SLOTS_PER_EPOCH, BabbageBlock, BabbageTxBody, BabbageTxOut, BaseAddress, Block,
    BlockHeader, BlockNo, BootstrapWitness, ByronBlock, ByronTx, ByronTxAux, ByronTxIn, ByronTxOut,
    ByronTxWitness, CborDecode, CborEncode, CommitteeAuthorization, Constitution, ConwayBlock,
    ConwayTxBody, DCert, DRep, DatumOption, Decoder, Encoder, EnterpriseAddress, EpochNo, Era,
    ExUnits, GovAction, GovActionId, HeaderHash, LedgerError, LedgerState, LedgerStateCheckpoint,
    MaryTxBody, MaryTxOut, MultiEraSubmittedTx, MultiEraTxOut, MultiEraUtxo, NativeScript, Nonce,
    PlutusData, Point, PointerAddress, PoolMetadata, PoolParams, PraosHeader, PraosHeaderBody,
    ProposalProcedure, ProtocolParameterUpdate, ProtocolParameters, Redeemer, RegisteredDrep,
    Relay, RewardAccount, RewardAccountState, Script, ScriptRef, ShelleyBlock,
    ShelleyCompatibleSubmittedTx, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyTx,
    ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyUpdate, ShelleyUtxo, ShelleyVkeyWitness,
    ShelleyVrfCert, ShelleyWitnessSet, SlotNo, StakeCredential, Tx, TxId, UnitInterval, Value,
    Vote, Voter, VotingProcedure, VotingProcedures, compute_tx_id, native_script_hash, vkey_hash,
};

/// Deterministic Ed25519 test key for submitted-tx tests that require valid
/// VKey witnesses. Derives a key pair from a fixed 32-byte seed, provides
/// the corresponding VKey hash (for addresses) and a signing function that
/// produces real Ed25519 signatures that pass `verify_vkey_signatures`.
struct TestSigner {
    signing_key: yggdrasil_crypto::ed25519::SigningKey,
    pub vkey: [u8; 32],
    pub vkey_hash: [u8; 28],
}

impl TestSigner {
    /// Creates a signer from a deterministic seed.
    fn new(seed: [u8; 32]) -> Self {
        let signing_key = yggdrasil_crypto::ed25519::SigningKey::from_bytes(seed);
        let vk = signing_key.verification_key().expect("ed25519 vkey");
        let hash = yggdrasil_ledger::vkey_hash(&vk.0);
        Self {
            signing_key,
            vkey: vk.0,
            vkey_hash: hash,
        }
    }

    /// Signs a transaction body hash and returns a valid VKey witness.
    fn witness(&self, tx_body_hash: &[u8; 32]) -> ShelleyVkeyWitness {
        let sig = self.signing_key.sign(tx_body_hash).expect("ed25519 sign");
        ShelleyVkeyWitness {
            vkey: self.vkey,
            signature: sig.0,
        }
    }

    /// Enterprise key-hash address (type 6, network 1) for this signer.
    fn enterprise_addr(&self) -> Vec<u8> {
        let mut addr = vec![0x61]; // 0110_0001 = type 6 (enterprise), network 1
        addr.extend_from_slice(&self.vkey_hash);
        addr
    }
}

/// Compute a valid `script_data_hash` from a witness set and optional protocol params.
///
/// Convenience helper for tests that include redeemers and need to pass the
/// bidirectional `ppViewHashesDontMatch` check.
fn compute_test_script_data_hash(
    ws: &ShelleyWitnessSet,
    params: Option<&ProtocolParameters>,
    conway_redeemer_format: bool,
) -> [u8; 32] {
    let ws_bytes = ws.to_cbor_bytes();
    yggdrasil_ledger::plutus_validation::compute_script_data_hash(
        Some(&ws_bytes),
        params,
        conway_redeemer_format,
        None,
        None,
        None,
        None,
    )
    .expect("compute test script_data_hash")
}

mod block_body_size;
mod block_ex_units;
mod conway_cert_deposit_validation;
mod conway_governance_parity;
mod core_cbor;
mod deposit_preservation;
mod dormant_epoch;
mod duplicate_inputs;
mod epoch_boundary_fees;
mod eras_allegra_mary;
mod eras_alonzo;
mod eras_babbage;
mod eras_byron;
mod eras_conway;
mod eras_praos_blocks;
mod extra_redeemer;
mod extraneous_script_witness;
mod golden;
mod governance_updates;
mod hardfork_drep_cleanup;
mod invalid_metadata;
mod is_valid_handling;
mod ledger_state_basic;
mod ledger_state_committee;
mod ledger_state_era_application;
mod ledger_state_pools_rewards_queries;
mod ledger_state_stake_and_drep;
mod mir;
mod missing_metadata_hash;
mod missing_redeemer;
mod multi_era_utxo;
mod network_validation;
mod no_cost_model;
mod output_validation;
mod plutus_evaluation;
mod plutus_scripts;
mod ppup_validation;
mod ref_scripts_size;
mod reference_input_contention;
mod script_data_hash;
mod shelley;
mod submitted_conway_governance;
mod submitted_extra_redeemer;
mod submitted_plutus_evaluation;
mod submitted_script_witness;
mod supplemental_datums;
mod treasury_donation;
mod txbody_keys;
mod types_and_certs;
mod unspendable_utxo;
mod witness_validation;
