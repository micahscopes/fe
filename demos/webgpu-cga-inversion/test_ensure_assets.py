import os
import subprocess
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "ensure-assets.sh"
ASSETS = ("kernel.fe", "frag.wgsl", "layout.json", "reference.json", "frag.wasm")


def executable(path: Path, body: str) -> Path:
    path.write_text("#!/usr/bin/env bash\nset -euo pipefail\n" + body)
    path.chmod(0o755)
    return path


class EnsureAssetsTests(unittest.TestCase):
    def run_script(self, bundle: Path, **extra: str) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env.update({"CGA_BUNDLE_DIR": str(bundle), **extra})
        return subprocess.run([SCRIPT], env=env, text=True, capture_output=True)

    def test_complete_bundle_is_verified_without_generation(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "gen"
            bundle.mkdir()
            for asset in ASSETS:
                (bundle / asset).touch()
            marker = root / "verified"
            verify = executable(root / "verify", f"touch {marker!s}\n")
            result = self.run_script(bundle, CGA_VERIFY_CMD=str(verify))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(marker.exists())

    def test_partial_bundle_reports_every_missing_asset_and_pinned_checkout_hint(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = Path(raw) / "gen"
            bundle.mkdir()
            (bundle / "layout.json").touch()
            result = self.run_script(bundle, SONATINA_DIR="")
            self.assertEqual(result.returncode, 2)
            self.assertIn("kernel.fe", result.stderr)
            self.assertIn("frag.wasm", result.stderr)
            self.assertIn("pinned local Sonatina", result.stderr)

    def test_schedule32_bundle_requires_generated_actor_contract(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = Path(raw) / "gen-schedule32"
            bundle.mkdir()
            for asset in ASSETS:
                (bundle / asset).touch()
            result = self.run_script(bundle, CGA_BUNDLE="schedule32", SONATINA_DIR="")
            self.assertEqual(result.returncode, 2)
            self.assertIn("actor/module.wasm", result.stderr)
            self.assertIn("actor/interface.js", result.stderr)
            self.assertIn("actor/manifest.json", result.stderr)

    def test_missing_bundle_can_be_generated_then_verified_in_one_command(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "gen"
            bundle.mkdir()
            generator = executable(
                root / "generate",
                "for asset in kernel.fe frag.wgsl layout.json reference.json frag.wasm; "
                f"do touch {bundle!s}/\"$asset\"; done\n",
            )
            marker = root / "verified"
            verify = executable(root / "verify", f"touch {marker!s}\n")
            result = self.run_script(
                bundle,
                CGA_GENERATE_CMD=str(generator),
                CGA_VERIFY_CMD=str(verify),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(marker.exists())
            self.assertTrue(all((bundle / asset).is_file() for asset in ASSETS))


if __name__ == "__main__":
    unittest.main()
