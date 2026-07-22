import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cdp_presentation import presentation_passes


def passing_value():
    return {
        "acceptance": {"state": "presentation", "presentation": "canvas",
                       "verified": False, "adapter": "SwiftShader"},
        "evidence": {
            "verificationOff": True,
            "fetchedAssets": ["./gen/layout.json", "./gen/kernel.fe", "./gen/frag.wgsl"],
            "wasmWorkerCreated": False,
            "wasmOracleRenderCount": 0,
            "gpuReadbackCount": 0,
            "interactionCount": 17,
        },
        "performance": {
            "artifactFetchMs": 3.0,
            "gpuInitMs": 12.0,
            "firstFrameSubmitMs": 0.5,
            "initialAcceptanceMs": None,
            "frames": {
                "count": 17,
                "sampleCount": 16,
                "fps": 58.2,
                "lastSubmitCpuMs": 0.2,
                "averageSubmitCpuMs": 0.3,
                "maxSubmitCpuMs": 0.7,
            },
        },
    }


class PresentationPredicateTests(unittest.TestCase):
    def test_accepts_complete_zero_readback_measurement(self):
        self.assertTrue(presentation_passes(passing_value()))

    def test_rejects_oracle_worker_wasm_fetch_and_readback(self):
        cases = [
            ("wasmWorkerCreated", True),
            ("wasmOracleRenderCount", 1),
            ("gpuReadbackCount", 1),
        ]
        for field, bad in cases:
            with self.subTest(field=field):
                value = passing_value()
                value["evidence"][field] = bad
                self.assertFalse(presentation_passes(value))
        value = passing_value()
        value["evidence"]["fetchedAssets"].append("./gen/frag.wasm")
        self.assertFalse(presentation_passes(value))

    def test_rejects_missing_samples_or_unbounded_submit_cpu(self):
        for field, bad in [("sampleCount", 7), ("averageSubmitCpuMs", 1000)]:
            with self.subTest(field=field):
                value = passing_value()
                value["performance"]["frames"][field] = bad
                self.assertFalse(presentation_passes(value))


if __name__ == "__main__":
    unittest.main()
