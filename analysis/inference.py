"""Exact rank tests and summaries of the recorded, unpaired timing samples."""

from __future__ import annotations

import argparse
import csv
import itertools
import json
import math
import statistics as st
from collections import defaultdict
from functools import lru_cache
from pathlib import Path

@lru_cache(maxsize=None)
def rank_sum_distribution(n: int, m: int) -> tuple[int, ...]:
    size = n + m
    highest = n * (2 * size - n + 1) // 2
    counts = [[0] * (highest + 1) for _ in range(n + 1)]
    counts[0][0] = 1
    for rank in range(1, size + 1):
        for chosen in range(min(rank, n), 0, -1):
            for total in range(highest, rank - 1, -1):
                counts[chosen][total] += counts[chosen - 1][total - rank]
    assert sum(counts[n]) == math.comb(size, n)
    return tuple(counts[n])

def mann_whitney_exact(x: list[int], y: list[int]) -> tuple[int, float]:
    if not x or not y:
        raise ValueError("Both samples must contain observations")
    pooled = x + y
    if len(set(pooled)) != len(pooled):
        raise ValueError("Exact no-ties test requires unique pooled observations")
    n, m = len(x), len(y)
    ranks = {value: rank for rank, value in enumerate(sorted(pooled), 1)}
    rank_sum = sum(ranks[value] for value in x)
    offset = n * (n + 1) // 2
    u = rank_sum - offset
    threshold = offset + min(u, n * m - u)
    counts = rank_sum_distribution(n, m)
    p = min(1.0, 2 * sum(counts[:threshold + 1]) / math.comb(n + m, n))
    return u, p

def holm(pvalues: list[float]) -> list[float]:
    adjusted = [0.0] * len(pvalues)
    previous = 0.0
    for rank, index in enumerate(sorted(range(len(pvalues)), key=pvalues.__getitem__)):
        previous = max(previous, (len(pvalues) - rank) * pvalues[index])
        adjusted[index] = min(1.0, previous)
    return adjusted

def read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="") as stream:
        return list(csv.DictReader(stream))

def analyze(source: Path, destination: Path) -> dict:
    families: dict[str, dict[str, list[int]]] = defaultdict(lambda: defaultdict(list))
    for row in read_csv(source / "exp2.csv"):
        families["exp2_" + row["instance"]][row["alg"] + "/" + row["ordering"]].append(int(row["makespan_ns"]))
    for row in read_csv(source / "exp3.csv"):
        families["exp3_" + row["regime"]][row["alg"] + "/" + row["param"]].append(int(row["makespan_ns"]))
    for row in read_csv(source / "mandelbrot.csv"):
        families["mandelbrot"][row["strategy"]].append(int(row["makespan_ns"]))

    report = {
        "method": "Exact two-sided Mann–Whitney U, no ties; Holm within each workload family",
        "alpha": 0.05,
        "interpretation": "Exploratory comparison of distributions under an independence assumption; not an equivalence test or proof of a causal strategy effect.",
        "families": {},
    }
    flattened = []
    summaries = []
    for family, variants in sorted(families.items()):
        comparisons = []
        for (name_a, x), (name_b, y) in itertools.combinations(sorted(variants.items()), 2):
            u, p = mann_whitney_exact(x, y)
            comparisons.append({"a": name_a, "b": name_b, "n_a": len(x), "n_b": len(y), "u": u, "p_raw": p,
                                "median_a_ms": st.median(x) / 1e6, "median_b_ms": st.median(y) / 1e6,
                                "median_b_relative_to_a_pct": 100 * (st.median(y) / st.median(x) - 1)})
        for item, p in zip(comparisons, holm([item["p_raw"] for item in comparisons])):
            item["p_holm"] = p
            item["reject_equal_distributions"] = p < 0.05
            flattened.append({"family": family, **item})
        report["families"][family] = {"number_of_comparisons": len(comparisons), "comparisons": comparisons}
        for variant, values in sorted(variants.items()):
            q1, _, q3 = st.quantiles(values, n=4, method="inclusive")
            summaries.append({"family": family, "variant": variant, "n": len(values), "median_ms": st.median(values) / 1e6,
                              "q1_ms": q1 / 1e6, "q3_ms": q3 / 1e6, "iqr_pct": 100 * (q3 - q1) / st.median(values)})
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "significance.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    for filename, rows in [("significance.csv", flattened), ("timing_summary.csv", summaries)]:
        with (destination / filename).open("w", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=list(rows[0]), lineterminator="\n")
            writer.writeheader()
            writer.writerows(rows)
    return report

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=Path("data/original"))
    parser.add_argument("--output", type=Path, default=Path("generated"))
    args = parser.parse_args()
    result = analyze(args.input, args.output)
    print("Compared", sum(f["number_of_comparisons"] for f in result["families"].values()), "pairs in", len(result["families"]), "families")
