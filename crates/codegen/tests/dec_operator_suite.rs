//! Acceptance for the DEC operator-suite actor demo (`demos/sketches/dec`).
//!
//! Three mechanical gates:
//!   1. The operator actor's declared topology reaches a canonical child
//!      interface: six wasm lanes placed in a Worker, with pinned message
//!      layouts.
//!   2. The wasm lanes execute under wasmtime and match a hand-computed
//!      oracle that shares no code with the Fe derivation: d on basis
//!      cochains, d compose d exactly zero, the composed Laplacian, and the
//!      five diagonal Hodge weights of the unit fan hexagon.
//!   3. Negative: applying `d` at the top grade of the complex is rejected
//!      with a readable diagnostic BECAUSE the successor grade has no
//!      `OnComplex` impl. The failure comes from the grade structure in the
//!      types, not from any name check.
//!
//! No GPU is touched here and no parallelism claim is made anywhere: the
//! lanes are sequential wasm, and the demo's render leg is compiled (bundle
//! test) but never executed in this sandbox.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    CanonicalExecution, CanonicalInterfaceManifest, CanonicalLane, CanonicalLayout,
    CanonicalPlacement, CanonicalShape, WasmCompileOptions, WebBuildOptions, WebBundle,
    canonical_lane_decls_from_module, compile_runtime_package_wasm_with_options,
    verify_canonical_wasm_abi,
};
use hir::hir_def::HirIngot;
use url::Url;

const WASM_LANES: [&str; 6] = ["probe", "d0", "d1", "dd0", "laplace0", "hodge"];
fn ingot_root(relative: &str) -> Url {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn field_offset(layout: &CanonicalLayout, name: &str) -> usize {
    let CanonicalShape::Record { fields } = &layout.shape else {
        panic!("expected a record layout");
    };
    fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing canonical field `{name}`"))
        .offset as usize
}

struct LaneCaller {
    store: wasmtime::Store<()>,
    instance: wasmtime::Instance,
    memory: wasmtime::Memory,
    manifest: CanonicalInterfaceManifest,
}

impl LaneCaller {
    fn lane(&self, name: &str) -> &CanonicalLane {
        self.manifest
            .lanes
            .iter()
            .find(|lane| lane.name == name)
            .unwrap_or_else(|| panic!("missing canonical lane `{name}`"))
    }

    /// One canonical round trip: reset, alloc, write request bytes, call the
    /// lane export, read the response record back.
    fn call(&mut self, name: &str, write: impl Fn(&CanonicalLayout, &mut [u8])) -> Vec<u8> {
        let lane = self.lane(name).clone();
        let export = lane.export.as_deref().expect("wasm lane export");
        let reset = self
            .instance
            .get_typed_func::<(), ()>(&mut self.store, "fe_cabi_reset")
            .unwrap();
        reset.call(&mut self.store, ()).unwrap();
        let alloc = self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "fe_cabi_alloc")
            .unwrap();
        let request_ptr = alloc
            .call(
                &mut self.store,
                (lane.request.size as i32, lane.request.align as i32),
            )
            .unwrap();
        let mut request = vec![0_u8; lane.request.size as usize];
        write(&lane.request, &mut request);
        self.memory
            .write(&mut self.store, request_ptr as usize, &request)
            .unwrap();
        let entry = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, export)
            .unwrap();
        let response_ptr = entry.call(&mut self.store, request_ptr).unwrap();
        let mut response = vec![0_u8; lane.response.size as usize];
        self.memory
            .read(&self.store, response_ptr as usize, &mut response)
            .unwrap();
        response
    }
}

fn put_f32(layout: &CanonicalLayout, bytes: &mut [u8], name: &str, value: f32) {
    let offset = field_offset(layout, name);
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(layout: &CanonicalLayout, bytes: &mut [u8], name: &str, value: u32) {
    let offset = field_offset(layout, name);
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn get_f32(layout: &CanonicalLayout, bytes: &[u8], name: &str) -> f32 {
    let offset = field_offset(layout, name);
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn get_i32(layout: &CanonicalLayout, bytes: &[u8], name: &str) -> i32 {
    let offset = field_offset(layout, name);
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn compile_dec_actor() -> (CanonicalInterfaceManifest, Vec<u8>) {
    let mut db = DriverDataBase::default();
    let url = ingot_root("../../demos/sketches/dec");
    assert!(
        !driver::init_ingot(&mut db, &url),
        "dec ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("dec ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected dec diagnostics:\n{diagnostics}"
    );

    let declarations = canonical_lane_decls_from_module(&db, top_mod).unwrap();
    let manifest = CanonicalInterfaceManifest::build(declarations).unwrap();

    let wasm_entries = manifest
        .lanes
        .iter()
        .filter(|lane| lane.intent.execution == CanonicalExecution::Wasm)
        .map(|lane| lane.name.clone())
        .collect::<Vec<_>>();
    let package = mir::build_wasm_runtime_package_for_entries(&db, top_mod, &wasm_entries).unwrap();
    let lanes: Vec<CanonicalLane> = manifest
        .lanes
        .iter()
        .filter(|lane| lane.intent.execution == CanonicalExecution::Wasm)
        .cloned()
        .collect();
    let wasm = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_canonical_lanes(lanes),
    )
    .unwrap()
    .bytes;
    wasmparser::validate(&wasm).unwrap();
    verify_canonical_wasm_abi(&wasm, &manifest).unwrap();
    (manifest, wasm)
}

fn caller() -> LaneCaller {
    let (manifest, wasm) = compile_dec_actor();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    LaneCaller {
        store,
        instance,
        memory,
        manifest,
    }
}

const COCHAIN0_FIELDS: [&str; 7] = ["v0", "v1", "v2", "v3", "v4", "v5", "v6"];
const COCHAIN1_FIELDS: [&str; 12] = [
    "e0", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9", "e10", "e11",
];
const COCHAIN2_FIELDS: [&str; 6] = ["t0", "t1", "t2", "t3", "t4", "t5"];

#[test]
fn declared_topology_reaches_the_canonical_interface() {
    let (manifest, _wasm) = compile_dec_actor();

    for name in WASM_LANES {
        let lane = manifest
            .lanes
            .iter()
            .find(|lane| lane.name == name)
            .unwrap();
        assert_eq!(lane.intent.execution, CanonicalExecution::Wasm, "{name}");
        assert_eq!(
            lane.intent.placement,
            CanonicalPlacement::Worker,
            "declared Worker placement must survive to the interface: {name}"
        );
        assert!(lane.intent.capabilities.is_empty(), "{name}");
        assert_eq!(
            lane.export.as_deref(),
            Some(format!("fe_cabi_{name}").as_str())
        );
    }

    assert_eq!(manifest.lanes.len(), WASM_LANES.len());

    // Pinned per-grade wire layouts: distinct nominal shapes per grade is the
    // point of the message design.
    let lane = |name: &str| {
        manifest
            .lanes
            .iter()
            .find(|lane| lane.name == name)
            .unwrap()
    };
    assert_eq!(lane("d0").request.size, 28, "Cochain0 is seven packed f32s");
    assert_eq!(
        lane("d0").response.size,
        48,
        "Cochain1 is twelve packed f32s"
    );
    assert_eq!(lane("d1").response.size, 24, "Cochain2 is six packed f32s");
    assert_eq!(lane("dd0").request.size, 28);
    assert_eq!(lane("dd0").response.size, 24);
    assert_eq!(lane("laplace0").response.size, 28);
    assert_eq!(lane("probe").request.size, 4);
    assert_eq!(lane("probe").response.size, 16);
    assert_eq!(lane("hodge").response.size, 20);
}

#[test]
fn lanes_execute_and_match_the_hand_oracle() {
    let mut actor = caller();

    // probe: the complex knows its own shape; Euler characteristic of a disk.
    let probe_layout = actor.lane("probe").response.clone();
    let info = actor.call("probe", |layout, bytes| {
        put_u32(layout, bytes, "generation", 1);
    });
    assert_eq!(get_i32(&probe_layout, &info, "vertices"), 7);
    assert_eq!(get_i32(&probe_layout, &info, "edges"), 12);
    assert_eq!(get_i32(&probe_layout, &info, "faces"), 6);
    assert_eq!(get_i32(&probe_layout, &info, "euler"), 1);

    // d0 on the center-vertex basis cochain: every spoke reads -1 (oriented
    // center -> ring), every ring edge reads 0. Hand-derived from the fan
    // orientation, independently of the Fe incidence.
    let c1_layout = actor.lane("d0").response.clone();
    let response = actor.call("d0", |layout, bytes| {
        put_f32(layout, bytes, "v0", 1.0);
    });
    for (index, field) in COCHAIN1_FIELDS.iter().enumerate() {
        let expected = if index < 6 { -1.0 } else { 0.0 };
        assert_eq!(get_f32(&c1_layout, &response, field), expected, "{field}");
    }

    // d0 on the ring-vertex-1 basis cochain: spoke 0 reads +1, ring edge 0
    // (v1 -> v2) reads -1, ring edge 5 (v6 -> v1) reads +1, all else 0.
    let response = actor.call("d0", |layout, bytes| {
        put_f32(layout, bytes, "v1", 1.0);
    });
    for (index, field) in COCHAIN1_FIELDS.iter().enumerate() {
        let expected = match index {
            0 => 1.0,
            6 => -1.0,
            11 => 1.0,
            _ => 0.0,
        };
        assert_eq!(get_f32(&c1_layout, &response, field), expected, "{field}");
    }

    // d1 on the spoke-0 basis 1-form: face 0 contains spoke 0 with +1, face 5
    // closes with spoke 0 as its trailing edge with -1.
    let c2_layout = actor.lane("d1").response.clone();
    let response = actor.call("d1", |layout, bytes| {
        put_f32(layout, bytes, "e0", 1.0);
    });
    for (index, field) in COCHAIN2_FIELDS.iter().enumerate() {
        let expected = match index {
            0 => 1.0,
            5 => -1.0,
            _ => 0.0,
        };
        assert_eq!(get_f32(&c2_layout, &response, field), expected, "{field}");
    }

    // d compose d is EXACTLY zero for every basis 0-form: the incidence
    // arithmetic is integer-valued, so f32 addition is exact here and the
    // assertion is bit-strict on purpose.
    let dd_layout = actor.lane("dd0").response.clone();
    for basis in COCHAIN0_FIELDS {
        let response = actor.call("dd0", |layout, bytes| {
            put_f32(layout, bytes, basis, 1.0);
        });
        for field in COCHAIN2_FIELDS {
            assert_eq!(
                get_f32(&dd_layout, &response, field),
                0.0,
                "d(d({basis})) must vanish exactly at {field}"
            );
        }
    }

    // The composed Laplacian on the center bump. Hand derivation on the unit
    // fan (all triangles equilateral, side 1), positive-semidefinite sign:
    //   lap(center) = (1 / (sqrt3/2)) * 6 * (sqrt3/3) * (1 - 0) = 4
    //   lap(ring)   = (1 / (sqrt3/6)) * (sqrt3/3) * (0 - 1)     = -2
    // The sqrt(3) factors cancel algebraically but not bit-for-bit in f32,
    // hence the tolerance.
    let c0_layout = actor.lane("laplace0").response.clone();
    let response = actor.call("laplace0", |layout, bytes| {
        put_f32(layout, bytes, "v0", 1.0);
    });
    let lap_center = get_f32(&c0_layout, &response, "v0");
    assert!(
        (lap_center - 4.0).abs() < 2e-5,
        "lap(center) = {lap_center}"
    );
    for field in COCHAIN0_FIELDS.into_iter().skip(1) {
        let lap_ring = get_f32(&c0_layout, &response, field);
        assert!((lap_ring + 2.0).abs() < 2e-5, "lap({field}) = {lap_ring}");
    }

    // The Laplacian of a constant is EXACTLY zero: every d0 difference is an
    // exact 0.0, and zero survives the star scalings exactly.
    let response = actor.call("laplace0", |layout, bytes| {
        for field in COCHAIN0_FIELDS {
            put_f32(layout, bytes, field, 1.0);
        }
    });
    for field in COCHAIN0_FIELDS {
        assert_eq!(
            get_f32(&c0_layout, &response, field),
            0.0,
            "lap(constant) must vanish exactly at {field}"
        );
    }

    // The five distinct diagonal Hodge weights of the unit fan hexagon,
    // against independently written decimals (sqrt(3)/2, sqrt(3)/6,
    // sqrt(3)/3, sqrt(3)/6, 4/sqrt(3)). No code is shared with the Fe side.
    let hodge_layout = actor.lane("hodge").response.clone();
    let response = actor.call("hodge", |layout, bytes| {
        put_u32(layout, bytes, "generation", 1);
    });
    let expectations: [(&str, f32); 5] = [
        ("star0_center", 0.866_025_4),
        ("star0_ring", 0.288_675_13),
        ("star1_spoke", 0.577_350_27),
        ("star1_ring", 0.288_675_13),
        ("star2_face", 2.309_401_1),
    ];
    for (field, expected) in expectations {
        let actual = get_f32(&hodge_layout, &response, field);
        assert!(
            (actual - expected).abs() < 1e-6,
            "{field}: expected {expected}, got {actual}"
        );
    }
}

#[test]
fn web_bundle_carries_the_declared_topology() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("../../demos/sketches/dec");
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("dec ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");

    let bundle =
        WebBundle::compile(&db, top_mod, WebBuildOptions::render("dec_render", None)).unwrap();

    assert!(!bundle.wgsl.is_empty(), "render leg must emit WGSL");
    assert!(
        bundle.manifest.canonical_interface.is_none(),
        "the render parent must not publish sibling child behaviors as surface lanes"
    );
    assert!(bundle.interface_js.is_none());
    assert_eq!(bundle.scoped_tasks.len(), 2);

    let child = bundle
        .structured_child
        .as_ref()
        .expect("typed DEC operator child");
    assert_eq!(child.actor, "DecOperator");
    assert_eq!(child.interface.lanes.len(), WASM_LANES.len());
    assert!(child.interface.lanes.iter().all(|lane| {
        lane.name.starts_with("request_")
            && lane.intent.execution == CanonicalExecution::Wasm
            && lane.intent.placement == CanonicalPlacement::Worker
    }));

    let files = bundle.materialized_files().unwrap();
    let paths = files.iter().map(|file| file.path()).collect::<Vec<_>>();
    for expected in [
        "tasks/tasks.js",
        "tasks/materialized-task.js",
        "tasks/host-completion.js",
        "tasks/child.wasm",
        "tasks/interface.js",
        "tasks/runtime/worker-host.js",
        "tasks/runtime/actor-router.js",
    ] {
        assert!(paths.contains(&expected), "missing {expected}: {paths:?}");
    }
    let index = files
        .iter()
        .find(|file| file.path() == "index.html")
        .expect("render index");
    let index = std::str::from_utf8(index.bytes()).unwrap();
    assert!(
        index.contains("scopedTasksUrl: \"./tasks/tasks.js\""),
        "standalone render output must connect its compiler-derived task package"
    );

    let manifest = String::from_utf8(bundle.manifest_json().unwrap()).unwrap();
    assert!(!manifest.contains("DecOperator"));
    assert!(!manifest.contains("tasks.js"));
    assert!(!manifest.contains("child.wasm"));
}

#[test]
fn wrong_grade_is_rejected_by_the_grade_structure() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/dec_wrong_grade_ingot");
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("wrong-grade fixture ingot");
    let top_mod = ingot.root_mod(&db);

    // Reaching format_diags at all is the no-ICE half of the assertion.
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        !diagnostics.is_empty(),
        "applying d at the top grade must be a compile error"
    );
    assert!(
        diagnostics.contains("OnComplex"),
        "the diagnostic must name the unsatisfied grade bound, got:\n{diagnostics}"
    );
    // The rejection must come from the missing OnComplex impl for the
    // successor token, not from a lexical check on the operator name.
    assert!(
        !diagnostics.contains("not found"),
        "rejection must be a trait-solving failure, not a resolution failure:\n{diagnostics}"
    );
}
