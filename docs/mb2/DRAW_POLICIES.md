# Fe primitive and draw policies

Primitive assembly is a Fe value, evaluated per raster pass. It is independent
of whether counts are compile-time constants, instanced, or GPU-written.

```fe
VertexStage<V, PointList<64>>
VertexStage<V, LineList<128>>
VertexStage<V, LineStrip<65>>
VertexStage<V, TriangleList<192>>
VertexStage<V, TriangleStrip<4>>

VertexStage<V, Instanced<TriangleStrip<4>, 256>>
VertexStage<V, IndirectDraw<LineStripTopology, DrawIndirectBuffer<Commands>>>
```

The short names are aliases for `DirectDraw<Primitive, N>`. `PrimitivePolicy`
denotes a `PrimitivePlan { topology, front_face }`; `Clockwise<P>` changes only
winding. Applications can implement the trait and compose their own policies.
The generic `primitive_plan<P>()` evaluator shares the compiler's const-plan
discovery machinery with resource policies. The compiler does not own a second
enumeration of point/line/triangle topology semantics.

`Instanced<D, N>` preserves the inner primitive policy and multiplies its instance
count with overflow checking. Its vertex behavior takes `vertex_index` followed
by `instance_index`. `IndirectDraw<P, Args>` also supplies both indices; `Args`
must name one exact actor indirect resource containing the four WebGPU words
`[vertex_count, instance_count, first_vertex, first_instance]`. Compute writes
and the draw consumes that same buffer without host readback. Optional device
features such as nonzero indirect first-instance are not implicitly enabled.

Zero direct vertices or zero instances are valid no-ops. Counts must fit u32;
neither truncation nor saturation is used to conceal an oversized request.
An invalid policy must evaluate to an error, not an empty plan that activates a
host default. The projection checks that CTFE produced the exact nominal plan
record, including when ill-typed evaluation exposes a recovery Unit.

Protocol v12 carries `primitive` on each authored draw. Old triangle-list marker
types and older bundles remain readable, but a v12 bundle needs a v12 host.
Newly compiled standard `TriangleList` and `IndirectTriangleList` names use the
same generic policies as lines and points.

## Curve and patch use

- A line strip connects successive vertices. It must not bridge a projective
  pole just because adjacent parameters lie on opposite sides of infinity.
  Use independent line-list segments, or independently instanced strips, when
  a continuity test rejects the interval.
- Native lines and points are useful thin primitives, not an arbitrary-width
  stroke API. A four-vertex instanced triangle strip provides an ordinary
  geometric ribbon with controllable width. Native MSAA can cover its boundary;
  alpha blending handles a separately authored fade.
- Indexed drawing and strip restart are the next slice, not implemented by
  these non-indexed policies. Each strip instance currently starts anew.
- Blend/depth attachment policy remains actor-wide. Primitive assembly is now
  per-pass; that does not imply arbitrary pass-local depth/blend overrides yet.

## Evidence

`actor_primitive_topologies` compiles all five topologies, instanced lines,
clockwise line-strip policy, a compute-written indirect line strip and an empty
draw. `actor_primitive_missing_policy` proves that `u32` cannot masquerade as a
primitive policy. The host test covers both winding values, every topology,
invalid/null plans and no-op/count bounds. Existing actor/resource regressions
continue to cover shared identity, binding budgets and stage visibility.

Browser receipt (2026-09-04, release `fe web dev`, Chromium via Chrome MCP):
the 256×256 scene rendered without pipeline errors. White pixel counts in its
six cells were `[4, 116, 138, 1035, 2116, 138]`: points, instanced lines, line
strip, triangle, triangle strip and indirect line strip. The two strip cells
match; the four-vertex triangle strip fills a square while the three-vertex
triangle fills half its square. The zero-count draw adds no pixels. Inspection
of the GPU-readback poster also confirmed the expected geometry visually.

Serve the regression independently:

```sh
target/release/fe web dev crates/codegen/tests/fixtures/actor_primitive_topologies/index.html
```
