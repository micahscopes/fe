mod lower_runtime;

use std::collections::{BTreeMap, VecDeque};

use common::{facts::TypedFactSet, ingot::Ingot};
use driver::DriverDataBase;
use hir::hir_def::{HirIngot, TopLevelMod};
use mir::runtime::ir::RuntimePackagePlan;
use mir::{RuntimePackage, build_runtime_package, build_test_runtime_package};
use rustc_hash::FxHashSet;
use sonatina_codegen::{EvmCompile, OptLevel as SonatinaOptLevel};
use sonatina_ir::{
    Module,
    ir_writer::{FuncWriter, ModuleWriter},
    isa::evm::Evm,
    module::{FuncRef, ModuleCtx},
};
use sonatina_triple::{Architecture, EvmVersion, OperatingSystem, TargetTriple, Vendor};
use sonatina_verifier::{
    Location, VerificationLevel, VerificationReport, VerifierConfig, verify_module,
};

use crate::{
    OptLevel, TargetDataLayout, TestMetadata, TestModuleOutput,
    debug::{
        BytecodeSourceMapEntry, BytecodeSourceMapFilter, bytecode_source_map_entries,
        bytecode_source_map_entries_summary, bytecode_source_span_exports,
        bytecode_source_span_exports_for_object,
    },
    origin::{
        BytecodeObjectKey, BytecodePackageOrigins, BytecodeSectionKey, BytecodeSectionNameKey,
        FrontendOriginLabelMap, SonatinaFunctionExportKey, SonatinaPackageOrigins,
        SonatinaPostOptPackageOrigins,
    },
    runtime_package::ensure_runtime_package_has_roots,
    test_output::{TestRootMetadataError, runtime_test_root_metadata},
};

#[derive(Debug)]
pub enum LowerError {
    RuntimeLower(mir::LowerError),
    Unsupported(String),
    Internal(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::RuntimeLower(err) => write!(f, "{err}"),
            LowerError::Unsupported(message) => write!(f, "unsupported: {message}"),
            LowerError::Internal(message) => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for LowerError {}

impl From<mir::LowerError> for LowerError {
    fn from(err: mir::LowerError) -> Self {
        LowerError::RuntimeLower(err)
    }
}

#[derive(Debug, Clone)]
pub struct SonatinaContractBytecode {
    pub deploy: Vec<u8>,
    pub runtime: Vec<u8>,
    pub source_map_entries: Vec<BytecodeSourceMapEntry>,
    pub bytecode_origin_coverage: Option<crate::origin::BytecodeOriginCoverage>,
    pub post_opt_origin_coverage: Option<crate::origin::SonatinaPostOptOriginCoverage>,
    pub origin_facts: Option<TypedFactSet>,
    pub snapshot_origin_facts: Option<TypedFactSet>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SonatinaTestOptions {
    pub emit_observability: bool,
}

pub(crate) fn create_evm_isa() -> Evm {
    Evm::new(TargetTriple::new(
        Architecture::Evm,
        Vendor::Ethereum,
        OperatingSystem::Evm(EvmVersion::Osaka),
    ))
}

fn create_module_ctx() -> ModuleCtx {
    ModuleCtx::new(&create_evm_isa())
}

fn ensure_module_sonatina_ir_valid(module: &Module) -> Result<(), LowerError> {
    let report = verify_module(module, &VerifierConfig::for_level(VerificationLevel::Full));
    if report.has_errors() {
        return Err(LowerError::Internal(format_verification_report(
            module, &report,
        )));
    }
    Ok(())
}

fn format_verification_report(module: &Module, report: &VerificationReport) -> String {
    const MAX_FUNC_CONTEXTS: usize = 3;

    let mut out = report.to_string();
    let funcs = failing_function_contexts(module, report);
    if funcs.is_empty() {
        return out;
    }

    out.push_str("\n\nVerifier function IR context");
    if funcs.len() > MAX_FUNC_CONTEXTS {
        out.push_str(&format!(
            " (showing first {MAX_FUNC_CONTEXTS} of {})",
            funcs.len()
        ));
    }
    out.push_str(":\n");

    for (func_ref, func_name, func_ir) in funcs.into_iter().take(MAX_FUNC_CONTEXTS) {
        out.push_str(&format!(
            "\n---- func{} (%{func_name}) ----\n{func_ir}\n",
            func_ref.as_u32()
        ));
    }

    out
}

fn failing_function_contexts(
    module: &Module,
    report: &VerificationReport,
) -> Vec<(FuncRef, String, String)> {
    let mut funcs = Vec::new();
    for diagnostic in report.errors() {
        let Some(func_ref) = diagnostic_func_ref(&diagnostic.primary) else {
            continue;
        };
        if funcs.iter().any(|(existing, _, _)| *existing == func_ref)
            || !module.func_store.contains(func_ref)
        {
            continue;
        }
        let Some(func_name) = module
            .ctx
            .get_sig(func_ref)
            .map(|sig| sig.name().to_string())
        else {
            continue;
        };
        let func_ir = module.func_store.view(func_ref, |func| {
            FuncWriter::new(func_ref, func).dump_string()
        });
        funcs.push((func_ref, func_name, func_ir));
    }
    funcs
}

fn diagnostic_func_ref(location: &Location) -> Option<FuncRef> {
    match location {
        Location::Function(func)
        | Location::Block { func, .. }
        | Location::Inst { func, .. }
        | Location::Value { func, .. } => Some(*func),
        Location::Type {
            func: Some(func), ..
        } => Some(*func),
        Location::Module
        | Location::Global(_)
        | Location::Object { .. }
        | Location::Type { func: None, .. } => None,
    }
}

fn to_sonatina_opt_level(opt_level: OptLevel) -> SonatinaOptLevel {
    match opt_level {
        OptLevel::O0 => SonatinaOptLevel::O0,
        OptLevel::O1 => SonatinaOptLevel::O1,
        OptLevel::Os => SonatinaOptLevel::Os,
        OptLevel::O2 => SonatinaOptLevel::O2,
    }
}

fn evm_compile(module: Module, opt_level: OptLevel, emit_observability: bool) -> EvmCompile {
    EvmCompile::new(module)
        .with_opt_level(to_sonatina_opt_level(opt_level))
        .with_observability(emit_observability)
}

fn format_object_compile_errors(errors: &[sonatina_codegen::object::ObjectCompileError]) -> String {
    errors
        .iter()
        .map(|error| format!("{error:?}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn compile_runtime_objects(
    module: Module,
    opt_level: OptLevel,
    emit_observability: bool,
) -> Result<Vec<sonatina_codegen::object::ObjectArtifact>, LowerError> {
    let mut compile = evm_compile(module, opt_level, emit_observability);
    ensure_module_sonatina_ir_valid(compile.optimize())?;
    compile
        .compile()
        .map_err(|errors| LowerError::Internal(format_object_compile_errors(&errors)))
}

#[derive(Debug)]
struct RuntimeObjectCompilation<'db> {
    artifacts: Vec<sonatina_codegen::object::ObjectArtifact>,
    post_opt_origins: SonatinaPostOptPackageOrigins<'db>,
    bytecode_origins: BytecodePackageOrigins<'db>,
    function_keys: BTreeMap<FuncRef, SonatinaFunctionExportKey>,
    frontend_origin_labels: FrontendOriginLabelMap,
    origin_lifetime: std::marker::PhantomData<&'db ()>,
}

fn compile_runtime_objects_with_origins<'db>(
    module: Module,
    opt_level: OptLevel,
    emit_observability: bool,
    pre_opt_origins: &SonatinaPackageOrigins<'db>,
) -> Result<RuntimeObjectCompilation<'db>, LowerError> {
    let mut compile = evm_compile(module, opt_level, emit_observability);
    let optimized = compile.optimize();
    ensure_module_sonatina_ir_valid(optimized)?;
    let function_keys = sonatina_function_keys(optimized);
    let post_opt_origins = SonatinaPostOptPackageOrigins::from_module(optimized, pre_opt_origins);
    let artifacts = compile
        .compile()
        .map_err(|errors| LowerError::Internal(format_object_compile_errors(&errors)))?;
    let bytecode_origins = BytecodePackageOrigins::from_artifacts(&artifacts, &post_opt_origins);
    let frontend_origin_labels =
        bytecode_origins.frontend_origin_label_map(|func| function_keys.get(&func).cloned());

    Ok(RuntimeObjectCompilation {
        artifacts,
        post_opt_origins,
        bytecode_origins,
        function_keys,
        frontend_origin_labels,
        origin_lifetime: std::marker::PhantomData,
    })
}

fn sonatina_function_keys(module: &Module) -> BTreeMap<FuncRef, SonatinaFunctionExportKey> {
    module
        .funcs()
        .into_iter()
        .map(|func| {
            let name = module.ctx.func_sig(func, |sig| sig.name().to_string());
            (func, SonatinaFunctionExportKey::new(name))
        })
        .collect()
}

fn artifact_observability_json_with_origins(
    artifact: &sonatina_codegen::object::ObjectArtifact,
    frontend_origin_labels: &FrontendOriginLabelMap,
) -> Option<String> {
    let mut observability = artifact.observability()?;
    observability
        .apply_frontend_provenance(frontend_origin_labels.as_sonatina_frontend_provenance());
    Some(observability.to_json())
}

fn section_name_for_runtime(name: &mir::RuntimeSectionName) -> sonatina_ir::SectionName {
    match name {
        mir::RuntimeSectionName::Init => "init".into(),
        mir::RuntimeSectionName::Runtime => "runtime".into(),
        mir::RuntimeSectionName::Main => "main".into(),
        mir::RuntimeSectionName::Test(name) => format!("test_{name}").into(),
        mir::RuntimeSectionName::CodeRegion(symbol) => format!("code_region_{symbol}").into(),
    }
}

fn wrap_as_init_code(runtime: &[u8]) -> Vec<u8> {
    fn push_u256(mut value: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        while value > 0 {
            bytes.push((value & 0xff) as u8);
            value >>= 8;
        }
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes.reverse();
        let mut out = Vec::with_capacity(1 + bytes.len());
        out.push(0x5f + bytes.len() as u8);
        out.extend(bytes);
        out
    }

    let len_push = push_u256(runtime.len());
    let mut init = Vec::with_capacity(32 + runtime.len());
    init.extend(len_push.clone());
    init.push(0x61);
    let off_pos = init.len();
    init.extend([0, 0]);
    init.extend([0x60, 0x00]);
    init.push(0x39);
    init.extend(len_push);
    init.extend([0x60, 0x00]);
    init.push(0xf3);
    let off = init.len();
    init[off_pos] = ((off >> 8) & 0xff) as u8;
    init[off_pos + 1] = (off & 0xff) as u8;
    init.extend_from_slice(runtime);
    init
}

pub fn compile_runtime_package_sonatina(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    layout: TargetDataLayout,
) -> Result<Module, LowerError> {
    lower_runtime::compile_runtime_package_sonatina(db, package, layout)
}

pub fn compile_runtime_package_sonatina_with_origins<'db>(
    db: &'db DriverDataBase,
    package: &RuntimePackage<'db>,
    layout: TargetDataLayout,
) -> Result<(Module, SonatinaPackageOrigins<'db>), LowerError> {
    lower_runtime::compile_runtime_package_sonatina_with_origins(db, package, layout)
}

fn select_runtime_package_contract<'db>(
    db: &'db dyn mir::MirDb,
    package: RuntimePackage<'db>,
    contract: Option<&str>,
) -> Result<RuntimePackage<'db>, LowerError> {
    let Some(contract) = contract else {
        return Ok(package);
    };
    let matches = root_objects_named(db, package, contract);
    match matches.as_slice() {
        [] => Err(LowerError::Internal(format!(
            "root object `{contract}` not found in runtime package"
        ))),
        [root] => Ok(filter_runtime_package_to_root_objects(
            db,
            package,
            &[*root],
        )),
        _ => Err(LowerError::Internal(format!(
            "multiple root objects named `{contract}` in runtime package"
        ))),
    }
}

fn select_ingot_runtime_packages<'db>(
    db: &'db dyn mir::MirDb,
    ingot: Ingot<'db>,
    contract: Option<&str>,
) -> Result<Vec<RuntimePackage<'db>>, LowerError> {
    let mut packages = Vec::new();
    for &top_mod in ingot.all_modules(db) {
        let package = build_runtime_package(db, top_mod)?;
        if package.root_objects(db).is_empty() {
            continue;
        }
        let Some(contract) = contract else {
            packages.push(package);
            continue;
        };
        let matches = root_objects_named(db, package, contract);
        if matches.len() > 1 {
            return Err(LowerError::Internal(format!(
                "multiple root objects named `{contract}` in runtime package"
            )));
        }
        if let Some(root) = matches.first().copied() {
            packages.push(filter_runtime_package_to_root_objects(db, package, &[root]));
        }
    }
    if let Some(contract) = contract {
        if packages.is_empty() {
            return Err(LowerError::Internal(format!(
                "root object `{contract}` not found in ingot runtime packages"
            )));
        }
        if packages.len() > 1 {
            return Err(LowerError::Internal(format!(
                "duplicate root object `{contract}` across ingot modules"
            )));
        }
    }
    Ok(packages)
}

fn root_objects_named<'db>(
    db: &'db dyn mir::MirDb,
    package: RuntimePackage<'db>,
    name: &str,
) -> Vec<mir::RuntimeObject<'db>> {
    package
        .root_objects(db)
        .into_iter()
        .filter(|object| object.name(db) == name)
        .collect()
}

fn filter_runtime_package_to_root_objects<'db>(
    db: &'db dyn mir::MirDb,
    package: RuntimePackage<'db>,
    roots: &[mir::RuntimeObject<'db>],
) -> RuntimePackage<'db> {
    let root_names = roots
        .iter()
        .map(|object| object.name(db).clone())
        .collect::<FxHashSet<_>>();
    let package_objects = package.objects(db);
    let section_set = reachable_sections(db, &package_objects, roots);
    let objects = package
        .objects(db)
        .into_iter()
        .filter_map(|object| {
            let sections = object
                .sections(db)
                .into_iter()
                .filter(|section| {
                    section_set.contains(&runtime_section_key(db, object, &section.name))
                })
                .collect::<Vec<_>>();
            (!sections.is_empty())
                .then(|| mir::RuntimeObject::new(db, object.name(db).clone(), sections))
        })
        .collect::<Vec<_>>();
    let function_set = reachable_functions(db, &objects);
    let functions = package
        .functions(db)
        .into_iter()
        .filter(|function| function_set.contains(&function.instance(db)))
        .collect::<Vec<_>>();
    let const_region_set = reachable_const_regions(db, &objects, &functions);
    let const_regions = package
        .const_regions(db)
        .into_iter()
        .filter(|region| const_region_set.contains(region))
        .collect::<Vec<_>>();
    let code_regions = package
        .code_regions(db)
        .into_iter()
        .filter(|region| section_set.contains(&section_ref_key(db, region.source(db))))
        .collect::<Vec<_>>();
    let root_objects = package
        .objects(db)
        .into_iter()
        .filter(|object| root_names.contains(&object.name(db)))
        .filter_map(|object| {
            objects
                .iter()
                .find(|filtered| filtered.name(db) == object.name(db))
                .copied()
        })
        .collect::<Vec<_>>();
    let primary_object = package
        .primary_object(db)
        .filter(|object| root_names.contains(&object.name(db)))
        .and_then(|object| {
            objects
                .iter()
                .find(|filtered| filtered.name(db) == object.name(db))
                .copied()
        })
        .or_else(|| root_objects.first().copied());

    RuntimePackage::new(
        db,
        package.top_mod(db),
        functions,
        RuntimePackagePlan::new(
            db,
            objects,
            const_regions,
            code_regions,
            root_objects,
            primary_object,
        ),
    )
}

fn reachable_sections<'db>(
    db: &'db dyn mir::MirDb,
    objects: &[mir::RuntimeObject<'db>],
    roots: &[mir::RuntimeObject<'db>],
) -> FxHashSet<(String, mir::RuntimeSectionName)> {
    let mut seen = FxHashSet::default();
    let mut queue = roots
        .iter()
        .flat_map(|object| {
            object
                .sections(db)
                .into_iter()
                .map(|section| runtime_section_key(db, *object, &section.name))
        })
        .collect::<VecDeque<_>>();
    while let Some((object_name, section_name)) = queue.pop_front() {
        if !seen.insert((object_name.clone(), section_name.clone())) {
            continue;
        }
        for section in objects
            .iter()
            .flat_map(|object| {
                object
                    .sections(db)
                    .into_iter()
                    .map(move |section| (*object, section))
            })
            .filter(|(object, _)| object.name(db) == object_name)
            .filter(|(_, section)| section.name == section_name)
            .map(|(_, section)| section)
        {
            for embed in section.embeds {
                queue.push_back(section_ref_key(db, embed.source));
            }
        }
    }
    seen
}

fn runtime_section_key<'db>(
    db: &'db dyn mir::MirDb,
    object: mir::RuntimeObject<'db>,
    section: &mir::RuntimeSectionName,
) -> (String, mir::RuntimeSectionName) {
    (object.name(db).clone(), section.clone())
}

fn section_ref_key<'db>(
    db: &'db dyn mir::MirDb,
    section_ref: mir::RuntimeSectionRef<'db>,
) -> (String, mir::RuntimeSectionName) {
    match section_ref {
        mir::RuntimeSectionRef::Local { object, section }
        | mir::RuntimeSectionRef::External { object, section } => {
            runtime_section_key(db, object, &section)
        }
    }
}

fn reachable_functions<'db>(
    db: &'db dyn mir::MirDb,
    objects: &[mir::RuntimeObject<'db>],
) -> FxHashSet<mir::RuntimeInstance<'db>> {
    let mut seen = FxHashSet::default();
    let mut queue = objects
        .iter()
        .flat_map(|object| object.sections(db))
        .map(|section| section.entry.instance(db))
        .collect::<VecDeque<_>>();
    while let Some(instance) = queue.pop_front() {
        if !seen.insert(instance) {
            continue;
        }
        for call in instance.calls(db) {
            queue.push_back(call.callee);
        }
    }
    seen
}

fn reachable_const_regions<'db>(
    db: &'db dyn mir::MirDb,
    objects: &[mir::RuntimeObject<'db>],
    functions: &[mir::RuntimeFunction<'db>],
) -> FxHashSet<mir::ConstRegionId<'db>> {
    let mut seen = FxHashSet::default();
    for section in objects.iter().flat_map(|object| object.sections(db)) {
        seen.extend(section.const_regions);
    }
    for function in functions {
        seen.extend(function.referenced_const_regions(db));
    }
    seen
}

pub fn emit_runtime_package_sonatina_ir(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    layout: TargetDataLayout,
) -> Result<String, LowerError> {
    ensure_runtime_package_has_roots(db, package, "Sonatina IR")?;
    let module = compile_runtime_package_sonatina(db, package, layout)?;
    let mut writer = ModuleWriter::new(&module);
    Ok(writer.dump_string())
}

pub fn emit_runtime_package_sonatina_ir_optimized(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    layout: TargetDataLayout,
    opt_level: OptLevel,
) -> Result<String, LowerError> {
    ensure_runtime_package_has_roots(db, package, "Sonatina IR")?;
    let module = compile_runtime_package_sonatina(db, package, layout)?;
    ensure_module_sonatina_ir_valid(&module)?;
    let mut compile = evm_compile(module, opt_level, false);
    let optimized = compile.optimize();
    ensure_module_sonatina_ir_valid(optimized)?;
    let mut writer = ModuleWriter::new(optimized);
    Ok(writer.dump_string())
}

pub fn emit_runtime_package_sonatina_bytecode(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    layout: TargetDataLayout,
    opt_level: OptLevel,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    emit_runtime_package_sonatina_bytecode_impl(db, package, layout, opt_level, false)
}

pub fn emit_runtime_package_sonatina_bytecode_with_source_maps<'db>(
    db: &'db DriverDataBase,
    package: &RuntimePackage<'db>,
    layout: TargetDataLayout,
    opt_level: OptLevel,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    emit_runtime_package_sonatina_bytecode_impl(db, package, layout, opt_level, true)
}

fn emit_runtime_package_sonatina_bytecode_impl<'db>(
    db: &'db DriverDataBase,
    package: &RuntimePackage<'db>,
    layout: TargetDataLayout,
    opt_level: OptLevel,
    emit_source_maps: bool,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    ensure_runtime_package_has_roots(db, package, "Sonatina bytecode")?;
    let (
        artifacts,
        mut source_map_entries_by_object,
        mut bytecode_origin_coverage_by_object,
        mut post_opt_origin_coverage_by_object,
        mut origin_facts_by_object,
        mut snapshot_origin_facts_by_object,
    ) = if emit_source_maps {
        let (module, pre_opt_origins) =
            compile_runtime_package_sonatina_with_origins(db, package, layout)?;
        ensure_module_sonatina_ir_valid(&module)?;
        let compiled =
            compile_runtime_objects_with_origins(module, opt_level, true, &pre_opt_origins)?;
        let runtime_origins = mir::runtime_package_origins(db, *package);
        let source_resolutions = compiled
            .bytecode_origins
            .resolve_source_spans(db, &runtime_origins);
        let mut entries_by_object = BTreeMap::<String, Vec<BytecodeSourceMapEntry>>::new();
        for entry in bytecode_source_map_entries(db, &source_resolutions, None) {
            entries_by_object
                .entry(entry.object().to_string())
                .or_default()
                .push(entry);
        }

        let mut facts_by_object = BTreeMap::new();
        let mut snapshot_facts_by_object = BTreeMap::new();
        let mut coverage_by_object = BTreeMap::new();
        let mut post_opt_coverage_by_object = BTreeMap::new();
        for object in package.root_objects(db) {
            let object_name = object.name(db).clone();
            let object_key = BytecodeObjectKey::new(object_name.clone());
            let coverage = compiled.bytecode_origins.coverage_for_object(&object_key);
            if !coverage.is_empty() {
                coverage_by_object.insert(object_name.clone(), coverage);
            }
            let post_opt_coverage = compiled
                .bytecode_origins
                .post_opt_origin_coverage_for_object(&object_key, &compiled.post_opt_origins);
            if !post_opt_coverage.is_empty() {
                post_opt_coverage_by_object.insert(object_name.clone(), post_opt_coverage);
            }
            if let Some(facts) = compiled
                .bytecode_origins
                .end_to_end_origin_facts_for_object(
                    &object_key,
                    &pre_opt_origins,
                    &runtime_origins,
                    |func| compiled.function_keys.get(&func).cloned(),
                )
                .map_err(|err| LowerError::Internal(err.to_string()))?
            {
                let facts = facts
                    .with_source_spans(bytecode_source_span_exports_for_object(
                        db,
                        &source_resolutions,
                        &object_key,
                    ))
                    .map_err(|err| LowerError::Internal(err.to_string()))?;
                facts_by_object.insert(object_name.clone(), facts);
            }
            if let Some(facts) = compiled
                .bytecode_origins
                .post_opt_snapshot_origin_facts_for_object(
                    &object_key,
                    &compiled.post_opt_origins,
                    |func| compiled.function_keys.get(&func).cloned(),
                )
                .map_err(|err| LowerError::Internal(err.to_string()))?
            {
                snapshot_facts_by_object.insert(object_name, facts);
            }
        }

        (
            compiled.artifacts,
            entries_by_object,
            coverage_by_object,
            post_opt_coverage_by_object,
            facts_by_object,
            snapshot_facts_by_object,
        )
    } else {
        let module = compile_runtime_package_sonatina(db, package, layout)?;
        ensure_module_sonatina_ir_valid(&module)?;
        (
            compile_runtime_objects(module, opt_level, false)?,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
    };
    let artifacts_by_name = artifacts
        .iter()
        .map(|artifact| (artifact.object.0.as_str(), artifact))
        .collect::<std::collections::HashMap<_, _>>();

    let mut out = BTreeMap::new();
    for object in package.root_objects(db) {
        let object_name = object.name(db);
        let artifact = artifacts_by_name
            .get(object_name.as_str())
            .copied()
            .ok_or_else(|| {
                LowerError::Internal(format!("compiled object `{object_name}` not found"))
            })?;
        let init = artifact
            .sections
            .get(&section_name_for_runtime(&mir::RuntimeSectionName::Init));
        let runtime = artifact
            .sections
            .get(&section_name_for_runtime(&mir::RuntimeSectionName::Runtime));
        let (deploy, runtime) = match (init, runtime) {
            (Some(init), Some(runtime)) => (init.bytes.clone(), runtime.bytes.clone()),
            _ => {
                let sections = object.sections(db);
                let section = sections.first().ok_or_else(|| {
                    LowerError::Internal(format!("root object `{object_name}` has no sections"))
                })?;
                let runtime = artifact
                    .sections
                    .get(&section_name_for_runtime(&section.name))
                    .ok_or_else(|| {
                        LowerError::Internal(format!(
                            "compiled object `{object_name}` is missing section `{:?}`",
                            section.name
                        ))
                    })?
                    .bytes
                    .clone();
                (wrap_as_init_code(&runtime), runtime)
            }
        };
        out.insert(
            object_name.clone(),
            SonatinaContractBytecode {
                deploy,
                runtime,
                source_map_entries: source_map_entries_by_object
                    .remove(object_name.as_str())
                    .unwrap_or_default(),
                bytecode_origin_coverage: bytecode_origin_coverage_by_object
                    .remove(object_name.as_str()),
                post_opt_origin_coverage: post_opt_origin_coverage_by_object
                    .remove(object_name.as_str()),
                origin_facts: origin_facts_by_object.remove(object_name.as_str()),
                snapshot_origin_facts: snapshot_origin_facts_by_object.remove(object_name.as_str()),
            },
        );
    }
    Ok(out)
}

pub fn emit_module_sonatina_ir(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<String, LowerError> {
    let package = build_runtime_package(db, top_mod)?;
    emit_runtime_package_sonatina_ir(db, &package, crate::EVM_LAYOUT)
}

pub fn emit_module_sonatina_ir_optimized(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    opt_level: OptLevel,
    contract: Option<&str>,
) -> Result<String, LowerError> {
    let package = build_runtime_package(db, top_mod)?;
    let package = select_runtime_package_contract(db, package, contract)?;
    emit_runtime_package_sonatina_ir_optimized(db, &package, crate::EVM_LAYOUT, opt_level)
}

pub fn emit_ingot_sonatina_ir(db: &DriverDataBase, ingot: Ingot<'_>) -> Result<String, LowerError> {
    let mut modules = Vec::new();
    for &top_mod in ingot.all_modules(db) {
        let package = build_runtime_package(db, top_mod)?;
        if package.root_objects(db).is_empty() {
            continue;
        }
        modules.push(emit_runtime_package_sonatina_ir(
            db,
            &package,
            crate::EVM_LAYOUT,
        )?);
    }
    if modules.is_empty() {
        return Err(mir::LowerError::Unsupported(
            "runtime package has no root objects; refusing to emit target-only Sonatina IR"
                .to_string(),
        )
        .into());
    }
    Ok(modules.join("\n\n"))
}

pub fn emit_ingot_sonatina_ir_optimized(
    db: &DriverDataBase,
    ingot: Ingot<'_>,
    opt_level: OptLevel,
    contract: Option<&str>,
) -> Result<String, LowerError> {
    let mut modules = Vec::new();
    for package in select_ingot_runtime_packages(db, ingot, contract)? {
        modules.push(emit_runtime_package_sonatina_ir_optimized(
            db,
            &package,
            crate::EVM_LAYOUT,
            opt_level,
        )?);
    }
    if modules.is_empty() {
        return Err(mir::LowerError::Unsupported(
            "runtime package has no root objects; refusing to emit target-only Sonatina IR"
                .to_string(),
        )
        .into());
    }
    Ok(modules.join("\n\n"))
}

pub fn validate_module_sonatina_ir(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<String, LowerError> {
    let package = build_runtime_package(db, top_mod)?;
    compile_runtime_package_sonatina(db, &package, crate::EVM_LAYOUT)?;
    Ok("ok\n".to_string())
}

pub fn emit_module_sonatina_bytecode(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    opt_level: OptLevel,
    contract: Option<&str>,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    let package = build_runtime_package(db, top_mod)?;
    let package = select_runtime_package_contract(db, package, contract)?;
    emit_runtime_package_sonatina_bytecode(db, &package, crate::EVM_LAYOUT, opt_level)
}

pub fn emit_module_sonatina_bytecode_with_source_maps<'db>(
    db: &'db DriverDataBase,
    top_mod: TopLevelMod<'db>,
    opt_level: OptLevel,
    contract: Option<&str>,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    let package = build_runtime_package(db, top_mod)?;
    let package = select_runtime_package_contract(db, package, contract)?;
    emit_runtime_package_sonatina_bytecode_with_source_maps(
        db,
        &package,
        crate::EVM_LAYOUT,
        opt_level,
    )
}

pub fn emit_ingot_sonatina_bytecode(
    db: &DriverDataBase,
    ingot: Ingot<'_>,
    opt_level: OptLevel,
    contract: Option<&str>,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    emit_ingot_sonatina_bytecode_impl(db, ingot, opt_level, contract, false)
}

pub fn emit_ingot_sonatina_bytecode_with_source_maps<'db>(
    db: &'db DriverDataBase,
    ingot: Ingot<'db>,
    opt_level: OptLevel,
    contract: Option<&str>,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    emit_ingot_sonatina_bytecode_impl(db, ingot, opt_level, contract, true)
}

fn emit_ingot_sonatina_bytecode_impl<'db>(
    db: &'db DriverDataBase,
    ingot: Ingot<'db>,
    opt_level: OptLevel,
    contract: Option<&str>,
    emit_source_maps: bool,
) -> Result<BTreeMap<String, SonatinaContractBytecode>, LowerError> {
    let mut outputs = BTreeMap::new();
    for package in select_ingot_runtime_packages(db, ingot, contract)? {
        let package_outputs = if emit_source_maps {
            emit_runtime_package_sonatina_bytecode_with_source_maps(
                db,
                &package,
                crate::EVM_LAYOUT,
                opt_level,
            )?
        } else {
            emit_runtime_package_sonatina_bytecode(db, &package, crate::EVM_LAYOUT, opt_level)?
        };
        for (name, bytecode) in package_outputs {
            if outputs.insert(name.clone(), bytecode).is_some() {
                return Err(LowerError::Internal(format!(
                    "duplicate root object `{name}` across ingot modules"
                )));
            }
        }
    }
    if outputs.is_empty() {
        return Err(mir::LowerError::Unsupported(
            "runtime package has no root objects; refusing to emit target-only Sonatina bytecode"
                .to_string(),
        )
        .into());
    }
    Ok(outputs)
}

pub fn emit_test_module_sonatina(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    opt_level: OptLevel,
    options: SonatinaTestOptions,
    filter: Option<&str>,
) -> Result<TestModuleOutput, LowerError> {
    let package = build_test_runtime_package(db, top_mod, filter)?;
    if package.root_objects(db).is_empty() {
        return Ok(TestModuleOutput { tests: Vec::new() });
    }
    let (module, pre_opt_origins) =
        compile_runtime_package_sonatina_with_origins(db, &package, crate::EVM_LAYOUT)?;
    ensure_module_sonatina_ir_valid(&module)?;
    let compiled = compile_runtime_objects_with_origins(
        module,
        opt_level,
        options.emit_observability,
        &pre_opt_origins,
    )?;
    let artifacts_by_name = compiled
        .artifacts
        .iter()
        .map(|artifact| (artifact.object.0.as_str(), artifact))
        .collect::<std::collections::HashMap<_, _>>();
    let runtime_origins = mir::runtime_package_origins(db, package);
    let source_resolutions = compiled
        .bytecode_origins
        .resolve_source_spans(db, &runtime_origins);

    let mut tests = Vec::new();
    for object in package.root_objects(db) {
        let sections = object.sections(db);
        let Some(section) = sections.first() else {
            continue;
        };
        let mir::RuntimeSectionName::Test(_) = &section.name else {
            continue;
        };
        let compiled_section_name = section_name_for_runtime(&section.name);
        let artifact = artifacts_by_name
            .get(object.name(db).as_str())
            .copied()
            .ok_or_else(|| {
                LowerError::Internal(format!("compiled object `{}` not found", object.name(db)))
            })?;
        let runtime = artifact
            .sections
            .get(&compiled_section_name)
            .ok_or_else(|| {
                LowerError::Internal(format!(
                    "compiled object `{}` missing test section",
                    object.name(db)
                ))
            })?;
        let metadata = runtime_test_root_metadata(db, &section.entry.owner(db), &section.name)
            .map_err(|err| match err {
                TestRootMetadataError::InvalidPackage(message) => LowerError::Internal(message),
                TestRootMetadataError::Unsupported(message) => LowerError::Unsupported(message),
            })?;
        let object_name = object.name(db).clone();
        let bytecode_section_key = BytecodeSectionKey::new(
            BytecodeObjectKey::new(object_name.clone()),
            BytecodeSectionNameKey::new(compiled_section_name.0.clone()),
        );
        let source_map_filter = BytecodeSourceMapFilter::new(bytecode_section_key.clone());
        let sonatina_bytecode_origin_coverage = compiled
            .bytecode_origins
            .coverage_for_section(&bytecode_section_key);
        let sonatina_post_opt_origin_coverage = compiled
            .bytecode_origins
            .post_opt_origin_coverage_for_section(
                &bytecode_section_key,
                &compiled.post_opt_origins,
            );
        let sonatina_source_map_entries =
            bytecode_source_map_entries(db, &source_resolutions, Some(&source_map_filter));
        let sonatina_source_spans =
            bytecode_source_span_exports(db, &source_resolutions, Some(&source_map_filter));
        let sonatina_origin_facts = compiled
            .bytecode_origins
            .end_to_end_origin_facts_for_object(
                &BytecodeObjectKey::new(object_name.clone()),
                &pre_opt_origins,
                &runtime_origins,
                |func| compiled.function_keys.get(&func).cloned(),
            )
            .map_err(|err| LowerError::Internal(err.to_string()))?
            .map(|facts| facts.with_source_spans(sonatina_source_spans))
            .transpose()
            .map_err(|err| LowerError::Internal(err.to_string()))?;
        let sonatina_snapshot_origin_facts = compiled
            .bytecode_origins
            .post_opt_snapshot_origin_facts_for_object(
                &BytecodeObjectKey::new(object_name.clone()),
                &compiled.post_opt_origins,
                |func| compiled.function_keys.get(&func).cloned(),
            )
            .map_err(|err| LowerError::Internal(err.to_string()))?;
        tests.push(TestMetadata {
            display_name: metadata.display_name,
            hir_name: metadata.hir_name,
            symbol_name: section.entry.symbol(db).clone(),
            object_name: object_name.clone(),
            bytecode: wrap_as_init_code(&runtime.bytes),
            sonatina_observability_json: artifact_observability_json_with_origins(
                artifact,
                &compiled.frontend_origin_labels,
            ),
            sonatina_source_map_summary: bytecode_source_map_entries_summary(
                &sonatina_source_map_entries,
                Some(source_map_filter.metadata()),
            ),
            sonatina_source_map_entries,
            sonatina_bytecode_origin_coverage: (!sonatina_bytecode_origin_coverage.is_empty())
                .then_some(sonatina_bytecode_origin_coverage),
            sonatina_post_opt_origin_coverage: (!sonatina_post_opt_origin_coverage.is_empty())
                .then_some(sonatina_post_opt_origin_coverage),
            sonatina_origin_facts,
            sonatina_snapshot_origin_facts,
            value_param_count: 0,
            effect_param_count: 0,
            init_bytecode: Vec::new(),
            expected_revert: metadata.expected_revert,
            initial_balance: metadata.initial_balance,
        });
    }
    Ok(TestModuleOutput { tests })
}

pub fn emit_test_ingot_sonatina(
    db: &DriverDataBase,
    ingot: Ingot<'_>,
    opt_level: OptLevel,
    options: SonatinaTestOptions,
    filter: Option<&str>,
) -> Result<TestModuleOutput, LowerError> {
    let mut top_mods = ingot.all_modules(db).to_vec();
    top_mods.sort_by(|left, right| left.name(db).cmp(&right.name(db)));

    let mut output = TestModuleOutput { tests: Vec::new() };
    for top_mod in top_mods {
        output.extend(emit_test_module_sonatina(
            db, top_mod, opt_level, options, filter,
        )?);
    }
    output.sort_tests();
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origin::{
        BytecodeOriginSource, BytecodeSourceResolutionResult, CodegenOriginNode, SonatinaInstStage,
        SonatinaOriginSource, SonatinaPostOptOriginSource, SonatinaPostOptPackageOrigins,
    };
    use common::{
        InputDb,
        diagnostics::SpanKind,
        facts::OriginFactIndex,
        origin::{OriginExportKind, OriginLinkKind},
    };
    use driver::DriverDataBase;
    use std::collections::BTreeSet;
    use std::{fs, path::PathBuf};
    use url::Url;

    fn temp_fixture_url(name: &str) -> Url {
        let fixture_path = std::env::temp_dir().join(name);
        Url::from_file_path(&fixture_path).expect("fixture path should be absolute")
    }

    #[test]
    fn fe_opt_levels_map_to_sonatina_opt_levels() {
        assert_eq!(to_sonatina_opt_level(OptLevel::O0), SonatinaOptLevel::O0);
        assert_eq!(to_sonatina_opt_level(OptLevel::O1), SonatinaOptLevel::O1);
        assert_eq!(to_sonatina_opt_level(OptLevel::Os), SonatinaOptLevel::Os);
        assert_eq!(to_sonatina_opt_level(OptLevel::O2), SonatinaOptLevel::O2);
    }

    #[test]
    fn module_sonatina_bytecode_respects_contract_filter() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/cli_output/build/multi_contract.fe");
        let fixture_source =
            fs::read_to_string(&fixture_path).expect("multi_contract fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let bytecode = emit_module_sonatina_bytecode(&db, top_mod, OptLevel::O0, Some("Foo"))
            .expect("selected contract should compile");
        let keys = bytecode.keys().map(String::as_str).collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec!["Foo"],
            "selected contract bytecode should exclude unselected roots"
        );
    }

    #[test]
    fn result_map_chain_test_runtime_package_retains_value_enum_asserts() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/result_map_chain_infers_independently.fe");
        let fixture_source = fs::read_to_string(&fixture_path)
            .expect("result_map_chain_infers_independently fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");

        let module = compile_runtime_package_sonatina(&db, &package, crate::EVM_LAYOUT)
            .expect("test runtime package should lower to Sonatina IR");
        let dumped = ModuleWriter::new(&module).dump_string();
        let map_helpers = dumped
            .lines()
            .filter(|line| line.starts_with("func private %map"))
            .collect::<Vec<_>>();
        assert_eq!(
            map_helpers.len(),
            2,
            "expected two map helpers in test runtime package:\n{dumped}"
        );
        assert!(
            map_helpers
                .iter()
                .all(|line| line.starts_with("func private %map__g")),
            "expected colliding map helpers to include generic discriminators:\n{dumped}"
        );
        assert!(
            dumped.contains("func private %unwrap"),
            "expected unwrap helper in test runtime package:\n{dumped}"
        );
        assert!(
            dumped.contains("enum.assert_variant"),
            "expected value enum proofs in test runtime package:\n{dumped}"
        );

        if let Err(err) = ensure_module_sonatina_ir_valid(&module) {
            panic!("pre-opt test module should verify: {err}\n\n{dumped}");
        }
        compile_runtime_objects(module, OptLevel::O0, false)
            .expect("test runtime package should compile");
    }

    #[test]
    fn sonatina_origins_cover_preopt_instruction_ids() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/int_downcast.fe");
        let fixture_source =
            fs::read_to_string(&fixture_path).expect("int_downcast fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");

        let (module, origins) =
            compile_runtime_package_sonatina_with_origins(&db, &package, crate::EVM_LAYOUT)
                .expect("test runtime package should lower to Sonatina IR with origins");

        let mut total_insts = 0usize;
        let mut total_stmt = 0usize;
        let mut total_terminator = 0usize;
        let mut total_synthetic = 0usize;
        for function_origins in origins.functions() {
            let layout_insts = module.func_store.view(function_origins.function(), |func| {
                func.layout
                    .iter_block()
                    .flat_map(|block| func.layout.iter_inst(block))
                    .map(|inst| inst.0)
                    .collect::<BTreeSet<_>>()
            });
            let unique_insts = function_origins
                .records()
                .iter()
                .map(|record| record.origin().inst().0)
                .collect::<BTreeSet<_>>();

            assert_eq!(
                unique_insts, layout_insts,
                "every pre-opt Sonatina instruction should be classified"
            );
            assert_eq!(
                unique_insts.len(),
                function_origins.records().len(),
                "each Sonatina instruction should have exactly one origin classification"
            );

            let coverage = function_origins.coverage();
            total_insts += coverage.total();
            total_stmt += coverage.runtime_stmt();
            total_terminator += coverage.runtime_terminator();
            total_synthetic += coverage.synthetic();

            assert!(
                function_origins
                    .origin_graph()
                    .links()
                    .iter()
                    .all(|link| !matches!(
                        link.from(),
                        crate::origin::SonatinaOriginNode::SonatinaInst(_)
                    )),
                "origin links should point from runtime/synthetic sources to Sonatina instructions"
            );
        }

        assert!(
            total_insts > 0,
            "fixture should lower to Sonatina instructions"
        );
        assert!(
            total_synthetic > 0,
            "prologue instructions should be classified synthetic"
        );
        assert!(
            total_stmt > 0,
            "runtime statement instructions should be classified"
        );
        assert!(
            total_terminator > 0,
            "runtime terminator instructions should be classified"
        );
        assert!(
            origins
                .records()
                .all(|record| !matches!(record.source(), SonatinaOriginSource::Unmapped(_)))
        );
    }

    #[test]
    fn post_opt_origins_cover_optimized_sonatina_instruction_ids() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/int_downcast.fe");
        let fixture_source =
            fs::read_to_string(&fixture_path).expect("int_downcast fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");

        let (module, pre_opt_origins) =
            compile_runtime_package_sonatina_with_origins(&db, &package, crate::EVM_LAYOUT)
                .expect("test runtime package should lower to Sonatina IR with origins");
        let mut compile = evm_compile(module, OptLevel::O1, true);
        let optimized = compile.optimize();
        let post_opt_origins =
            SonatinaPostOptPackageOrigins::from_module(optimized, &pre_opt_origins);

        let mut total_insts = 0usize;
        let pre_opt_total = pre_opt_origins.records().count();
        for function_origins in post_opt_origins.functions() {
            let layout_insts = optimized
                .func_store
                .view(function_origins.function(), |func| {
                    func.layout
                        .iter_block()
                        .flat_map(|block| func.layout.iter_inst(block))
                        .map(|inst| inst.0)
                        .collect::<BTreeSet<_>>()
                });
            let origin_insts = function_origins
                .records()
                .iter()
                .map(|record| record.origin().inst().0)
                .collect::<BTreeSet<_>>();

            assert_eq!(
                origin_insts, layout_insts,
                "every optimized Sonatina instruction should have a post-opt origin classification"
            );
            assert!(
                function_origins
                    .records()
                    .iter()
                    .all(|record| record.origin().stage() == SonatinaInstStage::PostOpt),
                "post-opt origin bundles must not reuse pre-opt instruction nodes"
            );
            total_insts += function_origins.records().len();
        }

        let coverage = post_opt_origins.coverage();
        assert_eq!(coverage.total(), total_insts);
        assert!(coverage.is_post_opt_partitioned());
        assert_eq!(
            pre_opt_total,
            coverage.observed_pre_opt_total(),
            "every pre-opt instruction should either retain a same-ID post-opt snapshot match or be recorded as a pre-opt snapshot loss"
        );
        assert_eq!(
            coverage.pre_opt_snapshot_losses(),
            post_opt_origins.pre_opt_snapshot_losses().count()
        );
        assert!(
            post_opt_origins
                .pre_opt_snapshot_losses()
                .all(|record| record.pre_opt().origin().stage() == SonatinaInstStage::PreOpt),
            "pre-opt snapshot losses must preserve pre-opt instruction origins"
        );
        assert!(coverage.total() > 0);
    }

    #[test]
    fn bytecode_origins_join_from_postopt_sonatina_observability() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/int_downcast.fe");
        let fixture_source =
            fs::read_to_string(&fixture_path).expect("int_downcast fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");

        let (module, pre_opt_origins) =
            compile_runtime_package_sonatina_with_origins(&db, &package, crate::EVM_LAYOUT)
                .expect("test runtime package should lower to Sonatina IR with origins");
        let compiled =
            compile_runtime_objects_with_origins(module, OptLevel::O0, true, &pre_opt_origins)
                .expect("test runtime package should compile with origin observability");
        let post_opt_coverage = compiled.post_opt_origins.coverage();

        assert_eq!(
            post_opt_coverage.created_or_unmatched_after_preopt_snapshot(),
            0,
            "O0 should preserve the pre-opt Sonatina instruction ID snapshot"
        );
        assert_eq!(
            post_opt_coverage.same_inst_id(),
            post_opt_coverage.total(),
            "O0 post-opt origins should be classified as same-instruction-ID snapshot aliases"
        );
        assert_eq!(
            post_opt_coverage.pre_opt_snapshot_losses(),
            0,
            "O0 should not lose pre-opt instruction IDs before the post-opt snapshot"
        );

        assert!(
            !compiled.bytecode_origins.records().is_empty(),
            "observability should produce bytecode PC origin records"
        );
        let bytecode_coverage = compiled.bytecode_origins.coverage();
        assert_eq!(
            bytecode_coverage.total(),
            compiled.bytecode_origins.records().len()
        );
        assert_eq!(
            bytecode_coverage.classified_total(),
            bytecode_coverage.total(),
            "every bytecode PC origin record should have exactly one classified source"
        );
        assert!(
            compiled
                .bytecode_origins
                .records()
                .iter()
                .all(|record| record.origin().range().start() < record.origin().range().end()),
            "bytecode origin records should use non-empty PC ranges"
        );
        assert!(
            compiled
                .bytecode_origins
                .records()
                .iter()
                .any(|record| matches!(
                    record.source(),
                    BytecodeOriginSource::SonatinaPostOpt(post_opt)
                        if post_opt.origin().stage() == SonatinaInstStage::PostOpt
                            && matches!(
                                post_opt.source(),
                                SonatinaPostOptOriginSource::SameInstId(_)
                            )
                )),
            "at least one bytecode range should join through a post-opt Sonatina instruction backed by pre-opt origins"
        );
        assert!(
            compiled.bytecode_origins.records().iter().all(|record| {
                let BytecodeOriginSource::SonatinaPostOpt(post_opt) = record.source() else {
                    return true;
                };
                match post_opt.source() {
                    SonatinaPostOptOriginSource::SameInstId(_) => {
                        compiled
                            .post_opt_origins
                            .record_for_inst(post_opt.origin().function(), post_opt.origin().inst())
                            == Some(post_opt)
                    }
                    SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot => true,
                }
            }),
            "pre-opt-backed bytecode origins should reuse post-opt origin records, not rebuild pre-opt joins at the PC boundary"
        );

        let graph = compiled.bytecode_origins.origin_graph();
        assert!(
            graph.links().iter().any(|link| {
                link.kind() == OriginLinkKind::Alias
                    && matches!(
                        link.from(),
                        CodegenOriginNode::SonatinaInst(origin)
                            if origin.stage() == SonatinaInstStage::PreOpt
                    )
                    && matches!(
                        link.to(),
                        CodegenOriginNode::SonatinaInst(origin)
                            if origin.stage() == SonatinaInstStage::PostOpt
                    )
            }),
            "pre-opt to post-opt joins are snapshot identity aliases, not pass-lineage transform events"
        );
        assert!(
            graph.links().iter().any(|link| {
                matches!(
                    link.from(),
                    CodegenOriginNode::SonatinaInst(origin)
                        if origin.stage() == SonatinaInstStage::PostOpt
                ) && matches!(link.to(), CodegenOriginNode::BytecodePc(_))
            }),
            "bytecode links should originate from post-opt Sonatina instruction nodes"
        );
        assert!(
            graph.links().iter().all(|link| {
                !(matches!(
                    link.from(),
                    CodegenOriginNode::SonatinaInst(origin)
                        if origin.stage() == SonatinaInstStage::PreOpt
                ) && matches!(link.to(), CodegenOriginNode::BytecodePc(_)))
            }),
            "bytecode PC ranges must not link directly from pre-opt instruction IDs"
        );

        let mut observability = compiled.artifacts[0]
            .observability()
            .expect("compiled artifact should include observability");
        observability.apply_frontend_provenance(
            compiled
                .frontend_origin_labels
                .as_sonatina_frontend_provenance(),
        );
        assert!(
            observability
                .to_json()
                .contains("\"frontend_provenance\":\"runtime."),
            "Sonatina observability JSON should be enriched with runtime origin labels"
        );
    }

    #[test]
    fn bytecode_source_resolver_joins_pc_ranges_to_hir_spans() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/int_downcast.fe");
        let fixture_source =
            fs::read_to_string(&fixture_path).expect("int_downcast fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");
        let runtime_origins = mir::runtime_package_origins(&db, package);

        let (module, pre_opt_origins) =
            compile_runtime_package_sonatina_with_origins(&db, &package, crate::EVM_LAYOUT)
                .expect("test runtime package should lower to Sonatina IR with origins");
        let compiled =
            compile_runtime_objects_with_origins(module, OptLevel::O0, true, &pre_opt_origins)
                .expect("test runtime package should compile with origin observability");
        let resolutions = compiled
            .bytecode_origins
            .resolve_source_spans(&db, &runtime_origins);

        assert_eq!(
            resolutions.len(),
            compiled.bytecode_origins.records().len(),
            "source resolution should classify every bytecode origin record"
        );
        assert!(
            resolutions.iter().all(|resolution| !matches!(
                resolution.result(),
                BytecodeSourceResolutionResult::RuntimeStmtMissing(_)
                    | BytecodeSourceResolutionResult::RuntimeTerminatorMissing(_)
            )),
            "runtime-backed bytecode origins should resolve through owner-aware MIR origin lookup"
        );

        let user_source_snippets = resolutions
            .iter()
            .filter_map(|resolution| match resolution.result() {
                BytecodeSourceResolutionResult::SourceSpan { span, .. }
                    if span.file == file && span.kind == SpanKind::Original =>
                {
                    let text = span.file.text(&db);
                    let start: usize = span.range.start().into();
                    let end: usize = span.range.end().into();
                    Some(text[start..end].trim().to_string())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(
            user_source_snippets
                .iter()
                .any(|snippet| !snippet.is_empty()),
            "at least one bytecode PC range should resolve to original user source"
        );
    }

    #[test]
    fn bytecode_source_resolver_keeps_test_bodies_separate() {
        let mut db = DriverDataBase::default();
        let file_url = temp_fixture_url("bytecode_source_resolver_bodies.fe");
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
#[test]
fn test_origin_a() uses (evm: mut Evm) {
    let marker_a: u256 = 11
    assert(marker_a == 11)
}

#[test]
fn test_origin_b() uses (evm: mut Evm) {
    let marker_b: u256 = 22
    assert(marker_b == 22)
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");
        let runtime_origins = mir::runtime_package_origins(&db, package);

        let (module, pre_opt_origins) =
            compile_runtime_package_sonatina_with_origins(&db, &package, crate::EVM_LAYOUT)
                .expect("test runtime package should lower to Sonatina IR with origins");
        let compiled =
            compile_runtime_objects_with_origins(module, OptLevel::O0, true, &pre_opt_origins)
                .expect("test runtime package should compile with origin observability");
        let snippets = compiled
            .bytecode_origins
            .resolve_source_spans(&db, &runtime_origins)
            .into_iter()
            .filter_map(|resolution| match resolution.result() {
                BytecodeSourceResolutionResult::SourceSpan { span, .. }
                    if span.file == file && span.kind == SpanKind::Original =>
                {
                    let text = span.file.text(&db);
                    let start: usize = span.range.start().into();
                    let end: usize = span.range.end().into();
                    Some(text[start..end].trim().to_string())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(
            snippets
                .iter()
                .any(|snippet| snippet.contains("marker_a") || snippet.contains("11")),
            "expected source mappings for the first test body; snippets: {snippets:?}"
        );
        assert!(
            snippets
                .iter()
                .any(|snippet| snippet.contains("marker_b") || snippet.contains("22")),
            "expected source mappings for the second test body; snippets: {snippets:?}"
        );
    }

    #[test]
    fn test_module_observability_json_is_enriched_with_runtime_origin_labels() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/int_downcast.fe");
        let fixture_source =
            fs::read_to_string(&fixture_path).expect("int_downcast fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);

        let output = emit_test_module_sonatina(
            &db,
            top_mod,
            OptLevel::O0,
            SonatinaTestOptions {
                emit_observability: true,
            },
            None,
        )
        .expect("test module should compile with Sonatina observability");

        assert!(
            output.tests.iter().any(|case| {
                case.sonatina_observability_json
                    .as_deref()
                    .is_some_and(|json| json.contains("\"frontend_provenance\":\"runtime."))
            }),
            "public test metadata should carry runtime origin labels in Sonatina observability JSON"
        );
        let source_map_debug = output
            .tests
            .iter()
            .map(|case| {
                let summary = case
                    .sonatina_source_map_summary
                    .as_ref()
                    .map(|summary| {
                        format!(
                            "total={} source={} runtime_synthetic={} sonatina_synthetic={} post_preopt_gap={} bytecode_unmapped={} object={:?} section={:?}",
                            summary.total(),
                            summary.source(),
                            summary.runtime_synthetic(),
                            summary.sonatina_synthetic(),
                            summary.post_preopt_snapshot_gap(),
                            summary.bytecode_unmapped(),
                            summary.object(),
                            summary.section(),
                        )
                    })
                    .unwrap_or_else(|| "none".to_string());
                format!(
                    "{} object={} summary={} entries={}",
                    case.display_name, case.object_name, summary,
                    case.sonatina_source_map_entries.len(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            output.tests.iter().any(|case| {
                case.sonatina_source_map_summary
                    .as_ref()
                    .and_then(|summary| {
                        let object_key = summary.object().map(BytecodeObjectKey::new);
                        let section_key =
                            object_key
                                .as_ref()
                                .zip(summary.section())
                                .map(|(object, section)| {
                                    BytecodeSectionKey::new(
                                        object.clone(),
                                        BytecodeSectionNameKey::new(section),
                                    )
                                });
                        let metadata = section_key
                            .as_ref()
                            .map(crate::debug::BytecodeSourceMapExportMetadata::section)
                            .or_else(|| {
                                object_key
                                    .as_ref()
                                    .map(crate::debug::BytecodeSourceMapExportMetadata::object)
                            });
                        let source_map_options =
                            crate::debug::BytecodeSourceMapExportOptions::new()
                                .with_optional_metadata(metadata);
                        crate::debug::bytecode_source_map_entries_json(
                            &case.sonatina_source_map_entries,
                            source_map_options,
                        )
                        .expect("source map should serialize")
                    })
                    .is_some_and(|json| {
                        let export = serde_json::from_str::<
                            crate::debug::OwnedBytecodeSourceMapExport,
                        >(&json)
                        .expect("source map should match owned schema");
                        export.schema_version()
                            == crate::debug::OwnedBytecodeSourceMapExport::SCHEMA_VERSION
                            && export.entries().iter().any(|entry| {
                                entry.pc_start() < entry.pc_end()
                                    && matches!(
                                        entry.kind(),
                                        crate::debug::BytecodeSourceMapEntryKind::Source {
                                            file,
                                            ..
                                        }
                                            if file.contains("int_downcast.fe")
                                    )
                            })
                    })
            }),
            "public test metadata should carry typed origin source maps; observed:\n{source_map_debug}"
        );
        assert!(
            output.tests.iter().any(|case| {
                case.sonatina_source_map_summary
                    .as_ref()
                    .is_some_and(|summary| summary.total() > 0 && summary.source() > 0)
            }),
            "public test metadata should carry typed source-map summaries"
        );
        assert!(
            output.tests.iter().any(|case| {
                case.sonatina_source_map_summary
                    .as_ref()
                    .is_some_and(|summary| {
                        summary.total() == case.sonatina_source_map_entries.len()
                    })
                    && case
                        .sonatina_bytecode_origin_coverage
                        .is_some_and(|coverage| {
                            coverage.total() == case.sonatina_source_map_entries.len()
                                && coverage.is_partitioned()
                        })
                    && case
                        .sonatina_post_opt_origin_coverage
                        .is_some_and(|coverage| {
                            coverage.total() > 0
                                && coverage.is_post_opt_partitioned()
                                && coverage.observed_pre_opt_total() > 0
                        })
                    && case
                        .sonatina_source_map_entries
                        .iter()
                        .any(|entry| entry.kind().kind_name() == "source")
            }),
            "public test metadata should expose typed source-map entries"
        );
        assert!(
            output.tests.iter().any(|case| {
                case.sonatina_origin_facts.as_ref().is_some_and(|facts| {
                    let index =
                        OriginFactIndex::new(facts).expect("origin facts should be queryable");
                    let has_runtime_to_bytecode_path = facts.origin_nodes().any(|runtime_node| {
                        matches!(
                            runtime_node.key().kind(),
                            OriginExportKind::RuntimeStmt | OriginExportKind::RuntimeTerminator
                        ) && facts.origin_nodes().any(|bytecode_node| {
                            bytecode_node.key().kind() == OriginExportKind::BytecodePc
                                && index.has_path(runtime_node.id(), bytecode_node.id())
                        })
                    });

                    facts.origin_nodes().count() > 0
                        && facts.origin_links().count() > 0
                        && facts.source_spans().count() > 0
                        && facts.origin_nodes().any(|node| {
                            matches!(
                                node.key().kind(),
                                OriginExportKind::RuntimeStmt | OriginExportKind::RuntimeTerminator
                            )
                        })
                        && facts
                            .origin_nodes()
                            .any(|node| node.key().kind() == OriginExportKind::BytecodePc)
                        && facts.origin_nodes().any(|node| {
                            node.key().kind() == OriginExportKind::BytecodePc
                                && index.source_spans_for_origin(node.id()).next().is_some()
                        })
                        && has_runtime_to_bytecode_path
                        && serde_json::to_value(facts.to_owned_export())
                            .expect("origin facts should serialize")
                            .get("schema_version")
                            .and_then(|version| version.as_u64())
                            == Some(1)
                })
            }),
            "public test metadata should expose typed origin facts"
        );
        assert!(
            output.tests.iter().any(|case| {
                case.sonatina_snapshot_origin_facts
                    .as_ref()
                    .is_some_and(|facts| {
                        facts.origin_nodes().count() > 0
                            && facts.origin_links().count() > 0
                            && facts
                                .origin_nodes()
                                .any(|node| node.key().kind() == OriginExportKind::SonatinaInst)
                            && facts
                                .origin_links()
                                .all(|link| link.kind() != OriginLinkKind::Transformed)
                            && serde_json::to_value(facts.to_owned_export())
                                .expect("snapshot origin facts should serialize")
                                .get("schema_version")
                                .and_then(|version| version.as_u64())
                                == Some(1)
                    })
            }),
            "public test metadata should expose typed Sonatina snapshot-diff facts"
        );
    }

    #[test]
    fn int_downcast_test_runtime_package_verifies_with_enum_param_init_cfg() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/int_downcast.fe");
        let fixture_source =
            fs::read_to_string(&fixture_path).expect("int_downcast fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");

        let module = compile_runtime_package_sonatina(&db, &package, crate::EVM_LAYOUT)
            .expect("test runtime package should lower to Sonatina IR");
        let dumped = ModuleWriter::new(&module).dump_string();

        if let Err(err) = ensure_module_sonatina_ir_valid(&module) {
            panic!("pre-opt test module should verify: {err}\n\n{dumped}");
        }
        compile_runtime_objects(module, OptLevel::O0, false)
            .expect("test runtime package should compile");
    }

    #[test]
    fn enum_state_machine_test_runtime_package_supports_storage_enum_roundtrips() {
        let mut db = DriverDataBase::default();
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fe/tests/fixtures/fe_test/enum_state_machine.fe");
        let fixture_source = fs::read_to_string(&fixture_path)
            .expect("enum_state_machine fixture should be readable");
        let file_url = Url::from_file_path(&fixture_path).expect("fixture path should be absolute");
        db.workspace()
            .touch(&mut db, file_url.clone(), Some(fixture_source));
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let package = build_test_runtime_package(&db, top_mod, None)
            .expect("test runtime package should build");

        let module = compile_runtime_package_sonatina(&db, &package, crate::EVM_LAYOUT)
            .expect("test runtime package should lower to Sonatina IR");
        let dumped = ModuleWriter::new(&module).dump_string();

        if let Err(err) = ensure_module_sonatina_ir_valid(&module) {
            panic!("pre-opt test module should verify: {err}\n\n{dumped}");
        }
        compile_runtime_objects(module, OptLevel::O0, false)
            .expect("test runtime package should compile");
    }

    #[test]
    fn if_both_arms_return_test_runtime_package_has_no_empty_unreachable_blocks() {
        let mut db = DriverDataBase::default();
        let file_url = temp_fixture_url("if_both_arms_return_sonatina_runtime.fe");
        db.workspace().touch(
            &mut db,
            file_url.clone(),
            Some(
                r#"
fn f(x: u256) -> u256 {
    if x == 0 {
        return 1
    } else {
        return 2
    }
}

#[test]
fn roundtrip() {
    assert(f(0) == 1)
    assert(f(1) == 2)
}
"#
                .to_string(),
            ),
        );
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);

        emit_test_module_sonatina(
            &db,
            top_mod,
            OptLevel::O0,
            SonatinaTestOptions::default(),
            None,
        )
        .expect(
            "if branches that both return should lower without empty unreachable Sonatina blocks",
        );
    }
}
