import assert from "node:assert/strict";
import test from "node:test";

import {
  FeCompilerPool,
  canonicalCompileKey,
} from "./compiler-pool.js";
import { WorkerCrashError } from "./compiler-adapter.example.js";

const digest = async text => `digest:${text}`;

function request(source = "one", overrides = {}) {
  return {
    source,
    sourceUrl: "fe-memory:///app.fe",
    attributes: { "data-fe-entry": "main", ignored: "host-only" },
    ...overrides,
  };
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function until(predicate) {
  for (let attempt = 0; attempt < 20 && !predicate(); attempt += 1) {
    await new Promise(resolve => setTimeout(resolve, 0));
  }
  assert(predicate(), "condition did not become true");
}

test("canonical keys cover source digests, entries, targets, and options only", async () => {
  const first = await canonicalCompileKey(request(), digest);
  const reorderedHostAttributes = await canonicalCompileKey(
    request("one", {
      attributes: { ignored: "different", "data-fe-entry": "main" },
    }),
    digest,
  );
  assert.equal(first, reorderedHostAttributes);
  assert.equal(
    await canonicalCompileKey(request("one", {
      options: { debug_info: false, optimization: "none" },
    }), digest),
    first,
  );
  assert.notEqual(first, await canonicalCompileKey(request("two"), digest));
  assert.notEqual(
    first,
    await canonicalCompileKey(request("one", {
      options: { optimization: "size", debug_info: false },
    }), digest),
  );
});

test("identical in-flight requests coalesce onto one worker compilation", async () => {
  const work = deferred();
  let calls = 0;
  const pool = new FeCompilerPool({
    size: 2,
    digest,
    createCompiler: () => ({
      compile() {
        calls += 1;
        return work.promise;
      },
    }),
  });
  const first = pool.compile(request());
  const second = pool.compile(request());
  await until(() => calls === 1);
  assert.equal(calls, 1);
  work.resolve({ value: 42 });
  assert.strictEqual(await first, await second);
});

test("subscriber cancellation does not abort shared compilation", async () => {
  const work = deferred();
  let receivedSignal;
  let started = false;
  const pool = new FeCompilerPool({
    digest,
    createCompiler: () => ({
      compile(value) {
        started = true;
        receivedSignal = value.signal;
        return work.promise;
      },
    }),
  });
  const controller = new AbortController();
  const cancelled = pool.compile(request("shared", { signal: controller.signal }));
  const retained = pool.compile(request("shared"));
  await until(() => started);
  controller.abort(new Error("subscriber left"));
  await assert.rejects(cancelled, /subscriber left/);
  assert.equal(receivedSignal, undefined, "subscriber signals never own shared work");
  work.resolve({ value: 7 });
  assert.equal((await retained).value, 7);
});

test("bounded cache uses deterministic least-recently-used eviction", async () => {
  const calls = [];
  const pool = new FeCompilerPool({
    capacity: 2,
    digest,
    createCompiler: () => ({
      async compile(value) {
        calls.push(value.source);
        return { value: value.source };
      },
    }),
  });
  await pool.compile(request("a"));
  await pool.compile(request("b"));
  await pool.compile(request("a")); // promote a; b is now least recent
  await pool.compile(request("c")); // evict b
  await pool.compile(request("b"));
  assert.deepEqual(calls, ["a", "b", "c", "b"]);
});

test("worker crashes reject current subscribers and replace the worker", async () => {
  let workers = 0;
  const pool = new FeCompilerPool({
    digest,
    createCompiler() {
      workers += 1;
      const id = workers;
      return {
        async compile() {
          if (id === 1) throw new WorkerCrashError("worker one crashed");
          return { worker: id };
        },
      };
    },
  });
  await assert.rejects(pool.compile(request("crash")), /worker one crashed/);
  assert.deepEqual(await pool.compile(request("retry")), { worker: 2 });
  assert.equal(workers, 2);
});

test("pool capacity bounds concurrent worker execution", async () => {
  const pending = [];
  let active = 0;
  let maximum = 0;
  const pool = new FeCompilerPool({
    size: 2,
    digest,
    createCompiler: () => ({
      compile() {
        active += 1;
        maximum = Math.max(maximum, active);
        const work = deferred();
        pending.push(() => {
          active -= 1;
          work.resolve({});
        });
        return work.promise;
      },
    }),
  });
  const requests = ["a", "b", "c"].map(value => pool.compile(request(value)));
  await until(() => maximum === 2);
  assert.equal(maximum, 2);
  pending.shift()();
  await until(() => pending.length === 2);
  assert.equal(maximum, 2);
  while (pending.length) pending.shift()();
  await Promise.all(requests);
});
