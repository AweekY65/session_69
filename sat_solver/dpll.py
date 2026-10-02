"""A self-contained DPLL SAT solver.

Implemented from scratch on top of the Python standard library only --
no external SAT solver (Z3, MiniSat, ...) or remote service is involved.

The solver implements the classic DPLL procedure:

* unit propagation (boolean constraint propagation),
* pure literal elimination,
* a deterministic branching heuristic (DLIS: pick the variable with the
  most remaining occurrences, try the majority polarity first),
* chronological backtracking with conflict statistics.

Semantics of degenerate inputs:

* an empty clause list is SAT (empty model, trivially extensible),
* an empty clause is UNSAT,
* duplicate literals / tautologies are normalised away by the DIMACS
  parser (:mod:`sat_solver.dimacs`).

The solver is complete: it never guesses UNSAT from a timeout -- UNSAT is
only reported after the whole search tree has been exhausted.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, List, Optional


@dataclass
class SolverStats:
    """Search statistics collected during a DPLL run."""

    decisions: int = 0        # branching points entered
    propagations: int = 0     # unit-propagated literal assignments
    pure_literals: int = 0    # pure-literal assignments
    backtracks: int = 0       # failed branches that were undone
    max_depth: int = 0        # deepest decision level reached

    def __str__(self) -> str:
        return (
            f"decisions={self.decisions} propagations={self.propagations} "
            f"pure_literals={self.pure_literals} backtracks={self.backtracks} "
            f"max_depth={self.max_depth}"
        )


@dataclass
class SolveResult:
    """Outcome of a solve call.

    model maps every variable (1..num_vars) to a bool when sat is
    True; variables left unassigned by the search are filled with
    False (any value satisfies the remaining clauses, so the partial
    model is explicitly extensible).  model is None for UNSAT.
    """

    sat: bool
    model: Optional[Dict[int, bool]]
    stats: SolverStats


def _assign(clauses: List[frozenset], lit: int) -> List[frozenset]:
    """Simplify *clauses* under the assumption that literal *lit* is true.

    Clauses containing *lit* are satisfied and dropped; -lit is
    removed from every other clause.
    """
    out: List[frozenset] = []
    neg = -lit
    for clause in clauses:
        if lit in clause:
            continue
        if neg in clause:
            reduced = clause.difference((neg,))
            out.append(reduced)
        else:
            out.append(clause)
    return out


def _choose_literal(clauses: List[frozenset]) -> int:
    """DLIS branching heuristic.

    Count positive and negative occurrences of every variable in the
    remaining clauses; pick the variable with the highest combined count
    (ties broken by the smallest variable id for determinism) and branch
    on the polarity that occurs most often.
    """
    pos: Dict[int, int] = {}
    neg: Dict[int, int] = {}
    for clause in clauses:
        for lit in clause:
            if lit > 0:
                pos[lit] = pos.get(lit, 0) + 1
            else:
                neg[-lit] = neg.get(-lit, 0) + 1
    best_var = None
    best_score = -1
    for var in set(pos) | set(neg):
        score = pos.get(var, 0) + neg.get(var, 0)
        if score > best_score or (score == best_score and var < best_var):
            best_var = var
            best_score = score
    if pos.get(best_var, 0) >= neg.get(best_var, 0):
        return best_var
    return -best_var


class DpllSolver:
    """DPLL solver for a CNF formula given as a list of literal sets."""

    def __init__(self, num_vars: int, clauses: List[frozenset]):
        self.num_vars = num_vars
        self.clauses = list(clauses)
        self.stats = SolverStats()

    # -- public API ------------------------------------------------------

    def solve(self) -> SolveResult:
        assignment: Dict[int, bool] = {}
        result = self._dpll(self.clauses, assignment, depth=0)
        if result is None:
            return SolveResult(sat=False, model=None, stats=self.stats)
        model = {var: result.get(var, False) for var in range(1, self.num_vars + 1)}
        return SolveResult(sat=True, model=model, stats=self.stats)

    # -- core recursion --------------------------------------------------

    def _dpll(
        self,
        clauses: List[frozenset],
        assignment: Dict[int, bool],
        depth: int,
    ) -> Optional[Dict[int, bool]]:
        if depth > self.stats.max_depth:
            self.stats.max_depth = depth

        # Fixpoint of unit propagation and pure literal elimination.
        while True:
            clauses = self._propagate_units(clauses, assignment)
            if clauses is None:
                return None  # empty clause derived: conflict
            if not clauses:
                return assignment
            pures = self._find_pure_literals(clauses)
            if not pures:
                break
            for lit in pures:
                assignment[abs(lit)] = lit > 0
                self.stats.pure_literals += 1
                clauses = _assign(clauses, lit)
            if not clauses:
                return assignment

        # Branch on the heuristic literal; try the other polarity on
        # backtracking.
        lit = _choose_literal(clauses)
        self.stats.decisions += 1
        for trial in (lit, -lit):
            branch_assignment = dict(assignment)
            branch_assignment[abs(trial)] = trial > 0
            reduced = _assign(clauses, trial)
            result = self._dpll(reduced, branch_assignment, depth + 1)
            if result is not None:
                return result
            self.stats.backtracks += 1
        return None

    def _propagate_units(
        self,
        clauses: List[frozenset],
        assignment: Dict[int, bool],
    ) -> Optional[List[frozenset]]:
        """Repeatedly assign unit clauses until fixpoint.

        Returns the simplified clause list, or None if an empty
        clause (conflict) was derived.
        """
        while True:
            unit = None
            for clause in clauses:
                size = len(clause)
                if size == 0:
                    return None
                if size == 1:
                    unit = next(iter(clause))
                    break
            if unit is None:
                return clauses
            assignment[abs(unit)] = unit > 0
            self.stats.propagations += 1
            clauses = _assign(clauses, unit)

    @staticmethod
    def _find_pure_literals(clauses: List[frozenset]) -> List[int]:
        """Literals whose variable occurs with a single polarity."""
        pos: set = set()
        neg: set = set()
        for clause in clauses:
            for lit in clause:
                if lit > 0:
                    pos.add(lit)
                else:
                    neg.add(-lit)
        pures = [v for v in pos - neg] + [-v for v in neg - pos]
        pures.sort(key=abs)
        return pures


def verify_model(clauses: List[frozenset], model: Dict[int, bool]) -> bool:
    """Return True iff *model* satisfies every clause in *clauses*."""
    for clause in clauses:
        if not any(model.get(abs(lit), False) == (lit > 0) for lit in clause):
            return False
    return True


def solve_dimacs(formula) -> SolveResult:
    """Convenience wrapper: solve a parsed :class:`CnfFormula`."""
    solver = DpllSolver(formula.num_vars, formula.clauses)
    return solver.solve()
