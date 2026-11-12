//! Task ordering and assignment experiments.

use crate::engine::{self, RunStats, Work, WorkerCtx};
use crate::exact;
use crate::io;
use crate::report::{self, Align};
use crate::sched;
use crate::workload::{self, Dist, Ordering_};

pub struct Args {
    pub runs: usize,
    pub seed: u64,
    pub out: String,
    pub detail: String,
    pub quiet: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            runs: 15,
            seed: 0x5eed_0002,
            out: "results/raw/exp2.csv".into(),
            detail: "results/raw/exp2_detail.json".into(),
            quiet: false,
        }
    }
}

struct Instance {
    tag: &'static str,
    n: usize,
    m: usize,
    total_ns: u64,
    dist: Dist,
    heavy_w: f64,
    seed: u64,
}

static SMALL: Instance = Instance {
    tag: "small",
    n: 16,
    m: 6,
    total_ns: 300_000_000,
    dist: Dist::Bimodal,
    heavy_w: 3.0,
    seed: 0x1a2b_3c4d,
};

static MEDIUM: Instance = Instance {
    tag: "medium",
    n: 64,
    m: 6,
    total_ns: 200_000_000,
    dist: Dist::Bimodal,
    heavy_w: 10.0,
    seed: 0x00c0_ffee,
};

#[derive(Clone, Copy)]
enum V {
    Block(Ordering_),
    Cyclic(Ordering_),
    GreedyLpt,
}

const VARIANTS: [V; 7] = [
    V::GreedyLpt,
    V::Block(Ordering_::Lpt),
    V::Block(Ordering_::Natural),
    V::Block(Ordering_::Ascending),
    V::Cyclic(Ordering_::Lpt),
    V::Cyclic(Ordering_::Natural),
    V::Cyclic(Ordering_::Ascending),
];

impl V {
    fn alg(&self) -> &'static str {
        match self {
            V::Block(_) => "static_block",
            V::Cyclic(_) => "static_cyclic",
            V::GreedyLpt => "lpt_greedy",
        }
    }
    fn order(&self) -> &'static str {
        match self {
            V::Block(o) | V::Cyclic(o) => o.name(),
            V::GreedyLpt => "lpt",
        }
    }
}

struct Meas {
    stats: Vec<RunStats>,
    rep: RunStats,
    med_ns: f64,
    plan_ns: u64,
}

fn measure(v: V, base: &[u64], m: usize, ipns: f64, runs: usize) -> Meas {
    let plan_ns;
    let work_costs: Vec<u64>;
    let assign;
    let greedy = matches!(v, V::GreedyLpt);
    if greedy {
        let (a, pn) = sched::lpt_assignment(base, m);
        assign = a;
        plan_ns = pn;
        work_costs = base.to_vec();
    } else {
        let ord = match v {
            V::Block(o) | V::Cyclic(o) => o,
            V::GreedyLpt => Ordering_::Lpt,
        };
        work_costs = workload::reorder(base, ord, 0);
        plan_ns = 0;
        assign = Vec::new();
    }
    let n = work_costs.len();
    let work = |ctx: &mut WorkerCtx, j: usize| ctx.do_task(j as u32, work_costs[j]);
    let wk: &Work = &work;
    let cyclic = matches!(v, V::Cyclic(_));
    let (stats, rep_idx, med_ns) = engine::repeat(runs, || {
        if greedy {
            sched::preassigned(m, ipns, &assign, wk)
        } else if cyclic {
            sched::static_cyclic(m, ipns, n, wk)
        } else {
            sched::static_block(m, ipns, n, wk)
        }
    });
    let rep = stats[rep_idx].clone();
    Meas {
        stats,
        rep,
        med_ns,
        plan_ns,
    }
}

pub fn run(a: &Args, ipns: f64) -> std::io::Result<()> {
    let mut csv_rows: Vec<Vec<String>> = Vec::new();
    let mut cfg_json: Vec<String> = Vec::new();
    let mut blocks: Vec<(&Instance, Vec<Vec<String>>, f64)> = Vec::new();
    let mut head_small = String::new();
    let mut note_small = String::new();
    let mut head_medium = String::new();
    let mut best_small = f64::MAX;
    let mut best_label = String::new();
    let mut opt_small = 0.0;
    let mut lb_small = 0.0;
    let mut opt_meas = 0.0;
    let mut lb_meas = 0.0;

    for inst in [&SMALL, &MEDIUM] {
        let base = workload::gen_costs_w(
            inst.dist,
            inst.n,
            inst.total_ns,
            inst.seed,
            0.5,
            inst.heavy_w,
        );
        let w_nominal: u64 = base.iter().sum();
        let cv = workload::cv(&base);

        let serial_work = |ctx: &mut WorkerCtx, j: usize| ctx.do_task(j as u32, base[j]);
        let serial: &Work = &serial_work;
        let mut t1s = Vec::new();
        let mut t_max = 0u64;
        for _ in 0..a.runs {
            let st = sched::static_block(1, ipns, base.len(), serial);
            t1s.push(st.makespan_ns as f64);
            t_max = t_max.max(
                st.recs
                    .iter()
                    .map(|r| r.end_ns - r.start_ns)
                    .max()
                    .unwrap_or(0),
            );
        }
        let t1 = engine::median(&t1s);
        let k = t1 / w_nominal as f64;
        let lb_nominal = exact::lower_bound(&base, inst.m) as f64;
        let lb_meas_inst = (t_max as f64).max(t1 / inst.m as f64);
        let opt_nominal = if inst.tag == "small" {
            exact::optimum(&base, inst.m) as f64
        } else {
            f64::NAN
        };
        let opt_meas_inst = opt_nominal * k;

        if inst.tag == "small" {
            opt_small = opt_nominal;
            lb_small = lb_nominal;
            opt_meas = opt_meas_inst;
            lb_meas = lb_meas_inst;
            head_small = format!(
                "small instance: n={}, m={}, CV={:.2}, W={} ms | exact optimum = {} ms, lower bound = {} ms, OPT/LB = {:.3}",
                inst.n, inst.m, cv, report::ms(w_nominal as f64), report::ms(opt_nominal),
                report::ms(lb_nominal), opt_nominal / lb_nominal
            );
            note_small = format!(
                "  measured on this machine: T_1 = {} ms, calibration k = T_1/W = {:.4}, measured lower bound = {} ms, measured optimum = {} ms",
                report::ms(t1), k, report::ms(lb_meas_inst), report::ms(opt_meas_inst)
            );
        } else {
            head_medium = format!(
                "medium instance: n={}, m={}, CV={:.2}, W={} ms | nominal lower bound = {} ms -> measured lower bound = {} ms | measured T_1 = {} ms",
                inst.n, inst.m, cv, report::ms(w_nominal as f64), report::ms(lb_nominal),
                report::ms(lb_meas_inst), report::ms(t1)
            );
        }

        let mut rows = Vec::new();
        let mut gantt = Vec::new();
        for v in VARIANTS {
            let meas = measure(v, &base, inst.m, ipns, a.runs);
            let t = meas.med_ns;
            let ratio_lb = t / lb_meas_inst;
            let ratio_opt = if inst.tag == "small" {
                t / opt_meas_inst
            } else {
                f64::NAN
            };
            let label = format!("{}/{}", v.alg(), v.order());

            for r in &meas.stats {
                csv_rows.push(vec![
                    inst.tag.to_string(),
                    format!("{}", inst.n),
                    format!("{}", inst.m),
                    v.alg().to_string(),
                    v.order().to_string(),
                    format!("{}", r.makespan_ns),
                    format!("{:.0}", opt_meas_inst),
                    format!("{:.0}", lb_meas_inst),
                    format!("{:.4}", r.makespan_ns as f64 / opt_meas_inst),
                    format!("{:.4}", r.makespan_ns as f64 / lb_meas_inst),
                    format!("{:.4}", cv),
                    format!("{}", meas.plan_ns),
                ]);
            }

            let row = vec![
                format!(
                    "{}{}{}",
                    if matches!(v, V::GreedyLpt) {
                        report::c::green()
                    } else {
                        report::c::cyan()
                    },
                    v.alg(),
                    report::c::RESET
                ),
                format!("{}{}{}", report::c::magenta(), v.order(), report::c::RESET),
                format!("{:.2}", t / 1e6),
                format!(
                    "{}{:.3}{}",
                    report::ratio_color(ratio_lb),
                    ratio_lb,
                    report::c::RESET
                ),
                if inst.tag == "small" {
                    format!(
                        "{}{:.3}{}",
                        report::ratio_color(ratio_opt),
                        ratio_opt,
                        report::c::RESET
                    )
                } else {
                    format!("{}", report::c::DIM)
                },
                format!("{}", meas.plan_ns as f64 / 1e3),
            ];
            rows.push(row);

            if inst.tag == "small" && t < best_small {
                best_small = t;
                best_label = label.clone();
            }
            if inst.tag == "small"
                || label == "static_cyclic/lpt"
                || label == "static_block/random"
                || label == "static_block/ascending"
            {
                gantt.push(engine::run_json_ext(
                    &format!("{}/{}", inst.tag, label),
                    &meas.rep,
                    &format!(
                        "\"ratio_lb\":{:.4},\"ratio_opt\":{:.4},\"plan_ns\":{},\"t1_ns\":{:.0},",
                        ratio_lb, ratio_opt, meas.plan_ns, t1
                    ),
                    inst.tag == "small",
                ));
            }
        }
        cfg_json.extend(gantt);
        blocks.push((inst, rows, t1 / 1e6));
    }

    if !a.quiet {
        print!(
            "{}",
            report::header("Experiment 2: static assignment rule x task order (+ exact optimum)")
        );
        for (inst, rows, _t1_ms) in &blocks {
            let head = if inst.tag == "small" {
                &head_small
            } else {
                &head_medium
            };
            print!(
                "{}",
                report::table(
                    Some(head.as_str()),
                    &["strategy", "order", "T_p ms", "T/LB", "T/OPT", "plan µs"],
                    rows,
                    &[Align::L, Align::L, Align::R, Align::R, Align::R, Align::R],
                )
            );
            if inst.tag == "small" && !note_small.is_empty() {
                println!("{note_small}");
            }
        }
        print!(
            "  list scheduling guarantee (2-1/m)·OPT = {} ms | LPT guarantee (4/3-1/(3m))·OPT = {} ms (both in measured units)\n",
            report::ms(opt_meas * (2.0 - 1.0 / SMALL.m as f64)),
            report::ms(opt_meas * (4.0 / 3.0 - 1.0 / (3.0 * SMALL.m as f64)))
        );
        print!(
            "  best measured on the small instance: {}{}{} = {} ms -> ratio {:.3} of the exact optimum\n",
            report::c::BOLD, best_label, report::c::RESET, report::ms(best_small), best_small / opt_meas
        );
        print!(
            "  small instance: nominal optimum {} ms, OPT/LB = {:.3}; all values below are scaled to measured units\n",
            report::ms(opt_small),
            opt_small / lb_small.max(1.0)
        );
    }

    io::write_csv(
        &a.out,
        &[
            "instance",
            "n",
            "m",
            "alg",
            "ordering",
            "makespan_ns",
            "opt_ns",
            "lb_ns",
            "ratio_opt",
            "ratio_lb",
            "cv",
            "plan_ns",
        ],
        &csv_rows,
    )?;

    let json = format!(
        "{{\n  \"experiment\": \"exp2\",\n  \"small\": {{\"n\": {}, \"m\": {}, \"opt_nominal_ns\": {:.0}, \"opt_measured_ns\": {:.0}, \"lb_measured_ns\": {:.0}, \"total_ns\": {}, \"plan_seed\": {}}},\n  \"medium\": {{\"n\": {}, \"m\": {}, \"lb_measured_ns\": {:.0}, \"total_ns\": {}}},\n  \"configs\": [\n    {}\n  ]\n}}\n",
        SMALL.n, SMALL.m, opt_small, opt_meas, lb_meas, SMALL.total_ns, SMALL.seed,
        MEDIUM.n, MEDIUM.m, 0.0, MEDIUM.total_ns,
        cfg_json.join(",\n    ")
    );
    io::write_text(&a.detail, &json)?;
    Ok(())
}
