//! Recursive authored records retain the existing flat storage-buffer ABI.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebActorResourceElement, WebBuildOptions, WebBundle, WebScalarKind};
use url::Url;

const NESTED: &str = include_str!("fixtures/actor_recursive_storage/src/lib.fe");
const INVALID: &str = include_str!("fixtures/actor_recursive_storage_invalid/src/lib.fe");

fn compile(source: &str) -> Result<WebBundle, String> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///recursive_storage_test.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    let diagnostics = db.run_on_ingot(top.ingot(&db)).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "fixture diagnostics:\n{diagnostics}"
    );
    WebBundle::compile(&db, top, WebBuildOptions::render("paint", None))
        .map_err(|error| error.to_string())
}

fn scalar_kinds(words: usize) -> Vec<WebScalarKind> {
    (0..words)
        .map(|index| {
            if (words == 10 && index >= 8) || (words != 10 && (10..14).contains(&index)) {
                WebScalarKind::U32
            } else {
                WebScalarKind::F32
            }
        })
        .collect()
}

fn roundtrip_inputs() -> Vec<Vec<u8>> {
    [18, 22, 10]
        .into_iter()
        .map(|words| {
            scalar_kinds(words)
                .into_iter()
                .cycle()
                .take(words * 2)
                .enumerate()
                .flat_map(|(index, scalar)| {
                    let bits = if scalar == WebScalarKind::U32 {
                        0x8000_0100u32 + index as u32
                    } else if index == 0 {
                        0x8000_0000 // signed zero survives a whole-value copy
                    } else {
                        (-(index as f32 + 0.25)).to_bits()
                    };
                    bits.to_le_bytes()
                })
                .collect()
        })
        .collect()
}

// Reflect the emitted shader, rather than inventing an optional trap binding.
// This fixture has six resources and may have a trailing backend trap buffer.
fn roundtrip_bindings(bundle: &WebBundle) -> Vec<u32> {
    let shader = naga::front::wgsl::parse_str(&bundle.pass_wgsl[0].source).unwrap();
    let mut bindings = shader
        .global_variables
        .iter()
        .filter_map(|(_, global)| global.binding.as_ref())
        .map(|binding| {
            assert_eq!(binding.group, 0, "fixture uses only bind group zero");
            binding.binding
        })
        .collect::<Vec<_>>();
    bindings.sort_unstable();
    assert!(
        bindings == [0, 1, 2, 3, 4, 5] || bindings == [0, 1, 2, 3, 4, 5, 6],
        "unexpected fixture binding contract: {bindings:?}"
    );
    bindings
}

fn roundtrip_buffer_sizes(bindings: &[u32]) -> Vec<u64> {
    let sizes = [144, 144, 176, 176, 80, 80, 256];
    bindings
        .iter()
        .map(|&binding| sizes[binding as usize])
        .collect()
}

// Export precisely the native GPU fixture for a browser with a remote adapter.
// Merely writing these files is not a GPU execution or a passing GPU gate.
fn dump_fixture(label: &str, bundle: &WebBundle) {
    let Some(root) = std::env::var_os("FE_RECURSIVE_STORAGE_DUMP") else {
        return;
    };
    let directory = std::path::PathBuf::from(root).join(label);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&bundle.manifest).unwrap(),
    )
    .unwrap();
    for (index, pass) in bundle.pass_wgsl.iter().enumerate() {
        std::fs::write(directory.join(format!("pass-{index}.wgsl")), &pass.source).unwrap();
    }
    let bindings = roundtrip_bindings(bundle);
    let trap_binding = bindings.contains(&6).then_some(6u32);
    std::fs::write(
        directory.join("roundtrip.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": 2,
            "computePass": 0,
            "entryPoint": "main",
            "bindGroup": 0,
            "bindings": bindings,
            "bufferSizes": roundtrip_buffer_sizes(&bindings),
            "inputBindings": [0, 2, 4],
            "outputBindings": [1, 3, 5],
            "inputBytes": roundtrip_inputs(),
            "outputInitialByte": 205,
            "trapBinding": trap_binding,
            "trapExpectedByte": trap_binding.map(|_| 0u8),
            "workgroups": [1, 1, 1]
        }))
        .unwrap(),
    )
    .unwrap();
}

fn flat_source() -> String {
    let start = NESTED.find("struct Pair<T>").unwrap();
    let end = NESTED.find("fn copy<T").unwrap();
    let mut records = String::new();
    for (name, words) in [("Snapshot", 18), ("Extended", 22), ("Anchor", 10)] {
        records.push_str(&format!("struct {name} {{\n"));
        for (index, scalar) in scalar_kinds(words).into_iter().enumerate() {
            let ty = if scalar == WebScalarKind::U32 {
                "u32"
            } else {
                "f32"
            };
            records.push_str(&format!("    f{index}: {ty},\n"));
        }
        records.push_str(&format!("}}\nimpl Copy for {name} {{}}\n"));
    }
    format!("{}{}{}", &NESTED[..start], records, &NESTED[end..])
        .replace("sample.projection.origin.left.first.left", "sample.f0")
        .replace("sample.projection.span", "sample.f8")
}

#[test]
fn nested_generic_storage_preserves_flat_72_88_40_byte_layouts() {
    let nested = compile(NESTED).expect("nested generic storage bundle");
    let flat = compile(&flat_source()).expect("legacy flat storage bundle");
    dump_fixture("nested", &nested);
    dump_fixture("flat", &flat);
    assert_eq!(nested.manifest.resources.len(), 6);
    for (index, (actual, legacy)) in nested
        .manifest
        .resources
        .iter()
        .zip(&flat.manifest.resources)
        .enumerate()
    {
        let words = [18, 18, 22, 22, 10, 10][index];
        let bytes = words as u32 * 4;
        assert_eq!(
            (actual.span, actual.stride, actual.length),
            (bytes, bytes, 2)
        );
        assert_eq!(
            (actual.span, actual.stride, actual.length),
            (legacy.span, legacy.stride, legacy.length)
        );
        let WebActorResourceElement::Record { fields, span } = &actual.element else {
            panic!("expected a record resource");
        };
        let WebActorResourceElement::Record { fields: old, .. } = &legacy.element else {
            panic!("expected a legacy record resource");
        };
        assert_eq!(*span, bytes);
        assert_eq!(fields.len(), words);
        for (leaf, ((field, legacy), scalar)) in
            fields.iter().zip(old).zip(scalar_kinds(words)).enumerate()
        {
            assert_eq!((field.offset, field.scalar), (leaf as u32 * 4, scalar));
            assert_eq!((field.offset, field.scalar), (legacy.offset, legacy.scalar));
            assert_eq!(
                legacy.name,
                format!("f{leaf}"),
                "flat names remain unchanged"
            );
        }
    }
    let WebActorResourceElement::Record { fields, .. } = &nested.manifest.resources[0].element
    else {
        unreachable!();
    };
    assert_eq!(fields[0].name, "projection.origin.left.first.left");
    assert_eq!(fields[7].name, "projection.origin.right.second.right");
    assert_eq!(fields[10].name, "counts.first.left");
    assert_eq!(fields[17].name, "weights.second.right");
    assert_eq!(
        nested.pass_wgsl.len(),
        2,
        "compute and nested-value fragment loads both lower"
    );
}

#[test]
fn unsupported_nested_leaves_fail_with_the_semantic_path() {
    for ty in [
        "bool",
        "i32",
        "u64",
        "[f32; 2]",
        "StorageBuffer<u32, 1>",
        "MemPtr<u32>",
        "Closed",
    ] {
        let source = format!(
            "use core::MemPtr\nenum Closed {{ A, B }}\n{}",
            INVALID.replace("Payload<bool>", &format!("Payload<{ty}>"))
        );
        let error = compile(&source).expect_err("unsupported storage leaf must fail closed");
        assert!(error.contains("payload.value"), "type={ty}: {error}");
        if ty == "i32" {
            assert!(
                error.contains("signed `i32` storage requires explicit carrier bitcasts"),
                "{error}"
            );
        } else {
            assert!(
                error.contains("POD field") && error.contains("u32") && error.contains("f32"),
                "{error}"
            );
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter; run explicitly after the compile/layout gates"]
fn nested_and_flat_storage_roundtrip_all_leaf_bits_on_gpu() {
    for source in [NESTED.to_owned(), flat_source()] {
        execute(&compile(&source).expect("roundtrip bundle"));
    }
}

fn execute(bundle: &WebBundle) {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("recursive storage roundtrip requires a GPU adapter");
    eprintln!("recursive storage adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("recursive storage"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[0].source.as_str().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bound_slots = roundtrip_bindings(bundle);
    let sizes = roundtrip_buffer_sizes(&bound_slots);
    let buffers = sizes
        .iter()
        .map(|&size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .collect::<Vec<_>>();
    let mut expected = Vec::new();
    for (pair, bytes) in roundtrip_inputs().into_iter().enumerate() {
        queue.write_buffer(&buffers[pair * 2], 0, &bytes);
        queue.write_buffer(&buffers[pair * 2 + 1], 0, &vec![0xcd; bytes.len()]);
        expected.extend(bytes);
    }
    let mut output_slots = vec![1, 3, 5];
    if bound_slots.contains(&6) {
        expected.extend([0u8; 256]);
        output_slots.push(6);
    }
    let entries = buffers
        .iter()
        .zip(&bound_slots)
        .map(|(buffer, &binding)| wgpu::BindGroupEntry {
            binding,
            resource: buffer.as_entire_binding(),
        })
        .collect::<Vec<_>>();
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: expected.len() as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    let mut offset = 0;
    for index in output_slots {
        encoder.copy_buffer_to_buffer(&buffers[index], 0, &readback, offset, sizes[index]);
        offset += sizes[index];
    }
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).unwrap()
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .unwrap();
    receiver.recv().unwrap().unwrap();
    assert_eq!(
        &*slice.get_mapped_range(),
        &expected,
        "all record leaves, second-element strides and any declared trap words must roundtrip"
    );
}
