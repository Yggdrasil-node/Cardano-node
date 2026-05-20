//! Transaction-per-second throttle used by benchmark submission.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/TpsThrottle.hs`.
//! Ports upstream's TMVar watermark model using `Mutex` + `Condvar`:
//! empty blocks submission, `Allow(n)` permits `n` transmissions, and
//! `Stopped` terminates consumers.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::benchmarking::types::Req;
use crate::types::TpsRate;

/// Upstream `Step`: either allow the next transaction or stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    /// Upstream `Next`.
    Next,
    /// Upstream `Stop`.
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Slot {
    Empty,
    Allow(usize),
    Stopped,
}

#[derive(Debug)]
struct State {
    slot: Slot,
}

/// Mirror of upstream `TpsThrottle`.
#[derive(Clone, Debug)]
pub struct TpsThrottle {
    buffer_size: usize,
    count: usize,
    tps_rate: TpsRate,
    shared: Arc<(Mutex<State>, Condvar)>,
}

impl TpsThrottle {
    /// Construct a throttle with the upstream `newTpsThrottle` parameters.
    pub fn new(buffer_size: usize, count: usize, tps_rate: TpsRate) -> Self {
        Self {
            buffer_size,
            count,
            tps_rate,
            shared: Arc::new((Mutex::new(State { slot: Slot::Empty }), Condvar::new())),
        }
    }

    /// Upstream `startSending`.
    ///
    /// This method blocks while the watermark is full, just like
    /// `increaseWatermark` retries in STM when the queue reaches
    /// `buffersize`.
    pub fn start_sending(&self) {
        let target_delay = target_delay(self.tps_rate);
        let mut last_pre_delay = Instant::now();
        let mut last_delay = Duration::ZERO;

        for _ in 0..self.count {
            self.increase_watermark();
            let now = Instant::now();
            let loop_cost = now.saturating_duration_since(last_pre_delay);
            let delay = target_delay.saturating_sub(loop_cost.saturating_sub(last_delay));
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            last_pre_delay = now;
            last_delay = delay;
        }
    }

    /// Upstream `sendStop`.
    pub fn send_stop(&self) {
        let (lock, cv) = &*self.shared;
        let mut state = lock_state(lock);
        while matches!(state.slot, Slot::Allow(_)) {
            state = wait_state(cv, state);
        }
        state.slot = Slot::Stopped;
        cv.notify_all();
    }

    /// Upstream `receiveBlocking`.
    pub fn receive_blocking(&self) -> Step {
        let (lock, cv) = &*self.shared;
        let mut state = lock_state(lock);
        while matches!(state.slot, Slot::Empty) {
            state = wait_state(cv, state);
        }
        let step = receive_action(&mut state);
        cv.notify_all();
        step
    }

    /// Upstream `receiveNonBlocking`.
    pub fn receive_non_blocking(&self) -> Option<Step> {
        let (lock, cv) = &*self.shared;
        let mut state = lock_state(lock);
        if matches!(state.slot, Slot::Empty) {
            None
        } else {
            let step = receive_action(&mut state);
            cv.notify_all();
            Some(step)
        }
    }

    fn increase_watermark(&self) {
        let (lock, cv) = &*self.shared;
        let mut state = lock_state(lock);
        while matches!(state.slot, Slot::Allow(n) if n >= self.buffer_size) {
            state = wait_state(cv, state);
        }
        state.slot = match state.slot {
            Slot::Empty => Slot::Allow(1),
            Slot::Stopped => Slot::Stopped,
            Slot::Allow(n) => Slot::Allow(n + 1),
        };
        cv.notify_all();
    }
}

fn lock_state(lock: &Mutex<State>) -> MutexGuard<'_, State> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_state<'a>(cv: &Condvar, guard: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
    match cv.wait(guard) {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Upstream `newTpsThrottle`.
pub fn new_tps_throttle(buffer_size: usize, count: usize, tps_rate: TpsRate) -> TpsThrottle {
    TpsThrottle::new(buffer_size, count, tps_rate)
}

/// Upstream `consumeTxsBlocking`.
pub fn consume_txs_blocking(tps_throttle: &TpsThrottle, req: Req) -> (Step, usize) {
    let mut consumed = 0;
    for _ in 0..req.get() {
        match tps_throttle.receive_blocking() {
            Step::Stop => return (Step::Stop, consumed),
            Step::Next => consumed += 1,
        }
    }
    (Step::Next, consumed)
}

/// Upstream `consumeTxsNonBlocking`.
pub fn consume_txs_non_blocking(tps_throttle: &TpsThrottle, req: Req) -> (Step, usize) {
    if req.get() == 0 {
        return (Step::Next, 0);
    }

    match tps_throttle.receive_non_blocking() {
        None => (Step::Next, 0),
        Some(Step::Next) => (Step::Next, 1),
        Some(Step::Stop) => (Step::Stop, 0),
    }
}

fn receive_action(state: &mut State) -> Step {
    match state.slot {
        Slot::Stopped => {
            state.slot = Slot::Stopped;
            Step::Stop
        }
        Slot::Allow(1) => {
            state.slot = Slot::Empty;
            Step::Next
        }
        Slot::Allow(n) => {
            state.slot = Slot::Allow(n - 1);
            Step::Next
        }
        Slot::Empty => Step::Next,
    }
}

fn target_delay(tps_rate: TpsRate) -> Duration {
    if tps_rate.is_finite() && tps_rate > 0.0 {
        Duration::from_secs_f64(1.0 / tps_rate)
    } else {
        Duration::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonblocking_empty_returns_no_token() {
        let throttle = new_tps_throttle(32, 0, 1.0);

        assert_eq!(throttle.receive_non_blocking(), None);
        assert_eq!(consume_txs_non_blocking(&throttle, Req(1)), (Step::Next, 0));
    }

    #[test]
    fn receive_action_decrements_watermark_like_tmvar_state() {
        let throttle = new_tps_throttle(32, 0, 1.0);
        throttle.increase_watermark();
        throttle.increase_watermark();

        assert_eq!(throttle.receive_non_blocking(), Some(Step::Next));
        assert_eq!(throttle.receive_non_blocking(), Some(Step::Next));
        assert_eq!(throttle.receive_non_blocking(), None);
    }

    #[test]
    fn consume_nonblocking_takes_at_most_one_token() {
        let throttle = new_tps_throttle(32, 0, 1.0);
        throttle.increase_watermark();
        throttle.increase_watermark();

        assert_eq!(consume_txs_non_blocking(&throttle, Req(8)), (Step::Next, 1));
        assert_eq!(consume_txs_non_blocking(&throttle, Req(8)), (Step::Next, 1));
        assert_eq!(consume_txs_non_blocking(&throttle, Req(8)), (Step::Next, 0));
    }

    #[test]
    fn consume_blocking_counts_tokens_until_requested_count() {
        let throttle = new_tps_throttle(32, 3, 100_000.0);
        let feeder = {
            let throttle = throttle.clone();
            std::thread::spawn(move || throttle.start_sending())
        };

        assert_eq!(consume_txs_blocking(&throttle, Req(3)), (Step::Next, 3));
        feeder.join().expect("feeder should finish");
    }

    #[test]
    fn send_stop_preserves_stop_for_consumers() {
        let throttle = new_tps_throttle(32, 0, 1.0);

        throttle.send_stop();

        assert_eq!(throttle.receive_non_blocking(), Some(Step::Stop));
        assert_eq!(consume_txs_blocking(&throttle, Req(2)), (Step::Stop, 0));
    }
}
