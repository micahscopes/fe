import {
  compileActorAdapter,
  createActorAdapter,
} from "../interface.js";
import { installCanonicalWorkerHost } from "./worker-host-core.js";

installCanonicalWorkerHost({ compileActorAdapter, createActorAdapter });
