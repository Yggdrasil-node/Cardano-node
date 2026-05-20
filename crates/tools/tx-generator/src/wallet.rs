//! Wallet queue operations used by the benchmark script runtime.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Wallet.hs`.
//! Ports the `WalletRef`/fund-source operations over the upstream
//! `FundQueue` abstraction. Rust keeps the state single-threaded in
//! `Script.Env` instead of wrapping it in `MVar`.

use crate::tx_generator::fund::Fund;
use crate::tx_generator::fund_queue::{
    FundQueue, empty_fund_queue, insert_fund, remove_all_funds, remove_funds, to_list,
};

/// Rust analogue of upstream `WalletRef = MVar FundQueue`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletRef {
    queue: FundQueue,
}

impl Default for WalletRef {
    fn default() -> Self {
        init_wallet()
    }
}

impl WalletRef {
    /// Insert a new fund into the wallet queue.
    pub fn insert_fund(&mut self, fund: Fund) {
        wallet_ref_insert_fund(self, fund);
    }

    /// Return the queued funds in upstream `toList` order.
    pub fn funds(&self) -> Vec<Fund> {
        ask_wallet_ref(self, to_list)
    }
}

/// Mirror of upstream `initWallet`.
pub fn init_wallet() -> WalletRef {
    WalletRef {
        queue: empty_fund_queue(),
    }
}

/// Mirror of upstream `askWalletRef`.
pub fn ask_wallet_ref<T>(wallet_ref: &WalletRef, f: impl FnOnce(&FundQueue) -> T) -> T {
    f(&wallet_ref.queue)
}

/// Mirror of upstream `walletRefInsertFund`.
pub fn wallet_ref_insert_fund(wallet_ref: &mut WalletRef, fund: Fund) {
    let queue = std::mem::take(&mut wallet_ref.queue);
    wallet_ref.queue = insert_fund(queue, fund);
}

/// Mirror of upstream `mkWalletFundStoreList`.
pub fn mk_wallet_fund_store_list(wallet_ref: &mut WalletRef, funds: Vec<Fund>) {
    for fund in funds {
        wallet_ref_insert_fund(wallet_ref, fund);
    }
}

/// Mirror of upstream `mkWalletFundStore`.
pub fn mk_wallet_fund_store(wallet_ref: &mut WalletRef, fund: Fund) {
    wallet_ref_insert_fund(wallet_ref, fund);
}

/// Mirror of upstream `walletSource`.
pub fn wallet_source(wallet_ref: &mut WalletRef, munch: usize) -> Result<Vec<Fund>, String> {
    let queue = std::mem::take(&mut wallet_ref.queue);
    match remove_funds(munch, queue.clone()) {
        Some((new_queue, funds)) => {
            wallet_ref.queue = new_queue;
            Ok(funds)
        }
        None => {
            wallet_ref.queue = queue;
            Err("WalletSource: out of funds".to_string())
        }
    }
}

/// Mirror of upstream `walletPreview`.
pub fn wallet_preview(wallet_ref: &WalletRef, munch: usize) -> Vec<Fund> {
    remove_funds(munch, wallet_ref.queue.clone())
        .map(|(_queue, funds)| funds)
        .unwrap_or_else(|| remove_all_funds(wallet_ref.queue.clone()).1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AnyCardanoEra;

    fn fund(tx_in: &str) -> Fund {
        Fund::key_fund(AnyCardanoEra::Conway, tx_in, 1, "key")
    }

    #[test]
    fn wallet_ref_insert_fund_preserves_fifo_source_order() {
        let mut wallet = init_wallet();
        wallet_ref_insert_fund(&mut wallet, fund("a#0"));
        wallet_ref_insert_fund(&mut wallet, fund("b#0"));

        let funds = wallet_source(&mut wallet, 2).expect("funds");

        assert_eq!(
            funds
                .iter()
                .map(|fund| fund.tx_in.as_str())
                .collect::<Vec<_>>(),
            vec!["a#0", "b#0"]
        );
        assert_eq!(
            wallet_source(&mut wallet, 1),
            Err("WalletSource: out of funds".to_string())
        );
    }

    #[test]
    fn wallet_preview_does_not_consume_and_returns_all_when_short() {
        let mut wallet = init_wallet();
        wallet_ref_insert_fund(&mut wallet, fund("a#0"));

        assert_eq!(wallet_preview(&wallet, 2)[0].tx_in, "a#0");
        assert_eq!(wallet_source(&mut wallet, 1).expect("fund")[0].tx_in, "a#0");
    }
}
