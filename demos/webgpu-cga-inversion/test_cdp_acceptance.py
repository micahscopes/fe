import json
import unittest

from cdp_acceptance import acceptance_passes, encode_client_frame, parse_frame_bytes


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


if __name__ == "__main__":
    unittest.main()
