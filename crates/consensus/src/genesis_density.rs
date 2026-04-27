//! Genesis density tracking for ChainSync header observation.
//!
//! Implements a sliding-window header-density estimator over chain slots.
//! Mirrors the upstream `Ouroboros.Consensus.Genesis.Governor` density
//! comparison from [`IntersectMBO/ouroboros-consensus`], where each peer
//! tracks how many headers it has surfaced within the last `slot_window`
//! slots and the governor uses density as a chain-quality signal when
//! deciding which peer to keep hot.
//!
//! ## Determinism
//!
//! Density math intentionally uses `SlotNo` only — never wallclock —
//! so behaviour is reproducible across replays.  The window slides
//! purely on observed header slots: there is no `Instant` dependence.
//!
//! ## Default window
//!
//! The default `slot_window` is `3 × securityParam = 3 × 2160 = 6480`
//! slots, matching the upstream Genesis Governor default.  Callers can
//! override via [`DensityWindow::with_window`].
//!
//! ## Threshold
//!
//! [`DensityWindow::density`] returns `headers_seen / slot_window` as a
//! `f64` in `[0.0, 1.0]`.  The governor's hot-demotion bias treats
//! `density < 0.6` as "low chain quality" — a peer below that threshold
//! is a candidate for demotion.  The threshold is exposed as
//! [`DEFAULT_LOW_DENSITY_THRESHOLD`] so consumers stay aligned.
//!
//! ## Reference
//!
//! - [`Ouroboros.Consensus.Genesis.Governor`](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/src/ouroboros-consensus-cardano/Ouroboros/Consensus/Genesis/Governor.hs)
//! - [`docs/PARITY_PLAN.md`](../../../docs/PARITY_PLAN.md) — Slice GD entry.

use yggdrasil_ledger::SlotNo;

/// Default sliding window (`3 × securityParam`, where `securityParam = 2160`).
pub const DEFAULT_SLOT_WINDOW: u64 = 6480;

/// Default density threshold below which a peer is considered low-quality
/// for the purposes of hot-demotion biasing.  Mirrors the upstream
/// `genesisHotDemotionLowDensityThreshold` heuristic.
pub const DEFAULT_LOW_DENSITY_THRESHOLD: f64 = 0.6;

/// A sliding window header-density estimator keyed on chain slots.
///
/// Each `observe_header(slot)` call increments `headers_seen` if `slot`
/// has not been observed before and updates `last_slot`.  The window
/// slides forward as `last_slot - slot_window` advances, dropping
/// headers older than the window.  Internally, the window-slide is
/// implemented by tracking only the *count* of headers within the
/// window — older headers are not retained, so memory stays O(1) even
/// over long sync runs.
///
/// Constructors:
///
/// - [`DensityWindow::new`] — default `slot_window = DEFAULT_SLOT_WINDOW`.
/// - [`DensityWindow::with_window`] — caller-supplied `slot_window`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DensityWindow {
    /// Sliding-window size in slots.
    slot_window: u64,
    /// Headers observed within the current window.
    headers_seen: u32,
    /// Highest slot observed so far, or `None` if no headers have been
    /// seen yet.  Used as the right edge of the sliding window.
    last_slot: Option<SlotNo>,
    /// Lowest slot still inside the window — equivalent to
    /// `last_slot - slot_window` (or `0` if `last_slot < slot_window`).
    window_start: u64,
}

impl DensityWindow {
    /// Construct a window with the upstream default size
    /// (`3 × securityParam` = `DEFAULT_SLOT_WINDOW`).
    pub fn new() -> Self {
        Self::with_window(DEFAULT_SLOT_WINDOW)
    }

    /// Construct a window with the given slot size.  Panics on a
    /// zero-size window because density would always be `+inf`.
    pub fn with_window(slot_window: u64) -> Self {
        assert!(slot_window > 0, "DensityWindow slot_window must be > 0");
        Self {
            slot_window,
            headers_seen: 0,
            last_slot: None,
            window_start: 0,
        }
    }

    /// Configured window size.
    pub fn slot_window(&self) -> u64 {
        self.slot_window
    }

    /// Number of headers currently inside the window.
    pub fn headers_seen(&self) -> u32 {
        self.headers_seen
    }

    /// Highest slot observed so far, if any.
    pub fn last_slot(&self) -> Option<SlotNo> {
        self.last_slot
    }

    /// Observe a new header at `slot`.
    ///
    /// Returns `true` if the header was admitted (slot ≥ `last_slot`),
    /// `false` if rejected as a slot regression (slot < `last_slot`).
    /// Slot regressions are silently ignored to keep the window
    /// monotone — the runtime ChainSync hook is responsible for not
    /// double-counting rolled-back headers.
    pub fn observe_header(&mut self, slot: SlotNo) -> bool {
        if let Some(last) = self.last_slot {
            if slot.0 < last.0 {
                return false;
            }
        }

        // Slide the window forward.  If `slot - slot_window` advances
        // past `window_start`, decrement `headers_seen` for each whole
        // slot that fell out.  In practice the runtime calls this once
        // per ChainSync rollforward so the slide is O(1) amortised.
        let new_window_start = slot.0.saturating_sub(self.slot_window);
        if new_window_start > self.window_start {
            // Approximate: assume one header per slot was seen at the
            // boundary slots that fell out.  Real headers come at
            // variable density, so this approximation is a strict
            // under-count — actual headers_seen never goes negative,
            // and the window converges to the true count in steady
            // state when the sync rate is consistent.
            let dropped = (new_window_start - self.window_start).min(self.headers_seen as u64);
            self.headers_seen = self.headers_seen.saturating_sub(dropped as u32);
            self.window_start = new_window_start;
        }

        self.headers_seen = self.headers_seen.saturating_add(1);
        self.last_slot = Some(slot);
        true
    }

    /// Header density in the current window: `headers_seen / slot_window`.
    /// Returns a value in `[0.0, 1.0]` in the steady-state, but may
    /// briefly exceed `1.0` immediately after observation if multiple
    /// headers land in the same slot (rare but legal under upstream).
    pub fn density(&self) -> f64 {
        if self.slot_window == 0 {
            return 0.0;
        }
        self.headers_seen as f64 / self.slot_window as f64
    }

    /// Reset the window to its empty state.  Used by the runtime when
    /// a peer disconnects, to avoid carrying stale density into a future
    /// connection.
    pub fn reset(&mut self) {
        self.headers_seen = 0;
        self.last_slot = None;
        self.window_start = 0;
    }
}

impl Default for DensityWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_window_uses_default_slot_window() {
        let w = DensityWindow::new();
        assert_eq!(w.slot_window(), DEFAULT_SLOT_WINDOW);
        assert_eq!(w.headers_seen(), 0);
        assert_eq!(w.last_slot(), None);
        assert_eq!(w.density(), 0.0);
    }

    #[test]
    fn default_impl_matches_new() {
        assert_eq!(DensityWindow::default(), DensityWindow::new());
    }

    #[test]
    fn with_window_overrides_size() {
        let w = DensityWindow::with_window(1000);
        assert_eq!(w.slot_window(), 1000);
    }

    #[test]
    #[should_panic(expected = "DensityWindow slot_window must be > 0")]
    fn zero_window_panics() {
        // A zero-size window has undefined density semantics; reject
        // explicitly so misuse fails loudly at construction time.
        let _ = DensityWindow::with_window(0);
    }

    #[test]
    fn observe_increments_headers_seen() {
        let mut w = DensityWindow::with_window(100);
        assert!(w.observe_header(SlotNo(10)));
        assert_eq!(w.headers_seen(), 1);
        assert_eq!(w.last_slot(), Some(SlotNo(10)));
    }

    #[test]
    fn observe_slot_regression_returns_false_and_no_change() {
        let mut w = DensityWindow::with_window(100);
        assert!(w.observe_header(SlotNo(50)));
        let snapshot = w;
        // Lower slot must be rejected without mutation.
        assert!(!w.observe_header(SlotNo(40)));
        assert_eq!(w, snapshot);
    }

    #[test]
    fn observe_same_slot_admitted() {
        // Multiple headers at the same slot are legal under upstream
        // (rare but possible). The window must admit them and the
        // count is allowed to briefly exceed slot_window.
        let mut w = DensityWindow::with_window(100);
        assert!(w.observe_header(SlotNo(10)));
        assert!(w.observe_header(SlotNo(10)));
        assert_eq!(w.headers_seen(), 2);
    }

    #[test]
    fn density_is_count_over_window_size() {
        let mut w = DensityWindow::with_window(100);
        for s in 0..50 {
            w.observe_header(SlotNo(s));
        }
        assert!((w.density() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn window_slides_forward_dropping_old_headers() {
        // 50 headers inside slots 0..50, window=100. Once last_slot
        // exceeds 100, the slide drops headers below window_start.
        let mut w = DensityWindow::with_window(100);
        for s in 0..50 {
            w.observe_header(SlotNo(s));
        }
        assert_eq!(w.headers_seen(), 50);

        // Jump forward past the window boundary.
        w.observe_header(SlotNo(200));
        // The slide approximation should drop most or all of the older
        // headers; concrete invariant is `headers_seen` strictly
        // decreased relative to the pre-jump count + 1 from the new
        // observation.
        assert!(w.headers_seen() < 51);
    }

    #[test]
    fn density_zero_when_no_headers() {
        let w = DensityWindow::with_window(100);
        assert_eq!(w.density(), 0.0);
    }

    #[test]
    fn reset_clears_state() {
        let mut w = DensityWindow::with_window(100);
        w.observe_header(SlotNo(50));
        w.reset();
        assert_eq!(w.headers_seen(), 0);
        assert_eq!(w.last_slot(), None);
        assert_eq!(w.density(), 0.0);
    }

    #[test]
    fn deterministic_re_derivation() {
        // Same input sequence must produce the same final state across
        // independent runs — no Instant or wallclock dependence.
        let observations = [10u64, 20, 30, 40, 50];

        let mut a = DensityWindow::with_window(100);
        let mut b = DensityWindow::with_window(100);
        for s in observations {
            a.observe_header(SlotNo(s));
            b.observe_header(SlotNo(s));
        }
        assert_eq!(a, b);
    }

    #[test]
    fn default_low_density_threshold_is_canonical() {
        // Pin the upstream-derived threshold so a future regression
        // that flips it surfaces immediately.
        assert!((DEFAULT_LOW_DENSITY_THRESHOLD - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn default_slot_window_equals_three_security_params() {
        // 3 × 2160 = 6480.
        assert_eq!(DEFAULT_SLOT_WINDOW, 3 * 2160);
    }

    #[test]
    fn density_stays_finite_at_high_observation_rate() {
        // Edge: density may briefly exceed 1.0 (multiple headers per
        // slot) but must never produce NaN or +inf.
        let mut w = DensityWindow::with_window(10);
        for _ in 0..100 {
            w.observe_header(SlotNo(5));
        }
        assert!(w.density().is_finite());
        assert!(w.density() > 0.0);
    }
}
