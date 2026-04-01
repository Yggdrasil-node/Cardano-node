use yggdrasil_ledger::{
	Address, AllegraTxBody, AlonzoCompatibleSubmittedTx, AlonzoTxBody, AlonzoTxOut, Anchor,
	BabbageBlock, BabbageTxBody, BabbageTxOut, BaseAddress, Block, BlockHeader, BlockNo,
	BootstrapWitness, ByronBlock, ByronTx, ByronTxAux, ByronTxIn, ByronTxOut, ByronTxWitness,
	CborDecode, CborEncode, CommitteeAuthorization,
	Constitution, ConwayBlock, ConwayTxBody, DCert, DRep, DatumOption, Decoder, Encoder,
	EnterpriseAddress, Era, EpochNo, ExUnits, GovAction, GovActionId, HeaderHash, LedgerError,
	LedgerState, LedgerStateCheckpoint, MaryTxBody, MaryTxOut, MultiEraSubmittedTx, MultiEraTxOut, MultiEraUtxo,
	NativeScript, Nonce, PlutusData, Point, PointerAddress, PoolMetadata, PoolParams,
	ProtocolParameterUpdate, ProtocolParameters,
	PraosHeader, PraosHeaderBody, ProposalProcedure, Redeemer, RegisteredDrep, Relay,
	RewardAccount, RewardAccountState, Script, ScriptRef, ShelleyBlock,
	ShelleyCompatibleSubmittedTx, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyTx,
	ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyUpdate, ShelleyUtxo, ShelleyVkeyWitness,
	ShelleyVrfCert, ShelleyWitnessSet, SlotNo, StakeCredential, Tx, TxId, UnitInterval, Value,
	Vote, Voter, VotingProcedure, VotingProcedures,
	BYRON_SLOTS_PER_EPOCH, compute_tx_id, native_script_hash, vkey_hash,
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
        Self { signing_key, vkey: vk.0, vkey_hash: hash }
    }

    /// Signs a transaction body hash and returns a valid VKey witness.
    fn witness(&self, tx_body_hash: &[u8; 32]) -> ShelleyVkeyWitness {
        let sig = self.signing_key.sign(tx_body_hash).expect("ed25519 sign");
        ShelleyVkeyWitness { vkey: self.vkey, signature: sig.0 }
    }

    /// Enterprise key-hash address (type 6, network 1) for this signer.
    fn enterprise_addr(&self) -> Vec<u8> {
        let mut addr = vec![0x61]; // 0110_0001 = type 6 (enterprise), network 1
        addr.extend_from_slice(&self.vkey_hash);
        addr
    }
}

mod block_ex_units;
mod core_cbor;
mod duplicate_inputs;
mod epoch_boundary_fees;
mod eras_allegra_mary;
mod eras_alonzo;
mod eras_babbage;
mod eras_byron;
mod eras_conway;
mod eras_praos_blocks;
mod golden;
mod governance_updates;
mod is_valid_handling;
mod ledger_state_basic;
mod ledger_state_committee;
mod ledger_state_era_application;
mod ledger_state_pools_rewards_queries;
mod ledger_state_stake_and_drep;
mod mir;
mod invalid_metadata;
mod missing_redeemer;
mod missing_metadata_hash;
mod multi_era_utxo;
mod no_cost_model;
mod extraneous_script_witness;
mod extra_redeemer;
mod supplemental_datums;
mod unspendable_utxo;
mod network_validation;
mod output_validation;
mod plutus_evaluation;
mod plutus_scripts;
mod ppup_validation;
mod ref_scripts_size;
mod reference_input_contention;
mod shelley;
mod script_data_hash;
mod submitted_conway_governance;
mod submitted_extra_redeemer;
mod submitted_plutus_evaluation;
mod submitted_script_witness;
mod treasury_donation;
mod deposit_preservation;
mod conway_cert_deposit_validation;
mod conway_governance_parity;
mod dormant_epoch;
mod txbody_keys;
mod types_and_certs;
mod witness_validation;
