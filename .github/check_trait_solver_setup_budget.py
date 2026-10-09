"""RD102: direct production solver entry constructions must only decrease."""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
SOLVER = ROOT / "crates/hir/src/analysis/ty/trait_resolution"
BASELINE = 14  # Approved H1 premise normalization replaces one direct setup.
TYPES = ("TraitSolveCx", "CanonicalGoalQuery", "TraitSolverQuery")
RAW = re.compile(r'(?:br|cr|r)(#*)"')
CHAR = re.compile(r"'(?:\\(?:u\{[^}]*\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'")


def code_only(source: str) -> str:
    """Mask comments and literals, preserving offsets for diagnostic locations."""
    out = list(source)
    i = 0
    while i < len(source):
        start = i
        if source.startswith("//", i):
            end = source.find("\n", i)
            i = len(source) if end < 0 else end
        elif source.startswith("/*", i):
            i += 2
            depth = 1
            while i < len(source) and depth:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
        elif raw := RAW.match(source, i):
            close = '"' + raw[1]
            end = source.find(close, raw.end())
            i = len(source) if end < 0 else end + len(close)
        elif source[i] == '"':
            i += 1
            while i < len(source):
                if source[i] == "\\":
                    i += 2
                elif source[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
        elif char := CHAR.match(source, i):
            i = char.end()
        else:
            i += 1
            continue
        out[start:i] = " " * (i - start)
    return "".join(out)


def production_code(source: str) -> str:
    source = code_only(source)
    # Remove the test item, not the rest of the file. Test directories are excluded
    # by inventory(); this handles inline unit-test modules and individual tests.
    test = re.compile(r"#\s*\[\s*(?:cfg\s*\(\s*test\s*\)|test)\s*\]")
    while match := test.search(source):
        # A conditional field/value ends at its top-level comma, not at
        # the next item body. Do not erase its enclosing struct or impl.
        field = re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?\w+\s*:(?!:)", source[match.end():])
        if field:
            end = match.end() + field.end()
            nesting = 0
            while end < len(source):
                ch = source[end]
                if ch in "<([{":
                    nesting += 1
                elif ch in ">)]}":
                    if nesting == 0:
                        break
                    nesting -= 1
                elif ch == "," and nesting == 0:
                    end += 1
                    break
                end += 1
            source = source[:match.start()] + " " * (end - match.start()) + source[end:]
            continue
        # Semicolons in array argument types are not item terminators.
        end = match.end()
        nesting = 0
        while end < len(source):
            ch = source[end]
            if ch in "([":
                nesting += 1
            elif ch in ")]":
                nesting -= 1
            elif nesting == 0 and ch in "{;":
                break
            end += 1
        if end == len(source):
            raise ValueError("test item has no body or terminator")
        if source[end] == ";":
            source = source[:match.start()] + " " * (end + 1 - match.start()) + source[end + 1:]
            continue
        brace = end
        depth, end = 1, brace + 1
        while depth and end < len(source):
            depth += (source[end] == "{") - (source[end] == "}")
            end += 1
        if depth:
            raise ValueError("unbalanced test item")
        source = source[:match.start()] + " " * (end - match.start()) + source[end:]
    return source


def count(source: str) -> int:
    source = production_code(source)
    names = set(TYPES)
    for name in TYPES:
        names.update(re.findall(r"\b" + name + r"\s+as\s+(\w+)", source))
        names.update(re.findall(r"\btype\s+(\w+)\s*=\s*" + name + r"\b", source))
    constructor = r"\b(?:" + "|".join(sorted(names)) + r")\s*(?:::\s*<[^;{}]*?>\s*)?::\s*(?:new|from_query|default)\b"
    literal = r"\b(?:" + "|".join(sorted(names)) + r")\s*\{"
    return len(re.findall(constructor, source)) + len(re.findall(literal, source))


def inventory() -> dict:
    return {
        path.relative_to(ROOT): count(path.read_text())
        for path in sorted((ROOT / "crates").glob("*/src/**/*.rs"))
        if not path.is_relative_to(SOLVER)
    }


def main() -> int:
    cases = [
        ("TraitSolveCx :: new (db, scope)", 1),
        ('// TraitSolveCx::new(db)\nlet s = r#"TraitSolveCx::new(db)"#;', 0),
        ('/* outer /* TraitSolveCx::new(db) */ end */', 0),
        ('#[cfg(test)] mod tests { TraitSolveCx::new(db); } fn after() { TraitSolveCx::new(db); }', 1),
        ('use x::TraitSolveCx as Cx; Cx::new(db, scope);', 1),
        ('CanonicalGoalQuery::from_query(db, q); TraitSolveCx { scope };', 2),
        ('#[test] fn t(a: [u8; 2]) { TraitSolveCx::new(db); } fn after() { TraitSolveCx::new(db); }', 1),
        ('#[cfg(test)] use x::Foo; TraitSolveCx::new(db);', 1),
        ('type Cx = TraitSolveCx; let ctor = Cx::new; ctor(db);', 1),
        ("TraitSolveCx::<'db>::new(db); TraitSolverQuery { goal };", 2),
        ("struct A { #[cfg(test)] n: usize, } impl A { fn f() { TraitSolveCx::new(db); } }", 1),
        ("struct A { #[cfg(test)] n: Option<Vec<(u8, u8)>> } fn f() { TraitSolveCx::new(db); }", 1),
        ("let a = A { #[cfg(test)] n: Some((1, 2)), value: TraitSolveCx::new(db), };", 1),
    ]
    if any(count(source) != expected for source, expected in cases):
        raise AssertionError("solver construction counter self-check failed")
    # Item and body positions must both project their provision chain. This
    # excludes a second context constructor beside context_at's record reader.
    semantic = ROOT / "crates/hir/src/core/semantic/mod.rs"
    context_constructor = r"\bImplEnv\s*::\s*new(?:_\w+)?\b"
    for name in ("new", "new_with_selections", "new_with_choices"):
        assert re.search(context_constructor, production_code(f"ImplEnv::{name}(db);"))
    assert not re.search(context_constructor, production_code(
        '// ImplEnv::new(db)\n#[cfg(test)] fn control() { ImplEnv::new(db); }'
    ))
    if re.search(context_constructor, production_code(semantic.read_text())):
        print("semantic item context bypasses the provision chain", file=sys.stderr)
        return 1
    by_file = inventory()
    total = sum(by_file.values())
    print(f"direct solver constructions outside solver module: {total} (ceiling {BASELINE})")
    if total > BASELINE:
        for path, value in by_file.items():
            if value:
                print(f"  {path}: {value}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
