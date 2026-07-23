import os
import pathlib
import subprocess
import tempfile
import unittest


DEMOS = pathlib.Path(__file__).resolve().parent
SERVE = DEMOS / "serve.sh"
TEST_TMP = DEMOS.parent / "output" / "demo-test-tmp"


class DemoServeCommandTests(unittest.TestCase):
    def run_generate(self, demo, *args):
        TEST_TMP.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=TEST_TMP) as tmp:
            tmp = pathlib.Path(tmp)
            log = tmp / "calls"
            command = tmp / "generate"
            command.write_text(
                "#!/usr/bin/env bash\nset -eu\nprintf '%s\\n' \"$1\" >> \"$CALL_LOG\"\n"
            )
            command.chmod(0o755)
            trunk = tmp / "trunk"
            trunk.write_text(
                "#!/usr/bin/env bash\nset -eu\nprintf 'trunk' >> \"$CALL_LOG\"\n"
                "printf ' <%s>' \"$@\" >> \"$CALL_LOG\"\nprintf '\\n' >> \"$CALL_LOG\"\n"
            )
            trunk.chmod(0o755)
            env = {
                **os.environ,
                "FE_DEMO_GENERATE_CMD": str(command),
                "FORCE_DEMO_REGEN": "1",
                "CALL_LOG": str(log),
                "FE_DEMO_STATE_DIR": str(tmp / "state"),
                "FE_DEMO_TMPDIR": str(tmp),
                "PATH": f"{tmp}:{os.environ['PATH']}",
            }
            result = subprocess.run(
                [str(SERVE), demo, *args],
                env=env,
                text=True,
                capture_output=True,
            )
            calls = log.read_text().splitlines() if log.exists() else []
            return result, calls

    def test_each_public_selector_routes_to_one_generator(self):
        for demo in [
            "keystone",
            "mandelbrot",
            "mandelbrot-interactive",
            "clifford-interactive",
            "cga",
            "cga-d1",
            "cga-schedule32",
            "qcga",
        ]:
            with self.subTest(demo=demo):
                result, calls = self.run_generate(demo)
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = "cga" if demo == "cga-schedule32" else demo
                self.assertEqual(calls, [expected])

    def test_serve_runs_selected_preflight_then_trunk_watch(self):
        result, calls = self.run_generate("cga-schedule32", "--serve")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[0], "cga")
        self.assertEqual(
            calls[1],
            f"trunk <serve> <--config> <{DEMOS / 'Trunk.toml'}>",
        )

    def test_no_watch_is_explicit_and_requires_serving(self):
        result, calls = self.run_generate("qcga", "--serve", "--no-watch")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[0], "qcga")
        self.assertEqual(
            calls[1],
            f"trunk <serve> <--config> <{DEMOS / 'Trunk.toml'}> <--no-autoreload>",
        )
        result, calls = self.run_generate("qcga", "--no-watch")
        self.assertEqual(result.returncode, 2)
        self.assertIn("--no-watch requires --serve", result.stderr)
        self.assertEqual(calls, [])

    def test_unknown_selector_fails_before_serving(self):
        result = subprocess.run(
            [str(SERVE), "unknown"],
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("unknown demo", result.stderr)


if __name__ == "__main__":
    unittest.main()
