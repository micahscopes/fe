import os
import pathlib
import subprocess
import tempfile
import time
import unittest


DEMOS = pathlib.Path(__file__).resolve().parent
OVERLAY = DEMOS / "with-sonatina-overlay.sh"
GENERATION_LOCK = DEMOS / "with-fe-generation-lock.sh"
QCGA_GENERATE = DEMOS / "webgpu-qcga3d-quadric" / "generate.sh"
TEST_TMP = DEMOS.parent / "output" / "demo-test-tmp"


class SonatinaOverlayTests(unittest.TestCase):
    def test_generation_lock_forces_native_temps_under_demo_workspace(self):
        TEST_TMP.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=TEST_TMP) as temporary:
            result = subprocess.run(
                [
                    str(GENERATION_LOCK),
                    "sh",
                    "-c",
                    'test "$TMPDIR" = "$FE_DEMO_TMPDIR" && test -d "$TMPDIR"',
                ],
                env={
                    **os.environ,
                    "FE_DEMO_STATE_DIR": temporary,
                    "FE_DEMO_TMPDIR": temporary,
                    "TMPDIR": "/tmp",
                },
                text=True,
                capture_output=True,
            )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_offline_cache_miss_fails_without_running_command(self):
        TEST_TMP.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=TEST_TMP) as cache:
            env = {
                **os.environ,
                "FE_BROWSER_CACHE_DIR": cache,
                "FE_DEMO_TMPDIR": cache,
                "FE_BROWSER_OFFLINE": "1",
            }
            env.pop("SONATINA_DIR", None)
            result = subprocess.run(
                [str(OVERLAY), "sh", "-c", "exit 99"],
                env=env,
                text=True,
                capture_output=True,
            )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("offline browser build cache is missing Sonatina base", result.stderr)

    def test_generator_waits_for_lock_before_source_checks(self):
        TEST_TMP.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=TEST_TMP) as temporary:
            temporary = pathlib.Path(temporary)
            acquired = temporary / "acquired"
            release = temporary / "release"
            holder = subprocess.Popen(
                [
                    str(GENERATION_LOCK),
                    "sh",
                    "-c",
                    'touch "$1"; while [ ! -e "$2" ]; do sleep .02; done',
                    "holder",
                    str(acquired),
                    str(release),
                ],
                env={**os.environ, "FE_DEMO_STATE_DIR": str(temporary)},
            )
            deadline = time.monotonic() + 5
            while not acquired.exists() and holder.poll() is None:
                if time.monotonic() >= deadline:
                    holder.kill()
                    self.fail("generation-lock holder did not start")
                time.sleep(0.02)

            env = {
                **os.environ,
                "FE_DEMO_STATE_DIR": str(temporary),
                "FE_DEMO_TMPDIR": str(temporary),
            }
            env.pop("SONATINA_DIR", None)
            generator = subprocess.Popen(
                [str(QCGA_GENERATE)],
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            try:
                time.sleep(0.1)
                self.assertIsNone(
                    generator.poll(),
                    "generator reached its source checks while generation lock was held",
                )
            finally:
                release.touch()
                holder.wait(timeout=5)
            _, stderr = generator.communicate(timeout=5)
            self.assertEqual(holder.returncode, 0)
            self.assertEqual(generator.returncode, 2, stderr)
            self.assertIn("QCGA generation requires SONATINA_DIR", stderr)


if __name__ == "__main__":
    unittest.main()
