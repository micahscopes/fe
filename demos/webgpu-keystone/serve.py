#!/usr/bin/env python3
"""Static server for the Fe -> GPU keystone page.

Serves this directory on http://localhost:8787 with:
  * `application/wasm` for `.wasm` (so `WebAssembly.instantiate` streams cleanly);
  * cross-origin isolation headers (COOP: same-origin, COEP: require-corp).

COOP/COEP are NOT load-bearing for WebGPU or plain wasm here; they are set so the
same server keeps working when wasm threads (SharedArrayBuffer) arrive. No build
step, no framework: just the generated gen/ assets and the fixed runtime files.
"""

import http.server
import os
import socketserver

PORT = int(os.environ.get("PORT", "8787"))
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
        # Quieter: one line per request without the client address noise.
        print("  " + (fmt % args))


def main():
    gen = os.path.join(ROOT, "gen")
    if not os.path.isdir(gen) or not os.path.exists(os.path.join(gen, "layout.json")):
        print("WARNING: gen/ is missing or incomplete.")
        print("  Run first: cargo run -p fe-codegen --example gen_webgpu_demo")
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("127.0.0.1", PORT), Handler) as httpd:
        print(f"Fe -> GPU keystone: serving {ROOT}")
        print(f"  open http://localhost:{PORT} in your WebGPU Chrome")
        print("  (Ctrl-C to stop)")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nstopped")


if __name__ == "__main__":
    main()
