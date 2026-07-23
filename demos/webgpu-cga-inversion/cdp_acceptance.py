#!/usr/bin/env python3
"""Poll a Chrome page's structured typed-CGA result over CDP, stdlib only."""

import argparse
import base64
import hashlib
import json
import os
import socket
import struct
import time
import urllib.parse
import urllib.request


def encode_client_frame(text, mask_key=None):
    payload = text.encode("utf-8")
    mask = mask_key or os.urandom(4)
    length = len(payload)
    if length < 126:
        header = bytes([0x81, 0x80 | length])
    elif length <= 0xFFFF:
        header = bytes([0x81, 0xFE]) + struct.pack("!H", length)
    else:
        header = bytes([0x81, 0xFF]) + struct.pack("!Q", length)
    masked = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
    return header + mask + masked


def parse_frame_bytes(data):
    if len(data) < 2:
        return None
    first, second = data[0], data[1]
    length = second & 0x7F
    offset = 2
    if length == 126:
        if len(data) < 4:
            return None
        length = struct.unpack("!H", data[2:4])[0]
        offset = 4
    elif length == 127:
        if len(data) < 10:
            return None
        length = struct.unpack("!Q", data[2:10])[0]
        offset = 10
    masked = bool(second & 0x80)
    mask = b""
    if masked:
        if len(data) < offset + 4:
            return None
        mask = data[offset:offset + 4]
        offset += 4
    if len(data) < offset + length:
        return None
    payload = data[offset:offset + length]
    if masked:
        payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
    return first & 0x0F, payload, offset + length


def acceptance_passes(value, expected_presentation, expected_state="green"):
    if not isinstance(value, dict):
        return False
    if value.get("state") != expected_state or value.get("presentation") != expected_presentation:
        return False
    if expected_state == "presentation":
        return (
            value.get("verified") is False
            and "wasmHash" not in value
            and "gpuHash" not in value
        )
    return True


class WebSocket:
    def __init__(self, url, timeout):
        parsed = urllib.parse.urlparse(url)
        self.sock = socket.create_connection((parsed.hostname, parsed.port), timeout=timeout)
        self.sock.settimeout(timeout)
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        path = parsed.path + (("?" + parsed.query) if parsed.query else "")
        request = (
            f"GET {path} HTTP/1.1\r\nHost: {parsed.netloc}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        self.sock.sendall(request.encode("ascii"))
        response = b""
        while b"\r\n\r\n" not in response:
            response += self.sock.recv(4096)
        if not response.startswith(b"HTTP/1.1 101"):
            raise RuntimeError(f"CDP WebSocket upgrade failed: {response[:120]!r}")
        expected = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode("ascii")).digest()
        )
        if expected.lower() not in response.lower():
            raise RuntimeError("CDP WebSocket handshake returned an invalid accept key")
        self.buffer = response.split(b"\r\n\r\n", 1)[1]

    def send_json(self, value):
        self.sock.sendall(encode_client_frame(json.dumps(value, separators=(",", ":"))))

    def recv_json(self):
        while True:
            parsed = parse_frame_bytes(self.buffer)
            if parsed is None:
                chunk = self.sock.recv(65536)
                if not chunk:
                    raise RuntimeError("CDP WebSocket closed")
                self.buffer += chunk
                continue
            opcode, payload, consumed = parsed
            self.buffer = self.buffer[consumed:]
            if opcode == 1:
                return json.loads(payload)
            if opcode == 8:
                raise RuntimeError("CDP WebSocket sent a close frame")
            if opcode == 9:
                self.sock.sendall(bytes([0x8A, len(payload)]) + payload)

    def close(self):
        self.sock.close()


def find_page(debug_port, page_url, deadline):
    endpoint = f"http://127.0.0.1:{debug_port}/json/list"
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(endpoint, timeout=0.5) as response:
                targets = json.load(response)
            for target in targets:
                if target.get("type") == "page" and target.get("url", "").startswith(page_url):
                    return target["webSocketDebuggerUrl"]
        except Exception:
            pass
        time.sleep(0.1)
    raise TimeoutError(f"Chrome did not expose the typed-CGA page over CDP: {page_url}")


def command(ws, command_id, method, params, deadline):
    """Run one CDP command, retrying the page-creation context race."""
    while time.monotonic() < deadline:
        ws.send_json({"id": command_id, "method": method, "params": params})
        while time.monotonic() < deadline:
            response = ws.recv_json()
            if response.get("id") != command_id:
                continue
            error = response.get("error")
            if error is not None:
                message = error.get("message", "") if isinstance(error, dict) else str(error)
                if method == "Runtime.evaluate" and "default execution context" in message:
                    time.sleep(0.1)
                    break
                raise RuntimeError(error)
            return response.get("result", {})
    raise TimeoutError(f"CDP command {method} timed out")


def poll_acceptance(debug_port, page_url, expected_presentation, expected_state, timeout):
    deadline = time.monotonic() + timeout
    ws = WebSocket(find_page(debug_port, page_url, deadline), max(1, timeout))
    command_id = 0
    try:
        while time.monotonic() < deadline:
            command_id += 1
            result = command(
                ws, command_id, "Runtime.evaluate", {
                    "expression": "JSON.stringify(window.__cgaAcceptance || null)",
                    "returnByValue": True,
                },
                deadline,
            )
            raw = result.get("result", {}).get("value", "null")
            value = json.loads(raw) if isinstance(raw, str) else None
            if isinstance(value, dict) and value.get("state") != "pending":
                print(json.dumps(value, sort_keys=True))
                return acceptance_passes(value, expected_presentation, expected_state)
            time.sleep(0.1)
    finally:
        ws.close()
    raise TimeoutError("typed-CGA browser result remained pending until the deadline")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--debug-port", type=int, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--presentation", choices=["offscreen", "canvas"], required=True)
    parser.add_argument("--expected-state", choices=["green", "presentation"], default="green")
    parser.add_argument("--timeout", type=float, default=90)
    args = parser.parse_args()
    try:
        passed = poll_acceptance(
            args.debug_port, args.url, args.presentation, args.expected_state, args.timeout
        )
    except Exception as error:
        raise SystemExit(f"CDP acceptance failed: {error}")
    if not passed:
        raise SystemExit(
            f"CDP result was not {args.expected_state!r} in the expected presentation mode"
        )


if __name__ == "__main__":
    main()
