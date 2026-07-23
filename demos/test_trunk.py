import os
import pathlib
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.request


REPO = pathlib.Path(__file__).resolve().parent.parent
CONFIG = REPO / "demos" / "Trunk.toml"


class TrunkDemoTests(unittest.TestCase):
    def trunk_env(self):
        env = {**os.environ, "NO_COLOR": "false"}
        return env

    def test_build_copies_common_static_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            subprocess.run(
                [
                    "trunk",
                    "build",
                    "--config",
                    str(CONFIG),
                    "--dist",
                    tmp,
                ],
                cwd=REPO,
                env=self.trunk_env(),
                check=True,
                capture_output=True,
                text=True,
            )
            root = pathlib.Path(tmp)
            for relative in [
                "index.html",
                "shared/gpu-actor.js",
                "webgpu-cga-inversion/index.html",
                "webgpu-cga-inversion/gen-schedule32/frag.wasm",
                "webgpu-cga-inversion/gen-schedule32/frag.wgsl",
            ]:
                self.assertTrue((root / relative).is_file(), relative)

    def test_server_exposes_assets_and_isolation_headers(self):
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        process = subprocess.Popen(
            [
                "trunk",
                "serve",
                "--config",
                str(CONFIG),
                "--port",
                str(port),
                "--no-autoreload",
            ],
            cwd=REPO,
            env=self.trunk_env(),
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        try:
            response = None
            url = f"http://127.0.0.1:{port}/webgpu-cga-inversion/gen-schedule32/frag.wasm"
            for _ in range(100):
                try:
                    response = urllib.request.urlopen(url, timeout=0.5)
                    break
                except Exception:
                    if process.poll() is not None:
                        output = process.stdout.read()
                        self.fail(f"trunk exited early: {output}")
                    time.sleep(0.1)
            self.assertIsNotNone(response, "trunk server did not become ready")
            with response:
                self.assertEqual(response.status, 200)
                self.assertEqual(response.headers["Content-Type"], "application/wasm")
                self.assertEqual(response.headers["Cross-Origin-Opener-Policy"], "same-origin")
                self.assertEqual(response.headers["Cross-Origin-Embedder-Policy"], "require-corp")
                self.assertEqual(response.headers["Cross-Origin-Resource-Policy"], "same-origin")
                self.assertEqual(response.headers["Cache-Control"], "no-store")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            if process.stdout is not None:
                process.stdout.close()


if __name__ == "__main__":
    unittest.main()
