import os
import pathlib
import subprocess
import tempfile
import unittest


DEMOS = pathlib.Path(__file__).resolve().parent
OVERLAY = DEMOS / "with-sonatina-overlay.sh"


class SonatinaOverlayTests(unittest.TestCase):
    def test_offline_cache_miss_fails_without_running_command(self):
        with tempfile.TemporaryDirectory() as cache:
            env = {
                **os.environ,
                "FE_BROWSER_CACHE_DIR": cache,
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


if __name__ == "__main__":
    unittest.main()
