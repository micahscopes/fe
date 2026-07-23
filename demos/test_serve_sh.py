import os
import pathlib
import subprocess
import tempfile
import unittest


DEMOS = pathlib.Path(__file__).resolve().parent
SERVE = DEMOS / "serve.sh"


class DemoServeCommandTests(unittest.TestCase):
    def run_generate(self, demo):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            log = tmp / "calls"
            command = tmp / "generate"
            command.write_text(
                "#!/usr/bin/env bash\nset -eu\nprintf '%s\\n' \"$1\" >> \"$CALL_LOG\"\n"
            )
            command.chmod(0o755)
            env = {
                **os.environ,
                "FE_DEMO_GENERATE_CMD": str(command),
                "FORCE_DEMO_REGEN": "1",
                "CALL_LOG": str(log),
            }
            result = subprocess.run(
                [str(SERVE), demo],
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
