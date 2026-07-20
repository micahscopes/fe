#!/usr/bin/env python3
"""Static server for the Fe demos ROOT (both keystone and mandelbrot pages).

Serves this `demos/` directory on http://localhost:8788 so BOTH pages work off
one origin:
  * http://localhost:8788/webgpu-keystone/   (the scalar Fe -> GPU keystone)
  * http://localhost:8788/webgpu-mandelbrot/ (the first Fe-computed IMAGE)

The mandelbrot page imports the kernel-blind runners from ../webgpu-keystone/
relatively, so both pages must be served from a common root; that root is here.
Headers match the per-demo servers:
  * `application/wasm` for `.wasm` (so `WebAssembly.instantiate` streams cleanly);
  * cross-origin isolation headers (COOP: same-origin, COEP: require-corp), set
    so the same server keeps working when wasm threads (SharedArrayBuffer)
    arrive. Not load-bearing for WebGPU or plain wasm here.
No build step, no framework: just the generated gen/ assets and the fixed runtime
files.
"""

import http.server
import os
import socketserver

PORT = int(os.environ.get("PORT", "8788"))
# Bind 127.0.0.1 by default (local use). Set HOST=0.0.0.0 to expose the server on
# all interfaces (e.g. when the browser runs in a separate network namespace).
HOST = os.environ.get("HOST", "127.0.0.1")
ROOT = os.path.dirname(os.path.abspath(__file__))


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".js": "text/javascript",
        ".mjs": "text/javascript",
        ".json": "application/json",
    }

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=ROOT, **kwargs)

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, fmt, *args):
        print("  " + (fmt % args))


def main():
    generators = {
        "webgpu-keystone": "gen_webgpu_demo",
        "webgpu-mandelbrot": "gen_mandelbrot_demo",
    }
    for demo, example in generators.items():
        gen = os.path.join(ROOT, demo, "gen")
        if not os.path.isdir(gen) or not os.path.exists(os.path.join(gen, "layout.json")):
            print(f"WARNING: {demo}/gen is missing or incomplete.")
            print(f"  Run first: cargo run -p fe-codegen --example {example}")
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer((HOST, PORT), Handler) as httpd:
        print(f"Fe demos: serving {ROOT} on {HOST}:{PORT}")
        print(f"  keystone:   http://localhost:{PORT}/webgpu-keystone/")
        print(f"  mandelbrot: http://localhost:{PORT}/webgpu-mandelbrot/")
        print("  (Ctrl-C to stop)")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nstopped")


if __name__ == "__main__":
    main()
