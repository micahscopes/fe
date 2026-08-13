export declare const canonicalInterfaceManifest: Readonly<object>;
export declare const compiledCanonicalInterface: Readonly<object>;

export type ProbeRequest = {
  generation: number;
};
export type ProbeResponse = {
  vertices: number;
  edges: number;
  faces: number;
  euler: number;
};

export type D0Request = {
  v0: number;
  v1: number;
  v2: number;
  v3: number;
  v4: number;
  v5: number;
  v6: number;
};
export type D0Response = {
  e0: number;
  e1: number;
  e2: number;
  e3: number;
  e4: number;
  e5: number;
  e6: number;
  e7: number;
  e8: number;
  e9: number;
  e10: number;
  e11: number;
};

export type D1Request = {
  e0: number;
  e1: number;
  e2: number;
  e3: number;
  e4: number;
  e5: number;
  e6: number;
  e7: number;
  e8: number;
  e9: number;
  e10: number;
  e11: number;
};
export type D1Response = {
  t0: number;
  t1: number;
  t2: number;
  t3: number;
  t4: number;
  t5: number;
};

export type Dd0Request = {
  v0: number;
  v1: number;
  v2: number;
  v3: number;
  v4: number;
  v5: number;
  v6: number;
};
export type Dd0Response = {
  t0: number;
  t1: number;
  t2: number;
  t3: number;
  t4: number;
  t5: number;
};

export type Laplace0Request = {
  v0: number;
  v1: number;
  v2: number;
  v3: number;
  v4: number;
  v5: number;
  v6: number;
};
export type Laplace0Response = {
  v0: number;
  v1: number;
  v2: number;
  v3: number;
  v4: number;
  v5: number;
  v6: number;
};

export type HodgeRequest = {
  generation: number;
};
export type HodgeResponse = {
  star0_center: number;
  star0_ring: number;
  star1_spoke: number;
  star1_ring: number;
  star2_face: number;
};

export type SubmitViewRequest = {
  generation: number;
  c0: number;
  c1: number;
  c2: number;
  c3: number;
  c4: number;
  c5: number;
  c6: number;
  show_laplacian: number;
};
export type SubmitViewResponse = {
  submitted: boolean;
};

export interface CanonicalInterfaceCaller {
  call(lane: "probe", value: ProbeRequest): Promise<ProbeResponse>;
  call(lane: "d0", value: D0Request): Promise<D0Response>;
  call(lane: "d1", value: D1Request): Promise<D1Response>;
  call(lane: "dd0", value: Dd0Request): Promise<Dd0Response>;
  call(lane: "laplace0", value: Laplace0Request): Promise<Laplace0Response>;
  call(lane: "hodge", value: HodgeRequest): Promise<HodgeResponse>;
  call(lane: "submit_view", value: SubmitViewRequest): Promise<SubmitViewResponse>;
}

export declare function createInterfaceCaller(
  exports: WebAssembly.Exports,
): CanonicalInterfaceCaller;

export interface CanonicalActorRequest<Lane extends string, Payload> {
  lane: Lane;
  payload: Payload;
}
export interface CanonicalActorContext {
  readonly signal: AbortSignal;
}
export interface CanonicalActorShape {
  readonly requestSchema: Readonly<Record<string, (value: unknown) => void>>;
  readonly resultSchema: Readonly<Record<string, (value: unknown) => void>>;
  transferRequest(value: unknown, request: { lane: string }): ArrayBuffer[];
  transferResult(value: unknown, request: { lane: string }): ArrayBuffer[];
}
export interface CanonicalActorAdapter extends CanonicalActorShape {
  dispatch(request: CanonicalActorRequest<"probe", ProbeRequest>, context?: CanonicalActorContext): Promise<ProbeResponse>;
  dispatch(request: CanonicalActorRequest<"d0", D0Request>, context?: CanonicalActorContext): Promise<D0Response>;
  dispatch(request: CanonicalActorRequest<"d1", D1Request>, context?: CanonicalActorContext): Promise<D1Response>;
  dispatch(request: CanonicalActorRequest<"dd0", Dd0Request>, context?: CanonicalActorContext): Promise<Dd0Response>;
  dispatch(request: CanonicalActorRequest<"laplace0", Laplace0Request>, context?: CanonicalActorContext): Promise<Laplace0Response>;
  dispatch(request: CanonicalActorRequest<"hodge", HodgeRequest>, context?: CanonicalActorContext): Promise<HodgeResponse>;
  dispatch(request: CanonicalActorRequest<"submit_view", SubmitViewRequest>, context?: CanonicalActorContext): Promise<SubmitViewResponse>;
}

export interface CanonicalHostEffectHandlers {
  "probe"?: (request: ProbeRequest, context: CanonicalActorContext) => ProbeResponse | PromiseLike<ProbeResponse>;
  "d0"?: (request: D0Request, context: CanonicalActorContext) => D0Response | PromiseLike<D0Response>;
  "d1"?: (request: D1Request, context: CanonicalActorContext) => D1Response | PromiseLike<D1Response>;
  "dd0"?: (request: Dd0Request, context: CanonicalActorContext) => Dd0Response | PromiseLike<Dd0Response>;
  "laplace0"?: (request: Laplace0Request, context: CanonicalActorContext) => Laplace0Response | PromiseLike<Laplace0Response>;
  "hodge"?: (request: HodgeRequest, context: CanonicalActorContext) => HodgeResponse | PromiseLike<HodgeResponse>;
  "submit_view"?: (request: SubmitViewRequest, context: CanonicalActorContext) => SubmitViewResponse | PromiseLike<SubmitViewResponse>;
}
export declare function compileActorAdapter(): CanonicalActorShape;
export declare function createActorAdapter(
  exports: WebAssembly.Exports,
  options?: { maxPendingPerLane?: number },
): CanonicalActorAdapter;
export declare function createHostEffectAdapter(
  handlers: CanonicalHostEffectHandlers,
  options?: { maxPendingPerLane?: number },
): CanonicalActorAdapter;
