#!/usr/bin/env python3
"""GPU-free schema preflight for generated typed-CGA browser artifacts.

This does not execute wasm or WebGPU and therefore does not earn acceptance.
"""

import json
import os
import hashlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
GEN = Path(os.environ.get("CGA_BUNDLE_DIR", HERE / "gen"))
sys.path.insert(0, str(HERE.parent / "shared"))
from browser_runtime_preflight import validate_browser_runtime

required = ["layout.json", "reference.json", "kernel.fe", "frag.wgsl", "frag.wasm"]
missing = [name for name in required if not (GEN / name).is_file()]
if missing:
    raise SystemExit(f"missing generated CGA artifacts: {', '.join(missing)}")

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
    (2, 0, 4, "F32"), (3, 4, 4, "F32"), (4, 8, 4, "F32"),
    (5, 12, 4, "F32"), (6, 16, 4, "F32")
]
assert [p["name"] for p in params] == ["cam_x", "cam_y", "zoom", "inv_cx", "inv_cy"]
assert input_binding["span"] == 20 and input_binding["stride"] == 20
assert [(item["arg_index"], item["scalar"], item["source"]) for item in layout["builtin_inputs"]] == [
    (0, "I32", "FragmentPositionX"), (1, "I32", "FragmentPositionY")
]
assert reference["width"] == 128 and reference["height"] == 128
assert isinstance(reference["fnv1a32"], int)
for field in ["sky_pixels", "hit_pixels", "upper_pixels", "lower_pixels"]:
    assert isinstance(reference.get(field), int) and reference[field] >= 0, (
        f"reference {field} must be a non-negative integer"
    )
assert reference["upper_pixels"] > 0, "reference must contain upper-palette pixels"
assert reference["lower_pixels"] > 0, "reference must contain lower-palette pixels"
assert reference["upper_pixels"] + reference["lower_pixels"] == reference["hit_pixels"], (
    "palette pixel counts must sum to hit_pixels"
)
assert reference["sky_pixels"] + reference["hit_pixels"] == reference["width"] * reference["height"], (
    "sky_pixels + hit_pixels must cover the full reference frame"
)
assert reference["shape"] == "inverted_offset_torus_cyclide"
assert reference["inversion_center_runtime"] is True
kernel = (GEN / "kernel.fe").read_text()
algebra = reference["algebra"]
provider_algebra = algebra.startswith(
    "canonical Fe helpers shared by a forced typed Schedule<32> witness"
)
if algebra == "typed support-specialized recursive Cl(4,1) S*P*S":
    tokens = [
        "recursive type fn MvTF",
        "sandwich_support_cl41",
        "let sandwich: MvTF<5>",
        "raw_16 - raw_8",
        "safe_weight",
        "ring_radius",
    ]
elif algebra.startswith("canonical CTFE-derived Schedule<32>;"):
    tokens = [
        "recursive type fn Schedule",
        "recursive type fn Storage",
        "fn eval_specialized",
        "FOUR_CHUNK_TYPED_EVALUATION",
        "safe_weight",
        "ring_radius",
    ]
elif provider_algebra:
    tokens = [
        "recursive type fn Schedule",
        "type Schedule32 = Schedule<32>",
        "struct CanonicalCgaProvider",
        "for triple in 0..80",
        "const SANDWICH_OUTPUT_BITS",
        'builder.emit_method("sandwich"',
        "safe_weight",
        "ring_radius",
    ]
else:
    raise AssertionError(f"unrecognized CGA algebra contract: {algebra!r}")
for token in tokens:
    assert token in kernel, f"generated kernel lacks runtime-center cyclide token {token!r}"
if provider_algebra:
    assert "trait Eval5" not in kernel
    assert "ScheduleChunk" not in kernel
wgsl = (GEN / "frag.wgsl").read_text()
assert "@fragment" in wgsl and "loop" in wgsl and "sqrt(" in wgsl
if provider_algebra:
    for compile_time_token in ["CanonicalCgaProvider", "builder.emit_method", "triple < 80"]:
        assert compile_time_token not in wgsl, (
            f"provider compile-time token leaked into browser WGSL: {compile_time_token!r}"
        )
    assert wgsl.count("loop {") == 1, "provider schedule must not become a runtime loop"
if algebra.startswith("canonical CTFE-derived Schedule<32>;") or provider_algebra:
    actor_manifest = json.loads((GEN / "actor/manifest.json").read_text())
    assert actor_manifest["protocol"] == "fe-web-bundle"
    assert actor_manifest["protocol_version"] == 4
    validate_browser_runtime(actor_manifest, GEN / "actor")
    declared = [
        actor_manifest["artifacts"]["wasm"],
        actor_manifest["artifacts"]["wgsl"],
        *[artifact["path"] for artifact in actor_manifest["artifacts"]["canonical_adapters"]],
    ]
    assert len(declared) == len(set(declared))
    metadata = {
        artifact["path"]: artifact
        for artifact in actor_manifest["artifacts"]["canonical_adapters"]
    }
    for name in declared:
        path = GEN / "actor" / name
        assert path.is_file(), f"actor WebBundle is missing declared artifact {name}"
        if name in metadata:
            payload = path.read_bytes()
            assert len(payload) == metadata[name]["bytes"]
            assert hashlib.sha256(payload).hexdigest() == metadata[name]["sha256"]
    assert (GEN / "actor" / actor_manifest["artifacts"]["wasm"]).stat().st_size == (
        actor_manifest["artifacts"]["wasm_bytes"]
    )
    assert (GEN / "actor" / actor_manifest["artifacts"]["wgsl"]).stat().st_size == (
        actor_manifest["artifacts"]["wgsl_bytes"]
    )
print("typed-CGA browser artifact schema preflight: ok (not execution acceptance)")
