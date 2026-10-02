"""Automated tests for the local DPLL SAT solver.

Run from the repository root with either of:

    python -m unittest discover -s tests -v
    python -m pytest tests -v
"""

import itertools
import random
import unittest

from sat_solver import (
    DimacsError,
    DpllSolver,
    parse_dimacs,
    verify_model,
)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

def brute_force_sat(num_vars, clauses):
    """Exhaustive oracle: try all 2^n assignments."""
    for values in itertools.product([False, True], repeat=num_vars):
        model = {var: values[var - 1] for var in range(1, num_vars + 1)}
        if verify_model(clauses, model):
            return True
    return False


def solve(num_vars, clauses):
    return DpllSolver(num_vars, clauses).solve()


def pigeonhole(pigeons, holes):
    """PHP(pigeons, holes) as (num_vars, clauses); UNSAT iff pigeons > holes."""
    def var(pigeon, hole):
        return pigeon * holes + hole + 1

    clauses = []
    for p in range(pigeons):
        clauses.append(frozenset(var(p, h) for h in range(holes)))
    for h in range(holes):
        for p1 in range(pigeons):
            for p2 in range(p1 + 1, pigeons):
                clauses.append(frozenset((-var(p1, h), -var(p2, h))))
    return pigeons * holes, clauses


# ---------------------------------------------------------------------------
# basic SAT / UNSAT
# ---------------------------------------------------------------------------

class TestBasicSolving(unittest.TestCase):
    def test_simple_sat_model_verifies(self):
        formula = parse_dimacs("p cnf 3 2\n1 -2 0\n2 3 0\n")
        result = solve(formula.num_vars, formula.clauses)
        self.assertTrue(result.sat)
        self.assertEqual(sorted(result.model), [1, 2, 3])
        self.assertTrue(verify_model(formula.clauses, result.model))

    def test_simple_unsat(self):
        formula = parse_dimacs("p cnf 1 2\n1 0\n-1 0\n")
        result = solve(formula.num_vars, formula.clauses)
        self.assertFalse(result.sat)
        self.assertIsNone(result.model)

    def test_empty_formula_is_sat(self):
        formula = parse_dimacs("p cnf 4 0\n")
        self.assertEqual(formula.clauses, [])
        result = solve(formula.num_vars, formula.clauses)
        self.assertTrue(result.sat)
        self.assertEqual(len(result.model), 4)  # extensible: defaults to False

    def test_empty_clause_is_unsat(self):
        formula = parse_dimacs("p cnf 2 1\n0\n")
        self.assertEqual(formula.clauses, [frozenset()])
        result = solve(formula.num_vars, formula.clauses)
        self.assertFalse(result.sat)

    def test_duplicate_literals_are_deduplicated(self):
        formula = parse_dimacs("p cnf 2 1\n1 1 1 2 2 0\n")
        self.assertEqual(formula.clauses, [frozenset((1, 2))])

    def test_tautology_clause_is_dropped(self):
        formula = parse_dimacs("p cnf 2 2\n1 -1 0\n2 0\n")
        self.assertEqual(formula.clauses, [frozenset((2,))])
        result = solve(formula.num_vars, formula.clauses)
        self.assertTrue(result.sat)
        self.assertTrue(result.model[2])


# ---------------------------------------------------------------------------
# DPLL machinery: propagation, pure literals, decisions, backtracking
# ---------------------------------------------------------------------------

class TestDpllMachinery(unittest.TestCase):
    def test_unit_propagation_chain(self):
        # 1 forces 2, 2 forces 3, 3 forces 4 -- no decisions needed.
        formula = parse_dimacs(
            "p cnf 4 4\n1 0\n-1 2 0\n-2 3 0\n-3 4 0\n"
        )
        result = solve(formula.num_vars, formula.clauses)
        self.assertTrue(result.sat)
        self.assertTrue(all(result.model.values()))
        self.assertGreaterEqual(result.stats.propagations, 4)
        self.assertEqual(result.stats.decisions, 0)
        self.assertEqual(result.stats.backtracks, 0)

    def test_pure_literal_elimination(self):
        # Variable 2 only occurs positively -> pure literal, no decision.
        formula = parse_dimacs("p cnf 2 2\n1 2 0\n-1 2 0\n")
        result = solve(formula.num_vars, formula.clauses)
        self.assertTrue(result.sat)
        self.assertGreaterEqual(result.stats.pure_literals, 1)
        self.assertTrue(verify_model(formula.clauses, result.model))

    def test_deep_backtracking_unsat_pigeonhole(self):
        # PHP(4,3) is UNSAT and forces real search with backtracking.
        num_vars, clauses = pigeonhole(4, 3)
        result = solve(num_vars, clauses)
        self.assertFalse(result.sat)
        self.assertGreater(result.stats.decisions, 0)
        self.assertGreater(result.stats.backtracks, 0)
        self.assertGreater(result.stats.max_depth, 1)

    def test_sat_pigeonhole_boundary(self):
        # PHP(3,3) is SAT: model must place every pigeon.
        num_vars, clauses = pigeonhole(3, 3)
        result = solve(num_vars, clauses)
        self.assertTrue(result.sat)
        self.assertTrue(verify_model(clauses, result.model))

    def test_backtracking_recovers_from_wrong_decision(self):
        # No units and no pure literals, so DLIS branches on variable 1
        # (4 occurrences, positive polarity first).  1=True propagates
        # 2=True and 3=True, which falsifies (-2 v -3); the solver must
        # backtrack and flip the decision to reach the SAT model
        # 1=False, 2=False, 3=False.
        clauses = [
            frozenset((-1, 2)),
            frozenset((-1, 3)),
            frozenset((-2, -3)),
            frozenset((1, -2)),
            frozenset((1, -3)),
        ]
        result = solve(3, clauses)
        self.assertTrue(result.sat)
        self.assertFalse(result.model[1])
        self.assertTrue(verify_model(clauses, result.model))
        self.assertGreater(result.stats.decisions, 0)
        self.assertGreater(result.stats.backtracks, 0)

    def test_unsat_is_not_a_timeout_guess(self):
        # UNSAT must come with an exhausted search: every branch failed.
        num_vars, clauses = pigeonhole(3, 2)
        result = solve(num_vars, clauses)
        self.assertFalse(result.sat)
        self.assertGreater(result.stats.backtracks, 0)


# ---------------------------------------------------------------------------
# DIMACS parsing and validation
# ---------------------------------------------------------------------------

class TestDimacsParsing(unittest.TestCase):
    def test_comments_and_whitespace(self):
        text = (
            "c a comment\n"
            "c\n"
            "p cnf 2 2\n"
            "c another comment\n"
            "1 -2 0\n"
            "2 0\n"
            "c trailing comment\n"
        )
        formula = parse_dimacs(text)
        self.assertEqual(formula.num_vars, 2)
        self.assertEqual(len(formula.clauses), 2)

    def test_clause_spanning_multiple_lines(self):
        formula = parse_dimacs("p cnf 3 1\n1\n2\n3 0\n")
        self.assertEqual(formula.clauses, [frozenset((1, 2, 3))])

    def test_optional_percent_end_marker(self):
        formula = parse_dimacs("p cnf 1 1\n1 0\n%\n")
        self.assertEqual(formula.clauses, [frozenset((1,))])

    def test_missing_header(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("1 0\n")

    def test_wrong_problem_type(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p sat 1 1\n1 0\n")

    def test_truncated_header(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 3\n")

    def test_non_integer_literal(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 2 1\n1 x 0\n")

    def test_literal_out_of_range(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 2 1\n1 3 0\n")

    def test_clause_not_terminated(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 2 1\n1 2\n")

    def test_too_few_clauses(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 2 3\n1 0\n2 0\n")

    def test_too_many_clauses(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 2 1\n1 0\n2 0\n")

    def test_garbage_after_clauses(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 1 1\n1 0\nhello\n")

    def test_content_after_percent_marker(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 1 1\n1 0\n%\n1 0\n")

    def test_duplicate_header(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 1 1\np cnf 1 1\n1 0\n")

    def test_percent_marker_before_clauses_complete(self):
        with self.assertRaises(DimacsError):
            parse_dimacs("p cnf 1 2\n1 0\n%\n")


# ---------------------------------------------------------------------------
# random small formulas checked against the exhaustive oracle
# ---------------------------------------------------------------------------

class TestRandomAgainstOracle(unittest.TestCase):
    def test_random_small_formulas(self):
        rng = random.Random(20261002)
        for case in range(300):
            num_vars = rng.randint(1, 5)
            num_clauses = rng.randint(0, 12)
            clauses = []
            for _ in range(num_clauses):
                size = rng.randint(1, 3)
                lits = set()
                for _ in range(size):
                    var = rng.randint(1, num_vars)
                    lits.add(var if rng.random() < 0.5 else -var)
                # skip tautologies: parser would drop them anyway
                if any(-lit in lits for lit in lits):
                    continue
                clauses.append(frozenset(lits))

            expected = brute_force_sat(num_vars, clauses)
            result = solve(num_vars, clauses)
            self.assertEqual(
                result.sat, expected, f"case {case}: clauses={clauses}"
            )
            if result.sat:
                self.assertTrue(
                    verify_model(clauses, result.model),
                    f"case {case}: model does not satisfy {clauses}",
                )


if __name__ == "__main__":
    unittest.main()
