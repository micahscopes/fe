#!/usr/bin/env python3
"""Inline the interactive Fe mandelbrot page into a single self-contained
standalone.html Micah opens directly (file:// or a data: URL, zero fetches).

Everything the page needs is embedded: the compiler-emitted gen/ bundle (frag.wgsl,
layout.json, ctl.json, reference.json, kernel.fe, ctl.fe as base64 text; frag.wasm +
ctl.wasm as base64 bytes in window.MANDEL_LIVE_ASSETS), and all page JS folded into
one classic script (the ES-module import/export seams removed so it runs with no
module loader and no network). The multi-file index.html remains the real
static-hosting deliverable; this file is its offline twin.

Usage: python3 build-standalone.py   ->   writes ./standalone.html
"""

import base64
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
GEN = os.path.join(HERE, "gen")
DEMOS = os.path.dirname(HERE)
KEYSTONE = os.path.join(DEMOS, "webgpu-keystone")
SHARED = os.path.join(DEMOS, "shared")


def read(path):
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


def b64_bytes(path):
    with open(path, "rb") as f:
        return base64.b64encode(f.read()).decode("ascii")


def js_b64_text(s):
    """A safe JS string literal via base64+atob (so </script> or any byte in the
    Fe source / JSON cannot break out of the inlined <script>)."""
    enc = base64.b64encode(s.encode("utf-8")).decode("ascii")
    return f'_b64utf8("{enc}")'


def strip_module_seams(src):
    out = []
    for line in src.splitlines():
        if re.match(r"\s*import\s.+from\s", line):
            continue
        line = re.sub(r"^export\s+", "", line)
        out.append(line)
    return "\n".join(out)


def main():
    assets = {
        "layout": read(os.path.join(GEN, "layout.json")),
        "ctl": read(os.path.join(GEN, "ctl.json")),
        "reference": read(os.path.join(GEN, "reference.json")),
        "frag_fe": read(os.path.join(GEN, "kernel.fe")),
        "ctl_fe": read(os.path.join(GEN, "ctl.fe")),
        "wgsl": read(os.path.join(GEN, "frag.wgsl")),
        "frag_wasm_b64": b64_bytes(os.path.join(GEN, "frag.wasm")),
        "ctl_wasm_b64": b64_bytes(os.path.join(GEN, "ctl.wasm")),
    }

    bootstrap = (
        "function _b64utf8(b){return decodeURIComponent(escape(atob(b)));}\n"
        "window.MANDEL_LIVE_ASSETS = {\n"
        f'  layout: {js_b64_text(assets["layout"])},\n'
        f'  ctl: {js_b64_text(assets["ctl"])},\n'
        f'  reference: {js_b64_text(assets["reference"])},\n'
        f'  frag_fe: {js_b64_text(assets["frag_fe"])},\n'
        f'  ctl_fe: {js_b64_text(assets["ctl_fe"])},\n'
        f'  wgsl: {js_b64_text(assets["wgsl"])},\n'
        f'  frag_wasm_b64: "{assets["frag_wasm_b64"]}",\n'
        f'  ctl_wasm_b64: "{assets["ctl_wasm_b64"]}"\n'
        "};\n"
    )

    # Fold the ES-module page JS into one classic script, in dependency order:
    # shared actor protocol/endpoint, kernel-blind runners, the Mandelbrot actor
    # adapter, demo-blind pump, then main (which calls main()).
    page_js = "\n".join(
        strip_module_seams(read(p))
        for p in [
            os.path.join(SHARED, "actor-coordinator.js"),
            os.path.join(SHARED, "actor-endpoint.js"),
            os.path.join(KEYSTONE, "wasm-runner.js"),
            os.path.join(KEYSTONE, "webgpu-runner.js"),
            os.path.join(HERE, "actor-runtime.js"),
            os.path.join(HERE, "live-pump.js"),
            os.path.join(HERE, "main.js"),
        ]
    )

    html = read(os.path.join(HERE, "index.html"))
    script_tag = '<script type="module" src="./main.js"></script>'
    assert script_tag in html, "index.html module script tag not found (layout changed?)"
    inlined = f"<script>{bootstrap}</script>\n<script>{page_js}</script>"
    html = html.replace(script_tag, inlined)
    html = html.replace(
        "<title>Interactive Fe mandelbrot: Fe render + Fe controls</title>",
        "<title>Interactive Fe mandelbrot - standalone (inlined)</title>",
    )

    out = os.path.join(HERE, "standalone.html")
    with open(out, "w", encoding="utf-8") as f:
        f.write(html)
    print(f"wrote {out} ({os.path.getsize(out):,} bytes)")


if __name__ == "__main__":
    main()
