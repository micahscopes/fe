//! Shared semantic recovery for Fe `actor` declarations.
//!
//! Actor syntax lowers to an ordinary state struct plus public behavior
//! functions. Consumers recover their common origin here, then classify roles
//! by nominal attributes. Keeping this target-neutral prevents WebGPU bundles,
//! resident Wasm actors, and future native hosts from growing separate actor
//! recognizers.

use compiler_db::DriverDataBase;
use hir::analysis::{
    name_resolution::{PathRes, resolve_path},
    ty::{
        adt_def::AdtRef, trait_def::TraitInstId, trait_resolution::PredicateListId, ty_def::TyId,
    },
};
use hir::hir_def::{AttrListId, GpuResource, ItemKind, PathId, Struct, TopLevelMod};
use hir::span::{ActorDesugaredFocus, DesugaredOrigin, HirOrigin};

#[derive(Debug)]
pub(crate) struct SemanticActor<'db> {
    pub(crate) state: Struct<'db>,
    pub(crate) behaviors: Vec<hir::hir_def::Func<'db>>,
}

/// Compiler-owned projection of one nominal GPU resource type.
///
/// Keep positional generic knowledge here rather than teaching every backend
/// that legacy storage is `<T, N>`, readback is `<T, N, M>`, and the typed
/// policy family is `<Kind, Access, Residency, Init, Recovery, Visibility, N,
/// T>`. Backends consume the semantic element/length pair; bundle construction
/// may additionally inspect the policy marker types.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SemanticGpuResource<'db> {
    pub(crate) kind: GpuResource,
    pub(crate) element_ty: TyId<'db>,
    pub(crate) length_ty: TyId<'db>,
    pub(crate) family: Option<SemanticGpuResourceFamily<'db>>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SemanticGpuResourceFamily<'db> {
    pub(crate) kind_ty: TyId<'db>,
    pub(crate) access_ty: TyId<'db>,
    pub(crate) residency_ty: TyId<'db>,
    pub(crate) init_ty: TyId<'db>,
    pub(crate) recovery_ty: TyId<'db>,
    pub(crate) visibility_ty: TyId<'db>,
}

/// Recover one GPU resource's semantic shape after aliases and views have been
/// normalized. `Ok(None)` means the type is not a GPU resource; malformed
/// attributed resources fail closed with a stable compiler-owned explanation.
pub(crate) fn semantic_gpu_resource<'db>(
    db: &'db dyn hir::analysis::HirAnalysisDb,
    ty: TyId<'db>,
) -> Result<Option<SemanticGpuResource<'db>>, &'static str> {
    let ty = ty.as_view(db).unwrap_or(ty);
    let Some(attrs) = nominal_attrs(db, ty) else {
        return Ok(None);
    };
    let Some(kind) = attrs.gpu_resource(db) else {
        return Ok(None);
    };
    let args = ty.generic_args(db);
    let resource = match kind {
        GpuResource::Storage => {
            let [element_ty, length_ty] = args else {
                return Err(
                    "GPU storage resource type requires exactly element and length arguments",
                );
            };
            SemanticGpuResource {
                kind,
                element_ty: *element_ty,
                length_ty: *length_ty,
                family: None,
            }
        }
        GpuResource::Readback => {
            let [element_ty, length_ty, _message_ty] = args else {
                return Err(
                    "GPU readback resource type requires exactly element, length, and message arguments",
                );
            };
            SemanticGpuResource {
                kind,
                element_ty: *element_ty,
                length_ty: *length_ty,
                family: None,
            }
        }
        GpuResource::StorageFamily => {
            let [
                kind_ty,
                access_ty,
                residency_ty,
                init_ty,
                recovery_ty,
                visibility_ty,
                length_ty,
                element_ty,
            ] = args
            else {
                return Err(
                    "GPU storage family requires kind, access, residency, initialization, recovery, visibility, length, and element arguments",
                );
            };
            SemanticGpuResource {
                kind,
                element_ty: *element_ty,
                length_ty: *length_ty,
                family: Some(SemanticGpuResourceFamily {
                    kind_ty: *kind_ty,
                    access_ty: *access_ty,
                    residency_ty: *residency_ty,
                    init_ty: *init_ty,
                    recovery_ty: *recovery_ty,
                    visibility_ty: *visibility_ty,
                }),
            }
        }
    };
    Ok(Some(resource))
}

pub(crate) fn semantic_actors<'db>(
    db: &'db DriverDataBase,
    top_mod: TopLevelMod<'db>,
) -> Vec<SemanticActor<'db>> {
    let items = top_mod.all_items(db);
    let mut actors = Vec::new();
    for item in items {
        let ItemKind::Struct(state) = item else {
            continue;
        };
        let HirOrigin::Desugared(DesugaredOrigin::Actor(state_origin)) = state.origin(db) else {
            continue;
        };
        if state_origin.focus != ActorDesugaredFocus::State {
            continue;
        }
        let behaviors = items
            .iter()
            .filter_map(|item| {
                let ItemKind::Func(func) = item else {
                    return None;
                };
                let HirOrigin::Desugared(DesugaredOrigin::Actor(origin)) = func.origin(db) else {
                    return None;
                };
                (origin.actor == state_origin.actor
                    && matches!(origin.focus, ActorDesugaredFocus::Behavior(_)))
                .then_some(*func)
            })
            .collect();
        actors.push(SemanticActor {
            state: *state,
            behaviors,
        });
    }
    actors
}

pub(crate) fn resolve_metadata_ty<'db>(
    db: &'db dyn hir::analysis::HirAnalysisDb,
    path: PathId<'db>,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
) -> Option<TyId<'db>> {
    for candidate in [scope, scope.top_mod(db).scope()] {
        for candidate_path in [path, path.strip_generic_args(db)] {
            match resolve_path(
                db,
                candidate_path,
                candidate,
                PredicateListId::empty_list(db),
                true,
            )
            .ok()
            {
                Some(PathRes::Ty(ty) | PathRes::TyAlias(_, ty)) => return Some(ty),
                _ => {}
            }
        }
    }
    None
}

/// Resolve the attributes carried by one nominal actor role.
///
/// Most GPU roles are structs, while target-placement roles such as `Worker`
/// are traits. Consumers care about the nominal metadata rather than that
/// syntactic distinction, so keep the resolution rule in one place.
pub(crate) fn resolve_metadata_attrs<'db>(
    db: &'db dyn hir::analysis::HirAnalysisDb,
    path: PathId<'db>,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
) -> Option<AttrListId<'db>> {
    for candidate in [scope, scope.top_mod(db).scope()] {
        for candidate_path in [path, path.strip_generic_args(db)] {
            let Ok(resolved) = resolve_path(
                db,
                candidate_path,
                candidate,
                PredicateListId::empty_list(db),
                true,
            ) else {
                continue;
            };
            match resolved {
                PathRes::Trait(trait_) => return trait_.def(db).scope().attrs(db),
                PathRes::Ty(ty) | PathRes::TyAlias(_, ty) => return nominal_attrs(db, ty),
                _ => {}
            }
        }
    }
    None
}

/// Resolve a nominal actor role as a trait instance without discarding the
/// source-supplied generic arguments.
///
/// Placement markers need only their declaration attributes, but capability
/// roles such as `Dispatch<WebGpuBackend>` also use the instantiated backend
/// type to prove that the capability and backend carry the same identity.
pub(crate) fn resolve_metadata_trait_inst<'db>(
    db: &'db dyn hir::analysis::HirAnalysisDb,
    path: PathId<'db>,
    scope: hir::hir_def::scope_graph::ScopeId<'db>,
) -> Option<TraitInstId<'db>> {
    for candidate in [scope, scope.top_mod(db).scope()] {
        for candidate_path in [path, path.strip_generic_args(db)] {
            let Ok(PathRes::Trait(trait_inst)) = resolve_path(
                db,
                candidate_path,
                candidate,
                PredicateListId::empty_list(db),
                true,
            ) else {
                continue;
            };
            return Some(trait_inst);
        }
    }
    None
}

pub(crate) fn nominal_attrs<'db>(
    db: &'db dyn hir::analysis::HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<hir::hir_def::AttrListId<'db>> {
    let ty = ty.as_view(db).unwrap_or(ty);
    let adt = ty.adt_def(db)?;
    let AdtRef::Struct(struct_) = adt.adt_ref(db) else {
        return None;
    };
    struct_.scope().attrs(db)
}
