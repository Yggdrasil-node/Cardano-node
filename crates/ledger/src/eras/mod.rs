pub mod allegra;
pub mod alonzo;
pub mod babbage;
pub mod byron;
pub mod conway;
pub mod mary;
pub mod shelley;

pub use allegra::ALLEGRA_NAME;
pub use allegra::{AllegraTxBody, NativeScript};
pub use alonzo::ALONZO_NAME;
pub use alonzo::{AlonzoTxBody, AlonzoTxOut, ExUnits, Redeemer};
pub use babbage::BABBAGE_NAME;
pub use babbage::{BabbageTxBody, BabbageTxOut, DatumOption};
pub use byron::BYRON_NAME;
pub use byron::{ByronBlock, BYRON_SLOTS_PER_EPOCH};
pub use conway::CONWAY_NAME;
pub use conway::{
    Anchor, ConwayTxBody, GovActionId, ProposalProcedure, Vote, Voter, VotingProcedure,
    VotingProcedures,
};
pub use mary::MARY_NAME;
pub use mary::{
    AssetName, MaryTxBody, MaryTxOut, MintAsset, MultiAsset, PolicyId, Value,
};
pub use shelley::SHELLEY_NAME;
pub use shelley::{
    ShelleyBlock, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyTx, ShelleyTxBody,
    ShelleyTxIn, ShelleyTxOut, ShelleyUtxo, ShelleyVkeyWitness, ShelleyVrfCert, ShelleyWitnessSet,
};

/// Supported Cardano eras in canonical order from Byron through Conway.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Era {
    Byron,
    Shelley,
    Allegra,
    Mary,
    Alonzo,
    Babbage,
    Conway,
}
