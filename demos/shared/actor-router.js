const LANE_PATTERN = /^[a-z](?:[a-z0-9]|[._-](?=[a-z0-9])){0,63}$/;

export class ActorLaneRoutingError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "ActorLaneRoutingError";
    this.code = code;
  }
}

function plainObject(value, name) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be a plain object`);
  }
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    throw new TypeError(`${name} must be a plain object`);
  }
  return value;
}

function exactKeys(value, expected, name) {
  plainObject(value, name);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.join("\0") !== wanted.join("\0")) {
    throw new TypeError(`${name} has unexpected or missing fields`);
  }
}

function canonicalLaneNames(compiledLanes) {
  plainObject(compiledLanes, "compiled canonical lanes");
  const lanes = Object.keys(compiledLanes);
  if (lanes.length === 0) throw new TypeError("compiled canonical lanes must not be empty");
  for (const lane of lanes) {
    if (!LANE_PATTERN.test(lane)) throw new TypeError(`invalid canonical lane name ${lane}`);
  }
  return lanes.sort();
}

/**
 * Build an exact router from compiler-derived canonical lanes and an explicit
 * placement map. Placement remains application policy; value schemas and the
 * set of lanes remain compiler-owned.
 *
 * Every canonical lane must be owned exactly once. There is deliberately no
 * fallback owner: drift between Fe lanes and browser placement fails during
 * initialization instead of silently routing work to the wrong actor.
 */
export function createExactLaneRouter(compiledLanes, ownership) {
  const lanes = canonicalLaneNames(compiledLanes);
  plainObject(ownership, "actor lane ownership");
  const ownerNames = Object.keys(ownership).sort();
  if (ownerNames.length === 0) throw new TypeError("actor lane ownership must not be empty");

  const canonical = new Set(lanes);
  const routes = Object.create(null);
  const owners = Object.create(null);
  for (const owner of ownerNames) {
    if (!LANE_PATTERN.test(owner)) throw new TypeError(`invalid actor owner name ${owner}`);
    const descriptor = ownership[owner];
    exactKeys(descriptor, ["dispatch", "lanes"], `actor owner ${owner}`);
    if (typeof descriptor.dispatch !== "function") {
      throw new TypeError(`actor owner ${owner}.dispatch must be a function`);
    }
    if (!Array.isArray(descriptor.lanes) || descriptor.lanes.length === 0) {
      throw new TypeError(`actor owner ${owner}.lanes must be a non-empty array`);
    }
    for (const lane of descriptor.lanes) {
      if (typeof lane !== "string" || !canonical.has(lane)) {
        throw new ActorLaneRoutingError(
          "FE_ACTOR_UNKNOWN_OWNERSHIP_LANE",
          `actor owner ${owner} names unknown canonical lane ${String(lane)}`,
        );
      }
      if (Object.hasOwn(routes, lane)) {
        throw new ActorLaneRoutingError(
          "FE_ACTOR_DUPLICATE_LANE_OWNER",
          `canonical lane ${lane} has multiple actor owners`,
        );
      }
      routes[lane] = descriptor.dispatch;
      owners[lane] = owner;
    }
  }
  const unowned = lanes.filter((lane) => !Object.hasOwn(routes, lane));
  if (unowned.length !== 0) {
    throw new ActorLaneRoutingError(
      "FE_ACTOR_UNOWNED_LANE",
      `canonical actor lanes have no owner: ${unowned.join(", ")}`,
    );
  }

  return Object.freeze({
    lanes: Object.freeze(lanes),
    ownerOf(lane) {
      if (!Object.hasOwn(owners, lane)) {
        throw new ActorLaneRoutingError(
          "FE_ACTOR_UNKNOWN_LANE",
          `unknown canonical actor lane ${String(lane)}`,
        );
      }
      return owners[lane];
    },
    dispatch(request) {
      const lane = request?.lane;
      if (typeof lane !== "string" || !Object.hasOwn(routes, lane)) {
        throw new ActorLaneRoutingError(
          "FE_ACTOR_UNKNOWN_LANE",
          `unknown canonical actor lane ${String(lane)}`,
        );
      }
      return routes[lane](request);
    },
  });
}
