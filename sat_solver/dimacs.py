"""Strict DIMACS CNF parser.

Accepted grammar (a strict subset of the de-facto DIMACS CNF format)::

    <file>    ::= <comment>* <header> <clause>* <trailer>
    <comment> ::= "c" <anything> "\n"
    <header>  ::= "p cnf" <nvars> <nclauses>
    <clause>  ::= <literal>* "0"
    <trailer> ::= <comment>* ["%" <anything>] <comment/whitespace>*

Validation rules (all violations raise :class:`DimacsError`):

* The ``p cnf`` header is mandatory and must appear exactly once, before
  any clause.
* Literals must be non-zero integers with absolute value in ``[1, nvars]``.
* Every clause must be terminated by a literal ``0``; running out of input
  mid-clause is an error.
* The number of clauses must match the header exactly: fewer clauses means
  unexpected end of file; more clauses, or any other non-comment garbage
  after the declared clauses, is an error.  A single optional ``%`` line
  is accepted as an explicit end-of-file marker.

Clause normalisation (deterministic semantics):

* Duplicate literals inside a clause are removed.
* Tautological clauses (containing both ``x`` and ``-x``) are dropped,
  they are always satisfied.
* An empty clause (``0`` on its own) is kept as an empty frozenset and
  makes the formula trivially UNSAT.
* An empty formula (zero clauses) is trivially SAT.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional


class DimacsError(ValueError):
    """Raised when a DIMACS CNF input is malformed."""


@dataclass(frozen=True)
class CnfFormula:
    """A parsed CNF formula.

    ``num_vars`` is the declared number of variables (variables are
    numbered 1..num_vars).  ``clauses`` is a list of frozensets of
    non-zero integers; a positive integer ``i`` denotes the literal
    ``x_i``, a negative integer ``-i`` denotes ``not x_i``.
    """

    num_vars: int
    clauses: List[frozenset]


def _tokenize(text: str) -> List[tuple]:
    """Split *text* into (line_no, token) pairs, dropping comment lines."""
    tokens: List[tuple] = []
    for line_no, raw_line in enumerate(text.splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith("c"):
            # Comment line: 'c' must be followed by whitespace or EOL.
            if len(line) > 1 and not line[1].isspace():
                raise DimacsError(
                    f"line {line_no}: tokens must be separated by whitespace"
                )
            continue
        if line.startswith("%"):
            # Explicit end-of-file marker; recorded as a special token.
            tokens.append((line_no, "%"))
            continue
        for tok in line.split():
            tokens.append((line_no, tok))
    return tokens


def _parse_int(token: str, line_no: int, what: str) -> int:
    try:
        return int(token)
    except ValueError:
        raise DimacsError(
            f"line {line_no}: expected an integer for {what}, got {token!r}"
        ) from None


def _normalize_clause(literals: List[int]) -> Optional[frozenset]:
    """Deduplicate literals; return ``None`` for tautological clauses."""
    seen = set()
    for lit in literals:
        if -lit in seen:
            return None  # tautology: always satisfied, clause is dropped
        seen.add(lit)
    return frozenset(seen)


def parse_dimacs(text: str) -> CnfFormula:
    """Parse DIMACS CNF *text* into a :class:`CnfFormula`.

    Raises :class:`DimacsError` on any malformed input.
    """
    tokens = _tokenize(text)
    pos = 0

    def peek():
        return tokens[pos] if pos < len(tokens) else (None, None)

    # --- header ---------------------------------------------------------
    line_no, tok = peek()
    if tok != "p":
        raise DimacsError("missing header: expected 'p cnf <nvars> <nclauses>'")
    header_line = line_no
    pos += 1
    line_no, tok = peek()
    if tok != "cnf":
        raise DimacsError(
            f"line {line_no}: expected 'cnf' after 'p', got {tok!r}"
        )
    pos += 1
    line_no, tok = peek()
    if tok is None:
        raise DimacsError(f"line {header_line}: truncated header")
    num_vars = _parse_int(tok, line_no, "number of variables")
    pos += 1
    line_no, tok = peek()
    if tok is None:
        raise DimacsError(f"line {header_line}: truncated header")
    num_clauses = _parse_int(tok, line_no, "number of clauses")
    pos += 1
    if num_vars < 0:
        raise DimacsError(f"line {header_line}: negative variable count")
    if num_clauses < 0:
        raise DimacsError(f"line {header_line}: negative clause count")

    # --- clauses --------------------------------------------------------
    # `parsed` counts clauses as declared in the file; `clauses` only
    # keeps the non-tautological normalised ones.
    clauses: List[frozenset] = []
    parsed = 0
    current: List[int] = []
    current_line = None
    while pos < len(tokens) and parsed < num_clauses:
        line_no, tok = tokens[pos]
        pos += 1
        if tok == "%":
            raise DimacsError(
                f"line {line_no}: unexpected end-of-file marker '%': "
                f"expected {num_clauses} clauses, got {parsed}"
            )
        if tok == "p":
            raise DimacsError(f"line {line_no}: duplicate header")
        lit = _parse_int(tok, line_no, "literal")
        if lit == 0:
            parsed += 1
            clause = _normalize_clause(current)
            if clause is not None:  # drop tautologies
                clauses.append(clause)
            current = []
            current_line = None
        else:
            if abs(lit) > num_vars:
                raise DimacsError(
                    f"line {line_no}: literal {lit} out of range "
                    f"(declared variables: 1..{num_vars})"
                )
            if current_line is None:
                current_line = line_no
            current.append(lit)

    if current:
        raise DimacsError(
            f"line {current_line}: clause not terminated by 0 "
            "(unexpected end of file)"
        )
    if parsed < num_clauses:
        raise DimacsError(
            f"unexpected end of file: expected {num_clauses} clauses, "
            f"got {parsed}"
        )

    # --- trailer: only an optional '%' marker is allowed ----------------
    while pos < len(tokens):
        line_no, tok = tokens[pos]
        pos += 1
        if tok == "%":
            if pos < len(tokens):
                extra_line, extra_tok = tokens[pos]
                raise DimacsError(
                    f"line {extra_line}: unexpected content {extra_tok!r} "
                    "after end-of-file marker '%'"
                )
            break
        raise DimacsError(
            f"line {line_no}: unexpected content {tok!r} after the "
            f"declared {num_clauses} clauses"
        )

    return CnfFormula(num_vars=num_vars, clauses=clauses)


def parse_dimacs_file(path: str) -> CnfFormula:
    """Read *path* and parse it with :func:`parse_dimacs`."""
    with open(path, "r", encoding="utf-8") as handle:
        return parse_dimacs(handle.read())
