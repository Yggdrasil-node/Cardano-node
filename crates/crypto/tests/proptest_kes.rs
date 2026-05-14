//! Wave 9 PR 28 follow-on — property-based tests for Sum-KES sign/verify roundtrip.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side property-test harness;
//! upstream `cardano-crypto-class` has its own Haskell `QuickCheck`
//! tests for `verifyKES (signKES sk period msg) sk period msg`. This
//! Rust port covers the same invariant: every (seed, depth, period,
//! message) tuple in the valid input space must roundtrip through
//! `sign_sum_kes` / `verify_sum_kes` cleanly. Bad-period and bad-key
//! invariants are exercised as the negative cases.
//!
//! Run:
//!   cargo test -p yggdrasil-crypto --test proptest_kes
//!
//! Configurable cases per `proptest!` body via the `PROPTEST_CASES`
//! env var (default 16 here — Sum-KES sign at depth 3 is ~250 µs, so
//! 16 cases × 8 periods = ~30 ms total runtime; tighter than the
//! default 256 cases × 8 = ~500 ms which is too long for `cargo test`'s
//! default expectations).

use proptest::prelude::*;
use yggdrasil_crypto::sum_kes::{
    SumKesSignature, derive_sum_kes_vk, gen_sum_kes_signing_key, sign_sum_kes, update_sum_kes,
    verify_sum_kes,
};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        .. ProptestConfig::default()
    })]

    /// For any 32-byte seed + depth ∈ {0, 1, 2, 3} (covering 1, 2, 4,
    /// 8 KES evolution periods), the (sign, verify) roundtrip succeeds
    /// at every period in the valid range when given a non-empty
    /// message.
    #[test]
    fn sum_kes_sign_verify_roundtrips_every_period(
        seed in proptest::array::uniform32(any::<u8>()),
        depth in 0u32..=3u32,
        message in proptest::collection::vec(any::<u8>(), 1..=128),
    ) {
        let sk = gen_sum_kes_signing_key(&seed, depth)
            .expect("gen_sum_kes_signing_key with a 32-byte seed is infallible");
        let vk = derive_sum_kes_vk(&sk).expect("derive_sum_kes_vk should succeed");

        let total = 1u32 << depth; // depth 0 → 1, depth 1 → 2, depth 2 → 4, depth 3 → 8

        // Mutable copy of `sk` for update_sum_kes; each period must
        // sign cleanly before we advance.
        let mut current_sk = sk;
        for period in 0..total {
            let sig: SumKesSignature = sign_sum_kes(&current_sk, period, &message)
                .expect("sign_sum_kes inside the valid period range should succeed");
            verify_sum_kes(&vk, period, &message, &sig)
                .expect("verify_sum_kes on a freshly-signed signature must succeed");
            if period + 1 < total {
                current_sk = update_sum_kes(&current_sk, period)
                    .expect("update_sum_kes inside the valid range should succeed")
                    .expect("non-final period must yield a new key");
            }
        }
    }

    /// Verification with the WRONG message must fail. Mutating any
    /// single byte of the message after signing breaks the
    /// authentication tag; the verifier should reject.
    #[test]
    fn sum_kes_verify_rejects_tampered_message(
        seed in proptest::array::uniform32(any::<u8>()),
        depth in 0u32..=2u32,
        message in proptest::collection::vec(any::<u8>(), 1..=64),
        flip_byte_index in 0usize..64,
    ) {
        let sk = gen_sum_kes_signing_key(&seed, depth).expect("genkey infallible");
        let vk = derive_sum_kes_vk(&sk).expect("vk derive ok");
        let sig = sign_sum_kes(&sk, 0, &message).expect("sign ok");

        // Tamper with one byte.
        let idx = flip_byte_index % message.len();
        let mut tampered = message.clone();
        tampered[idx] = tampered[idx].wrapping_add(1);
        if tampered == message {
            // (Should never happen because wrapping_add(1) always
            // changes the byte; defensive prop_assume! anyway.)
            prop_assume!(false);
        }

        let result = verify_sum_kes(&vk, 0, &tampered, &sig);
        prop_assert!(
            result.is_err(),
            "verify_sum_kes must reject a tampered message; got Ok",
        );
    }

    /// Verification with the WRONG period must fail. A signature
    /// produced at period N is bound to that period; verifying it
    /// against any other period must reject.
    #[test]
    fn sum_kes_verify_rejects_wrong_period(
        seed in proptest::array::uniform32(any::<u8>()),
        depth in 1u32..=3u32,        // at depth 0 there's only one period; no wrong-period case
        actual_period in 0u32..8u32,
        verify_period in 0u32..8u32,
        message in proptest::collection::vec(any::<u8>(), 1..=64),
    ) {
        let total = 1u32 << depth;
        prop_assume!(actual_period < total);
        prop_assume!(verify_period < total);
        prop_assume!(actual_period != verify_period);

        // Generate the signing key at depth and advance it to
        // `actual_period`.
        let mut sk = gen_sum_kes_signing_key(&seed, depth).expect("genkey infallible");
        let vk = derive_sum_kes_vk(&sk).expect("vk derive ok");
        for p in 0..actual_period {
            sk = update_sum_kes(&sk, p)
                .expect("update advances cleanly")
                .expect("intermediate period must yield a key");
        }

        let sig = sign_sum_kes(&sk, actual_period, &message).expect("sign at actual_period ok");
        let result = verify_sum_kes(&vk, verify_period, &message, &sig);
        prop_assert!(
            result.is_err(),
            "verify_sum_kes must reject the wrong period; \
             signed at {actual_period}, verified at {verify_period}; got Ok",
        );
    }
}

#[cfg(test)]
mod fixed_cases {
    //! Non-proptest sanity checks. These run once each rather than
    //! sweeping the input space; they're cheap insurance against
    //! `proptest!`'s case-reduction shrinking past a real regression.

    use super::*;

    #[test]
    fn depth_zero_signs_one_period() {
        let seed = [0xa5_u8; 32];
        let sk = gen_sum_kes_signing_key(&seed, 0).expect("depth 0 genkey");
        let vk = derive_sum_kes_vk(&sk).expect("depth 0 vk");
        let sig = sign_sum_kes(&sk, 0, b"hello").expect("depth 0 sign");
        verify_sum_kes(&vk, 0, b"hello", &sig).expect("depth 0 verify");
    }

    #[test]
    fn depth_three_advances_through_all_eight_periods() {
        let seed = [0x33_u8; 32];
        let sk0 = gen_sum_kes_signing_key(&seed, 3).expect("depth 3 genkey");
        let vk = derive_sum_kes_vk(&sk0).expect("depth 3 vk");
        let mut sk = sk0;
        for period in 0u32..8u32 {
            let msg = format!("period-{period}-message");
            let sig = sign_sum_kes(&sk, period, msg.as_bytes()).expect("sign per period");
            verify_sum_kes(&vk, period, msg.as_bytes(), &sig).expect("verify per period");
            if period < 7 {
                sk = update_sum_kes(&sk, period)
                    .expect("advance per period")
                    .expect("non-final period must yield a key");
            }
        }
    }
}
