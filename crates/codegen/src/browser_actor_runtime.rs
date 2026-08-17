//! Fixed, demo-blind browser mechanics for compiler-derived canonical actors.
//!
//! These modules are published only when a compiled Fe interface needs them.
//! Keeping the inventory here lets render bundles and resident structured
//! children consume one exact runtime instead of maintaining parallel copies.

pub const BROWSER_ACTOR_RUNTIME_PROTOCOL: &str = "fe-browser-actor-runtime";
pub const BROWSER_ACTOR_RUNTIME_VERSION: u32 = 4;

pub const BROWSER_ACTOR_RUNTIME_FILES: &[(&str, &str)] = &[
    (
        "runtime/actor-coordinator.js",
        include_str!("../assets/browser-runtime/actor-coordinator.js"),
    ),
    (
        "runtime/actor-endpoint.js",
        include_str!("../assets/browser-runtime/actor-endpoint.js"),
    ),
    (
        "runtime/actor-router.js",
        include_str!("../assets/browser-runtime/actor-router.js"),
    ),
    (
        "runtime/gpu-actor.js",
        include_str!("../assets/browser-runtime/gpu-actor.js"),
    ),
    (
        "runtime/message-port-actor.js",
        include_str!("../assets/browser-runtime/message-port-actor.js"),
    ),
    (
        "runtime/module-worker-actor.js",
        include_str!("../assets/browser-runtime/module-worker-actor.js"),
    ),
    (
        "runtime/worker-host.js",
        include_str!("../assets/browser-runtime/worker-host.js"),
    ),
    (
        "runtime/actor-client.js",
        include_str!("../assets/browser-runtime/actor-client.js"),
    ),
];

pub fn browser_actor_runtime_files() -> &'static [(&'static str, &'static str)] {
    BROWSER_ACTOR_RUNTIME_FILES
}
