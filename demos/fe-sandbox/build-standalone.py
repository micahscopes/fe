#!/usr/bin/env python3
"""Inline the Fe web sandbox (S1) into a single self-contained standalone.html.

Everything the page needs is embedded: the vendored tree-sitter runtime + Fe
grammar wasm + highlights.scm (base64 in window.FE_ASSETS), the token CSS, both
committed kernel gen/ bundles, and all page JS folded into classic scripts (the ES
module `import`/`export` seams removed so the file runs with no module loader and
no network). The result loads from `file://` or a `data:` URL with zero fetches -
the same-origin asset-inline shim used to verify the page when the sandbox browser
is netns-isolated from the dev server. The multi-file index.html remains the real
static-hosting deliverable (it fetches ./vendor and ../webgpu-*/gen); this file is
its offline twin (also a handy one-file shareable).

Usage: python3 build-standalone.py   ->   writes ./standalone.html
"""

import base64
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
VENDOR = os.path.join(HERE, "vendor")
DEMOS = os.path.dirname(HERE)


def read(path):
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


def b64(path):
    with open(path, "rb") as f:
        return base64.b64encode(f.read()).decode("ascii")


def kernel_bundle(gen_dir):
    return {
        "fe": read(os.path.join(gen_dir, "kernel.fe")),
        "wgsl": read(os.path.join(gen_dir, "kernel.wgsl")),
        "layout": read(os.path.join(gen_dir, "layout.json")),
        "reference": read(os.path.join(gen_dir, "reference.json")),
        "wasm_b64": b64(os.path.join(gen_dir, "kernel.wasm")),
    }


def js_string(s):
    """Encode a Python string as a safe single JS string literal (base64+atob),
    so </script> and any bytes in the query/source cannot break out."""
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
        "ts_wasm_b64": b64(os.path.join(VENDOR, "tree-sitter.wasm")),
        "fe_wasm_b64": b64(os.path.join(VENDOR, "tree-sitter-fe.wasm")),
        "highlights_scm": read(os.path.join(VENDOR, "highlights.scm")),
        "kernels": {
            "poseidon": kernel_bundle(os.path.join(DEMOS, "webgpu-keystone", "gen")),
            "mandelbrot": kernel_bundle(os.path.join(DEMOS, "webgpu-mandelbrot", "gen")),
        },
    }

    # Build the FE_ASSETS bootstrap (all payloads via _b64utf8 so nothing can
    # break out of the script; wasm payloads stay raw base64 for the wasm loader).
    def kern_js(k):
        return (
            "{"
            f'fe:{js_string(k["fe"])},'
            f'wgsl:{js_string(k["wgsl"])},'
            f'layout:{js_string(k["layout"])},'
            f'reference:{js_string(k["reference"])},'
            f'wasm_b64:"{k["wasm_b64"]}"'
            "}"
        )

    fe_assets_js = (
        "function _b64utf8(b){return decodeURIComponent(escape(atob(b)));}\n"
        "window.FE_ASSETS = {\n"
        f'  ts_wasm_b64: "{assets["ts_wasm_b64"]}",\n'
        f'  fe_wasm_b64: "{assets["fe_wasm_b64"]}",\n'
        f'  highlights_scm: {js_string(assets["highlights_scm"])},\n'
        "  kernels: {\n"
        f'    poseidon: {kern_js(assets["kernels"]["poseidon"])},\n'
        f'    mandelbrot: {kern_js(assets["kernels"]["mandelbrot"])}\n'
        "  }\n"
        "};\n"
    )

    css = read(os.path.join(VENDOR, "fe-highlight.css"))
    ts_js = read(os.path.join(VENDOR, "tree-sitter.js"))
    highlighter_js = read(os.path.join(VENDOR, "fe-highlighter.js"))
    editor_js = read(os.path.join(HERE, "fe-editor.js"))

    # Fold the ES-module page JS (runners + main) into one classic script.
    runners = (
        strip_module_seams(read(os.path.join(DEMOS, "webgpu-keystone", "wasm-runner.js")))
        + "\n"
        + strip_module_seams(read(os.path.join(DEMOS, "webgpu-keystone", "webgpu-runner.js")))
    )
    main_js = strip_module_seams(read(os.path.join(HERE, "main.js")))
    page_js = runners + "\n" + main_js

    html = read(os.path.join(HERE, "index.html"))
    # Inline the token CSS.
    html = html.replace(
        '<link rel="stylesheet" href="./vendor/fe-highlight.css" />',
        f"<style>\n{css}\n</style>",
    )
    # Replace the external script block with fully inlined scripts.
    script_block = (
        '<script src="./vendor/tree-sitter.js"></script>\n'
        '<script src="./vendor/fe-highlighter.js"></script>\n'
        '<script src="./fe-editor.js"></script>\n'
        '<script type="module" src="./main.js"></script>'
    )
    inlined = (
        f"<script>{fe_assets_js}</script>\n"
        f"<script>{ts_js}</script>\n"
        f"<script>{highlighter_js}</script>\n"
        f"<script>{editor_js}</script>\n"
        f"<script>{page_js}</script>"
    )
    assert script_block in html, "index.html script block not found (layout changed?)"
    html = html.replace(script_block, inlined)
    html = html.replace(
        "Fe web sandbox (S1) — highlighted editor + the keystone",
        "Fe web sandbox (S1) — standalone (inlined)",
    )

    out = os.path.join(HERE, "standalone.html")
    with open(out, "w", encoding="utf-8") as f:
        f.write(html)
    print(f"wrote {out} ({os.path.getsize(out):,} bytes)")


if __name__ == "__main__":
    main()
