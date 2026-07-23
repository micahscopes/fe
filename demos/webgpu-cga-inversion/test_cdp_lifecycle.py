import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cdp_lifecycle import lifecycle_passes


def snapshot(generation=50, epoch=1, readbacks=1):
    return {
        "generation": generation,
        "coordinator": {
            "generation": generation,
            "render": {"active": None, "pending": None},
            "verify": {"active": None, "pending": None},
        },
        "workerEpoch": epoch,
        "workerPending": 0,
        "readbacks": readbacks,
        "timestampReadbacks": 0,
        "interactionCount": 48,
        "evidence": {
            "renderRequested": 50,
            "renderSettled": 50,
            "renderDropped": 46,
            "renderPublished": 3,
            "staleRenderPublished": 0,
            "actorAborted": 1,
            "actorBackpressureErrors": 0,
            "maxLifecycleRenderActive": 1,
            "maxLifecycleRenderPending": 1,
            "maxWorkerPending": 1,
            "workerRestartCount": 1,
            "workerEpoch": epoch,
            "gpuSubmittedCount": 3,
            "gpuCompletedCount": 3,
            "lastSubmittedGeneration": generation,
            "lastCompletedGeneration": generation,
            "lastPublishedGeneration": generation,
            "verificationRequestedCount": 1,
        },
    }


def passing_value():
    baseline = snapshot(generation=1, epoch=0)
    baseline["interactionCount"] = 0
    baseline["evidence"].update({
        "renderRequested": 1,
        "renderSettled": 1,
        "renderDropped": 0,
        "renderPublished": 1,
        "actorAborted": 0,
        "workerRestartCount": 0,
        "workerEpoch": 0,
        "gpuSubmittedCount": 1,
        "gpuCompletedCount": 1,
        "lastSubmittedGeneration": 1,
        "lastCompletedGeneration": 1,
        "lastPublishedGeneration": 1,
        "verificationRequestedCount": 0,
    })
    return {
        "baseline": baseline,
        "afterBurst": snapshot(),
        "afterCancel": snapshot(),
        "afterRestart": snapshot(),
        "afterCompletion": snapshot(),
        "final": snapshot(readbacks=2),
        "verification": {"state": "green", "generation": 50},
        "acceptance": {"state": "green", "generation": 50},
        "timing": {
            "submittedFrameCadenceHz": 60.0,
            "completedFrameCadenceHz": None,
            "gpuCompletionMeasured": False,
            "completionCheckpointAwaited": True,
        },
    }


class LifecyclePredicateTests(unittest.TestCase):
    def test_accepts_bounded_recovered_zero_implicit_readback_run(self):
        self.assertTrue(lifecycle_passes(passing_value()))

    def test_rejects_stale_unbounded_or_unrecovered_actor_state(self):
        for field, bad in (
            ("staleRenderPublished", 1),
            ("maxLifecycleRenderPending", 2),
            ("maxWorkerPending", 9),
            ("actorAborted", 0),
            ("workerRestartCount", 0),
            ("gpuCompletedCount", 2),
        ):
            with self.subTest(field=field):
                value = passing_value()
                value["final"]["evidence"][field] = bad
                self.assertFalse(lifecycle_passes(value))

    def test_rejects_hidden_readback_and_gpu_completion_claim(self):
        value = passing_value()
        value["afterRestart"]["readbacks"] += 1
        self.assertFalse(lifecycle_passes(value))
        value = passing_value()
        value["timing"]["gpuCompletionMeasured"] = True
        self.assertFalse(lifecycle_passes(value))

    def test_rejects_failure_to_coalesce(self):
        value = passing_value()
        value["final"]["evidence"]["gpuSubmittedCount"] = 49
        value["final"]["evidence"]["gpuCompletedCount"] = 49
        self.assertFalse(lifecycle_passes(value))


if __name__ == "__main__":
    unittest.main()
