"""Step 3: premise-only queries are keyed by a position's premises.

`SolverPremises::from_unpositioned_pair` builds premises that do not come from
a position context, and a raw `ImplEnv::new(` builds a context from a bare
pair. Each is a site still to be given the context of its position, so their
count may only decrease. A file that already reads a position context
(`context_at(`) may not gain transitional callers: it should pass the context
it has.
"""
from pathlib import Path
import importlib.util
import re
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
BASELINE = 128  # transitional premises and raw ImplEnv::new; may only decrease
spec = importlib.util.spec_from_file_location(
    "solver_budget", ROOT / ".github/check_trait_solver_setup_budget.py"
)
solver_budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(solver_budget)

TRANSITIONAL = re.compile(r"\bSolverPremises\s*::\s*from_unpositioned_pair\s*\(")
RAW_CONTEXT = re.compile(r"\bImplEnv\s*::\s*new\s*\(")
CONTEXT_READER = re.compile(r"\bcontext_at\s*\(")


def count(source: str) -> tuple[int, int, bool]:
    code = solver_budget.production_code(source)
    return (
        len(TRANSITIONAL.findall(code)),
        len(RAW_CONTEXT.findall(code)),
        bool(CONTEXT_READER.search(code)),
    )


def inventory() -> dict:
    return {
        path.relative_to(ROOT): count(path.read_text())
        for path in sorted((ROOT / "crates").glob("*/src/**/*.rs"))
    }


def main() -> int:
    cases = [
        ("SolverPremises::from_unpositioned_pair(scope, a)", (1, 0, False)),
        ("SolverPremises :: from_unpositioned_pair (scope, a)", (1, 0, False)),
        ("pub fn from_unpositioned_pair(scope: S, a: A) -> Self", (0, 0, False)),
        ("ImplEnv::new(db, scope, a, vec![]); ImplEnv::new_with_choices(db)", (0, 1, False)),
        ("// SolverPremises::from_unpositioned_pair(s, a)\nlet c = context_at(db, s, None);", (0, 0, True)),
        ("#[cfg(test)] mod t { fn f() { ImplEnv::new(db, s, a, v); } }", (0, 0, False)),
    ]
    if any(count(source) != expected for source, expected in cases):
        raise AssertionError("premises counter self-check failed")
    by_file = inventory()
    total = sum(t + r for t, r, _ in by_file.values())
    status = 0
    print(f"unpositioned solver premises: {total} (ceiling {BASELINE})")
    if total > BASELINE:
        for path, (t, r, _) in by_file.items():
            if t or r:
                print(f"  {path}: {t} transitional, {r} raw ImplEnv::new", file=sys.stderr)
        status = 1
    for path, (t, _, reads_context) in by_file.items():
        allowed = MIXED.get(str(path), 0)
        if reads_context and t > allowed:
            print(
                f"{path} reads a position context but has {t} transitional premise "
                f"constructions (allowed {allowed}); pass the context instead",
                file=sys.stderr,
            )
            status = 1
    return status


# Files that read a position context and still had transitional callers when
# this guard was added. These counts may only decrease.
MIXED = {
    "crates/hir/src/analysis/semantic/ctfe/machine.rs": 3,
    "crates/hir/src/analysis/ty/mod.rs": 1,
    "crates/hir/src/analysis/ty/ty_check/const_requirements.rs": 2,
    "crates/hir/src/analysis/ty/ty_check/mod.rs": 13,
    "crates/hir/src/core/semantic/mod.rs": 3,
    "crates/hir/src/core/semantic/storage_layout.rs": 5,
    "crates/hir/src/diagnosable.rs": 6,
}

if __name__ == "__main__":
    raise SystemExit(main())
