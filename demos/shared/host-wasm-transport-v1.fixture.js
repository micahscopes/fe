// @generated transport blueprint; layout is delegated to the codec.
export const FE_HOST_WASM_CODEC_CONTRACT = "fe:host-wasm-codec/v1";
export function createFeCoreWasmTransport(codec, semanticAdapter) {
  if (codec.protocol !== FE_HOST_WASM_CODEC_CONTRACT) throw new TypeError(`expected ${FE_HOST_WASM_CODEC_CONTRACT} codec`);
  const required = ["realloc","post_return","resource_transfer",];
  const unsupported = required.filter(feature => !codec.supports(feature));
  if (unsupported.length) throw new TypeError(`codec lacks ${unsupported.join(", ")}`);
  const mechanicsBlockers = [];
  if (mechanicsBlockers.length) throw new TypeError(`transport blueprint is not executable: ${mechanicsBlockers.join("; ")}`);
  const session = codec.createSession();
  const imports = {
    "send": (...coreArgs) => {
      const args = session.liftArguments("fixture/send", coreArgs);
      const result = semanticAdapter.imports["fe:fixture"]["send"](...args);
      return session.lowerResult("fixture/send", result);
    },
  };
  const postReturnNames = {"fixture/send": "cabi_post_fixture_send",};
  const attach = instance => {
    const exports = instance.exports;
    const memory = exports["memory"];
    const alloc = exports["cabi_alloc"];
    const realloc = exports["cabi_realloc"];
    if (!(memory instanceof WebAssembly.Memory)) throw new TypeError("missing canonical memory export");
    if (typeof alloc !== "function" || typeof realloc !== "function") throw new TypeError("missing canonical allocator exports");
    const postReturns = Object.fromEntries(Object.entries(postReturnNames).map(([identity, name]) => {
      const cleanup = exports[name];
      if (typeof cleanup !== "function") throw new TypeError(`missing post-return export ${name} for ${identity}`);
      return [identity, cleanup];
    }));
    session.attach({ instance, memory, alloc, realloc, postReturns });
    return instance;
  };
  return { imports: { "fe:fixture": imports }, attach, session };
}
