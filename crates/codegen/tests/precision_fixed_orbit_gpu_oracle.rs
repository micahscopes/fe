//! Independent WebGPU oracle for the exact `Fixed<8>` reference orbit.
//!
//! The Fe fixture writes the production packed projection and a test-only exact
//! audit record. This gate reconstructs each input f32 mathematically, evolves
//! one BigUint sign-magnitude recurrence, and checks every word of every
//! `Z_0..Z_32` sample at four centers. No Fe arithmetic or generated shader is
//! reused by the oracle.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBinding, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode,
    resolve_web_entry,
};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use url::Url;

const L: usize = 8;
const LIMB_BITS: usize = 13;
const LIMB_BASE: u32 = 8192;
const FRACTION_BITS: i32 = 91;
const CENTER_COUNT: usize = 4;
const STEPS: usize = 32;
const SAMPLES_PER_CENTER: usize = STEPS + 1;
const SAMPLE_COUNT: usize = CENTER_COUNT * SAMPLES_PER_CENTER;
const PACKED_WORDS: usize = 2;
const AUDIT_WORDS: usize = 21;
const INVALID_REFERENCE: u32 = 0x7fc0_0000;

const CENTERS: [([f32; 4], [f32; 4]); CENTER_COUNT] = [
    ([-2.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]),
    ([-0.5, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]),
    ([1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]),
    (
        [
            -0.7436438798904419,
            -0.0000000071467170,
            -0.0000000000000003,
            -0.00000000000000000000002,
        ],
        [
            0.1318259090185165,
            -0.0000000048132045,
            0.00000000000000025,
            0.000000000000000000000015,
        ],
    ),
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fx {
    sign: u32,
    mag: BigUint,
}

#[derive(Clone, Debug)]
struct ExpectedSample {
    x: Fx,
    y: Fx,
    re_bits: u32,
    im_bits: u32,
    valid: u32,
}

fn modulus() -> BigUint {
    BigUint::from(1u32) << (LIMB_BITS * L)
}

fn scale() -> BigUint {
    BigUint::from(1u32) << (LIMB_BITS * (L - 1))
}

fn to_limbs(magnitude: &BigUint) -> [u32; L] {
    let mask = BigUint::from(LIMB_BASE - 1);
    std::array::from_fn(|index| {
        ((magnitude >> (LIMB_BITS * index)) & &mask)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0)
    })
}

fn ref_add(a: &Fx, b: &Fx) -> Fx {
    if a.sign == b.sign {
        Fx {
            sign: a.sign,
            mag: (&a.mag + &b.mag) % modulus(),
        }
    } else if a.mag >= b.mag {
        Fx {
            sign: a.sign,
            mag: &a.mag - &b.mag,
        }
    } else {
        Fx {
            sign: b.sign,
            mag: &b.mag - &a.mag,
        }
    }
}

fn ref_sub(a: &Fx, b: &Fx) -> Fx {
    ref_add(
        a,
        &Fx {
            sign: 1 - b.sign,
            mag: b.mag.clone(),
        },
    )
}

fn ref_mul(a: &Fx, b: &Fx) -> Fx {
    let product = &a.mag * &b.mag;
    let rounded = (product + (scale() >> 1usize)) / scale();
    Fx {
        sign: a.sign ^ b.sign,
        mag: rounded % modulus(),
    }
}

fn ref_escaped(magnitude_squared: &BigUint) -> bool {
    let integer = magnitude_squared / scale();
    let fraction = magnitude_squared % scale();
    integer > BigUint::from(4u32)
        || (integer == BigUint::from(4u32) && fraction != BigUint::from(0u32))
}

/// Decode the binary32 value and truncate its exact magnitude at the Fixed<8>
/// radix boundary. This is mathematically independent of Fe's repeated f32
/// limb extraction.
fn from_f32(value: f32) -> Fx {
    let bits = value.to_bits();
    let sign = bits >> 31;
    let exponent = (bits >> 23) & 0xff;
    let fraction = bits & 0x7f_ffff;
    let (mantissa, shift) = if exponent == 0 {
        (fraction, FRACTION_BITS - 149)
    } else {
        (
            (1u32 << 23) | fraction,
            exponent as i32 - 127 - 23 + FRACTION_BITS,
        )
    };
    let mag = if shift >= 0 {
        BigUint::from(mantissa) << shift as usize
    } else {
        BigUint::from(mantissa) >> (-shift) as usize
    };
    Fx {
        sign: if mag == BigUint::from(0u32) { 0 } else { sign },
        mag,
    }
}

fn from_words(words: [f32; 4]) -> Fx {
    words
        .into_iter()
        .map(from_f32)
        .fold(from_f32(0.0), |sum, word| ref_add(&sum, &word))
}

/// Correctly rounded binary32 encoding of an exact Fixed<8> magnitude.
fn reference_bits(value: &Fx) -> u32 {
    if value.mag == BigUint::from(0u32) {
        return 0;
    }
    let high = value.mag.bits() as i32 - 1;
    let mut exponent = high - FRACTION_BITS + 127;
    let mut retained = if high <= 23 {
        (&value.mag << (23 - high) as usize)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0)
    } else {
        let shift = (high - 23) as usize;
        let mut quotient = (&value.mag >> shift)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0);
        let remainder = &value.mag - (BigUint::from(quotient) << shift);
        let half = BigUint::from(1u32) << (shift - 1);
        if remainder > half || (remainder == half && quotient & 1 == 1) {
            quotient += 1;
        }
        quotient
    };
    if retained == 1 << 24 {
        retained = 1 << 23;
        exponent += 1;
    }
    (value.sign << 31) | ((exponent as u32) << 23) | (retained & 0x7f_ffff)
}

fn expected_samples(cx: Fx, cy: Fx) -> Vec<ExpectedSample> {
    let zero = from_f32(0.0);
    let mut x = zero.clone();
    let mut y = zero;
    let mut valid = 1u32;
    let mut samples = vec![ExpectedSample {
        x: x.clone(),
        y: y.clone(),
        re_bits: 0,
        im_bits: 0,
        valid,
    }];
    for _ in 0..STEPS {
        if valid == 1 {
            let xx = ref_mul(&x, &x);
            let yy = ref_mul(&y, &y);
            let magnitude_squared = ref_add(&xx, &yy);
            if ref_escaped(&magnitude_squared.mag) {
                valid = 0;
            } else {
                let next_x = ref_add(&ref_sub(&xx, &yy), &cx);
                let xy = ref_mul(&x, &y);
                let next_y = ref_add(&ref_add(&xy, &xy), &cy);
                x = next_x;
                y = next_y;
            }
        }
        let (re_bits, im_bits) = if valid == 1 {
            (reference_bits(&x), reference_bits(&y))
        } else {
            (INVALID_REFERENCE, INVALID_REFERENCE)
        };
        samples.push(ExpectedSample {
            x: x.clone(),
            y: y.clone(),
            re_bits,
            im_bits,
            valid,
        });
    }
    samples
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_graph() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/precision_fixed_orbit_gpu_oracle");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "fixed orbit GPU oracle ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("fixed orbit GPU oracle should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "fixed orbit GPU oracle source diagnostics:\n{diagnostics}"
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the audit actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(
            entry,
            Some("crates/codegen/tests/fixtures/precision_fixed_orbit_gpu_oracle".into()),
        ),
    )
    .expect("fixed orbit audit should compile into a WebBundle")
}

fn request_browser_profile_device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();
    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(error) if allow_skip => {
            eprintln!(
                "  Fixed orbit GPU oracle SKIPPED (MB2_ALLOW_GPU_SKIP): no WebGPU adapter: {error:?}"
            );
            return None;
        }
        Err(error) => panic!(
            "Fixed orbit GPU oracle has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or \
             set MB2_ALLOW_GPU_SKIP to record an explicit non-execution on a genuinely GPU-less host."
        ),
    };
    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        },
    )) {
        Ok(pair) => pair,
        Err(error) if allow_skip => {
            eprintln!(
                "  Fixed orbit GPU oracle SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {error:?}"
            );
            return None;
        }
        Err(error) => panic!(
            "Fixed orbit browser-profile device request with no required features failed: {error:?}"
        ),
    };
    Some((adapter, device, queue))
}

fn buffer_type(binding: &WebBinding) -> wgpu::BufferBindingType {
    wgpu::BufferBindingType::Storage {
        read_only: binding.access == WebBindingAccess::Read,
    }
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

#[test]
fn fixed8_reference_orbit_matches_independent_biguint_oracle_on_webgpu() {
    let bundle = compile_graph();
    assert!(bundle.wasm.is_empty(), "audit graph has no CPU fallback");
    assert_eq!(bundle.manifest.resources.len(), 2);
    assert_eq!(bundle.manifest.resources[0].name, "packed");
    assert_eq!(bundle.manifest.resources[0].length as usize, SAMPLE_COUNT);
    assert_eq!(
        bundle.manifest.resources[0].stride as usize,
        PACKED_WORDS * 4
    );
    assert_eq!(bundle.manifest.resources[1].name, "audit");
    assert_eq!(bundle.manifest.resources[1].length as usize, SAMPLE_COUNT);
    assert_eq!(
        bundle.manifest.resources[1].stride as usize,
        AUDIT_WORDS * 4
    );
    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.manifest.passes[0].source_entry, "build");
    assert_eq!(bundle.manifest.passes[0].dispatch, Some([1, 1, 1]));
    assert_eq!(CENTER_COUNT * SAMPLES_PER_CENTER - 1, SAMPLE_COUNT - 1);

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  Fixed<8> orbit WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );

    let packed_bytes = (SAMPLE_COUNT * PACKED_WORDS * 4) as u64;
    let audit_bytes = (SAMPLE_COUNT * AUDIT_WORDS * 4) as u64;
    let packed = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fixed orbit packed projection"),
        size: packed_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let audit = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fixed orbit exact audit"),
        size: audit_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fixed orbit test-only readback"),
        size: packed_bytes + audit_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let pass = &bundle.manifest.passes[0];
    let layout_entries = pass
        .layout
        .bindings
        .iter()
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding: binding.binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: buffer_type(binding),
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fixed orbit audit layout"),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fixed orbit audit pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fixed orbit generated compute"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[0].source.as_str().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("fixed orbit audit pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let group_entries = pass
        .layout
        .bindings
        .iter()
        .map(|binding| {
            assert_eq!(binding.role, WebBindingRole::Resource);
            let buffer = match binding.name.as_str() {
                "packed" => &packed,
                "audit" => &audit,
                other => panic!("unexpected fixed orbit binding `{other}`"),
            };
            wgpu::BindGroupEntry {
                binding: binding.binding,
                resource: buffer.as_entire_binding(),
            }
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fixed orbit audit group"),
        layout: &layout,
        entries: &group_entries,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("fixed orbit exact audit"),
    });
    {
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("build exact reference checkpoints"),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pipeline);
        compute.set_bind_group(0, &group, &[]);
        compute.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&packed, 0, &staging, 0, packed_bytes);
    encoder.copy_buffer_to_buffer(&audit, 0, &staging, packed_bytes, audit_bytes);
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        tx.send(result)
            .expect("map callback receiver should remain open");
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .expect("fixed orbit WebGPU submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only fixed orbit staging buffer should map");
    let data = slice.get_mapped_range();
    let result = words(&data);
    let packed_words = &result[..SAMPLE_COUNT * PACKED_WORDS];
    let audit_words = &result[SAMPLE_COUNT * PACKED_WORDS..];

    for (center_index, (x_words, y_words)) in CENTERS.into_iter().enumerate() {
        let expected = expected_samples(from_words(x_words), from_words(y_words));
        assert_eq!(expected.len(), SAMPLES_PER_CENTER);
        for (step, sample) in expected.iter().enumerate() {
            let index = center_index * SAMPLES_PER_CENTER + step;
            let packed = &packed_words[index * PACKED_WORDS..][..PACKED_WORDS];
            let audit = &audit_words[index * AUDIT_WORDS..][..AUDIT_WORDS];
            let x_limbs = to_limbs(&sample.x.mag);
            let y_limbs = to_limbs(&sample.y.mag);
            let context = format!("center {center_index}, Z_{step}");

            assert_eq!(
                packed,
                [sample.re_bits, sample.im_bits],
                "{context}: packed"
            );
            assert_eq!(audit[0], sample.x.sign, "{context}: x sign");
            assert_eq!(&audit[1..9], &x_limbs, "{context}: x limbs");
            assert_eq!(audit[9], sample.y.sign, "{context}: y sign");
            assert_eq!(&audit[10..18], &y_limbs, "{context}: y limbs");
            assert_eq!(audit[18], sample.re_bits, "{context}: audit re bits");
            assert_eq!(audit[19], sample.im_bits, "{context}: audit im bits");
            assert_eq!(audit[20], sample.valid, "{context}: validity");
        }
    }
    assert_eq!(
        audit_words[(2 * SAMPLES_PER_CENTER + 3) * AUDIT_WORDS + 20],
        0,
        "the deliberately escaping (1,1) reference must sentinel at Z_3"
    );
    assert_eq!(
        packed_words[(2 * SAMPLES_PER_CENTER + 3) * PACKED_WORDS],
        INVALID_REFERENCE,
        "the deliberately escaping reference must never expose wrapped data"
    );

    drop(data);
    staging.unmap();
    eprintln!(
        "  Fixed<8> orbit: {SAMPLE_COUNT} exact checkpoints, {} packed words, {} audit words green",
        SAMPLE_COUNT * PACKED_WORDS,
        SAMPLE_COUNT * AUDIT_WORDS
    );
}
