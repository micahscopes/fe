import json
import sys
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from cdp_acceptance import acceptance_passes, command, encode_client_frame, parse_frame_bytes


class FakeWebSocket:
    def __init__(self, responses):
        self.responses = iter(responses)
        self.sent = []

    def send_json(self, value):
        self.sent.append(value)

    def recv_json(self):
        return next(self.responses)


class CdpAcceptanceTests(unittest.TestCase):
    def test_masked_client_frame_round_trips(self):
        message = json.dumps({"id": 7, "method": "Runtime.evaluate"})
        frame = encode_client_frame(message, b"\x01\x02\x03\x04")
        opcode, payload, consumed = parse_frame_bytes(frame)
        self.assertEqual(opcode, 1)
        self.assertEqual(payload.decode(), message)
        self.assertEqual(consumed, len(frame))

    def test_incomplete_frame_waits_for_more_bytes(self):
        frame = encode_client_frame("x" * 200, b"mask")
        self.assertIsNone(parse_frame_bytes(frame[:-1]))
        self.assertEqual(parse_frame_bytes(frame)[1], b"x" * 200)

    def test_acceptance_requires_green_expected_presentation(self):
        self.assertTrue(acceptance_passes({"state": "green", "presentation": "offscreen"}, "offscreen"))
        self.assertFalse(acceptance_passes({"state": "pending", "presentation": "offscreen"}, "offscreen"))
        self.assertFalse(acceptance_passes({"state": "green", "presentation": "canvas"}, "offscreen"))

    def test_unverified_presentation_contract_forbids_acceptance_hashes(self):
        value = {"state": "presentation", "presentation": "offscreen", "verified": False}
        self.assertTrue(acceptance_passes(value, "offscreen", "presentation"))
        self.assertFalse(acceptance_passes({**value, "verified": True}, "offscreen", "presentation"))
        self.assertFalse(acceptance_passes({**value, "wasmHash": 1}, "offscreen", "presentation"))
        self.assertFalse(acceptance_passes({**value, "gpuHash": 1}, "offscreen", "presentation"))
        self.assertFalse(acceptance_passes({**value, "state": "green"}, "offscreen", "presentation"))

    def test_runtime_evaluate_retries_missing_default_context(self):
        ws = FakeWebSocket([
            {"id": 7, "error": {"code": -32000,
                                "message": "Cannot find default execution context"}},
            {"id": 7, "result": {"result": {"value": "ready"}}},
        ])
        result = command(
            ws, 7, "Runtime.evaluate", {"expression": "1"},
            time.monotonic() + 2,
        )
        self.assertEqual(result, {"result": {"value": "ready"}})
        self.assertEqual(len(ws.sent), 2)

    def test_nontransient_cdp_error_fails_without_retry(self):
        ws = FakeWebSocket([
            {"id": 8, "error": {"code": -32000, "message": "evaluation failed"}},
        ])
        with self.assertRaisesRegex(RuntimeError, "evaluation failed"):
            command(
                ws, 8, "Runtime.evaluate", {"expression": "1"},
                time.monotonic() + 2,
            )
        self.assertEqual(len(ws.sent), 1)


if __name__ == "__main__":
    unittest.main()
