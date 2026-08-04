// live-pump.js - the DEMO-BLIND interactive pump, shared by every demo whose
// control fn owns plain (float, not fixed-point) state: webgpu-cga3d-interactive,
// webgpu-qcga-interactive, webgpu-desargues-interactive.
//
// Kernel-blind about the CONTROL REPLY VALUE TYPE as well as its arity: stored
// state values are kept as JS numbers (not truncated to integers), because the
// controls this pump drives are plain f32 state (e.g. lambda/theta/zoom, the
// SAME uniforms the fragment reads). webgpu-clifford-interactive and
// webgpu-mandelbrot-interactive still keep their own local copies for now -
// their control fns own fixed-point i32 state (forced through `| 0`) and, in
// mandelbrot-interactive's case, an async/epoch-tracked update path - both
// real divergences from this pump, not yet unified. See
// DEMO_MODERNIZATION_PLAN.md.
//
// Everything else - the marshal-call-store loop, the event normalization, the
// rAF coalescing - is demo-blind. It knows nothing about pencils, quadrics,
// meet/join, pan, or zoom. It reads the compiler-emitted `ctl.json` (the
// control arg order + the event map) and does exactly four things per input
// event:
//   1. normalize the DOM event to a flat field bag (movementX/Y, deltaY, ...);
//   2. marshal those fields + the STORED state vector into the control fn's
//      arg vector, strictly following ctl.json's event_map (state args from
//      the stored vector, the rest from the event) - NO arithmetic, not even
//      accumulation;
//   3. call the control fn ONCE and store the returned vector (the Fe control
//      fn owns the entire state, floats and all);
//   4. mark dirty; a rAF calls the supplied renderFn(state) at most once per
//      frame.
//
// JS state math done here: NONE. `Math.sign(deltaY)` is event normalization (a
// wheel notch's direction); every real computation lives in Fe.

// Read one control arg from its event_map spec + the current fields/state/kind.
function readArg(spec, fields, state, kind) {
  if (!spec) return 0;
  if (spec.source === "view") return state[spec.index];
  if (spec.source === "pointer") {
    // `when: "drag"` fields (movementX/Y) are zero unless this is a drag move.
    if (spec.when === "drag" && kind !== "drag") return 0;
    const v = fields[spec.field];
    return typeof v === "number" ? v | 0 : 0;
  }
  if (spec.source === "wheel") {
    if (kind !== "wheel") return 0;
    if (spec.field === "deltaYSign") return Math.sign(fields.deltaY || 0) | 0;
    const v = fields[spec.field];
    return typeof v === "number" ? v | 0 : 0;
  }
  return 0;
}

// createLivePump({ canvas, updateView, ctlMeta, renderFn, onView }) - wire the
// canvas events to the Fe control fn + the supplied renderFn. `updateView` is
// the raw wasm export ((f32,f32,f32,i32,i32,i32) -> [f32,f32,f32]). `ctlMeta`
// is the parsed ctl.json. Returns { getView, setView, applyFields, destroy }
// (applyFields drives the pump from a synthetic field bag, for
// scripted/preset gestures).
export function createLivePump({ canvas, updateView, ctlMeta, renderFn, onView }) {
  const args = ctlMeta.args;
  const eventMap = ctlMeta.event_map;
  // The reply arity is compiler-emitted (result_order), never hardcoded.
  const arity = (ctlMeta.result_order || ctlMeta.result_types || []).length || 3;
  const initLen = arity;
  let state = (ctlMeta.view_init || new Array(initLen).fill(0)).slice(0, arity).map((n) => Number(n));
  let dirty = true;
  let rafPending = false;

  // The ONE call: marshal (fields + stored state) -> control fn -> new state.
  function applyFields(fields, kind) {
    const argv = args.map((name) => readArg(eventMap[name], fields, state, kind));
    const reply = updateView(...argv);
    if (!Array.isArray(reply) || reply.length !== arity) {
      throw new Error(
        `control fn must return a ${arity}-value multi-value reply (native wasm); got ${JSON.stringify(reply)}`
      );
    }
    state = reply.map((n) => Number(n));
    if (typeof onView === "function") onView(state);
    scheduleDraw();
    return state;
  }

  function scheduleDraw() {
    dirty = true;
    if (rafPending) return;
    rafPending = true;
    requestAnimationFrame(() => {
      rafPending = false;
      if (!dirty) return;
      dirty = false;
      renderFn(state);
    });
  }

  // --- DOM event normalization -> the field bag the marshaller reads. ------
  let dragging = false;
  const onPointerDown = (ev) => {
    dragging = true;
    canvas.setPointerCapture?.(ev.pointerId);
    canvas.style.cursor = "grabbing";
  };
  const onPointerMove = (ev) => {
    if (!dragging) return;
    applyFields(
      { movementX: ev.movementX, movementY: ev.movementY, offsetX: ev.offsetX, offsetY: ev.offsetY },
      "drag"
    );
  };
  const endDrag = (ev) => {
    dragging = false;
    canvas.releasePointerCapture?.(ev.pointerId);
    canvas.style.cursor = "grab";
  };
  const onWheel = (ev) => {
    ev.preventDefault();
    applyFields({ deltaY: ev.deltaY, offsetX: ev.offsetX, offsetY: ev.offsetY }, "wheel");
  };

  canvas.addEventListener("pointerdown", onPointerDown);
  canvas.addEventListener("pointermove", onPointerMove);
  canvas.addEventListener("pointerup", endDrag);
  canvas.addEventListener("pointercancel", endDrag);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.style.cursor = "grab";

  // First paint at the initial state.
  scheduleDraw();

  return {
    getView: () => state.slice(),
    setView: (vec) => {
      state = vec.slice(0, arity).map((n) => Number(n));
      if (typeof onView === "function") onView(state);
      scheduleDraw();
    },
    // Scripted drive (evaluate_script / preset buttons): feed a synthetic field
    // bag through the SAME marshal+call+store path the DOM events use.
    applyFields,
    destroy: () => {
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", endDrag);
      canvas.removeEventListener("pointercancel", endDrag);
      canvas.removeEventListener("wheel", onWheel);
    },
  };
}
