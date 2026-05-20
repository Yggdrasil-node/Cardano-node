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
use crate::tx_generator::utxo::{ToUtxo, ToUtxoList, TxIx, make_to_utxo_list};
use crate::types::{Lovelace, PayWithChange};
use yggdrasil_ledger::MultiEraTxOut;

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

/// Mirror of upstream `createAndStore`.
pub fn create_and_store(
    create: &ToUtxo,
    lovelace: Lovelace,
    tx_ix: TxIx,
    tx_id_hex: &str,
) -> Result<(MultiEraTxOut, Fund), String> {
    let (utxo, pending) = create.build(lovelace)?;
    Ok((utxo, pending.fund_for_tx_id(tx_ix, tx_id_hex)))
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

/// Mirror of upstream `mangleWithChange`.
pub fn mangle_with_change(
    change: &ToUtxo,
    payment: &ToUtxo,
    outs: PayWithChange,
) -> Result<ToUtxoList, String> {
    match outs {
        PayWithChange::PayExact(values) => mangle_repeat(payment, &values),
        PayWithChange::PayWithChange(change_value, payments) => {
            let mut values = Vec::with_capacity(payments.len() + 1);
            values.push(change_value);
            values.extend(payments);
            let mut builders = Vec::with_capacity(values.len());
            builders.push(change.clone());
            builders.extend(std::iter::repeat_n(payment.clone(), values.len() - 1));
            mangle(&builders, &values)
        }
    }
}

/// Mirror of upstream `mangle`.
pub fn mangle(builders: &[ToUtxo], values: &[Lovelace]) -> Result<ToUtxoList, String> {
    make_to_utxo_list(builders, values)
}

/// Convenience for upstream `mangle $ repeat toUTxO` call sites.
pub fn mangle_repeat(builder: &ToUtxo, values: &[Lovelace]) -> Result<ToUtxoList, String> {
    let builders = std::iter::repeat_n(builder.clone(), values.len()).collect::<Vec<_>>();
    mangle(&builders, values)
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
    use crate::script::types::NetworkId;
    use crate::tx_generator::utxo::mk_utxo_variant;
    use crate::types::AnyCardanoEra;

    const TX_ID: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn fund(tx_in: &str) -> Fund {
        Fund::key_fund(AnyCardanoEra::Conway, tx_in, 1, "key")
    }

    fn signing_key(byte: u8) -> crate::script::types::SigningKeyEnvelope {
        crate::script::types::SigningKeyEnvelope::payment_signing_key_shelley(format!(
            "5820{}",
            hex::encode([byte; 32])
        ))
    }

    fn builder(key_name: &str, byte: u8) -> ToUtxo {
        mk_utxo_variant(
            AnyCardanoEra::Conway,
            NetworkId::Testnet(42),
            key_name,
            signing_key(byte),
        )
        .expect("builder")
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

    #[test]
    fn create_and_store_builds_output_and_fund_for_tx_id() {
        let to_utxo = builder("pay", 7);
        let (output, fund) = create_and_store(&to_utxo, 123, 2, TX_ID).expect("created");

        assert_eq!(output.coin(), 123);
        assert_eq!(fund.tx_in, format!("{TX_ID}#2"));
        assert_eq!(fund.lovelace, 123);
        assert_eq!(fund.key_name, "pay");
    }

    #[test]
    fn mangle_with_change_places_change_first() {
        let change = builder("change", 1);
        let payment = builder("payment", 2);
        let list = mangle_with_change(
            &change,
            &payment,
            PayWithChange::PayWithChange(10, vec![20, 30]),
        )
        .expect("list");
        let funds = list.funds_for_tx_id(TX_ID);

        assert_eq!(
            funds
                .into_iter()
                .map(|fund| (fund.key_name, fund.lovelace, fund.tx_in))
                .collect::<Vec<_>>(),
            vec![
                ("change".to_string(), 10, format!("{TX_ID}#0")),
                ("payment".to_string(), 20, format!("{TX_ID}#1")),
                ("payment".to_string(), 30, format!("{TX_ID}#2")),
            ]
        );
    }

    #[test]
    fn mangle_repeat_matches_repeat_to_utxo_call_shape() {
        let payment = builder("payment", 2);
        let list = mangle_repeat(&payment, &[20, 30]).expect("list");

        assert_eq!(
            list.funds_for_tx_id(TX_ID)
                .into_iter()
                .map(|fund| (fund.key_name, fund.lovelace))
                .collect::<Vec<_>>(),
            vec![("payment".to_string(), 20), ("payment".to_string(), 30)]
        );
    }
}
