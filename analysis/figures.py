"""Rebuild the four manuscript figures from the original measurements."""

from __future__ import annotations

import csv
import json
import statistics as st
from collections import defaultdict
from decimal import Decimal, ROUND_HALF_UP
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / "data/original"
OUT = ROOT / "generated/figures"
OUT.mkdir(parents=True, exist_ok=True)
WIDTH = 170 / 25.4
plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 10,
                     "axes.titlesize": 10, "axes.labelsize": 10,
                     "xtick.labelsize": 10, "ytick.labelsize": 10,
                     "legend.fontsize": 10, "svg.fonttype": "none",
                     "axes.spines.top": False, "axes.spines.right": False})

def rows(name):
    with (RAW / (name + ".csv")).open(newline="") as stream:
        return list(csv.DictReader(stream))

def grouped(name, keys):
    result = defaultdict(list)
    for row in rows(name):
        result[tuple(row[k] for k in keys)].append(row)
    return result

def median(values, key, scale=1):
    return st.median(float(v[key]) * scale for v in values)

def fmt(value, digits=2):
    return format(Decimal(str(value)).quantize(Decimal(10) ** -digits, rounding=ROUND_HALF_UP), f".{digits}f").replace(".", ",")

def save(fig, number):
    for text in fig.findobj(matplotlib.text.Text):
        if text.get_text():
            assert text.get_fontsize() >= 10
    for extension in ["png", "svg"]:
        fig.savefig(OUT / f"figure{number}.{extension}", dpi=300, facecolor="white")
    plt.close(fig)

def static_profiles():
    groups = grouped("exp1", ["dist", "p"])
    ps = [1, 2, 4, 6, 8, 12]
    fig, axes = plt.subplots(1, 2, figsize=(WIDTH, 2.9))
    axes[0].plot(ps, ps, "--", color="#888888")
    labels = [("uniform", "равномерный"), ("exponential", "экспоненциальный"),
              ("bimodal", "бимодальный"), ("one_heavy", "одна тяжёлая")]
    for (profile, label), color in zip(labels, ["#226aa8", "#c76b16", "#7b52a3", "#b73d45"]):
        samples = [[int(r["t1_ns"]) / int(r["makespan_ns"]) for r in groups[(profile, str(p))]] for p in ps]
        center = [st.median(s) for s in samples]
        axes[0].errorbar(ps, center, yerr=[[m - min(s) for m, s in zip(center, samples)],
                                          [max(s) - m for m, s in zip(center, samples)]],
                         marker="o", markersize=3, capsize=2, color=color, label=label)
        axes[1].plot(ps, [st.median(int(r["makespan_ns"]) * p / int(r["t1_ns"]) for r in groups[(profile, str(p))]) for p in ps], "s-", color=color)
    for ax in axes:
        ax.set_xticks(ps)
        ax.set_xlabel("число потоков p")
        ax.axvline(6, color="#888888", linestyle=":")
    axes[0].set_ylabel("ускорение S")
    axes[0].set_title("(а) Ускорение")
    axes[1].set_ylabel("Tₚ / (W / p)")
    axes[1].set_title("(б) Дисбаланс")
    axes[1].set_yscale("log")
    axes[1].set_yticks([1, 2, 4, 8], ["1", "2", "4", "8"])
    axes[1].yaxis.set_minor_formatter(matplotlib.ticker.NullFormatter())
    fig.legend(*axes[0].get_legend_handles_labels(), loc="lower center", ncol=2, frameon=False)
    fig.subplots_adjust(left=.13, right=.98, top=.87, bottom=.34, wspace=.50)
    save(fig, 1)

def task_order():
    groups = grouped("exp2", ["instance", "alg", "ordering"])
    values = []
    for (instance, alg, order), sample in groups.items():
        if instance != "medium":
            continue
        label = "жадный LPT" if alg == "lpt_greedy" else ("блоки" if alg == "static_block" else "цикл") + " / " + {"lpt": "убывание", "ascending": "возрастание", "random": "случайный"}[order]
        values.append((st.median(int(r["makespan_ns"]) / int(r["lb_ns"]) for r in sample), label))
    values.sort()
    fig, ax = plt.subplots(figsize=(WIDTH, 2.3))
    ax.barh(range(len(values)), [v for v, _ in values], color="#226aa8")
    ax.set_yticks(range(len(values)), [label for _, label in values])
    ax.invert_yaxis()
    ax.axvline(1, color="black")
    ax.set_xlim(0, 4.6)
    ax.set_xlabel("отношение Tₚ / LB")
    ax.set_title("n = 64; p = 6; LB = 33,835 мс")
    for i, (value, _) in enumerate(values):
        ax.text(value + .035, i, fmt(value, 3), va="center")
    fig.subplots_adjust(left=.32, right=.99, top=.86, bottom=.26)
    save(fig, 2)

def strategy_label(alg, param):
    if alg == "atomic":
        return "счётчик B=" + param.split("=")[1]
    if alg == "guided":
        return "guided " + param
    if alg == "lpt_greedy":
        return "жадный LPT"
    if alg == "steal":
        return "перехват работы"
    if alg == "static_cyclic":
        return "цикл / убыв."
    return "блоки / " + ("убыв." if param == "lpt" else "случ.")

def strategies():
    groups = grouped("exp3", ["regime", "alg", "param"])
    fig, axes = plt.subplots(2, 1, figsize=(WIDTH, 4.8), gridspec_kw={"height_ratios": [12, 7]})
    for ax, regime, title in zip(axes, ["coarse", "fine"], ["(а) Крупные задачи; n = 64", "(б) Мелкие задачи; n = 4096"]):
        values = []
        for (reg, alg, param), sample in groups.items():
            if reg == regime:
                values.append((st.median(int(r["t1_ns"]) / int(r["makespan_ns"]) for r in sample),
                               strategy_label(alg, param), median(sample, "idle_frac", 100), alg))
        values.sort(reverse=True)
        ax.barh(range(len(values)), [v[0] for v in values], color=["#226aa8" if v[3] in ["atomic", "guided", "steal"] else "#7b52a3" for v in values])
        ax.set_yticks(range(len(values)), [v[1] for v in values])
        ax.invert_yaxis()
        ax.set_title(title)
        ax.set_xlim(0, 8)
        ax.set_xlabel("ускорение S; в скобках — простой, %")
        for i, (speed, _, idle, _) in enumerate(values):
            ax.text(speed + .07, i, f"{fmt(speed)} ({fmt(idle, 1)})", va="center")
    fig.subplots_adjust(left=.28, right=.97, top=.95, bottom=.13, hspace=.48)
    save(fig, 3)

def mandelbrot():
    data = json.loads((RAW / "mandelbrot_detail.json").read_text())
    reference = data["configs"][0]
    costs = reference["row_costs"]
    fig = plt.figure(figsize=(WIDTH, 4.1))
    grid = fig.add_gridspec(2, 2, height_ratios=[1, .95])
    ax = fig.add_subplot(grid[0, :])
    ax.plot(range(len(costs)), costs, color="#226aa8")
    ax.set_yscale("log")
    ax.set_xlabel("номер строки изображения")
    ax.set_ylabel("стоимость, отн. ед.")
    heavy = [c for c in costs if c > max(costs) * .5]
    ax.set_title(f"(а) {len(heavy)} строк из {len(costs)} дают {sum(heavy) / sum(costs):.0%} работы; разброс {max(costs) / min(costs):.0f} раз")
    norm = matplotlib.colors.LogNorm(vmin=min(costs), vmax=max(costs))
    cmap = plt.get_cmap("viridis")
    for col, (name, title) in enumerate([("static_block", "(б) Непрерывные блоки"), ("atomic_chunk1", "(в) Счётчик, B = 1")]):
        config = next(c for c in data["configs"] if c["strategy"] == name)
        ax = fig.add_subplot(grid[1, col])
        for event in config["recs"]:
            ax.barh(event["t"], (event["e"] - event["s"]) / 1e6, left=(event["s"] - config["release_ns"]) / 1e6,
                    color=cmap(norm(costs[event["j"]])), height=.75)
        ax.set_xlim(0, reference["makespan_ns"] / 1e6 * 1.02)
        ax.set_yticks(range(6))
        ax.invert_yaxis()
        ax.set_xlabel("время, мс")
        ax.set_ylabel("поток")
        ax.set_title(title)
        ax.text(.97, .04, f"Tₚ = {fmt(config['makespan_ns'] / 1e6, 1)} мс", transform=ax.transAxes, ha="right", va="bottom")
    fig.subplots_adjust(left=.12, right=.99, top=.92, bottom=.12, hspace=.65, wspace=.38)
    save(fig, 4)

if __name__ == "__main__":
    static_profiles()
    task_order()
    strategies()
    mandelbrot()
    print("Saved four figures to", OUT)
