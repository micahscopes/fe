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

Current derive convention: `ParamBindingsProvider` numbers the input record's
fields in declaration order. Keep those fields as the view's matching prefix
and append readouts. It does not yet derive arbitrary cross-record field
ordinals; interleaving unrelated observations with inputs is unsupported by
that derive. General field-ordinal reflection would remove this existing order
restriction without host-side remapping.

Gates: `surface_readout` compiler integration test (initialized positive and
missing-owner negative), plus runtime readout creation, refresh, malformed-plan
rejection, direct edit rejection and scripted `.params` rejection. The Quilting
composition is the downstream browser consumer.
