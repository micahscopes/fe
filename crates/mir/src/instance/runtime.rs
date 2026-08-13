use cranelift_entity::EntityRef;
use hir::analysis::semantic::{SemanticInstance, check_semantic_borrows, check_semantic_noesc};
use hir::analysis::ty::corelib::{RuntimeControlEffectFuncKind, runtime_control_effect_func_kind};
use hir::analysis::ty::ty_check::BodyOwner;
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

pub use hir::hir_def::{HostResultCodec, IndirectHostResult};

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

/// The target-neutral host namespace for a runtime function, from the
/// `#[host_import(module = "...")]` attribute on its `extern` block. `None` for
/// a locally-defined function, an attribute-less `extern`, or a synthetic
/// instance. Only a DECLARED-EXTERNAL
/// (non-builtin `extern`) function can carry a module. EVM externs are recognized
/// builtins, so `declared_external_func` returns `None` for them and this stays
/// `None` (never consulted on the EVM path anyway).
pub fn host_import_module<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> Option<String> {
    let RuntimeInstanceSource::Semantic(semantic) = instance.key(db).source(db) else {
        return None;
    };
    let func = declared_external_func(db, semantic)?;
    hir::hir_def::ItemKind::Func(func)
        .attrs(db)?
        .host_import_module(db)
}

/// The host operation name for a runtime function: a DECLARED-EXTERNAL `extern`
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
pub fn host_import_name<'db>(db: &'db dyn MirDb, instance: RuntimeInstance<'db>) -> Option<String> {
    let RuntimeInstanceSource::Semantic(semantic) = instance.key(db).source(db) else {
        return None;
    };
    let func = declared_external_func(db, semantic)?;
    Some(func.name(db).to_opt()?.data(db).to_string())
}

/// Recover the nominal control operation represented by a runtime call. This
/// consults the declaring Fe item, not an import string or caller-authored
/// table, so an unrelated extern named `suspend` cannot acquire continuation
/// semantics.
pub fn runtime_control_effect_kind<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> Option<RuntimeControlEffectFuncKind> {
    let RuntimeInstanceSource::Semantic(semantic) = instance.key(db).source(db) else {
        return None;
    };
    let BodyOwner::Func(func) = semantic.key(db).owner(db) else {
        return None;
    };
    runtime_control_effect_func_kind(db, func)
}

/// Codec-versioned indirect aggregate result metadata carried unchanged from
/// the authored extern declaration into backend lowering.
pub fn indirect_host_result<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> Option<IndirectHostResult> {
    let RuntimeInstanceSource::Semantic(semantic) = instance.key(db).source(db) else {
        return None;
    };
    let func = declared_external_func(db, semantic)?;
    hir::hir_def::ItemKind::Func(func)
        .attrs(db)?
        .indirect_host_result(db)
}

/// Compatibility alias for code that still uses the Wasm-specific vocabulary.
pub fn wasm_import_module<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
) -> Option<String> {
    host_import_module(db, instance)
}

/// Compatibility alias for code that still uses the Wasm-specific vocabulary.
pub fn wasm_import_name<'db>(db: &'db dyn MirDb, instance: RuntimeInstance<'db>) -> Option<String> {
    host_import_name(db, instance)
}

/// Whether a runtime class is a recursively flat product of host ABI lanes.
/// Scalars, memory-region pointers, and structs composed solely from those
/// values are admitted. Arrays, enums, object/const references, and non-memory
/// providers remain fail-closed. Individual backends still decide whether they
/// can realize a declared host import.
fn is_host_import_boundary_class(db: &dyn MirDb, class: &RuntimeClass<'_>) -> bool {
    match class {
        RuntimeClass::Scalar(_)
        | RuntimeClass::RawAddr {
            space: AddressSpaceKind::Memory,
            ..
        }
        | RuntimeClass::Ref {
            kind:
                RefKind::Provider {
                    space: AddressSpaceKind::Memory,
                    ..
                },
            ..
        } => true,
        RuntimeClass::AggregateValue { layout } => match layout.data(db) {
            Layout::Struct(struct_layout) => struct_layout
                .fields
                .iter()
                .all(|field| is_host_import_boundary_class(db, field)),
            Layout::Array(_) | Layout::Enum(_) => false,
        },
        _ => false,
    }
}

fn external_declaration_body<'db>(
    db: &'db dyn MirDb,
    instance: RuntimeInstance<'db>,
    func: Func<'db>,
) -> Result<RuntimeBody<'db>, LowerError> {
    // A compiler-recognized control operation is represented by an extern Fe
    // declaration so semantic analysis can type it normally, but it never
    // crosses the flat host-import ABI. Resumable materialization consumes the
    // call and supplies the typed delivery local on re-entry, so aggregate
    // outcomes are legal here even though an ordinary host import returning the
    // same enum would correctly fail closed below.
    let compiler_consumed_control = runtime_control_effect_func_kind(db, func).is_some();
    if let Some(attrs) = hir::hir_def::ItemKind::Func(func).attrs(db)
        && attrs.get_attr(db, "host_import").is_some()
        && attrs.get_attr(db, "wasm_import").is_some()
    {
        let name = func
            .name(db)
            .to_opt()
            .map(|ident| ident.data(db).to_string())
            .unwrap_or_else(|| "<extern>".to_string());
        return Err(LowerError::Unsupported(format!(
            "extern host import `{name}` declares both `#[host_import]` and its \
             `#[wasm_import]` compatibility alias"
        )));
    }
    let signature = instance.interface_signature(db);
    let name = func
        .name(db)
        .to_opt()
        .map(|ident| ident.data(db).to_string())
        .unwrap_or_else(|| "<extern>".to_string());
    for (idx, param) in signature.params.iter().enumerate() {
        if compiler_consumed_control {
            break;
        }
        if !is_host_import_boundary_class(db, &param.class) {
            let ty = func
                .arg_tys(db)
                .get(idx)
                .map(|binder| binder.skip_binder().pretty_print(db).to_string())
                .unwrap_or_else(|| format!("{:?}", param.class));
            return Err(LowerError::Unsupported(format!(
                "extern host import `{name}` parameter {idx} has type `{ty}`, which is not \
                 representable across the flat host-import boundary (only recursively flat \
                 scalar products and memory-region pointers are supported)"
            )));
        }
    }
    let indirect_result = indirect_host_result(db, instance);
    if let Some(descriptor) = indirect_result
        && (descriptor.codec != HostResultCodec::FeHostWasm || descriptor.version != 1)
    {
        return Err(LowerError::Unsupported(format!(
            "extern host import `{name}` declares an unsupported indirect host result codec"
        )));
    }
    if indirect_result.is_some()
        && signature
            .ret
            .as_ref()
            .is_none_or(|ret| !matches!(ret, RuntimeClass::AggregateValue { .. }))
    {
        let ty = func.return_ty(db).pretty_print(db).to_string();
        return Err(LowerError::Unsupported(format!(
            "extern host import `{name}` declares an indirect host result, but authored return \
             type `{ty}` is not an aggregate"
        )));
    }
    if let Some(ret) = &signature.ret
        && !is_host_import_boundary_class(db, ret)
        && indirect_result.is_none()
        && !compiler_consumed_control
    {
        let ty = func.return_ty(db).pretty_print(db).to_string();
        return Err(LowerError::Unsupported(format!(
            "extern host import `{name}` return type `{ty}` is not representable across the \
             flat host-import boundary (only recursively flat scalar products and \
             memory-region pointers are supported)"
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
