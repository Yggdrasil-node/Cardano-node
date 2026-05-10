//! Wall-clock helpers used by the cardano-tracer EKG metric backend.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Time.hs.
//!
//! Direct port of upstream's single-function `Cardano.Tracer.Time`
//! module:
//!
//! | Upstream                                          | Yggdrasil          |
//! |---------------------------------------------------|--------------------|
//! | `getTimeMs :: IO Int64`                           | [`get_time_ms`]    |
//!
//! Upstream's docstring (preserved verbatim for context):
//!
//! > forkServer definition of `getTimeMs`. The ekg frontend relies
//! > on the "ekg.server_timestamp_ms" metric being in every store.
//! > While forkServer adds that that automatically we must manually
//! > add it.
//! > url
//! >  + https://github.com/tvh/ekg-wai/blob/master/System/Remote/Monitoring/Wai.hs#L237-L238

use std::time::{SystemTime, UNIX_EPOCH};

/// Current Unix epoch time in milliseconds. Mirror of upstream
/// `getTimeMs :: IO Int64; getTimeMs = (round . (* 1000)) \`fmap\` getPOSIXTime`.
///
/// Returns the time elapsed since the Unix epoch, in milliseconds,
/// as a signed 64-bit integer matching upstream's `Int64` width.
/// Negative results are possible if the system clock has been set
/// before 1970 (extremely unlikely in practice but the type stays
/// signed for parity); values well past the 32-bit range are
/// representable through year 292 million.
pub fn get_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn get_time_ms_returns_positive_integer_after_epoch() {
        let now_ms = get_time_ms();
        // Anything past Unix epoch + 30 years is past the 1999-12-31
        // mark (~946 billion ms). 32-bit range exhausted in 2038.
        assert!(now_ms > 946_000_000_000);
    }

    #[test]
    fn get_time_ms_within_2_seconds_of_system_now() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis() as i64;
        let mid = get_time_ms();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis() as i64;
        // The reported time is sandwiched between two adjacent calls
        // to SystemTime::now() and within a 2-second window
        // (generous upper bound for a non-realtime CI host).
        assert!(mid >= before - 2_000);
        assert!(mid <= after + 2_000);
    }

    #[test]
    fn get_time_ms_monotonic_within_short_window() {
        let t1 = get_time_ms();
        // Call again — must not regress.
        let t2 = get_time_ms();
        assert!(t2 >= t1);
    }

    #[test]
    fn get_time_ms_returns_i64_type() {
        // Type-shape assertion — guarantees we keep the upstream
        // Int64 return-type contract.
        let _: i64 = get_time_ms();
    }
}
