import { ACTOR_PROTOCOL_VERSION } from "./actor-coordinator.js";
import { actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";

function exactKeys(value, expected, name) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object`);
  }
  const actual = Object.keys(value).sort();
  if (actual.join("\0") !== [...expected].sort().join("\0")) {
    throw new TypeError(`${name} has unexpected or missing fields`);
  }
}

function fieldValidator(spec, name) {
  exactKeys(spec, ["kind", "length"], name);
  if (!Number.isSafeInteger(spec.length) || spec.length < 0) {
    throw new TypeError(`${name}.length must be a non-negative safe integer`);
  }
  if (spec.kind === "i32-array") return actorField.int32Array(spec.length);
  if (spec.kind === "f32-array") return actorField.float32Array(spec.length);
  if (spec.kind === "u8-array") return actorField.uint8Array(spec.length);
  throw new TypeError(`${name}.kind is unsupported`);
}

function requestValidator(spec, name) {
  exactKeys(spec, ["fields", "kind"], name);
  if (spec.kind !== "record") throw new TypeError(`${name}.kind must be record`);
  if (!spec.fields || typeof spec.fields !== "object" || Array.isArray(spec.fields)) {
    throw new TypeError(`${name}.fields must be an object`);
  }
  const fields = Object.fromEntries(Object.entries(spec.fields).map(([field, value]) => [
    field, fieldValidator(value, `${name}.fields.${field}`),
  ]));
  return (payload) => exactObject(payload, fields);
}

export function compileActorManifest(manifest) {
  exactKeys(manifest, ["lanes", "protocol", "version"], "actor manifest");
  if (manifest.protocol !== "fe-demo-actor") throw new TypeError("unsupported actor protocol");
  if (manifest.version !== ACTOR_PROTOCOL_VERSION) throw new TypeError("unsupported actor version");
  if (!manifest.lanes || typeof manifest.lanes !== "object" || Array.isArray(manifest.lanes)) {
    throw new TypeError("actor manifest lanes must be an object");
  }
  const request = {};
  const result = {};
  for (const [lane, spec] of Object.entries(manifest.lanes)) {
    exactKeys(spec, ["request", "result"], `actor lane ${lane}`);
    request[lane] = requestValidator(spec.request, `actor lane ${lane}.request`);
    result[lane] = actorResultSchema(fieldValidator(spec.result, `actor lane ${lane}.result`));
  }
  return Object.freeze({ request: Object.freeze(request), result: Object.freeze(result) });
}
