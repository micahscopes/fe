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
    ty::{trait_def::TraitInstId, trait_resolution::PredicateListId, ty_def::TyId},
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
/// policy family has six policy axes before its length and element. Backends
/// consume the semantic element/length pair and the complete concrete type;
/// they never receive those Fe policy axes as a parallel Rust schema.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SemanticGpuResource<'db> {
    /// Concrete normalized Fe resource type supplied to generic CTFE policy
    /// projection. This preserves the complete denotation without exposing its
    /// policy-axis positions to target backends.
    pub(crate) resource_ty: TyId<'db>,
    pub(crate) kind: GpuResource,
    pub(crate) element_ty: TyId<'db>,
    pub(crate) length_ty: TyId<'db>,
    pub(crate) has_typed_policy: bool,
}

/// Semantic element discovery shared by resource consumers. This deliberately
/// does not choose a physical layout or decide whether signed storage is legal
/// in a browser interface.
pub(crate) enum SemanticResourceElement<'db> {
    Scalar(ResourceScalar),
    Record {
        name: Option<String>,
        fields: Vec<(Option<String>, TyId<'db>)>,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum ResourceScalar { U32, I32, F32 }

pub(crate) fn resource_scalar(
    db: &dyn hir::analysis::HirAnalysisDb,
    ty: TyId<'_>,
) -> Option<ResourceScalar> {
    use hir::analysis::ty::ty_def::{PrimTy, TyBase, TyData};
    let ty = ty.as_view(db).unwrap_or(ty);
    match ty.base_ty(db).data(db) {
        TyData::TyBase(TyBase::Prim(PrimTy::U32)) => Some(ResourceScalar::U32),
        TyData::TyBase(TyBase::Prim(PrimTy::I32)) => Some(ResourceScalar::I32),
        TyData::TyBase(TyBase::Prim(PrimTy::F32)) => Some(ResourceScalar::F32),
        _ => None,
    }
}

pub(crate) enum ResourceElementError { NotRecord, EmptyOrInconsistent }

#[cfg(test)]
mod resource_element_tests {
    use super::*;
    use common::InputDb;
    use hir::hir_def::HirIngot;

    #[test]
    fn aliases_and_generic_fields_retain_semantic_signedness() {
        let mut db = DriverDataBase::default();
        let url = url::Url::parse("file:///resource_element_view.fe").unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(r#"
struct Pair<T> { first: T, second: f32 }
type SignedPair = Pair<i32>
pub fn identity(value: SignedPair) -> SignedPair { value }
"#.to_owned()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let function = top.ingot(&db).all_funcs(&db).iter().copied()
            .find(|f| f.name(&db).to_opt().is_some_and(|n| n.data(&db) == "identity"))
            .unwrap();
        let shape = semantic_resource_element(&db, function.return_ty(&db));
        let Ok(SemanticResourceElement::Record { fields, .. }) = shape else {
            panic!("alias to instantiated record must retain its shape");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0.as_deref(), Some("first"));
        assert_eq!(fields[1].0.as_deref(), Some("second"));
        assert!(matches!(resource_scalar(&db, fields[0].1), Some(ResourceScalar::I32)));
        assert!(matches!(resource_scalar(&db, fields[1].1), Some(ResourceScalar::F32)));
    }
}

pub(crate) fn semantic_resource_element<'db>(
    db: &'db dyn hir::analysis::HirAnalysisDb,
    ty: TyId<'db>,
) -> Result<SemanticResourceElement<'db>, ResourceElementError> {
    use hir::analysis::ty::adt_def::AdtRef;
    use hir::hir_def::FieldParent;
    let ty = ty.as_view(db).unwrap_or(ty);
    if let Some(scalar) = resource_scalar(db, ty) {
        return Ok(SemanticResourceElement::Scalar(scalar));
    }
    let Some(adt) = ty.adt_def(db) else {
        return Err(ResourceElementError::NotRecord);
    };
    let AdtRef::Struct(record) = adt.adt_ref(db) else {
        return Err(ResourceElementError::NotRecord);
    };
    let declared = FieldParent::Struct(record).fields(db).collect::<Vec<_>>();
    let instantiated = ty.field_types(db);
    if declared.is_empty() || declared.len() != instantiated.len() {
        return Err(ResourceElementError::EmptyOrInconsistent);
    }
    Ok(SemanticResourceElement::Record {
        name: record.name(db).to_opt().map(|name| name.data(db).to_string()),
        fields: declared.into_iter().zip(instantiated).map(|(field, ty)| {
            (field.name(db).map(|name| name.data(db).to_string()), ty)
        }).collect(),
    })
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
                resource_ty: ty,
                kind,
                element_ty: *element_ty,
                length_ty: *length_ty,
                has_typed_policy: false,
            }
        }
        GpuResource::Readback => {
            let [element_ty, length_ty, _message_ty] = args else {
                return Err(
                    "GPU readback resource type requires exactly element, length, and message arguments",
                );
            };
            SemanticGpuResource {
                resource_ty: ty,
                kind,
                element_ty: *element_ty,
                length_ty: *length_ty,
                has_typed_policy: true,
            }
        }
        GpuResource::Indirect => {
            let [_brand_ty, length_ty, element_ty] = args else {
                return Err(
                    "GPU indirect resource type requires exactly brand, length, and element arguments",
                );
            };
            SemanticGpuResource {
                resource_ty: ty,
                kind,
                element_ty: *element_ty,
                length_ty: *length_ty,
                has_typed_policy: true,
            }
        }
        GpuResource::StorageFamily => {
            let [
                _kind_ty,
                _access_ty,
                _residency_ty,
                _init_ty,
                _recovery_ty,
                _visibility_ty,
                length_ty,
                element_ty,
            ] = args
            else {
                return Err(
                    "GPU storage family requires kind, access, residency, initialization, recovery, visibility, length, and element arguments",
                );
            };
            SemanticGpuResource {
                resource_ty: ty,
                kind,
                element_ty: *element_ty,
                length_ty: *length_ty,
                has_typed_policy: true,
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
    // Attributes belong to the nominal ADT scope for both structs and enums.
    // Resource/control projections historically needed only structs; typed
    // decision effects also use attributed fieldless enums.
    adt.scope(db).attrs(db)
}
