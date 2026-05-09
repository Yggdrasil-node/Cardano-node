//! Type error sub-tree.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell over the
//! `type/error/*` sub-modules. Upstream has no `Cardano/CLI/Type/Error.hs`
//! top-level file; the surface lives under
//! `Cardano/CLI/Type/Error/*.hs`.

pub mod address_cmd_error;
pub mod address_info_error;
pub mod bootstrap_witness_error;
pub mod cardano_address_signing_key_conversion_error;
pub mod debug_cmd_error;
pub mod delegation_error;
pub mod genesis_cmd_error;
pub mod governance_actions_error;
pub mod governance_cmd_error;
pub mod governance_query_error;
pub mod hash_cmd_error;
pub mod itn_key_conversion_error;
pub mod key_cmd_error;
pub mod node_cmd_error;
pub mod node_era_mismatch_error;
pub mod plutus_script_decode_error;
pub mod protocol_params_error;
pub mod query_cmd_error;
pub mod registration_error;
pub mod script_data_error;
pub mod script_decode_error;
pub mod stake_address_delegation_error;
pub mod stake_address_registration_error;
pub mod stake_credential_error;
pub mod stake_pool_cmd_error;
pub mod tx_cmd_error;
pub mod tx_validation_error;
