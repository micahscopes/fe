# Fe raster color policies

`std::webgpu::raster` composes color attachment lifetime, blending, write masks,
depth and multisampling as ordinary Fe policies. The actor's const
`RasterConfiguration` behavior evaluates the policy; the browser realizes that
value. Applications do not supply JavaScript pipeline descriptors. Protocol v11
stores the raster policy independently of the optional `view()` description;
UI-free render actors retain exactly the same raster semantics. The host still
reads the nested policy from older bundles.

```fe
type Transparent = RasterState<
    NoDepth, Samples4, CullNone,
    ColorTarget<OpaqueBlack<ClearThenLoadStore>, StraightAlpha, COLOR_WRITE_ALL>,
>

const fn raster() -> RasterPlan uses (RasterConfiguration) {
    raster_plan<Transparent>()
}
```

## Color meanings

| Policy | Fragment RGB convention | Result |
| --- | --- | --- |
| `ReplaceColor` | Ordinary RGB | Replace the enabled channels; no blending |
| `StraightAlpha` | RGB has not been multiplied by alpha | Source-over composition |
| `PremultipliedAlpha` | RGB already multiplied by alpha | Source-over composition |
| `AdditiveColor` | RGB/alpha already scaled to the desired contribution | Add both source and destination |

Both source-over policies compute output alpha as
`source.alpha + destination.alpha * (1 - source.alpha)`. Additive blending does
not multiply RGB by source alpha automatically. The target's numeric format
determines clamping. These policies do not choose the fragment's color space.

Implement `RasterBlendPolicy` for other combinations of the core `BlendFactor`
and `BlendOperation` values. `Min` and `Max` use `One` for both factors.
`WithBlendConstant<Color, Constant>` supplies a `RasterBlendConstant` policy for
the constant factors. RGB and alpha have independent blend components.

`COLOR_WRITE_RED`, `GREEN`, `BLUE`, `ALPHA` and `ALL` are channel bit masks.
A mask of zero intentionally preserves all destination channels; it does not
disable rasterization or depth writes. Blending and depth writes are independent.

## Boundaries

- This is ordinary ordered blending, **not order-independent transparency**.
  Transparent overlap still requires an appropriate ordering or a different
  compositing algorithm. An invisible fragment can still write depth if enabled.
- The current actor raster plan is shared by its raster passes. Per-pass raster
  overrides and indexed drawing are separate
  API work, not implied by the color policy.
- Dual-source blending requires an optional WebGPU feature and is not included
  in the portable factors.
- Multisampling is separate from blend state; it does not resolve transparency
  ordering. Alpha-to-coverage is explicit, never inferred from a blend mode.

## Sample coverage and depth comparisons

```fe
type Cutout = WithAlphaToCoverage<RasterState<
    DepthTest<Depth24Plus, CompareLessEqual, DepthWrite, ClearThenLoadStore>,
    Samples4, CullNone, OpaqueBlack<ClearThenLoadStore>,
>>
type TwoSamples = WithSampleMask<Cutout, 5>
```

These wrappers modify only their own field of the Fe plan. The sample mask is
an unsigned 32-bit value; zero is valid and disables all covered samples.
`WithAlphaToCoverage` turns fragment alpha into multisample coverage. It requires
four samples on the portable path and does not enable blending. Combining it
with source-alpha blending applies both attenuations, which is usually not what
an alpha-tested cutout wants. Neither mechanism supplies OIT.

All eight core depth comparisons are available, including `CompareNever`,
`CompareEqual`, and `CompareNotEqual`. Those three use a depth clear of 1;
custom `RasterDepthCompare` implementations can choose a different clear value.

Protocol v13 transports `sample_mask` and `alpha_to_coverage` from Fe to the
native pipeline descriptor. Older bundles retain an all-ones mask and disabled
alpha-to-coverage. Invalid masks and single-sample alpha coverage are rejected
before pipeline creation. Current output is the alpha-bearing canvas target;
future typed targets must also enforce WebGPU's blendable-alpha-format rule.

The `actor_sample_coverage` fixture checks Fe composition and shader compilation;
host tests cover all eight comparisons, unsigned masks (including high-bit and
zero masks), malformed values, and legacy decoding. The normative contract is
[WebGPU multisample state](https://www.w3.org/TR/webgpu/#multisample-state).

## Transport and executable evidence

`setBlendConstant` is selected from pinned WebGPU WebIDL. Its canonical adapter
accepts the official sequence/dictionary union, including `double` components.
The generated render bridge derives the dictionary case tag from bindgen's
canonical metadata. It neither reimplements the native call nor changes the IDL
to accommodate Fe's current scalar subset.

The flat Fe WebIDL emitter does not yet support `double`. The provenance gate
asserts this diagnostic separately from the canonical JavaScript transport test;
the latter executes both union forms and checks exact double values and released
borrows. This is not a claim that arbitrary binary64 Fe imports already work.

- `actor_construct`: const policy projection, custom constant and RGB-only mask.
- `fe-render-runtime.test.mjs`: blend descriptor interpretation and invalid
  factor/operation/mask rejection.
- `upstream_provenance`: pinned selection, generated operations and union ABI.
- `render_runtime_assembles_the_pinned_webgpu_webidl_transport`: assembled runtime
  syntax and one generated native call, without a handwritten fallback.
- `tests/fixtures/actor_alpha_raster/index.html`: serve with `fe web dev`; the Fe
  fragment outputs half-alpha red over blue. The live triangle must be purple,
  with opaque destination alpha retained by the RGB-only write mask.

Browser receipt (2026-09-04, release `fe web dev`, Chromium via Chrome MCP):
the UI-free v10 baseline produced center RGBA `[255, 0, 0, 128]` and background
`[0, 0, 255, 255]`; its raster configuration had been lost. With v11, the same
fixture produced `[128, 0, 127, 255]` and `[0, 0, 255, 255]`. The live plan
reported four samples, write mask 7, and blend constant `(0.25, 0.5, 0.75, 1)`
without a pipeline error. Pixels were read from the runtime's GPU-readback
poster after `await surface.freeze()`, not from an expired canvas texture.

## Remaining shared API work

The color slice does not constitute a complete WebGPU rendering interface.
Continue in small independently tested changes on mb2:

Per-pass primitive assembly is implemented separately in [Draw policies](DRAW_POLICIES.md).

1. Typed index resources, 16/32-bit formats, indexed and indexed-indirect draws,
   first/base offsets, strip restart, and exact resource identity/usage tracking.
   Compute-authored arguments and indices must remain GPU-resident.
2. Pass-local raster composition so opaque geometry, transparent curves and
   overlays can use different blend/depth-write policies in one actor graph.
3. Complete depth comparisons, depth bias and stencil
   policies; viewport/scissor and stencil-reference commands through WebIDL.
4. Multisample masks and explicit alpha-to-coverage; typed color targets and
   capability-checked formats, including a non-packed floating color output path.

Compare each slice with the [WebGPU specification](https://www.w3.org/TR/webgpu/)
and pinned WebIDL, not just with the immediate demo. Optional features must be
explicit capabilities; unsupported combinations must fail before submission.
Keep policy values in Fe, physical realization in the common backend, and test
both emitted descriptors and rendered pixels. Native line primitives are useful
but do not eliminate the need for geometric ribbons with controllable thickness.
