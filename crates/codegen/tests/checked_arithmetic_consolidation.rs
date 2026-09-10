#![cfg(feature = "spirv-backend")]

use common::InputDb;
use driver::DriverDataBase;
use ethers_core::{
    abi::{decode, ParamType, Token},
    types::U256,
};
use fe_codegen::{compile_runtime_package_spirv_with_workgroup, layout_for, BackendKind, OptLevel};
use fe_contract_harness::{ExecutionOptions, FeContractHarness, HarnessError};
use sha2::{Digest, Sha256};
use sonatina_codegen::isa::spirv::SpirvArtifact;
use std::{fs, path::Path};
use url::Url;

const SOURCE: &str = include_str!("fixtures/checked_arithmetic_consolidation.fe");
const SOURCE_SHA256: &str = "752da3465399a25520dc0afb90022a102d3067f60524e726c2ea0fd7fdc96eac";
const PANIC_OVERFLOW: &[u8] = &[
    0x4e, 0x48, 0x7b, 0x71, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0x11,
];

const EVM_WRAPPER: &str = r#"
use std::abi::sol

msg PolicyArithmeticMsg {
    #[selector = sol("uadd(uint32,uint32)")]
    UAdd { a: u32, b: u32 } -> u32,
    #[selector = sol("usub(uint32,uint32)")]
    USub { a: u32, b: u32 } -> u32,
    #[selector = sol("umul(uint32,uint32)")]
    UMul { a: u32, b: u32 } -> u32,
    #[selector = sol("iadd(int32,int32)")]
    IAdd { a: i32, b: i32 } -> i32,
    #[selector = sol("isub(int32,int32)")]
    ISub { a: i32, b: i32 } -> i32,
    #[selector = sol("imul(int32,int32)")]
    IMul { a: i32, b: i32 } -> i32,
}

pub contract PolicyArithmetic {
    recv PolicyArithmeticMsg {
        UAdd { a, b } -> u32 { policy_checked_u32_add(a, b) }
        USub { a, b } -> u32 { policy_checked_u32_sub(a, b) }
        UMul { a, b } -> u32 { policy_checked_u32_mul(a, b) }
        IAdd { a, b } -> i32 { policy_checked_i32_add(a, b) }
        ISub { a, b } -> i32 { policy_checked_i32_sub(a, b) }
        IMul { a, b } -> i32 { policy_checked_i32_mul(a, b) }
    }
}
"#;

fn assert_source_identity() {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    for witness in [
        "policy_checked_u32_add",
        "policy_checked_u32_sub",
        "policy_checked_u32_mul",
        "policy_checked_i32_add",
        "policy_checked_i32_sub",
        "policy_checked_i32_mul",
        "policy_gpu_add",
        "policy_gpu_sub",
        "policy_gpu_mul",
    ] {
        assert!(
            SOURCE.contains(witness),
            "fixture lost source witness `{witness}`"
        );
    }
}

fn write_artifact(name: &str, bytes: &[u8]) {
    let Some(directory) = std::env::var_os("FE_POLICY_ARITHMETIC_ARTIFACT_DIR") else {
        return;
    };
    let directory = Path::new(&directory);
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join(name), bytes).unwrap();
}

fn int_token(value: i32) -> Token {
    if value >= 0 {
        Token::Int(U256::from(value as u32))
    } else {
        Token::Int(U256::MAX - U256::from((-i64::from(value) - 1) as u64))
    }
}

fn assert_evm_overflow(harness: &FeContractHarness, signature: &str, args: &[Token]) {
    match harness.call_function(signature, args, ExecutionOptions::default()) {
        Err(HarnessError::Revert(data)) => assert_eq!(data.0, PANIC_OVERFLOW, "{signature}"),
        other => panic!("{signature} must revert with Panic(0x11), got {other:?}"),
    }
}

#[test]
fn evm_checked_add_sub_mul_preserve_values_and_panic_payloads() {
    assert_source_identity();
    let source = format!("{SOURCE}\n{EVM_WRAPPER}");
    assert!(source.starts_with(SOURCE));
    let harness = FeContractHarness::compile("PolicyArithmetic", &source).unwrap();
    write_artifact(
        "checked-arithmetic.evm",
        &hex::decode(harness.runtime_bytecode()).unwrap(),
    );

    let ok = harness
        .call_function(
            "uadd(uint32,uint32)",
            &[Token::Uint(40u32.into()), Token::Uint(2u32.into())],
            ExecutionOptions::default(),
        )
        .unwrap();
    assert_eq!(
        decode(&[ParamType::Uint(32)], &ok.return_data).unwrap(),
        [Token::Uint(42u32.into())]
    );

    let ok = harness
        .call_function(
            "iadd(int32,int32)",
            &[int_token(-7), int_token(5)],
            ExecutionOptions::default(),
        )
        .unwrap();
    assert_eq!(
        decode(&[ParamType::Int(32)], &ok.return_data).unwrap(),
        [int_token(-2)]
    );

    assert_evm_overflow(
        &harness,
        "uadd(uint32,uint32)",
        &[Token::Uint(u32::MAX.into()), Token::Uint(1u32.into())],
    );
    assert_evm_overflow(
        &harness,
        "usub(uint32,uint32)",
        &[Token::Uint(0u32.into()), Token::Uint(1u32.into())],
    );
    assert_evm_overflow(
        &harness,
        "umul(uint32,uint32)",
        &[Token::Uint(u32::MAX.into()), Token::Uint(2u32.into())],
    );
    assert_evm_overflow(
        &harness,
        "iadd(int32,int32)",
        &[int_token(i32::MAX), int_token(1)],
    );
    assert_evm_overflow(
        &harness,
        "isub(int32,int32)",
        &[int_token(i32::MIN), int_token(1)],
    );
    assert_evm_overflow(
        &harness,
        "imul(int32,int32)",
        &[int_token(i32::MIN), int_token(-1)],
    );
}

fn compile_wasm() -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///policy_checked_arithmetic_wasm.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    BackendKind::Wasm
        .create()
        .compile(&db, top, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap()
        .into_bytecode()
        .unwrap()
}

#[test]
fn wasm_checked_add_sub_mul_preserve_values_and_traps() {
    assert_source_identity();
    let wasm = compile_wasm();
    wasmparser::validate(&wasm).unwrap();
    write_artifact("checked-arithmetic.wasm", &wasm);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();

    for (name, args, expected) in [
        ("policy_checked_u32_add", (40i32, 2i32), 42i32),
        ("policy_checked_u32_sub", (44, 2), 42),
        ("policy_checked_u32_mul", (21, 2), 42),
        ("policy_checked_i32_add", (-7, 5), -2),
        ("policy_checked_i32_sub", (-7, 5), -12),
        ("policy_checked_i32_mul", (-7, 5), -35),
    ] {
        let function = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, name)
            .unwrap();
        assert_eq!(function.call(&mut store, args).unwrap(), expected, "{name}");
    }
    for (name, args) in [
        ("policy_checked_u32_add", (-1i32, 1i32)),
        ("policy_checked_u32_sub", (0, 1)),
        ("policy_checked_u32_mul", (-1, 2)),
        ("policy_checked_i32_add", (i32::MAX, 1)),
        ("policy_checked_i32_sub", (i32::MIN, 1)),
        ("policy_checked_i32_mul", (i32::MIN, -1)),
    ] {
        let function = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, name)
            .unwrap();
        assert!(function.call(&mut store, args).is_err(), "{name} must trap");
    }
}

fn compile_shader(entry: &str) -> SpirvArtifact {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{entry}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top, entry).unwrap();
    compile_runtime_package_spirv_with_workgroup(&db, &package, [1, 1, 1]).unwrap()
}

fn execute_shader(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    artifact: &SpirvArtifact,
    input: u32,
) -> (u32, u32) {
    let wgsl = artifact.wgsl.as_ref().unwrap();
    let result = artifact.layout.result.expect("scalar result binding");
    let trap = artifact
        .layout
        .trap
        .expect("checked arithmetic trap binding");
    assert_eq!(
        (result.group, result.binding, result.offset, result.width),
        (0, 0, 0, 4)
    );
    assert_eq!(
        (trap.group, trap.binding, trap.offset, trap.width),
        (0, 2, 0, 4)
    );

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("policy_checked_arithmetic"),
        source: wgpu::ShaderSource::Wgsl(wgsl.clone().into()),
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("policy_checked_output"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let input_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("policy_checked_input"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&input_buffer, 0, &input.to_le_bytes());
    let trap_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("policy_checked_trap"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("policy_checked_staging"),
        size: 8,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("policy_checked_layout"),
        entries: &[
            storage_entry(0, false),
            storage_entry(1, true),
            storage_entry(2, false),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("policy_checked_pipeline_layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("policy_checked_pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("policy_checked_bindings"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: output.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: trap_buffer.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, 4);
    encoder.copy_buffer_to_buffer(&trap_buffer, 0, &staging, 4, 4);
    queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = slice.get_mapped_range();
    let value = u32::from_le_bytes(mapped[0..4].try_into().unwrap());
    let trapped = u32::from_le_bytes(mapped[4..8].try_into().unwrap());
    drop(mapped);
    staging.unmap();
    (value, trapped)
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

#[test]
fn gpu_checked_add_sub_mul_report_trap_status_for_live_inputs() {
    assert_source_identity();
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .expect("software Vulkan adapter must be available");
    eprintln!("GPU adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))
    .unwrap();

    for (entry, ok_input, ok_output, overflow_input) in [
        ("policy_gpu_add", 41, 42, u32::MAX),
        ("policy_gpu_sub", 43, 42, 0),
        ("policy_gpu_mul", 21, 42, u32::MAX),
    ] {
        let artifact = compile_shader(entry);
        write_artifact(&format!("{entry}.spv"), &artifact.as_bytes());
        write_artifact(
            &format!("{entry}.wgsl"),
            artifact.wgsl.as_ref().unwrap().as_bytes(),
        );
        assert_eq!(
            execute_shader(&device, &queue, &artifact, ok_input),
            (ok_output, 0)
        );
        assert_eq!(
            execute_shader(&device, &queue, &artifact, overflow_input).1,
            1
        );
    }
}
