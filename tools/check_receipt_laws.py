#!/usr/bin/env python3
"""Require explicit law evidence in step receipts."""
import argparse
from pathlib import Path
import re
import sys


def check(path: Path) -> list[str]:
    text = path.read_text()
    sections = re.findall(
        r"(?mi)^#{1,6}\s+Laws\s*$\n(.*?)(?=^#{1,6}\s|\Z)", text, re.S
    )
    if not sections:
        return ["missing Laws section"]
    section = sections[-1]
    laws = re.findall(r"\bH(\d+)\b", section)
    errors = []
    if not laws:
        errors.append("Laws section names no numbered laws")
    invalid = sorted({law for law in laws if not 1 <= int(law) <= 16})
    if invalid:
        errors.append("unknown laws: " + ", ".join("H" + law for law in invalid))
    for paragraph in re.split(r"\n\s*\n", section):
        if not re.search(r"\bH\d+\b", paragraph):
            continue
        # Evidence must be explicit; incomplete coverage must say so too.
        if not re.search(
            r"\b(pass\w*|check\w*|guard\w*|test\w*|probe\w*|fixture\w*|"
            r"enforc\w*|unchecked|pending|incomplete|unproven)\b",
            paragraph,
            re.I,
        ):
            errors.append("law paragraph lacks check or explicit unchecked status")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("receipts", nargs="+", type=Path)
    args = parser.parse_args()
    failed = False
    for path in args.receipts:
        try:
            errors = check(path)
        except (OSError, UnicodeError) as error:
            errors = [str(error)]
        failed |= bool(errors)
        print(f"{'FAIL' if errors else 'PASS'} {path}: " + ("; ".join(errors) or "laws stated"))
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
