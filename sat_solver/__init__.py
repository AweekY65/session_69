"""A self-contained local SAT solver (DPLL) with a strict DIMACS parser.

Everything runs locally: CNF inputs, solving state and models live only
in memory or in local files.  No external solver or service is used.
"""

from .dimacs import CnfFormula, DimacsError, parse_dimacs, parse_dimacs_file
from .dpll import DpllSolver, SolveResult, SolverStats, solve_dimacs, verify_model

__all__ = [
    "CnfFormula",
    "DimacsError",
    "parse_dimacs",
    "parse_dimacs_file",
    "DpllSolver",
    "SolveResult",
    "SolverStats",
    "solve_dimacs",
    "verify_model",
]
