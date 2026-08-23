//! MIR -> Sonatina IR lowering for the wasm target.
//!
//! This is the first genuinely-Fe-compiled wasm path: `MIR runtime package ->
//! Sonatina IR (portable vocabulary, Wasm32 ISA) -> WAFFLE -> wasm bytes`.
//! The portable value lane includes scalar arithmetic/control flow/calls,
//! recursively flattened struct and fixed-array values, materialized aggregate
//! slots and object references, target-layout memory projections, fieldless
//! enum tags, flattened payload-enum values, and the canonical browser ABI
//! wrappers. Unsupported memory-carried payload enums, wide scalar operations,
//! EVM host operations, and other target-specific constructs fail closed rather
//! than being silently approximated.
//!
//! Why a separate path rather than parameterizing the EVM lowerer
//! (`lower_runtime.rs`): the EVM path hardcodes `Type::I256` as the word in ~90
//! places and lowers Fe's checked arithmetic to `uaddo` + an EVM `revert` panic
//! block (see `emit_panic_revert`), which is EVM-native. A faithful portable
//! rewrite of that lowerer is target-backend scale. Here we lower clean MIR directly:
//! at the MIR level `a + b` is `RExpr::Binary { op: Arith(Add), .. }` with no
//! overflow machinery attached, so we emit a plain portable `arith::Add`. The
//! Checked wasm32 `usize` add/sub/mul are explicitly guarded. Other checked
//! arithmetic remains supported only where the portable backend implements its
//! exact semantics; unsupported cases fail closed.
//!
//! It reuses Sonatina's `FunctionBuilder` SSA-variable machinery (declare/def/
//! use + `seal_all`) exactly as the EVM lowerer does, so loop-carried values
//! (`sum_to`'s accumulator) get their phis inserted automatically. MIR runtime
//! locals may be value-carried or materialized in the canonical arena. Scalar
//! slots remain SSA-promoted; fixed aggregate slots receive an independent
//! target-layout copy so dynamic indexing has ordinary Fe value semantics.
//! Memory-space provider/object references use explicit bounds-checked address
//! projection and typed loads/stores. Place forms outside those admitted,
//! layout-derived cases continue to fail closed.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use compiler_db::DriverDataBase;
use hir::projection::IndexSource;
use hir::{
    analysis::{
        semantic::instantiate_with_generic_args,
        ty::{
            adt_def::AdtRef,
            const_ty::{ConstTyData, EvaluatedConstTy},
            ty_def::{PrimTy, TyBase, TyData, TyId},
        },
    },
    hir_def::{ArithBinOp, BinOp, CompBinOp, GpuIntrinsic, GpuResource, UnOp},
};
use mir::{
    AddressSpaceKind, ConstNode, ConstScalar, IntrinsicArithBinOp, Layout, LayoutId, PlaceElem,
    PlaceRoot, RBlockId, RExpr, RLocal, RLocalId, RStmt, RTerminator, RefKind, RefView,
    RuntimeBody, RuntimeBuiltin, RuntimeCarrier, RuntimeClass, RuntimeFunction, RuntimeInlineHint,
    RuntimeInstance, RuntimeLinkage, RuntimeLocalRoot, RuntimePackage, RuntimePlace, ScalarClass,
    ScalarRepr, ScalarRole, VariantId,
};
use rustc_hash::FxHashMap;
#[cfg(feature = "sonatina-indirect-calls")]
use sonatina_ir::inst::{control_flow::CallIndirect, data::GetFunctionPtr};
use sonatina_ir::{
    BlockId, GlobalVariableData, Immediate, Linkage, Module, Signature, Type, ValueId,
    builder::{FunctionBuilder, ModuleBuilder, Variable},
    func_cursor::InstInserter,
    global_variable::GvInitializer,
    inst::{
        arith::{
            Add, Fabs, Fadd, Fceil, Fclamp, Fdiv, Ffloor, Fmax, FmaxRelaxed, Fmin, FminRelaxed,
            Fmul, Fneg, Fround, Fsqrt, Fsub, Ftrunc, Mul, Sar, Shl, Shr, Sub, Udiv, Umod,
        },
        cast::{Bitcast, F32ToI32, I32ToF32, Sext, Trunc, Zext},
        cmp::{Eq as CmpEq, Feq, Fle, Flt, IsZero, Lt, Slt},
        control_flow::{Br, Call, Jump, Phi, Return, Unreachable},
        data::{
            MemAllocDynamic, MemCheckpoint, MemRewind, Mload, Mstore, ObjIndex, ObjLoad, ObjProj,
            ObjStore,
        },
        logic::{And, Or, Xor},
        native::inst_set::NativeInstSet,
    },
    isa::{Isa, wasm32::Wasm32},
    module::{FuncRef, ModuleCtx},
};
use sonatina_triple::{Architecture, OperatingSystem, TargetTriple, Vendor};

use super::LowerError;
use super::lower_runtime::{
    assign_sonatina_function_symbols, bytes_to_i256, linkage_for_runtime, scalar_ty,
};

fn wasm_lower_trace(message: impl FnOnce() -> String) {
    if std::env::var_os("FE_WASM_LOWER_TRACE").is_some() {
        eprintln!("[fe wasm lowering] {}", message());
    }
}

fn wasm_lower_trace_detail(message: impl FnOnce() -> String) {
    if std::env::var_os("FE_WASM_LOWER_TRACE_DETAIL").is_some() {
        eprintln!("[fe wasm lowering] {}", message());
    }
}

/// Keep generated core-Wasm signatures within wasmparser's validated resource
/// limit. Private Fe-to-Fe calls may replace an oversized by-value aggregate
/// lane with one compiler-owned arena pointer; host-visible signatures never
/// receive that internal representation.
const MAX_WASM_FUNCTION_PARAMS: usize = 1000;
const MAX_WASM_FUNCTION_RETURNS: usize = 1000;
/// Keep one generated Fe aggregate from consuming the browser validator's
/// finite local budget. Values above the same validated arity boundary used by
/// private calls are represented by one compiler-owned arena pointer. Their
/// MIR class remains `AggregateValue`, so this is a backend representation
/// choice rather than an authored reference or an ABI change.
const MAX_WASM_FLATTENED_LOCAL_AGGREGATE: usize = 1000;

/// The Wasm32 ISA the wasm lowering targets (little-endian, 32-bit pointers,
/// portable `NativeInstSet` vocabulary).
pub(crate) fn create_wasm32_isa() -> Wasm32 {
    Wasm32::new(TargetTriple::new(
        Architecture::Wasm32,
        Vendor::Unknown,
        OperatingSystem::Native,
    ))
}

fn gpu_intrinsic(db: &DriverDataBase, instance: RuntimeInstance<'_>) -> Option<GpuIntrinsic> {
    let semantic = instance.key(db).semantic(db)?;
    let hir::analysis::ty::ty_check::BodyOwner::Func(func) = semantic.key(db).owner(db) else {
        return None;
    };
    func.scope().attrs(db)?.gpu_intrinsic(db)
}

fn semantic_gpu_resource(db: &DriverDataBase, ty: TyId<'_>) -> bool {
    let ty = ty.as_view(db).unwrap_or(ty);
    let Some(adt) = ty.adt_def(db) else {
        return false;
    };
    let AdtRef::Struct(struct_) = adt.adt_ref(db) else {
        return false;
    };
    struct_
        .scope()
        .attrs(db)
        .is_some_and(|attrs| attrs.gpu_resource(db) == Some(GpuResource::Storage))
}

fn semantic_const_u32(db: &DriverDataBase, ty: TyId<'_>) -> Option<u32> {
    let TyData::ConstTy(value) = ty.data(db) else {
        return None;
    };
    let evaluated = value.evaluate(db, None);
    let ConstTyData::Evaluated(EvaluatedConstTy::LitInt(value), _) = evaluated.data(db) else {
        return None;
    };
    u32::try_from(value.data(db).clone()).ok()
}

/// Lower a MIR runtime package to a Sonatina IR `Module` built under the Wasm32
/// ISA. The resulting module is handed to the Sonatina WAFFLE backend to emit
/// wasm bytes. The second return value is the symbol -> wasm-import-module side
/// table (R3.3): the WAFFLE backend consults it to name each external
/// declaration's import module, defaulting to the flat `"fe"` convention for
/// symbols it does not list.
pub fn compile_runtime_package_wasm(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
) -> Result<(Module, HashMap<String, String>), LowerError> {
    compile_runtime_package_wasm_with_canonical_lanes(db, package, &[], &[], None, None, None, &[])
}

/// Build the shared Sonatina module for a shader target. Shader entrypoints
/// never receive untyped host values: compiler-derived WebGPU bindings and the
/// resident Wasm wrapper mediate that boundary. Accordingly this path omits
/// host-forgery enum traps that WebGPU render stages cannot realize.
pub(crate) fn compile_runtime_package_shader_ir(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
) -> Result<(Module, HashMap<String, String>), LowerError> {
    compile_runtime_package_wasm_inner(db, package, &[], &[], None, None, None, &[], false, false)
}

/// Overlay-only callback-capstone entry point. The default pin cannot name the
/// typed indirect-call instructions used here, so normal builds do not compile
/// this surface.
#[cfg(feature = "sonatina-indirect-calls")]
pub fn compile_runtime_package_wasm_with_guest_callbacks(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    callbacks: &[crate::guest_callbacks::ResolvedGuestCallback],
) -> Result<(Module, HashMap<String, String>), LowerError> {
    let isa = create_wasm32_isa();
    let builder = ModuleBuilder::new(ModuleCtx::new(&isa));
    let mut lowerer =
        PortableModuleLowerer::new(db, builder, &isa, package, HashSet::new(), &[], true, true)?;
    lowerer.declare_functions()?;
    lowerer.lower_bodies()?;
    lowerer.synthesize_guest_callbacks(callbacks)?;
    let import_modules = lowerer.import_modules();
    Ok((lowerer.finish(), import_modules))
}

pub(crate) fn compile_runtime_package_wasm_with_canonical_lanes(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    canonical_lanes: &[crate::CanonicalLane],
    export_aliases: &[(String, String)],
    resident_transition: Option<&super::WasmResidentTransition>,
    resident_initializer: Option<&super::WasmResidentInitializer>,
    resident_projection: Option<&super::WasmResidentProjection>,
    resident_policies: &[super::WasmResidentPolicy],
) -> Result<(Module, HashMap<String, String>), LowerError> {
    compile_runtime_package_wasm_inner(
        db,
        package,
        canonical_lanes,
        export_aliases,
        resident_transition,
        resident_initializer,
        resident_projection,
        resident_policies,
        true,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_runtime_package_wasm_inner(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    canonical_lanes: &[crate::CanonicalLane],
    export_aliases: &[(String, String)],
    resident_transition: Option<&super::WasmResidentTransition>,
    resident_initializer: Option<&super::WasmResidentInitializer>,
    resident_projection: Option<&super::WasmResidentProjection>,
    resident_policies: &[super::WasmResidentPolicy],
    validate_host_enum_params: bool,
    enable_scoped_arena: bool,
) -> Result<(Module, HashMap<String, String>), LowerError> {
    wasm_lower_trace(|| {
        format!(
            "begin runtime package, functions={}",
            package.functions(db).len(),
        )
    });
    // Reject unsupported indirect host results before constructing any
    // Sonatina signatures. A local wrapper may itself return the authored enum
    // and appear before the import in package order, so gating inside
    // `declare_functions` is already too late.
    for function in package.functions(db) {
        let instance = function.instance(db);
        let Some(name) = mir::host_import_name(db, instance) else {
            continue;
        };
        let Some(descriptor) = mir::indirect_host_result(db, instance) else {
            continue;
        };
        let mut missing = Vec::new();
        if descriptor.requires_realloc {
            missing.push("realloc");
        }
        if descriptor.requires_post_return {
            missing.push("post-return");
        }
        if !missing.is_empty() {
            return Err(LowerError::Unsupported(format!(
                "extern host import `{name}` uses indirect host result codec `{}`, but the Wasm \
                 backend is missing required capabilities: {}",
                mir::IndirectHostResult::FE_HOST_WASM_PROTOCOL,
                missing.join(", ")
            )));
        }
    }

    // CONSULT (DispatchKind axis): the wasm target realizes the `Export` kind.
    // Every entry (`main`, the `fe_task` task table, the degraded-mode
    // `on_ready` continuation) is a named export the host invokes directly, with
    // no in-band selector and no synthesized dispatch root. This names what this
    // lowering already does; a mismatch fires in debug, zero effect in release.
    debug_assert!(
        {
            let kind = crate::dispatch::DispatchKind::for_backend(crate::BackendKind::Wasm);
            matches!(kind, crate::dispatch::DispatchKind::Export) && kind.entries_invoked_directly()
        },
        "wasm lowering must realize the Export DispatchKind (entries invoked directly)"
    );
    let isa = create_wasm32_isa();
    let builder = ModuleBuilder::new(ModuleCtx::new(&isa));
    let mut wrapped_lane_names: HashSet<String> = canonical_lanes
        .iter()
        .map(|lane| lane.name.clone())
        .collect();
    if let Some(transition) = resident_transition {
        wrapped_lane_names.insert(transition.source.clone());
    }
    if let Some(initializer) = resident_initializer {
        wrapped_lane_names.insert(initializer.source.clone());
    }
    if let Some(projection) = resident_projection {
        wrapped_lane_names.insert(projection.source.clone());
    }
    for policy in resident_policies {
        let assigned = assign_sonatina_function_symbols(db, package);
        let policy_symbols = assigned
            .into_iter()
            .filter_map(|(instance, symbol)| {
                (mir::runtime_instance_symbol_key(db, instance) == policy.callee_instance_key)
                    .then_some(symbol)
            })
            .collect::<Vec<_>>();
        let [symbol] = policy_symbols.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident policy instance `{}` must select exactly one assigned Fe symbol (found {})",
                policy.callee_instance_key,
                policy_symbols.len()
            )));
        };
        wrapped_lane_names.insert(symbol.clone());
    }
    let mut lowerer = PortableModuleLowerer::new(
        db,
        builder,
        &isa,
        package,
        wrapped_lane_names,
        export_aliases,
        validate_host_enum_params,
        enable_scoped_arena,
    )?;
    wasm_lower_trace(|| "prepared portable runtime bodies".to_owned());
    lowerer.declare_functions()?;
    wasm_lower_trace(|| "declared portable runtime functions".to_owned());
    lowerer.lower_bodies()?;
    wasm_lower_trace(|| "lowered portable runtime function bodies".to_owned());
    for lane in canonical_lanes {
        lowerer.synthesize_canonical_lane(lane)?;
    }
    if let Some(transition) = resident_transition {
        lowerer.synthesize_resident_transition(
            transition,
            resident_initializer,
            resident_projection,
        )?;
    } else if resident_initializer.is_some() || resident_projection.is_some() {
        return Err(LowerError::Unsupported(
            "resident actor initializer/projection requires a resident transition".to_owned(),
        ));
    }
    for (policy_index, policy) in resident_policies.iter().enumerate() {
        lowerer.synthesize_resident_policy(policy, policy_index)?;
    }
    let import_modules = lowerer.import_modules();
    wasm_lower_trace(|| "finished portable Sonatina module".to_owned());
    Ok((lowerer.finish(), import_modules))
}

/// Change 5: whether the lowered module emits any `MemAllocDynamic`. The default
/// `BackendKind::Wasm` driver must build with `.with_canonical_arena()` iff so
/// (`MemAllocDynamic` lowers to the arena's `fe_cabi_alloc`, which only exists
/// when the arena is enabled); a module with no dynamic allocation stays
/// byte-identical to the pre-arena default (preserving the canonical-arena
/// opt-in assertions). Scans every function's instructions, downcasting to
/// `MemAllocDynamic` against each function's own instruction set.
pub(crate) fn module_emits_dynamic_alloc(module: &Module) -> bool {
    use sonatina_ir::InstDowncast;
    module.funcs().into_iter().any(|func_ref| {
        module
            .func_store
            .try_view(func_ref, |function| {
                let inst_set = function.inst_set();
                function.layout.iter_block().any(|block| {
                    function.layout.iter_inst(block).any(|inst_id| {
                        let inst_data = function.dfg.inst(inst_id);
                        <&MemAllocDynamic as InstDowncast>::downcast(inst_set, inst_data).is_some()
                    })
                })
            })
            .unwrap_or(false)
    })
}

fn fieldless_tag_immediate(ty: Type, tag: u32) -> Option<Immediate> {
    match ty {
        Type::I1 if tag <= 1 => Some(Immediate::I1(tag != 0)),
        Type::I8 => Some(Immediate::I8(tag as u8 as i8)),
        Type::I16 => Some(Immediate::I16(tag as u16 as i16)),
        Type::I32 => Some(Immediate::I32(tag as i32)),
        Type::I64 => Some(Immediate::I64(i64::from(tag))),
        _ => None,
    }
}

fn zero_immediate(ty: Type) -> Option<Immediate> {
    match ty {
        Type::I1 => Some(Immediate::I1(false)),
        Type::I8 => Some(Immediate::I8(0)),
        Type::I16 => Some(Immediate::I16(0)),
        Type::I32 => Some(Immediate::I32(0)),
        Type::I64 => Some(Immediate::I64(0)),
        Type::F32 => Some(Immediate::F32(0)),
        _ => None,
    }
}

/// A recursively scalar canonical value, viewed from its wasm32 memory layout.
///
/// Canonical variants are tagged unions in memory, but Fe payload enums use a
/// value ABI of `tag + every variant payload lane`. Keeping that distinction in
/// one compiler-owned plan lets request lowering load only the active union
/// member and fill inactive value lanes with canonical zeros. Response lowering
/// performs the inverse operation and stores only the active union member.
#[derive(Clone, Debug)]
enum CanonicalScalarValuePlan {
    Scalar {
        offset: u32,
        ty: Type,
    },
    Record(Vec<CanonicalScalarValuePlan>),
    Variant {
        tag_offset: u32,
        variants: Vec<Vec<CanonicalScalarValuePlan>>,
    },
}

impl CanonicalScalarValuePlan {
    fn append_flat_types(&self, output: &mut Vec<Type>) {
        match self {
            Self::Scalar { ty, .. } => output.push(*ty),
            Self::Record(fields) => {
                for field in fields {
                    field.append_flat_types(output);
                }
            }
            Self::Variant { variants, .. } => {
                output.push(Type::I32);
                for variant in variants {
                    for field in variant {
                        field.append_flat_types(output);
                    }
                }
            }
        }
    }

    fn flat_width(&self) -> usize {
        match self {
            Self::Scalar { .. } => 1,
            Self::Record(fields) => fields.iter().map(Self::flat_width).sum(),
            Self::Variant { variants, .. } => {
                1 + variants
                    .iter()
                    .flatten()
                    .map(Self::flat_width)
                    .sum::<usize>()
            }
        }
    }
}

fn canonical_layout_contains_variant(layout: &crate::CanonicalLayout) -> bool {
    match &layout.shape {
        crate::CanonicalShape::Record { fields } => fields
            .iter()
            .any(|field| canonical_layout_contains_variant(&field.layout)),
        crate::CanonicalShape::Variant { .. } => true,
        crate::CanonicalShape::Bool
        | crate::CanonicalShape::U8
        | crate::CanonicalShape::I32
        | crate::CanonicalShape::U32
        | crate::CanonicalShape::I64
        | crate::CanonicalShape::U64
        | crate::CanonicalShape::F32
        | crate::CanonicalShape::Bytes { .. }
        | crate::CanonicalShape::String { .. }
        | crate::CanonicalShape::List { .. } => false,
    }
}

fn canonical_scalar_value_plan(
    layout: &crate::CanonicalLayout,
    base: u32,
    path: &str,
) -> Result<CanonicalScalarValuePlan, LowerError> {
    use crate::CanonicalShape;
    let scalar = |ty| CanonicalScalarValuePlan::Scalar { offset: base, ty };
    Ok(match &layout.shape {
        CanonicalShape::Bool => scalar(Type::I1),
        CanonicalShape::U8 => scalar(Type::I8),
        CanonicalShape::I32 | CanonicalShape::U32 => scalar(Type::I32),
        CanonicalShape::I64 | CanonicalShape::U64 => scalar(Type::I64),
        CanonicalShape::F32 => scalar(Type::F32),
        CanonicalShape::Record { fields } => {
            let mut plans = Vec::with_capacity(fields.len());
            for field in fields {
                let offset = base.checked_add(field.offset).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "canonical scalar record offset overflow at `{path}.{}`",
                        field.name
                    ))
                })?;
                plans.push(canonical_scalar_value_plan(
                    &field.layout,
                    offset,
                    &format!("{path}.{}", field.name),
                )?);
            }
            CanonicalScalarValuePlan::Record(plans)
        }
        CanonicalShape::Variant {
            tag_offset,
            variants,
        } => {
            let tag_offset = base.checked_add(*tag_offset).ok_or_else(|| {
                LowerError::Unsupported(format!(
                    "canonical scalar variant tag offset overflow at `{path}`"
                ))
            })?;
            let mut plans = Vec::with_capacity(variants.len());
            for (expected_tag, variant) in variants.iter().enumerate() {
                if variant.tag != expected_tag as u32 {
                    return Err(LowerError::Unsupported(format!(
                        "canonical scalar variant `{path}` has non-contiguous tag {}",
                        variant.tag
                    )));
                }
                let mut fields = Vec::with_capacity(variant.fields.len());
                for field in &variant.fields {
                    let offset = base.checked_add(field.offset).ok_or_else(|| {
                        LowerError::Unsupported(format!(
                            "canonical scalar variant offset overflow at `{path}.{}.{}`",
                            variant.name, field.name
                        ))
                    })?;
                    fields.push(canonical_scalar_value_plan(
                        &field.layout,
                        offset,
                        &format!("{path}.{}.{}", variant.name, field.name),
                    )?);
                }
                plans.push(fields);
            }
            if plans.is_empty() {
                return Err(LowerError::Unsupported(format!(
                    "canonical scalar variant `{path}` has no cases"
                )));
            }
            CanonicalScalarValuePlan::Variant {
                tag_offset,
                variants: plans,
            }
        }
        CanonicalShape::Bytes { .. }
        | CanonicalShape::String { .. }
        | CanonicalShape::List { .. } => {
            return Err(LowerError::Unsupported(format!(
                "canonical scalar variant tree `{path}` contains a memory descriptor; bytes, strings, and lists require the variant post-return bridge"
            )));
        }
    })
}

fn canonical_offset_address(
    fb: &mut FunctionBuilder<InstInserter>,
    is: &NativeInstSet,
    base: ValueId,
    offset: u32,
) -> ValueId {
    if offset == 0 {
        base
    } else {
        let offset = fb.make_imm_value(Immediate::I32(offset as i32));
        fb.insert_inst(Add::new(is, base, offset), Type::I32)
    }
}

fn append_canonical_zero_values(
    fb: &mut FunctionBuilder<InstInserter>,
    plan: &CanonicalScalarValuePlan,
    output: &mut Vec<ValueId>,
) -> Result<(), LowerError> {
    let mut types = Vec::new();
    plan.append_flat_types(&mut types);
    for ty in types {
        let immediate = zero_immediate(ty).ok_or_else(|| {
            LowerError::Internal(format!("canonical scalar variant has no zero for `{ty:?}`"))
        })?;
        output.push(fb.make_imm_value(immediate));
    }
    Ok(())
}

fn load_canonical_scalar_value(
    fb: &mut FunctionBuilder<InstInserter>,
    is: &NativeInstSet,
    base: ValueId,
    plan: &CanonicalScalarValuePlan,
) -> Result<Vec<ValueId>, LowerError> {
    match plan {
        CanonicalScalarValuePlan::Scalar { offset, ty } => {
            let address = canonical_offset_address(fb, is, base, *offset);
            Ok(vec![fb.insert_inst(Mload::new(is, address, *ty), *ty)])
        }
        CanonicalScalarValuePlan::Record(fields) => {
            let mut values = Vec::new();
            for field in fields {
                values.extend(load_canonical_scalar_value(fb, is, base, field)?);
            }
            Ok(values)
        }
        CanonicalScalarValuePlan::Variant {
            tag_offset,
            variants,
        } => {
            let tag_address = canonical_offset_address(fb, is, base, *tag_offset);
            let tag = fb.insert_inst(Mload::new(is, tag_address, Type::I32), Type::I32);
            let merge = fb.append_block();
            let invalid = fb.append_block();
            let mut incoming = Vec::<(BlockId, Vec<ValueId>)>::with_capacity(variants.len());

            for active_index in 0..variants.len() {
                let active_block = fb.append_block();
                let next = if active_index + 1 == variants.len() {
                    invalid
                } else {
                    fb.append_block()
                };
                let expected = fb.make_imm_value(Immediate::I32(active_index as i32));
                let matches = fb.insert_inst(CmpEq::new(is, tag, expected), Type::I1);
                fb.insert_inst_no_result(Br::new(is, matches, active_block, next));

                fb.switch_to_block(active_block);
                let mut values = vec![tag];
                for (variant_index, fields) in variants.iter().enumerate() {
                    for field in fields {
                        if variant_index == active_index {
                            values.extend(load_canonical_scalar_value(fb, is, base, field)?);
                        } else {
                            append_canonical_zero_values(fb, field, &mut values)?;
                        }
                    }
                }
                let predecessor = fb.current_block().ok_or_else(|| {
                    LowerError::Internal(
                        "canonical scalar variant lost its active block".to_owned(),
                    )
                })?;
                fb.insert_inst_no_result(Jump::new(is, merge));
                incoming.push((predecessor, values));
                fb.switch_to_block(next);
            }

            fb.insert_inst_no_result(Unreachable::new(is));
            fb.switch_to_block(merge);
            let mut types = Vec::new();
            plan.append_flat_types(&mut types);
            let mut values = Vec::with_capacity(types.len());
            for (lane, ty) in types.into_iter().enumerate() {
                let incoming = incoming
                    .iter()
                    .map(|(block, values)| (values[lane], *block))
                    .collect();
                values.push(fb.insert_inst(Phi::new(is, incoming), ty));
            }
            Ok(values)
        }
    }
}

fn store_canonical_scalar_value(
    fb: &mut FunctionBuilder<InstInserter>,
    is: &NativeInstSet,
    base: ValueId,
    plan: &CanonicalScalarValuePlan,
    values: &[ValueId],
    cursor: &mut usize,
) -> Result<(), LowerError> {
    match plan {
        CanonicalScalarValuePlan::Scalar { offset, ty } => {
            let value = values.get(*cursor).copied().ok_or_else(|| {
                LowerError::Internal(
                    "canonical scalar response has fewer lanes than its plan".to_owned(),
                )
            })?;
            *cursor += 1;
            let address = canonical_offset_address(fb, is, base, *offset);
            fb.insert_inst_no_result(Mstore::new(is, address, value, *ty));
        }
        CanonicalScalarValuePlan::Record(fields) => {
            for field in fields {
                store_canonical_scalar_value(fb, is, base, field, values, cursor)?;
            }
        }
        CanonicalScalarValuePlan::Variant {
            tag_offset,
            variants,
        } => {
            let tag = values.get(*cursor).copied().ok_or_else(|| {
                LowerError::Internal("canonical scalar response has no variant tag lane".to_owned())
            })?;
            *cursor += 1;
            let mut ranges = Vec::with_capacity(variants.len());
            for fields in variants {
                let start = *cursor;
                for field in fields {
                    *cursor = (*cursor).checked_add(field.flat_width()).ok_or_else(|| {
                        LowerError::Internal(
                            "canonical scalar response lane count overflow".to_owned(),
                        )
                    })?;
                }
                ranges.push((start, *cursor));
            }

            let merge = fb.append_block();
            let invalid = fb.append_block();
            for (active_index, fields) in variants.iter().enumerate() {
                let active_block = fb.append_block();
                let next = if active_index + 1 == variants.len() {
                    invalid
                } else {
                    fb.append_block()
                };
                let expected = fb.make_imm_value(Immediate::I32(active_index as i32));
                let matches = fb.insert_inst(CmpEq::new(is, tag, expected), Type::I1);
                fb.insert_inst_no_result(Br::new(is, matches, active_block, next));

                fb.switch_to_block(active_block);
                let tag_address = canonical_offset_address(fb, is, base, *tag_offset);
                fb.insert_inst_no_result(Mstore::new(is, tag_address, tag, Type::I32));
                let mut active_cursor = ranges[active_index].0;
                for field in fields {
                    store_canonical_scalar_value(fb, is, base, field, values, &mut active_cursor)?;
                }
                if active_cursor != ranges[active_index].1 {
                    return Err(LowerError::Internal(
                        "canonical scalar response variant width drifted".to_owned(),
                    ));
                }
                fb.insert_inst_no_result(Jump::new(is, merge));
                fb.switch_to_block(next);
            }
            fb.insert_inst_no_result(Unreachable::new(is));
            fb.switch_to_block(merge);
        }
    }
    Ok(())
}

fn zero_canonical_memory(
    fb: &mut FunctionBuilder<InstInserter>,
    is: &NativeInstSet,
    base: ValueId,
    byte_len: u32,
) -> Result<(), LowerError> {
    let byte_len = i32::try_from(byte_len).map_err(|_| {
        LowerError::Unsupported(
            "canonical variant response exceeds the Wasm i32 memory bound".to_owned(),
        )
    })?;
    let entry = fb.current_block().ok_or_else(|| {
        LowerError::Internal("canonical response zeroing has no entry block".to_owned())
    })?;
    let header = fb.append_block();
    let body = fb.append_block();
    let done = fb.append_block();
    fb.insert_inst_no_result(Jump::new(is, header));

    fb.switch_to_block(header);
    let zero = fb.make_imm_value(Immediate::I32(0));
    let index = fb.insert_inst(Phi::new(is, vec![(zero, entry)]), Type::I32);
    let limit = fb.make_imm_value(Immediate::I32(byte_len));
    let more = fb.insert_inst(Lt::new(is, index, limit), Type::I1);
    fb.insert_inst_no_result(Br::new(is, more, body, done));

    fb.switch_to_block(body);
    let address = fb.insert_inst(Add::new(is, base, index), Type::I32);
    let zero_byte = fb.make_imm_value(Immediate::I8(0));
    fb.insert_inst_no_result(Mstore::new(is, address, zero_byte, Type::I8));
    let one = fb.make_imm_value(Immediate::I32(1));
    let next = fb.insert_inst(Add::new(is, index, one), Type::I32);
    let back = fb.current_block().ok_or_else(|| {
        LowerError::Internal("canonical response zeroing lost its loop body".to_owned())
    })?;
    fb.append_phi_arg(index, next, back);
    fb.insert_inst_no_result(Jump::new(is, header));
    fb.switch_to_block(done);
    Ok(())
}

fn all_ones_immediate(ty: Type) -> Option<Immediate> {
    match ty {
        Type::I8 => Some(Immediate::I8(-1)),
        Type::I16 => Some(Immediate::I16(-1)),
        Type::I32 => Some(Immediate::I32(-1)),
        Type::I64 => Some(Immediate::I64(-1)),
        _ => None,
    }
}

/// Lower through the same fail-closed portable instruction vocabulary as Wasm,
/// but attach the host-native data layout required by Cranelift.
#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) fn compile_runtime_package_native(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
) -> Result<Module, LowerError> {
    use sonatina_ir::isa::native::Native;

    let architecture = if cfg!(target_arch = "x86_64") {
        Architecture::X86_64
    } else {
        Architecture::Aarch64
    };
    let isa = Native::new(TargetTriple::new(
        architecture,
        Vendor::Unknown,
        OperatingSystem::Native,
    ));
    let builder = ModuleBuilder::new(ModuleCtx::new(&isa));
    let mut lowerer =
        PortableModuleLowerer::new(db, builder, &isa, package, HashSet::new(), &[], true, false)?;
    lowerer.declare_functions()?;
    lowerer.lower_bodies()?;
    Ok(lowerer.finish())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FlatShape {
    Leaf(Type),
    Product(Vec<FlatShape>),
}

impl FlatShape {
    fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Product(fields) => fields.iter().map(Self::leaf_count).sum(),
        }
    }

    fn leaf_types(&self, out: &mut Vec<Type>) {
        match self {
            Self::Leaf(ty) => out.push(*ty),
            Self::Product(fields) => fields.iter().for_each(|field| field.leaf_types(out)),
        }
    }

    fn field_range(&self, index: usize) -> Option<(usize, usize, &FlatShape)> {
        let Self::Product(fields) = self else {
            return None;
        };
        let field = fields.get(index)?;
        let start = fields[..index].iter().map(Self::leaf_count).sum();
        Some((start, start + field.leaf_count(), field))
    }
}

// One accepted fixed-signature Cl(4,1) MvT<5> product expands to 29,118 MIR
// statements (including structural extracts/rebuilds). A conformal sandwich
// composes exactly two of those expansions, 58,236 statements. Keep a hard
// injected-expansion cap at the next power of two so larger helper explosions
// fail shut. Original caller statements are deliberately not charged here.
const INLINE_VALUE_STMT_BUDGET: usize = 65_536;
const INLINE_SPECIALIZATION_CACHE_LIMIT: usize = 256;

/// Prepare the body view shared by direct Wasm lowering and Render's Wasm to
/// SPIR-V translation. This intentionally handles only value-only leaf helpers;
/// every other call remains visible to the normal fail-closed lowering. Fresh
/// locals exist only in this backend overlay, so `semantic_locals` remains the
/// canonical source-level mapping and is deliberately not extended.
struct PreparedInlineBodies<'db> {
    bodies: FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
    #[cfg(test)]
    residuals: FxHashMap<RuntimeInstance<'db>, (usize, usize)>,
    #[cfg(test)]
    unspecialized_preparations: usize,
}

/// Materialize const-backed aggregate handles in the backend's private body
/// overlay. MIR deliberately preserves these handles for EVM const-data
/// lowering; the value-only Wasm/SPIR-V path instead needs the same immutable
/// data expressed as scalar leaves and `AggregateMake`s.
fn reify_inline_const_aggregates<'db>(db: &'db DriverDataBase, body: &mut RuntimeBody<'db>) {
    fn field_classes<'db>(
        db: &'db DriverDataBase,
        layout: LayoutId<'db>,
    ) -> Option<Vec<RuntimeClass<'db>>> {
        match layout.data(db) {
            Layout::Struct(data) => Some(data.fields.to_vec()),
            Layout::Array(data) => Some(vec![data.elem.clone(); data.len as usize]),
            Layout::Enum(_) => None,
        }
    }

    fn emit<'db>(
        db: &'db DriverDataBase,
        locals: &mut Vec<RLocal<'db>>,
        stmts: &mut Vec<RStmt<'db>>,
        dst: RLocalId,
        node: &ConstNode<'db>,
        class: RuntimeClass<'db>,
    ) -> Option<()> {
        match (node, class) {
            (ConstNode::Scalar(value), RuntimeClass::Scalar(class)) => {
                locals[dst.as_u32() as usize].carrier =
                    RuntimeCarrier::Value(RuntimeClass::Scalar(class));
                locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                stmts.push(RStmt::Assign {
                    dst,
                    expr: RExpr::ConstScalar(value.clone()),
                });
            }
            (ConstNode::Aggregate { layout, fields }, RuntimeClass::AggregateValue { .. }) => {
                let classes = field_classes(db, *layout)?;
                if classes.len() != fields.len() {
                    return None;
                }
                let semantic_ty = locals[dst.as_u32() as usize].semantic_ty;
                let mut values = Vec::with_capacity(fields.len());
                for (field, class) in fields.iter().zip(classes) {
                    let value = RLocalId::from_u32(locals.len() as u32);
                    locals.push(RLocal {
                        // Backend-overlay locals have no source-level identity.
                        // Preserve the aggregate constant's provenance type;
                        // lowering and specialization intentionally use only
                        // the exact runtime carrier attached below.
                        semantic_ty,
                        carrier: RuntimeCarrier::Value(class.clone()),
                        root: RuntimeLocalRoot::None,
                    });
                    emit(db, locals, stmts, value, field, class)?;
                    values.push(value);
                }
                locals[dst.as_u32() as usize].carrier =
                    RuntimeCarrier::Value(RuntimeClass::AggregateValue { layout: *layout });
                locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                stmts.push(RStmt::Assign {
                    dst,
                    expr: RExpr::AggregateMake {
                        layout: *layout,
                        fields: values.into_boxed_slice(),
                    },
                });
            }
            _ => return None,
        }
        Some(())
    }

    fn ref_root(place: &RuntimePlace<'_>) -> Option<RLocalId> {
        match place.root {
            PlaceRoot::Ref(root) => Some(root),
            PlaceRoot::Slot(_) | PlaceRoot::Ptr { .. } | PlaceRoot::Provider(_) => None,
        }
    }

    fn projected_ref_root(place: &RuntimePlace<'_>) -> Option<RLocalId> {
        (!place.path.is_empty()).then(|| ref_root(place)).flatten()
    }

    fn projected_roots_in_expr(expr: &RExpr<'_>, roots: &mut HashSet<RLocalId>) {
        let root = match expr {
            // Address formation needs storage even for the whole value. Once a
            // const handle is reified as scalar leaves, `addr_of *value` must
            // target the private materialized copy rather than the now-value
            // carrier.
            RExpr::MaterializePlaceToObject { place } | RExpr::AddrOf { place } => ref_root(place),
            // A whole-value load can remain an ordinary aggregate value copy.
            // Only projections require addressable target-layout storage.
            RExpr::Load { place } => projected_ref_root(place),
            _ => None,
        };
        if let Some(root) = root {
            roots.insert(root);
        }
    }

    fn rewrite_projected_place(
        place: &mut RuntimePlace<'_>,
        addressable: &FxHashMap<RLocalId, RLocalId>,
    ) {
        let Some(root) = projected_ref_root(place) else {
            return;
        };
        if let Some(object) = addressable.get(&root) {
            place.root = PlaceRoot::Ref(*object);
        }
    }

    fn rewrite_addressed_place(
        place: &mut RuntimePlace<'_>,
        addressable: &FxHashMap<RLocalId, RLocalId>,
    ) {
        let Some(root) = ref_root(place) else {
            return;
        };
        if let Some(object) = addressable.get(&root) {
            place.root = PlaceRoot::Ref(*object);
        }
    }

    fn rewrite_projected_expr(expr: &mut RExpr<'_>, addressable: &FxHashMap<RLocalId, RLocalId>) {
        match expr {
            RExpr::MaterializePlaceToObject { place } | RExpr::AddrOf { place } => {
                rewrite_addressed_place(place, addressable)
            }
            RExpr::Load { place } => rewrite_projected_place(place, addressable),
            _ => {}
        }
    }

    let mut const_origin_sets = body
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            RStmt::Assign {
                dst,
                expr: RExpr::ConstRef { .. },
            } => Some((*dst, HashSet::from([*dst]))),
            _ => None,
        })
        .collect::<FxHashMap<_, _>>();
    loop {
        let mut changed = false;
        for block in &body.blocks {
            for stmt in &block.stmts {
                let RStmt::Assign { dst, expr } = stmt else {
                    continue;
                };
                let src = match expr {
                    RExpr::Use(src) | RExpr::RetagRef { value: src } => src,
                    _ => continue,
                };
                if let Some(origins) = const_origin_sets.get(src).cloned() {
                    let destination = const_origin_sets.entry(*dst).or_default();
                    let previous_len = destination.len();
                    destination.extend(origins);
                    changed |= destination.len() != previous_len;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let const_origins = const_origin_sets
        .into_iter()
        .filter_map(|(alias, origins)| {
            (origins.len() == 1).then(|| (alias, *origins.iter().next().unwrap()))
        })
        .collect::<FxHashMap<_, _>>();
    let mut projected_const_roots = HashSet::new();
    for block in &body.blocks {
        for stmt in &block.stmts {
            match stmt {
                RStmt::Assign { expr, .. } => {
                    let mut roots = HashSet::new();
                    projected_roots_in_expr(expr, &mut roots);
                    for root in roots {
                        if let Some(origin) = const_origins.get(&root) {
                            projected_const_roots.insert(*origin);
                        }
                    }
                }
                RStmt::Store { dst, .. } | RStmt::CopyInto { dst, .. } => {
                    if let Some(root) = projected_ref_root(dst)
                        && let Some(origin) = const_origins.get(&root)
                    {
                        projected_const_roots.insert(*origin);
                    }
                }
                RStmt::EnumAssertVariant { .. }
                | RStmt::EnumSetTag { .. }
                | RStmt::EnumWriteVariant { .. } => {}
            }
        }
    }

    // Reify every const handle whose ConstNode `emit` can expand into scalar
    // leaves + AggregateMake (structs and arrays; enums return None below and
    // stay fail-closed). This was formerly gated to const refs consumed by a
    // whole-value `Load`, but dec's `slots_filled(0.0)` cochain seeds are
    // consumed as a call receiver and as an AggregateMake field, never a
    // whole-value Load, so that gate left them as `Ref{Const, AggregateValue}`
    // and `ty_for_class` rejected them. Expanding unconditionally is sound: a
    // reifiable const aggregate IS a value. A projected consumer additionally
    // receives one arena-backed copy, and only its places are redirected to
    // that private object. Ordinary value consumers keep the flattened value,
    // so materialization neither introduces aliasing nor changes call shapes.
    let mut addressable = FxHashMap::default();
    let (locals, blocks) = (&mut body.locals, &mut body.blocks);
    for block in blocks.iter_mut() {
        let mut rewritten = Vec::with_capacity(block.stmts.len());
        for stmt in std::mem::take(&mut block.stmts) {
            if let RStmt::Assign {
                dst,
                expr: RExpr::ConstRef { region, layout },
            } = &stmt
            {
                let node = region.value(db);
                if emit(
                    db,
                    locals,
                    &mut rewritten,
                    *dst,
                    &node,
                    RuntimeClass::AggregateValue { layout: *layout },
                )
                .is_some()
                {
                    if projected_const_roots.contains(dst) {
                        let object = RLocalId::from_u32(locals.len() as u32);
                        let semantic_ty = locals[dst.as_u32() as usize].semantic_ty;
                        locals.push(RLocal {
                            semantic_ty,
                            carrier: RuntimeCarrier::Value(RuntimeClass::object_ref(*layout)),
                            root: RuntimeLocalRoot::None,
                        });
                        rewritten.push(RStmt::Assign {
                            dst: object,
                            expr: RExpr::MaterializeToObject { src: *dst },
                        });
                        addressable.insert(*dst, object);
                    }
                    continue;
                }
            }
            if let RStmt::Assign {
                dst,
                expr: RExpr::Use(src),
            } = &stmt
                && let Some(RuntimeClass::AggregateValue { layout }) =
                    locals[src.as_u32() as usize].carrier.value_class().cloned()
                && matches!(
                    locals[dst.as_u32() as usize].carrier.value_class(),
                    Some(RuntimeClass::Ref {
                        kind: RefKind::Const,
                        ..
                    })
                )
            {
                locals[dst.as_u32() as usize].carrier =
                    RuntimeCarrier::Value(RuntimeClass::AggregateValue { layout });
                locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
            }
            if let RStmt::Assign {
                dst,
                expr:
                    RExpr::Load {
                        place:
                            RuntimePlace {
                                root: PlaceRoot::Ref(src),
                                path,
                            },
                    },
            } = &stmt
                && path.is_empty()
                && let Some(RuntimeClass::AggregateValue { layout }) =
                    locals[src.as_u32() as usize].carrier.value_class().cloned()
            {
                locals[dst.as_u32() as usize].carrier =
                    RuntimeCarrier::Value(RuntimeClass::AggregateValue { layout });
                locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                rewritten.push(RStmt::Assign {
                    dst: *dst,
                    expr: RExpr::Use(*src),
                });
                continue;
            }
            rewritten.push(stmt);
        }
        block.stmts = rewritten;
    }
    let alias_objects = const_origins
        .iter()
        .filter_map(|(alias, origin)| {
            addressable
                .get(origin)
                .copied()
                .map(|object| (*alias, object))
        })
        .collect::<Vec<_>>();
    addressable.extend(alias_objects);
    for block in blocks.iter_mut() {
        for stmt in &mut block.stmts {
            match stmt {
                RStmt::Assign { expr, .. } => rewrite_projected_expr(expr, &addressable),
                RStmt::Store { dst, .. } | RStmt::CopyInto { dst, .. } => {
                    rewrite_projected_place(dst, &addressable)
                }
                RStmt::EnumAssertVariant { .. }
                | RStmt::EnumSetTag { .. }
                | RStmt::EnumWriteVariant { .. } => {}
            }
        }
    }
    if let Some(ret) = body.blocks.iter().find_map(|block| match block.terminator {
        RTerminator::Return(Some(value)) => Some(value),
        _ => None,
    }) && matches!(
        body.signature.ret,
        Some(RuntimeClass::Ref {
            kind: RefKind::Const,
            ..
        })
    ) {
        body.signature.ret = body.locals[ret.as_u32() as usize]
            .carrier
            .value_class()
            .cloned();
    }
}

fn prepare_inline_value_bodies<'db>(
    db: &'db DriverDataBase,
    package: &RuntimePackage<'db>,
) -> PreparedInlineBodies<'db> {
    fn visit<'db>(
        db: &'db DriverDataBase,
        package: &RuntimePackage<'db>,
        instance: RuntimeInstance<'db>,
        arg_shape: mir::RuntimeArgShapeKey,
        visiting: &mut HashSet<(RuntimeInstance<'db>, mir::RuntimeArgShapeKey)>,
        base_done: &mut FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
        specialized_done: &mut FxHashMap<
            (RuntimeInstance<'db>, mir::RuntimeArgShapeKey),
            RuntimeBody<'db>,
        >,
        specialization_work: &mut usize,
        #[cfg(test)] residual_stmt_counts: &mut FxHashMap<RuntimeInstance<'db>, (usize, usize)>,
        #[cfg(test)] unspecialized_preparations: &mut usize,
    ) -> RuntimeBody<'db> {
        let cache_key = (instance, arg_shape);
        let specialized = cache_key.1.has_known_facts();
        if specialized {
            if let Some(body) = specialized_done.get(&cache_key) {
                return body.clone();
            }
        } else if let Some(body) = base_done.get(&instance) {
            return body.clone();
        }
        // Once the bounded amount of shape-specific work is exhausted, fail
        // closed to the already prepared base body instead of repeatedly
        // traversing new shapes. Base bodies have their own unbounded
        // one-per-instance cache; the limit applies only to additional
        // shape-specialized variants.
        if specialized {
            if *specialization_work >= INLINE_SPECIALIZATION_CACHE_LIMIT {
                if let Some(body) = base_done.get(&instance) {
                    return body.clone();
                }
                let mut body = instance.body(db);
                reify_inline_const_aggregates(db, &mut body);
                return body;
            }
            *specialization_work += 1;
        } else {
            #[cfg(test)]
            {
                *unspecialized_preparations += 1;
            }
        }
        let mut body = instance.body(db);
        reify_inline_const_aggregates(db, &mut body);
        if !visiting.insert(cache_key.clone()) {
            return body;
        }
        let (seed_aggregates, seed_constants, seed_stmts) =
            seed_parameter_facts(db, &mut body, &cache_key.1);
        let mut expanded = seed_stmts.len();
        if let Some(block) = body.blocks.first_mut() {
            block.stmts.splice(0..0, seed_stmts);
        }
        let (locals, blocks) = (&mut body.locals, &mut body.blocks);
        for block in blocks {
            let mut stmts = Vec::with_capacity(block.stmts.len());
            let mut aggregate_facts = seed_aggregates.clone();
            let mut scalar_constants = seed_constants.clone();
            for stmt in std::mem::take(&mut block.stmts) {
                let RStmt::Assign {
                    dst,
                    expr: RExpr::Call { callee, args },
                } = &stmt
                else {
                    record_value_fact(&stmt, &mut aggregate_facts, &mut scalar_constants);
                    stmts.push(stmt);
                    continue;
                };
                let callee_shape =
                    mir::runtime_arg_shape_key(args, &aggregate_facts, &scalar_constants);
                let callee_body = visit(
                    db,
                    package,
                    *callee,
                    callee_shape,
                    visiting,
                    base_done,
                    specialized_done,
                    specialization_work,
                    #[cfg(test)]
                    residual_stmt_counts,
                    #[cfg(test)]
                    unspecialized_preparations,
                );
                if let Some(RuntimeClass::AggregateValue { layout }) =
                    callee_body.signature.ret.clone()
                    && matches!(
                        locals[dst.as_u32() as usize].carrier.value_class(),
                        Some(RuntimeClass::Ref {
                            kind: RefKind::Const,
                            ..
                        })
                    )
                {
                    locals[dst.as_u32() as usize].carrier =
                        RuntimeCarrier::Value(RuntimeClass::AggregateValue { layout });
                    locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                }
                let Some(replacement) = inline_value_call(
                    db,
                    package,
                    locals,
                    *dst,
                    &callee_body,
                    args,
                    &aggregate_facts,
                    INLINE_VALUE_STMT_BUDGET.saturating_sub(expanded),
                ) else {
                    stmts.push(stmt);
                    continue;
                };
                #[cfg(test)]
                {
                    let counts = residual_stmt_counts.entry(instance).or_default();
                    counts.0 += replacement.stmts.len() + replacement.pruned;
                    counts.1 += replacement.stmts.len();
                }
                let mut replacement = replacement.stmts;
                expanded += replacement.len();
                for stmt in &replacement {
                    record_value_fact(stmt, &mut aggregate_facts, &mut scalar_constants);
                }
                stmts.append(&mut replacement);
            }
            block.stmts = stmts;
        }
        visiting.remove(&cache_key);
        if specialized {
            specialized_done.insert(cache_key, body.clone());
        } else {
            base_done.insert(instance, body.clone());
        }
        body
    }

    let mut visiting = HashSet::new();
    let mut base_done = FxHashMap::default();
    let mut specialized_done = FxHashMap::default();
    let mut specialization_work = 0usize;
    #[cfg(test)]
    let mut residual_stmt_counts = FxHashMap::default();
    #[cfg(test)]
    let mut unspecialized_preparations = 0;
    for function in package.functions(db) {
        let instance = function.instance(db);
        let params = instance.body(db).signature.params.len();
        let shape =
            mir::RuntimeArgShapeKey(vec![mir::RuntimeArgFact::Unknown; params].into_boxed_slice());
        visit(
            db,
            package,
            instance,
            shape,
            &mut visiting,
            &mut base_done,
            &mut specialized_done,
            &mut specialization_work,
            #[cfg(test)]
            &mut residual_stmt_counts,
            #[cfg(test)]
            &mut unspecialized_preparations,
        );
    }
    PreparedInlineBodies {
        bodies: base_done,
        #[cfg(test)]
        residuals: residual_stmt_counts,
        #[cfg(test)]
        unspecialized_preparations,
    }
}

fn seed_parameter_facts<'db>(
    db: &'db DriverDataBase,
    body: &mut RuntimeBody<'db>,
    shape: &mir::RuntimeArgShapeKey,
) -> (
    mir::RuntimeAggregateFacts,
    mir::RuntimeScalarConstFacts,
    Vec<RStmt<'db>>,
) {
    fn seed<'db>(
        db: &'db DriverDataBase,
        body: &mut RuntimeBody<'db>,
        local: RLocalId,
        class: &RuntimeClass<'db>,
        fact: &mir::RuntimeArgFact,
        aggregates: &mut mir::RuntimeAggregateFacts,
        constants: &mut mir::RuntimeScalarConstFacts,
        stmts: &mut Vec<RStmt<'db>>,
        budget: usize,
    ) -> Option<()> {
        match fact {
            mir::RuntimeArgFact::Unknown => Some(()),
            mir::RuntimeArgFact::ScalarConst(value) => {
                if !matches!(class, RuntimeClass::Scalar(_)) {
                    return None;
                }
                constants.insert(local, value.clone());
                Some(())
            }
            mir::RuntimeArgFact::Aggregate(field_facts) => {
                let RuntimeClass::AggregateValue { layout } = class else {
                    return None;
                };
                let field_classes: Box<[RuntimeClass<'db>]> = match layout.data(db) {
                    Layout::Struct(layout) => layout.fields,
                    Layout::Array(layout) => {
                        vec![layout.elem; layout.len as usize].into_boxed_slice()
                    }
                    Layout::Enum(_) => return None,
                };
                if field_classes.len() != field_facts.len() {
                    return None;
                }
                let template = body
                    .locals
                    .iter()
                    .enumerate()
                    .find(|(index, _)| RLocalId::from_u32(*index as u32) == local)?
                    .1
                    .clone();
                let semantic_ty = template
                    .semantic_ty
                    .as_view(db)
                    .unwrap_or(template.semantic_ty);
                let field_semantic_tys = match layout.data(db) {
                    Layout::Struct(_) => semantic_ty.field_types(db),
                    Layout::Array(layout) => {
                        let (_, args) = semantic_ty.decompose_ty_app(db);
                        let element = args.first().copied()?;
                        vec![element; layout.len as usize]
                    }
                    Layout::Enum(_) => return None,
                };
                if field_semantic_tys.len() != field_facts.len() {
                    return None;
                }
                let mut fields = Vec::with_capacity(field_facts.len());
                for (index, ((field_class, field_fact), field_semantic_ty)) in field_classes
                    .iter()
                    .zip(field_facts)
                    .zip(field_semantic_tys)
                    .enumerate()
                {
                    if stmts.len() >= budget {
                        return None;
                    }
                    let field = RLocalId::from_u32(body.locals.len() as u32);
                    body.locals.push(mir::RLocal {
                        semantic_ty: field_semantic_ty,
                        carrier: RuntimeCarrier::Value(field_class.clone()),
                        root: RuntimeLocalRoot::None,
                    });
                    fields.push(field);
                    stmts.push(RStmt::Assign {
                        dst: field,
                        expr: RExpr::AggregateExtract {
                            value: local,
                            index: index as u32,
                        },
                    });
                    seed(
                        db,
                        body,
                        field,
                        field_class,
                        field_fact,
                        aggregates,
                        constants,
                        stmts,
                        budget,
                    )?;
                }
                aggregates.insert(local, fields.into_boxed_slice());
                Some(())
            }
        }
    }

    let mut aggregates = mir::RuntimeAggregateFacts::default();
    let mut constants = mir::RuntimeScalarConstFacts::default();
    let mut stmts = Vec::new();
    if body.blocks.len() != 1 || body.signature.params.len() != shape.0.len() {
        return (aggregates, constants, stmts);
    }
    let params = body.signature.params.clone();
    let original_locals = body.locals.len();
    for (param, fact) in params.iter().zip(shape.0.iter()) {
        if seed(
            db,
            body,
            param.local,
            &param.class,
            fact,
            &mut aggregates,
            &mut constants,
            &mut stmts,
            INLINE_VALUE_STMT_BUDGET,
        )
        .is_none()
        {
            body.locals.truncate(original_locals);
            return (
                mir::RuntimeAggregateFacts::default(),
                mir::RuntimeScalarConstFacts::default(),
                Vec::new(),
            );
        }
    }
    (aggregates, constants, stmts)
}

fn record_value_fact(
    stmt: &RStmt<'_>,
    aggregates: &mut mir::RuntimeAggregateFacts,
    constants: &mut mir::RuntimeScalarConstFacts,
) {
    let RStmt::Assign { dst, expr } = stmt else {
        return;
    };
    match expr {
        RExpr::AggregateMake { fields, .. } => {
            aggregates.insert(*dst, fields.clone());
        }
        RExpr::ConstScalar(value) => {
            constants.insert(*dst, value.clone());
        }
        RExpr::Use(src) => {
            if let Some(fields) = aggregates.get(src).cloned() {
                aggregates.insert(*dst, fields);
            }
            if let Some(value) = constants.get(src).cloned() {
                constants.insert(*dst, value);
            }
        }
        _ => {}
    }
}

fn inline_value_call<'db>(
    db: &'db DriverDataBase,
    package: &RuntimePackage<'db>,
    caller_locals: &mut Vec<mir::RLocal<'db>>,
    dst: RLocalId,
    callee_body: &RuntimeBody<'db>,
    args: &[RLocalId],
    aggregate_facts: &mir::RuntimeAggregateFacts,
    budget: usize,
) -> Option<InlineValueCall<'db>> {
    let function = package
        .functions(db)
        .into_iter()
        .find(|function| function.instance(db) == callee_body.owner)?;
    if !matches!(
        function.linkage(db),
        RuntimeLinkage::Private | RuntimeLinkage::Internal
    ) || function.inline_hint(db) != RuntimeInlineHint::Always
        || callee_body.blocks.len() != 1
        || callee_body.signature.params.len() != args.len()
        || !callee_body.provider_bindings.is_empty()
        || callee_body.locals.iter().any(|local| {
            !matches!(local.carrier, RuntimeCarrier::Value(_))
                || !matches!(local.root, RuntimeLocalRoot::None)
        })
    {
        return None;
    }
    let RTerminator::Return(Some(ret)) = callee_body.blocks[0].terminator else {
        return None;
    };
    if callee_body.blocks[0].stmts.iter().any(|stmt| {
        !matches!(
            stmt,
            RStmt::Assign { expr, .. }
                if matches!(
                    expr,
                    RExpr::Use(_)
                        | RExpr::ConstScalar(_)
                        | RExpr::Unary { .. }
                        | RExpr::Binary { .. }
                        | RExpr::Cast { .. }
                        | RExpr::Bitcast { .. }
                        | RExpr::Builtin(
                            RuntimeBuiltin::IntrinsicArith { .. }
                                | RuntimeBuiltin::F32FromI32 { .. }
                                | RuntimeBuiltin::I32FromF32 { .. }
                                | RuntimeBuiltin::F32Sqrt { .. }
                                | RuntimeBuiltin::F32Abs { .. }
                                | RuntimeBuiltin::F32Min { .. }
                                | RuntimeBuiltin::F32Max { .. }
                                | RuntimeBuiltin::F32MinRelaxed { .. }
                                | RuntimeBuiltin::F32MaxRelaxed { .. }
                                | RuntimeBuiltin::F32Clamp { .. }
                                | RuntimeBuiltin::F32Floor { .. }
                                | RuntimeBuiltin::F32Ceil { .. }
                                | RuntimeBuiltin::F32Trunc { .. }
                                | RuntimeBuiltin::F32Round { .. }
                        )
                        | RExpr::AggregateMake { .. }
                        | RExpr::AggregateExtract { .. }
                )
        )
    }) {
        return None;
    }
    let ret_class = callee_body.signature.ret.as_ref()?;
    let local = |id: RLocalId| {
        caller_locals
            .iter()
            .enumerate()
            .find(|(index, _)| RLocalId::from_u32(*index as u32) == id)
            .map(|(_, local)| local)
    };
    let dst_class = local(dst)?.carrier.value_class()?;
    if ret_class.is_transport()
        || dst_class.is_transport()
        || !ret_class.shares_runtime_rep_with(db, dst_class)
    {
        return None;
    }
    let mut map = FxHashMap::default();
    for (param, arg) in callee_body.signature.params.iter().zip(args) {
        let arg_class = local(*arg)?.carrier.value_class()?;
        if !param.class.shares_runtime_rep_with(db, arg_class) {
            return None;
        }
        map.insert(param.local, *arg);
    }
    let base_len = caller_locals.len();
    let mut staged_locals = Vec::new();
    for (idx, local) in callee_body.locals.iter().enumerate() {
        let old = RLocalId::from_u32(idx as u32);
        if map.contains_key(&old) {
            continue;
        }
        let fresh = RLocalId::from_u32((base_len + staged_locals.len()) as u32);
        staged_locals.push(local.clone());
        map.insert(old, fresh);
    }
    let remap = |id: RLocalId| map.get(&id).copied();
    let mut out = Vec::with_capacity(callee_body.blocks[0].stmts.len() + 1);
    for stmt in &callee_body.blocks[0].stmts {
        let RStmt::Assign { dst, expr } = stmt else {
            return None;
        };
        let expr = remap_inline_expr(expr, &remap)?;
        out.push(RStmt::Assign {
            dst: remap(*dst)?,
            expr,
        });
    }
    out.push(RStmt::Assign {
        dst,
        expr: RExpr::Use(remap(ret)?),
    });
    #[cfg(test)]
    let before_specialization = out.len();
    let out = mir::specialize_pure_inline_stmts(out, aggregate_facts, dst)?;
    if out.len() > budget {
        return None;
    }
    caller_locals.extend(staged_locals);
    Some(InlineValueCall {
        #[cfg(test)]
        pruned: before_specialization.saturating_sub(out.len()),
        stmts: out,
    })
}

struct InlineValueCall<'db> {
    stmts: Vec<RStmt<'db>>,
    #[cfg(test)]
    pruned: usize,
}

fn remap_inline_expr<'db>(
    expr: &RExpr<'db>,
    map: &impl Fn(RLocalId) -> Option<RLocalId>,
) -> Option<RExpr<'db>> {
    Some(match expr {
        RExpr::Use(value) => RExpr::Use(map(*value)?),
        RExpr::ConstScalar(value) => RExpr::ConstScalar(value.clone()),
        RExpr::Unary { op, value } => RExpr::Unary {
            op: *op,
            value: map(*value)?,
        },
        RExpr::Binary { op, lhs, rhs } => RExpr::Binary {
            op: *op,
            lhs: map(*lhs)?,
            rhs: map(*rhs)?,
        },
        RExpr::Cast { value, to } => RExpr::Cast {
            value: map(*value)?,
            to: to.clone(),
        },
        RExpr::Bitcast { value, to } => RExpr::Bitcast {
            value: map(*value)?,
            to: to.clone(),
        },
        RExpr::Builtin(builtin) => RExpr::Builtin(match builtin {
            RuntimeBuiltin::IntrinsicArith {
                op,
                checked,
                lhs,
                rhs,
                class,
            } => RuntimeBuiltin::IntrinsicArith {
                op: *op,
                checked: *checked,
                lhs: map(*lhs)?,
                rhs: map(*rhs)?,
                class: class.clone(),
            },
            RuntimeBuiltin::F32FromI32 { value } => RuntimeBuiltin::F32FromI32 {
                value: map(*value)?,
            },
            RuntimeBuiltin::I32FromF32 { value } => RuntimeBuiltin::I32FromF32 {
                value: map(*value)?,
            },
            RuntimeBuiltin::F32Sqrt { value } => RuntimeBuiltin::F32Sqrt {
                value: map(*value)?,
            },
            RuntimeBuiltin::F32Abs { value } => RuntimeBuiltin::F32Abs {
                value: map(*value)?,
            },
            RuntimeBuiltin::F32Min { lhs, rhs } => RuntimeBuiltin::F32Min {
                lhs: map(*lhs)?,
                rhs: map(*rhs)?,
            },
            RuntimeBuiltin::F32Max { lhs, rhs } => RuntimeBuiltin::F32Max {
                lhs: map(*lhs)?,
                rhs: map(*rhs)?,
            },
            RuntimeBuiltin::F32MinRelaxed { lhs, rhs } => RuntimeBuiltin::F32MinRelaxed {
                lhs: map(*lhs)?,
                rhs: map(*rhs)?,
            },
            RuntimeBuiltin::F32MaxRelaxed { lhs, rhs } => RuntimeBuiltin::F32MaxRelaxed {
                lhs: map(*lhs)?,
                rhs: map(*rhs)?,
            },
            RuntimeBuiltin::F32Clamp { value, lo, hi } => RuntimeBuiltin::F32Clamp {
                value: map(*value)?,
                lo: map(*lo)?,
                hi: map(*hi)?,
            },
            RuntimeBuiltin::F32Floor { value } => RuntimeBuiltin::F32Floor {
                value: map(*value)?,
            },
            RuntimeBuiltin::F32Ceil { value } => RuntimeBuiltin::F32Ceil {
                value: map(*value)?,
            },
            RuntimeBuiltin::F32Trunc { value } => RuntimeBuiltin::F32Trunc {
                value: map(*value)?,
            },
            RuntimeBuiltin::F32Round { value } => RuntimeBuiltin::F32Round {
                value: map(*value)?,
            },
            _ => return None,
        }),
        RExpr::AggregateMake { layout, fields } => RExpr::AggregateMake {
            layout: *layout,
            fields: fields
                .iter()
                .map(|field| map(*field))
                .collect::<Option<_>>()?,
        },
        RExpr::AggregateExtract { value, index } => RExpr::AggregateExtract {
            value: map(*value)?,
            index: *index,
        },
        _ => return None,
    })
}

/// Reify statically projected read-only aggregate parameters as flattened
/// values for Wasm. Fe presents `Copy`/owned records as memory-provider refs
/// and immutable constants as const refs, but the Wasm product ABI already
/// carries their closed scalar trees by value. Convert only compile-time
/// `Field` paths through struct layouts; dynamic indexes, enums, stores,
/// address-taking, mutable object refs, and non-aggregate/resource pointees are
/// left untouched and continue to fail closed in normal lowering.
fn is_reifiable_aggregate_ref(kind: &RefKind<'_>) -> bool {
    matches!(
        kind,
        RefKind::Const
            | RefKind::Provider {
                space: AddressSpaceKind::Memory,
                ..
            }
    )
}

fn is_struct_aggregate(db: &DriverDataBase, class: &RuntimeClass<'_>) -> bool {
    let RuntimeClass::AggregateValue { layout } = class else {
        return false;
    };
    matches!(layout.data(db), Layout::Struct(_))
}

fn is_static_leaf_load_from_slot<'db>(
    db: &'db DriverDataBase,
    body: &RuntimeBody<'db>,
    stmt: &RStmt<'_>,
    candidate: RLocalId,
) -> bool {
    let RStmt::Assign {
        expr:
            RExpr::Load {
                place:
                    RuntimePlace {
                        root: PlaceRoot::Slot(root),
                        path,
                    },
            },
        ..
    } = stmt
    else {
        return false;
    };
    if *root != candidate || path.is_empty() {
        return false;
    }
    let Some(mut class) = body.value_class(candidate).cloned() else {
        return false;
    };
    for elem in path.iter() {
        let PlaceElem::Field(index) = elem else {
            return false;
        };
        let RuntimeClass::AggregateValue { layout } = class else {
            return false;
        };
        let Layout::Struct(struct_layout) = layout.data(db) else {
            return false;
        };
        let Some(field) = struct_layout.fields.get(index.0 as usize).cloned() else {
            return false;
        };
        class = field;
    }
    !matches!(class, RuntimeClass::AggregateValue { .. })
}

fn is_static_value_load_from_ref<'db>(
    db: &'db DriverDataBase,
    body: &RuntimeBody<'db>,
    stmt: &RStmt<'_>,
    candidate: RLocalId,
) -> bool {
    let RStmt::Assign {
        expr:
            RExpr::Load {
                place:
                    RuntimePlace {
                        root: PlaceRoot::Ref(root),
                        path,
                    },
            },
        ..
    } = stmt
    else {
        return false;
    };
    if *root != candidate || path.is_empty() {
        return false;
    }
    let Some(RuntimeClass::Ref { pointee, .. }) = body.value_class(candidate) else {
        return false;
    };
    let mut class = pointee.as_ref().clone();
    for elem in path.iter() {
        let PlaceElem::Field(index) = elem else {
            return false;
        };
        let RuntimeClass::AggregateValue { layout } = class else {
            return false;
        };
        let Layout::Struct(struct_layout) = layout.data(db) else {
            return false;
        };
        let Some(field) = struct_layout.fields.get(index.0 as usize).cloned() else {
            return false;
        };
        class = field;
    }
    // Every traversed field is statically known and the resulting class is a
    // closed value subtree. It may itself be a record: the reifier below
    // projects that subtree into flattened SSA leaves before any later reads.
    true
}

/// Prove that a reference-shaped aggregate parameter is consumed only as a
/// value: immutable static leaf reads or an explicit whole-value forwarding
/// assignment. Once flattened, the latter copies all leaves, so it preserves
/// Fe value semantics rather than aliasing a linear-memory object.
fn ref_param_has_only_value_reads<'db>(
    db: &'db DriverDataBase,
    body: &RuntimeBody<'db>,
    candidate: RLocalId,
) -> bool {
    for block in &body.blocks {
        for stmt in &block.stmts {
            if is_static_value_load_from_ref(db, body, stmt, candidate)
                || matches!(
                    stmt,
                    RStmt::Assign {
                        expr: RExpr::Use(src) | RExpr::RetagRef { value: src },
                        ..
                    } if *src == candidate
                )
            {
                continue;
            }
            let mut used = FxHashMap::default();
            collect_stmt_uses(stmt, &mut used);
            if used.contains_key(&candidate) {
                return false;
            }
        }
        let mut used = FxHashMap::default();
        collect_terminator_uses(&block.terminator, &mut used);
        if used.contains_key(&candidate) {
            return false;
        }
    }
    true
}

fn place_is_rooted_at(place: &RuntimePlace<'_>, candidate: RLocalId) -> bool {
    match place.root {
        PlaceRoot::Slot(root) | PlaceRoot::Ref(root) => root == candidate,
        PlaceRoot::Ptr { addr, .. } => addr == candidate,
        PlaceRoot::Provider(_) => false,
    }
}

fn expr_mentions_aggregate_candidate(expr: &RExpr<'_>, candidate: RLocalId) -> bool {
    match expr {
        RExpr::Use(value)
        | RExpr::Unary { value, .. }
        | RExpr::Cast { value, .. }
        | RExpr::Bitcast { value, .. }
        | RExpr::MaterializeToObject { src: value }
        | RExpr::ProviderFromRaw { raw: value, .. }
        | RExpr::WordToRawAddr { value, .. }
        | RExpr::ProviderToRaw { value }
        | RExpr::RetagRef { value }
        | RExpr::AggregateExtract { value, .. }
        | RExpr::EnumTagOfValue { value }
        | RExpr::EnumIsVariant { value, .. }
        | RExpr::EnumExtract { value, .. }
        | RExpr::EnumGetTag { root: value }
        | RExpr::EnumAssertVariantRef { root: value, .. } => *value == candidate,
        RExpr::Binary { lhs, rhs, .. } => *lhs == candidate || *rhs == candidate,
        RExpr::MaterializePlaceToObject { place }
        | RExpr::AddrOf { place }
        | RExpr::Load { place } => place_is_rooted_at(place, candidate),
        RExpr::AggregateMake { fields, .. }
        | RExpr::Call { args: fields, .. }
        | RExpr::EnumMake { fields, .. } => fields.contains(&candidate),
        // Runtime builtins accept scalar/address operands, never an aggregate
        // value. The candidate's AggregateValue class therefore makes it
        // impossible for its local id to occur in a well-typed builtin.
        RExpr::Builtin(_)
        | RExpr::ConstScalar(_)
        | RExpr::Placeholder { .. }
        | RExpr::ConstRef { .. }
        | RExpr::AllocObject { .. } => false,
    }
}

/// Prove that an own aggregate parameter is observed only by immutable,
/// statically-known field projections. Unrelated computation is allowed, but
/// every occurrence of the candidate itself is checked exhaustively.
fn slot_param_has_only_static_field_reads<'db>(
    db: &'db DriverDataBase,
    body: &RuntimeBody<'db>,
    candidate: RLocalId,
) -> bool {
    for block in &body.blocks {
        for stmt in &block.stmts {
            if is_static_leaf_load_from_slot(db, body, stmt, candidate) {
                continue;
            }
            match stmt {
                RStmt::Assign { expr, .. } => {
                    if expr_mentions_aggregate_candidate(expr, candidate) {
                        return false;
                    }
                }
                RStmt::Store { dst, src } | RStmt::CopyInto { dst, src } => {
                    if *src == candidate || place_is_rooted_at(dst, candidate) {
                        return false;
                    }
                }
                RStmt::EnumAssertVariant { value, .. } => {
                    if *value == candidate {
                        return false;
                    }
                }
                RStmt::EnumSetTag { root, .. } => {
                    if *root == candidate {
                        return false;
                    }
                }
                RStmt::EnumWriteVariant { root, fields, .. } => {
                    if *root == candidate || fields.contains(&candidate) {
                        return false;
                    }
                }
            }
        }
        let terminator_is_safe = match &block.terminator {
            RTerminator::Goto(_) | RTerminator::Trap | RTerminator::Stop => true,
            RTerminator::Branch { cond, .. } => *cond != candidate,
            RTerminator::SwitchScalar { discr, .. }
            | RTerminator::MatchEnumTag { tag: discr, .. } => *discr != candidate,
            RTerminator::TerminalCall { args, .. } => !args.contains(&candidate),
            RTerminator::ReturnData { offset, len } | RTerminator::Revert { offset, len } => {
                *offset != candidate && *len != candidate
            }
            RTerminator::SelfDestruct { beneficiary } => *beneficiary != candidate,
            RTerminator::Return(value) => value.is_none_or(|value| value != candidate),
        };
        if !terminator_is_safe {
            return false;
        }
    }
    true
}

fn reify_static_aggregate_params<'db>(db: &'db DriverDataBase, body: &mut RuntimeBody<'db>) {
    let mut candidates = body
        .signature
        .params
        .iter()
        .filter_map(|param| match &param.class {
            RuntimeClass::Ref { pointee, kind, .. }
                if is_reifiable_aggregate_ref(kind)
                    && matches!(pointee.as_ref(), RuntimeClass::AggregateValue { .. })
                    && is_struct_aggregate(db, pointee)
                    && (!pointee.contains_array_value(db)
                        || ref_param_has_only_value_reads(db, body, param.local)) =>
            {
                Some((param.local, pointee.as_ref().clone()))
            }
            RuntimeClass::AggregateValue { .. }
                if matches!(
                    body.locals[param.local.as_u32() as usize].root,
                    RuntimeLocalRoot::Slot(_)
                ) && slot_param_has_only_static_field_reads(db, body, param.local) =>
            {
                Some((param.local, param.class.clone()))
            }
            _ => None,
        })
        .collect::<FxHashMap<_, _>>();
    if candidates.is_empty() {
        return;
    }

    for param in &mut body.signature.params {
        if let Some(class) = candidates.get(&param.local) {
            param.class = class.clone();
            body.locals[param.local.as_u32() as usize].carrier =
                RuntimeCarrier::Value(class.clone());
            body.locals[param.local.as_u32() as usize].root = RuntimeLocalRoot::None;
        }
    }

    let (locals, blocks) = (&mut body.locals, &mut body.blocks);
    for block in blocks {
        let mut rewritten = Vec::with_capacity(block.stmts.len());
        for stmt in std::mem::take(&mut block.stmts) {
            if let RStmt::Assign { dst, expr } = &stmt
                && let Some(src) = (match expr {
                    RExpr::Use(src) | RExpr::RetagRef { value: src } => Some(*src),
                    _ => None,
                })
                && let Some(class) = candidates.get(&src).cloned()
            {
                // Enum values retain their reference/tag representation until
                // the value lane has an explicit flattened enum ABI. Record
                // forwarding is safe because every leaf is projected by
                // declaration order; applying that rule to an enum would
                // erase the tag/reference operations used by its consumer.
                if !is_struct_aggregate(db, &class) {
                    rewritten.push(stmt);
                    continue;
                }
                let dst_class = locals[dst.as_u32() as usize].carrier.value_class();
                let compatible = match dst_class {
                    Some(RuntimeClass::Ref { pointee, .. }) => {
                        class.shares_runtime_rep_with(db, pointee)
                    }
                    Some(other) => class.shares_runtime_rep_with(db, other),
                    None => false,
                };
                if compatible {
                    locals[dst.as_u32() as usize].carrier = RuntimeCarrier::Value(class.clone());
                    locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                    candidates.insert(*dst, class);
                    // `RetagRef` changes only Fe capability metadata and MIR's
                    // verifier already proves an identical runtime
                    // representation. Once this backend overlay reifies the
                    // reference as a recursive scalar value, preserve that
                    // proof as an ordinary value copy rather than carrying a
                    // meaningless reference operation into Wasm.
                    rewritten.push(RStmt::Assign {
                        dst: *dst,
                        expr: RExpr::Use(src),
                    });
                    continue;
                }
                rewritten.push(stmt);
                continue;
            }
            let RStmt::Assign {
                dst,
                expr: RExpr::Load { place },
            } = &stmt
            else {
                rewritten.push(stmt);
                continue;
            };
            let root = match place.root {
                PlaceRoot::Ref(root) | PlaceRoot::Slot(root) => root,
                _ => {
                    rewritten.push(stmt);
                    continue;
                }
            };
            let path = &place.path;
            let Some(mut class) = candidates.get(&root).cloned() else {
                rewritten.push(stmt);
                continue;
            };
            if path.is_empty() {
                locals[dst.as_u32() as usize].carrier = RuntimeCarrier::Value(class);
                locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                rewritten.push(RStmt::Assign {
                    dst: *dst,
                    expr: RExpr::Use(root),
                });
                continue;
            }
            if !path.iter().all(|elem| matches!(elem, PlaceElem::Field(_))) {
                rewritten.push(stmt);
                continue;
            }

            let mut current = root;
            let mut semantic_ty = locals[root.as_u32() as usize].semantic_ty;
            let mut projections = Vec::with_capacity(path.len());
            let mut valid = true;
            for (position, elem) in path.iter().enumerate() {
                let PlaceElem::Field(index) = elem else {
                    unreachable!()
                };
                let RuntimeClass::AggregateValue { layout } = class else {
                    valid = false;
                    break;
                };
                let Layout::Struct(struct_layout) = layout.data(db) else {
                    valid = false;
                    break;
                };
                let Some(field_class) = struct_layout.fields.get(index.0 as usize).cloned() else {
                    valid = false;
                    break;
                };
                let semantic_fields = semantic_ty
                    .as_view(db)
                    .unwrap_or(semantic_ty)
                    .field_types(db);
                let Some(field_semantic_ty) = semantic_fields.get(index.0 as usize).copied() else {
                    valid = false;
                    break;
                };
                let target = if position + 1 == path.len() {
                    *dst
                } else {
                    let temp = RLocalId::from_u32(locals.len() as u32);
                    locals.push(RLocal {
                        semantic_ty: field_semantic_ty,
                        carrier: RuntimeCarrier::Value(field_class.clone()),
                        root: RuntimeLocalRoot::None,
                    });
                    temp
                };
                projections.push(RStmt::Assign {
                    dst: target,
                    expr: RExpr::AggregateExtract {
                        value: current,
                        index: u32::from(index.0),
                    },
                });
                current = target;
                class = field_class;
                semantic_ty = field_semantic_ty;
            }
            if valid {
                locals[dst.as_u32() as usize].carrier = RuntimeCarrier::Value(class);
                locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                rewritten.extend(projections);
            } else {
                rewritten.push(stmt);
            }
        }
        block.stmts = rewritten;
    }
}

fn repair_scalar_semantic_types_from_array_projections<'db>(
    db: &'db DriverDataBase,
    body: &mut RuntimeBody<'db>,
) {
    for block in &body.blocks {
        for stmt in &block.stmts {
            let RStmt::Assign {
                dst,
                expr: RExpr::AggregateExtract { value, .. },
            } = stmt
            else {
                continue;
            };
            let source_ty = body.locals[value.as_u32() as usize]
                .semantic_ty
                .as_view(db)
                .unwrap_or(body.locals[value.as_u32() as usize].semantic_ty);
            if !source_ty.is_array(db) {
                continue;
            }
            let (_, args) = source_ty.decompose_ty_app(db);
            if let Some(element_ty) = args.first().copied() {
                body.locals[dst.as_u32() as usize].semantic_ty = element_ty;
            }
        }
    }
}

fn semantic_place_result_ty<'db>(
    db: &'db DriverDataBase,
    body: &RuntimeBody<'db>,
    place: &RuntimePlace<'db>,
) -> Option<TyId<'db>> {
    let root = match place.root {
        PlaceRoot::Slot(local) | PlaceRoot::Ref(local) => local,
        PlaceRoot::Provider(_) | PlaceRoot::Ptr { .. } => return None,
    };
    let mut ty = body.locals[root.as_u32() as usize].semantic_ty;
    for elem in place.path.iter() {
        ty = ty.as_view(db).unwrap_or(ty);
        ty = match elem {
            PlaceElem::Field(index) => ty.field_types(db).get(index.0 as usize).copied()?,
            PlaceElem::Index(_) => {
                if !ty.is_array(db) {
                    return None;
                }
                let (_, args) = ty.decompose_ty_app(db);
                args.first().copied()?
            }
            PlaceElem::VariantField { .. } | PlaceElem::Deref => return None,
        };
    }
    Some(ty)
}

fn lower_usize_array_place_classes<'db>(db: &'db DriverDataBase, body: &mut RuntimeBody<'db>) {
    let snapshot = body.clone();
    for block in &mut body.blocks {
        for stmt in &mut block.stmts {
            let RStmt::Assign {
                dst,
                expr: RExpr::Load { place },
            } = stmt
            else {
                continue;
            };
            if semantic_place_result_ty(db, &snapshot, place)
                .is_some_and(|ty| is_usize_semantic_ty(db, ty))
                && let RuntimeCarrier::Value(RuntimeClass::Scalar(scalar)) =
                    &mut body.locals[dst.as_u32() as usize].carrier
                && is_u256_unsigned(scalar.repr)
            {
                scalar.repr = USIZE_WASM_REPR;
                body.locals[dst.as_u32() as usize].semantic_ty =
                    semantic_place_result_ty(db, &snapshot, place).unwrap();
            }
        }
    }
}

/// True when a runtime local's source-level type is `usize`. On wasm32 `usize`
/// is a 32-bit pointer-width integer, but MIR classifies it as a 256-bit scalar
/// (shared with `u256`, `type_info.rs`). The narrowing pass below keys on this
/// exact predicate so a genuine `u256` (semantic_ty `U256`) stays 256-bit and
/// fail-closed, while a `usize` index/counter narrows to `i32`.
fn is_usize_semantic_ty<'db>(
    db: &'db DriverDataBase,
    ty: hir::analysis::ty::ty_def::TyId<'db>,
) -> bool {
    let mut ty = ty;
    while let Some(inner) = ty.as_view(db) {
        ty = inner;
    }
    // A reified const generic such as `N` is represented by its evaluated
    // `ConstTy`, not directly by the primitive type that carries the value.
    // Its runtime width is nevertheless the width of that const's declared
    // type (`usize` here), so wasm32 legalization must inspect that carrier.
    if let Some(const_value_ty) = ty.const_ty_ty(db) {
        ty = const_value_ty;
    }
    matches!(
        ty.base_ty(db).data(db),
        TyData::TyBase(TyBase::Prim(PrimTy::Usize))
    )
}

/// Resolve the source-level type carried by an RMIR local in the context of
/// the concrete runtime instance that owns the body. Most semantic locals are
/// already instantiated before RMIR lowering, but provider-selected generic
/// helpers can retain a `TyParam` label even though their runtime class and
/// instance substitution are concrete. Target legalization must consume that
/// semantic substitution rather than infer from a helper symbol or runtime
/// width (which would incorrectly turn a genuine `u256` into a pointer).
fn instantiated_runtime_local_ty<'db>(
    db: &'db DriverDataBase,
    instance: RuntimeInstance<'db>,
    ty: TyId<'db>,
) -> TyId<'db> {
    let Some(semantic) = instance.key(db).semantic(db) else {
        return ty;
    };
    let ty = instantiate_with_generic_args(db, ty, semantic.key(db).subst(db).generic_args(db));
    semantic.normalized_ty(db, ty)
}

/// The i32-width unsigned scalar repr that a narrowed `usize` carries on wasm32.
const USIZE_WASM_REPR: ScalarRepr = ScalarRepr::Int {
    bits: 32,
    signed: false,
};

/// Whether a scalar repr is the 256-bit unsigned integer that MIR assigns to
/// `usize` (and `u256`). Only the ones whose local's `semantic_ty` is `usize`
/// are narrowed; the rest stay 256-bit and fail closed.
fn is_u256_unsigned(repr: ScalarRepr) -> bool {
    matches!(
        repr,
        ScalarRepr::Int {
            bits: 256,
            signed: false
        }
    )
}

/// Whether a big-endian integer constant (`ConstScalar::Int::words`, the byte
/// order `bytes_to_i256` / `I256::from_be_bytes` consume) fits in an unsigned
/// 32-bit value. A narrowed `usize` carries a 32-bit value on wasm32; a wider
/// constant would truncate its high bits when rematerialized at the narrowed i32
/// type, and a semantically out-of-bounds index (`>= 2^32`) whose low 32 bits
/// happen to be small would slip past the unsigned array bounds check. Such a
/// constant must NOT be narrowed (the body falls back to the 256-bit fail-closed
/// path instead).
fn const_int_fits_u32(words: &[u8]) -> bool {
    let len = words.len();
    len <= 4 || words[..len - 4].iter().all(|&byte| byte == 0)
}

/// Change 4: narrow `usize` scalar locals (256-bit in MIR) to `i32` on the wasm
/// path so a runtime array index / loop counter can materialize and address
/// linear memory. This is the target-correct width for `usize` on wasm32, not a
/// truncation hack: `usize` IS 32-bit there.
///
/// The pass keys strictly on `semantic_ty == PrimTy::Usize`, so a real `u256`
/// (semantic `U256`) keeps its 256-bit repr and stays rejected by
/// `scalar_ty_r1`. It rewrites the narrowed locals' carriers plus the embedded
/// `ScalarClass` reprs keyed off them (a `Cast`'s `to`, an `IntrinsicArith`'s
/// `class`, an `IntTruncate`'s `from`/`to`) and any signature params that are
/// narrowed locals. `ConstScalar` literals need no rewrite: their immediate is
/// re-materialized at the narrowed carrier type.
///
/// It is fail-open: the rewrite is staged on a clone and only committed if every
/// embedded repr keyed off a narrowed local was the expected 256-bit unsigned
/// one. Any inconsistency leaves the body untouched so the old fail-closed error
/// fires instead of a silently-miscompiled narrowing.
fn narrow_usize_scalars<'db>(
    db: &'db DriverDataBase,
    instance: RuntimeInstance<'db>,
    body: &mut RuntimeBody<'db>,
) {
    let narrowed: FxHashMap<RLocalId, TyId<'db>> = body
        .locals
        .iter()
        .enumerate()
        .filter_map(|(idx, local)| {
            let RuntimeCarrier::Value(RuntimeClass::Scalar(scalar)) = &local.carrier else {
                return None;
            };
            let concrete_ty = instantiated_runtime_local_ty(db, instance, local.semantic_ty);
            (is_u256_unsigned(scalar.repr) && is_usize_semantic_ty(db, concrete_ty))
                .then_some((RLocalId::from_u32(idx as u32), concrete_ty))
        })
        .collect();
    if narrowed.is_empty() {
        return;
    }
    let is_narrowed = |local: &RLocalId| narrowed.contains_key(local);

    let mut staged = body.clone();
    let mut ok = true;

    for (id, concrete_ty) in &narrowed {
        let idx = id.as_u32() as usize;
        staged.locals[idx].semantic_ty = *concrete_ty;
        if let RuntimeCarrier::Value(RuntimeClass::Scalar(scalar)) = &mut staged.locals[idx].carrier
        {
            scalar.repr = USIZE_WASM_REPR;
        }
        if let RuntimeLocalRoot::Slot(RuntimeClass::Scalar(scalar))
        | RuntimeLocalRoot::Ref(RuntimeClass::Scalar(scalar)) = &mut staged.locals[idx].root
            && is_u256_unsigned(scalar.repr)
        {
            scalar.repr = USIZE_WASM_REPR;
        }
    }
    for param in &mut staged.signature.params {
        if is_narrowed(&param.local) {
            if let RuntimeClass::Scalar(scalar) = &mut param.class {
                if is_u256_unsigned(scalar.repr) {
                    scalar.repr = USIZE_WASM_REPR;
                } else if scalar.repr != USIZE_WASM_REPR {
                    ok = false;
                }
            } else {
                ok = false;
            }
        }
    }

    let mut saw_return = false;
    let mut all_returns_narrowed = true;
    for block in &staged.blocks {
        if let RTerminator::Return(Some(value)) = &block.terminator {
            saw_return = true;
            all_returns_narrowed &= is_narrowed(value);
        }
    }
    if saw_return && all_returns_narrowed {
        match &mut staged.signature.ret {
            Some(RuntimeClass::Scalar(scalar)) if is_u256_unsigned(scalar.repr) => {
                scalar.repr = USIZE_WASM_REPR;
            }
            Some(RuntimeClass::Scalar(scalar)) if scalar.repr == USIZE_WASM_REPR => {}
            _ => ok = false,
        }
    }

    for block in &mut staged.blocks {
        for stmt in &mut block.stmts {
            let RStmt::Assign { dst, expr } = stmt else {
                continue;
            };
            let dst_narrowed = is_narrowed(dst);
            match expr {
                RExpr::Cast { value: _, to } if dst_narrowed => {
                    if is_u256_unsigned(to.repr) {
                        to.repr = USIZE_WASM_REPR;
                    } else {
                        ok = false;
                    }
                }
                RExpr::Bitcast { value: _, to } if dst_narrowed => {
                    if is_u256_unsigned(to.repr) {
                        to.repr = USIZE_WASM_REPR;
                    } else {
                        ok = false;
                    }
                }
                RExpr::Builtin(RuntimeBuiltin::IntrinsicArith { class, .. }) if dst_narrowed => {
                    if is_u256_unsigned(class.repr) {
                        class.repr = USIZE_WASM_REPR;
                    } else {
                        ok = false;
                    }
                }
                RExpr::Builtin(RuntimeBuiltin::IntTruncate { value, from, to }) => {
                    // Fail closed on any repr mismatch (staged-body property "any
                    // inconsistency leaves the body untouched"): a partial rewrite
                    // would leave one side 256-bit and silently miscompile.
                    if dst_narrowed {
                        if is_u256_unsigned(to.repr) {
                            to.repr = USIZE_WASM_REPR;
                        } else {
                            ok = false;
                        }
                    }
                    if is_narrowed(value) {
                        if is_u256_unsigned(from.repr) {
                            from.repr = USIZE_WASM_REPR;
                        } else if from.repr != USIZE_WASM_REPR {
                            ok = false;
                        }
                    }
                }
                // CRITICAL bounds-safety: a narrowed `usize` carries a 32-bit value
                // on wasm32. A `usize` constant whose value exceeds `u32::MAX` would
                // truncate its high bits when rematerialized at the narrowed i32
                // type (`immediate_for_const_scalar` -> `Immediate::from_i256`), so
                // a semantically out-of-bounds index could alias an in-bounds
                // element before the unsigned bounds check ever runs. Refuse to
                // narrow: the whole body falls back to the 256-bit fail-closed path.
                RExpr::ConstScalar(ConstScalar::Int { words, signed, .. }) if dst_narrowed => {
                    if *signed || !const_int_fits_u32(words.as_slice()) {
                        ok = false;
                    }
                }
                _ => {}
            }
        }
    }

    if ok {
        *body = staged;
    }
}

/// The set of runtime locals referenced as an operand anywhere in `body`. Every
/// `match` below destructures WITHOUT `..`, so the compiler forces each field to
/// be named: no operand position can be silently missed (the safety net for a
/// pass that erases locals). A definition (`RStmt::Assign.dst`, a place that is
/// written) is NOT a use; only reads are collected.
fn collect_used_locals(body: &RuntimeBody<'_>) -> FxHashMap<RLocalId, ()> {
    let mut used = FxHashMap::default();
    // Provider bindings pin their backing value local.
    for binding in &body.provider_bindings {
        used.insert(binding.value, ());
    }
    for block in &body.blocks {
        for stmt in &block.stmts {
            collect_stmt_uses(stmt, &mut used);
        }
        collect_terminator_uses(&block.terminator, &mut used);
    }
    used
}

fn collect_place_uses(place: &RuntimePlace<'_>, used: &mut FxHashMap<RLocalId, ()>) {
    match &place.root {
        PlaceRoot::Slot(local) => {
            used.insert(*local, ());
        }
        PlaceRoot::Ref(value) => {
            used.insert(*value, ());
        }
        PlaceRoot::Provider(_) => {}
        PlaceRoot::Ptr {
            addr,
            space: _,
            class: _,
        } => {
            used.insert(*addr, ());
        }
    }
    for elem in place.path.iter() {
        match elem {
            PlaceElem::Field(_) => {}
            PlaceElem::Index(IndexSource::Constant(_)) => {}
            PlaceElem::Index(IndexSource::Dynamic(value)) => {
                used.insert(*value, ());
            }
            PlaceElem::VariantField {
                variant: _,
                field: _,
            } => {}
            PlaceElem::Deref => {}
        }
    }
}

fn collect_expr_uses(expr: &RExpr<'_>, used: &mut FxHashMap<RLocalId, ()>) {
    match expr {
        RExpr::Use(value) => {
            used.insert(*value, ());
        }
        RExpr::ConstScalar(_) => {}
        RExpr::Placeholder { class: _ } => {}
        RExpr::Builtin(builtin) => collect_builtin_uses(builtin, used),
        RExpr::Unary { op: _, value } => {
            used.insert(*value, ());
        }
        RExpr::Binary { op: _, lhs, rhs } => {
            used.insert(*lhs, ());
            used.insert(*rhs, ());
        }
        RExpr::Cast { value, to: _ } => {
            used.insert(*value, ());
        }
        RExpr::Bitcast { value, to: _ } => {
            used.insert(*value, ());
        }
        RExpr::ConstRef {
            region: _,
            layout: _,
        } => {}
        RExpr::AllocObject { layout: _ } => {}
        RExpr::MaterializeToObject { src } => {
            used.insert(*src, ());
        }
        RExpr::MaterializePlaceToObject { place } => collect_place_uses(place, used),
        RExpr::ProviderFromRaw {
            raw,
            provider_ty: _,
            space: _,
            target: _,
        } => {
            used.insert(*raw, ());
        }
        RExpr::WordToRawAddr {
            value,
            space: _,
            target: _,
        } => {
            used.insert(*value, ());
        }
        RExpr::ProviderToRaw { value } => {
            used.insert(*value, ());
        }
        RExpr::RetagRef { value } => {
            used.insert(*value, ());
        }
        RExpr::AddrOf { place } => collect_place_uses(place, used),
        RExpr::Load { place } => collect_place_uses(place, used),
        RExpr::AggregateExtract { value, index: _ } => {
            used.insert(*value, ());
        }
        RExpr::AggregateMake { layout: _, fields } => {
            for field in fields.iter() {
                used.insert(*field, ());
            }
        }
        RExpr::Call { callee: _, args } => {
            for arg in args.iter() {
                used.insert(*arg, ());
            }
        }
        RExpr::EnumMake {
            layout: _,
            variant: _,
            fields,
        } => {
            for field in fields.iter() {
                used.insert(*field, ());
            }
        }
        RExpr::EnumTagOfValue { value } => {
            used.insert(*value, ());
        }
        RExpr::EnumIsVariant { value, variant: _ } => {
            used.insert(*value, ());
        }
        RExpr::EnumExtract {
            value,
            variant: _,
            field: _,
        } => {
            used.insert(*value, ());
        }
        RExpr::EnumGetTag { root } => {
            used.insert(*root, ());
        }
        RExpr::EnumAssertVariantRef { root, variant: _ } => {
            used.insert(*root, ());
        }
    }
}

fn collect_stmt_uses(stmt: &RStmt<'_>, used: &mut FxHashMap<RLocalId, ()>) {
    match stmt {
        RStmt::Assign { dst: _, expr } => collect_expr_uses(expr, used),
        RStmt::EnumAssertVariant { value, variant: _ } => {
            used.insert(*value, ());
        }
        RStmt::Store { dst, src } => {
            collect_place_uses(dst, used);
            used.insert(*src, ());
        }
        RStmt::CopyInto { dst, src } => {
            collect_place_uses(dst, used);
            used.insert(*src, ());
        }
        RStmt::EnumSetTag { root, variant: _ } => {
            used.insert(*root, ());
        }
        RStmt::EnumWriteVariant {
            root,
            variant: _,
            fields,
        } => {
            used.insert(*root, ());
            for field in fields.iter() {
                used.insert(*field, ());
            }
        }
    }
}

fn collect_terminator_uses(terminator: &RTerminator<'_>, used: &mut FxHashMap<RLocalId, ()>) {
    match terminator {
        RTerminator::Goto(_) => {}
        RTerminator::Branch {
            cond,
            then_bb: _,
            else_bb: _,
        } => {
            used.insert(*cond, ());
        }
        RTerminator::SwitchScalar {
            discr,
            cases: _,
            default: _,
        } => {
            used.insert(*discr, ());
        }
        RTerminator::MatchEnumTag {
            tag,
            enum_layout: _,
            cases: _,
            default: _,
        } => {
            used.insert(*tag, ());
        }
        RTerminator::TerminalCall { callee: _, args } => {
            for arg in args.iter() {
                used.insert(*arg, ());
            }
        }
        RTerminator::ReturnData { offset, len } => {
            used.insert(*offset, ());
            used.insert(*len, ());
        }
        RTerminator::Revert { offset, len } => {
            used.insert(*offset, ());
            used.insert(*len, ());
        }
        RTerminator::SelfDestruct { beneficiary } => {
            used.insert(*beneficiary, ());
        }
        RTerminator::Trap => {}
        RTerminator::Return(value) => {
            if let Some(value) = value {
                used.insert(*value, ());
            }
        }
        RTerminator::Stop => {}
    }
}

fn collect_builtin_uses(builtin: &RuntimeBuiltin<'_>, used: &mut FxHashMap<RLocalId, ()>) {
    let mut mark = |value: &RLocalId| {
        used.insert(*value, ());
    };
    match builtin {
        RuntimeBuiltin::IntTruncate {
            value,
            from: _,
            to: _,
        } => mark(value),
        RuntimeBuiltin::Mload { addr } => mark(addr),
        RuntimeBuiltin::Mstore { addr, value } => {
            mark(addr);
            mark(value);
        }
        RuntimeBuiltin::Mstore8 { addr, value } => {
            mark(addr);
            mark(value);
        }
        RuntimeBuiltin::Mcopy { dst, src, len } => {
            mark(dst);
            mark(src);
            mark(len);
        }
        RuntimeBuiltin::Msize => {}
        RuntimeBuiltin::Sload { slot } => mark(slot),
        RuntimeBuiltin::Sstore { slot, value } => {
            mark(slot);
            mark(value);
        }
        RuntimeBuiltin::CallValue => {}
        RuntimeBuiltin::ReturnDataSize => {}
        RuntimeBuiltin::ReturnDataCopy { dst, offset, len } => {
            mark(dst);
            mark(offset);
            mark(len);
        }
        RuntimeBuiltin::CallDataSize => {}
        RuntimeBuiltin::CallDataLoad { offset } => mark(offset),
        RuntimeBuiltin::CallDataCopy { dst, offset, len } => {
            mark(dst);
            mark(offset);
            mark(len);
        }
        RuntimeBuiltin::CodeSize => {}
        RuntimeBuiltin::CodeCopy { dst, offset, len } => {
            mark(dst);
            mark(offset);
            mark(len);
        }
        RuntimeBuiltin::ExtCodeSize { addr } => mark(addr),
        RuntimeBuiltin::ExtCodeCopy {
            addr,
            dst,
            offset,
            len,
        } => {
            mark(addr);
            mark(dst);
            mark(offset);
            mark(len);
        }
        RuntimeBuiltin::ExtCodeHash { addr } => mark(addr),
        RuntimeBuiltin::Keccak256 { offset, len } => {
            mark(offset);
            mark(len);
        }
        RuntimeBuiltin::AddMod { lhs, rhs, modulus } => {
            mark(lhs);
            mark(rhs);
            mark(modulus);
        }
        RuntimeBuiltin::MulMod { lhs, rhs, modulus } => {
            mark(lhs);
            mark(rhs);
            mark(modulus);
        }
        RuntimeBuiltin::Byte { pos, value } => {
            mark(pos);
            mark(value);
        }
        RuntimeBuiltin::SignExtend { byte, value } => {
            mark(byte);
            mark(value);
        }
        RuntimeBuiltin::IntrinsicArith {
            op: _,
            checked: _,
            lhs,
            rhs,
            class: _,
        } => {
            mark(lhs);
            mark(rhs);
        }
        RuntimeBuiltin::Saturating {
            op: _,
            lhs,
            rhs,
            class: _,
        } => {
            mark(lhs);
            mark(rhs);
        }
        RuntimeBuiltin::F32FromI32 { value } => mark(value),
        RuntimeBuiltin::I32FromF32 { value } => mark(value),
        RuntimeBuiltin::F32Sqrt { value } => mark(value),
        RuntimeBuiltin::F32Abs { value } => mark(value),
        RuntimeBuiltin::F32Min { lhs, rhs }
        | RuntimeBuiltin::F32Max { lhs, rhs }
        | RuntimeBuiltin::F32MinRelaxed { lhs, rhs }
        | RuntimeBuiltin::F32MaxRelaxed { lhs, rhs } => {
            mark(lhs);
            mark(rhs);
        }
        RuntimeBuiltin::F32Clamp { value, lo, hi } => {
            mark(value);
            mark(lo);
            mark(hi);
        }
        RuntimeBuiltin::F32Floor { value } => mark(value),
        RuntimeBuiltin::F32Ceil { value } => mark(value),
        RuntimeBuiltin::F32Trunc { value } => mark(value),
        RuntimeBuiltin::F32Round { value } => mark(value),
        RuntimeBuiltin::Address => {}
        RuntimeBuiltin::Caller => {}
        RuntimeBuiltin::Origin => {}
        RuntimeBuiltin::GasPrice => {}
        RuntimeBuiltin::CoinBase => {}
        RuntimeBuiltin::Balance { addr } => mark(addr),
        RuntimeBuiltin::Timestamp => {}
        RuntimeBuiltin::Number => {}
        RuntimeBuiltin::PrevRandao => {}
        RuntimeBuiltin::GasLimit => {}
        RuntimeBuiltin::ChainId => {}
        RuntimeBuiltin::BaseFee => {}
        RuntimeBuiltin::SelfBalance => {}
        RuntimeBuiltin::BlockHash { block } => mark(block),
        RuntimeBuiltin::BlobHash { index } => mark(index),
        RuntimeBuiltin::BlobBaseFee => {}
        RuntimeBuiltin::Gas => {}
        RuntimeBuiltin::CurrentCodeRegionLen => {}
        RuntimeBuiltin::CodeRegionOffset { region: _ } => {}
        RuntimeBuiltin::CodeRegionLen { region: _ } => {}
        RuntimeBuiltin::Malloc { size } => mark(size),
        RuntimeBuiltin::Call {
            gas,
            addr,
            value,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        } => {
            mark(gas);
            mark(addr);
            mark(value);
            mark(args_offset);
            mark(args_len);
            mark(ret_offset);
            mark(ret_len);
        }
        RuntimeBuiltin::StaticCall {
            gas,
            addr,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        } => {
            mark(gas);
            mark(addr);
            mark(args_offset);
            mark(args_len);
            mark(ret_offset);
            mark(ret_len);
        }
        RuntimeBuiltin::DelegateCall {
            gas,
            addr,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        } => {
            mark(gas);
            mark(addr);
            mark(args_offset);
            mark(args_len);
            mark(ret_offset);
            mark(ret_len);
        }
        RuntimeBuiltin::Create { value, offset, len } => {
            mark(value);
            mark(offset);
            mark(len);
        }
        RuntimeBuiltin::Create2 {
            value,
            offset,
            len,
            salt,
        } => {
            mark(value);
            mark(offset);
            mark(len);
            mark(salt);
        }
        RuntimeBuiltin::Log0 { offset, len } => {
            mark(offset);
            mark(len);
        }
        RuntimeBuiltin::Log1 {
            offset,
            len,
            topic0,
        } => {
            mark(offset);
            mark(len);
            mark(topic0);
        }
        RuntimeBuiltin::Log2 {
            offset,
            len,
            topic0,
            topic1,
        } => {
            mark(offset);
            mark(len);
            mark(topic0);
            mark(topic1);
        }
        RuntimeBuiltin::Log3 {
            offset,
            len,
            topic0,
            topic1,
            topic2,
        } => {
            mark(offset);
            mark(len);
            mark(topic0);
            mark(topic1);
            mark(topic2);
        }
        RuntimeBuiltin::Log4 {
            offset,
            len,
            topic0,
            topic1,
            topic2,
            topic3,
        } => {
            mark(offset);
            mark(len);
            mark(topic0);
            mark(topic1);
            mark(topic2);
            mark(topic3);
        }
        RuntimeBuiltin::CallDataSelector => {}
        RuntimeBuiltin::MakeContractFieldRef {
            slot: _,
            class: _,
            kind: _,
        } => {}
    }
}

/// Remove provably-dead value-carried ARRAY/ENUM aggregate assignments so an
/// unused by-value array (for example the vestigial const-array rvalue MIR keeps
/// when `let mut a: [u32; N] = [0; N]` is also materialized into a heap object)
/// does not force the wasm value path to represent an aggregate it cannot carry.
///
/// Restricted to `Array` / `Enum` layouts on purpose: those NEVER lower on the
/// wasm value path (`single_scalar_field` / `scalar_tuple_element_tys` reject
/// them), so no currently-lowering kernel can contain a dead one -- this pass is
/// therefore invisible to every existing kernel and only unblocks the new array
/// shapes. Single-scalar-field newtypes and flattenable structs (which DO lower)
/// are deliberately left untouched to preserve their exact emission.
///
/// A statement is removed only when its destination is (a) a value-carried
/// `AggregateValue` of an array/enum layout, (b) defined by a side-effect-free
/// expression (`Use` / `AggregateMake` / `ConstRef` / `Placeholder`), (c) not a
/// signature parameter, and (d) referenced by nothing (per the exhaustive
/// `collect_used_locals`). The removed local's carrier becomes `Erased` so the
/// SSA declaration loop skips it. Iterated to a fixpoint: erasing `%a = use %b`
/// can make `%b` dead in turn.
/// Item 3 (slice A ownership contract): the canonical arena bump-allocates
/// upward from byte 1024 and knows nothing about host-chosen fixed addresses. A
/// function that BOTH allocates a local array (`AllocObject` -> `MemAllocDynamic`)
/// AND accesses a direct host memory region (a `MemPtr` / `RawAddr{Memory}`
/// parameter, or a raw `Ptr{Memory}` place) could grow its array over the host
/// region. Until a disjoint address partition exists, that mix fails closed.
/// Functions that only allocate, or only touch host memory, are unaffected
/// (object-ref array element accesses use `Ref`-rooted places, never `Ptr`).
fn check_host_region_arena_disjoint(
    body: &RuntimeBody<'_>,
    arena_owned: &HashSet<RLocalId>,
) -> Result<(), LowerError> {
    let allocates_array = body.blocks.iter().any(|block| {
        block.stmts.iter().any(|stmt| {
            matches!(
                stmt,
                RStmt::Assign {
                    expr: RExpr::AllocObject { .. },
                    ..
                }
            )
        })
    });
    if !allocates_array {
        return Ok(());
    }
    let host_region_param = body.signature.params.iter().any(|param| {
        if arena_owned.contains(&param.local) {
            return false;
        }
        matches!(
            param.class,
            RuntimeClass::RawAddr {
                space: AddressSpaceKind::Memory,
                ..
            }
        )
    });
    let host_region_place = body.blocks.iter().any(|block| {
        block
            .stmts
            .iter()
            .any(|stmt| stmt_uses_host_memory_pointer(stmt, arena_owned))
    });
    if host_region_param || host_region_place {
        return Err(LowerError::Unsupported(
            "wasm target: a function that allocates a local array cannot also use a direct \
             host memory region (a `MemPtr`/`RawAddr{Memory}` parameter or raw memory \
             pointer); the canonical arena and host-chosen fixed addresses have no disjoint \
             partition in slice A, so this mix fails closed"
                .to_string(),
        ));
    }
    Ok(())
}

/// Whether a statement dereferences a raw host memory address (`Ptr{Memory}`
/// place root). Object-ref array accesses use `Ref`-rooted places, so this never
/// flags an arena allocation's own element reads/writes.
fn stmt_uses_host_memory_pointer(stmt: &RStmt<'_>, arena_owned: &HashSet<RLocalId>) -> bool {
    fn is_host_memory_ptr(place: &RuntimePlace<'_>) -> bool {
        matches!(
            place.root,
            PlaceRoot::Ptr {
                space: AddressSpaceKind::Memory,
                ..
            }
        )
    }
    let is_unowned_host_memory_ptr = |place: &RuntimePlace<'_>| {
        is_host_memory_ptr(place)
            && !matches!(
                place.root,
                PlaceRoot::Ptr { addr, .. } if arena_owned.contains(&addr)
            )
    };
    match stmt {
        RStmt::Store { dst, .. } | RStmt::CopyInto { dst, .. } => is_unowned_host_memory_ptr(dst),
        RStmt::Assign { expr, .. } => match expr {
            RExpr::Load { place }
            | RExpr::AddrOf { place }
            | RExpr::MaterializePlaceToObject { place } => is_unowned_host_memory_ptr(place),
            _ => false,
        },
        _ => false,
    }
}

/// Keep residual call sites aligned with parameters that the portable backend
/// reified from borrowed aggregate references into flattened aggregate values.
///
/// Runtime MIR deliberately presents `Copy` values through const, object, or
/// memory-provider references. `reify_static_aggregate_params` turns an
/// admissible helper parameter into its value ABI, materializing an independent
/// callee-local arena copy when dynamic indexing requires addressability. A call
/// that survives the bounded inliner must undergo the same conversion at its
/// boundary: load the complete caller-owned value, then pass its scalar leaves.
/// Otherwise the callee signature expects N leaves while the caller contributes
/// one borrowed pointer, producing malformed Wasm or leaving an unlowerable
/// dynamic-index read in the residual helper.
fn reify_residual_call_arguments<'db>(
    db: &'db DriverDataBase,
    package: &RuntimePackage<'db>,
    bodies: &mut FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
) {
    let reified_params = package
        .functions(db)
        .into_iter()
        .filter_map(|function| {
            let instance = function.instance(db);
            let original = instance.body(db);
            let prepared = bodies.get(&instance)?;
            if original.signature.params.len() != prepared.signature.params.len() {
                return None;
            }
            let params = original
                .signature
                .params
                .iter()
                .zip(&prepared.signature.params)
                .map(
                    |(original, prepared)| match (&original.class, &prepared.class) {
                        (
                            RuntimeClass::Ref {
                                pointee,
                                kind:
                                    RefKind::Const
                                    | RefKind::Object
                                    | RefKind::Provider {
                                        space: AddressSpaceKind::Memory,
                                        ..
                                    },
                                view: RefView::Whole,
                            },
                            prepared @ RuntimeClass::AggregateValue { .. },
                        ) if pointee.shares_runtime_rep_with(db, prepared) => {
                            Some(prepared.clone())
                        }
                        _ => None,
                    },
                )
                .collect::<Vec<_>>();
            params
                .iter()
                .any(Option::is_some)
                .then_some((instance, params))
        })
        .collect::<FxHashMap<_, _>>();
    if reified_params.is_empty() {
        return;
    }

    fn adapt_args<'db>(
        db: &'db DriverDataBase,
        locals: &mut Vec<RLocal<'db>>,
        emitted: &mut Vec<RStmt<'db>>,
        args: &mut [RLocalId],
        params: &[Option<RuntimeClass<'db>>],
    ) {
        if args.len() != params.len() {
            return;
        }
        for (arg, target) in args.iter_mut().zip(params) {
            let Some(target) = target else {
                continue;
            };
            let source = *arg;
            let Some(local) = locals.get(source.as_u32() as usize) else {
                continue;
            };
            let RuntimeCarrier::Value(RuntimeClass::Ref {
                pointee,
                kind:
                    RefKind::Const
                    | RefKind::Object
                    | RefKind::Provider {
                        space: AddressSpaceKind::Memory,
                        ..
                    },
                view: RefView::Whole,
            }) = &local.carrier
            else {
                continue;
            };
            if !pointee.shares_runtime_rep_with(db, target) {
                continue;
            }
            let value = RLocalId::from_u32(locals.len() as u32);
            locals.push(RLocal {
                semantic_ty: local.semantic_ty,
                carrier: RuntimeCarrier::Value(target.clone()),
                root: RuntimeLocalRoot::None,
            });
            emitted.push(RStmt::Assign {
                dst: value,
                expr: RExpr::Load {
                    place: RuntimePlace {
                        root: PlaceRoot::Ref(source),
                        path: Box::default(),
                    },
                },
            });
            *arg = value;
        }
    }

    for body in bodies.values_mut() {
        let (locals, blocks) = (&mut body.locals, &mut body.blocks);
        for block in blocks {
            let mut rewritten = Vec::with_capacity(block.stmts.len());
            for mut stmt in std::mem::take(&mut block.stmts) {
                if let RStmt::Assign {
                    expr: RExpr::Call { callee, args },
                    ..
                } = &mut stmt
                    && let Some(params) = reified_params.get(callee)
                {
                    adapt_args(db, locals, &mut rewritten, args, params);
                }
                rewritten.push(stmt);
            }
            if let RTerminator::TerminalCall { callee, args } = &mut block.terminator
                && let Some(params) = reified_params.get(callee)
            {
                adapt_args(db, locals, &mut rewritten, args, params);
            }
            block.stmts = rewritten;
        }
    }
}

/// Fold the exact compiler-internal round trip produced when a borrowed
/// aggregate call boundary is reified back to a flattened value boundary:
///
/// ```text
/// object = MaterializeToObject(value)
/// loaded = Load(*object)
/// ```
///
/// The two statements must be adjacent, the load must read the whole fresh
/// object, and all three aggregate representations must agree. Replacing the
/// load with `Use(value)` is therefore an identity, not an alias analysis. If
/// the object has no remaining use anywhere in the body, its now-dead internal
/// materialization is removed as well. This matters for shaders because an
/// otherwise pure record store inside a loop must not acquire a private arena
/// allocation merely by crossing a borrow-typed library API.
fn fold_fresh_materialize_load_roundtrips<'db>(
    db: &'db DriverDataBase,
    body: &mut RuntimeBody<'db>,
) {
    let mut bypassed = HashSet::new();
    {
        let (locals, blocks) = (&body.locals, &mut body.blocks);
        for block in blocks {
            let mut previous_materialization: Option<(RLocalId, RLocalId)> = None;
            for stmt in &mut block.stmts {
                let folded = match (&previous_materialization, &mut *stmt) {
                    (
                        Some((object, value)),
                        RStmt::Assign {
                            dst,
                            expr: RExpr::Load { place },
                        },
                    ) if matches!(place.root, PlaceRoot::Ref(root) if root == *object)
                        && place.path.is_empty() =>
                    {
                        let value_class = locals
                            .get(value.as_u32() as usize)
                            .and_then(|local| local.carrier.value_class());
                        let object_class = locals
                            .get(object.as_u32() as usize)
                            .and_then(|local| local.carrier.value_class());
                        let loaded_class = locals
                            .get(dst.as_u32() as usize)
                            .and_then(|local| local.carrier.value_class());
                        let compatible_object = matches!(
                            object_class,
                            Some(RuntimeClass::Ref {
                                pointee,
                                kind: RefKind::Object,
                                view: RefView::Whole,
                            }) if value_class.is_some_and(|value_class| {
                                value_class.shares_runtime_rep_with(db, pointee)
                            })
                        );
                        if compatible_object
                            && value_class
                                .zip(loaded_class)
                                .is_some_and(|(value, loaded)| {
                                    value.shares_runtime_rep_with(db, loaded)
                                })
                        {
                            *stmt = RStmt::Assign {
                                dst: *dst,
                                expr: RExpr::Use(*value),
                            };
                            bypassed.insert(*object);
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                previous_materialization = if folded {
                    None
                } else if let RStmt::Assign {
                    dst,
                    expr: RExpr::MaterializeToObject { src },
                } = stmt
                {
                    Some((*dst, *src))
                } else {
                    None
                };
            }
        }
    }
    if bypassed.is_empty() {
        return;
    }

    let used = collect_used_locals(body);
    let removable = bypassed
        .into_iter()
        .filter(|candidate| {
            if used.contains_key(candidate) {
                return false;
            }
            let mut definitions = 0_usize;
            let only_materializations = body.blocks.iter().all(|block| {
                block.stmts.iter().all(|stmt| match stmt {
                    RStmt::Assign { dst, expr } if dst == candidate => {
                        definitions += 1;
                        matches!(expr, RExpr::MaterializeToObject { .. })
                    }
                    _ => true,
                })
            });
            definitions != 0 && only_materializations
        })
        .collect::<HashSet<_>>();
    if removable.is_empty() {
        return;
    }
    for block in &mut body.blocks {
        block.stmts.retain(|stmt| {
            !matches!(stmt, RStmt::Assign { dst, expr: RExpr::MaterializeToObject { .. } }
                if removable.contains(dst))
        });
    }
    for local in removable {
        body.locals[local.as_u32() as usize].carrier = RuntimeCarrier::Erased;
        body.locals[local.as_u32() as usize].root = RuntimeLocalRoot::None;
    }
}

fn drop_dead_pure_aggregate_values<'db>(db: &'db DriverDataBase, body: &mut RuntimeBody<'db>) {
    fn is_pure_aggregate_def(expr: &RExpr<'_>) -> bool {
        matches!(
            expr,
            RExpr::Use(_)
                | RExpr::AggregateMake { .. }
                | RExpr::ConstRef { .. }
                | RExpr::Placeholder { .. }
        )
    }

    let params: FxHashMap<RLocalId, ()> = body
        .signature
        .params
        .iter()
        .map(|param| (param.local, ()))
        .collect();

    loop {
        let used = collect_used_locals(body);
        // A destination is eligible for deletion only when EVERY assignment to it
        // is a pure aggregate def. A multi-definition local (for example a pure
        // aggregate def plus an effectful `Call` def to the same runtime
        // destination) must keep all of its assignments: deleting the effectful
        // def on the strength of one pure def would drop the side effect. Fold
        // over every def of a candidate dst, disqualifying it if any is impure.
        let mut candidates: FxHashMap<RLocalId, bool> = FxHashMap::default();
        for block in &body.blocks {
            for stmt in &block.stmts {
                let RStmt::Assign { dst, expr } = stmt else {
                    continue;
                };
                if used.contains_key(dst) || params.contains_key(dst) {
                    continue;
                }
                let RuntimeCarrier::Value(RuntimeClass::AggregateValue { layout }) =
                    body.locals[dst.as_u32() as usize].carrier
                else {
                    continue;
                };
                if !matches!(layout.data(db), Layout::Array(_) | Layout::Enum(_)) {
                    continue;
                }
                let all_pure = candidates.entry(*dst).or_insert(true);
                *all_pure &= is_pure_aggregate_def(expr);
            }
        }
        let dead: Vec<RLocalId> = candidates
            .into_iter()
            .filter_map(|(dst, all_pure)| all_pure.then_some(dst))
            .collect();
        if dead.is_empty() {
            return;
        }
        let dead_set: FxHashMap<RLocalId, ()> = dead.iter().map(|id| (*id, ())).collect();
        for block in &mut body.blocks {
            block.stmts.retain(
                |stmt| !matches!(stmt, RStmt::Assign { dst, .. } if dead_set.contains_key(dst)),
            );
        }
        for id in dead {
            body.locals[id.as_u32() as usize].carrier = RuntimeCarrier::Erased;
        }
    }
}

fn drop_dead_scalar_constants_and_copies(body: &mut RuntimeBody<'_>) {
    let params = body
        .signature
        .params
        .iter()
        .map(|param| param.local)
        .collect::<HashSet<_>>();
    loop {
        let used = collect_used_locals(body);
        let mut dead = Vec::new();
        for block in &body.blocks {
            for stmt in &block.stmts {
                let RStmt::Assign { dst, expr } = stmt else {
                    continue;
                };
                if params.contains(dst) || used.contains_key(dst) {
                    continue;
                }
                if matches!(body.value_class(*dst), Some(RuntimeClass::Scalar(_)))
                    && matches!(expr, RExpr::ConstScalar(_) | RExpr::Use(_))
                {
                    dead.push(*dst);
                }
            }
        }
        if dead.is_empty() {
            break;
        }
        let dead_set = dead.iter().copied().collect::<HashSet<_>>();
        for block in &mut body.blocks {
            block.stmts.retain(
                |stmt| !matches!(stmt, RStmt::Assign { dst, .. } if dead_set.contains(dst)),
            );
        }
        for local in dead {
            body.locals[local.as_u32() as usize].carrier = RuntimeCarrier::Erased;
            body.locals[local.as_u32() as usize].root = RuntimeLocalRoot::None;
        }
    }
}

fn normalize_portable_body<'db>(
    db: &'db DriverDataBase,
    instance: RuntimeInstance<'db>,
    body: &mut RuntimeBody<'db>,
) {
    reify_static_aggregate_params(db, body);
    repair_scalar_semantic_types_from_array_projections(db, body);
    lower_usize_array_place_classes(db, body);
    narrow_usize_scalars(db, instance, body);
    drop_dead_pure_aggregate_values(db, body);
    drop_dead_scalar_constants_and_copies(body);
}

struct PortableModuleLowerer<'db, 'a, I>
where
    I: Isa<InstSet = NativeInstSet>,
{
    db: &'db DriverDataBase,
    builder: ModuleBuilder,
    isa: &'a I,
    package: &'a RuntimePackage<'db>,
    prepared_bodies: FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
    func_symbols: FxHashMap<RuntimeInstance<'db>, String>,
    func_map: FxHashMap<RuntimeInstance<'db>, FuncRef>,
    resource_element_cache: FxHashMap<TyId<'db>, GpuResourceElementType>,
    resource_type_cache: FxHashMap<TyId<'db>, Type>,
    /// By-value aggregate parameters selected for the private indirect ABI.
    /// Selection is derived from the complete flattened signature and the
    /// validated core-Wasm arity limit. The caller materializes a fresh arena
    /// copy, preserving Fe value semantics, and the callee receives one i32.
    indirect_aggregate_params: FxHashMap<RuntimeInstance<'db>, HashSet<RLocalId>>,
    /// Private functions whose oversized aggregate result is transferred as one
    /// arena pointer. The caller's enclosing arena lifetime owns that fresh
    /// value; host-visible results never use this internal representation.
    indirect_aggregate_returns: HashSet<RuntimeInstance<'db>>,
    /// Per-body aggregate values represented by one canonical-arena pointer.
    /// This includes selected private parameters/results and oversized local
    /// products whose flattened SSA form would exhaust Wasm's local budget.
    address_carried_aggregate_values: FxHashMap<RuntimeInstance<'db>, HashSet<RLocalId>>,
    /// Compiler-derived continuation segments. Their symbols and typed bodies
    /// come exclusively from the MIR suspension machine; no manifest or host
    /// entry table participates in declaration or lowering.
    resumable_continuations: Vec<PreparedResumableContinuation<'db>>,
    wrapped_lane_names: HashSet<String>,
    validate_host_enum_params: bool,
    /// Bodies whose arena-backed locals are compiler-proven not to escape.
    /// Only the Wasm lowering enables these scopes. Shader and native paths do
    /// not emit arena-control instructions their backends cannot realize.
    scoped_arena_bodies: HashSet<RuntimeInstance<'db>>,
    /// Private bodies proven not to let an arena-backed value or reference
    /// escape. Unlike whole-function reclamation, this proof follows only call
    /// edges that actually carry an arena address, so unrelated scalar helpers
    /// cannot invalidate an otherwise safe private ABI boundary.
    indirect_aggregate_safe_bodies: HashSet<RuntimeInstance<'db>>,
    /// Per-body address provenance for canonical-arena objects. Parameters
    /// enter this set only when every internal call site supplies a value
    /// derived from an arena allocation. Public/raw entry parameters are never
    /// trusted merely because their flattened Wasm carrier is `i32`.
    arena_owned_locals: FxHashMap<RuntimeInstance<'db>, HashSet<RLocalId>>,
}

struct PreparedResumableContinuation<'db> {
    symbol: String,
    linkage: RuntimeLinkage,
    body: RuntimeBody<'db>,
    func_ref: Option<FuncRef>,
}

struct ScopedArenaAnalysis<'db> {
    allocates: bool,
    callees: HashSet<RuntimeInstance<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ArenaPointerFieldKey<'db> {
    root_layout: LayoutId<'db>,
    fields: Box<[u16]>,
}

#[derive(Clone, Copy, Debug)]
enum GpuResourceElementType {
    U32,
    Record { ty: Type, fields: usize },
}

impl GpuResourceElementType {
    fn ty(self) -> Type {
        match self {
            Self::U32 => Type::I32,
            Self::Record { ty, .. } => ty,
        }
    }
}

impl<'db, 'a, I> PortableModuleLowerer<'db, 'a, I>
where
    I: Isa<InstSet = NativeInstSet>,
{
    fn new(
        db: &'db DriverDataBase,
        builder: ModuleBuilder,
        isa: &'a I,
        package: &'a RuntimePackage<'db>,
        wrapped_lane_names: HashSet<String>,
        export_aliases: &[(String, String)],
        validate_host_enum_params: bool,
        enable_scoped_arena: bool,
    ) -> Result<Self, LowerError> {
        wasm_lower_trace(|| "prepare inline value bodies".to_owned());
        let mut prepared_bodies = prepare_inline_value_bodies(db, package).bodies;
        wasm_lower_trace(|| format!("prepared {} inline value bodies", prepared_bodies.len()));
        for (instance, body) in &mut prepared_bodies {
            normalize_portable_body(db, *instance, body);
        }
        wasm_lower_trace(|| "normalized portable bodies".to_owned());
        reify_residual_call_arguments(db, package, &mut prepared_bodies);
        wasm_lower_trace(|| "reified residual call arguments".to_owned());
        for body in prepared_bodies.values_mut() {
            fold_fresh_materialize_load_roundtrips(db, body);
        }
        wasm_lower_trace(|| "folded materialize-load roundtrips".to_owned());
        let mut func_symbols = assign_sonatina_function_symbols(db, package);
        for function in package.functions(db) {
            let instance = function.instance(db);
            let assigned = func_symbols.get(&instance).map(String::as_str);
            let declared = function.symbol(db);
            if let Some((_, export)) = export_aliases
                .iter()
                .find(|(source, _)| source == &declared || Some(source.as_str()) == assigned)
            {
                func_symbols.insert(instance, export.clone());
            }
        }
        let plans = mir::derive_runtime_resumable_plans(db, *package).map_err(|error| {
            LowerError::Unsupported(format!(
                "failed to derive Wasm continuation frames: {error:?}"
            ))
        })?;
        let mut resumable_continuations = Vec::new();
        for plan in &plans {
            let continuation_linkage = package
                .functions(db)
                .into_iter()
                .find(|function| function.instance(db) == plan.body)
                .map(|function| function.linkage(db))
                .ok_or_else(|| {
                    LowerError::Internal(format!(
                        "resumable plan `{}` has no runtime function declaration",
                        mir::runtime_instance_symbol_key(db, plan.body)
                    ))
                })?;
            let machine =
                mir::materialize_runtime_resumable_machine(db, plan).map_err(|error| {
                    LowerError::Unsupported(format!(
                        "Wasm resumable stack materialization is incomplete for `{}`: {error:?}",
                        func_symbols
                            .get(&plan.body)
                            .cloned()
                            .unwrap_or_else(|| mir::runtime_instance_symbol_key(db, plan.body))
                    ))
                })?;
            let authored_symbol = func_symbols
                .get(&plan.body)
                .cloned()
                .unwrap_or_else(|| mir::runtime_instance_symbol_key(db, plan.body));
            let start_symbol = format!("__fe_task_start_{authored_symbol}");
            func_symbols.insert(plan.body, start_symbol);
            let mut entry_body = machine.entry.body;
            normalize_portable_body(db, plan.body, &mut entry_body);
            prepared_bodies.insert(plan.body, entry_body);
            for continuation in machine.continuations {
                let symbol = format!(
                    "__fe_task_resume_{authored_symbol}_{}",
                    continuation.continuation_state
                );
                let mut body = continuation.body;
                normalize_portable_body(db, plan.body, &mut body);
                resumable_continuations.push(PreparedResumableContinuation {
                    symbol,
                    linkage: continuation_linkage.clone(),
                    body,
                    func_ref: None,
                });
            }
        }
        let mut lowerer = Self {
            db,
            builder,
            isa,
            package,
            prepared_bodies,
            func_symbols,
            func_map: FxHashMap::default(),
            resource_element_cache: FxHashMap::default(),
            resource_type_cache: FxHashMap::default(),
            indirect_aggregate_params: FxHashMap::default(),
            indirect_aggregate_returns: HashSet::new(),
            address_carried_aggregate_values: FxHashMap::default(),
            resumable_continuations,
            wrapped_lane_names,
            validate_host_enum_params,
            scoped_arena_bodies: HashSet::new(),
            indirect_aggregate_safe_bodies: HashSet::new(),
            arena_owned_locals: FxHashMap::default(),
        };
        let (indirect_params, indirect_returns) = lowerer.derive_indirect_aggregate_abi()?;
        lowerer.indirect_aggregate_params = indirect_params;
        lowerer.indirect_aggregate_returns = indirect_returns;
        lowerer.address_carried_aggregate_values =
            lowerer.derive_address_carried_aggregate_values();
        if enable_scoped_arena {
            wasm_lower_trace(|| "derive typed arena provenance".to_owned());
            lowerer.arena_owned_locals = lowerer.derive_arena_owned_locals();
            lowerer.indirect_aggregate_safe_bodies =
                lowerer.derive_indirect_aggregate_safe_bodies();
            wasm_lower_trace(|| "derive scoped arena bodies".to_owned());
            lowerer.scoped_arena_bodies = lowerer.derive_scoped_arena_bodies();
            wasm_lower_trace(|| {
                format!(
                    "derived scoped arena bodies, scoped={}",
                    lowerer.scoped_arena_bodies.len(),
                )
            });
        }
        Ok(lowerer)
    }

    fn finish(self) -> Module {
        self.builder.build()
    }

    /// SPIR-V derives its kernel ABI from the first declared function. Runtime
    /// package planning is call-graph ordered, so private helpers can otherwise
    /// precede the object section entry. Keep actual section entries first while
    /// preserving package order within the entry and non-entry partitions.
    fn functions_in_declaration_order(&self) -> Vec<RuntimeFunction<'db>> {
        let entries = self
            .package
            .root_objects(self.db)
            .into_iter()
            .flat_map(|object| object.sections(self.db))
            .map(|section| section.entry.instance(self.db))
            .collect::<HashSet<_>>();
        let mut functions = self.package.functions(self.db);
        functions.sort_by_key(|function| !entries.contains(&function.instance(self.db)));
        functions
    }

    fn effective_linkage(&self, function: RuntimeFunction<'db>) -> Linkage {
        let symbol = self.function_symbol(function.instance(self.db));
        if self.wrapped_lane_names.contains(&symbol) {
            Linkage::Private
        } else {
            linkage_for_runtime(function.linkage(self.db))
        }
    }

    /// Select the smallest deterministic set of private by-value aggregate
    /// parameters needed to fit each flattened signature under core Wasm's
    /// validated parameter limit. The largest savings are selected first. A
    /// public, external, non-memory-lowerable, or still-oversized signature
    /// fails before Sonatina emits an invalid module.
    fn derive_indirect_aggregate_abi(
        &self,
    ) -> Result<
        (
            FxHashMap<RuntimeInstance<'db>, HashSet<RLocalId>>,
            HashSet<RuntimeInstance<'db>>,
        ),
        LowerError,
    > {
        let mut selected = FxHashMap::default();
        let mut indirect_returns = HashSet::new();
        for function in self.functions_in_declaration_order() {
            let instance = function.instance(self.db);
            if gpu_intrinsic(self.db, instance).is_some()
                || mir::runtime_control_effect_kind(self.db, instance).is_some()
            {
                continue;
            }
            let body = self
                .prepared_bodies
                .get(&instance)
                .cloned()
                .unwrap_or_else(|| instance.body(self.db));
            let symbol = self.function_symbol(instance);
            let linkage = self.effective_linkage(function);
            let mut arities = Vec::with_capacity(body.signature.params.len());
            let mut total = 0usize;
            for param in &body.signature.params {
                let arity = if body
                    .local(param.local)
                    .is_some_and(|local| semantic_gpu_resource(self.db, local.semantic_ty))
                {
                    1
                } else {
                    self.scalar_tuple_element_tys(&param.class)
                        .map_or(1, |leaves| leaves.len())
                };
                total = total.checked_add(arity).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "Wasm function `{symbol}` has a flattened parameter arity overflow"
                    ))
                })?;
                arities.push((param.local, param.class.clone(), arity));
            }
            let return_arity = body
                .signature
                .ret
                .as_ref()
                .and_then(|class| self.scalar_tuple_element_tys(class))
                .map_or_else(
                    || usize::from(body.signature.ret.is_some()),
                    |leaves| leaves.len(),
                );
            if return_arity > MAX_WASM_FUNCTION_RETURNS {
                let return_class = body.signature.ret.as_ref().ok_or_else(|| {
                    LowerError::Internal(format!(
                        "Wasm function `{symbol}` has result arity without a result class"
                    ))
                })?;
                if matches!(linkage, Linkage::Private)
                    && matches!(return_class, RuntimeClass::AggregateValue { .. })
                    && self.aggregate_is_memory_lowerable(return_class)
                {
                    indirect_returns.insert(instance);
                    wasm_lower_trace(|| {
                        format!(
                            "derived private aggregate result ABI, symbol={symbol}, flattened_returns={return_arity}"
                        )
                    });
                } else {
                    return Err(LowerError::Unsupported(format!(
                        "Wasm function `{symbol}` returns {return_arity} flattened values, exceeding the validated limit of {MAX_WASM_FUNCTION_RETURNS}; expose typed caller-owned storage or keep the function private"
                    )));
                }
            }
            if total <= MAX_WASM_FUNCTION_PARAMS {
                continue;
            }
            let mut candidates = arities
                .iter()
                .filter_map(|(local, class, arity)| {
                    (matches!(linkage, Linkage::Private)
                        && *arity > 1
                        && matches!(class, RuntimeClass::AggregateValue { .. })
                        && self.aggregate_is_memory_lowerable(class))
                    .then_some((*arity - 1, *local))
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| left.1.as_u32().cmp(&right.1.as_u32()))
            });
            let mut function_selected = HashSet::new();
            for (savings, local) in candidates {
                function_selected.insert(local);
                total -= savings;
                if total <= MAX_WASM_FUNCTION_PARAMS {
                    break;
                }
            }
            if total > MAX_WASM_FUNCTION_PARAMS {
                return Err(LowerError::Unsupported(format!(
                    "Wasm function `{symbol}` requires {total} flattened parameters after every eligible private aggregate is indirect, exceeding the validated limit of {MAX_WASM_FUNCTION_PARAMS}"
                )));
            }
            wasm_lower_trace(|| {
                format!(
                    "derived private aggregate ABI, symbol={symbol}, indirect={}, parameters={total}",
                    function_selected.len(),
                )
            });
            selected.insert(instance, function_selected);
        }
        Ok((selected, indirect_returns))
    }

    /// Derive every aggregate local represented by one arena pointer. Private
    /// indirect parameters/results seed the set, as do memory-lowerable local
    /// products above the validated flattening boundary. Plain `Use` bindings
    /// propagate the representation to a fixed point. This keeps the choice
    /// structural and type-driven, with no proof- or function-name exceptions.
    fn derive_address_carried_aggregate_values(
        &self,
    ) -> FxHashMap<RuntimeInstance<'db>, HashSet<RLocalId>> {
        let mut derived = FxHashMap::default();
        let mut total = 0usize;
        for (instance, body) in &self.prepared_bodies {
            let indirect_params = self
                .indirect_aggregate_params
                .get(instance)
                .cloned()
                .unwrap_or_default();
            let values = self.derive_body_address_carried_aggregate_values(body, &indirect_params);
            total += values.len();
            if !values.is_empty() {
                derived.insert(*instance, values);
            }
        }
        wasm_lower_trace(|| {
            format!(
                "derived address-carried aggregate values, bodies={}, values={total}",
                derived.len(),
            )
        });
        derived
    }

    fn derive_body_address_carried_aggregate_values(
        &self,
        body: &RuntimeBody<'db>,
        indirect_params: &HashSet<RLocalId>,
    ) -> HashSet<RLocalId> {
        let mut values = indirect_params.clone();
        for (index, local) in body.locals.iter().enumerate() {
            if !matches!(local.root, RuntimeLocalRoot::None) {
                continue;
            }
            let Some(class) = local.carrier.value_class() else {
                continue;
            };
            if !matches!(class, RuntimeClass::AggregateValue { .. })
                || !self.aggregate_is_memory_lowerable(class)
            {
                continue;
            }
            let Some(shape) = self.flat_shape(class) else {
                continue;
            };
            if shape.leaf_count() > MAX_WASM_FLATTENED_LOCAL_AGGREGATE {
                values.insert(RLocalId::from_u32(index as u32));
            }
        }
        loop {
            let mut changed = false;
            for block in &body.blocks {
                for stmt in &block.stmts {
                    let RStmt::Assign { dst, expr } = stmt else {
                        continue;
                    };
                    let indirect = match expr {
                        RExpr::Call { callee, .. } => {
                            self.indirect_aggregate_returns.contains(callee)
                        }
                        RExpr::Use(src) => values.contains(src),
                        _ => false,
                    };
                    if !indirect {
                        continue;
                    }
                    let Some(class) = body.value_class(*dst) else {
                        continue;
                    };
                    if matches!(class, RuntimeClass::AggregateValue { .. })
                        && self.aggregate_is_memory_lowerable(class)
                        && values.insert(*dst)
                    {
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        values
    }

    /// The symbol -> host-namespace side table for external declarations. Each
    /// non-builtin `extern` whose block carries
    /// `#[host_import(module = "...")]` maps its Sonatina symbol (which becomes
    /// the Wasm import's field name) to that module string. Attribute-less externs are
    /// omitted, so the WAFFLE backend falls back to the flat `"fe"` convention for
    /// them. Keyed by the same symbol the lowering assigns, so it matches the
    /// import name the backend reads.
    fn import_modules(&self) -> HashMap<String, String> {
        let mut modules = HashMap::new();
        for function in self.functions_in_declaration_order() {
            if function.linkage(self.db) != RuntimeLinkage::External {
                continue;
            }
            let instance = function.instance(self.db);
            if mir::runtime_control_effect_kind(self.db, instance).is_some() {
                continue;
            }
            if let Some(module) = mir::host_import_module(self.db, instance) {
                modules.insert(self.function_symbol(instance), module);
            }
        }
        modules
    }

    fn function_symbol(&self, instance: RuntimeInstance<'db>) -> String {
        // A DECLARED-EXTERNAL host import is named by its BASE op identifier (the
        // stable name the broker binds - "the import table IS the op set"), NOT the
        // internal Sonatina symbol, which is mangled per instance
        // (`std__lib__webgpu__raw__gpu_buffer_create_HASH`). The fork's WAFFLE emitter
        // uses this string verbatim as the wasm import field name.
        if let Some(name) = mir::host_import_name(self.db, instance) {
            return name;
        }
        self.func_symbols
            .get(&instance)
            .cloned()
            .or_else(|| {
                self.package
                    .functions(self.db)
                    .into_iter()
                    .find(|function| function.instance(self.db) == instance)
                    .map(|function| function.symbol(self.db).clone())
            })
            .unwrap_or_else(|| format!("{:?}", instance.key(self.db)))
    }

    fn gpu_resource_element_type(
        &mut self,
        resource_ty: TyId<'db>,
    ) -> Result<GpuResourceElementType, LowerError> {
        let resource_ty = resource_ty.as_view(self.db).unwrap_or(resource_ty);
        let [element_ty, _, ..] = resource_ty.generic_args(self.db) else {
            return Err(LowerError::Unsupported(
                "GPU storage resource type requires element and length arguments".to_owned(),
            ));
        };
        let element_ty = element_ty.as_view(self.db).unwrap_or(*element_ty);
        if let Some(element) = self.resource_element_cache.get(&element_ty).copied() {
            return Ok(element);
        }
        let element = if matches!(
            element_ty.base_ty(self.db).data(self.db),
            TyData::TyBase(TyBase::Prim(PrimTy::U32))
        ) {
            GpuResourceElementType::U32
        } else {
            let adt = element_ty.adt_def(self.db).ok_or_else(|| {
                LowerError::Unsupported(
                    "GPU storage elements must be u32 or POD records of u32 fields".to_owned(),
                )
            })?;
            let AdtRef::Struct(struct_) = adt.adt_ref(self.db) else {
                return Err(LowerError::Unsupported(
                    "GPU storage elements must be u32 or POD records of u32 fields".to_owned(),
                ));
            };
            let fields = element_ty.field_types(self.db);
            if fields.is_empty()
                || fields.iter().any(|field| {
                    !matches!(
                        field.base_ty(self.db).data(self.db),
                        TyData::TyBase(TyBase::Prim(PrimTy::U32))
                    )
                })
            {
                return Err(LowerError::Unsupported(
                    "GPU storage POD records must contain one or more u32 fields".to_owned(),
                ));
            }
            let name = struct_
                .name(self.db)
                .to_opt()
                .map(|name| name.data(self.db).to_string())
                .unwrap_or_else(|| {
                    format!("gpu_resource_record_{}", self.resource_element_cache.len())
                });
            let field_tys = vec![Type::I32; fields.len()];
            let ty = self.builder.declare_struct_type(&name, &field_tys, false);
            GpuResourceElementType::Record {
                ty,
                fields: fields.len(),
            }
        };
        self.resource_element_cache.insert(element_ty, element);
        Ok(element)
    }

    fn gpu_resource_type(&mut self, resource_ty: TyId<'db>) -> Result<Type, LowerError> {
        let resource_ty = resource_ty.as_view(self.db).unwrap_or(resource_ty);
        if let Some(ty) = self.resource_type_cache.get(&resource_ty).copied() {
            return Ok(ty);
        }
        if !semantic_gpu_resource(self.db, resource_ty) {
            return Err(LowerError::Internal(
                "non-resource semantic type reached GPU resource lowering".to_owned(),
            ));
        }
        let [_, length_ty, ..] = resource_ty.generic_args(self.db) else {
            return Err(LowerError::Unsupported(
                "GPU storage resource type requires element and length arguments".to_owned(),
            ));
        };
        let length = semantic_const_u32(self.db, *length_ty)
            .and_then(|length| usize::try_from(length).ok())
            .filter(|length| *length != 0)
            .ok_or_else(|| {
                LowerError::Unsupported(
                    "GPU storage resource length must be a concrete nonzero u32-sized integer"
                        .to_owned(),
                )
            })?;
        let element_ty = self.gpu_resource_element_type(resource_ty)?.ty();
        let array_ty = self.builder.declare_array_type(element_ty, length);
        let resource_ref_ty = self.builder.objref_type(array_ty);
        self.resource_type_cache
            .insert(resource_ty, resource_ref_ty);
        Ok(resource_ref_ty)
    }

    fn declare_functions(&mut self) -> Result<(), LowerError> {
        // DECLARED-EXTERNAL host imports dedup to ONE import per `(module, op-name)`
        // identity: the same `extern` reached through two effect-provider scopes
        // (e.g. `main`'s `Dispatch` + `Wait` vs `main_begin`'s `Dispatch`) mints two
        // instances, but they name the SAME broker op, so the import table must carry
        // it once. Each instance maps to the shared import `FuncRef`; bodyless imports
        // never lower a body (`lower_bodies` skips block-empty functions).
        let mut import_refs: FxHashMap<(String, String), FuncRef> = FxHashMap::default();
        for function in self.functions_in_declaration_order() {
            let instance = function.instance(self.db);
            if gpu_intrinsic(self.db, instance).is_some() {
                continue;
            }
            if mir::runtime_control_effect_kind(self.db, instance).is_some() {
                // The nominal declaration has already been consumed by the
                // compiler-derived state machine. It is neither a function nor
                // an import in the emitted module.
                continue;
            }
            if let Some(name) = mir::host_import_name(self.db, instance) {
                if let Some(descriptor) = mir::indirect_host_result(self.db, instance) {
                    let mut missing = Vec::new();
                    if descriptor.requires_realloc {
                        missing.push("realloc");
                    }
                    if descriptor.requires_post_return {
                        missing.push("post-return");
                    }
                    if !missing.is_empty() {
                        return Err(LowerError::Unsupported(format!(
                            "extern host import `{name}` uses indirect host result codec \
                             `{}`, but the Wasm backend is missing required \
                             capabilities: {}",
                            mir::IndirectHostResult::FE_HOST_WASM_PROTOCOL,
                            missing.join(", ")
                        )));
                    }
                }
                let module =
                    mir::host_import_module(self.db, instance).unwrap_or_else(|| "fe".to_string());
                let key = (module, name);
                if let Some(func_ref) = import_refs.get(&key) {
                    self.func_map.insert(instance, *func_ref);
                    continue;
                }
                let signature = self.lower_signature(function)?;
                let func_ref = self.builder.declare_function(signature).map_err(|err| {
                    LowerError::Internal(format!("failed to declare wasm function: {err}"))
                })?;
                import_refs.insert(key, func_ref);
                self.func_map.insert(instance, func_ref);
                continue;
            }
            let signature = self.lower_signature(function)?;
            let func_ref = self.builder.declare_function(signature).map_err(|err| {
                LowerError::Internal(format!("failed to declare wasm function: {err}"))
            })?;
            let inline_hint = match function.inline_hint(self.db) {
                RuntimeInlineHint::Auto => sonatina_ir::InlineHint::Auto,
                RuntimeInlineHint::Hint => sonatina_ir::InlineHint::Inline,
                RuntimeInlineHint::Always => sonatina_ir::InlineHint::Always,
                RuntimeInlineHint::Never => sonatina_ir::InlineHint::Never,
            };
            self.builder.ctx.set_inline_hint(func_ref, inline_hint);
            self.func_map.insert(instance, func_ref);
        }
        for index in 0..self.resumable_continuations.len() {
            let symbol = self.resumable_continuations[index].symbol.clone();
            let linkage = self.resumable_continuations[index].linkage.clone();
            let body = self.resumable_continuations[index].body.clone();
            let signature = self.lower_body_signature(
                &symbol,
                linkage_for_runtime(linkage),
                &body,
                &HashSet::new(),
                false,
            )?;
            let func_ref = self.builder.declare_function(signature).map_err(|err| {
                LowerError::Internal(format!(
                    "failed to declare Wasm continuation `{symbol}`: {err}"
                ))
            })?;
            self.resumable_continuations[index].func_ref = Some(func_ref);
        }
        Ok(())
    }

    fn lower_signature(&mut self, function: RuntimeFunction<'db>) -> Result<Signature, LowerError> {
        let instance = function.instance(self.db);
        let symbol = self.function_symbol(instance);
        let body = self
            .prepared_bodies
            .get(&instance)
            .cloned()
            .unwrap_or_else(|| instance.body(self.db));
        // R2.1: a scalar-tuple param/return FLATTENS into N wasm scalar
        // params/results (one per element word); every other param/return maps
        // 1:1 through `ty_for_class` exactly as before. The flattening order is
        // preserved so the prologue's running wasm-arg index matches, and a
        // scalar-tuple RETURN becomes a wasm multi-value result the host reads.
        let linkage = if self.wrapped_lane_names.contains(&symbol) {
            // The host ABI is a synthesized canonical or surface-frame
            // wrapper. Its underlying typed Fe lane remains an internal
            // implementation dependency even though it seeded the package.
            Linkage::Private
        } else {
            linkage_for_runtime(function.linkage(self.db))
        };
        let indirect_params = self
            .indirect_aggregate_params
            .get(&instance)
            .cloned()
            .unwrap_or_default();
        let indirect_return = self.indirect_aggregate_returns.contains(&instance);
        self.lower_body_signature(&symbol, linkage, &body, &indirect_params, indirect_return)
    }

    fn lower_body_signature(
        &mut self,
        symbol: &str,
        linkage: Linkage,
        body: &RuntimeBody<'db>,
        indirect_params: &HashSet<RLocalId>,
        indirect_return: bool,
    ) -> Result<Signature, LowerError> {
        let mut args = Vec::with_capacity(body.signature.params.len());
        for param in &body.signature.params {
            let semantic_ty = body
                .local(param.local)
                .map(|local| local.semantic_ty)
                .ok_or_else(|| {
                    LowerError::Internal("runtime parameter local is missing".to_owned())
                })?;
            if semantic_gpu_resource(self.db, semantic_ty) {
                args.push(self.gpu_resource_type(semantic_ty)?);
            } else if indirect_params.contains(&param.local) {
                args.push(Type::I32);
            } else if let Some(elem_tys) = self.scalar_tuple_element_tys(&param.class) {
                args.extend(elem_tys);
            } else if matches!(linkage, Linkage::Private)
                && self.is_memory_lowerable_object_ref(&param.class)
            {
                // A mutable owned aggregate receiver is object-backed inside
                // one generated Wasm module. Its caller has already
                // materialized an independent Fe value in the canonical arena,
                // and the function lowerer represents the receiver as that i32
                // arena address. Admit the same representation in PRIVATE
                // helper signatures so ordinary fluent value methods can edit
                // their owned copy. Public, continuation, and host-visible
                // signatures deliberately stay on the recursively flattened
                // value ABI: an arena pointer must never become application or
                // browser protocol.
                args.push(Type::I32);
            } else {
                args.push(self.ty_for_class(&param.class).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while declaring parameter local {:?} of Wasm function `{symbol}`",
                        param.local
                    )),
                    other => other,
                })?);
            }
        }
        let ret_tys: Vec<Type> = if indirect_return {
            vec![Type::I32]
        } else {
            match &body.signature.ret {
                None => Vec::new(),
                Some(class) => {
                    if let Some(elem_tys) = self.scalar_tuple_element_tys(class) {
                        elem_tys
                    } else {
                        vec![self.ty_for_class(class).map_err(|error| match error {
                            LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                                "{message}; while declaring the return of Wasm function `{symbol}`"
                            )),
                            other => other,
                        })?]
                    }
                }
            }
        };
        Ok(Signature::new(symbol, linkage, &args, &ret_tys))
    }

    fn lower_bodies(&mut self) -> Result<(), LowerError> {
        let functions = self.package.functions(self.db);
        let total = functions.len();
        for (index, function) in functions.into_iter().enumerate() {
            let instance = function.instance(self.db);
            if gpu_intrinsic(self.db, instance).is_some() {
                continue;
            }
            let symbol = self.function_symbol(instance);
            wasm_lower_trace_detail(|| format!("lower function {}/{total}: {symbol}", index + 1));
            if (index + 1) % 500 == 0 || index + 1 == total {
                wasm_lower_trace(|| {
                    format!("lower function progress, completed={}/{total}", index + 1)
                });
            }
            let body = self
                .prepared_bodies
                .get(&instance)
                .cloned()
                .unwrap_or_else(|| instance.body(self.db));
            if body.blocks.is_empty() {
                continue;
            }
            let func_ref = *self.func_map.get(&instance).ok_or_else(|| {
                LowerError::Internal("wasm function lowered before it was declared".to_string())
            })?;
            let validate_enum_params = self.validate_host_enum_params
                && function.linkage(self.db) == RuntimeLinkage::Internal
                && !self.wrapped_lane_names.contains(&symbol);
            let scoped_arena = self.scoped_arena_bodies.contains(&instance);
            let indirect_aggregate_params = self
                .indirect_aggregate_params
                .get(&instance)
                .cloned()
                .unwrap_or_default();
            let indirect_aggregate_return = self.indirect_aggregate_returns.contains(&instance);
            let started = Instant::now();
            let lowered = PortableFunctionLowerer::new(
                self,
                body,
                func_ref,
                validate_enum_params,
                scoped_arena,
                indirect_aggregate_params,
                indirect_aggregate_return,
            )?
            .lower();
            let elapsed = started.elapsed();
            if elapsed.as_secs() >= 1 {
                wasm_lower_trace(|| {
                    format!(
                        "slow function lowering, symbol={symbol}, elapsed_ms={}",
                        elapsed.as_millis()
                    )
                });
            }
            lowered.map_err(|error| match error {
                LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                    "{message}; while lowering Wasm function `{symbol}`"
                )),
                LowerError::Internal(message) => LowerError::Internal(format!(
                    "{message}; while lowering Wasm function `{symbol}`"
                )),
                other => other,
            })?;
        }
        for index in 0..self.resumable_continuations.len() {
            let symbol = self.resumable_continuations[index].symbol.clone();
            let body = self.resumable_continuations[index].body.clone();
            let func_ref = self.resumable_continuations[index]
                .func_ref
                .ok_or_else(|| {
                    LowerError::Internal(format!(
                        "Wasm continuation `{symbol}` lowered before declaration"
                    ))
                })?;
            PortableFunctionLowerer::new(self, body, func_ref, true, false, HashSet::new(), false)?
                .lower()
                .map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while lowering compiler-derived Wasm continuation `{symbol}`"
                    )),
                    LowerError::Internal(message) => LowerError::Internal(format!(
                        "{message}; while lowering compiler-derived Wasm continuation `{symbol}`"
                    )),
                    other => other,
                })?;
        }
        Ok(())
    }

    #[cfg(feature = "sonatina-indirect-calls")]
    fn synthesize_guest_callbacks(
        &mut self,
        callbacks: &[crate::guest_callbacks::ResolvedGuestCallback],
    ) -> Result<(), LowerError> {
        use smallvec1::smallvec;

        fn core_ty(ty: fe_host_abi::CoreType) -> Result<Type, LowerError> {
            Ok(match ty {
                fe_host_abi::CoreType::I32 => Type::I32,
                fe_host_abi::CoreType::I64 => Type::I64,
                fe_host_abi::CoreType::F32 => Type::F32,
                fe_host_abi::CoreType::F64 => {
                    return Err(LowerError::Unsupported(
                        "f64 guest callbacks await an f64 Sonatina carrier".into(),
                    ));
                }
            })
        }

        for (index, callback) in callbacks.iter().enumerate() {
            // Rich and async lanes never reach this resolved scalar manifest.
            // Keep this first materializer deliberately single-result.
            if callback.core_params.len() != 1 || callback.core_results.len() > 1 {
                return Err(LowerError::Unsupported(
                    "guest callback trampolines currently require one scalar parameter and at \
                     most one scalar result"
                        .into(),
                ));
            }
            let target = self
                .func_symbols
                .iter()
                .find_map(|(instance, symbol)| {
                    (symbol == &callback.runtime_symbol)
                        .then(|| self.func_map.get(instance).copied())
                        .flatten()
                })
                .ok_or_else(|| {
                    LowerError::Internal(format!(
                        "resolved guest callback target `{}` was not declared",
                        callback.runtime_symbol
                    ))
                })?;
            let params = callback
                .core_params
                .iter()
                .copied()
                .map(core_ty)
                .collect::<Result<Vec<_>, _>>()?;
            let results = callback
                .core_results
                .iter()
                .copied()
                .map(core_ty)
                .collect::<Result<Vec<_>, _>>()?;
            let function_ty = self
                .builder
                .ctx
                .with_ty_store_mut(|types| types.make_func(&params, &results));
            let function_ptr_ty = function_ty.to_ptr(&self.builder.ctx);
            let prefix = format!("fe_guest_callback_{index}");
            let generation = self.builder.declare_gv(GlobalVariableData::new(
                format!("{prefix}_generation"),
                Type::I32,
                Linkage::Private,
                false,
                Some(GvInitializer::make_imm(0i32)),
            ));
            let occupied = self.builder.declare_gv(GlobalVariableData::new(
                format!("{prefix}_occupied"),
                Type::I32,
                Linkage::Private,
                false,
                Some(GvInitializer::make_imm(0i32)),
            ));

            // register() -> token. One manifest entry owns one reusable slot;
            // generation is bumped before every re-registration.
            let register = self
                .builder
                .declare_function(Signature::new_single(
                    &format!("{prefix}_register"),
                    Linkage::Public,
                    &[],
                    Type::I32,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(register);
                let entry = fb.append_block();
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let occupied_addr = fb.make_global_value(occupied);
                let occupied_value = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), occupied_addr, Type::I32),
                    Type::I32,
                );
                let zero = fb.make_imm_value(Immediate::I32(0));
                let vacant = fb.insert_inst(
                    CmpEq::new(self.isa.inst_set(), occupied_value, zero),
                    Type::I1,
                );
                fb.insert_inst_no_result(Br::new(self.isa.inst_set(), vacant, valid, invalid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(self.isa.inst_set()));
                fb.switch_to_block(valid);
                let generation_addr = fb.make_global_value(generation);
                let old_generation = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), generation_addr, Type::I32),
                    Type::I32,
                );
                let one = fb.make_imm_value(Immediate::I32(1));
                let next = fb.insert_inst(
                    Add::new(self.isa.inst_set(), old_generation, one),
                    Type::I32,
                );
                fb.insert_inst_no_result(Mstore::new(
                    self.isa.inst_set(),
                    generation_addr,
                    next,
                    Type::I32,
                ));
                fb.insert_inst_no_result(Mstore::new(
                    self.isa.inst_set(),
                    occupied_addr,
                    one,
                    Type::I32,
                ));
                let scale = fb.make_imm_value(Immediate::I32(1 << 16));
                let generation_bits =
                    fb.insert_inst(Mul::new(self.isa.inst_set(), next, scale), Type::I32);
                let token = fb.insert_inst(
                    Add::new(self.isa.inst_set(), generation_bits, one),
                    Type::I32,
                );
                fb.insert_return(token);
                fb.seal_all();
                fb.finish();
            }

            let mut trampoline_params = vec![Type::I32];
            trampoline_params.extend_from_slice(&params);

            // Expose the opaque Wasm table index separately from the lifetime
            // token. Hosts may retain it as dispatch metadata, but registration
            // authority is still the generation-checked token below.
            let pointer_export = self
                .builder
                .declare_function(Signature::new_single(
                    &format!("{prefix}_table_slot"),
                    Linkage::Public,
                    &[],
                    function_ptr_ty,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(pointer_export);
                let entry = fb.append_block();
                fb.switch_to_block(entry);
                let pointer = fb.insert_inst(
                    GetFunctionPtr::new(self.isa.inst_set(), target),
                    function_ptr_ty,
                );
                fb.insert_return(pointer);
                fb.seal_all();
                fb.finish();
            }

            let mut raw_params = vec![function_ptr_ty];
            raw_params.extend_from_slice(&params);
            let raw_invoke = self
                .builder
                .declare_function(Signature::new(
                    &format!("{prefix}_invoke_raw"),
                    Linkage::Public,
                    &raw_params,
                    &results,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(raw_invoke);
                let entry = fb.append_block();
                fb.switch_to_block(entry);
                let args = fb.args().to_vec();
                let call = CallIndirect::new(
                    self.isa.inst_set(),
                    args[0],
                    function_ptr_ty,
                    smallvec![args[1]],
                );
                if results.is_empty() {
                    fb.insert_inst_no_result(call);
                    fb.insert_return_unit();
                } else {
                    let result = fb.insert_inst(call, results[0]);
                    fb.insert_return(result);
                }
                fb.seal_all();
                fb.finish();
            }

            let invoke = self
                .builder
                .declare_function(Signature::new(
                    &format!("{prefix}_invoke"),
                    Linkage::Public,
                    &trampoline_params,
                    &results,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(invoke);
                let entry = fb.append_block();
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let args = fb.args().to_vec();
                let occupied_addr = fb.make_global_value(occupied);
                let generation_addr = fb.make_global_value(generation);
                let occupied_value = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), occupied_addr, Type::I32),
                    Type::I32,
                );
                let current_generation = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), generation_addr, Type::I32),
                    Type::I32,
                );
                let one = fb.make_imm_value(Immediate::I32(1));
                let scale = fb.make_imm_value(Immediate::I32(1 << 16));
                let generation_bits = fb.insert_inst(
                    Mul::new(self.isa.inst_set(), current_generation, scale),
                    Type::I32,
                );
                let expected = fb.insert_inst(
                    Add::new(self.isa.inst_set(), generation_bits, one),
                    Type::I32,
                );
                let token_ok =
                    fb.insert_inst(CmpEq::new(self.isa.inst_set(), args[0], expected), Type::I1);
                let occupied_ok = fb.insert_inst(
                    CmpEq::new(self.isa.inst_set(), occupied_value, one),
                    Type::I1,
                );
                let valid_token = fb.insert_inst(
                    And::new(self.isa.inst_set(), token_ok, occupied_ok),
                    Type::I1,
                );
                fb.insert_inst_no_result(Br::new(self.isa.inst_set(), valid_token, valid, invalid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(self.isa.inst_set()));
                fb.switch_to_block(valid);
                let callee = fb.insert_inst(
                    GetFunctionPtr::new(self.isa.inst_set(), target),
                    function_ptr_ty,
                );
                let call = CallIndirect::new(
                    self.isa.inst_set(),
                    callee,
                    function_ptr_ty,
                    smallvec![args[1]],
                );
                if results.is_empty() {
                    fb.insert_inst_no_result(call);
                    fb.insert_return_unit();
                } else {
                    let result = fb.insert_inst(call, results[0]);
                    fb.insert_return(result);
                }
                fb.seal_all();
                fb.finish();
            }

            let release = self
                .builder
                .declare_function(Signature::new(
                    &format!("{prefix}_release"),
                    Linkage::Public,
                    &[Type::I32],
                    &[],
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(release);
                let entry = fb.append_block();
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let token = fb.args()[0];
                let occupied_addr = fb.make_global_value(occupied);
                let generation_addr = fb.make_global_value(generation);
                let occupied_value = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), occupied_addr, Type::I32),
                    Type::I32,
                );
                let current_generation = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), generation_addr, Type::I32),
                    Type::I32,
                );
                let one = fb.make_imm_value(Immediate::I32(1));
                let scale = fb.make_imm_value(Immediate::I32(1 << 16));
                let generation_bits = fb.insert_inst(
                    Mul::new(self.isa.inst_set(), current_generation, scale),
                    Type::I32,
                );
                let expected = fb.insert_inst(
                    Add::new(self.isa.inst_set(), generation_bits, one),
                    Type::I32,
                );
                let token_ok =
                    fb.insert_inst(CmpEq::new(self.isa.inst_set(), token, expected), Type::I1);
                let occupied_ok = fb.insert_inst(
                    CmpEq::new(self.isa.inst_set(), occupied_value, one),
                    Type::I1,
                );
                let valid_token = fb.insert_inst(
                    And::new(self.isa.inst_set(), token_ok, occupied_ok),
                    Type::I1,
                );
                fb.insert_inst_no_result(Br::new(self.isa.inst_set(), valid_token, valid, invalid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(self.isa.inst_set()));
                fb.switch_to_block(valid);
                let zero = fb.make_imm_value(Immediate::I32(0));
                fb.insert_inst_no_result(Mstore::new(
                    self.isa.inst_set(),
                    occupied_addr,
                    zero,
                    Type::I32,
                ));
                fb.insert_return_unit();
                fb.seal_all();
                fb.finish();
            }
        }
        Ok(())
    }

    fn synthesize_resident_transition(
        &mut self,
        transition: &super::WasmResidentTransition,
        initializer: Option<&super::WasmResidentInitializer>,
        projection: Option<&super::WasmResidentProjection>,
    ) -> Result<(), LowerError> {
        match &transition.transport {
            super::WasmResidentEventTransport::Direct { event_fields } => self
                .synthesize_direct_resident_transition(
                    transition,
                    initializer,
                    projection,
                    *event_fields,
                ),
            super::WasmResidentEventTransport::Batch {
                event_fields,
                event_stride,
                accumulate_f32_fields,
                coalesce_tag_field,
                coalesce_tag_variant,
            } => {
                if initializer.is_some() || projection.is_some() {
                    return Err(LowerError::Unsupported(
                        "batched resident transitions do not yet admit an authored initializer/projection"
                            .to_owned(),
                    ));
                }
                self.synthesize_batched_resident_transition(
                    transition,
                    *event_fields,
                    *event_stride,
                    accumulate_f32_fields,
                    *coalesce_tag_field,
                    *coalesce_tag_variant,
                )
            }
        }
    }

    /// Synthesize the target-neutral one-event resident actor ABI. The host
    /// passes one flattened event plus inert resources. Complete actor state
    /// stays in private Wasm globals and the authored Fe transition returns the
    /// next complete state.
    fn synthesize_direct_resident_transition(
        &mut self,
        transition: &super::WasmResidentTransition,
        initializer: Option<&super::WasmResidentInitializer>,
        projection: Option<&super::WasmResidentProjection>,
        event_fields: usize,
    ) -> Result<(), LowerError> {
        if event_fields == 0 {
            return Err(LowerError::Unsupported(
                "resident actor transition must declare at least one event field".to_owned(),
            ));
        }

        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| self.function_symbol(**instance) == transition.source)
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must select exactly one lowered Fe behavior (found {})",
                transition.source,
                candidates.len()
            )));
        };
        let (callee_args, result_tys) = self.builder.sig(*callee, |signature| {
            (signature.args().to_vec(), signature.ret_tys().to_vec())
        });
        if callee_args.len() < event_fields {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} arguments, fewer than its {} event leaves",
                transition.source,
                callee_args.len(),
                event_fields
            )));
        }
        if result_tys.is_empty() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return complete actor state",
                transition.source
            )));
        }

        let event_tys = &callee_args[..event_fields];
        let actor_tys = &callee_args[event_fields..];
        if actor_tys.len() != transition.actor_param_is_resource.len() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} flattened actor arguments but its resource mask has {} entries",
                transition.source,
                actor_tys.len(),
                transition.actor_param_is_resource.len()
            )));
        }
        let state_tys = actor_tys
            .iter()
            .zip(&transition.actor_param_is_resource)
            .filter_map(|(ty, is_resource)| (!is_resource).then_some(*ty))
            .collect::<Vec<_>>();
        if state_tys != result_tys {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return the complete flattened non-resource actor state: arguments {state_tys:?}, results {result_tys:?}",
                transition.source
            )));
        }
        for (index, limit) in &transition.event_tag_limits {
            if *limit == 0
                || event_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid event enum constraint ({index}, {limit}) for {event_tys:?}",
                    transition.source
                )));
            }
        }
        for (index, limit) in &transition.state_tag_limits {
            if *limit == 0
                || result_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid state enum constraint ({index}, {limit}) for {result_tys:?}",
                    transition.source
                )));
            }
        }

        let state_globals = result_tys
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                self.builder.declare_gv(GlobalVariableData::new(
                    format!("__fe_actor_state_v1_{index}"),
                    *ty,
                    Linkage::Private,
                    false,
                    None,
                ))
            })
            .collect::<Vec<_>>();
        let state_initialized = self.builder.declare_gv(GlobalVariableData::new(
            "__fe_actor_state_v1_initialized".to_owned(),
            Type::I32,
            Linkage::Private,
            false,
            Some(GvInitializer::make_imm(0i32)),
        ));

        if let Some(initializer) = initializer {
            let candidates = self
                .func_map
                .iter()
                .filter(|(instance, _)| self.function_symbol(**instance) == initializer.source)
                .map(|(_, func_ref)| *func_ref)
                .collect::<Vec<_>>();
            let [initializer_callee] = candidates.as_slice() else {
                return Err(LowerError::Unsupported(format!(
                    "resident initializer `{}` must select exactly one lowered Fe behavior (found {})",
                    initializer.source,
                    candidates.len()
                )));
            };
            let (initializer_args, initializer_results) =
                self.builder.sig(*initializer_callee, |signature| {
                    (signature.args().to_vec(), signature.ret_tys().to_vec())
                });
            if !initializer_args.is_empty() || initializer_results != result_tys {
                return Err(LowerError::Unsupported(format!(
                    "resident initializer `{}` must take no arguments and return complete actor state {result_tys:?}; got {initializer_args:?} -> {initializer_results:?}",
                    initializer.source
                )));
            }
            let initialize = self
                .builder
                .declare_function(Signature::new(
                    &initializer.export,
                    Linkage::Public,
                    &[],
                    &result_tys,
                ))
                .map_err(|error| {
                    LowerError::Internal(format!(
                        "failed to declare resident initializer `{}`: {error}",
                        initializer.export
                    ))
                })?;
            {
                let is = self.isa.inst_set();
                let mut fb = self.builder.func_builder::<InstInserter>(initialize);
                let entry = fb.append_block();
                fb.switch_to_block(entry);
                let results = fb.insert_call_results(
                    *initializer_callee,
                    smallvec1::SmallVec::<[ValueId; 8]>::new(),
                );
                for ((global, ty), value) in state_globals
                    .iter()
                    .copied()
                    .zip(result_tys.iter().copied())
                    .zip(results.iter().copied())
                {
                    let address = fb.make_global_value(global);
                    fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
                }
                let initialized_address = fb.make_global_value(state_initialized);
                let one = fb.make_imm_value(Immediate::I32(1));
                fb.insert_inst_no_result(Mstore::new(is, initialized_address, one, Type::I32));
                fb.insert_return_values(&results);
                fb.seal_all();
                fb.finish();
            }
        }

        if let Some(projection) = projection {
            let candidates = self
                .func_map
                .iter()
                .filter(|(instance, _)| self.function_symbol(**instance) == projection.source)
                .map(|(_, func_ref)| *func_ref)
                .collect::<Vec<_>>();
            let [projection_callee] = candidates.as_slice() else {
                return Err(LowerError::Unsupported(format!(
                    "resident projection `{}` must select exactly one lowered Fe behavior (found {})",
                    projection.source,
                    candidates.len()
                )));
            };
            let (projection_args, projection_results) =
                self.builder.sig(*projection_callee, |signature| {
                    (signature.args().to_vec(), signature.ret_tys().to_vec())
                });
            if projection_args != result_tys || projection_results.is_empty() {
                return Err(LowerError::Unsupported(format!(
                    "resident projection `{}` must take complete actor state {result_tys:?} and return a non-empty closed value; got {projection_args:?} -> {projection_results:?}",
                    projection.source
                )));
            }
            let project = self
                .builder
                .declare_function(Signature::new(
                    &projection.export,
                    Linkage::Public,
                    &[],
                    &projection_results,
                ))
                .map_err(|error| {
                    LowerError::Internal(format!(
                        "failed to declare resident projection `{}`: {error}",
                        projection.export
                    ))
                })?;
            {
                let is = self.isa.inst_set();
                let mut fb = self.builder.func_builder::<InstInserter>(project);
                let entry = fb.append_block();
                let invoke = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let initialized_address = fb.make_global_value(state_initialized);
                let initialized_value =
                    fb.insert_inst(Mload::new(is, initialized_address, Type::I32), Type::I32);
                let one = fb.make_imm_value(Immediate::I32(1));
                let initialized = fb.insert_inst(CmpEq::new(is, initialized_value, one), Type::I1);
                fb.insert_inst_no_result(Br::new(is, initialized, invoke, invalid));

                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(is));

                fb.switch_to_block(invoke);
                let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
                for (global, ty) in state_globals
                    .iter()
                    .copied()
                    .zip(result_tys.iter().copied())
                {
                    let address = fb.make_global_value(global);
                    args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                }
                let results = fb.insert_call_results(*projection_callee, args);
                fb.insert_return_values(&results);
                fb.seal_all();
                fb.finish();
            }
        }

        let state_replace = self
            .builder
            .declare_function(Signature::new(
                &transition.state_replace_export,
                Linkage::Public,
                &result_tys,
                &[],
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident state replacement `{}`: {error}",
                    transition.state_replace_export
                ))
            })?;
        {
            let is = self.isa.inst_set();
            let mut fb = self.builder.func_builder::<InstInserter>(state_replace);
            let entry = fb.append_block();
            let valid = fb.append_block();
            let invalid = fb.append_block();
            fb.switch_to_block(entry);
            let values = fb.args().to_vec();
            let mut all_valid = None;
            for (index, limit) in &transition.state_tag_limits {
                let mut tag_valid = None;
                for tag in 0..*limit {
                    let expected = fb.make_imm_value(
                        fieldless_tag_immediate(result_tys[*index], tag)
                            .expect("enum tag type validated above"),
                    );
                    let equal = fb.insert_inst(CmpEq::new(is, values[*index], expected), Type::I1);
                    tag_valid = Some(match tag_valid {
                        Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                        None => equal,
                    });
                }
                let tag_valid = tag_valid.expect("nonzero enum limit validated above");
                all_valid = Some(match all_valid {
                    Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                    None => tag_valid,
                });
            }
            if let Some(all_valid) = all_valid {
                fb.insert_inst_no_result(Br::new(is, all_valid, valid, invalid));
            } else {
                fb.insert_inst_no_result(Jump::new(is, valid));
            }

            fb.switch_to_block(invalid);
            fb.insert_inst_no_result(Unreachable::new(is));

            fb.switch_to_block(valid);
            for ((global, ty), value) in state_globals
                .iter()
                .copied()
                .zip(result_tys.iter().copied())
                .zip(values)
            {
                let address = fb.make_global_value(global);
                fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
            }
            let initialized_address = fb.make_global_value(state_initialized);
            let one = fb.make_imm_value(Immediate::I32(1));
            fb.insert_inst_no_result(Mstore::new(is, initialized_address, one, Type::I32));
            fb.insert_inst_no_result(Return::new_unit(is));
            fb.seal_all();
            fb.finish();
        }

        let mut wrapper_args = event_tys.to_vec();
        wrapper_args.extend(
            actor_tys
                .iter()
                .zip(&transition.actor_param_is_resource)
                .filter_map(|(ty, is_resource)| is_resource.then_some(*ty)),
        );
        let wrapper = self
            .builder
            .declare_function(Signature::new(
                &transition.export,
                Linkage::Public,
                &wrapper_args,
                &result_tys,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident transition wrapper `{}`: {error}",
                    transition.export
                ))
            })?;

        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        let invoke = fb.append_block();
        let invalid = fb.append_block();
        fb.switch_to_block(entry);
        let wrapper_values = fb.args().to_vec();
        let initialized_address = fb.make_global_value(state_initialized);
        let initialized_value =
            fb.insert_inst(Mload::new(is, initialized_address, Type::I32), Type::I32);
        let one = fb.make_imm_value(Immediate::I32(1));
        let mut ready = fb.insert_inst(CmpEq::new(is, initialized_value, one), Type::I1);
        for (index, limit) in &transition.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("enum tag type validated above"),
                );
                let equal =
                    fb.insert_inst(CmpEq::new(is, wrapper_values[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            ready = fb.insert_inst(
                And::new(
                    is,
                    ready,
                    tag_valid.expect("nonzero enum limit validated above"),
                ),
                Type::I1,
            );
        }
        fb.insert_inst_no_result(Br::new(is, ready, invoke, invalid));

        fb.switch_to_block(invalid);
        fb.insert_inst_no_result(Unreachable::new(is));

        fb.switch_to_block(invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        args.extend(wrapper_values[..event_fields].iter().copied());
        let mut resource_index = event_fields;
        let mut state_index = 0usize;
        for is_resource in &transition.actor_param_is_resource {
            if *is_resource {
                args.push(wrapper_values[resource_index]);
                resource_index += 1;
            } else {
                let global = state_globals[state_index];
                let ty = result_tys[state_index];
                let address = fb.make_global_value(global);
                args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                state_index += 1;
            }
        }
        let results = fb.insert_call_results(*callee, args);
        if results.len() != result_tys.len() {
            return Err(LowerError::Internal(
                "resident transition call result arity changed after signature inspection"
                    .to_owned(),
            ));
        }
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
            .zip(results.iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        fb.insert_return_values(&results);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    /// Lower an ordinary Fe decision function into a resident policy export.
    /// Its event prefix is host supplied, its state segment is loaded from and
    /// committed to private globals, and only the decision suffix is returned
    /// to the host. This is the generic mechanism used by presentation policy;
    /// it contains no browser or scheduling algorithm.
    fn synthesize_resident_policy(
        &mut self,
        policy: &super::WasmResidentPolicy,
        policy_index: usize,
    ) -> Result<(), LowerError> {
        if policy.event_fields == 0 || policy.decision_fields == 0 {
            return Err(LowerError::Unsupported(
                "derived policy requires non-empty fact and decision records".to_owned(),
            ));
        }
        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| {
                mir::runtime_instance_symbol_key(self.db, **instance) == policy.callee_instance_key
            })
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident policy `{}` must select exactly one lowered Fe behavior (found {})",
                policy.callee_instance_key,
                candidates.len()
            )));
        };
        let (callee_args, result_tys) = self.builder.sig(*callee, |signature| {
            (signature.args().to_vec(), signature.ret_tys().to_vec())
        });
        let expected_args = policy.event_fields + policy.state_fields;
        let expected_results = policy.state_fields + policy.decision_fields;
        if callee_args.len() != expected_args || result_tys.len() != expected_results {
            return Err(LowerError::Unsupported(format!(
                "resident policy `{}` must flatten to {expected_args} arguments and {expected_results} results; got {} -> {}",
                policy.callee_instance_key,
                callee_args.len(),
                result_tys.len()
            )));
        }
        let (event_tys, state_tys) = if policy.event_first {
            (
                &callee_args[..policy.event_fields],
                &callee_args[policy.event_fields..],
            )
        } else {
            (
                &callee_args[policy.state_fields..],
                &callee_args[..policy.state_fields],
            )
        };
        if result_tys[..policy.state_fields] != *state_tys {
            return Err(LowerError::Unsupported(format!(
                "resident policy `{}` must return its complete state as the leading result prefix: arguments {state_tys:?}, results {:?}",
                policy.callee_instance_key,
                &result_tys[..policy.state_fields]
            )));
        }
        for (index, limit) in &policy.event_tag_limits {
            if *limit == 0
                || event_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident policy `{}` has invalid event enum constraint ({index}, {limit}) for {event_tys:?}",
                    policy.callee_instance_key
                )));
            }
        }
        for (index, limit) in &policy.state_tag_limits {
            if *limit == 0
                || state_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident policy `{}` has invalid state enum constraint ({index}, {limit}) for {state_tys:?}",
                    policy.callee_instance_key
                )));
            }
        }

        // Wasm globals without an explicit initializer are zero-initialized.
        // The nominal Fe state contract deliberately defines that value as its
        // initial state, so no host-authored seed or policy JSON is required.
        let state_globals = state_tys
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                self.builder.declare_gv(GlobalVariableData::new(
                    format!("__fe_resident_policy_{policy_index}_state_v1_{index}"),
                    *ty,
                    Linkage::Private,
                    false,
                    None,
                ))
            })
            .collect::<Vec<_>>();
        let decision_tys = &result_tys[policy.state_fields..];
        let wrapper = self
            .builder
            .declare_function(Signature::new(
                &policy.export,
                Linkage::Public,
                event_tys,
                decision_tys,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident policy wrapper `{}`: {error}",
                    policy.export
                ))
            })?;

        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        let invoke = fb.append_block();
        let invalid = fb.append_block();
        fb.switch_to_block(entry);
        let event_values = fb.args().to_vec();
        let mut all_valid = None;
        for (index, limit) in &policy.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("resident policy enum tag type validated above"),
                );
                let equal =
                    fb.insert_inst(CmpEq::new(is, event_values[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            let tag_valid = tag_valid.expect("nonzero resident policy enum limit validated above");
            all_valid = Some(match all_valid {
                Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                None => tag_valid,
            });
        }
        if let Some(all_valid) = all_valid {
            fb.insert_inst_no_result(Br::new(is, all_valid, invoke, invalid));
        } else {
            fb.insert_inst_no_result(Jump::new(is, invoke));
        }

        fb.switch_to_block(invalid);
        fb.insert_inst_no_result(Unreachable::new(is));

        fb.switch_to_block(invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        if policy.event_first {
            args.extend(event_values.iter().copied());
        }
        for (global, ty) in state_globals.iter().copied().zip(state_tys.iter().copied()) {
            let address = fb.make_global_value(global);
            args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        if !policy.event_first {
            args.extend(event_values);
        }
        let results = fb.insert_call_results(*callee, args);
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(state_tys.iter().copied())
            .zip(results[..policy.state_fields].iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        fb.insert_return_values(&results[policy.state_fields..]);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    /// Lower Fe's batched resident transition policy. The host writes fixed-
    /// stride scalar event records into exported linear memory. Coalescing
    /// executes in generated Wasm: selected f32 leaves accumulate while every
    /// other fact comes from the newest record. The authored Fe transition is
    /// invoked exactly once and its full state reply is committed atomically.
    fn synthesize_batched_resident_transition(
        &mut self,
        transition: &super::WasmResidentTransition,
        event_fields: usize,
        event_stride: i32,
        accumulate_f32_fields: &[usize],
        coalesce_tag_field: usize,
        coalesce_tag_variant: u32,
    ) -> Result<(), LowerError> {
        if event_fields == 0 || event_stride <= 0 {
            return Err(LowerError::Unsupported(
                "batched resident transition requires a non-empty, positive-stride event"
                    .to_owned(),
            ));
        }
        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| self.function_symbol(**instance) == transition.source)
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must select exactly one lowered Fe behavior (found {})",
                transition.source,
                candidates.len()
            )));
        };
        let (callee_args, result_tys) = self.builder.sig(*callee, |signature| {
            (signature.args().to_vec(), signature.ret_tys().to_vec())
        });
        if callee_args.len() < event_fields {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} arguments, fewer than its {} event leaves",
                transition.source,
                callee_args.len(),
                event_fields
            )));
        }
        if result_tys.is_empty() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return complete actor state",
                transition.source
            )));
        }

        let event_tys = callee_args[..event_fields].to_vec();
        for (index, limit) in &transition.event_tag_limits {
            if *limit == 0
                || event_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid batched event enum constraint ({index}, {limit}) for {event_tys:?}",
                    transition.source
                )));
            }
        }
        for index in accumulate_f32_fields {
            if event_tys.get(*index) != Some(&Type::F32) {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` accumulation leaf {index} is not f32",
                    transition.source
                )));
            }
        }
        let coalesce_limit = transition
            .event_tag_limits
            .iter()
            .find_map(|(index, limit)| (*index == coalesce_tag_field).then_some(*limit))
            .ok_or_else(|| {
                LowerError::Unsupported(format!(
                    "resident transition `{}` coalescing field {coalesce_tag_field} is not a validated fieldless enum leaf",
                    transition.source
                ))
            })?;
        if coalesce_tag_variant >= coalesce_limit {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` coalescing variant {coalesce_tag_variant} exceeds enum bound {coalesce_limit}",
                transition.source
            )));
        }
        let coalesce_tag = event_tys
            .get(coalesce_tag_field)
            .and_then(|ty| fieldless_tag_immediate(*ty, coalesce_tag_variant))
            .ok_or_else(|| {
                LowerError::Unsupported(format!(
                    "resident transition `{}` cannot represent coalescing variant {coalesce_tag_variant} in event leaf {coalesce_tag_field}",
                    transition.source
                ))
            })?;
        let actor_tys = &callee_args[event_fields..];
        if actor_tys.len() != transition.actor_param_is_resource.len() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} flattened actor arguments but its resource mask has {} entries",
                transition.source,
                actor_tys.len(),
                transition.actor_param_is_resource.len()
            )));
        }
        let state_tys = actor_tys
            .iter()
            .zip(&transition.actor_param_is_resource)
            .filter_map(|(ty, is_resource)| (!is_resource).then_some(*ty))
            .collect::<Vec<_>>();
        if state_tys != result_tys {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return the complete flattened non-resource actor state: arguments {state_tys:?}, results {result_tys:?}",
                transition.source
            )));
        }
        for (index, limit) in &transition.state_tag_limits {
            if *limit == 0
                || result_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid batched state enum constraint ({index}, {limit}) for {result_tys:?}",
                    transition.source
                )));
            }
        }

        let state_globals = result_tys
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                self.builder.declare_gv(GlobalVariableData::new(
                    format!("__fe_actor_state_v1_{index}"),
                    *ty,
                    Linkage::Private,
                    false,
                    None,
                ))
            })
            .collect::<Vec<_>>();
        let state_initialized = self.builder.declare_gv(GlobalVariableData::new(
            "__fe_actor_state_v1_initialized".to_owned(),
            Type::I32,
            Linkage::Private,
            false,
            Some(GvInitializer::make_imm(0i32)),
        ));

        let state_replace = self
            .builder
            .declare_function(Signature::new(
                &transition.state_replace_export,
                Linkage::Public,
                &result_tys,
                &[],
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident state replacement `{}`: {error}",
                    transition.state_replace_export
                ))
            })?;
        {
            let is = self.isa.inst_set();
            let mut fb = self.builder.func_builder::<InstInserter>(state_replace);
            let entry = fb.append_block();
            let valid = fb.append_block();
            let invalid = fb.append_block();
            fb.switch_to_block(entry);
            let values = fb.args().to_vec();
            let mut all_valid = None;
            for (index, limit) in &transition.state_tag_limits {
                let mut tag_valid = None;
                for tag in 0..*limit {
                    let expected = fb.make_imm_value(
                        fieldless_tag_immediate(result_tys[*index], tag)
                            .expect("enum tag type validated above"),
                    );
                    let equal = fb.insert_inst(CmpEq::new(is, values[*index], expected), Type::I1);
                    tag_valid = Some(match tag_valid {
                        Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                        None => equal,
                    });
                }
                let tag_valid = tag_valid.expect("nonzero enum limit validated above");
                all_valid = Some(match all_valid {
                    Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                    None => tag_valid,
                });
            }
            if let Some(all_valid) = all_valid {
                fb.insert_inst_no_result(Br::new(is, all_valid, valid, invalid));
            } else {
                fb.insert_inst_no_result(Jump::new(is, valid));
            }

            fb.switch_to_block(invalid);
            fb.insert_inst_no_result(Unreachable::new(is));

            fb.switch_to_block(valid);
            for ((global, ty), value) in state_globals
                .iter()
                .copied()
                .zip(result_tys.iter().copied())
                .zip(values)
            {
                let address = fb.make_global_value(global);
                fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
            }
            let initialized_address = fb.make_global_value(state_initialized);
            let one = fb.make_imm_value(Immediate::I32(1));
            fb.insert_inst_no_result(Mstore::new(is, initialized_address, one, Type::I32));
            fb.insert_inst_no_result(Return::new_unit(is));
            fb.seal_all();
            fb.finish();
        }

        let mut wrapper_args = vec![Type::I32, Type::I32];
        wrapper_args.extend(
            actor_tys
                .iter()
                .zip(&transition.actor_param_is_resource)
                .filter_map(|(ty, is_resource)| is_resource.then_some(*ty)),
        );
        let wrapper = self
            .builder
            .declare_function(Signature::new(
                &transition.export,
                Linkage::Public,
                &wrapper_args,
                &result_tys,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident transition wrapper `{}`: {error}",
                    transition.export
                ))
            })?;

        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        let classify_header = fb.append_block();
        let classify_body = fb.append_block();
        let classify_done = fb.append_block();
        let initialize = fb.append_block();
        let invalid = fb.append_block();
        let header = fb.append_block();
        let body = fb.append_block();
        let done = fb.append_block();
        let invoke = fb.append_block();
        let sequential_header = fb.append_block();
        let sequential_body = fb.append_block();
        let sequential_invoke = fb.append_block();
        let sequential_done = fb.append_block();
        fb.switch_to_block(entry);
        let wrapper_values = fb.args().to_vec();
        let events_ptr = wrapper_values[0];
        let event_count = wrapper_values[1];
        let zero = fb.make_imm_value(Immediate::I32(0));
        let has_events = fb.insert_inst(Lt::new(is, zero, event_count), Type::I1);
        let initialized_address = fb.make_global_value(state_initialized);
        let initialized_value =
            fb.insert_inst(Mload::new(is, initialized_address, Type::I32), Type::I32);
        let initialized_one = fb.make_imm_value(Immediate::I32(1));
        let initialized =
            fb.insert_inst(CmpEq::new(is, initialized_value, initialized_one), Type::I1);
        let ready = fb.insert_inst(And::new(is, has_events, initialized), Type::I1);
        fb.insert_inst_no_result(Br::new(is, ready, classify_header, invalid));

        // Preserve the one-transition hot path for a homogeneous gesture
        // burst. Any heterogeneous batch is folded in source order below, so
        // a direct parameter edit can never swallow an older gesture merely
        // because its event tag is the newest record.
        fb.switch_to_block(classify_header);
        let classify_index = fb.insert_inst(Phi::new(is, vec![(zero, entry)]), Type::I32);
        let true_value = fb.make_imm_value(Immediate::I1(true));
        let all_coalescible = fb.insert_inst(Phi::new(is, vec![(true_value, entry)]), Type::I1);
        let classify_more = fb.insert_inst(Lt::new(is, classify_index, event_count), Type::I1);
        fb.insert_inst_no_result(Br::new(is, classify_more, classify_body, classify_done));

        fb.switch_to_block(classify_body);
        let stride = fb.make_imm_value(Immediate::I32(event_stride));
        let byte_offset = fb.insert_inst(Mul::new(is, classify_index, stride), Type::I32);
        let event_ptr = fb.insert_inst(Add::new(is, events_ptr, byte_offset), Type::I32);
        let tag_offset = fb.make_imm_value(Immediate::I32((coalesce_tag_field as i32) * 4));
        let tag_ptr = fb.insert_inst(Add::new(is, event_ptr, tag_offset), Type::I32);
        let tag_ty = event_tys[coalesce_tag_field];
        let tag = fb.insert_inst(Mload::new(is, tag_ptr, tag_ty), tag_ty);
        let expected_tag = fb.make_imm_value(coalesce_tag.clone());
        let is_coalescible = fb.insert_inst(CmpEq::new(is, tag, expected_tag), Type::I1);
        let next_all = fb.insert_inst(And::new(is, all_coalescible, is_coalescible), Type::I1);
        let one = fb.make_imm_value(Immediate::I32(1));
        let next_classify_index = fb.insert_inst(Add::new(is, classify_index, one), Type::I32);
        let classify_body_block = fb
            .current_block()
            .expect("resident batch classification has a current block");
        fb.append_phi_arg(classify_index, next_classify_index, classify_body_block);
        fb.append_phi_arg(all_coalescible, next_all, classify_body_block);
        fb.insert_inst_no_result(Jump::new(is, classify_header));

        fb.switch_to_block(classify_done);
        fb.insert_inst_no_result(Br::new(is, all_coalescible, initialize, sequential_header));

        fb.switch_to_block(invalid);
        fb.insert_inst_no_result(Unreachable::new(is));

        fb.switch_to_block(initialize);
        let mut initial = Vec::with_capacity(event_fields);
        for (index, ty) in event_tys.iter().copied().enumerate() {
            let address = if index == 0 {
                events_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32((index as i32) * 4));
                fb.insert_inst(Add::new(is, events_ptr, offset), Type::I32)
            };
            initial.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        let initialize_block = fb
            .current_block()
            .expect("resident batch initialization has a current block");
        fb.insert_inst_no_result(Jump::new(is, header));

        fb.switch_to_block(header);
        let one = fb.make_imm_value(Immediate::I32(1));
        let event_index = fb.insert_inst(Phi::new(is, vec![(one, initialize_block)]), Type::I32);
        let mut coalesced = Vec::with_capacity(event_fields);
        for (value, ty) in initial.into_iter().zip(event_tys.iter().copied()) {
            coalesced.push(fb.insert_inst(Phi::new(is, vec![(value, initialize_block)]), ty));
        }
        let more = fb.insert_inst(Lt::new(is, event_index, event_count), Type::I1);
        fb.insert_inst_no_result(Br::new(is, more, body, done));

        fb.switch_to_block(body);
        let stride = fb.make_imm_value(Immediate::I32(event_stride));
        let byte_offset = fb.insert_inst(Mul::new(is, event_index, stride), Type::I32);
        let event_ptr = fb.insert_inst(Add::new(is, events_ptr, byte_offset), Type::I32);
        let mut incoming = Vec::with_capacity(event_fields);
        for (index, ty) in event_tys.iter().copied().enumerate() {
            let address = if index == 0 {
                event_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32((index as i32) * 4));
                fb.insert_inst(Add::new(is, event_ptr, offset), Type::I32)
            };
            incoming.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        let mut next = incoming;
        for index in accumulate_f32_fields {
            next[*index] =
                fb.insert_inst(Fadd::new(is, coalesced[*index], next[*index]), Type::F32);
        }
        let next_index = fb.insert_inst(Add::new(is, event_index, one), Type::I32);
        let body_block = fb
            .current_block()
            .expect("resident batch body has a current block");
        fb.append_phi_arg(event_index, next_index, body_block);
        for (phi, value) in coalesced.iter().copied().zip(next) {
            fb.append_phi_arg(phi, value, body_block);
        }
        fb.insert_inst_no_result(Jump::new(is, header));

        fb.switch_to_block(done);
        let mut tags_valid = None;
        for (index, limit) in &transition.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("batched enum tag type validated above"),
                );
                let equal = fb.insert_inst(CmpEq::new(is, coalesced[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            let tag_valid = tag_valid.expect("nonzero batched enum limit validated above");
            tags_valid = Some(match tags_valid {
                Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                None => tag_valid,
            });
        }
        if let Some(tags_valid) = tags_valid {
            fb.insert_inst_no_result(Br::new(is, tags_valid, invoke, invalid));
        } else {
            fb.insert_inst_no_result(Jump::new(is, invoke));
        }

        fb.switch_to_block(invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        args.extend(coalesced);
        let mut resource_index = 2usize;
        let mut state_index = 0usize;
        for is_resource in &transition.actor_param_is_resource {
            if *is_resource {
                args.push(wrapper_values[resource_index]);
                resource_index += 1;
            } else {
                let global = state_globals[state_index];
                let ty = result_tys[state_index];
                let address = fb.make_global_value(global);
                args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                state_index += 1;
            }
        }
        let results = fb.insert_call_results(*callee, args);
        if results.len() != result_tys.len() {
            return Err(LowerError::Internal(
                "resident transition call result arity changed after signature inspection"
                    .to_owned(),
            ));
        }
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
            .zip(results.iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        fb.insert_return_values(&results);

        // Slow path for a heterogeneous batch. The browser still crosses the
        // Wasm boundary once, but the generated wrapper folds each untouched
        // event through the same authored Fe transition in source order. This
        // is deliberately generic over event meaning; only the semantically
        // derived homogeneous coalescing tag chooses the fast path above.
        fb.switch_to_block(sequential_header);
        let sequential_index = fb.insert_inst(Phi::new(is, vec![(zero, classify_done)]), Type::I32);
        let sequential_more = fb.insert_inst(Lt::new(is, sequential_index, event_count), Type::I1);
        fb.insert_inst_no_result(Br::new(
            is,
            sequential_more,
            sequential_body,
            sequential_done,
        ));

        fb.switch_to_block(sequential_body);
        let stride = fb.make_imm_value(Immediate::I32(event_stride));
        let byte_offset = fb.insert_inst(Mul::new(is, sequential_index, stride), Type::I32);
        let event_ptr = fb.insert_inst(Add::new(is, events_ptr, byte_offset), Type::I32);
        let mut sequential_event = Vec::with_capacity(event_fields);
        for (index, ty) in event_tys.iter().copied().enumerate() {
            let address = if index == 0 {
                event_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32((index as i32) * 4));
                fb.insert_inst(Add::new(is, event_ptr, offset), Type::I32)
            };
            sequential_event.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        let mut sequential_tags_valid = None;
        for (index, limit) in &transition.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("batched enum tag type validated above"),
                );
                let equal =
                    fb.insert_inst(CmpEq::new(is, sequential_event[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            let tag_valid = tag_valid.expect("nonzero batched enum limit validated above");
            sequential_tags_valid = Some(match sequential_tags_valid {
                Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                None => tag_valid,
            });
        }
        if let Some(tags_valid) = sequential_tags_valid {
            fb.insert_inst_no_result(Br::new(is, tags_valid, sequential_invoke, invalid));
        } else {
            fb.insert_inst_no_result(Jump::new(is, sequential_invoke));
        }

        fb.switch_to_block(sequential_invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        args.extend(sequential_event);
        let mut resource_index = 2usize;
        let mut state_index = 0usize;
        for is_resource in &transition.actor_param_is_resource {
            if *is_resource {
                args.push(wrapper_values[resource_index]);
                resource_index += 1;
            } else {
                let global = state_globals[state_index];
                let ty = result_tys[state_index];
                let address = fb.make_global_value(global);
                args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                state_index += 1;
            }
        }
        let results = fb.insert_call_results(*callee, args);
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
            .zip(results.iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        let one = fb.make_imm_value(Immediate::I32(1));
        let next_sequential_index = fb.insert_inst(Add::new(is, sequential_index, one), Type::I32);
        let sequential_invoke_block = fb
            .current_block()
            .expect("resident sequential batch invocation has a current block");
        fb.append_phi_arg(
            sequential_index,
            next_sequential_index,
            sequential_invoke_block,
        );
        fb.insert_inst_no_result(Jump::new(is, sequential_header));

        fb.switch_to_block(sequential_done);
        let mut final_state = Vec::with_capacity(result_tys.len());
        for (global, ty) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
        {
            let address = fb.make_global_value(global);
            final_state.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        fb.insert_return_values(&final_state);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    fn synthesize_canonical_lane(&mut self, lane: &crate::CanonicalLane) -> Result<(), LowerError> {
        let export = lane.export.as_deref().ok_or_else(|| {
            LowerError::Internal(format!(
                "host-effect lane `{}` reached canonical Wasm lowering",
                lane.name
            ))
        })?;
        fn flatten(
            layout: &crate::CanonicalLayout,
            base: u32,
            leaves: &mut Vec<(u32, Type)>,
            descriptors: &mut Vec<(u32, u32, u32, u32, u32)>,
        ) -> Result<(), LowerError> {
            use crate::CanonicalShape;
            let ty = match &layout.shape {
                CanonicalShape::Bool => Some(Type::I1),
                CanonicalShape::U8 => Some(Type::I8),
                CanonicalShape::I32 | CanonicalShape::U32 => Some(Type::I32),
                CanonicalShape::I64 | CanonicalShape::U64 => Some(Type::I64),
                CanonicalShape::F32 => Some(Type::F32),
                CanonicalShape::Record { fields } => {
                    for field in fields {
                        let offset = base.checked_add(field.offset).ok_or_else(|| {
                            LowerError::Unsupported(
                                "canonical record field offset overflow".to_owned(),
                            )
                        })?;
                        flatten(&field.layout, offset, leaves, descriptors)?;
                    }
                    None
                }
                CanonicalShape::Variant { .. } => {
                    return Err(LowerError::Unsupported(
                        "canonical variants require wasm32 enum runtime-class lowering".to_owned(),
                    ));
                }
                CanonicalShape::Bytes {
                    pointer_offset,
                    length_offset,
                }
                | CanonicalShape::String {
                    pointer_offset,
                    length_offset,
                    ..
                } => {
                    let pointer_offset = base.checked_add(*pointer_offset).ok_or_else(|| {
                        LowerError::Unsupported(
                            "canonical descriptor pointer offset overflow".to_owned(),
                        )
                    })?;
                    let length_offset = base.checked_add(*length_offset).ok_or_else(|| {
                        LowerError::Unsupported(
                            "canonical descriptor length offset overflow".to_owned(),
                        )
                    })?;
                    leaves.push((pointer_offset, Type::I32));
                    leaves.push((length_offset, Type::I32));
                    descriptors.push((pointer_offset, length_offset, 1, u32::MAX, 1));
                    None
                }
                CanonicalShape::List {
                    max,
                    stride,
                    pointer_offset,
                    length_offset,
                    ..
                } => {
                    let pointer_offset = base.checked_add(*pointer_offset).ok_or_else(|| {
                        LowerError::Unsupported("canonical list pointer offset overflow".to_owned())
                    })?;
                    let length_offset = base.checked_add(*length_offset).ok_or_else(|| {
                        LowerError::Unsupported("canonical list length offset overflow".to_owned())
                    })?;
                    if *stride != 4 || *max > u32::MAX / *stride {
                        return Err(LowerError::Unsupported(
                            "canonical list has unsafe stride or maximum".to_owned(),
                        ));
                    }
                    leaves.push((pointer_offset, Type::I32));
                    leaves.push((length_offset, Type::I32));
                    descriptors.push((pointer_offset, length_offset, *stride, *max, *stride));
                    None
                }
            };
            if let Some(ty) = ty {
                leaves.push((base, ty));
            }
            Ok(())
        }

        let has_variant = canonical_layout_contains_variant(&lane.request)
            || canonical_layout_contains_variant(&lane.response);
        let (request_plan, response_plan) = if has_variant {
            (
                Some(canonical_scalar_value_plan(
                    &lane.request,
                    0,
                    "canonical_request",
                )?),
                Some(canonical_scalar_value_plan(
                    &lane.response,
                    0,
                    "canonical_response",
                )?),
            )
        } else {
            (None, None)
        };
        let mut request = Vec::new();
        let mut response = Vec::new();
        let mut request_descriptors = Vec::new();
        let mut response_descriptors = Vec::new();
        if !has_variant {
            flatten(&lane.request, 0, &mut request, &mut request_descriptors)?;
            flatten(&lane.response, 0, &mut response, &mut response_descriptors)?;
        }
        // Input descriptors remain borrowed views into caller-owned memory.
        let _ = request_descriptors;
        let mut request_tys = Vec::new();
        let mut response_tys = Vec::new();
        if let Some(plan) = &request_plan {
            plan.append_flat_types(&mut request_tys);
        } else {
            request_tys.extend(request.iter().map(|(_, ty)| *ty));
        }
        if let Some(plan) = &response_plan {
            plan.append_flat_types(&mut response_tys);
        } else {
            response_tys.extend(response.iter().map(|(_, ty)| *ty));
        }
        if request_tys.is_empty() || response_tys.is_empty() {
            return Err(LowerError::Unsupported(
                "canonical wrapper records must contain scalar leaves".to_owned(),
            ));
        }

        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| self.function_symbol(**instance) == lane.name)
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "canonical lane `{}` must select exactly one lowered Fe entry (found {})",
                lane.name,
                candidates.len()
            )));
        };
        let signature_matches = self.builder.sig(*callee, |signature| {
            signature.args() == request_tys && signature.ret_tys() == response_tys
        });
        if !signature_matches {
            return Err(LowerError::Unsupported(format!(
                "canonical lane `{}` flattened signature does not match selected Fe entry",
                lane.name
            )));
        }

        let wrapper = self
            .builder
            .declare_function(Signature::new_single(
                export,
                Linkage::Public,
                &[Type::I32],
                Type::I32,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare canonical wrapper `{}`: {error}",
                    export
                ))
            })?;
        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        fb.switch_to_block(entry);
        let request_ptr = fb.args()[0];
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        if let Some(plan) = &request_plan {
            args.extend(load_canonical_scalar_value(&mut fb, is, request_ptr, plan)?);
        } else {
            for (offset, ty) in &request {
                let addr = canonical_offset_address(&mut fb, is, request_ptr, *offset);
                args.push(fb.insert_inst(Mload::new(is, addr, *ty), *ty));
            }
        }
        let results = fb.insert_call_results(*callee, args);
        if results.len() != response_tys.len() {
            return Err(LowerError::Internal(
                "canonical wrapper call result arity changed after signature check".to_owned(),
            ));
        }
        let allocation_size = lane
            .response
            .size
            .checked_add(lane.response.align - 1)
            .and_then(|size| i32::try_from(size).ok())
            .ok_or_else(|| {
                LowerError::Unsupported(
                    "canonical aligned response allocation exceeds Wasm i32".to_owned(),
                )
            })?;
        let allocation_size = fb.make_imm_value(Immediate::I32(allocation_size));
        let raw_response_ptr = fb.insert_inst(MemAllocDynamic::new(is, allocation_size), Type::I32);
        let response_ptr = if lane.response.align == 1 {
            raw_response_ptr
        } else {
            // MemAllocDynamic's Wasm bridge requests byte alignment. Allocate
            // enough slack and align the exposed canonical response pointer.
            // Canonical layout construction already guarantees power-of-two
            // alignment; the arena itself is bounded well below i32::MAX.
            let align_minus_one =
                fb.make_imm_value(Immediate::I32((lane.response.align - 1) as i32));
            let biased = fb.insert_inst(Add::new(is, raw_response_ptr, align_minus_one), Type::I32);
            let mask = fb.make_imm_value(Immediate::I32(-(lane.response.align as i32)));
            fb.insert_inst(And::new(is, biased, mask), Type::I32)
        };
        if response_plan.is_some() {
            // The canonical tagged-union contract makes inactive storage
            // deterministic. Arena reset reuses bytes, so zero the complete
            // response record before storing the validated active payload.
            zero_canonical_memory(&mut fb, is, response_ptr, lane.response.size)?;
        }
        let mut copied_pointers = HashMap::new();
        for (pointer_offset, length_offset, stride, max, alignment) in response_descriptors {
            let pointer_index = response
                .iter()
                .position(|(offset, _)| *offset == pointer_offset)
                .ok_or_else(|| {
                    LowerError::Internal(
                        "canonical descriptor pointer leaf was not flattened".to_owned(),
                    )
                })?;
            let length_index = response
                .iter()
                .position(|(offset, _)| *offset == length_offset)
                .ok_or_else(|| {
                    LowerError::Internal(
                        "canonical descriptor length leaf was not flattened".to_owned(),
                    )
                })?;
            let source = results[pointer_index];
            let length = results[length_index];
            if stride > 1 {
                let maximum = fb.make_imm_value(Immediate::I32(max as i32));
                let too_long = fb.insert_inst(Lt::new(is, maximum, length), Type::I1);
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.insert_inst_no_result(Br::new(is, too_long, invalid, valid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(is));
                fb.switch_to_block(valid);
                // The pointer is semantically ignored for an empty list, just
                // as it is by the JS decoder. Non-empty typed payloads must be
                // naturally aligned before the byte-copy loop can read them.
                let zero = fb.make_imm_value(Immediate::I32(0));
                let empty = fb.insert_inst(CmpEq::new(is, length, zero), Type::I1);
                let aligned_block = fb.append_block();
                let check_alignment = fb.append_block();
                fb.insert_inst_no_result(Br::new(is, empty, aligned_block, check_alignment));
                fb.switch_to_block(check_alignment);
                let mask = fb.make_imm_value(Immediate::I32((alignment - 1) as i32));
                let low_bits = fb.insert_inst(And::new(is, source, mask), Type::I32);
                let aligned = fb.insert_inst(CmpEq::new(is, low_bits, zero), Type::I1);
                let misaligned = fb.append_block();
                fb.insert_inst_no_result(Br::new(is, aligned, aligned_block, misaligned));
                fb.switch_to_block(misaligned);
                fb.insert_inst_no_result(Unreachable::new(is));
                fb.switch_to_block(aligned_block);
            }
            let byte_length = if stride == 1 {
                length
            } else {
                let stride = fb.make_imm_value(Immediate::I32(stride as i32));
                fb.insert_inst(Mul::new(is, length, stride), Type::I32)
            };
            let allocation_length = if alignment == 1 {
                byte_length
            } else {
                let slack = fb.make_imm_value(Immediate::I32((alignment - 1) as i32));
                fb.insert_inst(Add::new(is, byte_length, slack), Type::I32)
            };
            let raw_destination =
                fb.insert_inst(MemAllocDynamic::new(is, allocation_length), Type::I32);
            let destination = if alignment == 1 {
                raw_destination
            } else {
                let slack = fb.make_imm_value(Immediate::I32((alignment - 1) as i32));
                let biased = fb.insert_inst(Add::new(is, raw_destination, slack), Type::I32);
                let mask = fb.make_imm_value(Immediate::I32(-(alignment as i32)));
                fb.insert_inst(And::new(is, biased, mask), Type::I32)
            };

            // Returned descriptors are borrowed inside Fe. The canonical
            // wrapper establishes explicit host ownership by copying exactly
            // `len * stride` bytes into its arena before publishing the
            // descriptor. The length leaf remains an element count.
            let copy_entry = fb
                .current_block()
                .expect("canonical wrapper copy requires a current block");
            let copy_header = fb.append_block();
            let copy_body = fb.append_block();
            let copy_done = fb.append_block();
            fb.insert_inst_no_result(Jump::new(is, copy_header));
            fb.switch_to_block(copy_header);
            let zero = fb.make_imm_value(Immediate::I32(0));
            let index = fb.insert_inst(Phi::new(is, vec![(zero, copy_entry)]), Type::I32);
            let more = fb.insert_inst(Lt::new(is, index, byte_length), Type::I1);
            fb.insert_inst_no_result(Br::new(is, more, copy_body, copy_done));

            fb.switch_to_block(copy_body);
            let source_byte = fb.insert_inst(Add::new(is, source, index), Type::I32);
            let destination_byte = fb.insert_inst(Add::new(is, destination, index), Type::I32);
            let byte = fb.insert_inst(Mload::new(is, source_byte, Type::I8), Type::I8);
            fb.insert_inst_no_result(Mstore::new(is, destination_byte, byte, Type::I8));
            let one = fb.make_imm_value(Immediate::I32(1));
            let next = fb.insert_inst(Add::new(is, index, one), Type::I32);
            let copy_back = fb
                .current_block()
                .expect("canonical wrapper copy body requires a current block");
            fb.append_phi_arg(index, next, copy_back);
            fb.insert_inst_no_result(Jump::new(is, copy_header));
            fb.switch_to_block(copy_done);
            copied_pointers.insert(pointer_offset, destination);
        }
        if let Some(plan) = &response_plan {
            let mut cursor = 0;
            store_canonical_scalar_value(&mut fb, is, response_ptr, plan, &results, &mut cursor)?;
            if cursor != results.len() {
                return Err(LowerError::Internal(
                    "canonical scalar response left unconsumed value lanes".to_owned(),
                ));
            }
        } else {
            for ((offset, ty), value) in response.into_iter().zip(results) {
                let value = copied_pointers.get(&offset).copied().unwrap_or(value);
                let addr = canonical_offset_address(&mut fb, is, response_ptr, offset);
                fb.insert_inst_no_result(Mstore::new(is, addr, value, ty));
            }
        }
        fb.insert_return(response_ptr);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    /// The single Sonatina value type for a runtime class. Recursive product
    /// values are handled by `flat_shape`; materialized objects are i32 arena
    /// pointers. This method therefore covers scalar leaves, transport words,
    /// single-scalar newtypes, and fieldless-enum tags, and fails closed for
    /// classes without one scalar/pointer representation.
    fn ty_for_class(&self, class: &RuntimeClass<'db>) -> Result<Type, LowerError> {
        match class {
            // Value-carried enum tags use the same canonical 32-bit carrier as
            // their fieldless enum value. Narrow layout tags are only a memory
            // concern; letting an EnumTag scalar become i8/i16 leaks compact
            // representation into function signatures and authored GPU state.
            RuntimeClass::Scalar(ScalarClass {
                role: ScalarRole::EnumTag { .. },
                ..
            }) => Ok(Type::I32),
            RuntimeClass::Scalar(scalar) => scalar_ty_r1(scalar),
            // R3.4b step 1: the `MemPtr<B::Word>` transport class is the backend
            // pointer word: i32 on wasm32 (exactly the linear-memory offset the JS
            // broker already sees). A `MemPtr<u32>` classifies as a memory-space
            // `RawAddr` (a raw memory address, the class the runtime classifier
            // actually assigns every host-minted region pointer and every region
            // pointer crossing the WebGPU import boundary); a memory-space provider
            // reference is admitted on the same footing. This is the wasm mirror of
            // the EVM lowerer's I256 transport repr (`lower_runtime.rs`
            // `ty_for_class`). Non-memory addresses/provider refs and object/const
            // refs are R2 memory lowering and stay fail-closed (the catch-all below).
            RuntimeClass::RawAddr {
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
            } => Ok(Type::I32),
            // R3.4b step 2: a single-scalar-field aggregate (a `u32` newtype such
            // as `Pending<T>` / `KernelId` / `WebGpuRef`) is represented as its one
            // field's scalar. A payload-free enum is represented by its
            // compiler-derived tag. Payload enums use `flat_shape`'s tagged
            // lane tree and therefore never reach this single-value path.
            RuntimeClass::AggregateValue { layout } => {
                if let Layout::Enum(enum_layout) = layout.data(self.db)
                    && enum_layout
                        .variants
                        .iter()
                        .any(|variant| !variant.fields.is_empty())
                {
                    return Err(LowerError::Unsupported(
                        "wasm target: payload enum value transport is not implemented; only \
                         payload-free enums have a scalar tag representation"
                            .to_owned(),
                    ));
                }
                if self.fieldless_enum_tag(*layout).is_some() {
                    // WebAssembly exposes every narrow integer through an i32
                    // value. Keep enum SSA values in that canonical carrier;
                    // compact in-memory tags truncate/extend at projections.
                    return Ok(Type::I32);
                }
                let scalar = self.single_scalar_field(*layout).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target supports only recursively flattened scalar records, \
                     one-field scalar newtypes, and fieldless enums; `{class:?}` has no \
                     admitted value representation"
                    ))
                })?;
                scalar_ty_r1(&scalar)
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) supports only scalar values; `{other:?}` \
                 (aggregate/ref/raw-addr) is not lowered yet"
            ))),
        }
    }

    /// If `layout` is a struct with EXACTLY ONE field that is an R1-envelope
    /// scalar, return that field's scalar class; otherwise `None` (multi-field,
    /// empty, array, and enum layouts return `None`). This is what lets the
    /// `u32` newtypes (`Pending<T>` / `KernelId` / `WebGpuRef`) execute: their
    /// runtime representation IS their single field's word.
    fn single_scalar_field(&self, layout: LayoutId<'db>) -> Option<ScalarClass<'db>> {
        match layout.data(self.db) {
            Layout::Struct(struct_layout) => match &*struct_layout.fields {
                [RuntimeClass::Scalar(scalar)] => Some(scalar.clone()),
                _ => None,
            },
            Layout::Array(_) | Layout::Enum(_) => None,
        }
    }

    /// A payload-free enum is exactly its compiler-derived tag scalar. This is
    /// the compact value representation used by ordinary Fe policy enums such
    /// as `ParamKind`; payload enums use a distinct flattened lane tree.
    fn fieldless_enum_tag(&self, layout: LayoutId<'db>) -> Option<ScalarClass<'db>> {
        match layout.data(self.db) {
            Layout::Enum(enum_layout)
                if enum_layout
                    .variants
                    .iter()
                    .all(|variant| variant.fields.is_empty()) =>
            {
                Some(enum_layout.tag)
            }
            Layout::Struct(_) | Layout::Array(_) | Layout::Enum(_) => None,
        }
    }

    fn fieldless_enum_variant_const(
        &self,
        layout: LayoutId<'db>,
        variant: VariantId<'db>,
    ) -> Result<(ConstScalar, Type), LowerError> {
        let tag = self.fieldless_enum_tag(layout).ok_or_else(|| {
            LowerError::Unsupported(
                "wasm target: only payload-free enums have a scalar value representation"
                    .to_owned(),
            )
        })?;
        if variant.enum_layout != layout {
            return Err(LowerError::Internal(
                "enum variant belongs to a different runtime layout".to_owned(),
            ));
        }
        let Layout::Enum(enum_layout) = layout.data(self.db) else {
            unreachable!()
        };
        if usize::from(variant.index) >= enum_layout.variants.len() {
            return Err(LowerError::Internal(
                "enum variant index is outside its runtime layout".to_owned(),
            ));
        }
        let ScalarRepr::Int { .. } = tag.repr else {
            return Err(LowerError::Internal(
                "enum tag does not have an integer scalar representation".to_owned(),
            ));
        };
        let value = ConstScalar::Int {
            bits: 32,
            signed: false,
            words: if variant.index == 0 {
                Vec::new()
            } else {
                variant
                    .index
                    .to_be_bytes()
                    .into_iter()
                    .skip_while(|byte| *byte == 0)
                    .collect()
            },
        };
        Ok((value, Type::I32))
    }

    fn enum_variant_const(
        &self,
        layout: LayoutId<'db>,
        variant: VariantId<'db>,
    ) -> Result<(ConstScalar, Type), LowerError> {
        if variant.enum_layout != layout {
            return Err(LowerError::Internal(
                "enum variant belongs to a different runtime layout".to_owned(),
            ));
        }
        let Layout::Enum(enum_layout) = layout.data(self.db) else {
            return Err(LowerError::Internal(
                "enum variant requested from a non-enum layout".to_owned(),
            ));
        };
        if usize::from(variant.index) >= enum_layout.variants.len() {
            return Err(LowerError::Internal(
                "enum variant index is outside its runtime layout".to_owned(),
            ));
        }
        Ok((
            ConstScalar::Int {
                bits: 32,
                signed: false,
                words: if variant.index == 0 {
                    Vec::new()
                } else {
                    variant
                        .index
                        .to_be_bytes()
                        .into_iter()
                        .skip_while(|byte| *byte == 0)
                        .collect()
                },
            },
            Type::I32,
        ))
    }

    fn arena_address_param(class: &RuntimeClass<'db>) -> bool {
        match class {
            RuntimeClass::RawAddr {
                space: AddressSpaceKind::Memory,
                target: Some(_),
            } => true,
            RuntimeClass::Ref {
                pointee,
                kind:
                    RefKind::Provider {
                        space: AddressSpaceKind::Memory,
                        ..
                    },
                view: RefView::Whole,
            } => matches!(**pointee, RuntimeClass::AggregateValue { .. }),
            _ => false,
        }
    }

    fn arena_owned_place_source(
        body: &RuntimeBody<'db>,
        place: &RuntimePlace<'db>,
    ) -> Option<RLocalId> {
        match place.root {
            PlaceRoot::Slot(local) | PlaceRoot::Ref(local) => Some(local),
            PlaceRoot::Provider(binding) => body
                .provider_bindings
                .get(binding.as_u32() as usize)
                .map(|binding| binding.value),
            PlaceRoot::Ptr {
                addr,
                space: AddressSpaceKind::Memory,
                ..
            } => Some(addr),
            PlaceRoot::Ptr { .. } => None,
        }
    }

    /// Identify a typed memory-pointer field and the local carrying its root
    /// object. The key is structural across function bodies, so an initializer
    /// actor and a later stage actor agree on the same workspace field without
    /// relying on source names. Dynamic indexes, dereferences, opaque pointers,
    /// and non-memory transports stay outside this proof.
    fn arena_pointer_field(
        &self,
        body: &RuntimeBody<'db>,
        place: &RuntimePlace<'db>,
    ) -> Option<(ArenaPointerFieldKey<'db>, RLocalId)> {
        let program = self.db as &dyn mir::MirDb;
        let resolved = mir::resolve_runtime_place(self.db, &program, body, place).ok()?;
        if !matches!(
            resolved.result_class,
            RuntimeClass::RawAddr {
                space: AddressSpaceKind::Memory,
                target: Some(_),
            }
        ) {
            return None;
        }
        let root_layout = match &resolved.root_kind {
            mir::ResolvedPlaceRootKind::Slot { class, .. }
            | mir::ResolvedPlaceRootKind::Ref { class, .. }
            | mir::ResolvedPlaceRootKind::Provider { class, .. }
            | mir::ResolvedPlaceRootKind::Ptr { class, .. } => {
                let RuntimeClass::AggregateValue { layout } = class else {
                    return None;
                };
                *layout
            }
        };
        let fields = resolved
            .path
            .iter()
            .map(|element| match element {
                mir::ResolvedPlaceElem::Field { field, .. } => Some(field.0),
                mir::ResolvedPlaceElem::Index { .. }
                | mir::ResolvedPlaceElem::VariantField { .. }
                | mir::ResolvedPlaceElem::Deref { .. } => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if fields.is_empty() {
            return None;
        }
        let root = Self::arena_owned_place_source(body, place)?;
        Some((
            ArenaPointerFieldKey {
                root_layout,
                fields: fields.into_boxed_slice(),
            },
            root,
        ))
    }

    /// Prove canonical-arena address provenance across private calls. A raw
    /// memory parameter is admitted only when the function is not exported and
    /// every materialized call site supplies an address descended from
    /// `AllocObject` or another target-layout materialization. A worklist
    /// propagates that least fixed point through local copy edges, private
    /// helper calls, and typed pointer fields in arena-owned actor workspaces. A
    /// field load is admitted only when every store to the same structural field
    /// in the closed package has both an arena-owned root and arena-owned source.
    /// A public parameter, an unproven integer-to-pointer conversion, one forged
    /// call site, or one forged field store prevents the value from entering the
    /// proven set.
    fn derive_arena_owned_locals(&self) -> FxHashMap<RuntimeInstance<'db>, HashSet<RLocalId>> {
        type LocalKey<'db> = (RuntimeInstance<'db>, RLocalId);

        let private_instances = self
            .package
            .functions(self.db)
            .into_iter()
            .filter(|function| function.linkage(self.db) == RuntimeLinkage::Private)
            .map(|function| function.instance(self.db))
            .collect::<HashSet<_>>();
        let mut owned = self
            .prepared_bodies
            .keys()
            .copied()
            .map(|instance| (instance, HashSet::new()))
            .collect::<FxHashMap<_, _>>();
        let mut seeds = Vec::<LocalKey<'db>>::new();
        let mut local_dependents = FxHashMap::<LocalKey<'db>, Vec<LocalKey<'db>>>::default();
        let mut call_dependents = FxHashMap::<LocalKey<'db>, Vec<LocalKey<'db>>>::default();
        let mut call_requirements = FxHashMap::<LocalKey<'db>, usize>::default();
        let mut field_stores =
            FxHashMap::<ArenaPointerFieldKey<'db>, Vec<(LocalKey<'db>, LocalKey<'db>)>>::default();
        let mut field_loads =
            Vec::<(ArenaPointerFieldKey<'db>, LocalKey<'db>, LocalKey<'db>)>::new();

        for (instance, locals) in &self.address_carried_aggregate_values {
            seeds.extend(locals.iter().map(|local| (*instance, *local)));
        }

        for (instance, body) in &self.prepared_bodies {
            for block in &body.blocks {
                for stmt in &block.stmts {
                    if let RStmt::Store { dst, src } | RStmt::CopyInto { dst, src } = stmt
                        && let Some((field, root)) = self.arena_pointer_field(body, dst)
                    {
                        field_stores
                            .entry(field)
                            .or_default()
                            .push(((*instance, root), (*instance, *src)));
                    }
                    let RStmt::Assign { dst, expr } = stmt else {
                        continue;
                    };
                    let destination = (*instance, *dst);
                    match expr {
                        RExpr::AllocObject { .. }
                        | RExpr::MaterializeToObject { .. }
                        | RExpr::MaterializePlaceToObject { .. } => seeds.push(destination),
                        RExpr::Use(source)
                        | RExpr::ProviderToRaw { value: source }
                        | RExpr::RetagRef { value: source }
                        | RExpr::WordToRawAddr { value: source, .. }
                        | RExpr::ProviderFromRaw { raw: source, .. } => {
                            local_dependents
                                .entry((*instance, *source))
                                .or_default()
                                .push(destination);
                        }
                        RExpr::AddrOf { place } => {
                            if let Some(source) = Self::arena_owned_place_source(body, place) {
                                local_dependents
                                    .entry((*instance, source))
                                    .or_default()
                                    .push(destination);
                            }
                        }
                        RExpr::Load { place } => {
                            if let Some((field, root)) = self.arena_pointer_field(body, place) {
                                field_loads.push((field, (*instance, root), destination));
                            }
                        }
                        _ => {}
                    }

                    let RExpr::Call { callee, args } = expr else {
                        continue;
                    };
                    if !private_instances.contains(callee) {
                        continue;
                    }
                    let Some(callee_body) = self.prepared_bodies.get(callee) else {
                        continue;
                    };
                    for (argument, parameter) in args.iter().zip(&callee_body.signature.params) {
                        if !Self::arena_address_param(&parameter.class) {
                            continue;
                        }
                        let parameter = (*callee, parameter.local);
                        *call_requirements.entry(parameter).or_default() += 1;
                        call_dependents
                            .entry((*instance, *argument))
                            .or_default()
                            .push(parameter);
                    }
                }
            }
        }

        let mut queue = VecDeque::<LocalKey<'db>>::new();
        for (instance, local) in seeds {
            if owned.entry(instance).or_default().insert(local) {
                queue.push_back((instance, local));
            }
        }
        let mut satisfied_requirements = FxHashMap::<LocalKey<'db>, usize>::default();
        let mut owned_fields = HashSet::<ArenaPointerFieldKey<'db>>::new();
        loop {
            while let Some(source) = queue.pop_front() {
                if let Some(dependents) = local_dependents.get(&source) {
                    for &(instance, local) in dependents {
                        if owned.entry(instance).or_default().insert(local) {
                            queue.push_back((instance, local));
                        }
                    }
                }
                if let Some(parameters) = call_dependents.get(&source) {
                    for &parameter in parameters {
                        let satisfied = satisfied_requirements.entry(parameter).or_default();
                        *satisfied += 1;
                        if *satisfied == call_requirements[&parameter]
                            && owned.entry(parameter.0).or_default().insert(parameter.1)
                        {
                            queue.push_back(parameter);
                        }
                    }
                }
            }

            let mut changed = false;
            for (field, stores) in &field_stores {
                if owned_fields.contains(field)
                    || !stores.iter().all(|(root, source)| {
                        owned
                            .get(&root.0)
                            .is_some_and(|locals| locals.contains(&root.1))
                            && owned
                                .get(&source.0)
                                .is_some_and(|locals| locals.contains(&source.1))
                    })
                {
                    continue;
                }
                owned_fields.insert(field.clone());
                changed = true;
            }
            for (field, root, destination) in &field_loads {
                if !owned_fields.contains(field)
                    || !owned
                        .get(&root.0)
                        .is_some_and(|locals| locals.contains(&root.1))
                {
                    continue;
                }
                if owned
                    .entry(destination.0)
                    .or_default()
                    .insert(destination.1)
                {
                    queue.push_back(*destination);
                    changed = true;
                }
            }
            if queue.is_empty() && !changed {
                break;
            }
        }
        owned
    }

    fn arena_owned_local(&self, body: &RuntimeBody<'db>, local: RLocalId) -> bool {
        self.arena_owned_locals
            .get(&body.owner)
            .is_some_and(|owned| owned.contains(&local))
    }

    /// Prove the private ABI boundary independently from whole-function arena
    /// reclamation. The local allowlist rejects raw conversion, host effects,
    /// nonlocal stores, and reference returns. Its fixed point follows only
    /// calls that receive an arena-backed value or reference. Calls made solely
    /// from flattened scalar lanes cannot observe the internal pointer and do
    /// not affect this escape proof.
    fn derive_indirect_aggregate_safe_bodies(&self) -> HashSet<RuntimeInstance<'db>> {
        let resumable_owners = self
            .resumable_continuations
            .iter()
            .map(|continuation| continuation.body.owner)
            .collect::<HashSet<_>>();
        let mut dependencies =
            FxHashMap::<RuntimeInstance<'db>, HashSet<RuntimeInstance<'db>>>::default();
        let mut safe = HashSet::new();
        for (instance, body) in &self.prepared_bodies {
            if resumable_owners.contains(instance) || self.analyze_scoped_arena_body(body).is_err()
            {
                continue;
            }
            safe.insert(*instance);
            let mut forwarded = HashSet::new();
            for block in &body.blocks {
                for stmt in &block.stmts {
                    let RStmt::Assign {
                        expr: RExpr::Call { callee, args },
                        ..
                    } = stmt
                    else {
                        continue;
                    };
                    let Some(callee_body) = self.prepared_bodies.get(callee) else {
                        continue;
                    };
                    let indirect_params = self
                        .indirect_aggregate_params
                        .get(callee)
                        .cloned()
                        .unwrap_or_default();
                    let carries_arena_address = args
                        .iter()
                        .zip(&callee_body.signature.params)
                        .any(|(argument, parameter)| {
                            indirect_params.contains(&parameter.local)
                                || body.value_class(*argument).is_some_and(|source| {
                                    (source.contains_transport(self.db)
                                        && parameter.class.contains_transport(self.db))
                                        || matches!(
                                            &parameter.class,
                                            RuntimeClass::Ref {
                                                pointee,
                                                kind: RefKind::Const,
                                                view: RefView::Whole,
                                            } if matches!(source, RuntimeClass::AggregateValue { .. })
                                                && source.shares_runtime_rep_with(self.db, pointee)
                                                && self.aggregate_is_memory_lowerable(source)
                                        )
                                })
                        });
                    if carries_arena_address {
                        forwarded.insert(*callee);
                    }
                }
            }
            dependencies.insert(*instance, forwarded);
        }
        loop {
            let rejected = safe
                .iter()
                .copied()
                .filter(|instance| {
                    dependencies
                        .get(instance)
                        .is_some_and(|callees| callees.iter().any(|callee| !safe.contains(callee)))
                })
                .collect::<Vec<_>>();
            if rejected.is_empty() {
                break;
            }
            for instance in rejected {
                safe.remove(&instance);
            }
        }
        wasm_lower_trace(|| {
            format!(
                "derived indirect aggregate escape-safe bodies, safe={}",
                safe.len(),
            )
        });
        safe
    }

    /// Derive the closed set of ordinary Fe functions whose arena-backed
    /// temporaries cannot cross a function boundary. The analysis starts from
    /// a strict local allowlist, then removes every body that calls outside the
    /// remaining set. This fixed point admits mutually recursive pure helpers
    /// while failing closed on host effects, raw addresses, providers, and
    /// pointer escape. Aggregate references may cross an ordinary Fe call as
    /// scoped borrows: the callee's checkpoint is taken after the caller-owned
    /// object exists, and the callee may neither return that reference nor
    /// store a newly allocated reference through it.
    fn derive_scoped_arena_bodies(&self) -> HashSet<RuntimeInstance<'db>> {
        let resumable_owners = self
            .resumable_continuations
            .iter()
            .map(|continuation| continuation.body.owner)
            .collect::<HashSet<_>>();
        let analyses = self
            .prepared_bodies
            .iter()
            .filter(|(instance, _)| !resumable_owners.contains(instance))
            .filter_map(|(instance, body)| match self.analyze_scoped_arena_body(body) {
                Ok(analysis) => Some((*instance, analysis)),
                Err(reason) => {
                    if self.indirect_aggregate_returns.contains(instance)
                        || self
                            .indirect_aggregate_params
                            .get(instance)
                            .is_some_and(|params| !params.is_empty())
                    {
                        wasm_lower_trace_detail(|| {
                            format!(
                                "reject indirect ABI body from scoped arena, symbol={}, reason={reason}",
                                self.function_symbol(*instance),
                            )
                        });
                    }
                    None
                }
            })
            .collect::<FxHashMap<_, _>>();
        let mut safe = analyses.keys().copied().collect::<HashSet<_>>();
        loop {
            let rejected = safe
                .iter()
                .copied()
                .filter(|instance| {
                    analyses[instance]
                        .callees
                        .iter()
                        .any(|callee| !safe.contains(callee))
                })
                .collect::<Vec<_>>();
            if rejected.is_empty() {
                break;
            }
            for instance in rejected {
                if self.indirect_aggregate_returns.contains(&instance)
                    || self
                        .indirect_aggregate_params
                        .get(&instance)
                        .is_some_and(|params| !params.is_empty())
                {
                    let mut missing = analyses[&instance]
                        .callees
                        .iter()
                        .filter(|callee| !safe.contains(callee))
                        .map(|callee| self.function_symbol(*callee))
                        .collect::<Vec<_>>();
                    missing.sort();
                    wasm_lower_trace_detail(|| {
                        format!(
                            "reject indirect ABI body from scoped arena fixed point, symbol={}, callees={missing:?}",
                            self.function_symbol(instance),
                        )
                    });
                }
                safe.remove(&instance);
            }
        }
        safe.retain(|instance| analyses[instance].allocates);
        safe
    }

    fn analyze_scoped_arena_body(
        &self,
        body: &RuntimeBody<'db>,
    ) -> Result<ScopedArenaAnalysis<'db>, &'static str> {
        if body.blocks.is_empty() {
            return Err("body has no blocks");
        }
        if body.signature.params.iter().any(|param| {
            body.local(param.local).is_none_or(|local| {
                semantic_gpu_resource(self.db, local.semantic_ty)
                    || (!self.scoped_arena_param_is_admissible(&param.class)
                        && !self.arena_owned_local(body, param.local))
            })
        }) {
            return Err("inadmissible parameter boundary");
        }
        if body.signature.ret.as_ref().is_some_and(|class| {
            class.contains_transport(self.db) || self.flat_shape(class).is_none()
        }) {
            return Err("inadmissible return boundary");
        }

        let mut analysis = ScopedArenaAnalysis {
            allocates: self.indirect_aggregate_returns.contains(&body.owner)
                || self
                    .indirect_aggregate_params
                    .get(&body.owner)
                    .is_some_and(|params| !params.is_empty())
                || body.signature.params.iter().any(|param| {
                    matches!(
                        body.local(param.local).map(|local| &local.root),
                        Some(RuntimeLocalRoot::Slot(_))
                    ) && self.scalar_tuple_element_tys(&param.class).is_some()
                }),
            callees: HashSet::new(),
        };
        for block in &body.blocks {
            for stmt in &block.stmts {
                match stmt {
                    RStmt::Assign { dst, expr } => {
                        analysis.allocates |= self
                            .address_carried_aggregate_values
                            .get(&body.owner)
                            .is_some_and(|locals| locals.contains(dst));
                        self.analyze_scoped_arena_expr(body, expr, &mut analysis)?;
                    }
                    RStmt::EnumAssertVariant { .. } => {}
                    RStmt::Store { dst, src } => {
                        if !self.scoped_arena_place_is_local(body, dst, true)
                            || body
                                .value_class(*src)
                                .is_some_and(|class| class.contains_transport(self.db))
                        {
                            return Err("store may escape through a nonlocal place");
                        }
                    }
                    RStmt::CopyInto { dst, src } => {
                        if !self.scoped_arena_place_is_local(body, dst, true)
                            || !self.scoped_arena_copy_source_is_local(body, *src)
                        {
                            return Err("copy may escape through a nonlocal place");
                        }
                    }
                    RStmt::EnumSetTag { root, .. } => {
                        if !self.scoped_arena_writable_ref(body, *root) {
                            return Err("enum tag write targets a nonlocal reference");
                        }
                    }
                    RStmt::EnumWriteVariant { root, fields, .. } => {
                        if !self.scoped_arena_writable_ref(body, *root)
                            || fields.iter().any(|field| {
                                body.value_class(*field)
                                    .is_some_and(|class| class.contains_transport(self.db))
                            })
                        {
                            return Err("enum payload write may escape");
                        }
                    }
                }
            }
            match &block.terminator {
                RTerminator::Goto(_)
                | RTerminator::Branch { .. }
                | RTerminator::SwitchScalar { .. }
                | RTerminator::MatchEnumTag { .. }
                | RTerminator::Trap
                | RTerminator::Return(_)
                | RTerminator::Stop => {}
                RTerminator::TerminalCall { .. }
                | RTerminator::ReturnData { .. }
                | RTerminator::Revert { .. }
                | RTerminator::SelfDestruct { .. } => {
                    return Err("effectful terminal may expose arena state");
                }
            }
        }
        Ok(analysis)
    }

    fn analyze_scoped_arena_expr(
        &self,
        body: &RuntimeBody<'db>,
        expr: &RExpr<'db>,
        analysis: &mut ScopedArenaAnalysis<'db>,
    ) -> Result<(), &'static str> {
        match expr {
            RExpr::Use(src) => {
                analysis.allocates |= body
                    .value_class(*src)
                    .is_some_and(|class| self.is_memory_lowerable_object_ref(class));
            }
            RExpr::ConstScalar(_)
            | RExpr::Unary { .. }
            | RExpr::Binary { .. }
            | RExpr::Cast { .. }
            | RExpr::Bitcast { .. }
            | RExpr::ConstRef { .. }
            | RExpr::AggregateExtract { .. }
            | RExpr::EnumTagOfValue { .. }
            | RExpr::EnumIsVariant { .. }
            | RExpr::EnumExtract { .. } => {}
            RExpr::AllocObject { .. }
            | RExpr::MaterializeToObject { .. }
            | RExpr::MaterializePlaceToObject { .. } => {
                analysis.allocates = true;
                if let RExpr::MaterializePlaceToObject { place } = expr
                    && !self.scoped_arena_place_is_local(body, place, false)
                {
                    return Err("place materialization reads a nonlocal place");
                }
            }
            RExpr::Builtin(builtin) if scoped_arena_builtin_is_pure(builtin) => {}
            RExpr::Call { callee, args } => {
                if mir::host_import_name(self.db, *callee).is_some()
                    || mir::runtime_control_effect_kind(self.db, *callee).is_some()
                    || gpu_intrinsic(self.db, *callee).is_some()
                {
                    return Err("call crosses a host, effect, or GPU boundary");
                }
                let callee_body = self
                    .prepared_bodies
                    .get(callee)
                    .ok_or("call target has no prepared body")?;
                if callee_body.blocks.is_empty()
                    || args.len() != callee_body.signature.params.len()
                    || args
                        .iter()
                        .zip(&callee_body.signature.params)
                        .any(|(arg, param)| {
                            (!self.scoped_arena_param_is_admissible(&param.class)
                                && !self.arena_owned_local(callee_body, param.local))
                                || (body
                                    .value_class(*arg)
                                    .is_some_and(|class| class.contains_transport(self.db))
                                    && !self.scoped_arena_borrowed_ref(body, *arg)
                                    && !self.arena_owned_local(body, *arg))
                        })
                    || callee_body.signature.ret.as_ref().is_some_and(|class| {
                        class.contains_transport(self.db) || self.flat_shape(class).is_none()
                    })
                {
                    return Err("call boundary can carry an escaping reference");
                }
                analysis.allocates |= self.indirect_aggregate_returns.contains(callee)
                    || args
                        .iter()
                        .zip(&callee_body.signature.params)
                        .any(|(arg, param)| {
                            self.indirect_aggregate_params
                                .get(callee)
                                .is_some_and(|params| params.contains(&param.local))
                                || matches!(
                                    &param.class,
                                    RuntimeClass::Ref {
                                        pointee,
                                        kind: RefKind::Const,
                                        view: RefView::Whole,
                                    } if body.value_class(*arg).is_some_and(|source| {
                                        matches!(source, RuntimeClass::AggregateValue { .. })
                                            && source.shares_runtime_rep_with(self.db, pointee)
                                            && self.aggregate_is_memory_lowerable(source)
                                    })
                                )
                        });
                analysis.callees.insert(*callee);
            }
            RExpr::AggregateMake { fields, .. } | RExpr::EnumMake { fields, .. } => {
                if fields.iter().any(|field| {
                    body.value_class(*field)
                        .is_some_and(|class| class.contains_transport(self.db))
                }) {
                    return Err("aggregate value contains a transport");
                }
            }
            RExpr::Load { place } => {
                if !self.scoped_arena_place_is_local(body, place, false) {
                    return Err("load reads a nonlocal place");
                }
            }
            RExpr::EnumGetTag { root } | RExpr::EnumAssertVariantRef { root, .. } => {
                if !self.scoped_arena_read_ref_is_local(body, *root) {
                    return Err("enum reference reads a nonlocal object");
                }
            }
            RExpr::Placeholder { .. } => return Err("placeholder expression remains"),
            RExpr::Builtin(_) => return Err("effectful builtin expression remains"),
            RExpr::ProviderFromRaw { .. } => return Err("raw address becomes a provider"),
            RExpr::WordToRawAddr { .. } => return Err("integer becomes a raw address"),
            RExpr::ProviderToRaw { .. } => return Err("provider becomes a raw address"),
            RExpr::RetagRef { .. } => return Err("reference capability is retagged"),
            RExpr::AddrOf { place } => {
                if !self.scoped_arena_place_is_local(body, place, false) {
                    return Err("address of a nonlocal place is observed");
                }
                analysis.allocates |= matches!(
                    place.root,
                    PlaceRoot::Slot(local)
                        if matches!(body.value_class(local), Some(RuntimeClass::Scalar(_)))
                );
            }
        }
        Ok(())
    }

    fn scoped_arena_param_is_admissible(&self, class: &RuntimeClass<'db>) -> bool {
        if !class.contains_transport(self.db) {
            return self.flat_shape(class).is_some();
        }
        matches!(
            class,
            RuntimeClass::Ref {
                pointee,
                kind: RefKind::Const | RefKind::Object,
                view: RefView::Whole,
            } if self.aggregate_is_memory_lowerable(pointee)
        )
    }

    /// A reference is scope-local when it is either an object created in this
    /// body or an aggregate borrow received from its caller. Calls may pass
    /// either kind only to another body admitted by the same fixed-point proof.
    /// This type check is backed by the expression allowlist above: allocation,
    /// materialization, an admitted parameter, a const region, and `AddrOf` an
    /// already-local place are the only reference origins that survive. Raw,
    /// provider, retagged, effect-returned, and transport-returned origins all
    /// reject the body before reaching this predicate.
    fn scoped_arena_borrowed_ref(&self, body: &RuntimeBody<'db>, local: RLocalId) -> bool {
        matches!(
            body.value_class(local),
            Some(RuntimeClass::Ref {
                pointee,
                kind: RefKind::Const | RefKind::Object,
                view: RefView::Whole,
            }) if self.aggregate_is_memory_lowerable(pointee)
        )
    }

    fn scoped_arena_writable_ref(&self, body: &RuntimeBody<'db>, local: RLocalId) -> bool {
        matches!(
            body.value_class(local),
            Some(RuntimeClass::Ref {
                pointee,
                kind: RefKind::Object,
                view: RefView::Whole,
            }) if self.aggregate_is_memory_lowerable(pointee)
        )
    }

    fn scoped_arena_read_ref_is_local(&self, body: &RuntimeBody<'db>, local: RLocalId) -> bool {
        self.scoped_arena_borrowed_ref(body, local)
            || matches!(
                body.value_class(local),
                Some(RuntimeClass::Ref {
                    kind: RefKind::Const,
                    ..
                })
            )
    }

    fn scoped_arena_copy_source_is_local(&self, body: &RuntimeBody<'db>, local: RLocalId) -> bool {
        body.value_class(local).is_some_and(|class| {
            !class.contains_transport(self.db) || self.scoped_arena_borrowed_ref(body, local)
        })
    }

    fn scoped_arena_place_is_local(
        &self,
        body: &RuntimeBody<'db>,
        place: &RuntimePlace<'db>,
        write: bool,
    ) -> bool {
        match place.root {
            PlaceRoot::Slot(_) => true,
            PlaceRoot::Ref(local) => {
                self.scoped_arena_writable_ref(body, local)
                    || (!write && self.scoped_arena_read_ref_is_local(body, local))
            }
            PlaceRoot::Provider(binding) => body
                .provider_bindings
                .get(binding.as_u32() as usize)
                .is_some_and(|binding| self.arena_owned_local(body, binding.value)),
            PlaceRoot::Ptr {
                addr,
                space: AddressSpaceKind::Memory,
                ..
            } => self.arena_owned_local(body, addr),
            PlaceRoot::Ptr { .. } => false,
        }
    }

    /// Whether `class` is a reference that a private Wasm helper may carry as
    /// one canonical-arena pointer. Object refs name mutable arena storage and
    /// const refs are read-only borrows whose call graph is admitted only by
    /// the scoped-arena escape proof. Both scalar pointees and recursively
    /// memory-lowerable aggregates use that representation. Memory-provider
    /// references retain their existing scalar-handle ABI unless their pointee
    /// is an aggregate already materialized in the arena. Public params and
    /// returns still go through the flattened value ABI or fail closed.
    fn is_memory_lowerable_object_ref(&self, class: &RuntimeClass<'db>) -> bool {
        let RuntimeClass::Ref {
            pointee,
            kind,
            view: RefView::Whole,
        } = class
        else {
            return false;
        };
        match kind {
            RefKind::Const | RefKind::Object => self.aggregate_is_memory_lowerable(pointee),
            RefKind::Provider {
                space: AddressSpaceKind::Memory,
                ..
            } => {
                matches!(**pointee, RuntimeClass::AggregateValue { .. })
                    && self.aggregate_is_memory_lowerable(pointee)
            }
            _ => false,
        }
    }

    fn object_value_layout(&self, class: &RuntimeClass<'db>) -> Option<LayoutId<'db>> {
        let RuntimeClass::Ref {
            pointee,
            kind: RefKind::Object,
            view: RefView::Whole,
        } = class
        else {
            return None;
        };
        let RuntimeClass::AggregateValue { layout } = pointee.as_ref() else {
            return None;
        };
        Some(*layout)
    }

    /// Target layout behind any admitted whole aggregate reference. Const
    /// borrows, object locals, and materialized memory-provider parameters
    /// share target-derived address arithmetic while retaining their distinct
    /// write permissions.
    fn memory_lowerable_ref_layout(&self, class: &RuntimeClass<'db>) -> Option<LayoutId<'db>> {
        let RuntimeClass::Ref {
            pointee,
            kind:
                RefKind::Const
                | RefKind::Object
                | RefKind::Provider {
                    space: AddressSpaceKind::Memory,
                    ..
                },
            view: RefView::Whole,
        } = class
        else {
            return None;
        };
        let RuntimeClass::AggregateValue { layout } = pointee.as_ref() else {
            return None;
        };
        self.aggregate_is_memory_lowerable(pointee)
            .then_some(*layout)
    }

    /// Whether an aggregate value's every scalar leaf passes the R1 scalar
    /// envelope (so it can be stored/loaded through typed Mload/Mstore at i32
    /// addresses). Structs recurse over their fields, arrays over their element;
    /// enums (tagged union memory layout) and nested transports (ref/raw-addr
    /// leaves) fail closed in slice 1.
    fn aggregate_is_memory_lowerable(&self, class: &RuntimeClass<'db>) -> bool {
        match class {
            RuntimeClass::Scalar(scalar) => scalar_ty_r1(scalar).is_ok(),
            RuntimeClass::AggregateValue { layout } => match layout.data(self.db) {
                Layout::Struct(struct_layout) => struct_layout
                    .fields
                    .iter()
                    .all(|field| self.aggregate_is_memory_lowerable(field)),
                Layout::Array(array_layout) => {
                    self.aggregate_is_memory_lowerable(&array_layout.elem)
                }
                // A payload-free enum is one compact target-layout tag in
                // memory and one canonical i32 lane in the value ABI. Payload
                // enums still need a tagged-union memory policy and remain
                // fail-closed.
                Layout::Enum(_) => self.fieldless_enum_tag(*layout).is_some(),
            },
            RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => false,
        }
    }

    fn flat_shape(&self, class: &RuntimeClass<'db>) -> Option<FlatShape> {
        self.flat_shape_visit(class, &mut HashSet::new())
    }

    fn product_element_class(
        &self,
        layout: LayoutId<'db>,
        index: usize,
    ) -> Option<RuntimeClass<'db>> {
        match layout.data(self.db) {
            Layout::Struct(struct_layout) => struct_layout.fields.get(index).cloned(),
            Layout::Array(array_layout) => {
                (index < usize::try_from(array_layout.len).ok()?).then(|| array_layout.elem.clone())
            }
            Layout::Enum(_) => None,
        }
    }

    /// One optional closed-enum upper bound per flattened Wasm parameter leaf.
    /// This follows the same admitted product tree as `flat_shape`; unlike the
    /// scalar type vector, it retains which i32 leaves are fieldless-enum tags
    /// so exported Fe functions can reject host-forged variants at their ABI
    /// boundary before application pattern matching observes them.
    fn flat_leaf_enum_bounds(&self, class: &RuntimeClass<'db>) -> Option<Vec<Option<u32>>> {
        fn visit<'db, I>(
            module: &PortableModuleLowerer<'db, '_, I>,
            class: &RuntimeClass<'db>,
            active: &mut HashSet<LayoutId<'db>>,
            out: &mut Vec<Option<u32>>,
        ) -> Option<()>
        where
            I: Isa<InstSet = NativeInstSet>,
        {
            match class {
                RuntimeClass::Scalar(scalar) => {
                    scalar_ty_r1(scalar).ok()?;
                    out.push(None);
                }
                RuntimeClass::AggregateValue { layout } => {
                    if !active.insert(*layout) {
                        return None;
                    }
                    match layout.data(module.db) {
                        Layout::Struct(struct_layout) => {
                            for field in &struct_layout.fields {
                                visit(module, field, active, out)?;
                            }
                        }
                        Layout::Enum(enum_layout) => {
                            out.push(Some(u32::try_from(enum_layout.variants.len()).ok()?));
                            for variant in &enum_layout.variants {
                                for field in &variant.fields {
                                    visit(module, field, active, out)?;
                                }
                            }
                        }
                        Layout::Array(array_layout) => {
                            for _ in 0..array_layout.len {
                                visit(module, &array_layout.elem, active, out)?;
                            }
                        }
                    }
                    active.remove(layout);
                }
                RuntimeClass::Ref {
                    pointee,
                    kind:
                        RefKind::Provider {
                            space: AddressSpaceKind::Memory,
                            ..
                        },
                    view: RefView::Whole,
                } if matches!(
                    &**pointee,
                    RuntimeClass::AggregateValue { layout }
                        if module.fieldless_enum_tag(*layout).is_some()
                ) =>
                {
                    let RuntimeClass::AggregateValue { layout } = &**pointee else {
                        unreachable!()
                    };
                    let Layout::Enum(enum_layout) = layout.data(module.db) else {
                        unreachable!()
                    };
                    out.push(Some(u32::try_from(enum_layout.variants.len()).ok()?));
                }
                transport @ (RuntimeClass::RawAddr { .. } | RuntimeClass::Ref { .. }) => {
                    module.ty_for_class(transport).ok()?;
                    out.push(None);
                }
            }
            Some(())
        }

        let mut bounds = Vec::new();
        visit(self, class, &mut HashSet::new(), &mut bounds)?;
        Some(bounds)
    }

    fn flat_shape_visit(
        &self,
        class: &RuntimeClass<'db>,
        active: &mut HashSet<LayoutId<'db>>,
    ) -> Option<FlatShape> {
        match class {
            RuntimeClass::Scalar(scalar) => scalar_ty_r1(scalar).ok().map(FlatShape::Leaf),
            RuntimeClass::AggregateValue { layout } => {
                if !active.insert(*layout) {
                    return None;
                }
                let shape = match layout.data(self.db) {
                    Layout::Struct(struct_layout) => {
                        let fields = struct_layout
                            .fields
                            .iter()
                            .map(|field| self.flat_shape_visit(field, active))
                            .collect::<Option<Vec<_>>>()?;
                        // Unit structs contribute zero leaves but remain a valid
                        // node in a closed product tree.
                        FlatShape::Product(fields)
                    }
                    Layout::Enum(enum_layout)
                        if enum_layout
                            .variants
                            .iter()
                            .all(|variant| variant.fields.is_empty()) =>
                    {
                        FlatShape::Leaf(Type::I32)
                    }
                    Layout::Enum(enum_layout) => {
                        let mut variants = Vec::with_capacity(enum_layout.variants.len() + 1);
                        variants.push(FlatShape::Leaf(Type::I32));
                        for variant in &enum_layout.variants {
                            variants.push(FlatShape::Product(
                                variant
                                    .fields
                                    .iter()
                                    .map(|field| self.flat_shape_visit(field, active))
                                    .collect::<Option<Vec<_>>>()?,
                            ));
                        }
                        FlatShape::Product(variants)
                    }
                    Layout::Array(array_layout) => {
                        let len = usize::try_from(array_layout.len).ok()?;
                        let elem = self.flat_shape_visit(&array_layout.elem, active)?;
                        FlatShape::Product(vec![elem; len])
                    }
                };
                active.remove(layout);
                Some(shape)
            }
            // Preserve the previously admitted immediate one-word transport
            // leaves through the existing admissibility SSOT. This accepts only
            // memory RawAddr / memory Provider Ref; object, const, and non-memory
            // refs still fail here.
            transport @ (RuntimeClass::RawAddr { .. } | RuntimeClass::Ref { .. }) => {
                self.ty_for_class(transport).ok().map(FlatShape::Leaf)
            }
        }
    }

    /// The recursively flattened scalar leaves of a nontrivial product tree, or
    /// `None` for scalars, the existing direct one-word-newtype path,
    /// refs and unsupported leaves. Structs and fixed-size arrays share this
    /// closed product representation; unit structs flatten to zero leaves, as
    /// required for terminal `Nil` products.
    /// This is the INTERFACE-level generalization of the single-scalar-field
    /// newtype scalarization to N fields: a `(Pending<B,T1>, Pending<B,T2>)`
    /// own-tuple param flattens into N wasm params, and a `(u64, u64)` return
    /// flattens into N wasm results, with one SSA variable per leaf. A product
    /// with one scalar leaf plus unit structure (for example `Cell<Nil>`) also
    /// uses this path; a direct one-field scalar newtype keeps the R3.4b path.
    /// It is NOT a place/memory model: no element is ever addressed, offset, or
    /// stored.
    ///
    fn scalar_tuple_element_tys(&self, class: &RuntimeClass<'db>) -> Option<Vec<Type>> {
        let shape = self.flat_shape(class)?;
        let mut leaves = Vec::new();
        shape.leaf_types(&mut leaves);
        let preserves_scalar_newtype_path = matches!(
            class,
            RuntimeClass::AggregateValue { layout }
                if self.single_scalar_field(*layout).is_some()
                    || self.fieldless_enum_tag(*layout).is_some()
        );
        (matches!(class, RuntimeClass::AggregateValue { .. }) && !preserves_scalar_newtype_path)
            .then_some(leaves)
    }
}

fn scoped_arena_builtin_is_pure(builtin: &RuntimeBuiltin<'_>) -> bool {
    matches!(
        builtin,
        RuntimeBuiltin::IntTruncate { .. }
            | RuntimeBuiltin::AddMod { .. }
            | RuntimeBuiltin::MulMod { .. }
            | RuntimeBuiltin::Byte { .. }
            | RuntimeBuiltin::SignExtend { .. }
            | RuntimeBuiltin::IntrinsicArith { .. }
            | RuntimeBuiltin::Saturating { .. }
            | RuntimeBuiltin::F32FromI32 { .. }
            | RuntimeBuiltin::I32FromF32 { .. }
            | RuntimeBuiltin::F32Sqrt { .. }
            | RuntimeBuiltin::F32Abs { .. }
            | RuntimeBuiltin::F32Min { .. }
            | RuntimeBuiltin::F32Max { .. }
            | RuntimeBuiltin::F32MinRelaxed { .. }
            | RuntimeBuiltin::F32MaxRelaxed { .. }
            | RuntimeBuiltin::F32Clamp { .. }
            | RuntimeBuiltin::F32Floor { .. }
            | RuntimeBuiltin::F32Ceil { .. }
            | RuntimeBuiltin::F32Trunc { .. }
            | RuntimeBuiltin::F32Round { .. }
    )
}

/// R1 scalar type mapping: reuses the target-neutral scalar carrier mapping but
/// rejects anything wider than i64 (and anything address-shaped), which fails
/// closed per the ratified "u256-on-wasm is out of scope" decision.
fn scalar_ty_r1<'db>(scalar: &ScalarClass<'db>) -> Result<Type, LowerError> {
    let ty = match scalar.repr {
        ScalarRepr::Float { bits: 32 } => Type::F32,
        ScalarRepr::Float { bits } => {
            return Err(LowerError::Unsupported(format!(
                "wasm target carries f32 only; unsupported f{bits} scalar"
            )));
        }
        _ => scalar_ty(scalar)?,
    };
    match ty {
        Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::F32 => Ok(ty),
        wide => Err(LowerError::Unsupported(format!(
            "wasm target (R1) scalar envelope is bool / u8..u64 / i8..i64 / f32; \
             `{wide:?}` (u128/u256/address) is out of scope"
        ))),
    }
}

/// Scalar MIR slots normally stay in SSA. A source-level borrow needs a stable
/// target address instead, so identify exactly the scalar slots consumed by an
/// `AddrOf` expression before local declarations are chosen. This is a closed
/// syntactic property of the prepared body: references cannot be formed by any
/// other runtime expression.
fn address_taken_scalar_slots(body: &RuntimeBody<'_>) -> HashSet<RLocalId> {
    let mut slots = HashSet::new();
    for block in &body.blocks {
        for stmt in &block.stmts {
            let RStmt::Assign {
                expr:
                    RExpr::AddrOf {
                        place:
                            RuntimePlace {
                                root: PlaceRoot::Slot(local),
                                ..
                            },
                    },
                ..
            } = stmt
            else {
                continue;
            };
            if matches!(body.value_class(*local), Some(RuntimeClass::Scalar(_))) {
                slots.insert(*local);
            }
        }
    }
    slots
}

struct PortableFunctionLowerer<'ctx, 'db, 'a, I>
where
    I: Isa<InstSet = NativeInstSet>,
{
    module: &'ctx mut PortableModuleLowerer<'db, 'a, I>,
    body: RuntimeBody<'db>,
    fb: FunctionBuilder<InstInserter>,
    prologue_block: BlockId,
    block_map: Vec<BlockId>,
    vars: FxHashMap<RLocalId, Variable>,
    /// R2.1: scalar-tuple locals carry ONE SSA variable per element word (a
    /// `(Pending, Pending)` local is two i32 vars). A local is in exactly one of
    /// `vars` (a single scalar word) or `tuple_vars` (a flattened scalar tuple),
    /// never both. This is the value-model image of the tuple: it is never
    /// addressed or stored, only its elements are read (`extract_value`), written
    /// (`aggregate_make`), passed as flattened params, and returned as flattened
    /// results.
    tuple_vars: FxHashMap<RLocalId, Vec<Variable>>,
    /// Fixed-size product parameters that Fe models as by-value Slots. Their
    /// public/private Wasm ABI is flattened, then the prologue materializes an
    /// independent arena copy so dynamic indexing observes normal Fe value
    /// semantics without aliasing the caller's storage.
    materialized_param_slots: HashSet<RLocalId>,
    /// Oversized private by-value aggregate parameters carried by one arena
    /// pointer. The caller supplied a fresh deep copy, so this remains an owned
    /// Fe value even though its internal Wasm ABI is indirect.
    indirect_aggregate_params: HashSet<RLocalId>,
    /// Indirect parameters/results plus oversized local aggregates and their
    /// direct `Use` bindings. Every member carries one arena pointer in `vars`.
    address_carried_aggregate_values: HashSet<RLocalId>,
    /// Whether this function transfers its aggregate result to its caller as
    /// one arena pointer rather than flattened Wasm results.
    indirect_aggregate_return: bool,
    /// Scalar Slots remain SSA-promoted unless their address is actually taken.
    /// Each member here carries an aligned canonical-arena pointer in `vars`;
    /// whole-slot reads and writes use target-typed memory operations.
    materialized_scalar_slots: HashSet<RLocalId>,
    /// Only host-visible entries need dynamic closed-enum validation. Private
    /// Fe-to-Fe calls are already typed and avoid the extra branch entirely.
    validate_enum_params: bool,
    /// Change 3: the lazily-created per-function trap block. A dynamic array
    /// index emits `Br(idx < len, ok, trap)`, and checked `usize` overflow emits
    /// `Br(overflow, trap, cont)`; every such check in the function branches to
    /// this one block, whose sole instruction is `Unreachable` (a wasm trap, the
    /// portable image of the EVM revert an out-of-bounds index or overflow panic
    /// would take). Created on first use so functions with no such check emit no
    /// trap block.
    trap_block: Option<BlockId>,
    scoped_arena: bool,
    arena_checkpoint: Option<ValueId>,
}

impl<'ctx, 'db, 'a, I> PortableFunctionLowerer<'ctx, 'db, 'a, I>
where
    I: Isa<InstSet = NativeInstSet>,
{
    fn new(
        module: &'ctx mut PortableModuleLowerer<'db, 'a, I>,
        body: RuntimeBody<'db>,
        func_ref: FuncRef,
        validate_enum_params: bool,
        scoped_arena: bool,
        indirect_aggregate_params: HashSet<RLocalId>,
        indirect_aggregate_return: bool,
    ) -> Result<Self, LowerError> {
        let address_carried_aggregate_values = module
            .address_carried_aggregate_values
            .get(&body.owner)
            .cloned()
            .unwrap_or_default();
        let mut fb = module.builder.func_builder::<InstInserter>(func_ref);
        let prologue_block = fb.append_block();
        let block_map = body.blocks.iter().map(|_| fb.append_block()).collect();

        // Declare one SSA variable per value-carried local. A primitive scalar
        // Slot used only through whole-slot loads/stores is promoted to the same
        // SSA representation; projected/aggregate/addressed Slot operations stay
        // fail-closed. R2.1: a scalar-tuple local gets ONE variable per element
        // word (`tuple_vars`); every other value-carried local keeps its single
        // `ty_for_class` variable (and a multi-field aggregate that is NOT a
        // scalar tuple still fails closed there, unchanged).
        let mut vars = FxHashMap::default();
        let mut tuple_vars: FxHashMap<RLocalId, Vec<Variable>> = FxHashMap::default();
        let mut materialized_param_slots = HashSet::new();
        let materialized_scalar_slots = address_taken_scalar_slots(&body);
        for (idx, local) in body.locals.iter().enumerate() {
            if let RuntimeCarrier::Value(class) = &local.carrier {
                let local_id = RLocalId::from_u32(idx as u32);
                if semantic_gpu_resource(module.db, local.semantic_ty) {
                    let ty = module.gpu_resource_type(local.semantic_ty)?;
                    vars.insert(local_id, fb.declare_var(ty));
                    continue;
                }
                if address_carried_aggregate_values.contains(&local_id) {
                    if !matches!(class, RuntimeClass::AggregateValue { .. })
                        || !module.aggregate_is_memory_lowerable(class)
                    {
                        return Err(LowerError::Internal(format!(
                            "indirect Wasm parameter {local_id:?} is not a memory-lowerable aggregate"
                        )));
                    }
                    vars.insert(local_id, fb.declare_var(Type::I32));
                    continue;
                }
                if matches!(local.root, RuntimeLocalRoot::Slot(_)) {
                    // Conditional-value and multi-exit joins can materialize a
                    // primitive scalar through a MIR Slot even when every
                    // reached operation is a direct load/store of the whole
                    // scalar. Promote exactly that closed shape to an SSA var;
                    // projected/aggregate slots and aliasing operations remain
                    // fail-closed in expression/statement lowering.
                    if matches!(class, RuntimeClass::Scalar(_)) {
                        let ty = if materialized_scalar_slots.contains(&local_id) {
                            Type::I32
                        } else {
                            module.ty_for_class(class)?
                        };
                        vars.insert(local_id, fb.declare_var(ty));
                    } else if body
                        .signature
                        .params
                        .iter()
                        .any(|param| param.local == local_id)
                        && module.scalar_tuple_element_tys(class).is_some()
                    {
                        vars.insert(local_id, fb.declare_var(Type::I32));
                        materialized_param_slots.insert(local_id);
                    }
                    continue;
                }
                if let Some(elem_tys) = module.scalar_tuple_element_tys(class) {
                    let elem_vars = elem_tys
                        .iter()
                        .map(|ty| fb.declare_var(*ty))
                        .collect::<Vec<_>>();
                    tuple_vars.insert(local_id, elem_vars);
                } else if module.is_memory_lowerable_object_ref(class)
                    || module.object_value_layout(class).is_some()
                {
                    // Change 1: a function-local aggregate behind an object /
                    // memory-provider reference lowers to an i32 linear-memory
                    // pointer (the arena offset the AllocObject arm mints). The
                    // local's SSA value IS that pointer; element reads/writes go
                    // through i32 address arithmetic + typed Mload/Mstore. SSA/phi
                    // is free (only the pointer is carried, never the aggregate).
                    vars.insert(local_id, fb.declare_var(Type::I32));
                } else {
                    let ty = module.ty_for_class(class).map_err(|error| match error {
                        LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                            "{message}; while declaring Wasm local {local_id:?} in `{}`",
                            module.function_symbol(body.owner)
                        )),
                        other => other,
                    })?;
                    vars.insert(local_id, fb.declare_var(ty));
                }
            }
        }

        Ok(Self {
            module,
            body,
            fb,
            prologue_block,
            block_map,
            vars,
            tuple_vars,
            materialized_param_slots,
            indirect_aggregate_params,
            address_carried_aggregate_values,
            indirect_aggregate_return,
            materialized_scalar_slots,
            validate_enum_params,
            trap_block: None,
            scoped_arena,
            arena_checkpoint: None,
        })
    }

    fn inst_set(&self) -> &'static sonatina_ir::inst::native::inst_set::NativeInstSet {
        self.module.isa.inst_set()
    }

    fn lower(mut self) -> Result<(), LowerError> {
        let arena_owned = self
            .module
            .arena_owned_locals
            .get(&self.body.owner)
            .cloned()
            .unwrap_or_default();
        check_host_region_arena_disjoint(&self.body, &arena_owned)?;
        let is = self.inst_set();

        // Prologue: bind incoming argument values to their parameter locals,
        // then jump to the MIR entry block (block 0). R2.1: a scalar-tuple param
        // was flattened into N wasm args, so we walk a RUNNING wasm-arg index and
        // bind those N args to the param's N element variables. For every other
        // param this is one arg to one variable, identical to before.
        self.fb.switch_to_block(self.prologue_block);
        if self.scoped_arena {
            let checkpoint_ty = self.fb.ptr_type(Type::I8);
            let checkpoint = self.fb.insert_inst(MemCheckpoint::new(is), checkpoint_ty);
            self.arena_checkpoint = Some(checkpoint);
        }
        let mut scalar_slots = self
            .materialized_scalar_slots
            .iter()
            .copied()
            .collect::<Vec<_>>();
        scalar_slots.sort_by_key(|local| local.as_u32());
        for local in scalar_slots {
            let pointer = self.lower_alloc_target_storage(
                crate::WASM_LAYOUT.word_size_bytes,
                "address-taken scalar slot",
            )?;
            let var = self.var_for(local)?;
            self.fb.def_var(var, pointer);
        }
        let params = self.body.signature.params.clone();
        let arg_values: Vec<ValueId> = self.fb.args().to_vec();
        let mut wasm_arg_idx = 0usize;
        for param in params.iter() {
            let enum_bounds = self
                .validate_enum_params
                .then(|| self.module.flat_leaf_enum_bounds(&param.class))
                .flatten();
            if self.indirect_aggregate_params.contains(&param.local) {
                let var = self.var_for(param.local)?;
                let arg = arg_values[wasm_arg_idx];
                if self.fb.type_of(arg) != Type::I32 {
                    return Err(LowerError::Internal(format!(
                        "indirect aggregate parameter {:?} is not carried by an i32 pointer",
                        param.local
                    )));
                }
                self.fb.def_var(var, arg);
                wasm_arg_idx += 1;
            } else if self.materialized_scalar_slots.contains(&param.local) {
                let arg = arg_values[wasm_arg_idx];
                if let Some(limit) = enum_bounds
                    .as_ref()
                    .and_then(|bounds| bounds.first())
                    .copied()
                    .flatten()
                {
                    self.validate_enum_tag(arg, limit);
                }
                let pointer = self.local_value(param.local)?;
                let ty = self.materialized_scalar_slot_ty(param.local)?;
                self.fb
                    .insert_inst_no_result(Mstore::new(self.inst_set(), pointer, arg, ty));
                wasm_arg_idx += 1;
            } else if self.materialized_param_slots.contains(&param.local) {
                let RuntimeClass::AggregateValue { layout } = &param.class else {
                    return Err(LowerError::Internal(format!(
                        "materialized parameter slot {:?} is not an aggregate",
                        param.local
                    )));
                };
                let shape = self.module.flat_shape(&param.class).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "materialized parameter slot {:?} lost its flat shape",
                        param.local
                    ))
                })?;
                let end = wasm_arg_idx
                    .checked_add(shape.leaf_count())
                    .ok_or_else(|| {
                        LowerError::Internal("materialized parameter arity overflow".to_owned())
                    })?;
                let leaves = arg_values.get(wasm_arg_idx..end).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "materialized parameter {:?} exceeds Wasm argument arity",
                        param.local
                    ))
                })?;
                let pointer = self.lower_alloc_object(*layout)?;
                let mut cursor = 0usize;
                self.store_materialized_leaves(pointer, &param.class, leaves, &mut cursor)?;
                if cursor != leaves.len() {
                    return Err(LowerError::Internal(format!(
                        "materialized parameter {:?} consumed {cursor} of {} leaves",
                        param.local,
                        leaves.len()
                    )));
                }
                let var = self.var_for(param.local)?;
                self.fb.def_var(var, pointer);
                wasm_arg_idx = end;
            } else if let Some(elem_vars) = self.tuple_vars.get(&param.local).cloned() {
                for (leaf, elem_var) in elem_vars.into_iter().enumerate() {
                    let arg = arg_values[wasm_arg_idx];
                    if let Some(limit) = enum_bounds
                        .as_ref()
                        .and_then(|bounds| bounds.get(leaf))
                        .copied()
                        .flatten()
                    {
                        self.validate_enum_tag(arg, limit);
                    }
                    self.fb.def_var(elem_var, arg);
                    wasm_arg_idx += 1;
                }
            } else {
                let var = self.var_for(param.local)?;
                let arg = arg_values[wasm_arg_idx];
                if let Some(limit) = enum_bounds
                    .as_ref()
                    .and_then(|bounds| bounds.first())
                    .copied()
                    .flatten()
                {
                    self.validate_enum_tag(arg, limit);
                }
                self.fb.def_var(var, arg);
                wasm_arg_idx += 1;
            }
        }
        let entry = self.block_map[0];
        self.fb.insert_inst_no_result(Jump::new(is, entry));

        let blocks = self.body.blocks.clone();
        let reachable = compute_reachable_blocks(&self.body);
        for (idx, block) in blocks.iter().enumerate() {
            self.fb.switch_to_block(self.block_map[idx]);
            if !reachable[idx] {
                self.fb.insert_inst_no_result(Unreachable::new(is));
                continue;
            }
            for stmt in &block.stmts {
                self.lower_stmt(stmt)?;
            }
            self.lower_terminator(&block.terminator)
                .map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while lowering block {idx} terminator {:?}",
                        block.terminator
                    )),
                    other => other,
                })?;
        }

        self.fb.seal_all();
        self.fb.finish();
        Ok(())
    }

    fn lower_stmt(&mut self, stmt: &RStmt<'db>) -> Result<(), LowerError> {
        match stmt {
            RStmt::Assign { dst, expr } => {
                // R3.4b step 3: a unit-returning call lowered as a statement. MIR
                // emits `Assign { dst: <Erased unit temp>, expr: Call }` for a call
                // whose callee returns unit (`body.rs` ~2972-2984); the Erased temp
                // carries no value class, so there is no destination variable to
                // define. Emit a no-result call instruction (the EVM precedent is
                // `lower_runtime.rs` ~1276-1283).
                if let RExpr::Call { callee, args } = expr
                    && callee.body(self.module.db).signature.ret.is_none()
                {
                    return self.lower_call_stmt(*callee, args);
                }
                // MIR can preserve the value of a statement-position block in
                // an erased unit sink. A pure `Use` has no effect to emit; its
                // value is intentionally discarded. Keep every effectful or
                // otherwise unsupported erased expression fail-closed.
                if self.body.value_class(*dst).is_none() {
                    return match expr {
                        RExpr::Use(_) => Ok(()),
                        other => Err(LowerError::Unsupported(format!(
                            "wasm target: erased destination {dst:?} cannot discard effectful or unsupported expression `{other:?}`"
                        ))),
                    };
                }
                if self.materialized_scalar_slots.contains(dst) {
                    let value = self.lower_expr(expr, *dst).map_err(|error| match error {
                        LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                            "{message}; while lowering address-taken scalar assignment destination {dst:?}, expression {expr:?}",
                        )),
                        other => other,
                    })?;
                    let ty = self.materialized_scalar_slot_ty(*dst)?;
                    let actual = self.fb.type_of(value);
                    if actual != ty {
                        return Err(LowerError::Internal(format!(
                            "wasm address-taken scalar assignment type mismatch for {dst:?}: lowered `{expr:?}` as {actual:?}, destination requires {ty:?}"
                        )));
                    }
                    let pointer = self.local_value(*dst)?;
                    self.fb
                        .insert_inst_no_result(Mstore::new(self.inst_set(), pointer, value, ty));
                    return Ok(());
                }
                if self.address_carried_aggregate_values.contains(dst) {
                    let value = self
                        .lower_address_carried_aggregate_assign(*dst, expr)
                        .map_err(|error| match error {
                            LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                                "{message}; while lowering address-carried aggregate assignment destination {dst:?}, expression {expr:?}",
                            )),
                            other => other,
                        })?;
                    let var = self.var_for(*dst)?;
                    if self.fb.type_of(value) != Type::I32 {
                        return Err(LowerError::Internal(format!(
                            "address-carried aggregate assignment {dst:?} produced {:?}, expected i32",
                            self.fb.type_of(value),
                        )));
                    }
                    self.fb.def_var(var, value);
                    return Ok(());
                }
                // R2.1: a scalar-tuple destination is produced element-wise (one
                // SSA def per element word), not as a single value, so it takes a
                // dedicated arm rather than the single-`ValueId` `lower_expr` path.
                if self.tuple_vars.contains_key(dst) {
                    return self.lower_tuple_assign(*dst, expr);
                }
                let value = self.lower_expr(expr, *dst).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while lowering assignment destination {dst:?} with class {:?}, expression {expr:?}",
                        self.body.value_class(*dst),
                    )),
                    other => other,
                })?;
                let var = self.var_for(*dst).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; assignment destination {dst:?}, expression {expr:?}"
                    )),
                    other => other,
                })?;
                let expected = self.local_ty(*dst)?;
                let actual = self.fb.type_of(value);
                if actual != expected {
                    return Err(LowerError::Internal(format!(
                        "wasm assignment type mismatch for {dst:?}: lowered `{expr:?}` as {actual:?}, destination requires {expected:?}"
                    )));
                }
                self.fb.def_var(var, value);
                Ok(())
            }
            RStmt::CopyInto { dst, src } => self.lower_copy_value_into_place(dst, *src),
            RStmt::Store { dst, src }
                if matches!((&dst.root, &*dst.path), (PlaceRoot::Slot(_), [])) =>
            {
                let PlaceRoot::Slot(local) = &dst.root else {
                    unreachable!()
                };
                let local = *local;
                if !matches!(self.body.value_class(local), Some(RuntimeClass::Scalar(_))) {
                    return Err(unsupported_place(dst));
                }
                let value = self.local_value(*src)?;
                if self.materialized_scalar_slots.contains(&local) {
                    let pointer = self.local_value(local)?;
                    let ty = self.materialized_scalar_slot_ty(local)?;
                    self.fb
                        .insert_inst_no_result(Mstore::new(self.inst_set(), pointer, value, ty));
                } else {
                    let var = self.var_for(local)?;
                    self.fb.def_var(var, value);
                }
                Ok(())
            }
            RStmt::Store { dst, src } => {
                if let Some((addr, ty)) = self.raw_memory_scalar_place(dst)? {
                    let value = self.local_value(*src)?;
                    let actual = self.fb.type_of(value);
                    if actual != ty {
                        return Err(LowerError::Internal(format!(
                            "wasm memory store type mismatch for `{dst:?}`: source {src:?} has {actual:?}, destination requires {ty:?}"
                        )));
                    }
                    self.fb
                        .insert_inst_no_result(Mstore::new(self.inst_set(), addr, value, ty));
                    return Ok(());
                }
                Err(LowerError::Unsupported(format!(
                    "wasm target (R1) statement `{stmt:?}` is not supported"
                )))
            }
            RStmt::EnumAssertVariant { value, variant } => {
                let class = self.body.value_class(*value).ok_or_else(|| {
                    LowerError::Internal("enum assertion value has no runtime class".to_owned())
                })?;
                let RuntimeClass::AggregateValue { layout } = class else {
                    return Err(LowerError::Unsupported(
                        "wasm target: enum assertions require a value-carried enum".to_owned(),
                    ));
                };
                let layout = *layout;
                let actual = self.enum_tag_value(*value)?;
                let (expected, ty) = self.module.enum_variant_const(layout, *variant)?;
                let expected = self
                    .fb
                    .make_imm_value(immediate_for_const_scalar(&expected, ty)?);
                let valid = self
                    .fb
                    .insert_inst(CmpEq::new(self.inst_set(), actual, expected), Type::I1);
                let cont = self.fb.append_block();
                let trap = self.trap_block();
                self.fb
                    .insert_inst_no_result(Br::new(self.inst_set(), valid, cont, trap));
                self.fb.switch_to_block(cont);
                Ok(())
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) statement `{other:?}` is not supported"
            ))),
        }
    }

    /// Lower a unit-returning call as a statement: a no-result `Call` instruction.
    /// Mirrors `lower_call` but for the unit case (`lower_call` rejects unit-return
    /// callees as value expressions; here the result is discarded by construction).
    fn lower_call_stmt(
        &mut self,
        callee: RuntimeInstance<'db>,
        args: &[RLocalId],
    ) -> Result<(), LowerError> {
        if let Some(intrinsic) = gpu_intrinsic(self.module.db, callee) {
            return match intrinsic {
                GpuIntrinsic::StorageStore => self.lower_gpu_storage_store(args),
                GpuIntrinsic::StorageLoad => Err(LowerError::Internal(
                    "GPU storage load appeared as a unit-returning call".to_owned(),
                )),
            };
        }
        let is = self.inst_set();
        let callee_ref =
            *self.module.func_map.get(&callee).ok_or_else(|| {
                LowerError::Internal("wasm call target was not declared".to_string())
            })?;
        let (arg_vals, call_checkpoint) = self.checked_call_arg_values(callee, callee_ref, args)?;
        self.fb
            .insert_inst_no_result(Call::new(is, callee_ref, arg_vals.into_iter().collect()));
        self.rewind_call_arena(call_checkpoint);
        Ok(())
    }

    fn gpu_resource_element_object(
        &mut self,
        args: &[RLocalId],
    ) -> Result<(ValueId, GpuResourceElementType), LowerError> {
        let [resource, index, ..] = args else {
            return Err(LowerError::Internal(
                "GPU storage intrinsic requires resource and index arguments".to_owned(),
            ));
        };
        let resource_ty = self
            .body
            .local(*resource)
            .map(|local| local.semantic_ty)
            .ok_or_else(|| {
                LowerError::Internal("GPU storage resource local is missing".to_owned())
            })?;
        if !semantic_gpu_resource(self.module.db, resource_ty) {
            return Err(LowerError::Internal(
                "GPU storage intrinsic receiver is not an attributed resource".to_owned(),
            ));
        }
        let element = self.module.gpu_resource_element_type(resource_ty)?;
        let element_ref_ty = self.module.builder.objref_type(element.ty());
        let resource = self.local_value(*resource)?;
        let index = self.local_value(*index)?;
        let object = self.fb.insert_inst(
            ObjIndex::new(self.inst_set(), resource, index),
            element_ref_ty,
        );
        Ok((object, element))
    }

    fn lower_gpu_storage_load_scalar(&mut self, args: &[RLocalId]) -> Result<ValueId, LowerError> {
        let (object, element) = self.gpu_resource_element_object(args)?;
        match element {
            GpuResourceElementType::U32 => Ok(self
                .fb
                .insert_inst(ObjLoad::new(self.inst_set(), object), Type::I32)),
            GpuResourceElementType::Record { .. } => Err(LowerError::Internal(
                "GPU storage record load reached scalar expression lowering".to_owned(),
            )),
        }
    }

    fn lower_gpu_storage_load_tuple(
        &mut self,
        dst: RLocalId,
        args: &[RLocalId],
    ) -> Result<(), LowerError> {
        let (object, element) = self.gpu_resource_element_object(args)?;
        let GpuResourceElementType::Record { fields, .. } = element else {
            return Err(LowerError::Internal(
                "GPU scalar storage load reached tuple expression lowering".to_owned(),
            ));
        };
        let dst_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
            LowerError::Internal("GPU storage record destination is not flattened".to_owned())
        })?;
        if dst_vars.len() != fields {
            return Err(LowerError::Internal(format!(
                "GPU storage record has {fields} fields but its destination has {} leaves",
                dst_vars.len()
            )));
        }
        let field_ref_ty = self.module.builder.objref_type(Type::I32);
        for (index, var) in dst_vars.into_iter().enumerate() {
            let index = self.fb.make_imm_value(Immediate::I32(index as i32));
            let field = self.fb.insert_inst(
                ObjProj::new(self.inst_set(), smallvec1::smallvec![object, index]),
                field_ref_ty,
            );
            let value = self
                .fb
                .insert_inst(ObjLoad::new(self.inst_set(), field), Type::I32);
            self.fb.def_var(var, value);
        }
        Ok(())
    }

    fn lower_gpu_storage_store(&mut self, args: &[RLocalId]) -> Result<(), LowerError> {
        let [_, _, value, ..] = args else {
            return Err(LowerError::Internal(
                "GPU storage store requires resource, index, and value arguments".to_owned(),
            ));
        };
        let (object, element) = self.gpu_resource_element_object(args)?;
        let values = self.local_flat_values(*value)?;
        match element {
            GpuResourceElementType::U32 => {
                let [value] = values.as_slice() else {
                    return Err(LowerError::Internal(
                        "GPU u32 storage store value is not scalar".to_owned(),
                    ));
                };
                self.fb
                    .insert_inst_no_result(ObjStore::new(self.inst_set(), object, *value));
            }
            GpuResourceElementType::Record { fields, .. } => {
                if values.len() != fields {
                    return Err(LowerError::Internal(format!(
                        "GPU storage record has {fields} fields but its stored value has {} leaves",
                        values.len()
                    )));
                }
                let field_ref_ty = self.module.builder.objref_type(Type::I32);
                for (index, value) in values.into_iter().enumerate() {
                    let index = self.fb.make_imm_value(Immediate::I32(index as i32));
                    let field = self.fb.insert_inst(
                        ObjProj::new(self.inst_set(), smallvec1::smallvec![object, index]),
                        field_ref_ty,
                    );
                    self.fb
                        .insert_inst_no_result(ObjStore::new(self.inst_set(), field, value));
                }
            }
        }
        Ok(())
    }

    /// Lower one residual call only when its concrete Sonatina signature and
    /// the caller's prepared lanes agree exactly. Most value arguments flatten
    /// in DFS declaration order. A private read-only aggregate parameter keeps
    /// one pointer instead: the caller materializes an independent arena copy,
    /// and the callee's escape proof guarantees its lifetime. A caller admitted
    /// by the whole-function proof reclaims that copy at its ordinary return.
    /// Otherwise a direct-result call receives a local checkpoint that is
    /// rewound immediately after its scalar results have been captured. Any
    /// missed boundary adaptation fails before an invalid Wasm call reaches the
    /// emitter.
    fn checked_call_arg_values(
        &mut self,
        callee: RuntimeInstance<'db>,
        callee_ref: FuncRef,
        args: &[RLocalId],
    ) -> Result<(Vec<ValueId>, Option<ValueId>), LowerError> {
        let params = self
            .module
            .prepared_bodies
            .get(&callee)
            .ok_or_else(|| LowerError::Internal("prepared Wasm callee is missing".to_owned()))?
            .signature
            .params
            .iter()
            .map(|param| param.class.clone())
            .collect::<Vec<_>>();
        if args.len() != params.len() {
            return Err(LowerError::Internal(format!(
                "call to `{}` has {} Fe arguments but {} prepared parameters",
                self.module.function_symbol(callee),
                args.len(),
                params.len(),
            )));
        }
        let mut values = Vec::new();
        let indirect_params = self
            .module
            .indirect_aggregate_params
            .get(&callee)
            .cloned()
            .unwrap_or_default();
        let callee_params = self
            .module
            .prepared_bodies
            .get(&callee)
            .ok_or_else(|| LowerError::Internal("prepared Wasm callee is missing".to_owned()))?
            .signature
            .params
            .clone();
        let mut call_checkpoint = None;
        for ((arg, param), prepared_param) in args.iter().zip(&params).zip(&callee_params) {
            let source = self.body.value_class(*arg).cloned().ok_or_else(|| {
                LowerError::Internal(format!("call argument {arg:?} has no runtime class"))
            })?;
            let materialize_indirect_value = indirect_params.contains(&prepared_param.local);
            let materialize_read_borrow = matches!(
                param,
                RuntimeClass::Ref {
                    pointee,
                    kind: RefKind::Const,
                    view: RefView::Whole,
                } if matches!(&source, RuntimeClass::AggregateValue { .. })
                    && source.shares_runtime_rep_with(self.module.db, pointee)
                    && self.module.aggregate_is_memory_lowerable(&source)
            );
            if materialize_indirect_value {
                if !self.scoped_arena
                    && !self.module.indirect_aggregate_safe_bodies.contains(&callee)
                {
                    return Err(LowerError::Internal(format!(
                        "call to `{}` requires an indirect aggregate value copy, but the callee failed the arena escape proof",
                        self.module.function_symbol(callee),
                    )));
                }
                if !self.scoped_arena && call_checkpoint.is_none() {
                    // An indirect result is allocated after the same would-be
                    // checkpoint and must remain live in the caller. Keep both
                    // the by-value argument copy and result in the enclosing
                    // arena lifetime; their MIR carrier remains AggregateValue,
                    // so no Fe reference capability is created or exposed.
                    if !self.module.indirect_aggregate_returns.contains(&callee) {
                        let checkpoint_ty = self.fb.ptr_type(Type::I8);
                        call_checkpoint = Some(
                            self.fb
                                .insert_inst(MemCheckpoint::new(self.inst_set()), checkpoint_ty),
                        );
                    }
                }
                if !matches!(param, RuntimeClass::AggregateValue { .. })
                    || !source.shares_runtime_rep_with(self.module.db, param)
                    || !self.module.aggregate_is_memory_lowerable(&source)
                {
                    return Err(LowerError::Internal(format!(
                        "call to `{}` selected an incompatible indirect aggregate argument {arg:?}",
                        self.module.function_symbol(callee),
                    )));
                }
                let argument = if self.is_address_carried_aggregate_value(*arg) {
                    let RuntimeClass::AggregateValue { layout } = source else {
                        return Err(LowerError::Internal(format!(
                            "indirect aggregate argument {arg:?} lost its value layout",
                        )));
                    };
                    let source = self.local_value(*arg)?;
                    self.lower_deep_object_copy(source, layout)?
                } else {
                    self.lower_materialize_to_object(*arg)?
                };
                values.push(argument);
            } else if materialize_read_borrow {
                if !self.scoped_arena
                    && !self.module.indirect_aggregate_safe_bodies.contains(&callee)
                {
                    return Err(LowerError::Internal(format!(
                        "call to `{}` requires a scoped aggregate borrow, but the callee failed the arena escape proof",
                        self.module.function_symbol(callee),
                    )));
                }
                if !self.scoped_arena && call_checkpoint.is_none() {
                    // See the indirect-value arm above. The returned aggregate
                    // owns the enclosing lifetime, so the borrowed materialized
                    // input cannot be reclaimed at this call boundary.
                    if !self.module.indirect_aggregate_returns.contains(&callee) {
                        let checkpoint_ty = self.fb.ptr_type(Type::I8);
                        call_checkpoint = Some(
                            self.fb
                                .insert_inst(MemCheckpoint::new(self.inst_set()), checkpoint_ty),
                        );
                    }
                }
                values.push(self.lower_materialize_to_object(*arg)?);
            } else {
                values.extend(self.local_flat_values(*arg)?);
            }
        }
        let expected = self
            .module
            .builder
            .ctx
            .get_sig(callee_ref)
            .ok_or_else(|| LowerError::Internal("wasm call target has no signature".to_owned()))?
            .args()
            .to_vec();
        let classes = || {
            args.iter()
                .map(|arg| self.body.value_class(*arg).cloned())
                .collect::<Vec<_>>()
        };
        if values.len() != expected.len() {
            return Err(LowerError::Internal(format!(
                "call to `{}` flattened {} arguments from {:?}, but its Wasm signature requires {}",
                self.module.function_symbol(callee),
                values.len(),
                classes(),
                expected.len(),
            )));
        }
        for (index, (value, expected)) in values.iter().zip(expected).enumerate() {
            let actual = self.fb.type_of(*value);
            if actual != expected {
                return Err(LowerError::Internal(format!(
                    "call to `{}` flattened argument {index} as {actual:?} from {:?}, but its Wasm signature requires {expected:?}",
                    self.module.function_symbol(callee),
                    classes(),
                )));
            }
        }
        Ok((values, call_checkpoint))
    }

    fn local_flat_shape(&self, local: RLocalId) -> Result<FlatShape, LowerError> {
        let class = self.body.value_class(local).ok_or_else(|| {
            LowerError::Internal(format!("flattened local {local:?} has no runtime class"))
        })?;
        self.module.flat_shape(class).ok_or_else(|| {
            LowerError::Unsupported(format!(
                "wasm target (R2.2): `{class:?}` is not a recursive product tree of wasm scalars"
            ))
        })
    }

    fn is_address_carried_aggregate_value(&self, local: RLocalId) -> bool {
        self.materialized_param_slots.contains(&local)
            || self.address_carried_aggregate_values.contains(&local)
    }

    fn lower_address_carried_aggregate_assign(
        &mut self,
        dst: RLocalId,
        expr: &RExpr<'db>,
    ) -> Result<ValueId, LowerError> {
        match expr {
            RExpr::AggregateMake { layout, fields } => {
                self.lower_aggregate_make_to_object(dst, *layout, fields)
            }
            RExpr::Load { place } => {
                let Some((source, source_class)) = self.raw_memory_aggregate_place(place)? else {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: address-carried aggregate load requires a memory-backed place: {place:?}"
                    )));
                };
                let destination_class = self.body.value_class(dst).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "address-carried aggregate destination {dst:?} has no class"
                    ))
                })?;
                if !source_class.shares_runtime_rep_with(self.module.db, destination_class) {
                    return Err(LowerError::Unsupported(
                        "address-carried aggregate load has incompatible layouts".to_owned(),
                    ));
                }
                let RuntimeClass::AggregateValue { layout } = destination_class else {
                    return Err(LowerError::Internal(
                        "address-carried aggregate destination lost its layout".to_owned(),
                    ));
                };
                self.lower_deep_object_copy(source, *layout)
            }
            RExpr::AggregateExtract { value, index } => {
                if !self.is_address_carried_aggregate_value(*value) {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: oversized aggregate projection source {value:?} is not address-carried"
                    )));
                }
                let source_class = self.body.value_class(*value).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("aggregate source {value:?} has no class"))
                })?;
                let RuntimeClass::AggregateValue {
                    layout: source_layout,
                } = source_class
                else {
                    return Err(LowerError::Internal(
                        "address-carried aggregate projection source lost its layout".to_owned(),
                    ));
                };
                let field_class = self
                    .module
                    .product_element_class(source_layout, *index as usize)
                    .ok_or_else(|| {
                        LowerError::Unsupported(format!(
                            "wasm target: aggregate projection index {index} is out of bounds"
                        ))
                    })?;
                let destination_class = self.body.value_class(dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!(
                        "aggregate projection destination {dst:?} has no class"
                    ))
                })?;
                if !field_class.shares_runtime_rep_with(self.module.db, &destination_class) {
                    return Err(LowerError::Unsupported(
                        "address-carried aggregate projection has incompatible layouts".to_owned(),
                    ));
                }
                let source = self.local_value(*value)?;
                let projected = match source_layout.data(self.module.db) {
                    Layout::Struct(_) => {
                        let offset = mir::struct_field_offset_bytes(
                            self.module.db,
                            source_layout,
                            hir::analysis::semantic::FieldIndex(*index as u16),
                            crate::WASM_LAYOUT,
                        );
                        self.offset_addr(source, offset)?
                    }
                    Layout::Array(_) => {
                        let stride = mir::array_elem_size_bytes(
                            self.module.db,
                            source_layout,
                            crate::WASM_LAYOUT,
                        );
                        let offset = (*index as usize).checked_mul(stride).ok_or_else(|| {
                            LowerError::Unsupported(
                                "wasm aggregate projection offset overflow".to_owned(),
                            )
                        })?;
                        self.offset_addr(source, offset)?
                    }
                    Layout::Enum(_) => {
                        return Err(LowerError::Unsupported(
                            "wasm target: payload-enum address-carried projection is not implemented"
                                .to_owned(),
                        ));
                    }
                };
                let RuntimeClass::AggregateValue { layout } = destination_class else {
                    return Err(LowerError::Internal(
                        "address-carried aggregate projection destination lost its layout"
                            .to_owned(),
                    ));
                };
                self.lower_deep_object_copy(projected, layout)
            }
            // Indirect calls and ordinary by-value copies already lower to one
            // independently-owned arena pointer in the scalar expression lane.
            RExpr::Call { .. } | RExpr::Use(_) => self.lower_expr(expr, dst),
            other => Err(LowerError::Unsupported(format!(
                "wasm target: oversized aggregate producer `{other:?}` is not memory-lowered"
            ))),
        }
    }

    fn lower_aggregate_make_to_object(
        &mut self,
        dst: RLocalId,
        layout: LayoutId<'db>,
        fields: &[RLocalId],
    ) -> Result<ValueId, LowerError> {
        let destination_class = self.body.value_class(dst).ok_or_else(|| {
            LowerError::Internal(format!("aggregate destination {dst:?} has no class"))
        })?;
        let RuntimeClass::AggregateValue {
            layout: destination_layout,
        } = destination_class
        else {
            return Err(LowerError::Internal(
                "address-carried aggregate make destination is not an aggregate".to_owned(),
            ));
        };
        if *destination_layout != layout {
            return Err(LowerError::Internal(
                "address-carried aggregate make changed layouts".to_owned(),
            ));
        }
        let destination = self.lower_alloc_object(layout)?;
        match layout.data(self.module.db) {
            Layout::Struct(struct_layout) => {
                if fields.len() != struct_layout.fields.len() {
                    return Err(LowerError::Internal(
                        "address-carried struct make field count changed".to_owned(),
                    ));
                }
                for (index, (field, expected)) in
                    fields.iter().zip(struct_layout.fields.iter()).enumerate()
                {
                    let offset = mir::struct_field_offset_bytes(
                        self.module.db,
                        layout,
                        hir::analysis::semantic::FieldIndex(index as u16),
                        crate::WASM_LAYOUT,
                    );
                    let address = self.offset_addr(destination, offset)?;
                    self.lower_store_local_at(address, *field, expected)?;
                }
            }
            Layout::Array(array_layout) => {
                let len = usize::try_from(array_layout.len).map_err(|_| {
                    LowerError::Unsupported("wasm array length exceeds usize".to_owned())
                })?;
                if fields.len() != len {
                    return Err(LowerError::Internal(
                        "address-carried array make element count changed".to_owned(),
                    ));
                }
                let stride = mir::array_elem_size_bytes(self.module.db, layout, crate::WASM_LAYOUT);
                if let Some(first) = fields.first().copied()
                    && fields.iter().all(|field| *field == first)
                {
                    self.lower_repeated_array_store(
                        destination,
                        first,
                        &array_layout.elem,
                        len,
                        stride,
                    )?;
                } else {
                    for (index, field) in fields.iter().enumerate() {
                        let offset = index.checked_mul(stride).ok_or_else(|| {
                            LowerError::Unsupported(
                                "wasm aggregate array offset overflow".to_owned(),
                            )
                        })?;
                        let address = self.offset_addr(destination, offset)?;
                        self.lower_store_local_at(address, *field, &array_layout.elem)?;
                    }
                }
            }
            Layout::Enum(_) => {
                return Err(LowerError::Unsupported(
                    "wasm target: oversized payload-enum construction is not implemented"
                        .to_owned(),
                ));
            }
        }
        Ok(destination)
    }

    fn lower_repeated_array_store(
        &mut self,
        destination: ValueId,
        value: RLocalId,
        class: &RuntimeClass<'db>,
        len: usize,
        stride: usize,
    ) -> Result<(), LowerError> {
        if len == 0 {
            return Ok(());
        }
        let len = i32::try_from(len)
            .map_err(|_| LowerError::Unsupported("wasm array length exceeds i32".to_owned()))?;
        let stride = i32::try_from(stride)
            .map_err(|_| LowerError::Unsupported("wasm array stride exceeds i32".to_owned()))?;
        let is = self.inst_set();
        let entry = self.fb.current_block().ok_or_else(|| {
            LowerError::Internal("repeated aggregate make has no current block".to_owned())
        })?;
        let header = self.fb.append_block();
        let body = self.fb.append_block();
        let done = self.fb.append_block();
        self.fb.insert_inst_no_result(Jump::new(is, header));

        self.fb.switch_to_block(header);
        let zero = self.fb.make_imm_value(Immediate::I32(0));
        let index = self
            .fb
            .insert_inst(Phi::new(is, vec![(zero, entry)]), Type::I32);
        let end = self.fb.make_imm_value(Immediate::I32(len));
        let more = self.fb.insert_inst(Lt::new(is, index, end), Type::I1);
        self.fb.insert_inst_no_result(Br::new(is, more, body, done));

        self.fb.switch_to_block(body);
        let stride = self.fb.make_imm_value(Immediate::I32(stride));
        let offset = self.fb.insert_inst(Mul::new(is, index, stride), Type::I32);
        let address = self
            .fb
            .insert_inst(Add::new(is, destination, offset), Type::I32);
        self.lower_store_local_at(address, value, class)?;
        let one = self.fb.make_imm_value(Immediate::I32(1));
        let next = self.fb.insert_inst(Add::new(is, index, one), Type::I32);
        let back = self.fb.current_block().ok_or_else(|| {
            LowerError::Internal("repeated aggregate make body has no block".to_owned())
        })?;
        self.fb.append_phi_arg(index, next, back);
        self.fb.insert_inst_no_result(Jump::new(is, header));
        self.fb.switch_to_block(done);
        Ok(())
    }

    fn lower_store_local_at(
        &mut self,
        destination: ValueId,
        source: RLocalId,
        expected: &RuntimeClass<'db>,
    ) -> Result<(), LowerError> {
        let actual = self.body.value_class(source).cloned().ok_or_else(|| {
            LowerError::Internal(format!("aggregate field {source:?} has no class"))
        })?;
        if !actual.shares_runtime_rep_with(self.module.db, expected) {
            return Err(LowerError::Unsupported(format!(
                "aggregate field {source:?} has an incompatible runtime representation"
            )));
        }
        match expected {
            RuntimeClass::Scalar(scalar) => {
                let value = self.local_value(source)?;
                let ty = scalar_ty_r1(scalar)?;
                self.fb
                    .insert_inst_no_result(Mstore::new(self.inst_set(), destination, value, ty));
                Ok(())
            }
            RuntimeClass::AggregateValue { layout } => {
                if self.is_address_carried_aggregate_value(source) {
                    let source = self.local_value(source)?;
                    return self.lower_copy_object_bytes_into(source, destination, *layout);
                }
                let shape = self.local_flat_shape(source)?;
                let leaves = self.local_flat_values(source)?;
                if leaves.len() != shape.leaf_count() {
                    return Err(LowerError::Internal(
                        "aggregate field leaf count changed while materializing".to_owned(),
                    ));
                }
                let mut cursor = 0;
                self.store_materialized_leaves(destination, &actual, &leaves, &mut cursor)?;
                if cursor != leaves.len() {
                    return Err(LowerError::Internal(
                        "aggregate field materialization did not consume every leaf".to_owned(),
                    ));
                }
                Ok(())
            }
            RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => {
                Err(LowerError::Unsupported(
                    "wasm target: address-carried aggregate contains a transport field".to_owned(),
                ))
            }
        }
    }

    /// Snapshot a local's leaves in DFS declaration order before callers write
    /// any destination variables.
    fn local_flat_values(&mut self, local: RLocalId) -> Result<Vec<ValueId>, LowerError> {
        if let Some(vars) = self.tuple_vars.get(&local).cloned() {
            return Ok(vars.iter().map(|var| self.fb.use_var(*var)).collect());
        }
        if self.is_address_carried_aggregate_value(local) {
            let class = self.body.value_class(local).cloned().ok_or_else(|| {
                LowerError::Internal(format!(
                    "materialized parameter {local:?} has no runtime class"
                ))
            })?;
            let shape = self.local_flat_shape(local)?;
            let pointer = self.local_value(local)?;
            let mut values = Vec::with_capacity(shape.leaf_count());
            self.load_materialized_leaves(pointer, &class, &shape, &mut values)?;
            return Ok(values);
        }
        if self.materialized_scalar_slots.contains(&local) {
            let pointer = self.local_value(local)?;
            let ty = self.materialized_scalar_slot_ty(local)?;
            return Ok(vec![self.load_memory_scalar(pointer, ty)]);
        }
        Ok(vec![self.local_value(local)?])
    }

    fn enum_tag_value(&mut self, value: RLocalId) -> Result<ValueId, LowerError> {
        if self.tuple_vars.contains_key(&value) {
            return self
                .local_flat_values(value)?
                .into_iter()
                .next()
                .ok_or_else(|| LowerError::Internal("payload enum has no tag lane".to_owned()));
        }
        self.local_value(value)
    }

    fn enum_field_range(
        &self,
        value: RLocalId,
        variant: VariantId<'db>,
        field: hir::analysis::semantic::FieldIndex,
    ) -> Result<(usize, usize, FlatShape), LowerError> {
        let class = self.body.value_class(value).ok_or_else(|| {
            LowerError::Internal(format!("enum source {value:?} has no runtime class"))
        })?;
        let RuntimeClass::AggregateValue { layout } = class else {
            return Err(LowerError::Unsupported(
                "enum extraction source is not a value-carried enum".to_owned(),
            ));
        };
        if variant.enum_layout != *layout {
            return Err(LowerError::Internal(
                "enum extraction variant belongs to another layout".to_owned(),
            ));
        }
        let shape = self.local_flat_shape(value)?;
        let (variant_start, _, variant_shape) = shape
            .field_range(usize::from(variant.index) + 1)
            .ok_or_else(|| {
            LowerError::Internal("payload enum variant shape is missing".to_owned())
        })?;
        let (field_start, field_end, field_shape) = variant_shape
            .field_range(usize::from(field.0))
            .ok_or_else(|| {
                LowerError::Internal("payload enum field shape is missing".to_owned())
            })?;
        Ok((
            variant_start + field_start,
            variant_start + field_end,
            field_shape.clone(),
        ))
    }

    fn zero_value(&mut self, ty: Type) -> Result<ValueId, LowerError> {
        let immediate = zero_immediate(ty).ok_or_else(|| {
            LowerError::Unsupported(format!(
                "wasm payload-enum inactive lane has unsupported type {ty:?}"
            ))
        })?;
        Ok(self.fb.make_imm_value(immediate))
    }

    /// R2.2: lower an assignment whose destination is a recursively flattened
    /// scalar struct tree. The tree is a set of per-leaf SSA variables in DFS
    /// declaration order. Producing forms are shape-compatible `AggregateMake`,
    /// `Use`, and `AggregateExtract`. All sources are snapshotted before any
    /// destination definition. Calls returning the same recursively flattened
    /// shape become Wasm multi-value calls and bind leaf-for-leaf. Everything
    /// else fails closed.
    fn lower_tuple_assign(&mut self, dst: RLocalId, expr: &RExpr<'db>) -> Result<(), LowerError> {
        match expr {
            RExpr::Placeholder { class } => {
                let dst_class = self.body.value_class(dst).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "zero-leaf product destination {dst:?} has no runtime class"
                    ))
                })?;
                let dst_vars = self.tuple_vars.get(&dst).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "zero-leaf product destination {dst:?} has no flattened variables"
                    ))
                })?;
                let placeholder_is_unit_product = self
                    .module
                    .scalar_tuple_element_tys(class)
                    .is_some_and(|leaves| leaves.is_empty());
                if dst_vars.is_empty()
                    && placeholder_is_unit_product
                    && class.shares_runtime_rep_with(self.module.db, dst_class)
                {
                    // A closed unit product has no runtime bits to initialize.
                    return Ok(());
                }
                Err(LowerError::Unsupported(format!(
                    "wasm target (R2.2): non-unit aggregate placeholder for {dst:?} \
                     cannot be represented as flattened SSA values"
                )))
            }
            RExpr::EnumMake {
                layout,
                variant,
                fields,
            } => {
                if variant.enum_layout != *layout {
                    return Err(LowerError::Internal(
                        "payload enum construction uses a foreign variant".to_owned(),
                    ));
                }
                let Layout::Enum(enum_layout) = layout.data(self.module.db) else {
                    return Err(LowerError::Internal(
                        "payload enum construction uses a non-enum layout".to_owned(),
                    ));
                };
                let expected_fields = enum_layout
                    .variants
                    .get(usize::from(variant.index))
                    .ok_or_else(|| {
                        LowerError::Internal("payload enum variant is out of bounds".to_owned())
                    })?
                    .fields
                    .as_ref();
                if fields.len() != expected_fields.len() {
                    return Err(LowerError::Internal(format!(
                        "payload enum variant expects {} fields, got {}",
                        expected_fields.len(),
                        fields.len()
                    )));
                }
                let shape = self.local_flat_shape(dst)?;
                let mut leaf_types = Vec::new();
                shape.leaf_types(&mut leaf_types);
                let mut values = leaf_types
                    .into_iter()
                    .map(|ty| self.zero_value(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let (tag, ty) = self.module.enum_variant_const(*layout, *variant)?;
                values[0] = self
                    .fb
                    .make_imm_value(immediate_for_const_scalar(&tag, ty)?);
                for (field_index, (field, expected_class)) in
                    fields.iter().zip(expected_fields).enumerate()
                {
                    let actual_class = self.body.value_class(*field).ok_or_else(|| {
                        LowerError::Internal(format!("enum field {field:?} has no class"))
                    })?;
                    if !actual_class.shares_runtime_rep_with(self.module.db, expected_class) {
                        return Err(LowerError::Unsupported(
                            "payload enum field has an incompatible runtime representation"
                                .to_owned(),
                        ));
                    }
                    let (start, end, expected_shape) = self.enum_field_range(
                        dst,
                        *variant,
                        hir::analysis::semantic::FieldIndex(field_index as u16),
                    )?;
                    let actual_shape = self.local_flat_shape(*field)?;
                    if actual_shape != expected_shape {
                        return Err(LowerError::Unsupported(
                            "payload enum field has an incompatible flattened shape".to_owned(),
                        ));
                    }
                    let field_values = self.local_flat_values(*field)?;
                    if field_values.len() != end - start {
                        return Err(LowerError::Internal(
                            "payload enum field leaf arity changed".to_owned(),
                        ));
                    }
                    values[start..end].copy_from_slice(&field_values);
                }
                let dst_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal("payload enum destination has no lanes".to_owned())
                })?;
                if dst_vars.len() != values.len() {
                    return Err(LowerError::Internal(
                        "payload enum destination lane arity changed".to_owned(),
                    ));
                }
                for (variable, value) in dst_vars.into_iter().zip(values) {
                    self.fb.def_var(variable, value);
                }
                Ok(())
            }
            RExpr::AggregateMake { fields, .. } => {
                let elem_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("R2.1 tuple dst {dst:?} has no element vars"))
                })?;
                let dst_shape = self.local_flat_shape(dst)?;
                let FlatShape::Product(expected_fields) = dst_shape else {
                    return Err(LowerError::Internal(format!(
                        "R2.1 flattened destination {dst:?} is not a struct"
                    )));
                };
                let dst_class = self.body.value_class(dst).ok_or_else(|| {
                    LowerError::Internal(format!("R2.1 tuple dst {dst:?} has no runtime class"))
                })?;
                let RuntimeClass::AggregateValue { layout } = dst_class else {
                    return Err(LowerError::Internal(format!(
                        "R2.1 flattened destination {dst:?} is not an aggregate"
                    )));
                };
                let expected_classes = match layout.data(self.module.db) {
                    Layout::Struct(dst_layout) => dst_layout.fields.to_vec(),
                    Layout::Array(dst_layout) => {
                        vec![dst_layout.elem.clone(); dst_layout.len as usize]
                    }
                    Layout::Enum(_) => {
                        return Err(LowerError::Unsupported(
                            "wasm target (R2.2): payload enums cannot be flattened".to_string(),
                        ));
                    }
                };
                if fields.len() != expected_fields.len() || fields.len() != expected_classes.len() {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target (R2.1): aggregate make has {} fields but recursive \
                         destination shape expects {}",
                        fields.len(),
                        expected_fields.len()
                    )));
                }
                let mut values = Vec::new();
                for ((field, expected_shape), expected_class) in fields
                    .iter()
                    .zip(expected_fields)
                    .zip(expected_classes.iter())
                {
                    let actual_class = self.body.value_class(*field).ok_or_else(|| {
                        LowerError::Internal(format!("aggregate field {field:?} has no class"))
                    })?;
                    if !actual_class.shares_runtime_rep_with(self.module.db, expected_class) {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target (R2.2): aggregate field {field:?} has an incompatible \
                             recursive runtime representation"
                        )));
                    }
                    let actual_shape = self.local_flat_shape(*field)?;
                    if actual_shape != expected_shape {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target (R2.1): aggregate field {field:?} shape \
                             {actual_shape:?} does not match {expected_shape:?}"
                        )));
                    }
                    values.extend(self.local_flat_values(*field)?);
                }
                if values.len() != elem_vars.len() {
                    return Err(LowerError::Internal(format!(
                        "R2.1 aggregate make produced {} leaves for {} destination vars",
                        values.len(),
                        elem_vars.len()
                    )));
                }
                for (elem_var, value) in elem_vars.into_iter().zip(values) {
                    self.fb.def_var(elem_var, value);
                }
                Ok(())
            }
            RExpr::Use(src) => {
                let dst_class = self.body.value_class(dst).ok_or_else(|| {
                    LowerError::Internal(format!("R2.1 tuple dst {dst:?} has no scalar-tuple type"))
                })?;
                let src_class = self.body.value_class(*src).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target (R2.1): scalar-tuple copy source {src:?} is not a \
                             shallow scalar tuple"
                    ))
                })?;
                if !src_class.shares_runtime_rep_with(self.module.db, dst_class) {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target (R2.1): scalar-tuple copy {src:?}->{dst:?} has incompatible \
                         runtime representations"
                    )));
                }
                let dst_shape = self.local_flat_shape(dst)?;
                let src_shape = self.local_flat_shape(*src)?;
                if src_shape != dst_shape {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target (R2.1): scalar-tree copy {src:?}->{dst:?} has mismatched \
                         recursive shapes {src_shape:?} != {dst_shape:?}"
                    )));
                }
                let dst_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("R2.1 tuple dst {dst:?} has no element vars"))
                })?;
                let values = if self.is_address_carried_aggregate_value(*src) {
                    let pointer = self.local_value(*src)?;
                    let source_class = self.body.value_class(*src).cloned().ok_or_else(|| {
                        LowerError::Internal(format!(
                            "materialized parameter {src:?} has no runtime class"
                        ))
                    })?;
                    let mut values = Vec::with_capacity(src_shape.leaf_count());
                    self.load_materialized_leaves(pointer, &source_class, &src_shape, &mut values)?;
                    values
                } else {
                    self.local_flat_values(*src)?
                };
                if values.len() != dst_vars.len() {
                    return Err(LowerError::Internal(format!(
                        "R2.1 scalar-tree copy has {} source values but {} destination vars",
                        values.len(),
                        dst_vars.len()
                    )));
                }
                for (dst_var, value) in dst_vars.into_iter().zip(values) {
                    self.fb.def_var(dst_var, value);
                }
                Ok(())
            }
            RExpr::AggregateExtract { value, index } => {
                let source_class = self.body.value_class(*value).ok_or_else(|| {
                    LowerError::Internal(format!("aggregate source {value:?} has no class"))
                })?;
                let RuntimeClass::AggregateValue { layout } = source_class else {
                    return Err(LowerError::Unsupported(
                        "wasm target (R2.2): aggregate extract source is not a struct".to_string(),
                    ));
                };
                let field_class = self
                    .module
                    .product_element_class(*layout, *index as usize)
                    .ok_or_else(|| {
                        LowerError::Unsupported(format!(
                            "wasm target (R2.2): aggregate extract index {index} is out of bounds"
                        ))
                    })?;
                let dst_class = self.body.value_class(dst).ok_or_else(|| {
                    LowerError::Internal(format!("aggregate destination {dst:?} has no class"))
                })?;
                if !field_class.shares_runtime_rep_with(self.module.db, dst_class) {
                    return Err(LowerError::Unsupported(
                        "wasm target (R2.2): aggregate projection destination has an \
                         incompatible recursive runtime representation"
                            .to_string(),
                    ));
                }
                let source_shape = self.local_flat_shape(*value)?;
                let Some((start, end, field_shape)) = source_shape.field_range(*index as usize)
                else {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target (R2.1): aggregate extract index {index} is outside \
                         recursive source shape {source_shape:?}"
                    )));
                };
                let dst_shape = self.local_flat_shape(dst)?;
                if &dst_shape != field_shape {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target (R2.1): aggregate extract field shape {field_shape:?} \
                         does not match destination shape {dst_shape:?}"
                    )));
                }
                let source_values = self.local_flat_values(*value)?;
                let values = source_values[start..end].to_vec();
                let dst_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("R2.1 tuple dst {dst:?} has no element vars"))
                })?;
                for (dst_var, value) in dst_vars.into_iter().zip(values) {
                    self.fb.def_var(dst_var, value);
                }
                Ok(())
            }
            RExpr::EnumExtract {
                value,
                variant,
                field,
            } => {
                let (start, end, expected_shape) =
                    self.enum_field_range(*value, *variant, *field)?;
                let dst_shape = self.local_flat_shape(dst)?;
                if dst_shape != expected_shape {
                    return Err(LowerError::Unsupported(
                        "payload enum extraction destination has an incompatible shape".to_owned(),
                    ));
                }
                let values = self.local_flat_values(*value)?[start..end].to_vec();
                let dst_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal(
                        "payload enum extraction destination has no lanes".to_owned(),
                    )
                })?;
                if dst_vars.len() != values.len() {
                    return Err(LowerError::Internal(
                        "payload enum extraction lane arity changed".to_owned(),
                    ));
                }
                for (variable, value) in dst_vars.into_iter().zip(values) {
                    self.fb.def_var(variable, value);
                }
                Ok(())
            }
            RExpr::Load { place } => {
                let Some((pointer, source_class)) = self.raw_memory_aggregate_place(place)? else {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: aggregate load requires a memory-backed aggregate place: \
                         {place:?}"
                    )));
                };
                let dst_class = self.body.value_class(dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("aggregate destination {dst:?} has no class"))
                })?;
                if !source_class.shares_runtime_rep_with(self.module.db, &dst_class) {
                    return Err(LowerError::Unsupported(
                        "wasm target: whole aggregate load has incompatible layouts".to_owned(),
                    ));
                }
                let shape = self.local_flat_shape(dst)?;
                let mut values = Vec::with_capacity(shape.leaf_count());
                self.load_materialized_leaves(pointer, &source_class, &shape, &mut values)?;
                let dst_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("aggregate destination {dst:?} has no vars"))
                })?;
                if dst_vars.len() != values.len() {
                    return Err(LowerError::Internal(
                        "whole aggregate load arity mismatch".to_owned(),
                    ));
                }
                for (var, value) in dst_vars.into_iter().zip(values) {
                    self.fb.def_var(var, value);
                }
                Ok(())
            }
            RExpr::Call { callee, args } => {
                if let Some(intrinsic) = gpu_intrinsic(self.module.db, *callee) {
                    return match intrinsic {
                        GpuIntrinsic::StorageLoad => self.lower_gpu_storage_load_tuple(dst, args),
                        GpuIntrinsic::StorageStore => Err(LowerError::Internal(
                            "GPU storage store appeared as a tuple-returning call".to_owned(),
                        )),
                    };
                }
                let callee_body = self.module.prepared_bodies.get(callee).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "prepared Wasm body for `{}` is missing",
                        self.module.function_symbol(*callee),
                    ))
                })?;
                let callee_class = callee_body.signature.ret.as_ref().ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target: unit-returning call to `{}` cannot initialize an aggregate",
                        self.module.function_symbol(*callee),
                    ))
                })?;
                let dst_class = self.body.value_class(dst).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "aggregate call destination {dst:?} has no runtime class"
                    ))
                })?;
                if !callee_class.shares_runtime_rep_with(self.module.db, dst_class) {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: call to `{}` returns an aggregate representation incompatible with its destination",
                        self.module.function_symbol(*callee),
                    )));
                }
                let callee_shape = self.module.flat_shape(callee_class).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target: call to `{}` returns an aggregate that cannot be recursively flattened",
                        self.module.function_symbol(*callee),
                    ))
                })?;
                let dst_shape = self.local_flat_shape(dst)?;
                if callee_shape != dst_shape {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: call to `{}` returns shape {callee_shape:?}, but destination has shape {dst_shape:?}",
                        self.module.function_symbol(*callee),
                    )));
                }
                let callee_ref = *self.module.func_map.get(callee).ok_or_else(|| {
                    LowerError::Internal("wasm call target was not declared".to_string())
                })?;
                let (arg_vals, call_checkpoint) =
                    self.checked_call_arg_values(*callee, callee_ref, args)?;
                let results = self
                    .fb
                    .insert_call_results(callee_ref, arg_vals.into_iter().collect());
                self.rewind_call_arena(call_checkpoint);
                let dst_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!(
                        "aggregate call destination {dst:?} has no flattened variables"
                    ))
                })?;
                if results.len() != dst_vars.len() {
                    return Err(LowerError::Internal(format!(
                        "call to `{}` produced {} wasm results for {} destination leaves",
                        self.module.function_symbol(*callee),
                        results.len(),
                        dst_vars.len(),
                    )));
                }
                for (var, value) in dst_vars.into_iter().zip(results) {
                    self.fb.def_var(var, value);
                }
                Ok(())
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R2.1): scalar-tuple destination assigned from `{other:?}` is \
                 not supported (only recursive scalar-tree make/copy/extract lower; \
                 aggregate slots and tuple call results remain unsupported)"
            ))),
        }
    }

    fn lower_expr(&mut self, expr: &RExpr<'db>, dst: RLocalId) -> Result<ValueId, LowerError> {
        match expr {
            RExpr::Use(src) => {
                // Item 2: a whole-aggregate local behind an object/memory-provider
                // reference carries its arena POINTER as its SSA value, so a plain
                // `Use` copies the pointer, not the bytes. This is SAFE when it
                // binds a freshly produced object to its variable (`a = use <temp>`
                // where `<temp>` is an `AllocObject`/`MaterializeToObject`/... -- the
                // ordinary local-array init: `a` and the temp are one array). It is
                // the ALIASING BUG when it copies an EXISTING array reference
                // (`let b = a` lowers to `b = use a`, where `a` is itself bound from
                // an object): `a[0] = 9; b[0]` would wrongly observe 9, whereas Fe
                // `[T; N]` is `Copy` (deep-copy semantics). Fail closed on the
                // latter. Materialize an independent value here: Fe aggregate
                // copy semantics are deep even though this target represents
                // addressable aggregates by arena pointers.
                let copy_indirect_value = self.is_address_carried_aggregate_value(*src);
                let copy_object_ref = self.is_object_ref_local(*src)
                    && !self.is_fresh_object_binding(*src)
                    && !self.is_borrow_alias_binding(*src);
                if copy_indirect_value || copy_object_ref {
                    let class = self.body.value_class(*src).cloned().ok_or_else(|| {
                        LowerError::Internal(format!(
                            "aggregate copy source {src:?} has no runtime class"
                        ))
                    })?;
                    let layout = match &class {
                        RuntimeClass::AggregateValue { layout } if copy_indirect_value => *layout,
                        _ => self
                            .module
                            .memory_lowerable_ref_layout(&class)
                            .ok_or_else(|| {
                                LowerError::Internal(format!(
                                    "aggregate copy source {src:?} lost its memory layout"
                                ))
                            })?,
                    };
                    let source = self.local_value(*src)?;
                    return self.lower_deep_object_copy(source, layout);
                }
                self.local_value(*src)
            }
            RExpr::ConstScalar(constant) => {
                let ty = self.local_ty(dst)?;
                let imm = immediate_for_const_scalar(constant, ty)?;
                Ok(self.fb.make_imm_value(imm))
            }
            RExpr::Placeholder {
                class:
                    class @ RuntimeClass::Ref {
                        pointee,
                        kind:
                            RefKind::Provider {
                                space: AddressSpaceKind::Memory,
                                ..
                            },
                        view: RefView::Whole,
                    },
            } if pointee.aggregate_layout().is_some_and(|layout| {
                mir::layout_size_bytes(self.module.db, layout, crate::WASM_LAYOUT) == 0
            }) =>
            {
                let dst_class = self.body.value_class(dst).ok_or_else(|| {
                    LowerError::Internal(format!(
                        "zero-sized borrow placeholder destination {dst:?} has no class"
                    ))
                })?;
                if !class.shares_runtime_rep_with(self.module.db, dst_class) {
                    return Err(LowerError::Internal(format!(
                        "zero-sized borrow placeholder class {class:?} does not match \
                         destination {dst_class:?}"
                    )));
                }
                let ty = self.local_ty(dst)?;
                let immediate = zero_immediate(ty).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target: zero-sized borrow placeholder has unsupported carrier \
                         type {ty:?}"
                    ))
                })?;
                Ok(self.fb.make_imm_value(immediate))
            }
            RExpr::Binary { op, lhs, rhs } => self.lower_binary(*op, *lhs, *rhs, dst),
            RExpr::Unary { op, value } => self.lower_unary(*op, *value),
            RExpr::Cast { value, to } => {
                let source_ty = self.local_ty(*value)?;
                let target_ty = scalar_ty_r1(to).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while lowering cast into {dst:?} from {value:?} to {to:?}"
                    )),
                    other => other,
                })?;
                let source = self.local_value(*value)?;
                if source_ty == target_ty {
                    return Ok(source);
                }
                let int_bits = |ty| match ty {
                    Type::I1 => Some(1),
                    Type::I8 => Some(8),
                    Type::I16 => Some(16),
                    Type::I32 => Some(32),
                    Type::I64 => Some(64),
                    _ => None,
                };
                let (Some(source_bits), Some(target_bits)) =
                    (int_bits(source_ty), int_bits(target_ty))
                else {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: scalar cast `{source_ty:?}` -> `{target_ty:?}` is outside the integer cast envelope"
                    )));
                };
                let is = self.inst_set();
                if source_bits > target_bits {
                    Ok(self
                        .fb
                        .insert_inst(Trunc::new(is, source, target_ty), target_ty))
                } else {
                    let source_signed = matches!(
                        self.body.value_class(*value),
                        Some(RuntimeClass::Scalar(ScalarClass {
                            repr: ScalarRepr::Int { signed: true, .. },
                            ..
                        }))
                    );
                    if source_signed && source_ty != Type::I1 {
                        Ok(self
                            .fb
                            .insert_inst(Sext::new(is, source, target_ty), target_ty))
                    } else {
                        Ok(self
                            .fb
                            .insert_inst(Zext::new(is, source, target_ty), target_ty))
                    }
                }
            }
            RExpr::Bitcast { value, to } => {
                let source_ty = self.local_ty(*value)?;
                let value = self.local_value(*value)?;
                let target_ty = scalar_ty_r1(to).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while lowering bitcast into {dst:?} from {value:?} to {to:?}"
                    )),
                    other => other,
                })?;
                if source_ty == target_ty {
                    Ok(value)
                } else {
                    Ok(self.fb.insert_inst(
                        Bitcast::new(self.module.builder.inst_set(), value, target_ty),
                        target_ty,
                    ))
                }
            }
            RExpr::Call { callee, args } => self.lower_call(*callee, args).map_err(|error| {
                let symbol = self.module.function_symbol(*callee);
                match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while calling `{symbol}` into {dst:?}"
                    )),
                    other => other,
                }
            }),
            RExpr::Builtin(builtin) => self.lower_builtin(builtin, dst),
            // R3.4b step 2: single-scalar-field newtype construction/projection is
            // a no-op on the represented word. `AggregateMake` of one field yields
            // that field's value; `AggregateExtract` at index 0 yields the
            // aggregate's value (which IS the field's word). This is what executes
            // the `u32` newtypes (`Pending<T>` / `KernelId` / `WebGpuRef`).
            RExpr::AggregateMake { layout, fields } => {
                self.lower_scalar_newtype_make(*layout, fields)
            }
            RExpr::AggregateExtract { value, index } => {
                if self.tuple_vars.contains_key(value)
                    || self.is_address_carried_aggregate_value(*value)
                {
                    let source_class = self.body.value_class(*value).ok_or_else(|| {
                        LowerError::Internal(format!("aggregate source {value:?} has no class"))
                    })?;
                    let RuntimeClass::AggregateValue { layout } = source_class else {
                        return Err(LowerError::Unsupported(
                            "wasm target (R2.2): scalar extract source is not a struct".to_string(),
                        ));
                    };
                    let field_class = self
                        .module
                        .product_element_class(*layout, *index as usize)
                        .ok_or_else(|| {
                            LowerError::Unsupported(format!(
                                "wasm target (R2.2): scalar extract index {index} is out of bounds"
                            ))
                        })?;
                    let dst_class = self.body.value_class(dst).ok_or_else(|| {
                        LowerError::Internal(format!("scalar destination {dst:?} has no class"))
                    })?;
                    if !field_class.shares_runtime_rep_with(self.module.db, dst_class) {
                        return Err(LowerError::Unsupported(
                            "wasm target (R2.2): scalar projection destination has an \
                             incompatible runtime representation"
                                .to_string(),
                        ));
                    }
                    let source_shape = self.local_flat_shape(*value)?;
                    let Some((start, end, _)) = source_shape.field_range(*index as usize) else {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target (R2.2): scalar aggregate extract index {index} is \
                             outside recursive source shape {source_shape:?}"
                        )));
                    };
                    if end - start != 1 {
                        return Err(LowerError::Internal(format!(
                            "scalar destination requested aggregate field with {} leaves",
                            end - start
                        )));
                    }
                    return Ok(self.local_flat_values(*value)?[start]);
                }
                self.lower_scalar_newtype_extract(*value, *index)
            }
            RExpr::EnumMake {
                layout,
                variant,
                fields,
            } => {
                if !fields.is_empty() {
                    return Err(LowerError::Unsupported(
                        "wasm target: payload enum construction is not lowered".to_owned(),
                    ));
                }
                let (constant, ty) = self
                    .module
                    .fieldless_enum_variant_const(*layout, *variant)?;
                Ok(self
                    .fb
                    .make_imm_value(immediate_for_const_scalar(&constant, ty)?))
            }
            RExpr::EnumTagOfValue { value } => {
                let class = self.body.value_class(*value).ok_or_else(|| {
                    LowerError::Internal("enum tag source has no runtime class".to_owned())
                })?;
                let RuntimeClass::AggregateValue { layout } = class else {
                    return Err(LowerError::Unsupported(
                        "wasm target: enum tag source is not a value-carried enum".to_owned(),
                    ));
                };
                if !matches!(layout.data(self.module.db), Layout::Enum(_)) {
                    return Err(LowerError::Internal(
                        "enum tag source carries a non-enum aggregate layout".to_owned(),
                    ));
                }
                let value = self.enum_tag_value(*value)?;
                if self.fb.type_of(value) != Type::I32 {
                    return Err(LowerError::Internal(
                        "enum tag escaped its canonical i32 carrier".to_owned(),
                    ));
                }
                // A value-carried fieldless enum stays in the target-neutral
                // 32-bit SSA carrier. Compact layout tags matter only when an
                // enum is projected to addressable memory; narrowing here
                // leaked i8/i16 into function signatures and GPU uniform
                // control flow even though the browser/Wasm ABI is i32.
                Ok(value)
            }
            RExpr::EnumIsVariant { value, variant } => {
                let class = self.body.value_class(*value).ok_or_else(|| {
                    LowerError::Internal("enum comparison source has no runtime class".to_owned())
                })?;
                let RuntimeClass::AggregateValue { layout } = class else {
                    return Err(LowerError::Unsupported(
                        "wasm target: enum comparison source is not a value-carried enum"
                            .to_owned(),
                    ));
                };
                let layout = *layout;
                let actual = self.enum_tag_value(*value)?;
                let (expected, ty) = self.module.enum_variant_const(layout, *variant)?;
                let expected = self
                    .fb
                    .make_imm_value(immediate_for_const_scalar(&expected, ty)?);
                Ok(self
                    .fb
                    .insert_inst(CmpEq::new(self.inst_set(), actual, expected), Type::I1))
            }
            RExpr::EnumExtract {
                value,
                variant,
                field,
            } => {
                let (start, end, _) = self.enum_field_range(*value, *variant, *field)?;
                if end - start != 1 {
                    return Err(LowerError::Unsupported(
                        "scalar destination cannot receive a multi-lane payload enum field"
                            .to_owned(),
                    ));
                }
                Ok(self.local_flat_values(*value)?[start])
            }
            RExpr::EnumGetTag { root } => {
                let class = self.body.value_class(*root).ok_or_else(|| {
                    LowerError::Internal("enum reference has no runtime class".to_owned())
                })?;
                let RuntimeClass::Ref { pointee, kind, .. } = class else {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: enum reference tag source is not a reference: {class:?}"
                    )));
                };
                let RuntimeClass::AggregateValue { layout } = &**pointee else {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: enum reference does not point to an aggregate: {pointee:?}"
                    )));
                };
                let tag = self.module.fieldless_enum_tag(*layout).ok_or_else(|| {
                    LowerError::Unsupported(
                        "wasm target: payload enum reference tags are not lowered".to_owned(),
                    )
                })?;
                match kind {
                    // A borrowed method receiver carried through the canonical
                    // memory provider uses the provider word itself as the
                    // value (the same established identity used by scalar
                    // actor fields and one-word newtypes). Extract the compact
                    // MIR tag from that canonical i32 carrier.
                    RefKind::Provider {
                        space: AddressSpaceKind::Memory,
                        ..
                    } => {
                        let value = self.local_value(*root)?;
                        if self.fb.type_of(value) != Type::I32 {
                            return Err(LowerError::Internal(
                                "fieldless enum provider escaped its canonical i32 carrier"
                                    .to_owned(),
                            ));
                        }
                        Ok(value)
                    }
                    // Object references, unlike provider-value receivers, are
                    // actual linear-memory addresses.
                    RefKind::Object => {
                        let address = self.local_value(*root)?;
                        let compact_ty = scalar_ty_r1(&tag)?;
                        let compact = self.load_memory_scalar(address, compact_ty);
                        if compact_ty == Type::I32 {
                            Ok(compact)
                        } else {
                            Ok(self.fb.insert_inst(
                                Zext::new(self.inst_set(), compact, Type::I32),
                                Type::I32,
                            ))
                        }
                    }
                    RefKind::Const
                    | RefKind::Provider {
                        space:
                            AddressSpaceKind::Storage
                            | AddressSpaceKind::Transient
                            | AddressSpaceKind::Calldata
                            | AddressSpaceKind::Code,
                        ..
                    } => Err(LowerError::Unsupported(
                        "wasm target: enum reference tags require a memory-backed receiver"
                            .to_owned(),
                    )),
                }
            }
            // Browser component descriptors construct memory pointers from
            // their canonical wasm32 `u32` offsets. At this target the scalar
            // and raw-address carriers are the same i32; retain the MIR
            // address-space check and lower only that representation identity.
            RExpr::WordToRawAddr {
                value,
                space: AddressSpaceKind::Memory,
                ..
            } if self.local_ty(*value)? == Type::I32 => self.local_value(*value),
            // On wasm32 both a memory provider handle and its explicit raw form
            // are the same i32 linear-memory byte offset. Keep this conversion
            // as a checked representation identity; object/const/non-memory
            // providers remain outside the admitted RawAddr slice.
            RExpr::ProviderToRaw { value }
                if matches!(
                    self.body.value_class(*value),
                    Some(
                        RuntimeClass::RawAddr {
                            space: AddressSpaceKind::Memory,
                            ..
                        } | RuntimeClass::Ref {
                            kind: RefKind::Provider {
                                space: AddressSpaceKind::Memory,
                                ..
                            },
                            ..
                        }
                    )
                ) =>
            {
                self.local_value(*value)
            }
            // A borrowed method receiver projected from a materialized actor
            // state record is already represented by its canonical-arena byte
            // address. Preserve that address as the reference value; the
            // callee's ordinary field loads remain target-layout-derived.
            RExpr::AddrOf { place } => {
                if let Some((address, _)) = self.raw_memory_scalar_place(place)? {
                    return Ok(address);
                }
                let Some((address, _)) = self.raw_memory_aggregate_place(place)? else {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: address-of requires a memory-backed place: \
                         {place:?}"
                    )));
                };
                Ok(address)
            }
            // R2.0 (Fable seat ruling, control-effects ladder section 7): the only
            // place read the wasm target lowers is an IDENTITY on an already
            // value-carried transport word. Own-mode consumption of a word-carried
            // token (`Wait::wait<T>(_ pending: own Pending<T>)`) reaches lowering as
            // exactly this shape (`load *%p`); anything needing an address, an offset,
            // a store, or an object materialization is R2 proper and stays fail-closed.
            RExpr::Load { place } => {
                let dst_class = self.body.value_class(dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("place-read destination {dst:?} has no class"))
                })?;
                if let RuntimeClass::AggregateValue { layout } = &dst_class
                    && (self.module.single_scalar_field(*layout).is_some()
                        || self.module.fieldless_enum_tag(*layout).is_some())
                {
                    let Some((address, projected)) = self.raw_memory_aggregate_place(place)? else {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target: scalar-represented aggregate load requires a memory-backed place: {place:?}"
                        )));
                    };
                    if !projected.shares_runtime_rep_with(self.module.db, &dst_class) {
                        return Err(LowerError::Unsupported(
                            "wasm target: scalar-represented aggregate load has an incompatible layout"
                                .to_owned(),
                        ));
                    }
                    if let Some(scalar) = self.module.single_scalar_field(*layout) {
                        return Ok(self.load_memory_scalar(address, scalar_ty_r1(&scalar)?));
                    }
                    let tag = self.module.fieldless_enum_tag(*layout).ok_or_else(|| {
                        LowerError::Internal(
                            "scalar-represented aggregate lost its enum tag layout".to_owned(),
                        )
                    })?;
                    let compact_ty = scalar_ty_r1(&tag)?;
                    let compact = self.load_memory_scalar(address, compact_ty);
                    return if compact_ty == Type::I32 {
                        Ok(compact)
                    } else {
                        Ok(self
                            .fb
                            .insert_inst(Zext::new(self.inst_set(), compact, Type::I32), Type::I32))
                    };
                }
                if let RuntimeClass::Ref { pointee, .. } = &dst_class
                    && self
                        .module
                        .memory_lowerable_ref_layout(&dst_class)
                        .is_some()
                {
                    let Some((address, projected)) = self.raw_memory_aggregate_place(place)? else {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target: aggregate borrow requires a memory-backed place: {place:?}"
                        )));
                    };
                    if !projected.shares_runtime_rep_with(self.module.db, pointee) {
                        return Err(LowerError::Unsupported(
                            "wasm target: projected aggregate borrow has an incompatible pointee"
                                .to_owned(),
                        ));
                    }
                    return Ok(address);
                }
                self.lower_place_read(place)
            }
            // Change 2: allocate a function-local aggregate in the wasm canonical
            // arena. The value produced is the aligned i32 linear-memory pointer.
            RExpr::AllocObject { layout } => self.lower_alloc_object(*layout),
            RExpr::MaterializeToObject { src } => self.lower_materialize_to_object(*src),
            RExpr::MaterializePlaceToObject { place } => {
                self.lower_materialize_place_to_object(place, dst)
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) expression `{other:?}` is not supported"
            ))),
        }
    }

    /// Change 2: `AllocObject` -> a canonical-arena allocation returning an
    /// 8-byte-aligned i32 pointer. Size comes from MIR's target-layout SSOT
    /// (`layout_size_bytes` under `WASM_LAYOUT`; a `[u32; N]` is `N * 8`). The
    /// arena (`fe_cabi_alloc`) is byte-granular and not alignment-aware, so we
    /// over-allocate by 7 and round the returned pointer up to the next 8-byte
    /// boundary `(p + 7) & -8` (the align-up dance the canonical-lane wrappers
    /// use). A loop that reaches this each iteration grows the arena and never
    /// frees; `fe_cabi_reset` between top-level calls reclaims it.
    fn lower_alloc_object(&mut self, layout: LayoutId<'db>) -> Result<ValueId, LowerError> {
        let size = mir::layout_size_bytes(self.module.db, layout, crate::WASM_LAYOUT);
        self.lower_alloc_target_storage(size, &format!("AllocObject `{layout:?}`"))
    }

    fn lower_alloc_target_storage(
        &mut self,
        size: usize,
        description: &str,
    ) -> Result<ValueId, LowerError> {
        let is = self.inst_set();
        const ALIGN: i32 = 8;
        let alloc_size = size
            .checked_add((ALIGN - 1) as usize)
            .and_then(|size| i32::try_from(size).ok())
            .ok_or_else(|| {
                LowerError::Unsupported(format!(
                    "wasm target: {description} size {size} bytes exceeds i32"
                ))
            })?;
        let alloc_size = self.fb.make_imm_value(Immediate::I32(alloc_size));
        let raw = self
            .fb
            .insert_inst(MemAllocDynamic::new(is, alloc_size), Type::I32);
        let slack = self.fb.make_imm_value(Immediate::I32(ALIGN - 1));
        let biased = self.fb.insert_inst(Add::new(is, raw, slack), Type::I32);
        let mask = self.fb.make_imm_value(Immediate::I32(-ALIGN));
        Ok(self.fb.insert_inst(And::new(is, biased, mask), Type::I32))
    }

    /// Copy one addressable Fe aggregate into independent canonical-arena
    /// storage. A compact byte loop preserves the target layout exactly without
    /// expanding large records/arrays into one pair of instructions per byte.
    fn lower_deep_object_copy(
        &mut self,
        source: ValueId,
        layout: LayoutId<'db>,
    ) -> Result<ValueId, LowerError> {
        let destination = self.lower_alloc_object(layout)?;
        self.lower_copy_object_bytes_into(source, destination, layout)?;
        Ok(destination)
    }

    fn lower_copy_object_bytes_into(
        &mut self,
        source: ValueId,
        destination: ValueId,
        layout: LayoutId<'db>,
    ) -> Result<(), LowerError> {
        let byte_len = mir::layout_size_bytes(self.module.db, layout, crate::WASM_LAYOUT);
        let byte_len = i32::try_from(byte_len).map_err(|_| {
            LowerError::Unsupported(format!("wasm aggregate copy size {byte_len} exceeds i32"))
        })?;
        let is = self.inst_set();
        let copy_entry = self.fb.current_block().ok_or_else(|| {
            LowerError::Internal("aggregate copy has no current block".to_owned())
        })?;
        let copy_header = self.fb.append_block();
        let copy_body = self.fb.append_block();
        let copy_done = self.fb.append_block();
        self.fb.insert_inst_no_result(Jump::new(is, copy_header));

        self.fb.switch_to_block(copy_header);
        let zero = self.fb.make_imm_value(Immediate::I32(0));
        let index = self
            .fb
            .insert_inst(Phi::new(is, vec![(zero, copy_entry)]), Type::I32);
        let end = self.fb.make_imm_value(Immediate::I32(byte_len));
        let more = self.fb.insert_inst(Lt::new(is, index, end), Type::I1);
        self.fb
            .insert_inst_no_result(Br::new(is, more, copy_body, copy_done));

        self.fb.switch_to_block(copy_body);
        let source_byte = self.fb.insert_inst(Add::new(is, source, index), Type::I32);
        let destination_byte = self
            .fb
            .insert_inst(Add::new(is, destination, index), Type::I32);
        let byte = self
            .fb
            .insert_inst(Mload::new(is, source_byte, Type::I8), Type::I8);
        self.fb
            .insert_inst_no_result(Mstore::new(is, destination_byte, byte, Type::I8));
        let one = self.fb.make_imm_value(Immediate::I32(1));
        let next = self.fb.insert_inst(Add::new(is, index, one), Type::I32);
        let copy_back = self
            .fb
            .current_block()
            .ok_or_else(|| LowerError::Internal("aggregate copy body has no block".to_owned()))?;
        self.fb.append_phi_arg(index, next, copy_back);
        self.fb.insert_inst_no_result(Jump::new(is, copy_header));
        self.fb.switch_to_block(copy_done);
        Ok(())
    }

    fn store_materialized_leaves(
        &mut self,
        pointer: ValueId,
        class: &RuntimeClass<'db>,
        leaves: &[ValueId],
        cursor: &mut usize,
    ) -> Result<(), LowerError> {
        match class {
            RuntimeClass::Scalar(scalar) => {
                let value = *leaves.get(*cursor).ok_or_else(|| {
                    LowerError::Internal(
                        "materialized aggregate is missing a scalar leaf".to_owned(),
                    )
                })?;
                let ty = scalar_ty_r1(scalar)?;
                self.fb
                    .insert_inst_no_result(Mstore::new(self.inst_set(), pointer, value, ty));
                *cursor += 1;
                Ok(())
            }
            RuntimeClass::AggregateValue { layout } => match layout.data(self.module.db) {
                Layout::Struct(struct_layout) => {
                    for (index, field) in struct_layout.fields.iter().enumerate() {
                        let offset = mir::struct_field_offset_bytes(
                            self.module.db,
                            *layout,
                            hir::analysis::semantic::FieldIndex(index as u16),
                            crate::WASM_LAYOUT,
                        );
                        let address = self.offset_addr(pointer, offset)?;
                        self.store_materialized_leaves(address, field, leaves, cursor)?;
                    }
                    Ok(())
                }
                Layout::Array(array_layout) => {
                    let stride =
                        mir::array_elem_size_bytes(self.module.db, *layout, crate::WASM_LAYOUT);
                    for index in 0..usize::try_from(array_layout.len).map_err(|_| {
                        LowerError::Unsupported("wasm array length exceeds usize".to_owned())
                    })? {
                        let offset = index.checked_mul(stride).ok_or_else(|| {
                            LowerError::Unsupported(
                                "wasm materialization array offset overflow".to_owned(),
                            )
                        })?;
                        let address = self.offset_addr(pointer, offset)?;
                        self.store_materialized_leaves(
                            address,
                            &array_layout.elem,
                            leaves,
                            cursor,
                        )?;
                    }
                    Ok(())
                }
                Layout::Enum(_) => {
                    let tag = self.module.fieldless_enum_tag(*layout).ok_or_else(|| {
                        LowerError::Unsupported(
                            "wasm target: payload-enum materialization is not implemented"
                                .to_owned(),
                        )
                    })?;
                    let value = *leaves.get(*cursor).ok_or_else(|| {
                        LowerError::Internal(
                            "materialized enum is missing its canonical tag leaf".to_owned(),
                        )
                    })?;
                    let compact_ty = scalar_ty_r1(&tag)?;
                    let actual_ty = self.fb.type_of(value);
                    let stored = if actual_ty == compact_ty {
                        value
                    } else if actual_ty == Type::I32
                        && matches!(compact_ty, Type::I1 | Type::I8 | Type::I16)
                    {
                        self.fb
                            .insert_inst(Trunc::new(self.inst_set(), value, compact_ty), compact_ty)
                    } else {
                        return Err(LowerError::Internal(format!(
                            "materialized enum tag has value type {actual_ty:?}, compact memory requires {compact_ty:?}"
                        )));
                    };
                    self.fb.insert_inst_no_result(Mstore::new(
                        self.inst_set(),
                        pointer,
                        stored,
                        compact_ty,
                    ));
                    *cursor += 1;
                    Ok(())
                }
            },
            RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => {
                Err(LowerError::Unsupported(
                    "wasm target: materialized aggregate contains a transport leaf".to_owned(),
                ))
            }
        }
    }

    /// Store one recursively flattened Fe value into an addressable aggregate
    /// projection. This is the write-side twin of aggregate-place loading: the
    /// source remains a value tree, the destination address and every nested
    /// offset come from MIR's target layout, and no pointer aliases escape into
    /// the value ABI. It enables ordinary `mut self` value builders to assign a
    /// complete nested record rather than spelling one store per scalar leaf.
    fn lower_copy_value_into_place(
        &mut self,
        destination: &RuntimePlace<'db>,
        source: RLocalId,
    ) -> Result<(), LowerError> {
        let Some((pointer, destination_class)) = self.raw_memory_aggregate_place(destination)?
        else {
            return Err(LowerError::Unsupported(format!(
                "wasm target: aggregate copy destination is not canonical-arena-backed: {destination:?}"
            )));
        };
        let source_class = self.body.value_class(source).cloned().ok_or_else(|| {
            LowerError::Internal(format!("aggregate copy source {source:?} has no class"))
        })?;
        if !matches!(source_class, RuntimeClass::AggregateValue { .. }) {
            return Err(LowerError::Unsupported(format!(
                "wasm target: aggregate copy source {source:?} is not a flattened value"
            )));
        }
        if !source_class.shares_runtime_rep_with(self.module.db, &destination_class) {
            return Err(LowerError::Unsupported(format!(
                "wasm target: aggregate copy source and destination layouts differ: {source_class:?} / {destination_class:?}"
            )));
        }
        if !self.module.aggregate_is_memory_lowerable(&source_class) {
            return Err(LowerError::Unsupported(format!(
                "wasm target: aggregate copy source {source:?} has non-memory-lowerable leaves"
            )));
        }
        let shape = self.module.flat_shape(&source_class).ok_or_else(|| {
            LowerError::Unsupported(format!(
                "wasm target: aggregate copy source {source:?} cannot be recursively flattened"
            ))
        })?;
        let leaves = self.local_flat_values(source)?;
        if leaves.len() != shape.leaf_count() {
            return Err(LowerError::Internal(format!(
                "aggregate copy source {source:?} exposed {} leaves for shape {shape:?}",
                leaves.len()
            )));
        }
        let mut cursor = 0usize;
        self.store_materialized_leaves(pointer, &source_class, &leaves, &mut cursor)?;
        if cursor != leaves.len() {
            return Err(LowerError::Internal(format!(
                "aggregate copy source {source:?} consumed {cursor} of {} leaves",
                leaves.len()
            )));
        }
        Ok(())
    }

    /// Load a flattened product from its target-layout memory representation.
    /// This is deliberately structural rather than a fixed leaf stride: MIR
    /// packs `bool`/`u8` arrays, while wider scalar arrays and nested records use
    /// their target-derived field offsets and element strides.
    fn load_materialized_leaves(
        &mut self,
        pointer: ValueId,
        class: &RuntimeClass<'db>,
        shape: &FlatShape,
        values: &mut Vec<ValueId>,
    ) -> Result<(), LowerError> {
        match (class, shape) {
            (RuntimeClass::Scalar(_), FlatShape::Leaf(ty)) => {
                values.push(self.load_memory_scalar(pointer, *ty));
                Ok(())
            }
            (RuntimeClass::AggregateValue { layout }, FlatShape::Leaf(Type::I32))
                if self.module.fieldless_enum_tag(*layout).is_some() =>
            {
                let tag = self.module.fieldless_enum_tag(*layout).unwrap();
                let compact_ty = scalar_ty_r1(&tag)?;
                let compact = self.load_memory_scalar(pointer, compact_ty);
                let canonical = if compact_ty == Type::I32 {
                    compact
                } else {
                    self.fb
                        .insert_inst(Zext::new(self.inst_set(), compact, Type::I32), Type::I32)
                };
                values.push(canonical);
                Ok(())
            }
            (RuntimeClass::AggregateValue { layout }, FlatShape::Product(fields)) => {
                match layout.data(self.module.db) {
                    Layout::Struct(struct_layout) => {
                        if struct_layout.fields.len() != fields.len() {
                            return Err(LowerError::Internal(
                                "materialized struct shape/layout arity mismatch".to_owned(),
                            ));
                        }
                        for (index, (field, field_shape)) in
                            struct_layout.fields.iter().zip(fields).enumerate()
                        {
                            let offset = mir::struct_field_offset_bytes(
                                self.module.db,
                                *layout,
                                hir::analysis::semantic::FieldIndex(index as u16),
                                crate::WASM_LAYOUT,
                            );
                            let address = self.offset_addr(pointer, offset)?;
                            self.load_materialized_leaves(address, field, field_shape, values)?;
                        }
                        Ok(())
                    }
                    Layout::Array(array_layout) => {
                        let len = usize::try_from(array_layout.len).map_err(|_| {
                            LowerError::Unsupported("wasm array length exceeds usize".to_owned())
                        })?;
                        if len != fields.len() {
                            return Err(LowerError::Internal(
                                "materialized array shape/layout arity mismatch".to_owned(),
                            ));
                        }
                        let stride =
                            mir::array_elem_size_bytes(self.module.db, *layout, crate::WASM_LAYOUT);
                        for (index, field_shape) in fields.iter().enumerate() {
                            let offset = index.checked_mul(stride).ok_or_else(|| {
                                LowerError::Unsupported(
                                    "wasm materialized array offset overflow".to_owned(),
                                )
                            })?;
                            let address = self.offset_addr(pointer, offset)?;
                            self.load_materialized_leaves(
                                address,
                                &array_layout.elem,
                                field_shape,
                                values,
                            )?;
                        }
                        Ok(())
                    }
                    Layout::Enum(_) => Err(LowerError::Unsupported(
                        "wasm target: payload-enum aggregate loads are not implemented".to_owned(),
                    )),
                }
            }
            _ => Err(LowerError::Internal(format!(
                "materialized aggregate class/shape mismatch: {class:?} / {shape:?}"
            ))),
        }
    }

    fn lower_materialize_to_object(&mut self, src: RLocalId) -> Result<ValueId, LowerError> {
        let class = self.body.value_class(src).cloned().ok_or_else(|| {
            LowerError::Internal(format!(
                "materialization source {src:?} has no runtime class"
            ))
        })?;
        let RuntimeClass::AggregateValue { layout } = class else {
            return Err(LowerError::Unsupported(format!(
                "wasm target: materialization source {src:?} is not an aggregate value"
            )));
        };
        if !self.module.aggregate_is_memory_lowerable(&class) {
            return Err(LowerError::Unsupported(format!(
                "wasm target: materialization source {src:?} has non-scalar memory leaves"
            )));
        }
        if self.is_address_carried_aggregate_value(src) {
            // The prologue has already reconstructed this by-value aggregate
            // parameter in canonical-arena storage so dynamic indexing and
            // mutation can address it. A later `MaterializeToObject` still
            // denotes a fresh Fe value. Copy that existing object instead of
            // trying to flatten its one pointer as though it were every leaf.
            let source = self.local_value(src)?;
            return self.lower_deep_object_copy(source, layout);
        }
        let shape = self.module.flat_shape(&class).ok_or_else(|| {
            LowerError::Unsupported(format!(
                "wasm target: materialization source {src:?} cannot be flattened"
            ))
        })?;
        let leaves = self.local_flat_values(src)?;
        if leaves.len() != shape.leaf_count() {
            return Err(LowerError::Internal(format!(
                "materialization source {src:?} exposed {} leaves for shape {shape:?}",
                leaves.len()
            )));
        }
        let pointer = self.lower_alloc_object(layout)?;
        let mut cursor = 0usize;
        self.store_materialized_leaves(pointer, &class, &leaves, &mut cursor)?;
        if cursor != leaves.len() {
            return Err(LowerError::Internal(format!(
                "materialization source {src:?} consumed {cursor} of {} leaves",
                leaves.len()
            )));
        }
        Ok(pointer)
    }

    fn lower_materialize_place_to_object(
        &mut self,
        place: &RuntimePlace<'db>,
        dst: RLocalId,
    ) -> Result<ValueId, LowerError> {
        let dst_class = self.body.value_class(dst).cloned().ok_or_else(|| {
            LowerError::Internal(format!("materialization destination {dst:?} has no class"))
        })?;
        let layout = self.module.object_value_layout(&dst_class).ok_or_else(|| {
            LowerError::Internal(format!(
                "materialization destination {dst:?} is not an object value"
            ))
        })?;

        if !place.path.is_empty() {
            let Some((source_pointer, source_class)) = self.raw_memory_aggregate_place(place)?
            else {
                return Err(LowerError::Unsupported(
                    "wasm target: projected aggregate materialization requires canonical-arena storage"
                        .to_owned(),
                ));
            };
            if source_class.aggregate_layout() != Some(layout) {
                return Err(LowerError::Unsupported(
                    "wasm target: projected aggregate materialization source/destination layouts differ"
                        .to_owned(),
                ));
            }
            return self.lower_deep_object_copy(source_pointer, layout);
        }

        let source = match place.root {
            PlaceRoot::Ref(source) => source,
            _ => {
                return Err(LowerError::Unsupported(
                    "wasm target: aggregate-place materialization requires an object root"
                        .to_owned(),
                ));
            }
        };
        let source_class = self.body.value_class(source).cloned().ok_or_else(|| {
            LowerError::Internal(format!("materialization source {source:?} has no class"))
        })?;
        if self.module.object_value_layout(&source_class) != Some(layout) {
            return Err(LowerError::Unsupported(
                "wasm target: aggregate-place materialization source/destination layouts differ"
                    .to_owned(),
            ));
        }
        let source_pointer = self.local_value(source)?;
        let destination = self.lower_alloc_object(layout)?;
        let size = mir::layout_size_bytes(self.module.db, layout, crate::WASM_LAYOUT);
        for offset in 0..size {
            let src = self.offset_addr(source_pointer, offset)?;
            let dst = self.offset_addr(destination, offset)?;
            let byte = self
                .fb
                .insert_inst(Mload::new(self.inst_set(), src, Type::I8), Type::I8);
            self.fb
                .insert_inst_no_result(Mstore::new(self.inst_set(), dst, byte, Type::I8));
        }
        Ok(destination)
    }

    /// R2.0: lower a place READ that is an identity on an already value-carried
    /// transport word. Admits a `Ref`-rooted place whose carrier is a memory-space
    /// provider reference with a `ty_for_class`-admissible pointee, at the empty
    /// path (the whole transport word) or `[Field(0)]` on a single-scalar-field
    /// newtype (the field IS the word). This is the wasm image of the EVM lowerer's
    /// `place_terminal_for_carrier` -> `ObjLoad` (a value-model read, no memory):
    /// the identity `use_var` on the carrier local. Everything else (Slot/Provider/
    /// Ptr roots, object/const ref carriers, non-memory provider spaces, deeper or
    /// other paths, multi-field pointees, and all real linear memory) stays
    /// fail-closed as R2. See the ladder doc section 7.2 for the exact boundary.
    /// Load one Fe scalar from linear memory. Wasm narrow loads occupy an i32
    /// register even when the logical Fe carrier is i1/i8/i16, so truncate the
    /// register before binding it to a narrow SSA local.
    fn load_memory_scalar(&mut self, address: ValueId, ty: Type) -> ValueId {
        let register_ty = match ty {
            Type::I1 | Type::I8 | Type::I16 => Type::I32,
            _ => ty,
        };
        let loaded = self
            .fb
            .insert_inst(Mload::new(self.inst_set(), address, ty), register_ty);
        if register_ty == ty {
            loaded
        } else {
            self.fb
                .insert_inst(Trunc::new(self.inst_set(), loaded, ty), ty)
        }
    }

    fn lower_place_read(&mut self, place: &RuntimePlace<'db>) -> Result<ValueId, LowerError> {
        if let Some((addr, ty)) = self.raw_memory_scalar_place(place)? {
            return Ok(self.load_memory_scalar(addr, ty));
        }
        if let (PlaceRoot::Slot(local), []) = (&place.root, &*place.path) {
            if matches!(self.body.value_class(*local), Some(RuntimeClass::Scalar(_))) {
                return self.local_value(*local);
            }
            return Err(unsupported_place(place));
        }
        let PlaceRoot::Ref(v) = &place.root else {
            return Err(unsupported_place(place));
        };
        let v = *v;
        let class = self.body.value_class(v).ok_or_else(|| {
            LowerError::Internal(format!("R2.0 place root {v:?} carries no value class"))
        })?;
        let RuntimeClass::Ref {
            pointee,
            kind:
                RefKind::Provider {
                    space: AddressSpaceKind::Memory,
                    ..
                },
            ..
        } = class
        else {
            return Err(unsupported_place(place));
        };
        // Reuse the `ty_for_class` admissibility SSOT; do not duplicate the envelope.
        self.module.ty_for_class(pointee)?;
        match &*place.path {
            // Empty path: the load is the whole transport word.
            [] => self.local_value(v),
            // `[Field(0)]` on a single-scalar-field newtype: the field IS the word.
            [PlaceElem::Field(idx)]
                if idx.0 == 0
                    && pointee
                        .aggregate_layout()
                        .and_then(|layout| self.module.single_scalar_field(layout))
                        .is_some() =>
            {
                self.local_value(v)
            }
            _ => Err(unsupported_place(place)),
        }
    }

    /// Resolve an aggregate-valued projection backed by the canonical arena.
    /// This is the aggregate counterpart of `raw_memory_scalar_place`: nested
    /// record reads retain value semantics by loading their flattened leaves,
    /// rather than passing an aliasing pointer through the Wasm value lane.
    fn raw_memory_aggregate_place(
        &mut self,
        place: &RuntimePlace<'db>,
    ) -> Result<Option<(ValueId, RuntimeClass<'db>)>, LowerError> {
        let program = self.module.db as &dyn mir::MirDb;
        let resolved = mir::resolve_runtime_place(self.module.db, &program, &self.body, place)
            .map_err(|error| LowerError::Internal(format!("invalid runtime place: {error:?}")))?;
        let RuntimeClass::AggregateValue { .. } = resolved.result_class.clone() else {
            return Ok(None);
        };
        // Dynamic indexing requires a typed aggregate extent. Local objects,
        // materialized parameters, and raw providers that retain a concrete
        // target layout all carry that authority. Untyped raw addresses retain
        // the prior constant-projection envelope.
        let (addr_local, mut current_class, allow_dynamic_index) = match resolved.root_kind {
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if matches!(
                    self.body.value_class(value),
                    Some(RuntimeClass::Ref {
                        kind: RefKind::Object,
                        ..
                    })
                ) =>
            {
                (value, class, true)
            }
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if self.body.value_class(value).is_some_and(|class| {
                    matches!(
                        class,
                        RuntimeClass::Ref {
                            kind: RefKind::Provider {
                                space: AddressSpaceKind::Memory,
                                ..
                            },
                            pointee,
                            ..
                        } if matches!(pointee.as_ref(), RuntimeClass::AggregateValue { .. })
                    )
                }) =>
            {
                (value, class, true)
            }
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if self.body.value_class(value).is_some_and(|class| {
                    self.module.memory_lowerable_ref_layout(class).is_some()
                }) =>
            {
                // Object and memory-provider refs matched above. The remaining
                // admitted class is a read-only const borrow inside the private
                // scoped call graph, so checked indexing through its arena
                // pointer is sound while stores remain forbidden.
                (value, class, true)
            }
            mir::ResolvedPlaceRootKind::Slot { local, class }
                if self.is_address_carried_aggregate_value(local) =>
            {
                (local, class, true)
            }
            mir::ResolvedPlaceRootKind::Provider {
                value,
                provider_class:
                    RuntimeClass::RawAddr {
                        space: AddressSpaceKind::Memory,
                        target,
                    },
                class,
                ..
            } => (value, class, target.is_some()),
            mir::ResolvedPlaceRootKind::Ptr {
                addr,
                space: AddressSpaceKind::Memory,
                class,
            } => (addr, class, true),
            _ => return Ok(None),
        };
        let mut addr = self.local_value(addr_local)?;
        let mut byte_offset = 0usize;
        for elem in resolved.path {
            match elem {
                mir::ResolvedPlaceElem::Field { field, class } => {
                    let RuntimeClass::AggregateValue { layout } = current_class else {
                        return Err(LowerError::Internal(
                            "resolved aggregate field base is not a struct".to_owned(),
                        ));
                    };
                    byte_offset = byte_offset
                        .checked_add(mir::struct_field_offset_bytes(
                            self.module.db,
                            layout,
                            field,
                            crate::WASM_LAYOUT,
                        ))
                        .ok_or_else(|| {
                            LowerError::Unsupported(
                                "wasm aggregate field byte offset overflow".to_owned(),
                            )
                        })?;
                    current_class = class;
                }
                mir::ResolvedPlaceElem::Index { index, class } => {
                    let RuntimeClass::AggregateValue { layout } = current_class else {
                        return Err(LowerError::Internal(
                            "resolved aggregate index base is not an array".to_owned(),
                        ));
                    };
                    let Layout::Array(array) = layout.data(self.module.db) else {
                        return Err(LowerError::Internal(
                            "resolved aggregate index layout is not an array".to_owned(),
                        ));
                    };
                    let stride =
                        mir::array_elem_size_bytes(self.module.db, layout, crate::WASM_LAYOUT);
                    match index {
                        IndexSource::Constant(index) => {
                            if (index as u64) >= array.len {
                                return Err(LowerError::Unsupported(format!(
                                    "wasm aggregate constant index {index} is out of bounds for length {}",
                                    array.len
                                )));
                            }
                            byte_offset = byte_offset
                                .checked_add(index.checked_mul(stride).ok_or_else(|| {
                                    LowerError::Unsupported(
                                        "wasm aggregate element byte offset overflow".to_owned(),
                                    )
                                })?)
                                .ok_or_else(|| {
                                    LowerError::Unsupported(
                                        "wasm aggregate element byte offset overflow".to_owned(),
                                    )
                                })?;
                        }
                        IndexSource::Dynamic(index_local) if allow_dynamic_index => {
                            addr = self.offset_addr(addr, byte_offset)?;
                            byte_offset = 0;
                            let is = self.inst_set();
                            let index = self.local_value(index_local)?;
                            let len = i32::try_from(array.len).map_err(|_| {
                                LowerError::Unsupported(format!(
                                    "wasm aggregate array length {} exceeds i32",
                                    array.len
                                ))
                            })?;
                            let len = self.fb.make_imm_value(Immediate::I32(len));
                            let in_bounds = self.fb.insert_inst(Lt::new(is, index, len), Type::I1);
                            let trap = self.trap_block();
                            let ok = self.fb.append_block();
                            self.fb
                                .insert_inst_no_result(Br::new(is, in_bounds, ok, trap));
                            self.fb.switch_to_block(ok);
                            let stride = i32::try_from(stride).map_err(|_| {
                                LowerError::Unsupported(format!(
                                    "wasm aggregate array element stride {stride} exceeds i32"
                                ))
                            })?;
                            let stride = self.fb.make_imm_value(Immediate::I32(stride));
                            let scaled =
                                self.fb.insert_inst(Mul::new(is, index, stride), Type::I32);
                            addr = self.fb.insert_inst(Add::new(is, addr, scaled), Type::I32);
                        }
                        IndexSource::Dynamic(_) => {
                            return Err(LowerError::Unsupported(
                                "wasm aggregate dynamic index requires function-local or materialized aggregate storage"
                                    .to_owned(),
                            ));
                        }
                    }
                    current_class = class;
                }
                other => {
                    return Err(LowerError::Unsupported(format!(
                        "wasm aggregate place projection `{other:?}` is not supported"
                    )));
                }
            }
        }
        addr = self.offset_addr(addr, byte_offset)?;
        Ok(Some((addr, resolved.result_class)))
    }

    /// Resolve a Wasm linear-memory scalar place behind a memory address: a
    /// memory `RawAddr` / memory-provider root or a function-local object-ref
    /// root. Addresses are i32 byte offsets on wasm32, not Sonatina compound
    /// pointers, so field/index arithmetic uses ordinary i32 Add/Mul rather than
    /// `Gep`. Offsets and array strides come exclusively from MIR's target-layout
    /// SSOT. A dynamic array index emits an `idx < len` bounds check that traps
    /// (`Unreachable`) on failure. Untyped raw regions, variants, and
    /// dereferences remain fail-closed.
    fn raw_memory_scalar_place(
        &mut self,
        place: &RuntimePlace<'db>,
    ) -> Result<Option<(ValueId, Type)>, LowerError> {
        let program = self.module.db as &dyn mir::MirDb;
        let resolved = mir::resolve_runtime_place(self.module.db, &program, &self.body, place)
            .map_err(|error| LowerError::Internal(format!("invalid runtime place: {error:?}")))?;
        let scalar = match resolved.result_class.clone() {
            RuntimeClass::Scalar(scalar) => Some(scalar),
            RuntimeClass::AggregateValue { layout } => {
                let Some(scalar) = self.module.single_scalar_field(layout) else {
                    return Ok(None);
                };
                Some(scalar)
            }
            // `BrowserPtr<T>` fields are classified as typed memory addresses,
            // not ordinary scalar newtypes. They still occupy exactly one i32
            // word in the authoritative Wasm layout. Their arena provenance is
            // checked after resolving the root below, before either a load or a
            // store can expose that word.
            RuntimeClass::RawAddr {
                space: AddressSpaceKind::Memory,
                ..
            } => None,
            RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => return Ok(None),
        };
        // `allow_index` requires a target-derived aggregate extent. This is
        // carried by object refs, materialized parameters, and typed raw
        // providers. A raw address with no target layout stays field-only.
        let (addr_local, mut current_class, allow_index) = match resolved.root_kind {
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if matches!(
                    self.body.value_class(value),
                    Some(RuntimeClass::RawAddr {
                        space: AddressSpaceKind::Memory,
                        ..
                    })
                ) =>
            {
                (value, class, false)
            }
            // Change 3a: a `Ref{kind: Object}` root whose backing local carries
            // the function-local arena i32 pointer (minted by AllocObject and
            // declared by the Change 1 decl arm). `class` here is the
            // dereferenced aggregate pointee (`resolve_runtime_place` sets a
            // `Ref` root's class to the pointee), which the Field / Index path
            // walk then projects. Restricted to the object model deliberately:
            // element addressing through a memory-PROVIDER ref (an array param /
            // host region) is the deferred host-region dynamic-index case and
            // stays fail-closed in slice 1.
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if matches!(
                    self.body.value_class(value),
                    Some(RuntimeClass::Ref {
                        kind: RefKind::Object,
                        ..
                    })
                ) =>
            {
                (value, class, true)
            }
            // Flattened aggregate parameters are materialized into independent
            // arena storage in the prologue. Their provider-typed receiver is
            // therefore an actual addressable aggregate, not the one-word
            // provider identity used for scalar handles.
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if self.body.value_class(value).is_some_and(|class| {
                    matches!(
                        class,
                        RuntimeClass::Ref {
                            kind: RefKind::Provider {
                                space: AddressSpaceKind::Memory,
                                ..
                            },
                            pointee,
                            ..
                        } if matches!(pointee.as_ref(), RuntimeClass::AggregateValue { .. })
                    )
                }) =>
            {
                (value, class, true)
            }
            // A private read-only aggregate borrow retains one canonical-arena
            // pointer instead of flattening potentially thousands of leaves.
            // Dynamic reads are bounds checked below; write lowering does not
            // admit `RefKind::Const` destinations.
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if matches!(
                    self.body.value_class(value),
                    Some(RuntimeClass::Ref {
                        kind: RefKind::Const,
                        ..
                    })
                ) && self.body.value_class(value).is_some_and(|class| {
                    self.module.memory_lowerable_ref_layout(class).is_some()
                }) =>
            {
                (value, class, true)
            }
            mir::ResolvedPlaceRootKind::Slot { local, class }
                if self.is_address_carried_aggregate_value(local)
                    || self.materialized_scalar_slots.contains(&local) =>
            {
                (local, class, true)
            }
            mir::ResolvedPlaceRootKind::Provider {
                value,
                provider_class:
                    RuntimeClass::RawAddr {
                        space: AddressSpaceKind::Memory,
                        target,
                    },
                class,
                ..
            } => (value, class, target.is_some()),
            mir::ResolvedPlaceRootKind::Ptr {
                addr,
                space: AddressSpaceKind::Memory,
                class,
            } => (addr, class, true),
            _ => return Ok(None),
        };
        if scalar.is_none() && !self.module.arena_owned_local(&self.body, addr_local) {
            return Ok(None);
        }
        // The base pointer is materialized up front so a dynamic index can flush
        // the pending constant offset onto it mid-walk. A field-only path emits
        // no instructions in the loop, so this stays byte-identical to the prior
        // struct-field lowering (one trailing Add iff a nonzero offset remains).
        let mut addr = self.local_value(addr_local)?;
        let mut byte_offset = 0usize;
        for elem in resolved.path {
            match elem {
                mir::ResolvedPlaceElem::Field { field, class } => {
                    let RuntimeClass::AggregateValue { layout } = current_class else {
                        return Err(LowerError::Internal(
                            "resolved field projection base is not a struct".to_string(),
                        ));
                    };
                    byte_offset = byte_offset
                        .checked_add(mir::struct_field_offset_bytes(
                            self.module.db,
                            layout,
                            field,
                            crate::WASM_LAYOUT,
                        ))
                        .ok_or_else(|| {
                            LowerError::Unsupported(
                                "wasm memory scalar struct field byte offset overflow".to_string(),
                            )
                        })?;
                    current_class = class;
                }
                // Change 3b: array element addressing. Stride is MIR's SSOT
                // (`array_elem_size_bytes`: bool/u8 pack to 1, else word-rounded).
                // A constant index folds `k * stride` into the pending offset
                // (bounds-checked at compile time); a dynamic index flushes the
                // pending offset, bounds-checks `idx < len` to a lazy trap, then
                // adds `idx * stride`.
                mir::ResolvedPlaceElem::Index { index, class } if allow_index => {
                    let RuntimeClass::AggregateValue { layout } = current_class else {
                        return Err(LowerError::Internal(
                            "resolved index projection base is not an array".to_string(),
                        ));
                    };
                    let Layout::Array(array_layout) = layout.data(self.module.db) else {
                        return Err(LowerError::Internal(
                            "resolved index projection layout is not an array".to_string(),
                        ));
                    };
                    let len = array_layout.len;
                    let stride =
                        mir::array_elem_size_bytes(self.module.db, layout, crate::WASM_LAYOUT);
                    match index {
                        IndexSource::Constant(k) => {
                            if (k as u64) >= len {
                                return Err(LowerError::Unsupported(format!(
                                    "wasm array constant index {k} is out of bounds for length {len}"
                                )));
                            }
                            let elem_offset = k.checked_mul(stride).ok_or_else(|| {
                                LowerError::Unsupported(
                                    "wasm array element byte offset overflow".to_string(),
                                )
                            })?;
                            byte_offset =
                                byte_offset.checked_add(elem_offset).ok_or_else(|| {
                                    LowerError::Unsupported(
                                        "wasm array element byte offset overflow".to_string(),
                                    )
                                })?;
                        }
                        IndexSource::Dynamic(index_local) => {
                            addr = self.offset_addr(addr, byte_offset)?;
                            byte_offset = 0;
                            let is = self.inst_set();
                            let idx = self.local_value(index_local)?;
                            let len = i32::try_from(len).map_err(|_| {
                                LowerError::Unsupported(format!(
                                    "wasm array length {len} exceeds i32"
                                ))
                            })?;
                            let len_val = self.fb.make_imm_value(Immediate::I32(len));
                            // Unsigned `Lt`: a narrowed `usize` index is unsigned,
                            // so an out-of-range (or wrapped) value fails the check
                            // and traps.
                            let in_bounds =
                                self.fb.insert_inst(Lt::new(is, idx, len_val), Type::I1);
                            let trap = self.trap_block();
                            let ok = self.fb.append_block();
                            self.fb
                                .insert_inst_no_result(Br::new(is, in_bounds, ok, trap));
                            self.fb.switch_to_block(ok);
                            let stride = i32::try_from(stride).map_err(|_| {
                                LowerError::Unsupported(format!(
                                    "wasm array element stride {stride} exceeds i32"
                                ))
                            })?;
                            let stride_val = self.fb.make_imm_value(Immediate::I32(stride));
                            let scaled = self
                                .fb
                                .insert_inst(Mul::new(is, idx, stride_val), Type::I32);
                            addr = self.fb.insert_inst(Add::new(is, addr, scaled), Type::I32);
                        }
                    }
                    current_class = class;
                }
                other => {
                    return Err(LowerError::Unsupported(format!(
                        "wasm memory scalar place projection `{other:?}` is not supported; \
                         only struct fields and typed array indexes have \
                         target-layout byte-offset lowering"
                    )));
                }
            }
        }
        let addr = self.offset_addr(addr, byte_offset)?;
        let semantic_place_ty = semantic_place_result_ty(self.module.db, &self.body, place);
        let ty = if scalar.is_none() {
            Type::I32
        } else if semantic_place_ty.is_some_and(|ty| is_usize_semantic_ty(self.module.db, ty)) {
            Type::I32
        } else {
            let scalar = scalar.as_ref().expect("scalar result checked above");
            scalar_ty_r1(scalar).map_err(|error| match error {
                LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                    "{message}; while lowering memory place {place:?} with result class {scalar:?}"
                )),
                other => other,
            })?
        };
        Ok(Some((addr, ty)))
    }

    /// Add a constant byte offset to a linear-memory address, emitting the `Add`
    /// only when the offset is nonzero (so a zero-offset access stays a bare
    /// pointer, byte-identical to the prior lowering).
    fn offset_addr(&mut self, addr: ValueId, byte_offset: usize) -> Result<ValueId, LowerError> {
        if byte_offset == 0 {
            return Ok(addr);
        }
        let offset = i32::try_from(byte_offset).map_err(|_| {
            LowerError::Unsupported(format!(
                "wasm memory scalar place byte offset {byte_offset} exceeds i32"
            ))
        })?;
        let offset = self.fb.make_imm_value(Immediate::I32(offset));
        Ok(self
            .fb
            .insert_inst(Add::new(self.inst_set(), addr, offset), Type::I32))
    }

    /// The lazily-created per-function trap block: a single block whose sole
    /// instruction is `Unreachable` (a wasm trap). Every dynamic-index bounds
    /// check and every checked-`usize` overflow check branches here on failure.
    /// Created on first use and cached so functions with no such check emit no
    /// trap block; the builder's current block is restored so callers continue
    /// emitting into their own block.
    fn trap_block(&mut self) -> BlockId {
        if let Some(block) = self.trap_block {
            return block;
        }
        let is = self.inst_set();
        let resume = self.fb.current_block();
        let block = self.fb.append_block();
        self.fb.switch_to_block(block);
        self.fb.insert_inst_no_result(Unreachable::new(is));
        if let Some(resume) = resume {
            self.fb.switch_to_block(resume);
        }
        self.trap_block = Some(block);
        block
    }

    /// Branch to the shared trap block when `cond` (an `i1`) is set, continuing
    /// in a fresh block otherwise. Used by checked-`usize` overflow detection;
    /// the dynamic-index bounds check inlines the equivalent `Br` directly.
    fn trap_if(&mut self, cond: ValueId) {
        let is = self.inst_set();
        let trap = self.trap_block();
        let cont = self.fb.append_block();
        self.fb.insert_inst_no_result(Br::new(is, cond, trap, cont));
        self.fb.switch_to_block(cont);
    }

    /// Continue only when `tag < variant_count` in the enum's unsigned i32
    /// carrier. Negative/oversized host words therefore trap as well.
    fn validate_enum_tag(&mut self, tag: ValueId, variant_count: u32) {
        let is = self.inst_set();
        let limit = self.fb.make_imm_value(Immediate::I32(variant_count as i32));
        let valid = self.fb.insert_inst(Lt::new(is, tag, limit), Type::I1);
        let trap = self.trap_block();
        let cont = self.fb.append_block();
        self.fb
            .insert_inst_no_result(Br::new(is, valid, cont, trap));
        self.fb.switch_to_block(cont);
    }

    /// `AggregateMake` of a single-scalar-field newtype: the aggregate IS its one
    /// field's word, so construction is `Use` of that field. Multi-field (and
    /// empty) constructions fail closed.
    fn lower_scalar_newtype_make(
        &mut self,
        layout: LayoutId<'db>,
        fields: &[RLocalId],
    ) -> Result<ValueId, LowerError> {
        if self.module.single_scalar_field(layout).is_none() {
            return Err(LowerError::Unsupported(format!(
                "wasm target (R3.4b) aggregate construction of `{layout:?}` is not a \
                 single-scalar-field newtype (multi-field aggregates are R2)"
            )));
        }
        let [field] = fields else {
            return Err(LowerError::Internal(
                "single-scalar-field aggregate must have exactly one field".to_string(),
            ));
        };
        self.local_value(*field)
    }

    /// `AggregateExtract` of field 0 from a single-scalar-field newtype: the
    /// extracted word IS the aggregate's value. Any other field index fails closed
    /// (a single-scalar-field newtype has only field 0).
    fn lower_scalar_newtype_extract(
        &mut self,
        value: RLocalId,
        index: u32,
    ) -> Result<ValueId, LowerError> {
        if index != 0 {
            return Err(LowerError::Unsupported(format!(
                "wasm target (R3.4b) aggregate extract at index {index} is not supported \
                 (only field 0 of a single-scalar-field newtype)"
            )));
        }
        self.local_value(value)
    }

    /// Fe's primitive `+`/`-`/`*` lower to `IntrinsicArith` builtins (not
    /// `Binary`). R1 emits plain (unchecked) arithmetic and IGNORES the
    /// `checked` flag: Fe's checked-overflow semantics need real wasm overflow
    /// flags/traps, which are R2 (and the WAFFLE translator currently fakes the
    /// flag as 0), so R1 requires non-overflowing values. Every other builtin
    /// (memory, EVM host, addmod/mulmod, saturating, byte/sign-extend) fails
    /// closed.
    fn lower_builtin(
        &mut self,
        builtin: &RuntimeBuiltin<'db>,
        dst: RLocalId,
    ) -> Result<ValueId, LowerError> {
        match builtin {
            RuntimeBuiltin::IntTruncate { value, from, to } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                let target = scalar_ty_r1(to).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while lowering integer truncation into {dst:?} from {value:?} ({from:?} -> {to:?})"
                    )),
                    other => other,
                })?;
                let source = self.fb.type_of(value);
                if source == target {
                    return Ok(value);
                }
                let bits = |ty| match ty {
                    Type::I1 => Some(1),
                    Type::I8 => Some(8),
                    Type::I16 => Some(16),
                    Type::I32 => Some(32),
                    Type::I64 => Some(64),
                    _ => None,
                };
                let source_bits = bits(source).ok_or_else(|| {
                    LowerError::Unsupported("integer truncation requires an integer source".into())
                })?;
                let target_bits = bits(target).ok_or_else(|| {
                    LowerError::Unsupported("integer truncation requires an integer target".into())
                })?;
                if source_bits > target_bits {
                    Ok(self.fb.insert_inst(Trunc::new(is, value, target), target))
                } else {
                    let signed = matches!(from.repr, ScalarRepr::Int { signed: true, .. });
                    if signed {
                        Ok(self.fb.insert_inst(Sext::new(is, value, target), target))
                    } else {
                        Ok(self.fb.insert_inst(Zext::new(is, value, target), target))
                    }
                }
            }
            RuntimeBuiltin::IntrinsicArith {
                op,
                lhs,
                rhs,
                class,
                checked,
            } => self.lower_intrinsic_arith(*op, *lhs, *rhs, class, *checked, dst),
            RuntimeBuiltin::F32FromI32 { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(I32ToF32::new(is, value), Type::F32))
            }
            RuntimeBuiltin::I32FromF32 { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(F32ToI32::new(is, value), Type::I32))
            }
            RuntimeBuiltin::F32Sqrt { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(Fsqrt::new(is, value), Type::F32))
            }
            RuntimeBuiltin::F32Abs { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(Fabs::new(is, value), Type::F32))
            }
            RuntimeBuiltin::F32Min { lhs, rhs } => {
                let is = self.inst_set();
                let lhs = self.local_value(*lhs)?;
                let rhs = self.local_value(*rhs)?;
                Ok(self.fb.insert_inst(Fmin::new(is, lhs, rhs), Type::F32))
            }
            RuntimeBuiltin::F32Max { lhs, rhs } => {
                let is = self.inst_set();
                let lhs = self.local_value(*lhs)?;
                let rhs = self.local_value(*rhs)?;
                Ok(self.fb.insert_inst(Fmax::new(is, lhs, rhs), Type::F32))
            }
            RuntimeBuiltin::F32MinRelaxed { lhs, rhs } => {
                let is = self.inst_set();
                let lhs = self.local_value(*lhs)?;
                let rhs = self.local_value(*rhs)?;
                Ok(self
                    .fb
                    .insert_inst(FminRelaxed::new(is, lhs, rhs), Type::F32))
            }
            RuntimeBuiltin::F32MaxRelaxed { lhs, rhs } => {
                let is = self.inst_set();
                let lhs = self.local_value(*lhs)?;
                let rhs = self.local_value(*rhs)?;
                Ok(self
                    .fb
                    .insert_inst(FmaxRelaxed::new(is, lhs, rhs), Type::F32))
            }
            RuntimeBuiltin::F32Clamp { value, lo, hi } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                let lo = self.local_value(*lo)?;
                let hi = self.local_value(*hi)?;
                Ok(self
                    .fb
                    .insert_inst(Fclamp::new(is, value, lo, hi), Type::F32))
            }
            RuntimeBuiltin::F32Floor { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(Ffloor::new(is, value), Type::F32))
            }
            RuntimeBuiltin::F32Ceil { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(Fceil::new(is, value), Type::F32))
            }
            RuntimeBuiltin::F32Trunc { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(Ftrunc::new(is, value), Type::F32))
            }
            RuntimeBuiltin::F32Round { value } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                Ok(self.fb.insert_inst(Fround::new(is, value), Type::F32))
            }
            RuntimeBuiltin::Malloc { size } => {
                let is = self.inst_set();
                let size = self.local_value(*size)?;
                if self.fb.type_of(size) != Type::I32 {
                    return Err(LowerError::Unsupported(
                        "wasm malloc requires an i32 byte size; wide sizes need an explicit \
                         checked conversion"
                            .to_string(),
                    ));
                }
                Ok(self
                    .fb
                    .insert_inst(MemAllocDynamic::new(is, size), Type::I32))
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) builtin `{other:?}` is not supported \
                 (memory/EVM-host/addmod/saturating builtins are R2)"
            ))),
        }
    }

    fn lower_intrinsic_arith(
        &mut self,
        op: IntrinsicArithBinOp,
        lhs: RLocalId,
        rhs: RLocalId,
        class: &ScalarClass<'db>,
        checked: bool,
        dst: RLocalId,
    ) -> Result<ValueId, LowerError> {
        if matches!(class.repr, ScalarRepr::Float { .. }) {
            let is = self.inst_set();
            let lhs = self.local_value(lhs)?;
            let rhs = self.local_value(rhs)?;
            return Ok(match op {
                IntrinsicArithBinOp::Add => self.fb.insert_inst(Fadd::new(is, lhs, rhs), Type::F32),
                IntrinsicArithBinOp::Sub => self.fb.insert_inst(Fsub::new(is, lhs, rhs), Type::F32),
                IntrinsicArithBinOp::Mul => self.fb.insert_inst(Fmul::new(is, lhs, rhs), Type::F32),
                IntrinsicArithBinOp::Div => self.fb.insert_inst(Fdiv::new(is, lhs, rhs), Type::F32),
                other => {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: f32 intrinsic arithmetic `{other:?}` is not supported"
                    )));
                }
            });
        }
        let ty = scalar_ty_r1(class).map_err(|error| match error {
            LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                "{message}; while lowering intrinsic arithmetic {op:?} into {dst:?} from {lhs:?}, {rhs:?} with class {class:?}"
            )),
            other => other,
        })?;
        // CRITICAL bounds-safety: checked arithmetic on a narrowed `usize` (the
        // wasm32 pointer width) MUST trap on overflow. R1 otherwise ignores the
        // `checked` flag and emits a wrapping op, which would turn a semantically
        // out-of-range index (`usize::MAX + 1`) into a small in-bounds one and
        // slip past the array bounds check. Scoped strictly to the narrowed usize
        // path (keyed on semantic `Usize` + i32 width): every other scalar keeps
        // R1's wrapping behavior, so no existing kernel changes.
        if checked
            && ty == Type::I32
            && is_usize_semantic_ty(
                self.module.db,
                self.body.locals[dst.as_u32() as usize].semantic_ty,
            )
        {
            let lhs = self.local_value(lhs)?;
            let rhs = self.local_value(rhs)?;
            return self.lower_checked_usize_arith(op, lhs, rhs);
        }
        let is = self.inst_set();
        let lhs = self.local_value(lhs)?;
        let rhs = self.local_value(rhs)?;
        Ok(match op {
            IntrinsicArithBinOp::Add => self.fb.insert_inst(Add::new(is, lhs, rhs), ty),
            IntrinsicArithBinOp::Sub => self.fb.insert_inst(Sub::new(is, lhs, rhs), ty),
            IntrinsicArithBinOp::Mul => self.fb.insert_inst(Mul::new(is, lhs, rhs), ty),
            IntrinsicArithBinOp::Div if !class.is_signed_int() => {
                self.fb.insert_inst(Udiv::new(is, lhs, rhs), ty)
            }
            IntrinsicArithBinOp::Rem if !class.is_signed_int() => {
                self.fb.insert_inst(Umod::new(is, lhs, rhs), ty)
            }
            other => {
                return Err(LowerError::Unsupported(format!(
                    "wasm target (R1) intrinsic arithmetic `{other:?}` is not supported \
                     (signed div/rem and pow are R2)"
                )));
            }
        })
    }

    /// Checked unsigned 32-bit (`usize` on wasm32) arithmetic: compute the result
    /// and trap (`Unreachable`) on overflow, matching Fe's checked-overflow panic.
    /// `Add`/`Sub` detect wrap with an unsigned compare; `Mul` widens to i64,
    /// multiplies, and traps when the product exceeds `u32::MAX`. WebAssembly's
    /// unsigned division and remainder instructions provide the required
    /// divide-by-zero trap directly; unsigned division has no overflow case.
    fn lower_checked_usize_arith(
        &mut self,
        op: IntrinsicArithBinOp,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, LowerError> {
        let is = self.inst_set();
        match op {
            IntrinsicArithBinOp::Add => {
                let sum = self.fb.insert_inst(Add::new(is, lhs, rhs), Type::I32);
                // Unsigned overflow iff the wrapped sum is below an addend.
                let overflow = self.fb.insert_inst(Lt::new(is, sum, lhs), Type::I1);
                self.trap_if(overflow);
                Ok(sum)
            }
            IntrinsicArithBinOp::Sub => {
                // Unsigned underflow iff lhs < rhs.
                let underflow = self.fb.insert_inst(Lt::new(is, lhs, rhs), Type::I1);
                self.trap_if(underflow);
                Ok(self.fb.insert_inst(Sub::new(is, lhs, rhs), Type::I32))
            }
            IntrinsicArithBinOp::Mul => {
                let lhs64 = self
                    .fb
                    .insert_inst(Zext::new(is, lhs, Type::I64), Type::I64);
                let rhs64 = self
                    .fb
                    .insert_inst(Zext::new(is, rhs, Type::I64), Type::I64);
                let product = self.fb.insert_inst(Mul::new(is, lhs64, rhs64), Type::I64);
                let limit = self.fb.make_imm_value(Immediate::I64(u32::MAX as i64));
                // Overflow iff the 64-bit product exceeds u32::MAX (unsigned `>`).
                let overflow = self.fb.insert_inst(Lt::new(is, limit, product), Type::I1);
                self.trap_if(overflow);
                Ok(self
                    .fb
                    .insert_inst(Trunc::new(is, product, Type::I32), Type::I32))
            }
            IntrinsicArithBinOp::Div => Ok(self.fb.insert_inst(Udiv::new(is, lhs, rhs), Type::I32)),
            IntrinsicArithBinOp::Rem => Ok(self.fb.insert_inst(Umod::new(is, lhs, rhs), Type::I32)),
            other => Err(LowerError::Unsupported(format!(
                "wasm target: checked usize intrinsic arithmetic `{other:?}` is not supported \
                 (pow is R2)"
            ))),
        }
    }

    /// Signedness of a binary op's operands, from the MIR value class (the EVM
    /// path's `lower_arith`/`lower_comp` precedent). Sonatina types are signless,
    /// so this is the ONLY place the distinction exists on the wasm path; every
    /// signedness-sensitive op must key on it, never on the sonatina type.
    fn operand_signedness(&self, lhs: RLocalId, rhs: RLocalId) -> Result<bool, LowerError> {
        self.body
            .value_class(lhs)
            .or_else(|| self.body.value_class(rhs))
            .map(RuntimeClass::is_signed_scalar)
            .ok_or_else(|| {
                LowerError::Internal(
                    "binary op with no classed operand (cannot determine signedness)".to_string(),
                )
            })
    }

    fn lower_binary(
        &mut self,
        op: BinOp,
        lhs: RLocalId,
        rhs: RLocalId,
        dst: RLocalId,
    ) -> Result<ValueId, LowerError> {
        let float_operands = [lhs, rhs].into_iter().any(|local| {
            matches!(
                self.body.value_class(local),
                Some(RuntimeClass::Scalar(ScalarClass {
                    repr: ScalarRepr::Float { .. },
                    ..
                }))
            )
        });
        // Keep the MIR operand ids for the signedness key (the value class lives on
        // the RLocalId, not the sonatina ValueId, which is signless). The sonatina
        // ValueIds shadow below for the instruction constructors.
        let (lhs_local, rhs_local) = (lhs, rhs);
        let (lhs_ty, rhs_ty) = (self.local_ty(lhs_local)?, self.local_ty(rhs_local)?);
        let lhs = self.local_value(lhs)?;
        let rhs = self.local_value(rhs)?;
        if float_operands {
            let is = self.inst_set();
            let BinOp::Comp(comp) = op else {
                return Err(LowerError::Unsupported(format!(
                    "wasm target: f32 binary op `{op:?}` is not supported; arithmetic must lower through IntrinsicArith"
                )));
            };
            return Ok(match comp {
                CompBinOp::Eq => self.fb.insert_inst(Feq::new(is, lhs, rhs), Type::I1),
                CompBinOp::NotEq => {
                    let equal = self.fb.insert_inst(Feq::new(is, lhs, rhs), Type::I1);
                    let false_value = self.fb.make_imm_value(Immediate::from(false));
                    self.fb
                        .insert_inst(CmpEq::new(is, equal, false_value), Type::I1)
                }
                CompBinOp::Lt => self.fb.insert_inst(Flt::new(is, lhs, rhs), Type::I1),
                CompBinOp::LtEq => self.fb.insert_inst(Fle::new(is, lhs, rhs), Type::I1),
                // Reversing operands preserves unordered/NaN behavior: both Flt
                // and Fle remain false if either operand is NaN.
                CompBinOp::Gt => self.fb.insert_inst(Flt::new(is, rhs, lhs), Type::I1),
                CompBinOp::GtEq => self.fb.insert_inst(Fle::new(is, rhs, lhs), Type::I1),
            });
        }
        match op {
            // R1 emits plain (unchecked) arithmetic. Fe's checked-overflow
            // semantics lower to an EVM revert on the EVM path; the wasm
            // equivalent (real overflow flags / traps) is R2, so R1 requires
            // non-overflowing values.
            BinOp::Arith(arith) => {
                let ty = self.local_ty(dst)?;
                // WebAssembly has only i32/i64 integer registers. Narrow
                // Sonatina values are consequently represented by i32 at the
                // WAFFLE boundary, where a u8 immediate such as 128 can arrive
                // sign-extended while a memory load arrives zero-extended.
                // Canonicalize both narrow operands explicitly before any
                // operation, then truncate value results back to Fe's logical
                // width. This makes `u8 & 192 == 128` and the rest of the
                // narrow integer matrix independent of incidental register
                // extension.
                let signed_shift = matches!(arith, ArithBinOp::RShift)
                    && self.operand_signedness(lhs_local, rhs_local)?;
                let lhs = self.promote_narrow_int(lhs, lhs_ty, signed_shift);
                let rhs = self.promote_narrow_int(rhs, rhs_ty, false);
                let op_ty = match ty {
                    Type::I8 | Type::I16 => Type::I32,
                    _ => ty,
                };
                let is = self.inst_set();
                let result = match arith {
                    ArithBinOp::Add => self.fb.insert_inst(Add::new(is, lhs, rhs), op_ty),
                    ArithBinOp::Sub => self.fb.insert_inst(Sub::new(is, lhs, rhs), op_ty),
                    ArithBinOp::Mul => self.fb.insert_inst(Mul::new(is, lhs, rhs), op_ty),
                    ArithBinOp::RShift => {
                        // Sonatina's shift constructor order is (bits, value), the
                        // EVM path's convention (lower_runtime.rs:4101-4109). Signed
                        // `>>` is Sar (arithmetic); unsigned `>>` is Shr (logical).
                        // Both arms are live now that fork push #3 opened the
                        // matching u32 `Shr` arm in the SPIR-V emitter and the
                        // type-keyed `Shr` (-> I32ShrU) in the sonatina wasm
                        // translator, so neither is a silent-skip path; the u32
                        // color-ramp shift (`(i * 655) >> 8`, the M4 coloring) flows
                        // end to end on both backends.
                        if signed_shift {
                            self.fb.insert_inst(Sar::new(is, rhs, lhs), op_ty)
                        } else {
                            self.fb.insert_inst(Shr::new(is, rhs, lhs), op_ty)
                        }
                    }
                    // Left shift is bit-identical for signed and unsigned, so no
                    // signedness branch. Shift constructor order is (bits, value)
                    // like Sar/Shr (EVM precedent lower_runtime.rs:4128).
                    ArithBinOp::LShift => self.fb.insert_inst(Shl::new(is, rhs, lhs), op_ty),
                    // Bitwise: direct operand order. The sonatina fork's SPIR-V
                    // emitter maps And/Or/Xor as of e423231f + 43e9f3b0 (the R2
                    // bitwise re-pin), matching the wasm translator leg. This is
                    // exactly blake3's op set (XOR + shifts + wrapping Add), so a
                    // blake3 const fn lowers on the runtime legs, not just CTFE.
                    ArithBinOp::BitAnd => self.fb.insert_inst(And::new(is, lhs, rhs), op_ty),
                    ArithBinOp::BitOr => self.fb.insert_inst(Or::new(is, lhs, rhs), op_ty),
                    ArithBinOp::BitXor => self.fb.insert_inst(Xor::new(is, lhs, rhs), op_ty),
                    other => {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target (R1) arithmetic op `{other:?}` is not supported \
                             (div/rem/pow are R2)"
                        )));
                    }
                };
                Ok(self.restore_narrow_int(result, ty, op_ty))
            }
            BinOp::Comp(comp) => {
                // Sign-aware (M2): the whole matrix derives from a signed/unsigned
                // less-than. Signedness comes from the operand CLASS, not the
                // sonatina type (signless). The key is symmetric in the pair, so the
                // `>`/`>=`/`<=` operand swaps below reuse it unchanged.
                let signed = self.operand_signedness(lhs_local, rhs_local)?;
                // Equality compares raw bit patterns; ordered comparisons use
                // the semantic signedness carried by RuntimeClass.
                let ordered = !matches!(comp, CompBinOp::Eq | CompBinOp::NotEq);
                let lhs = self.promote_narrow_int(lhs, lhs_ty, signed && ordered);
                let rhs = self.promote_narrow_int(rhs, rhs_ty, signed && ordered);
                let is = self.inst_set();
                Ok(match comp {
                    // i32 -> Slt, u32 -> Lt.
                    CompBinOp::Lt => self.int_lt(lhs, rhs, signed),
                    CompBinOp::Eq => self.fb.insert_inst(CmpEq::new(is, lhs, rhs), Type::I1),
                    // `a > b` is `b < a`: swap the operands. The signedness key is
                    // symmetric in the pair, so the Slt-vs-Lt choice is unchanged.
                    CompBinOp::Gt => self.int_lt(rhs, lhs, signed),
                    // There is no native int `!=`/`<=`/`>=`. Derive them by negating
                    // an i1 with `CmpEq(x, false)` (returns 1 iff x == 0, i.e. boolean
                    // NOT), the identical construction the float `NotEq` arm above uses
                    // and one the SPIR-V translator maps.
                    CompBinOp::NotEq => {
                        let equal = self.fb.insert_inst(CmpEq::new(is, lhs, rhs), Type::I1);
                        let false_value = self.fb.make_imm_value(Immediate::from(false));
                        self.fb
                            .insert_inst(CmpEq::new(is, equal, false_value), Type::I1)
                    }
                    // `a >= b` == `!(a < b)`. `lt` is computed with the correct
                    // signedness first; the negation is sign-agnostic.
                    CompBinOp::GtEq => {
                        let less = self.int_lt(lhs, rhs, signed);
                        let false_value = self.fb.make_imm_value(Immediate::from(false));
                        self.fb
                            .insert_inst(CmpEq::new(is, less, false_value), Type::I1)
                    }
                    // `a <= b` == `!(b < a)`.
                    CompBinOp::LtEq => {
                        let greater = self.int_lt(rhs, lhs, signed);
                        let false_value = self.fb.make_imm_value(Immediate::from(false));
                        self.fb
                            .insert_inst(CmpEq::new(is, greater, false_value), Type::I1)
                    }
                })
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) binary op `{other:?}` is not supported"
            ))),
        }
    }

    /// Normalize a logical i8/i16 into WebAssembly's physical i32 register.
    /// The explicit extension is semantic: it removes any dependence on how a
    /// preceding immediate or narrow memory load happened to populate the high
    /// register bits.
    fn promote_narrow_int(&mut self, value: ValueId, ty: Type, signed: bool) -> ValueId {
        if !matches!(ty, Type::I8 | Type::I16) {
            return value;
        }
        let is = self.inst_set();
        if signed {
            self.fb
                .insert_inst(Sext::new(is, value, Type::I32), Type::I32)
        } else {
            self.fb
                .insert_inst(Zext::new(is, value, Type::I32), Type::I32)
        }
    }

    fn restore_narrow_int(&mut self, value: ValueId, logical: Type, physical: Type) -> ValueId {
        if logical == physical {
            value
        } else {
            let is = self.inst_set();
            self.fb.insert_inst(Trunc::new(is, value, logical), logical)
        }
    }

    /// Sign-aware integer less-than `a < b`, the primitive the R1 compare
    /// matrix derives from: `Slt` for signed operands (i32), `Lt` for unsigned
    /// (u32). The signedness comes from the operand CLASS, not the sonatina type
    /// (which is signless), so callers pass the flag they already resolved from
    /// the operand pair; the derived `>`/`>=`/`<=` cases can then swap `a`/`b`
    /// while keeping that same class key. Result is `Type::I1`.
    fn int_lt(&mut self, a: ValueId, b: ValueId, signed: bool) -> ValueId {
        let is = self.inst_set();
        if signed {
            self.fb.insert_inst(Slt::new(is, a, b), Type::I1)
        } else {
            self.fb.insert_inst(Lt::new(is, a, b), Type::I1)
        }
    }

    fn lower_unary(&mut self, op: UnOp, value: RLocalId) -> Result<ValueId, LowerError> {
        let is_float = matches!(
            self.body.value_class(value),
            Some(RuntimeClass::Scalar(ScalarClass {
                repr: ScalarRepr::Float { .. },
                ..
            }))
        );
        if is_float {
            let is = self.inst_set();
            let value = self.local_value(value)?;
            return match op {
                UnOp::Minus => Ok(self.fb.insert_inst(Fneg::new(is, value), Type::F32)),
                other => Err(LowerError::Unsupported(format!(
                    "wasm target: f32 unary op `{other:?}` is not supported"
                ))),
            };
        }
        let is = self.inst_set();
        let logical_ty = self.local_ty(value)?;
        let value = self.local_value(value)?;
        match op {
            // Fe's logical `!` is typed over bool before MIR. Sonatina carries
            // that as i1; Wasm represents it as i32 and translates IsZero to
            // the width-correct `i32.eqz` while retaining an i1 result in IR.
            UnOp::Not => Ok(self.fb.insert_inst(IsZero::new(is, value), Type::I1)),
            // Sonatina has no unary complement instruction. XOR with an
            // all-ones value is exactly `~x` for every admitted integer width.
            // Narrow Fe integers use Wasm's physical i32 lane and are
            // truncated back to their logical width after the operation.
            UnOp::BitNot => {
                let physical_ty = match logical_ty {
                    Type::I8 | Type::I16 => Type::I32,
                    other => other,
                };
                let value = self.promote_narrow_int(value, logical_ty, false);
                let ones = all_ones_immediate(physical_ty).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target: bitwise-not does not support `{logical_ty:?}`"
                    ))
                })?;
                let ones = self.fb.make_imm_value(ones);
                let complemented = self.fb.insert_inst(Xor::new(is, value, ones), physical_ty);
                Ok(self.restore_narrow_int(complemented, logical_ty, physical_ty))
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) unary op `{other:?}` is not supported"
            ))),
        }
    }

    fn lower_call(
        &mut self,
        callee: RuntimeInstance<'db>,
        args: &[RLocalId],
    ) -> Result<ValueId, LowerError> {
        if let Some(intrinsic) = gpu_intrinsic(self.module.db, callee) {
            return match intrinsic {
                GpuIntrinsic::StorageLoad => self.lower_gpu_storage_load_scalar(args),
                GpuIntrinsic::StorageStore => Err(LowerError::Internal(
                    "GPU storage store appeared as a value-returning call".to_owned(),
                )),
            };
        }
        if let Some(name) = callee
            .key(self.module.db)
            .semantic(self.module.db)
            .and_then(
                |semantic| match semantic.key(self.module.db).owner(self.module.db) {
                    hir::analysis::ty::ty_check::BodyOwner::Func(func) => func
                        .name(self.module.db)
                        .to_opt()
                        .map(|name| name.data(self.module.db).to_string()),
                    _ => None,
                },
            )
        {
            if matches!(
                name.as_str(),
                "__sqrt_f32"
                    | "__rsqrt_f32"
                    | "__abs_f32"
                    | "__min_f32"
                    | "__max_f32"
                    | "__min_relaxed_f32"
                    | "__max_relaxed_f32"
                    | "__clamp_f32"
                    | "__floor_f32"
                    | "__ceil_f32"
                    | "__trunc_f32"
                    | "__round_f32"
                    | "__f32_from_i32"
                    | "__i32_from_f32"
            ) {
                return Err(LowerError::Unsupported(format!(
                    "wasm target: f32 intrinsic `{name}` needs dedicated Sonatina lowering and must not become an external call"
                )));
            }
        }
        let is = self.inst_set();
        let callee_ref =
            *self.module.func_map.get(&callee).ok_or_else(|| {
                LowerError::Internal("wasm call target was not declared".to_string())
            })?;
        let ret_class = self
            .module
            .prepared_bodies
            .get(&callee)
            .ok_or_else(|| {
                LowerError::Internal(format!(
                    "prepared Wasm body for `{}` is missing",
                    self.module.function_symbol(callee),
                ))
            })?
            .signature
            .ret
            .clone();
        let ret_ty = if self.module.indirect_aggregate_returns.contains(&callee) {
            Type::I32
        } else {
            match ret_class {
                Some(class) => self.module.ty_for_class(&class)?,
                None => {
                    return Err(LowerError::Unsupported(
                        "wasm target (R1) does not support calling a unit-returning function \
                     as a value expression"
                            .to_string(),
                    ));
                }
            }
        };
        let (arg_vals, call_checkpoint) = self.checked_call_arg_values(callee, callee_ref, args)?;
        let result = self.fb.insert_inst(
            Call::new(is, callee_ref, arg_vals.into_iter().collect()),
            ret_ty,
        );
        self.rewind_call_arena(call_checkpoint);
        Ok(result)
    }

    fn lower_terminator(&mut self, terminator: &RTerminator<'db>) -> Result<(), LowerError> {
        let is = self.inst_set();
        match terminator {
            RTerminator::Return(Some(value)) => {
                if self.indirect_aggregate_return {
                    let class = self.body.value_class(*value).ok_or_else(|| {
                        LowerError::Internal(format!(
                            "indirect aggregate return value {value:?} has no runtime class"
                        ))
                    })?;
                    if !matches!(class, RuntimeClass::AggregateValue { .. })
                        || !self.module.aggregate_is_memory_lowerable(class)
                    {
                        return Err(LowerError::Internal(format!(
                            "indirect aggregate return value {value:?} is not memory-lowerable"
                        )));
                    }
                    let pointer = if self.is_address_carried_aggregate_value(*value) {
                        self.local_value(*value)?
                    } else {
                        self.lower_materialize_to_object(*value)?
                    };
                    // Ownership transfers into the caller's enclosing arena
                    // lifetime. Rewinding here would invalidate the returned
                    // value; the first non-indirect ancestor reclaims it.
                    self.fb
                        .insert_inst_no_result(Return::new_single(is, pointer));
                    return Ok(());
                }
                // R2.1: returning a flattened scalar tuple is a wasm MULTI-VALUE
                // return of its element words (the host reads the N results).
                // Addressable by-value parameters retain that public flattened
                // ABI even though their function-local representation is one
                // canonical-arena pointer, so load their leaves before rewinding
                // the scoped arena. Every other return is the single-value form.
                let flattened = self
                    .body
                    .signature
                    .ret
                    .as_ref()
                    .is_some_and(|class| self.module.scalar_tuple_element_tys(class).is_some());
                if flattened {
                    let values = self.local_flat_values(*value)?;
                    self.rewind_scoped_arena();
                    self.fb.insert_return_values(&values);
                } else {
                    let value = self.local_value(*value)?;
                    self.rewind_scoped_arena();
                    self.fb.insert_inst_no_result(Return::new_single(is, value));
                }
            }
            // A unit return and a `Stop` (the synthetic main-root exit) both
            // become a plain wasm return.
            RTerminator::Return(None) | RTerminator::Stop => {
                self.rewind_scoped_arena();
                self.fb.insert_inst_no_result(Return::new_unit(is));
            }
            RTerminator::Goto(target) => {
                let block = self.block_for(*target)?;
                self.fb.insert_inst_no_result(Jump::new(is, block));
            }
            RTerminator::Branch {
                cond,
                then_bb,
                else_bb,
            } => {
                let cond = self.local_value(*cond)?;
                let then_block = self.block_for(*then_bb)?;
                let else_block = self.block_for(*else_bb)?;
                self.fb
                    .insert_inst_no_result(Br::new(is, cond, then_block, else_block));
            }
            RTerminator::MatchEnumTag {
                tag,
                enum_layout,
                cases,
                default,
            } => {
                if !matches!(enum_layout.data(self.module.db), Layout::Enum(_)) {
                    return Err(LowerError::Internal(
                        "enum-tag match uses a non-enum layout".to_owned(),
                    ));
                }
                let actual = self.local_value(*tag)?;
                let invalid = match default {
                    Some(default) => self.block_for(*default)?,
                    None => self.trap_block(),
                };
                if cases.is_empty() {
                    self.fb.insert_inst_no_result(Jump::new(is, invalid));
                } else {
                    for (index, (variant, target)) in cases.iter().enumerate() {
                        let (expected, ty) =
                            self.module.enum_variant_const(*enum_layout, *variant)?;
                        let expected = self
                            .fb
                            .make_imm_value(immediate_for_const_scalar(&expected, ty)?);
                        let matches = self
                            .fb
                            .insert_inst(CmpEq::new(is, actual, expected), Type::I1);
                        let target = self.block_for(*target)?;
                        let otherwise = if index + 1 == cases.len() {
                            invalid
                        } else {
                            self.fb.append_block()
                        };
                        self.fb
                            .insert_inst_no_result(Br::new(is, matches, target, otherwise));
                        if index + 1 != cases.len() {
                            self.fb.switch_to_block(otherwise);
                        }
                    }
                }
            }
            RTerminator::TerminalCall { callee, args }
                if gpu_intrinsic(self.module.db, *callee) == Some(GpuIntrinsic::StorageStore) =>
            {
                self.lower_gpu_storage_store(args)?;
                self.rewind_scoped_arena();
                self.fb.insert_inst_no_result(Return::new_unit(is));
            }
            RTerminator::TerminalCall { callee, .. } => {
                return Err(LowerError::Unsupported(format!(
                    "wasm target (R1) does not support terminal call to `{}`; the callee is \
                     never-returning and needs an explicit portable lowering",
                    self.module.function_symbol(*callee),
                )));
            }
            RTerminator::Trap => {
                self.fb.insert_inst_no_result(Unreachable::new(is));
            }
            other => {
                return Err(LowerError::Unsupported(format!(
                    "wasm target (R1) terminator `{other:?}` is not supported \
                     (return-data/revert/switch/match/self-destruct are R2)"
                )));
            }
        }
        Ok(())
    }

    fn rewind_scoped_arena(&mut self) {
        if let Some(checkpoint) = self.arena_checkpoint {
            self.fb
                .insert_inst_no_result(MemRewind::new(self.inst_set(), checkpoint));
        }
    }

    fn rewind_call_arena(&mut self, checkpoint: Option<ValueId>) {
        if let Some(checkpoint) = checkpoint {
            self.fb
                .insert_inst_no_result(MemRewind::new(self.inst_set(), checkpoint));
        }
    }

    fn local_value(&mut self, local: RLocalId) -> Result<ValueId, LowerError> {
        let var = self.var_for(local)?;
        Ok(self.fb.use_var(var))
    }

    /// Whether `local` is a memory-lowerable object/memory-provider reference
    /// (Change 1): its SSA value is an arena i32 pointer, not a copyable value.
    fn is_object_ref_local(&self, local: RLocalId) -> bool {
        self.body
            .value_class(local)
            .is_some_and(|class| self.module.is_memory_lowerable_object_ref(class))
    }

    /// Whether an object-ref `local` is bound directly from a FRESHLY produced
    /// object (every definition is an `AllocObject` / `MaterializeToObject` /
    /// `MaterializePlaceToObject` / `ConstRef`). The ordinary local-array init
    /// (`a = use <alloc/materialize temp>`) satisfies this: the variable and the
    /// temp name one array, so binding the pointer is a safe move. A copy of an
    /// existing array (`b = use a`, where `a`'s definition is itself a `use` of an
    /// object) does NOT, so it fails closed rather than pointer-aliasing. A local
    /// with no definition (an array parameter) also does not qualify (deferred).
    fn is_fresh_object_binding(&self, local: RLocalId) -> bool {
        let mut has_fresh_def = false;
        for block in &self.body.blocks {
            for stmt in &block.stmts {
                let RStmt::Assign { dst, expr } = stmt else {
                    continue;
                };
                if *dst != local {
                    continue;
                }
                if matches!(
                    expr,
                    RExpr::AllocObject { .. }
                        | RExpr::MaterializeToObject { .. }
                        | RExpr::MaterializePlaceToObject { .. }
                        | RExpr::ConstRef { .. }
                ) {
                    has_fresh_def = true;
                } else {
                    return false;
                }
            }
        }
        has_fresh_def
    }

    /// Whether an object-ref local denotes a borrow of existing storage rather
    /// than an owned aggregate value. Forwarding this pointer must preserve
    /// identity: copying the pointee would turn a mutable borrow into a write to
    /// a detached temporary, so the caller would not observe the mutation.
    fn is_borrow_alias_binding(&self, local: RLocalId) -> bool {
        fn visit(
            body: &RuntimeBody<'_>,
            local: RLocalId,
            visiting: &mut HashSet<RLocalId>,
        ) -> bool {
            if !visiting.insert(local) {
                return false;
            }
            let mut has_definition = false;
            for block in &body.blocks {
                for stmt in &block.stmts {
                    let RStmt::Assign { dst, expr } = stmt else {
                        continue;
                    };
                    if *dst != local {
                        continue;
                    }
                    has_definition = true;
                    match expr {
                        RExpr::AddrOf { .. } | RExpr::Load { .. }
                            if matches!(
                                body.value_class(local),
                                Some(RuntimeClass::Ref {
                                    kind: RefKind::Object | RefKind::Const,
                                    ..
                                })
                            ) => {}
                        RExpr::Use(source) | RExpr::RetagRef { value: source }
                            if visit(body, *source, visiting) => {}
                        _ => return false,
                    }
                }
            }
            visiting.remove(&local);
            has_definition
        }

        visit(&self.body, local, &mut HashSet::new())
    }

    fn var_for(&self, local: RLocalId) -> Result<Variable, LowerError> {
        self.vars.get(&local).copied().ok_or_else(|| {
            LowerError::Unsupported(format!(
                "wasm target (R1): local {local:?} is not a value-carried scalar \
                 (address-taken/aggregate locals are R2)"
            ))
        })
    }

    fn local_ty(&self, local: RLocalId) -> Result<Type, LowerError> {
        let class = self.body.value_class(local).cloned().ok_or_else(|| {
            LowerError::Internal(format!("local {local:?} carries no value class"))
        })?;
        if self.is_address_carried_aggregate_value(local)
            || self.materialized_scalar_slots.contains(&local)
            || self.module.is_memory_lowerable_object_ref(&class)
            || self.module.object_value_layout(&class).is_some()
        {
            Ok(Type::I32)
        } else {
            self.module.ty_for_class(&class)
        }
    }

    fn materialized_scalar_slot_ty(&self, local: RLocalId) -> Result<Type, LowerError> {
        let Some(RuntimeClass::Scalar(class)) = self.body.value_class(local) else {
            return Err(LowerError::Internal(format!(
                "materialized scalar slot {local:?} has no scalar class"
            )));
        };
        scalar_ty_r1(class)
    }

    fn block_for(&self, block: RBlockId) -> Result<BlockId, LowerError> {
        self.block_map
            .get(block.as_u32() as usize)
            .copied()
            .ok_or_else(|| LowerError::Internal(format!("unknown runtime block {block:?}")))
    }
}

fn compute_reachable_blocks(body: &RuntimeBody<'_>) -> Vec<bool> {
    let mut reachable = vec![false; body.blocks.len()];
    let mut worklist = vec![0_usize];
    while let Some(idx) = worklist.pop() {
        if std::mem::replace(&mut reachable[idx], true) {
            continue;
        }
        match &body.blocks[idx].terminator {
            RTerminator::Goto(block) => worklist.push(block.as_u32() as usize),
            RTerminator::Branch {
                then_bb, else_bb, ..
            } => {
                worklist.push(then_bb.as_u32() as usize);
                worklist.push(else_bb.as_u32() as usize);
            }
            RTerminator::SwitchScalar { cases, default, .. } => {
                worklist.extend(cases.iter().map(|(_, block)| block.as_u32() as usize));
                worklist.push(default.as_u32() as usize);
            }
            RTerminator::MatchEnumTag { cases, default, .. } => {
                worklist.extend(cases.iter().map(|(_, block)| block.as_u32() as usize));
                worklist.extend(default.iter().map(|block| block.as_u32() as usize));
            }
            RTerminator::TerminalCall { .. }
            | RTerminator::ReturnData { .. }
            | RTerminator::Revert { .. }
            | RTerminator::SelfDestruct { .. }
            | RTerminator::Trap
            | RTerminator::Return(_)
            | RTerminator::Stop => {}
        }
    }
    reachable
}

/// R2.0 fail-closed error for a place read outside the admitted transport-word
/// identity sliver (ladder doc section 7.2): addresses, offsets, stores, object
/// materializations, deeper paths, multi-field pointees, and all real linear
/// memory stay R2.
fn unsupported_place(place: &RuntimePlace<'_>) -> LowerError {
    LowerError::Unsupported(format!(
        "wasm target: place read `{place:?}` is R2. Only an empty-path or `[Field(0)]` \
         read on a memory-space provider-ref transport word lowers (R2.0); addresses, \
         offsets, stores, and object materializations stay fail-closed."
    ))
}

fn immediate_for_const_scalar(constant: &ConstScalar, ty: Type) -> Result<Immediate, LowerError> {
    match constant {
        ConstScalar::Bool(value) => Ok(Immediate::from(*value)),
        ConstScalar::Int { words, signed, .. } => {
            Ok(Immediate::from_i256(bytes_to_i256(words, *signed), ty))
        }
        ConstScalar::Float { bits } if ty == Type::F32 => Ok(Immediate::F32(*bits)),
        ConstScalar::Float { .. } => Err(LowerError::Internal(format!(
            "wasm target: f32 constant was assigned non-f32 Sonatina type `{ty:?}`"
        ))),
        other => Err(LowerError::Unsupported(format!(
            "wasm target (R1) constant `{other:?}` is not supported"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::InputDb;
    use hir::analysis::semantic::FieldIndex;
    use mir::ScalarRole;
    use url::Url;

    #[test]
    fn aggregate_param_reification_excludes_mutable_object_refs() {
        let db = DriverDataBase::default();
        assert!(is_reifiable_aggregate_ref(&RefKind::Const));
        assert!(is_reifiable_aggregate_ref(&RefKind::Provider {
            provider_ty: hir::analysis::ty::ty_def::TyId::invalid(
                &db,
                hir::analysis::ty::ty_def::InvalidCause::Other,
            ),
            space: AddressSpaceKind::Memory,
        }));
        assert!(!is_reifiable_aggregate_ref(&RefKind::Object));
    }

    #[test]
    fn own_aggregate_slot_reification_is_leaf_read_only_and_fail_closed() {
        let source = r#"
use core::BrowserList

struct Request {
    values: BrowserList<u32, 4>,
    mode: u32,
}

pub fn lane(request: own Request) -> u32 {
    request.values.len + request.mode
}
"#;
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///aggregate_slot_reification.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_owned()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "lane").unwrap();
        let function = package.functions(&db)[0];
        let body = function.instance(&db).body(&db);
        let candidate = body.signature.params[0].local;
        assert!(slot_param_has_only_static_field_reads(
            &db, &body, candidate
        ));

        let mut whole = body.clone();
        let RStmt::Assign {
            expr: RExpr::Load { place },
            ..
        } = &mut whole.blocks[0].stmts[0]
        else {
            panic!("expected projected request load");
        };
        place.path = Box::new([]);
        assert!(!slot_param_has_only_static_field_reads(
            &db, &whole, candidate
        ));
        let mut multi_leaf = body.clone();
        let RStmt::Assign {
            expr: RExpr::Load { place },
            ..
        } = &mut multi_leaf.blocks[0].stmts[0]
        else {
            panic!("expected projected request load");
        };
        place.path = Box::new([PlaceElem::Field(FieldIndex(0))]);
        assert!(!slot_param_has_only_static_field_reads(
            &db,
            &multi_leaf,
            candidate
        ));
        let mut dynamic = body.clone();
        let RStmt::Assign {
            expr: RExpr::Load { place },
            ..
        } = &mut dynamic.blocks[0].stmts[0]
        else {
            panic!("expected projected request load");
        };
        place.path = Box::new([PlaceElem::Deref]);
        assert!(!slot_param_has_only_static_field_reads(
            &db, &dynamic, candidate
        ));

        let mut address_taken = body.clone();
        let RStmt::Assign {
            expr: RExpr::Load { place },
            ..
        } = &mut address_taken.blocks[0].stmts[0]
        else {
            panic!("expected projected request load");
        };
        let place = place.clone();
        address_taken.blocks[0].stmts[0] = RStmt::Assign {
            dst: RLocalId::from_u32(1),
            expr: RExpr::AddrOf {
                place: place.clone(),
            },
        };
        assert!(!slot_param_has_only_static_field_reads(
            &db,
            &address_taken,
            candidate
        ));

        let mut stored = body.clone();
        stored.blocks[0].stmts[0] = RStmt::Store {
            dst: place,
            src: RLocalId::from_u32(1),
        };
        assert!(!slot_param_has_only_static_field_reads(
            &db, &stored, candidate
        ));
    }

    #[test]
    fn authored_mvt2_specialization_prunes_structural_residuals() {
        let source = include_str!("../../tests/fixtures/spirv/mvt2_f32_helper_render.fe");
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///mvt2_specialization_residual.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let package = mir::build_wasm_runtime_package(&db, top_mod)
            .expect("authored MvT2 package should lower to Runtime MIR");

        let rebuild = package
            .functions(&db)
            .into_iter()
            .find(|function| function.symbol(&db).contains("rebuild_mvt2"))
            .expect("authored rebuild_mvt2 helper should exist")
            .instance(&db);
        let prepared = prepare_inline_value_bodies(&db, &package);
        let (before, after) = prepared.residuals.get(&rebuild).copied().unwrap_or((0, 0));
        assert!(
            before > after,
            "shape-seeded recursive preparation did not shrink rebuild_mvt2: {before} -> {after}"
        );
    }

    #[test]
    fn authored_mvt5_specialization_measures_smaller_nested_residual() {
        let source = include_str!("../../tests/fixtures/spirv/mvt5_f32_nested_helper_render.fe");
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///mvt5_specialization_residual.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let package = mir::build_wasm_runtime_package(&db, top_mod)
            .expect("authored MvT5 package should lower to Runtime MIR");
        let nested = package
            .functions(&db)
            .into_iter()
            .find(|function| function.symbol(&db).contains("nested_swap_mvt5"))
            .expect("authored nested_swap_mvt5 helper should exist")
            .instance(&db);

        let prepared = prepare_inline_value_bodies(&db, &package);
        let (before, after) = prepared.residuals.get(&nested).copied().unwrap_or((0, 0));
        assert_eq!(
            (before, after),
            (102, 6),
            "authored nested MvT5 structural residual changed"
        );
    }

    #[test]
    fn inline_preparation_caches_every_unspecialized_body_past_shape_limit() {
        let mut source = String::from("fn layer0(_ value: i32) -> i32 { value + 1 }\n");
        for index in 1..320 {
            source.push_str(&format!(
                "fn layer{index}(_ value: i32) -> i32 {{ layer{}(value) }}\n",
                index - 1,
            ));
        }
        source.push_str("pub fn run(_ value: i32) -> i32 { layer319(value) }\n");

        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///inline_base_cache_over_shape_limit.fe").unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics:\n{diagnostics}"
        );
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "run")
            .expect("long helper chain should lower to Runtime MIR");
        let function_count = package.functions(&db).len();
        assert!(
            function_count > INLINE_SPECIALIZATION_CACHE_LIMIT,
            "fixture must cross the shape-specialization cache limit"
        );

        let prepared = prepare_inline_value_bodies(&db, &package);
        assert_eq!(prepared.bodies.len(), function_count);
        assert_eq!(
            prepared.unspecialized_preparations, function_count,
            "an unspecialized Runtime MIR body must be prepared exactly once"
        );
    }

    #[test]
    fn reduced_staged_scalar_eval_prepares_call_free_entry() {
        let source = r#"
struct Zero {}
struct Term<const I: i32> {}
struct Add<L, R> {}
const fn payload(_ i: usize) -> i32 {
    if i == 0 { 1 } else if i == 1 { 4 } else if i == 2 { 7 } else { 10 }
}
recursive type fn Schedule<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<Term<{payload(N - 1)}>, Schedule<{N - 1}>>
    }
}
trait Eval { fn eval(x: i32) -> i32 }
impl Eval for Zero {
    #[inline(always)]
    fn eval(x: i32) -> i32 { 0 }
}
impl<const I: i32> Eval for Term<I> {
    #[inline(always)]
    fn eval(x: i32) -> i32 { x + I }
}
impl<L: Eval, R: Eval> Eval for Add<L, R> {
    #[inline(always)]
    fn eval(x: i32) -> i32 {
        <L as Eval>::eval(x: x) + <R as Eval>::eval(x: x)
    }
}
pub fn staged_scalar_schedule4(x: i32) -> i32 {
    <Schedule<4> as Eval>::eval(x: x)
}
"#;
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///staged_scalar_inline_schedule4.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics:\n{diagnostics}"
        );
        let package = mir::build_wasm_runtime_package(&db, top_mod)
            .expect("reduced staged schedule should lower to Runtime MIR");
        let entry = package.root_objects(&db)[0].sections(&db)[0]
            .entry
            .instance(&db);
        let prepared = prepare_inline_value_bodies(&db, &package);
        let body = prepared.bodies.get(&entry).expect("prepared entry body");
        let callees = body
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter_map(|stmt| match stmt {
                RStmt::Assign {
                    expr: RExpr::Call { callee, .. },
                    ..
                } => package
                    .functions(&db)
                    .into_iter()
                    .find(|function| function.instance(&db) == *callee)
                    .map(|function| function.symbol(&db).clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(callees.is_empty(), "residual scalar calls: {callees:#?}");
    }

    #[test]
    fn narrow_usize_pass_narrows_usize_but_preserves_u256() {
        // A `usize` local (256-bit in MIR, semantic `Usize`) narrows to i32 on
        // the wasm path; a genuine `u256` local (semantic `U256`) keeps its
        // 256-bit repr and stays fail-closed.
        let source = r#"
pub fn kernel(k: u32) -> u256 {
    let idx: usize = k as usize
    let mut total: u256 = 0
    if idx < 4 {
        total = total + 1
    }
    total
}
"#;
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///narrow_usize_pass.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(diagnostics.is_empty(), "diags:\n{diagnostics}");
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "kernel").unwrap();
        let body = package.functions(&db)[0].instance(&db).body(&db);

        let is_u256_unsigned_local = |local: &RLocal| {
            matches!(
                &local.carrier,
                RuntimeCarrier::Value(RuntimeClass::Scalar(ScalarClass {
                    repr: ScalarRepr::Int {
                        bits: 256,
                        signed: false
                    },
                    ..
                }))
            )
        };
        let usize_idx = body
            .locals
            .iter()
            .position(|local| {
                is_u256_unsigned_local(local) && is_usize_semantic_ty(&db, local.semantic_ty)
            })
            .expect("a usize local should exist");
        let u256_idx = body
            .locals
            .iter()
            .position(|local| {
                is_u256_unsigned_local(local)
                    && matches!(
                        local.semantic_ty.base_ty(&db).data(&db),
                        TyData::TyBase(TyBase::Prim(PrimTy::U256))
                    )
            })
            .expect("a u256 local should exist");

        let mut narrowed = body.clone();
        let instance = package.functions(&db)[0].instance(&db);
        narrow_usize_scalars(&db, instance, &mut narrowed);

        assert!(
            matches!(
                &narrowed.locals[usize_idx].carrier,
                RuntimeCarrier::Value(RuntimeClass::Scalar(ScalarClass {
                    repr: ScalarRepr::Int {
                        bits: 32,
                        signed: false
                    },
                    ..
                }))
            ),
            "usize local should narrow to i32; got {:?}",
            narrowed.locals[usize_idx].carrier
        );
        assert!(
            matches!(
                &narrowed.locals[u256_idx].carrier,
                RuntimeCarrier::Value(RuntimeClass::Scalar(ScalarClass {
                    repr: ScalarRepr::Int {
                        bits: 256,
                        signed: false
                    },
                    ..
                }))
            ),
            "genuine u256 local must stay 256-bit (fail-closed); got {:?}",
            narrowed.locals[u256_idx].carrier
        );
    }

    #[test]
    fn f32_carrier_type_and_immediate_preserve_exact_bits() {
        let class = ScalarClass {
            repr: ScalarRepr::Float { bits: 32 },
            role: ScalarRole::Plain,
        };
        assert_eq!(scalar_ty_r1(&class).unwrap(), Type::F32);

        for bits in [0x8000_0000, 0x7fc0_1234] {
            assert_eq!(
                immediate_for_const_scalar(&ConstScalar::Float { bits }, Type::F32).unwrap(),
                Immediate::F32(bits),
            );
        }
    }

    #[test]
    fn non_f32_float_width_fails_closed() {
        let class = ScalarClass {
            repr: ScalarRepr::Float { bits: 64 },
            role: ScalarRole::Plain,
        };
        let error = scalar_ty_r1(&class).unwrap_err().to_string();
        assert!(error.contains("f64") && error.contains("f32"), "{error}");
    }

    #[test]
    fn const_int_fits_u32_rejects_high_words() {
        // `words` are big-endian (bytes_to_i256 / I256::from_be_bytes order).
        assert!(const_int_fits_u32(&[]));
        assert!(const_int_fits_u32(&[0x2a]));
        assert!(const_int_fits_u32(&[0xff, 0xff, 0xff, 0xff])); // u32::MAX
        // 0x80000000 and 0xffffffff FIT in u32 (their bit pattern is preserved and
        // the unsigned bounds check rejects them); only genuinely wider values are
        // refused.
        assert!(const_int_fits_u32(&[
            0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00
        ]));
        assert!(const_int_fits_u32(&[
            0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff
        ]));
        // 2^32 and above do NOT fit: a high word is set.
        assert!(!const_int_fits_u32(&[0x01, 0x00, 0x00, 0x00, 0x00]));
        assert!(!const_int_fits_u32(&[
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00
        ]));
    }

    #[test]
    fn dead_aggregate_pass_keeps_multi_definition_with_effectful_call() {
        // The dead-aggregate pass may delete a value-carried array/enum local only
        // when EVERY definition of it is a pure aggregate def. A multi-definition
        // local with an effectful `Call` def must keep all of its assignments;
        // deleting the call on the strength of one pure def would drop the effect.
        let source = r#"
fn seed(_ x: u32) -> u32 { x }
pub fn kernel(k: u32) -> u32 {
    let mut a: [u32; 2] = [0; 2]
    a[0] = seed(k)
    a[k as usize]
}
"#;
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///dead_aggregate_multidef.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(diagnostics.is_empty(), "diags:\n{diagnostics}");
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "kernel").unwrap();
        let real = package.functions(&db)[0].instance(&db).body(&db);

        // A real, effectful `Call` expression (the call to `seed`).
        let call_expr = real
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .find_map(|stmt| match stmt {
                RStmt::Assign {
                    expr: expr @ RExpr::Call { .. },
                    ..
                } => Some(expr.clone()),
                _ => None,
            })
            .expect("kernel should contain a Call to seed");

        // A real `[u32; 2]` Array layout (from the array object-ref or its value).
        let array_layout = real
            .locals
            .iter()
            .find_map(|local| match &local.carrier {
                RuntimeCarrier::Value(RuntimeClass::Ref { pointee, .. }) => match &**pointee {
                    RuntimeClass::AggregateValue { layout }
                        if matches!(layout.data(&db), Layout::Array(_)) =>
                    {
                        Some(*layout)
                    }
                    _ => None,
                },
                RuntimeCarrier::Value(RuntimeClass::AggregateValue { layout })
                    if matches!(layout.data(&db), Layout::Array(_)) =>
                {
                    Some(*layout)
                }
                _ => None,
            })
            .expect("kernel should have an Array-layout local");

        // A real scalar local to borrow as an inert operand + semantic_ty.
        let scalar_local = real
            .locals
            .iter()
            .find(|local| {
                matches!(
                    &local.carrier,
                    RuntimeCarrier::Value(RuntimeClass::Scalar(_))
                )
            })
            .expect("kernel should have a scalar local")
            .clone();
        let any_ty = scalar_local.semantic_ty;

        // Fabricate a two-local body: local 0 is an inert scalar; local 1 is an
        // UNUSED array-value local with TWO defs -- a pure `Use` and the effectful
        // `Call`. Keep the real owner/key/signature so the body stays well-formed.
        let mut body = real.clone();
        body.provider_bindings = Vec::new();
        body.signature.params = Vec::new();
        body.locals = vec![
            scalar_local,
            RLocal {
                semantic_ty: any_ty,
                carrier: RuntimeCarrier::Value(RuntimeClass::AggregateValue {
                    layout: array_layout,
                }),
                root: RuntimeLocalRoot::None,
            },
        ];
        let dead = RLocalId::from_u32(1);
        body.blocks.truncate(1);
        body.blocks[0].stmts = vec![
            RStmt::Assign {
                dst: dead,
                expr: RExpr::Use(RLocalId::from_u32(0)),
            },
            RStmt::Assign {
                dst: dead,
                expr: call_expr,
            },
        ];
        body.blocks[0].terminator = RTerminator::Stop;

        drop_dead_pure_aggregate_values(&db, &mut body);

        assert!(
            body.blocks[0].stmts.iter().any(|stmt| matches!(
                stmt,
                RStmt::Assign {
                    expr: RExpr::Call { .. },
                    ..
                }
            )),
            "the effectful Call definition must not be deleted"
        );
        assert!(
            !matches!(body.locals[1].carrier, RuntimeCarrier::Erased),
            "a multi-def local with an impure def must not be erased"
        );
    }

    #[test]
    fn fresh_materialize_whole_load_folds_to_original_aggregate_value() {
        let source = r#"
pub fn kernel(k: u32) -> u32 {
    let a: [u32; 2] = [k, k]
    a[0]
}
"#;
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///fresh_materialize_load_fold.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(diagnostics.is_empty(), "diags:\n{diagnostics}");
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "kernel").unwrap();
        let real = package.functions(&db)[0].instance(&db).body(&db);
        let array_local = real
            .locals
            .iter()
            .find(|local| {
                local
                    .carrier
                    .value_class()
                    .and_then(RuntimeClass::aggregate_layout)
                    .is_some_and(|layout| matches!(layout.data(&db), Layout::Array(_)))
            })
            .expect("kernel should expose its array layout")
            .clone();
        let layout = array_local
            .carrier
            .value_class()
            .and_then(RuntimeClass::aggregate_layout)
            .unwrap();
        let aggregate = RuntimeClass::AggregateValue { layout };

        let mut body = real.clone();
        body.provider_bindings.clear();
        body.signature.params.clear();
        body.locals = vec![
            RLocal {
                semantic_ty: array_local.semantic_ty,
                carrier: RuntimeCarrier::Value(aggregate.clone()),
                root: RuntimeLocalRoot::None,
            },
            RLocal {
                semantic_ty: array_local.semantic_ty,
                carrier: RuntimeCarrier::Value(RuntimeClass::object_ref(layout)),
                root: RuntimeLocalRoot::None,
            },
            RLocal {
                semantic_ty: array_local.semantic_ty,
                carrier: RuntimeCarrier::Value(aggregate.clone()),
                root: RuntimeLocalRoot::None,
            },
        ];
        let value = RLocalId::from_u32(0);
        let object = RLocalId::from_u32(1);
        let loaded = RLocalId::from_u32(2);
        body.blocks.truncate(1);
        body.blocks[0].stmts = vec![
            RStmt::Assign {
                dst: value,
                expr: RExpr::Placeholder {
                    class: aggregate.clone(),
                },
            },
            RStmt::Assign {
                dst: object,
                expr: RExpr::MaterializeToObject { src: value },
            },
            RStmt::Assign {
                dst: loaded,
                expr: RExpr::Load {
                    place: RuntimePlace {
                        root: PlaceRoot::Ref(object),
                        path: Box::default(),
                    },
                },
            },
        ];
        body.blocks[0].terminator = RTerminator::Return(Some(loaded));

        fold_fresh_materialize_load_roundtrips(&db, &mut body);

        assert!(
            body.blocks[0].stmts.iter().all(|stmt| !matches!(
                stmt,
                RStmt::Assign {
                    expr: RExpr::MaterializeToObject { .. } | RExpr::Load { .. },
                    ..
                }
            )),
            "the fresh materialize/load round trip should disappear: {body:#?}"
        );
        assert!(
            body.blocks[0].stmts.iter().any(|stmt| matches!(
                stmt,
                RStmt::Assign {
                    dst,
                    expr: RExpr::Use(src),
                } if *dst == loaded && *src == value
            )),
            "the loaded value should forward the original aggregate: {body:#?}"
        );
        assert!(
            matches!(body.locals[1].carrier, RuntimeCarrier::Erased),
            "the unobserved compiler materialization local should be erased"
        );
    }
}
