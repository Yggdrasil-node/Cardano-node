use yggdrasil_mempool::{Mempool, MempoolEntry};

#[test]
fn mempool_prioritizes_higher_fees() {
    let mut mempool = Mempool::default();
    mempool.insert(MempoolEntry {
        tx_id: String::from("low"),
        fee: 1,
    });
    mempool.insert(MempoolEntry {
        tx_id: String::from("high"),
        fee: 10,
    });

    assert_eq!(
        mempool
            .pop_best()
            .expect("mempool should return the highest fee entry")
            .tx_id,
        "high"
    );
}
