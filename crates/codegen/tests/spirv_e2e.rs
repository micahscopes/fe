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
