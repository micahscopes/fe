"""Step 3: premise-only queries are keyed by a position's premises.

`SolverPremises::from_unpositioned_pair` builds premises that do not come from
a position context, and an `ImplEnv::new*` call outside the context builders
builds a context from parts rather than reading it from a position. Each is a
site still to be given the context of its position. Every file has a ceiling
(zero unless listed below), so such sites can be removed but not added or
moved to another file.
"""
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

TRANSITIONAL = re.compile(r"\bSolverPremises\s*::\s*from_unpositioned_pair\s*\(")
RAW_CONTEXT = re.compile(r"\bImplEnv\s*::\s*new\s*\(")
BUILT_CONTEXT = re.compile(r"\bImplEnv\s*::\s*new_with_(?:selections|choices)\s*\(")
# The position context reader and the context type itself build contexts.
CONTEXT_BUILDERS = {
    "crates/hir/src/core/semantic/provision.rs",
    "crates/hir/src/analysis/semantic/instance/template.rs",
}


def count(source: str, builder: bool = False) -> tuple[int, int, int]:
    code = solver_budget.production_code(source)
    return (
        len(TRANSITIONAL.findall(code)),
        len(RAW_CONTEXT.findall(code)),
        0 if builder else len(BUILT_CONTEXT.findall(code)),
    )


def inventory() -> dict:
    result = {}
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        name = str(path.relative_to(ROOT))
        result[name] = sum(count(path.read_text(), name in CONTEXT_BUILDERS))
    return result


def main() -> int:
    cases = [
        ("SolverPremises::from_unpositioned_pair(scope, a)", False, (1, 0, 0)),
        ("SolverPremises :: from_unpositioned_pair (scope, a)", False, (1, 0, 0)),
        ("pub fn from_unpositioned_pair(scope: S, a: A) -> Self", False, (0, 0, 0)),
        ("ImplEnv::new(db, s, a, v); ImplEnv::new_with_choices(db)", False, (0, 1, 1)),
        ("ImplEnv::new_with_selections(db, s, a, w, v)", True, (0, 0, 0)),
        ("// SolverPremises::from_unpositioned_pair(s, a)\nImplEnv::new_with_selections(x)", False, (0, 0, 1)),
        ("#[cfg(test)] mod t { fn f() { ImplEnv::new(db, s, a, v); } }", False, (0, 0, 0)),
    ]
    if any(count(source, builder) != expected for source, builder, expected in cases):
        raise AssertionError("premises counter self-check failed")
    unknown = set(CEILINGS) - set(inventory())
    if unknown:
        print(f"ceilings name missing files: {sorted(unknown)}", file=sys.stderr)
        return 1
    by_file = inventory()
    status = 0
    for path, value in by_file.items():
        ceiling = CEILINGS.get(path, 0)
        if value > ceiling:
            print(
                f"{path}: {value} unpositioned premises or built contexts (ceiling {ceiling})",
                file=sys.stderr,
            )
            status = 1
    total = sum(by_file.values())
    print(f"unpositioned solver premises and built contexts: {total} (ceiling {sum(CEILINGS.values())})")
    return status


# Per-file ceilings when this guard was added; they may only decrease.
CEILINGS = {
    "crates/driver/src/elaboration/project.rs": 1,
    "crates/hir/src/analysis/name_resolution/path_resolver.rs": 2,
    "crates/hir/src/analysis/semantic/borrowck/availability.rs": 1,
    "crates/hir/src/analysis/semantic/borrowck/memory.rs": 1,
    "crates/hir/src/analysis/semantic/capability/shape.rs": 1,
    "crates/hir/src/analysis/semantic/ctfe/machine.rs": 5,
    "crates/hir/src/analysis/semantic/ctfe/service.rs": 2,
    "crates/hir/src/analysis/semantic/instance/const_ref.rs": 13,
    "crates/hir/src/analysis/semantic/instance/semantic.rs": 9,
    "crates/hir/src/analysis/semantic/lower/body.rs": 7,
    "crates/hir/src/analysis/semantic/lower/local_facts.rs": 2,
    "crates/hir/src/analysis/semantic/lower/pattern.rs": 4,
    "crates/hir/src/analysis/semantic/normalized/layout_plan.rs": 1,
    "crates/hir/src/analysis/semantic/normalized/normalize.rs": 1,
    "crates/hir/src/analysis/semantic/normalized/verify.rs": 2,
    "crates/hir/src/analysis/ty/abi_ty.rs": 1,
    "crates/hir/src/analysis/ty/const_ty.rs": 3,
    "crates/hir/src/analysis/ty/effects.rs": 3,
    "crates/hir/src/analysis/ty/method_cmp.rs": 9,
    "crates/hir/src/analysis/ty/method_table.rs": 2,
    "crates/hir/src/analysis/ty/mod.rs": 1,
    "crates/hir/src/analysis/ty/normalize.rs": 1,
    "crates/hir/src/analysis/ty/provider.rs": 2,
    "crates/hir/src/analysis/ty/trait_def.rs": 3,
    "crates/hir/src/analysis/ty/trait_resolution/mod.rs": 2,
    "crates/hir/src/analysis/ty/trait_resolution/table_solver.rs": 1,
    "crates/hir/src/analysis/ty/ty_check/callable.rs": 1,
    "crates/hir/src/analysis/ty/ty_check/const_requirements.rs": 2,
    "crates/hir/src/analysis/ty/ty_check/env.rs": 2,
    "crates/hir/src/analysis/ty/ty_check/expr.rs": 8,
    "crates/hir/src/analysis/ty/ty_check/mod.rs": 13,
    "crates/hir/src/analysis/ty/ty_error.rs": 2,
    "crates/hir/src/analysis/ty/ty_lower.rs": 4,
    "crates/hir/src/core/semantic/mod.rs": 3,
    "crates/hir/src/core/semantic/storage_layout.rs": 5,
    "crates/hir/src/diagnosable.rs": 6,
    "crates/mir/src/runtime/lower/boundary.rs": 1,
    "crates/mir/src/runtime/lower/infer.rs": 1,
    "crates/mir/src/runtime/lower/type_info.rs": 2,
}

if __name__ == "__main__":
    raise SystemExit(main())
