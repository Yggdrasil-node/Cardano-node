//! Fund-specialized FIFO queue.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/FundQueue.hs`.
//! Ports the thin `FundQueue` wrappers over `Internal.Fifo`.

use crate::tx_generator::fund::Fund;
use crate::tx_generator::internal::fifo::Fifo;

/// Mirror of upstream `type FundQueue = Fifo Fund`.
pub type FundQueue = Fifo<Fund>;

/// Mirror of upstream `emptyFundQueue`.
pub fn empty_fund_queue() -> FundQueue {
    Fifo::empty_fifo()
}

/// Mirror of upstream `toList`.
pub fn to_list(queue: &FundQueue) -> Vec<Fund> {
    queue.to_list()
}

/// Mirror of upstream `insertFund`.
pub fn insert_fund(queue: FundQueue, fund: Fund) -> FundQueue {
    queue.insert(fund)
}

/// Mirror of upstream `removeFund`.
pub fn remove_fund(queue: FundQueue) -> Option<(FundQueue, Fund)> {
    queue.remove()
}

/// Mirror of upstream `removeFunds`.
pub fn remove_funds(count: usize, queue: FundQueue) -> Option<(FundQueue, Vec<Fund>)> {
    queue.remove_n(count)
}

/// Mirror of upstream `removeAllFunds`.
pub fn remove_all_funds(queue: FundQueue) -> (FundQueue, Vec<Fund>) {
    (empty_fund_queue(), to_list(&queue))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AnyCardanoEra;

    fn fund(tx_in: &str) -> Fund {
        Fund::key_fund(AnyCardanoEra::Conway, tx_in, 1, "key")
    }

    #[test]
    fn fund_queue_wrappers_preserve_fifo_order() {
        let queue = insert_fund(insert_fund(empty_fund_queue(), fund("a#0")), fund("b#0"));
        let (queue, first) = remove_fund(queue).expect("first");
        let (_queue, second) = remove_fund(queue).expect("second");

        assert_eq!(first.tx_in, "a#0");
        assert_eq!(second.tx_in, "b#0");
    }

    #[test]
    fn remove_all_funds_empties_and_lists() {
        let queue = insert_fund(insert_fund(empty_fund_queue(), fund("a#0")), fund("b#0"));
        let (queue, funds) = remove_all_funds(queue);

        assert_eq!(
            funds
                .iter()
                .map(|fund| fund.tx_in.as_str())
                .collect::<Vec<_>>(),
            vec!["a#0", "b#0"]
        );
        assert!(remove_fund(queue).is_none());
    }
}
