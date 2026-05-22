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
//! Slices of the Option A `run()` integration arc (see the
//! `docs/COMPLETION_ROADMAP.md` A4 dmq-node entry): the `Distribution`
//! leaf and the `Gsv` / `PeerGsv` latency model that builds on it.
//!
//! Upstream's `GSV` / `PeerGSV` are spelled `Gsv` / `PeerGsv` here —
//! Rust's `upper_case_acronyms` lint requires the mixed-case form.

use std::time::Duration;

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

/// A G/S/V latency model for one direction of a peer link.
///
/// Mirror of upstream `data GSV` — `G` the minimum (size-independent)
/// latency, `S` the per-byte transmission time, `V` the latency
/// variance distribution. Upstream's `S` is a general
/// `SizeInBytes -> DiffTime` function; in practice every `GSV` is
/// ballistic (built via `ballisticGSV`), so the Rust port models `S`
/// as the linear per-byte rate. Time quantities are `f64` seconds,
/// the natural representation for the DeltaQ arithmetic (mirror of
/// upstream `DiffTime`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gsv {
    /// `G` — the minimum, size-independent latency, in seconds.
    pub g: f64,
    /// `S` — the per-byte transmission time, in seconds per byte.
    pub s: f64,
    /// `V` — the latency-variance distribution.
    pub v: Distribution<f64>,
}

impl Gsv {
    /// The convolution of two GSVs — the per-component composition of
    /// two link segments in series.
    ///
    /// Mirror of upstream `instance Semigroup GSV`.
    pub fn convolve(self, other: Gsv) -> Gsv {
        Gsv {
            g: self.g + other.g,
            s: self.s + other.s,
            v: self.v.convolve(other.v),
        }
    }
}

/// Construct a ballistic `Gsv` — a linear latency model.
///
/// Mirror of upstream `ballisticGSV`.
pub fn ballistic_gsv(g: f64, s: f64, v: Distribution<f64>) -> Gsv {
    Gsv { g, s, v }
}

/// The measured GSV latency model for a peer, in both directions.
///
/// Mirror of upstream `data PeerGSV`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeerGsv {
    /// When this sample was taken — mirror of upstream `Time`, a
    /// duration since the monotonic origin.
    pub sample_time: Duration,
    /// The outbound-direction GSV.
    pub outbound_gsv: Gsv,
    /// The inbound-direction GSV.
    pub inbound_gsv: Gsv,
}

/// The default `PeerGsv`, used before any keepalive measurement has
/// been taken.
///
/// Mirror of upstream `defaultGSV`: `G` 500 ms, `S` 2 µs/byte
/// (~4 Mbps), `V` the degenerate-zero distribution.
pub fn default_gsv() -> PeerGsv {
    let gsv = ballistic_gsv(0.5, 2e-6, degenerate_distribution(0.0));
    PeerGsv {
        sample_time: Duration::ZERO,
        outbound_gsv: gsv,
        inbound_gsv: gsv,
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

    #[test]
    fn default_gsv_matches_upstream_constants() {
        let p = default_gsv();
        assert_eq!(p.sample_time, Duration::ZERO);
        assert_eq!(p.outbound_gsv.g, 0.5);
        assert_eq!(p.outbound_gsv.s, 2e-6);
        assert_eq!(p.inbound_gsv.v, Distribution::DegenerateDistribution(0.0));
        // Both directions share the default link model.
        assert_eq!(p.outbound_gsv, p.inbound_gsv);
    }

    #[test]
    fn gsv_convolution_composes_components() {
        let a = ballistic_gsv(0.1, 1e-6, degenerate_distribution(0.0));
        let b = ballistic_gsv(0.2, 3e-6, degenerate_distribution(0.0));
        let c = a.convolve(b);
        // G and S add; V convolves.
        assert!((c.g - 0.3).abs() < 1e-12);
        assert!((c.s - 4e-6).abs() < 1e-18);
        assert_eq!(c.v, Distribution::DegenerateDistribution(0.0));
    }
}
