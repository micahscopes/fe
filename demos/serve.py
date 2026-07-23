#!/usr/bin/env python3
"""Static server for the common Fe demos root.

Headers match the per-demo servers:
  * `application/wasm` for `.wasm` (so `WebAssembly.instantiate` streams cleanly);
  * cross-origin isolation headers (COOP: same-origin, COEP: require-corp), set
    so the same server keeps working when wasm threads (SharedArrayBuffer)
    arrive. Not load-bearing for WebGPU or plain wasm here.
Generation and validation belong to `demos/serve.sh`; this module only serves
the resulting static applications from their required common origin.
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
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer((HOST, PORT), Handler) as httpd:
        print(f"Fe demos: serving {ROOT} on {HOST}:{PORT}")
        print(f"  keystone:    http://localhost:{PORT}/webgpu-keystone/")
        print(f"  mandelbrot:  http://localhost:{PORT}/webgpu-mandelbrot/")
        print(f"  interactive: http://localhost:{PORT}/webgpu-mandelbrot-interactive/")
        print(f"  Clifford:     http://localhost:{PORT}/webgpu-clifford-interactive/")
        print(f"  CGA:          http://localhost:{PORT}/webgpu-cga-inversion/")
        print(f"  QCGA:         http://localhost:{PORT}/webgpu-qcga3d-quadric/")
        print("  (Ctrl-C to stop)")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nstopped")


if __name__ == "__main__":
    main()
