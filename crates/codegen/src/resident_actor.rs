//! Target-neutral compilation of one scalar resident Fe actor transition.
//!
//! The compiler recognizes only the nominal `#[actor_transition(resident)]`
//! role and closed scalar value shapes. It does not know component, DOM,
//! surface, event-name, or application semantics.

use std::fmt;

use compiler_db::DriverDataBase;
use hir::analysis::{
    semantic::instantiate_with_generic_args,
    ty::{adt_def::AdtRef, normalize::normalize_ty, ty_check::BodyOwner, ty_def::TyId},
};
use hir::hir_def::{ActorTransition, TopLevelMod};

use crate::actor_semantics::{SemanticActor, nominal_attrs, resolve_metadata_ty, semantic_actors};
use crate::{
    CanonicalExecution, CanonicalField, CanonicalInterfaceManifest, CanonicalPlacement,
    CanonicalType, LowerError, WasmCompileOptions, WasmTaskAdapter,
    canonical_interface::{canonical_lane_decl_from_func, canonical_lane_intent},
    canonical_type_from_semantic, compile_runtime_package_wasm_with_options,
    materialized_task_adapters, verify_canonical_wasm_abi,
};

pub const RESIDENT_ACTOR_TRANSITION_EXPORT: &str = "fe_actor_transition_v1";
pub const RESIDENT_ACTOR_STATE_REPLACE_EXPORT: &str = "fe_actor_state_replace_v1";
pub const RESIDENT_ACTOR_INITIALIZE_EXPORT: &str = "fe_actor_initialize_v1";
pub const RESIDENT_ACTOR_PROJECT_EXPORT: &str = "fe_actor_project_v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentActorContract {
    pub actor: String,
    pub init_source_entry: String,
    pub projection_source_entry: String,
    pub source_entry: String,
    pub event: CanonicalType,
    pub state: CanonicalType,
    pub event_leaf_count: usize,
    pub state_leaf_count: usize,
    pub projection: CanonicalType,
    pub projection_leaf_count: usize,
    pub event_tag_limits: Vec<(usize, u32)>,
    pub state_tag_limits: Vec<(usize, u32)>,
    /// Source identities of role-selected scoped tasks. These stay inside the
    /// compiler; the browser receives only generated fixed task adapters.
    pub scoped_task_source_entries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentActorArtifact {
    pub contract: ResidentActorContract,
    pub wasm: Vec<u8>,
    pub scoped_tasks: Vec<WasmTaskAdapter>,
    /// Separately compiled canonical actors selected by the nominal child
    /// types consumed by this resident actor's Fe supervision tasks. The
    /// browser receives no source function name, route selector, or authored
    /// child identifier. Build tooling publishes these typed artifacts beside
    /// the parent continuation package.
    pub structured_children: Vec<StructuredChildActorArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredChildActorArtifact {
    pub actor: String,
    pub wasm: Vec<u8>,
    pub interface: CanonicalInterfaceManifest,
    pub scope: StructuredChildScopeImports,
}

/// Compiler-derived host import identities for one nominal child type.
///
/// `key` is an opaque package identity derived from the Fe semantic type. It
/// is never authored, serialized as application data, or interpreted by the
/// fixed browser runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredChildScopeImports {
    pub key: String,
    pub spawn: String,
    pub failure: String,
    pub close: String,
}

#[derive(Debug)]
pub enum ResidentActorError {
    Contract(String),
    Lower(LowerError),
}

impl fmt::Display for ResidentActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(message) => write!(f, "resident actor contract: {message}"),
            Self::Lower(error) => write!(f, "resident actor lowering: {error}"),
        }
    }
}

impl std::error::Error for ResidentActorError {}

fn behavior_is_resident(db: &DriverDataBase, behavior: hir::hir_def::Func<'_>) -> bool {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.actor_transition(db) == Some(ActorTransition::Resident))
}

fn behavior_is_initializer(db: &DriverDataBase, behavior: hir::hir_def::Func<'_>) -> bool {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.is_actor_initializer(db))
}

fn behavior_is_projection(db: &DriverDataBase, behavior: hir::hir_def::Func<'_>) -> bool {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.is_actor_projection(db))
}

pub(crate) fn behavior_is_scoped_task(
    db: &DriverDataBase,
    behavior: hir::hir_def::Func<'_>,
) -> bool {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.is_actor_scoped_task(db))
}

#[derive(Debug, Clone)]
struct WorkerScopeChild<'db> {
    ty: TyId<'db>,
    scope: StructuredChildScopeImports,
    has_spawn: bool,
    has_failure: bool,
    has_close: bool,
}

fn worker_scope_children<'db>(
    db: &'db DriverDataBase,
    package: mir::RuntimePackage<'db>,
) -> Result<Vec<WorkerScopeChild<'db>>, ResidentActorError> {
    let mut children = Vec::<WorkerScopeChild<'db>>::new();
    for function in package.functions(db) {
        let instance = function.instance(db);
        if mir::host_import_module(db, instance).as_deref() != Some("fe:worker-scope") {
            continue;
        }
        let kind = mir::runtime_actor_effect_kind(db, instance).ok_or_else(|| {
            ResidentActorError::Contract(
                "structured-child host import is not a nominal standard-library operation"
                    .to_owned(),
            )
        })?;
        if !matches!(
            kind,
            mir::RuntimeActorEffectFuncKind::ChildSpawnBegin
                | mir::RuntimeActorEffectFuncKind::ChildFailureBegin
                | mir::RuntimeActorEffectFuncKind::ChildClose
        ) {
            return Err(ResidentActorError::Contract(
                "non-scope actor operation resolved in the structured-child host namespace"
                    .to_owned(),
            ));
        }
        let semantic = instance.key(db).semantic(db).ok_or_else(|| {
            ResidentActorError::Contract(
                "structured-child operation is not a semantic Fe import".to_owned(),
            )
        })?;
        let args = semantic.key(db).subst(db).generic_args(db);
        let [child] = args.as_slice() else {
            return Err(ResidentActorError::Contract(format!(
                "structured-child operation must retain exactly one nominal child type; found {} generic arguments",
                args.len()
            )));
        };
        let spawn = mir::actor_scope_import_name(
            db,
            mir::RuntimeActorEffectFuncKind::ChildSpawnBegin,
            *child,
        )
        .ok_or_else(|| {
            ResidentActorError::Contract(
                "compiler rejected the nominal child spawn operation".to_owned(),
            )
        })?;
        let failure = mir::actor_scope_import_name(
            db,
            mir::RuntimeActorEffectFuncKind::ChildFailureBegin,
            *child,
        )
        .ok_or_else(|| {
            ResidentActorError::Contract(
                "compiler rejected the nominal child failure operation".to_owned(),
            )
        })?;
        let close =
            mir::actor_scope_import_name(db, mir::RuntimeActorEffectFuncKind::ChildClose, *child)
                .ok_or_else(|| {
                ResidentActorError::Contract(
                    "compiler rejected the nominal child close operation".to_owned(),
                )
            })?;
        let key = spawn
            .strip_prefix("spawn_")
            .ok_or_else(|| {
                ResidentActorError::Contract(
                    "compiler-derived structured-child spawn identity is malformed".to_owned(),
                )
            })?
            .to_owned();
        let actual = mir::host_import_name(db, instance).ok_or_else(|| {
            ResidentActorError::Contract(
                "structured-child host import has no compiler-derived operation name".to_owned(),
            )
        })?;
        let expected = match kind {
            mir::RuntimeActorEffectFuncKind::ChildSpawnBegin => &spawn,
            mir::RuntimeActorEffectFuncKind::ChildFailureBegin => &failure,
            mir::RuntimeActorEffectFuncKind::ChildClose => &close,
            mir::RuntimeActorEffectFuncKind::SendBegin
            | mir::RuntimeActorEffectFuncKind::AskBegin => unreachable!(),
        };
        if actual != *expected {
            return Err(ResidentActorError::Contract(format!(
                "structured-child import `{actual}` differs from its semantic identity `{expected}`"
            )));
        }
        let index = children
            .iter()
            .position(|candidate| candidate.ty == *child)
            .unwrap_or_else(|| {
                children.push(WorkerScopeChild {
                    ty: *child,
                    scope: StructuredChildScopeImports {
                        key,
                        spawn,
                        failure,
                        close,
                    },
                    has_spawn: false,
                    has_failure: false,
                    has_close: false,
                });
                children.len() - 1
            });
        let child = &mut children[index];
        match kind {
            mir::RuntimeActorEffectFuncKind::ChildSpawnBegin => child.has_spawn = true,
            mir::RuntimeActorEffectFuncKind::ChildFailureBegin => child.has_failure = true,
            mir::RuntimeActorEffectFuncKind::ChildClose => child.has_close = true,
            mir::RuntimeActorEffectFuncKind::SendBegin
            | mir::RuntimeActorEffectFuncKind::AskBegin => unreachable!(),
        }
    }
    for child in &children {
        if !child.has_spawn || !child.has_failure || !child.has_close {
            return Err(ResidentActorError::Contract(format!(
                "structured-child scope `{}` must import matching typed spawn, failure, and close operations",
                child.ty.pretty_print(db),
            )));
        }
    }
    children.sort_by(|left, right| left.scope.key.cmp(&right.scope.key));
    let mut imports = std::collections::BTreeSet::new();
    for child in &children {
        for import in [&child.scope.spawn, &child.scope.failure, &child.scope.close] {
            if !imports.insert(import.clone()) {
                return Err(ResidentActorError::Contract(format!(
                    "compiler-derived structured-child import collision at `{import}`"
                )));
            }
        }
    }
    Ok(children)
}

fn actor_for_nominal_ty<'a, 'db>(
    db: &'db DriverDataBase,
    actors: &'a [SemanticActor<'db>],
    mut ty: TyId<'db>,
) -> Option<&'a SemanticActor<'db>> {
    while let Some(inner) = ty.as_view(db) {
        ty = inner;
    }
    let AdtRef::Struct(state) = ty.adt_def(db)?.adt_ref(db) else {
        return None;
    };
    actors.iter().find(|actor| actor.state == state)
}

fn canonical_mailbox_scalar_value(ty: &CanonicalType) -> bool {
    match ty {
        CanonicalType::Bool
        | CanonicalType::U8
        | CanonicalType::I32
        | CanonicalType::U32
        | CanonicalType::I64
        | CanonicalType::U64
        | CanonicalType::F32 => true,
        CanonicalType::Record(fields) => fields
            .iter()
            .all(|field| canonical_mailbox_scalar_value(&field.ty)),
        CanonicalType::Variant(variants) => variants.iter().all(|variant| {
            variant
                .fields
                .iter()
                .all(|field| canonical_mailbox_scalar_value(&field.ty))
        }),
        CanonicalType::Bytes | CanonicalType::String | CanonicalType::List { .. } => false,
    }
}

/// Prove that each parent mailbox import denotes one actual behavior on the
/// nominal supervised child. The public `Handles` bound is useful authoring
/// evidence, but this compiler check deliberately does not trust a manually
/// written impl of that trait to invent a Worker endpoint.
fn validate_actor_mailbox_requests(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    package: mir::RuntimePackage<'_>,
) -> Result<(), ResidentActorError> {
    let actors = semantic_actors(db, top_mod);
    let supervised_children = worker_scope_children(db, package)?;
    for function in package.functions(db) {
        let instance = function.instance(db);
        if mir::runtime_actor_effect_kind(db, instance)
            != Some(mir::RuntimeActorEffectFuncKind::AskBegin)
        {
            continue;
        }
        if supervised_children.is_empty() {
            return Err(ResidentActorError::Contract(
                "typed child mailbox requires an owning structured-child scope".to_owned(),
            ));
        }
        if mir::host_import_module(db, instance).as_deref() != Some("fe:worker-mailbox") {
            return Err(ResidentActorError::Contract(
                "typed child mailbox resolved outside its nominal host namespace".to_owned(),
            ));
        }
        let semantic = instance.key(db).semantic(db).ok_or_else(|| {
            ResidentActorError::Contract(
                "typed child mailbox operation is not a semantic Fe import".to_owned(),
            )
        })?;
        let args = semantic.key(db).subst(db).generic_args(db);
        let [child, request, response] = args.as_slice() else {
            return Err(ResidentActorError::Contract(format!(
                "typed child mailbox must retain child, request, and response types; found {} generic arguments",
                args.len(),
            )));
        };
        let child = child.as_view(db).unwrap_or(*child);
        let expected_child = supervised_children.iter().find(|candidate| {
            let candidate = candidate.ty.as_view(db).unwrap_or(candidate.ty);
            child == candidate
        });
        if expected_child.is_none() {
            return Err(ResidentActorError::Contract(format!(
                "typed mailbox child `{}` has no matching structured-child scope",
                child.pretty_print(db),
            )));
        }
        let request = request.as_view(db).unwrap_or(*request);
        let response = response.as_view(db).unwrap_or(*response);
        let actor = actor_for_nominal_ty(db, &actors, child).ok_or_else(|| {
            ResidentActorError::Contract(format!(
                "typed mailbox child `{}` is not an actor in this module",
                child.pretty_print(db),
            ))
        })?;
        let matches = actor
            .behaviors
            .iter()
            .copied()
            .filter(|behavior| {
                let Ok(intent) = canonical_lane_intent(db, *behavior) else {
                    return false;
                };
                if intent.execution != CanonicalExecution::Wasm
                    || intent.placement != CanonicalPlacement::Worker
                {
                    return false;
                }
                let candidate_args = behavior.arg_tys(db);
                let [candidate] = candidate_args.as_slice() else {
                    return false;
                };
                let candidate = candidate
                    .skip_binder()
                    .as_view(db)
                    .unwrap_or(*candidate.skip_binder());
                let returned = behavior
                    .return_ty(db)
                    .as_view(db)
                    .unwrap_or(behavior.return_ty(db));
                candidate == request && returned == response
            })
            .collect::<Vec<_>>();
        let [behavior] = matches.as_slice() else {
            return Err(ResidentActorError::Contract(format!(
                "typed mailbox edge `{} -> {}` selects {} Worker behaviors on `{}`; exactly one is required",
                request.pretty_print(db),
                response.pretty_print(db),
                matches.len(),
                child.pretty_print(db),
            )));
        };
        let request_value = canonical_type_from_semantic(db, request, "worker_mailbox_request")
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        let response_value = canonical_type_from_semantic(db, response, "worker_mailbox_response")
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        if !canonical_mailbox_scalar_value(&request_value)
            || !canonical_mailbox_scalar_value(&response_value)
        {
            return Err(ResidentActorError::Contract(
                "typed child mailbox currently requires owned scalar value trees; bytes, strings, and lists need the canonical post-return memory bridge"
                    .to_owned(),
            ));
        }
        let import = mir::host_import_name(db, instance).ok_or_else(|| {
            ResidentActorError::Contract("typed child mailbox has no generated import".to_owned())
        })?;
        let expected_import = mir::actor_mailbox_import_name(db, child, request, response);
        if import != expected_import {
            return Err(ResidentActorError::Contract(format!(
                "typed child mailbox import `{import}` differs from its semantic edge `{expected_import}`"
            )));
        }
        let behavior_name = behavior
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_owned())
            .unwrap_or_else(|| "<unnamed>".to_owned());
        let declaration = canonical_lane_decl_from_func(db, *behavior, &behavior_name, &import)
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        if declaration.request != request_value || declaration.response != response_value {
            return Err(ResidentActorError::Contract(
                "typed child mailbox edge differs from its canonical Worker declaration".to_owned(),
            ));
        }
    }
    Ok(())
}

fn wasm_imports(wasm: &[u8]) -> Result<Vec<String>, ResidentActorError> {
    let mut imports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ImportSection(section) = payload.map_err(|error| {
            ResidentActorError::Contract(format!(
                "structured-child Wasm could not be inspected: {error}"
            ))
        })? {
            for import in section.into_imports() {
                let import = import.map_err(|error| {
                    ResidentActorError::Contract(format!(
                        "structured-child Wasm import could not be inspected: {error}"
                    ))
                })?;
                imports.push(format!("{}::{}", import.module, import.name));
            }
        }
    }
    Ok(imports)
}

fn compile_structured_children(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    parent_package: mir::RuntimePackage<'_>,
    optimize: bool,
) -> Result<Vec<StructuredChildActorArtifact>, ResidentActorError> {
    let children = worker_scope_children(db, parent_package)?;
    let actors = semantic_actors(db, top_mod);
    let mut artifacts = Vec::with_capacity(children.len());
    for child in children {
        let child_ty = child.ty;
        let actor = actor_for_nominal_ty(db, &actors, child_ty).ok_or_else(|| {
            ResidentActorError::Contract(format!(
                "structured-child type `{}` is not the state type of an actor in this module",
                child_ty.pretty_print(db)
            ))
        })?;
        let actor_name = actor
            .state
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_owned())
            .ok_or_else(|| {
                ResidentActorError::Contract(
                    "structured child actor has no resolvable name".to_owned(),
                )
            })?;
        let mut declarations = Vec::new();
        let mut entries = Vec::new();
        for behavior in &actor.behaviors {
            let name = behavior
                .name(db)
                .to_opt()
                .map(|name| name.data(db).to_owned())
                .ok_or_else(|| {
                    ResidentActorError::Contract(format!(
                        "structured child actor `{actor_name}` has an unnamed behavior"
                    ))
                })?;
            let intent = canonical_lane_intent(db, *behavior)
                .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
            if intent.placement != CanonicalPlacement::Worker {
                continue;
            }
            if intent.execution != CanonicalExecution::Wasm {
                return Err(ResidentActorError::Contract(format!(
                    "structured child actor `{actor_name}` behavior `{name}` must execute as Wasm"
                )));
            }
            let behavior_args = behavior.arg_tys(db);
            let [request] = behavior_args.as_slice() else {
                return Err(ResidentActorError::Contract(format!(
                    "structured child actor `{actor_name}` behavior `{name}` must take exactly one request"
                )));
            };
            let request = request
                .skip_binder()
                .as_view(db)
                .unwrap_or(*request.skip_binder());
            let response = behavior
                .return_ty(db)
                .as_view(db)
                .unwrap_or(behavior.return_ty(db));
            let lane = mir::actor_mailbox_import_name(db, child_ty, request, response);
            let mut declaration = canonical_lane_decl_from_func(db, *behavior, &name, &lane)
                .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
            declaration.export = Some(format!("fe_cabi_{lane}"));
            entries.push(name);
            declarations.push(declaration);
        }
        if declarations.is_empty() {
            return Err(ResidentActorError::Contract(format!(
                "structured child actor `{actor_name}` has no canonical `Worker` behavior"
            )));
        }
        let interface = CanonicalInterfaceManifest::build(declarations)
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        let package = mir::build_wasm_runtime_package_for_entries(db, top_mod, &entries)
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        let mut options =
            WasmCompileOptions::default().with_canonical_lanes(interface.lanes.clone());
        for (source, lane) in entries.iter().zip(&interface.lanes) {
            options = options.with_export_alias(source, &lane.name);
        }
        let options = if optimize {
            options.with_optimization()
        } else {
            options
        };
        let wasm = compile_runtime_package_wasm_with_options(db, &package, options)
            .map_err(ResidentActorError::Lower)?
            .bytes;
        verify_canonical_wasm_abi(&wasm, &interface)
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        let imports = wasm_imports(&wasm)?;
        if !imports.is_empty() {
            return Err(ResidentActorError::Contract(format!(
                "structured child actor `{actor_name}` is not closed over browser capabilities; generated Worker host cannot satisfy imports {}",
                imports.join(", ")
            )));
        }
        artifacts.push(StructuredChildActorArtifact {
            actor: actor_name,
            wasm,
            interface,
            scope: child.scope,
        });
    }
    Ok(artifacts)
}

/// Validate and materialize the target-neutral runtime support shared by
/// resident and render actors. The caller owns actor-role selection and the
/// parent root set; this helper proves that the resulting package's typed
/// mailbox edges correspond to actual nominal child actors.
pub(crate) fn compile_scoped_task_support(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    parent_package: mir::RuntimePackage<'_>,
    expected_tasks: usize,
    optimize: bool,
) -> Result<(Vec<WasmTaskAdapter>, Vec<StructuredChildActorArtifact>), ResidentActorError> {
    validate_actor_mailbox_requests(db, top_mod, parent_package)?;
    let scoped_tasks =
        materialized_task_adapters(db, &parent_package).map_err(ResidentActorError::Lower)?;
    if scoped_tasks.len() != expected_tasks {
        return Err(ResidentActorError::Contract(format!(
            "actor declares {expected_tasks} scoped task behavior(s), but {} resumable machine(s) were materialized",
            scoped_tasks.len(),
        )));
    }
    let structured_children = compile_structured_children(db, top_mod, parent_package, optimize)?;
    Ok((scoped_tasks, structured_children))
}

fn scalar_layout(
    ty: &CanonicalType,
    path: &str,
    offset: usize,
    tag_limits: &mut Vec<(usize, u32)>,
) -> Result<usize, ResidentActorError> {
    match ty {
        CanonicalType::Bool
        | CanonicalType::U8
        | CanonicalType::I32
        | CanonicalType::U32
        | CanonicalType::I64
        | CanonicalType::U64
        | CanonicalType::F32 => Ok(1),
        CanonicalType::Record(fields) => {
            let mut count = 0usize;
            for field in fields {
                let field_offset = offset.checked_add(count).ok_or_else(|| {
                    ResidentActorError::Contract(format!("scalar leaf offset overflow at `{path}`"))
                })?;
                count = count
                    .checked_add(scalar_layout(
                        &field.ty,
                        &format!("{path}.{}", field.name),
                        field_offset,
                        tag_limits,
                    )?)
                    .ok_or_else(|| {
                        ResidentActorError::Contract(format!(
                            "scalar leaf count overflow at `{path}`"
                        ))
                    })?;
            }
            Ok(count)
        }
        CanonicalType::Variant(variants)
            if variants.iter().all(|variant| variant.fields.is_empty()) =>
        {
            let count = u32::try_from(variants.len()).map_err(|_| {
                ResidentActorError::Contract(format!(
                    "`{path}` has too many fieldless variants for the Wasm boundary"
                ))
            })?;
            if count == 0 {
                return Err(ResidentActorError::Contract(format!(
                    "`{path}` is an empty enum"
                )));
            }
            tag_limits.push((offset, count));
            Ok(1)
        }
        CanonicalType::Variant(_) => Err(ResidentActorError::Contract(format!(
            "`{path}` is a payload enum; the scalar resident ABI admits only fieldless enums"
        ))),
        // Canonical borrowed values cross the resident boundary as their
        // wasm32 `(pointer, length)` descriptor. The fixed component adapter
        // owns request allocation and copies standards strings into Wasm;
        // authored Fe owns any retention. Returned/state descriptors continue
        // to point at Fe-owned resident memory. Payload bytes are therefore
        // deliberately not mistaken for scalar state leaves.
        CanonicalType::Bytes | CanonicalType::String | CanonicalType::List { .. } => Ok(2),
    }
}

/// Validate every canonical actor-sink operation in the compiled package
/// against the unique resident event. The fixed browser handler forwards the
/// import's already-flattened scalar parameters directly to the fixed resident
/// transition export, so accepting a merely same-width or caller-described
/// layout here would turn JavaScript into an implicit event codec.
fn validate_actor_sink_events(
    db: &DriverDataBase,
    package: &mir::RuntimePackage<'_>,
    contract: &ResidentActorContract,
) -> Result<(), ResidentActorError> {
    let resident_functions = package
        .functions(db)
        .into_iter()
        .filter(|function| {
            function.linkage(db) == mir::RuntimeLinkage::Internal
                && function.symbol(db).as_str() == contract.source_entry.as_str()
        })
        .collect::<Vec<_>>();
    let [resident] = resident_functions.as_slice() else {
        return Err(ResidentActorError::Contract(format!(
            "actor `{}` resident transition `{}` resolved to {} runtime roots",
            contract.actor,
            contract.source_entry,
            resident_functions.len(),
        )));
    };
    let resident_body = resident.instance(db).body(db);
    let Some(event) = resident_body.signature.params.first() else {
        return Err(ResidentActorError::Contract(format!(
            "actor `{}` resident transition has no runtime event parameter",
            contract.actor,
        )));
    };
    let mir::instance::RuntimeInstanceSource::Semantic(resident_semantic) =
        resident.instance(db).key(db).source(db)
    else {
        return Err(ResidentActorError::Contract(format!(
            "actor `{}` resident transition is not a semantic Fe function",
            contract.actor,
        )));
    };
    let BodyOwner::Func(resident_func) = resident_semantic.key(db).owner(db) else {
        return Err(ResidentActorError::Contract(format!(
            "actor `{}` resident transition is not a Fe function",
            contract.actor,
        )));
    };
    // Read the authored semantic argument rather than the runtime local's
    // lowering carrier. The latter may be a representation-preserving `View`
    // wrapper even though the public actor boundary consumes the same nominal
    // value type.
    let resident_event_ty = resident_func
        .arg_tys(db)
        .first()
        .map(|ty| *ty.skip_binder())
        .ok_or_else(|| {
            ResidentActorError::Contract(format!(
                "actor `{}` resident transition has no semantic event type",
                contract.actor,
            ))
        })?;
    let resident_event_ty = instantiate_with_generic_args(
        db,
        resident_event_ty,
        resident_semantic.key(db).subst(db).generic_args(db),
    );
    let mut resident_event_ty = normalize_ty(
        db,
        resident_event_ty,
        resident_func.scope(),
        resident_semantic.assumptions(db),
    );
    while let Some(inner) = resident_event_ty.as_view(db) {
        resident_event_ty = inner;
    }

    for function in package.functions(db) {
        let instance = function.instance(db);
        if mir::runtime_actor_effect_kind(db, instance)
            != Some(mir::RuntimeActorEffectFuncKind::SendBegin)
        {
            continue;
        }
        let body = instance.body(db);
        let [sent] = body.signature.params.as_slice() else {
            return Err(ResidentActorError::Contract(format!(
                "actor `{}` typed sink must take exactly one event value; found {} parameters",
                contract.actor,
                body.signature.params.len(),
            )));
        };
        let mir::instance::RuntimeInstanceSource::Semantic(sink_semantic) =
            instance.key(db).source(db)
        else {
            return Err(ResidentActorError::Contract(format!(
                "actor `{}` typed sink is not a semantic Fe function",
                contract.actor,
            )));
        };
        let BodyOwner::Func(sink_func) = sink_semantic.key(db).owner(db) else {
            return Err(ResidentActorError::Contract(format!(
                "actor `{}` typed sink is not a Fe function",
                contract.actor,
            )));
        };
        let sink_event_ty = sink_func
            .arg_tys(db)
            .first()
            .map(|ty| *ty.skip_binder())
            .ok_or_else(|| {
                ResidentActorError::Contract(format!(
                    "actor `{}` typed sink declaration has no event type",
                    contract.actor,
                ))
            })?;
        let sink_event_ty = instantiate_with_generic_args(
            db,
            sink_event_ty,
            sink_semantic.key(db).subst(db).generic_args(db),
        );
        let mut sink_event_ty = normalize_ty(
            db,
            sink_event_ty,
            sink_func.scope(),
            sink_semantic.assumptions(db),
        );
        while let Some(inner) = sink_event_ty.as_view(db) {
            sink_event_ty = inner;
        }
        if sink_event_ty != resident_event_ty {
            return Err(ResidentActorError::Contract(format!(
                "actor `{}` typed sink event differs from its resident transition: sent `{}`, resident `{}`",
                contract.actor,
                sink_event_ty.pretty_print(db),
                resident_event_ty.pretty_print(db),
            )));
        }
        if sent.class != event.class {
            return Err(ResidentActorError::Contract(format!(
                "actor `{}` typed sink event differs from its resident transition: sent {:?}, resident {:?}",
                contract.actor, sent.class, event.class,
            )));
        }
    }
    Ok(())
}

/// Recover and validate the module's unique resident actor behavior.
/// `Ok(None)` means no actor selected this execution role. A selected role
/// fails closed on ambiguity, non-record events, partial/reordered state, or
/// rich values outside the canonical borrowed descriptor shapes.
pub fn resident_actor_contract(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<Option<ResidentActorContract>, ResidentActorError> {
    let actors = semantic_actors(db, top_mod);
    let selected = actors
        .iter()
        .flat_map(|actor| {
            actor
                .behaviors
                .iter()
                .copied()
                .filter(|behavior| behavior_is_resident(db, *behavior))
                .map(move |behavior| (actor, behavior))
        })
        .collect::<Vec<_>>();
    let (actor, behavior) = match selected.as_slice() {
        [] => return Ok(None),
        [selected] => *selected,
        _ => {
            return Err(ResidentActorError::Contract(format!(
                "module declares {} resident transition behaviors; exactly one is required",
                selected.len()
            )));
        }
    };

    let actor_name = actor
        .state
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| ResidentActorError::Contract("actor has no resolvable name".to_owned()))?;
    let source_entry = behavior
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| {
            ResidentActorError::Contract("resident behavior has no resolvable name".to_owned())
        })?;
    let declaration = hir::lower::module_actor_decls(db, top_mod)
        .into_iter()
        .find(|declaration| declaration.name == actor_name)
        .ok_or_else(|| {
            ResidentActorError::Contract(format!(
                "actor `{actor_name}` has no structural declaration"
            ))
        })?;
    let arg_tys = behavior.arg_tys(db);
    if arg_tys.len() != declaration.fields.len() + 1 {
        return Err(ResidentActorError::Contract(format!(
            "behavior `{source_entry}` must take exactly one event before {} actor fields; found {} semantic arguments",
            declaration.fields.len(),
            arg_tys.len()
        )));
    }

    let event = canonical_type_from_semantic(db, *arg_tys[0].skip_binder(), "actor_event")
        .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
    if !matches!(event, CanonicalType::Record(_)) {
        return Err(ResidentActorError::Contract(format!(
            "behavior `{source_entry}` event must be one named record, got {event:?}"
        )));
    }
    let mut event_tag_limits = Vec::new();
    let event_leaf_count = scalar_layout(&event, "actor_event", 0, &mut event_tag_limits)?;

    let mut state_fields = Vec::with_capacity(declaration.fields.len());
    for (field, ty) in declaration.fields.iter().zip(&arg_tys[1..]) {
        let ty = canonical_type_from_semantic(
            db,
            *ty.skip_binder(),
            &format!("actor_state.{}", field.name),
        )
        .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        state_fields.push(CanonicalField::new(field.name.clone(), ty));
    }
    let state = CanonicalType::Record(state_fields);
    let returned = canonical_type_from_semantic(db, behavior.return_ty(db), "actor_state_response")
        .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
    if returned != state {
        return Err(ResidentActorError::Contract(format!(
            "behavior `{source_entry}` must return the actor's complete state in declaration order: expected {state:?}, got {returned:?}"
        )));
    }
    let mut state_tag_limits = Vec::new();
    let state_leaf_count = scalar_layout(&state, "actor_state", 0, &mut state_tag_limits)?;

    let initializers = actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| behavior_is_initializer(db, *behavior))
        .collect::<Vec<_>>();
    let [initializer] = initializers.as_slice() else {
        return Err(ResidentActorError::Contract(format!(
            "actor `{actor_name}` declares {} complete-state initializers; exactly one is required",
            initializers.len()
        )));
    };
    let init_source_entry = initializer
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| {
            ResidentActorError::Contract("actor initializer has no resolvable name".to_owned())
        })?;
    if !initializer.arg_tys(db).is_empty() {
        return Err(ResidentActorError::Contract(format!(
            "initializer `{init_source_entry}` must be self-less and take no arguments"
        )));
    }
    let initialized_state =
        canonical_type_from_semantic(db, initializer.return_ty(db), "initial_actor_state")
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
    if initialized_state != state {
        return Err(ResidentActorError::Contract(format!(
            "initializer `{init_source_entry}` must return complete actor state: expected {state:?}, got {initialized_state:?}"
        )));
    }

    let projections = actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| behavior_is_projection(db, *behavior))
        .collect::<Vec<_>>();
    let [projection_behavior] = projections.as_slice() else {
        return Err(ResidentActorError::Contract(format!(
            "actor `{actor_name}` declares {} state projections; exactly one is required",
            projections.len()
        )));
    };
    let projection_source_entry = projection_behavior
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| {
            ResidentActorError::Contract("actor projection has no resolvable name".to_owned())
        })?;
    let projection_args = projection_behavior.arg_tys(db);
    if projection_args.len() != declaration.fields.len() {
        return Err(ResidentActorError::Contract(format!(
            "projection `{projection_source_entry}` must take self and therefore exactly {} flattened state arguments; found {}",
            declaration.fields.len(),
            projection_args.len()
        )));
    }
    for (index, (expected, actual)) in arg_tys[1..].iter().zip(&projection_args).enumerate() {
        let expected = canonical_type_from_semantic(
            db,
            *expected.skip_binder(),
            &format!("actor_state.{}", declaration.fields[index].name),
        )
        .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        let actual = canonical_type_from_semantic(
            db,
            *actual.skip_binder(),
            &format!("projection_state.{}", declaration.fields[index].name),
        )
        .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
        if actual != expected {
            return Err(ResidentActorError::Contract(format!(
                "projection `{projection_source_entry}` state argument {index} differs from the actor field: expected {expected:?}, got {actual:?}"
            )));
        }
    }
    let projection =
        canonical_type_from_semantic(db, projection_behavior.return_ty(db), "actor_projection")
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
    if !matches!(projection, CanonicalType::Record(_)) {
        return Err(ResidentActorError::Contract(format!(
            "projection `{projection_source_entry}` must return one named record, got {projection:?}"
        )));
    }
    let mut projection_tag_limits = Vec::new();
    let projection_leaf_count = scalar_layout(
        &projection,
        "actor_projection",
        0,
        &mut projection_tag_limits,
    )?;

    let mut scoped_task_source_entries = Vec::new();
    for task in actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| behavior_is_scoped_task(db, *behavior))
    {
        let name = task
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())
            .ok_or_else(|| {
                ResidentActorError::Contract("scoped task has no resolvable name".to_owned())
            })?;
        let task_args = task.arg_tys(db);
        if !task_args.is_empty() && task_args.len() != declaration.fields.len() {
            return Err(ResidentActorError::Contract(format!(
                "scoped task `{name}` must be self-less or take self as exactly {} flattened actor-state arguments; found {}",
                declaration.fields.len(),
                task_args.len(),
            )));
        }
        for (index, (expected, actual)) in arg_tys[1..].iter().zip(&task_args).enumerate() {
            let expected = canonical_type_from_semantic(
                db,
                *expected.skip_binder(),
                &format!("actor_state.{}", declaration.fields[index].name),
            )
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
            let actual = canonical_type_from_semantic(
                db,
                *actual.skip_binder(),
                &format!("scoped_task_state.{}", declaration.fields[index].name),
            )
            .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
            if actual != expected {
                return Err(ResidentActorError::Contract(format!(
                    "scoped task `{name}` state argument {index} differs from the actor field: expected {expected:?}, got {actual:?}"
                )));
            }
        }
        scoped_task_source_entries.push(name);
    }

    Ok(Some(ResidentActorContract {
        actor: actor_name,
        init_source_entry,
        projection_source_entry,
        source_entry,
        event,
        state,
        event_leaf_count,
        state_leaf_count,
        projection,
        projection_leaf_count,
        event_tag_limits,
        state_tag_limits,
        scoped_task_source_entries,
    }))
}

/// Compile the unique role-selected transition behind the fixed resident actor
/// ABI. No JSON or source-level behavior name is required by the eventual host.
pub fn compile_resident_actor(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<Option<ResidentActorArtifact>, ResidentActorError> {
    compile_resident_actor_with_optimization(db, top_mod, true)
}

/// Compile a resident actor while honoring the caller's Wasm optimization
/// policy. The convenience [`compile_resident_actor`] retains its historical
/// optimized default for direct codegen consumers; protocol facades use this
/// form so `OptimizationLevel::None` remains meaningful.
pub fn compile_resident_actor_with_optimization(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    optimize: bool,
) -> Result<Option<ResidentActorArtifact>, ResidentActorError> {
    let Some(contract) = resident_actor_contract(db, top_mod)? else {
        return Ok(None);
    };
    let mut entries = vec![
        contract.init_source_entry.clone(),
        contract.projection_source_entry.clone(),
        contract.source_entry.clone(),
    ];
    entries.extend(contract.scoped_task_source_entries.iter().cloned());
    let package = mir::build_wasm_runtime_package_for_entries(db, top_mod, &entries)
        .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
    validate_actor_sink_events(db, &package, &contract)?;
    let (scoped_tasks, structured_children) = compile_scoped_task_support(
        db,
        top_mod,
        package,
        contract.scoped_task_source_entries.len(),
        optimize,
    )?;
    // A scoped task can receive generated rich host values between Wasm
    // continuation entries. Use the checked LIFO canonical stack for every
    // scoped-task actor so those values have one compiler-owned allocation
    // lifetime. Generated transports may bind their operation-specific
    // post-return names to this common implementation.
    let options = if scoped_tasks.is_empty() {
        WasmCompileOptions::default()
    } else {
        WasmCompileOptions::default().with_canonical_stack_memory(["fe_cabi_post_return"])
    }
    .with_resident_actor_transition_checked(
        &contract.source_entry,
        RESIDENT_ACTOR_TRANSITION_EXPORT,
        RESIDENT_ACTOR_STATE_REPLACE_EXPORT,
        contract.event_leaf_count,
        vec![false; contract.state_leaf_count],
        contract.event_tag_limits.clone(),
        contract.state_tag_limits.clone(),
    )
    .with_resident_actor_initializer(
        &contract.init_source_entry,
        RESIDENT_ACTOR_INITIALIZE_EXPORT,
    )
    .with_resident_actor_projection(
        &contract.projection_source_entry,
        RESIDENT_ACTOR_PROJECT_EXPORT,
    );
    let options = if optimize {
        options.with_optimization()
    } else {
        options
    };
    let artifact = compile_runtime_package_wasm_with_options(db, &package, options)
        .map_err(ResidentActorError::Lower)?;
    wasmparser::validate(&artifact.bytes).map_err(|error| {
        ResidentActorError::Contract(format!("generated resident Wasm is invalid: {error}"))
    })?;
    Ok(Some(ResidentActorArtifact {
        contract,
        wasm: artifact.bytes,
        scoped_tasks,
        structured_children,
    }))
}
