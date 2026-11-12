//! Parallel Mandelbrot workload.

use lbe::engine::{self, RunStats, Work, WorkerCtx};
use lbe::io;
use lbe::report::{self, Align};
use lbe::sched::{self, StealCfg};
use lbe::spin;
use std::sync::Mutex;

#[derive(Clone, Copy)]
struct Cfg {
    img_w: usize,
    img_h: usize,
    iters: u16,
    ssaa: usize,
    art_w: usize,
    art_h: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Strat {
    Block,
    Cyclic,
    Atomic(usize),
    Steal,
}

impl Strat {
    fn name(&self) -> String {
        match self {
            Strat::Block => "static_block".into(),
            Strat::Cyclic => "static_cyclic".into(),
            Strat::Atomic(c) => format!("atomic_chunk{c}"),
            Strat::Steal => "steal".into(),
        }
    }
    fn alg(&self) -> &'static str {
        match self {
            Strat::Block => "static_block",
            Strat::Cyclic => "static_cyclic",
            Strat::Atomic(_) => "atomic",
            Strat::Steal => "steal",
        }
    }
    fn param(&self) -> String {
        match self {
            Strat::Atomic(c) => format!("{c}"),
            _ => "0".into(),
        }
    }
}

fn escape(cx: f64, cy: f64, max_iter: u16) -> u16 {
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    let mut x2 = 0.0f64;
    let mut y2 = 0.0f64;
    let mut i: u16 = 0;
    while i < max_iter && x2 + y2 <= 4.0 {
        y = 2.0 * x * y + cy;
        x = x2 - y2 + cx;
        x2 = x * x;
        y2 = y * y;
        i += 1;
    }
    i
}

fn compute_row(y: usize, cfg: &Cfg) -> Vec<u16> {
    let scale = 3.2 / cfg.img_w as f64;
    let x_center = -0.75f64;
    let y_center = 0.42f64;
    let cy0 = y_center - (y as f64 - cfg.img_h as f64 / 2.0) * scale;
    let mut row = vec![0u16; cfg.img_w];
    let s = cfg.ssaa as f64;
    let step = 1.0 / s;
    for (px, cell) in row.iter_mut().enumerate() {
        let cx0 = x_center + (px as f64 - cfg.img_w as f64 / 2.0) * scale;
        let mut acc = 0u32;
        for sy in 0..cfg.ssaa {
            for sx in 0..cfg.ssaa {
                let cx = cx0 + (sx as f64 + 0.5) * step * scale;
                let cy = cy0 + (sy as f64 + 0.5) * step * scale;
                acc += escape(cx, cy, cfg.iters) as u32;
            }
        }
        *cell = (acc / (cfg.ssaa * cfg.ssaa) as u32).min(cfg.iters as u32) as u16;
    }
    row
}

fn palette(e: u16, max_iter: u16) -> (u8, u8, u8) {
    if e >= max_iter {
        return (10, 10, 28);
    }
    let t = (((e as f64 + 1.0).ln()) / 7.0).clamp(0.0, 1.0);
    let stops: [(f64, (f64, f64, f64)); 4] = [
        (0.00, (10.0, 14.0, 40.0)),
        (0.35, (40.0, 90.0, 200.0)),
        (0.70, (90.0, 205.0, 230.0)),
        (1.00, (250.0, 245.0, 205.0)),
    ];
    let mut seg = 0;
    for i in 0..stops.len() - 1 {
        if t >= stops[i].0 && t <= stops[i + 1].0 {
            seg = i;
            break;
        }
    }
    let (t0, c0) = stops[seg];
    let (t1, c1) = stops[seg + 1];
    let u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
    (
        (c0.0 + (c1.0 - c0.0) * u) as u8,
        (c0.1 + (c1.1 - c0.1) * u) as u8,
        (c0.2 + (c1.2 - c0.2) * u) as u8,
    )
}

fn render_art(img: &[Vec<u16>], cfg: &Cfg) -> String {
    let mut out = String::new();
    let sx = cfg.img_w as f64 / cfg.art_w as f64;
    let sy = cfg.img_h as f64 / cfg.art_h as f64;
    let mut y = 0;
    while y < cfg.art_h {
        for ax in 0..cfg.art_w {
            let ix = (((ax as f64 + 0.5) * sx) as usize).min(cfg.img_w - 1);
            let iy0 = (((y as f64 + 0.5) * sy) as usize).min(cfg.img_h - 1);
            let iy1 = (((y as f64 + 1.5) * sy) as usize).min(cfg.img_h - 1);
            let (r0, g0, b0) = palette(img[iy0][ix], cfg.iters);
            let (r1, g1, b1) = palette(img[iy1][ix], cfg.iters);
            out.push_str(&format!(
                "\x1b[38;2;{r0};{g0};{b0}m\x1b[48;2;{r1};{g1};{b1}m\u{2580}"
            ));
        }
        out.push_str(report::c::RESET);
        out.push('\n');
        y += 2;
    }
    out
}

fn get_usize(args: &[String], name: &str, d: usize) -> usize {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let rest: Vec<String> = argv.iter().skip(1).cloned().collect();
    let quick = has(&rest, "--quick");
    let cfg = if quick {
        Cfg {
            img_w: 160,
            img_h: 68,
            iters: 600,
            ssaa: 2,
            art_w: 80,
            art_h: 34,
        }
    } else {
        Cfg {
            img_w: get_usize(&rest, "--width", 280),
            img_h: get_usize(&rest, "--height", 120),
            iters: get_usize(&rest, "--iters", 2500) as u16,
            ssaa: get_usize(&rest, "--ssaa", 2),
            art_w: get_usize(&rest, "--art-width", 140),
            art_h: get_usize(&rest, "--art-height", 60),
        }
    };
    let p = get_usize(&rest, "--p", 6);
    let runs = get_usize(&rest, "--runs", if quick { 3 } else { 9 });
    let ipns = spin::calibrate_iters_per_ns();
    let _ = ipns;

    let out_csv = argv
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| argv.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "results/raw/mandelbrot.csv".into());
    let out_detail = argv
        .iter()
        .position(|a| a == "--detail")
        .and_then(|i| argv.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "results/raw/mandelbrot_detail.json".into());

    let strats = [
        Strat::Block,
        Strat::Cyclic,
        Strat::Atomic(1),
        Strat::Atomic(4),
        Strat::Steal,
    ];

    let mut images: Vec<(String, Vec<Vec<u16>>)> = Vec::new();
    let mut row_costs: Vec<(String, Vec<u64>)> = Vec::new();
    let mut csv_rows: Vec<Vec<String>> = Vec::new();
    let mut detail: Vec<String> = Vec::new();

    let mut t1s = Vec::new();
    {
        let rows: Vec<Mutex<Vec<u16>>> = (0..cfg.img_h).map(|_| Mutex::new(Vec::new())).collect();
        let work = |ctx: &mut WorkerCtx, j: usize| {
            ctx.do_task_with(j as u32, || {
                let r = compute_row(j, &cfg);
                *rows[j].lock().unwrap() = r;
            });
        };
        let wk: &Work = &work;
        for _ in 0..runs.max(3) {
            t1s.push(sched::static_block(1, ipns, cfg.img_h, wk).makespan_ns as f64);
        }
    }
    let t1 = engine::median(&t1s);

    let mut table_rows: Vec<Vec<String>> = Vec::new();

    for st in strats {
        let rows_buf: Vec<Mutex<Vec<u16>>> = (0..cfg.img_h)
            .map(|_| Mutex::new(vec![0u16; cfg.img_w]))
            .collect();
        let work = |ctx: &mut WorkerCtx, j: usize| {
            ctx.do_task_with(j as u32, || {
                let r = compute_row(j, &cfg);
                *rows_buf[j].lock().unwrap() = r;
            });
        };
        let wk: &Work = &work;

        let mut collected: Vec<(RunStats, Vec<Vec<u16>>)> = Vec::new();
        for _ in 0..runs {
            let s = match st {
                Strat::Block => sched::static_block(p, ipns, cfg.img_h, wk),
                Strat::Cyclic => sched::static_cyclic(p, ipns, cfg.img_h, wk),
                Strat::Atomic(c) => sched::atomic_dyn(p, ipns, cfg.img_h, c, wk),
                Strat::Steal => sched::work_steal(
                    p,
                    ipns,
                    cfg.img_h,
                    0x1234_5678,
                    &StealCfg {
                        spread: true,
                        steal_half: true,
                    },
                    wk,
                ),
            };
            let img: Vec<Vec<u16>> = rows_buf.iter().map(|m| m.lock().unwrap().clone()).collect();
            collected.push((s, img));
        }
        let ms: Vec<f64> = collected
            .iter()
            .map(|(s, _)| s.makespan_ns as f64)
            .collect();
        let med = engine::median(&ms);
        let idx = collected
            .iter()
            .enumerate()
            .min_by(|(_, (a, _)), (_, (b, _))| {
                (a.makespan_ns as f64 - med)
                    .abs()
                    .partial_cmp(&(b.makespan_ns as f64 - med).abs())
                    .unwrap()
            })
            .map(|(i, _)| i)
            .unwrap();
        let (s, img) = &collected[idx];

        let row_cost: Vec<u64> = img
            .iter()
            .map(|r| r.iter().map(|x| *x as u64).sum::<u64>())
            .collect();
        let cmax = *row_cost.iter().max().unwrap_or(&1);
        let cmin = (*row_cost.iter().min().unwrap_or(&1)).max(1);
        let m = engine::metrics(s, t1 as u64, p, t1 as u64);

        for (ri, (r, _)) in collected.iter().enumerate() {
            let mm = engine::metrics(r, t1 as u64, p, t1 as u64);
            csv_rows.push(vec![
                st.name(),
                st.alg().to_string(),
                st.param(),
                format!("{}", p),
                format!("{ri}"),
                format!("{}", mm.makespan_ns),
                format!("{:.0}", t1),
                format!("{:.4}", mm.speedup),
                format!("{:.4}", mm.efficiency),
                format!("{:.4}", mm.idle_frac),
                format!("{}", cmax),
                format!("{}", cmin),
            ]);
        }

        table_rows.push(vec![
            format!(
                "{}{}{}",
                if matches!(st, Strat::Block) {
                    report::c::red()
                } else {
                    report::c::green()
                },
                st.name(),
                report::c::RESET
            ),
            format!("{:.1}", m.makespan_ns as f64 / 1e6),
            format!(
                "{}{:.2}{}",
                report::speedup_color(m.speedup, p as f64),
                m.speedup,
                report::c::RESET
            ),
            format!("{:.2}", m.efficiency),
            format!("{:>4.1}", m.idle_frac * 100.0),
            format!("{:.2}", cmax as f64 / cmin as f64),
        ]);

        detail.push(engine::run_json_ext(
            &st.name(),
            s,
            &format!(
                "\"strategy\":\"{}\",\"p\":{},\"pixels\":[{},{},{},{}],\"row_costs\":[{}],",
                st.name(),
                p,
                cfg.img_w,
                cfg.img_h,
                cfg.iters,
                cfg.ssaa,
                row_cost
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            true,
        ));

        images.push((st.name(), img.clone()));
        row_costs.push((st.name(), row_cost));
    }

    let (best_name, best_img) = images
        .iter()
        .min_by_key(|(name, _)| if name == "static_block" { 1 } else { 0 })
        .unwrap();

    print!(
        "{}",
        report::header(
            "Real workload: Mandelbrot escape-time, one task = one image row (data-dependent cost)"
        )
    );
    print!(
        "  image {}x{} px, {} max iterations, {}x{} supersampling, p={}, T_1 (real, 1 thread) = {} ms\n",
        cfg.img_w, cfg.img_h, cfg.iters, cfg.ssaa, cfg.ssaa, p, report::ms(t1)
    );
    if let Some((_, rc)) = row_costs.first() {
        let cmax = *rc.iter().max().unwrap();
        let cmin = *rc.iter().min().unwrap();
        print!(
            "  row cost profile: cheapest row {:.0} units, most expensive row {:.0} units -> the most expensive row is {}{:.1}x{} the cheapest\n",
            cmin as f64, cmax as f64, report::c::yellow(), cmax as f64 / cmin.max(1) as f64, report::c::RESET
        );
        let thr = cmax as f64 * 0.5;
        let heavy: Vec<usize> = rc
            .iter()
            .enumerate()
            .filter(|(_, v)| **v as f64 > thr)
            .map(|(i, _)| i)
            .collect();
        let lo = heavy.first().copied().unwrap_or(0);
        let hi = heavy.last().copied().unwrap_or(0);
        let share: f64 = heavy.iter().map(|i| rc[*i] as f64).sum::<f64>()
            / rc.iter().sum::<u64>().max(1) as f64
            * 100.0;
        print!(
            "  {} rows out of {} hold {:.0}% of all the work (rows {}..{}): that is where the set boundary crosses the image\n",
            heavy.len(), cfg.img_h, share, lo, hi
        );
    }
    print!(
        "{}",
        report::table(
            None,
            &[
                "strategy",
                "T_p ms",
                "speedup",
                "efficiency",
                "idle %",
                "max/min row cost"
            ],
            &table_rows,
            &[Align::L, Align::R, Align::R, Align::R, Align::R, Align::R],
        )
    );

    if !has(&rest, "--no-art") {
        print!(
            "{}",
            report::header("Mandelbrot set rendered by the parallel workers (true colour ANSI)")
        );
        print!("{}", render_art(best_img, &cfg));
        println!(
            "  rendered by strategy {} (p={}), one task = {:.0} µs average",
            best_name,
            p,
            t1 / cfg.img_h as f64 / 1e3
        );
    }

    if let Err(e) = io::write_csv(
        &out_csv,
        &[
            "strategy",
            "alg",
            "param",
            "p",
            "run",
            "makespan_ns",
            "t1_ns",
            "speedup",
            "efficiency",
            "idle_frac",
            "row_cost_max",
            "row_cost_min",
        ],
        &csv_rows,
    ) {
        eprintln!("csv: {e}");
    }
    let json = format!(
        "{{\n  \"workload\": \"mandelbrot\",\n  \"image\": [{}, {}],\n  \"iters\": {},\n  \"ssaa\": {},\n  \"p\": {},\n  \"t1_ns\": {:.0},\n  \"configs\": [\n    {}\n  ]\n}}\n",
        cfg.img_w,
        cfg.img_h,
        cfg.iters,
        cfg.ssaa,
        p,
        t1,
        detail.join(",\n    ")
    );
    if let Err(e) = io::write_text(&out_detail, &json) {
        eprintln!("json: {e}");
    }
}
