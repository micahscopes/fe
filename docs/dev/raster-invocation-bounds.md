# Direct-draw invocation bounds

The arc viewer exposed a missing connection between two compiler-owned facts:
`Instanced<TriangleStrip<4>,256>` supplies the actual draw counts, but the vertex
body previously received unconstrained u32 arguments during optimization. Its
ordinary `segment_index + 1` therefore retained an overflow trap which raster
admission correctly refused to discard.

The actor compilation plan now carries the direct draw's vertex/instance counts
to a crate-private raster lowering entry. The same `WebActorDraw::Direct` value
supplies the emitted manifest. Current direct realization uses firstVertex and
firstInstance zero, so nonempty counts imply inclusive index bounds `[0,N-1]`.
An implementation which later adds nonzero offsets must change this contract.

Sonatina's range branch simplifier can specialize a function using caller-proved
u32 argument intervals. It validates positions, lane types, interval ordering and
duplicate specifications before mutation. These are immutable argument facts,
not return-value facts or assumptions about arbitrary local values. Normal
unconstrained analysis and interprocedural return summaries remain unchanged.

Important limits:

- Only the actor compiler's direct-draw path supplies bounds. Public standalone
  raster compilation APIs remain unconstrained.
- Indirect buffers, mutable actor fields and fragment inputs supply no fixed
  index bounds, even if a particular test happens to write small values.
- A vertex entry with source-language callers is not specialized: those calls
  are not governed by the draw declaration.
- Zero-count draws supply no interval for that index. This change does not
  synthesize a vacuously safe shader for an otherwise unsupported empty draw.
- This proves arithmetic guards; it does not substitute wrapping arithmetic,
  clamp inputs, inline an entire helper graph or add JavaScript filtering.
- Bounds currently specialize the entry, not arbitrary helper parameters.
  Helper-call argument propagation is separate future work if needed.

The small regression `raster_invocation_bounds` exercises vertex/instance
increment success, underflow/overflow rejection, actor-data rejection and
indirect-draw rejection. Sonatina's invocation-bound test includes the unsigned
maximum boundary and rejects malformed specifications without changing IR.
Run results and actual arc-viewer acceptance must be recorded separately; these
fixtures alone do not prove browser rendering or all patch controls complete.

Observation limitation: specialization currently precedes the normal shader
capture lifecycle. Its removed guards therefore are already absent at that
capture's initial stage. Comparing that stage with an unconstrained standalone
capture is not evidence of an inliner change. A future shared request-fact
observation should include draw bounds and this early optimization boundary.

## September 7 execution receipt

- Sonatina `568f5514`: focused invocation-bound test and 40 range-related tests
  passed in release mode.
- Fe `raster_invocation_bounds`: all six cases passed in 22.94 seconds.
- `authored_raster_e2e`: seven tests passed in 18.26 seconds, including original
  and bounded-increment pixel readback on AMD Radeon 780M (RADV PHOENIX).
  The first attempt had no Vulkan loader in the process environment and failed
  adapter discovery; the successful run explicitly supplied the installed loader
  and Radeon `VK_DRIVER_FILES`. No GPU skip was enabled.
- Release CLI rebuilt; the unchanged arc vertex expression now compiles. The
  standalone has three passes, 26,250 Wasm bytes and 68,796 total WGSL bytes.
- Chrome MCP page 67 at `http://127.0.0.1:38332/` renders the arc and all three
  handles. Delivering middle-handle down/move/up events through the shared runtime
  transition entry selected ID 1, moved only the middle control and visibly changed
  the curve. No browser warnings/errors were reported. This is not yet a native
  pointer-capture/touch acceptance test.

Logs: `/laboratory/quilting/scratch/arc-direct-draw-{compiler-test,raster-radeon}-20260907.log`
and `arc-bounded-web-dev-20260907.log`. No patch-family authoring completion is
implied by restoring the arc viewer.
