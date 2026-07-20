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
