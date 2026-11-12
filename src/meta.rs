//! Machine information and overhead measurements.

use crate::report::{self, Align};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Barrier;
use std::time::Instant;

pub struct Passport {
    pub cpu_model: String,
    pub logical_cpus: usize,
    pub physical_cores: usize,
    pub os: String,
    pub kernel: String,
    pub governor: String,
    pub rustc: String,
    pub cpu_max_mhz: String,
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn kv(path: &str, key: &str) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            return Some(rest.trim_matches('"').to_string());
        }
    }
    None
}

fn cmd(prog: &str, args: &[&str]) -> String {
    match Command::new(prog).args(args).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => "n/a".to_string(),
    }
}

pub fn collect() -> Passport {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let mut cpu_model = String::from("n/a");
    for line in cpuinfo.lines() {
        if line.starts_with("model name") && cpu_model == "n/a" {
            cpu_model = line.split(':').nth(1).unwrap_or("n/a").trim().to_string();
        }
    }
    let mut pairs = std::collections::HashSet::new();
    let mut cur_phys = String::new();
    let mut cur_core: Option<String> = None;
    for line in cpuinfo.lines() {
        if line.starts_with("processor") {
            if let Some(c) = cur_core.take() {
                pairs.insert(format!("{cur_phys}:{c}"));
            }
            cur_phys.clear();
        }
        if let Some(v) = line.strip_prefix("physical id") {
            cur_phys = v.trim_start_matches([':', ' ', '\t']).trim().to_string();
        }
        if let Some(v) = line.strip_prefix("core id") {
            cur_core = Some(v.trim_start_matches([':', ' ', '\t']).trim().to_string());
        }
    }
    if let Some(c) = cur_core.take() {
        pairs.insert(format!("{cur_phys}:{c}"));
    }
    let physical = pairs.len().max(1);

    Passport {
        cpu_model,
        logical_cpus: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        physical_cores: physical,
        os: kv("/etc/os-release", "PRETTY_NAME").unwrap_or_else(|| "n/a".into()),
        kernel: read("/proc/sys/kernel/osrelease").unwrap_or_else(|| "n/a".into()),
        governor: read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
            .unwrap_or_else(|| "n/a".into()),
        rustc: cmd("rustc", &["--version"]),
        cpu_max_mhz: read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
            .map(|f| format!("{:.2}", f.parse::<f64>().unwrap_or(0.0) / 1000.0))
            .unwrap_or_else(|| "n/a".into()),
    }
}

pub fn passport_table(p: &Passport, iters_per_ns: f64) -> String {
    let rows = vec![
        vec!["CPU".into(), p.cpu_model.clone()],
        vec![
            "cores / threads".into(),
            format!(
                "{} physical / {} logical (SMT: {})",
                p.physical_cores,
                p.logical_cpus,
                if p.logical_cpus > p.physical_cores {
                    "on"
                } else {
                    "off"
                }
            ),
        ],
        vec!["max frequency".into(), format!("{} MHz", p.cpu_max_mhz)],
        vec!["OS".into(), p.os.clone()],
        vec!["kernel".into(), p.kernel.clone()],
        vec!["CPU governor".into(), p.governor.clone()],
        vec!["toolchain".into(), p.rustc.clone()],
        vec![
            "work unit".into(),
            format!(
                "{:.3} loop-iterations per ns ({:.2} ns per iteration)",
                iters_per_ns,
                1.0 / iters_per_ns
            ),
        ],
    ];
    report::table(
        Some("Experiment passport"),
        &["property", "value"],
        &rows,
        &[Align::L, Align::L],
    )
}

pub fn bench_atomic(p: usize, ops_per_thread: u64) -> f64 {
    let counter = AtomicU64::new(0);
    let barrier = Barrier::new(p + 1);
    let t = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..p {
            s.spawn(|| {
                barrier.wait();
                for _ in 0..ops_per_thread {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        barrier.wait();
    });
    let d = t.elapsed().as_nanos() as f64;
    d / (p as f64 * ops_per_thread as f64)
}

pub fn bench_spawn(p: usize, reps: usize) -> f64 {
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let t = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..p {
                s.spawn(|| {
                    std::hint::black_box(1);
                });
            }
        });
        samples.push(t.elapsed().as_nanos() as f64 / 1000.0);
    }
    crate::engine::median(&samples)
}

pub fn overhead_table(iters_per_ns: f64) -> String {
    let mut rows = Vec::new();
    for p in [1usize, 2, 4, 6, 8, 12] {
        let ns = bench_atomic(p, 200_000);
        rows.push(vec![
            format!("{p}"),
            format!("{:.2}", ns),
            format!("{:.0}", 1000.0 / ns),
        ]);
    }
    let spawn = bench_spawn(6, 60);
    let mut out = report::table(
        None,
        &["threads", "atomic fetch_add, ns/op", "Mops/s"],
        &rows,
        &[Align::R, Align::R, Align::R],
    );
    out.push_str(&format!(
        "  thread spawn+join, p=6: {} {:.1} µs{}\n",
        report::c::yellow(),
        spawn,
        report::c::RESET
    ));
    out.push_str(&format!("  work unit: {:.3} iterations/ns\n", iters_per_ns));
    out
}
