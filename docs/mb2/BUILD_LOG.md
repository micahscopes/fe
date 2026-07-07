# mb2 multi-backend: build log

Running, dated log of what each slice landed. Branch `mb2` off `type-fn`
(HEAD `81cf8e083`, carrying FCO + the built type->type CTFE keystone).
Driving doc: `/workspace/mb2-fable-ladder-01.md` (the Fable ladder).
No AI attribution in commits. No em-dashes. Never push.

Green means the exact full CI command:
`cargo nextest run --release --workspace --all-features --no-fail-fast --locked`.

## 2026-07-07 - Slice A0: baseline

- mb2 base `81cf8e083` ("fix(hir): make ground type-fn normalization stack-safe
  [s3c]") is exactly the commit where the final type-fn release CI ran green:
  2512 passed / 0 failed. Recorded as the A0 branch baseline; NOT re-run this
  session per the orchestrator note (re-running the full release suite is the
  orchestrator's job at slice boundaries).
- No GAT support in the base, confirmed: `AssocTyDecl` / `AssocTyDef` carry no
  `generic_params`; `ty_lower.rs` `GenericArg::AssocType` is the pre-D1 TODO;
  `ty_def.rs` `HasKind for TyData::AssocTy` computes kind from bounds only.
- Every later slice diffs against 2512.
