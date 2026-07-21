// live-pump.js - the DEMO-BLIND interactive pump (spec section 5.2).
//
// It knows nothing about mandelbrots, pan, zoom, or views. It reads the compiler-
// emitted `ctl.json` (the update_view arg order + the event map) and does exactly
// four things per input event:
//   1. normalize the DOM event to a flat field bag (movementX/Y, offsetX/Y, deltaY);
//   2. marshal those fields + the STORED view triple into update_view's arg vector,
//      strictly following ctl.json's event_map (view args from the stored triple,
//      the rest from the event) - NO arithmetic, not even delta accumulation;
//   3. call update_view ONCE and store the returned triple (the Fe control fn owns
//      the entire view state);
//   4. mark dirty; a rAF calls the supplied renderFn(view) at most once per frame.
//
// JS view math done here: NONE. `Math.sign(deltaY)` is event normalization (a wheel
// notch's direction), listed in the spec as allowed; every real view computation
// (pan follow, zoom step, cursor anchor, clamps) lives in the Fe update_view.

// Read one update_view arg from its event_map spec + the current fields/view/kind.
function readArg(spec, fields, view, kind) {
  if (!spec) return 0;
  if (spec.source === "view") return view[spec.index] | 0;
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
// canvas events to the Fe control fn + the supplied renderFn. `updateView` is the
// raw wasm export (i32 x8) -> [i32,i32,i32]. `ctlMeta` is the parsed ctl.json.
// Returns { getView, setView, applyFields, destroy } (applyFields drives the pump
// from a synthetic field bag, for scripted/preset gestures).
export function createLivePump({ canvas, updateView, ctlMeta, renderFn, onView }) {
  const args = ctlMeta.args;
  const eventMap = ctlMeta.event_map;
  let view = (ctlMeta.view_init || [0, 0, 384]).slice(0, 3).map((n) => n | 0);
  let dirty = true;
  let rafPending = false;

  // The ONE call: marshal (fields + stored view) -> update_view -> new view.
  function applyFields(fields, kind) {
    const argv = args.map((name) => readArg(eventMap[name], fields, view, kind));
    const reply = updateView(...argv);
    if (!Array.isArray(reply) || reply.length !== 3) {
      throw new Error(
        `update_view must return a 3-value multi-value reply (native wasm); got ${JSON.stringify(reply)}`
      );
    }
    view = [reply[0] | 0, reply[1] | 0, reply[2] | 0];
    if (typeof onView === "function") onView(view);
    scheduleDraw();
    return view;
  }

  function scheduleDraw() {
    dirty = true;
    if (rafPending) return;
    rafPending = true;
    requestAnimationFrame(() => {
      rafPending = false;
      if (!dirty) return;
      dirty = false;
      renderFn(view);
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

  // First paint at the initial view.
  scheduleDraw();

  return {
    getView: () => view.slice(),
    setView: (triple) => {
      view = triple.slice(0, 3).map((n) => n | 0);
      if (typeof onView === "function") onView(view);
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
