# MB2 refreshed Sonatina integration

Experimental branch, not the published demo toolchain yet.

Pins Sonatina d5c001d83500080c5f1c49941a92b53588730ede, rebased onto
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


The dependency now also repairs phi-free shared returning loop exits. A compact
Fe array-addition implementation had caused the backend to reject its helper
and force expansion through its callers. The Sonatina regression reproduces
the multiply-consumed return before repair;32 structurizer tests and a focused
WGSL helper-emission/validation regression pass after repair. Full renderer and
application GPU checks remain separate. Library test API migrations do not
change instruction capabilities or weaken assertions.
