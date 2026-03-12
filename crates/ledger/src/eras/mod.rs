mod allegra;
mod alonzo;
mod babbage;
mod byron;
mod conway;
mod mary;
pub mod shelley;

pub use allegra::ALLEGRA_NAME;
pub use alonzo::ALONZO_NAME;
pub use babbage::BABBAGE_NAME;
pub use byron::BYRON_NAME;
pub use conway::CONWAY_NAME;
pub use mary::MARY_NAME;
pub use shelley::SHELLEY_NAME;
pub use shelley::{
    ShelleyTx, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyVkeyWitness, ShelleyWitnessSet,
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
