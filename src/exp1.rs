//! Static scheduling across four workload profiles.

use crate::engine::{self, RunStats, Work, WorkerCtx};
use crate::report::{self, Align};
use crate::sched;
use crate::workload::{self, Dist};
use crate::{io, meta};

pub struct Args {
    pub runs: usize,
    pub n: usize,
    pub total_ns: u64,
    pub ps: Vec<usize>,
    pub seed: u64,
    pub out: String,
    pub detail: String,
    pub quiet: bool,
    pub detail_ps: Vec<usize>,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            runs: 15,
            n: 64,
            total_ns: 200_000_000,
            ps: vec![1, 2, 4, 6, 8, 12],
            seed: 0x5eed_0001,
            out: "results/raw/exp1.csv".into(),
            detail: "results/raw/exp1_detail.json".into(),
            quiet: false,
            detail_ps: vec![6],
        }
    }
}

pub fn run(a: &Args, ipns: f64) -> std::io::Result<()> {
    let mut csv_rows: Vec<Vec<String>> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut bar_blocks = String::new();
    let mut cfg_json: Vec<String> = Vec::new();
    let mut serial_json: Vec<String> = Vec::new();
    let mut noise_floor = 0.0f64;

    for (di, &dist) in Dist::all().iter().enumerate() {
        let costs = workload::gen_costs(
            dist,
            a.n,
            a.total_ns,
            a.seed.wrapping_add(di as u64 * 7919),
            0.5,
        );
        let n = costs.len();
        let cvv = workload::cv(&costs);
        let total_work: u64 = costs.iter().sum();
        let max_task = *costs.iter().max().unwrap_or(&0);
        let work = |ctx: &mut WorkerCtx, j: usize| ctx.do_task(j as u32, costs[j]);
        let wk: &Work = &work;

        let mut t1s = Vec::new();
        let mut serial_last: Option<RunStats> = None;
        for _ in 0..a.runs {
            let st = sched::static_block(1, ipns, n, wk);
            t1s.push(st.makespan_ns as f64);
            serial_last = Some(st);
        }
        let t1 = engine::median(&t1s);
        let t_max: u64 = serial_last
            .as_ref()
            .map(|s| {
                s.recs
                    .iter()
                    .map(|r| r.end_ns - r.start_ns)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        if dist == Dist::Uniform {
            noise_floor = engine::iqr(&t1s) / t1;
        }
        serial_json.push(format!(
            "{{\"dist\":\"{}\",\"n\":{},\"total_work_ns\":{},\"max_task_ns\":{},\"cv\":{:.4},\"t1_ns\":{:.0},\"t1_iqr_rel\":{:.4},\"t_max_measured_ns\":{}}}",
            dist.name(),
            n,
            total_work,
            max_task,
            cvv,
            t1,
            engine::iqr(&t1s) / t1,
            t_max
        ));

        for &p in &a.ps {
            let mut runs: Vec<RunStats> = Vec::new();
            for _ in 0..a.runs {
                runs.push(sched::static_block(p, ipns, n, wk));
            }
            let makespans: Vec<f64> = runs.iter().map(|r| r.makespan_ns as f64).collect();
            let med = engine::median(&makespans);
            let rep = runs
                .iter()
                .enumerate()
                .min_by(|(_, x), (_, y)| {
                    let dx = (x.makespan_ns as f64 - med).abs();
                    let dy = (y.makespan_ns as f64 - med).abs();
                    dx.partial_cmp(&dy).unwrap()
                })
                .map(|(i, _)| i)
                .unwrap();
            let s = &runs[rep];
            let m = engine::metrics(s, t1 as u64, p, t1 as u64);
            let iqr_rel = engine::iqr(&makespans) / med.max(1.0);

            table_rows.push(vec![
                format!("{}{}{}", report::c::cyan(), dist.name(), report::c::RESET),
                format!("{:.2}", cvv),
                format!("{}", p),
                format!("{:.1}", m.makespan_ns as f64 / 1e6),
                format!("{:.1}", m.ideal_ns as f64 / 1e6),
                format!(
                    "{}{:.2}{}",
                    report::speedup_color(m.speedup, p as f64),
                    m.speedup,
                    report::c::RESET
                ),
                format!("{:.2}", m.efficiency),
                format!(
                    "{}{:.2}{}",
                    report::ratio_color(m.lif),
                    m.lif,
                    report::c::RESET
                ),
                format!("{:>4.1}", m.idle_frac * 100.0),
                format!("{:.1}", iqr_rel * 100.0),
            ]);

            for (ri, r) in runs.iter().enumerate() {
                let mm = engine::metrics(r, t1 as u64, p, t1 as u64);
                csv_rows.push(vec![
                    dist.name().to_string(),
                    format!("{}", p),
                    format!("{}", ri),
                    format!("{}", mm.makespan_ns),
                    format!("{:.0}", t1),
                    format!("{:.4}", mm.speedup),
                    format!("{:.4}", mm.efficiency),
                    format!("{:.4}", mm.lif),
                    format!("{:.4}", mm.idle_frac),
                    format!("{}", mm.busy_max),
                    format!("{}", mm.busy_min),
                    format!("{:.0}", mm.busy_std),
                    format!("{}", *r.spans.iter().max().unwrap_or(&0)),
                    format!("{}", r.steals),
                    format!("{}", r.ops),
                ]);
            }

            if a.detail_ps.contains(&p) {
                let mut th = Vec::new();
                for t in 0..p {
                    th.push(format!(
                        "{{\"t\":{},\"span_ns\":{},\"busy_ns\":{},\"tasks\":{}}}",
                        t, s.spans[t], s.busy[t], s.tasks[t]
                    ));
                }
                let mut recs = Vec::new();
                for r in &s.recs {
                    recs.push(format!(
                        "{{\"t\":{},\"j\":{},\"s\":{},\"e\":{}}}",
                        r.thread, r.task, r.start_ns, r.end_ns
                    ));
                }
                cfg_json.push(format!(
                    "{{\"exp\":\"exp1\",\"dist\":\"{}\",\"p\":{},\"cv\":{:.4},\"makespan_ns\":{},\"release_ns\":{},\"ideal_ns\":{},\"threads\":[{}],\"recs\":[{}]}}",
                    dist.name(), p, cvv, s.makespan_ns, s.release_ns, m.ideal_ns,
                    th.join(","), recs.join(",")
                ));

                let maxb = *s.busy.iter().max().unwrap_or(&1);
                let mut b = String::new();
                b.push_str(&format!(
                    "\n  {}{}{} p={}, CV={:.2}, T={:.1} ms, ideal={:.1} ms  (imbalance factor {}{:.2}{})\n",
                    report::c::BOLD, dist.name(), report::c::RESET, p, cvv,
                    s.makespan_ns as f64 / 1e6, m.ideal_ns as f64 / 1e6,
                    report::ratio_color(m.lif), m.lif, report::c::RESET
                ));
                for t in 0..p {
                    let frac = s.busy[t] as f64 / maxb as f64;
                    let color = if frac > 0.90 {
                        report::c::green()
                    } else if frac > 0.60 {
                        report::c::yellow()
                    } else {
                        report::c::red()
                    };
                    b.push_str(&format!(
                        "    thr {:>2} {}{:>7.1} ms{} {:>5.0}%  tasks {:>3}  {}\n",
                        t,
                        report::c::white(),
                        s.busy[t] as f64 / 1e6,
                        report::c::RESET,
                        frac * 100.0,
                        s.tasks[t],
                        report::bar(s.busy[t] as f64, maxb as f64, 34, &color)
                    ));
                }
                bar_blocks.push_str(&b);
            }
        }
    }

    if !a.quiet {
        let p = meta::collect();
        print!("{}", meta::passport_table(&p, ipns));
        print!("{}", report::header("Parallel overhead of this machine"));
        print!("{}", meta::overhead_table(ipns));
        print!(
            "{}",
            report::header(
                "Experiment 1: static equal blocks, 4 workload profiles (n=64, random order)"
            )
        );
        let mut t = report::table(
            None,
            &[
                "workload",
                "CV",
                "p",
                "T_p ms",
                "ideal ms",
                "speedup",
                "efficiency",
                "imbalance",
                "idle %",
                "IQR %",
            ],
            &table_rows,
            &[
                Align::L,
                Align::R,
                Align::R,
                Align::R,
                Align::R,
                Align::R,
                Align::R,
                Align::R,
                Align::R,
                Align::R,
            ],
        );
        t.push_str(&format!(
            "  noise floor (residual IQR/T_1 at p=1, uniform): {}{:.2} %{}\n",
            report::c::yellow(),
            noise_floor * 100.0,
            report::c::RESET
        ));
        print!("{t}");
        print!("{}", report::header("Per-thread workload (median run)"));
        print!("{bar_blocks}");
    }

    io::write_csv(
        &a.out,
        &[
            "dist",
            "p",
            "run",
            "makespan_ns",
            "t1_ns",
            "speedup",
            "efficiency",
            "lif",
            "idle_frac",
            "busy_max_ns",
            "busy_min_ns",
            "busy_std_ns",
            "span_max_ns",
            "steals",
            "ops",
        ],
        &csv_rows,
    )?;

    let json = format!(
        "{{\n  \"experiment\": \"exp1\",\n  \"n\": {},\n  \"total_ns\": {},\n  \"runs\": {},\n  \"ps\": [{}],\n  \"serial\": [{}],\n  \"configs\": [\n    {}\n  ]\n}}\n",
        a.n,
        a.total_ns,
        a.runs,
        a.ps.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(","),
        serial_json.join(",\n    "),
        cfg_json.join(",\n    ")
    );
    io::write_text(&a.detail, &json)?;
    Ok(())
}
