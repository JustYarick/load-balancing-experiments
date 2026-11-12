//! Deterministic task cost generation.

use crate::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dist {
    Uniform,
    Exponential,
    Bimodal,
    OneHeavy,
}

impl Dist {
    pub fn name(&self) -> &'static str {
        match self {
            Dist::Uniform => "uniform",
            Dist::Exponential => "exponential",
            Dist::Bimodal => "bimodal",
            Dist::OneHeavy => "one_heavy",
        }
    }
    pub fn parse(s: &str) -> Option<Dist> {
        match s {
            "uniform" => Some(Dist::Uniform),
            "exponential" | "exp" => Some(Dist::Exponential),
            "bimodal" => Some(Dist::Bimodal),
            "one_heavy" | "oneheavy" => Some(Dist::OneHeavy),
            _ => None,
        }
    }
    pub fn all() -> [Dist; 4] {
        [
            Dist::Uniform,
            Dist::Exponential,
            Dist::Bimodal,
            Dist::OneHeavy,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ordering_ {
    Natural,
    Lpt,
    Ascending,
}

impl Ordering_ {
    pub fn name(&self) -> &'static str {
        match self {
            Ordering_::Natural => "random",
            Ordering_::Lpt => "lpt",
            Ordering_::Ascending => "ascending",
        }
    }
}

pub fn gen_costs(dist: Dist, n: usize, total_ns: u64, seed: u64, heavy_frac: f64) -> Vec<u64> {
    gen_costs_w(dist, n, total_ns, seed, heavy_frac, 10.0)
}

pub fn gen_costs_w(
    dist: Dist,
    n: usize,
    total_ns: u64,
    seed: u64,
    heavy_frac: f64,
    heavy_w: f64,
) -> Vec<u64> {
    let mut rng = Rng::new(seed);
    let n = n.max(2);
    let mut w: Vec<f64> = Vec::with_capacity(n);
    match dist {
        Dist::Uniform => {
            for _ in 0..n {
                w.push(0.5 + rng.next_f64());
            }
        }
        Dist::Exponential => {
            for _ in 0..n {
                let u = rng.next_f64().max(1e-12);
                w.push(-u.ln() + 0.05);
            }
        }
        Dist::Bimodal => {
            for _ in 0..n {
                let heavy = rng.next_f64() < 0.20;
                w.push(if heavy { heavy_w } else { 1.0 });
            }
        }
        Dist::OneHeavy => {
            let rest = (1.0 - heavy_frac).max(0.0);
            w.push(heavy_frac.max(1e-9));
            for _ in 1..n {
                w.push(rest / (n - 1) as f64 * (0.5 + rng.next_f64()));
            }
        }
    }

    let sum: f64 = w.iter().sum();
    let mut costs: Vec<u64> = w
        .iter()
        .map(|x| ((x / sum) * total_ns as f64).round().max(1.0) as u64)
        .collect();

    if dist != Dist::OneHeavy {
        rng.shuffle(&mut costs);
    }
    costs
}

pub fn reorder(costs: &[u64], ord: Ordering_, seed: u64) -> Vec<u64> {
    let mut v = costs.to_vec();
    match ord {
        Ordering_::Natural => {}
        Ordering_::Lpt => v.sort_by(|a, b| b.cmp(a)),
        Ordering_::Ascending => v.sort_by(|a, b| a.cmp(b)),
    }
    let _ = Rng::new(seed);
    v
}

pub fn cv(costs: &[u64]) -> f64 {
    let n = costs.len() as f64;
    let mean = costs.iter().sum::<u64>() as f64 / n;
    let var = costs
        .iter()
        .map(|c| {
            let d = *c as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    var.sqrt() / mean
}
