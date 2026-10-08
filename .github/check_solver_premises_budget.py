"""Step 3: premise-only queries are keyed by a position's premises.

The three named solver contexts that are not program positions (coherence,
signature comparison, implementation search) are each built from one place.

`SolverPremises::from_unpositioned_pair` builds premises that do not come from
a position context, and an `ImplEnv::new*` call outside the context builders
builds a context from parts rather than reading it from a position. Each is a
site still to be given the context of its position. Every file has a main
ceiling (zero unless listed), which only decreases, plus an EXPOSED allowance
for pairs a migration made visible rather than added, each with its reason; so
such sites can be removed but not added or moved to another file.
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


# Solver contexts that are not program positions. Each answers one question,
# so each may be built from exactly one place (its definition aside).
NAMED_EXCEPTIONS = {
    "coherence": "crates/hir/src/analysis/ty/trait_resolution/mod.rs",
    "signature_comparison": "crates/hir/src/analysis/ty/method_cmp.rs",
    "ingot_impl_search": "crates/hir/src/analysis/ty/trait_def.rs",
}


def exception_callers(sources: dict) -> dict:
    found = {name: [] for name in NAMED_EXCEPTIONS}
    for path, source in sources.items():
        code = solver_budget.production_code(source)
        for name in NAMED_EXCEPTIONS:
            for match in re.finditer(r"\b(?:TraitSolveCx|Self)\s*::\s*" + name + r"\s*\(", code):
                found[name].append(path)
    return found


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
    unknown = (set(CEILINGS) | set(EXPOSED)) - set(inventory())
    if unknown:
        print(f"ceilings name missing files: {sorted(unknown)}", file=sys.stderr)
        return 1
    if any(not reason.strip() for _, reason in EXPOSED.values()):
        print("every EXPOSED entry needs a reason", file=sys.stderr)
        return 1
    by_file = inventory()
    status = 0
    sources = {
        str(path.relative_to(ROOT)): path.read_text()
        for path in sorted((ROOT / "crates").glob("*/src/**/*.rs"))
    }
    for name, callers in exception_callers(sources).items():
        if callers != [NAMED_EXCEPTIONS[name]]:
            print(
                f"TraitSolveCx::{name} must be built only in {NAMED_EXCEPTIONS[name]}, "
                f"found in {callers}",
                file=sys.stderr,
            )
            status = 1
    for path, value in by_file.items():
        ceiling = CEILINGS.get(path, 0)
        exposed = EXPOSED.get(path, (0, ""))[0]
        if value > ceiling + exposed:
            print(
                f"{path}: {value} unpositioned premises or built contexts "
                f"(ceiling {ceiling}, exposed {exposed})",
                file=sys.stderr,
            )
            status = 1
        elif exposed and value < ceiling + exposed:
            # Exposed pairs are removed first: lower EXPOSED before CEILINGS.
            print(
                f"{path}: {value} counted, below ceiling {ceiling} + exposed {exposed}; "
                f"lower its EXPOSED entry",
                file=sys.stderr,
            )
            status = 1
    total = sum(by_file.values())
    main = sum(CEILINGS.values())
    exposed = sum(count for count, _ in EXPOSED.values())
    print(
        f"unpositioned solver premises and built contexts: {total} "
        f"(main ceiling {main}, exposed {exposed})"
    )
    return status


# The main per-file ceilings. They may only decrease: a rise in the main count
# fails review even when the guard is edited to allow it.
CEILINGS = {
    "crates/driver/src/elaboration/project.rs": 1,
    "crates/hir/src/analysis/name_resolution/path_resolver.rs": 5,  # EXPOSED: existing path lookup pair (+1), removed by decision 1
    "crates/hir/src/analysis/semantic/borrowck/availability.rs": 1,
    "crates/hir/src/analysis/semantic/borrowck/memory.rs": 1,
    "crates/hir/src/analysis/semantic/capability/handle.rs": 1,
    "crates/hir/src/analysis/semantic/capability/shape.rs": 1,
    "crates/hir/src/analysis/semantic/ctfe/machine.rs": 5,
    "crates/hir/src/analysis/semantic/ctfe/service.rs": 3,
    "crates/hir/src/analysis/semantic/instance/const_ref.rs": 13,
    "crates/hir/src/analysis/semantic/instance/semantic.rs": 7,
    "crates/hir/src/analysis/semantic/lower/body.rs": 4,
    "crates/hir/src/analysis/semantic/lower/local_facts.rs": 1,
    "crates/hir/src/analysis/semantic/lower/pattern.rs": 2,
    "crates/hir/src/analysis/semantic/normalized/layout_plan.rs": 1,
    "crates/hir/src/analysis/semantic/normalized/normalize.rs": 1,
    "crates/hir/src/analysis/semantic/normalized/verify.rs": 2,
    "crates/hir/src/analysis/ty/abi_ty.rs": 1,
    "crates/hir/src/analysis/ty/assoc_const.rs": 1,
    "crates/hir/src/analysis/ty/const_ty.rs": 4,
    "crates/hir/src/analysis/ty/effects.rs": 1,
    "crates/hir/src/analysis/ty/method_cmp.rs": 5,
    "crates/hir/src/analysis/ty/method_table.rs": 1,
    "crates/hir/src/analysis/ty/mod.rs": 1,
    "crates/hir/src/analysis/ty/normalize.rs": 1,
    "crates/hir/src/analysis/ty/provider.rs": 2,
    "crates/hir/src/analysis/ty/trait_def.rs": 2,
    "crates/hir/src/analysis/ty/trait_resolution/mod.rs": 2,
    "crates/hir/src/analysis/ty/trait_resolution/table_solver.rs": 1,
    "crates/hir/src/analysis/ty/ty_check/const_requirements.rs": 2,
    "crates/hir/src/analysis/ty/ty_check/env.rs": 2,
    "crates/hir/src/analysis/ty/ty_check/expr.rs": 9,
    "crates/hir/src/analysis/ty/ty_check/mod.rs": 8,
    "crates/hir/src/analysis/ty/ty_check/pat.rs": 1,
    "crates/hir/src/analysis/ty/ty_lower.rs": 6,
    "crates/hir/src/core/semantic/mod.rs": 3,
    "crates/hir/src/core/semantic/storage_layout.rs": 3,
    "crates/hir/src/diagnosable.rs": 7,
    "crates/mir/src/runtime/lower/boundary.rs": 1,
    "crates/mir/src/runtime/lower/infer.rs": 1,
    "crates/mir/src/runtime/lower/type_info.rs": 2,
}

# Pairs that a migration made visible (a caller that already passed a scope and
# assumptions now names them as transitional premises) rather than added. Each
# entry is (count, reason); entries may only decrease, and are removed before
# the main ceilings when their sites get a position context.
EFFECT_HANDLE = "resolve_effect_handle takes SolverPremises; this caller holds only a pair"
WRITTEN_TYPES = "written-type diagnostics take one premises value; this item caller holds only its pair"
EXPOSED = {
    "crates/hir/src/analysis/semantic/capability/handle.rs": (1, EFFECT_HANDLE),
    "crates/hir/src/analysis/semantic/capability/semantics.rs": (1, EFFECT_HANDLE),
    "crates/hir/src/analysis/semantic/layout_evidence/lower.rs": (1, EFFECT_HANDLE),
    "crates/hir/src/analysis/ty/generic_defaults.rs": (1, WRITTEN_TYPES),
    "crates/hir/src/analysis/ty/provider.rs": (1, EFFECT_HANDLE),
    "crates/hir/src/analysis/ty/ty_lower.rs": (1, EFFECT_HANDLE),
    "crates/hir/src/analysis/ty/type_fn.rs": (1, WRITTEN_TYPES),
    "crates/hir/src/core/semantic/mod.rs": (6, WRITTEN_TYPES),
    "crates/hir/src/diagnosable.rs": (4, WRITTEN_TYPES),
}

if __name__ == "__main__":
    raise SystemExit(main())
