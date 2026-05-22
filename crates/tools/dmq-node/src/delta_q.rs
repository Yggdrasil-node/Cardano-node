//! dmq-node DeltaQ latency-modelling foundations.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of upstream `Ouroboros.Network.DeltaQ`
//! — the latency-distribution model the keepalive protocol measures
//! into `PeerGSV` records, which the DMQ `NodeKernel`'s keepalive
//! registry holds. dmq-node carries its own copy (the R732
//! dmq-node-local decision).
//!
//! Slice of the Option A `run()` integration arc (see the
//! `docs/COMPLETION_ROADMAP.md` A4 dmq-node entry); this slice ports
//! the `Distribution` leaf — `GSV` / `PeerGSV` build on it.

/// An (improper) probability distribution.
///
/// Mirror of upstream `data Distribution n`. The upstream
/// representation — like this port — currently covers only degenerate
/// distributions: a single value taken with probability 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Distribution<N> {
    /// A [degenerate distribution][wiki] — the value taken with
    /// probability 1.
    ///
    /// [wiki]: https://en.wikipedia.org/wiki/Degenerate_distribution
    DegenerateDistribution(N),
}

/// Make a degenerate distribution.
///
/// Mirror of upstream `degenerateDistribution`.
pub fn degenerate_distribution<N>(n: N) -> Distribution<N> {
    Distribution::DegenerateDistribution(n)
}

impl<N: std::ops::Add<Output = N>> Distribution<N> {
    /// The [convolution][wiki] of two distributions — for degenerate
    /// distributions, the values add.
    ///
    /// Mirror of upstream `convolveDistribution` (the `Semigroup`
    /// instance).
    ///
    /// [wiki]: https://en.wikipedia.org/wiki/Convolution
    pub fn convolve(self, other: Distribution<N>) -> Distribution<N> {
        let Distribution::DegenerateDistribution(a) = self;
        let Distribution::DegenerateDistribution(b) = other;
        Distribution::DegenerateDistribution(a + b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degenerate_distribution_wraps_its_value() {
        let d = degenerate_distribution(7i64);
        assert_eq!(d, Distribution::DegenerateDistribution(7));
    }

    #[test]
    fn convolution_of_degenerate_distributions_adds() {
        let a = degenerate_distribution(3i64);
        let b = degenerate_distribution(4i64);
        assert_eq!(a.convolve(b), Distribution::DegenerateDistribution(7));
    }

    #[test]
    fn convolution_with_a_zero_distribution_is_identity() {
        let d = degenerate_distribution(5i64);
        let zero = degenerate_distribution(0i64);
        assert_eq!(d.convolve(zero), d);
    }
}
