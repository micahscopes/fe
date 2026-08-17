import assert from "node:assert/strict";
import test from "node:test";

import {
  HOST_RUNTIME_PROTOCOL,
  HostRuntimeError,
  createFeHostRuntime,
  createAsyncIteratorFutureBridge,
  reportHostProtocolError,
} from "./host-runtime.js";

test("async iterator bridge is sequential, cancellable, and suppresses late settlement", async () => {
  const runtime = createFeHostRuntime();
  const terminal = [];
  const bridge = createAsyncIteratorFutureBridge(runtime.resources, {
    resolve(token, value) { terminal.push(["resolve", token, value]); },
    reject(token, error) { terminal.push(["reject", token, error]); },
    cancel(token, reason) { terminal.push(["cancel", token, reason]); },
  });
  const deferred = [];
  let returned = 0;
  const source = {
    [Symbol.asyncIterator]() {
      return {
        next() {
          let resolve;
          const promise = new Promise((done) => { resolve = done; });
          deferred.push(resolve);
          return promise;
        },
        return() {
          returned += 1;
          return Promise.resolve({ done: true });
        },
      };
    },
  };
  const handle = bridge.create(source);
  bridge.next(handle, 1);
  assert.throws(
    () => bridge.next(handle, 2),
    (error) => error.code === "async_iterator_backpressure",
  );
  await Promise.resolve();
  deferred.shift()({ done: false, value: "first" });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(terminal, [["resolve", 1, "first"]]);

  bridge.next(handle, 2);
  await Promise.resolve();
  assert.equal(bridge.cancel(handle, 2, "stop"), true);
  assert.deepEqual(terminal.at(-1), ["cancel", 2, "stop"]);
  deferred.shift()({ done: false, value: "late" });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(terminal.length, 2, "late resolution must be suppressed");
  bridge.drop(handle);
  assert.equal(returned, 1);
  assert.throws(() => bridge.next(handle, 3), /stale resource handle/);
});

test("resource handles are opaque, consuming, generation-safe, and inventoried", () => {
  const runtime = createFeHostRuntime();
  assert.equal(runtime.protocol, HOST_RUNTIME_PROTOCOL);
  let dropped = 0;
  const first = runtime.resources.insert({ name: "first" }, () => dropped++);
  assert.equal(runtime.resources.borrow(first).name, "first");
  assert.throws(
    () => runtime.resources.borrow(Object.freeze({})),
    (error) => error instanceof HostRuntimeError && error.code === "invalid_handle",
  );
  runtime.resources.drop(first);
  assert.equal(dropped, 1);
  assert.throws(
    () => runtime.resources.drop(first),
    (error) => error.code === "stale_handle",
  );

  const secondValue = { name: "second" };
  const second = runtime.resources.insert(secondValue);
  assert.notEqual(first, second);
  assert.equal(runtime.resources.take(second), secondValue);
  assert.throws(() => runtime.resources.borrow(second), /stale resource handle/);
  assert.deepEqual(runtime.inventory(), { resources: 0, callbacks: 0, futures: 0 });
});

test("resource conversion capture commits ownership or rolls it back exactly once", () => {
  const runtime = createFeHostRuntime();
  let dropped = 0;
  const committed = runtime.resources.capture(() =>
    runtime.resources.insert({ name: "committed" }, () => dropped++));
  assert.equal(runtime.inventory().resources, 1);
  committed.commit();
  assert.equal(runtime.resources.borrow(committed.value).name, "committed");
  assert.throws(() => committed.rollback(), /already finalized/);
  runtime.resources.drop(committed.value);
  assert.equal(dropped, 1);

  const rolledBack = runtime.resources.capture(() =>
    runtime.resources.insert({ name: "rolled-back" }, () => dropped++));
  assert.equal(runtime.inventory().resources, 1);
  rolledBack.rollback();
  assert.equal(runtime.inventory().resources, 0);
  assert.equal(dropped, 2);
  assert.throws(() => runtime.resources.borrow(rolledBack.value), /stale resource handle/);

  assert.throws(
    () => runtime.resources.capture(() => {
      runtime.resources.insert({ name: "partial" }, () => dropped++);
      throw new Error("conversion failed");
    }),
    /conversion failed/,
  );
  assert.equal(runtime.inventory().resources, 0);
  assert.equal(dropped, 3);
});

test("callback release during reentrant invocation is deferred safely", () => {
  const runtime = createFeHostRuntime();
  let handle;
  const calls = [];
  handle = runtime.callbacks.register("example/callback", (value) => {
    calls.push(value);
    if (value === 1) {
      assert.equal(runtime.callbacks.invoke(handle, "example/callback", [2]), 20);
      runtime.callbacks.release(handle);
      assert.equal(runtime.callbacks.liveCount, 1);
    }
    return value * 10;
  });
  assert.equal(runtime.callbacks.invoke(handle, "example/callback", [1]), 10);
  assert.deepEqual(calls, [1, 2]);
  assert.equal(runtime.callbacks.liveCount, 0);
  assert.throws(
    () => runtime.callbacks.invoke(handle, "example/callback", [3]),
    /stale callback handle/,
  );
});

test("borrow scopes reject consumption and invalidate escaped handles", async () => {
  const runtime = createFeHostRuntime();
  let escaped;
  const value = { type: "click" };
  const result = runtime.resources.withBorrowed(value, (handle) => {
    escaped = handle;
    assert.equal(runtime.resources.borrow(handle), value);
    assert.throws(
      () => runtime.resources.take(handle),
      (error) => error.code === "borrowed_handle_consumed",
    );
    return 42;
  });
  assert.equal(result, 42);
  assert.throws(() => runtime.resources.borrow(escaped), /stale resource handle/);

  let asyncHandle;
  const pending = runtime.resources.withBorrowed(value, async (handle) => {
    asyncHandle = handle;
    await Promise.resolve();
    return runtime.resources.borrow(handle).type;
  });
  assert.equal(runtime.resources.borrow(asyncHandle), value);
  assert.equal(await pending, "click");
  assert.throws(() => runtime.resources.borrow(asyncHandle), /stale resource handle/);
  assert.equal(runtime.inventory().resources, 0);
});

test("async callback remains rooted until its in-flight invocation settles", async () => {
  const runtime = createFeHostRuntime();
  let finish;
  const handle = runtime.callbacks.register(
    "example/async",
    () => new Promise((resolve) => { finish = resolve; }),
  );
  const pending = runtime.callbacks.invoke(handle, "example/async");
  runtime.callbacks.release(handle);
  assert.equal(runtime.callbacks.liveCount, 1);
  finish(42);
  assert.equal(await pending, 42);
  assert.equal(runtime.callbacks.liveCount, 0);
});

test("future resolve, reject, and cancel are exactly once", async () => {
  const runtime = createFeHostRuntime();
  const resolved = runtime.futures.create();
  runtime.futures.settle(resolved.token, { ok: 42 });
  assert.equal(await resolved.promise, 42);
  assert.equal(runtime.futures.inspect(resolved.token).state, "resolved");
  assert.throws(
    () => runtime.futures.settle(resolved.token, { error: new Error("late") }),
    (error) => error.code === "future_already_completed",
  );

  const rejected = runtime.futures.create();
  const failure = new Error("host failure");
  runtime.futures.settle(rejected.token, { error: failure });
  await assert.rejects(rejected.promise, /host failure/);

  const cancelled = runtime.futures.create();
  const reason = new DOMException("stop", "AbortError");
  runtime.futures.cancel(cancelled.token, reason);
  await assert.rejects(cancelled.promise, /stop/);
  assert.equal(runtime.futures.inspect(cancelled.token).state, "cancelled");
  assert.throws(
    () => runtime.futures.settle(cancelled.token, { ok: "late" }),
    (error) => error.code === "future_already_completed",
  );

  for (const item of [resolved, rejected, cancelled]) {
    runtime.futures.release(item.token);
  }
  assert.deepEqual(runtime.inventory(), { resources: 0, callbacks: 0, futures: 0 });
});

const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
};

test("Promise bridge forwards one terminal result and retires caller tokens", async () => {
  const runtime = createFeHostRuntime();
  const calls = [];
  const bridge = runtime.createFutureBridge({
    resolve: (token, value) => calls.push(["resolve", token, value]),
    reject: (token, error) => calls.push(["reject", token, error.message]),
    cancel: (token) => calls.push(["cancel", token]),
  });

  bridge.subscribe(11, Promise.resolve(42));
  bridge.subscribe(12, Promise.reject(new Error("nope")));
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, [
    ["resolve", 11, 42],
    ["reject", 12, "nope"],
  ]);
  assert.deepEqual(bridge.inventory(), {
    active: 0,
    resolved: 1,
    rejected: 1,
    cancelled: 0,
    unsubscribed: 0,
    suppressedLate: 0,
    protocolErrors: 0,
  });
});

test("AbortSignal cancels subscription but only owned hooks abort producers", async () => {
  const runtime = createFeHostRuntime();
  const first = deferred();
  const second = deferred();
  const calls = [];
  let ownedAborts = 0;
  const bridge = runtime.createFutureBridge({
    resolve: (token, value) => calls.push(["resolve", token, value]),
    reject: (token) => calls.push(["reject", token]),
    cancel: (token, reason) => calls.push(["cancel", token, reason]),
  });

  const subscriptionOnly = new AbortController();
  bridge.subscribe(21, first.promise, { signal: subscriptionOnly.signal });
  subscriptionOnly.abort("stop-listening");
  first.resolve("late");

  const owned = new AbortController();
  bridge.subscribe(22, second.promise, {
    signal: owned.signal,
    ownedCancellation: (reason) => {
      ownedAborts += 1;
      assert.equal(reason, "stop-work");
    },
  });
  owned.abort("stop-work");
  second.reject(new Error("late failure"));
  await Promise.resolve();
  await Promise.resolve();

  assert.equal(ownedAborts, 1);
  assert.deepEqual(calls, [
    ["cancel", 21, "stop-listening"],
    ["cancel", 22, "stop-work"],
  ]);
  assert.equal(bridge.inventory().active, 0);
  assert.equal(bridge.inventory().cancelled, 2);
  assert.equal(bridge.inventory().suppressedLate, 2);
});

test("unsubscribe is non-cancelling and races fail closed without leaks", async () => {
  const runtime = createFeHostRuntime();
  const pending = deferred();
  const calls = [];
  const protocolErrors = [];
  const bridge = runtime.createFutureBridge({
    resolve: () => {
      throw new Error("guest resolve export failed");
    },
    reject: (token) => calls.push(["reject", token]),
    cancel: (token) => calls.push(["cancel", token]),
  }, {
    onProtocolError: (error, context) => {
      protocolErrors.push([error.message, context.phase, context.token]);
    },
  });

  bridge.subscribe(31, pending.promise);
  assert.throws(
    () => bridge.subscribe(31, Promise.resolve()),
    (error) => error.code === "future_token_in_use",
  );
  assert.equal(bridge.unsubscribe(31), true);
  assert.equal(bridge.unsubscribe(31), false);
  pending.resolve("ignored");
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, []);

  bridge.subscribe(-1, Promise.resolve(9));
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(protocolErrors, [["guest resolve export failed", "resolve", -1]]);
  assert.equal(bridge.abort(-1), false, "late abort is suppressed");
  assert.throws(() => bridge.subscribe(2 ** 31, Promise.resolve()), /core-Wasm i32/);
  assert.deepEqual(bridge.inventory(), {
    active: 0,
    resolved: 1,
    rejected: 0,
    cancelled: 0,
    unsubscribed: 1,
    suppressedLate: 2,
    protocolErrors: 1,
  });
});

test("late settlement cannot retire a reused caller-owned token", async () => {
  const runtime = createFeHostRuntime();
  const oldWork = deferred();
  const newWork = deferred();
  const calls = [];
  const bridge = runtime.createFutureBridge({
    resolve: (token, value) => calls.push([token, value]),
    reject: () => assert.fail("unexpected rejection"),
    cancel: () => assert.fail("unexpected cancellation"),
  });

  bridge.subscribe(41, oldWork.promise);
  bridge.unsubscribe(41);
  bridge.subscribe(41, newWork.promise);
  oldWork.resolve("stale");
  await Promise.resolve();
  assert.equal(bridge.inventory().active, 1);
  newWork.resolve("current");
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, [[41, "current"]]);
  assert.equal(bridge.inventory().active, 0);
  assert.equal(bridge.inventory().suppressedLate, 1);
});

test("default protocol-error policy reports or schedules a visible throw", () => {
  const failure = new Error("protocol failure");
  const reported = [];
  reportHostProtocolError(failure, {
    reportError: (error) => reported.push(error),
    schedule: () => assert.fail("reportError branch must not schedule"),
  });
  assert.deepEqual(reported, [failure]);

  let scheduled;
  reportHostProtocolError(failure, {
    reportError: null,
    schedule: (callback) => { scheduled = callback; },
  });
  assert.equal(typeof scheduled, "function");
  assert.throws(scheduled, (error) => error === failure);
});
