//! cardano-testnet configuration-creation constants.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side port of the era-free
//! surface of upstream
//! `cardano-testnet/src/Testnet/Components/Configuration.hs`. The
//! bulk of `Configuration.hs` — `createConfigJson`,
//! `createSPOGenesisAndFiles`, the genesis-hash helpers, the
//! `eraToString` converters — is era / IO-coupled and lands once the
//! yggdrasil-ledger era surface is exposed at crate boundaries.

/// Seconds added to "now" when computing a fresh testnet's genesis
/// start time.
///
/// Mirror of upstream
/// `startTimeOffsetSeconds = if OS.isWin32 then 90 else 15` — CLI
/// commands are markedly slower on Windows, so testnet setup is given
/// more headroom there.
pub const START_TIME_OFFSET_SECONDS: i32 = if cfg!(windows) { 90 } else { 15 };

/// The number of UTxO keys a freshly-created testnet seeds.
///
/// Mirror of upstream `numSeededUTxOKeys = 3`.
pub const NUM_SEEDED_UTXO_KEYS: i32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_time_offset_matches_upstream_for_this_platform() {
        let expected = if cfg!(windows) { 90 } else { 15 };
        assert_eq!(START_TIME_OFFSET_SECONDS, expected);
    }

    #[test]
    fn num_seeded_utxo_keys_matches_upstream() {
        assert_eq!(NUM_SEEDED_UTXO_KEYS, 3);
    }
}
