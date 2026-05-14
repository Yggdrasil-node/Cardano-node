//! Wave 9 PR 28 — criterion microbenchmark for `yggdrasil_crypto::blake2b`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side benchmark scaffold; upstream
//! exercises blake2b throughput indirectly via the cardano-base
//! `cardano-crypto-praos-bench` package — Yggdrasil keeps the bench
//! local to the implementation crate so a `cargo bench
//! -p yggdrasil-crypto` invocation tracks regressions without
//! pulling in the broader benchmark harness.
//!
//! Run:
//!   cargo bench -p yggdrasil-crypto --bench blake2b_hash
//!
//! The bench measures three representative input sizes:
//!   - 32 B    (typical ChainSync header-hash payload)
//!   - 1 KiB   (typical small transaction)
//!   - 64 KiB  (typical large block body slab — close to the upstream
//!     `BlockBodySize` cap so the bench tracks the worst-case
//!     hash cost of a single block validation)
//!
//! Regressions here cascade into chain-sync throughput, mempool
//! admission rate, and block-fetch validation latency.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use yggdrasil_crypto::blake2b::{hash_bytes, hash_bytes_256};

fn bench_blake2b_default(c: &mut Criterion) {
    let sizes: &[usize] = &[32, 1024, 64 * 1024];
    let mut group = c.benchmark_group("blake2b/hash_bytes");
    for &size in sizes {
        let bytes = vec![0xa5u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("{size}B"), |b| {
            b.iter(|| {
                let h = hash_bytes(&bytes);
                std::hint::black_box(h);
            });
        });
    }
    group.finish();
}

fn bench_blake2b_256(c: &mut Criterion) {
    let sizes: &[usize] = &[32, 1024, 64 * 1024];
    let mut group = c.benchmark_group("blake2b/hash_bytes_256");
    for &size in sizes {
        let bytes = vec![0xa5u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("{size}B"), |b| {
            b.iter(|| {
                let h = hash_bytes_256(&bytes);
                std::hint::black_box(h);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_blake2b_default, bench_blake2b_256);
criterion_main!(benches);
