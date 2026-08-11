//! Target-neutral compilation of one scalar resident Fe actor transition.
//!
//! The compiler recognizes only the nominal `#[actor_transition(resident)]`
//! role and closed scalar value shapes. It does not know component, DOM,
//! surface, event-name, or application semantics.

use std::fmt;

use compiler_db::DriverDataBase;
use hir::hir_def::{ActorTransition, TopLevelMod};

use crate::actor_semantics::{nominal_attrs, resolve_metadata_ty, semantic_actors};
use crate::{
    CanonicalField, CanonicalType, LowerError, WasmCompileOptions, canonical_type_from_semantic,
    compile_runtime_package_wasm_with_options,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentActorArtifact {
    pub contract: ResidentActorContract,
    pub wasm: Vec<u8>,
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
    let package = mir::build_wasm_runtime_package_for_entries(
        db,
        top_mod,
        &[
            contract.init_source_entry.clone(),
            contract.projection_source_entry.clone(),
            contract.source_entry.clone(),
        ],
    )
    .map_err(|error| ResidentActorError::Contract(error.to_string()))?;
    let options = WasmCompileOptions::default()
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
    }))
}
