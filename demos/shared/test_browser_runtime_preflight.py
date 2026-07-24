import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from browser_runtime_preflight import (
    RUNTIME_PATHS,
    RUNTIME_PROTOCOL_VERSION,
    validate_browser_runtime,
    validate_shared_browser_runtime,
)


def fixture(root, suffix=b""):
    artifacts = []
    for index, relative in enumerate(RUNTIME_PATHS):
        payload = f"generic runtime module {index}".encode() + suffix
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
        artifacts.append({
            "path": relative,
            "bytes": len(payload),
            "sha256": hashlib.sha256(payload).hexdigest(),
        })
    return {
        "browser_runtime": {
            "protocol": "fe-browser-actor-runtime",
            "protocol_version": RUNTIME_PROTOCOL_VERSION,
            "artifacts": artifacts,
        },
    }


class BrowserRuntimePreflightTests(unittest.TestCase):
    def test_two_application_bundles_share_one_content_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            cga = tmp / "cga"
            qcga = tmp / "qcga"
            identity = validate_shared_browser_runtime({
                "cga": (fixture(cga), cga),
                "qcga": (fixture(qcga), qcga),
            })
            self.assertEqual(identity[0], "fe-browser-actor-runtime")
            self.assertEqual(identity[1], RUNTIME_PROTOCOL_VERSION)
            self.assertEqual(len(identity[2]), len(RUNTIME_PATHS))

    def test_accepts_v4_and_rejects_stale_packaged_runtime_versions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = fixture(root)
            self.assertEqual(
                validate_browser_runtime(manifest, root)[1],
                RUNTIME_PROTOCOL_VERSION,
            )
            manifest["browser_runtime"]["protocol_version"] = (
                RUNTIME_PROTOCOL_VERSION - 1
            )
            with self.assertRaises(AssertionError):
                validate_browser_runtime(manifest, root)

    def test_rejects_divergent_application_runtime(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            cga = tmp / "cga"
            qcga = tmp / "qcga"
            with self.assertRaisesRegex(AssertionError, "not exercising one packaged runtime"):
                validate_shared_browser_runtime({
                    "cga": (fixture(cga), cga),
                    "qcga": (fixture(qcga, b" changed"), qcga),
                })

    def test_rejects_tampering_and_noncanonical_module_inventory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = fixture(root)
            (root / RUNTIME_PATHS[0]).write_text("tampered")
            with self.assertRaisesRegex(AssertionError, "byte count differs"):
                validate_browser_runtime(manifest, root)
            manifest = fixture(root)
            manifest["browser_runtime"]["artifacts"].reverse()
            with self.assertRaises(AssertionError):
                validate_browser_runtime(manifest, root)


if __name__ == "__main__":
    unittest.main()
