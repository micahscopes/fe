"""Keep recognition of designated core items in the core item registry."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
HIR = ROOT / "crates/hir/src"
REGISTRY = HIR / "analysis/ty/corelib.rs"
DESIGNATED = "Bytes|DynString|DynArray|Evidence|Constraint"
NAME_COMPARISON = re.compile(
    rf"\b(?:name|expected_name)(?:\.data\([^)]*\))?(?:\.as_str\(\))?"
    rf"\s*(?:==|!=)\s*\"(?:{DESIGNATED})\""
)
OLD_HELPER = re.compile(r"\bis_core_adt_named\b")


def violations(source: str) -> list[str]:
    result = []
    for number, line in enumerate(source.splitlines(), 1):
        line = line.split("//", 1)[0]
        if NAME_COMPARISON.search(line) or OLD_HELPER.search(line):
            result.append(f"line {number}: core item recognized by spelling")
    return result


def main() -> int:
    errors = []
    for path in HIR.rglob("*.rs"):
        if path == REGISTRY:
            continue
        source = path.read_text().split("#[cfg(test)]", 1)[0]
        errors.extend(
            f"{path.relative_to(ROOT)}:{violation}"
            for violation in violations(source)
        )

    if not violations('if name.data(db) != "DynString" { return false; }'):
        errors.append("self-check: name comparison was not rejected")
    if not violations('is_core_adt_named(db, ty, "Bytes")'):
        errors.append("self-check: old helper was not rejected")
    if "core::abi::DynString" not in REGISTRY.read_text():
        errors.append("core ABI identity registry is missing")

    if errors:
        print("core item identity violations:\n" + "\n".join(errors), file=sys.stderr)
        return 1
    print("core item identity: designated type checks use the registry")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
