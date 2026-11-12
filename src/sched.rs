//! Static and dynamic scheduling strategies.

use crate::engine::{run_parallel, RunStats, Work};
use crate::rng::Rng;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub const REL: Ordering = Ordering::Relaxed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    StaticBlock,
    StaticCyclic,
    Atomic,
    Guided,
    Steal,
}

impl Strategy {
    pub fn name(&self) -> &'static str {
        match self {
            Strategy::StaticBlock => "static_block",
            Strategy::StaticCyclic => "static_cyclic",
            Strategy::Atomic => "atomic",
            Strategy::Guided => "guided",
            Strategy::Steal => "steal",
        }
    }
}

pub fn static_block(p: usize, ipns: f64, n: usize, work: &Work) -> RunStats {
    run_parallel(p, ipns, |i, ctx| {
        let i = i as usize;
        let lo = n * i / p;
        let hi = n * (i + 1) / p;
        for j in lo..hi {
            work(ctx, j);
        }
    })
}

pub fn static_cyclic(p: usize, ipns: f64, n: usize, work: &Work) -> RunStats {
    run_parallel(p, ipns, |i, ctx| {
        let mut j = i as usize;
        while j < n {
            work(ctx, j);
            j += p;
        }
    })
}

pub fn lpt_assignment(costs: &[u64], p: usize) -> (Vec<Vec<usize>>, u64) {
    let t0 = std::time::Instant::now();
    let mut order: Vec<usize> = (0..costs.len()).collect();
    order.sort_by(|&a, &b| costs[b].cmp(&costs[a]));
    let mut assign: Vec<Vec<usize>> = vec![Vec::new(); p.max(1)];
    let mut load: Vec<u64> = vec![0; p.max(1)];
    for &j in &order {
        let mut best = 0usize;
        for i in 1..load.len() {
            if load[i] < load[best] {
                best = i;
            }
        }
        assign[best].push(j);
        load[best] += costs[j];
    }
    (assign, t0.elapsed().as_nanos() as u64)
}

pub fn preassigned(p: usize, ipns: f64, assign: &[Vec<usize>], work: &Work) -> RunStats {
    run_parallel(p, ipns, |i, ctx| {
        for &j in &assign[i as usize] {
            work(ctx, j);
        }
    })
}

pub fn atomic_dyn(p: usize, ipns: f64, n: usize, chunk: usize, work: &Work) -> RunStats {
    let chunk = chunk.max(1);
    let next = AtomicUsize::new(0);
    run_parallel(p, ipns, |_i, ctx| loop {
        let s = next.fetch_add(chunk, REL);
        ctx.ops += 1;
        if s >= n {
            break;
        }
        let e = (s + chunk).min(n);
        for j in s..e {
            work(ctx, j);
        }
    })
}

pub fn guided(p: usize, ipns: f64, n: usize, k: usize, work: &Work) -> RunStats {
    let kp = (k.max(1) * p.max(1)) as usize;
    let next = AtomicUsize::new(0);
    run_parallel(p, ipns, |_i, ctx| loop {
        let mut s = next.load(REL);
        let mut end = usize::MAX;
        loop {
            if s >= n {
                break;
            }
            let remaining = n - s;
            let chunk = ((remaining + kp - 1) / kp).max(1);
            let e = (s + chunk).min(n);
            match next.compare_exchange_weak(s, e, REL, REL) {
                Ok(_) => {
                    end = e;
                    break;
                }
                Err(cur) => s = cur,
            }
        }
        if end == usize::MAX {
            return;
        }
        ctx.ops += 1;
        for j in s..end {
            work(ctx, j);
        }
    })
}

pub struct StealCfg {
    pub spread: bool,
    pub steal_half: bool,
}

pub fn work_steal(
    p: usize,
    ipns: f64,
    n: usize,
    seed: u64,
    cfg: &StealCfg,
    work: &Work,
) -> RunStats {
    let deques: Vec<Mutex<VecDeque<u32>>> = (0..p).map(|_| Mutex::new(VecDeque::new())).collect();
    if cfg.spread {
        for j in 0..n {
            deques[j % p].lock().unwrap().push_back(j as u32);
        }
    } else {
        let mut d0 = deques[0].lock().unwrap();
        for j in 0..n {
            d0.push_back(j as u32);
        }
    }
    let remaining = AtomicUsize::new(n);
    let backoff_limit = 8 * p.max(1);

    run_parallel(p, ipns, |i, ctx| {
        let me = i as usize;
        let mut rng = Rng::new(seed ^ (me as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15));
        let mut fails = 0usize;
        loop {
            if remaining.load(REL) == 0 {
                break;
            }
            let own = {
                let mut d = deques[me].lock().unwrap();
                ctx.ops += 1;
                d.pop_front()
            };
            if let Some(t) = own {
                fails = 0;
                work(ctx, t as usize);
                remaining.fetch_sub(1, REL);
                continue;
            }
            let victim = rng.below(p);
            ctx.ops += 1;
            let taken: Option<Vec<u32>> = {
                let mut d = deques[victim].lock().unwrap();
                let len = d.len();
                if len > 1 {
                    let take = if cfg.steal_half { (len / 2).max(1) } else { 1 };
                    let mut buf = Vec::with_capacity(take);
                    for _ in 0..take {
                        if let Some(t) = d.pop_back() {
                            buf.push(t);
                        }
                    }
                    Some(buf)
                } else {
                    None
                }
            };
            if let Some(buf) = taken {
                fails = 0;
                ctx.steals += buf.len() as u64;
                let mut d = deques[me].lock().unwrap();
                for t in buf {
                    d.push_back(t);
                }
                continue;
            }
            fails += 1;
            if fails < backoff_limit {
                std::hint::spin_loop();
            } else if fails < 4 * backoff_limit {
                std::thread::yield_now();
            } else {
                std::thread::sleep(std::time::Duration::from_micros(20));
            }
        }
    })
}
