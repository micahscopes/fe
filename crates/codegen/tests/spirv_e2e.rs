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

// ===========================================================================
// Slice M0 - the Fe-AUTHORED escape-time loop+branch, VALIDATED (mandelbrot
// ladder rung 0). See /workspace/mb2-mandelbrot-plan.md section 5.
//
// The one real unknown before any mandelbrot rendering: does a FE-AUTHORED
// escape-time loop (a bounded while with an in-loop escape branch and an early
// `return` of the iteration count) compile through the Fe compiler to
// naga-VALIDATED SPIR-V at the u32 (browser) word? Every prior escape-time
// module in the tree is hand-built Sonatina IR (the sonatina fork's
// `mandelbrot_snapshot.rs::build_escape_time`, i64, authored in Rust via
// ModuleBuilder) - never Fe source. The straight-line keystones (above) proved
// the wire but exercised NO branches and NO loop, so the structurizer +
// phi-locals had never run on Fe-authored control flow at the u32 word. M0
// retires exactly that unknown. No fork change: unsigned Lt + Add + Mul +
// branches are all already in-envelope.
//
// Honest label (same as the R-val keystone): VALIDATED, NOT executed. No GPU is
// touched here; naga's validator accepting the module is the whole claim.
// ===========================================================================

/// The M0 SSOT fixture: a bounded while-loop, an in-loop escape branch, an early
/// `return i`, u32-only. Lives under `fixtures/spirv/` so the top-level
/// `sonatina_ir` dir-test `*.fe` glob does not mint an incidental EVM-IR snapshot.
const ESCAPE_TIME_U32_SOURCE: &str = include_str!("fixtures/spirv/escape_time_u32.fe");

/// M0: does a Fe-AUTHORED escape-time loop+branch compile to naga-validated
/// SPIR-V? Mirrors `keystone_lowers_to_naga_validated_spirv` (validated, NOT
/// executed - no GPU): Fe source -> wasm-path Sonatina Module -> naga-backed
/// `SpirvBackend`, asserting the SPIR-V magic and a WGSL side artifact. Adds the
/// browser-profile gate (u32 word, no 64-bit tokens, wgsl-in reparse,
/// `Capabilities::default()`), so a green here earns "browser-viable validated
/// SPIR-V for a Fe-authored escape-time loop+branch".
#[test]
fn escape_time_u32_lowers_to_naga_validated_spirv() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///escape_time_u32.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(ESCAPE_TIME_U32_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    // Same seam as the keystone: reuse the wasm-path Sonatina Module (no SPIR-V
    // lowering port) and hand it to the naga-backed SpirvBackend. A failure here
    // means the Fe-authored loop+branch did NOT survive MIR/Sonatina/structurizer
    // or that naga rejected the structurized module.
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("escape-time kernel should build a wasm runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv(&db, &package).expect(
        "Fe-authored escape-time loop+branch should compile to naga-validated SPIR-V unchanged",
    );

    assert!(
        !artifact.words.is_empty(),
        "SPIR-V word stream must be non-empty"
    );
    assert_eq!(
        artifact.words[0], SPIRV_MAGIC,
        "words[0] must be the SPIR-V magic 0x07230203 (got {:#010x})",
        artifact.words[0]
    );

    // Browser profile: the u32 kernel must lower to a Uint word (not Sint/i64),
    // and its WGSL must validate with NO SHADER_INT64. This is the M0 claim.
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the u32 escape-time kernel must lower to a Uint word (WordKind::U32), not Sint/i64"
    );
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit a WGSL side artifact for the escape-time kernel");
    assert_browser_profile_wgsl(wgsl);
    eprintln!(
        "M0: Fe-authored escape-time loop+branch -> naga-validated browser-profile SPIR-V \
         (validated, NOT executed)"
    );
}

// ===========================================================================
// Slice S2 - the EXECUTED keystone (rung R-lava).
//
// R-val (above) proves the Fe-produced SPIR-V is a structurally valid compute
// module. This turns *validated* into *executed*: ONE Fe function compiled by
// the Fe compiler to three backends and run on three independent real runtimes
// (revm / wasmtime / lavapipe), all returning the same pinned constant.
//
// The honest headline (rung R-lava): "One Fe function, compiled by the Fe
// compiler to EVM/wasm/SPIR-V, executed on revm/wasmtime/lavapipe, all ==
// 186898420806." Nuance stated plainly: the EVM Module (Evm ISA) and the
// wasm/SPIR-V Module (Wasm32 ISA) are different lowerings of the *same* Fe
// kernel source; wasm and SPIR-V share one Module. The kernel body is
// byte-for-byte identical across all three legs (it is `KEYSTONE_SOURCE`
// prepended unchanged; the EVM leg adds only a trivial recv arm that calls it).
// ===========================================================================

/// The hand-verified keystone constant: `1 -> 20 -> 650 -> 432306 ->
/// 186898420806`, every intermediate < 2^64 so revm-checked, wasm-wrap and
/// SPIR-V-wrap arithmetic all agree bit-for-bit (no overflow on any backend).
const KEYSTONE_EXPECTED: u64 = 186_898_420_806;

/// The EVM leg's contract shim: a trivial `recv` arm that returns the UNCHANGED
/// kernel's value as a `u256`, giving `poseidon_sigma()` a contract shape revm
/// can call. This is appended to `KEYSTONE_SOURCE` (which is prepended
/// byte-for-byte identical to the wasm/SPIR-V legs), so the kernel body is the
/// same Fe function on all three backends.
const KEYSTONE_EVM_WRAPPER: &str = "\
use std::abi::sol\n\
\n\
msg PoseidonMsg {\n\
\x20   #[selector = sol(\"run()\")]\n\
\x20   Run -> u256,\n\
}\n\
\n\
pub contract PoseidonExec {\n\
\x20   recv PoseidonMsg {\n\
\x20       Run -> u256 {\n\
\x20           poseidon_sigma() as u256\n\
\x20       }\n\
\x20   }\n\
}\n";

/// Compile the keystone kernel to wasm through `BackendKind::Wasm`
/// (`compile_runtime_package_wasm` -> WAFFLE).
fn compile_keystone_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///spirv_keystone_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KEYSTONE_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("keystone should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// Execute the keystone wasm under wasmtime and read back `poseidon_sigma()`.
fn run_wasm_poseidon(bytes: &[u8]) -> u64 {
    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "poseidon_sigma")
        .expect("`poseidon_sigma` export should exist");
    f.call(&mut store, ()).expect("poseidon_sigma() should run") as u64
}

/// Execute the keystone's EVM bytecode under revm via the contract-harness rail.
/// The same Fe kernel is wrapped in a `run()` recv arm; the kernel body is
/// unchanged (`KEYSTONE_SOURCE` prepended verbatim).
fn run_evm_poseidon() -> u64 {
    use fe_contract_harness::{ExecutionOptions, FeContractHarness, bytes_to_u256};

    let source = format!("{KEYSTONE_SOURCE}\n{KEYSTONE_EVM_WRAPPER}");
    let harness = FeContractHarness::compile("PoseidonExec", &source)
        .expect("keystone EVM contract should compile");
    let mut instance = harness
        .deploy_with_init()
        .expect("keystone EVM contract should deploy under revm");
    let result = instance
        .call_function("run()", &[], ExecutionOptions::default())
        .expect("run() should execute under revm");
    let value = bytes_to_u256(&result.return_data).expect("run() should return one u256 word");
    // The keystone constant is < 2^64, so the low limb is the exact value.
    value.as_limbs()[0]
}

/// Execute the keystone's SPIR-V on lavapipe (software Vulkan) via wgpu, mirror
/// of Sonatina's `mandelbrot_snapshot_spirv_gpu`.
///
/// ANTI-FUDGE (the load-bearing honesty guard): a missing GPU adapter or device
/// is a **hard failure**, never a silent skip, so "executed on GPU" can never be
/// printed without the GPU actually running the shader. The only escape hatch is
/// an explicit `MB2_ALLOW_GPU_SKIP` env flag (for a genuinely GPU-less CI host);
/// with it set, the leg returns `None` and the caller downgrades the rung
/// honestly. FACT 1 establishes lavapipe is present in-sandbox, so the default
/// (unset) path executes.
///
/// Returns `Some(value)` when the GPU ran the shader, `None` only when skipped
/// under `MB2_ALLOW_GPU_SKIP`.
fn run_spirv_poseidon_on_lavapipe() -> Option<u64> {
    // Fe -> the wasm-path Sonatina Module -> naga-backed SpirvBackend -> WGSL.
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///spirv_keystone_gpu.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KEYSTONE_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("keystone should build a wasm runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv(&db, &package)
        .expect("keystone should compile Fe -> naga-validated SPIR-V");
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for GPU execution");

    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();

    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(
        &wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            ..Default::default()
        },
    )) {
        Ok(a) => a,
        Err(e) => {
            if allow_skip {
                eprintln!("  SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): no Vulkan adapter: {e:?}");
                return None;
            }
            panic!(
                "SPIR-V leg: no GPU/Vulkan adapter available ({e:?}). The keystone \
                 requires lavapipe to EXECUTE (rung R-lava); a missing device is a \
                 hard failure, not a skip. Set VK_ICD_FILENAMES / LD_LIBRARY_PATH / \
                 WGPU_BACKEND=vulkan for lavapipe, or MB2_ALLOW_GPU_SKIP to downgrade \
                 the rung on a genuinely GPU-less host."
            );
        }
    };

    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::SHADER_INT64,
            ..Default::default()
        },
    )) {
        Ok(dq) => dq,
        Err(e) => {
            if allow_skip {
                eprintln!("  SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): no SHADER_INT64: {e:?}");
                return None;
            }
            panic!(
                "SPIR-V leg: adapter has no SHADER_INT64 support ({e:?}). The keystone \
                 executes i64 in-sandbox; this is a hard failure, not a skip."
            );
        }
    };

    eprintln!("  SPIR-V leg GPU adapter: {}", adapter.get_info().name);

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("poseidon_sigma"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });

    // The scalar (non-ObjAlloc) SPIR-V shape emits a single-value `Output` struct
    // at @group(0)@binding(0) and an (unused, param-count-derived) `Input` at
    // binding 1. Declare an EXPLICIT layout for both bindings so the pipeline is
    // deterministic regardless of how wgpu reflects the unused input global, then
    // bind a real output buffer and a dummy input buffer.
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("poseidon_output"),
        size: 8,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("poseidon_input_unused"),
        size: 8,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("poseidon_staging"),
        size: 8,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("poseidon_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("poseidon_pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("poseidon_pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("poseidon_bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: output_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output_buf, 0, &staging_buf, 0, 8);
    queue.submit(Some(encoder.finish()));

    let slice = staging_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).expect("map_async callback channel should be open");
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    });
    rx.recv()
        .expect("map_async callback should fire")
        .expect("staging buffer should map for read");
    let data = slice.get_mapped_range();
    let gpu_result = u64::from_le_bytes(data[0..8].try_into().expect("8 bytes read back"));
    drop(data);
    staging_buf.unmap();

    Some(gpu_result)
}

/// R-lava, the keystone: ONE Fe function compiled by the Fe compiler to
/// {EVM, wasm, SPIR-V}, EXECUTED on {revm, wasmtime, lavapipe}, all returning
/// the same pinned constant 186898420806.
#[test]
fn keystone_executes_equal_on_evm_wasm_and_spirv() {
    // wasm leg: Fe -> wasm (BackendKind::Wasm) -> executed under wasmtime.
    let wasm_bytes = compile_keystone_to_wasm();
    let wasm_result = run_wasm_poseidon(&wasm_bytes);

    // EVM leg: Fe -> EVM bytecode (BackendKind::Sonatina) -> executed under revm.
    let evm_result = run_evm_poseidon();

    // SPIR-V leg: Fe -> SPIR-V -> executed on lavapipe (software Vulkan) via wgpu.
    let spirv_opt = run_spirv_poseidon_on_lavapipe();

    eprintln!("keystone executed values (all must be {KEYSTONE_EXPECTED}):");
    eprintln!("  EVM    (revm)     = {evm_result}");
    eprintln!("  wasm   (wasmtime) = {wasm_result}");
    match spirv_opt {
        Some(v) => eprintln!("  SPIR-V (lavapipe) = {v}"),
        None => eprintln!("  SPIR-V (lavapipe) = SKIPPED (MB2_ALLOW_GPU_SKIP)"),
    }

    // The two runtime-independent legs always execute and must equal the pin.
    assert_eq!(
        evm_result, KEYSTONE_EXPECTED,
        "Fe -> EVM executed under revm must be {KEYSTONE_EXPECTED}"
    );
    assert_eq!(
        wasm_result, KEYSTONE_EXPECTED,
        "Fe -> wasm executed under wasmtime must be {KEYSTONE_EXPECTED}"
    );
    assert_eq!(
        evm_result, wasm_result,
        "the EVM and wasm executions of the same Fe kernel must agree"
    );

    // The GPU leg: when it ran (the default, hard-failing path), it is the
    // headline. It is only ever `None` under the explicit MB2_ALLOW_GPU_SKIP
    // escape hatch, which downgrades the rung honestly (R-lava -> R-val).
    match spirv_opt {
        Some(spirv_result) => {
            assert_eq!(
                spirv_result, KEYSTONE_EXPECTED,
                "Fe -> SPIR-V executed on lavapipe must be {KEYSTONE_EXPECTED}"
            );
            assert_eq!(
                wasm_result, spirv_result,
                "the wasm and SPIR-V executions of the same Fe kernel must agree"
            );
            eprintln!(
                "R-lava: one Fe function, executed-equal on revm / wasmtime / lavapipe = \
                 {KEYSTONE_EXPECTED}"
            );
        }
        None => {
            eprintln!(
                "R-val only: SPIR-V validated but NOT executed (GPU skipped via \
                 MB2_ALLOW_GPU_SKIP). The keystone's executed-GPU claim is NOT earned \
                 on this run."
            );
        }
    }
}

// ===========================================================================
// Slice B2 - the u32 BROWSER-PROFILE keystone (rung R-lava, browser-viable).
//
// The i64 keystone above proves the cross-backend wire, but i64 shaders cannot
// run in a browser: WebGPU/WGSL has no 64-bit integers. This slice proves the
// SAME machinery executes a u32-only kernel that IS browser-viable: it lowers
// to a naga `Uint` scalar (not `Sint`), the emitted WGSL passes the
// browser-shaped capability set (`Capabilities::default()`, NO SHADER_INT64 -
// note sonatina itself validates with `Capabilities::all()`, so this is a
// strictly stronger independent gate), and the lavapipe leg EXECUTES it after
// requesting the device with NO required features (the exact feature set a
// WebGPU browser exposes; dropping SHADER_INT64 is what makes this the browser
// proof).
//
// The pin, 4261282562, was re-derived independently by an oracle before being
// written here (never trust the doc's arithmetic): 1 -> 14 -> 210 -> 251 ->
// 63252 -> 65278 -> 4261282562. Every intermediate is < 2^32; the result is
// > 2^31 (top bit set), so any signed-i32 mishandling surfaces as a wrong
// (negative) value; and 65278^2 = 4261217284 uses ~99.2% of the u32 range, so a
// 16-bit-truncated or partially-widened multiply cannot silently pass.
// ===========================================================================

/// The single SSOT fixture: `include_str!`-ed here and (later) by the page
/// generator, so the tested source and the shipped source are byte-identical by
/// construction. It lives under `fixtures/spirv/` (not top-level `fixtures/`) so
/// the `sonatina_ir` dir-test's `*.fe` glob (top-level only) does not pick it up
/// and mint an incidental EVM-IR snapshot; the byte-identity gate stays 128/0.
const KEYSTONE_U32_SOURCE: &str = include_str!("fixtures/spirv/poseidon_sigma_u32.fe");

/// The independently oracle-verified pin. `1 -> 14 -> 210 -> 251 -> 63252 ->
/// 65278 -> 4261282562`, all intermediates < 2^32, no wrap on any backend.
const KEYSTONE_U32_EXPECTED: u32 = 4_261_282_562;

/// The EVM leg's contract shim: a trivial `run()` recv arm returning the
/// UNCHANGED kernel's u32 value widened to `u256`. Appended to the byte-identical
/// `KEYSTONE_U32_SOURCE`, so the kernel body is the same Fe function on all three
/// backends (the EVM leg adds only the recv arm).
const KEYSTONE_U32_EVM_WRAPPER: &str = "\
use std::abi::sol\n\
\n\
msg PoseidonU32Msg {\n\
\x20   #[selector = sol(\"run()\")]\n\
\x20   Run -> u256,\n\
}\n\
\n\
pub contract PoseidonU32Exec {\n\
\x20   recv PoseidonU32Msg {\n\
\x20       Run -> u256 {\n\
\x20           poseidon_sigma_u32() as u256\n\
\x20       }\n\
\x20   }\n\
}\n";

/// Compile the u32 keystone to wasm through `BackendKind::Wasm`.
fn compile_keystone_u32_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///poseidon_sigma_u32_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KEYSTONE_U32_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("u32 keystone should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// Execute the u32 keystone wasm under wasmtime. Fe `u32` lowers to wasm `i32`,
/// so the export returns `i32`; reinterpret as `u32` (the pin > 2^31 comes back
/// as a negative `i32`, which `as u32` restores exactly).
fn run_wasm_poseidon_u32(bytes: &[u8]) -> u32 {
    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "poseidon_sigma_u32")
        .expect("`poseidon_sigma_u32` export should exist");
    f.call(&mut store, ())
        .expect("poseidon_sigma_u32() should run") as u32
}

/// Execute the u32 keystone's EVM bytecode under revm via the contract harness.
fn run_evm_poseidon_u32() -> u32 {
    use fe_contract_harness::{ExecutionOptions, FeContractHarness, bytes_to_u256};

    let source = format!("{KEYSTONE_U32_SOURCE}\n{KEYSTONE_U32_EVM_WRAPPER}");
    let harness = FeContractHarness::compile("PoseidonU32Exec", &source)
        .expect("u32 keystone EVM contract should compile");
    let mut instance = harness
        .deploy_with_init()
        .expect("u32 keystone EVM contract should deploy under revm");
    let result = instance
        .call_function("run()", &[], ExecutionOptions::default())
        .expect("run() should execute under revm");
    let value = bytes_to_u256(&result.return_data).expect("run() should return one u256 word");
    // The pin is < 2^32, so the low limb, truncated to u32, is the exact value.
    value.as_limbs()[0] as u32
}

/// Compile the u32 keystone Fe -> the wasm-path Sonatina Module -> naga-backed
/// `SpirvBackend`, returning the full artifact (words + WGSL + compiler-stated
/// layout).
fn compile_keystone_u32_to_spirv() -> sonatina_codegen::isa::spirv::SpirvArtifact {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///poseidon_sigma_u32_gpu.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KEYSTONE_U32_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("u32 keystone should build a wasm runtime package");
    fe_codegen::compile_runtime_package_spirv(&db, &package)
        .expect("u32 keystone should compile Fe -> naga-validated SPIR-V")
}

/// The browser-profile WGSL gate (static, GPU-free). Proves the emitted WGSL is
/// browser-viable independently of any GPU: (1) it carries no 64-bit scalar
/// token, (2) naga's `wgsl-in` front end round-trips it, and (3) it validates
/// under `Capabilities::default()` - the browser-shaped set with NO SHADER_INT64.
fn assert_browser_profile_wgsl(wgsl: &str) {
    // (1) No 64-bit integer scalar tokens: browsers have no i64/u64.
    for tok in ["i64", "u64"] {
        assert!(
            !wgsl.contains(tok),
            "browser-profile WGSL must contain no `{tok}` scalar token; found one in:\n{wgsl}"
        );
    }
    // Positive check: the u32 word must actually appear (guards against an empty
    // or degenerate emit passing the negative token scan vacuously).
    assert!(
        wgsl.contains("u32"),
        "u32 keystone WGSL should use the `u32` scalar; got:\n{wgsl}"
    );

    // (2) Reparse with the naga `wgsl-in` front end.
    let reparsed = naga::front::wgsl::parse_str(wgsl)
        .unwrap_or_else(|e| panic!("naga wgsl-in should reparse the emitted WGSL: {e:?}\n{wgsl}"));

    // (3) Re-validate with the BROWSER capability set. sonatina validates with
    // `Capabilities::all()` internally; this default set excludes SHADER_INT64,
    // so acceptance here is the browser-profile proof, not a tautology.
    let caps = naga::valid::Capabilities::default();
    assert!(
        !caps.contains(naga::valid::Capabilities::SHADER_INT64),
        "the browser capability set must exclude SHADER_INT64"
    );
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
        .validate(&reparsed)
        .unwrap_or_else(|e| {
            panic!("browser-profile validation (no SHADER_INT64) should accept the u32 WGSL: {e:?}")
        });
}

/// Execute the u32 keystone's WGSL on lavapipe (software Vulkan) via wgpu,
/// requesting the device with **NO required features** - the browser profile.
///
/// This is the load-bearing browser proof: a WebGPU browser exposes no
/// SHADER_INT64, so requesting an empty feature set here mirrors exactly what
/// the kernel will face in Chrome. A u32-only kernel must execute on that
/// feature set. ANTI-FUDGE (verbatim from S2): a missing adapter/device is a
/// HARD FAILURE, never a silent skip; the only escape is `MB2_ALLOW_GPU_SKIP`,
/// which downgrades the rung honestly.
///
/// Returns `Some(value)` when the GPU ran the shader, `None` only under skip.
fn run_wgsl_u32_on_lavapipe(wgsl: &str) -> Option<u32> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();

    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(
        &wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            ..Default::default()
        },
    )) {
        Ok(a) => a,
        Err(e) => {
            if allow_skip {
                eprintln!("  u32 SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): no Vulkan adapter: {e:?}");
                return None;
            }
            panic!(
                "u32 SPIR-V leg: no GPU/Vulkan adapter available ({e:?}). The browser-profile \
                 keystone requires lavapipe to EXECUTE; a missing device is a hard failure, not \
                 a skip. Set VK_ICD_FILENAMES / LD_LIBRARY_PATH / WGPU_BACKEND=vulkan for \
                 lavapipe, or MB2_ALLOW_GPU_SKIP to downgrade the rung on a genuinely GPU-less \
                 host."
            );
        }
    };

    // BROWSER PROFILE: NO required features. Dropping SHADER_INT64 is precisely
    // what a WebGPU browser offers; a real failure here means the kernel is NOT
    // browser-viable, which is a STOP condition, not a skip.
    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        },
    )) {
        Ok(dq) => dq,
        Err(e) => {
            if allow_skip {
                eprintln!(
                    "  u32 SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {e:?}"
                );
                return None;
            }
            panic!(
                "u32 SPIR-V leg: browser-profile device request (NO required features) failed \
                 ({e:?}). This is a hard failure, not a skip."
            );
        }
    };

    eprintln!(
        "  u32 SPIR-V leg GPU adapter (BROWSER PROFILE, no required features): {}",
        adapter.get_info().name
    );

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("poseidon_sigma_u32"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });

    // Same scalar SPIR-V shape as S2: a single-value `Output` struct at
    // @group(0)@binding(0) plus an (unused, param-count-derived) `Input` at
    // binding 1. Declare an EXPLICIT layout for both bindings. Buffers are 4
    // bytes (a u32), the browser-profile delta from S2's 8-byte i64 buffers.
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("poseidon_u32_output"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("poseidon_u32_input_unused"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("poseidon_u32_staging"),
        size: 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("poseidon_u32_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("poseidon_u32_pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("poseidon_u32_pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("poseidon_u32_bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: output_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output_buf, 0, &staging_buf, 0, 4);
    queue.submit(Some(encoder.finish()));

    let slice = staging_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).expect("map_async callback channel should be open");
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    });
    rx.recv()
        .expect("map_async callback should fire")
        .expect("staging buffer should map for read");
    let data = slice.get_mapped_range();
    let gpu_result = u32::from_le_bytes(data[0..4].try_into().expect("4 bytes read back"));
    drop(data);
    staging_buf.unmap();

    Some(gpu_result)
}

/// B2, the browser-profile keystone: ONE u32 Fe function compiled by the Fe
/// compiler to {EVM, wasm, SPIR-V}, EXECUTED on {revm, wasmtime, lavapipe with
/// NO required features}, all returning the browser-viable pin 4261282562. The
/// SPIR-V leg lowers to a naga `Uint` scalar and its WGSL passes the browser
/// capability set. This is the in-sandbox proof that the kernel is browser-ready.
#[test]
fn u32_keystone_executes_equal_browser_profile() {
    // --- Static browser-profile facts (GPU-free), asserted before execution. ---
    let artifact = compile_keystone_u32_to_spirv();

    // The u32 kernel must lower to a `Uint` word (NOT `Sint`): this is the
    // content-derived-scalar guarantee (Fe u32 -> Sonatina I32 -> naga Uint/4).
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the u32 kernel must lower to a Uint word (WordKind::U32), not Sint/i64"
    );
    assert_eq!(
        artifact
            .layout
            .result
            .expect("scalar keystone must state a single-slot result (Grid mode has none)")
            .width,
        4,
        "the u32 result must read back as 4 bytes"
    );

    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the u32 kernel");
    assert_browser_profile_wgsl(wgsl);
    eprintln!(
        "  u32 WGSL passed the browser profile: no 64-bit tokens, wgsl-in reparse OK, \
         validated with Capabilities::default() (no SHADER_INT64)"
    );

    // --- Executed legs. ---
    // wasm leg: Fe -> wasm (BackendKind::Wasm) -> executed under wasmtime.
    let wasm_bytes = compile_keystone_u32_to_wasm();
    let wasm_result = run_wasm_poseidon_u32(&wasm_bytes);

    // EVM leg: Fe -> EVM bytecode (BackendKind::Sonatina) -> executed under revm.
    let evm_result = run_evm_poseidon_u32();

    // SPIR-V leg: Fe -> SPIR-V -> executed on lavapipe with NO required features.
    let spirv_opt = run_wgsl_u32_on_lavapipe(wgsl);

    eprintln!("u32 keystone executed values (all must be {KEYSTONE_U32_EXPECTED}):");
    eprintln!("  EVM    (revm)                    = {evm_result}");
    eprintln!("  wasm   (wasmtime)                = {wasm_result}");
    match spirv_opt {
        Some(v) => eprintln!("  SPIR-V (lavapipe, browser profile) = {v}"),
        None => eprintln!("  SPIR-V (lavapipe) = SKIPPED (MB2_ALLOW_GPU_SKIP)"),
    }

    // The two runtime-independent legs always execute and must equal the pin.
    assert_eq!(
        evm_result, KEYSTONE_U32_EXPECTED,
        "Fe -> EVM executed under revm must be {KEYSTONE_U32_EXPECTED}"
    );
    assert_eq!(
        wasm_result, KEYSTONE_U32_EXPECTED,
        "Fe -> wasm executed under wasmtime must be {KEYSTONE_U32_EXPECTED}"
    );
    assert_eq!(
        evm_result, wasm_result,
        "the EVM and wasm executions of the same u32 Fe kernel must agree"
    );

    // The GPU leg: when it ran (the default, hard-failing path), it is the
    // headline browser-profile proof. `None` only under MB2_ALLOW_GPU_SKIP.
    match spirv_opt {
        Some(spirv_result) => {
            assert_eq!(
                spirv_result, KEYSTONE_U32_EXPECTED,
                "Fe -> SPIR-V executed on lavapipe (browser profile) must be \
                 {KEYSTONE_U32_EXPECTED}"
            );
            assert_eq!(
                wasm_result, spirv_result,
                "the wasm and SPIR-V executions of the same u32 Fe kernel must agree"
            );
            eprintln!(
                "B2: one u32 Fe function, executed-equal on revm / wasmtime / lavapipe \
                 (browser profile, no SHADER_INT64) = {KEYSTONE_U32_EXPECTED}; browser-viable."
            );
        }
        None => {
            eprintln!(
                "R-val only: u32 SPIR-V validated (browser profile) but NOT executed (GPU \
                 skipped via MB2_ALLOW_GPU_SKIP). The browser-execution claim is NOT earned \
                 on this run."
            );
        }
    }
}

// ===========================================================================
// M1b - the first Fe GRID kernel (mandelbrot ladder rung 1).
//
// `grid_gradient_u32(px, py) = px + py * 1024` is an ordinary Fe function. Two
// delivery mechanisms, one function: the wasm leg CALLS it per pixel with
// explicit `(px, py)`; the SPIR-V grid leg dispatches it, gid.xy arriving as
// args 0,1 (driver-declared Grid envelope, layout-stated, no in-band marker).
// The honest headline is the same-Fe-function cross-backend per-pixel equality
// below: every one of 4096 GPU pixels equals BOTH the in-test oracle
// `x + 1024*y` AND the wasmtime execution of the very same Fe function.
// ===========================================================================

/// The single SSOT grid fixture: `include_str!`-ed here (and, later, by the page
/// generator) so the tested source and the shipped source are byte-identical by
/// construction. Under `fixtures/spirv/` so the top-level `*.fe` dir-test glob
/// does not mint an incidental EVM-IR snapshot.
const GRID_GRADIENT_SOURCE: &str = include_str!("fixtures/spirv/grid_gradient_u32.fe");

/// The e2e dispatch frame: an 8x8 grid of 8x8 workgroups = 64x64 pixels. The
/// value stride (1024) deliberately differs from this row stride (64), so an
/// index-as-value or value-as-index confusion cannot silently pass.
const GRID_W: u32 = 64;
const GRID_H: u32 = 64;

/// The oracle, re-derived in-test and NEVER trusted from the spec: the grid
/// gradient packs the coordinates base-1024, `v = x + 1024 * y`. Injective for
/// any width <= 1024, and asymmetric, so a transpose/flip/stride bug diverges.
fn grid_gradient_oracle(x: u32, y: u32) -> u32 {
    x + 1024 * y
}

/// Compile the grid gradient fixture to wasm through `BackendKind::Wasm`.
fn compile_grid_gradient_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///grid_gradient_u32_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(GRID_GRADIENT_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("grid gradient should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// Execute a `(i32, i32) -> i32` grid wasm export over the whole
/// `width x height` frame, row-major, under wasmtime. Fe `u32` lowers to wasm
/// `i32`, so the export returns `i32`; reinterpret as `u32`. This is the
/// cross-backend oracle the lavapipe grid is compared against pixel-for-pixel
/// (M1 grid gradient with `grid_gradient_u32`; M2 fractal with
/// `mandel_pixel_q12`).
fn wasm_grid_all(bytes: &[u8], width: u32, height: u32, export: &str) -> Vec<u32> {
    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, export)
        .unwrap_or_else(|_| panic!("`{export}` export should exist as (i32, i32) -> i32"));
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let v = f
                .call(&mut store, (x as i32, y as i32))
                .unwrap_or_else(|e| panic!("{export}(px, py) should run: {e:?}")) as u32;
            out.push(v);
        }
    }
    out
}

/// M1b wasm leg (GPU-FREE, runs everywhere): compile the grid kernel via
/// `BackendKind::Wasm`, call it under wasmtime, and assert 16 spread sample
/// pixels equal the in-test oracle `x + 1024*y`. This is the cross-backend
/// anchor: it exercises the very Fe function the grid leg dispatches, so its
/// green is a precondition for the honest same-function equality claim.
#[test]
fn grid_gradient_u32_wasm_leg() {
    let bytes = compile_grid_gradient_to_wasm();
    wasmparser::validate(&bytes).expect("Fe-emitted wasm should be valid");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "grid_gradient_u32")
        .expect("`grid_gradient_u32` export should exist as (i32, i32) -> i32");

    // 16 spread sample pixels on the 64x64 e2e frame: 4 corners, 4 edge
    // midpoints, the center, both diagonals, and a few off-axis extras.
    let samples: [(u32, u32); 16] = [
        (0, 0),
        (63, 0),
        (0, 63),
        (63, 63),
        (31, 0),
        (0, 31),
        (63, 31),
        (31, 63),
        (32, 32),
        (16, 16),
        (48, 48),
        (16, 47),
        (47, 16),
        (10, 50),
        (50, 10),
        (5, 58),
    ];
    for (x, y) in samples {
        let got = f
            .call(&mut store, (x as i32, y as i32))
            .expect("grid_gradient_u32(px, py) should run") as u32;
        let want = grid_gradient_oracle(x, y);
        assert_eq!(
            got, want,
            "wasm grid_gradient_u32({x}, {y}) must equal the oracle x + 1024*y = {want}"
        );
    }
    eprintln!(
        "M1b wasm leg: Fe grid_gradient_u32 -> wasm executed under wasmtime; \
         16 spread samples all == x + 1024*y."
    );
}

/// Execute a grid kernel's WGSL on lavapipe (software Vulkan) via wgpu at the
/// browser profile (NO required features), over a `width x height` frame of 8x8
/// workgroups. Generalized from the M1 grid-gradient harness so M2's 512x512
/// fractal reuses EXACTLY the same execution path (only the frame size and the
/// output buffer grow): the dispatch is `(width/8, height/8, 1)`, the output
/// buffer `width*height*4` bytes, with a 4-byte dummy input and full-grid
/// staging/readback. ANTI-FUDGE (verbatim from B2): a missing adapter/device is
/// a HARD FAILURE, never a silent skip; the only escape is `MB2_ALLOW_GPU_SKIP`.
///
/// `width` and `height` must be multiples of the 8x8 workgroup size (the kernel
/// derives `row_width = num_workgroups.x * wgx`, so a non-multiple frame would
/// dispatch the wrong pixel count). Returns `Some(grid)` (the `width*height`
/// words, row-major) when the GPU ran the shader, `None` only under skip.
fn run_grid_u32_on_lavapipe(
    wgsl: &str,
    width: u32,
    height: u32,
    params: &[u32],
    label: &str,
) -> Option<Vec<u32>> {
    assert!(
        width % 8 == 0 && height % 8 == 0,
        "grid frame {width}x{height} must be a multiple of the 8x8 workgroup size"
    );
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();
    let out_bytes = u64::from(width * height * 4);

    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(
        &wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            ..Default::default()
        },
    )) {
        Ok(a) => a,
        Err(e) => {
            if allow_skip {
                eprintln!(
                    "  grid SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): no Vulkan adapter: {e:?}"
                );
                return None;
            }
            panic!(
                "grid SPIR-V leg: no GPU/Vulkan adapter available ({e:?}). The grid rung \
                 requires lavapipe to EXECUTE; a missing device is a hard failure, not a skip. \
                 Set VK_ICD_FILENAMES / LD_LIBRARY_PATH / WGPU_BACKEND=vulkan for lavapipe, or \
                 MB2_ALLOW_GPU_SKIP to downgrade the rung on a genuinely GPU-less host."
            );
        }
    };

    // BROWSER PROFILE: NO required features (drop SHADER_INT64), exactly what a
    // WebGPU browser offers. A real failure here means the grid kernel is NOT
    // browser-viable, a STOP condition, not a skip.
    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        },
    )) {
        Ok(dq) => dq,
        Err(e) => {
            if allow_skip {
                eprintln!(
                    "  grid SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {e:?}"
                );
                return None;
            }
            panic!(
                "grid SPIR-V leg: browser-profile device request (NO required features) failed \
                 ({e:?}). This is a hard failure, not a skip."
            );
        }
    };

    eprintln!(
        "  grid SPIR-V leg [{label}] GPU adapter (BROWSER PROFILE, no required features): {}",
        adapter.get_info().name
    );

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });

    // Grid output: the full width x height u32 array (one element per pixel).
    // Same two-binding shape as the scalar keystone (Output @binding(0), unused
    // Input @binding(1)), the deltas being the output/staging sizes.
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grid_output"),
        size: out_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    // Broadcast input buffer: grid-kernel args 2.. load from here (member idx-2,
    // named p0..pN by the fork). M1/M2 kernels take no params (`params == &[]`)
    // and never read it, so the 4-byte dummy floor keeps their unused binding
    // valid, exactly as before. A param-carrying kernel (clifford: 4 rotor
    // members, span 16) sizes the buffer to 4 * len and gets the words written
    // before dispatch; COPY_DST is required for `queue.write_buffer`.
    let input_bytes = std::cmp::max(4u64, 4 * params.len() as u64);
    let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grid_input"),
        size: input_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !params.is_empty() {
        let param_bytes: Vec<u8> = params.iter().flat_map(|p| p.to_le_bytes()).collect();
        queue.write_buffer(&input_buf, 0, &param_bytes);
    }
    let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grid_staging"),
        size: out_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("grid_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("grid_pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("grid_pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("grid_bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: output_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        // (width/8) x (height/8) workgroups of 8x8 invocations = the width x
        // height grid. row_width = num_workgroups.x * wgx, derived in the
        // shader, never threaded as a param (64x64 -> dispatch 8x8; 512x512 ->
        // dispatch 64x64).
        pass.dispatch_workgroups(width / 8, height / 8, 1);
    }
    encoder.copy_buffer_to_buffer(&output_buf, 0, &staging_buf, 0, out_bytes);
    queue.submit(Some(encoder.finish()));

    let slice = staging_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).expect("map_async callback channel should be open");
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    });
    rx.recv()
        .expect("map_async callback should fire")
        .expect("staging buffer should map for read");
    let data = slice.get_mapped_range();
    let grid: Vec<u32> = data
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes per u32")))
        .collect();
    drop(data);
    staging_buf.unmap();

    Some(grid)
}

/// M1b headline: the Fe grid kernel, compiled through the driver-declared Grid
/// seam, EXECUTES on lavapipe at the browser profile, and every one of 4096
/// pixels equals BOTH the in-test oracle AND the wasmtime execution of the same
/// Fe function. The same-Fe-function cross-backend per-pixel equality is the
/// honest claim.
#[test]
fn grid_gradient_u32_executes_on_lavapipe_browser_profile() {
    // --- Compile through the Grid driver seam and assert the stated layout. ---
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///grid_gradient_u32_gpu.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(GRID_GRADIENT_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("grid gradient should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_grid(&db, &package, [8, 8, 1])
        .expect("grid gradient should compile Fe -> naga-validated SPIR-V in Grid mode");

    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Grid,
        "the grid driver seam must state LayoutMode::Grid"
    );
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the grid kernel must lower to the u32 word (browser profile)"
    );
    assert_eq!(
        artifact.layout.workgroup_size,
        [8, 8, 1],
        "the layout must record the [8,8,1] workgroup size the driver set"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Grid mode has no single-slot result: the whole output array is the result"
    );
    let output_stride = artifact
        .layout
        .bindings
        .iter()
        .find(|b| b.role == sonatina_codegen::isa::spirv::Role::Output)
        .expect("the grid layout must have an Output binding")
        .stride;
    assert_eq!(
        output_stride, 4,
        "the grid output stride is one u32 word per element (4 bytes)"
    );

    // --- Browser-profile WGSL gate + the grid-specific tokens. ---
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the grid kernel");
    assert_browser_profile_wgsl(wgsl);
    assert!(
        wgsl.contains("global_invocation_id"),
        "grid WGSL must bind global_invocation_id (the per-pixel gid); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("num_workgroups"),
        "grid WGSL must read num_workgroups (row_width = num_workgroups.x * wgx); got:\n{wgsl}"
    );
    eprintln!(
        "  grid WGSL passed the browser profile and carries global_invocation_id + num_workgroups."
    );

    // --- The cross-backend oracle: the same Fe function, executed under wasmtime
    // over the whole 64x64 frame. ---
    let wasm_bytes = compile_grid_gradient_to_wasm();
    let wasm_grid = wasm_grid_all(&wasm_bytes, GRID_W, GRID_H, "grid_gradient_u32");

    // --- The GPU leg: EXECUTE on lavapipe (browser profile) and compare every
    // pixel to BOTH the oracle and the wasmtime leg. ---
    match run_grid_u32_on_lavapipe(wgsl, GRID_W, GRID_H, &[], "grid_gradient_u32") {
        Some(grid) => {
            assert_eq!(
                grid.len(),
                (GRID_W * GRID_H) as usize,
                "grid readback must be 64*64 = 4096 words"
            );
            for y in 0..GRID_H {
                for x in 0..GRID_W {
                    let i = (y * GRID_W + x) as usize;
                    let got = grid[i];
                    let oracle = grid_gradient_oracle(x, y);
                    assert_eq!(
                        got, oracle,
                        "lavapipe grid[{y}*64+{x}] = {got} must equal the oracle x + 1024*y = \
                         {oracle}"
                    );
                    assert_eq!(
                        got, wasm_grid[i],
                        "lavapipe grid[{y}*64+{x}] = {got} must equal the wasmtime leg for the \
                         same (x, y) = {}",
                        wasm_grid[i]
                    );
                }
            }
            eprintln!(
                "grid: all 4096 pixels grid[y*64+x] == x + 1024*y (oracle) AND == the wasmtime \
                 leg (same Fe function, two backends)."
            );
            eprintln!(
                "M1b: Fe grid_gradient_u32 EXECUTED on lavapipe (browser profile); 4096 pixels \
                 cross-backend-equal to wasmtime. Grid mode earns R-lava."
            );
        }
        None => {
            eprintln!(
                "R-val only: grid SPIR-V validated (browser profile) but NOT executed (GPU \
                 skipped via MB2_ALLOW_GPU_SKIP). The grid-execution claim is NOT earned on \
                 this run."
            );
        }
    }
}

// ===========================================================================
// M2 (mandelbrot ladder rung 2): the REAL Q12 fractal compute.
//
// The kernel (`mandelbrot_q12.fe`) is signed Q12 fixed point (1.0 = 4096) in
// i32, an escape-time loop with an in-loop signed continue-compare (`mag <
// 67108864`, Slt) and an arithmetic right shift (`>> 12`, Sar). Those two ops
// are what fork push #2 (M2a) opened in both fork backends and what M2b's
// `wasm_lower.rs` edit opens on the fe wasm path (sign-aware Lt->Slt and a new
// RShift->Sar arm, keyed on the MIR operand class, not the signless sonatina
// type).
//
// M2b's GPU-FREE gate (this section): the wasm leg proves the fractal
// pixel-exact vs an INDEPENDENT oracle across the FULL 512x512 frame with no
// GPU in the loop, and the EVM leg agrees at 5 probe pixels. The lavapipe leg
// (all 262,144 pixels tri-equal) is M2c.
// ===========================================================================

/// The single SSOT fixture: `include_str!`-ed here (and, later, by the page
/// generator) so the tested source and the shipped source are byte-identical by
/// construction. Under `fixtures/spirv/` so the top-level `*.fe` dir-test glob
/// does not mint an incidental EVM-IR snapshot.
const MANDELBROT_Q12_SOURCE: &str = include_str!("fixtures/spirv/mandelbrot_q12.fe");

/// The independent Q12 escape-time oracle, re-derived HERE from the kernel logic
/// (never trusted from the spec), integer-identical to the fixture: `i32`
/// arithmetic, arithmetic `>>` on i32, the same in-kernel constant literals, and
/// the same continue-condition escape convention (`mag >= 4.0` in Q24 returns the
/// iteration count; an exhausted loop returns MAX_ITER = 100).
///
/// The temp ordering is LOAD-BEARING: `nzi` (the new imaginary part) is computed
/// from the OLD `zr`, BEFORE `zr` is reassigned. Reorder it and the two
/// implementations diverge. The overflow proof (spec 2.2, re-checked: every
/// intermediate over the fixed 512x512 view stays < 2^31) means this runs in a
/// debug build with no i32 overflow panic.
fn mandel_oracle_q12(px: i32, py: i32) -> u32 {
    let c_re: i32 = -8192 + px * 24;
    let c_im: i32 = -6144 + py * 24;
    let mut zr: i32 = 0;
    let mut zi: i32 = 0;
    let mut i: u32 = 0;
    while i < 100 {
        let rr: i32 = zr * zr;
        let ii: i32 = zi * zi;
        let mag: i32 = rr + ii;
        if mag < 67_108_864 {
            let t: i32 = rr - ii;
            let nzi: i32 = ((zr * 2) * zi) >> 12; // uses the OLD zr
            zr = (t >> 12) + c_re;
            zi = nzi + c_im;
            i += 1;
        } else {
            return i;
        }
    }
    i // loop exhausted: i == 100
}

/// Compile the Q12 mandelbrot fixture to wasm through `BackendKind::Wasm`.
fn compile_mandelbrot_q12_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandelbrot_q12_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDELBROT_Q12_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("mandelbrot Q12 should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// M2b wasm leg (GPU-FREE, runs everywhere): compile the Q12 fractal kernel via
/// `BackendKind::Wasm`, execute it under wasmtime over the FULL 512x512 grid, and
/// assert every pixel equals the independent `mandel_oracle_q12`. This is the
/// honest scalar-path proof that the signed Q12 fractal (Slt continue-compare +
/// Sar shift, both newly opened on the fe wasm path) computes correctly WITHOUT
/// any GPU. Fe `u32` returns as wasm `i32`; reinterpret via `as u32`.
#[test]
fn mandelbrot_q12_wasm_leg() {
    let bytes = compile_mandelbrot_q12_to_wasm();
    wasmparser::validate(&bytes).expect("Fe-emitted wasm should be valid");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "mandel_pixel_q12")
        .expect("`mandel_pixel_q12` export should exist as (i32, i32) -> i32");

    // The FULL 512x512 frame, row-major, every pixel == the oracle. The escape
    // counts are collected into a set for the histogram recognizability check.
    let mut distinct = std::collections::HashSet::new();
    for py in 0..512i32 {
        for px in 0..512i32 {
            let got = f
                .call(&mut store, (px, py))
                .expect("mandel_pixel_q12(px, py) should run") as u32;
            let want = mandel_oracle_q12(px, py);
            assert_eq!(
                got, want,
                "wasm mandel_pixel_q12({px}, {py}) = {got} must equal the oracle = {want}"
            );
            distinct.insert(want);
        }
    }

    // Recognizability, DERIVED in-test from the oracle (not a baked pixel table):
    //   - (256, 256) maps to c = -0.5 + 0i, deep inside the main cardioid, so the
    //     loop exhausts and returns MAX_ITER = 100 (an interior pixel);
    //   - (0, 0) maps to c = -2.0 - 1.5i, |c| = 2.5 > 2, so it escapes on the very
    //     first iteration and returns 1;
    //   - the escape-time histogram over the frame has >= 10 distinct values (a
    //     flat/degenerate image, e.g. all-interior or all-escape, could not).
    assert_eq!(
        mandel_oracle_q12(256, 256),
        100,
        "center (256,256) -> c=-0.5+0i is interior: the loop exhausts at MAX_ITER=100"
    );
    assert_eq!(
        mandel_oracle_q12(0, 0),
        1,
        "corner (0,0) -> c=-2.0-1.5i (|c|=2.5>2) escapes on the first iteration"
    );
    assert!(
        distinct.len() >= 10,
        "the escape-time histogram must have >= 10 distinct values (got {})",
        distinct.len()
    );

    eprintln!(
        "M2b wasm leg: Fe mandel_pixel_q12 -> wasm executed under wasmtime; ALL 262,144 pixels \
         (512x512) == the independent oracle; {} distinct escape counts (interior=100, \
         fast-escape=1 confirmed).",
        distinct.len()
    );
}

/// The EVM leg's shim: a parameterless `mandel_probe()` free function that packs
/// the Q12 kernel's value at 5 pixels (the 4 corners + the center) base-1000
/// (each pixel is 0..=100, so 1000 is injective and the packing is exact), plus a
/// trivial `run()` recv arm returning that packing as `u256`. Appended to the
/// UNCHANGED `MANDELBROT_Q12_SOURCE`, so the kernel body is byte-identical to the
/// wasm/SPIR-V legs (the EVM leg adds only the probe fn and the recv arm). The
/// `use` after a fn item mirrors the keystone wrapper (Fe items are unordered).
const MANDEL_Q12_EVM_WRAPPER: &str = "\
pub fn mandel_probe() -> u256 {\n\
\x20   let p0: u256 = mandel_pixel_q12(px: 0, py: 0) as u256\n\
\x20   let p1: u256 = mandel_pixel_q12(px: 511, py: 0) as u256\n\
\x20   let p2: u256 = mandel_pixel_q12(px: 0, py: 511) as u256\n\
\x20   let p3: u256 = mandel_pixel_q12(px: 511, py: 511) as u256\n\
\x20   let p4: u256 = mandel_pixel_q12(px: 256, py: 256) as u256\n\
\x20   p0 + p1 * 1000 + p2 * 1000000 + p3 * 1000000000 + p4 * 1000000000000\n\
}\n\
\n\
use std::abi::sol\n\
\n\
msg MandelMsg {\n\
\x20   #[selector = sol(\"run()\")]\n\
\x20   Run -> u256,\n\
}\n\
\n\
pub contract MandelExec {\n\
\x20   recv MandelMsg {\n\
\x20       Run -> u256 {\n\
\x20           mandel_probe()\n\
\x20       }\n\
\x20   }\n\
}\n";

/// M2b EVM leg: the SAME Fe kernel compiled to EVM bytecode (`BackendKind::
/// Sonatina`) and executed under revm, agreeing with the oracle at 5 probe
/// pixels. The signed Q12 ops (Slt/Sar) are native on the EVM path (the mature
/// lowerer already keys compares/shifts on `is_signed_scalar`), so this puts all
/// executed Fe backends on the record for signed Q12. The base-1000 packing is
/// re-derived here from the independent oracle, never copied.
#[test]
fn mandelbrot_q12_evm_spot_check() {
    use fe_contract_harness::{ExecutionOptions, FeContractHarness, bytes_to_u256};

    // The 5 probe pixels, in the SAME order and base-1000 positions as the Fe
    // `mandel_probe()` fn: p0..p4 = corners + center.
    const PROBE_PIXELS: [(i32, i32); 5] =
        [(0, 0), (511, 0), (0, 511), (511, 511), (256, 256)];
    let mut want: u64 = 0;
    let mut scale: u64 = 1;
    for (px, py) in PROBE_PIXELS {
        want += mandel_oracle_q12(px, py) as u64 * scale;
        scale *= 1000;
    }

    let source = format!("{MANDELBROT_Q12_SOURCE}\n{MANDEL_Q12_EVM_WRAPPER}");
    let harness = FeContractHarness::compile("MandelExec", &source)
        .expect("mandelbrot probe EVM contract should compile");
    let mut instance = harness
        .deploy_with_init()
        .expect("mandelbrot probe EVM contract should deploy under revm");
    let result = instance
        .call_function("run()", &[], ExecutionOptions::default())
        .expect("run() should execute under revm");
    let value = bytes_to_u256(&result.return_data).expect("run() should return one u256 word");

    // The base-1000 packing of 5 pixels each <= 100 is < 2^64, so it lives in the
    // low limb; the upper 192 bits must be zero.
    assert!(
        value.as_limbs()[1..].iter().all(|&limb| limb == 0),
        "the base-1000 packing must fit in the low 64 bits (got upper limbs {:?})",
        &value.as_limbs()[1..]
    );
    assert_eq!(
        value.as_limbs()[0],
        want,
        "revm mandel_probe() base-1000 packing must equal the oracle packing = {want}"
    );

    eprintln!(
        "M2b EVM leg: Fe mandel_probe() (5 probe pixels of the signed Q12 kernel) executed \
         under revm; base-1000 packing == the oracle packing {want}."
    );
}

// ===========================================================================
// M2c (the lavapipe leg): the signed Q12 fractal EXECUTES on the GPU.
// ===========================================================================

/// The fixed M2 mandelbrot view (spec 2.1): a 512x512 frame. 512 is a multiple
/// of the 8x8 workgroup, so the grid harness dispatches (512/8, 512/8) = (64,
/// 64, 1) workgroups over 262,144 pixels.
const MANDEL_W: u32 = 512;
const MANDEL_H: u32 = 512;

/// M2c headline (spec 5.2.3): the signed Q12 fractal, compiled through the Grid
/// driver seam, EXECUTES on lavapipe at the browser profile, and every one of
/// 262,144 pixels is TRI-EQUAL: the GPU escape count == the independent
/// `mandel_oracle_q12` == the wasmtime execution of the SAME Fe function. That
/// three-way per-pixel agreement over the whole frame is the honest
/// cross-backend claim, and this is the FIRST EXECUTED u32 escape-time loop on
/// lavapipe (M0 only validated; M1's executed grid kernel was straight-line).
///
/// Honesty deltas over M0 (the checks M0's validate-only test lacked): the WGSL
/// must contain `loop` (the naga structurizer really emitted the escape loop,
/// not a flattened body) and `bitcast<i32>` (the signed Slt/Sar really went
/// through the fork's i32 sign mapping, not a logical-shift shortcut).
/// Hard-fail-not-skip: a missing GPU is a hard failure; the only escape is
/// `MB2_ALLOW_GPU_SKIP` (adapter printed on execute).
#[test]
fn mandelbrot_q12_executes_on_lavapipe_browser_profile() {
    // --- Compile the Q12 fractal through the Grid driver seam. ---
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandelbrot_q12_gpu.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDELBROT_Q12_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("mandelbrot Q12 should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_grid(&db, &package, [8, 8, 1])
        .expect("mandelbrot Q12 should compile Fe -> naga-validated SPIR-V in Grid mode");

    // --- Layout asserts (same schema as M1: Grid, u32 word, no single-slot
    // result, 4-byte output stride). ---
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Grid,
        "the grid driver seam must state LayoutMode::Grid"
    );
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the Q12 fractal must lower to the u32 word (browser profile)"
    );
    assert_eq!(
        artifact.layout.workgroup_size,
        [8, 8, 1],
        "the layout must record the [8,8,1] workgroup size the driver set"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Grid mode has no single-slot result: the whole output array is the result"
    );
    let output_stride = artifact
        .layout
        .bindings
        .iter()
        .find(|b| b.role == sonatina_codegen::isa::spirv::Role::Output)
        .expect("the grid layout must have an Output binding")
        .stride;
    assert_eq!(
        output_stride, 4,
        "the grid output stride is one u32 word per element (4 bytes)"
    );

    // --- Browser-profile WGSL gate + the M2 honesty tokens. ---
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the fractal kernel");
    assert_browser_profile_wgsl(wgsl);
    assert!(
        wgsl.contains("loop"),
        "the fractal WGSL must contain a `loop` (the naga structurizer emitted the escape loop; \
         the honesty check M0's validate-only test lacked); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("bitcast<i32>"),
        "the fractal WGSL must contain `bitcast<i32>` (the signed Slt/Sar really went through the \
         fork's i32 sign mapping, not a logical-shift shortcut); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("global_invocation_id"),
        "grid WGSL must bind global_invocation_id (the per-pixel gid); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("num_workgroups"),
        "grid WGSL must read num_workgroups (row_width = num_workgroups.x * wgx); got:\n{wgsl}"
    );
    eprintln!(
        "  fractal WGSL passed the browser profile and carries loop + bitcast<i32> + \
         global_invocation_id + num_workgroups."
    );

    // --- Cross-backend leg #2: the SAME Fe function under wasmtime over the
    // whole 512x512 frame (the wasm value, one of the three in the tri-equal
    // claim). oracle == wasm is proven exhaustively by `mandelbrot_q12_wasm_leg`;
    // recomputed here so the tri-equal claim lives inside this one test. ---
    let wasm_bytes = compile_mandelbrot_q12_to_wasm();
    let wasm_grid = wasm_grid_all(&wasm_bytes, MANDEL_W, MANDEL_H, "mandel_pixel_q12");

    // --- The GPU leg: EXECUTE the fractal on lavapipe (browser profile,
    // 512x512, dispatch (64,64,1)) and compare every pixel to BOTH the oracle
    // AND the wasmtime leg (tri-equal). ---
    match run_grid_u32_on_lavapipe(wgsl, MANDEL_W, MANDEL_H, &[], "mandel_pixel_q12") {
        Some(grid) => {
            assert_eq!(
                grid.len(),
                (MANDEL_W * MANDEL_H) as usize,
                "grid readback must be 512*512 = 262144 words"
            );
            let mut distinct = std::collections::HashSet::new();
            for y in 0..MANDEL_H {
                for x in 0..MANDEL_W {
                    let idx = (y * MANDEL_W + x) as usize;
                    let got = grid[idx];
                    let oracle = mandel_oracle_q12(x as i32, y as i32);
                    assert_eq!(
                        got, oracle,
                        "lavapipe fractal[{y}*512+{x}] = {got} must equal the oracle escape count \
                         {oracle}"
                    );
                    assert_eq!(
                        got, wasm_grid[idx],
                        "lavapipe fractal[{y}*512+{x}] = {got} must equal the wasmtime leg for the \
                         same (x, y) = {}",
                        wasm_grid[idx]
                    );
                    distinct.insert(got);
                }
            }

            // Recognizability, DERIVED from the executed GPU grid (not baked):
            // the interior center returns MAX_ITER, the fast-escape corner
            // returns 1, and the escape histogram is non-degenerate. A transpose,
            // an all-black, or an all-escape image could not pass all three.
            assert_eq!(
                grid[(256 * MANDEL_W + 256) as usize],
                100,
                "GPU center pixel (256,256) -> c=-0.5+0i is interior: MAX_ITER=100"
            );
            assert_eq!(
                grid[0], 1,
                "GPU corner pixel (0,0) -> c=-2.0-1.5i (|c|=2.5>2) escapes on the first iteration"
            );
            assert!(
                distinct.len() >= 10,
                "the GPU escape-time histogram must have >= 10 distinct values (got {})",
                distinct.len()
            );

            eprintln!(
                "M2c: Fe mandel_pixel_q12 EXECUTED on lavapipe (browser profile, 512x512); ALL \
                 262,144 pixels TRI-EQUAL (lavapipe == oracle == wasmtime); {} distinct escape \
                 counts (interior=100, fast-escape=1 confirmed on the GPU). The signed Q12 \
                 fractal is cross-backend-honest on the GPU. Grid mode earns R-lava for the \
                 fractal.",
                distinct.len()
            );
        }
        None => {
            eprintln!(
                "R-val only: fractal SPIR-V validated (browser profile) but NOT executed (GPU \
                 skipped via MB2_ALLOW_GPU_SKIP). The 262,144-pixel tri-equal claim is NOT earned \
                 on this run."
            );
        }
    }
}

// ===========================================================================
// C1 (clifford ladder rung 1): the Cl(3) ROTOR SANDWICH v' = R v ~R.
//
// The kernel (`clifford_rotor_q12.fe`) is signed Q12 (1.0 = 4096) in i32,
// straight-line (no loop), and stays inside the EXACT M2 op envelope
// (Add/Sub/Mul/Slt/Sar). The rotor R = rc + r12 e12 + r13 e13 + r23 e23
// arrives as four ordinary function args (on the GPU leg they are broadcast
// grid-input members; on this GPU-FREE leg they are just wasmtime typed-func
// args). Each pixel maps to v = x e1 + y e2 + z0 e3 and is conjugated through
// TWO geometric products, carrying the grade-3 trivector intermediate `tw`.
//
// C1's GPU-FREE gate (this section): the wasm leg proves the sandwich
// pixel-exact vs an INDEPENDENT oracle across the FULL 512x512 frame for each
// of four pinned rotors (identity + 180-deg e12 integer-EXACT at every pixel;
// 90-deg e12 + a tilted default), with no GPU in the loop, and the EVM leg
// agrees at 5 probe pixels under the 180-deg rotor. The lavapipe leg is C2.
//
// The blade algebra was re-derived here, not copied from the spec; the three
// hand anchors below are integer identities proven in-test, not comments.
// ===========================================================================

/// The single SSOT fixture, `include_str!`-ed (later also by the page
/// generator) so the tested source and the shipped source are byte-identical.
const CLIFFORD_Q12_SOURCE: &str = include_str!("fixtures/spirv/clifford_rotor_q12.fe");

/// The four pinned rotors (spec 2.3), each `(name, rc, r12, r13, r23)` in Q12
/// two's complement. Re-derived / re-checked on the host:
///   - identity  = (4096,0,0,0): v' == v EXACTLY at every pixel.
///   - e12_180   = (0,4096,0,0) = e12: v' == (-x,-y,z0) EXACTLY; z' comes
///                 ENTIRELY from the trivector path (t3 == 0, sz = r12*tw>>12).
///   - e12_90    = (2896,2896,0,0), 2896 = round(4096*cos45): a quarter turn.
///   - tilted    = (3712,577,1154,1154): theta/2=25deg (rc=round(4096*cos25)
///                 =3712) about the unit bivector (1,2,2)/3 scaled by
///                 sin25*4096=1731 -> (577,1154,1154). All three bivector
///                 components nonzero; norm^2 = 16_775_305 ~ 4096^2 (near-unit,
///                 ~0.01% deficit, host-quantized; the trig lives OUTSIDE the
///                 equality claim, exactly like M2's baked view constants).
const CLIFFORD_ROTORS: [(&str, i32, i32, i32, i32); 4] = [
    ("identity", 4096, 0, 0, 0),
    ("e12_180", 0, 4096, 0, 0),
    ("e12_90", 2896, 2896, 0, 0),
    ("tilted_default", 3712, 577, 1154, 1154),
];

/// The rotor-sandwich half of the independent oracle, re-derived from the
/// Cl(3) blade products (NOT copied from the doc), integer-identical to the
/// fixture: i32 arithmetic, arithmetic `>>` on i32, the SAME literals, the SAME
/// `>> 12` placements. Returns the rotated point `v' = (sx, sy, sz)` in Q12.
///
/// The intermediate ordering is LOAD-BEARING: the second geometric product
/// consumes the TRUNCATED `t1..tw`, so the two `>> 12` layers must sit exactly
/// where the kernel places them. The trivector `tw` is grade-3 and feeds sx/sy/
/// sz; the outgoing e123 coefficient cancels identically in exact algebra
/// (c*tw - p*t3 + q*t2 - r*t1 == 0 over any commutative ring) so it is never
/// computed. Overflow proof (re-checked per pinned rotor): every accumulator
/// stays < 2^31, so this runs panic-free in a debug build.
fn clifford_sandwich_q12(
    px: i32,
    py: i32,
    rc: i32,
    r12: i32,
    r13: i32,
    r23: i32,
) -> (i32, i32, i32) {
    let x: i32 = (px - 256) * 16;
    let y: i32 = (py - 256) * 16;
    let z: i32 = 2048;

    // First product t = R v (grade 1 + grade 3), each accumulator >> 12.
    let t1: i32 = (rc * x + r12 * y + r13 * z) >> 12;
    let t2: i32 = (rc * y - r12 * x + r23 * z) >> 12;
    let t3: i32 = (rc * z - r13 * x - r23 * y) >> 12;
    let tw: i32 = (r12 * z - r13 * y + r23 * x) >> 12; // e123: load-bearing

    // Second product v' = t ~R (reverse negates grade 2), grade-1 output.
    let sx: i32 = (rc * t1 + r12 * t2 + r13 * t3 + r23 * tw) >> 12;
    let sy: i32 = (rc * t2 - r12 * t1 - r13 * tw + r23 * t3) >> 12;
    let sz: i32 = (rc * t3 + r12 * tw - r13 * t1 - r23 * t2) >> 12;
    (sx, sy, sz)
}

/// The checker-sampling half of the oracle: 0.5-unit cells (`>> 11`), sum
/// parity == XOR of the per-axis parities, `parity(n) = n - ((n>>1)*2)` (exact
/// for negatives because Sar floors), two-tone + depth cue, clamped to 0..255
/// with the two var-left Slt compares the kernel uses. Returns i32 (the kernel
/// returns i32 to dodge the wasm Cast surface; the value is in 0..255).
fn clifford_shade_q12(sx: i32, sy: i32, sz: i32) -> i32 {
    let cell: i32 = (sx >> 11) + (sy >> 11) + (sz >> 11);
    let par: i32 = cell - ((cell >> 1) * 2);
    let mut shade: i32 = par * 160 + 48 + (sz >> 7);
    if shade < 0 {
        shade = 0;
    }
    if shade < 256 {
        return shade;
    }
    255
}

/// The full independent Q12 clifford oracle: sandwich then sample. Mirrors
/// `clifford_pixel_q12` op-for-op; the two are integer-identical by
/// construction (that is the point of the twice-written discipline).
fn clifford_oracle_q12(px: i32, py: i32, rc: i32, r12: i32, r13: i32, r23: i32) -> i32 {
    let (sx, sy, sz) = clifford_sandwich_q12(px, py, rc, r12, r13, r23);
    clifford_shade_q12(sx, sy, sz)
}

/// Compile the Q12 clifford fixture to wasm through `BackendKind::Wasm`.
fn compile_clifford_q12_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///clifford_rotor_q12_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(CLIFFORD_Q12_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("clifford Q12 should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// C1 wasm leg (GPU-FREE, runs everywhere): compile the rotor-sandwich kernel
/// via `BackendKind::Wasm`, execute it under wasmtime as a `(i32 x6) -> i32`
/// typed func over the FULL 512x512 grid for EACH of the four pinned rotors,
/// and assert every pixel equals the independent `clifford_oracle_q12`. This is
/// the honest scalar-path proof that the Cl(3) sandwich (two geometric products
/// through the M2 op envelope, broadcast params as plain args) computes
/// correctly WITHOUT any GPU.
#[test]
fn clifford_rotor_q12_wasm_leg() {
    let bytes = compile_clifford_q12_to_wasm();
    wasmparser::validate(&bytes).expect("Fe-emitted clifford wasm should be valid");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut store, "clifford_pixel_q12")
        .expect("`clifford_pixel_q12` export should exist as (i32 x6) -> i32");

    // --- Integer-EXACT hand anchors on the sandwich itself (spec 2.3),
    // asserted over the FULL grid at the ORACLE level: the rotated point, not
    // just the shade. wasm == oracle (proven below) then chains to wasm-exact.
    for py in 0..512i32 {
        for px in 0..512i32 {
            let x = (px - 256) * 16;
            let y = (py - 256) * 16;
            // Identity R=(4096,0,0,0): v' == v EXACTLY.
            assert_eq!(
                clifford_sandwich_q12(px, py, 4096, 0, 0, 0),
                (x, y, 2048),
                "identity rotor must fix v=({x},{y},2048) exactly at ({px},{py})"
            );
            // 180-deg e12 R=(0,4096,0,0): v' == (-x,-y,z0) EXACTLY.
            assert_eq!(
                clifford_sandwich_q12(px, py, 0, 4096, 0, 0),
                (-x, -y, 2048),
                "e12_180 rotor must map v to (-x,-y,z0) exactly at ({px},{py})"
            );
        }
    }
    // The 180-deg z' is recovered ENTIRELY from the grade-3 trivector path:
    // t3 == 0 while tw == z0 (nonzero), and sz = (r12 * tw) >> 12 == 2048. An
    // integer identity, not a comment: prove the trivector is load-bearing.
    {
        let (px, py) = (300, 200);
        let z = 2048i32;
        let (rc, r12) = (0i32, 4096i32);
        let t3 = (rc * z - 0 * ((px - 256) * 16) - 0 * ((py - 256) * 16)) >> 12;
        let tw = (r12 * z - 0 * ((py - 256) * 16) + 0 * ((px - 256) * 16)) >> 12;
        assert_eq!(t3, 0, "e12_180: the grade-1 t3 vanishes");
        assert_eq!(tw, 2048, "e12_180: the grade-3 trivector tw == z0 (nonzero)");
        let sz = (rc * t3 + r12 * tw) >> 12;
        assert_eq!(sz, 2048, "e12_180: z' is reconstructed from the trivector alone");
    }

    // --- The FULL 512x512 frame, every pixel == the oracle, for each rotor.
    for (name, rc, r12, r13, r23) in CLIFFORD_ROTORS {
        let mut distinct = std::collections::BTreeSet::new();
        let mut min_shade = i32::MAX;
        let mut max_shade = i32::MIN;
        for py in 0..512i32 {
            for px in 0..512i32 {
                let got = f
                    .call(&mut store, (px, py, rc, r12, r13, r23))
                    .expect("clifford_pixel_q12 should run");
                let want = clifford_oracle_q12(px, py, rc, r12, r13, r23);
                assert_eq!(
                    got, want,
                    "wasm clifford_pixel_q12({px},{py}; {name}) = {got} must equal oracle = {want}"
                );
                distinct.insert(want);
                min_shade = min_shade.min(want);
                max_shade = max_shade.max(want);
            }
        }
        // Recognizability, DERIVED in-test (not a baked pixel table). Two facts:
        //   (1) BOTH checker tone bands are populated for every rotor: a dark
        //       cell shade well under the light base (48+160=208) and a light
        //       cell shade at or above it. A flat/degenerate image cannot pass.
        //   (2) The shade STRUCTURE distinguishes the rotor class, and it is a
        //       geometric fact, not a baked table: a pure-e12 rotor (identity,
        //       180, 90) rotates only in the e1-e2 plane, so the e3 slab height
        //       z' is FIXED across the frame (t3, tw depend on z alone) -> the
        //       depth cue `sz>>7` is constant -> a flat two-tone checker with
        //       exactly two shades. Only the TILTED rotor (r13,r23 nonzero)
        //       tumbles the slab in 3D, so z' varies and the depth cue spreads
        //       the shades. The exact two-tone values are re-derived here:
        //         par*160 + 48 + (sz>>7); identity/180 have sz=2048 (>>7 = 16)
        //         -> {64, 224}; 90 has sz=2047 (>>7 = 15) -> {63, 223}.
        assert!(
            min_shade < 100 && max_shade >= 208,
            "{name}: both checker tone bands must be populated (min={min_shade}, max={max_shade})"
        );
        match name {
            "identity" | "e12_180" => assert!(
                distinct.iter().copied().eq([64, 224]),
                "{name}: pure-e12 rotor fixes z0 -> flat 2-tone checker {{64,224}}, got {distinct:?}"
            ),
            "e12_90" => assert!(
                distinct.iter().copied().eq([63, 223]),
                "e12_90: z fixed at sz=2047 -> flat 2-tone checker {{63,223}}, got {distinct:?}"
            ),
            "tilted_default" => assert!(
                distinct.len() >= 8,
                "tilted_default: the 3D tumble's depth cue must spread the shades (got {} distinct)",
                distinct.len()
            ),
            _ => unreachable!("unexpected pinned rotor name {name}"),
        }
        eprintln!(
            "C1 wasm leg [{name} = ({rc},{r12},{r13},{r23})]: ALL 262,144 pixels (512x512) == the \
             independent oracle; {} distinct shades (bands {min_shade}..={max_shade}).",
            distinct.len()
        );
    }

    // --- 180 frame == identity frame point-reflected through the center
    // (up to the step-16 slab offset), asserted via the ORACLE relation (not a
    // hand-baked image): under e12_180, v' = (-x,-y,z0), and (-x,-y) is the
    // slab point of pixel (512-px, 512-py) under identity. So the 180 shade at
    // (px,py) equals the identity shade at the reflected pixel, at EVERY pixel.
    for py in [0, 137, 256, 400, 511i32] {
        for px in [0, 91, 256, 373, 511i32] {
            let s180 = clifford_oracle_q12(px, py, 0, 4096, 0, 0);
            let s_id_reflected = clifford_oracle_q12(512 - px, 512 - py, 4096, 0, 0, 0);
            assert_eq!(
                s180, s_id_reflected,
                "e12_180 at ({px},{py}) must equal identity at the reflected ({}, {})",
                512 - px,
                512 - py
            );
        }
    }

    eprintln!(
        "C1 wasm leg: Fe clifford_pixel_q12 -> wasm under wasmtime; ALL 4 pinned rotors \
         pixel-exact vs the oracle over the full 512x512; identity + e12_180 integer-EXACT at \
         every pixel (z' via the trivector), 180 == identity point-reflected."
    );
}

/// The EVM leg's shim: a parameterless `clifford_probe()` free function that
/// packs the kernel's shade at 5 pixels (the 4 corners + the center) under the
/// 180-deg e12 rotor base-1000 (each shade is 0..=255, so 1000 is injective and
/// the packing is exact), plus a trivial `run()` recv arm returning it as
/// `u256`. Appended to the UNCHANGED `CLIFFORD_Q12_SOURCE`, so the kernel body
/// is byte-identical to the wasm/SPIR-V legs; the EVM leg adds only the probe fn
/// and the recv arm.
///
/// The kernel returns i32 (to dodge the wasm/SPIR-V Cast surface), and Fe
/// rightly refuses `i32 as u256` as a non-provably-lossless sign change. Each
/// clamped shade is 0..=255, so we take it into a u8 via `.downcast_unchecked()`
/// (a checked-off narrowing sign change; the runtime EVM path lowers it, per
/// `int_downcast.fe`) and then widen `u8 as u256` (the lossless unsigned widen
/// the mandelbrot probe relied on). All of this is EVM-path-only.
const CLIFFORD_Q12_EVM_WRAPPER: &str = "\
use core::num::IntDowncast\n\
\n\
pub fn clifford_probe() -> u256 {\n\
\x20   let s0: u8 = clifford_pixel_q12(px: 0, py: 0, rc: 0, r12: 4096, r13: 0, r23: 0).downcast_unchecked()\n\
\x20   let s1: u8 = clifford_pixel_q12(px: 511, py: 0, rc: 0, r12: 4096, r13: 0, r23: 0).downcast_unchecked()\n\
\x20   let s2: u8 = clifford_pixel_q12(px: 0, py: 511, rc: 0, r12: 4096, r13: 0, r23: 0).downcast_unchecked()\n\
\x20   let s3: u8 = clifford_pixel_q12(px: 511, py: 511, rc: 0, r12: 4096, r13: 0, r23: 0).downcast_unchecked()\n\
\x20   let s4: u8 = clifford_pixel_q12(px: 256, py: 256, rc: 0, r12: 4096, r13: 0, r23: 0).downcast_unchecked()\n\
\x20   let p0: u256 = s0 as u256\n\
\x20   let p1: u256 = s1 as u256\n\
\x20   let p2: u256 = s2 as u256\n\
\x20   let p3: u256 = s3 as u256\n\
\x20   let p4: u256 = s4 as u256\n\
\x20   p0 + p1 * 1000 + p2 * 1000000 + p3 * 1000000000 + p4 * 1000000000000\n\
}\n\
\n\
use std::abi::sol\n\
\n\
msg CliffordMsg {\n\
\x20   #[selector = sol(\"run()\")]\n\
\x20   Run -> u256,\n\
}\n\
\n\
pub contract CliffordExec {\n\
\x20   recv CliffordMsg {\n\
\x20       Run -> u256 {\n\
\x20           clifford_probe()\n\
\x20       }\n\
\x20   }\n\
}\n";

/// C1 EVM leg: the SAME Fe kernel compiled to EVM bytecode (`BackendKind::
/// Sonatina`) and executed under revm, agreeing with the oracle at 5 probe
/// pixels under the 180-deg e12 rotor (the exact hand anchor). The signed Q12
/// ops (Slt/Sar) are native on the EVM path, so this puts all executed Fe
/// backends on the record for the rotor sandwich. The base-1000 packing is
/// re-derived here from the independent oracle, never copied.
#[test]
fn clifford_rotor_q12_evm_spot_check() {
    use fe_contract_harness::{ExecutionOptions, FeContractHarness, bytes_to_u256};

    // The 5 probe pixels, in the SAME order and base-1000 positions as the Fe
    // `clifford_probe()` fn: p0..p4 = corners + center, all under e12_180.
    const PROBE_PIXELS: [(i32, i32); 5] = [(0, 0), (511, 0), (0, 511), (511, 511), (256, 256)];
    let mut want: u64 = 0;
    let mut scale: u64 = 1;
    for (px, py) in PROBE_PIXELS {
        want += clifford_oracle_q12(px, py, 0, 4096, 0, 0) as u64 * scale;
        scale *= 1000;
    }

    let source = format!("{CLIFFORD_Q12_SOURCE}\n{CLIFFORD_Q12_EVM_WRAPPER}");
    let harness = FeContractHarness::compile("CliffordExec", &source)
        .expect("clifford probe EVM contract should compile");
    let mut instance = harness
        .deploy_with_init()
        .expect("clifford probe EVM contract should deploy under revm");
    let result = instance
        .call_function("run()", &[], ExecutionOptions::default())
        .expect("run() should execute under revm");
    let value = bytes_to_u256(&result.return_data).expect("run() should return one u256 word");

    // 5 shades each <= 255 packed base-1000 is < 2^64, so it lives in the low
    // limb; the upper 192 bits must be zero.
    assert!(
        value.as_limbs()[1..].iter().all(|&limb| limb == 0),
        "the base-1000 packing must fit in the low 64 bits (got upper limbs {:?})",
        &value.as_limbs()[1..]
    );
    assert_eq!(
        value.as_limbs()[0],
        want,
        "revm clifford_probe() base-1000 packing must equal the oracle packing = {want}"
    );

    eprintln!(
        "C1 EVM leg: Fe clifford_probe() (5 probe pixels of the rotor sandwich under e12_180) \
         executed under revm; base-1000 packing == the oracle packing {want}."
    );
}

// ===========================================================================
// C2 (clifford ladder rung 2): the lavapipe leg with GRID BROADCAST PARAMS.
//
// This is the FIRST fe-compiled kernel to go through the grid broadcast-param
// path. The rotor components (rc, r12, r13, r23 = kernel args 2..5) are
// delivered to the GPU via the grid Input struct (members p0..p3, span 16),
// written to the input buffer before dispatch by the generalized
// `run_grid_u32_on_lavapipe` (its new `params` arg). Args 0,1 (px, py) stay
// grid builtins. Both prior grid kernels (grid_gradient, mandelbrot) are 2-arg
// gid-only, so the four-member broadcast input struct is exercised end-to-end
// from Fe source HERE for the first time.
//
// C2's gate: EXECUTE on lavapipe (browser profile, 512x512, dispatch (64,64,1))
// for the pinned rotors and assert ALL 262,144 pixels tri-equal (lavapipe ==
// the wasm leg == the oracle). The layout's input stride 16 is the static proof
// of the four broadcast rotor members. Hard-fail-not-skip; MB2_ALLOW_GPU_SKIP
// only, adapter printed.
// ===========================================================================

/// Run the Fe clifford wasm kernel (the 6-arg typed func) over the FULL 512x512
/// grid for one pinned rotor, returning the per-pixel shade grid (row-major,
/// u32). This is the wasm value in the tri-equal claim: `wasm == oracle` is
/// proven exhaustively by `clifford_rotor_q12_wasm_leg`, and it is recomputed
/// here so the three-way equality lives inside the one executing test.
fn wasm_clifford_grid_all(bytes: &[u8], rc: i32, r12: i32, r13: i32, r23: i32) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut store, "clifford_pixel_q12")
        .expect("`clifford_pixel_q12` export should exist as (i32 x6) -> i32");
    let mut out = Vec::with_capacity((MANDEL_W * MANDEL_H) as usize);
    for py in 0..MANDEL_H as i32 {
        for px in 0..MANDEL_W as i32 {
            let v = f
                .call(&mut store, (px, py, rc, r12, r13, r23))
                .expect("clifford_pixel_q12 should run") as u32;
            out.push(v);
        }
    }
    out
}

/// C2 headline (the FIRST fe grid-broadcast execution): the Q12 rotor sandwich,
/// compiled through the Grid driver seam with FOUR broadcast rotor params,
/// EXECUTES on lavapipe at the browser profile, and every one of 262,144 pixels
/// equals BOTH the independent oracle AND the wasmtime execution of the same Fe
/// function (tri-equal) for each pinned GPU rotor. The rotor params reach the
/// GPU via the Input struct (span 16, members p0..p3), written to the input
/// buffer before dispatch by the generalized harness.
///
/// The name contains "lavapipe", so the nextest serial group filter
/// (`test(lavapipe)`) catches it; the software Vulkan device is single-threaded.
#[test]
fn clifford_rotor_q12_executes_on_lavapipe_browser_profile() {
    // --- Compile the rotor sandwich through the Grid driver seam. ---
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///clifford_rotor_q12_gpu.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(CLIFFORD_Q12_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("clifford Q12 should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_grid(&db, &package, [8, 8, 1])
        .expect("clifford Q12 should compile Fe -> naga-validated SPIR-V in Grid mode");

    // --- Layout asserts: the M1/M2 schema (Grid, u32 word, no single-slot
    // result, 4-byte output stride) PLUS the input stride 16 that is the
    // layout-level proof of the FOUR broadcast rotor members (the C2 crux:
    // rc,r12,r13,r23 at member offsets 0,4,8,12). ---
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Grid,
        "the grid driver seam must state LayoutMode::Grid"
    );
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the Q12 rotor sandwich must lower to the u32 word (browser profile)"
    );
    assert_eq!(
        artifact.layout.workgroup_size,
        [8, 8, 1],
        "the layout must record the [8,8,1] workgroup size the driver set"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Grid mode has no single-slot result: the whole output array is the result"
    );
    let output_stride = artifact
        .layout
        .bindings
        .iter()
        .find(|b| b.role == sonatina_codegen::isa::spirv::Role::Output)
        .expect("the grid layout must have an Output binding")
        .stride;
    assert_eq!(
        output_stride, 4,
        "the grid output stride is one u32 word per element (4 bytes)"
    );
    let input_stride = artifact
        .layout
        .bindings
        .iter()
        .find(|b| b.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("the grid layout must have an Input binding")
        .stride;
    assert_eq!(
        input_stride, 16,
        "the FOUR broadcast rotor members (rc,r12,r13,r23), 4 bytes each, span 16 bytes: \
         the layout-level proof that the fe kernel's args 2..5 became the grid broadcast params"
    );

    // --- Browser-profile WGSL gate + the C2 honesty tokens: signed ops through
    // the fork sign mapping (`bitcast<i32>`), the per-pixel gid + row-width
    // derivation, and the multi-member broadcast load from the Input struct
    // (`input.p0` .. `input.p3`, proving all four rotor members are read). ---
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the rotor-sandwich kernel");
    assert_browser_profile_wgsl(wgsl);
    assert!(
        wgsl.contains("bitcast<i32>"),
        "the sandwich WGSL must contain `bitcast<i32>` (the signed Sar/Slt really went through \
         the fork's i32 sign mapping); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("global_invocation_id"),
        "grid WGSL must bind global_invocation_id (the per-pixel gid); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("num_workgroups"),
        "grid WGSL must read num_workgroups (row_width = num_workgroups.x * wgx); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("input.p0"),
        "the sandwich WGSL must load the first broadcast rotor member `input.p0`; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("input.p3"),
        "the sandwich WGSL must load the FOURTH broadcast rotor member `input.p3` (all four \
         rc,r12,r13,r23 members are read: the multi-member broadcast); got:\n{wgsl}"
    );
    eprintln!(
        "  sandwich WGSL passed the browser profile and carries bitcast<i32> + \
         global_invocation_id + num_workgroups + input.p0..input.p3 (4-member broadcast)."
    );

    // --- The wasm bytecode (rotor-independent: the rotor arrives as args), used
    // to recompute the wasm value in the tri-equal claim per rotor. ---
    let wasm_bytes = compile_clifford_q12_to_wasm();

    // --- The GPU leg: for each pinned rotor, write (rc,r12,r13,r23) to the
    // broadcast input buffer, EXECUTE on lavapipe (512x512), and compare every
    // pixel to BOTH the oracle AND the wasmtime leg (tri-equal). At least
    // identity + e12_180 + one tilted (spec 4.2.3), covering both the exact
    // pure-e12 anchors and the 3D-tumbling tilted default. ---
    const GPU_ROTORS: [(&str, i32, i32, i32, i32); 3] = [
        ("identity", 4096, 0, 0, 0),
        ("e12_180", 0, 4096, 0, 0),
        ("tilted_default", 3712, 577, 1154, 1154),
    ];
    for (name, rc, r12, r13, r23) in GPU_ROTORS {
        let wasm_grid = wasm_clifford_grid_all(&wasm_bytes, rc, r12, r13, r23);
        // The broadcast params, in kernel-arg order: arg2=rc -> p0, arg3=r12 ->
        // p1, arg4=r13 -> p2, arg5=r23 -> p3 (i32 two's complement in the u32
        // word). The generalized harness writes these to the input buffer.
        let params = [rc as u32, r12 as u32, r13 as u32, r23 as u32];
        match run_grid_u32_on_lavapipe(wgsl, MANDEL_W, MANDEL_H, &params, "clifford_pixel_q12") {
            Some(grid) => {
                assert_eq!(
                    grid.len(),
                    (MANDEL_W * MANDEL_H) as usize,
                    "grid readback must be 512*512 = 262144 words"
                );
                let mut distinct = std::collections::BTreeSet::new();
                for y in 0..MANDEL_H {
                    for x in 0..MANDEL_W {
                        let idx = (y * MANDEL_W + x) as usize;
                        let got = grid[idx];
                        let oracle = clifford_oracle_q12(x as i32, y as i32, rc, r12, r13, r23) as u32;
                        assert_eq!(
                            got, oracle,
                            "lavapipe clifford[{y}*512+{x}; {name}] = {got} must equal the oracle \
                             shade {oracle}"
                        );
                        assert_eq!(
                            got, wasm_grid[idx],
                            "lavapipe clifford[{y}*512+{x}; {name}] = {got} must equal the wasmtime \
                             leg for the same pixel = {}",
                            wasm_grid[idx]
                        );
                        distinct.insert(got);
                    }
                }

                // Recognizability, DERIVED from the executed GPU grid (not baked):
                // a pure-e12 rotor (identity, 180) fixes the e3 slab height z', so
                // the depth cue is constant -> a flat two-tone checker {64, 224}.
                // Only the tilted rotor tumbles the slab in 3D, spreading z' and
                // thus the shades. The exact two-tone values are the C1 anchors.
                match name {
                    "identity" | "e12_180" => assert!(
                        distinct.iter().copied().eq([64u32, 224u32]),
                        "{name}: pure-e12 rotor fixes z0 -> flat 2-tone checker {{64,224}} on the \
                         GPU, got {distinct:?}"
                    ),
                    "tilted_default" => assert!(
                        distinct.len() >= 8,
                        "tilted_default: the 3D tumble's depth cue must spread the GPU shades (got \
                         {} distinct)",
                        distinct.len()
                    ),
                    _ => unreachable!("unexpected GPU rotor name {name}"),
                }

                eprintln!(
                    "C2 [{name} = ({rc},{r12},{r13},{r23})]: Fe clifford_pixel_q12 EXECUTED on \
                     lavapipe (browser profile, 512x512) with the rotor as 4 broadcast params; \
                     ALL 262,144 pixels TRI-EQUAL (lavapipe == oracle == wasmtime); {} distinct \
                     shades.",
                    distinct.len()
                );
            }
            None => {
                eprintln!(
                    "R-val only: clifford SPIR-V validated (browser profile) but NOT executed (GPU \
                     skipped via MB2_ALLOW_GPU_SKIP). The 262,144-pixel tri-equal broadcast claim \
                     is NOT earned on this run."
                );
                return;
            }
        }
    }

    eprintln!(
        "C2: the FIRST fe-compiled grid-broadcast kernel EXECUTED on lavapipe. The rotor sandwich \
         went through the four-member broadcast Input struct (span 16) end to end from Fe source; \
         all three pinned GPU rotors are tri-equal at every one of 262,144 pixels. Grid broadcast \
         params earn R-lava."
    );
}

// ===========================================================================
// R1b/R2 (renderer-in-Fe): the mandelbrot escape-time AND its color map as ONE
// Fe FRAGMENT shader, compiled through the driver-declared Render seam into ONE
// SPIR-V module with TWO entry points (a fixed fullscreen-triangle @vertex + a
// @fragment that IS the fractal), and RENDERED byte-exact on lavapipe vs an
// independent oracle AND the wasm leg (tri-equal for the RENDERED image).
//
// The escape-time body is BYTE-IDENTICAL to mandel_pixel_q12 (signed Q12, the
// same fixed 512x512 view); the addition is the M4 integer color map folded into
// the SAME loop as a loop-carried color phi (spec 4.2). The one new op is `Shr`
// (logical `>>` on the u32 color ramp), opened fail-closed-then-u32 by fork
// push #3 alongside the Render prologue/epilogue.
// ===========================================================================

/// The SSOT render fixture: `include_str!`-ed here so the tested source and the
/// (later) shipped source are byte-identical by construction.
const MANDEL_FRAG_RGBA_SOURCE: &str = include_str!("fixtures/spirv/mandel_frag_rgba.fe");

/// Focused f32 Render regression: coordinate conversion, three broadcast f32s,
/// float arithmetic/comparison, a loop-carried float, early return, and the
/// final f32 -> i32 -> packed-u32 color path in one small kernel.
const F32_RENDER_PROBE_SOURCE: &str = include_str!("fixtures/spirv/f32_render_probe.fe");
const MVT2_F32_RENDER_SOURCE: &str = include_str!("fixtures/spirv/mvt2_f32_render.fe");
const MVT5_F32_RENDER_SOURCE: &str = include_str!("fixtures/spirv/mvt5_f32_render.fe");

/// D1's fixed-versor, scalarized Cl(4,1) inversion distance-estimator.
const CGA_INVERSION_DE_RENDER_SOURCE: &str =
    include_str!("fixtures/spirv/cga_inversion_de_render.fe");

const CONDITIONAL_F32_SELECT_SOURCE: &str =
    include_str!("fixtures/spirv/conditional_f32_select.fe");

const CONDITIONAL_F32_LOOP_CARRY_SOURCE: &str =
    include_str!("fixtures/spirv/conditional_f32_loop_carry.fe");

#[test]
fn conditional_f32_select_materializes_typed_spirv_result_slot() {
    const W: u32 = 8;
    const H: u32 = 1;
    const LOW: f32 = 25.0;
    const HIGH: f32 = 75.0;

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///conditional_f32_select.fe").expect("test URL should parse");
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(CONDITIONAL_F32_SELECT_SOURCE.to_string()),
    );
    let file = db.workspace().get(&db, &url).expect("fixture should load");
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("conditional f32 selector should build a runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("conditional f32 selector should materialize a typed SPIR-V result slot");
    let input = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("low/high require a broadcast Input binding");
    assert_eq!((input.span, input.stride), (8, 8));
    assert!(input
        .members
        .iter()
        .all(|member| member.scalar == sonatina_codegen::isa::spirv::SpirvScalarKind::F32));

    let wgsl = artifact.wgsl.as_ref().expect("Render compilation emits WGSL");
    assert_browser_profile_wgsl(wgsl);
    let mut input_bytes = Vec::with_capacity(8);
    input_bytes.extend_from_slice(&LOW.to_bits().to_le_bytes());
    input_bytes.extend_from_slice(&HIGH.to_bits().to_le_bytes());
    let rgba = run_render_rgba8_on_lavapipe(wgsl, W, H, &input_bytes)
        .expect("conditional f32 result-slot probe requires browser-profile execution");
    for x in 0..W {
        let shade = if x < 4 { LOW as u8 } else { HIGH as u8 };
        let offset = (x * 4) as usize;
        assert_eq!(
            &rgba[offset..offset + 4],
            &[shade, shade, shade, 255],
            "selected f32 branch value was not preserved at x={x}"
        );
    }
}

#[test]
fn conditional_f32_selection_feeds_loop_carry_and_both_render_exits() {
    const W: u32 = 8;
    const H: u32 = 1;
    const LOW: f32 = 10.0;
    const HIGH: f32 = 60.0;

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///conditional_f32_loop_carry.fe").expect("test URL should parse");
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(CONDITIONAL_F32_LOOP_CARRY_SOURCE.to_string()),
    );
    let file = db.workspace().get(&db, &url).expect("fixture should load");
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("composed f32 control flow should build a runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("conditional f32 selection and loop carry should lower to Render SPIR-V");
    let input = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("low/high require a broadcast Input binding");
    assert_eq!((input.span, input.stride), (8, 8));
    assert_eq!(input.members.len(), 2);
    assert!(input
        .members
        .iter()
        .all(|member| member.scalar == sonatina_codegen::isa::spirv::SpirvScalarKind::F32));

    let wgsl = artifact.wgsl.as_ref().expect("Render compilation emits WGSL");
    assert_browser_profile_wgsl(wgsl);
    assert!(wgsl.contains("loop"), "the f32 accumulator must remain loop-carried");
    let mut input_bytes = Vec::with_capacity(8);
    input_bytes.extend_from_slice(&LOW.to_bits().to_le_bytes());
    input_bytes.extend_from_slice(&HIGH.to_bits().to_le_bytes());
    let rgba = run_render_rgba8_on_lavapipe(wgsl, W, H, &input_bytes)
        .expect("composed f32 control-flow probe requires browser-profile execution");
    for x in 0..W {
        // x<4 takes low=10 four times and reaches the normal exit (40).
        // x>=4 takes high=60 twice and reaches the early exit (120).
        let shade = if x < 4 { 40 } else { 120 };
        let offset = (x * 4) as usize;
        assert_eq!(
            &rgba[offset..offset + 4],
            &[shade, shade, shade, 255],
            "conditional f32 loop-carry result differs at x={x}"
        );
    }
}

/// The independent Q12 escape-time + color-map oracle, re-derived HERE from the
/// kernel logic (never trusted from the spec), integer-identical to the fixture:
/// the SAME i32 escape math as `mandel_oracle_q12` (arithmetic `>>` on i32, the
/// same literals and escape convention), plus the integer ramp `v = (i*655) >> 8`
/// (LOGICAL `>>` on u32, matching the Fe `Shr`) packed r=g=v, b=255-v, a=255. The
/// return is the packed little-endian RGBA8 word `unpack4x8unorm` maps exactly to
/// the rgba8unorm target's bytes.
///
/// `color` is initialized to the fixture's (dead) init value: the first loop
/// iteration always enters the accept branch (zr=zi=0 => mag=0 < threshold), so
/// the init never surfaces on an escape; matched here only for exactness.
fn mandel_frag_oracle(px: i32, py: i32) -> u32 {
    let c_re: i32 = -8192 + px * 24;
    let c_im: i32 = -6144 + py * 24;
    let mut zr: i32 = 0;
    let mut zi: i32 = 0;
    let mut i: u32 = 0;
    let mut color: u32 = 4_278_190_080;
    while i < 100 {
        let rr: i32 = zr * zr;
        let ii: i32 = zi * zi;
        let mag: i32 = rr + ii;
        if mag < 67_108_864 {
            let t: i32 = rr - ii;
            let nzi: i32 = ((zr * 2) * zi) >> 12; // arithmetic (i32), uses the OLD zr
            zr = (t >> 12) + c_re;
            zi = nzi + c_im;
            i += 1;
            let v: u32 = (i * 655) >> 8; // LOGICAL (u32): the color ramp Shr, 0..255
            color = v + v * 256 + (255 - v) * 65536 + 4_278_190_080;
        } else {
            return color; // escape: the carried ramp color
        }
    }
    4_278_190_080 // interior: opaque black
}

/// Compile the render fixture to wasm through `BackendKind::Wasm`. The fragment
/// function is an ordinary `(i32, i32) -> i32` export on the wasm path (Fe `u32`
/// lowers to wasm `i32`); calling it per pixel is the wasm leg of the tri-equal.
fn compile_mandel_frag_rgba_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandel_frag_rgba_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDEL_FRAG_RGBA_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("mandel_frag_rgba should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// Count the `OpEntryPoint` (opcode 15) instructions in a raw SPIR-V word stream
/// by walking the instruction headers past the 5-word module header. A Render
/// module must carry exactly TWO (the @vertex + @fragment stages in ONE module).
fn count_spirv_entry_points(words: &[u32]) -> usize {
    let mut eps = 0usize;
    let mut idx = 5usize; // skip the 5-word SPIR-V header
    while idx < words.len() {
        let opword = words[idx];
        let wc = (opword >> 16) as usize;
        if (opword & 0xffff) == 15 {
            eps += 1;
        }
        if wc == 0 {
            break;
        }
        idx += wc;
    }
    eps
}

/// R2 render harness: execute a Render-mode WGSL OFFSCREEN on lavapipe (browser
/// profile, `Features::empty()`): a `w x h` rgba8unorm target
/// (RENDER_ATTACHMENT | COPY_SRC), the fullscreen-triangle `draw(0..3)`, then
/// `copy_texture_to_buffer` (256-aligned bytes_per_row) + readback. Returns the
/// TIGHTLY-packed RGBA bytes (row padding stripped) when the GPU ran the shader.
///
/// ANTI-FUDGE (verbatim from the grid/keystone harnesses): a missing
/// adapter/device is a HARD FAILURE, never a silent skip; the only escape is
/// `MB2_ALLOW_GPU_SKIP`, which downgrades the rung honestly (returns `None`).
/// Ported from the fork's executed render probe + the file's grid harness.
fn run_render_rgba8_on_lavapipe(wgsl: &str, w: u32, h: u32, input: &[u8]) -> Option<Vec<u8>> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();

    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(
        &wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            ..Default::default()
        },
    )) {
        Ok(a) => a,
        Err(e) => {
            if allow_skip {
                eprintln!(
                    "  render SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): no Vulkan adapter: {e:?}"
                );
                return None;
            }
            panic!(
                "render SPIR-V leg: no GPU/Vulkan adapter available ({e:?}). The render rung \
                 requires lavapipe to EXECUTE; a missing device is a hard failure, not a skip. \
                 Set VK_ICD_FILENAMES / LD_LIBRARY_PATH / WGPU_BACKEND=vulkan for lavapipe, or \
                 MB2_ALLOW_GPU_SKIP to downgrade the rung on a genuinely GPU-less host."
            );
        }
    };

    // BROWSER PROFILE: NO required features (drop SHADER_INT64), exactly what a
    // WebGPU browser offers. A real failure here means the fragment is NOT
    // browser-viable, a STOP condition, not a skip.
    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        },
    )) {
        Ok(dq) => dq,
        Err(e) => {
            if allow_skip {
                eprintln!(
                    "  render SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {e:?}"
                );
                return None;
            }
            panic!(
                "render SPIR-V leg: browser-profile device request (NO required features) failed \
                 ({e:?}). This is a hard failure, not a skip."
            );
        }
    };

    eprintln!(
        "  render SPIR-V leg GPU adapter (BROWSER PROFILE, no required features): {}",
        adapter.get_info().name
    );

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mandel_frag_render"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });

    // The broadcast input storage buffer at @group(0) @binding(1), FRAGMENT
    // visibility. v1 fragments take no broadcast params (`input.is_empty()`), so
    // the 4-byte dummy floor keeps the unused binding valid; a param-carrying
    // fragment writes its words before the draw.
    let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render_input"),
        size: input.len().max(4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !input.is_empty() {
        queue.write_buffer(&input_buf, 0, input);
    }
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("render_bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("render_pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("render_bg"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 1,
            resource: input_buf.as_entire_binding(),
        }],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fullscreen"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("render_target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());

    // 256-aligned bytes_per_row (COPY_BYTES_PER_ROW_ALIGNMENT). For 512x4 = 2048
    // this is already aligned; assert to document the invariant.
    let bytes_per_row = ((w * 4 + 255) / 256) * 256;
    assert_eq!(
        bytes_per_row % 256,
        0,
        "copy_texture_to_buffer bytes_per_row must be 256-aligned"
    );
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render_staging"),
        size: u64::from(bytes_per_row * h),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).expect("map_async callback channel should be open");
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    });
    rx.recv()
        .expect("map_async callback should fire")
        .expect("staging buffer should map for read");
    let data = slice.get_mapped_range();
    let row = (w * 4) as usize;
    let mut out = Vec::with_capacity(row * h as usize);
    for y in 0..h {
        let off = (y * bytes_per_row) as usize;
        out.extend_from_slice(&data[off..off + row]);
    }
    drop(data);
    staging.unmap();

    Some(out)
}

/// Independent Rust model of `f32_render_probe.fe`. All pinned values are small
/// exact integers or halves, keeping the expected conversion and RGBA bytes
/// deterministic while still executing native f32 operations.
fn f32_render_probe_oracle(px: i32, py: i32, gain: f32, bias: f32, cutoff: f32) -> u32 {
    let mut value = ((px as f32) + (py as f32)) * gain + bias;
    for _ in 0..3 {
        value = value * 0.5 + bias;
        if value > cutoff {
            let hot = value as i32;
            return (hot + hot * 256 + 16_711_680 - 16_777_216) as u32;
        }
    }
    let cool = value as i32;
    (cool + 65_280 + cool * 65_536 - 16_777_216) as u32
}

#[test]
fn f32_render_probe_executes_on_lavapipe_against_independent_oracle() {
    const W: u32 = 8;
    const H: u32 = 8;
    const GAIN: f32 = 4.0;
    const BIAS: f32 = 8.0;
    const CUTOFF: f32 = 30.0;

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///f32_render_probe.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(F32_RENDER_PROBE_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("the f32 Render probe should compile from Fe source");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("the f32 probe should lower to validated Render SPIR-V");

    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render
    );
    assert_eq!(count_spirv_entry_points(&artifact.words), 2);
    let input = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("three f32 broadcast arguments require an Input binding");
    assert_eq!((input.group, input.binding), (0, 1));
    assert_eq!(input.access, sonatina_codegen::isa::spirv::Access::Read);
    assert_eq!(input.stride, 12, "three packed f32 broadcasts occupy 12 bytes");
    assert_eq!(input.span, 12, "the broadcast struct occupies exactly 12 bytes");
    assert_eq!(input.members.len(), 3);
    for (member, arg_index) in input.members.iter().zip(2..=4) {
        assert_eq!(member.arg_index, arg_index);
        assert_eq!(member.offset, (arg_index - 2) * 4);
        assert_eq!(member.width, 4);
        assert_eq!(
            member.scalar,
            sonatina_codegen::isa::spirv::SpirvScalarKind::F32,
        );
    }
    assert_eq!(artifact.layout.builtin_inputs.len(), 2);
    assert_eq!(artifact.layout.builtin_inputs[0].arg_index, 0);
    assert_eq!(
        artifact.layout.builtin_inputs[0].scalar,
        sonatina_codegen::isa::spirv::SpirvScalarKind::I32,
    );
    assert_eq!(
        artifact.layout.builtin_inputs[0].source,
        sonatina_codegen::isa::spirv::SpirvBuiltinSource::FragmentPositionX,
    );
    assert_eq!(artifact.layout.builtin_inputs[1].arg_index, 1);
    assert_eq!(
        artifact.layout.builtin_inputs[1].scalar,
        sonatina_codegen::isa::spirv::SpirvScalarKind::I32,
    );
    assert_eq!(
        artifact.layout.builtin_inputs[1].source,
        sonatina_codegen::isa::spirv::SpirvBuiltinSource::FragmentPositionY,
    );

    let wgsl = artifact.wgsl.as_ref().expect("Render compilation emits WGSL");
    assert_browser_profile_wgsl(wgsl);
    assert!(wgsl.contains("loop"), "loop-carried f32 must survive into WGSL");

    let values = [GAIN, BIAS, CUTOFF];
    let mut input_bytes = vec![0u8; input.span as usize];
    for member in &input.members {
        let value = values[(member.arg_index - 2) as usize];
        let start = member.offset as usize;
        let end = start + member.width as usize;
        input_bytes[start..end].copy_from_slice(&value.to_bits().to_le_bytes());
    }
    assert_eq!(
        f32_render_probe_oracle(7, 7, GAIN, BIAS, CUTOFF).to_le_bytes(),
        [40, 40, 255, 255],
        "the probe must exercise its first-iteration early return",
    );
    assert_eq!(
        f32_render_probe_oracle(0, 0, GAIN, BIAS, CUTOFF).to_le_bytes(),
        [15, 255, 15, 255],
        "the probe must also exercise normal exit after all three iterations",
    );
    let rgba = run_render_rgba8_on_lavapipe(wgsl, W, H, &input_bytes)
        .expect("the focused f32 Render regression requires GPU execution");
    for y in 0..H {
        for x in 0..W {
            let offset = ((y * W + x) * 4) as usize;
            let expected = f32_render_probe_oracle(
                x as i32,
                y as i32,
                GAIN,
                BIAS,
                CUTOFF,
            )
            .to_le_bytes();
            assert_eq!(
                &rgba[offset..offset + 4],
                &expected,
                "f32 Render pixel ({x},{y}) must match the independent Rust oracle"
            );
        }
    }
}

#[test]
fn recursive_mvt2_f32_render_executes_on_lavapipe() {
    const W: u32 = 2;
    const H: u32 = 2;
    const COEFFS: [f32; 4] = [11.0, 22.0, 33.0, 44.0];

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mvt2_f32_render.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MVT2_F32_RENDER_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("call-free recursive f32 tree should build a runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("call-free recursive f32 tree should compile as Render SPIR-V");
    let input = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("four f32 coefficients require an Input binding");
    assert_eq!((input.group, input.binding), (0, 1));
    assert_eq!(input.access, sonatina_codegen::isa::spirv::Access::Read);
    assert_eq!((input.span, input.stride, input.members.len()), (16, 16, 4));
    for (member, arg_index) in input.members.iter().zip(2..=5) {
        assert_eq!(member.arg_index, arg_index);
        assert_eq!(member.offset, (arg_index - 2) * 4);
        assert_eq!(member.width, 4);
        assert_eq!(
            member.scalar,
            sonatina_codegen::isa::spirv::SpirvScalarKind::F32,
        );
    }
    assert_eq!(artifact.layout.builtin_inputs.len(), 2);
    assert_eq!(artifact.layout.builtin_inputs[0].arg_index, 0);
    assert_eq!(
        artifact.layout.builtin_inputs[0].scalar,
        sonatina_codegen::isa::spirv::SpirvScalarKind::I32,
    );
    assert_eq!(
        artifact.layout.builtin_inputs[0].source,
        sonatina_codegen::isa::spirv::SpirvBuiltinSource::FragmentPositionX,
    );
    assert_eq!(artifact.layout.builtin_inputs[1].arg_index, 1);
    assert_eq!(
        artifact.layout.builtin_inputs[1].scalar,
        sonatina_codegen::isa::spirv::SpirvScalarKind::I32,
    );
    assert_eq!(
        artifact.layout.builtin_inputs[1].source,
        sonatina_codegen::isa::spirv::SpirvBuiltinSource::FragmentPositionY,
    );

    let mut input_bytes = vec![0u8; input.span as usize];
    for member in &input.members {
        let value = COEFFS[(member.arg_index - 2) as usize];
        input_bytes[member.offset as usize..member.offset as usize + 4]
            .copy_from_slice(&value.to_bits().to_le_bytes());
    }
    let wgsl = artifact.wgsl.as_deref().expect("Render compilation emits WGSL");
    assert_browser_profile_wgsl(wgsl);
    let rgba = run_render_rgba8_on_lavapipe(wgsl, W, H, &input_bytes)
        .expect("MvT<2> f32 Render regression requires GPU execution");
    for y in 0..H {
        for x in 0..W {
            let offset = ((y * W + x) * 4) as usize;
            assert_eq!(
                &rgba[offset..offset + 4],
                &[11 + x as u8, 22 + y as u8, 121, 255],
                "nested f32 tree pixel ({x},{y}) must preserve all four leaves",
            );
        }
    }
}

#[test]
fn generated_recursive_mvt5_f32_render_executes_on_lavapipe() {
    const W: u32 = 8;
    const H: u32 = 4;

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mvt5_f32_render.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MVT5_F32_RENDER_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("call-free depth-5 f32 tree should build a runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("call-free depth-5 f32 tree should compile as Render SPIR-V");
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render
    );
    assert_eq!(count_spirv_entry_points(&artifact.words), 2);
    assert_eq!(
        artifact
            .layout
            .bindings
            .iter()
            .filter(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
            .count(),
        1,
        "the 32 broadcasts must use exactly one Input binding",
    );
    let input = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("32 f32 leaves require an Input binding");
    assert_eq!((input.group, input.binding), (0, 1));
    assert_eq!(input.access, sonatina_codegen::isa::spirv::Access::Read);
    assert_eq!((input.span, input.stride, input.members.len()), (128, 128, 32));
    for (member, arg_index) in input.members.iter().zip(2..=33) {
        assert_eq!(member.arg_index, arg_index);
        assert_eq!(member.offset, (arg_index - 2) * 4);
        assert_eq!(member.width, 4);
        assert_eq!(member.scalar, sonatina_codegen::isa::spirv::SpirvScalarKind::F32);
    }
    assert_eq!(artifact.layout.builtin_inputs.len(), 2);
    assert_eq!(artifact.layout.builtin_inputs[0].arg_index, 0);
    assert_eq!(
        artifact.layout.builtin_inputs[0].scalar,
        sonatina_codegen::isa::spirv::SpirvScalarKind::I32,
    );
    assert_eq!(
        artifact.layout.builtin_inputs[0].source,
        sonatina_codegen::isa::spirv::SpirvBuiltinSource::FragmentPositionX,
    );
    assert_eq!(artifact.layout.builtin_inputs[1].arg_index, 1);
    assert_eq!(
        artifact.layout.builtin_inputs[1].scalar,
        sonatina_codegen::isa::spirv::SpirvScalarKind::I32,
    );
    assert_eq!(
        artifact.layout.builtin_inputs[1].source,
        sonatina_codegen::isa::spirv::SpirvBuiltinSource::FragmentPositionY,
    );
    let mut input_bytes = vec![0u8; input.span as usize];
    for member in &input.members {
        let leaf = (member.arg_index - 2) as i32;
        let value = (3 * leaf + 2) as f32;
        input_bytes[member.offset as usize..member.offset as usize + 4]
            .copy_from_slice(&value.to_bits().to_le_bytes());
    }
    let wgsl = artifact.wgsl.as_deref().expect("Render compilation emits WGSL");
    assert_browser_profile_wgsl(wgsl);
    let rgba = run_render_rgba8_on_lavapipe(wgsl, W, H, &input_bytes)
        .expect("MvT<5> f32 Render regression requires GPU execution");
    for y in 0..H {
        for x in 0..W {
            let offset = ((y * W + x) * 4) as usize;
            let i = (x + 8 * y) as i32;
            let expected = ((2 * i + 1) * (3 * i + 2) + (1000 + i)) as u32;
            assert_eq!(
                &rgba[offset..offset + 4],
                &expected.to_le_bytes(),
                "depth-5 transformed DFS leaf {i} at pixel ({x},{y})",
            );
        }
    }
}

/// Independent scalar oracle for D1. Keep the operation grouping identical to
/// the Fe source and avoid `mul_add`: this models the actual f32 program, not a
/// higher-precision restatement of its geometry.
fn cga_inversion_de_oracle(
    px: i32,
    py: i32,
    cam_x: f32,
    cam_y: f32,
    zoom: f32,
) -> (u32, u8) {
    let fx = px as f32;
    let fy = py as f32;
    let sx = (fx - 64.0) * zoom;
    let sy = (fy - 64.0) * zoom;
    let rz = 1.8_f32;
    let inv_len = 1.0 / (sx * sx + sy * sy + rz * rz).sqrt();
    let rdx = sx * inv_len;
    let rdy = sy * inv_len;
    let rdz = rz * inv_len;

    let mut t = 0.0_f32;
    let mut i = 0_i32;
    while i < 64 {
        let x = cam_x + rdx * t;
        let y = cam_y + rdy * t;
        let z = -4.0 + rdz * t;
        let vx = x - 0.5;
        let rho2 = vx * vx + y * y + z * z;

        // The normalized fixed-versor sandwich S*P*S, scalarized after CTFE.
        let qx = 0.5 + vx / rho2;
        let qy = y / rho2;
        let qz = z / rho2;
        let ax = qx + 0.65;
        let ay = qy + 0.30;
        let distance_a = (ax * ax + ay * ay + qz * qz).sqrt() - 0.27;
        let bx = qx + 0.65;
        let by = qy - 0.30;
        let distance_b = (bx * bx + by * by + qz * qz).sqrt() - 0.27;
        let a_is_closer = distance_a < distance_b;
        let base = if a_is_closer { distance_a } else { distance_b };
        let distance = base * rho2;
        t = t + distance * 0.2;
        if distance < 0.0025 {
            let shade = 32 + i * 3;
            if a_is_closer {
                return (
                    (shade + (255 - shade) * 256 + 224 * 65_536 - 16_777_216_i32)
                        as u32,
                    1,
                );
            }
            return (
                (224 + (shade + 16) * 256 + shade * 65_536 - 16_777_216_i32) as u32,
                2,
            );
        }
        i += 1;
    }
    ((8 + 12 * 256 + 24 * 65_536 - 16_777_216_i32) as u32, 0)
}

/// D1: render a two-sphere union through a fixed, fold-derived Cl(4,1)
/// inversion on lavapipe. This is the scalar partial evaluation of the
/// generated product (D2 will execute the recursive product at runtime), and
/// every pixel must equal an independent Rust-f32 oracle byte-for-byte.
#[test]
fn cga_inversion_de_render_executes_on_lavapipe_against_f32_oracle() {
    const W: u32 = 128;
    const H: u32 = 128;
    const CAM_X: f32 = 0.0;
    const CAM_Y: f32 = 0.0;
    const ZOOM: f32 = 0.0125;

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///cga_inversion_de_render.fe").expect("test URL should parse");
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(CGA_INVERSION_DE_RENDER_SOURCE.to_string()),
    );
    let file = db.workspace().get(&db, &url).expect("file should load");
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("D1 fixed-versor source should build a wasm runtime package");
    assert!(
        package
            .functions(&db)
            .iter()
            .all(|function| function.linkage(&db) != mir::RuntimeLinkage::External),
        "all f32 helpers must become typed MIR/Sonatina operations, never host imports",
    );
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("D1 should lower to naga-validated Render SPIR-V");

    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render,
    );
    assert_eq!(count_spirv_entry_points(&artifact.words), 2);
    let input = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("D1's camera arguments require a broadcast Input binding");
    assert_eq!((input.group, input.binding), (0, 1));
    assert_eq!(input.access, sonatina_codegen::isa::spirv::Access::Read);
    assert_eq!((input.span, input.stride), (12, 12));
    assert_eq!(input.members.len(), 3);
    for (member, arg_index) in input.members.iter().zip(2..=4) {
        assert_eq!(member.arg_index, arg_index);
        assert_eq!(member.offset, (arg_index - 2) * 4);
        assert_eq!(member.width, 4);
        assert_eq!(
            member.scalar,
            sonatina_codegen::isa::spirv::SpirvScalarKind::F32,
        );
    }
    assert_eq!(artifact.layout.builtin_inputs.len(), 2);
    for (builtin, arg_index) in artifact.layout.builtin_inputs.iter().zip(0..=1) {
        assert_eq!(builtin.arg_index, arg_index);
        assert_eq!(
            builtin.scalar,
            sonatina_codegen::isa::spirv::SpirvScalarKind::I32,
        );
    }

    let wgsl = artifact.wgsl.as_ref().expect("Render compilation emits WGSL");
    assert_browser_profile_wgsl(wgsl);
    assert!(wgsl.contains("loop"), "D1 must retain its raymarch loop");
    assert!(wgsl.contains("sqrt("), "D1 must use native f32 sqrt");
    for forbidden in ["__f32_from_i32", "__sqrt_f32", "__i32_from_f32"] {
        assert!(
            !wgsl.contains(forbidden),
            "f32 helper `{forbidden}` escaped into WGSL instead of lowering intrinsically",
        );
    }

    let values = [CAM_X, CAM_Y, ZOOM];
    let mut input_bytes = vec![0_u8; input.span as usize];
    for member in &input.members {
        let start = member.offset as usize;
        input_bytes[start..start + 4].copy_from_slice(
            &values[(member.arg_index - 2) as usize]
                .to_bits()
                .to_le_bytes(),
        );
    }
    let rgba = run_render_rgba8_on_lavapipe(wgsl, W, H, &input_bytes)
        .expect("D1 requires browser-profile lavapipe execution");
    assert_eq!(rgba.len(), (W * H * 4) as usize);

    let mut sky_count = 0_usize;
    let mut material_a_count = 0_usize;
    let mut material_b_count = 0_usize;
    let mut distinct = std::collections::HashSet::new();
    for y in 0..H {
        for x in 0..W {
            let offset = ((y * W + x) * 4) as usize;
            let actual = &rgba[offset..offset + 4];
            let (expected, material) = cga_inversion_de_oracle(
                x as i32,
                y as i32,
                CAM_X,
                CAM_Y,
                ZOOM,
            );
            let expected = expected.to_le_bytes();
            assert_eq!(
                actual, &expected,
                "D1 conformal-inversion pixel ({x},{y}) differs from the Rust-f32 oracle",
            );
            distinct.insert(expected);
            match material {
                0 => sky_count += 1,
                1 => material_a_count += 1,
                2 => material_b_count += 1,
                other => panic!("unexpected oracle material {other}"),
            }
        }
    }
    assert!(sky_count > 0, "D1 image must contain background");
    assert!(material_a_count > 0, "D1 image must contain inverted sphere A");
    assert!(material_b_count > 0, "D1 image must contain inverted sphere B");
    assert!(
        distinct.len() >= 8,
        "step shading must expose a non-degenerate 3D surface ({} colors)",
        distinct.len(),
    );
}

/// R1b (validation, GPU-FREE): the Fe fragment kernel compiles through the Render
/// driver seam into ONE naga-validated SPIR-V module with TWO entry points
/// (@vertex + @fragment), states its render ABI, and its browser-profile WGSL
/// carries the render epilogue (`unpack4x8unorm`, `@location(0)`), the escape loop
/// (`loop`), and the signed-i32 sign mapping (`bitcast<i32>`).
#[test]
fn mandel_frag_rgba_compiles_to_render_spirv() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandel_frag_rgba_render.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDEL_FRAG_RGBA_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("mandel_frag_rgba should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("mandel_frag_rgba should compile Fe -> naga-validated SPIR-V in Render mode");

    // --- Render ABI (self-describing layout). ---
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render,
        "the render driver seam must state LayoutMode::Render"
    );
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the render fragment must lower to the u32 word (browser profile)"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Render mode has no single-slot result: the color target is the result"
    );
    assert_eq!(
        artifact.layout.workgroup_size,
        [0, 0, 0],
        "Render mode has no workgroup size"
    );
    assert_eq!(
        artifact.layout.vertex_entry.as_deref(),
        Some("vs_fullscreen"),
        "Render mode states the @vertex entry name"
    );
    assert_eq!(
        artifact.layout.fragment_entry.as_deref(),
        Some("fs_main"),
        "Render mode states the @fragment entry name"
    );
    assert_eq!(
        artifact.layout.color_target_format.as_deref(),
        Some("rgba8unorm"),
        "Render mode states its color-target format"
    );
    assert!(
        artifact
            .layout
            .bindings
            .iter()
            .all(|b| b.role != sonatina_codegen::isa::spirv::Role::Output),
        "Render mode has no output storage binding (the color target is the output)"
    );

    // --- TWO entry points in ONE SPIR-V module. ---
    assert_eq!(
        count_spirv_entry_points(&artifact.words),
        2,
        "one Render SPIR-V module must carry BOTH entry points (@vertex + @fragment)"
    );

    // --- Browser-profile WGSL gate + the render/fractal honesty tokens. ---
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the render fragment");
    assert_browser_profile_wgsl(wgsl);
    assert!(
        wgsl.contains("@vertex"),
        "render WGSL must contain the @vertex stage; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("@fragment"),
        "render WGSL must contain the @fragment stage; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("@location(0)"),
        "the fragment must write @location(0); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("unpack4x8unorm"),
        "the render epilogue must be unpack4x8unorm (packed u32 -> vec4<f32>); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("loop"),
        "the fractal WGSL must contain a `loop` (the escape loop, not a flattened body); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("bitcast<i32>"),
        "the fractal WGSL must contain `bitcast<i32>` (the signed Slt/Sar sign mapping); got:\n{wgsl}"
    );
    eprintln!(
        "R1b: Fe mandel_frag_rgba compiled -> ONE Render SPIR-V module, 2 entry points \
         (@vertex + @fragment), browser-profile WGSL with unpack4x8unorm + @location(0) + loop + \
         bitcast<i32>. {} SPIR-V words.",
        artifact.words.len()
    );
}

/// The fixed R2 render view: a 512x512 frame (spec 5.1), the SAME view the M2
/// compute grid uses (mandel_frag_rgba reuses mandel_pixel_q12's Q12 coordinate
/// map), so the rendered image is the same fractal, now colored ON the GPU.
const FRAG_W: u32 = 512;
const FRAG_H: u32 = 512;

/// R2 headline (spec 5.1): the Fe FRAGMENT shader RENDERS on lavapipe at the
/// browser profile, and every one of 262,144 pixels x 4 bytes is TRI-EQUAL: the
/// rendered rgba8unorm texture == the independent `mandel_frag_oracle` == the
/// wasm execution of the SAME Fe function. That three-way per-byte agreement over
/// the whole rendered frame earns "Fe emitted a working render pipeline" as a
/// fact. Exact equality is legitimate: unorm8 round-trip is exact, the target is
/// rgba8unorm (NOT srgb), and blending is off.
///
/// Hard-fail-not-skip: a missing GPU is a hard failure; the only escape is
/// `MB2_ALLOW_GPU_SKIP` (adapter printed on execute). The name contains
/// "lavapipe" so the nextest serial group filter (`test(lavapipe)`) catches it.
#[test]
fn mandel_frag_rgba_renders_on_lavapipe_browser_profile() {
    // --- Compile the fragment through the Render driver seam. ---
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandel_frag_rgba_lavapipe.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDEL_FRAG_RGBA_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("mandel_frag_rgba should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("mandel_frag_rgba should compile Fe -> naga-validated SPIR-V in Render mode");
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render,
        "the render driver seam must state LayoutMode::Render"
    );
    assert_eq!(
        count_spirv_entry_points(&artifact.words),
        2,
        "one Render SPIR-V module must carry BOTH entry points"
    );
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the render fragment");
    assert_browser_profile_wgsl(wgsl);

    // --- The wasm leg: the SAME Fe function under wasmtime over the whole frame,
    // returning the packed RGBA8 color per pixel (Fe u32 -> wasm i32 -> u32). ---
    let wasm_bytes = compile_mandel_frag_rgba_to_wasm();
    let wasm_colors = wasm_grid_all(&wasm_bytes, FRAG_W, FRAG_H, "mandel_frag_rgba");

    // --- The GPU leg: RENDER the fragment on lavapipe (browser profile, 512x512
    // rgba8unorm offscreen target) and compare every pixel's 4 bytes to BOTH the
    // oracle AND the wasm leg (tri-equal for the RENDERED image). ---
    match run_render_rgba8_on_lavapipe(wgsl, FRAG_W, FRAG_H, &[]) {
        Some(rgba) => {
            assert_eq!(
                rgba.len(),
                (FRAG_W * FRAG_H * 4) as usize,
                "render readback must be 512*512*4 = 1048576 bytes (tightly packed)"
            );
            let mut distinct = std::collections::HashSet::new();
            for y in 0..FRAG_H {
                for x in 0..FRAG_W {
                    let idx = (y * FRAG_W + x) as usize;
                    let px = &rgba[idx * 4..idx * 4 + 4];
                    let oracle = mandel_frag_oracle(x as i32, y as i32);
                    let oracle_bytes = oracle.to_le_bytes();
                    let wasm_bytes_px = wasm_colors[idx].to_le_bytes();
                    assert_eq!(
                        px, &oracle_bytes,
                        "lavapipe rendered pixel ({x},{y}) RGBA {px:?} must equal the oracle color \
                         {oracle_bytes:?} (packed 0x{oracle:08X})"
                    );
                    assert_eq!(
                        px, &wasm_bytes_px,
                        "lavapipe rendered pixel ({x},{y}) RGBA {px:?} must equal the wasm leg color \
                         {wasm_bytes_px:?} for the same (x,y)"
                    );
                    distinct.insert(oracle);
                }
            }

            // Recognizability, DERIVED from the rendered image (not baked): the
            // interior center is opaque BLACK, the fast-escape corner is a colored
            // (non-black) band, and the color histogram is non-degenerate. An
            // all-black, an all-one-color, or a transposed image could not pass.
            let center = (256 * FRAG_W + 256) as usize;
            assert_eq!(
                &rgba[center * 4..center * 4 + 4],
                &[0x00, 0x00, 0x00, 0xFF],
                "GPU center pixel (256,256) -> c=-0.5+0i is interior: opaque black 0xFF000000"
            );
            assert_eq!(
                mandel_frag_oracle(256, 256),
                0xFF00_0000,
                "the oracle agrees the interior center is opaque black"
            );
            // The corner (0,0) escapes fast (i=1); its color is the ramp at i=1,
            // which is NOT black (b channel is bright). Confirm the rendered corner
            // is non-black AND equals the oracle.
            let corner_black = &rgba[0..4] == [0x00, 0x00, 0x00, 0xFF];
            assert!(
                !corner_black,
                "GPU corner pixel (0,0) escapes fast and must be a COLORED band, not black; got {:?}",
                &rgba[0..4]
            );
            assert!(
                distinct.len() >= 10,
                "the rendered color histogram must have >= 10 distinct colors (got {}); a \
                 degenerate image could not",
                distinct.len()
            );

            eprintln!(
                "R2: Fe mandel_frag_rgba RENDERED on lavapipe (browser profile, 512x512 \
                 rgba8unorm); ALL 262,144 pixels (x4 bytes) TRI-EQUAL (texture == oracle == wasm); \
                 {} distinct colors; interior=black + fast-escape corner colored. The Fe compiler \
                 compiled the fractal AND its coloring; the GPU rendered every pixel; JavaScript \
                 painted nothing. Render mode earns R-lava.",
                distinct.len()
            );
        }
        None => {
            eprintln!(
                "R-val only: render SPIR-V validated (browser profile) but NOT executed (GPU \
                 skipped via MB2_ALLOW_GPU_SKIP). The 262,144-pixel rendered tri-equal claim is \
                 NOT earned on this run."
            );
        }
    }
}

// ===========================================================================
// I1 (interactive renderer-in-Fe): the VIEW-PARAMETERIZED fragment.
//
// `mandel_view_frag(px, py, center_re, center_im, scale_q) -> u32` is the R1
// fragment with the fixed-view constants replaced by THREE broadcast view params.
// Args 0,1 (px, py) stay the render position builtins; args 2..4 (the view) ride
// the broadcast Input struct at @group(0) @binding(1) (members p0,p1,p2 at
// offsets 0,4,8; span 12), exactly the path Clifford C2 proved on the grid, now
// on the render path (sonatina `mod.rs` render arm loads args 2.. from the same
// Input struct). The pixel->complex map is
//     c_re = center_re + (((px - 256) * scale_q) >> 4)
//     c_im = center_im + (((py - 256) * scale_q) >> 4)
//
// I1's gate: RENDER on lavapipe (browser profile, 512x512) at the pinned views
// the spec names and assert ALL 262,144 pixels x 4 bytes TRI-EQUAL (texture ==
// Rust oracle == wasm leg). The DEFAULT token (-2048, 0, 384) is the regression
// anchor: it must reproduce R1's `mandel_frag_rgba` view BYTE-FOR-BYTE (asserted
// against the R1 `mandel_frag_oracle`). Hard-fail-not-skip; MB2_ALLOW_GPU_SKIP
// only, adapter printed.
// ===========================================================================

/// The SSOT I1 fixture: `include_str!`-ed so the tested and (later) shipped source
/// are byte-identical by construction.
const MANDEL_VIEW_FRAG_SOURCE: &str = include_str!("fixtures/spirv/mandel_view_frag.fe");

/// The independent view-parameterized Q12 escape+color oracle, re-derived HERE
/// from the kernel logic (never trusted from the spec), integer-identical to the
/// fixture: the SAME i32 escape math as `mandel_frag_oracle` with the fixed-view
/// constants replaced by the broadcast view (`center + (((p - 256) * scale_q) >>
/// 4)`, arithmetic i32 `>>`), plus the same u32 color ramp. At the default token
/// (-2048, 0, 384) this is provably identical to `mandel_frag_oracle` (384 = 24*16,
/// the `>> 4` is exact because `(p-256)*384` is divisible by 16).
fn mandel_view_frag_oracle(px: i32, py: i32, center_re: i32, center_im: i32, scale_q: i32) -> u32 {
    let c_re: i32 = center_re + (((px - 256) * scale_q) >> 4);
    let c_im: i32 = center_im + (((py - 256) * scale_q) >> 4);
    let mut zr: i32 = 0;
    let mut zi: i32 = 0;
    let mut i: u32 = 0;
    let mut color: u32 = 4_278_190_080;
    while i < 100 {
        let rr: i32 = zr * zr;
        let ii: i32 = zi * zi;
        let mag: i32 = rr + ii;
        if mag < 67_108_864 {
            let t: i32 = rr - ii;
            let nzi: i32 = ((zr * 2) * zi) >> 12; // arithmetic (i32), uses OLD zr
            zr = (t >> 12) + c_re;
            zi = nzi + c_im;
            i += 1;
            let v: u32 = (i * 655) >> 8; // LOGICAL (u32): the color ramp Shr, 0..255
            color = v + v * 256 + (255 - v) * 65536 + 4_278_190_080;
        } else {
            return color; // escape: the carried ramp color
        }
    }
    4_278_190_080 // interior: opaque black
}

/// Compile the I1 fixture to wasm through `BackendKind::Wasm`. The view fragment
/// is an ordinary `(i32, i32, i32, i32, i32) -> i32` export on the wasm path (Fe
/// `u32` lowers to wasm `i32`); calling it per pixel is the wasm leg of the
/// tri-equal.
fn compile_mandel_view_frag_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandel_view_frag_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDEL_VIEW_FRAG_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("mandel_view_frag should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// Run the Fe view fragment (the 5-arg typed func) over the FULL 512x512 grid for
/// one pinned view, returning the per-pixel packed RGBA8 grid (row-major, u32).
/// This is the wasm value in the tri-equal claim, recomputed here so the three-way
/// equality lives inside the one executing test (as C2's `wasm_clifford_grid_all`).
fn wasm_view_frag_grid_all(bytes: &[u8], center_re: i32, center_im: i32, scale_q: i32) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "mandel_view_frag")
        .expect("`mandel_view_frag` export should exist as (i32 x5) -> i32");
    let mut out = Vec::with_capacity((FRAG_W * FRAG_H) as usize);
    for py in 0..FRAG_H as i32 {
        for px in 0..FRAG_W as i32 {
            let v = f
                .call(&mut store, (px, py, center_re, center_im, scale_q))
                .expect("mandel_view_frag should run") as u32;
            out.push(v);
        }
    }
    out
}

/// The pinned views the spec (section 7) names, each `(name, center_re, center_im,
/// scale_q, min_distinct)`. `min_distinct` is a DERIVED non-degeneracy floor
/// (measured, not baked): the default and the two interior valleys show hundreds
/// of colors; the clamp corner is a fast-escape near-uniform patch (2 colors),
/// asserted `>= 2` so a fully-degenerate one-color image still fails.
const VIEW_PINS: [(&str, i32, i32, i32, usize); 4] = [
    ("default", -2048, 0, 384, 10),
    ("seahorse", -3072, 410, 48, 10),
    ("ceiling", 1126, 29, 16, 10),
    ("clamp_corner", 10240, 10240, 384, 2),
];

/// I1 (validation, GPU-FREE): the view fragment compiles through the Render seam
/// into ONE naga-validated SPIR-V module with TWO entry points, states its render
/// ABI, and its browser-profile WGSL carries the render epilogue AND the
/// three-member broadcast load (`input.p0`..`input.p2`, span 12) proving args 2..4
/// became the broadcast view.
#[test]
fn mandel_view_frag_compiles_to_render_spirv() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandel_view_frag_render.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDEL_VIEW_FRAG_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("mandel_view_frag should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("mandel_view_frag should compile Fe -> naga-validated SPIR-V in Render mode");

    // --- Render ABI + TWO entry points. ---
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render,
        "the render driver seam must state LayoutMode::Render"
    );
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the render fragment must lower to the u32 word (browser profile)"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Render mode has no single-slot result: the color target is the result"
    );
    assert_eq!(
        artifact.layout.vertex_entry.as_deref(),
        Some("vs_fullscreen"),
        "Render mode states the @vertex entry name"
    );
    assert_eq!(
        artifact.layout.fragment_entry.as_deref(),
        Some("fs_main"),
        "Render mode states the @fragment entry name"
    );
    assert_eq!(
        count_spirv_entry_points(&artifact.words),
        2,
        "one Render SPIR-V module must carry BOTH entry points (@vertex + @fragment)"
    );

    // --- The three-member broadcast view: Input binding stride 12 is the static
    // proof that args 2,3,4 (center_re, center_im, scale_q) became broadcast
    // members p0,p1,p2 at offsets 0,4,8. ---
    let input_stride = artifact
        .layout
        .bindings
        .iter()
        .find(|b| b.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("the render layout must have an Input binding")
        .stride;
    assert_eq!(
        input_stride, 12,
        "the THREE broadcast view members (center_re, center_im, scale_q), 4 bytes each, span 12 \
         bytes: the layout-level proof that the fragment's args 2..4 became the render broadcast view"
    );

    // --- Browser-profile WGSL gate + the render/fractal/broadcast honesty tokens. ---
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the render fragment");
    assert_browser_profile_wgsl(wgsl);
    assert!(
        wgsl.contains("@vertex") && wgsl.contains("@fragment"),
        "render WGSL must contain BOTH @vertex and @fragment stages; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("@location(0)") && wgsl.contains("unpack4x8unorm"),
        "the render epilogue must write @location(0) via unpack4x8unorm; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("loop") && wgsl.contains("bitcast<i32>"),
        "the fractal WGSL must contain the escape `loop` and `bitcast<i32>` (signed Slt/Sar); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("input.p0"),
        "the fragment must load the first broadcast view member `input.p0` (center_re); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("input.p2"),
        "the fragment must load the THIRD broadcast view member `input.p2` (scale_q); all three \
         view members are read; got:\n{wgsl}"
    );
    eprintln!(
        "I1-val: Fe mandel_view_frag compiled -> ONE Render SPIR-V module, 2 entry points, Input \
         stride 12 (3 broadcast view members), WGSL with unpack4x8unorm + loop + bitcast<i32> + \
         input.p0..input.p2. {} SPIR-V words.",
        artifact.words.len()
    );
}

/// I1 headline (spec section 7): the VIEW-PARAMETERIZED Fe fragment RENDERS on
/// lavapipe at the browser profile, and at EACH pinned view every one of 262,144
/// pixels x 4 bytes is TRI-EQUAL (texture == `mandel_view_frag_oracle` == the wasm
/// execution of the same Fe function), with the view delivered as three broadcast
/// words written to the Input buffer before the draw. The DEFAULT view (-2048, 0,
/// 384) is additionally asserted BYTE-FOR-BYTE against R1's `mandel_frag_oracle`:
/// the parameterized fragment reproduces R1's fixed view exactly.
///
/// Hard-fail-not-skip: a missing GPU is a hard failure; the only escape is
/// `MB2_ALLOW_GPU_SKIP` (adapter printed on execute). The name contains "lavapipe"
/// so the nextest serial group filter (`test(lavapipe)`) catches it.
#[test]
fn mandel_view_frag_renders_on_lavapipe_browser_profile() {
    // --- Compile the view fragment through the Render driver seam. ---
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandel_view_frag_lavapipe.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDEL_VIEW_FRAG_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("mandel_view_frag should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("mandel_view_frag should compile Fe -> naga-validated SPIR-V in Render mode");
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render,
        "the render driver seam must state LayoutMode::Render"
    );
    assert_eq!(
        count_spirv_entry_points(&artifact.words),
        2,
        "one Render SPIR-V module must carry BOTH entry points"
    );
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the render fragment");
    assert_browser_profile_wgsl(wgsl);

    // --- The DEFAULT-view regression anchor, at the ORACLE level: the
    // parameterized oracle at (-2048, 0, 384) must be byte-identical to R1's fixed
    // `mandel_frag_oracle` for EVERY pixel (independent of any GPU). This is the
    // "the default token reproduces R1 byte-for-byte" invariant, checked before we
    // ever touch the GPU. ---
    for py in 0..FRAG_H {
        for px in 0..FRAG_W {
            assert_eq!(
                mandel_view_frag_oracle(px as i32, py as i32, -2048, 0, 384),
                mandel_frag_oracle(px as i32, py as i32),
                "the view oracle at the default token (-2048,0,384) must equal R1's fixed-view \
                 oracle at pixel ({px},{py}) (the byte-for-byte regression anchor)"
            );
        }
    }
    eprintln!(
        "I1 anchor: mandel_view_frag_oracle(px,py,-2048,0,384) == R1 mandel_frag_oracle(px,py) for \
         all 262,144 pixels (default view is byte-identical to R1's fixed view)."
    );

    // --- The wasm bytecode (view-independent: the view arrives as args 2..4). ---
    let wasm_bytes = compile_mandel_view_frag_to_wasm();

    // --- The GPU leg: for each pinned view, write (center_re, center_im, scale_q)
    // to the render broadcast Input buffer, RENDER on lavapipe (512x512 rgba8unorm
    // offscreen), and compare every pixel's 4 bytes to BOTH the oracle AND the wasm
    // leg (tri-equal). The default view is additionally asserted against R1's oracle
    // bytes (the byte-for-byte anchor, now on the RENDERED image). ---
    for (name, center_re, center_im, scale_q, min_distinct) in VIEW_PINS {
        let wasm_colors = wasm_view_frag_grid_all(&wasm_bytes, center_re, center_im, scale_q);
        // The view words, in kernel-arg order: arg2=center_re -> p0, arg3=center_im
        // -> p1, arg4=scale_q -> p2 (i32 two's complement in the u32 word). Written
        // to the Input buffer before the draw, exactly how C2 passes signed params.
        let params: [u32; 3] = [center_re as u32, center_im as u32, scale_q as u32];
        let input_bytes: Vec<u8> = params.iter().flat_map(|p| p.to_le_bytes()).collect();
        match run_render_rgba8_on_lavapipe(wgsl, FRAG_W, FRAG_H, &input_bytes) {
            Some(rgba) => {
                assert_eq!(
                    rgba.len(),
                    (FRAG_W * FRAG_H * 4) as usize,
                    "render readback must be 512*512*4 = 1048576 bytes (tightly packed)"
                );
                let mut distinct = std::collections::HashSet::new();
                for y in 0..FRAG_H {
                    for x in 0..FRAG_W {
                        let idx = (y * FRAG_W + x) as usize;
                        let px = &rgba[idx * 4..idx * 4 + 4];
                        let oracle = mandel_view_frag_oracle(x as i32, y as i32, center_re, center_im, scale_q);
                        let oracle_bytes = oracle.to_le_bytes();
                        let wasm_bytes_px = wasm_colors[idx].to_le_bytes();
                        assert_eq!(
                            px, &oracle_bytes,
                            "lavapipe rendered pixel ({x},{y}) [view {name}] RGBA {px:?} must equal \
                             the oracle color {oracle_bytes:?} (packed 0x{oracle:08X})"
                        );
                        assert_eq!(
                            px, &wasm_bytes_px,
                            "lavapipe rendered pixel ({x},{y}) [view {name}] RGBA {px:?} must equal \
                             the wasm leg color {wasm_bytes_px:?} for the same (x,y)"
                        );
                        // The default-view BYTE-FOR-BYTE R1 anchor, on the RENDERED
                        // image: the GPU bytes must equal R1's fixed-view oracle.
                        if name == "default" {
                            let r1_bytes = mandel_frag_oracle(x as i32, y as i32).to_le_bytes();
                            assert_eq!(
                                px, &r1_bytes,
                                "DEFAULT view rendered pixel ({x},{y}) must be BYTE-IDENTICAL to R1's \
                                 mandel_frag_oracle {r1_bytes:?}; the regression anchor"
                            );
                        }
                        distinct.insert(oracle);
                    }
                }
                assert!(
                    distinct.len() >= min_distinct,
                    "view {name}: rendered color histogram must have >= {min_distinct} distinct \
                     colors (got {}); a degenerate/transposed image could not pass the per-(x,y) \
                     tri-equal either",
                    distinct.len()
                );
                eprintln!(
                    "I1 [{name} = ({center_re},{center_im},{scale_q})]: Fe mandel_view_frag RENDERED \
                     on lavapipe (browser profile, 512x512) with the view as 3 broadcast params; ALL \
                     262,144 pixels TRI-EQUAL (texture == oracle == wasm); {} distinct colors.{}",
                    distinct.len(),
                    if name == "default" { " DEFAULT view is BYTE-IDENTICAL to R1." } else { "" }
                );
            }
            None => {
                eprintln!(
                    "R-val only [{name}]: render SPIR-V validated but NOT executed (GPU skipped via \
                     MB2_ALLOW_GPU_SKIP). The view-parameterized tri-equal claim is NOT earned."
                );
                return;
            }
        }
    }

    eprintln!(
        "I1: the VIEW-PARAMETERIZED Fe fragment RENDERED on lavapipe at all {} pinned views; the \
         view rode the 3-member broadcast Input struct (span 12); every pixel is tri-equal and the \
         default token is byte-identical to R1. Interactive render params earn R-lava.",
        VIEW_PINS.len()
    );
}

// ===========================================================================
// I2 (interactive controls-in-Fe): the Fe->wasm VIEW CONTROLLER.
//
// `update_view(center_re, center_im, scale_q, dx, dy, dzoom, mx, my) -> (i32,
// i32, i32)` is a pure, stateless, oracle-checkable pan/zoom function compiled
// ONLY to wasm (native MULTI-VALUE return, R2.1). It does pointer-drag pan
// (center += screen-delta scaled to complex space, the image following the
// pointer), cursor-anchored zoom (7/8 per notch in, 9/8 out), and the clamps
// (|center| <= 10240, scale_q in [16, 384]) that ARE the fragment's no-overflow
// contract. `view_init()` returns the default token (-2048, 0, 384).
//
// I2's gate (spec section 4): a deterministic gesture tape (seeded LCG, 10,000
// mixed drag/wheel events) asserting the wasmtime triple equals an INDEPENDENT
// Rust oracle at every step, PLUS directed cases for all four center clamps, both
// scale clamps, the 26-notch descent, and the cursor-anchor property. No GPU.
// ===========================================================================

/// The SSOT I2 fixture: `include_str!`-ed so the tested and (later) shipped source
/// are byte-identical.
const MANDEL_VIEW_CTL_SOURCE: &str = include_str!("fixtures/spirv/mandel_view_ctl.fe");

/// The default view token (spec section 2): center (-0.5, 0) in Q12, scale 384
/// (= 24 Q12-units/px, the proven R1 fixed view). The gesture tape starts here.
/// A `view_init()` export (a constant-tuple return) lands with the I3 page
/// assembly: the R1 const-aggregate return is a distinct lowering gap, and the
/// load-bearing control fn under I2 is `update_view`.
const VIEW_INIT: (i32, i32, i32) = (-2048, 0, 384);

/// The independent Rust twin of `update_view`, re-derived HERE from the pan/zoom
/// semantics (never trusted from the spec), integer-identical to the fixture: the
/// pan follows the pointer (center moves opposite, scaled by step/px), the zoom is
/// 1/8-per-notch, the anchor correction uses the OLD scale minus the NEW clamped
/// scale, and the clamps are applied last. All `>>` are arithmetic i32.
fn update_view_oracle(
    center_re: i32, center_im: i32, scale_q: i32,
    dx: i32, dy: i32, dzoom: i32, mx: i32, my: i32,
) -> (i32, i32, i32) {
    let mut re: i32 = center_re - ((dx * scale_q) >> 4);
    let mut im: i32 = center_im - ((dy * scale_q) >> 4);
    let mut sq: i32 = scale_q;
    if dzoom < 0 {
        sq = scale_q - (scale_q >> 3);
    }
    if dzoom > 0 {
        sq = scale_q + (scale_q >> 3);
    }
    if sq < 16 {
        sq = 16;
    }
    if sq > 384 {
        sq = 384;
    }
    re += ((mx - 256) * (scale_q - sq)) >> 4;
    im += ((my - 256) * (scale_q - sq)) >> 4;
    if re > 10240 {
        re = 10240;
    }
    if re < -10240 {
        re = -10240;
    }
    if im > 10240 {
        im = 10240;
    }
    if im < -10240 {
        im = -10240;
    }
    (re, im, sq)
}

/// Compile the I2 control fixture to wasm through `BackendKind::Wasm`.
fn compile_mandel_view_ctl_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandel_view_ctl_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MANDEL_VIEW_CTL_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("mandel_view_ctl should compile Fe -> wasm");
    let bytes = output.into_bytecode().expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("Fe-emitted control wasm should be valid");
    bytes
}

/// The complex point (in Q12) under a screen cursor for a given view: the fragment's
/// own `center + (((m - 256) * scale_q) >> 4)` map, re-used here to measure zoom
/// anchoring (the cursor-anchor property).
fn point_under_cursor(center: i32, scale_q: i32, m: i32) -> i32 {
    center + (((m - 256) * scale_q) >> 4)
}

/// I2 headline (spec section 4): a deterministic 10,000-event gesture tape (seeded
/// LCG, mixed pan+zoom+anchor) asserts the wasmtime `update_view` triple EQUALS the
/// independent Rust oracle at EVERY step, feeding each reply forward as the next
/// view (the exact broker round-trip). The tape's random walk visits all four
/// center clamps and both scale clamps (asserted for coverage), so the clamp arms
/// are exercised in-band. The 3-tuple reply crosses as a native wasm MULTI-VALUE
/// result (R2.1).
#[test]
fn update_view_matches_oracle_over_gesture_tape() {
    let wasm = compile_mandel_view_ctl_to_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");

    let (ir, ii, isq) = VIEW_INIT;

    // update_view: 8 flattened i32 args -> a 3-value wasm multi-value reply (R2.1).
    let update_view = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32, i32, i32), (i32, i32, i32)>(
            &mut store,
            "update_view",
        )
        .expect("`update_view` export should exist as (i32 x8) -> (i32, i32, i32)");

    // Seeded LCG (Knuth MMIX constants); each step draws pan deltas (-64..63),
    // dzoom in {-1,0,1}, and a cursor (0..511)^2. The reply feeds forward as the
    // next view: exactly the broker's opaque-triple round-trip.
    let mut s: u64 = 0x1234_5678_9abc_def0;
    let (mut cr, mut ci, mut sq) = (ir, ii, isq);
    let (mut hit_re_hi, mut hit_re_lo, mut hit_im_hi, mut hit_im_lo) = (0u32, 0u32, 0u32, 0u32);
    let (mut hit_sq16, mut hit_sq384) = (0u32, 0u32);
    let mut distinct_sq = std::collections::BTreeSet::new();
    const STEPS: usize = 10_000;
    for step in 0..STEPS {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = s;
        let dx = ((r >> 8) & 127) as i32 - 64;
        let dy = ((r >> 16) & 127) as i32 - 64;
        let dzoom = match (r >> 24) & 3 {
            0 => -1,
            1 => 1,
            _ => 0,
        };
        let mx = ((r >> 32) & 511) as i32;
        let my = ((r >> 42) & 511) as i32;

        let got = update_view
            .call(&mut store, (cr, ci, sq, dx, dy, dzoom, mx, my))
            .expect("update_view should run");
        let want = update_view_oracle(cr, ci, sq, dx, dy, dzoom, mx, my);
        assert_eq!(
            got, want,
            "gesture-tape step {step}: wasm update_view({cr},{ci},{sq}; dx={dx},dy={dy},\
             dz={dzoom},mx={mx},my={my}) = {got:?} must equal the Rust oracle {want:?}"
        );

        cr = got.0;
        ci = got.1;
        sq = got.2;
        // Clamp-coverage bookkeeping (the walk must actually visit the boundaries).
        if cr == 10240 {
            hit_re_hi += 1;
        }
        if cr == -10240 {
            hit_re_lo += 1;
        }
        if ci == 10240 {
            hit_im_hi += 1;
        }
        if ci == -10240 {
            hit_im_lo += 1;
        }
        if sq == 16 {
            hit_sq16 += 1;
        }
        if sq == 384 {
            hit_sq384 += 1;
        }
        distinct_sq.insert(sq);
        // The controller's contract must hold at every reply: the clamps are the
        // fragment's no-overflow envelope, so a single out-of-range triple is a bug.
        assert!(
            (-10240..=10240).contains(&cr) && (-10240..=10240).contains(&ci),
            "step {step}: center ({cr},{ci}) escaped the |center| <= 10240 clamp"
        );
        assert!(
            (16..=384).contains(&sq),
            "step {step}: scale_q {sq} escaped the [16, 384] clamp"
        );
    }

    // Coverage: the tape must have exercised EVERY clamp arm in-band (else the
    // in-band clamp branches were never proven equal to the oracle under those
    // conditions). All six are asserted > 0.
    assert!(
        hit_re_hi > 0 && hit_re_lo > 0 && hit_im_hi > 0 && hit_im_lo > 0,
        "the gesture tape must visit all four center clamps (re_hi={hit_re_hi}, re_lo={hit_re_lo}, \
         im_hi={hit_im_hi}, im_lo={hit_im_lo})"
    );
    assert!(
        hit_sq16 > 0 && hit_sq384 > 0,
        "the gesture tape must visit both scale clamps (sq==16: {hit_sq16}, sq==384: {hit_sq384})"
    );
    eprintln!(
        "I2 tape: {STEPS} mixed pan/zoom/anchor events under wasmtime; the update_view triple \
         equals the independent oracle at EVERY step. Clamp coverage: re_hi={hit_re_hi} \
         re_lo={hit_re_lo} im_hi={hit_im_hi} im_lo={hit_im_lo} sq16={hit_sq16} sq384={hit_sq384}; \
         {} distinct scale values.",
        distinct_sq.len()
    );
}

/// I2 directed cases (spec section 4): the named boundary/anchor scenarios, each
/// asserting the wasm `update_view` reply equals the independent oracle AND the
/// specific spec property (the clamp actually clamps; the 26-notch descent lands
/// at the floor; the point under the cursor drifts <= 1 Q12 unit under an anchored
/// zoom). GPU-free.
#[test]
fn update_view_directed_clamps_and_anchor() {
    let wasm = compile_mandel_view_ctl_to_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let update_view = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32, i32, i32), (i32, i32, i32)>(
            &mut store,
            "update_view",
        )
        .expect("`update_view` export should exist as (i32 x8) -> (i32, i32, i32)");

    // A local helper: call the wasm fn, assert it equals the oracle, return the triple.
    let mut call = |cr: i32, ci: i32, sq: i32, dx: i32, dy: i32, dz: i32, mx: i32, my: i32| {
        let got = update_view
            .call(&mut store, (cr, ci, sq, dx, dy, dz, mx, my))
            .expect("update_view should run");
        let want = update_view_oracle(cr, ci, sq, dx, dy, dz, mx, my);
        assert_eq!(
            got, want,
            "update_view({cr},{ci},{sq}; dx={dx},dy={dy},dz={dz},mx={mx},my={my}) = {got:?} must \
             equal the oracle {want:?}"
        );
        got
    };

    // --- (1) All four center clamps. A huge pan at the center cursor (mx=my=256,
    // no anchor drift) drives the center past each boundary; the reply must be
    // pinned exactly at +-10240. dx>0 pans re negative (image follows pointer), so
    // to push re to +10240 use dx<0. ---
    let (re_hi, _, _) = call(10000, 0, 384, -64, 0, 0, 256, 256);
    assert_eq!(re_hi, 10240, "a large +re pan must clamp center_re to +10240");
    let (re_lo, _, _) = call(-10000, 0, 384, 64, 0, 0, 256, 256);
    assert_eq!(re_lo, -10240, "a large -re pan must clamp center_re to -10240");
    let (_, im_hi, _) = call(0, 10000, 384, 0, -64, 0, 256, 256);
    assert_eq!(im_hi, 10240, "a large +im pan must clamp center_im to +10240");
    let (_, im_lo, _) = call(0, -10000, 384, 0, 64, 0, 256, 256);
    assert_eq!(im_lo, -10240, "a large -im pan must clamp center_im to -10240");

    // --- (2) Both scale clamps. Zoom out from 384 stays at 384 (ceiling); zoom in
    // from 16 stays at 16 (floor). ---
    let (_, _, sq_ceiling) = call(0, 0, 384, 0, 0, 1, 256, 256);
    assert_eq!(sq_ceiling, 384, "zoom-out from the ceiling scale 384 must stay clamped at 384");
    let (_, _, sq_floor) = call(0, 0, 16, 0, 0, -1, 256, 256);
    assert_eq!(sq_floor, 16, "zoom-in from the floor scale 16 must stay clamped at 16");

    // --- (3) The 26-notch descent (spec section 2): 26 zoom-in notches at the
    // center cursor (no anchor drift) carry scale_q from 384 down to the floor 16.
    // Each step is oracle-checked inside `call`; here we also pin the count. ---
    let mut sq = 384;
    let mut notches = 0;
    while sq > 16 {
        let (_, _, nsq) = call(0, 0, sq, 0, 0, -1, 256, 256);
        assert!(nsq < sq || nsq == 16, "each zoom-in notch must decrease scale_q (or clamp to 16)");
        sq = nsq;
        notches += 1;
        assert!(notches <= 40, "the descent must terminate at the floor well within 40 notches");
    }
    assert_eq!(sq, 16, "the notch descent must land exactly on the floor scale 16");
    assert_eq!(
        notches, 26,
        "the 7/8-per-notch descent from 384 must reach the floor 16 in exactly 26 notches (spec 2)"
    );

    // --- (4) The cursor-anchor property (spec section 4): under an anchored zoom
    // at a view away from the clamps, the complex point under the cursor drifts by
    // <= 1 Q12 unit in each axis (exact up to the >>4 truncation). Use a mid view
    // and an off-center cursor. ---
    for &(cr0, ci0, sq0, mx, my) in &[
        (0, 0, 128, 400, 100),
        (1024, -512, 256, 40, 500),
        (-3072, 410, 96, 300, 220),
    ] {
        // Point under the cursor BEFORE the zoom.
        let p_re0 = point_under_cursor(cr0, sq0, mx);
        let p_im0 = point_under_cursor(ci0, sq0, my);
        // Anchored zoom in (dzoom < 0), no pan.
        let (cr1, ci1, sq1) = call(cr0, ci0, sq0, 0, 0, -1, mx, my);
        // Point under the SAME cursor AFTER.
        let p_re1 = point_under_cursor(cr1, sq1, mx);
        let p_im1 = point_under_cursor(ci1, sq1, my);
        assert!(
            (p_re1 - p_re0).abs() <= 1 && (p_im1 - p_im0).abs() <= 1,
            "cursor-anchor: at view ({cr0},{ci0},{sq0}) cursor ({mx},{my}), the point under the \
             cursor drifted ({},{}) Q12 units on an anchored zoom; must be <= 1 each",
            (p_re1 - p_re0).abs(),
            (p_im1 - p_im0).abs()
        );
    }

    eprintln!(
        "I2 directed: all four center clamps pin at +-10240; both scale clamps hold at 16/384; the \
         26-notch descent lands exactly on the floor 16; the cursor-anchored zoom keeps the point \
         under the cursor within 1 Q12 unit. wasm == oracle in every case."
    );
}

// ===========================================================================
// C3 (clifford ladder rung 3, renderer-in-Fe): the Cl(3) rotor sandwich as a
// RENDER FRAGMENT, and its interactive ROTOR CONTROLLER.
//
// C3a: `clifford_frag_rgba` (the C1/C2 sandwich body VERBATIM, returning a packed
// RGBA8 word instead of a bare shade) compiles through the Render seam and RENDERS
// on lavapipe at the pinned rotors, tri-equal (texture == oracle == wasm leg). The
// FOUR rotor components ride the render broadcast Input struct (members p0..p3,
// span 16), exactly the four-member path C2 proved on the grid, now on the render
// arm. C3b: `update_rotor` (the Fe->wasm rotor controller) matches its independent
// Rust oracle over a gesture tape (native 4-value multi-return).
// ===========================================================================

/// The SSOT C3 fragment fixture: `include_str!`-ed here and (later) by the page
/// generator, so tested and shipped source are byte-identical.
const CLIFFORD_FRAG_RGBA_SOURCE: &str = include_str!("fixtures/spirv/clifford_frag_rgba.fe");
/// The SSOT C3 control fixture (the pan/drag `update_rotor` fn).
const CLIFFORD_CTL_SOURCE: &str = include_str!("fixtures/spirv/clifford_ctl.fe");

/// The independent RGBA oracle for `clifford_frag_rgba`, re-derived HERE from the
/// kernel logic: the C1 rotor sandwich + shade (reusing `clifford_sandwich_q12` and
/// `clifford_shade_q12`, the twice-written oracle already proven integer-identical
/// to the kernel), then the SAME pure-i32 packing the fragment uses (R=G=shade,
/// B=255-shade, A=255; alpha folded in as `- 16777216`). The returned i32 word's
/// bit pattern IS the little-endian RGBA8; `as u32` reinterprets it for the
/// byte-wise comparison (to_le_bytes = [R,G,B,A]), no arithmetic change.
fn clifford_frag_rgba_oracle(px: i32, py: i32, rc: i32, r12: i32, r13: i32, r23: i32) -> u32 {
    let (sx, sy, sz) = clifford_sandwich_q12(px, py, rc, r12, r13, r23);
    let shade = clifford_shade_q12(sx, sy, sz); // 0..255
    let packed: i32 = shade + shade * 256 + (255 - shade) * 65536 - 16_777_216;
    packed as u32
}

/// The pinned rotors the C3 render/wasm legs use, each `(name, rc, r12, r13, r23,
/// min_distinct)`. A pure-e12 rotor (identity, e12_90) fixes the e3 slab height, so
/// the depth cue is constant -> a flat two-tone checker -> exactly 2 packed colors;
/// only the tilted rotor tumbles the slab in 3D and spreads the shades. The floors
/// are DERIVED (not baked): >= 2 for the flat rotors (a one-color image still
/// fails), >= 8 for the tilted 3D tumble.
const CLIFFORD_RGBA_PINS: [(&str, i32, i32, i32, i32, usize); 4] = [
    ("identity", 4096, 0, 0, 0, 2),
    ("e12_90", 2896, 2896, 0, 0, 2),
    ("tilted_default", 3712, 577, 1154, 1154, 8),
    ("e12_180", 0, 4096, 0, 0, 2),
];

/// Compile the C3 fragment to wasm through `BackendKind::Wasm`. On the wasm path it
/// is an ordinary `(i32 x6) -> i32` export (Fe `u32` lowers to wasm `i32`); calling
/// it per pixel is the wasm leg of the tri-equal AND the browser AMBER leg.
fn compile_clifford_frag_rgba_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///clifford_frag_rgba_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(CLIFFORD_FRAG_RGBA_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("clifford_frag_rgba should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// Run the C3 fragment (the 6-arg typed func) over the FULL 512x512 grid for one
/// pinned rotor, returning the per-pixel packed RGBA8 grid (row-major, u32).
fn wasm_clifford_frag_grid_all(bytes: &[u8], rc: i32, r12: i32, r13: i32, r23: i32) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut store, "clifford_frag_rgba")
        .expect("`clifford_frag_rgba` export should exist as (i32 x6) -> i32");
    let mut out = Vec::with_capacity((FRAG_W * FRAG_H) as usize);
    for py in 0..FRAG_H as i32 {
        for px in 0..FRAG_W as i32 {
            let v = f
                .call(&mut store, (px, py, rc, r12, r13, r23))
                .expect("clifford_frag_rgba should run") as u32;
            out.push(v);
        }
    }
    out
}

/// C3a wasm leg (GPU-FREE): compile `clifford_frag_rgba` via `BackendKind::Wasm`,
/// execute under wasmtime over the FULL 512x512 grid for each pinned rotor, and
/// assert every packed-RGBA word equals the independent oracle. Proves the
/// signed-sandwich -> u32 color map (the `as u32` bridge + the branchless clamp +
/// the RGBA packing) computes correctly WITHOUT any GPU. Identity/e12_90 are
/// additionally pinned to exactly two packed colors (the flat two-tone checker).
#[test]
fn clifford_frag_rgba_wasm_leg() {
    let bytes = compile_clifford_frag_rgba_to_wasm();
    wasmparser::validate(&bytes).expect("Fe-emitted clifford frag wasm should be valid");

    for (name, rc, r12, r13, r23, min_distinct) in CLIFFORD_RGBA_PINS {
        let grid = wasm_clifford_frag_grid_all(&bytes, rc, r12, r13, r23);
        let mut distinct = std::collections::HashSet::new();
        for py in 0..FRAG_H {
            for px in 0..FRAG_W {
                let idx = (py * FRAG_W + px) as usize;
                let got = grid[idx];
                let want = clifford_frag_rgba_oracle(px as i32, py as i32, rc, r12, r13, r23);
                assert_eq!(
                    got, want,
                    "wasm clifford_frag_rgba({px},{py}; {name}) = 0x{got:08X} must equal the oracle \
                     0x{want:08X}"
                );
                // Alpha is fully opaque for every pixel (byte 3 == 0xFF).
                assert_eq!(
                    got >> 24,
                    255,
                    "pixel ({px},{py}; {name}) must be opaque (alpha 255); got 0x{got:08X}"
                );
                distinct.insert(got);
            }
        }
        assert!(
            distinct.len() >= min_distinct,
            "{name}: the packed-RGBA image must have >= {min_distinct} distinct colors (got {})",
            distinct.len()
        );
        match name {
            "identity" | "e12_90" | "e12_180" => assert_eq!(
                distinct.len(),
                2,
                "{name}: a pure-e12 rotor fixes the slab depth -> exactly 2 packed colors, got {}",
                distinct.len()
            ),
            "tilted_default" => assert!(
                distinct.len() >= 8,
                "tilted_default: the 3D tumble's depth cue must spread the colors (got {})",
                distinct.len()
            ),
            _ => unreachable!("unexpected pinned rotor {name}"),
        }
        eprintln!(
            "C3a wasm leg [{name} = ({rc},{r12},{r13},{r23})]: ALL 262,144 packed-RGBA words == the \
             independent oracle; {} distinct colors, all opaque.",
            distinct.len()
        );
    }
}

/// C3a compile (GPU-FREE): the C3 fragment compiles through the Render seam into
/// ONE naga-validated SPIR-V module with TWO entry points, states its render ABI,
/// and its browser-profile WGSL carries the render epilogue AND the FOUR-member
/// broadcast rotor load (`input.p0`..`input.p3`, span 16). Straight-line branchless
/// (no `loop`, no structurizer conditional), signed sandwich (`bitcast<i32>`).
#[test]
fn clifford_frag_rgba_compiles_to_render_spirv() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///clifford_frag_rgba_render.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(CLIFFORD_FRAG_RGBA_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("clifford_frag_rgba should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("clifford_frag_rgba should compile Fe -> naga-validated SPIR-V in Render mode");

    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render,
        "the render driver seam must state LayoutMode::Render"
    );
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the render fragment must lower to the u32 word (browser profile)"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Render mode has no single-slot result: the color target is the result"
    );
    assert_eq!(
        artifact.layout.vertex_entry.as_deref(),
        Some("vs_fullscreen"),
        "Render mode states the @vertex entry name"
    );
    assert_eq!(
        artifact.layout.fragment_entry.as_deref(),
        Some("fs_main"),
        "Render mode states the @fragment entry name"
    );
    assert_eq!(
        count_spirv_entry_points(&artifact.words),
        2,
        "one Render SPIR-V module must carry BOTH entry points (@vertex + @fragment)"
    );

    // The FOUR-member broadcast rotor: Input stride 16 is the static proof that args
    // 2..5 (rc, r12, r13, r23) became broadcast members p0,p1,p2,p3 at 0,4,8,12.
    let input_stride = artifact
        .layout
        .bindings
        .iter()
        .find(|b| b.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("the render layout must have an Input binding")
        .stride;
    assert_eq!(
        input_stride, 16,
        "the FOUR broadcast rotor members (rc, r12, r13, r23), 4 bytes each, span 16 bytes: the \
         layout-level proof that the fragment's args 2..5 became the render broadcast rotor"
    );

    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the render fragment");
    assert_browser_profile_wgsl(wgsl);
    assert!(
        wgsl.contains("@vertex") && wgsl.contains("@fragment"),
        "render WGSL must contain BOTH @vertex and @fragment stages; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("@location(0)") && wgsl.contains("unpack4x8unorm"),
        "the render epilogue must write @location(0) via unpack4x8unorm; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("bitcast<i32>"),
        "the signed rotor sandwich must round-trip through bitcast<i32> (Sar/Slt on i32); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("input.p0") && wgsl.contains("input.p3"),
        "the fragment must load the FIRST and FOURTH broadcast rotor members (input.p0 = rc, \
         input.p3 = r23); all four are read; got:\n{wgsl}"
    );
    eprintln!(
        "C3a-val: Fe clifford_frag_rgba compiled -> ONE Render SPIR-V module, 2 entry points, Input \
         stride 16 (4 broadcast rotor members), WGSL with unpack4x8unorm + bitcast<i32> + \
         input.p0..input.p3. {} SPIR-V words.",
        artifact.words.len()
    );
}

/// C3a headline: the Cl(3) rotor sandwich RENDERS on lavapipe at the browser
/// profile, and at EACH pinned rotor every one of 262,144 pixels x 4 bytes is
/// TRI-EQUAL (texture == `clifford_frag_rgba_oracle` == the wasm execution), with
/// the rotor delivered as four broadcast words written to the Input buffer before
/// the draw. Hard-fail-not-skip; `MB2_ALLOW_GPU_SKIP` only, adapter printed. The
/// name contains "lavapipe" so the nextest serial group filter catches it.
#[test]
fn clifford_frag_rgba_renders_on_lavapipe_browser_profile() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///clifford_frag_rgba_lavapipe.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(CLIFFORD_FRAG_RGBA_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("clifford_frag_rgba should build a wasm runtime package");

    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("clifford_frag_rgba should compile Fe -> naga-validated SPIR-V in Render mode");
    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Render,
        "the render driver seam must state LayoutMode::Render"
    );
    assert_eq!(
        count_spirv_entry_points(&artifact.words),
        2,
        "one Render SPIR-V module must carry BOTH entry points"
    );
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the render fragment");
    assert_browser_profile_wgsl(wgsl);

    let wasm_bytes = compile_clifford_frag_rgba_to_wasm();

    for (name, rc, r12, r13, r23, min_distinct) in CLIFFORD_RGBA_PINS {
        let wasm_colors = wasm_clifford_frag_grid_all(&wasm_bytes, rc, r12, r13, r23);
        // The rotor words, in kernel-arg order: arg2=rc -> p0, arg3=r12 -> p1,
        // arg4=r13 -> p2, arg5=r23 -> p3 (i32 two's complement in the u32 word).
        let params: [u32; 4] = [rc as u32, r12 as u32, r13 as u32, r23 as u32];
        let input_bytes: Vec<u8> = params.iter().flat_map(|p| p.to_le_bytes()).collect();
        match run_render_rgba8_on_lavapipe(wgsl, FRAG_W, FRAG_H, &input_bytes) {
            Some(rgba) => {
                assert_eq!(
                    rgba.len(),
                    (FRAG_W * FRAG_H * 4) as usize,
                    "render readback must be 512*512*4 = 1048576 bytes (tightly packed)"
                );
                let mut distinct = std::collections::HashSet::new();
                for y in 0..FRAG_H {
                    for x in 0..FRAG_W {
                        let idx = (y * FRAG_W + x) as usize;
                        let px = &rgba[idx * 4..idx * 4 + 4];
                        let oracle = clifford_frag_rgba_oracle(x as i32, y as i32, rc, r12, r13, r23);
                        let oracle_bytes = oracle.to_le_bytes();
                        let wasm_bytes_px = wasm_colors[idx].to_le_bytes();
                        assert_eq!(
                            px, &oracle_bytes,
                            "lavapipe rendered pixel ({x},{y}) [rotor {name}] RGBA {px:?} must equal \
                             the oracle color {oracle_bytes:?} (packed 0x{oracle:08X})"
                        );
                        assert_eq!(
                            px, &wasm_bytes_px,
                            "lavapipe rendered pixel ({x},{y}) [rotor {name}] RGBA {px:?} must equal \
                             the wasm leg color {wasm_bytes_px:?} for the same (x,y)"
                        );
                        distinct.insert(oracle);
                    }
                }
                assert!(
                    distinct.len() >= min_distinct,
                    "rotor {name}: rendered color histogram must have >= {min_distinct} distinct \
                     colors (got {}); a degenerate/transposed image could not pass the per-(x,y) \
                     tri-equal either",
                    distinct.len()
                );
                eprintln!(
                    "C3a [{name} = ({rc},{r12},{r13},{r23})]: Fe clifford_frag_rgba RENDERED on \
                     lavapipe (browser profile, 512x512) with the rotor as 4 broadcast params; ALL \
                     262,144 pixels TRI-EQUAL (texture == oracle == wasm); {} distinct colors.",
                    distinct.len()
                );
            }
            None => {
                eprintln!(
                    "R-val only [{name}]: render SPIR-V validated but NOT executed (GPU skipped via \
                     MB2_ALLOW_GPU_SKIP). The clifford render tri-equal claim is NOT earned."
                );
                return;
            }
        }
    }

    eprintln!(
        "C3a: the Cl(3) rotor sandwich RENDERED on lavapipe at all {} pinned rotors; the rotor rode \
         the 4-member broadcast Input struct (span 16); every pixel is tri-equal. Interactive render \
         rotors earn R-lava.",
        CLIFFORD_RGBA_PINS.len()
    );
}

/// The initial rotor the page seeds (a gentle 3D tilt, so the opening image reads
/// as a tumbled checker, not a flat grid). Emitted into `ctl.json` as data.
const ROTOR_INIT: (i32, i32, i32, i32) = (3712, 577, 1154, 1154);

/// The independent Rust twin of `update_rotor`, re-derived HERE from the rotor
/// composition (never trusted from the fixture), integer-identical: the yaw rotor
/// (e12 plane, driven by sign(dx)) then the pitch rotor (e13 plane, sign(dy)), each
/// the Pythagorean small rotor (4095/128) composed by geometric product then `>>
/// 12` (a no-drag axis uses cosine 4096 = exact identity), and the [-8192, 8192]
/// component clamp. All `>>` are arithmetic i32 (Sar), matching Fe.
fn update_rotor_oracle(
    rc: i32, r12: i32, r13: i32, r23: i32, dx: i32, dy: i32,
) -> (i32, i32, i32, i32) {
    let is_neg = |d: i32| if d < 0 { 1 } else { 0 };
    let is_pos = |d: i32| if d > 0 { 1 } else { 0 };
    let dir_sin = |d: i32| (is_neg(d) - is_pos(d)) * 128;
    let dir_cos = |d: i32| 4096 - (is_neg(d) + is_pos(d));
    let clamp_comp = |c: i32| c.clamp(-8192, 8192);

    let c0y = dir_cos(dx);
    let sy0 = dir_sin(dx);
    let rc1 = (c0y * rc - sy0 * r12) >> 12;
    let r121 = (sy0 * rc + c0y * r12) >> 12;
    let r131 = (c0y * r13 + sy0 * r23) >> 12;
    let r231 = (c0y * r23 - sy0 * r13) >> 12;

    let c0p = dir_cos(dy);
    let sp0 = dir_sin(dy);
    let rc2 = (c0p * rc1 - sp0 * r131) >> 12;
    let r122 = (c0p * r121 - sp0 * r231) >> 12;
    let r132 = (sp0 * rc1 + c0p * r131) >> 12;
    let r232 = (c0p * r231 + sp0 * r121) >> 12;

    (
        clamp_comp(rc2),
        clamp_comp(r122),
        clamp_comp(r132),
        clamp_comp(r232),
    )
}

/// Integer squared magnitude of a rotor (fits in i64; each component <= 8192 so
/// the sum <= 4 * 8192^2 = 268M).
fn rotor_norm_sq(r: (i32, i32, i32, i32)) -> i64 {
    (r.0 as i64).pow(2) + (r.1 as i64).pow(2) + (r.2 as i64).pow(2) + (r.3 as i64).pow(2)
}

/// Compile the C3 control fixture to wasm through `BackendKind::Wasm`.
fn compile_clifford_ctl_to_wasm() -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///clifford_ctl_wasm.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(CLIFFORD_CTL_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("clifford_ctl should compile Fe -> wasm");
    let bytes = output.into_bytecode().expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("Fe-emitted control wasm should be valid");
    bytes
}

/// C3b headline: a deterministic 10,000-event gesture tape (seeded LCG, mixed
/// horizontal/vertical drag deltas) asserts the wasmtime `update_rotor` 4-tuple
/// EQUALS the independent Rust oracle at EVERY step, feeding each reply forward as
/// the next rotor (the exact broker round-trip). The 4-value reply crosses as a
/// native wasm MULTI-VALUE result. Additionally: a no-drag event is the EXACT
/// identity (no rotor drift), and the rotor magnitude stays bounded near unit.
#[test]
fn update_rotor_matches_oracle_over_gesture_tape() {
    let wasm = compile_clifford_ctl_to_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");

    // update_rotor: 6 flattened i32 args -> a 4-value wasm multi-value reply.
    let update_rotor = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32), (i32, i32, i32, i32)>(
            &mut store,
            "update_rotor",
        )
        .expect("`update_rotor` export should exist as (i32 x6) -> (i32, i32, i32, i32)");

    // --- No-drag event is the EXACT identity (the c0(0)=4096 fix): a pump firing
    // on a zero-movement pointer event must NOT shrink or perturb the rotor. ---
    for &r in &[
        (4096, 0, 0, 0),
        ROTOR_INIT,
        (2896, 2896, 0, 0),
        (1000, -2000, 3000, -500),
    ] {
        let got = update_rotor
            .call(&mut store, (r.0, r.1, r.2, r.3, 0, 0))
            .expect("update_rotor should run");
        assert_eq!(
            got, r,
            "a no-drag event (dx=dy=0) must be the exact identity on rotor {r:?}; got {got:?}"
        );
        assert_eq!(got, update_rotor_oracle(r.0, r.1, r.2, r.3, 0, 0));
    }

    // --- The gesture tape: seeded LCG, feed replies forward. ---
    let mut s: u64 = 0x0f1e_2d3c_4b5a_6978;
    let (ir, i12, i13, i23) = ROTOR_INIT;
    let (mut rc, mut r12, mut r13, mut r23) = (ir, i12, i13, i23);
    let mut norm_min = i64::MAX;
    let mut norm_max = i64::MIN;
    let mut hit_clamp = 0u32;
    const STEPS: usize = 10_000;
    for step in 0..STEPS {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = s;
        // Drag deltas in -32..31 (pointer movementX/Y), often zero on one axis.
        let dx = ((r >> 8) & 63) as i32 - 32;
        let dy = ((r >> 20) & 63) as i32 - 32;

        let got = update_rotor
            .call(&mut store, (rc, r12, r13, r23, dx, dy))
            .expect("update_rotor should run");
        let want = update_rotor_oracle(rc, r12, r13, r23, dx, dy);
        assert_eq!(
            got, want,
            "gesture-tape step {step}: wasm update_rotor({rc},{r12},{r13},{r23}; dx={dx},dy={dy}) = \
             {got:?} must equal the Rust oracle {want:?}"
        );

        rc = got.0;
        r12 = got.1;
        r13 = got.2;
        r23 = got.3;
        // The component clamp is the contract: no component ever escapes [-8192, 8192].
        for c in [rc, r12, r13, r23] {
            assert!(
                (-8192..=8192).contains(&c),
                "step {step}: rotor component {c} escaped the [-8192, 8192] clamp"
            );
            if c == 8192 || c == -8192 {
                hit_clamp += 1;
            }
        }
        let n = rotor_norm_sq((rc, r12, r13, r23));
        norm_min = norm_min.min(n);
        norm_max = norm_max.max(n);
    }

    // The rotor stays a well-conditioned rotor: never collapses toward zero (a
    // degenerate rotor would make the sandwich vanish) and never explodes. The
    // Pythagorean small rotor + the c0(0)=4096 identity keep |R| near the Q12 unit
    // (4096^2 = 16,777,216); the walk breathes but stays within a factor of ~4x in
    // norm-squared (a factor ~2 in magnitude), well inside the clamp.
    let unit_sq: i64 = 4096 * 4096;
    assert!(
        norm_min > unit_sq / 4,
        "the rotor must never collapse (norm^2 min {norm_min} must stay > unit/4 = {})",
        unit_sq / 4
    );
    assert!(
        norm_max < unit_sq * 16,
        "the rotor must never explode (norm^2 max {norm_max} must stay < 16*unit = {})",
        unit_sq * 16
    );
    eprintln!(
        "C3b tape: {STEPS} mixed drag events under wasmtime; the update_rotor 4-tuple equals the \
         independent oracle at EVERY step (native 4-value multi-return). No-drag events are the \
         exact identity; rotor norm^2 stayed in [{norm_min}, {norm_max}] (unit = {unit_sq}); clamp \
         touched {hit_clamp} times."
    );
}

/// C3b directed: a single yaw drag rotates the rotor in the e12 plane while keeping
/// the scalar/bivector magnitude near unit, and a large accumulated same-direction
/// drag is pinned by the component clamp (never overflows the sandwich envelope).
#[test]
fn update_rotor_directed_rotation_and_clamp() {
    let wasm = compile_clifford_ctl_to_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let update_rotor = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32), (i32, i32, i32, i32)>(
            &mut store,
            "update_rotor",
        )
        .expect("`update_rotor` export should exist as (i32 x6) -> (i32, i32, i32, i32)");

    let mut call = |rc: i32, r12: i32, r13: i32, r23: i32, dx: i32, dy: i32| {
        let got = update_rotor
            .call(&mut store, (rc, r12, r13, r23, dx, dy))
            .expect("update_rotor should run");
        let want = update_rotor_oracle(rc, r12, r13, r23, dx, dy);
        assert_eq!(
            got, want,
            "update_rotor({rc},{r12},{r13},{r23}; dx={dx},dy={dy}) = {got:?} must equal the oracle \
             {want:?}"
        );
        got
    };

    // --- (1) A single yaw drag from the identity rotor rotates in the e12 plane:
    // r12 becomes nonzero, the scalar drops from 4096, and the magnitude stays
    // within ~1% of the Q12 unit. dx < 0 spins one way, dx > 0 the other. ---
    let unit_sq: i64 = 4096 * 4096;
    let pos = call(4096, 0, 0, 0, 8, 0);
    assert!(pos.1 < 0, "dx>0 must rotate the identity rotor to a NEGATIVE r12; got {pos:?}");
    let neg = call(4096, 0, 0, 0, -8, 0);
    assert!(neg.1 > 0, "dx<0 must rotate the identity rotor to a POSITIVE r12; got {neg:?}");
    for r in [pos, neg] {
        let n = rotor_norm_sq(r);
        assert!(
            (n - unit_sq).abs() < unit_sq / 50,
            "a single yaw step must keep the rotor within ~2% of unit norm^2 ({unit_sq}); got {n}"
        );
        // The e13/e23 plane is untouched by a pure-yaw (dy=0) step.
        assert_eq!((r.2, r.3), (0, 0), "a pure-yaw step must leave r13, r23 at zero; got {r:?}");
    }

    // --- (2) A pitch drag rotates in the e13 plane (r13 becomes nonzero). ---
    let pitch = call(4096, 0, 0, 0, 0, 8);
    assert!(pitch.2 < 0, "dy>0 must rotate the identity rotor to a NEGATIVE r13; got {pitch:?}");
    assert_eq!((pitch.1, pitch.3), (0, 0), "a pure-pitch step must leave r12, r23 at zero; got {pitch:?}");

    // --- (3) The component clamp holds: a long same-direction drag grows the rotor
    // toward the clamp, and every component is pinned in [-8192, 8192] (the
    // sandwich's no-overflow floor). Feed replies forward for many steps. ---
    let (mut rc, mut r12, mut r13, mut r23) = (8000, 8000, 8000, 8000);
    for _ in 0..64 {
        let got = call(rc, r12, r13, r23, -20, -20);
        rc = got.0;
        r12 = got.1;
        r13 = got.2;
        r23 = got.3;
        for c in [rc, r12, r13, r23] {
            assert!(
                (-8192..=8192).contains(&c),
                "the component clamp must pin every component in [-8192, 8192]; got {c}"
            );
        }
    }

    eprintln!(
        "C3b directed: a yaw drag rotates the identity rotor in the e12 plane (sign(dx) picks the \
         direction) keeping |R| within ~2% of unit; a pitch drag rotates in e13; the component \
         clamp pins a runaway drag inside [-8192, 8192]. wasm == oracle in every case."
    );
}

// ===========================================================================
// MSM-P / MSM-0a: the first crypto-on-GPU rung. A fully-unrolled, single-
// function, BRANCHLESS field multiply-mod-p (BN254 scalar field Fr, the zk-SNARK
// proving hot loop's inner kernel), 13-bit x 20 limbs in u32 words, ZERO new ops
// (Add / Sub / Mul / Shr-by-literal only). The kernel computes the CIOS
// Montgomery product a*b*R^-1 mod p; the INDEPENDENT gate oracle is num-bigint's
// (a*b*R^-1 mod p) (p prime => R^-1 = R^(p-2) mod p by Fermat). Tri-equal:
// lavapipe (SPIR-V) == wasmtime (wasm) == num-bigint oracle, at every operand
// including carry-heavy p-1 and dense-limb cases.
//
// MSM-P (field_mul_probe.fe) is the lowering de-risk probe: the SAME idioms mod a
// 51-bit prime, 4 limbs, ~192 statements. MSM-0a (field_mul_bn254_fr.fe) is the
// real 254-bit BN254 Fr, 20 limbs, ~2880 statements. Both fixtures are GENERATED
// (scratchpad/gen_field_mul.py) from one CIOS engine that is validated numeric-
// ally against the bigint oracle before emission, so the Fe source and the
// reference algorithm cannot drift.
// ===========================================================================

use num_bigint::BigUint;

const FIELD_MUL_PROBE_SOURCE: &str = include_str!("fixtures/spirv/field_mul_probe.fe");
const FIELD_MUL_BN254_FR_SOURCE: &str = include_str!("fixtures/spirv/field_mul_bn254_fr.fe");

/// 13-bit limbs (base 2^13). Products of two limbs are 26-bit; a CIOS column
/// step stays < 2^27, so all arithmetic fits u32 with no mul-hi and no u64.
const MSM_LIMB_BITS: usize = 13;

/// The MSM-P probe modulus: a 51-bit prime (R = 2^52 = B^4).
fn probe_prime() -> BigUint {
    BigUint::from(2_251_799_813_685_119u64)
}

/// BN254 (alt_bn128) scalar field order Fr, the 254-bit prime SNARK scalars live
/// in (R = 2^260 = B^20). Parsed from decimal here, never trusted from a limb
/// table, so the oracle is anchored to the canonical curve constant.
fn bn254_fr_prime() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .expect("BN254 Fr decimal should parse")
}

/// Decompose a field element into `n` little-endian 13-bit limbs (u32 words).
fn msm_to_limbs(x: &BigUint, n: usize) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..n)
        .map(|j| {
            let limb = (x >> (MSM_LIMB_BITS * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

/// The INDEPENDENT bigint oracle: the CIOS Montgomery product a*b*R^-1 mod p,
/// computed with num-bigint (which knows nothing of 13-bit limbs or CIOS), then
/// decomposed into `n` limbs for a limb-for-limb match against the kernel. R^-1
/// is R^(p-2) mod p (Fermat; both moduli are prime).
fn mont_oracle_limbs(a: &BigUint, b: &BigUint, p: &BigUint, n: usize) -> Vec<u32> {
    let r = BigUint::from(1u32) << (MSM_LIMB_BITS * n);
    let rinv = r.modpow(&(p - BigUint::from(2u32)), p);
    let mont = (((a * b) % p) * &rinv) % p;
    msm_to_limbs(&mont, n)
}

/// The operand set: canonical edge cases (0, 1, 2, p-1, p-2, (p-1)/2), the
/// carry-heavy dense-limb value (every 13-bit limb saturated, reduced mod p), the
/// Montgomery anchors R and R^2 mod p, and deterministic pseudo-random elements
/// (xorshift, no rand dependency). Every ordered pair (a, b) is a test product.
fn msm_operands(p: &BigUint, n: usize) -> Vec<(String, BigUint)> {
    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    let mut v: Vec<(String, BigUint)> = vec![
        ("0".into(), BigUint::from(0u32)),
        ("1".into(), one.clone()),
        ("2".into(), two.clone()),
        ("p-1".into(), p - &one),
        ("p-2".into(), p - &two),
        ("(p-1)/2".into(), (p - &one) / &two),
    ];
    let mut dense = BigUint::from(0u32);
    for j in 0..n {
        dense |= BigUint::from(8191u32) << (MSM_LIMB_BITS * j);
    }
    v.push(("dense".into(), &dense % p));
    let r = BigUint::from(1u32) << (MSM_LIMB_BITS * n);
    v.push(("R".into(), &r % p));
    v.push(("R^2".into(), (&r * &r) % p));
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    for idx in 0..3 {
        let mut x = BigUint::from(0u32);
        for _ in 0..(MSM_LIMB_BITS * n / 64 + 1) {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            x = (x << 64) | BigUint::from(s);
        }
        v.push((format!("rand{idx}"), x % p));
    }
    v
}

/// Compile a Fe source to wasm bytecode through `BackendKind::Wasm`.
fn compile_source_to_wasm(source: &str, tag: &str) -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{tag}_wasm.fe")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("field-mul kernel should compile Fe -> wasm");
    output.into_bytecode().expect("wasm output should be bytecode")
}

/// Execute the wasm field-mul over all `n` limb indices (arg0 = k = limb index)
/// for a single (a, b), returning the `n` product limbs. The kernel takes
/// `2 + 2n` args, past wasmtime's typed-tuple arity, so the untyped `Func::call`
/// path is used.
fn wasm_field_mul_limbs(
    bytes: &[u8],
    fn_name: &str,
    a_limbs: &[u32],
    b_limbs: &[u32],
    n: usize,
) -> Vec<u32> {
    use wasmtime::{Engine, Instance, Module, Store, Val};
    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = Store::new(&engine, ());
    let instance =
        Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_func(&mut store, fn_name)
        .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut params: Vec<Val> = Vec::with_capacity(2 + 2 * n);
        params.push(Val::I32(k as i32));
        params.push(Val::I32(0));
        for &l in a_limbs {
            params.push(Val::I32(l as i32));
        }
        for &l in b_limbs {
            params.push(Val::I32(l as i32));
        }
        let mut results = [Val::I32(0)];
        f.call(&mut store, &params, &mut results)
            .unwrap_or_else(|e| panic!("{fn_name}(k={k}, ...) should run: {e:?}"));
        let limb = match results[0] {
            Val::I32(v) => v as u32,
            other => panic!("{fn_name} result must be i32, got {other:?}"),
        };
        out.push(limb);
    }
    out
}

/// Execute a grid field-mul kernel on lavapipe (software Vulkan) at the browser
/// profile (NO required features), once per broadcast param-set, reusing ONE
/// device + pipeline (the kernel WGSL compiles once; only the input buffer
/// changes per operand pair). Returns one `width*height` grid per param-set.
///
/// ANTI-FUDGE (verbatim discipline from the grid harness): a missing adapter or
/// device is a HARD FAILURE, never a silent skip; the only escape is
/// `MB2_ALLOW_GPU_SKIP`, which downgrades the whole batch to `None`.
fn run_grid_batches_on_lavapipe(
    wgsl: &str,
    width: u32,
    height: u32,
    param_sets: &[Vec<u32>],
    label: &str,
) -> Option<Vec<Vec<u32>>> {
    assert!(
        width % 8 == 0 && height % 8 == 0,
        "grid frame {width}x{height} must be a multiple of the 8x8 workgroup size"
    );
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();
    let out_bytes = u64::from(width * height * 4);
    let param_len = param_sets.first().map_or(0, |p| p.len());
    let input_bytes = std::cmp::max(4u64, 4 * param_len as u64);

    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(
        &wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            ..Default::default()
        },
    )) {
        Ok(a) => a,
        Err(e) => {
            if allow_skip {
                eprintln!("  {label} SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): no Vulkan adapter: {e:?}");
                return None;
            }
            panic!(
                "{label} SPIR-V leg: no GPU/Vulkan adapter available ({e:?}). This crypto rung \
                 requires lavapipe to EXECUTE; a missing device is a hard failure, not a skip. Set \
                 VK_ICD_FILENAMES / LD_LIBRARY_PATH / WGPU_BACKEND=vulkan for lavapipe, or \
                 MB2_ALLOW_GPU_SKIP to downgrade on a genuinely GPU-less host."
            );
        }
    };
    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        },
    )) {
        Ok(dq) => dq,
        Err(e) => {
            if allow_skip {
                eprintln!("  {label} SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {e:?}");
                return None;
            }
            panic!(
                "{label} SPIR-V leg: browser-profile device request (NO required features) failed \
                 ({e:?}). This is a hard failure, not a skip."
            );
        }
    };
    eprintln!(
        "  {label} SPIR-V leg GPU adapter (BROWSER PROFILE, no required features): {}",
        adapter.get_info().name
    );

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("msm_output"),
        size: out_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("msm_input"),
        size: input_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("msm_staging"),
        size: out_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("msm_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("msm_pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("msm_pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("msm_bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: output_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input_buf.as_entire_binding(),
            },
        ],
    });

    let mut grids = Vec::with_capacity(param_sets.len());
    for params in param_sets {
        if !params.is_empty() {
            let bytes: Vec<u8> = params.iter().flat_map(|p| p.to_le_bytes()).collect();
            queue.write_buffer(&input_buf, 0, &bytes);
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(width / 8, height / 8, 1);
        }
        encoder.copy_buffer_to_buffer(&output_buf, 0, &staging_buf, 0, out_bytes);
        queue.submit(Some(encoder.finish()));

        let slice = staging_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).expect("map_async callback channel should be open");
        });
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        });
        rx.recv()
            .expect("map_async callback should fire")
            .expect("staging buffer should map for read");
        let data = slice.get_mapped_range();
        let grid: Vec<u32> = data
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes per u32")))
            .collect();
        drop(data);
        staging_buf.unmap();
        grids.push(grid);
    }
    Some(grids)
}

/// The GPU-FREE anchor: the Fe field-mul, compiled to wasm and executed under
/// wasmtime, matches the independent num-bigint Montgomery oracle at every
/// ordered operand pair (limb-for-limb). Precondition for the honest same-Fe-
/// function cross-backend claim.
fn field_mul_wasm_gate(source: &str, fn_name: &str, p: &BigUint, n: usize, label: &str) {
    let bytes = compile_source_to_wasm(source, fn_name);
    let ops = msm_operands(p, n);
    let mut count = 0usize;
    for (na, a) in &ops {
        let al = msm_to_limbs(a, n);
        for (nb, b) in &ops {
            let bl = msm_to_limbs(b, n);
            let got = wasm_field_mul_limbs(&bytes, fn_name, &al, &bl, n);
            let want = mont_oracle_limbs(a, b, p, n);
            assert_eq!(
                got, want,
                "{label} wasm {fn_name}({na} * {nb}) limbs must equal the bigint oracle \
                 a*b*R^-1 mod p"
            );
            count += 1;
        }
    }
    eprintln!(
        "  {label} wasm leg: Fe {fn_name} -> wasm executed under wasmtime; all {count} operand \
         products limb-equal to the num-bigint Montgomery oracle (incl p-1, dense-limb carries)."
    );
}

/// The headline gate: the Fe field-mul EXECUTES on lavapipe (browser profile),
/// tri-equal (lavapipe SPIR-V == wasmtime wasm == num-bigint oracle) at every
/// ordered operand pair. Hard-fail-not-skip on a missing GPU.
fn field_mul_lavapipe_gate(
    source: &str,
    fn_name: &str,
    p: &BigUint,
    n: usize,
    grid_w: u32,
    label: &str,
) {
    // --- Compile through the Grid driver seam. ---
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{fn_name}_gpu.fe")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("field-mul kernel should build a wasm runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_grid(&db, &package, [8, 8, 1])
        .expect("field-mul kernel should compile Fe -> naga-validated SPIR-V in Grid mode");

    assert_eq!(
        artifact.layout.mode,
        sonatina_codegen::isa::spirv::LayoutMode::Grid,
        "the grid driver seam must state LayoutMode::Grid"
    );
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
        "the field-mul kernel must lower to the u32 word (browser profile)"
    );
    let input_stride = artifact
        .layout
        .bindings
        .iter()
        .find(|b| b.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("the grid layout must have an Input binding")
        .stride;
    assert_eq!(
        input_stride as usize,
        4 * 2 * n,
        "the {} broadcast input limbs (a0..a{}, b0..b{}) span 4*2*n bytes: the layout-level proof \
         that the two field elements ride the grid broadcast-param path",
        2 * n,
        n - 1,
        n - 1,
    );

    // --- Browser-profile WGSL gate + broadcast-member tokens. ---
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the field-mul kernel");
    assert_browser_profile_wgsl(wgsl);
    assert!(
        wgsl.contains("global_invocation_id"),
        "grid WGSL must bind global_invocation_id (the per-limb gid.x = k); got a {}-char module",
        wgsl.len()
    );
    assert!(
        wgsl.contains("input.p0") && wgsl.contains(&format!("input.p{}", 2 * n - 1)),
        "the WGSL must load the first and last broadcast limb members (input.p0 .. input.p{})",
        2 * n - 1
    );
    eprintln!(
        "  {label} WGSL passed the browser profile ({} chars); carries global_invocation_id + \
         input.p0..input.p{} ({}-member broadcast).",
        wgsl.len(),
        2 * n - 1,
        2 * n
    );

    // --- The wasm leg (same Fe function), recomputed per pair. ---
    let wasm_bytes = compile_source_to_wasm(source, fn_name);

    // --- Build every ordered operand pair's broadcast param-set. ---
    let ops = msm_operands(p, n);
    let mut names: Vec<(String, String)> = Vec::new();
    let mut param_sets: Vec<Vec<u32>> = Vec::new();
    let mut oracles: Vec<Vec<u32>> = Vec::new();
    let mut wasms: Vec<Vec<u32>> = Vec::new();
    for (na, a) in &ops {
        let al = msm_to_limbs(a, n);
        for (nb, b) in &ops {
            let bl = msm_to_limbs(b, n);
            let mut params = Vec::with_capacity(2 * n);
            params.extend_from_slice(&al);
            params.extend_from_slice(&bl);
            names.push((na.clone(), nb.clone()));
            oracles.push(mont_oracle_limbs(a, b, p, n));
            wasms.push(wasm_field_mul_limbs(&wasm_bytes, fn_name, &al, &bl, n));
            param_sets.push(params);
        }
    }

    // --- EXECUTE on lavapipe (one device+pipeline, one dispatch per pair) and
    // assert every product limb is tri-equal. ---
    match run_grid_batches_on_lavapipe(wgsl, grid_w, 8, &param_sets, label) {
        Some(grids) => {
            assert_eq!(grids.len(), param_sets.len(), "one grid per operand pair");
            for (idx, grid) in grids.iter().enumerate() {
                let (na, nb) = &names[idx];
                for limb in 0..n {
                    // Grid mode stores invocation k's return at output index k
                    // (row 0). Limb k = grid[k].
                    let got = grid[limb];
                    assert_eq!(
                        got, oracles[idx][limb],
                        "{label}: lavapipe {fn_name}({na} * {nb}) limb {limb} = {got} must equal \
                         the num-bigint oracle {}",
                        oracles[idx][limb]
                    );
                    assert_eq!(
                        got, wasms[idx][limb],
                        "{label}: lavapipe {fn_name}({na} * {nb}) limb {limb} = {got} must equal \
                         the wasmtime leg {} (same Fe function, two backends)",
                        wasms[idx][limb]
                    );
                }
            }
            eprintln!(
                "{label}: Fe {fn_name} EXECUTED on lavapipe (browser profile); all {} operand \
                 products (incl p-1 x p-1 and dense-limb carries) TRI-EQUAL across {n} limbs \
                 (lavapipe == wasmtime == num-bigint oracle). ZERO new ops.",
                param_sets.len()
            );
        }
        None => {
            eprintln!(
                "R-val only: {label} SPIR-V validated (browser profile) but NOT executed (GPU \
                 skipped via MB2_ALLOW_GPU_SKIP). The tri-equal GPU claim is NOT earned this run."
            );
        }
    }
}

/// MSM-P wasm leg (GPU-free de-risk): the 4-limb probe field-mul == the bigint
/// oracle at every operand pair.
#[test]
fn field_mul_probe_wasm_leg() {
    field_mul_wasm_gate(FIELD_MUL_PROBE_SOURCE, "field_mul_probe", &probe_prime(), 4, "MSM-P probe");
}

/// MSM-P headline: the 4-limb probe field-mul EXECUTES on lavapipe, tri-equal.
/// De-risks the 13-bit-limb idiom on the GPU before the 20-limb BN254 kernel.
#[test]
fn field_mul_probe_executes_on_lavapipe_browser_profile() {
    field_mul_lavapipe_gate(
        FIELD_MUL_PROBE_SOURCE,
        "field_mul_probe",
        &probe_prime(),
        4,
        8, // 4 limbs, rounded up to the 8x8 workgroup floor
        "MSM-P probe",
    );
}

/// MSM-0a wasm leg (GPU-free): the full 254-bit BN254 Fr field-mul == the bigint
/// oracle at every operand pair.
#[test]
fn field_mul_bn254_fr_wasm_leg() {
    field_mul_wasm_gate(
        FIELD_MUL_BN254_FR_SOURCE,
        "field_mul_bn254_fr",
        &bn254_fr_prime(),
        20,
        "MSM-0a BN254 Fr",
    );
}

/// MSM-0a headline: the unrolled 254-bit BN254 Fr field-multiply-mod-p EXECUTES
/// on lavapipe at the browser profile, tri-equal (lavapipe == wasmtime ==
/// num-bigint oracle) at every operand pair including the carry-heavy p-1 x p-1
/// and dense-limb cases. The first real crypto field arithmetic Fe compiles to
/// the GPU, ZERO new ops.
#[test]
fn field_mul_bn254_fr_executes_on_lavapipe_browser_profile() {
    field_mul_lavapipe_gate(
        FIELD_MUL_BN254_FR_SOURCE,
        "field_mul_bn254_fr",
        &bn254_fr_prime(),
        20,
        24, // 20 limbs, rounded up to a multiple of the 8-wide workgroup
        "MSM-0a BN254 Fr",
    );
}
