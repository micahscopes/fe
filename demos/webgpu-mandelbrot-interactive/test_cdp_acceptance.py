import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cdp_acceptance import acceptance_passes


GREEN = {
    "state": "green",
    "worker": True,
    "presentation": "offscreen",
    "controlsSteps": 4007,
    "verified": True,
    "gpuHash": 123,
    "wasmHash": 123,
    "referenceHash": 123,
    "adapter": "SwiftShader",
}


class AcceptancePredicateTests(unittest.TestCase):
    def test_requires_complete_worker_oracle_and_render_contract(self):
        self.assertTrue(acceptance_passes(GREEN))
        for field, bad in [
            ("state", "amber"),
            ("worker", False),
            ("presentation", "canvas"),
            ("controlsSteps", 4006),
            ("verified", False),
            ("gpuHash", 124),
            ("wasmHash", 124),
            ("referenceHash", 124),
            ("adapter", ""),
        ]:
            with self.subTest(field=field):
                self.assertFalse(acceptance_passes({**GREEN, field: bad}))

    def test_rejects_non_object_and_pending(self):
        self.assertFalse(acceptance_passes(None))
        self.assertFalse(acceptance_passes({"state": "pending"}))


if __name__ == "__main__":
    unittest.main()
