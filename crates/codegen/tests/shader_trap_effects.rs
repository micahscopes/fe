//! Artifact gate for the separate post-trap side-effect execution probe.
//! Compilation and trap metadata alone do not prove store suppression.
#![cfg(feature = "spirv-backend")]

use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use sonatina_codegen::isa::spirv::{Access, SpirvExternalResource, SpirvResourceElement, SpirvScalarKind};
use url::Url;

#[test]
fn trapping_helper_with_resource_stores_has_a_diagnostic_channel() {
    let mut db = DriverDataBase::default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/shader_trap_effects");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let package = mir::build_wasm_runtime_package_for_entry(&db, top, "probe").unwrap();
    let artifact = fe_codegen::compile_runtime_package_spirv_compute_with_resources(
        &db, &package, [1, 1, 1], &[SpirvExternalResource {
            arg_index: 0, group: 0, binding: 0, name: "data".into(),
            access: Access::ReadWrite, element: SpirvResourceElement::Scalar(SpirvScalarKind::U32),
            stride: 4, length: 4,
        }],
    ).unwrap();
    assert!(artifact.layout.trap.is_some(), "trapping helper needs a diagnostic channel");
    let resources = [SpirvExternalResource {
        arg_index: 0, group: 0, binding: 0, name: "data".into(),
        access: Access::ReadWrite, element: SpirvResourceElement::Scalar(SpirvScalarKind::U32),
        stride: 4, length: 4,
    }];
    let mut interface = fe_codegen::ComputeShaderInterface {
        workgroup_size: [1, 1, 1],
        dispatch_grid: [1, 1, 1],
        resources: &resources,
        builtin_arguments: &[],
        graph_failure: None,
    };
    let plain = fe_codegen::compile_runtime_package_spirv_compute_with_interface(
        &db, &package, interface,
    ).unwrap();
    assert_eq!(plain.wgsl, artifact.wgsl);
    assert_eq!(plain.as_bytes(), artifact.as_bytes());
    assert_eq!(plain.layout.graph_failure, None);

    interface.graph_failure = Some(sonatina_codegen::isa::naga::GraphFailureBinding {
        group: 0, binding: 2,
    });
    let scoped = fe_codegen::compile_runtime_package_spirv_compute_with_interface(
        &db, &package, interface,
    ).unwrap();
    assert_eq!(scoped.layout.graph_failure, interface.graph_failure);
    let scoped_trap = scoped.layout.trap.as_ref().unwrap();
    let plain_trap = plain.layout.trap.as_ref().unwrap();
    assert_eq!(
        (scoped_trap.group, scoped_trap.binding, scoped_trap.offset, scoped_trap.width),
        (plain_trap.group, plain_trap.binding, plain_trap.offset, plain_trap.width),
    );
    let wgsl = scoped.wgsl.as_ref().unwrap();
    assert!(wgsl.contains("atomicLoad"));
    assert!(wgsl.contains("atomicOr"));
    assert!(!scoped.as_bytes().is_empty());

    // The backend, not a parallel Fe binding classifier, owns collision checks.
    interface.graph_failure = Some(sonatina_codegen::isa::naga::GraphFailureBinding {
        group: 0, binding: 0,
    });
    let error = fe_codegen::compile_runtime_package_spirv_compute_with_interface(
        &db, &package, interface,
    ).err().expect("colliding epoch resource must fail closed");
    assert!(error.to_string().contains("collid"), "{error}");
    if let Some(directory) = std::env::var_os("FE_TRAP_EFFECTS_ARTIFACT_DIR") {
        let directory = std::path::Path::new(&directory);
        std::fs::write(directory.join("shader.spv"), artifact.as_bytes()).unwrap();
        std::fs::write(directory.join("shader.wgsl"), artifact.wgsl.as_ref().unwrap()).unwrap();
        std::fs::write(directory.join("layout.txt"), format!("{:#?}", artifact.layout)).unwrap();
        std::fs::write(directory.join("scoped.wgsl"), scoped.wgsl.as_ref().unwrap()).unwrap();
        std::fs::write(directory.join("scoped.spv"), scoped.as_bytes()).unwrap();
        std::fs::write(directory.join("scoped-layout.txt"), format!("{:#?}", scoped.layout)).unwrap();
    }
}
