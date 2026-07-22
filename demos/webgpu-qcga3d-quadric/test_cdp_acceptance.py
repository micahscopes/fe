import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cdp_acceptance import valid


class QcgaAcceptanceTests(unittest.TestCase):
    def test_verified_contract_requires_exact_worker_gpu_evidence(self):
        value = {
            "state": "green",
            "presentation": "offscreen",
            "verified": True,
            "pixels": 16384,
            "wasmHash": 2368784280,
            "gpuHash": 2368784280,
            "counters": {
                "fetches": ["gen/layout.json", "gen/frag.wgsl", "gen/kernel.fe",
                            "gen/reference.json", "gen/frag.wasm"],
                "workerCreates": 1,
                "readbacks": 1,
            },
        }
        self.assertTrue(valid(value, "verify"))
        for field, bad in (("gpuHash", 0), ("pixels", 1), ("verified", False)):
            with self.subTest(field=field):
                self.assertFalse(valid({**value, field: bad}, "verify"))

    def test_presentation_contract_forbids_wasm_and_readback(self):
        value = {
            "state": "presentation",
            "presentation": "canvas",
            "verified": False,
            "counters": {
                "fetches": ["gen/layout.json", "gen/frag.wgsl", "gen/kernel.fe"],
                "workerCreates": 0,
                "readbacks": 0,
            },
        }
        self.assertTrue(valid(value, "off"))
        value["counters"]["fetches"].append("gen/frag.wasm")
        self.assertFalse(valid(value, "off"))


if __name__ == "__main__":
    unittest.main()
