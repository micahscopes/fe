"""RD98: exporting decisions may read records, never construct or rerun proof search."""
from pathlib import Path
import importlib.util
import re
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "solver_budget", ROOT / ".github/check_trait_solver_setup_budget.py"
)
solver_budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(solver_budget)


def forbidden(source):
    code = solver_budget.production_code(source)
    return re.findall(
        r"\b(?:TraitSolveCx|UnificationTable|UnificationTableBase|"
        r"is_goal\w*satisfiable|solve_trait\w*|select_method_candidate|"
        r"resolve_trait_method\w*|lower_impl_trait)\b", code
    )


if __name__ == "__main__":
    assert forbidden("fn export() { TraitSolveCx::new(); }") == ["TraitSolveCx"]
    assert forbidden("fn export() { is_goal_query_satisfiable(); }")
    assert not forbidden('#[cfg(test)] mod tests { fn check() { TraitSolveCx::new(); } }')
    paths = sorted((ROOT / "crates/fe/src").glob("decision_export*.rs"))
    paths += sorted((ROOT / "crates/fe/src/decision_export").glob("**/*.rs"))
    assert paths, "missing decision exporter"
    violations = [(str(path.relative_to(ROOT)), forbidden(path.read_text())) for path in paths]
    violations = [(path, names) for path, names in violations if names]
    if violations:
        for path, names in violations:
            print(f"{path}: export constructs or reruns proof search: {names}", file=sys.stderr)
        raise SystemExit(1)
    print(f"decision export proof-search boundary: {len(paths)} files, 0 violations")
