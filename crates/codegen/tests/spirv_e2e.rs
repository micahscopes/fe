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
