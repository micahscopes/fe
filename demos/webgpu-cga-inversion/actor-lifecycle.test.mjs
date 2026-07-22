import assert from "node:assert/strict";
import { createCgaActorLifecycle } from "./actor-lifecycle.js";

const deferred = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

const verificationJobs = [];
const published = [];
const settled = [];
const lifecycle = createCgaActorLifecycle({
  mode: "continuous",
  render: ({ payload }) => payload.frame,
  verify(request) {
    const job = deferred();
    verificationJobs.push({ request, job });
    return job.promise;
  },
  onVerificationResult: (result) => published.push(result),
  onVerificationSettled: (result, status) => settled.push({ result, status }),
});

const initial = lifecycle.begin({ frame: "initial" }, { view: "initial", initial: true });
await tick();
assert.equal(verificationJobs.length, 1);
const interaction = lifecycle.interact({ frame: "interaction" });
lifecycle.enqueueVerification({ view: "interaction" });
verificationJobs[0].job.resolve("initial accepted");
await tick();
assert.equal(settled[0].result.requestId, initial.verification.requestId);
assert.equal(settled[0].status.fresh, false);
assert.equal(published.length, 0);
assert.equal(verificationJobs.length, 2);
verificationJobs[1].job.resolve("interaction accepted");
await tick();
assert.equal(published.length, 1);
assert.equal(published[0].generation, interaction.generation);

// Repeated current-generation checks retain one active plus only the latest pending.
lifecycle.enqueueVerification({ view: 1 });
await tick();
lifecycle.enqueueVerification({ view: 2 });
const newest = lifecycle.enqueueVerification({ view: 3 });
assert.equal(lifecycle.state().verify.pending, newest.requestId);
verificationJobs[2].job.resolve("superseded");
await tick();
assert.equal(verificationJobs.at(-1).request.requestId, newest.requestId);
verificationJobs.at(-1).job.resolve("newest");
await tick();
assert.equal(published.at(-1).payload.value, "newest");

let offVerificationRuns = 0;
const off = createCgaActorLifecycle({
  mode: "off",
  render: () => null,
  verify: () => { offVerificationRuns += 1; },
});
const offInitial = off.begin({ frame: 0 }, { view: 0 });
off.interact({ frame: 1 });
assert.equal(offInitial.verification, null);
assert.equal(off.enqueueVerification({ view: 1 }), null);
await tick();
assert.equal(offVerificationRuns, 0);

const rejectedSettlements = [];
const rejected = createCgaActorLifecycle({
  mode: "manual",
  render: () => null,
  verify: () => Promise.reject(new Error("initial device failure")),
  onVerificationSettled: (result, status) => rejectedSettlements.push({ result, status }),
});
const rejectedInitial = rejected.begin({ frame: 0 }, { view: 0, initial: true });
await tick();
assert.equal(rejectedSettlements.length, 1);
assert.equal(rejectedSettlements[0].result.payload.ok, false);
assert.match(rejectedSettlements[0].result.payload.error, /initial device failure/);
assert.equal(rejectedSettlements[0].status.request.requestId, rejectedInitial.verification.requestId);
assert.equal(rejectedSettlements[0].status.request.payload.initial, true);

console.log("CGA actor lifecycle initial/interaction/latest-wins/off/rejection: ok");
