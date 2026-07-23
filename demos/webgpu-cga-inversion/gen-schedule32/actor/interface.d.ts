export declare const canonicalInterfaceManifest: Readonly<object>;
export declare const compiledCanonicalInterface: Readonly<object>;

export type RenderRequest = {
  generation: number;
  cam_x: number;
  cam_y: number;
  zoom: number;
  inv_cx: number;
  inv_cy: number;
};
export type RenderResponse = {
  submitted: boolean;
};

export type VerifyRequest = {
  generation: number;
  cam_x: number;
  cam_y: number;
  zoom: number;
  inv_cx: number;
  inv_cy: number;
};
export type VerifyResponse = Uint8Array;

export type OracleRequest = {
  generation: number;
  cam_x: number;
  cam_y: number;
  zoom: number;
  inv_cx: number;
  inv_cy: number;
};
export type OracleResponse = Uint8Array;

export interface CanonicalInterfaceCaller {
  call(lane: "render", value: RenderRequest): Promise<RenderResponse>;
  call(lane: "verify", value: VerifyRequest): Promise<VerifyResponse>;
  call(lane: "oracle", value: OracleRequest): Promise<OracleResponse>;
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
  dispatch(request: CanonicalActorRequest<"render", RenderRequest>, context?: CanonicalActorContext): Promise<RenderResponse>;
  dispatch(request: CanonicalActorRequest<"verify", VerifyRequest>, context?: CanonicalActorContext): Promise<VerifyResponse>;
  dispatch(request: CanonicalActorRequest<"oracle", OracleRequest>, context?: CanonicalActorContext): Promise<OracleResponse>;
}

export interface CanonicalHostEffectHandlers {
  "render"?: (request: RenderRequest, context: CanonicalActorContext) => RenderResponse | PromiseLike<RenderResponse>;
  "verify"?: (request: VerifyRequest, context: CanonicalActorContext) => VerifyResponse | PromiseLike<VerifyResponse>;
  "oracle"?: (request: OracleRequest, context: CanonicalActorContext) => OracleResponse | PromiseLike<OracleResponse>;
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
