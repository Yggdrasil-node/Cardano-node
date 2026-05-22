//! cardano-testnet on-chain query helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side port of the portable
//! surface of upstream `cardano-testnet/src/Testnet/Components/Query.hs`
//! (the basename `query.rs` mirrors `Query.hs`, but the file is placed
//! under `components/` and ports only the era-free types). The bulk of
//! `Query.hs` is node-querying logic — `getEpochState`, `getGovState`,
//! `findAllUtxos`, the `wait*` loops — which runs against a live node
//! and lands with the testnet-harness rounds.

/// A period to wait for during a testnet run.
///
/// Mirror of upstream `data TestnetWaitPeriod` (`Components/Query.hs`).
/// `WaitForEpochs` carries an epoch count — upstream's `Cardano.Api`
/// `EpochInterval` is a `Word32` newtype.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestnetWaitPeriod {
    /// Wait for a number of epochs.
    WaitForEpochs(u32),
    /// Wait for a number of blocks.
    WaitForBlocks(u64),
    /// Wait for a number of slots.
    WaitForSlots(u64),
}

impl std::fmt::Display for TestnetWaitPeriod {
    /// Mirror of upstream `instance Show TestnetWaitPeriod` —
    /// `WaitForEpochs <n>` / `WaitForBlocks <n>` / `WaitForSlots <n>`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestnetWaitPeriod::WaitForEpochs(n) => write!(f, "WaitForEpochs {n}"),
            TestnetWaitPeriod::WaitForBlocks(n) => write!(f, "WaitForBlocks {n}"),
            TestnetWaitPeriod::WaitForSlots(n) => write!(f, "WaitForSlots {n}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_period_display_matches_upstream_show() {
        assert_eq!(
            TestnetWaitPeriod::WaitForEpochs(5).to_string(),
            "WaitForEpochs 5"
        );
        assert_eq!(
            TestnetWaitPeriod::WaitForBlocks(12).to_string(),
            "WaitForBlocks 12"
        );
        assert_eq!(
            TestnetWaitPeriod::WaitForSlots(900).to_string(),
            "WaitForSlots 900"
        );
    }

    #[test]
    fn wait_period_variants_are_distinct() {
        assert_ne!(
            TestnetWaitPeriod::WaitForBlocks(1),
            TestnetWaitPeriod::WaitForSlots(1)
        );
    }
}
