# MB2 refreshed Sonatina integration

Experimental branch, not the published demo toolchain yet.

Pins Sonatina 8c8e60b8221b272e96f11654ce7c16fd27098ac5, rebased onto
upstream 6dcaaa557, while preserving source type and field labels in WGSL.

The frontend adapts backend options, small-vector phi arguments, capability-checked
bitcast construction, native JIT artifact APIs and entry roots for return-lane
elimination. Wasm optimization levels continue to follow the frontend setting.

Validation: locked default-feature fe-codegen library check passes
(`/lab/argot/scratch/mb2-sonatina-upstream-rebase-20261022/check-fe-2.log`).
The CLI build, native-feature frontend check, targeted lowering regressions and
actual demo emission/numerical/performance gates remain pending.

The Sonatina checkpoint passes shader-name executable invariance (1/1), the MB2
native numeric subset (23/23), and upstream native JIT tests (74/74). Seven older
native allocation/wide-result tests still require contract reconciliation; these
checks do not certify physical phone startup.
