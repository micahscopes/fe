import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cdp_continuous_benchmark import benchmark_passes


def passing_value(timing="submit"):
    gpu = timing == "gpu"
    benchmark = {
        "mode": "continuous_gpu_timestamp" if gpu else "continuous_no_readback",
        "path": "direct",
        "resolution": {"width": 256, "height": 256, "pixels": 65536},
        "warmupFrames": 30,
        "sampleFrames": 120,
        "submittedFrameCadenceHz": None if gpu else 60.0,
        "completedFrameCadenceHz": 50.0 if gpu else None,
        "averageSubmitCpuMs": 0.2,
        "maxSubmitCpuMs": 0.7,
        "gpuCompletionMeasured": gpu,
        "timestampQuery": ({
            "available": True,
            "unit": "nanoseconds",
            "averageGpuElapsedMs": 3.0,
            "minGpuElapsedMs": 2.0,
            "maxGpuElapsedMs": 4.0,
            "samples": 120,
        } if gpu else {
            "available": False,
            "reason": "not requested; benchmark performs no GPU readback",
        }),
    }
    return {
        "acceptance": {
            "state": "presentation", "presentation": "canvas",
            "verified": False, "benchmark": copy.deepcopy(benchmark),
        },
        "benchmark": benchmark,
        "evidence": {
            "verificationOff": True,
            "fetchedAssets": [
                "./gen-schedule32/layout.json",
                "./gen-schedule32/kernel.fe",
                "./gen-schedule32/frag.wgsl",
            ],
            "wasmWorkerCreated": False,
            "wasmOracleRenderCount": 0,
            "gpuReadbackCount": 150 if gpu else 0,
            "gpuTimestampReadbackCount": 150 if gpu else 0,
        },
    }


class ContinuousBenchmarkPredicateTests(unittest.TestCase):
    def test_accepts_strict_no_readback_contract(self):
        self.assertTrue(benchmark_passes(passing_value()))

    def test_rejects_each_forbidden_readback_or_oracle_activity(self):
        for field, bad in (
            ("wasmWorkerCreated", True),
            ("wasmOracleRenderCount", 1),
            ("gpuReadbackCount", 1),
            ("gpuTimestampReadbackCount", 1),
        ):
            with self.subTest(field=field):
                value = passing_value()
                value["evidence"][field] = bad
                self.assertFalse(benchmark_passes(value))
        value = passing_value()
        value["evidence"]["fetchedAssets"].append("./gen-schedule32/actor/module.wasm")
        self.assertFalse(benchmark_passes(value))

    def test_requires_exact_measurement_shape_and_acceptance_copy(self):
        for path, bad in (
            (("benchmark", "sampleFrames"), 119),
            (("benchmark", "path"), "actor"),
            (("benchmark", "submittedFrameCadenceHz"), 0),
            (("acceptance", "verified"), True),
        ):
            with self.subTest(path=path):
                value = passing_value()
                value[path[0]][path[1]] = bad
                self.assertFalse(benchmark_passes(value))

    def test_accepts_explicit_gpu_timestamp_contract(self):
        self.assertTrue(benchmark_passes(passing_value("gpu"), "gpu"))

    def test_gpu_timing_requires_all_150_timestamp_readbacks(self):
        value = passing_value("gpu")
        value["evidence"]["gpuTimestampReadbackCount"] = 149
        self.assertFalse(benchmark_passes(value, "gpu"))


if __name__ == "__main__":
    unittest.main()
