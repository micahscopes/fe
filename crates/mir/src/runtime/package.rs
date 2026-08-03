use common::indexmap::IndexMap;
use hir::semantic::{RecvArmAbiInfo, RecvArmView};
use hir::{
    analysis::{
        semantic::{
            GenericSubst, ImplEnv, ManualContractSection, RootSemanticInstanceError,
            SemanticInstance, SemanticInstanceKey, get_or_build_semantic_instance,
            owner_effect_bindings, root_semantic_instance_key, same_owner_effect_binding,
        },
        ty::{
            const_ty::ConstTyData,
            corelib::{resolve_core_trait, resolve_lib_func_path, resolve_lib_type_path},
            trait_def::{TraitInstId, resolve_trait_method_instance},
            trait_resolution::TraitSolveCx,
            ty_check::{BodyOwner, EffectParamSite, LocalBinding},
            ty_def::{TyData, TyId},
        },
    },
    hir_def::{
        Contract, Func, IdentId, InlineHint, ItemKind, ManualContractRootAttr, TopLevelMod,
        Visibility,
    },
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    db::MirDb,
    instance::runtime::runtime_instance_lowered_body,
    instance::{
        RuntimeInstance, RuntimeInstanceKey, RuntimeInstanceSource, RuntimeSyntheticInstance,
        get_or_build_runtime_instance,
    },
    runtime::code_region::{code_region_symbol, runtime_code_region_for_manual_root},
    runtime::lower::body::{
        check_reachable_runtime_trait_calls_resolvable, declared_external_func,
    },
    runtime::lower::classify::{
        RuntimeVisibleBindingPlan, runtime_effect_binding_plan, runtime_param_class,
        runtime_visible_binding_class,
    },
    runtime::lower::interface::runtime_visible_binding_plans,
    runtime::lower::type_info::{
        RuntimeTypeEnv, provider_class_for_target_in_env, top_level_class_for_ty_in_env,
    },
    runtime::root_effects::{
        EntryEffectContext, entry_effect_arg_plans, target_root_provider_materialization,
    },
    runtime::stable_key::{
        item_identity, semantic_instance_identity, semantic_instance_symbol_identity,
        stable_identity_hash, type_identity,
    },
    runtime::{
        AddressSpaceKind, ConstRegionId, ContractInitAbiPlan, ContractRecvAbiPlan, DispatchArm,
        DispatchDefault, EntryEffectArgPlan, InitArgsPlan, LayoutId, LayoutKey, RefKind, RefView,
        ResolvedCodeRegion, RuntimeBoundarySpec, RuntimeClass, RuntimeCodeRegion,
        RuntimeCodeRegionKey, RuntimeFunction, RuntimeFunctionOwner, RuntimeInlineHint,
        RuntimeInputPlan, RuntimeLinkage, RuntimeObject, RuntimePackage, RuntimePackagePlan,
        RuntimeParamPlan, RuntimeReturnPlan, RuntimeSection, RuntimeSectionName, RuntimeSectionRef,
        RuntimeSyntheticSpec, ScalarClass, ScalarRepr, ScalarRole, TargetRootProviderBinding,
        TargetRootProviderMaterialization,
    },
    verify::verify_runtime_package,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum LowerError {
    Unsupported(String),
    /// Monomorphization re-resolved a trait method to a *different* impl than
    /// type-checking committed to at instantiation time (rung 3.3). Under
    /// coherence this can never happen for valid Fe; if it fires it is a real
    /// determinism violation in the resolver, and must hard-fail rather than
    /// silently lower against the wrong impl. The message carries both
    /// implementors for diagnosis.
    NondeterministicReResolution(String),
    /// The recorded `ImplEnv::selected_implementor` consumed by the MIR C1 rail's
    /// `Some` branch (cascade C1) is NOT a valid impl for the call's goal — it is
    /// not a member of the goal's impl-table candidate set, or does not apply to
    /// it. Under coherence every recorded implementor is a solver solution = a
    /// real applying candidate, so this can never happen for valid Fe; if it fires
    /// the record was forged/mismatched and must hard-fail rather than silently
    /// lower against a bad impl. The message carries the recorded implementor and
    /// the goal for diagnosis. See `recorded_implementor_is_valid_candidate`.
    ForgedRecordedImplementor(String),
    /// A runtime trait-method call could not be resolved to a single concrete
    /// impl body: the selection is ambiguous (several coexisting impls apply and
    /// none was uniquely chosen) or the impl that would be chosen does not apply
    /// here (its constraints are unsatisfied). This is a user-facing error on a
    /// legal program, surfaced as a clean diagnostic on every reachable
    /// resolution route (never a backend panic); the call must be disambiguated
    /// with a `with (...)` selection. The message names the trait-method and the
    /// goal only (no internal keys), so it is stable across runs. See the
    /// pre-flight `check_runtime_trait_calls_resolvable` (checks the body being
    /// lowered) and `check_reachable_runtime_trait_calls_resolvable` (walks the
    /// transitive callee graph, so an unresolvable call reached only through
    /// return-class inference on a callee is caught before that panics too).
    UnresolvedTraitSelection(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::Unsupported(message) => write!(f, "{message}"),
            LowerError::NondeterministicReResolution(message) => write!(f, "{message}"),
            LowerError::ForgedRecordedImplementor(message) => write!(f, "{message}"),
            LowerError::UnresolvedTraitSelection(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for LowerError {}

#[derive(Clone, Copy)]
struct ManualContractRoot<'db> {
    func: Func<'db>,
    instance: RuntimeInstance<'db>,
    contract_name: &'db str,
    section: ManualContractSection,
}

type ManualContractObjectSpec<'db> = (
    String,
    Vec<(RuntimeSectionName, RuntimeInstance<'db>)>,
    Vec<RuntimeInstance<'db>>,
);

#[derive(Debug)]
enum RuntimeRootCandidate<'db> {
    Root(Func<'db>),
    NotRoot,
    Rejected(RuntimeRootRejection<'db>),
}

#[derive(Debug)]
struct RuntimeRootRejection<'db> {
    func: Func<'db>,
    reason: RuntimeRootRejectionReason<'db>,
}

#[derive(Debug)]
enum RuntimeRootRejectionReason<'db> {
    RootSemanticInstance(RootSemanticInstanceError<'db>),
    UnsupportedEntryEffect(String),
}

#[derive(Debug, Clone)]
struct RuntimeGraphNode<'db> {
    direct_callees: Vec<RuntimeInstance<'db>>,
    referenced_const_regions: Vec<ConstRegionId<'db>>,
    referenced_code_regions: Vec<RuntimeCodeRegion<'db>>,
}

struct RuntimeGraph<'db> {
    // Insertion-ordered: keys are Salsa tracked IDs whose hash order varies
    // across runs, which would make `_0`/`_1` suffix assignment for
    // content-identical duplicates non-deterministic.
    nodes: IndexMap<RuntimeInstance<'db>, RuntimeGraphNode<'db>>,
    public_roots: FxHashSet<RuntimeInstance<'db>>,
    /// Functions the SOURCE declared as the module's public surface, whether or
    /// not they were seeded as roots. Entry-only root seeding deliberately drops
    /// callee-reachable candidates (seeding them would mint a second,
    /// scope-only-distinct instance and mangle both symbols), and that was safe
    /// only while the wasm backend exported every function. Sonatina
    /// `ac266c21` gated exports on `Linkage::Public`, so export eligibility must
    /// now be carried explicitly rather than inferred from root-ness. Empty on
    /// the EVM path, which has its own entry ABI.
    public_export_funcs: FxHashSet<Func<'db>>,
    object_specs: Vec<(String, Vec<(RuntimeSectionName, RuntimeInstance<'db>)>)>,
    code_region_roots: Vec<(RuntimeCodeRegion<'db>, RuntimeInstance<'db>)>,
}

struct RuntimeGraphBuilder<'db> {
    db: &'db dyn MirDb,
    queue: Vec<RuntimeInstance<'db>>,
    queued: FxHashSet<RuntimeInstance<'db>>,
    nodes: IndexMap<RuntimeInstance<'db>, RuntimeGraphNode<'db>>,
    public_roots: FxHashSet<RuntimeInstance<'db>>,
    public_export_funcs: FxHashSet<Func<'db>>,
    object_specs: Vec<(String, Vec<(RuntimeSectionName, RuntimeInstance<'db>)>)>,
    discovered_contract_specs: Vec<(String, Vec<(RuntimeSectionName, RuntimeInstance<'db>)>)>,
    code_region_roots: Vec<(RuntimeCodeRegion<'db>, RuntimeInstance<'db>)>,
    seen_region_roots: FxHashSet<RuntimeCodeRegion<'db>>,
    materialized_contracts: FxHashSet<Contract<'db>>,
    materialized_object_names: FxHashSet<String>,
    /// Memoizes bodies already validated by
    /// `check_reachable_runtime_trait_calls_resolvable`, keyed by
    /// `SemanticInstanceKey` (trait-call resolvability does not depend on
    /// runtime specialization), so the transitive pre-flight walk stays
    /// cheap across the many `RuntimeInstance`s that can share callees.
    checked_reachable_trait_calls: FxHashSet<SemanticInstanceKey<'db>>,
}

impl<'db> RuntimeGraphBuilder<'db> {
    fn new(
        db: &'db dyn MirDb,
        roots: Vec<RuntimeInstance<'db>>,
        object_specs: Vec<(String, Vec<(RuntimeSectionName, RuntimeInstance<'db>)>)>,
        public_export_funcs: FxHashSet<Func<'db>>,
    ) -> Self {
        let materialized_contracts = materialized_contracts_for_roots(db, &roots);
        let materialized_object_names = object_specs
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<FxHashSet<_>>();
        let mut builder = Self {
            db,
            queue: Vec::new(),
            queued: FxHashSet::default(),
            nodes: IndexMap::new(),
            public_roots: roots.iter().copied().collect(),
            public_export_funcs,
            object_specs,
            discovered_contract_specs: Vec::new(),
            code_region_roots: Vec::new(),
            seen_region_roots: FxHashSet::default(),
            materialized_contracts,
            materialized_object_names,
            checked_reachable_trait_calls: FxHashSet::default(),
        };
        for root in roots {
            builder.enqueue(root);
        }
        builder
    }

    fn build(mut self) -> Result<RuntimeGraph<'db>, LowerError> {
        while let Some(instance) = self.queue.pop() {
            self.queued.remove(&instance);
            if self.nodes.contains_key(&instance) {
                continue;
            }

            if let Some(semantic) = instance.key(self.db).semantic(self.db) {
                ensure_semantic_instance_is_smir_lowerable(self.db, semantic)?;
                check_reachable_runtime_trait_calls_resolvable(
                    self.db,
                    semantic.key(self.db),
                    &mut self.checked_reachable_trait_calls,
                )
                .map_err(|err| wrap_runtime_lowering_error(self.db, instance, err))?;
            }
            let lowered = runtime_instance_lowered_body(self.db, instance)
                .map_err(|err| wrap_runtime_lowering_error(self.db, instance, err))?;
            let direct_callees = lowered
                .direct_callees(self.db)
                .into_iter()
                .map(|edge| edge.callee)
                .collect::<Vec<_>>();
            let referenced_const_regions = lowered.referenced_const_regions(self.db);
            let referenced_code_regions = lowered.referenced_code_regions(self.db);
            for callee in direct_callees.iter().copied() {
                self.enqueue(callee);
            }
            self.process_referenced_regions(&referenced_code_regions)?;
            self.nodes.insert(
                instance,
                RuntimeGraphNode {
                    direct_callees,
                    referenced_const_regions,
                    referenced_code_regions,
                },
            );
        }

        self.discovered_contract_specs
            .sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));
        self.object_specs.extend(self.discovered_contract_specs);
        self.code_region_roots
            .sort_by_key(|(region, _)| code_region_symbol(self.db, *region));
        Ok(RuntimeGraph {
            nodes: self.nodes,
            public_roots: self.public_roots,
            public_export_funcs: self.public_export_funcs,
            object_specs: self.object_specs,
            code_region_roots: self.code_region_roots,
        })
    }

    fn enqueue(&mut self, instance: RuntimeInstance<'db>) {
        if !self.nodes.contains_key(&instance) && self.queued.insert(instance) {
            self.queue.push(instance);
        }
    }

    fn process_referenced_regions(
        &mut self,
        regions: &[RuntimeCodeRegion<'db>],
    ) -> Result<(), LowerError> {
        let mut function_roots = Vec::new();
        let mut referenced_contracts = Vec::new();
        let mut referenced_manual_roots = Vec::new();
        for region in regions.iter().copied() {
            match region.key(self.db) {
                RuntimeCodeRegionKey::FunctionRoot { .. } => {
                    if self.seen_region_roots.insert(region) {
                        function_roots.push(region);
                    }
                }
                RuntimeCodeRegionKey::ContractInit { contract }
                | RuntimeCodeRegionKey::ContractRuntime { contract } => {
                    if self.materialized_contracts.insert(contract) {
                        referenced_contracts.push(contract);
                    }
                }
                RuntimeCodeRegionKey::ManualContractRoot { func } => {
                    referenced_manual_roots.push(func);
                }
            }
        }

        function_roots.sort_by_key(|region| code_region_symbol(self.db, *region));
        for region in function_roots {
            let RuntimeCodeRegionKey::FunctionRoot { symbol, callee } = region.key(self.db).clone()
            else {
                unreachable!();
            };
            let root = synthetic_instance(
                self.db,
                RuntimeSyntheticSpec::CodeRegionRoot { symbol, callee },
                Vec::new(),
            );
            self.code_region_roots.push((region, root));
            self.enqueue(root);
        }

        referenced_contracts.sort_by_key(|contract| contract_name(self.db, *contract));
        for contract in referenced_contracts {
            let (name, sections, section_roots) = contract_object_spec(self.db, contract)?;
            if !self.materialized_object_names.insert(name.clone()) {
                continue;
            }
            self.discovered_contract_specs.push((name, sections));
            for root in section_roots {
                self.enqueue(root);
            }
        }
        referenced_manual_roots.sort_by_key(|func| {
            func.name(self.db)
                .to_opt()
                .map(|name| name.data(self.db).to_string())
        });
        for func in referenced_manual_roots {
            let Some((name, sections, section_roots)) =
                manual_contract_object_for_root(self.db, func)?
            else {
                continue;
            };
            if !self.materialized_object_names.insert(name.clone()) {
                continue;
            }
            self.discovered_contract_specs.push((name, sections));
            for root in section_roots {
                self.enqueue(root);
            }
        }
        Ok(())
    }
}

pub fn build_runtime_package<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
) -> Result<RuntimePackage<'db>, LowerError> {
    if !top_mod.all_contracts(db).is_empty()
        || !discover_manual_contract_roots(db, top_mod)?.is_empty()
    {
        return build_contract_package(db, top_mod);
    }

    let funcs = top_mod
        .all_funcs(db)
        .iter()
        .copied()
        .filter(|func| func.top_mod(db) == top_mod)
        .filter(|func| !func.is_extern(db) && !is_test_func(db, *func))
        .collect::<Vec<_>>();
    let mut funcs = funcs;
    funcs.sort_by_key(|func| {
        func.name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())
            .unwrap_or_default()
    });
    let mut entry_funcs = Vec::new();
    let mut rejections = Vec::new();
    for func in funcs.iter().copied() {
        match runtime_root_candidate(db, func)? {
            RuntimeRootCandidate::Root(func) => entry_funcs.push(func),
            RuntimeRootCandidate::NotRoot => {}
            RuntimeRootCandidate::Rejected(rejection) => rejections.push(rejection),
        }
    }
    if let Some(rejection) = rejections
        .iter()
        .find(|rejection| is_main_func(db, rejection.func))
    {
        return Err(LowerError::Unsupported(format_runtime_root_rejection(
            db, rejection,
        )));
    }
    if entry_funcs.is_empty() {
        if let Some(rejection) = rejections.first() {
            return Err(LowerError::Unsupported(format_runtime_root_rejection(
                db, rejection,
            )));
        }
        return Ok(RuntimePackage::new(
            db,
            top_mod,
            Vec::new(),
            RuntimePackagePlan::new(db, Vec::new(), Vec::new(), Vec::new(), Vec::new(), None),
        ));
    }

    let mut roots = Vec::new();
    for func in entry_funcs.iter().copied() {
        let semantic = semantic_instance_for_root_owner(db, BodyOwner::Func(func))?;
        let entry_effect_args =
            entry_effect_arg_plans(db, EntryEffectContext::StandaloneFunc { func }, semantic)?;
        roots.push((
            func,
            runtime_instance_for_semantic(db, semantic),
            entry_effect_args,
        ));
    }
    let entry = roots
        .iter()
        .find(|(func, _, _)| is_main_func(db, *func))
        .or_else(|| roots.first())
        .map(|(_, instance, entry_effect_args)| (*instance, entry_effect_args.clone()))
        .expect("entry root candidates should include the chosen entry function");
    let root = synthetic_instance(
        db,
        RuntimeSyntheticSpec::MainRoot {
            callee: entry.0,
            entry_effect_args: entry.1.into_boxed_slice(),
        },
        Vec::new(),
    );
    let mut package_roots = roots
        .into_iter()
        .map(|(_, instance, _)| instance)
        .collect::<Vec<_>>();
    package_roots.push(root);
    let package = build_non_contract_package(
        db,
        top_mod,
        package_roots,
        vec![(sanitize_object_name("main"), RuntimeSectionName::Main, root)],
        Some("main"),
        // Non-wasm path: export eligibility is the EVM entry ABI's concern.
        FxHashSet::default(),
    )?;
    verify_runtime_package(db, package)
        .map_err(|err| LowerError::Unsupported(format!("invalid runtime package: {err:?}")))?;
    Ok(package)
}

/// The wasm backend's runtime-package builder (R3.4c enabler,
/// wasm-worker/WebGPU interop doc section 9).
///
/// Unlike [`build_runtime_package`] (the EVM path), this admits value-param
/// carrying `pub` top-level functions of the entry module as reachability
/// ROOTS, and synthesizes NO root wrapper: the lowered entry functions ARE the
/// exported objects (the WAFFLE backend exports every bodied function plus
/// `memory`). It is an ADDITIVE sibling; it does not edit the EVM path, and
/// `mir` carries no `BackendKind` (the per-backend fork lives at the `Backend`
/// impls in codegen). The candidate/assembly lines are DUPLICATED from
/// [`build_runtime_package`] on purpose (interop doc 9.5): factoring a shared
/// helper would textually churn the EVM-byte-identity-sensitive path.
///
/// v0 effect-arg composition (interop doc 9.3): a wasm export root's
/// host-visible signature is exactly its non-erased VALUE params, in
/// declaration order; effect bindings contribute ZERO host-visible params. The
/// surviving synthesized effect-arg set MUST be EMPTY (every effect erases: an
/// ambient zero-sized provider, or a `with (...)`-established provider inside
/// the body). A `pub` export root with a surviving (non-erased) effect binding
/// is REJECTED with a named fail-closed diagnostic (a surviving effect would
/// give the exported entry an extra parameter the host cannot supply). The
/// general `WasmExportRoot` wrapper is a deferred extension with no current
/// customer and is not built here.
pub fn build_wasm_runtime_package<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
) -> Result<RuntimePackage<'db>, LowerError> {
    build_wasm_runtime_package_impl(db, top_mod, None)
}

/// Build the Wasm-shaped runtime package rooted at one caller-selected public
/// top-level function. Unlike [`build_wasm_runtime_package`], this never relies
/// on source/declaration ordering to choose the package entry.
pub fn build_wasm_runtime_package_for_entry<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
    entry_name: &str,
) -> Result<RuntimePackage<'db>, LowerError> {
    build_wasm_runtime_package_for_entries(db, top_mod, &[entry_name.to_owned()])
}

/// Build the Wasm-shaped runtime package rooted at an ordered set of exact
/// public top-level functions. Callers must deduplicate names before this
/// boundary; duplicates fail closed so wrapper selection cannot silently drift.
pub fn build_wasm_runtime_package_for_entries<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
    entry_names: &[String],
) -> Result<RuntimePackage<'db>, LowerError> {
    if entry_names.is_empty() {
        return Err(LowerError::Unsupported(
            "requested web entry set must not be empty".to_owned(),
        ));
    }
    let mut seen = FxHashSet::default();
    for name in entry_names {
        if !seen.insert(name.as_str()) {
            return Err(LowerError::Unsupported(format!(
                "requested web entry `{name}` is duplicated"
            )));
        }
    }
    build_wasm_runtime_package_impl(db, top_mod, Some(entry_names))
}

fn build_wasm_runtime_package_impl<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
    requested_entries: Option<&[String]>,
) -> Result<RuntimePackage<'db>, LowerError> {
    // Contracts fail closed on wasm: no silent EVM-shaped behavior.
    if !top_mod.all_contracts(db).is_empty()
        || !discover_manual_contract_roots(db, top_mod)?.is_empty()
    {
        return Err(LowerError::Unsupported(
            "the wasm backend does not support contracts".to_string(),
        ));
    }

    let mut funcs = top_mod
        .all_funcs(db)
        .iter()
        .copied()
        .filter(|func| func.top_mod(db) == top_mod)
        .filter(|func| !func.is_extern(db) && !is_test_func(db, *func))
        .collect::<Vec<_>>();
    funcs.sort_by_key(|func| {
        func.name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())
            .unwrap_or_default()
    });

    let mut entry_funcs = Vec::new();
    let mut rejections = Vec::new();
    for func in funcs.iter().copied() {
        match wasm_runtime_root_candidate(db, func)? {
            RuntimeRootCandidate::Root(func) => entry_funcs.push(func),
            RuntimeRootCandidate::NotRoot => {}
            RuntimeRootCandidate::Rejected(rejection) => rejections.push(rejection),
        }
    }
    if let Some(entry_names) = requested_entries {
        let mut selected = Vec::with_capacity(entry_names.len());
        for entry_name in entry_names {
            let named = funcs
                .iter()
                .copied()
                .filter(|func| {
                    func.name(db)
                        .to_opt()
                        .is_some_and(|name| name.data(db) == entry_name)
                })
                .collect::<Vec<_>>();
            match named.as_slice() {
                [] => {
                    return Err(LowerError::Unsupported(format!(
                        "requested web entry `{entry_name}` was not found as a top-level function of the entry module"
                    )));
                }
                [func] => {
                    if let Some(rejection) =
                        rejections.iter().find(|rejection| rejection.func == *func)
                    {
                        return Err(LowerError::Unsupported(format_runtime_root_rejection(
                            db, rejection,
                        )));
                    }
                    if !entry_funcs.contains(func) {
                        return Err(LowerError::Unsupported(format!(
                            "requested web entry `{entry_name}` is not an eligible public runtime root"
                        )));
                    }
                    selected.push(*func);
                }
                _ => {
                    return Err(LowerError::Unsupported(format!(
                        "requested web entry `{entry_name}` is ambiguous in the entry module"
                    )));
                }
            }
        }
        entry_funcs = selected;
    }
    if requested_entries.is_none() {
        if let Some(rejection) = rejections
            .iter()
            .find(|rejection| is_main_func(db, rejection.func))
        {
            return Err(LowerError::Unsupported(format_runtime_root_rejection(
                db, rejection,
            )));
        }
    }
    if entry_funcs.is_empty() {
        if let Some(rejection) = rejections.first() {
            return Err(LowerError::Unsupported(format_runtime_root_rejection(
                db, rejection,
            )));
        }
        return Ok(RuntimePackage::new(
            db,
            top_mod,
            Vec::new(),
            RuntimePackagePlan::new(db, Vec::new(), Vec::new(), Vec::new(), Vec::new(), None),
        ));
    }

    // ENTRY-ONLY root seeding (interop doc 9, amended). Seed as roots only the
    // admitted candidates that are NOT reachable as a callee within the entry
    // ingot. A candidate reachable as a callee already materializes exactly once
    // as its callee instance, bare-named via the export-everything policy (the
    // R1 status quo). Seeding it as a root too would mint a SECOND, scope-only-
    // distinct instance (the root uses the function's own `impl_env` scope; the
    // callee uses the caller's) and collide its export symbol, mangling both.
    // The reachability is an additive read-only pre-pass over the candidates'
    // semantic call graph; no existing function is touched.
    let reachable_as_callee = wasm_candidates_reachable_as_callee(db, top_mod, &entry_funcs)?;
    let seed_funcs = entry_funcs
        .iter()
        .copied()
        .filter(|func| !reachable_as_callee.contains(func))
        .collect::<Vec<_>>();
    // Mutual-recursion corollary: if every admitted candidate is a callee of
    // another, the seed set is empty. There is no export entry then; fail closed
    // naming the rule rather than emit an empty module.
    if seed_funcs.is_empty() {
        return Err(LowerError::Unsupported(
            "the wasm backend found no export root: every `pub` top-level function of \
             the entry module is reachable as a callee of another (mutually recursive \
             `pub` entries exclude each other under entry-only root seeding). Provide at \
             least one `pub` entry function that is not called by another `pub` entry"
                .to_string(),
        ));
    }

    // Seeded roots synthesize NO `MainRoot` wrapper: each lowered function IS its
    // own export. The object's section entry is a real function instance (the
    // verifier only requires a declared package function, interop doc 9.1), not a
    // NeverReturns wrapper.
    //
    // The export receives its value params DIRECTLY as wasm function arguments
    // (the host passes them), so each visible value param is classed by its
    // by-value TRANSPORT representation (from its `RuntimeParamPlan`), not by the
    // EVM entry ABI's in-memory representation. `runtime_instance_for_semantic`'s
    // default classes an entry param as a memory reference (calldata-in-memory),
    // whose body reads it with a `load`; on wasm the export instead takes the
    // scalar/pointer directly (u64 -> i64, `MemPtr<B::Word>` -> i32). This is a
    // per-backend ABI fact (interop doc 8 packet item c), applied via the additive
    // override hook, not by editing the EVM path.
    let mut package_roots = Vec::new();
    let mut main_root = None;
    for func in seed_funcs.iter().copied() {
        let semantic = semantic_instance_for_root_owner(db, BodyOwner::Func(func))?;
        let instance = runtime_instance_for_semantic_with_visible_param_overrides(
            db,
            semantic,
            wasm_export_param_class,
        );
        if is_main_func(db, func) {
            main_root = Some(instance);
        }
        package_roots.push(instance);
    }
    let entry = main_root.unwrap_or(package_roots[0]);
    // Export eligibility is the SOURCE's `pub` declaration, not root-seeding.
    // `seed_funcs` deliberately excludes callee-reachable candidates; they still
    // belong to the module's public surface and must keep their wasm export.
    let public_export_funcs = entry_funcs.iter().copied().collect::<FxHashSet<_>>();
    let package = build_non_contract_package(
        db,
        top_mod,
        package_roots,
        vec![(
            sanitize_object_name("main"),
            RuntimeSectionName::Main,
            entry,
        )],
        Some("main"),
        public_export_funcs,
    )?;
    verify_runtime_package(db, package)
        .map_err(|err| LowerError::Unsupported(format!("invalid runtime package: {err:?}")))?;
    Ok(package)
}

/// Entry-only root seeding pre-pass (interop doc 9, amended): the subset of the
/// admitted `candidates` that are reachable AS A CALLEE within the entry module,
/// i.e. the target of a call edge in the transitive semantic call graph rooted at
/// the candidates. Such a candidate materializes exactly once as its callee
/// instance (bare-named via export-everything, the R1 status quo); seeding it as
/// a root too would mint a second, scope-only-distinct instance and collide its
/// export symbol.
///
/// The traversal follows semantic call edges but stays WITHIN the entry module
/// (`func.top_mod(db) == top_mod`): a candidate is a top-level function of the
/// entry module, and inter-candidate calls (directly, or through the module's
/// private helpers) live there; library callees never call back into an entry
/// candidate, so pruning at the module boundary is both correct and bounds the
/// walk. This is an additive, read-only query; no existing function is touched.
fn wasm_candidates_reachable_as_callee<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
    candidates: &[Func<'db>],
) -> Result<FxHashSet<Func<'db>>, LowerError> {
    let candidate_set: FxHashSet<Func<'db>> = candidates.iter().copied().collect();
    let mut reachable_as_callee = FxHashSet::default();
    let mut visited: FxHashSet<SemanticInstance<'db>> = FxHashSet::default();
    let mut stack: Vec<SemanticInstance<'db>> = Vec::new();
    for func in candidates.iter().copied() {
        // Every candidate already passed `root_semantic_instance_key` in
        // `wasm_runtime_root_candidate`, so this cannot fail here.
        let key = root_semantic_instance_key(db, BodyOwner::Func(func)).map_err(|err| {
            LowerError::Unsupported(format!(
                "wasm export-root reachability pre-pass: {}",
                format_root_semantic_instance_rejection(db, &func_display_name(db, func), &err),
            ))
        })?;
        let semantic = get_or_build_semantic_instance(db, key);
        if visited.insert(semantic) {
            stack.push(semantic);
        }
    }
    while let Some(semantic) = stack.pop() {
        ensure_semantic_instance_is_smir_lowerable(db, semantic)?;
        for callee in semantic.callees(db) {
            let callee_key = callee.key;
            let BodyOwner::Func(callee_func) = callee_key.owner(db) else {
                continue;
            };
            if callee_func.top_mod(db) != top_mod {
                continue;
            }
            if candidate_set.contains(&callee_func) {
                reachable_as_callee.insert(callee_func);
            }
            let callee_semantic = get_or_build_semantic_instance(db, callee_key);
            if visited.insert(callee_semantic) {
                stack.push(callee_semantic);
            }
        }
    }
    Ok(reachable_as_callee)
}

fn ensure_semantic_instance_is_smir_lowerable<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
) -> Result<(), LowerError> {
    let key = semantic.key(db);
    if key.typed_body(db).has_smir_lowering_blocker(db) {
        let owner = key.owner(db);
        let display = match owner {
            BodyOwner::Func(func) => func_display_name(db, func),
            _ => format!("{owner:?}"),
        };
        return Err(LowerError::Unsupported(format!(
            "cannot lower {display} ({owner:?}) to semantic MIR because type checking left unresolved or invalid body operations: {}",
            key.typed_body(db)
                .smir_lowering_blocker_details(db)
                .join("; "),
        )));
    }
    Ok(())
}

/// The wasm sibling of [`runtime_root_candidate`] (interop doc 9.2/9.3). Unlike
/// the EVM candidate it ADMITS value-param-carrying functions as roots (the host
/// passes the value params to the exported entry). Admission requires: `pub` and
/// non-associated (checked here); non-extern/non-test and top-level (filtered by
/// the caller); the same two semantic checks the EVM path applies (a monomorphic
/// effect-concrete `root_semantic_instance_key`, and `entry_effect_arg_plans`
/// succeeds); PLUS the v0 effect-erasure requirement of 9.3 (the synthesized
/// effect-arg set is EMPTY and no interface param derives from an effect
/// binding). The filter lines DUPLICATE `runtime_root_candidate` on purpose: the
/// EVM path stays textually untouched (interop doc 9.5).
fn wasm_runtime_root_candidate<'db>(
    db: &'db dyn MirDb,
    func: Func<'db>,
) -> Result<RuntimeRootCandidate<'db>, LowerError> {
    // Only `pub`, non-associated functions are export roots. Value params are
    // ADMITTED (the enabler): there is no `func.params(db).next()` gate here.
    if func.vis(db) != Visibility::Public || func.is_associated_func(db) {
        return Ok(RuntimeRootCandidate::NotRoot);
    }
    let semantic = match root_semantic_instance_key(db, BodyOwner::Func(func)) {
        Ok(key) => get_or_build_semantic_instance(db, key),
        Err(err) => {
            return Ok(RuntimeRootCandidate::Rejected(RuntimeRootRejection {
                func,
                reason: RuntimeRootRejectionReason::RootSemanticInstance(err),
            }));
        }
    };
    let entry_effect_args =
        match entry_effect_arg_plans(db, EntryEffectContext::StandaloneFunc { func }, semantic) {
            Ok(plans) => plans,
            Err(err) => {
                return Ok(RuntimeRootCandidate::Rejected(RuntimeRootRejection {
                    func,
                    reason: RuntimeRootRejectionReason::UnsupportedEntryEffect(err.to_string()),
                }));
            }
        };
    // v0 (9.3): the surviving synthesized effect-arg set MUST be EMPTY and no
    // interface param may derive from an effect binding, so the exported
    // signature is exactly the function's value params and NO wrapper is needed
    // (`synthetic.rs` `build_root_call` is unreachable on the wasm path). A
    // surviving effect binding would give the exported entry an extra parameter
    // the host cannot supply: fail closed with the named rule.
    if !entry_effect_args.is_empty() || wasm_root_has_surviving_effect_param(db, semantic) {
        return Ok(RuntimeRootCandidate::Rejected(RuntimeRootRejection {
            func,
            reason: RuntimeRootRejectionReason::UnsupportedEntryEffect(format!(
                "function `{}` cannot be a wasm export root because it has a surviving \
                 (non-erased) effect parameter; wasm export roots must have fully-erased \
                 effect parameters. Establish providers with `with (...)` inside the body, \
                 or use an ambient zero-sized provider",
                func_display_name(db, func),
            )),
        }));
    }
    Ok(RuntimeRootCandidate::Root(func))
}

/// Part (b) of the 9.3 v0 rule: does any owner effect binding SURVIVE erasure
/// into the function's runtime interface signature? `runtime_visible_binding_plans`
/// lists only non-erased bindings, so an owner effect binding present there is a
/// non-erased effect INTERFACE param, and the exported entry would carry an
/// extra host-visible parameter the caller cannot supply.
fn wasm_root_has_surviving_effect_param<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
) -> bool {
    let owner = semantic.key(db).owner(db);
    let effect_bindings = owner_effect_bindings(db, owner);
    if effect_bindings.is_empty() {
        return false;
    }
    runtime_visible_binding_plans(db, semantic)
        .iter()
        .any(|entry| {
            effect_bindings
                .iter()
                .any(|binding| same_owner_effect_binding(entry.binding, *binding))
        })
}

/// The by-value TRANSPORT class of a wasm export root's visible value param:
/// the representation the host passes directly as a wasm function argument
/// (u64 -> i64 scalar; `MemPtr<B::Word>` -> the memory-provider ref the wasm
/// lowerer represents as i32). This mirrors the class a caller passing the same
/// value would use (a param's `RuntimeParamPlan` boundary), so the exported
/// entry's body consumes the argument directly instead of the EVM entry ABI's
/// `load`-through-a-memory-reference. Params whose plan is not a direct exact
/// boundary (borrow/view/abstract) get `None` here and fall back to the default
/// class, which fails closed downstream in the wasm lowerer if it is reached
/// (out of the R1/keystone envelope). Effect params never reach this hook: an
/// admitted wasm root has none that survive erasure.
fn wasm_export_param_class<'db>(
    entry: &RuntimeVisibleBindingPlan<'db>,
) -> Option<RuntimeClass<'db>> {
    match &entry.plan {
        RuntimeParamPlan::Boundary(
            RuntimeBoundarySpec::ExactTransport(class) | RuntimeBoundarySpec::ExactShape(class),
        ) => Some(class.clone()),
        RuntimeParamPlan::Erased
        | RuntimeParamPlan::Boundary(RuntimeBoundarySpec::BorrowLike { .. })
        | RuntimeParamPlan::ReadOnlyView { .. }
        | RuntimeParamPlan::PassActual => None,
    }
}

pub fn build_test_runtime_package<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
    filter: Option<&str>,
) -> Result<RuntimePackage<'db>, LowerError> {
    let mut roots = Vec::new();
    let mut objects = Vec::new();
    for &func in top_mod.all_funcs(db) {
        if func.top_mod(db) != top_mod {
            continue;
        }
        let Some(attrs) = ItemKind::from(func).attrs(db) else {
            continue;
        };
        if attrs.get_attr(db, "test").is_none() {
            continue;
        }

        let name = func
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())
            .unwrap_or_else(|| "<anonymous>".to_string());
        if let Some(filter) = filter
            && !name.contains(filter)
        {
            continue;
        }

        let semantic = semantic_instance_for_root_owner(db, BodyOwner::Func(func))?;
        let entry_effect_args =
            entry_effect_arg_plans(db, EntryEffectContext::TestFunc { func }, semantic)?;
        let runtime_root = runtime_instance_for_semantic(db, semantic);
        let root = synthetic_instance(
            db,
            RuntimeSyntheticSpec::TestRoot {
                name: name.clone(),
                callee: runtime_root,
                entry_effect_args: entry_effect_args.into_boxed_slice(),
            },
            Vec::new(),
        );
        roots.push(root);
        objects.push((
            sanitize_object_name(&name),
            vec![(RuntimeSectionName::Test(name), root)],
        ));
    }

    let primary = (objects.len() == 1).then(|| objects[0].0.clone());
    let package = build_sectioned_package(
        db,
        top_mod,
        roots,
        objects,
        primary.as_deref(),
        FxHashSet::default(),
    )?;
    verify_runtime_package(db, package)
        .map_err(|err| LowerError::Unsupported(format!("invalid runtime package: {err:?}")))?;
    Ok(package)
}

fn build_contract_package<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
) -> Result<RuntimePackage<'db>, LowerError> {
    let mut roots = Vec::new();
    let mut objects = Vec::new();
    let contracts = top_mod.all_contracts(db);
    for &contract in contracts {
        let (name, sections, section_roots) = contract_object_spec(db, contract)?;
        roots.extend(section_roots);
        objects.push((name, sections));
    }
    for (name, sections, section_roots) in manual_contract_objects(db, top_mod)? {
        roots.extend(section_roots);
        objects.push((name, sections));
    }

    let primary = (objects.len() == 1).then(|| objects[0].0.clone());
    let package = build_sectioned_package(
        db,
        top_mod,
        roots,
        objects,
        primary.as_deref(),
        FxHashSet::default(),
    )?;
    verify_runtime_package(db, package)
        .map_err(|err| LowerError::Unsupported(format!("invalid runtime package: {err:?}")))?;
    Ok(package)
}

fn manual_contract_objects<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
) -> Result<Vec<ManualContractObjectSpec<'db>>, LowerError> {
    let roots = discover_manual_contract_roots(db, top_mod)?;
    let mut by_contract =
        FxHashMap::<String, (Option<RuntimeInstance<'db>>, Option<RuntimeInstance<'db>>)>::default(
        );
    for root in roots {
        let entry = by_contract
            .entry(root.contract_name.to_string())
            .or_insert((None, None));
        match root.section {
            ManualContractSection::Init => {
                if entry.0.replace(root.instance).is_some() {
                    return Err(LowerError::Unsupported(format!(
                        "duplicate #[contract_init({})] root in package",
                        root.contract_name
                    )));
                }
            }
            ManualContractSection::Runtime => {
                if entry.1.replace(root.instance).is_some() {
                    return Err(LowerError::Unsupported(format!(
                        "duplicate #[contract_runtime({})] root in package",
                        root.contract_name
                    )));
                }
            }
        }
    }

    let high_level_names = top_mod
        .all_contracts(db)
        .iter()
        .filter_map(|contract| {
            contract
                .name(db)
                .to_opt()
                .map(|name| name.data(db).to_string())
        })
        .collect::<FxHashSet<_>>();
    for contract_name in by_contract.keys() {
        if high_level_names.contains(contract_name) {
            return Err(LowerError::Unsupported(format!(
                "manual contract roots for `{contract_name}` conflict with a high-level contract of the same name"
            )));
        }
    }

    let mut objects = by_contract
        .into_iter()
        .map(|(contract_name, (init, runtime))| {
            let mut sections = Vec::new();
            let mut roots = Vec::new();
            if let Some(init) = init {
                sections.push((RuntimeSectionName::Init, init));
                roots.push(init);
            }
            if let Some(runtime) = runtime {
                sections.push((RuntimeSectionName::Runtime, runtime));
                roots.push(runtime);
            }
            (sanitize_object_name(&contract_name), sections, roots)
        })
        .collect::<Vec<_>>();
    objects.sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));
    Ok(objects)
}

fn manual_contract_object_for_root<'db>(
    db: &'db dyn MirDb,
    func: Func<'db>,
) -> Result<Option<ManualContractObjectSpec<'db>>, LowerError> {
    let Some(attr) = func.manual_contract_root_attr(db) else {
        return Ok(None);
    };
    let contract_name = match attr {
        ManualContractRootAttr::Init { contract_name }
        | ManualContractRootAttr::Runtime { contract_name } => contract_name.data(db),
        ManualContractRootAttr::Error(err) => {
            return Err(LowerError::Unsupported(format!(
                "invalid manual contract root attr on `{}`: {err:?}",
                func.name(db)
                    .to_opt()
                    .map(|name| name.data(db).to_string())
                    .unwrap_or_else(|| "<anonymous>".to_string())
            )));
        }
    };
    let object_name = sanitize_object_name(contract_name);
    Ok(manual_contract_objects(db, func.top_mod(db))?
        .into_iter()
        .find(|(name, _, _)| name == &object_name))
}

fn discover_manual_contract_roots<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
) -> Result<Vec<ManualContractRoot<'db>>, LowerError> {
    let mut roots = Vec::new();
    for &func in top_mod.all_funcs(db) {
        if func.top_mod(db) != top_mod {
            continue;
        }
        let Some(attr) = func.manual_contract_root_attr(db) else {
            continue;
        };
        let (contract_name, section) = match attr {
            ManualContractRootAttr::Init { contract_name } => {
                (contract_name.data(db), ManualContractSection::Init)
            }
            ManualContractRootAttr::Runtime { contract_name } => {
                (contract_name.data(db), ManualContractSection::Runtime)
            }
            ManualContractRootAttr::Error(err) => {
                return Err(LowerError::Unsupported(format!(
                    "invalid manual contract root attr on `{}`: {err:?}",
                    func.name(db)
                        .to_opt()
                        .map(|name| name.data(db).to_string())
                        .unwrap_or_else(|| "<anonymous>".to_string())
                )));
            }
        };
        if !func.arg_tys(db).is_empty() || func.return_ty(db) != TyId::unit(db) {
            return Err(LowerError::Unsupported(format!(
                "manual contract root `{}` must be monomorphic, unit-returning, and take no ordinary value params",
                func.name(db)
                    .to_opt()
                    .map(|name| name.data(db).to_string())
                    .unwrap_or_else(|| "<anonymous>".to_string())
            )));
        }
        roots.push(ManualContractRoot {
            func,
            instance: manual_contract_root_instance(db, func)?,
            contract_name,
            section,
        });
    }
    roots.sort_by_key(|root| {
        (
            root.contract_name.to_string(),
            matches!(root.section, ManualContractSection::Runtime),
            root.func
                .name(db)
                .to_opt()
                .map(|name| name.data(db).to_string()),
        )
    });
    Ok(roots)
}

pub(crate) fn manual_contract_root_instance<'db>(
    db: &'db dyn MirDb,
    func: Func<'db>,
) -> Result<RuntimeInstance<'db>, LowerError> {
    let semantic = semantic_instance_for_root_owner(db, BodyOwner::Func(func))?;
    let callee = runtime_instance_for_semantic(db, semantic);
    let entry_effect_args = entry_effect_arg_plans(
        db,
        EntryEffectContext::ManualContractRoot { func },
        semantic,
    )?;
    Ok(synthetic_instance(
        db,
        RuntimeSyntheticSpec::ManualContractRoot {
            func,
            callee,
            entry_effect_args: entry_effect_args.into_boxed_slice(),
        },
        Vec::new(),
    ))
}

fn contract_runtime_root<'db>(
    db: &'db dyn MirDb,
    contract: Contract<'db>,
) -> Result<RuntimeInstance<'db>, LowerError> {
    let abi_ty = sol_abi_ty(db, contract.scope())?;
    let mut dispatch = Vec::new();
    let mut default = DispatchDefault::RevertEmpty;
    for arm in contract.recv_views(db).flat_map(|recv| recv.arms(db)) {
        let (abi_info, wrapper) = contract_recv_wrapper(db, arm, abi_ty)?;
        if abi_info.is_fallback {
            if matches!(default, DispatchDefault::Call { .. }) {
                return Err(LowerError::Unsupported(format!(
                    "contract `{}` has multiple fallback recv arms",
                    contract_name(db, contract)
                )));
            }
            default = DispatchDefault::Call { wrapper };
            continue;
        }

        let selector = abi_info.selector_value.ok_or_else(|| {
            LowerError::Unsupported(format!(
                "recv arm in `{}` is missing a resolved selector",
                contract_name(db, contract)
            ))
        })?;
        dispatch.push(DispatchArm { selector, wrapper });
    }
    dispatch.sort_by_key(|arm| arm.selector);
    Ok(synthetic_instance(
        db,
        RuntimeSyntheticSpec::ContractRuntimeRoot {
            contract,
            dispatch: dispatch.into_boxed_slice(),
            default,
        },
        Vec::new(),
    ))
}

fn contract_init_root<'db>(
    db: &'db dyn MirDb,
    contract: Contract<'db>,
) -> Result<RuntimeInstance<'db>, LowerError> {
    let init_abi = contract_init_abi(db, contract)?;
    Ok(synthetic_instance(
        db,
        RuntimeSyntheticSpec::ContractInitRoot {
            contract,
            init_abi,
            runtime_region: RuntimeCodeRegion::new(
                db,
                RuntimeCodeRegionKey::ContractRuntime { contract },
            ),
        },
        Vec::new(),
    ))
}

fn contract_init_abi<'db>(
    db: &'db dyn MirDb,
    contract: Contract<'db>,
) -> Result<RuntimeInstance<'db>, LowerError> {
    let plan = contract_init_abi_plan(db, contract)?;
    Ok(synthetic_instance(
        db,
        RuntimeSyntheticSpec::ContractInitAbi { plan },
        Vec::new(),
    ))
}

fn contract_init_abi_plan<'db>(
    db: &'db dyn MirDb,
    contract: Contract<'db>,
) -> Result<ContractInitAbiPlan<'db>, LowerError> {
    let Some(init) = contract.init(db) else {
        return Ok(ContractInitAbiPlan {
            contract,
            payable: false,
            user_init: None,
            entry_effect_args: Box::new([]),
            init_args: InitArgsPlan::None,
        });
    };

    let semantic = semantic_instance_for_root_owner(db, BodyOwner::ContractInit { contract })?;
    let user_init = Some(runtime_instance_for_semantic(db, semantic));
    let entry_effect_args = entry_effect_arg_plans(
        db,
        EntryEffectContext::HighLevelContract { contract },
        semantic,
    )?;
    let projected_fields = visible_init_arg_fields(db, semantic);
    let init_args = if contract.init_args_ty(db) == TyId::unit(db) {
        InitArgsPlan::None
    } else {
        InitArgsPlan::DecodeInitTail {
            tuple_ty: contract.init_args_ty(db),
            decode_fn: resolve_decode_instance(
                db,
                contract.scope(),
                contract.init_args_ty(db),
                memory_bytes_ty(db, contract.scope())?,
            )?,
            projected_fields,
        }
    };
    Ok(ContractInitAbiPlan {
        contract,
        payable: init.is_payable(db),
        user_init,
        entry_effect_args: entry_effect_args.into_boxed_slice(),
        init_args,
    })
}

fn contract_recv_wrapper<'db>(
    db: &'db dyn MirDb,
    arm: RecvArmView<'db>,
    abi_ty: TyId<'db>,
) -> Result<(RecvArmAbiInfo<'db>, RuntimeInstance<'db>), LowerError> {
    let contract = arm.contract(db);
    let abi_info = arm.abi_info(db, abi_ty);
    let recv = arm.recv(db);
    let owner = BodyOwner::ContractRecvArm {
        contract,
        recv_idx: recv.recv_idx(db),
        arm_idx: arm.arm_idx(db),
    };
    let semantic = semantic_instance_for_root_owner(db, owner)?;
    let user_recv = runtime_instance_for_semantic(db, semantic);
    let entry_effect_args = entry_effect_arg_plans(
        db,
        EntryEffectContext::HighLevelContract { contract },
        semantic,
    )?;
    let projected_fields = visible_recv_arg_fields(db, semantic, arm);
    let input = if abi_info.args_ty == TyId::unit(db) {
        RuntimeInputPlan::None
    } else {
        let host = contract_recv_host_binding(db, contract, recv.recv_idx(db), arm.arm_idx(db))?;
        RuntimeInputPlan::DecodeHostPayload {
            msg_ty: abi_info.args_ty,
            decode_args_fn: resolve_decode_runtime_args_instance(
                db,
                contract.scope(),
                host.declared_ty,
                host.class.clone(),
                abi_info.args_ty,
            )?,
            host,
            projected_fields,
        }
    };
    let ret = if let Some(ret_ty) = abi_info.ret_ty {
        RuntimeReturnPlan::Value { ty: ret_ty }
    } else {
        RuntimeReturnPlan::Unit
    };
    let wrapper = synthetic_instance(
        db,
        RuntimeSyntheticSpec::ContractRecvAbi {
            plan: ContractRecvAbiPlan {
                contract,
                selector: abi_info.selector_value,
                payable: match arm.arm(db) {
                    Some(recv_arm) => recv_arm.is_payable(db),
                    None => false,
                },
                user_recv,
                entry_effect_args: entry_effect_args.into_boxed_slice(),
                input,
                ret,
            },
        },
        Vec::new(),
    );
    Ok((abi_info, wrapper))
}

fn contract_recv_host_binding<'db>(
    db: &'db dyn MirDb,
    contract: Contract<'db>,
    recv_idx: u32,
    arm_idx: u32,
) -> Result<TargetRootProviderBinding<'db>, LowerError> {
    let site = EffectParamSite::ContractRecvArm {
        contract,
        recv_idx,
        arm_idx,
    };
    let registration = hir::analysis::ty::registered_root_providers(db, site)
        .first()
        .ok_or_else(|| {
            LowerError::Unsupported(format!(
                "contract `{}` recv arm has no registered root provider",
                contract_name(db, contract)
            ))
        })?;
    let env = RuntimeTypeEnv::new(
        Some(contract.scope()),
        hir::analysis::ty::trait_resolution::PredicateListId::empty_list(db),
    );
    let class = provider_class_for_target_in_env(
        db,
        env,
        Some(registration.provider_ty),
        AddressSpaceKind::Memory,
    );
    let materialization = target_root_provider_materialization(&class).ok_or_else(|| {
        LowerError::Unsupported(format!(
            "contract `{}` recv root provider class `{:?}` has no supported entry materialization",
            contract_name(db, contract),
            class
        ))
    })?;
    Ok(TargetRootProviderBinding {
        declared_ty: registration.provider_ty,
        class,
        materialization,
    })
}

fn build_non_contract_package<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
    roots: Vec<RuntimeInstance<'db>>,
    object_specs: Vec<(String, RuntimeSectionName, RuntimeInstance<'db>)>,
    primary_object_name: Option<&str>,
    public_export_funcs: FxHashSet<Func<'db>>,
) -> Result<RuntimePackage<'db>, LowerError> {
    build_sectioned_package(
        db,
        top_mod,
        roots,
        object_specs
            .into_iter()
            .map(|(name, section, entry)| (name, vec![(section, entry)]))
            .collect(),
        primary_object_name,
        public_export_funcs,
    )
}

fn build_sectioned_package<'db>(
    db: &'db dyn MirDb,
    top_mod: TopLevelMod<'db>,
    roots: Vec<RuntimeInstance<'db>>,
    object_specs: Vec<(String, Vec<(RuntimeSectionName, RuntimeInstance<'db>)>)>,
    primary_object_name: Option<&str>,
    public_export_funcs: FxHashSet<Func<'db>>,
) -> Result<RuntimePackage<'db>, LowerError> {
    let root_object_names = object_specs
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<FxHashSet<_>>();
    let mut graph =
        RuntimeGraphBuilder::new(db, roots, object_specs, public_export_funcs).build()?;
    let functions = collect_runtime_functions(db, &graph);
    let functions_by_instance = functions
        .iter()
        .map(|function| (function.instance(db), *function))
        .collect::<FxHashMap<_, _>>();
    let const_regions = collect_const_regions(db, &graph);
    let mut reachable_cache = FxHashMap::default();

    let mut objects = std::mem::take(&mut graph.object_specs)
        .into_iter()
        .map(|(name, sections)| {
            make_runtime_object(
                db,
                name,
                sections
                    .into_iter()
                    .map(|(section_name, entry_instance)| {
                        let entry = *functions_by_instance
                            .get(&entry_instance)
                            .expect("section entry should be declared as a runtime function");
                        let reachable = collect_reachable_from_entry(
                            &graph,
                            entry_instance,
                            &mut reachable_cache,
                        );
                        RuntimeSection {
                            name: section_name,
                            entry,
                            embeds: Vec::new(),
                            const_regions: collect_const_regions_for_reachable(
                                db, &graph, &reachable,
                            ),
                        }
                    })
                    .collect(),
            )
        })
        .collect::<Vec<_>>();

    if !graph.code_region_roots.is_empty() {
        let code_regions_object =
            build_code_regions_object(db, &graph, &functions_by_instance, &mut reachable_cache);
        objects.push(code_regions_object);
    }

    let code_regions = resolve_code_regions(
        db,
        &objects,
        &functions_by_instance,
        &graph.code_region_roots,
    );
    let code_region_map = code_regions
        .iter()
        .map(|region| (region.region(db), *region))
        .collect::<FxHashMap<_, _>>();
    objects = objects
        .into_iter()
        .map(|object| {
            rewrite_object_embeds(db, &graph, object, &code_region_map, &mut reachable_cache)
        })
        .collect();
    objects = remap_object_section_refs(db, &objects);
    let code_regions = remap_resolved_code_regions(db, &objects, code_regions);

    let root_objects: Vec<_> = objects
        .iter()
        .filter(|object| root_object_names.contains(&object.name(db)))
        .copied()
        .collect();
    let primary_object = primary_object_name.and_then(|primary| {
        objects
            .iter()
            .find(|object| object.name(db) == primary)
            .copied()
    });

    Ok(RuntimePackage::new(
        db,
        top_mod,
        functions,
        RuntimePackagePlan::new(
            db,
            objects,
            const_regions,
            code_regions,
            root_objects,
            primary_object,
        ),
    ))
}

fn build_code_regions_object<'db>(
    db: &'db dyn MirDb,
    graph: &RuntimeGraph<'db>,
    functions_by_instance: &FxHashMap<RuntimeInstance<'db>, RuntimeFunction<'db>>,
    reachable_cache: &mut FxHashMap<RuntimeInstance<'db>, FxHashSet<RuntimeInstance<'db>>>,
) -> RuntimeObject<'db> {
    let sections = graph
        .code_region_roots
        .iter()
        .map(|(region, instance)| {
            let RuntimeCodeRegionKey::FunctionRoot { symbol, .. } = region.key(db).clone() else {
                unreachable!();
            };
            let entry = *functions_by_instance
                .get(instance)
                .expect("code-region root should be declared as a runtime function");
            let reachable = collect_reachable_from_entry(graph, *instance, reachable_cache);
            RuntimeSection {
                name: RuntimeSectionName::CodeRegion(symbol),
                entry,
                embeds: Vec::new(),
                const_regions: collect_const_regions_for_reachable(db, graph, &reachable),
            }
        })
        .collect();
    make_runtime_object(db, "CodeRegions".to_string(), sections)
}

fn rewrite_object_embeds<'db>(
    db: &'db dyn MirDb,
    graph: &RuntimeGraph<'db>,
    object: RuntimeObject<'db>,
    code_region_map: &FxHashMap<RuntimeCodeRegion<'db>, ResolvedCodeRegion<'db>>,
    reachable_cache: &mut FxHashMap<RuntimeInstance<'db>, FxHashSet<RuntimeInstance<'db>>>,
) -> RuntimeObject<'db> {
    let section_refs = code_region_map
        .iter()
        .map(|(region, resolved)| (*region, resolved.source(db).clone()))
        .collect::<FxHashMap<_, _>>();
    let sections = object
        .sections(db)
        .iter()
        .cloned()
        .map(|mut section| {
            let reachable =
                collect_reachable_from_entry(graph, section.entry.instance(db), reachable_cache);
            section.embeds = collect_region_embeds(
                db,
                graph,
                &reachable,
                &section_refs,
                RuntimeSectionRef::Local {
                    object,
                    section: section.name.clone(),
                },
            );
            section
        })
        .collect();
    make_runtime_object(db, object.name(db).clone(), sections)
}

fn remap_resolved_code_regions<'db>(
    db: &'db dyn MirDb,
    objects: &[RuntimeObject<'db>],
    code_regions: Vec<ResolvedCodeRegion<'db>>,
) -> Vec<ResolvedCodeRegion<'db>> {
    code_regions
        .into_iter()
        .map(|region| {
            make_resolved_code_region(
                db,
                region.region(db),
                region.symbol(db).clone(),
                remap_section_ref(db, objects, region.source(db).clone()),
                region.root(db),
            )
        })
        .collect()
}

fn remap_object_section_refs<'db>(
    db: &'db dyn MirDb,
    objects: &[RuntimeObject<'db>],
) -> Vec<RuntimeObject<'db>> {
    objects
        .iter()
        .map(|object| {
            let sections = object
                .sections(db)
                .iter()
                .cloned()
                .map(|mut section| {
                    section.embeds = section
                        .embeds
                        .into_iter()
                        .map(|embed| crate::runtime::RuntimeEmbed {
                            source: remap_section_ref(db, objects, embed.source),
                            as_symbol: embed.as_symbol,
                        })
                        .collect();
                    section
                })
                .collect();
            make_runtime_object(db, object.name(db).clone(), sections)
        })
        .collect()
}

fn remap_section_ref<'db>(
    db: &'db dyn MirDb,
    objects: &[RuntimeObject<'db>],
    section_ref: RuntimeSectionRef<'db>,
) -> RuntimeSectionRef<'db> {
    let (old_object, section, is_local) = match section_ref {
        RuntimeSectionRef::Local { object, section } => (object, section, true),
        RuntimeSectionRef::External { object, section } => (object, section, false),
    };
    let object = objects
        .iter()
        .find(|candidate| candidate.name(db) == old_object.name(db))
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "missing rewritten runtime object `{}` while remapping section ref",
                old_object.name(db)
            )
        });
    if is_local {
        RuntimeSectionRef::Local { object, section }
    } else {
        RuntimeSectionRef::External { object, section }
    }
}

fn resolve_code_regions<'db>(
    db: &'db dyn MirDb,
    objects: &[RuntimeObject<'db>],
    functions_by_instance: &FxHashMap<RuntimeInstance<'db>, RuntimeFunction<'db>>,
    function_roots: &[(RuntimeCodeRegion<'db>, RuntimeInstance<'db>)],
) -> Vec<ResolvedCodeRegion<'db>> {
    let mut resolved = Vec::new();
    for object in objects {
        for section in object.sections(db) {
            let Some((region, symbol)) = resolved_section_region(db, &section) else {
                continue;
            };
            resolved.push(make_resolved_code_region(
                db,
                region,
                symbol,
                RuntimeSectionRef::Local {
                    object: *object,
                    section: section.name.clone(),
                },
                section.entry,
            ));
        }
    }

    if let Some(code_regions_object) = objects
        .iter()
        .find(|object| object.name(db) == "CodeRegions")
    {
        for (region, root_instance) in function_roots {
            let RuntimeCodeRegionKey::FunctionRoot { symbol, .. } = region.key(db).clone() else {
                continue;
            };
            let Some(_section) = code_regions_object
                .sections(db)
                .iter()
                .find(|section| section.name == RuntimeSectionName::CodeRegion(symbol.clone()))
            else {
                continue;
            };
            resolved.push(make_resolved_code_region(
                db,
                *region,
                symbol.clone(),
                RuntimeSectionRef::Local {
                    object: *code_regions_object,
                    section: RuntimeSectionName::CodeRegion(symbol),
                },
                *functions_by_instance
                    .get(root_instance)
                    .expect("code-region root should be declared as a runtime function"),
            ));
        }
    }

    resolved.sort_by_key(|region| region.symbol(db).clone());
    resolved
}

fn resolved_section_region<'db>(
    db: &'db dyn MirDb,
    section: &RuntimeSection<'db>,
) -> Option<(RuntimeCodeRegion<'db>, String)> {
    match section.entry.owner(db) {
        RuntimeFunctionOwner::Synthetic(
            RuntimeSyntheticSpec::ContractInitRoot { contract, .. }
            | RuntimeSyntheticSpec::ContractRuntimeRoot { contract, .. },
        ) => match section.name {
            RuntimeSectionName::Init => Some((
                RuntimeCodeRegion::new(db, RuntimeCodeRegionKey::ContractInit { contract }),
                format!("{}_init", contract_name(db, contract)),
            )),
            RuntimeSectionName::Runtime => Some((
                RuntimeCodeRegion::new(db, RuntimeCodeRegionKey::ContractRuntime { contract }),
                format!("{}_runtime", contract_name(db, contract)),
            )),
            RuntimeSectionName::Main
            | RuntimeSectionName::Test(_)
            | RuntimeSectionName::CodeRegion(_) => None,
        },
        RuntimeFunctionOwner::Synthetic(RuntimeSyntheticSpec::ManualContractRoot {
            func, ..
        }) => {
            let region = runtime_code_region_for_manual_root(db, func)?;
            Some((region, code_region_symbol(db, region)))
        }
        RuntimeFunctionOwner::Semantic(semantic) => {
            let BodyOwner::Func(func) = semantic.key(db).owner(db) else {
                return None;
            };
            let region = runtime_code_region_for_manual_root(db, func)?;
            Some((region, code_region_symbol(db, region)))
        }
        RuntimeFunctionOwner::Synthetic(
            RuntimeSyntheticSpec::MainRoot { .. }
            | RuntimeSyntheticSpec::TestRoot { .. }
            | RuntimeSyntheticSpec::ContractInitAbi { .. }
            | RuntimeSyntheticSpec::ContractRecvAbi { .. }
            | RuntimeSyntheticSpec::CodeRegionRoot { .. },
        ) => None,
    }
}

fn collect_region_embeds<'db>(
    db: &'db dyn MirDb,
    graph: &RuntimeGraph<'db>,
    reachable: &FxHashSet<RuntimeInstance<'db>>,
    section_refs: &FxHashMap<RuntimeCodeRegion<'db>, RuntimeSectionRef<'db>>,
    current_section: RuntimeSectionRef<'db>,
) -> Vec<crate::runtime::RuntimeEmbed<'db>> {
    let current_object = match &current_section {
        RuntimeSectionRef::Local { object, .. } | RuntimeSectionRef::External { object, .. } => {
            *object
        }
    };
    let mut seen = FxHashSet::default();
    let mut embeds = Vec::new();
    let mut instances = reachable.iter().copied().collect::<Vec<_>>();
    instances.sort_by_cached_key(|instance| {
        (
            runtime_instance_sort_key(db, *instance),
            runtime_instance_symbol_key(db, *instance),
        )
    });
    for instance in instances {
        let Some(node) = graph.nodes.get(&instance) else {
            continue;
        };
        for region in node.referenced_code_regions.iter().copied() {
            let Some(source) = section_refs.get(&region) else {
                continue;
            };
            if *source == current_section || !seen.insert(region) {
                continue;
            }
            let source = match source {
                RuntimeSectionRef::Local { object, section }
                | RuntimeSectionRef::External { object, section }
                    if *object == current_object =>
                {
                    RuntimeSectionRef::Local {
                        object: *object,
                        section: section.clone(),
                    }
                }
                RuntimeSectionRef::Local { object, section }
                | RuntimeSectionRef::External { object, section } => RuntimeSectionRef::External {
                    object: *object,
                    section: section.clone(),
                },
            };
            embeds.push(crate::runtime::RuntimeEmbed {
                source,
                as_symbol: code_region_symbol(db, region),
            });
        }
    }
    embeds.sort_by(|lhs, rhs| lhs.as_symbol.cmp(&rhs.as_symbol));
    embeds
}

fn collect_reachable_from_entry<'db>(
    graph: &RuntimeGraph<'db>,
    entry: RuntimeInstance<'db>,
    cache: &mut FxHashMap<RuntimeInstance<'db>, FxHashSet<RuntimeInstance<'db>>>,
) -> FxHashSet<RuntimeInstance<'db>> {
    if let Some(reachable) = cache.get(&entry) {
        return reachable.clone();
    }
    let mut seen = FxHashSet::default();
    let mut stack = vec![entry];
    while let Some(instance) = stack.pop() {
        if !seen.insert(instance) {
            continue;
        }
        if let Some(node) = graph.nodes.get(&instance) {
            for callee in node.direct_callees.iter().copied() {
                stack.push(callee);
            }
        }
    }
    cache.insert(entry, seen.clone());
    seen
}

fn collect_const_regions_for_reachable<'db>(
    db: &'db dyn MirDb,
    graph: &RuntimeGraph<'db>,
    reachable: &FxHashSet<RuntimeInstance<'db>>,
) -> Vec<ConstRegionId<'db>> {
    let mut seen = FxHashSet::default();
    let mut regions = Vec::new();
    let mut instances = reachable.iter().copied().collect::<Vec<_>>();
    instances.sort_by_cached_key(|instance| {
        (
            runtime_instance_sort_key(db, *instance),
            runtime_instance_symbol_key(db, *instance),
        )
    });
    for instance in instances {
        let Some(node) = graph.nodes.get(&instance) else {
            continue;
        };
        for region in node.referenced_const_regions.iter().copied() {
            if seen.insert(region) {
                regions.push(region);
            }
        }
    }
    regions
}

fn materialized_contracts_for_roots<'db>(
    db: &'db dyn MirDb,
    roots: &[RuntimeInstance<'db>],
) -> FxHashSet<Contract<'db>> {
    roots
        .iter()
        .filter_map(|root| match root.key(db).source(db) {
            RuntimeInstanceSource::Synthetic(synthetic) => match synthetic.spec(db) {
                RuntimeSyntheticSpec::ContractInitRoot { contract, .. }
                | RuntimeSyntheticSpec::ContractRuntimeRoot { contract, .. } => Some(contract),
                RuntimeSyntheticSpec::MainRoot { .. }
                | RuntimeSyntheticSpec::TestRoot { .. }
                | RuntimeSyntheticSpec::ManualContractRoot { .. }
                | RuntimeSyntheticSpec::ContractInitAbi { .. }
                | RuntimeSyntheticSpec::ContractRecvAbi { .. }
                | RuntimeSyntheticSpec::CodeRegionRoot { .. } => None,
            },
            RuntimeInstanceSource::Semantic(_) => None,
        })
        .collect()
}

fn semantic_instance_for_root_owner<'db>(
    db: &'db dyn MirDb,
    owner: BodyOwner<'db>,
) -> Result<SemanticInstance<'db>, LowerError> {
    let key = root_semantic_instance_key(db, owner).map_err(|err| match err {
        RootSemanticInstanceError::UnsupportedGenericParam {
            owner,
            owner_scope,
            offending_ty,
            param_idx,
        } => LowerError::Unsupported(format!(
            "root semantic instance for {owner:?} has unsupported generic param {param_idx} in {owner_scope:?}: {}",
            offending_ty.pretty_print(db),
        )),
        RootSemanticInstanceError::MissingRootProvider { owner } => LowerError::Unsupported(
            format!("root semantic instance for {owner:?} is missing a root provider binding"),
        ),
        RootSemanticInstanceError::UnclosedEffectEnv(err) => LowerError::Unsupported(format!(
            "root semantic instance for {:?} is not closed under synthesized root substitution: owner_scope={:?} param_idx={} args_len={} offending_ty={}",
            err.owner,
            err.owner_scope,
            err.param_idx,
            err.args_len,
            err.offending_ty.pretty_print(db),
        )),
    })?;
    Ok(get_or_build_semantic_instance(db, key))
}

fn contract_object_spec<'db>(
    db: &'db dyn MirDb,
    contract: Contract<'db>,
) -> Result<ManualContractObjectSpec<'db>, LowerError> {
    let runtime_root = contract_runtime_root(db, contract)?;
    let init_root = contract_init_root(db, contract)?;
    Ok((
        sanitize_object_name(&contract_name(db, contract)),
        vec![
            (RuntimeSectionName::Init, init_root),
            (RuntimeSectionName::Runtime, runtime_root),
        ],
        vec![init_root, runtime_root],
    ))
}

fn is_test_func<'db>(db: &'db dyn MirDb, func: Func<'db>) -> bool {
    ItemKind::from(func)
        .attrs(db)
        .is_some_and(|attrs| attrs.get_attr(db, "test").is_some())
}

fn runtime_root_candidate<'db>(
    db: &'db dyn MirDb,
    func: Func<'db>,
) -> Result<RuntimeRootCandidate<'db>, LowerError> {
    if func.is_associated_func(db) || func.params(db).next().is_some() {
        return Ok(RuntimeRootCandidate::NotRoot);
    }
    let semantic = match root_semantic_instance_key(db, BodyOwner::Func(func)) {
        Ok(key) => get_or_build_semantic_instance(db, key),
        Err(err) => {
            return Ok(RuntimeRootCandidate::Rejected(RuntimeRootRejection {
                func,
                reason: RuntimeRootRejectionReason::RootSemanticInstance(err),
            }));
        }
    };
    if let Err(err) =
        entry_effect_arg_plans(db, EntryEffectContext::StandaloneFunc { func }, semantic)
    {
        return Ok(RuntimeRootCandidate::Rejected(RuntimeRootRejection {
            func,
            reason: RuntimeRootRejectionReason::UnsupportedEntryEffect(err.to_string()),
        }));
    }
    Ok(RuntimeRootCandidate::Root(func))
}

fn is_main_func<'db>(db: &'db dyn MirDb, func: Func<'db>) -> bool {
    func.name(db)
        .to_opt()
        .is_some_and(|name| name.data(db) == "main")
}

fn func_display_name<'db>(db: &'db dyn MirDb, func: Func<'db>) -> String {
    func.name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .unwrap_or_else(|| "<anonymous>".to_string())
}

fn format_runtime_root_rejection<'db>(
    db: &'db dyn MirDb,
    rejection: &RuntimeRootRejection<'db>,
) -> String {
    let name = func_display_name(db, rejection.func);
    match &rejection.reason {
        RuntimeRootRejectionReason::RootSemanticInstance(err) => {
            format_root_semantic_instance_rejection(db, &name, err)
        }
        RuntimeRootRejectionReason::UnsupportedEntryEffect(message) => message.clone(),
    }
}

fn format_root_semantic_instance_rejection<'db>(
    db: &'db dyn MirDb,
    func_name: &str,
    err: &RootSemanticInstanceError<'db>,
) -> String {
    match err {
        RootSemanticInstanceError::UnsupportedGenericParam {
            offending_ty,
            param_idx,
            ..
        } if is_implicit_layout_const_param(db, *offending_ty) => format!(
            "function `{func_name}` cannot be used as a standalone runtime root because an effect provider type contains an inferred layout const parameter `{}` at generic parameter {param_idx}; roots cannot declare wildcard effect providers because there is no caller to supply a concrete provider. Move the effectful logic into a helper and call it from `{func_name}` with a concrete provider using `with (...)`, or use a contract field/provider context",
            offending_ty.pretty_print(db),
        ),
        RootSemanticInstanceError::UnsupportedGenericParam {
            offending_ty,
            param_idx,
            ..
        } => format!(
            "function `{func_name}` cannot be used as a standalone runtime root because generic parameter {param_idx} is not supported for root instantiation: {}",
            offending_ty.pretty_print(db),
        ),
        RootSemanticInstanceError::MissingRootProvider { .. } => format!(
            "function `{func_name}` cannot be used as a standalone runtime root because an effect provider could not be synthesized"
        ),
        RootSemanticInstanceError::UnclosedEffectEnv(err) => format!(
            "function `{func_name}` cannot be used as a standalone runtime root because its effect environment is not fully concrete: parameter {} is missing while instantiating {}",
            err.param_idx,
            err.offending_ty.pretty_print(db),
        ),
    }
}

fn is_implicit_layout_const_param<'db>(db: &'db dyn MirDb, ty: TyId<'db>) -> bool {
    if let TyData::ConstTy(const_ty) = ty.data(db)
        && let ConstTyData::TyParam(param, _) = const_ty.data(db)
    {
        return param.is_implicit();
    }
    false
}

pub(crate) fn runtime_instance_for_semantic<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
) -> RuntimeInstance<'db> {
    runtime_instance_for_semantic_with_visible_param_overrides(db, semantic, |_| None)
}

pub(crate) fn runtime_instance_for_semantic_with_visible_param_overrides<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
    mut override_class: impl FnMut(&RuntimeVisibleBindingPlan<'db>) -> Option<RuntimeClass<'db>>,
) -> RuntimeInstance<'db> {
    let typed_body = semantic.key(db).typed_body(db);
    let owner = semantic.key(db).owner(db);
    if let BodyOwner::Func(func) = owner
        && func.body(db).is_none()
    {
        panic!(
            "bodyless semantic function leaked into runtime instance construction: func={func:?} key={:?}",
            semantic.key(db)
        );
    }
    let env = RuntimeTypeEnv::for_semantic(db, semantic);
    let params: Vec<_> = runtime_visible_binding_plans(db, semantic)
        .iter()
        .map(|entry| {
            override_class(entry).unwrap_or_else(|| {
                runtime_class_for_visible_binding_entry(db, semantic, typed_body, owner, env, entry)
            })
        })
        .collect();
    let key = RuntimeInstanceKey::new(db, RuntimeInstanceSource::Semantic(semantic), params);
    get_or_build_runtime_instance(db, key)
}

fn runtime_class_for_visible_binding_entry<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
    typed_body: &hir::analysis::ty::ty_check::TypedBody<'db>,
    owner: BodyOwner<'db>,
    env: RuntimeTypeEnv<'db>,
    entry: &RuntimeVisibleBindingPlan<'db>,
) -> RuntimeClass<'db> {
    if owner_effect_bindings(db, owner)
        .into_iter()
        .any(|binding| same_owner_effect_binding(binding, entry.binding))
    {
        return owner_effect_binding_class(db, semantic, entry.binding).unwrap_or_else(|| {
            panic!(
                "runtime-visible owner effect binding has no runtime class: {:?}",
                entry
            )
        });
    }
    if matches!(entry.binding, LocalBinding::Local { .. }) {
        return top_level_class_for_ty_in_env(db, env, entry.semantic_ty, AddressSpaceKind::Memory)
            .unwrap_or_else(|| {
                panic!(
                    "runtime-visible recv arg binding has no top-level runtime class: {:?}",
                    entry
                )
            });
    }
    runtime_visible_binding_class(db, semantic, entry.binding)
        .map(|class| runtime_param_class(db, typed_body, entry.binding, env, class))
        .unwrap_or_else(|| {
            panic!(
                "runtime-visible typed binding has no runtime class: {:?}",
                entry
            )
        })
}

fn owner_effect_binding_class<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
    binding: hir::analysis::ty::ty_check::LocalBinding<'db>,
) -> Option<crate::runtime::RuntimeClass<'db>> {
    runtime_effect_binding_plan(db, semantic, binding).map(|plan| plan.class)
}

fn synthetic_instance<'db>(
    db: &'db dyn MirDb,
    spec: RuntimeSyntheticSpec<'db>,
    params: Vec<crate::runtime::RuntimeClass<'db>>,
) -> RuntimeInstance<'db> {
    let synthetic = RuntimeSyntheticInstance::new(db, spec);
    let key = RuntimeInstanceKey::new(db, RuntimeInstanceSource::Synthetic(synthetic), params);
    get_or_build_runtime_instance(db, key)
}

fn resolve_decode_instance<'db>(
    db: &'db dyn MirDb,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
    ty: TyId<'db>,
    input_ty: TyId<'db>,
) -> Result<RuntimeInstance<'db>, LowerError> {
    let abi_ty = sol_abi_ty(db, scope)?;
    let decoder_ty = sol_decoder_ty(db, scope, input_ty)?;
    let decode_trait = resolve_core_trait(db, scope, &["abi", "Decode"])
        .ok_or_else(|| LowerError::Unsupported("missing required core::abi::Decode".to_string()))?;
    let inst = TraitInstId::new_simple(db, decode_trait, vec![ty, abi_ty]);
    resolve_trait_runtime_instance(db, scope, inst, "decode_payload", vec![decoder_ty])
}

fn resolve_decode_runtime_args_instance<'db>(
    db: &'db dyn MirDb,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
    host_ty: TyId<'db>,
    host_class: RuntimeClass<'db>,
    msg_ty: TyId<'db>,
) -> Result<RuntimeInstance<'db>, LowerError> {
    let func = resolve_lib_func_path(db, scope, "core::contracts::decode_runtime_args")
        .ok_or_else(|| {
            LowerError::Unsupported(
                "missing required core::contracts::decode_runtime_args".to_string(),
            )
        })?;
    let assumptions = hir::analysis::ty::trait_resolution::PredicateListId::empty_list(db);
    let key = SemanticInstanceKey::new(
        db,
        BodyOwner::Func(func),
        GenericSubst::new(db, vec![host_ty, sol_abi_ty(db, scope)?, msg_ty]),
        hir::analysis::semantic::EffectProviderSubst::empty(db),
        ImplEnv::new(db, scope, assumptions, vec![]),
    );
    let semantic = get_or_build_semantic_instance(db, key);
    Ok(runtime_instance_for_semantic_with_visible_param_overrides(
        db,
        semantic,
        |entry| {
            if matches!(entry.binding, LocalBinding::Param { idx: 0, .. }) {
                Some(host_class.clone())
            } else {
                None
            }
        },
    ))
}

fn resolve_trait_runtime_instance<'db>(
    db: &'db dyn MirDb,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
    inst: TraitInstId<'db>,
    method: &str,
    extra_generic_args: Vec<TyId<'db>>,
) -> Result<RuntimeInstance<'db>, LowerError> {
    let assumptions = hir::analysis::ty::trait_resolution::PredicateListId::empty_list(db);
    let method = IdentId::new(db, method.to_string());
    let resolved = resolve_trait_method_instance(
        db,
        TraitSolveCx::new(db, scope).with_assumptions(assumptions),
        inst,
        method,
    )
    .ok_or_else(|| {
        LowerError::Unsupported(format!(
            "failed to resolve trait method `{}` for runtime package planning",
            method.data(db)
        ))
    })?;
    // DETERMINISM ASSERTION (rung 3.3): NOT applicable at this site, for the same
    // reason as the twin helper in `synthetic.rs`. This synthesizes a *fresh*
    // `TraitInstId` inside the MIR runtime-package planner (e.g. `core::abi::Decode`
    // built by `resolve_decode_instance`), with empty assumptions and no upstream
    // typeck instance carrying a committed `selected_implementor`. There is no
    // pinned choice to compare against here (carried value is structurally
    // `None`); the invariant is enforced at the `classify.rs` site that
    // re-resolves an instance typeck already committed to.
    let func = resolved.func;
    let mut impl_args = resolved.impl_args;
    impl_args.extend(extra_generic_args);
    let key = SemanticInstanceKey::new(
        db,
        BodyOwner::Func(func),
        GenericSubst::new(db, impl_args),
        hir::analysis::semantic::EffectProviderSubst::empty(db),
        ImplEnv::new(db, scope, assumptions, vec![inst]),
    );
    Ok(runtime_instance_for_semantic(
        db,
        get_or_build_semantic_instance(db, key),
    ))
}

fn sol_abi_ty<'db>(
    db: &'db dyn MirDb,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
) -> Result<TyId<'db>, LowerError> {
    resolve_lib_type_path(db, scope, "std::abi::Sol")
        .ok_or_else(|| LowerError::Unsupported("missing std::abi::Sol".to_string()))
}

fn visible_init_arg_fields<'db>(db: &'db dyn MirDb, semantic: SemanticInstance<'db>) -> Box<[u32]> {
    runtime_visible_binding_plans(db, semantic)
        .iter()
        .filter_map(|entry| match entry.binding {
            LocalBinding::Param { idx, .. } => Some(idx as u32),
            LocalBinding::Local { .. } | LocalBinding::EffectParam { .. } => None,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn visible_recv_arg_fields<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
    arm: RecvArmView<'db>,
) -> Box<[u32]> {
    let tuple_indices_by_pat = arm
        .arg_bindings(db)
        .iter()
        .map(|binding| (binding.pat, binding.tuple_index))
        .collect::<FxHashMap<_, _>>();
    runtime_visible_binding_plans(db, semantic)
        .iter()
        .filter_map(|entry| match entry.binding {
            LocalBinding::Local { pat, .. } => tuple_indices_by_pat.get(&pat).copied(),
            LocalBinding::Param { .. } | LocalBinding::EffectParam { .. } => None,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn memory_bytes_ty<'db>(
    db: &'db dyn MirDb,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
) -> Result<TyId<'db>, LowerError> {
    resolve_lib_type_path(db, scope, "std::evm::memory_input::MemoryBytes").ok_or_else(|| {
        LowerError::Unsupported("missing std::evm::memory_input::MemoryBytes".to_string())
    })
}

fn sol_decoder_ty<'db>(
    db: &'db dyn MirDb,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
    input_ty: TyId<'db>,
) -> Result<TyId<'db>, LowerError> {
    let ctor = resolve_lib_type_path(db, scope, "std::abi::sol::SolDecoder")
        .ok_or_else(|| LowerError::Unsupported("missing std::abi::sol::SolDecoder".to_string()))?;
    Ok(TyId::app(db, ctor, input_ty))
}

fn make_runtime_function<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
    symbol: String,
    linkage: RuntimeLinkage,
    inline_hint: RuntimeInlineHint,
    owner: RuntimeFunctionOwner<'db>,
    referenced_const_regions: Vec<ConstRegionId<'db>>,
) -> RuntimeFunction<'db> {
    RuntimeFunction::new(
        db,
        instance,
        symbol,
        linkage,
        inline_hint,
        owner,
        referenced_const_regions,
    )
}

fn make_runtime_object<'db>(
    db: &'db dyn MirDb,
    name: String,
    sections: Vec<RuntimeSection<'db>>,
) -> RuntimeObject<'db> {
    RuntimeObject::new(db, name, sections)
}

fn make_resolved_code_region<'db>(
    db: &'db dyn MirDb,
    region: RuntimeCodeRegion<'db>,
    symbol: String,
    source: RuntimeSectionRef<'db>,
    root: RuntimeFunction<'db>,
) -> ResolvedCodeRegion<'db> {
    ResolvedCodeRegion::new(db, region, symbol, source, root)
}

const RUNTIME_SYMBOL_DISAMBIG_HASH_LEN: usize = 4;

fn collect_runtime_functions<'db>(
    db: &'db dyn MirDb,
    graph: &RuntimeGraph<'db>,
) -> Vec<RuntimeFunction<'db>> {
    let mut instances = graph.nodes.keys().copied().collect::<Vec<_>>();
    instances.sort_by_cached_key(|instance| {
        (
            runtime_instance_sort_key(db, *instance),
            runtime_instance_symbol_key(db, *instance),
        )
    });
    let instance_symbols = instances
        .into_iter()
        .map(|instance| {
            (
                instance,
                runtime_instance_symbol_base(db, instance),
                runtime_instance_symbol_key(db, instance),
            )
        })
        .collect::<Vec<_>>();
    let duplicate_counts =
        instance_symbols
            .iter()
            .fold(FxHashMap::default(), |mut counts, (_, base, _)| {
                *counts.entry(base.clone()).or_insert(0usize) += 1;
                counts
            });
    let mut emitted_counts = FxHashMap::<String, usize>::default();
    let mut functions = instance_symbols
        .into_iter()
        .map(|(instance, base, symbol_key)| {
            let needs_disambiguator = duplicate_counts.get(&base).copied().unwrap_or_default() > 1;
            let symbol = runtime_instance_symbol(
                base,
                &symbol_key,
                needs_disambiguator,
                &mut emitted_counts,
            );
            runtime_function_for_instance(
                db,
                instance,
                symbol,
                graph.public_roots.contains(&instance)
                    || instance_is_declared_public_export(db, instance, &graph.public_export_funcs),
                graph
                    .nodes
                    .get(&instance)
                    .expect("runtime graph should contain every materialized instance")
                    .referenced_const_regions
                    .clone(),
            )
        })
        .collect::<Vec<_>>();
    functions.sort_by_key(|function| function.symbol(db));
    functions
}

fn runtime_instance_symbol(
    base: String,
    stable_key: &str,
    needs_disambiguator: bool,
    emitted_counts: &mut FxHashMap<String, usize>,
) -> String {
    let mut symbol = if needs_disambiguator {
        let hash = stable_identity_hash(stable_key);
        format!("{base}_{}", &hash[..RUNTIME_SYMBOL_DISAMBIG_HASH_LEN])
    } else {
        base
    };
    let ordinal = emitted_counts.entry(symbol.clone()).or_insert(0);
    if *ordinal > 0 {
        symbol = format!("{symbol}_{ordinal}");
    }
    *ordinal += 1;
    symbol
}

/// Is this instance one the source declared `pub` in the entry module?
///
/// Export eligibility must not be inferred from root-seeding: entry-only root
/// seeding drops callee-reachable candidates on purpose, and that was only safe
/// while the wasm backend exported every function (sonatina `ac266c21` ended
/// that). An empty set, as on the EVM path, makes this a no-op.
fn instance_is_declared_public_export<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
    public_export_funcs: &FxHashSet<Func<'db>>,
) -> bool {
    if public_export_funcs.is_empty() {
        return false;
    }
    let RuntimeInstanceSource::Semantic(semantic) = instance.key(db).source(db) else {
        return false;
    };
    let BodyOwner::Func(func) = semantic.key(db).owner(db) else {
        return false;
    };
    public_export_funcs.contains(&func)
}

fn runtime_function_for_instance<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
    symbol: String,
    public_entry: bool,
    referenced_const_regions: Vec<ConstRegionId<'db>>,
) -> RuntimeFunction<'db> {
    match instance.key(db).source(db) {
        RuntimeInstanceSource::Semantic(semantic) => {
            // A non-builtin `extern` is a DECLARED-EXTERNAL runtime function (no
            // body, defined outside the module): the wasm backend turns it into a
            // `("fe", <symbol>)` host import. Every other semantic function is a
            // locally-defined `Private` symbol (byte-identical to before; EVM
            // externs are recognized builtins, never declared-external here).
            let linkage = if declared_external_func(db, semantic).is_some() {
                RuntimeLinkage::External
            } else if public_entry {
                RuntimeLinkage::Internal
            } else {
                RuntimeLinkage::Private
            };
            make_runtime_function(
                db,
                instance,
                symbol,
                linkage,
                inline_hint_for_semantic(db, semantic),
                RuntimeFunctionOwner::Semantic(semantic),
                referenced_const_regions,
            )
        }
        RuntimeInstanceSource::Synthetic(synthetic) => {
            let spec = synthetic.spec(db).clone();
            let inline_hint = match &spec {
                RuntimeSyntheticSpec::ContractInitAbi { .. }
                | RuntimeSyntheticSpec::ContractRecvAbi { .. } => RuntimeInlineHint::Always,
                _ => RuntimeInlineHint::Auto,
            };
            make_runtime_function(
                db,
                instance,
                symbol,
                if public_entry {
                    RuntimeLinkage::Internal
                } else {
                    RuntimeLinkage::Private
                },
                inline_hint,
                RuntimeFunctionOwner::Synthetic(spec),
                referenced_const_regions,
            )
        }
    }
}

pub fn runtime_instance_stable_key<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> String {
    runtime_instance_sort_key(db, instance)
}

pub fn runtime_instance_symbol_key<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> String {
    runtime_instance_symbol_key_query(db, instance)
}

fn runtime_instance_sort_key<'db>(db: &'db dyn MirDb, instance: RuntimeInstance<'db>) -> String {
    runtime_instance_sort_key_query(db, instance)
}

#[salsa::tracked]
fn runtime_instance_symbol_key_query<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> String {
    let key = instance.key(db);
    let source = runtime_instance_source_symbol_key(db, key.source(db));
    let params = key
        .params(db)
        .iter()
        .map(|param| runtime_class_sort_key(db, param))
        .collect::<Vec<_>>()
        .join(",");
    format!("{source}:params[{params}]")
}

#[salsa::tracked]
fn runtime_instance_sort_key_query<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> String {
    let key = instance.key(db);
    let source = runtime_instance_source_sort_key(db, key.source(db));
    let params = key
        .params(db)
        .iter()
        .map(|param| runtime_class_sort_key(db, param))
        .collect::<Vec<_>>()
        .join(",");
    format!("{source}:params[{params}]")
}

fn runtime_instance_source_symbol_key<'db>(
    db: &'db dyn MirDb,
    source: RuntimeInstanceSource<'db>,
) -> String {
    match source {
        RuntimeInstanceSource::Semantic(semantic) => {
            format!(
                "semantic:{}",
                semantic_instance_symbol_identity(db, semantic)
            )
        }
        RuntimeInstanceSource::Synthetic(synthetic) => {
            runtime_synthetic_spec_symbol_key(db, synthetic.spec(db))
        }
    }
}

fn runtime_instance_source_sort_key<'db>(
    db: &'db dyn MirDb,
    source: RuntimeInstanceSource<'db>,
) -> String {
    match source {
        RuntimeInstanceSource::Semantic(semantic) => {
            format!("semantic:{}", semantic_instance_identity(db, semantic))
        }
        RuntimeInstanceSource::Synthetic(synthetic) => {
            runtime_synthetic_spec_sort_key(db, synthetic.spec(db))
        }
    }
}

fn runtime_synthetic_spec_symbol_key<'db>(
    db: &'db dyn MirDb,
    spec: RuntimeSyntheticSpec<'db>,
) -> String {
    match spec {
        RuntimeSyntheticSpec::MainRoot {
            callee,
            entry_effect_args,
        } => format!(
            "__synthetic:main_root:{}:{}",
            runtime_instance_symbol_key(db, callee),
            entry_effect_args_sort_key(db, entry_effect_args.as_ref())
        ),
        RuntimeSyntheticSpec::TestRoot {
            name,
            callee,
            entry_effect_args,
        } => format!(
            "__synthetic:test_root:{name}:{}:{}",
            runtime_instance_symbol_key(db, callee),
            entry_effect_args_sort_key(db, entry_effect_args.as_ref())
        ),
        RuntimeSyntheticSpec::ManualContractRoot {
            func,
            callee,
            entry_effect_args,
        } => format!(
            "__synthetic:manual_contract_root:{}:{}:{}",
            item_identity(db, func.into()),
            runtime_instance_symbol_key(db, callee),
            entry_effect_args_sort_key(db, entry_effect_args.as_ref())
        ),
        RuntimeSyntheticSpec::ContractInitAbi { plan } => format!(
            "__synthetic:contract_init_abi:{}:{}:{}:{}",
            item_identity(db, plan.contract.into()),
            plan.payable,
            plan.user_init
                .map(|instance| runtime_instance_symbol_key(db, instance))
                .unwrap_or_default(),
            init_args_plan_symbol_key(db, &plan.init_args)
        ),
        RuntimeSyntheticSpec::ContractRecvAbi { plan } => format!(
            "__synthetic:contract_recv_abi:{}:{}:{}:{}:{}",
            item_identity(db, plan.contract.into()),
            plan.selector
                .map_or_else(|| "fallback".to_string(), |selector| selector.to_string()),
            plan.payable,
            runtime_instance_symbol_key(db, plan.user_recv),
            runtime_input_plan_symbol_key(db, &plan.input)
        ),
        RuntimeSyntheticSpec::ContractInitRoot {
            contract,
            init_abi,
            runtime_region,
        } => format!(
            "__synthetic:contract_init_root:{}:{}:{}",
            item_identity(db, contract.into()),
            runtime_instance_symbol_key(db, init_abi),
            runtime_code_region_symbol_key(db, runtime_region)
        ),
        RuntimeSyntheticSpec::ContractRuntimeRoot {
            contract,
            dispatch,
            default,
        } => format!(
            "__synthetic:contract_runtime_root:{}:{}:{}",
            item_identity(db, contract.into()),
            dispatch
                .iter()
                .map(|arm| format!(
                    "{}:{}",
                    arm.selector,
                    runtime_instance_symbol_key(db, arm.wrapper)
                ))
                .collect::<Vec<_>>()
                .join(","),
            dispatch_default_symbol_key(db, &default)
        ),
        RuntimeSyntheticSpec::CodeRegionRoot { symbol, callee } => {
            format!(
                "__synthetic:code_region_root:{symbol}:{}",
                runtime_instance_symbol_key(db, callee)
            )
        }
    }
}

fn runtime_synthetic_spec_sort_key<'db>(
    db: &'db dyn MirDb,
    spec: RuntimeSyntheticSpec<'db>,
) -> String {
    match spec {
        RuntimeSyntheticSpec::MainRoot {
            callee,
            entry_effect_args,
        } => format!(
            "__synthetic:main_root:{}:{}",
            runtime_instance_sort_key(db, callee),
            entry_effect_args_sort_key(db, entry_effect_args.as_ref())
        ),
        RuntimeSyntheticSpec::TestRoot {
            name,
            callee,
            entry_effect_args,
        } => format!(
            "__synthetic:test_root:{name}:{}:{}",
            runtime_instance_sort_key(db, callee),
            entry_effect_args_sort_key(db, entry_effect_args.as_ref())
        ),
        RuntimeSyntheticSpec::ManualContractRoot {
            func,
            callee,
            entry_effect_args,
        } => format!(
            "__synthetic:manual_contract_root:{}:{}:{}",
            item_identity(db, func.into()),
            runtime_instance_sort_key(db, callee),
            entry_effect_args_sort_key(db, entry_effect_args.as_ref())
        ),
        RuntimeSyntheticSpec::ContractInitAbi { plan } => format!(
            "__synthetic:contract_init_abi:{}:{}:{}:{}",
            item_identity(db, plan.contract.into()),
            plan.payable,
            plan.user_init
                .map(|instance| runtime_instance_sort_key(db, instance))
                .unwrap_or_default(),
            init_args_plan_sort_key(db, &plan.init_args)
        ),
        RuntimeSyntheticSpec::ContractRecvAbi { plan } => format!(
            "__synthetic:contract_recv_abi:{}:{}:{}:{}:{}",
            item_identity(db, plan.contract.into()),
            plan.selector
                .map_or_else(|| "fallback".to_string(), |selector| selector.to_string()),
            plan.payable,
            runtime_instance_sort_key(db, plan.user_recv),
            runtime_input_plan_sort_key(db, &plan.input)
        ),
        RuntimeSyntheticSpec::ContractInitRoot {
            contract,
            init_abi,
            runtime_region,
        } => format!(
            "__synthetic:contract_init_root:{}:{}:{}",
            item_identity(db, contract.into()),
            runtime_instance_sort_key(db, init_abi),
            runtime_code_region_sort_key(db, runtime_region)
        ),
        RuntimeSyntheticSpec::ContractRuntimeRoot {
            contract,
            dispatch,
            default,
        } => format!(
            "__synthetic:contract_runtime_root:{}:{}:{}",
            item_identity(db, contract.into()),
            dispatch
                .iter()
                .map(|arm| format!(
                    "{}:{}",
                    arm.selector,
                    runtime_instance_sort_key(db, arm.wrapper)
                ))
                .collect::<Vec<_>>()
                .join(","),
            dispatch_default_sort_key(db, &default)
        ),
        RuntimeSyntheticSpec::CodeRegionRoot { symbol, callee } => {
            format!(
                "__synthetic:code_region_root:{symbol}:{}",
                runtime_instance_sort_key(db, callee)
            )
        }
    }
}

fn entry_effect_args_sort_key<'db>(db: &'db dyn MirDb, args: &[EntryEffectArgPlan<'db>]) -> String {
    args.iter()
        .map(|arg| match arg {
            EntryEffectArgPlan::ContractField(binding) => format!(
                "field:{}:{}:{}:{}:{}",
                binding.slot,
                type_identity(db, binding.declared_ty),
                runtime_class_sort_key(db, &binding.class),
                ref_kind_sort_key(db, &binding.kind),
                binding.init_immutable
            ),
            EntryEffectArgPlan::TargetRootProvider(binding) => format!(
                "root:{}:{}:{}",
                type_identity(db, binding.declared_ty),
                runtime_class_sort_key(db, &binding.class),
                target_root_provider_materialization_sort_key(db, binding.materialization)
            ),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn init_args_plan_symbol_key<'db>(db: &'db dyn MirDb, plan: &InitArgsPlan<'db>) -> String {
    match plan {
        InitArgsPlan::None => "none".to_string(),
        InitArgsPlan::DecodeInitTail {
            tuple_ty,
            decode_fn,
            projected_fields,
        } => format!(
            "decode:{}:{}:{projected_fields:?}",
            type_identity(db, *tuple_ty),
            runtime_instance_symbol_key(db, *decode_fn)
        ),
    }
}

fn init_args_plan_sort_key<'db>(db: &'db dyn MirDb, plan: &InitArgsPlan<'db>) -> String {
    match plan {
        InitArgsPlan::None => "none".to_string(),
        InitArgsPlan::DecodeInitTail {
            tuple_ty,
            decode_fn,
            projected_fields,
        } => format!(
            "decode:{}:{}:{projected_fields:?}",
            type_identity(db, *tuple_ty),
            runtime_instance_sort_key(db, *decode_fn)
        ),
    }
}

fn runtime_input_plan_symbol_key<'db>(db: &'db dyn MirDb, plan: &RuntimeInputPlan<'db>) -> String {
    match plan {
        RuntimeInputPlan::None => "none".to_string(),
        RuntimeInputPlan::DecodeHostPayload {
            msg_ty,
            host,
            decode_args_fn,
            projected_fields,
        } => format!(
            "decode:{}:{}:{}:{projected_fields:?}",
            type_identity(db, *msg_ty),
            target_root_provider_binding_sort_key(db, host),
            runtime_instance_symbol_key(db, *decode_args_fn)
        ),
    }
}

fn runtime_input_plan_sort_key<'db>(db: &'db dyn MirDb, plan: &RuntimeInputPlan<'db>) -> String {
    match plan {
        RuntimeInputPlan::None => "none".to_string(),
        RuntimeInputPlan::DecodeHostPayload {
            msg_ty,
            host,
            decode_args_fn,
            projected_fields,
        } => format!(
            "decode:{}:{}:{}:{projected_fields:?}",
            type_identity(db, *msg_ty),
            target_root_provider_binding_sort_key(db, host),
            runtime_instance_sort_key(db, *decode_args_fn)
        ),
    }
}

fn target_root_provider_binding_sort_key<'db>(
    db: &'db dyn MirDb,
    binding: &TargetRootProviderBinding<'db>,
) -> String {
    format!(
        "{}:{}:{}",
        type_identity(db, binding.declared_ty),
        runtime_class_sort_key(db, &binding.class),
        target_root_provider_materialization_sort_key(db, binding.materialization)
    )
}

fn dispatch_default_symbol_key<'db>(db: &'db dyn MirDb, default: &DispatchDefault<'db>) -> String {
    match default {
        DispatchDefault::RevertEmpty => "revert_empty".to_string(),
        DispatchDefault::Call { wrapper } => {
            format!("call:{}", runtime_instance_symbol_key(db, *wrapper))
        }
    }
}

fn dispatch_default_sort_key<'db>(db: &'db dyn MirDb, default: &DispatchDefault<'db>) -> String {
    match default {
        DispatchDefault::RevertEmpty => "revert_empty".to_string(),
        DispatchDefault::Call { wrapper } => {
            format!("call:{}", runtime_instance_sort_key(db, *wrapper))
        }
    }
}

fn runtime_code_region_symbol_key<'db>(
    db: &'db dyn MirDb,
    region: RuntimeCodeRegion<'db>,
) -> String {
    match region.key(db) {
        RuntimeCodeRegionKey::ContractInit { contract } => {
            format!("contract_init:{}", item_identity(db, contract.into()))
        }
        RuntimeCodeRegionKey::ContractRuntime { contract } => {
            format!("contract_runtime:{}", item_identity(db, contract.into()))
        }
        RuntimeCodeRegionKey::ManualContractRoot { func } => {
            format!("manual_root:{}", item_identity(db, func.into()))
        }
        RuntimeCodeRegionKey::FunctionRoot { symbol, callee } => {
            format!(
                "function_root:{symbol}:{}",
                runtime_instance_symbol_key(db, callee)
            )
        }
    }
}

fn runtime_code_region_sort_key<'db>(db: &'db dyn MirDb, region: RuntimeCodeRegion<'db>) -> String {
    match region.key(db) {
        RuntimeCodeRegionKey::ContractInit { contract } => {
            format!("contract_init:{}", item_identity(db, contract.into()))
        }
        RuntimeCodeRegionKey::ContractRuntime { contract } => {
            format!("contract_runtime:{}", item_identity(db, contract.into()))
        }
        RuntimeCodeRegionKey::ManualContractRoot { func } => {
            format!("manual_root:{}", item_identity(db, func.into()))
        }
        RuntimeCodeRegionKey::FunctionRoot { symbol, callee } => {
            format!(
                "function_root:{symbol}:{}",
                runtime_instance_sort_key(db, callee)
            )
        }
    }
}

fn runtime_class_sort_key<'db>(db: &'db dyn MirDb, class: &RuntimeClass<'db>) -> String {
    match class {
        RuntimeClass::Scalar(class) => scalar_class_sort_key(db, class),
        RuntimeClass::AggregateValue { layout } => {
            format!("agg:{}", layout_sort_key(db, *layout))
        }
        RuntimeClass::Ref {
            pointee,
            kind,
            view,
        } => format!(
            "ref:{}:{}:{}",
            ref_kind_sort_key(db, kind),
            ref_view_sort_key(db, view),
            runtime_class_sort_key(db, pointee)
        ),
        RuntimeClass::RawAddr { space, target } => format!(
            "raw:{}:{}",
            address_space_sort_key(*space),
            target
                .map(|layout| layout_sort_key(db, layout))
                .unwrap_or_default()
        ),
    }
}

fn scalar_class_sort_key<'db>(db: &'db dyn MirDb, class: &ScalarClass<'db>) -> String {
    format!(
        "{}:{}",
        scalar_repr_sort_key(class.repr),
        scalar_role_sort_key(db, &class.role)
    )
}

fn scalar_repr_sort_key(repr: ScalarRepr) -> String {
    match repr {
        ScalarRepr::Bool => "bool".to_string(),
        ScalarRepr::Int { bits, signed } => format!("int:{bits}:{signed}"),
        ScalarRepr::Float { bits } => format!("float:{bits}"),
        ScalarRepr::FixedBytes { len } => format!("bytes:{len}"),
        ScalarRepr::Address { bits } => format!("address:{bits}"),
    }
}

fn scalar_role_sort_key<'db>(db: &'db dyn MirDb, role: &ScalarRole<'db>) -> String {
    match role {
        ScalarRole::Plain => "plain".to_string(),
        ScalarRole::EnumTag { enum_layout } => {
            format!("enum_tag:{}", layout_sort_key(db, *enum_layout))
        }
    }
}

fn ref_kind_sort_key<'db>(db: &'db dyn MirDb, kind: &RefKind<'db>) -> String {
    match kind {
        RefKind::Const => "const".to_string(),
        RefKind::Object => "object".to_string(),
        RefKind::Provider { provider_ty, space } => {
            format!(
                "provider:{}:{}",
                type_identity(db, *provider_ty),
                address_space_sort_key(*space)
            )
        }
    }
}

fn ref_view_sort_key<'db>(db: &'db dyn MirDb, view: &RefView<'db>) -> String {
    match view {
        RefView::Whole => "whole".to_string(),
        RefView::EnumVariant(variant) => format!(
            "variant:{}:{}",
            layout_sort_key(db, variant.enum_layout),
            variant.index
        ),
    }
}

fn target_root_provider_materialization_sort_key<'db>(
    db: &'db dyn MirDb,
    materialization: TargetRootProviderMaterialization<'db>,
) -> String {
    match materialization {
        TargetRootProviderMaterialization::MemoryObject { layout } => {
            format!("memory_object:{}", layout_sort_key(db, layout))
        }
        TargetRootProviderMaterialization::MemoryRawAddr { layout } => {
            format!("memory_raw_addr:{}", layout_sort_key(db, layout))
        }
    }
}

fn layout_sort_key<'db>(db: &'db dyn MirDb, layout: LayoutId<'db>) -> String {
    layout_sort_key_query(db, layout)
}

#[salsa::tracked]
fn layout_sort_key_query<'db>(db: &'db dyn MirDb, layout: LayoutId<'db>) -> String {
    match layout.key(db) {
        LayoutKey::Struct(layout) => format!(
            "struct:{}:[{}]",
            type_identity(db, layout.source_ty),
            layout
                .fields
                .iter()
                .map(|field| runtime_class_sort_key(db, field))
                .collect::<Vec<_>>()
                .join(",")
        ),
        LayoutKey::Array(layout) => format!(
            "array:{}:{}:{}",
            type_identity(db, layout.source_ty),
            runtime_class_sort_key(db, &layout.elem),
            layout.len
        ),
        LayoutKey::Enum(layout) => format!(
            "enum:{}:[{}]",
            type_identity(db, layout.source_ty),
            layout
                .variants
                .iter()
                .map(|variant| format!(
                    "{}:[{}]",
                    variant.name,
                    variant
                        .fields
                        .iter()
                        .map(|field| runtime_class_sort_key(db, field))
                        .collect::<Vec<_>>()
                        .join(",")
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn address_space_sort_key(space: AddressSpaceKind) -> &'static str {
    match space {
        AddressSpaceKind::Memory => "memory",
        AddressSpaceKind::Storage => "storage",
        AddressSpaceKind::Transient => "transient",
        AddressSpaceKind::Calldata => "calldata",
        AddressSpaceKind::Code => "code",
    }
}

fn runtime_instance_symbol_base<'db>(db: &'db dyn MirDb, instance: RuntimeInstance<'db>) -> String {
    match instance.key(db).source(db) {
        RuntimeInstanceSource::Semantic(semantic) => {
            symbol_base_for_semantic_instance(db, semantic)
        }
        RuntimeInstanceSource::Synthetic(synthetic) => {
            symbol_base_for_runtime_instance(db, &synthetic.spec(db))
        }
    }
}

fn wrap_runtime_lowering_error<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
    err: LowerError,
) -> LowerError {
    match err {
        LowerError::Unsupported(message) => LowerError::Unsupported(format!(
            "MIR lowering failed: unsupported while lowering `{}`: {message}",
            runtime_instance_symbol_base(db, instance)
        )),
        LowerError::NondeterministicReResolution(message) => {
            LowerError::NondeterministicReResolution(format!(
                "MIR lowering failed: nondeterministic re-resolution while lowering `{}`: {message}",
                runtime_instance_symbol_base(db, instance)
            ))
        }
        LowerError::ForgedRecordedImplementor(message) => {
            LowerError::ForgedRecordedImplementor(format!(
                "MIR lowering failed: forged recorded implementor while lowering `{}`: {message}",
                runtime_instance_symbol_base(db, instance)
            ))
        }
        LowerError::UnresolvedTraitSelection(message) => {
            LowerError::UnresolvedTraitSelection(format!(
                "MIR lowering failed: unresolved trait selection while lowering `{}`: {message}",
                runtime_instance_symbol_base(db, instance)
            ))
        }
    }
}

fn symbol_base_for_semantic_instance<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
) -> String {
    let owner = semantic.key(db).owner(db);
    match owner {
        BodyOwner::Func(func) => func
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())
            .unwrap_or_else(|| "__anon".to_string()),
        BodyOwner::ContractInit { contract } => format!(
            "__{}_init",
            contract
                .name(db)
                .to_opt()
                .map(|name| name.data(db).to_string())
                .unwrap_or_else(|| "contract".to_string())
        ),
        BodyOwner::ContractRecvArm {
            contract,
            recv_idx,
            arm_idx,
        } => format!(
            "__{}_recv_{}_{}",
            contract
                .name(db)
                .to_opt()
                .map(|name| name.data(db).to_string())
                .unwrap_or_else(|| "contract".to_string()),
            recv_idx,
            arm_idx
        ),
        BodyOwner::Const(_) | BodyOwner::AnonConstBody { .. } => "__const".to_string(),
    }
}

fn symbol_base_for_runtime_instance<'db>(
    db: &'db dyn MirDb,
    spec: &RuntimeSyntheticSpec<'db>,
) -> String {
    match spec {
        RuntimeSyntheticSpec::MainRoot { .. } => "main_root".to_string(),
        RuntimeSyntheticSpec::TestRoot { name, .. } => {
            format!("test_root_{}", sanitize_symbol(name))
        }
        RuntimeSyntheticSpec::ManualContractRoot { func, .. } => {
            let (contract_name, section) = match func.manual_contract_root_attr(db) {
                Some(ManualContractRootAttr::Init { contract_name }) => {
                    (contract_name.data(db), ManualContractSection::Init)
                }
                Some(ManualContractRootAttr::Runtime { contract_name }) => {
                    (contract_name.data(db), ManualContractSection::Runtime)
                }
                Some(ManualContractRootAttr::Error(_)) | None => {
                    return "manual_contract_root".to_string();
                }
            };
            let section = match section {
                ManualContractSection::Init => "init",
                ManualContractSection::Runtime => "runtime",
            };
            format!(
                "manual_contract_{section}_root_{}",
                sanitize_symbol(contract_name)
            )
        }
        RuntimeSyntheticSpec::ContractInitAbi { plan } => {
            format!("contract_init_abi_{}", contract_name(db, plan.contract))
        }
        RuntimeSyntheticSpec::ContractRecvAbi { plan } => format!(
            "contract_recv_abi_{}_{}",
            contract_name(db, plan.contract),
            plan.selector
                .map_or_else(|| "fallback".to_string(), |selector| selector.to_string()),
        ),
        RuntimeSyntheticSpec::ContractInitRoot { contract, .. } => {
            format!("contract_init_root_{}", contract_name(db, *contract))
        }
        RuntimeSyntheticSpec::ContractRuntimeRoot { contract, .. } => {
            format!("contract_runtime_root_{}", contract_name(db, *contract))
        }
        RuntimeSyntheticSpec::CodeRegionRoot { symbol, .. } => {
            format!("code_region_root_{}", sanitize_symbol(symbol))
        }
    }
}

fn inline_hint_for_semantic<'db>(
    db: &'db dyn MirDb,
    semantic: SemanticInstance<'db>,
) -> RuntimeInlineHint {
    match semantic.key(db).owner(db) {
        BodyOwner::Func(func) => match func.inline_hint(db) {
            Some(InlineHint::Hint) => RuntimeInlineHint::Hint,
            Some(InlineHint::Always) => RuntimeInlineHint::Always,
            Some(InlineHint::Never) => RuntimeInlineHint::Never,
            None => RuntimeInlineHint::Auto,
        },
        BodyOwner::Const(_)
        | BodyOwner::AnonConstBody { .. }
        | BodyOwner::ContractInit { .. }
        | BodyOwner::ContractRecvArm { .. } => RuntimeInlineHint::Auto,
    }
}

fn collect_const_regions<'db>(
    db: &'db dyn MirDb,
    graph: &RuntimeGraph<'db>,
) -> Vec<ConstRegionId<'db>> {
    let mut seen = FxHashSet::default();
    let mut regions = Vec::new();
    let mut instances = graph.nodes.keys().copied().collect::<Vec<_>>();
    instances.sort_by_cached_key(|instance| {
        (
            runtime_instance_sort_key(db, *instance),
            runtime_instance_symbol_key(db, *instance),
        )
    });
    for instance in instances {
        for region in graph
            .nodes
            .get(&instance)
            .expect("runtime graph should contain every materialized instance")
            .referenced_const_regions
            .iter()
            .copied()
        {
            if seen.insert(region) {
                regions.push(region);
            }
        }
    }
    regions
}

fn contract_name<'db>(db: &'db dyn MirDb, contract: hir::hir_def::Contract<'db>) -> String {
    contract
        .name(db)
        .to_opt()
        .map(|name| sanitize_symbol(name.data(db)))
        .unwrap_or_else(|| "contract".to_string())
}

fn sanitize_symbol(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn sanitize_object_name(value: &str) -> String {
    let sanitized = sanitize_symbol(value);
    if sanitized.is_empty() {
        "object".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use common::InputDb;
    use driver::DriverDataBase;
    use url::Url;

    use super::*;

    #[test]
    fn malformed_nested_generic_call_reports_label_diagnostic_without_panicking() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///invalid_generic_call.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
struct Leaf { value: u32 }
struct Node<A> { left: A, right: A }
impl Copy for Leaf {}
impl<A: Copy> Copy for Node<A> {}

trait SetLeaf { fn set_leaf(self, index: u32, value: u32) -> Self }
impl SetLeaf for Leaf {
    fn set_leaf(self, index: u32, value: u32) -> Self { Leaf { value: value } }
}
impl<A: SetLeaf + Copy> SetLeaf for Node<A> {
    fn set_leaf(self, index: u32, value: u32) -> Self {
        Node {
            left: self.left.set_leaf(wrong: index, value: value),
            right: self.right,
        }
    }
}

pub fn run(value: u32) -> Node<Leaf> {
    let leaf = Leaf { value: 0 }
    Node { left: leaf, right: leaf }.set_leaf(index: 0, value: value)
}
"#
                .to_string(),
            ),
        );

        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(
            diagnostics.contains("argument label mismatch")
                && diagnostics.contains("expected `index` label, but `wrong` given"),
            "expected an ordinary argument-label diagnostic:\n{diagnostics}"
        );

        // Labels do not affect the runtime ABI, so this call still has complete
        // semantic lowering metadata.  Direct package construction may lower
        // it safely; the compiler driver rejects it using the diagnostic above.
        build_wasm_runtime_package(&db, top_mod)
            .expect("diagnosed label mismatch must not make semantic lowering panic");
    }

    #[test]
    fn wasm_package_rejects_unresolved_nested_generic_method_before_smir_lowering() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///unresolved_nested_generic_method.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
struct Leaf { value: u32 }
struct Node<A> { left: A, right: A }
impl Copy for Leaf {}
impl<A: Copy> Copy for Node<A> {}

impl<A: Copy> Node<A> {
    fn malformed(self, value: u32) -> Self {
        Node {
            left: self.left.missing(value: value),
            right: self.right,
        }
    }
}

pub fn run(value: u32) -> Node<Leaf> {
    let leaf = Leaf { value: 0 }
    Node { left: leaf, right: leaf }.malformed(value: value)
}
"#
                .to_string(),
            ),
        );

        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(
            !diagnostics.is_empty(),
            "unresolved nested method must produce a type-checking diagnostic"
        );
        let err = build_wasm_runtime_package(&db, top_mod)
            .expect_err("missing method metadata must block semantic MIR lowering");
        let message = err.to_string();
        assert!(
            message.contains("cannot lower")
                && message.contains("type checking left unresolved or invalid body operations"),
            "unexpected fail-closed error: {message}"
        );
    }

    #[test]
    fn wasm_package_lowers_valid_nested_generic_method_calls() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///valid_nested_generic_call.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
struct Leaf { value: u32 }
struct Node<A> { left: A, right: A }
impl Copy for Leaf {}
impl<A: Copy> Copy for Node<A> {}

trait SetLeaf { fn set_leaf(self, index: u32, value: u32) -> Self }
impl SetLeaf for Leaf {
    fn set_leaf(self, index: u32, value: u32) -> Self { Leaf { value: value } }
}
impl<A: SetLeaf + Copy> SetLeaf for Node<A> {
    fn set_leaf(self, index: u32, value: u32) -> Self {
        Node {
            left: self.left.set_leaf(index: index, value: value),
            right: self.right,
        }
    }
}

pub fn run(value: u32) -> Node<Leaf> {
    let leaf = Leaf { value: 0 }
    Node { left: leaf, right: leaf }.set_leaf(index: 0, value: value)
}
"#
                .to_string(),
            ),
        );

        build_wasm_runtime_package(&db, db.top_mod(file))
            .expect("valid nested generic method calls must lower to a Wasm runtime package");
    }

    fn recv_wrapper_plan<'db>(
        db: &'db DriverDataBase,
        top_mod: TopLevelMod<'db>,
        selector_sig: &str,
    ) -> ContractRecvAbiPlan<'db> {
        let contract = top_mod
            .all_contracts(db)
            .first()
            .copied()
            .expect("fixture should define a contract");
        let abi_ty = sol_abi_ty(db, contract.scope()).expect("Sol ABI type");
        let recv = hir::semantic::RecvView::new(db, contract, 0);
        let arm = recv
            .arms(db)
            .find(|arm| {
                arm.abi_info(db, abi_ty).selector_signature.as_deref() == Some(selector_sig)
            })
            .unwrap_or_else(|| panic!("missing recv arm `{selector_sig}`"));
        let (_, wrapper) = contract_recv_wrapper(db, arm, abi_ty).expect("recv wrapper");
        let RuntimeInstanceSource::Synthetic(synthetic) = wrapper.key(db).source(db) else {
            panic!("recv wrapper should be synthetic");
        };
        match synthetic.spec(db) {
            RuntimeSyntheticSpec::ContractRecvAbi { plan } => plan.clone(),
            other => panic!("expected recv wrapper synthetic spec, got {other:?}"),
        }
    }

    fn with_test_runtime_package<T>(
        file_name: &str,
        source: &str,
        filter: Option<&str>,
        f: impl for<'db> FnOnce(&'db DriverDataBase, RuntimePackage<'db>) -> T,
    ) -> T {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse(&format!("file:///{file_name}")).unwrap();
        let file = db
            .workspace()
            .touch(&mut db, file_url, Some(source.to_string()));
        let top_mod = db.top_mod(file);
        let package =
            build_test_runtime_package(&db, top_mod, filter).expect("test package should build");
        f(&db, package)
    }

    fn package_object_names(db: &DriverDataBase, package: RuntimePackage<'_>) -> Vec<String> {
        let mut names = package
            .objects(db)
            .into_iter()
            .map(|object| object.name(db).clone())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn package_root_object_names(db: &DriverDataBase, package: RuntimePackage<'_>) -> Vec<String> {
        let mut names = package
            .root_objects(db)
            .into_iter()
            .map(|object| object.name(db).clone())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn filtered_test_package_excludes_unreferenced_contract_roots() {
        with_test_runtime_package(
            "filtered_test_package_excludes_unreferenced_contract_roots.fe",
            r#"
pub contract Unused {}

#[test]
fn selected() {
    assert!(true)
}

#[test]
fn ignored() {
    assert!(true)
}
"#,
            Some("selected"),
            |db, package| {
                assert_eq!(package_root_object_names(db, package), vec!["selected"]);
                assert_eq!(package_object_names(db, package), vec!["selected"]);
            },
        );
    }

    #[test]
    fn filtered_test_package_discovers_create2_contract_dependency() {
        with_test_runtime_package(
            "filtered_test_package_discovers_create2_contract_dependency.fe",
            r#"
use std::abi::sol
use std::evm::Evm

pub msg ChildMsg {
    #[selector = sol("get()")]
    Get -> u256,
}

pub contract Child {
    recv ChildMsg {
        Get -> u256 {
            1
        }
    }
}

pub contract Unused {}

#[test]
fn selected() uses (evm: mut Evm) {
    let addr = evm.create2<Child>(value: 0, args: (), salt: 1)
    assert!(addr.inner != 0)
}
"#,
            Some("selected"),
            |db, package| {
                let object_names = package_object_names(db, package);

                assert_eq!(package_root_object_names(db, package), vec!["selected"]);
                assert!(
                    object_names.contains(&"Child".to_string()),
                    "selected test package should discover create2 contract dependency: {object_names:?}"
                );
                assert!(
                    !object_names.contains(&"Unused".to_string()),
                    "selected test package should not include unreferenced contracts: {object_names:?}"
                );
            },
        );
    }

    #[test]
    fn filtered_test_package_without_matches_is_empty() {
        with_test_runtime_package(
            "filtered_test_package_without_matches_is_empty.fe",
            r#"
pub contract Unused {}

#[test]
fn selected() {}
"#,
            Some("missing"),
            |db, package| {
                assert!(package.root_objects(db).is_empty());
                assert!(package.objects(db).is_empty());
            },
        );
    }

    #[test]
    fn contract_recv_wrapper_projects_only_runtime_visible_fields_in_runtime_order() {
        let mut db = DriverDataBase::default();
        let file_url =
            Url::parse("file:///contract_recv_wrapper_projects_visible_fields.fe").unwrap();
        db.workspace().touch(
            &mut db,
            file_url.clone(),
            Some(
                r#"
use std::abi::sol

msg DecodeMsg {
    #[selector = sol("raw(uint256)")]
    Raw { value: u256 } -> u256,
    #[selector = sol("swap(uint64,uint64)")]
    Swap { a: u64, b: u64 } -> u64,
}

pub contract DecodeHarness {
    recv DecodeMsg {
        Raw { value: _ } -> u256 { 0 }
        Swap { b, a } -> u64 { a }
    }
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

        let raw_plan = recv_wrapper_plan(&db, top_mod, "raw(uint256)");
        let RuntimeInputPlan::DecodeHostPayload {
            projected_fields, ..
        } = raw_plan.input
        else {
            panic!("raw(uint256) should decode host payload");
        };
        assert!(
            projected_fields.is_empty(),
            "ignored recv arm fields must not be forwarded to the runtime callee: {projected_fields:?}"
        );

        let swap_plan = recv_wrapper_plan(&db, top_mod, "swap(uint64,uint64)");
        let RuntimeInputPlan::DecodeHostPayload {
            projected_fields, ..
        } = swap_plan.input
        else {
            panic!("swap(uint64,uint64) should decode host payload");
        };
        assert_eq!(
            projected_fields.as_ref(),
            &[1, 0],
            "recv wrapper must forward decoded fields in runtime-visible binding order, not tuple order"
        );
    }

    #[test]
    fn contract_init_wrapper_is_synthesized_for_no_init_contracts() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///contract_init_wrapper_is_synthesized.fe").unwrap();
        db.workspace().touch(
            &mut db,
            file_url.clone(),
            Some(
                r#"
pub contract NoInitBox {}
"#
                .to_string(),
            ),
        );
        let file = db
            .workspace()
            .get(&db, &file_url)
            .expect("file should be loaded");
        let top_mod = db.top_mod(file);
        let contract = top_mod
            .all_contracts(&db)
            .first()
            .copied()
            .expect("fixture should define a contract");

        let init_abi = contract_init_abi(&db, contract).expect("init abi wrapper");
        let RuntimeInstanceSource::Synthetic(synthetic) = init_abi.key(&db).source(&db) else {
            panic!("init abi should be synthetic");
        };
        let RuntimeSyntheticSpec::ContractInitAbi { plan } = synthetic.spec(&db) else {
            panic!("expected synthetic contract init abi");
        };
        assert!(
            !plan.payable,
            "implicit constructor wrapper must reject deployment value"
        );
        assert!(
            plan.user_init.is_none(),
            "implicit constructor wrapper should not call a user init"
        );
        assert!(
            plan.entry_effect_args.is_empty(),
            "implicit constructor wrapper should not synthesize owner effect args"
        );
        assert!(
            matches!(plan.init_args, InitArgsPlan::None),
            "implicit constructor wrapper should not decode init args"
        );

        let root = contract_init_root(&db, contract).expect("init root");
        let RuntimeInstanceSource::Synthetic(synthetic) = root.key(&db).source(&db) else {
            panic!("init root should be synthetic");
        };
        let RuntimeSyntheticSpec::ContractInitRoot {
            init_abi: root_init_abi,
            ..
        } = synthetic.spec(&db)
        else {
            panic!("expected synthetic contract init root");
        };
        assert_eq!(
            root_init_abi, init_abi,
            "contract init root should always call the synthesized init abi wrapper"
        );
    }
}
