//! Nominal schedule variability without CPU timing.

use lbe::{exact, io, sched, workload};
use workload::{Dist, Ordering_};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut sets = 100usize;
    let mut output = "generated/model_variability.csv".to_string();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--sets" => {
                i += 1;
                sets = args.get(i).ok_or("missing --sets value")?.parse()?;
            }
            "--out" => {
                i += 1;
                output = args.get(i).ok_or("missing --out value")?.clone();
            }
            "--help" | "-h" => {
                println!("variability [--sets N] [--out CSV]\nNominal model only: n=64, p=6, W≈200 ms; four workload profiles.");
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
        i += 1;
    }
    if sets == 0 {
        return Err("--sets must be positive".into());
    }
    let n = 64;
    let p = 6;
    let mut rows = Vec::new();
    for (profile, dist) in Dist::all().into_iter().enumerate() {
        for set in 0..sets {
            let seed = 0x2026_1112_0000u64 + (profile as u64) * 10_000 + set as u64;
            let costs = workload::gen_costs(dist, n, 200_000_000, seed, 0.5);
            let lb = exact::lower_bound(&costs, p);
            let (assignment, _) = sched::lpt_assignment(&costs, p);
            let lpt = assignment
                .iter()
                .map(|jobs| jobs.iter().map(|&j| costs[j]).sum::<u64>())
                .max()
                .unwrap();
            let append = |rows: &mut Vec<Vec<String>>, alg: &str, order: &str, time: u64| {
                rows.push(vec![
                    dist.name().into(),
                    set.to_string(),
                    seed.to_string(),
                    n.to_string(),
                    p.to_string(),
                    alg.into(),
                    order.into(),
                    costs.iter().sum::<u64>().to_string(),
                    lb.to_string(),
                    time.to_string(),
                    format!("{:.12}", time as f64 / lb as f64),
                ]);
            };
            append(&mut rows, "lpt_greedy", "lpt", lpt);
            for order in [Ordering_::Natural, Ordering_::Lpt, Ordering_::Ascending] {
                let work = workload::reorder(&costs, order, 0);
                let block = (0..p)
                    .map(|worker| {
                        work[n * worker / p..n * (worker + 1) / p]
                            .iter()
                            .sum::<u64>()
                    })
                    .max()
                    .unwrap();
                let cycle = (0..p)
                    .map(|worker| work.iter().skip(worker).step_by(p).sum::<u64>())
                    .max()
                    .unwrap();
                append(&mut rows, "static_block", order.name(), block);
                append(&mut rows, "static_cyclic", order.name(), cycle);
            }
        }
    }
    io::write_csv(
        &output,
        &[
            "profile",
            "set",
            "seed",
            "n",
            "p",
            "alg",
            "ordering",
            "nominal_total_ns",
            "lb_ns",
            "model_makespan_ns",
            "model_ratio_lb",
        ],
        &rows,
    )?;
    println!(
        "Saved {} model schedules to {output}; no wall-clock timing measured",
        rows.len()
    );
    Ok(())
}
