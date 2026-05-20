//! Types for running benchmarks.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Types.hs`.
//! Ports the small counting wrappers and submission error policy used by
//! `Cardano.Benchmarking.GeneratorTx.Submission` and `TpsThrottle`.

use std::ops::{Add, AddAssign};

use serde::Serialize;

/// Transactions decided for announcement now.
///
/// Mirrors upstream `newtype ToAnnce tx = ToAnnce [tx]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToAnnce<T>(pub Vec<T>);

/// Transactions announced but not yet acknowledged by the peer.
///
/// Mirrors upstream `newtype UnAcked tx = UnAcked [tx]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnAcked<T>(pub Vec<T>);

macro_rules! count_wrapper {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub usize);

        impl $name {
            /// Return the wrapped count.
            pub fn get(self) -> usize {
                self.0
            }
        }

        impl From<usize> for $name {
            fn from(value: usize) -> Self {
                Self(value)
            }
        }

        impl Add for $name {
            type Output = Self;

            fn add(self, rhs: Self) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }

        impl AddAssign for $name {
            fn add_assign(&mut self, rhs: Self) {
                self.0 += rhs.0;
            }
        }
    };
}

count_wrapper!(
    Ack,
    "Peer acknowledged this many transaction ids from the outstanding window."
);
count_wrapper!(
    Req,
    "Peer requested this many transaction ids to add to the outstanding window."
);
count_wrapper!(Sent, "This many transactions were sent to the peer.");
count_wrapper!(
    Unav,
    "This many transactions were requested by the peer but unavailable."
);

/// Controls how benchmark submission reacts to per-peer errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionErrorPolicy {
    /// Upstream `FailOnError`.
    FailOnError,
    /// Upstream `LogErrors`.
    LogErrors,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_wrappers_add_like_upstream_newtypes() {
        let mut sent = Sent(2);
        sent += Sent(3);

        assert_eq!((Ack(1) + Ack(2)).get(), 3);
        assert_eq!(Req::from(4).get(), 4);
        assert_eq!(sent.get(), 5);
        assert_eq!(Unav(7).get(), 7);
    }

    #[test]
    fn sent_and_unavailable_serialize_as_bare_counts() {
        let sent = match serde_json::to_string(&Sent(11)) {
            Ok(value) => value,
            Err(err) => panic!("Sent should serialize as JSON count: {err}"),
        };
        let unavailable = match serde_json::to_string(&Unav(13)) {
            Ok(value) => value,
            Err(err) => panic!("Unav should serialize as JSON count: {err}"),
        };

        assert_eq!(sent, "11");
        assert_eq!(unavailable, "13");
    }
}
