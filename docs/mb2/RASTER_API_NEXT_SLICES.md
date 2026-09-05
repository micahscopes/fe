# Remaining raster API work

The direction is Fe-authored values, effectful realization through the shared
WebIDL path, and reusable actor resources. A demo must not invent a second
pipeline-descriptor or buffer-lifetime interface.

## Available now

- Fe color target policies: independent RGB/alpha blend operations and factors,
  constant factors, channel masks, straight/premultiplied alpha and additive modes.
- All five native primitive topologies, winding, instancing, non-indexed indirect
  draws with exact actor resource identity, and zero-count draws.
- Depth tests, independent depth writes, attachment load/store ownership, 1×/4×
  sampling, sample masks and explicit alpha-to-coverage.

The executable boundaries are in `actor_construct.rs` and the render-runtime
host tests. The color, primitive and sample-coverage fixtures can each be served
independently with `fe web dev`. These are not a claim of full WebGPU coverage.

## 1. Indexed drawing

Carry the index resource's exact actor field identity, not just its nominal
type. Expose uint16/uint32 index formats, range/offset, first index, base vertex,
first instance and strip restart through typed Fe draw policies. Admit index
usage during allocation even when the same buffer is written by compute; do
not add that buffer to a shader bind group merely because the draw consumes it.

Both direct and indexed-indirect commands must use the generated WebIDL
operations. An indexed indirect argument record has five words and a signed
base-vertex lane, unlike the four-word non-indexed record. Check alignment,
range, ownership, portable limits and first-instance feature requirements.

Acceptance: a compute-written index buffer and indirect arguments render with
no GPU→CPU readback; shared resource identity survives multiple passes; uint16
and uint32 paths, empty draws, strip restart and invalid ranges are exercised.

## 2. Pass-local pipeline policies

Allow opaque surfaces, faded curves and overlays in one actor without making
them share depth-write/blend behavior. Derive the policy from the stage's Fe
types and include it in pipeline realization/memoization. Do not branch on
entry-point names in the host.

Separate attachment ownership from pipeline tests: a pass using no depth test
must not implicitly clear or discard another pass's depth texture. Attachment
format, dimensions and sample count must agree across passes that share a
target; incompatible transitions need explicit resources/resolve operations.
Likewise, switching a pipeline is not permission to recreate its attachments.

Acceptance: an opaque surface writes depth, a translucent curve tests without
writing it, and an overlay ignores depth. Ordering and reload/recovery preserve
the declared result. This still does not provide order-independent transparency.

## 3. Raster regions and depth/stencil

Add typed viewport/scissor commands, depth bias and complete stencil state
(front/back operations, masks, reference, clear/store behavior). Keep dynamic
commands distinct from immutable pipeline state. Select operations from WebIDL
and validate both their numerical domains and attachment compatibility.

## 4. Targets and richer shader outputs

Support explicitly typed offscreen color/depth targets, formats, sampled reuse,
multiple render targets and float fragment results. Keep packed RGBA8 as a
convenience, not the only color representation. Feature-gate optional formats,
unclipped depth and other device capabilities rather than requesting elevated
limits by default. Alpha-to-coverage requires a blendable alpha-bearing target;
do not lose that rule when adding target types.

Derive buffer/attachment layouts and resource access from Fe declarations.
Keep allocation, recovery and destruction in the common effectful host rather
than in individual demos. Prefer small end-to-end fixtures over unexercised
enumerations of API constants.
