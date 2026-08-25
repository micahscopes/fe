import { createCanonicalIntentRouter } from "./actor-router.js";
import { createCanonicalMainThreadGpuClient } from "./gpu-actor.js";
import { attachMessagePortActorHost } from "./message-port-actor.js";

const INIT_ERROR = "FE_ACTOR_WORKER_INIT";

const instantiateCanonicalWasm = async (wasm, imports = {}) => {
  const instantiated = await WebAssembly.instantiate(wasm, imports);
  return instantiated.instance?.exports ?? instantiated.exports;
};

const mergeImports = (target, additions) => {
  if (!additions) return;
  for (const [moduleName, values] of Object.entries(additions)) {
    const module = target[moduleName] ?? (target[moduleName] = Object.create(null));
    for (const [name, value] of Object.entries(values)) {
      if (Object.hasOwn(module, name) && module[name] !== value) {
        throw new Error(`conflicting fixed Wasm import: ${moduleName}.${name}`);
      }
      module[name] = value;
    }
  }
};

const prepareCanonicalWorkerTasks = async (taskWasm, taskModule) => {
  if (!taskModule && !taskWasm) {
    return {
      start() { return () => {}; },
    };
  }
  if (!taskModule || !taskWasm) {
    throw new Error("compiler-published child task adapter and Wasm must be paired");
  }
  const wasm = await taskWasm;
  if (!(wasm instanceof WebAssembly.Module)) {
    throw new TypeError("compiler-published child task Wasm must be a compiled module");
  }
  if (typeof taskModule.createMaterializedTaskRegistry !== "function"
      || typeof taskModule.createHostCompletionBroker !== "function") {
    throw new Error("compiler-published child task package has an invalid fixed interface");
  }

  const required = WebAssembly.Module.imports(wasm);
  const needsWorkerScope = required.some(value => value.module === "fe:worker-scope");
  const needsWorkerMailbox = required.some(value => value.module === "fe:worker-mailbox");
  const brokerOptions = {};
  let structuredWorkerScopes = [];
  if (needsWorkerScope || needsWorkerMailbox) {
    if (typeof taskModule.createStructuredWorkerScopes !== "function") {
      throw new Error("nested Worker effects require compiler-derived child packages");
    }
    structuredWorkerScopes = await taskModule.createStructuredWorkerScopes();
    brokerOptions.workerScopes = structuredWorkerScopes;
  }
  const broker = taskModule.createHostCompletionBroker(brokerOptions);
  const imports = Object.create(null);
  mergeImports(imports, broker.imports);
  let mailboxBridge;
  if (needsWorkerMailbox) {
    if (typeof taskModule.createStructuredWorkerMailboxes !== "function") {
      throw new Error("nested Worker mailbox effects require compiler-derived adapters");
    }
    mailboxBridge = taskModule.createStructuredWorkerMailboxes(
      structuredWorkerScopes,
      broker.completions,
    );
    mergeImports(imports, mailboxBridge);
  }
  for (const value of required) {
    if (!Object.hasOwn(imports, value.module)
        || !Object.hasOwn(imports[value.module], value.name)) {
      throw new Error(`missing nested Worker Wasm import: ${value.module}.${value.name}`);
    }
  }
  const exports = await instantiateCanonicalWasm(wasm, imports);
  mailboxBridge?.attach(exports);

  return {
    start(fail) {
      const registry = taskModule.createMaterializedTaskRegistry(exports);
      const machines = Object.values(registry);
      if (machines.length === 0) {
        throw new Error("compiler-published child task package contains no task machines");
      }
      const lifetime = new AbortController();
      try {
        for (const machine of machines) {
          const inputWidth = machine.inputWidth ?? 0;
          if (inputWidth !== 0) {
            throw new Error(
              "structured child scoped tasks cannot receive Worker-owned actor state yet",
            );
          }
          broker.run(machine, [], { signal: lifetime.signal }).catch(error => {
            if (!lifetime.signal.aborted && error?.name !== "AbortError") fail(error);
          });
        }
      } catch (error) {
        lifetime.abort();
        broker.cancelAll();
        throw error;
      }
      return () => {
        lifetime.abort();
        broker.cancelAll();
      };
    },
  };
};

const canonicalDispatchers = (adapter, wasmActor, gpu) => {
  const owners = new Set();
  for (const intent of Object.values(adapter.intents)) {
    if (intent.execution === "wasm") owners.add("wasm");
    else if (intent.execution === "host_effect" && intent.placement === "main_thread") {
      owners.add("main_thread_host");
    } else {
      throw new TypeError("generated Worker host cannot own this canonical lane intent");
    }
  }
  const dispatchers = {};
  if (owners.has("wasm")) {
    dispatchers.wasm = (request, context) => wasmActor.dispatch(request, context);
  }
  if (owners.has("main_thread_host")) {
    if (!gpu) throw new TypeError("canonical main-thread host lanes require a GPU port");
    dispatchers.main_thread_host = (request, { signal } = {}) => gpu.request(
      request.lane,
      request.payload,
      request.generation,
      { signal },
    );
  }
  return dispatchers;
};

export async function attachCanonicalWorkerHost({
  port,
  gpuPort,
  wasm,
  actorEpoch,
}, interfaceModule, taskModule = null, taskWasm = null) {
  const { compileActorAdapter, createActorAdapter } = interfaceModule ?? {};
  if (typeof compileActorAdapter !== "function" || typeof createActorAdapter !== "function") {
    throw new TypeError("canonical Worker host requires a compiler-derived interface");
  }
  const tasks = await prepareCanonicalWorkerTasks(taskWasm, taskModule);
  const exports = await instantiateCanonicalWasm(wasm);
  const adapter = compileActorAdapter();
  const wasmActor = createActorAdapter(exports, { placement: "worker" });
  const hasMainThreadGpu = Object.values(adapter.intents).some((intent) =>
    intent.execution === "host_effect" && intent.placement === "main_thread");
  const gpu = hasMainThreadGpu
    ? createCanonicalMainThreadGpuClient(gpuPort, { adapter, initialEpoch: actorEpoch })
    : null;
  const router = createCanonicalIntentRouter(
    adapter,
    canonicalDispatchers(adapter, wasmActor, gpu),
  );
  let detachActor = () => {};
  const stopTasks = tasks.start(error => {
    detachActor();
    queueMicrotask(() => { throw error; });
  });
  detachActor = attachMessagePortActorHost(port, router.dispatch, {
    transferResult: adapter.transferResult,
  });
  port.postMessage({ type: "ready" });
  return Object.freeze({
    close() {
      detachActor();
      stopTasks();
    },
  });
}

export function installCanonicalWorkerHost(
  interfaceModule,
  scope = globalThis,
  taskModule = null,
  taskWasm = null,
) {
  scope.addEventListener("message", async ({ data }) => {
    if (data?.type !== "init") return;
    try {
      await attachCanonicalWorkerHost(data, interfaceModule, taskModule, taskWasm);
    } catch (error) {
      console.error("canonical Worker initialization failed", error);
      data?.port?.postMessage({ type: "init-error", error: INIT_ERROR });
    }
  });
}
