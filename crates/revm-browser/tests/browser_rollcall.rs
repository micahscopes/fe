use fe_revm_browser::{RevmSession, RevmStatus};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::*;

#[cfg(feature = "browser-tests")]
wasm_bindgen_test_configure!(run_in_browser);

const RUNTIME: &[u8] = include_bytes!("fixtures/rollcall_depth4/runtime.bin");
const COMMIT_CALLDATA: &[u8] = include_bytes!("fixtures/rollcall_depth4/commit.calldata.bin");
const ACCEPT_CALLDATA: &[u8] = include_bytes!("fixtures/rollcall_depth4/accept.calldata.bin");
const REJECT_CALLDATA: &[u8] = include_bytes!("fixtures/rollcall_depth4/reject.calldata.bin");

fn encoded_bool(value: bool) -> Vec<u8> {
    let mut word = vec![0; 32];
    word[31] = u8::from(value);
    word
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fe_rollcall_verifier_matches_native_accept_and_reject_vectors() {
    assert!(
        !RUNTIME.is_empty(),
        "the Fe runtime fixture must be populated"
    );

    let mut session = RevmSession::new(RUNTIME);
    let commit = session.call(COMMIT_CALLDATA);
    assert_eq!(commit.status(), RevmStatus::Success);
    assert!(commit.output().is_empty(), "commit should return no bytes");

    let accept = session.call(ACCEPT_CALLDATA);
    assert_eq!(accept.status(), RevmStatus::Success);
    assert_eq!(accept.output(), encoded_bool(true));

    let reject = session.call(REJECT_CALLDATA);
    assert_eq!(reject.status(), RevmStatus::Success);
    assert_eq!(reject.output(), encoded_bool(false));
}
