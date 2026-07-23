#!/usr/bin/env python3
"""Drive the generated Schedule32 actor chain through lifecycle pressure in Chrome."""

import argparse
import json
import math
import time

from cdp_acceptance import WebSocket, command, find_page


def lifecycle_passes(value):
    if not isinstance(value, dict):
        return False
    required = [
        "baseline", "afterBurst", "afterCancel", "afterRestart",
        "afterCompletion", "final", "verification", "acceptance", "timing",
    ]
    if any(not isinstance(value.get(name), dict) for name in required):
        return False
    baseline = value["baseline"]
    final = value["final"]
    evidence = final.get("evidence")
    timing = value["timing"]
    if not isinstance(evidence, dict):
        return False
    if value["acceptance"].get("state") != "green":
        return False
    if value["verification"].get("state") != "green":
        return False

    baseline_readbacks = baseline.get("readbacks")
    if not isinstance(baseline_readbacks, int) or baseline_readbacks < 1:
        return False
    for phase in ("afterBurst", "afterCancel", "afterRestart", "afterCompletion"):
        if value[phase].get("readbacks") != baseline_readbacks:
            return False
        if value[phase].get("timestampReadbacks") != 0:
            return False
    if final.get("readbacks") != baseline_readbacks + 1:
        return False
    if final.get("timestampReadbacks") != 0:
        return False

    interactions = final.get("interactionCount", 0) - baseline.get("interactionCount", 0)
    if interactions < 32:
        return False
    requested = evidence.get("renderRequested", 0) - baseline["evidence"].get(
        "renderRequested", 0
    )
    submitted = evidence.get("gpuSubmittedCount", 0) - baseline["evidence"].get(
        "gpuSubmittedCount", 0
    )
    if requested < interactions or not 1 <= submitted < interactions:
        return False
    if evidence.get("renderDropped", 0) <= baseline["evidence"].get("renderDropped", 0):
        return False
    if evidence.get("maxLifecycleRenderActive") != 1:
        return False
    if evidence.get("maxLifecycleRenderPending") != 1:
        return False
    if not 0 <= evidence.get("maxWorkerPending", -1) <= 8:
        return False
    if evidence.get("actorBackpressureErrors") != 0:
        return False
    if evidence.get("actorAborted") != 1:
        return False
    if evidence.get("workerRestartCount") != 1:
        return False
    if final.get("workerEpoch") != baseline.get("workerEpoch", -1) + 1:
        return False
    if evidence.get("workerEpoch") != final.get("workerEpoch"):
        return False
    if evidence.get("staleRenderPublished") != 0:
        return False

    generation = final.get("generation")
    if not isinstance(generation, int):
        return False
    for field in (
        "lastSubmittedGeneration", "lastCompletedGeneration", "lastPublishedGeneration"
    ):
        if evidence.get(field) != generation:
            return False
    if evidence.get("gpuSubmittedCount", 0) <= baseline["evidence"].get(
        "gpuSubmittedCount", 0
    ):
        return False
    if evidence.get("gpuCompletedCount") != evidence.get("gpuSubmittedCount"):
        return False
    if evidence.get("verificationRequestedCount") != 1:
        return False

    cadence = timing.get("submittedFrameCadenceHz")
    return (
        isinstance(cadence, (int, float))
        and math.isfinite(cadence)
        and cadence >= 0
        and timing.get("completedFrameCadenceHz") is None
        and timing.get("gpuCompletionMeasured") is False
        and timing.get("completionCheckpointAwaited") is True
    )


def run_lifecycle(debug_port, page_url, timeout):
    deadline = time.monotonic() + timeout
    ws = WebSocket(find_page(debug_port, page_url, deadline), max(1, timeout))
    try:
        command_id = 0
        while time.monotonic() < deadline:
            command_id += 1
            result = command(ws, command_id, "Runtime.evaluate", {
                "expression": "JSON.stringify({acceptance: window.__cgaAcceptance || null, ready: !!window.__cgaLifecycleSmoke})",
                "returnByValue": True,
            }, deadline)
            raw = result.get("result", {}).get("value", "null")
            state = json.loads(raw) if isinstance(raw, str) else None
            acceptance = state.get("acceptance") if isinstance(state, dict) else None
            if state and state.get("ready") and acceptance.get("state") == "green":
                break
            if isinstance(acceptance, dict) and acceptance.get("state") not in (None, "pending"):
                raise RuntimeError(f"lifecycle startup failed: {acceptance}")
            time.sleep(0.1)
        else:
            raise TimeoutError("lifecycle smoke did not become ready")

        command_id += 1
        result = command(ws, command_id, "Runtime.evaluate", {
            "expression": """
              (async () => {
                const hook = window.__cgaLifecycleSmoke;
                const baseline = hook.snapshot();
                const canvas = document.getElementById("view");
                const rect = canvas.getBoundingClientRect();
                for (let i = 0; i < 48; i++) {
                  canvas.dispatchEvent(new PointerEvent("pointermove", {
                    bubbles: true,
                    isPrimary: true,
                    pointerId: 1,
                    clientX: rect.left + 8 + (i % 24) * 4,
                    clientY: rect.top + 12 + (i % 16) * 3,
                  }));
                }
                const pressured = hook.snapshot();
                await hook.waitForIdle();
                const afterBurst = hook.snapshot();
                const afterCancel = await hook.cancelOracle();
                const afterRestart = await hook.restartWorker();
                const afterCompletion = await hook.awaitGpuCompletion();
                const checked = await hook.verifyCurrentExplicitly();
                for (let i = 0; i < 3; i++) {
                  await new Promise(requestAnimationFrame);
                }
                const final = hook.snapshot();
                const interaction = window.__cgaPerformance.interaction;
                return JSON.stringify({
                  baseline,
                  pressured,
                  afterBurst,
                  afterCancel,
                  afterRestart,
                  afterCompletion,
                  final,
                  verification: checked.result,
                  acceptance: window.__cgaAcceptance,
                  timing: {
                    submittedFrameCadenceHz: interaction.cadenceHz ?? 0,
                    completedFrameCadenceHz: null,
                    gpuCompletionMeasured: false,
                    completionCheckpointAwaited: true,
                  },
                });
              })()
            """,
            "awaitPromise": True,
            "returnByValue": True,
        }, deadline)
        raw = result.get("result", {}).get("value", "null")
        value = json.loads(raw) if isinstance(raw, str) else None
        print(json.dumps(value, sort_keys=True))
        return lifecycle_passes(value)
    finally:
        ws.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--debug-port", type=int, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--timeout", type=float, default=90)
    args = parser.parse_args()
    try:
        passed = run_lifecycle(args.debug_port, args.url, args.timeout)
    except Exception as error:
        raise SystemExit(f"CDP lifecycle smoke failed: {error}")
    if not passed:
        raise SystemExit("generated actor lifecycle contract did not pass")


if __name__ == "__main__":
    main()
