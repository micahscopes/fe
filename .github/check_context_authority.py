"""Step 3e: raw context construction and editing stay with their owners.

Rust privacy enforces the boundary. This guard prevents reopening a door or
adding another caller of an authority-sealed solver operation. Loose premise
pairs remain report-only until their header/owner migration.
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
budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(budget)
SEMANTIC = "crates/hir/src/core/semantic/"
SOLVER = "crates/hir/src/analysis/ty/trait_resolution/"
CONTEXT = SEMANTIC + "context.rs"
SOLVER_CONTEXT = SOLVER + "mod.rs"


def impl_methods(source: str, name: str) -> str:
    code = budget.production_code(source)
    bodies = []
    pattern = re.compile(r"\bimpl(?:\s*<[^{}]*>)?\s+" + name + r"(?:\s*<[^{}]*>)?\s*\{")
    for match in pattern.finditer(code):
        depth, end = 1, match.end()
        while depth and end < len(code):
            depth += (code[end] == "{") - (code[end] == "}")
            end += 1
        bodies.append(code[match.end():end - 1])
    return "\n".join(bodies)


def method_visibility(methods: str, name: str) -> list[str]:
    return re.findall(
        r"(?m)^\s*(?:(pub(?:\s*\([^)]*\))?)\s+)?fn\s+" + name + r"(?:\s*<[^{}]*>)?\s*\(",
        methods,
    )


def type_names(code: str, original: str) -> set[str]:
    names = {original}
    names.update(re.findall(r"\b" + original + r"\s+as\s+(\w+)", code))
    aliases = re.findall(r"\btype\s+(\w+)(?:\s*<[^;=]*>)?\s*=\s*([^;]+);", code)
    for _ in range(len(aliases)):
        for name, target in aliases:
            head = re.fullmatch(r"\s*(?:::)?(?:\w+\s*::\s*)*(\w+)(?:\s*<[^;]*>)?\s*", target)
            if head and head[1] in names:
                names.add(name)
    return names


def raw_references(code: str, name: str, methods: str) -> list[str]:
    names = "|".join(sorted(type_names(code, name)))
    return re.findall(
        r"\b(?:" + names + r")\s*(?:::\s*<[^;{}]*?>\s*)?::\s*(" + methods + r")\b",
        code,
    )


def violations(sources: dict[str, str]) -> list[str]:
    errors = []
    owner = sources.get(CONTEXT, "")
    solver = sources.get(SOLVER_CONTEXT, "")
    cx = impl_methods(solver, "TraitSolveCx")
    for method in ["new", "with_assumptions", "with_provisions"]:
        if method_visibility(cx, method) != [""]:
            errors.append(f"TraitSolveCx::{method} must remain private to the solver")
    env = impl_methods(owner, "ImplEnv")
    for method in ["new", "new_with_selections", "new_with_choices", "empty", "with_selections", "with_call_choices"]:
        if method_visibility(env, method) != ["pub(super)"]:
            errors.append(f"ImplEnv::{method} must remain private to the semantic owner")
    if method_visibility(impl_methods(owner, "SolverPremises"), "solver_context"):
        errors.append("SolverPremises must expose bounded questions, not a prover")
    code = budget.production_code(owner)
    if not re.search(r"struct\s+ContextAuthority\s*\(\s*\(\s*\)\s*\)\s*;", code):
        errors.append("ContextAuthority must retain its private unit field")
    if not re.search(r"pub\(super\)\s+fn\s+context_authority\s*\(", code):
        errors.append("only the semantic owner can mint ContextAuthority")
    if not re.search(r"pub\s+struct\s+ImplEnv\s*<[^>]*>\s*\(\s*storage::ImplEnv\s*<[^>]*>\s*\)\s*;", code):
        errors.append("ImplEnv's interned storage must remain private")
    for method in ["at_position", "without_caller_premises", "transported_destination", "fold_at_captured_origin"]:
        signature = re.search(r"fn\s+" + method + r"(?:\s*<[^{}]*>)?\s*\((.*?)\)\s*->", cx, re.S)
        if not signature or "ContextAuthority" not in signature[1]:
            errors.append(f"TraitSolveCx::{method} requires semantic authority")
    for path, source in sources.items():
        code = budget.production_code(source)
        if path.startswith(SEMANTIC) or path.startswith(SOLVER):
            continue
        for name, methods in [
            ("TraitSolveCx", "new|at_position|without_caller_premises|transported_destination|fold_at_captured_origin|with_assumptions|with_provisions"),
            ("ImplEnv", "new|new_with_selections|new_with_choices|empty|with_selections|with_call_choices"),
        ]:
            for method in raw_references(code, name, methods):
                errors.append(f"{path}: raw {name}::{method} outside its owner")
        if re.search(r"(?:\.|::)\s*(?:with_assumptions|with_provisions|with_selections|with_call_choices)\b", code):
            errors.append(f"{path}: raw context editor outside its owner")
    return errors


def main() -> int:
    sources = {
        str(path.relative_to(ROOT)): path.read_text()
        for path in sorted((ROOT / "crates").glob("*/src/**/*.rs"))
    }
    errors = violations(sources)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("context authority: constructors/editors private; premises expose questions; sealed bridges stay owned")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
