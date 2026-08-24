use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};

#[test]
fn browser_actor_providers_select_compute_and_verify_by_type() {
    let source = r#"
use core::Worker
use core::execution::{
    Checked, Compute, ComputeRequest, Verify, VerifyRequest, compute_then_verify,
}
use core::pending::{Pending, Suspend, TaskOutcome}
use std::host::Resumable
use std::runtime::{BrowserActorCompute, BrowserActorVerify}
use std::wasm::WasmBackend

struct Square {}
struct DirectCheck {}
struct Input { value: u32 }
struct Output { value: u32 }
impl Copy for Output {}
struct Claim { expected: u32 }
struct Verdict { accepted: bool }

actor ScalarSquare {
    fn evaluate(request: ComputeRequest<Square, Input>) -> Output uses (Worker) {
        Output { value: request.input.value * request.input.value }
    }
}

actor DirectVerifier {
    fn check(request: VerifyRequest<DirectCheck, Claim, Output>) -> Verdict uses (Worker) {
        Verdict { accepted: request.claim.expected == request.output.value }
    }
}

fn begin_square(_ input: own Input) -> Pending<WasmBackend, Output>
    uses (compute: mut Compute<WasmBackend, Square, Input, Output>)
{
    compute.compute_begin(input)
}

fn begin_check(
    _ claim: own Claim,
    _ output: own Output,
) -> Pending<WasmBackend, Verdict>
    uses (verify: mut Verify<WasmBackend, DirectCheck, Claim, Output, Verdict>)
{
    verify.verify_begin(claim, output)
}

fn run_workflow(
    _ input: own Input,
    _ claim: own Claim,
) -> TaskOutcome<u32, Checked<Output, Verdict>>
    uses (
        compute: mut Compute<WasmBackend, Square, Input, Output>,
        verify: mut Verify<WasmBackend, DirectCheck, Claim, Output, Verdict>,
        suspend: Suspend<WasmBackend, u32>,
    )
{
    compute_then_verify<
        WasmBackend,
        Square,
        DirectCheck,
        Input,
        Output,
        Claim,
        Verdict,
        u32,
    >(input, claim)
}

fn select_scalar(_ input: own Input) -> Pending<WasmBackend, Output> {
    with (
        Compute<WasmBackend, Square, Input, Output> = BrowserActorCompute<ScalarSquare, Square> {},
    ) {
        begin_square(input)
    }
}

fn select_direct(
    _ claim: own Claim,
    _ output: own Output,
) -> Pending<WasmBackend, Verdict> {
    with (
        Verify<WasmBackend, DirectCheck, Claim, Output, Verdict> = BrowserActorVerify<DirectVerifier, DirectCheck> {},
    ) {
        begin_check(claim, output)
    }
}

fn select_test_stack(
    _ input: own Input,
    _ claim: own Claim,
) -> TaskOutcome<u32, Checked<Output, Verdict>> {
    with (
        Compute<WasmBackend, Square, Input, Output> = BrowserActorCompute<ScalarSquare, Square> {},
        Verify<WasmBackend, DirectCheck, Claim, Output, Verdict> = BrowserActorVerify<DirectVerifier, DirectCheck> {},
        Suspend<WasmBackend, u32> = Resumable {},
    ) {
        run_workflow(input, claim)
    }
}
"#;

    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("execution_effects.fe".into(), source);
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn browser_actor_compute_provider_rejects_the_wrong_kernel_brand() {
    let source = r#"
use core::Worker
use core::execution::{Compute, ComputeRequest}
use core::pending::Pending
use std::runtime::BrowserActorCompute
use std::wasm::WasmBackend

struct Square {}
struct DifferentKernel {}
struct Input { value: u32 }
struct Output { value: u32 }

actor ScalarSquare {
    fn evaluate(request: ComputeRequest<Square, Input>) -> Output uses (Worker) {
        Output { value: request.input.value * request.input.value }
    }
}

fn begin_wrong(_ input: own Input) -> Pending<WasmBackend, Output>
    uses (compute: mut Compute<WasmBackend, DifferentKernel, Input, Output>)
{
    compute.compute_begin(input)
}

fn reject_wrong_brand(_ input: own Input) -> Pending<WasmBackend, Output> {
    with (
        Compute<WasmBackend, DifferentKernel, Input, Output> = BrowserActorCompute<ScalarSquare, DifferentKernel> {},
    ) {
        begin_wrong(input)
    }
}
"#;

    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("execution_effects_wrong_brand.fe".into(), source);
    let (top_mod, _) = db.top_mod(file);
    let rendered = format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains(
            "requires `BrowserActorCompute<ScalarSquare, DifferentKernel>` to implement",
        ) && rendered.contains(
            "does not implement `Compute<WasmBackend, DifferentKernel, Input, Output>`",
        ),
        "expected the wrong computation brand to be rejected:\n{rendered}"
    );
}
