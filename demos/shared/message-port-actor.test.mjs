import assert from "node:assert/strict";
import { actorEnvelope } from "./actor-coordinator.js";
import { createActorEndpoint, actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";
import { attachMessagePortActorHost, createMessagePortActorTransport } from "./message-port-actor.js";

const channel = new MessageChannel();
const detach = attachMessagePortActorHost(channel.port2, ({ payload }) => payload.value * 2);
const transport = createMessagePortActorTransport(channel.port1);
const schema = {
  render: (payload) => exactObject(payload, { value: actorField.finiteNumber }),
  verify: (payload) => exactObject(payload, { value: actorField.finiteNumber }),
};
const results = { render: actorResultSchema(actorField.finiteNumber),
  verify: actorResultSchema(actorField.finiteNumber) };
const endpoint = createActorEndpoint({ transport, requestSchema: schema, resultSchema: results });
const request = actorEnvelope({ type: "request", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 1, payload: { value: 21 } });
assert.equal((await endpoint.request(request)).payload.value, 42);

const pending = endpoint.request(actorEnvelope({ type: "request", lane: "verify", actorEpoch: 0,
  generation: 2, requestId: 2, payload: { value: 7 } }));
transport.fail("simulated worker crash");
const failed = await pending;
assert.equal(failed.payload.ok, false);
assert.match(failed.payload.error, /simulated worker crash/);
endpoint.close();
detach();

const malformedChannel = new MessageChannel();
const malformedEndpoint = createActorEndpoint({
  transport: createMessagePortActorTransport(malformedChannel.port1),
  requestSchema: schema,
  resultSchema: results,
});
const malformedPending = malformedEndpoint.request(actorEnvelope({ type: "request", lane: "verify",
  actorEpoch: 0, generation: 1, requestId: 3, payload: { value: 1 } }));
malformedChannel.port2.postMessage({
  ...actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
    generation: 1, requestId: 3, payload: { ok: true, value: 2 } }),
  surplus: true,
});
await assert.rejects(malformedPending, /unexpected or missing fields/);
malformedEndpoint.close();
malformedChannel.port2.close();

console.log("protocol-v2 MessagePort actor transport/host: ok");
