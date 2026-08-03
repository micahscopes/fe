import init, {
  compile_json as compileJson,
  install_panic_hook as installPanicHook,
  protocol_major as compilerProtocolMajor,
  protocol_minor as compilerProtocolMinor,
} from "./gen/compiler/fe_browser_compiler.js";
import { createCompilerWorkerRuntime } from "./compiler-worker-runtime.js";

await init();
installPanicHook();

const runtime = createCompilerWorkerRuntime({
  compileJson,
  compilerProtocolMajor,
  compilerProtocolMinor,
  postMessage(message, transfers) {
    globalThis.postMessage(message, { transfer: transfers });
  },
});

globalThis.addEventListener("message", (event) => runtime.receive(event));

