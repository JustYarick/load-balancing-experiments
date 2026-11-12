//! Dynamic scheduling and chunk size experiments.

use crate::engine::{self, Work, WorkerCtx};
use crate::exact;
use crate::io;
use crate::report::{self, Align};
use crate::sched::{self, StealCfg};
use crate::workload::{self, Dist, Ordering_};

#[derive(Clone, Copy)]
enum Kind {
    LptGreedy,
    StaticBlockLpt,
    StaticBlockRandom,
    StaticCyclicLpt,
    Atomic(usize),
    Guided(usize),
    Steal { spread: bool, half: bool },
}

impl Kind {
    fn alg_name(&self) -> &'static str {
        match self {
            Kind::LptGreedy => "lpt_greedy",
            Kind::StaticBlockLpt | Kind::StaticBlockRandom => "static_block",
            Kind::StaticCyclicLpt => "static_cyclic",
            Kind::Atomic(_) => "atomic",
            Kind::Guided(_) => "guided",
            Kind::Steal { .. } => "steal",
        }
    }
    fn param(&self) -> String {
        match self {
            Kind::LptGreedy => "lpt".into(),
            Kind::StaticBlockLpt | Kind::StaticCyclicLpt => "lpt".into(),
            Kind::StaticBlockRandom => "random".into(),
            Kind::Atomic(c) => format!("chunk={c}"),
            Kind::Guided(k) => format!("k={k}"),
            Kind::Steal { spread, half } => format!(
                "{}{}",
                if *spread { "spread" } else { "eager" },
                if *half { "+half" } else { "+single" }
            ),
        }
    }
    fn csv_param(&self) -> String {
        match self {
            Kind::LptGreedy => "greedy".into(),
            Kind::StaticBlockLpt | Kind::StaticCyclicLpt => "lpt".into(),
            Kind::StaticBlockRandom => "random".into(),
            Kind::Atomic(c) => format!("chunk={c}"),
            Kind::Guided(k) => format!("k={k}"),
            Kind::Steal { spread, half } => format!(
                "{}{}",
                if *spread { "spread" } else { "eager" },
                if *half { "+half" } else { "+single" }
            ),
        }
    }
    fn is_dynamic(&self) -> bool {
        matches!(self, Kind::Atomic(_) | Kind::Guided(_) | Kind::Steal { .. })
    }
    fn plan_ns(&self, costs: &[u64], p: usize) -> u64 {
        if matches!(self, Kind::LptGreedy) {
            sched::lpt_assignment(costs, p).1
        } else {
            0
        }
    }
}

pub struct Args {
    pub runs: usize,
    pub coarse_n: usize,
    pub fine_n: usize,
    pub total_ns: u64,
    pub p: usize,
    pub seed: u64,
    pub out: String,
    pub detail: String,
    pub quiet: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            runs: 15,
            coarse_n: 64,
            fine_n: 4096,
            total_ns: 200_000_000,
            p: 6,
            seed: 0x5eed_0003,
            out: "results/raw/exp3.csv".into(),
            detail: "results/raw/exp3_detail.json".into(),
            quiet: false,
        }
    }
}

const COARSE_SEED: u64 = 0x00c0_ffee;

const COARSE_SET: [Kind; 12] = [
    Kind::StaticBlockRandom,
    Kind::StaticBlockLpt,
    Kind::StaticCyclicLpt,
    Kind::LptGreedy,
    Kind::Atomic(1),
    Kind::Atomic(2),
    Kind::Atomic(4),
    Kind::Atomic(8),
    Kind::Atomic(16),
    Kind::Guided(2),
    Kind::Guided(4),
    Kind::Steal {
        spread: true,
        half: true,
    },
];

const FINE_SET: [Kind; 7] = [
    Kind::StaticCyclicLpt,
    Kind::LptGreedy,
    Kind::Atomic(1),
    Kind::Atomic(16),
    Kind::Atomic(64),
    Kind::Atomic(256),
    Kind::Guided(2),
];

fn exec(kind: Kind, costs: &[u64], p: usize, ipns: f64, seed: u64) -> engine::RunStats {
    let n = costs.len();
    let work = |ctx: &mut WorkerCtx, j: usize| ctx.do_task(j as u32, costs[j]);
    let wk: &Work = &work;
    match kind {
        Kind::LptGreedy => {
            let (assign, _plan) = sched::lpt_assignment(costs, p);
            sched::preassigned(p, ipns, &assign, wk)
        }
        Kind::StaticBlockLpt | Kind::StaticBlockRandom => sched::static_block(p, ipns, n, wk),
        Kind::StaticCyclicLpt => sched::static_cyclic(p, ipns, n, wk),
        Kind::Atomic(c) => sched::atomic_dyn(p, ipns, n, c, wk),
        Kind::Guided(k) => sched::guided(p, ipns, n, k, wk),
        Kind::Steal { spread, half } => sched::work_steal(
            p,
            ipns,
            n,
            seed,
            &StealCfg {
                spread,
                steal_half: half,
            },
            wk,
        ),
    }
}

fn costs_for(kind: Kind, base_rand: &[u64], base_lpt: &[u64]) -> Vec<u64> {
    if matches!(kind, Kind::StaticBlockLpt | Kind::StaticCyclicLpt) {
        base_lpt.to_vec()
    } else {
        base_rand.to_vec()
    }
}

pub fn run(a: &Args, ipns: f64) -> std::io::Result<()> {
    let mut csv_rows: Vec<Vec<String>> = Vec::new();
    let mut cfg_json: Vec<String> = Vec::new();
    let mut verdict: Vec<(String, String, bool, f64, f64, f64)> = Vec::new();

    let regimes: [(&str, usize, &[Kind]); 2] = [
        ("coarse", a.coarse_n, &COARSE_SET),
        ("fine", a.fine_n, &FINE_SET),
    ];

    for (regime, n, set) in regimes {
        let seed = match regime {
            "coarse" => COARSE_SEED,
            _ => a.seed.wrapping_add(n as u64),
        };
        let base_rand = workload::gen_costs(Dist::Bimodal, n, a.total_ns, seed, 0.5);
        let base_lpt = workload::reorder(&base_rand, Ordering_::Lpt, 0);
        let nominal_lb = exact::lower_bound(&base_rand, a.p);
        let cv = workload::cv(&base_rand);

        let serial_work = |ctx: &mut WorkerCtx, j: usize| ctx.do_task(j as u32, base_rand[j]);
        let serial: &Work = &serial_work;
        let mut t1s = Vec::new();
        let mut serial_last: Option<engine::RunStats> = None;
        for _ in 0..a.runs {
            let st = sched::static_block(1, ipns, n, serial);
            t1s.push(st.makespan_ns as f64);
            serial_last = Some(st);
        }
        let t1 = engine::median(&t1s);
        let t_max: u64 = serial_last
            .as_ref()
            .map(|st| {
                st.recs
                    .iter()
                    .map(|r| r.end_ns - r.start_ns)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let lb = t_max.max((t1 as u64).div_ceil(a.p as u64));

        let mut rows: Vec<Vec<String>> = Vec::new();
        for (ki, kind) in set.iter().enumerate() {
            let costs = costs_for(*kind, &base_rand, &base_lpt);
            let kid = ki as u64;
            let (runs, rep_idx, _med) = engine::repeat(a.runs, || {
                let run_seed = a.seed ^ kid.wrapping_mul(0x9e37_79b9) ^ 0x5a5a;
                exec(*kind, &costs, a.p, ipns, run_seed)
            });
            let rep = runs[rep_idx].clone();
            let m = engine::metrics(&rep, t1 as u64, a.p, t1 as u64);
            let t = rep.makespan_ns as f64;
            let ratio_lb = t / lb as f64;

            for (ri, r) in runs.iter().enumerate() {
                let mm = engine::metrics(r, t1 as u64, a.p, t1 as u64);
                csv_rows.push(vec![
                    regime.to_string(),
                    kind.alg_name().to_string(),
                    kind.csv_param(),
                    "bimodal".to_string(),
                    format!("{n}"),
                    format!("{}", a.p),
                    format!("{ri}"),
                    format!("{}", mm.makespan_ns),
                    format!("{:.0}", t1),
                    format!("{:.4}", mm.speedup),
                    format!("{:.4}", mm.efficiency),
                    format!("{:.4}", mm.lif),
                    format!("{:.4}", mm.idle_frac),
                    format!("{}", r.steals),
                    format!("{}", r.ops),
                ]);
            }

            let row = vec![
                format!(
                    "{}{}{}",
                    if kind.is_dynamic() {
                        report::c::green()
                    } else {
                        report::c::orange()
                    },
                    kind.alg_name(),
                    report::c::RESET
                ),
                kind.param(),
                if kind.is_dynamic() {
                    "dynamic".to_string()
                } else {
                    "static".to_string()
                },
                format!("{:.1}", t / 1e6),
                format!(
                    "{}{:.2}{}",
                    report::ratio_color(ratio_lb),
                    ratio_lb,
                    report::c::RESET
                ),
                format!(
                    "{}{:.2}{}",
                    report::speedup_color(m.speedup, a.p as f64),
                    m.speedup,
                    report::c::RESET
                ),
                format!("{:.2}", m.efficiency),
                format!("{:>4.1}", m.idle_frac * 100.0),
                format!("{:.1}", rep.ops as f64 / 1000.0),
                format!("{}", rep.steals),
            ];
            rows.push(row.clone());
            verdict.push((
                regime.to_string(),
                format!("{}/{}", kind.alg_name(), kind.param()),
                kind.is_dynamic(),
                m.speedup,
                ratio_lb,
                m.idle_frac * 100.0,
            ));

            let plan = kind.plan_ns(&costs, a.p);
            let include_recs = regime == "coarse";
            cfg_json.push(engine::run_json_ext(
                &format!("{}/{}/{}", regime, kind.alg_name(), kind.param()),
                &rep,
                &format!(
                    "\"regime\":\"{}\",\"n\":{},\"p\":{},\"cv\":{:.4},\"lb_ns\":{},\"nominal_lb_ns\":{},\"t_max_ns\":{},\"t1_ns\":{:.0},\"plan_ns\":{},\"ratio_lb\":{:.4},\"speedup\":{:.4},\"efficiency\":{:.4},\"idle_frac\":{:.4},",
                    regime, n, a.p, cv, lb, nominal_lb, t_max, t1, plan, ratio_lb, m.speedup, m.efficiency, m.idle_frac
                ),
                include_recs,
            ));
        }

        if !a.quiet {
            let title = if regime == "coarse" {
                format!(
                    "coarse regime: n={n} tasks (~{:.2} ms per task), p={}, CV={:.2}, lower bound {} ms",
                    a.total_ns as f64 / n as f64 / 1e6,
                    a.p,
                    cv,
                    report::ms(lb as f64)
                )
            } else {
                format!(
                    "fine regime: n={n} tasks (~{:.1} µs per task), p={}, CV={:.2}, lower bound {} ms",
                    a.total_ns as f64 / n as f64 / 1e3,
                    a.p,
                    cv,
                    report::ms(lb as f64)
                )
            };
            print!(
                "{}",
                report::table(
                    Some(title.as_str()),
                    &[
                        "strategy",
                        "param",
                        "kind",
                        "T_p ms",
                        "T/LB",
                        "speedup",
                        "efficiency",
                        "idle %",
                        "sync kops",
                        "steals",
                    ],
                    &rows,
                    &[
                        Align::L,
                        Align::L,
                        Align::C,
                        Align::R,
                        Align::R,
                        Align::R,
                        Align::R,
                        Align::R,
                        Align::R,
                        Align::R,
                    ],
                )
            );
        }
    }

    if !a.quiet {
        print!("{}", report::header("Experiment 3 verdict"));
        for reg in ["coarse", "fine"] {
            let sub: Vec<_> = verdict.iter().filter(|v| v.0 == reg).collect();
            if sub.is_empty() {
                continue;
            }
            let best = sub
                .iter()
                .max_by(|a, b| a.3.partial_cmp(&b.3).unwrap())
                .unwrap();
            let worst = sub
                .iter()
                .min_by(|a, b| a.3.partial_cmp(&b.3).unwrap())
                .unwrap();
            let bs = sub
                .iter()
                .filter(|v| !v.2)
                .max_by(|a, b| a.3.partial_cmp(&b.3).unwrap())
                .unwrap();
            let bd = sub
                .iter()
                .filter(|v| v.2)
                .max_by(|a, b| a.3.partial_cmp(&b.3).unwrap())
                .unwrap();
            let line = |tag: &str, v: &(String, String, bool, f64, f64, f64), c: &str| {
                format!(
                    "    {} {:<12} {}{:<24}{} speedup {}{:.2}{}  T/LB {:.2}  idle {:>4.1}%\n",
                    tag,
                    "",
                    c,
                    v.1,
                    report::c::RESET,
                    c,
                    v.3,
                    report::c::RESET,
                    v.4,
                    v.5
                )
            };
            print!(
                "  {}{}{}\n",
                report::c::BOLD,
                if reg == "coarse" {
                    "coarse tasks"
                } else {
                    "fine tasks  "
                },
                report::c::RESET
            );
            print!("{}", line("best static ", bs, &report::c::cyan()));
            print!("{}", line("best dynamic", bd, &report::c::green()));
            print!("{}", line("best overall", best, &report::c::green()));
            print!("{}", line("worst       ", worst, &report::c::red()));
        }
        print!(
            "  reminder: the sync cost of this machine is only ~2-9 ns per atomic operation\n  (see `lb calib`), so on these task granularities the price of small chunks\n  is negligible next to the imbalance they remove.\n"
        );
    }

    io::write_csv(
        &a.out,
        &[
            "regime",
            "alg",
            "param",
            "dist",
            "n",
            "p",
            "run",
            "makespan_ns",
            "t1_ns",
            "speedup",
            "efficiency",
            "lif",
            "idle_frac",
            "steals",
            "ops",
        ],
        &csv_rows,
    )?;

    let json = format!(
        "{{\n  \"experiment\": \"exp3\",\n  \"p\": {},\n  \"total_ns\": {},\n  \"configs\": [\n    {}\n  ]\n}}\n",
        a.p,
        a.total_ns,
        cfg_json.join(",\n    ")
    );
    io::write_text(&a.detail, &json)?;
    Ok(())
}
