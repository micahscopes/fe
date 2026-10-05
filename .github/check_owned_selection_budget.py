"""PLAN-v2: owned trait-selection context fields may only decrease.

Counts collection fields containing SelectedApplication, and the legacy
opened_provisions evidence list. Actual individual decisions are not context
copies. No Frame/evidence exemptions exist yet; introduce audited exemptions
with those authorities, never by merely naming an arbitrary struct Frame.
"""
from pathlib import Path
import importlib.util
import re
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
BASELINE = 2
spec = importlib.util.spec_from_file_location(
    "solver_budget", ROOT / ".github/check_trait_solver_setup_budget.py"
)
solver_budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(solver_budget)


def fields(source):
    code = solver_budget.production_code(source)
    names = {"SelectedApplication"}
    names.update(re.findall(r"\bSelectedApplication\s+as\s+(\w+)", code))
    aliases = dict(re.findall(r"\btype\s+(\w+)(?:\s*<[^;=]*>)?\s*=\s*([^;]+);", code))

    def expanded(typ):
        for _ in range(len(aliases)):
            changed = False
            for name, replacement in aliases.items():
                if re.search(r"\b" + re.escape(name) + r"\b", typ):
                    new = re.sub(r"\b" + re.escape(name) + r"\b", replacement, typ)
                    changed |= new != typ
                    typ = new
            if not changed:
                break
        return typ

    # Context copies must not escape the inventory by changing record shape.
    # Existing enum answers are scalar decisions, not owned context lists.
    # New enum/tuple context storage requires extending this counter explicitly.
    for match in re.finditer(r"\benum\s+(\w+)[^{;]*\{|\bstruct\s+(\w+)(?:\s*<[^;{}]*?>)?\s*\(", code):
        opening = code[match.end() - 1]
        closing = "}" if opening == "{" else ")"
        depth, end = 1, match.end()
        while end < len(code) and depth:
            depth += (code[end] == opening) - (code[end] == closing)
            end += 1
        body = expanded(code[match.end():end - 1])
        if (match[1] or match[2]) == "Callable" and re.search(r"\bImplEnv\b", body):
            raise ValueError("Callable context storage is retired; read the body position record")
        if re.search(r"\b(?:Vec|Arc|Box|Map|FxHashMap|IndexMap|HashMap|SmallVec|VecDeque|BTreeMap|HashSet)\b|\[", body) and any(
            re.search(r"\b" + name + r"\b", body) for name in names
        ):
            raise ValueError("enum/tuple owned selection context needs explicit inventory coverage")

    for match in re.finditer(r"\bstruct\s+(\w+)[^{;]*\{", code):
        start = match.end()
        depth, end = 1, start
        while end < len(code) and depth:
            depth += (code[end] == "{") - (code[end] == "}")
            end += 1
        if depth:
            raise ValueError("unbalanced struct")
        segment, nesting = start, 0
        for i in range(start, end):
            ch = code[i]
            if i == end - 1 or ch == "," and nesting == 0:
                part = code[segment:i]
                field = re.search(r"(\w+)\s*:(?!:)\s*(.*)", part, re.S)
                if field:
                    typ = expanded(field[2].strip())
                    if match[1] == "Callable" and re.search(r"\bImplEnv\b", typ):
                        raise ValueError("Callable context storage is retired; read the body position record")
                    if match[1] == "Decisions" and re.search(r"\bImplEnv\b", typ) and re.search(
                        r"\b(?:SecondaryMap|FxHashMap|IndexMap|HashMap|BTreeMap|Vec)\b", typ
                    ):
                        raise ValueError("per-position Decisions context storage is retired; read the root input and opening records")
                    collection = re.search(
                        r"\b(?:Vec|Arc|Box|SecondaryMap|FxHashMap|IndexMap|HashMap|SmallVec|VecDeque|BTreeMap|HashSet)\b|\[", typ
                    )
                    selected = any(re.search(r"\b" + name + r"\b", typ) for name in names)
                    evidence = field[1] == "opened_provisions" and "TraitInstId" in typ
                    if not typ.startswith("&") and collection and (selected or evidence):
                        yield {
                            "owner": match[1], "field": field[1],
                            "line": source.count("\n", 0, segment + field.start()) + 1,
                        }
                segment = i + 1
            elif ch in "<([{":
                nesting += 1
            elif ch in ">)]}":
                nesting -= 1


def self_check():
    cases = [
        ("struct A { renamed: Vec<SelectedApplication<'db>> }", 1),
        ("struct A { x: SecondaryMap<K, Option<Vec<SelectedApplication<'db>>>>, }", 1),
        ("use x::SelectedApplication as Pick; struct A { x: Vec<Pick>, }", 1),
        ("type Picks = Vec<SelectedApplication<'db>>; struct A { x: Picks, }", 1),
        ("struct A { x: &Vec<SelectedApplication<'db>>, }", 0),
        ("struct A { selections: FxHashMap<(V, usize), usize>, }", 0),
        ("fn f(x: Vec<SelectedApplication<'db>>) {}", 0),
        ('// struct A { x: Vec<SelectedApplication>, }\nconst S: &str = "struct A { x: Vec<SelectedApplication>, }";', 0),
        ("#[cfg(test)] mod tests { struct A { x: Vec<SelectedApplication>, } } struct B { x: Vec<SelectedApplication>, }", 1),
        ("struct Frame { x: Vec<SelectedApplication>, }", 1),
        ("struct A { opened_provisions: Vec<(ExprId, TraitInstId<'db>)>, }", 1),
    ]
    for source, expected in cases:
        assert len(list(fields(source))) == expected, source
    for source in [
        "struct Callable { context: ImplEnv<'db>, }",
        "type Alias = ImplEnv<'db>; struct Callable { renamed: Option<Alias>, }",
        "struct Callable(ImplEnv<'db>);",
    ]:
        try:
            list(fields(source))
        except ValueError as error:
            assert "Callable context storage is retired" in str(error)
        else:
            raise AssertionError("retired Callable context restored: " + source)
    for source in [
        "struct Decisions { contexts: SecondaryMap<ExprId, Option<ImplEnv>>, }",
        "type Contexts = HashMap<ExprId, ImplEnv>; struct Decisions { renamed: Contexts, }",
    ]:
        try:
            list(fields(source))
        except ValueError:
            pass
        else:
            raise AssertionError("retired position-context map restored: " + source)
    for source in ["struct A(Vec<SelectedApplication>);", "enum A { B(Vec<SelectedApplication>) }"]:
        try:
            list(fields(source))
        except ValueError:
            pass
        else:
            raise AssertionError("record-shape bypass accepted: " + source)


def main():
    self_check()
    rows = []
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        for row in fields(path.read_text()):
            rows.append({"path": str(path.relative_to(ROOT)), **row})
    print(f"owned trait-selection context fields: {len(rows)} (ceiling {BASELINE})")
    if len(rows) > BASELINE:
        for row in rows:
            print(f"  {row['path']}:{row['line']} {row['owner']}.{row['field']}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
