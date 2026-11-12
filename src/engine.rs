//! Parallel execution and timing.

use crate::spin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Barrier;
use std::time::Instant;

pub const REL: Ordering = Ordering::Relaxed;

#[derive(Clone, Copy, Debug)]
pub struct TaskRec {
    pub thread: u32,
    pub task: u32,
    pub start_ns: u64,
    pub end_ns: u64,
}

pub struct WorkerOut {
    pub thread: u32,
    pub end_ns: u64,
    pub busy_ns: u64,
    pub tasks: u64,
    pub steals: u64,
    pub ops: u64,
    pub recs: Vec<TaskRec>,
}

pub struct WorkerCtx {
    pub thread: u32,
    t_ref: Instant,
    pub iters_per_ns: f64,
    pub busy_ns: u64,
    pub tasks: u64,
    pub steals: u64,
    pub ops: u64,
    pub recs: Vec<TaskRec>,
}

impl WorkerCtx {
    #[inline]
    pub fn do_task(&mut self, task: u32, cost_ns: u64) {
        let iters_per_ns = self.iters_per_ns;
        self.do_task_with(task, move || {
            spin::run_task(cost_ns, iters_per_ns, task as u64);
        });
    }

    #[inline]
    pub fn do_task_with<F: FnOnce()>(&mut self, task: u32, f: F) {
        let start_ns = self.t_ref.elapsed().as_nanos() as u64;
        f();
        let end_ns = self.t_ref.elapsed().as_nanos() as u64;
        self.recs.push(TaskRec {
            thread: self.thread,
            task,
            start_ns,
            end_ns,
        });
        self.busy_ns += end_ns.saturating_sub(start_ns);
        self.tasks += 1;
    }
}

#[derive(Clone)]
pub struct RunStats {
    pub makespan_ns: u64,
    pub release_ns: u64,
    pub spans: Vec<u64>,
    pub busy: Vec<u64>,
    pub tasks: Vec<u64>,
    pub steals: u64,
    pub ops: u64,
    pub recs: Vec<TaskRec>,
}

pub type Work<'a> = dyn Fn(&mut WorkerCtx, usize) + Sync + 'a;

pub fn run_parallel<F>(p: usize, iters_per_ns: f64, worker: F) -> RunStats
where
    F: Fn(u32, &mut WorkerCtx) + Sync,
{
    let p = p.max(1);
    let t_ref = Instant::now();
    let barrier = Barrier::new(p + 1);
    let release = AtomicU64::new(0);
    let mut outs: Vec<WorkerOut> = Vec::with_capacity(p);

    std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(p);
        for i in 0..p as u32 {
            let b = &barrier;
            let w = &worker;
            let t = t_ref;
            handles.push(s.spawn(move || {
                let mut ctx = WorkerCtx {
                    thread: i,
                    t_ref: t,
                    iters_per_ns,
                    busy_ns: 0,
                    tasks: 0,
                    steals: 0,
                    ops: 0,
                    recs: Vec::new(),
                };
                b.wait();
                w(i, &mut ctx);
                let end_ns = t.elapsed().as_nanos() as u64;
                WorkerOut {
                    thread: i,
                    end_ns,
                    busy_ns: ctx.busy_ns,
                    tasks: ctx.tasks,
                    steals: ctx.steals,
                    ops: ctx.ops,
                    recs: ctx.recs,
                }
            }));
        }
        barrier.wait();
        release.store(t_ref.elapsed().as_nanos() as u64, Ordering::SeqCst);
        for h in handles {
            outs.push(h.join().expect("worker panicked"));
        }
    });

    outs.sort_by_key(|o| o.thread);
    let release_ns = release.load(Ordering::SeqCst);
    let makespan_ns = outs
        .iter()
        .map(|o| o.end_ns.saturating_sub(release_ns))
        .max()
        .unwrap_or(0);
    let mut recs = Vec::new();
    for o in &outs {
        recs.extend_from_slice(&o.recs);
    }
    RunStats {
        makespan_ns,
        release_ns,
        spans: outs
            .iter()
            .map(|o| o.end_ns.saturating_sub(release_ns))
            .collect(),
        busy: outs.iter().map(|o| o.busy_ns).collect(),
        tasks: outs.iter().map(|o| o.tasks).collect(),
        steals: outs.iter().map(|o| o.steals).sum(),
        ops: outs.iter().map(|o| o.ops).sum(),
        recs,
    }
}

pub struct Metrics {
    pub makespan_ns: u64,
    pub work_ns: u64,
    pub ideal_ns: u64,
    pub speedup: f64,
    pub efficiency: f64,
    pub lif: f64,
    pub idle_frac: f64,
    pub busy_max: u64,
    pub busy_min: u64,
    pub busy_std: f64,
}

pub fn metrics(s: &RunStats, total_work_ns: u64, p: usize, t1_ns: u64) -> Metrics {
    let p = p.max(1) as f64;
    let ideal_ns = (total_work_ns as f64 / p).ceil() as u64;
    let busy_sum: u64 = s.busy.iter().sum();
    let idle = p * s.makespan_ns as f64 - busy_sum as f64;
    let makespan = s.makespan_ns.max(1) as f64;
    Metrics {
        makespan_ns: s.makespan_ns,
        work_ns: total_work_ns,
        ideal_ns,
        speedup: t1_ns as f64 / makespan,
        efficiency: t1_ns as f64 / (makespan * p),
        lif: makespan / ideal_ns.max(1) as f64,
        idle_frac: (idle / (p * makespan)).max(0.0),
        busy_max: *s.busy.iter().max().unwrap_or(&0),
        busy_min: *s.busy.iter().min().unwrap_or(&0),
        busy_std: stddev(&s.busy),
    }
}

pub fn stddev(v: &[u64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let n = v.len() as f64;
    let mean = v.iter().sum::<u64>() as f64 / n;
    (v.iter().map(|x| (*x as f64 - mean).powi(2)).sum::<f64>() / n).sqrt()
}

pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

pub fn iqr(v: &[f64]) -> f64 {
    if v.len() < 4 {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |f: f64| -> f64 {
        let pos = f * (s.len() - 1) as f64;
        let lo = pos.floor() as usize;
        let hi = pos.ceil() as usize;
        if lo == hi {
            s[lo]
        } else {
            s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
        }
    };
    q(0.75) - q(0.25)
}

pub fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn repeat<F: FnMut() -> RunStats>(runs: usize, mut f: F) -> (Vec<RunStats>, usize, f64) {
    let mut v: Vec<RunStats> = Vec::with_capacity(runs);
    for _ in 0..runs {
        v.push(f());
    }
    let ms: Vec<f64> = v.iter().map(|r| r.makespan_ns as f64).collect();
    let med = median(&ms);
    let rep = v
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (a.makespan_ns as f64 - med)
                .abs()
                .partial_cmp(&(b.makespan_ns as f64 - med).abs())
                .unwrap()
        })
        .map(|(i, _)| i)
        .unwrap_or(0);
    (v, rep, med)
}

pub fn run_json(tag: &str, s: &RunStats, extra: &str) -> String {
    run_json_ext(tag, s, extra, true)
}

pub fn run_json_ext(tag: &str, s: &RunStats, extra: &str, include_recs: bool) -> String {
    let th: Vec<String> = (0..s.spans.len())
        .map(|t| {
            format!(
                "{{\"t\":{},\"span_ns\":{},\"busy_ns\":{},\"tasks\":{}}}",
                t, s.spans[t], s.busy[t], s.tasks[t]
            )
        })
        .collect();
    let recs: Vec<String> = if include_recs {
        s.recs
            .iter()
            .map(|r| {
                format!(
                    "{{\"t\":{},\"j\":{},\"s\":{},\"e\":{}}}",
                    r.thread, r.task, r.start_ns, r.end_ns
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    format!(
        "{{\"tag\":\"{}\",\"makespan_ns\":{},\"release_ns\":{},{}\"threads\":[{}],\"recs\":[{}]}}",
        json_escape(tag),
        s.makespan_ns,
        s.release_ns,
        extra,
        th.join(","),
        recs.join(",")
    )
}
