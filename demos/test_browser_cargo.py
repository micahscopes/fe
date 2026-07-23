#!/usr/bin/env python3
"""Contract tests for the temporary browser Cargo runner."""
from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


DEMOS = Path(__file__).resolve().parent
REPO = DEMOS.parent
RUNNER = DEMOS / "with-browser-cargo.sh"
EXPECTED = "ac266c210cad7872fc98380a73b4ca363877bc1f"
CRATES = ("ir", "triple", "codegen", "verifier", "macros", "parser")
TEST_TMP = REPO / "target" / "fe-browser-script-tests"
TEST_TMP.mkdir(parents=True, exist_ok=True)


class BrowserCargoTests(unittest.TestCase):
    def run_fake(self, exit_code: int = 0) -> tuple[subprocess.CompletedProcess[str], str]:
        with tempfile.TemporaryDirectory(dir=TEST_TMP) as raw:
            root = Path(raw)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            argv = root / "argv"
            fake = bin_dir / "cargo"
            fake.write_text(
                "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > \"$FAKE_ARGV\"\n"
                f"exit {exit_code}\n"
            )
            fake.chmod(0o755)
            before = (REPO / "Cargo.lock").read_bytes()
            env = os.environ.copy()
            env.update(
                {
                    "PATH": f"{bin_dir}:{env['PATH']}",
                    "FAKE_ARGV": str(argv),
                    "FE_DEMO_GENERATION_LOCK_ACTIVE": "1",
                    "FE_DEMO_TMPDIR": str(root / "tmp"),
                    "CARGO_TARGET_DIR": str(root / "target"),
                    "SONATINA_DIR": "/workspace/sonatina-sparse-api",
                    "RUSTC_WRAPPER": "must-not-leak",
                }
            )
            result = subprocess.run(
                [str(RUNNER), "check", "-p", "fe"],
                cwd=REPO,
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )
            after = (REPO / "Cargo.lock").read_bytes()
            self.assertEqual(before, after)
            return result, argv.read_text() if argv.exists() else ""

    def test_exact_overlay_patch_set_and_sanitized_build(self) -> None:
        result, argv = self.run_fake()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(EXPECTED, result.stderr)
        for crate in CRATES:
            needle = f'sonatina-{crate}.path="/workspace/sonatina-sparse-api/crates/{crate}"'
            self.assertEqual(argv.count(needle), 1, argv)
        self.assertIn("check\n-p\nfe\n", argv)

    def test_lock_restored_when_cargo_fails(self) -> None:
        result, _ = self.run_fake(17)
        self.assertEqual(result.returncode, 17)

    def test_wrong_or_dirty_overlay_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory(dir=TEST_TMP) as raw:
            checkout = Path(raw) / "sonatina"
            subprocess.run(["git", "clone", "-q", "/workspace/sonatina-sparse-api", checkout], check=True)
            (checkout / "dirty").write_text("x")
            env = os.environ.copy()
            env.update(
                {
                    "FE_DEMO_GENERATION_LOCK_ACTIVE": "1",
                    "FE_DEMO_TMPDIR": str(Path(raw) / "tmp"),
                    "SONATINA_DIR": str(checkout),
                }
            )
            result = subprocess.run(
                [str(RUNNER), "check"],
                cwd=REPO,
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires clean reviewed Sonatina", result.stderr)

    def test_browser_call_sites_share_one_patch_owner(self) -> None:
        cga = (DEMOS / "webgpu-cga-inversion" / "generate.sh").read_text()
        qcga = (DEMOS / "webgpu-qcga3d-quadric" / "generate.sh").read_text()
        serve = (DEMOS / "serve.sh").read_text()
        self.assertIn('if [ "$bundle" = schedule32 ]', cga)
        self.assertIn('"$repo/demos/with-browser-cargo.sh"', cga)
        self.assertIn('"$repo/demos/with-browser-cargo.sh"', qcga)
        self.assertIn('"$here/with-browser-cargo.sh"', serve)
        self.assertNotIn('patch.\\"https://github.com/micahscopes/sonatina', qcga)
        self.assertNotIn('patch.\\"https://github.com/micahscopes/sonatina', serve)
        # The only remaining local patches are the four-crate legacy D1 block,
        # whose ed43625b backend deliberately predates the browser runner.
        self.assertEqual(cga.count('patch.\\"https://github.com/micahscopes/sonatina'), 4)


if __name__ == "__main__":
    unittest.main()
