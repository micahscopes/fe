use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    compile_runtime_package_wasm_with_options, WasmCompileOptions, WebBuildOptions, WebBundle,
    WebResourceAccess,
};
use hir::hir_def::HirIngot;
use quilting_core::patch::QBTriPatch;
use quilting_core::quaternion::Quat;
use url::Url;
use wasmtime::{Instance, Store};

const FIXTURE: &[u8] =
    include_bytes!("../../../fixtures/classic-quilting/v1/direct-seed42-k1-1-1.cqa");
const VECTOR_LANES: usize = 10;

struct CompiledRaster {
    bundle: WebBundle,
    wasm: Vec<u8>,
}

static COMPILED: OnceLock<CompiledRaster> = OnceLock::new();
static COMPILED_PREDICATES: OnceLock<WebBundle> = OnceLock::new();
static COMPILED_TOPOLOGY: OnceLock<WebBundle> = OnceLock::new();
static COMPILED_SAMPLING: OnceLock<WebBundle> = OnceLock::new();
static COMPILED_DELAUNAY: OnceLock<WebBundle> = OnceLock::new();
static COMPILED_PARALLEL_CONSTRUCTION: OnceLock<WebBundle> = OnceLock::new();
static COMPILED_GENERATED_PATCH: OnceLock<WebBundle> = OnceLock::new();

fn compile_compute_oracle(relative_path: &str, entry: &str, label: &str) -> WebBundle {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "{label} ingot initialization diagnostics"
    );
    let top_mod = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap_or_else(|| panic!("{label} ingot"))
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected {label} diagnostics:\n{diagnostics}"
    );
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::compute(entry, Some(label.to_owned())),
    )
    .unwrap_or_else(|error| panic!("compile {label} WebGPU bundle: {error}"))
}

fn compile_render_oracle(relative_path: &str, entry: &str, label: &str) -> WebBundle {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "{label} ingot initialization diagnostics"
    );
    let top_mod = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap_or_else(|| panic!("{label} ingot"))
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected {label} diagnostics:\n{diagnostics}"
    );
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some(label.to_owned())),
    )
    .unwrap_or_else(|error| panic!("compile {label} WebGPU bundle: {error}"))
}

fn compiled_predicates() -> &'static WebBundle {
    COMPILED_PREDICATES.get_or_init(|| {
        compile_compute_oracle(
            "../../ingots/classic_quilting_predicate_webgpu_oracle",
            "classify",
            "classic-quilting-exact-predicate",
        )
    })
}

#[test]
fn exact_predicates_compile_to_browser_profile_wgsl() {
    let bundle = compiled_predicates();
    assert_eq!(bundle.manifest.passes.len(), 1);
    assert_eq!(bundle.manifest.passes[0].source_entry, "classify");
    assert_eq!(bundle.manifest.passes[0].dispatch, Some([1, 1, 1]));
    let module = naga::front::wgsl::parse_str(&bundle.wgsl).expect("predicate WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("predicate WGSL validates with browser capabilities");
    assert!(bundle.wgsl.contains("@compute"));
    assert!(!bundle.wgsl.contains("f32"));
    assert!(!bundle.wgsl.contains("f64"));
}

fn compiled_topology() -> &'static WebBundle {
    COMPILED_TOPOLOGY.get_or_init(|| {
        compile_compute_oracle(
            "../../ingots/classic_quilting_topology_webgpu_oracle",
            "exercise",
            "classic-quilting-exact-topology",
        )
    })
}

#[test]
fn exact_topology_compiles_to_browser_profile_wgsl() {
    let bundle = compiled_topology();
    assert_eq!(bundle.manifest.passes.len(), 1);
    assert_eq!(bundle.manifest.passes[0].source_entry, "exercise");
    assert_eq!(bundle.manifest.passes[0].dispatch, Some([1, 1, 1]));
    let module = naga::front::wgsl::parse_str(&bundle.wgsl).expect("topology WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("topology WGSL validates with browser capabilities");
    assert!(bundle.wgsl.contains("@compute"));
    assert!(!bundle.wgsl.contains("@fragment"));
    assert!(!bundle.wgsl.contains("f32"));
    assert!(!bundle.wgsl.contains("f64"));
}

fn compiled_sampling() -> &'static WebBundle {
    COMPILED_SAMPLING.get_or_init(|| {
        compile_compute_oracle(
            "../../ingots/classic_quilting_sampling_webgpu_oracle",
            "compact",
            "classic-quilting-gpu-sampling",
        )
    })
}

fn compiled_delaunay() -> &'static WebBundle {
    COMPILED_DELAUNAY.get_or_init(|| {
        compile_compute_oracle(
            "../../ingots/classic_quilting_construction_webgpu_oracle",
            "restore",
            "classic-quilting-constrained-delaunay",
        )
    })
}

fn compiled_parallel_construction() -> &'static WebBundle {
    COMPILED_PARALLEL_CONSTRUCTION.get_or_init(|| {
        compile_compute_oracle(
            "../../ingots/classic_quilting_parallel_construction_webgpu_oracle",
            "seal",
            "classic-quilting-parallel-construction",
        )
    })
}

fn compiled_generated_patch() -> &'static WebBundle {
    COMPILED_GENERATED_PATCH.get_or_init(|| {
        compile_render_oracle(
            "../../demos/sketches/classic_quilting_generated",
            "mesh_fragment",
            "classic-quilting-generated-patch",
        )
    })
}

#[test]
fn generated_patch_is_one_portable_gpu_resident_graph() {
    let bundle = compiled_generated_patch();
    let total_wgsl_bytes = bundle
        .pass_wgsl
        .iter()
        .map(|shader| shader.source.len())
        .sum::<usize>();
    let largest_wgsl_bytes = bundle
        .pass_wgsl
        .iter()
        .map(|shader| shader.source.len())
        .max()
        .expect("generated patch has shader passes");
    eprintln!(
        "  generated patch WGSL: {} passes, {total_wgsl_bytes} bytes total, {largest_wgsl_bytes} bytes largest pass",
        bundle.pass_wgsl.len(),
    );
    assert_eq!(
        bundle
            .manifest
            .passes
            .iter()
            .map(|pass| pass.source_entry.as_str())
            .collect::<Vec<_>>(),
        [
            "sample_initialize",
            "sample_propose",
            "sample_retire",
            "sample_advance",
            "sample_compact",
            "topology_initialize",
            "topology_locate",
            "topology_arbitrate",
            "topology_plan",
            "topology_scan",
            "topology_rebuild",
            "topology_retire",
            "topology_advance",
            "topology_seal",
            "background",
            "mesh_fragment",
        ]
    );
    assert!(bundle.manifest.resources.len() <= 8);
    assert_eq!(bundle.manifest.passes[0].dispatch, Some([5, 1, 1]));
    assert_eq!(bundle.manifest.passes[4].dispatch, Some([1, 1, 1]));
    assert_eq!(bundle.manifest.passes[5].dispatch, Some([5, 1, 1]));
    assert_eq!(bundle.manifest.passes[13].dispatch, Some([1, 1, 1]));
    assert_eq!(
        bundle.manifest.passes[15].layout.vertex_entry.as_deref(),
        Some("mesh_vertices")
    );
    assert_eq!(
        bundle.manifest.passes[15].layout.fragment_entry.as_deref(),
        Some("mesh_fragment")
    );
    for shader in &bundle.pass_wgsl {
        let module =
            naga::front::wgsl::parse_str(&shader.source).expect("generated patch WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("generated patch WGSL validates with browser capabilities");
    }
}

#[test]
fn parallel_topology_insertion_compiles_as_a_portable_integer_graph() {
    let bundle = compiled_parallel_construction();
    assert_eq!(bundle.manifest.passes.len(), 9);
    assert_eq!(
        bundle
            .manifest
            .passes
            .iter()
            .map(|pass| pass.source_entry.as_str())
            .collect::<Vec<_>>(),
        [
            "initialize",
            "locate",
            "arbitrate",
            "plan",
            "scan",
            "rebuild",
            "retire",
            "advance",
            "seal",
        ]
    );
    assert_eq!(bundle.manifest.passes[0].dispatch, Some([5, 1, 1]));
    assert_eq!(bundle.manifest.passes[8].dispatch, Some([1, 1, 1]));
    let cycle = bundle.manifest.passes[1]
        .cycle
        .expect("location starts the immutable topology cycle");
    assert_eq!(cycle.repeat, 64);
    assert!(bundle.manifest.passes[2..8]
        .iter()
        .all(|pass| pass.cycle == Some(cycle)));
    assert!(bundle.manifest.passes[0].cycle.is_none());
    assert!(bundle.manifest.passes[8].cycle.is_none());
    assert!(bundle.manifest.resources.len() <= 8);
    for shader in &bundle.pass_wgsl {
        let module =
            naga::front::wgsl::parse_str(&shader.source).expect("parallel hull WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("parallel hull WGSL validates with browser capabilities");
        assert!(shader.source.contains("@compute"));
        assert!(!shader.source.contains("@fragment"));
        assert!(!shader.source.contains("f32"));
        assert!(!shader.source.contains("f64"));
    }
}

#[test]
fn gpu_parallel_topology_insertion_satisfies_exact_planar_invariants() {
    let Some((adapter, device, queue)) = device() else {
        return;
    };
    eprintln!("GPU parallel topology adapter: {}", adapter.get_info().name);
    let bundle = compiled_parallel_construction();
    let expected = super::fe_oracle::scalar_sampling_oracle();
    let buffers = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| {
            let size = u64::from(resource.length) * u64::from(resource.stride);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&resource.name),
                size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (resource.name.clone(), buffer)
        })
        .collect::<BTreeMap<_, _>>();
    let point_words = expected
        .points
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    queue.write_buffer(&buffers["points"], 0, &u32_bytes(&point_words));
    queue.write_buffer(
        &buffers["source"],
        0,
        &u32_bytes(&[1, u32::try_from(expected.points.len()).unwrap()]),
    );

    let layout_entries = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| wgpu::BindGroupLayoutEntry {
            binding: resource.binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: resource.policy.access == WebResourceAccess::ReadOnly,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Fe classic Quilting parallel topology layout"),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Fe classic Quilting parallel topology pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let entries = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| wgpu::BindGroupEntry {
            binding: resource.binding,
            resource: buffers[&resource.name].as_entire_binding(),
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe classic Quilting parallel topology resources"),
        layout: &layout,
        entries: &entries,
    });
    let pipelines = bundle
        .pass_wgsl
        .iter()
        .map(|shader| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Fe classic Quilting parallel topology WGSL"),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Fe classic Quilting parallel topology stage"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        })
        .collect::<Vec<_>>();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Fe classic Quilting parallel topology graph"),
    });
    let initialize = &bundle.manifest.passes[0];
    dispatch_compute_pass(
        &mut encoder,
        &pipelines[0],
        &group,
        initialize
            .dispatch
            .expect("topology initialization dispatch"),
        initialize.repeat,
    );
    let cycle = bundle.manifest.passes[1]
        .cycle
        .expect("parallel topology cycle");
    for _ in 0..cycle.repeat {
        for index in 1..=7 {
            let stage = &bundle.manifest.passes[index];
            dispatch_compute_pass(
                &mut encoder,
                &pipelines[index],
                &group,
                stage.dispatch.expect("topology cycle dispatch"),
                stage.repeat,
            );
        }
    }
    let seal = &bundle.manifest.passes[8];
    dispatch_compute_pass(
        &mut encoder,
        &pipelines[8],
        &group,
        seal.dispatch.expect("topology seal dispatch"),
        seal.repeat,
    );

    let receipt_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only parallel topology receipt readback"),
        size: buffers["receipt"].size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let current_generation_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only parallel topology A readback"),
        size: buffers["triangles_a"].size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let next_generation_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only parallel topology B readback"),
        size: buffers["triangles_b"].size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(
        &buffers["receipt"],
        0,
        &receipt_staging,
        0,
        receipt_staging.size(),
    );
    encoder.copy_buffer_to_buffer(
        &buffers["triangles_a"],
        0,
        &current_generation_staging,
        0,
        current_generation_staging.size(),
    );
    encoder.copy_buffer_to_buffer(
        &buffers["triangles_b"],
        0,
        &next_generation_staging,
        0,
        next_generation_staging.size(),
    );
    queue.submit(Some(encoder.finish()));

    let receipt = readback(&device, &receipt_staging)
        .chunks_exact(size_of::<u32>())
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    let vertices = u32::try_from(expected.points.len()).unwrap();
    let triangle_count = vertices * 2 - expected.boundary_points - 2;
    assert_eq!(
        receipt[0], 1,
        "parallel insertion did not converge: {receipt:?}"
    );
    assert_eq!(receipt[1], vertices);
    assert_eq!(receipt[2], expected.boundary_points);
    assert_eq!(receipt[3], triangle_count);
    assert_eq!(&receipt[4..7], &[0, 0, 0]);
    assert!(receipt[7] <= 1);

    let active_words = if receipt[7] == 0 {
        readback(&device, &current_generation_staging)
    } else {
        readback(&device, &next_generation_staging)
    };
    let triangles = active_words
        .chunks_exact(4 * size_of::<u32>())
        .take(usize::try_from(triangle_count).unwrap())
        .map(|bytes| {
            let words = bytes
                .chunks_exact(size_of::<u32>())
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(words[3], 1);
            [words[0], words[1], words[2]]
        })
        .collect::<Vec<_>>();
    let mut used_vertices = BTreeSet::<u32>::new();
    let mut edge_uses = BTreeMap::<(u32, u32), Vec<(u32, u32)>>::new();
    for triangle in &triangles {
        assert!(triangle.iter().all(|vertex| *vertex < vertices));
        assert_eq!(triangle.iter().copied().collect::<BTreeSet<_>>().len(), 3);
        assert!(
            predicate_orientation(
                [
                    expected.points[triangle[0] as usize][1],
                    expected.points[triangle[0] as usize][2],
                ],
                [
                    expected.points[triangle[1] as usize][1],
                    expected.points[triangle[1] as usize][2],
                ],
                [
                    expected.points[triangle[2] as usize][1],
                    expected.points[triangle[2] as usize][2],
                ],
            ) > 0
        );
        used_vertices.extend(triangle.iter().copied());
        for edge in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            edge_uses
                .entry((edge.0.min(edge.1), edge.0.max(edge.1)))
                .or_default()
                .push(edge);
        }
    }
    assert_eq!(used_vertices.len(), expected.points.len());
    let expected_boundary = exact_boundary_edges(
        &expected.points,
        usize::try_from(expected.boundary_points).unwrap(),
    );
    let actual_boundary = edge_uses
        .iter()
        .filter_map(|(edge, uses)| (uses.len() == 1).then_some(*edge))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_boundary, expected_boundary);
    for uses in edge_uses.values() {
        assert!(uses.len() == 1 || uses.len() == 2);
        if uses.len() == 2 {
            assert_eq!(uses[0], (uses[1].1, uses[1].0));
        }
    }
}

#[test]
fn constrained_delaunay_compiles_as_an_ordered_browser_profile_graph() {
    let bundle = compiled_delaunay();
    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(
        bundle
            .manifest
            .passes
            .iter()
            .map(|pass| pass.source_entry.as_str())
            .collect::<Vec<_>>(),
        ["construct", "restore"]
    );
    assert!(bundle
        .manifest
        .passes
        .iter()
        .all(|pass| pass.dispatch == Some([1, 1, 1]) && pass.repeat == 1));
    for shader in &bundle.pass_wgsl {
        let module = naga::front::wgsl::parse_str(&shader.source).expect("Delaunay WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("Delaunay WGSL validates with browser capabilities");
        assert!(shader.source.contains("@compute"));
        assert!(!shader.source.contains("@fragment"));
        assert!(!shader.source.contains("f32"));
        assert!(!shader.source.contains("f64"));
    }
}

fn point_strictly_between(first: [u32; 3], second: [u32; 3], point: [u32; 3]) -> bool {
    point != first
        && point != second
        && (0..3).all(|lane| {
            point[lane] >= first[lane].min(second[lane])
                && point[lane] <= first[lane].max(second[lane])
        })
}

fn exact_boundary_edges(points: &[[u32; 3]], boundary: usize) -> BTreeSet<(u32, u32)> {
    let mut edges = BTreeSet::new();
    for first in 0..boundary {
        for second in first + 1..boundary {
            let same_hull_side =
                (0..3).any(|lane| points[first][lane] == 0 && points[second][lane] == 0);
            if !same_hull_side {
                continue;
            }
            let has_intermediate = (0..boundary).any(|point| {
                point != first
                    && point != second
                    && predicate_orientation(
                        [points[first][1], points[first][2]],
                        [points[second][1], points[second][2]],
                        [points[point][1], points[point][2]],
                    ) == 0
                    && point_strictly_between(points[first], points[second], points[point])
            });
            if !has_intermediate {
                edges.insert((
                    u32::try_from(first).unwrap(),
                    u32::try_from(second).unwrap(),
                ));
            }
        }
    }
    edges
}

#[test]
fn gpu_constrained_delaunay_satisfies_exact_planar_invariants() {
    let Some((adapter, device, queue)) = device() else {
        return;
    };
    eprintln!(
        "GPU constrained Delaunay adapter: {}",
        adapter.get_info().name
    );
    let bundle = compiled_delaunay();
    let expected = super::fe_oracle::scalar_sampling_oracle();
    let buffers = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| {
            let size = u64::from(resource.length) * u64::from(resource.stride);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&resource.name),
                size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (resource.name.clone(), buffer)
        })
        .collect::<BTreeMap<_, _>>();
    let point_words = expected
        .points
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    queue.write_buffer(&buffers["points"], 0, &u32_bytes(&point_words));
    queue.write_buffer(
        &buffers["source"],
        0,
        &u32_bytes(&[1, u32::try_from(expected.points.len()).unwrap()]),
    );

    let layout_entries = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| wgpu::BindGroupLayoutEntry {
            binding: resource.binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: resource.policy.access == WebResourceAccess::ReadOnly,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Fe classic Quilting construction layout"),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Fe classic Quilting construction pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let entries = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| wgpu::BindGroupEntry {
            binding: resource.binding,
            resource: buffers[&resource.name].as_entire_binding(),
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe classic Quilting construction resources"),
        layout: &layout,
        entries: &entries,
    });
    let pipelines = bundle
        .pass_wgsl
        .iter()
        .map(|shader| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Fe classic Quilting constrained Delaunay WGSL"),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Fe classic Quilting constrained Delaunay stage"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        })
        .collect::<Vec<_>>();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Fe classic Quilting construction graph"),
    });
    for (pass, pipeline) in bundle.manifest.passes.iter().zip(&pipelines) {
        dispatch_compute_pass(
            &mut encoder,
            pipeline,
            &group,
            pass.dispatch.expect("constrained Delaunay dispatch"),
            pass.repeat,
        );
    }
    let receipt_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only construction receipt readback"),
        size: 9 * size_of::<u32>() as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let triangle_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only construction triangle readback"),
        size: buffers["triangles"].size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let delaunay_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only Delaunay receipt readback"),
        size: 7 * size_of::<u32>() as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(
        &buffers["receipt"],
        0,
        &receipt_staging,
        0,
        receipt_staging.size(),
    );
    encoder.copy_buffer_to_buffer(
        &buffers["triangles"],
        0,
        &triangle_staging,
        0,
        triangle_staging.size(),
    );
    encoder.copy_buffer_to_buffer(
        &buffers["delaunay_receipt"],
        0,
        &delaunay_staging,
        0,
        delaunay_staging.size(),
    );
    queue.submit(Some(encoder.finish()));

    let receipt = readback(&device, &receipt_staging)
        .chunks_exact(size_of::<u32>())
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    let vertices = u32::try_from(expected.points.len()).unwrap();
    let triangle_count = vertices * 2 - expected.boundary_points - 2;
    assert_eq!(
        receipt,
        [
            1,
            0,
            vertices,
            expected.boundary_points,
            vertices,
            triangle_count,
            triangle_count,
            0,
            0,
        ]
    );
    let delaunay_receipt = readback(&device, &delaunay_staging)
        .chunks_exact(size_of::<u32>())
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(delaunay_receipt[0], 1);
    assert_eq!(delaunay_receipt[1], 1);
    assert_eq!(delaunay_receipt[2], vertices);
    assert_eq!(delaunay_receipt[3], triangle_count);
    assert_eq!(delaunay_receipt[5], 0);
    assert_eq!(delaunay_receipt[6], 0);
    let triangle_words = readback(&device, &triangle_staging);
    let triangles = triangle_words
        .chunks_exact(4 * size_of::<u32>())
        .take(usize::try_from(triangle_count).unwrap())
        .map(|bytes| {
            let words = bytes
                .chunks_exact(size_of::<u32>())
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(words[3], 1);
            [words[0], words[1], words[2]]
        })
        .collect::<Vec<_>>();
    let mut used_vertices = BTreeSet::<u32>::new();
    let mut edge_uses = BTreeMap::<(u32, u32), Vec<(usize, u32, u32)>>::new();
    for (triangle_ordinal, triangle) in triangles.iter().enumerate() {
        assert!(triangle.iter().all(|vertex| *vertex < vertices));
        assert_eq!(triangle.iter().copied().collect::<BTreeSet<_>>().len(), 3);
        assert!(
            predicate_orientation(
                [
                    expected.points[triangle[0] as usize][1],
                    expected.points[triangle[0] as usize][2]
                ],
                [
                    expected.points[triangle[1] as usize][1],
                    expected.points[triangle[1] as usize][2]
                ],
                [
                    expected.points[triangle[2] as usize][1],
                    expected.points[triangle[2] as usize][2]
                ],
            ) > 0
        );
        used_vertices.extend(triangle.iter().copied());
        for edge in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            edge_uses
                .entry((edge.0.min(edge.1), edge.0.max(edge.1)))
                .or_default()
                .push((triangle_ordinal, edge.0, edge.1));
        }
    }
    assert_eq!(used_vertices.len(), expected.points.len());
    let expected_boundary = exact_boundary_edges(
        &expected.points,
        usize::try_from(expected.boundary_points).unwrap(),
    );
    let actual_boundary = edge_uses
        .iter()
        .filter_map(|(edge, uses)| (uses.len() == 1).then_some(*edge))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_boundary, expected_boundary);
    for uses in edge_uses.values() {
        assert!(uses.len() == 1 || uses.len() == 2);
        if uses.len() == 2 {
            assert_eq!((uses[0].1, uses[0].2), (uses[1].2, uses[1].1));
            let first_triangle = triangles[uses[0].0];
            let second_triangle = triangles[uses[1].0];
            let u = uses[0].1;
            let v = uses[0].2;
            let first_opposite = *first_triangle
                .iter()
                .find(|vertex| **vertex != u && **vertex != v)
                .unwrap();
            let second_opposite = *second_triangle
                .iter()
                .find(|vertex| **vertex != u && **vertex != v)
                .unwrap();
            let point2 = |vertex: u32| {
                let point = expected.points[usize::try_from(vertex).unwrap()];
                [point[1], point[2]]
            };
            let convex =
                predicate_orientation(point2(first_opposite), point2(u), point2(second_opposite))
                    > 0
                    && predicate_orientation(
                        point2(second_opposite),
                        point2(v),
                        point2(first_opposite),
                    ) > 0;
            if convex {
                let incircle = predicate_incircle(
                    point2(u),
                    point2(v),
                    point2(first_opposite),
                    point2(second_opposite),
                );
                let current = (u.min(v), u.max(v));
                let proposed = (
                    first_opposite.min(second_opposite),
                    first_opposite.max(second_opposite),
                );
                assert!(incircle < 0 || (incircle == 0 && current <= proposed));
            }
        }
    }
}

#[test]
fn gpu_sampling_preserves_the_immutable_generation_cycle() {
    let bundle = compiled_sampling();
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .map(|resource| resource.name.as_str())
            .collect::<Vec<_>>(),
        ["workspace", "points", "receipt"],
    );
    assert_eq!(bundle.manifest.passes.len(), 5);
    assert_eq!(
        bundle
            .manifest
            .passes
            .iter()
            .map(|pass| pass.source_entry.as_str())
            .collect::<Vec<_>>(),
        ["initialize", "propose", "retire", "advance", "compact"]
    );
    assert_eq!(bundle.manifest.passes[0].dispatch, Some([3, 1, 1]));
    assert_eq!(bundle.manifest.passes[4].dispatch, Some([1, 1, 1]));
    let cycle = bundle.manifest.passes[1]
        .cycle
        .expect("proposal starts the immutable generation cycle");
    assert_eq!(cycle.repeat, 64);
    assert_eq!(bundle.manifest.passes[2].cycle, Some(cycle));
    assert_eq!(bundle.manifest.passes[3].cycle, Some(cycle));
    assert_eq!(bundle.manifest.passes[4].cycle, None);

    let module = naga::front::wgsl::parse_str(&bundle.wgsl).expect("sampling WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("sampling WGSL validates with browser capabilities");
    assert!(bundle.wgsl.contains("@compute"));
    assert!(!bundle.wgsl.contains("@fragment"));
    assert!(!bundle.wgsl.contains("f32"));
    assert!(!bundle.wgsl.contains("f64"));
}

fn dispatch_compute_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    resources: &wgpu::BindGroup,
    dispatch: [u32; 3],
    repeat: u32,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("Fe classic Quilting provider stage"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, resources, &[]);
    for _ in 0..repeat {
        pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
    }
}

#[test]
fn gpu_sampling_matches_the_scalar_placement_byte_for_byte() {
    let Some((adapter, device, queue)) = device() else {
        return;
    };
    eprintln!("GPU sampling adapter: {}", adapter.get_info().name);
    let bundle = compiled_sampling();
    let expected = super::fe_oracle::scalar_sampling_oracle();
    assert!(bundle
        .manifest
        .resources
        .iter()
        .all(|resource| resource.group == 0));

    let buffers = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| {
            let size = u64::from(resource.length) * u64::from(resource.stride);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&resource.name),
                size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (resource.name.clone(), buffer)
        })
        .collect::<BTreeMap<_, _>>();
    let layout_entries = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| wgpu::BindGroupLayoutEntry {
            binding: resource.binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: resource.policy.access == WebResourceAccess::ReadOnly,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Fe classic Quilting sampling layout"),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Fe classic Quilting sampling pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let entries = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| wgpu::BindGroupEntry {
            binding: resource.binding,
            resource: buffers[&resource.name].as_entire_binding(),
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe classic Quilting sampling resources"),
        layout: &layout,
        entries: &entries,
    });
    let pipelines = bundle
        .pass_wgsl
        .iter()
        .map(|shader| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Fe classic Quilting sampling WGSL"),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Fe classic Quilting sampling stage"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        })
        .collect::<Vec<_>>();

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Fe classic Quilting sampling graph"),
    });
    let initialize = &bundle.manifest.passes[0];
    dispatch_compute_pass(
        &mut encoder,
        &pipelines[0],
        &group,
        initialize.dispatch.expect("initialization dispatch"),
        initialize.repeat,
    );
    let cycle = bundle.manifest.passes[1].cycle.expect("sampling cycle");
    for _ in 0..cycle.repeat {
        for index in 1..=3 {
            let stage = &bundle.manifest.passes[index];
            dispatch_compute_pass(
                &mut encoder,
                &pipelines[index],
                &group,
                stage.dispatch.expect("cycle dispatch"),
                stage.repeat,
            );
        }
    }
    let compact = &bundle.manifest.passes[4];
    dispatch_compute_pass(
        &mut encoder,
        &pipelines[4],
        &group,
        compact.dispatch.expect("compaction dispatch"),
        compact.repeat,
    );

    let receipt_bytes = 7 * size_of::<u32>();
    let point_bytes = buffers["points"].size();
    let receipt_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only sampling receipt readback"),
        size: u64::try_from(receipt_bytes).unwrap(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let point_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only sampling point readback"),
        size: point_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(
        &buffers["receipt"],
        0,
        &receipt_staging,
        0,
        u64::try_from(receipt_bytes).unwrap(),
    );
    encoder.copy_buffer_to_buffer(&buffers["points"], 0, &point_staging, 0, point_bytes);
    queue.submit(Some(encoder.finish()));

    let receipt = readback(&device, &receipt_staging)
        .chunks_exact(size_of::<u32>())
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(
        receipt,
        [
            1,
            1,
            0,
            expected.candidate_slots,
            expected.accepted_candidates,
            expected.boundary_points,
            u32::try_from(expected.points.len()).unwrap(),
        ]
    );
    let words = readback(&device, &point_staging)
        .chunks_exact(size_of::<u32>())
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    let actual = words
        .chunks_exact(3)
        .take(expected.points.len())
        .map(|point| <[u32; 3]>::try_from(point).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected.points);
}

fn compiled_raster() -> &'static CompiledRaster {
    COMPILED.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ingots/classic_quilting_fixed_raster");
        let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
        let mut db = DriverDataBase::default();
        assert!(
            !driver::init_ingot(&mut db, &url),
            "fixed raster ingot initialization diagnostics"
        );
        let top_mod = db
            .workspace()
            .containing_ingot(&db, url)
            .expect("fixed raster ingot")
            .root_mod(&db);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(
            diagnostics.is_empty(),
            "unexpected fixed raster diagnostics:\n{diagnostics}"
        );
        let package = mir::build_wasm_runtime_package_for_entries(
            &db,
            top_mod,
            &["vertices".to_owned(), "shade".to_owned()],
        )
        .expect("build fixed raster Wasm runtime package");
        let wasm = compile_runtime_package_wasm_with_options(
            &db,
            &package,
            WasmCompileOptions::default().with_optimization(),
        )
        .expect("compile fixed raster Wasm")
        .bytes;
        wasmparser::validate(&wasm).expect("fixed raster Wasm validates");
        let bundle = WebBundle::compile(
            &db,
            top_mod,
            WebBuildOptions::render("shade", Some("classic-quilting-fixed-raster".to_owned())),
        )
        .expect("compile fixed authored raster bundle");
        CompiledRaster { bundle, wasm }
    })
}

fn curved_patch() -> QBTriPatch {
    QBTriPatch::new(
        [
            Quat::from_point(-0.75, -0.25, 0.1),
            Quat::from_point(0.8, -0.15, -0.2),
            Quat::from_point(0.05, 0.9, 0.35),
        ],
        [
            Quat::new(1.0, 0.2, -0.1, 0.05),
            Quat::new(0.9, -0.15, 0.25, 0.1),
            Quat::new(1.1, 0.1, 0.05, -0.2),
        ],
    )
}

fn normal_from_tangents(tangent_u: [f64; 3], tangent_v: [f64; 3]) -> [f64; 3] {
    let cross = [
        tangent_u[1] * tangent_v[2] - tangent_u[2] * tangent_v[1],
        tangent_u[2] * tangent_v[0] - tangent_u[0] * tangent_v[2],
        tangent_u[0] * tangent_v[1] - tangent_u[1] * tangent_v[0],
    ];
    let length = cross.iter().map(|value| value * value).sum::<f64>().sqrt();
    cross.map(|value| value / length)
}

fn oracle_f32(value: f64) -> f32 {
    assert!(value.is_finite());
    assert!(value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX));
    #[allow(clippy::cast_possible_truncation)]
    {
        value as f32
    }
}

fn expected_vectors() -> [[f32; VECTOR_LANES]; 3] {
    let artifact = crate::decode(FIXTURE).expect("checked smallest M0 fixture");
    let patch = curved_patch();
    let triangle = artifact.triangles[0];
    triangle.indices.map(|index| {
        let vertex = artifact.vertices[usize::try_from(index).unwrap()];
        let [_, u, v] = vertex.barycentric;
        let differential = patch.eval_differential(f64::from(u), f64::from(v));
        let normal = normal_from_tangents(differential.tangent_u, differential.tangent_v);
        [
            oracle_f32(differential.position[0]),
            oracle_f32(differential.position[1]),
            oracle_f32(differential.position[2] * 0.25),
            1.0,
            oracle_f32(differential.position[0]),
            oracle_f32(differential.position[1]),
            oracle_f32(differential.position[2]),
            oracle_f32(normal[0]),
            oracle_f32(normal[1]),
            oracle_f32(normal[2]),
        ]
    })
}

fn assert_vector_close(actual: &[f32], expected: &[f32], context: &str) {
    assert_eq!(actual.len(), expected.len());
    for (lane, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert!(actual.is_finite(), "{context} lane {lane} is nonfinite");
        assert!(
            (actual - expected).abs() <= 4.0e-6,
            "{context} lane {lane}: actual={actual:?}, expected={expected:?}"
        );
    }
}

#[test]
fn authored_raster_bundle_has_the_generated_draw_and_vector_interface() {
    let bundle = &compiled_raster().bundle;
    assert_eq!(bundle.manifest.passes.len(), 1);
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.draw_vertices, Some(3));
    assert_eq!(pass.layout.bindings.as_slice(), []);
    assert_eq!(pass.layout.vertex_entry.as_deref(), Some("vertices"));
    assert_eq!(pass.layout.fragment_entry.as_deref(), Some("shade"));
    assert_eq!(bundle.pass_wgsl.len(), 1);
    assert_eq!(bundle.wgsl, bundle.pass_wgsl[0].source);

    let module = naga::front::wgsl::parse_str(&bundle.wgsl).expect("authored raster WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("authored raster WGSL validates with browser capabilities");
    assert!(bundle.wgsl.contains("@vertex\nfn vertices"));
    assert!(bundle.wgsl.contains("@fragment\nfn shade"));
    for location in 0..6 {
        assert!(
            bundle.wgsl.contains(&format!("@location({location})")),
            "missing typed position/normal varying lane {location}"
        );
    }
}

fn instantiate_wasm() -> (Store<()>, Instance) {
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, &compiled_raster().wasm).expect("load fixed raster Wasm");
    assert!(module.imports().next().is_none());
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiate fixed raster Wasm");
    (store, instance)
}

#[test]
fn authored_raster_wasm_vectors_match_the_frozen_rust_oracle() {
    type RasterVector = (f32, f32, f32, f32, f32, f32, f32, f32, f32, f32);

    let (mut store, instance) = instantiate_wasm();
    let vertices = instance
        .get_typed_func::<i32, RasterVector>(&mut store, "vertices")
        .expect("fixed raster vertices export");
    for (index, expected) in expected_vectors().into_iter().enumerate() {
        let actual = vertices
            .call(&mut store, i32::try_from(index).unwrap())
            .expect("execute fixed raster vertex");
        assert_vector_close(
            &[
                actual.0, actual.1, actual.2, actual.3, actual.4, actual.5, actual.6, actual.7,
                actual.8, actual.9,
            ],
            &expected,
            &format!("Wasm vertex {index}"),
        );
    }
}

fn replace_once(source: &mut String, from: &str, to: &str) {
    assert_eq!(source.matches(from).count(), 1, "expected one `{from}`");
    *source = source.replacen(from, to, 1);
}

fn wgsl_capture_shader() -> String {
    let raster = &compiled_raster().bundle.wgsl;
    let fragment = raster
        .find("\n@fragment\n")
        .expect("generated raster fragment boundary");
    let mut shader = raster[..fragment].to_owned();
    replace_once(
        &mut shader,
        "    @builtin(position) position",
        "    position",
    );
    for location in 0..6 {
        replace_once(
            &mut shader,
            &format!("    @location({location}) v{location}_"),
            &format!("    v{location}_"),
        );
    }
    replace_once(
        &mut shader,
        "@vertex\nfn vertices(@builtin(vertex_index) vertex_index: u32)",
        "fn vertices(vertex_index: u32)",
    );
    shader.push_str(
        r"

@group(0) @binding(0)
var<storage, read_write> captured: array<f32>;

@compute @workgroup_size(1)
fn capture(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let vector = vertices(invocation.x);
    let base = invocation.x * 10u;
    captured[base + 0u] = vector.position.x;
    captured[base + 1u] = vector.position.y;
    captured[base + 2u] = vector.position.z;
    captured[base + 3u] = vector.position.w;
    captured[base + 4u] = vector.v0_;
    captured[base + 5u] = vector.v1_;
    captured[base + 6u] = vector.v2_;
    captured[base + 7u] = vector.v3_;
    captured[base + 8u] = vector.v4_;
    captured[base + 9u] = vector.v5_;
}
",
    );
    shader
}

fn device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();
    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(error) if allow_skip => {
            eprintln!("WebGPU execution skipped (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!("WebGPU execution has no adapter: {error:?}"),
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))
    .expect("request WebGPU device");
    Some((adapter, device, queue))
}

fn readback(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).unwrap();
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })
        .expect("poll fixed raster WGSL readback");
    receiver
        .recv()
        .expect("receive fixed raster map result")
        .expect("map fixed raster result");
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

fn predicate_orientation(first: [u32; 2], second: [u32; 2], third: [u32; 2]) -> i32 {
    let ab_b = i128::from(second[0]) - i128::from(first[0]);
    let ab_c = i128::from(second[1]) - i128::from(first[1]);
    let ac_b = i128::from(third[0]) - i128::from(first[0]);
    let ac_c = i128::from(third[1]) - i128::from(first[1]);
    i32::try_from(ab_b * ac_c - ab_c * ac_b).unwrap()
}

fn predicate_lift(delta_b: i128, delta_c: i128) -> i128 {
    delta_b * delta_b + delta_c * delta_c + delta_b * delta_c
}

fn predicate_incircle(first: [u32; 2], second: [u32; 2], third: [u32; 2], query: [u32; 2]) -> i32 {
    let delta = |point: [u32; 2]| {
        [
            i128::from(point[0]) - i128::from(query[0]),
            i128::from(point[1]) - i128::from(query[1]),
        ]
    };
    let [first_b, first_c] = delta(first);
    let [second_b, second_c] = delta(second);
    let [third_b, third_c] = delta(third);
    let first_second = first_b * second_c - second_b * first_c;
    let second_third = second_b * third_c - third_b * second_c;
    let third_first = third_b * first_c - first_b * third_c;
    let determinant = predicate_lift(first_b, first_c) * second_third
        + predicate_lift(second_b, second_c) * third_first
        + predicate_lift(third_b, third_c) * first_second;
    let winding = i128::from(predicate_orientation(first, second, third));
    i32::try_from(determinant.signum() * winding.signum()).unwrap()
}

fn u32_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

#[test]
fn exact_predicate_wgsl_matches_independent_i128_oracle_on_gpu() {
    const SCALE: u32 = 16_384;
    let Some((adapter, device, queue)) = device() else {
        return;
    };
    eprintln!("exact predicate WGSL adapter: {}", adapter.get_info().name);
    let bundle = compiled_predicates();
    let pass = &bundle.manifest.passes[0];
    let dispatch = pass.dispatch.expect("compute dispatch");
    assert_eq!(pass.layout.bindings.len(), 2);
    assert_eq!(pass.layout.bindings[0].name, "points");
    assert_eq!(pass.layout.bindings[1].name, "verdict");

    let point_bytes = u64::try_from(8 * size_of::<u32>()).unwrap();
    let verdict_bytes = u64::try_from(3 * size_of::<u32>()).unwrap();
    let points = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("exact predicate points"),
        size: point_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let verdict = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("exact predicate verdict"),
        size: verdict_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("exact predicate readback"),
        size: verdict_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("exact predicate layout"),
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
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("exact predicate resources"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: points.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: verdict.as_entire_binding(),
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("exact predicate pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Fe exact Quilting predicates"),
        source: wgpu::ShaderSource::Wgsl(bundle.wgsl.clone().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Fe exact Quilting predicates"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some(&pass.layout.entry_point),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    let cases = [
        [[0, 0], [SCALE, 0], [0, SCALE], [5_461, 5_461]],
        [[0, SCALE], [SCALE, 0], [0, 0], [8_192, 4_096]],
        [
            [9_216, 4_096],
            [8_192, 5_120],
            [7_168, 5_120],
            [7_168, 4_096],
        ],
        [[4_096, 4_096], [8_192, 4_096], [4_096, 8_192], [0, 0]],
        [[0, 0], [4_096, 4_096], [8_192, 8_192], [2_048, 6_144]],
    ];
    for (case_index, [first, second, third, query]) in cases.into_iter().enumerate() {
        let input = [
            first[0], first[1], second[0], second[1], third[0], third[1], query[0], query[1],
        ];
        queue.write_buffer(&points, 0, &u32_bytes(&input));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("exact predicate dispatch"),
        });
        {
            let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("exact predicate dispatch"),
                timestamp_writes: None,
            });
            compute.set_pipeline(&pipeline);
            compute.set_bind_group(0, &group, &[]);
            compute.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
        }
        encoder.copy_buffer_to_buffer(&verdict, 0, &staging, 0, verdict_bytes);
        queue.submit(Some(encoder.finish()));
        let bytes = readback(&device, &staging);
        let actual = bytes
            .chunks_exact(size_of::<u32>())
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        let orientation = predicate_orientation(first, second, third);
        let circle = predicate_incircle(first, second, third, query);
        let expected = [
            u32::from_ne_bytes(orientation.to_ne_bytes()),
            u32::from_ne_bytes(circle.to_ne_bytes()),
            u32::from(circle >= 0),
        ];
        assert_eq!(actual, expected, "predicate case {case_index}");
    }
}

#[test]
fn authored_raster_wgsl_vectors_match_the_frozen_rust_oracle() {
    let Some((adapter, device, queue)) = device() else {
        return;
    };
    eprintln!("fixed raster WGSL adapter: {}", adapter.get_info().name);
    let shader = wgsl_capture_shader();
    let parsed = naga::front::wgsl::parse_str(&shader).expect("WGSL capture shader parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&parsed)
    .expect("WGSL capture shader validates");

    let byte_count = u64::try_from(3 * VECTOR_LANES * size_of::<f32>()).unwrap();
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fixed raster WGSL vectors"),
        size: byte_count,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fixed raster WGSL vector readback"),
        size: byte_count,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fixed raster WGSL capture layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fixed raster WGSL capture group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: output.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fixed raster WGSL capture pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Fe-derived fixed raster WGSL capture"),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Fe-derived fixed raster WGSL capture"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("capture"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("fixed raster WGSL capture"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fixed raster WGSL capture"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(3, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, byte_count);
    queue.submit(Some(encoder.finish()));
    let bytes = readback(&device, &staging);
    let (words, remainder) = bytes.as_chunks::<{ size_of::<f32>() }>();
    assert_eq!(remainder, &[0_u8; 0]);
    let actual = words
        .iter()
        .map(|word| f32::from_le_bytes(*word))
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), 3 * VECTOR_LANES);
    for (index, expected) in expected_vectors().iter().enumerate() {
        let start = index * VECTOR_LANES;
        assert_vector_close(
            &actual[start..start + VECTOR_LANES],
            expected,
            &format!("WGSL vertex {index}"),
        );
    }
}
