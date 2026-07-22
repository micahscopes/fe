export const DEFAULT_CAMERA = Object.freeze({ x: 0, y: 0, zoom: 0.0125 });

export function normalizeCamera(camera) {
  if (![camera.x, camera.y, camera.zoom].every(Number.isFinite)) {
    throw new Error("camera values must be finite");
  }
  return Object.freeze({
    x: Math.fround(camera.x),
    y: Math.fround(camera.y),
    zoom: Math.fround(Math.min(0.05, Math.max(0.0025, camera.zoom))),
  });
}

export function panCamera(camera, dx, dy) {
  return normalizeCamera({
    x: camera.x - dx * camera.zoom,
    y: camera.y - dy * camera.zoom,
    zoom: camera.zoom,
  });
}

export function zoomCamera(camera, wheelDelta, px, py, width, height) {
  if (wheelDelta === 0) return normalizeCamera(camera);
  const factor = wheelDelta < 0 ? 0.85 : 1 / 0.85;
  const zoom = Math.min(0.05, Math.max(0.0025, camera.zoom * factor));
  return normalizeCamera({
    x: camera.x + (px - width / 2) * (camera.zoom - zoom),
    y: camera.y + (py - height / 2) * (camera.zoom - zoom),
    zoom,
  });
}

export function createTrailingCoalescer(run, schedule = setTimeout, cancel = clearTimeout, delay = 120) {
  let timer = null;
  let generation = 0;
  return {
    submit(value) {
      generation += 1;
      const submitted = generation;
      if (timer !== null) cancel(timer);
      timer = schedule(() => {
        timer = null;
        run(value, submitted);
      }, delay);
      return submitted;
    },
    generation() { return generation; },
  };
}
