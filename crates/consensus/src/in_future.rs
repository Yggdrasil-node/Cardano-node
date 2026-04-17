//! Blocks-from-the-future detection.
//!
//! During sync, the node must reject blocks whose slot is ahead of the
//! current wall-clock slot by more than a small tolerance (`ClockSkew`).
//! Blocks that are only slightly ahead are tolerated (near-future) and
//! processed normally, while blocks that are far ahead indicate a
//! misbehaving peer and should trigger disconnection.
//!
//! Reference: `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck`
//! and the design note `handling_blocks_from_the_future.md` in
//! `ouroboros-consensus`.

use std::time::Duration;

use yggdrasil_ledger::SlotNo;

/// Maximum tolerable clock skew between two honest nodes.
///
/// If a received block's slot is at most `ClockSkew` slots ahead of the
/// local wall-clock slot, it is considered near-future and processed
/// normally—this accounts for NTP drift between peers.
///
/// If the block is further ahead, it is considered far-future and the
/// peer should be disconnected.
///
/// Reference: `Ouroboros.Consensus.Block.Forging.ClockSkew`
/// — `defaultClockSkew = clockSkewInSeconds 2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockSkew {
    /// Maximum number of tolerable excess slots.
    pub max_slots: u64,
}

impl ClockSkew {
    /// Construct a `ClockSkew` from a time-based tolerance and a slot
    /// length.
    ///
    /// The tolerance is converted to a slot count by dividing by
    /// `slot_length`.  If `slot_length` is zero, a single-slot tolerance
    /// is used as a safe fallback.
    ///
    /// Example: with a 2-second tolerance and 1-second slots, the result
    /// is a 2-slot skew.  The upstream Haskell default is 2 seconds.
    #[must_use]
    pub fn from_duration(tolerance: Duration, slot_length: Duration) -> Self {
        let secs = if slot_length.is_zero() {
            1.0
        } else {
            slot_length.as_secs_f64()
        };
        let max_slots = (tolerance.as_secs_f64() / secs).ceil() as u64;
        Self {
            max_slots: max_slots.max(1),
        }
    }

    /// The upstream default: 2 seconds expressed in slots.
    #[must_use]
    pub fn default_for_slot_length(slot_length: Duration) -> Self {
        Self::from_duration(Duration::from_secs(2), slot_length)
    }
}

/// Outcome of comparing a block's slot against the current wall-clock slot.
///
/// Reference: the three branches in
/// `InFutureCheck.handleHeaderArrival`:
/// - not from the future → `NotFuture`
/// - near-future (≤ clockSkew) → `NearFuture`
/// - far-future (> clockSkew) → `FarFuture`
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FutureSlotJudgement {
    /// The block's slot is at or before the current wall-clock slot.
    NotFuture,
    /// The block's slot is ahead of the current wall-clock slot but
    /// within the `ClockSkew` tolerance.  Processing should continue
    /// normally (upstream would delay briefly).
    NearFuture {
        /// How many slots ahead the block is.
        excess_slots: u64,
    },
    /// The block's slot is ahead of the current wall-clock slot by more
    /// than the `ClockSkew` tolerance.  The peer should be disconnected.
    FarFuture {
        /// How many slots ahead the block is.
        excess_slots: u64,
    },
}

/// Judge whether a block's slot is from the future relative to the
/// current wall-clock slot and the configured clock skew tolerance.
///
/// This is a simplified version of the upstream three-phase
/// `recordHeaderArrival` → `judgeHeaderArrival` → `handleHeaderArrival`
/// pipeline.  The full upstream version converts slots to wall-clock
/// time via `slotToWallclock` and the hard-fork summary; we use a
/// slot-based comparison which is correct within a single era.
///
/// Reference: `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck`
/// `realHeaderInFutureCheck` (~line 131–171).
pub fn judge_header_slot(
    header_slot: SlotNo,
    current_wall_slot: SlotNo,
    clock_skew: ClockSkew,
) -> FutureSlotJudgement {
    if header_slot.0 <= current_wall_slot.0 {
        return FutureSlotJudgement::NotFuture;
    }

    let excess = header_slot.0 - current_wall_slot.0;
    if excess <= clock_skew.max_slots {
        FutureSlotJudgement::NearFuture {
            excess_slots: excess,
        }
    } else {
        FutureSlotJudgement::FarFuture {
            excess_slots: excess,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKEW_2: ClockSkew = ClockSkew { max_slots: 2 };

    #[test]
    fn not_future_when_equal() {
        assert_eq!(
            judge_header_slot(SlotNo(100), SlotNo(100), SKEW_2),
            FutureSlotJudgement::NotFuture,
        );
    }

    #[test]
    fn not_future_when_behind() {
        assert_eq!(
            judge_header_slot(SlotNo(50), SlotNo(100), SKEW_2),
            FutureSlotJudgement::NotFuture,
        );
    }

    #[test]
    fn near_future_within_skew() {
        assert_eq!(
            judge_header_slot(SlotNo(102), SlotNo(100), SKEW_2),
            FutureSlotJudgement::NearFuture { excess_slots: 2 },
        );
        assert_eq!(
            judge_header_slot(SlotNo(101), SlotNo(100), SKEW_2),
            FutureSlotJudgement::NearFuture { excess_slots: 1 },
        );
    }

    #[test]
    fn far_future_beyond_skew() {
        assert_eq!(
            judge_header_slot(SlotNo(103), SlotNo(100), SKEW_2),
            FutureSlotJudgement::FarFuture { excess_slots: 3 },
        );
        assert_eq!(
            judge_header_slot(SlotNo(200), SlotNo(100), SKEW_2),
            FutureSlotJudgement::FarFuture { excess_slots: 100 },
        );
    }

    #[test]
    fn clock_skew_from_duration_ceil() {
        // 2s tolerance, 1s slots → 2 slot skew
        let skew = ClockSkew::from_duration(Duration::from_secs(2), Duration::from_secs(1));
        assert_eq!(skew.max_slots, 2);

        // 3s tolerance, 2s slots → ceil(1.5) = 2 slot skew
        let skew = ClockSkew::from_duration(Duration::from_secs(3), Duration::from_secs(2));
        assert_eq!(skew.max_slots, 2);
    }

    #[test]
    fn clock_skew_default_1s_slots() {
        let skew = ClockSkew::default_for_slot_length(Duration::from_secs(1));
        assert_eq!(skew.max_slots, 2);
    }

    #[test]
    fn clock_skew_zero_slot_length_safe() {
        let skew = ClockSkew::from_duration(Duration::from_secs(2), Duration::ZERO);
        // With zero slot length fallback to 1s → 2 slots
        assert_eq!(skew.max_slots, 2);
    }
}
