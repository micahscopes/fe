"""Step 3d: ratchet context edits outside their semantic and solver owners.

Loose premises and direct constructions are reports, not architectural gates.
The edit ceiling is global: moving a call cannot make it disappear. Rust privacy
will close these doors after their consumers use position contexts.
"""
from pathlib import Path
import importlib.util
import os
import re
import subprocess
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


GUARD = ".github/check_solver_premises_budget.py"


def git(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)


# ImplEnv currently lives in template.rs; its move to core/semantic removes
# this temporary owner exemption. Other analysis/semantic files are consumers.
OWNED_MODULES = (
    "crates/hir/src/analysis/ty/trait_resolution/",
    "crates/hir/src/core/semantic/",
)
CONTEXT_OWNER = "crates/hir/src/analysis/semantic/instance/template.rs"
EDIT_CEILING = 27
EDIT_NAMES = "with_assumptions|with_provisions|with_selections|with_witnesses"


def context_edits(source: str) -> int:
    code = solver_budget.production_code(source)
    # Count references as well as calls, including UFCS and turbofish forms.
    edits = len(re.findall(r"(?:\.|::)\s*(?:" + EDIT_NAMES + r")\b", code))
    names = {"ImplEnv"}
    names.update(re.findall(r"\bImplEnv\s+as\s+(\w+)", code))
    names.update(re.findall(r"\btype\s+(\w+)(?:\s*<[^;{}]*>)?\s*=\s*ImplEnv\b", code))
    edits += len(re.findall(
        r"\b(?:" + "|".join(sorted(names)) + r")(?:\s*::\s*<[^;{}]*?>)?\s*::\s*empty\b", code
    ))
    return edits


def pair_signatures(source: str) -> int:
    code = solver_budget.production_code(source)
    count = 0
    for fn in re.finditer(r"\bfn\s+\w+", code):
        start = fn.end()
        angles = 0
        while start < len(code):
            ch = code[start]
            if ch == "<":
                angles += 1
            elif ch == ">" and angles and code[start - 1] != "-":
                angles -= 1
            elif not angles and ch in "({;":
                break
            start += 1
        if start == len(code) or code[start] != "(":
            continue
        end, depth = start + 1, 1
        while end < len(code) and depth:
            depth += (code[end] == "(") - (code[end] == ")")
            end += 1
        params = code[start:end]
        if re.search(r"\bScopeId\b", params) and re.search(r"\bPredicateListId\b", params):
            count += 1
    return count


def check_edit_budget(total: int) -> int:
    ceiling = EDIT_CEILING
    # Reject an uncommitted/committed rise in the ceiling. Old versions had no
    # edit ratchet, so the first retargeting commit establishes it at 38.
    for rev in (os.environ.get("PREMISES_GUARD_BASE", "mb3"), "HEAD", "HEAD^"):
        shown = git("show", f"{rev}:{GUARD}")
        if shown.returncode:
            if rev not in ("HEAD", "HEAD^") and os.environ.get("PREMISES_GUARD_SKIP_BASE") != "1":
                print(f"context edit guard base {rev!r} unavailable", file=sys.stderr)
                return 1
            continue
        previous = re.search(r"^EDIT_CEILING = (\d+)$", shown.stdout, re.M)
        if previous:
            ceiling = min(ceiling, int(previous[1]))
    if EDIT_CEILING > ceiling or total > ceiling:
        print(f"context edits: {total}; ceiling {EDIT_CEILING}, ratchet {ceiling}", file=sys.stderr)
        return 1
    return 0


def main() -> int:
    edit_cases = [
        ("cx.with_assumptions(a).with_provisions(p)", 2),
        ("TraitSolveCx::with_assumptions(cx, a); let f = ImplEnv::with_selections;", 2),
        ("ImplEnv::<'db>::empty(s); cx.with_witnesses::<T>(w)", 2),
        ("use m::ImplEnv as Env; let empty = Env::empty;", 1),
        ("type Env<'a> = ImplEnv<'a>; Env::empty(s)", 1),
        ('// cx.with_assumptions(a)\nlet s = "ImplEnv::empty(s)";', 0),
        ("#[cfg(test)] mod tests { fn f() { cx.with_selections(s); } }", 0),
        ("fn with_assumptions(a: A) {}", 0),
    ]
    signature_cases = [
        ("fn f(s: ScopeId, a: PredicateListId) {}", 1),
        ("fn f<F: Fn(ScopeId) -> PredicateListId>(f: F) {}", 0),
        ("fn f<F: Fn() -> ()>(s: ScopeId, a: PredicateListId) {}", 1),
        ("#[test] fn f(s: ScopeId, a: PredicateListId) {}", 0),
        ("fn f(s: ScopeId) -> PredicateListId {}", 0),
        ("fn f(s: ScopeId, a: Option<PredicateListId>);", 1),
    ]
    assert all(context_edits(s) == n for s, n in edit_cases), "context edit counter controls"
    assert all(pair_signatures(s) == n for s, n in signature_cases), "pair signature controls"
    sources = {
        str(path.relative_to(ROOT)): path.read_text()
        for path in sorted((ROOT / "crates").glob("*/src/**/*.rs"))
    }
    edits = {
        name: context_edits(source) for name, source in sources.items()
        if not name.startswith(OWNED_MODULES) and name != CONTEXT_OWNER
    }
    total = sum(edits.values())
    print(f"context edits outside owner modules: {total} (ceiling {EDIT_CEILING})")
    print(f"scope/premise pair signatures: {sum(pair_signatures(s) for s in sources.values())} (report only)")
    print(f"unpositioned solver premises and built contexts: {sum(inventory().values())} (report only)")
    status = check_edit_budget(total)
    for name, callers in exception_callers(sources).items():
        if callers != [NAMED_EXCEPTIONS[name]]:
            print(f"TraitSolveCx::{name}: unexpected owners {callers}", file=sys.stderr)
            status = 1
    if status:
        for name, count in edits.items():
            if count:
                print(f"  {name}: {count}", file=sys.stderr)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
