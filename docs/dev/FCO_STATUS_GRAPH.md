# FCO status snapshot

**Snapshot as of commit `f3c63e1f4` (2026-06-14). The repo is authoritative;
the full DAG is `fco_dependency_graph_v0.json`. Update this after each gate
commit.**

```mermaid
flowchart TD
  subgraph SUB["FCO substrate — M5 (you are here)"]
    D["where const predicate"] --> O["obligation — one queue"]
    O --> R{"discharge route"}
    R -->|concrete| CTFE["CTFE @ obligation level<br/>(never in proof forest)"]
    R -->|symbolic| ASM["assumption route<br/>exact term identity"]
    CTFE --> EV["evidence record"]
    ASM --> EV
    EV --> PREM["premises:<br/>Ctfe=none, Assumption=matched"]
    EV --> HOV["hover / fe explain receipts"]
    EV --> DIAG["diagnostics 8-0085 / 3-0025"]
    O --> GT["gate-not-select (trait-bound path)"]
    O --> GC["gate-not-select (concrete method-call path)"]
    MC["M0 method-predicate identity → 6-0016"] --> DIAG
    UNS["unsupported projection → named diag"]
  end
  SUB ==> NS["north star: kinded obligations / Constraint kind<br/>K00–K08 (G1 lifted to the kind level)"]

  classDef done fill:#1b5e20,color:#fff,stroke:#2e7d32;
  classDef partial fill:#6d4c41,color:#fff,stroke:#a1887f;
  classDef star fill:#1a237e,color:#fff,stroke:#3949ab;
  class D,O,R,CTFE,ASM,EV,PREM,HOV,DIAG,GT,MC done;
  class GC,UNS partial;
  class NS star;
```

| Item | Status | Anchor |
|---|---|---|
| Obligation pipeline · CTFE + assumption routes | **COMPLETE_AND_TESTED** | gates 1/3/4–7 |
| Evidence + premises (Ctfe none / Assumption matched) | **COMPLETE_AND_TESTED** | A3 `abf8a6247` |
| Hover / receipts | **COMPLETE_AND_TESTED** | gate 10 |
| Gate-not-select — trait-bound path | **COMPLETE_AND_TESTED** | gate 2 |
| M0 method const-predicate conformance (`6-0016`) | **COMPLETE_AND_TESTED** | `468bb69b7` |
| Gate-not-select — **concrete `w.big()` path** | **PARTIAL** | Gate-2 tail — quarantined |
| Expressibility-limit diagnostic (dedicated `8-0086`) | **PARTIAL** | named-not-ICE holds (`2-0002`) |
| WF-position evidence recording | **BLOCKED_BY_DESIGN** | — |
| **Kinded obligations / `Constraint` kind (north star)** | **NOT_EXPRESSIBLE_YET (tracked)** | K00–K08; track-now-implement-later |

**Lint:** branch passes `cargo fmt --all --check` + `clippy -D clippy::all` for
tracked code (`6207a808d`, `ed9754748`). The untracked
`crates/contract-harness/tests/foundry_gas.rs` is parallel-session WIP, broken,
and absent on a clean checkout.

**Next:** Gate-2 tail (unify the concrete method-call path through the same
selected-impl obligation emitter as the trait-bound path) → then `8-0086`
expressibility polish. K01 (named diagnostics for planned kind forms) is the
sanctioned early pull on the north-star spine.
