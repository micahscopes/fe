"""Reject repeated diagnostic obligations at the same primary highlight."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = (
    ROOT / "crates/uitest/fixtures",
    ROOT / "crates/fe/tests/fixtures",
)
HEADER = re.compile(r"^(error|warning)\[([^]]+)\]:", re.MULTILINE)
LOCATION = re.compile(r"^\s*┌─ (.+:\d+:\d+)", re.MULTILINE)
SOURCE_LINE = re.compile(r"^\s*(\d+)\s+│")
HIGHLIGHT = re.compile(r"\^+")
SECONDARY = re.compile(r"^\s*│\s*(?P<mark>-+)(?=\s|$)")


def primary_highlights(body: str) -> tuple[tuple[str, int, int], ...]:
    source_line = ""
    result = []
    for line in body.splitlines():
        numbered = SOURCE_LINE.match(line)
        if numbered:
            source_line = numbered.group(1)
        if line.lstrip().startswith("│") and (mark := HIGHLIGHT.search(line)):
            result.append((source_line, mark.start(), mark.end()))
    return tuple(result)


def obligation_sites(body: str) -> tuple[tuple[str, str, int, int], ...]:
    """Distinct bounds/predicates can fail at one call's primary highlight."""
    filename = source_line = ""
    result = []
    for line in body.splitlines():
        if location := LOCATION.match(line):
            filename = location.group(1).rsplit(":", 2)[0]
            source_line = ""
        if numbered := SOURCE_LINE.match(line):
            source_line = numbered.group(1)
        if mark := SECONDARY.match(line):
            column = mark.start("mark") - line.index("│") - 1
            result.append((filename, source_line, column, len(mark.group("mark"))))
    return tuple(result)


def violations(source: str) -> list[str]:
    seen = set()
    errors = []
    headers = list(HEADER.finditer(source))
    for index, header in enumerate(headers):
        end = headers[index + 1].start() if index + 1 < len(headers) else len(source)
        body = source[header.end() : end]
        location = LOCATION.search(body)
        if location is None:
            continue
        # Parser recovery can report a missing token and the unexpected token
        # at the same offset. Other diagnostics may contain several primary
        # highlights, as with two distinct conflicting declarations.
        highlights = () if header.group(2).startswith("1-") else primary_highlights(body)
        key = (
            header.group(1),
            header.group(2),
            location.group(1),
            highlights,
            obligation_sites(body),
        )
        if key in seen:
            errors.append(f"{header.group(1)}[{header.group(2)}] at {location.group(1)}")
        seen.add(key)
    return errors


def main() -> int:
    errors = []
    for fixtures in FIXTURES:
        for path in fixtures.rglob("*.snap"):
            errors.extend(f"{path.relative_to(ROOT)}: {item}" for item in violations(path.read_text()))

    sample = """error[6-0003]: first
  ┌─ example.fe:1:1
  │
1 │ bad
  │ ^^^ first
error[6-0003]: second
  ┌─ example.fe:1:1
  │
1 │ bad
  │ ^^^ second
"""
    if not violations(sample):
        errors.append("self-check: repeated rendered diagnostic was not rejected")

    cause = """error[8-0093]: const requirement is not satisfied
  ┌─ example.fe:10:1
  │
4 │ predicate
  │ ----- required by this predicate
10 │ call
   │ ^^^^ condition evaluated to false
"""
    distinct = cause + cause.replace("4 │ predicate", "7 │ predicate")
    if violations(distinct):
        errors.append("self-check: distinct predicate sites were conflated")
    if not violations(cause + cause):
        errors.append("self-check: duplicate predicate report was accepted")
    if not violations(cause + cause.replace("required by this predicate", "required here")):
        errors.append("self-check: relabeling hid a repeated obligation")
    other_file = cause.replace("example.fe:10:1", "other.fe:10:1")
    if violations(cause + other_file):
        errors.append("self-check: distinct source files were conflated")

    if errors:
        print("repeated rendered diagnostics:\n" + "\n".join(errors), file=sys.stderr)
        return 1
    print("no repeated diagnostic in a snapshot")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
