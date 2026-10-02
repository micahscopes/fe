"""Keep direct trait-solver construction from growing outside shared entry points."""

from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
HIR = ROOT / "crates/hir/src"
BASELINE = 57
CALL = "TraitSolveCx::new"


def count(source: str) -> int:
    return source.split("#[cfg(test)]", 1)[0].count(CALL)


def main() -> int:
    by_file = {
        path.relative_to(ROOT): count(path.read_text())
        for path in HIR.rglob("*.rs")
    }
    total = sum(by_file.values())
    if count("fn old() { TraitSolveCx::new(db); }") != 1:
        print("self-check: direct solver setup was not counted", file=sys.stderr)
        return 1
    if total > BASELINE:
        detail = "\n".join(f"  {path}: {value}" for path, value in sorted(by_file.items()) if value)
        print(
            f"direct trait solver setups grew from {BASELINE} to {total}:\n{detail}",
            file=sys.stderr,
        )
        return 1
    print(f"direct trait solver setups: {total} (budget {BASELINE})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
