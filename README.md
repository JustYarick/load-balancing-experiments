# Load Balancing

Experimental code accompanying the paper. Requires Rust/Cargo to build.

## Repository structure

```text
src/
  main.rs                — command-line interface for experiments exp1–exp3
  exp1.rs–exp3.rs         — experimental series
  sched.rs               — task scheduling algorithms
  engine.rs              — task execution and timing
  workload.rs            — workload generation
  exact.rs               — exact optimum and lower bounds
  spin.rs, rng.rs        — CPU workload and random number generator
  io.rs, report.rs       — result files and terminal output
  meta.rs, lib.rs        — machine information and library modules
  bin/mandelbrot.rs      — Mandelbrot experiment
  bin/variability.rs     — nominal model of variation between task sets
analysis/                — Python scripts for statistics, figures and data checks
data/original/           — original CSV/JSON measurements and checksums
data/review/             — results of additional analyses
tests/                   — statistical calculation tests
Cargo.toml, Cargo.lock   — Rust project configuration and dependencies
requirements-figures.txt — Python dependencies for plotting
```

## Build and run

```sh
cargo build --release --offline
./target/release/lb help
./target/release/lb exp1
./target/release/lb exp2
./target/release/lb exp3
./target/release/mandelbrot
```
