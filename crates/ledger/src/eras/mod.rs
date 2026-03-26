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
pub use alonzo::{AlonzoBlock, AlonzoTxBody, AlonzoTxOut, ExUnits, Redeemer};
pub use babbage::BABBAGE_NAME;
pub use babbage::{BabbageBlock, BabbageTxBody, BabbageTxOut, DatumOption};
pub use byron::BYRON_NAME;
pub use byron::{ByronBlock, ByronTx, ByronTxAux, ByronTxIn, ByronTxOut, ByronTxWitness, BYRON_SLOTS_PER_EPOCH};
pub use conway::CONWAY_NAME;
pub use conway::{
    Constitution, ConwayBlock, ConwayTxBody, GovAction, GovActionId, ProposalProcedure, Vote,
    Voter, VotingProcedure, VotingProcedures,
};
pub use mary::MARY_NAME;
pub use mary::{
    AssetName, MaryTxBody, MaryTxOut, MintAsset, MultiAsset, PolicyId, Value,
};
pub use shelley::SHELLEY_NAME;
pub use shelley::{
    BootstrapWitness, PraosHeader, PraosHeaderBody, ShelleyBlock, ShelleyHeader,
    ShelleyHeaderBody, ShelleyOpCert, ShelleyTx, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut,
    ShelleyUpdate, ShelleyUtxo, ShelleyVkeyWitness, ShelleyVrfCert, ShelleyWitnessSet,
    compute_block_body_hash,
};

/// Supported Cardano eras in canonical order from Byron through Conway.
///
/// The discriminant ordering (0 = Byron … 6 = Conway) is part of the public
/// API and is relied upon by `era_ordinal()` comparisons and the hard-fork
/// era-regression guard in `LedgerState::apply_block_validated`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub enum Era {
    Byron,
    Shelley,
    Allegra,
    Mary,
    Alonzo,
    Babbage,
    Conway,
}

impl Era {
    /// Return the canonical numeric ordinal of this era (Byron = 0, …, Conway = 6).
    ///
    /// The ordinal is used for hard-fork transition checks: the era of an incoming
    /// block must be **≥** the current ledger era.  Blocks from an older era are
    /// rejected as era regressions (hard-fork combinator invariant).
    ///
    /// Reference: `Ouroboros.Consensus.HardFork.Combinator` — era numbering.
    pub fn era_ordinal(self) -> u8 {
        match self {
            Self::Byron   => 0,
            Self::Shelley => 1,
            Self::Allegra => 2,
            Self::Mary    => 3,
            Self::Alonzo  => 4,
            Self::Babbage => 5,
            Self::Conway  => 6,
        }
    }

    /// Returns `true` if `other` is strictly later in the era sequence than `self`.
    ///
    /// Used by the hard-fork guard to detect a legitimate era transition
    /// (as opposed to normal same-era block sequencing).
    pub fn is_hard_fork_to(self, other: Era) -> bool {
        other.era_ordinal() > self.era_ordinal()
    }

    /// Returns `true` if `other` represents an era regression relative to `self`.
    ///
    /// An era regression occurs when the incoming block's era is earlier in the
    /// sequence than the current ledger era, which violates the hard-fork combinator
    /// invariant that the chain is append-only across era boundaries.
    pub fn is_era_regression(self, other: Era) -> bool {
        other.era_ordinal() < self.era_ordinal()
    }
}
