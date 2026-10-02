"""Command line interface: `python -m sat_solver <file.cnf>`.

Exit codes follow the SAT competition convention: 10 for SAT, 20 for
UNSAT, 1 for malformed input.
"""

import sys

from .dimacs import DimacsError, parse_dimacs_file
from .dpll import DpllSolver, verify_model


def main(argv=None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) != 1:
        print("usage: python -m sat_solver <file.cnf>", file=sys.stderr)
        return 2
    try:
        formula = parse_dimacs_file(argv[0])
    except DimacsError as exc:
        print(f"c DIMACS error: {exc}", file=sys.stderr)
        return 1

    solver = DpllSolver(formula.num_vars, formula.clauses)
    result = solver.solve()

    if result.sat:
        assert verify_model(formula.clauses, result.model)
        print("s SATISFIABLE")
        lits = " ".join(
            str(var if val else -var) for var, val in sorted(result.model.items())
        )
        print(f"v {lits} 0" if lits else "v 0")
    else:
        print("s UNSATISFIABLE")
    print(f"c stats: {result.stats}")
    return 10 if result.sat else 20


if __name__ == "__main__":
    sys.exit(main())
