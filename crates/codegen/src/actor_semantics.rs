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
    ty::{adt_def::AdtRef, trait_resolution::PredicateListId, ty_def::TyId},
};
use hir::hir_def::{ItemKind, PathId, Struct, TopLevelMod};
use hir::span::{ActorDesugaredFocus, DesugaredOrigin, HirOrigin};

#[derive(Debug)]
pub(crate) struct SemanticActor<'db> {
    pub(crate) state: Struct<'db>,
    pub(crate) behaviors: Vec<hir::hir_def::Func<'db>>,
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
    db: &'db DriverDataBase,
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

pub(crate) fn nominal_attrs<'db>(
    db: &'db DriverDataBase,
    ty: TyId<'db>,
) -> Option<hir::hir_def::AttrListId<'db>> {
    let ty = ty.as_view(db).unwrap_or(ty);
    let adt = ty.adt_def(db)?;
    let AdtRef::Struct(struct_) = adt.adt_ref(db) else {
        return None;
    };
    struct_.scope().attrs(db)
}
