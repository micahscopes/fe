import os
import pathlib
import shutil
import stat
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
OVERLAY = ROOT / "demos" / "with-sonatina-overlay.sh"
KNOWN_SOURCE = pathlib.Path(
    os.environ.get("SONATINA_TEST_SOURCE", "/workspace/sonatina-host-abi")
)


def run_overlay(*args, env=None):
    return subprocess.run(
        [str(OVERLAY), *map(str, args)],
        env={**os.environ, **(env or {})},
        text=True,
        capture_output=True,
    )


class SonatinaOverlayTests(unittest.TestCase):
    def require_source(self):
        if not (KNOWN_SOURCE / ".git").exists():
            self.skipTest("set SONATINA_TEST_SOURCE to a complete Sonatina checkout")
        return KNOWN_SOURCE

    def test_requires_a_command(self):
        result = run_overlay()
        self.assertEqual(result.returncode, 2)
        self.assertIn("usage:", result.stderr)

    def test_offline_cache_miss_fails_without_running_command(self):
        with tempfile.TemporaryDirectory() as temporary:
            marker = pathlib.Path(temporary) / "ran"
            result = run_overlay(
                "sh",
                "-c",
                f"touch {marker}",
                env={
                    "FE_BROWSER_CACHE_DIR": temporary,
                    "FE_DEMO_TMPDIR": temporary,
                    "FE_BROWSER_OFFLINE": "1",
                    "SONATINA_DIR": "",
                },
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn("offline cache is missing", result.stderr)
            self.assertFalse(marker.exists())

    def test_checksum_failure_precedes_source_or_command_use(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            (root / "demos").mkdir()
            shutil.copy2(OVERLAY, root / "demos" / OVERLAY.name)
            archive = root / "vendor" / "sonatina" / "mb2-browser-runtime"
            shutil.copytree(
                ROOT / "vendor" / "sonatina" / "mb2-browser-runtime", archive
            )
            with next(archive.glob("0032-*.patch")).open("ab") as patch:
                patch.write(b"\ncorrupt\n")
            result = subprocess.run(
                [str(root / "demos" / OVERLAY.name), "sh", "-c", "exit 99"],
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("checksum mismatch", result.stderr)

    def test_dirty_source_fails_without_mutating_it(self):
        source = self.require_source()
        with tempfile.TemporaryDirectory() as temporary:
            checkout = pathlib.Path(temporary) / "source"
            subprocess.run(
                ["git", "clone", "--quiet", "--shared", str(source), str(checkout)],
                check=True,
            )
            marker = checkout / "untracked-by-test"
            marker.write_text("keep me\n")
            before = subprocess.check_output(
                ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True
            )
            result = run_overlay(
                "sh",
                "-c",
                "exit 99",
                env={"SONATINA_DIR": str(checkout), "FE_DEMO_TMPDIR": temporary},
            )
            after = subprocess.check_output(
                ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn("must be clean", result.stderr)
            self.assertEqual(before, after)
            self.assertEqual(marker.read_text(), "keep me\n")

    def test_command_arguments_and_cargo_overrides_are_scoped(self):
        source = self.require_source()
        with tempfile.TemporaryDirectory() as temporary:
            temporary = pathlib.Path(temporary)
            fake_cargo = temporary / "cargo"
            output = temporary / "args"
            fake_cargo.write_text(
                "#!/bin/sh\n"
                'printf "%s\\n" "$FE_SONATINA_OVERLAY_ACTIVE" "$SONATINA_DIR" "$@" '
                f"> {output}\n"
            )
            fake_cargo.chmod(fake_cargo.stat().st_mode | stat.S_IXUSR)
            result = run_overlay(
                fake_cargo,
                "check",
                "--package",
                "sentinel package",
                env={"SONATINA_DIR": str(source), "FE_DEMO_TMPDIR": str(temporary)},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = output.read_text().splitlines()
            self.assertEqual(lines[0], "1")
            self.assertEqual(lines[-3:], ["check", "--package", "sentinel package"])
            joined = "\n".join(lines)
            for crate in (
                "sonatina-ir",
                "sonatina-triple",
                "sonatina-codegen",
                "sonatina-verifier",
            ):
                self.assertIn(crate, joined)
            self.assertFalse(pathlib.Path(lines[1]).exists(), "overlay must be cleaned")

    def test_offline_warm_cache_reconstructs_without_fetching(self):
        source = self.require_source()
        with tempfile.TemporaryDirectory() as temporary:
            temporary = pathlib.Path(temporary)
            cache = temporary / "cache"
            bare = cache / "sonatina.git"
            cache.mkdir()
            subprocess.run(
                ["git", "clone", "--quiet", "--bare", str(source), str(bare)],
                check=True,
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(bare),
                    "remote",
                    "set-url",
                    "origin",
                    "https://github.com/micahscopes/sonatina.git",
                ],
                check=True,
            )
            result = run_overlay(
                "sh",
                "-c",
                'test "$FE_SONATINA_OVERLAY_ACTIVE" = 1',
                env={
                    "SONATINA_DIR": "",
                    "FE_BROWSER_CACHE_DIR": str(cache),
                    "FE_DEMO_TMPDIR": str(temporary),
                    "FE_BROWSER_OFFLINE": "1",
                    "GIT_CONFIG_NOSYSTEM": "1",
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
