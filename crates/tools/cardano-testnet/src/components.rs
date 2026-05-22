//! cardano-testnet component surfaces.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Module-tree parent for the upstream
//! `Testnet/Components/` directory. Each component file ports the
//! era-free / portable surface of its upstream `.hs`; the
//! node-querying and genesis-creation bodies are runtime / era-coupled
//! and land with later rounds.

pub mod query;
