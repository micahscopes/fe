#!/usr/bin/env python3
"""GPU-free schema preflight for generated D1 browser artifacts.

This does not execute wasm or WebGPU and therefore does not earn acceptance.
"""

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
GEN = HERE / "gen"

required = ["layout.json", "reference.json", "kernel.fe", "frag.wgsl", "frag.wasm"]
missing = [name for name in required if not (GEN / name).is_file()]
if missing:
    raise SystemExit(f"missing generated D1 artifacts: {', '.join(missing)}")

layout = json.loads((GEN / "layout.json").read_text())
reference = json.loads((GEN / "reference.json").read_text())
assert layout["mode"] == "Render"
assert layout["width"] == 128 and layout["height"] == 128
assert layout["entry_point"] == "fs_main"
assert layout["vertex_entry"] == "vs_fullscreen"
assert layout["fragment_entry"] == "fs_main"
assert layout["color_target_format"] == "rgba8unorm"
input_bindings = [binding for binding in layout["bindings"] if binding["role"] == "Input"]
assert len(input_bindings) == 1
input_binding = input_bindings[0]
assert (input_binding["group"], input_binding["binding"], input_binding["access"]) == (0, 1, "Read")
params = layout["params"]
assert [(p["arg_index"], p["offset"], p["width"], p["scalar"]) for p in params] == [
    (2, 0, 4, "F32"), (3, 4, 4, "F32"), (4, 8, 4, "F32")
]
assert [p["name"] for p in params] == ["cam_x", "cam_y", "zoom"]
assert input_binding["span"] == 12 and input_binding["stride"] == 12
assert [(item["arg_index"], item["scalar"], item["source"]) for item in layout["builtin_inputs"]] == [
    (0, "I32", "FragmentPositionX"), (1, "I32", "FragmentPositionY")
]
assert reference["width"] == 128 and reference["height"] == 128
assert isinstance(reference["fnv1a32"], int)
for field in ["sky_pixels", "hit_pixels", "material_a_pixels", "material_b_pixels"]:
    assert isinstance(reference.get(field), int) and reference[field] >= 0, (
        f"reference {field} must be a non-negative integer"
    )
assert reference["material_a_pixels"] > 0, "reference must contain material A pixels"
assert reference["material_b_pixels"] > 0, "reference must contain material B pixels"
assert reference["material_a_pixels"] + reference["material_b_pixels"] == reference["hit_pixels"], (
    "material pixel counts must sum to hit_pixels"
)
assert reference["sky_pixels"] + reference["hit_pixels"] == reference["width"] * reference["height"], (
    "sky_pixels + hit_pixels must cover the full reference frame"
)
kernel = (GEN / "kernel.fe").read_text()
for token in ["distance_a", "distance_b", "a_is_closer"]:
    assert token in kernel, f"generated kernel lacks two-sphere semantic token {token!r}"
wgsl = (GEN / "frag.wgsl").read_text()
assert "@fragment" in wgsl and "loop" in wgsl and "sqrt(" in wgsl
print("D1 browser artifact schema preflight: ok (not execution acceptance)")
