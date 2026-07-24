import assert from "node:assert/strict";
import {
  ActorLaneRoutingError,
  createCanonicalIntentRouter,
  createExactLaneRouter,
} from "./actor-router.js";

const compiledLanes = Object.freeze({
  oracle: Object.freeze({}),
  oracle_pixel: Object.freeze({}),
  render: Object.freeze({}),
  verify: Object.freeze({}),
});
const calls = [];
const router = createExactLaneRouter(compiledLanes, {
  wasm: {
    lanes: ["oracle_pixel"],
    dispatch(request, context) {
      calls.push(["wasm", request, context]);
      return "wasm-result";
    },
  },
  webgpu: {
    lanes: ["render", "verify", "oracle"],
    dispatch(request, context) {
      calls.push(["webgpu", request, context]);
      return "host-result";
    },
  },
});
const render = Object.freeze({ lane: "render", payload: Object.freeze({ generation: 1 }) });
const pixel = Object.freeze({ lane: "oracle_pixel", payload: Object.freeze({ x: 2, y: 3 }) });
const renderContext = Object.freeze({ signal: new AbortController().signal });
const pixelContext = Object.freeze({ signal: new AbortController().signal });
assert.equal(router.dispatch(render, renderContext), "host-result");
assert.equal(router.dispatch(pixel, pixelContext), "wasm-result");
assert.deepEqual(calls, [
  ["webgpu", render, renderContext],
  ["wasm", pixel, pixelContext],
]);
assert.deepEqual(router.lanes, ["oracle", "oracle_pixel", "render", "verify"]);
assert.equal(router.ownerOf("verify"), "webgpu");
assert.equal(router.ownerOf("oracle_pixel"), "wasm");

const routingError = (callback, code) => assert.throws(callback, (error) => {
  assert(error instanceof ActorLaneRoutingError);
  assert.equal(error.code, code);
  return true;
});
routingError(
  () => router.dispatch({ lane: "new_fe_lane", payload: null }),
  "FE_ACTOR_UNKNOWN_LANE",
);
routingError(() => router.dispatch(null), "FE_ACTOR_UNKNOWN_LANE");
routingError(() => router.ownerOf("__proto__"), "FE_ACTOR_UNKNOWN_LANE");

routingError(
  () => createExactLaneRouter(compiledLanes, {
    host: { lanes: ["render", "verify", "oracle"], dispatch() {} },
  }),
  "FE_ACTOR_UNOWNED_LANE",
);
routingError(
  () => createExactLaneRouter(compiledLanes, {
    host: { lanes: ["render", "verify", "oracle"], dispatch() {} },
    wasm: { lanes: ["oracle", "oracle_pixel"], dispatch() {} },
  }),
  "FE_ACTOR_DUPLICATE_LANE_OWNER",
);
routingError(
  () => createExactLaneRouter(compiledLanes, {
    host: { lanes: ["render", "verify", "oracle"], dispatch() {} },
    wasm: { lanes: ["missing"], dispatch() {} },
  }),
  "FE_ACTOR_UNKNOWN_OWNERSHIP_LANE",
);
assert.throws(
  () => createExactLaneRouter(compiledLanes, {
    host: { lanes: ["render"], dispatch() {}, fallback() {} },
  }),
  /unexpected or missing fields/,
);
assert.throws(
  () => createExactLaneRouter(Object.freeze({ "Bad Lane": {} }), {
    host: { lanes: ["Bad Lane"], dispatch() {} },
  }),
  /invalid canonical lane name/,
);

const intent = (execution, placement, capabilities = []) =>
  Object.freeze({ execution, placement, capabilities: Object.freeze(capabilities) });
const canonicalAdapter = Object.freeze({
  intents: Object.freeze({
    render: intent("host_effect", "main_thread", [
      Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
    ]),
    verify: intent("host_effect", "main_thread", [
      Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
    ]),
    oracle: intent("host_effect", "worker"),
    oracle_pixel: intent("wasm", "any"),
  }),
});
const intentCalls = [];
const intentRouter = createCanonicalIntentRouter(canonicalAdapter, {
  main_thread_host(request, context) {
    intentCalls.push(["main", request.lane, context]);
  },
  worker_host(request, context) {
    intentCalls.push(["worker", request.lane, context]);
  },
  wasm(request, context) {
    intentCalls.push(["wasm", request.lane, context]);
  },
});
const intentContext = Object.freeze({ signal: new AbortController().signal });
intentRouter.dispatch({ lane: "render" }, intentContext);
intentRouter.dispatch({ lane: "oracle" }, intentContext);
intentRouter.dispatch({ lane: "oracle_pixel" }, intentContext);
assert.deepEqual(intentCalls, [
  ["main", "render", intentContext],
  ["worker", "oracle", intentContext],
  ["wasm", "oracle_pixel", intentContext],
]);
assert.equal(intentRouter.ownerOf("verify"), "main_thread_host");
assert.throws(
  () => createCanonicalIntentRouter(canonicalAdapter, {
    main_thread_host() {},
    wasm() {},
  }),
  /unexpected or missing fields/,
);
assert.throws(
  () => createCanonicalIntentRouter(canonicalAdapter, {
    fallback() {},
    main_thread_host() {},
    worker_host() {},
    wasm() {},
  }),
  /unexpected or missing fields/,
);
routingError(
  () => createCanonicalIntentRouter({
    intents: { misplaced: intent("host_effect", "any") },
  }, { worker_host() {} }),
  "FE_ACTOR_INVALID_LANE_INTENT",
);
routingError(
  () => createCanonicalIntentRouter({
    intents: {
      confused: intent("wasm", "any", [
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
      ]),
    },
  }, { wasm() {} }),
  "FE_ACTOR_INVALID_LANE_INTENT",
);
routingError(
  () => createCanonicalIntentRouter({
    intents: {
      invented: intent("host_effect", "main_thread", [
        Object.freeze({ capability: "ambient_authority", mutable: false }),
      ]),
    },
  }, { main_thread_host() {} }),
  "FE_ACTOR_INVALID_LANE_INTENT",
);
routingError(
  () => createCanonicalIntentRouter({
    intents: {
      repeated: intent("host_effect", "main_thread", [
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
      ]),
    },
  }, { main_thread_host() {} }),
  "FE_ACTOR_INVALID_LANE_INTENT",
);
routingError(
  () => createCanonicalIntentRouter({
    intents: {
      main_gpu: intent("host_effect", "main_thread", [
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
      ]),
      worker_gpu: intent("host_effect", "worker", [
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
      ]),
    },
  }, { main_thread_host() {}, worker_host() {} }),
  "FE_ACTOR_CONFLICTING_CAPABILITY_OWNER",
);
routingError(
  () => createCanonicalIntentRouter({
    intents: {
      render: intent("host_effect", "main_thread", [
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
      ]),
      inspect: intent("host_effect", "main_thread", [
        Object.freeze({ capability: "webgpu_dispatch", mutable: false }),
      ]),
    },
  }, { main_thread_host() {} }),
  "FE_ACTOR_CONFLICTING_CAPABILITY_CLAIM",
);
routingError(
  () => createCanonicalIntentRouter({
    intents: { render: intent("host_effect", "main_thread") },
    requestSchema: { render() {} },
  }, { main_thread_host() {} }),
  "FE_ACTOR_CONFLICTING_LANE_DESCRIPTORS",
);
assert.throws(
  () => createCanonicalIntentRouter({
    intents: { render: intent("host_effect", "main_thread") },
    requestSchema: { render() {}, invented() {} },
    resultSchema: { render() {} },
  }, { main_thread_host() {} }),
  /unexpected or missing fields/,
);
assert.throws(
  () => createCanonicalIntentRouter({
    intents: {
      malformed: intent("host_effect", "main_thread", [
        Object.freeze({ capability: "webgpu_dispatch", mutable: "yes" }),
      ]),
    },
  }, { main_thread_host() {} }),
  /mutable must be a boolean/,
);
assert.throws(
  () => createCanonicalIntentRouter({
    intents: {
      embellished: intent("host_effect", "main_thread", [
        Object.freeze({
          capability: "webgpu_dispatch",
          mutable: true,
          fallback: "ambient",
        }),
      ]),
    },
  }, { main_thread_host() {} }),
  /unexpected or missing fields/,
);

console.log("exact and intent-derived compiler-lane actor routing: ok");
