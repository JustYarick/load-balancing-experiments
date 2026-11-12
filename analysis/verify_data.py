"""Check archival checksums and nominal schedule invariants."""

import csv
import hashlib
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
original = ROOT / "data/original"
for line in (original / "SHA256SUMS").read_text().splitlines():
    expected, name = line.split("  ", 1)
    if hashlib.sha256((original / name).read_bytes()).hexdigest() != expected:
        raise SystemExit(f"Original data changed: {name}")

groups = defaultdict(list)
with (ROOT / "data/review/model_variability.csv").open(newline="") as stream:
    for row in csv.DictReader(stream):
        lb, time, total = (int(row[k]) for k in ["lb_ns", "model_makespan_ns", "nominal_total_ns"])
        assert 0 < lb <= time <= total
        assert abs(float(row["model_ratio_lb"]) - time / lb) < 1e-10
        if row["alg"] == "lpt_greedy":
            assert time / lb <= 2 - 1 / int(row["p"]) + 1e-6
        groups[(row["profile"], row["set"])].append(row)
assert len(groups) == 400
for rows in groups.values():
    assert len(rows) == 7
    for key in ["seed", "nominal_total_ns", "lb_ns"]:
        assert len({r[key] for r in rows}) == 1
print("Original checksums and 2,800 nominal schedule rows verified")
