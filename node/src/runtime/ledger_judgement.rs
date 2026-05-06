//! Genesis-derived ledger-judgement settings for live `LedgerStateJudgement`
//! computation.
//!
//! Mirrors upstream `Cardano.Node.Diffusion.Configuration.mkLedgerStateJudgement`:
//! the judgement flips from `YoungEnough` to `TooOld` when
//! `now - tipSlotTime > max_ledger_state_age_secs`. The three settings
//! (system start, slot length, max age) are bundled into a single
//! `LedgerJudgementSettings` struct so the call sites for
//! `refresh_ledger_peer_sources_from_chain_db` stay cohesive.
//!
//! Defaults to the conservative legacy fallback (`system_start = None`,
//! `slot_length = None`, `max_age = 129_600 s` — mainnet `3 * k/f *
//! slotLength` with k=2160, f=0.05) so test paths that don't configure
//! genesis still resolve to `YoungEnough`.
//!
//! Extracted from `runtime.rs` in R271c.

/// Genesis-derived inputs that drive the live `LedgerStateJudgement`
/// computation in `ChainDbConsensusLedgerSource`. Bundled into a single
/// struct so the three values stay cohesive across the
/// `refresh_ledger_peer_sources_from_chain_db` call sites; defaults to
/// the legacy `YoungEnough` fallback when both timing inputs are `None`.
#[derive(Clone, Copy, Debug)]
pub struct LedgerJudgementSettings {
    /// Seconds since the Unix epoch of `ShelleyGenesis.system_start`.
    pub system_start_unix_secs: Option<f64>,
    /// Slot duration in seconds from `ShelleyGenesis.slot_length`.
    pub slot_length_secs: Option<f64>,
    /// Maximum tolerated tip age in seconds before the judgement flips
    /// to `TooOld`. Upstream uses `stabilityWindow * slotLength`.
    pub max_ledger_state_age_secs: f64,
}

impl Default for LedgerJudgementSettings {
    fn default() -> Self {
        Self {
            system_start_unix_secs: None,
            slot_length_secs: None,
            // Conservative default ≈ mainnet `3 * k / f * slotLength`
            // with k=2160, f=0.05, slotLength=1.0 → 129_600 s. The
            // node-side production wiring overrides this from genesis.
            max_ledger_state_age_secs: 129_600.0,
        }
    }
}
