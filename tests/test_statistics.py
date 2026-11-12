"""Independent small-sample checks of the inference code."""

import itertools
import math
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "analysis"))
from inference import holm, mann_whitney_exact

class RankTests(unittest.TestCase):
    def test_complete_separation(self):
        self.assertAlmostEqual(mann_whitney_exact([1, 2], [3, 4])[1], 1 / 3)
        self.assertAlmostEqual(mann_whitney_exact(list(range(1, 16)), list(range(16, 31)))[1], 2 / math.comb(30, 15))

    def test_against_exhaustive_permutations(self):
        for x, y in [([1, 4, 7], [2, 3, 5, 6]), ([1, 2, 5, 8], [3, 4, 6, 7])]:
            u, p = mann_whitney_exact(x, y)
            n, m = len(x), len(y)
            limit = min(u, n * m - u)
            possible = [sum(ranks) - n * (n + 1) // 2 for ranks in itertools.combinations(range(1, n + m + 1), n)]
            expected = min(1, 2 * sum(v <= limit for v in possible) / len(possible))
            self.assertAlmostEqual(p, expected)
            self.assertAlmostEqual(p, mann_whitney_exact(y, x)[1])

    def test_ties_are_not_silently_ignored(self):
        with self.assertRaises(ValueError):
            mann_whitney_exact([1, 2], [2, 3])

    def test_holm_step_down(self):
        actual = holm([0.01, 0.04, 0.03])
        for a, b in zip(actual, [0.03, 0.06, 0.06]):
            self.assertAlmostEqual(a, b)

if __name__ == "__main__":
    unittest.main()
