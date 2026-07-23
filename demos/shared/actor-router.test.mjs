import assert from "node:assert/strict";
import {
  ActorLaneRoutingError,
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
    dispatch(request) {
      calls.push(["wasm", request]);
      return "wasm-result";
    },
  },
  webgpu: {
    lanes: ["render", "verify", "oracle"],
    dispatch(request) {
      calls.push(["webgpu", request]);
      return "host-result";
    },
  },
});
const render = Object.freeze({ lane: "render", payload: Object.freeze({ generation: 1 }) });
const pixel = Object.freeze({ lane: "oracle_pixel", payload: Object.freeze({ x: 2, y: 3 }) });
assert.equal(router.dispatch(render), "host-result");
assert.equal(router.dispatch(pixel), "wasm-result");
assert.deepEqual(calls, [["webgpu", render], ["wasm", pixel]]);
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

console.log("exact compiler-lane actor ownership router: ok");
