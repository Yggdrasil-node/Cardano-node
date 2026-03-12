use eyre::Result;
use yggdrasil_consensus::{ChainCandidate, select_preferred};
use yggdrasil_ledger::Era;
use yggdrasil_mempool::Mempool;
use yggdrasil_network::HandshakeVersion;
use yggdrasil_storage::ImmutableBlockStore;

/// Boots the current Yggdrasil foundation binary and reports a minimal runtime banner.
fn main() -> Result<()> {
    let preferred = select_preferred(
        ChainCandidate {
            block_no: 0,
            slot_no: 0,
        },
        ChainCandidate {
            block_no: 1,
            slot_no: 1,
        },
    );

    let _mempool = Mempool::default();
    let _storage = ImmutableBlockStore::default();

    println!(
        "Yggdrasil foundation ready: era roadmap starts at {:?}, preferred tip {}, handshake v{}",
        Era::Byron,
        preferred.block_no,
        HandshakeVersion(12).0,
    );

    Ok(())
}
