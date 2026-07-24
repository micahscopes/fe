"""Fail-closed validation for compiler-packaged browser actor runtimes."""

import hashlib
from pathlib import Path


RUNTIME_PROTOCOL = "fe-browser-actor-runtime"
RUNTIME_PROTOCOL_VERSION = 3
RUNTIME_PATHS = (
    "runtime/actor-coordinator.js",
    "runtime/actor-endpoint.js",
    "runtime/actor-router.js",
    "runtime/gpu-actor.js",
    "runtime/message-port-actor.js",
    "runtime/module-worker-actor.js",
    "runtime/worker-host.js",
    "runtime/actor-client.js",
)


def validate_browser_runtime(manifest, bundle_root):
    """Validate one WebBundle runtime and return its content identity."""
    runtime = manifest.get("browser_runtime")
    assert isinstance(runtime, dict), "WebBundle has no browser_runtime object"
    assert runtime.get("protocol") == RUNTIME_PROTOCOL
    assert runtime.get("protocol_version") == RUNTIME_PROTOCOL_VERSION
    artifacts = runtime.get("artifacts")
    assert isinstance(artifacts, list), "browser_runtime.artifacts must be a list"
    assert tuple(item.get("path") for item in artifacts) == RUNTIME_PATHS

    root = Path(bundle_root)
    identity = []
    for item in artifacts:
        assert set(item) == {"path", "bytes", "sha256"}, (
            f"unexpected runtime metadata fields for {item.get('path')}"
        )
        path = root / item["path"]
        assert path.is_file(), f"browser runtime is missing {item['path']}"
        payload = path.read_bytes()
        assert len(payload) == item["bytes"], f"runtime byte count differs for {item['path']}"
        digest = hashlib.sha256(payload).hexdigest()
        assert digest == item["sha256"], f"runtime digest differs for {item['path']}"
        identity.append((item["path"], item["bytes"], digest))
    return (RUNTIME_PROTOCOL, RUNTIME_PROTOCOL_VERSION, tuple(identity))


def validate_shared_browser_runtime(bundles):
    """Prove named applications package one byte-identical generic runtime."""
    identities = {
        name: validate_browser_runtime(manifest, root)
        for name, (manifest, root) in bundles.items()
    }
    assert len(identities) >= 2, "shared-runtime proof requires at least two applications"
    baseline_name, baseline = next(iter(identities.items()))
    for name, identity in identities.items():
        assert identity == baseline, (
            f"{name} browser runtime differs from {baseline_name}; "
            "applications are not exercising one packaged runtime surface"
        )
    return baseline
