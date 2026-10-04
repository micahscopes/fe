"""Keep impl const-condition discharge outside the trait proof engine."""

from pathlib import Path
import re
import sys

sys.dont_write_bytecode = True
from check_trait_solver_setup_budget import production_code


ROOT = Path(__file__).resolve().parents[1]
TY = ROOT / "crates/hir/src/analysis/ty"
SOLVER = TY / "trait_resolution"

# RD74 allows pure, bounded type normalization during matching. These names
# instead cross from structural proof into impl const-condition discharge.
FORBIDDEN_IN_SOLVER = (
    "ty_check::",
    "const_requirements::",
    "check_impl_const_requirements",
    "check_proof_const_requirements",
    "check_pending_const_obligations",
    "pending_proof_const_obligations",
    "discharge_requirement",
    "discharge_selected_requirement",
    "discharge_selected_requirement_query",
    "prove_selected_impl_evidence",
    "eval_const_instance",
    "eval_body_owner_const",
)
DISCHARGE = re.compile(
    r"\b(?:check_impl_const_requirements|check_proof_const_requirements|"
    r"check_pending_const_obligations|pending_proof_const_obligations|"
    r"discharge_requirement|discharge_selected_requirement|"
    r"discharge_selected_requirement_query)\b|\.discharge\s*\("
)


def production(source: str) -> str:
    return production_code(source)


def guarded_entry(source: str, signature: str, guard: str) -> bool:
    source = production(source)
    start = source.find(signature)
    if start < 0:
        return False
    body = source.find("{", start + len(signature))
    if body < 0:
        return False
    first_lines = "\n".join(
        line.split("//", 1)[0] for line in source[body + 1 : body + 200].splitlines()
    )
    return guard in first_lines


def violations(solver_sources: dict[str, str], other_sources: dict[str, str]) -> list[str]:
    errors = []
    for path, source in solver_sources.items():
        for number, line in enumerate(production(source).splitlines(), 1):
            line = line.split("//", 1)[0]
            if any(name in line for name in FORBIDDEN_IN_SOLVER):
                errors.append(f"{path}:{number}: condition discharge in proof search")

    for path, source in other_sources.items():
        if path != "ty_check/const_requirements.rs":
            for number, raw_line in enumerate(production(source).splitlines(), 1):
                line = raw_line.split("//", 1)[0]
                if re.search(r"\bdischarge_selected_requirement_query\b", line):
                    errors.append(f"{path}:{number}: cached discharge bypasses its guarded entry")
        if path.startswith("ty_check/"):
            continue
        bridge_start = source.find("pub fn prove_selected_impl_evidence")
        proof_done = source.find("let proof = candidate_proves_goal_with_conditions", bridge_start)
        bridge_end = source.find("Ok(SelectedImplEvidence", proof_done)
        offset = 0
        for number, raw_line in enumerate(production(source).splitlines(keepends=True), 1):
            line = raw_line.split("//", 1)[0]
            if not DISCHARGE.search(line):
                offset += len(raw_line)
                continue
            # Selected evidence checks returned obligations after the target
            # search has completed. This is the one post-proof bridge.
            allowed = (
                path == "trait_def.rs"
                and "check_proof_const_requirements" in line
                and "super::ty_check::" in line
                and bridge_start >= 0
                and proof_done < offset < bridge_end
            )
            if not allowed:
                errors.append(f"{path}:{number}: condition discharge outside checker")
            offset += len(raw_line)
    return errors


def main() -> int:
    solver_sources = {p.name: p.read_text() for p in SOLVER.glob("*.rs")}
    other_sources = {
        p.relative_to(TY).as_posix(): p.read_text()
        for p in TY.rglob("*.rs")
        if not p.is_relative_to(SOLVER)
    }
    entrypoints = {
        "mod.rs": ("fn is_query_satisfiable<", "fn query_candidate_proves_goal<"),
        "table_solver.rs": ("fn solve<", "fn has_solution<", "impl<'db> ResolutionContext"),
    }
    errors = violations(solver_sources, other_sources)
    for path, names in entrypoints.items():
        for name in names:
            if name not in solver_sources.get(path, ""):
                errors.append(f"{path}: solver entry point changed: {name}")
    solver_adapter = solver_sources.get("table_solver.rs", "")
    for name in ("solve", "has_solution"):
        if not guarded_entry(
            solver_adapter,
            f"pub(super) fn {name}<'db>(",
            "super::enter_proof_search()",
        ):
            errors.append(f"table_solver.rs: {name} lacks the proof-search guard")
    discharge = other_sources.get("ty_check/const_requirements.rs", "")
    if not guarded_entry(
        discharge,
        "fn discharge_selected_requirement<'db>(",
        "trait_resolution::assert_outside_proof_search()",
    ):
        errors.append("const_requirements.rs: obligation discharge lacks the runtime boundary")
    if len(re.findall(r"\bdischarge_selected_requirement_query\s*\(", production(discharge))) != 1:
        errors.append("const_requirements.rs: condition discharge bypasses its guarded entry")
    if len(re.findall(r"\bdischarge_selected_requirement_query\b", production(discharge))) != 2:
        errors.append("const_requirements.rs: cached discharge has an unguarded reference")
    # Check that an old direct evaluator call and an indirect helper edge both
    # fail this gate, so a refactor cannot make the check vacuous.
    if not violations({"table_solver.rs": "check_impl_const_requirements(db);"}, {}):
        errors.append("self-check: old direct solver hook was not rejected")
    if not violations(
        {"table_solver.rs": "helper(db);"},
        {"normalize.rs": "fn helper() { obligation.discharge(db); }"},
    ):
        errors.append("self-check: indirect obligation discharge was not rejected")
    if guarded_entry("fn solve() { work(); }", "fn solve()", "enter_proof_search()"):
        errors.append("self-check: missing runtime guard was not rejected")
    if not guarded_entry(
        "struct A { #[cfg(test)] test_only: bool, } fn solve() { enter_proof_search(); }",
        "fn solve()", "enter_proof_search()",
    ):
        errors.append("self-check: test-only field hid a later guarded production entry")
    if not violations(
        {"table_solver.rs": "struct A { #[cfg(test)] t: bool, } fn after() { check_impl_const_requirements(db); }"},
        {},
    ):
        errors.append("self-check: test-only field hid later production discharge")
    if violations(
        {"table_solver.rs": "#[cfg(test)] mod tests { fn test() { check_impl_const_requirements(db); } } fn after() {}"},
        {},
    ):
        errors.append("self-check: test-only discharge treated as production")
    for name in ("discharge_selected_requirement", "discharge_selected_requirement_query"):
        if not violations({"table_solver.rs": f"{name}(db);"}, {}):
            errors.append(f"self-check: direct {name} hook was not rejected")
        if not violations(
            {"table_solver.rs": "helper(db);"},
            {"normalize.rs": f"fn helper() {{ {name}(db); }}"},
        ):
            errors.append(f"self-check: indirect {name} hook was not rejected")
    if not violations({}, {"ty_check/callable.rs": "discharge_selected_requirement_query(db);"}):
        errors.append("self-check: checker-side cached discharge bypass was not rejected")
    if errors:
        print("trait proof boundary violations:\n" + "\n".join(errors), file=sys.stderr)
        return 1
    print("trait proof boundary: impl conditions remain post-proof")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
