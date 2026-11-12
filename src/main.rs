//! Command-line interface for the experiments.

use lbe::report;
use lbe::{exp1, exp2, exp3, meta, spin};

fn usage() {
    println!(
        "{}lb-experiments{} — load balancing in parallel computing (P||Cmax)\n",
        report::c::BOLD,
        report::c::RESET
    );
    println!("{}USAGE{}", report::c::BOLD, report::c::RESET);
    println!("  lb passport");
    println!("  lb calib");
    println!(
        "  lb exp1 [--runs N] [--n N] [--total-ms MS] [--ps 1,2,4,6,8,12] [--out F] [--detail F]"
    );
    println!("  lb exp2 [--runs N] [--out F] [--detail F]");
    println!("  lb exp3 [--runs N] [--p N] [--coarse-n N] [--fine-n N] [--out F] [--detail F]");
    println!("  lb help\n");
    println!("{}NOTES{}", report::c::BOLD, report::c::RESET);
    println!("  * every run is CPU-bound: tasks burn real cycles, durations are calibrated");
    println!("  * T_1 (serial reference) is measured in-process on the same code path");
    println!("  * strategies: static_block | static_cyclic | atomic | guided | steal");
    println!(
        "  * zero external crates: {}cargo build --release{} works offline",
        report::c::yellow(),
        report::c::RESET
    );
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn get_str(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn get_usize(args: &[String], name: &str, d: usize) -> usize {
    get_str(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

fn get_f64(args: &[String], name: &str, d: f64) -> f64 {
    get_str(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let rest: Vec<String> = argv.iter().skip(2).cloned().collect();
    let cmd = argv.get(1).map(|s| s.as_str()).unwrap_or("help");

    match cmd {
        "help" | "--help" | "-h" => usage(),
        "passport" => {
            let ipns = spin::calibrate_iters_per_ns();
            let p = meta::collect();
            print!("{}", meta::passport_table(&p, ipns));
        }
        "calib" => {
            let ipns = spin::calibrate_iters_per_ns();
            print!("{}", report::header("Parallel overhead micro-benchmarks"));
            print!("{}", meta::overhead_table(ipns));
        }
        "exp1" => {
            let ipns = spin::calibrate_iters_per_ns();
            let mut a = exp1::Args::default();
            a.runs = get_usize(&rest, "--runs", a.runs);
            a.n = get_usize(&rest, "--n", a.n);
            a.total_ns = (get_f64(&rest, "--total-ms", a.total_ns as f64 / 1e6) * 1e6) as u64;
            if let Some(ps) = get_str(&rest, "--ps") {
                a.ps = ps
                    .split(',')
                    .filter_map(|x| x.trim().parse().ok())
                    .collect();
            }
            if has(&rest, "--quiet") {
                a.quiet = true;
            }
            if let Some(v) = get_str(&rest, "--out") {
                a.out = v;
            }
            if let Some(v) = get_str(&rest, "--detail") {
                a.detail = v;
            }
            if let Err(e) = exp1::run(&a, ipns) {
                eprintln!("exp1 failed: {e}");
                std::process::exit(1);
            }
        }
        "exp2" => {
            let ipns = spin::calibrate_iters_per_ns();
            let mut a = exp2::Args::default();
            a.runs = get_usize(&rest, "--runs", a.runs);
            if has(&rest, "--quiet") {
                a.quiet = true;
            }
            if let Some(v) = get_str(&rest, "--out") {
                a.out = v;
            }
            if let Some(v) = get_str(&rest, "--detail") {
                a.detail = v;
            }
            if let Err(e) = exp2::run(&a, ipns) {
                eprintln!("exp2 failed: {e}");
                std::process::exit(1);
            }
        }
        "exp3" => {
            let ipns = spin::calibrate_iters_per_ns();
            let mut a = exp3::Args::default();
            a.runs = get_usize(&rest, "--runs", a.runs);
            a.p = get_usize(&rest, "--p", a.p);
            a.coarse_n = get_usize(&rest, "--coarse-n", a.coarse_n);
            a.fine_n = get_usize(&rest, "--fine-n", a.fine_n);
            a.total_ns = (get_f64(&rest, "--total-ms", a.total_ns as f64 / 1e6) * 1e6) as u64;
            if has(&rest, "--quiet") {
                a.quiet = true;
            }
            if let Some(v) = get_str(&rest, "--out") {
                a.out = v;
            }
            if let Some(v) = get_str(&rest, "--detail") {
                a.detail = v;
            }
            if let Err(e) = exp3::run(&a, ipns) {
                eprintln!("exp3 failed: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("unknown command: {other}\n");
            usage();
            std::process::exit(2);
        }
    }
}
