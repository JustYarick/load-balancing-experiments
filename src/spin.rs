//! Calibrated CPU workload.

use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
pub fn spin(iters: u64, seed: u64) -> u64 {
    let mut x = seed | 1;
    let mut i = 0u64;
    while i < iters {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 29;
        i += 1;
    }
    black_box(x)
}

pub fn calibrate_iters_per_ns() -> f64 {
    let probe = 300_000u64;
    let t0 = Instant::now();
    black_box(spin(probe, 0x9e3779b97f4a7c15));
    let d0 = t0.elapsed().as_nanos().max(1) as f64;
    let mut per_ns = probe as f64 / d0;

    let target_ns = 25_000_000.0;
    let iters = (per_ns * target_ns).max(10_000.0) as u64;
    let t1 = Instant::now();
    black_box(spin(iters, 0xdeadbeefcafebabe));
    let d1 = t1.elapsed().as_nanos().max(1) as f64;
    per_ns = iters as f64 / d1;

    let mut acc = per_ns;
    for k in 0..3u64 {
        let t = Instant::now();
        black_box(spin(iters, 0x1234_5678_9abc_def0 ^ k));
        let d = t.elapsed().as_nanos().max(1) as f64;
        acc += iters as f64 / d;
    }
    acc / 4.0
}

#[inline]
pub fn run_task(cost_ns: u64, iters_per_ns: f64, seed: u64) -> u64 {
    let iters = ((cost_ns as f64) * iters_per_ns) as u64;
    let t = Instant::now();
    black_box(spin(iters, seed));
    t.elapsed().as_nanos() as u64
}
