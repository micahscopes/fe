use cranelift_entity::EntityRef;
use hir::analysis::semantic::{SemanticInstance, check_semantic_borrows, check_semantic_noesc};
use hir::analysis::ty::ty_def::TyId;
use hir::hir_def::Func;
use salsa::Update;

use crate::{
    db::MirDb,
    runtime::{
        LowerError, LoweredRuntimeBody, RLocal, RLocalId, RuntimeBody, RuntimeCallEdge,
        RuntimeCarrier, RuntimeClass, RuntimeExitBehavior, RuntimeInterfaceSignature,
        RuntimeLocalRoot, RuntimeParam, RuntimeSyntheticSpec,
        lower::{
            body::{declared_external_func, lower_to_rmir},
            call::{
                collect_referenced_code_regions, collect_referenced_const_regions,
                collect_runtime_calls as collect_runtime_calls_lowered,
            },
            interface::runtime_param_locals,
            returns::{runtime_exit_behavior, runtime_return_class},
        },
        synthetic::{lower_synthetic_runtime_body, runtime_synthetic_interface_signature},
    },
};

#[salsa::interned]
#[derive(Debug)]
pub struct RuntimeSyntheticInstance<'db> {
    pub spec: RuntimeSyntheticSpec<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeInstanceSource<'db> {
    Semantic(SemanticInstance<'db>),
    Synthetic(RuntimeSyntheticInstance<'db>),
}

#[salsa::interned]
#[derive(Debug)]
pub struct RuntimeInstanceKey<'db> {
    pub source: RuntimeInstanceSource<'db>,
    #[return_ref]
    pub params: Vec<RuntimeClass<'db>>,
}

impl<'db> RuntimeInstanceKey<'db> {
    pub fn semantic(self, db: &'db dyn MirDb) -> Option<SemanticInstance<'db>> {
        match self.source(db) {
            RuntimeInstanceSource::Semantic(semantic) => Some(semantic),
            RuntimeInstanceSource::Synthetic(_) => None,
        }
    }
}

#[salsa::tracked]
#[derive(Debug)]
pub struct RuntimeInstance<'db> {
    pub key: RuntimeInstanceKey<'db>,
}

#[salsa::tracked]
impl<'db> RuntimeInstance<'db> {
    #[salsa::tracked]
    pub fn interface_signature(self, db: &'db dyn MirDb) -> RuntimeInterfaceSignature<'db> {
        runtime_interface_signature_for_key(db, self.key(db))
    }

    #[salsa::tracked]
    pub fn exit_behavior(self, db: &'db dyn MirDb) -> RuntimeExitBehavior {
        runtime_exit_behavior(db, self.key(db))
    }

    #[salsa::tracked]
    pub fn body(self, db: &'db dyn MirDb) -> RuntimeBody<'db> {
        expect_lowered_runtime_body(db, self).body(db)
    }

    #[salsa::tracked(return_ref)]
    pub fn calls(self, db: &'db dyn MirDb) -> Vec<RuntimeCallEdge<'db>> {
        expect_lowered_runtime_body(db, self).direct_callees(db)
    }

    #[salsa::tracked(return_ref)]
    pub fn referenced_const_regions(
        self,
        db: &'db dyn MirDb,
    ) -> Vec<crate::runtime::ConstRegionId<'db>> {
        expect_lowered_runtime_body(db, self).referenced_const_regions(db)
    }

    #[salsa::tracked(return_ref)]
    pub fn referenced_code_regions(
        self,
        db: &'db dyn MirDb,
    ) -> Vec<crate::runtime::RuntimeCodeRegion<'db>> {
        expect_lowered_runtime_body(db, self).referenced_code_regions(db)
    }
}

pub(crate) fn runtime_interface_signature_for_key<'db>(
    db: &'db dyn MirDb,
    key: RuntimeInstanceKey<'db>,
) -> RuntimeInterfaceSignature<'db> {
    match key.source(db) {
        RuntimeInstanceSource::Semantic(semantic) => RuntimeInterfaceSignature {
            params: key
                .params(db)
                .iter()
                .zip(runtime_param_locals(db, semantic, key.params(db)))
                .map(|(class, local)| RuntimeParam {
                    local: RLocalId::from_u32(local.index() as u32),
                    class: class.clone(),
                })
                .collect(),
            ret: runtime_return_class(db, key),
        },
        RuntimeInstanceSource::Synthetic(synthetic) => {
            runtime_synthetic_interface_signature(synthetic.spec(db).clone())
        }
    }
}

#[salsa::tracked]
pub fn get_or_build_runtime_instance<'db>(
    db: &'db dyn MirDb,
    key: RuntimeInstanceKey<'db>,
) -> RuntimeInstance<'db> {
    RuntimeInstance::new(db, key)
}

/// Materialize a DECLARED-EXTERNAL runtime function (a non-builtin `extern`,
/// which the wasm backend emits as a `("fe", <symbol>)` host import): its
/// signature only, no body. Fails closed if any parameter or the return type is
/// not representable across the v0 host-import boundary (scalar-only), naming the
/// offending Fe type. The signature is computed exactly like any other runtime
/// instance (`typed_body_for_bodyless_func` populates the extern's param/return
/// types), so no extern-specific signature path is needed; only the body is
/// empty.
fn external_declaration_body<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
    func: Func<'db>,
) -> Result<RuntimeBody<'db>, LowerError> {
    let signature = instance.interface_signature(db);
    let name = func
        .name(db)
        .to_opt()
        .map(|ident| ident.data(db).to_string())
        .unwrap_or_else(|| "<extern>".to_string());
    for (idx, param) in signature.params.iter().enumerate() {
        if !matches!(param.class, RuntimeClass::Scalar(_)) {
            let ty = func
                .arg_tys(db)
                .get(idx)
                .map(|binder| binder.skip_binder().pretty_print(db).to_string())
                .unwrap_or_else(|| format!("{:?}", param.class));
            return Err(LowerError::Unsupported(format!(
                "extern host import `{name}` parameter {idx} has type `{ty}`, which is not \
                 representable across the wasm import boundary (only scalar params/returns \
                 are supported in v0)"
            )));
        }
    }
    if let Some(ret) = &signature.ret
        && !matches!(ret, RuntimeClass::Scalar(_))
    {
        let ty = func.return_ty(db).pretty_print(db).to_string();
        return Err(LowerError::Unsupported(format!(
            "extern host import `{name}` return type `{ty}` is not representable across the \
             wasm import boundary (only scalar params/returns are supported in v0)"
        )));
    }

    // The declaration has no body, but the package verifier still requires each
    // signature param to resolve to a value-carried local (`verify_signature`).
    // Materialize exactly those param locals (value-carried scalars, no root) and
    // nothing else; there are no blocks, so nothing reads them at runtime.
    let filler = RLocal {
        semantic_ty: TyId::unit(db),
        carrier: RuntimeCarrier::Erased,
        root: RuntimeLocalRoot::None,
    };
    let mut locals: Vec<RLocal<'db>> = Vec::new();
    for (idx, param) in signature.params.iter().enumerate() {
        let local_idx = param.local.index();
        if local_idx >= locals.len() {
            locals.resize(local_idx + 1, filler.clone());
        }
        let semantic_ty = func
            .arg_tys(db)
            .get(idx)
            .map(|binder| *binder.skip_binder())
            .unwrap_or_else(|| TyId::unit(db));
        locals[local_idx] = RLocal {
            semantic_ty,
            carrier: RuntimeCarrier::Value(param.class.clone()),
            root: RuntimeLocalRoot::None,
        };
    }

    Ok(RuntimeBody {
        owner: instance,
        key: instance.key(db),
        signature,
        semantic_locals: Vec::new(),
        provider_bindings: Vec::new(),
        locals,
        blocks: Vec::new(),
    })
}

#[salsa::tracked]
fn lower_runtime_body<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> Result<LoweredRuntimeBody<'db>, LowerError> {
    let body = match instance.key(db).source(db) {
        RuntimeInstanceSource::Semantic(semantic) => {
            if let Some(func) = declared_external_func(db, semantic) {
                // A non-builtin `extern`: a DECLARED-EXTERNAL runtime function
                // with no body (a wasm host import). It has no Fe body to borrow-
                // or noesc-check or normalize; materialize its signature only.
                external_declaration_body(db, instance, func)?
            } else {
                if let Err(diag) = check_semantic_borrows(db, semantic) {
                    return Err(LowerError::Unsupported(format!(
                        "semantic borrow checking failed for {:?}: {}",
                        semantic.key(db),
                        diag.message
                    )));
                }
                if let Err(diag) = check_semantic_noesc(db, semantic) {
                    return Err(LowerError::Unsupported(format!(
                        "semantic noesc checking failed for {:?}: {}",
                        semantic.key(db),
                        diag.message
                    )));
                }
                lower_to_rmir(db, instance)?
            }
        }
        RuntimeInstanceSource::Synthetic(synthetic) => {
            lower_synthetic_runtime_body(db, instance, synthetic.spec(db).clone())
        }
    };
    let direct_callees = collect_runtime_calls_lowered(&body);
    let referenced_const_regions = collect_referenced_const_regions(&body);
    let referenced_code_regions = collect_referenced_code_regions(&body);
    Ok(LoweredRuntimeBody::new(
        db,
        body,
        direct_callees,
        referenced_const_regions,
        referenced_code_regions,
    ))
}

pub(crate) fn runtime_instance_lowered_body<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> Result<LoweredRuntimeBody<'db>, LowerError> {
    lower_runtime_body(db, instance)
}

fn expect_lowered_runtime_body<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> LoweredRuntimeBody<'db> {
    lower_runtime_body(db, instance).unwrap_or_else(|err| {
        panic!(
            "runtime lowering failed for {:?}: {err}",
            instance.key(db).source(db)
        )
    })
}
