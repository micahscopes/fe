use cranelift_entity::EntityRef;
use hir::analysis::semantic::{SemanticInstance, check_semantic_borrows, check_semantic_noesc};
use hir::analysis::ty::ty_def::TyId;
use hir::hir_def::Func;
use salsa::Update;

use crate::{
    db::MirDb,
    runtime::{
        AddressSpaceKind, Layout, LowerError, LoweredRuntimeBody, RLocal, RLocalId, RefKind,
        RuntimeBody, RuntimeCallEdge, RuntimeCarrier, RuntimeClass, RuntimeExitBehavior,
        RuntimeInterfaceSignature, RuntimeLocalRoot, RuntimeParam, RuntimeSyntheticSpec,
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

/// The wasm import MODULE for a runtime function, from the
/// `#[wasm_import(module = "...")]` attribute (R3.3) on its `extern` block, if
/// present and well-formed. `None` for a locally-defined function, an
/// attribute-less `extern`, or a synthetic instance; the wasm backend then falls
/// back to the flat `"fe"` v0 import-module convention. Only a DECLARED-EXTERNAL
/// (non-builtin `extern`) function can carry a module. EVM externs are recognized
/// builtins, so `declared_external_func` returns `None` for them and this stays
/// `None` (never consulted on the EVM path anyway).
pub fn wasm_import_module<'db>(db: &'db dyn MirDb, instance: RuntimeInstance<'db>) -> Option<String> {
    let RuntimeInstanceSource::Semantic(semantic) = instance.key(db).source(db) else {
        return None;
    };
    let func = declared_external_func(db, semantic)?;
    hir::hir_def::ItemKind::Func(func)
        .attrs(db)?
        .wasm_import_module(db)
}

/// The wasm import FIELD NAME for a runtime function: a DECLARED-EXTERNAL `extern`
/// function's BASE declared identifier (e.g. `gpu_buffer_create`), which is the
/// stable name the host broker binds under the import module - "the import table IS
/// the op set" (interop 4.1/4.2). `None` for a locally-defined or synthetic instance.
/// This DECOUPLES the host import field name from the internal Sonatina symbol, which
/// is mangled per instance (`std__lib__webgpu__raw__gpu_buffer_create_HASH`) and would
/// otherwise leak into the import table and duplicate one op across effect-provider
/// scopes. Every declared-external func returns its bare identifier here, so a
/// top-level extern (symbol already == base name) is unaffected and a std-ingot extern
/// imports as its op name. EVM never consults this (EVM externs are recognized
/// builtins, not declared-external).
pub fn wasm_import_name<'db>(db: &'db dyn MirDb, instance: RuntimeInstance<'db>) -> Option<String> {
    let RuntimeInstanceSource::Semantic(semantic) = instance.key(db).source(db) else {
        return None;
    };
    let func = declared_external_func(db, semantic)?;
    Some(func.name(db).to_opt()?.data(db).to_string())
}

/// Materialize a DECLARED-EXTERNAL runtime function (a non-builtin `extern`,
/// which the wasm backend emits as a `("fe", <symbol>)` host import): its
/// signature only, no body. Fails closed if any parameter or the return type is
/// not representable across the v0 host-import boundary (scalar-only), naming the
/// offending Fe type. The signature is computed exactly like any other runtime
/// instance (`typed_body_for_bodyless_func` populates the extern's param/return
/// types), so no extern-specific signature path is needed; only the body is
/// empty.
/// Whether a runtime class may cross the wasm host-import boundary. R3.2 admitted
/// scalars only; R3.4b (Amendment 4) additionally admits the `MemPtr<B::Word>`
/// transport class and, by the transport-newtype extension, the single-`u32`-field
/// capability newtypes (`WebGpuRef<u32, Global>`, `KernelId`, `PendingId`). Each is
/// represented by the wasm lowerer as its single word (i32 on wasm32): a `MemPtr<u32>`
/// classifies as a memory-space `RawAddr` (a raw memory address, the class the runtime
/// classifier assigns every host-minted region pointer); a memory-space provider
/// reference is admitted on the same footing; and a single-scalar-field aggregate is
/// its one field's scalar (exactly what `wasm_lower::ty_for_class` lowers it to).
/// Everything else (object/const refs, non-memory addresses/providers, multi-field /
/// empty aggregates, arrays, enums) stays fail-closed. EVM externs never reach this
/// predicate (they are recognized builtins, not declared-external), so EVM lowering is
/// untouched.
fn is_wasm_import_boundary_class(db: &dyn MirDb, class: &RuntimeClass<'_>) -> bool {
    matches!(
        class,
        RuntimeClass::Scalar(_)
            | RuntimeClass::RawAddr {
                space: AddressSpaceKind::Memory,
                ..
            }
            | RuntimeClass::Ref {
                kind: RefKind::Provider {
                    space: AddressSpaceKind::Memory,
                    ..
                },
                ..
            }
    ) || is_single_scalar_field_newtype(db, class)
}

/// A single-scalar-field aggregate (a `u32` capability newtype such as `WebGpuRef`,
/// `KernelId`, `PendingId`): it crosses the extern boundary WHOLE and transports as
/// its one field's scalar word. Multi-field / empty aggregates, arrays, and enums are
/// NOT single-scalar-field newtypes and stay fail-closed. This is the boundary-gate
/// twin of `wasm_lower::single_scalar_field`.
fn is_single_scalar_field_newtype(db: &dyn MirDb, class: &RuntimeClass<'_>) -> bool {
    let RuntimeClass::AggregateValue { layout } = class else {
        return false;
    };
    match layout.data(db) {
        Layout::Struct(struct_layout) => matches!(&*struct_layout.fields, [RuntimeClass::Scalar(_)]),
        Layout::Array(_) | Layout::Enum(_) => false,
    }
}

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
        if !is_wasm_import_boundary_class(db, &param.class) {
            let ty = func
                .arg_tys(db)
                .get(idx)
                .map(|binder| binder.skip_binder().pretty_print(db).to_string())
                .unwrap_or_else(|| format!("{:?}", param.class));
            return Err(LowerError::Unsupported(format!(
                "extern host import `{name}` parameter {idx} has type `{ty}`, which is not \
                 representable across the wasm import boundary (only scalar params, \
                 memory-region pointers, and single-u32-field capability newtypes are \
                 supported)"
            )));
        }
    }
    if let Some(ret) = &signature.ret
        && !is_wasm_import_boundary_class(db, ret)
    {
        let ty = func.return_ty(db).pretty_print(db).to_string();
        return Err(LowerError::Unsupported(format!(
            "extern host import `{name}` return type `{ty}` is not representable across the \
             wasm import boundary (only scalar, memory-pointer, and single-u32-field \
             capability-newtype returns are supported)"
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
