//! End-to-end acceptance: the first genuinely-Fe GPU-path slice (rung R-val).
//!
//! This takes a tiny straight-line Fe source, compiles it Fe -> MIR -> the
//! wasm-path Sonatina `Module` (Wasm32 ISA / `NativeInstSet`), hands that Module
//! UNCHANGED to Sonatina's naga-backed `SpirvBackend`, and asserts the emitted
//! SPIR-V is naga-*validated* and starts with the SPIR-V magic word
//! `0x07230203`.
//!
//! Honest label (the words this rung earns): **validated, NOT executed.** The
//! naga validator runs inside `compile_module`, so an `Ok` artifact is a
//! structurally-valid compute module and nothing more. Actual GPU execution on
//! lavapipe is a later slice (S2); this file deliberately does not touch a GPU.
//!
//! The load-bearing fact under test: the wasm-path Module feeds `SpirvBackend`
//! *without adaptation* (Sonatina downcasts generically against
//! `function.inst_set()`), so there is no SPIR-V lowering port.

use common::InputDb;
use driver::DriverDataBase;
use url::Url;

/// The SPIR-V module magic number, `words[0]` of every valid SPIR-V binary.
const SPIRV_MAGIC: u32 = 0x0723_0203;

/// The keystone kernel, authored as one straight-line, call-free, param-free Fe
/// function so it lands inside SPIR-V's narrow envelope (single function,
/// Add/Mul/Return only, no branches, no calls). `funcs().first()` in the SPIR-V
/// translator is therefore unambiguously this kernel. Semantics mirror the
/// hand-built Poseidon-sigma known answer; all intermediates stay < 2^64.
const KEYSTONE_SOURCE: &str = "\
pub fn poseidon_sigma() -> u64 {\n\
\x20   let a0: u64 = 1\n\
\x20   let s0: u64 = a0 + 3\n\
\x20   let a1: u64 = s0 * s0 + s0\n\
\x20   let s1: u64 = a1 + 5\n\
\x20   let a2: u64 = s1 * s1 + s1\n\
\x20   let s2: u64 = a2 + 7\n\
\x20   let a3: u64 = s2 * s2 + s2\n\
\x20   let s3: u64 = a3 + 11\n\
\x20   let a4: u64 = s3 * s3 + s3\n\
\x20   a4\n\
}\n";

/// R-val, the direct path: Fe source -> wasm-path Module -> `SpirvBackend` ->
/// inspect the raw `SpirvArtifact`. Proves the wasm Module feeds SPIR-V
/// unchanged, that naga validation passes (an `Ok` return means the validator
/// accepted the module), that `words[0]` is the SPIR-V magic, and that the WGSL
/// side artifact needed by the later GPU-exec slice is produced.
#[test]
fn keystone_lowers_to_naga_validated_spirv() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///spirv_keystone.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KEYSTONE_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    // Reuse the wasm-path Sonatina Module (no SPIR-V lowering port), then hand it
    // to Sonatina's SpirvBackend. Failure here means either the wasm Module did
    // NOT feed SpirvBackend cleanly, or naga rejected the module.
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("keystone should build a wasm runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv(&db, &package)
        .expect("keystone wasm Module should compile to naga-validated SPIR-V unchanged");

    assert!(
        !artifact.words.is_empty(),
        "SPIR-V word stream must be non-empty"
    );
    assert_eq!(
        artifact.words[0], SPIRV_MAGIC,
        "words[0] must be the SPIR-V magic 0x07230203 (got {:#010x})",
        artifact.words[0]
    );
    // The WGSL side artifact is what the later lavapipe/browser exec slices need;
    // confirm the validated naga module produced it here (not consumed in S1).
    assert!(
        artifact.wgsl.is_some(),
        "the naga backend should emit a WGSL side artifact alongside SPIR-V"
    );
}

/// R-val, through the public `BackendKind::Spirv` compile driver: proves the
/// thin driver emits the canonical little-endian SPIR-V bytes and fails closed
/// (via `Ok`/`Err`, never wrong bytes). Reconstructs `words[0]` from the leading
/// four little-endian bytes and asserts the magic.
#[test]
fn spirv_backend_driver_emits_valid_spirv_bytes() {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///spirv_keystone_driver.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KEYSTONE_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Spirv
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Spirv), OptLevel::O0)
        .expect("spirv backend driver should compile the keystone");
    let bytes = output
        .into_bytecode()
        .expect("spirv output should be bytecode");

    assert!(
        bytes.len() >= 4 && bytes.len() % 4 == 0,
        "SPIR-V bytes must be a non-trivial multiple of 4 (got {})",
        bytes.len()
    );
    let word0 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert_eq!(
        word0, SPIRV_MAGIC,
        "leading word must be the SPIR-V magic 0x07230203 (got {word0:#010x})"
    );
}
