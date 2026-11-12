"""Summarize variability between nominal task sets, not processor run times."""

import argparse
import csv
import statistics as st
from collections import defaultdict
from pathlib import Path

def summarize(source: Path, output: Path) -> list[dict]:
    groups = defaultdict(list)
    with source.open(newline="") as stream:
        for row in csv.DictReader(stream):
            groups[(row["profile"], row["alg"], row["ordering"])].append(float(row["model_ratio_lb"]))
    results = []
    for (profile, algorithm, order), values in sorted(groups.items()):
        deciles = st.quantiles(values, n=10, method="inclusive")
        quartiles = st.quantiles(values, n=4, method="inclusive")
        results.append({"profile": profile, "alg": algorithm, "ordering": order, "sets": len(values),
                        "median_model_ratio_lb": st.median(values), "p10": deciles[0], "p90": deciles[8],
                        "min": min(values), "max": max(values), "iqr_pct": 100 * (quartiles[2] - quartiles[0]) / st.median(values)})
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(results[0]), lineterminator="\n")
        writer.writeheader()
        writer.writerows(results)
    return results

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=Path("data/review/model_variability.csv"))
    parser.add_argument("--output", type=Path, default=Path("generated/model_summary.csv"))
    args = parser.parse_args()
    print("Summarized", len(summarize(args.input, args.output)), "model configurations")
