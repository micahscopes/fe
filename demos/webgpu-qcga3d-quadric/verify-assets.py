#!/usr/bin/env python3
import json
from pathlib import Path

here = Path(__file__).resolve().parent
gen = here / "gen"
required = [
    "kernel.fe", "frag.wgsl", "frag.wasm", "layout.json", "reference.json",
    "actor-source.fe", "actor-canonical.wasm", "actor-interface.js",
    "actor-interface.d.ts", "actor-manifest.json",
]
missing = [name for name in required if not (gen / name).is_file()]
assert not missing, f"missing QCGA assets: {missing}"
layout = json.loads((gen / "layout.json").read_text())
reference = json.loads((gen / "reference.json").read_text())
assert layout["mode"] == "Render"
assert layout["width"] == layout["height"] == 128
assert layout["vertex_entry"] == "vs_fullscreen"
assert layout["fragment_entry"] == "fs_main"
assert layout["color_target_format"] == "rgba8unorm"
assert layout["builtin_inputs"] == 2
assert layout["frag_wasm_export"] == "qcga3d_rotated_quadric_render"
assert layout["actor_wasm"] == "actor-canonical.wasm"
assert layout["actor_interface"] == "actor-interface.js"
assert layout["actor_lanes"] == ["render", "verify", "oracle"]
assert reference["width"] * reference["height"] == 16384
assert isinstance(reference["fnv1a32"], int)
assert reference["distinct_colors"] > 8
assert reference["provenance"] == layout["provenance"]
assert layout["provenance"]["sonatina_rev"].startswith("547519d4")
kernel = (gen / "kernel.fe").read_text()
assert "struct PointSupport" in kernel and "struct DualQuadricSupport" in kernel
wgsl = (gen / "frag.wgsl").read_text()
assert "@vertex" in wgsl and "@fragment" in wgsl and "sqrt(" in wgsl
assert not any(token in wgsl for token in ("i64", "u64"))
assert (gen / "frag.wasm").read_bytes()[:4] == b"\0asm"
assert (gen / "actor-canonical.wasm").read_bytes()[:4] == b"\0asm"
actor_source = (gen / "actor-source.fe").read_text()
assert "pub fn render(" in actor_source and "pub fn verify(" in actor_source
assert "pub fn oracle(" in actor_source
assert "pub fn oracle_pixel(" not in actor_source
assert "AllocatedBrowserBytes" in actor_source
interface = (gen / "actor-interface.js").read_text()
assert "createHostEffectAdapter" in interface and "createInterfaceCaller" in interface
manifest = json.loads((gen / "actor-manifest.json").read_text())
assert manifest["canonical_status"] == {
    "policy": "required", "embedded": True, "omission_reason": None,
}
assert [lane["name"] for lane in manifest["canonical_interface"]["lanes"]] == [
    "render", "verify", "oracle",
]
print(f"QCGA assets verified: fnv1a32={reference['fnv1a32']} colors={reference['distinct_colors']}")
