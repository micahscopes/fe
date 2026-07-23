#!/usr/bin/env python3
"""Prove CGA and QCGA package the same compiler-owned actor runtime."""

import json
from pathlib import Path

from browser_runtime_preflight import validate_shared_browser_runtime


DEMOS = Path(__file__).resolve().parent.parent
CGA_ROOT = DEMOS / "webgpu-cga-inversion" / "gen-schedule32" / "actor"
QCGA_ROOT = DEMOS / "webgpu-qcga3d-quadric" / "gen"


def read_manifest(path):
    assert path.is_file(), f"missing generated WebBundle manifest: {path}"
    return json.loads(path.read_text())


identity = validate_shared_browser_runtime({
    "Schedule32 CGA": (read_manifest(CGA_ROOT / "manifest.json"), CGA_ROOT),
    "QCGA": (read_manifest(QCGA_ROOT / "actor-manifest.json"), QCGA_ROOT),
})
print(
    f"CGA/QCGA shared browser actor runtime: ok "
    f"({identity[0]} v{identity[1]}, {len(identity[2])} modules)"
)
