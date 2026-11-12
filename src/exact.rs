//! Exact scheduling and lower bounds.

pub fn optimum(costs: &[u64], m: usize) -> u64 {
    let n = costs.len();
    assert!(n <= 16, "exact DP is limited to n <= 16");
    let full = 1usize << n;

    let mut sum = vec![0u64; full];
    for mask in 1..full {
        let b = mask.trailing_zeros() as usize;
        sum[mask] = sum[mask & (mask - 1)] + costs[b];
    }

    const INF: u64 = u64::MAX;
    let mut cur = vec![INF; full];
    cur[0] = 0;
    for _ in 0..m {
        let mut next = vec![INF; full];
        next[0] = 0;
        for mask in 1..full {
            let lsb = mask & mask.wrapping_neg();
            let mut best = INF;
            let mut s = mask;
            while s > 0 {
                if s & lsb != 0 {
                    let rest = mask ^ s;
                    if cur[rest] != INF {
                        let v = sum[s].max(cur[rest]);
                        if v < best {
                            best = v;
                        }
                    }
                }
                s = (s - 1) & mask;
            }
            next[mask] = best;
        }
        cur = next;
    }
    cur[full - 1]
}

pub fn lower_bound(costs: &[u64], m: usize) -> u64 {
    let w: u64 = costs.iter().sum();
    let max_t = *costs.iter().max().unwrap_or(&0);
    max_t.max(w.div_ceil(m.max(1) as u64))
}

pub fn bound_list_scheduling(opt: f64, m: usize) -> f64 {
    (2.0 - 1.0 / m as f64) * opt
}

pub fn bound_lpt(opt: f64, m: usize) -> f64 {
    (4.0 / 3.0 - 1.0 / (3.0 * m as f64)) * opt
}
