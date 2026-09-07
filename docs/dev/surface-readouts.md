# Actor-owned surface readouts

Fe surfaces may display initialized actor state without treating it as input:

```fe
struct Params { request:Param, resolved:Param }
const fn params()->Params {
    Params {
        request:Param::range(min:0.0,max:8.0,init:2.0),
        resolved:Param::readout(ParamReadout::Integer),
    }
}
```

`resolved` names an existing scalar actor-state leaf, possibly nested in a
separate diagnostic record. `InitialState` owns its initial value; ordinary Fe
transitions own updates. No initial value is synthesized by the declaration.
The host realizes the Fe `State` source and `Output` presentation as a native
`<output>`, refreshes it with the current snapshot, and rejects parameter edits.
This is UI input ownership, not a security boundary against arbitrary page code.

The compiler rejects readouts without `InitialState` and malformed combinations
of ownership, initialization and presentation. The authored `Fixed` drive
semantics preserve state even if used in an input record. No application
calculation belongs in the host or manifest generator.

Use `derive ApplyParamBindings for Controls using ParamBindingsProvider<Params>`.
The configured record is the same `Params` associated with `Controls` by
`SurfaceState`. The provider reflects parameter declarations and matches input
field names at compile time; their ordinals need not match the input record's
order. Readouts can therefore precede or interleave inputs without shifting
which state field an edit changes. Missing input labels leave an incomplete
generated state construction and fail compilation, rather than silently
binding to ordinal zero. Name matching emits no runtime string lookup and
requires no host-side index remapping.

Gates: `surface_readout` compiler integration test (initialized positive and
missing-owner negative), plus runtime readout creation, refresh, malformed-plan
rejection, direct edit rejection and scripted `.params` rejection. The Quilting
composition is the downstream browser consumer.
