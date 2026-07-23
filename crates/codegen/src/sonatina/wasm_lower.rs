//! Narrow MIR -> Sonatina IR lowering for the wasm target (R1).
//!
//! This is the first genuinely-Fe-compiled wasm path: `MIR runtime package ->
//! Sonatina IR (portable vocabulary, Wasm32 ISA) -> WAFFLE -> wasm bytes`. It is
//! deliberately narrow. It lowers only the scalar-arithmetic + control-flow +
//! call subset needed for `add`, `sum_to` (a loop/phi), and a two-function call
//! pair, and it FAILS CLOSED on everything else (aggregates, memory builtins,
//! checked-overflow reverts, u128/u256, EVM host ops). Those are R2.
//!
//! Why a separate path rather than parameterizing the EVM lowerer
//! (`lower_runtime.rs`): the EVM path hardcodes `Type::I256` as the word in ~90
//! places and lowers Fe's checked arithmetic to `uaddo` + an EVM `revert` panic
//! block (see `emit_panic_revert`), which is EVM-native. A faithful portable
//! rewrite of that lowerer is R2-scale. For R1 we lower the clean MIR directly:
//! at the MIR level `a + b` is `RExpr::Binary { op: Arith(Add), .. }` with no
//! overflow machinery attached, so we emit a plain portable `arith::Add`. The
//! WAFFLE translator currently fakes overflow flags as 0, so R1 is only correct
//! for non-overflowing values; real checked semantics are R2.
//!
//! It reuses Sonatina's `FunctionBuilder` SSA-variable machinery (declare/def/
//! use + `seal_all`) exactly as the EVM lowerer does, so loop-carried values
//! (`sum_to`'s accumulator) get their phis inserted automatically. MIR runtime
//! locals in this subset are normally value-carried (`RuntimeLocalRoot::None`).
//! One closed Slot shape is also admitted: a primitive scalar stored to and
//! loaded from the whole Slot is promoted to an SSA variable. Slot projections,
//! aggregates, addressing, and aliasing operations remain out of scope and fail
//! closed. Other place reads are fail-closed R2, with ONE admitted sliver (R2.0,
//! control-effects ladder
//! section 7): a Ref-rooted place whose carrier is a memory-space provider ref,
//! at the empty path or `[Field(0)]` on a single-scalar-field newtype, lowers as
//! the identity on the transport word (`use_var`); it is what lets an own-mode
//! word-carried token (`Wait::wait<T>(_ pending: own Pending<T>)`) consume. Apart
//! from whole primitive scalar Slot stores, stores, addresses, offsets, and
//! object materializations remain R2 and fail closed.

use std::collections::{HashMap, HashSet};

use driver::DriverDataBase;
use hir::hir_def::{ArithBinOp, BinOp, CompBinOp, UnOp};
use mir::{
    AddressSpaceKind, ConstNode, ConstScalar, IntrinsicArithBinOp, Layout, LayoutId, PlaceElem,
    PlaceRoot, RBlockId, RExpr, RLocal, RLocalId, RStmt, RTerminator, RefKind, RuntimeBody,
    RuntimeBuiltin, RuntimeCarrier, RuntimeClass, RuntimeFunction, RuntimeInlineHint,
    RuntimeInstance, RuntimeLinkage, RuntimeLocalRoot, RuntimePackage, RuntimePlace, ScalarClass,
    ScalarRepr,
};
use rustc_hash::FxHashMap;
use sonatina_ir::{
    BlockId, Immediate, Linkage, Module, Signature, Type, ValueId,
    builder::{FunctionBuilder, ModuleBuilder, Variable},
    func_cursor::InstInserter,
    inst::{
        arith::{Add, Fadd, Fdiv, Fmul, Fneg, Fsqrt, Fsub, Mul, Sar, Shr, Sub},
        cast::{F32ToI32, I32ToF32, Sext, Trunc, Zext},
        cmp::{Eq as CmpEq, Feq, Fle, Flt, Lt, Slt},
        control_flow::{Br, Call, Jump, Phi, Return, Unreachable},
        data::{MemAllocDynamic, Mload, Mstore},
        logic::And,
    },
    isa::{Isa, wasm32::Wasm32},
    module::{FuncRef, ModuleCtx},
};
use sonatina_triple::{Architecture, OperatingSystem, TargetTriple, Vendor};

use super::LowerError;
use super::lower_runtime::{
    assign_sonatina_function_symbols, bytes_to_i256, linkage_for_runtime, scalar_ty,
};

/// The Wasm32 ISA the wasm lowering targets (little-endian, 32-bit pointers,
/// portable `NativeInstSet` vocabulary).
pub(crate) fn create_wasm32_isa() -> Wasm32 {
    Wasm32::new(TargetTriple::new(
        Architecture::Wasm32,
        Vendor::Unknown,
        OperatingSystem::Native,
    ))
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
    compile_runtime_package_wasm_with_canonical_lanes(db, package, &[])
}

pub(crate) fn compile_runtime_package_wasm_with_canonical_lanes(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    canonical_lanes: &[crate::CanonicalLane],
) -> Result<(Module, HashMap<String, String>), LowerError> {
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
    let mut lowerer = WasmModuleLowerer::new(db, builder, &isa, package);
    lowerer.declare_functions()?;
    lowerer.lower_bodies()?;
    for lane in canonical_lanes {
        lowerer.synthesize_canonical_lane(lane)?;
    }
    let import_modules = lowerer.import_modules();
    Ok((lowerer.finish(), import_modules))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FlatShape {
    Leaf(Type),
    Struct(Vec<FlatShape>),
}

impl FlatShape {
    fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Struct(fields) => fields.iter().map(Self::leaf_count).sum(),
        }
    }

    fn leaf_types(&self, out: &mut Vec<Type>) {
        match self {
            Self::Leaf(ty) => out.push(*ty),
            Self::Struct(fields) => fields.iter().for_each(|field| field.leaf_types(out)),
        }
    }

    fn field_range(&self, index: usize) -> Option<(usize, usize, &FlatShape)> {
        let Self::Struct(fields) = self else {
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

    // Reify only const handles that MIR already materializes as a whole value.
    // Const refs used by projections/borrows remain refs and therefore retain
    // the backend's existing fail-closed behavior.
    let mut materialized = HashSet::new();
    for block in &body.blocks {
        for stmt in &block.stmts {
            if let RStmt::Assign {
                expr:
                    RExpr::Load {
                        place:
                            RuntimePlace {
                                root: PlaceRoot::Ref(src),
                                path,
                            },
                    },
                ..
            } = stmt
                && path.is_empty()
            {
                materialized.insert(*src);
            }
        }
    }
    loop {
        let before = materialized.len();
        for block in &body.blocks {
            for stmt in &block.stmts {
                if let RStmt::Assign {
                    dst,
                    expr: RExpr::Use(src),
                } = stmt
                    && materialized.contains(dst)
                {
                    materialized.insert(*src);
                }
            }
        }
        if materialized.len() == before {
            break;
        }
    }

    let (locals, blocks) = (&mut body.locals, &mut body.blocks);
    for block in blocks {
        let mut rewritten = Vec::with_capacity(block.stmts.len());
        for stmt in std::mem::take(&mut block.stmts) {
            if let RStmt::Assign {
                dst,
                expr: RExpr::ConstRef { region, layout },
            } = &stmt
                && materialized.contains(dst)
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
        done: &mut FxHashMap<(RuntimeInstance<'db>, mir::RuntimeArgShapeKey), RuntimeBody<'db>>,
        specialization_work: &mut usize,
        #[cfg(test)] residual_stmt_counts: &mut FxHashMap<RuntimeInstance<'db>, (usize, usize)>,
    ) -> RuntimeBody<'db> {
        let cache_key = (instance, arg_shape);
        if let Some(body) = done.get(&cache_key) {
            return body.clone();
        }
        // Once the bounded amount of shape-specific work is exhausted, fail
        // closed to the unspecialized body instead of repeatedly traversing new
        // shapes.
        if cache_key.1.has_known_facts() {
            if *specialization_work >= INLINE_SPECIALIZATION_CACHE_LIMIT {
                return instance.body(db);
            }
            *specialization_work += 1;
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
                    done,
                    specialization_work,
                    #[cfg(test)]
                    residual_stmt_counts,
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
        if done.len() < INLINE_SPECIALIZATION_CACHE_LIMIT {
            done.insert(cache_key, body.clone());
        }
        body
    }

    let mut visiting = HashSet::new();
    let mut done = FxHashMap::default();
    let mut roots = FxHashMap::default();
    let mut specialization_work = 0usize;
    #[cfg(test)]
    let mut residual_stmt_counts = FxHashMap::default();
    for function in package.functions(db) {
        let instance = function.instance(db);
        let params = instance.body(db).signature.params.len();
        let shape =
            mir::RuntimeArgShapeKey(vec![mir::RuntimeArgFact::Unknown; params].into_boxed_slice());
        let body = visit(
            db,
            package,
            instance,
            shape,
            &mut visiting,
            &mut done,
            &mut specialization_work,
            #[cfg(test)]
            &mut residual_stmt_counts,
        );
        roots.insert(instance, body);
    }
    PreparedInlineBodies {
        bodies: roots,
        #[cfg(test)]
        residuals: residual_stmt_counts,
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
                let mut fields = Vec::with_capacity(field_facts.len());
                for (index, (field_class, field_fact)) in
                    field_classes.iter().zip(field_facts).enumerate()
                {
                    if stmts.len() >= budget {
                        return None;
                    }
                    let field = RLocalId::from_u32(body.locals.len() as u32);
                    body.locals.push(mir::RLocal {
                        semantic_ty: template.semantic_ty,
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
                        | RExpr::Builtin(
                            RuntimeBuiltin::IntrinsicArith { .. }
                                | RuntimeBuiltin::F32FromI32 { .. }
                                | RuntimeBuiltin::I32FromF32 { .. }
                                | RuntimeBuiltin::F32Sqrt { .. }
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
/// values for Wasm. Fe presents ordinary record parameters as references, but
/// the Wasm product ABI already carries closed scalar trees by value. Convert
/// only compile-time `Field` paths through struct layouts; dynamic indexes,
/// enums, stores, address-taking, and non-aggregate/resource pointees are left
/// untouched and continue to fail closed in normal lowering.
fn is_reifiable_aggregate_ref(kind: &RefKind<'_>) -> bool {
    matches!(kind, RefKind::Const)
}

fn reify_static_aggregate_params<'db>(db: &'db DriverDataBase, body: &mut RuntimeBody<'db>) {
    let candidates = body
        .signature
        .params
        .iter()
        .filter_map(|param| match &param.class {
            RuntimeClass::Ref {
                pointee,
                kind,
                ..
            }
                if is_reifiable_aggregate_ref(kind)
                    && matches!(pointee.as_ref(), RuntimeClass::AggregateValue { .. }) =>
            {
                Some((param.local, pointee.as_ref().clone()))
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
            let RStmt::Assign {
                dst,
                expr:
                    RExpr::Load {
                        place:
                            RuntimePlace {
                                root: PlaceRoot::Ref(root),
                                path,
                            },
                    },
            } = &stmt
            else {
                rewritten.push(stmt);
                continue;
            };
            let Some(mut class) = candidates.get(root).cloned() else {
                rewritten.push(stmt);
                continue;
            };
            if path.is_empty() {
                locals[dst.as_u32() as usize].carrier = RuntimeCarrier::Value(class);
                locals[dst.as_u32() as usize].root = RuntimeLocalRoot::None;
                rewritten.push(RStmt::Assign {
                    dst: *dst,
                    expr: RExpr::Use(*root),
                });
                continue;
            }
            if !path.iter().all(|elem| matches!(elem, PlaceElem::Field(_))) {
                rewritten.push(stmt);
                continue;
            }

            let mut current = *root;
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
                let Some(field_semantic_ty) =
                    semantic_fields.get(index.0 as usize).copied()
                else {
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

struct WasmModuleLowerer<'db, 'a> {
    db: &'db DriverDataBase,
    builder: ModuleBuilder,
    isa: &'a Wasm32,
    package: &'a RuntimePackage<'db>,
    prepared_bodies: FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
    func_symbols: FxHashMap<RuntimeInstance<'db>, String>,
    func_map: FxHashMap<RuntimeInstance<'db>, FuncRef>,
}

impl<'db, 'a> WasmModuleLowerer<'db, 'a> {
    fn new(
        db: &'db DriverDataBase,
        builder: ModuleBuilder,
        isa: &'a Wasm32,
        package: &'a RuntimePackage<'db>,
    ) -> Self {
        let mut prepared_bodies = prepare_inline_value_bodies(db, package).bodies;
        for body in prepared_bodies.values_mut() {
            reify_static_aggregate_params(db, body);
        }
        Self {
            db,
            builder,
            isa,
            package,
            prepared_bodies,
            func_symbols: assign_sonatina_function_symbols(db, package),
            func_map: FxHashMap::default(),
        }
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

    /// The symbol -> wasm-import-module side table for external declarations
    /// (R3.3). Each non-builtin `extern` whose block carries
    /// `#[wasm_import(module = "...")]` maps its Sonatina symbol (which becomes
    /// the import's field name) to that module string. Attribute-less externs are
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
            if let Some(module) = mir::wasm_import_module(self.db, instance) {
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
        if let Some(name) = mir::wasm_import_name(self.db, instance) {
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
            if let Some(name) = mir::wasm_import_name(self.db, instance) {
                let module =
                    mir::wasm_import_module(self.db, instance).unwrap_or_else(|| "fe".to_string());
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
        Ok(())
    }

    fn lower_signature(&mut self, function: RuntimeFunction<'db>) -> Result<Signature, LowerError> {
        let instance = function.instance(self.db);
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
        let mut args = Vec::with_capacity(body.signature.params.len());
        for param in &body.signature.params {
            if let Some(elem_tys) = self.scalar_tuple_element_tys(&param.class) {
                args.extend(elem_tys);
            } else {
                args.push(self.ty_for_class(&param.class)?);
            }
        }
        let ret_tys: Vec<Type> = match &body.signature.ret {
            None => Vec::new(),
            Some(class) => {
                if let Some(elem_tys) = self.scalar_tuple_element_tys(class) {
                    elem_tys
                } else {
                    vec![self.ty_for_class(class)?]
                }
            }
        };
        let symbol = self.function_symbol(function.instance(self.db));
        let linkage = linkage_for_runtime(function.linkage(self.db));
        Ok(Signature::new(&symbol, linkage, &args, &ret_tys))
    }

    fn lower_bodies(&mut self) -> Result<(), LowerError> {
        for function in self.package.functions(self.db) {
            let instance = function.instance(self.db);
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
            WasmFunctionLowerer::new(self, body, func_ref)?.lower()?;
        }
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
            descriptors: &mut Vec<(u32, u32)>,
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
                    descriptors.push((pointer_offset, length_offset));
                    None
                }
            };
            if let Some(ty) = ty {
                leaves.push((base, ty));
            }
            Ok(())
        }

        let mut request = Vec::new();
        let mut response = Vec::new();
        let mut request_descriptors = Vec::new();
        let mut response_descriptors = Vec::new();
        flatten(&lane.request, 0, &mut request, &mut request_descriptors)?;
        flatten(
            &lane.response,
            0,
            &mut response,
            &mut response_descriptors,
        )?;
        // Input descriptors remain borrowed views into caller-owned memory.
        let _ = request_descriptors;
        if request.is_empty() || response.is_empty() {
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
        let request_tys = request.iter().map(|(_, ty)| *ty).collect::<Vec<_>>();
        let response_tys = response.iter().map(|(_, ty)| *ty).collect::<Vec<_>>();
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
        for (offset, ty) in request {
            let addr = if offset == 0 {
                request_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32(offset as i32));
                fb.insert_inst(Add::new(is, request_ptr, offset), Type::I32)
            };
            args.push(fb.insert_inst(Mload::new(is, addr, ty), ty));
        }
        let results = fb.insert_call_results(*callee, args);
        if results.len() != response.len() {
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
        let mut copied_pointers = HashMap::new();
        for (pointer_offset, length_offset) in response_descriptors {
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
            let destination = fb.insert_inst(MemAllocDynamic::new(is, length), Type::I32);

            // Returned descriptors are borrowed inside Fe. The canonical
            // wrapper establishes explicit host ownership by copying exactly
            // `len` bytes into its arena before publishing the descriptor.
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
            let more = fb.insert_inst(Lt::new(is, index, length), Type::I1);
            fb.insert_inst_no_result(Br::new(is, more, copy_body, copy_done));

            fb.switch_to_block(copy_body);
            let source_byte = fb.insert_inst(Add::new(is, source, index), Type::I32);
            let destination_byte =
                fb.insert_inst(Add::new(is, destination, index), Type::I32);
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
        for ((offset, ty), value) in response.into_iter().zip(results) {
            let value = copied_pointers.get(&offset).copied().unwrap_or(value);
            let addr = if offset == 0 {
                response_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32(offset as i32));
                fb.insert_inst(Add::new(is, response_ptr, offset), Type::I32)
            };
            fb.insert_inst_no_result(Mstore::new(is, addr, value, ty));
        }
        fb.insert_return(response_ptr);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    /// The Sonatina `Type` for a runtime class. R1 covered the scalar envelope
    /// (bool, u8..u64 / i8..i64); R3.4b adds the FIRST per-backend ABI facts: a
    /// memory-space provider reference is the backend pointer word (i32), and a
    /// single-scalar-field aggregate is its one field's scalar. Wider scalars
    /// (u128/u256/address), non-memory provider refs, object/const refs, raw
    /// addresses, and multi-field / empty aggregates all fail closed.
    fn ty_for_class(&self, class: &RuntimeClass<'db>) -> Result<Type, LowerError> {
        match class {
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
            // field's scalar. Multi-field and empty aggregates fail closed.
            RuntimeClass::AggregateValue { layout } => {
                let scalar = self.single_scalar_field(*layout).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target (R3.4b) supports only single-scalar-field aggregates; \
                         `{class:?}` is not a one-field scalar newtype"
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
    /// empty, array, and enum layouts stay fail-closed). This is what lets the
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

    fn flat_shape(&self, class: &RuntimeClass<'db>) -> Option<FlatShape> {
        self.flat_shape_visit(class, &mut HashSet::new())
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
                let Layout::Struct(struct_layout) = layout.data(self.db) else {
                    return None;
                };
                let fields = struct_layout
                    .fields
                    .iter()
                    .map(|field| self.flat_shape_visit(field, active))
                    .collect::<Option<Vec<_>>>()?;
                active.remove(layout);
                // Unit structs contribute zero leaves but remain a valid node in
                // a closed product tree. This matters for recursively encoded
                // products such as `Cell<Cell<Nil>>`: rejecting `Nil` here makes
                // the otherwise scalar-only parent tree fail flattening.
                Some(FlatShape::Struct(fields))
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

    /// The recursively flattened scalar leaves of a nontrivial struct tree, or
    /// `None` for scalars, the existing direct one-word-newtype path,
    /// arrays/enums, refs, and unsupported leaves. Unit structs flatten to zero
    /// leaves, as required for terminal `Nil` products.
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
        );
        (matches!(class, RuntimeClass::AggregateValue { .. }) && !preserves_scalar_newtype_path)
            .then_some(leaves)
    }
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

struct WasmFunctionLowerer<'ctx, 'db, 'a> {
    module: &'ctx mut WasmModuleLowerer<'db, 'a>,
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
}

impl<'ctx, 'db, 'a> WasmFunctionLowerer<'ctx, 'db, 'a> {
    fn new(
        module: &'ctx mut WasmModuleLowerer<'db, 'a>,
        body: RuntimeBody<'db>,
        func_ref: FuncRef,
    ) -> Result<Self, LowerError> {
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
        for (idx, local) in body.locals.iter().enumerate() {
            if let RuntimeCarrier::Value(class) = &local.carrier {
                let local_id = RLocalId::from_u32(idx as u32);
                if matches!(local.root, RuntimeLocalRoot::Slot(_)) {
                    // Conditional-value and multi-exit joins can materialize a
                    // primitive scalar through a MIR Slot even when every
                    // reached operation is a direct load/store of the whole
                    // scalar. Promote exactly that closed shape to an SSA var;
                    // projected/aggregate slots and aliasing operations remain
                    // fail-closed in expression/statement lowering.
                    if matches!(class, RuntimeClass::Scalar(_)) {
                        let ty = module.ty_for_class(class)?;
                        vars.insert(local_id, fb.declare_var(ty));
                    }
                    continue;
                }
                if let Some(elem_tys) = module.scalar_tuple_element_tys(class) {
                    let elem_vars = elem_tys
                        .iter()
                        .map(|ty| fb.declare_var(*ty))
                        .collect::<Vec<_>>();
                    tuple_vars.insert(local_id, elem_vars);
                } else {
                    let ty = module.ty_for_class(class)?;
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
        })
    }

    fn inst_set(&self) -> &'static sonatina_ir::inst::native::inst_set::NativeInstSet {
        self.module.isa.inst_set()
    }

    fn lower(mut self) -> Result<(), LowerError> {
        let is = self.inst_set();

        // Prologue: bind incoming argument values to their parameter locals,
        // then jump to the MIR entry block (block 0). R2.1: a scalar-tuple param
        // was flattened into N wasm args, so we walk a RUNNING wasm-arg index and
        // bind those N args to the param's N element variables. For every other
        // param this is one arg to one variable, identical to before.
        self.fb.switch_to_block(self.prologue_block);
        let params = self.body.signature.params.clone();
        let arg_values: Vec<ValueId> = self.fb.args().to_vec();
        let mut wasm_arg_idx = 0usize;
        for param in params.iter() {
            if let Some(elem_vars) = self.tuple_vars.get(&param.local).cloned() {
                for elem_var in elem_vars {
                    self.fb.def_var(elem_var, arg_values[wasm_arg_idx]);
                    wasm_arg_idx += 1;
                }
            } else {
                let var = self.var_for(param.local)?;
                self.fb.def_var(var, arg_values[wasm_arg_idx]);
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
                // R2.1: a scalar-tuple destination is produced element-wise (one
                // SSA def per element word), not as a single value, so it takes a
                // dedicated arm rather than the single-`ValueId` `lower_expr` path.
                if self.tuple_vars.contains_key(dst) {
                    return self.lower_tuple_assign(*dst, expr);
                }
                let value = self.lower_expr(expr, *dst).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; while lowering assignment destination {dst:?}, expression {expr:?}"
                    )),
                    other => other,
                })?;
                let var = self.var_for(*dst).map_err(|error| match error {
                    LowerError::Unsupported(message) => LowerError::Unsupported(format!(
                        "{message}; assignment destination {dst:?}, expression {expr:?}"
                    )),
                    other => other,
                })?;
                self.fb.def_var(var, value);
                Ok(())
            }
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
                let var = self.var_for(local)?;
                self.fb.def_var(var, value);
                Ok(())
            }
            RStmt::Store { dst, src } => {
                if let Some((addr, ty)) = self.raw_memory_scalar_place(dst)? {
                    let value = self.local_value(*src)?;
                    self.fb
                        .insert_inst_no_result(Mstore::new(self.inst_set(), addr, value, ty));
                    return Ok(());
                }
                Err(LowerError::Unsupported(format!(
                    "wasm target (R1) statement `{stmt:?}` is not supported"
                )))
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
        let is = self.inst_set();
        let callee_ref =
            *self.module.func_map.get(&callee).ok_or_else(|| {
                LowerError::Internal("wasm call target was not declared".to_string())
            })?;
        let arg_vals = self.call_arg_values(args)?;
        self.fb
            .insert_inst_no_result(Call::new(is, callee_ref, arg_vals.into_iter().collect()));
        Ok(())
    }

    /// Flatten value-carried struct-tree arguments in the same DFS field order
    /// used by `lower_signature` and the function prologue. Scalar arguments
    /// remain one wasm value. Arrays, enums, and place-backed aggregates never
    /// acquire tuple variables and therefore continue to fail closed here.
    fn call_arg_values(&mut self, args: &[RLocalId]) -> Result<Vec<ValueId>, LowerError> {
        let mut values = Vec::new();
        for arg in args {
            values.extend(self.local_flat_values(*arg)?);
        }
        Ok(values)
    }

    fn local_flat_shape(&self, local: RLocalId) -> Result<FlatShape, LowerError> {
        let class = self.body.value_class(local).ok_or_else(|| {
            LowerError::Internal(format!("flattened local {local:?} has no runtime class"))
        })?;
        self.module.flat_shape(class).ok_or_else(|| {
            LowerError::Unsupported(format!(
                "wasm target (R2.2): `{class:?}` is not a recursive struct tree of wasm scalars"
            ))
        })
    }

    /// Snapshot a local's leaves in DFS declaration order before callers write
    /// any destination variables.
    fn local_flat_values(&mut self, local: RLocalId) -> Result<Vec<ValueId>, LowerError> {
        if let Some(vars) = self.tuple_vars.get(&local).cloned() {
            return Ok(vars.iter().map(|var| self.fb.use_var(*var)).collect());
        }
        Ok(vec![self.local_value(local)?])
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
            RExpr::AggregateMake { fields, .. } => {
                let elem_vars = self.tuple_vars.get(&dst).cloned().ok_or_else(|| {
                    LowerError::Internal(format!("R2.1 tuple dst {dst:?} has no element vars"))
                })?;
                let dst_shape = self.local_flat_shape(dst)?;
                let FlatShape::Struct(expected_fields) = dst_shape else {
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
                let Layout::Struct(dst_layout) = layout.data(self.module.db) else {
                    return Err(LowerError::Unsupported(
                        "wasm target (R2.2): arrays/enums cannot be flattened".to_string(),
                    ));
                };
                let expected_classes = dst_layout.fields.to_vec();
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
                let values = self.local_flat_values(*src)?;
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
                let Layout::Struct(source_layout) = layout.data(self.module.db) else {
                    return Err(LowerError::Unsupported(
                        "wasm target (R2.2): arrays/enums cannot be projected".to_string(),
                    ));
                };
                let field_class = source_layout
                    .fields
                    .get(*index as usize)
                    .cloned()
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
            RExpr::Call { callee, args } => {
                let callee_body = callee.body(self.module.db);
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
                let arg_vals = self.call_arg_values(args)?;
                let results = self
                    .fb
                    .insert_call_results(callee_ref, arg_vals.into_iter().collect());
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
            RExpr::Use(src) => self.local_value(*src),
            RExpr::ConstScalar(constant) => {
                let ty = self.local_ty(dst)?;
                let imm = immediate_for_const_scalar(constant, ty)?;
                Ok(self.fb.make_imm_value(imm))
            }
            RExpr::Binary { op, lhs, rhs } => self.lower_binary(*op, *lhs, *rhs, dst),
            RExpr::Unary { op, value } => self.lower_unary(*op, *value),
            RExpr::Cast { value, to } => {
                let source_ty = self.local_ty(*value)?;
                let target_ty = scalar_ty_r1(to)?;
                if source_ty != target_ty {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target: non-identity scalar cast `{source_ty:?}` -> `{target_ty:?}` must lower through a dedicated numeric builtin"
                    )));
                }
                self.local_value(*value)
            }
            RExpr::Call { callee, args } => self.lower_call(*callee, args),
            RExpr::Builtin(builtin) => self.lower_builtin(builtin),
            // R3.4b step 2: single-scalar-field newtype construction/projection is
            // a no-op on the represented word. `AggregateMake` of one field yields
            // that field's value; `AggregateExtract` at index 0 yields the
            // aggregate's value (which IS the field's word). This is what executes
            // the `u32` newtypes (`Pending<T>` / `KernelId` / `WebGpuRef`).
            RExpr::AggregateMake { layout, fields } => {
                self.lower_scalar_newtype_make(*layout, fields)
            }
            RExpr::AggregateExtract { value, index } => {
                if self.tuple_vars.contains_key(value) {
                    let source_class = self.body.value_class(*value).ok_or_else(|| {
                        LowerError::Internal(format!("aggregate source {value:?} has no class"))
                    })?;
                    let RuntimeClass::AggregateValue { layout } = source_class else {
                        return Err(LowerError::Unsupported(
                            "wasm target (R2.2): scalar extract source is not a struct".to_string(),
                        ));
                    };
                    let Layout::Struct(source_layout) = layout.data(self.module.db) else {
                        return Err(LowerError::Unsupported(
                            "wasm target (R2.2): arrays/enums cannot be projected".to_string(),
                        ));
                    };
                    let field_class = source_layout
                        .fields
                        .get(*index as usize)
                        .cloned()
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
            // R2.0 (Fable seat ruling, control-effects ladder section 7): the only
            // place read the wasm target lowers is an IDENTITY on an already
            // value-carried transport word. Own-mode consumption of a word-carried
            // token (`Wait::wait<T>(_ pending: own Pending<T>)`) reaches lowering as
            // exactly this shape (`load *%p`); anything needing an address, an offset,
            // a store, or an object materialization is R2 proper and stays fail-closed.
            RExpr::Load { place } => self.lower_place_read(place),
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) expression `{other:?}` is not supported"
            ))),
        }
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
    fn lower_place_read(&mut self, place: &RuntimePlace<'db>) -> Result<ValueId, LowerError> {
        if let Some((addr, ty)) = self.raw_memory_scalar_place(place)? {
            return Ok(self
                .fb
                .insert_inst(Mload::new(self.inst_set(), addr, ty), ty));
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

    /// Resolve a Wasm linear-memory scalar place behind a memory `RawAddr`.
    /// Raw addresses are byte offsets carried
    /// as `i32` on wasm32, not Sonatina compound pointers. Consequently field or
    /// index address arithmetic uses checked/ordinary `i32` Add/Mul rather than
    /// `Gep`. Struct field offsets come exclusively from MIR's target-layout
    /// SSOT. Dynamic indexes, variants, and dereferences remain fail-closed.
    fn raw_memory_scalar_place(
        &mut self,
        place: &RuntimePlace<'db>,
    ) -> Result<Option<(ValueId, Type)>, LowerError> {
        let program = self.module.db as &dyn mir::MirDb;
        let resolved = mir::resolve_runtime_place(self.module.db, &program, &self.body, place)
            .map_err(|error| LowerError::Internal(format!("invalid runtime place: {error:?}")))?;
        let RuntimeClass::Scalar(scalar) = resolved.result_class.clone() else {
            return Ok(None);
        };
        let (addr_local, mut current_class) = match resolved.root_kind {
            mir::ResolvedPlaceRootKind::Ref { value, class }
                if matches!(
                    self.body.value_class(value),
                    Some(RuntimeClass::RawAddr {
                        space: AddressSpaceKind::Memory,
                        ..
                    })
                ) =>
            {
                (value, class)
            }
            mir::ResolvedPlaceRootKind::Provider {
                value,
                provider_class:
                    RuntimeClass::RawAddr {
                        space: AddressSpaceKind::Memory,
                        ..
                    },
                class,
                ..
            } => (value, class),
            mir::ResolvedPlaceRootKind::Ptr {
                addr,
                space: AddressSpaceKind::Memory,
                class,
            } => (addr, class),
            _ => return Ok(None),
        };
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
                                "wasm RawAddr struct field byte offset overflow".to_string(),
                            )
                        })?;
                    current_class = class;
                }
                other => {
                    return Err(LowerError::Unsupported(format!(
                        "wasm RawAddr scalar place projection `{other:?}` is not supported; \
                         only struct fields have target-layout byte-offset lowering"
                    )));
                }
            }
        }
        let mut addr = self.local_value(addr_local)?;
        if byte_offset != 0 {
            let offset = i32::try_from(byte_offset).map_err(|_| {
                LowerError::Unsupported(format!(
                    "wasm RawAddr struct field offset {byte_offset} exceeds i32"
                ))
            })?;
            let offset = self.fb.make_imm_value(Immediate::I32(offset));
            addr = self
                .fb
                .insert_inst(Add::new(self.inst_set(), addr, offset), Type::I32);
        }
        Ok(Some((
            addr,
            scalar_ty_r1(&scalar)?,
        )))
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
    fn lower_builtin(&mut self, builtin: &RuntimeBuiltin<'db>) -> Result<ValueId, LowerError> {
        match builtin {
            RuntimeBuiltin::IntTruncate { value, from, to } => {
                let is = self.inst_set();
                let value = self.local_value(*value)?;
                let target = scalar_ty_r1(to)?;
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
                    let signed = matches!(
                        from.repr,
                        ScalarRepr::Int { signed: true, .. }
                    );
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
                ..
            } => self.lower_intrinsic_arith(*op, *lhs, *rhs, class),
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
        let is = self.inst_set();
        let ty = scalar_ty_r1(class)?;
        let lhs = self.local_value(lhs)?;
        let rhs = self.local_value(rhs)?;
        Ok(match op {
            IntrinsicArithBinOp::Add => self.fb.insert_inst(Add::new(is, lhs, rhs), ty),
            IntrinsicArithBinOp::Sub => self.fb.insert_inst(Sub::new(is, lhs, rhs), ty),
            IntrinsicArithBinOp::Mul => self.fb.insert_inst(Mul::new(is, lhs, rhs), ty),
            other => {
                return Err(LowerError::Unsupported(format!(
                    "wasm target (R1) intrinsic arithmetic `{other:?}` is not supported \
                     (div/rem/pow are R2)"
                )));
            }
        })
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
        let is = self.inst_set();
        // Keep the MIR operand ids for the signedness key (the value class lives on
        // the RLocalId, not the sonatina ValueId, which is signless). The sonatina
        // ValueIds shadow below for the instruction constructors.
        let (lhs_local, rhs_local) = (lhs, rhs);
        let lhs = self.local_value(lhs)?;
        let rhs = self.local_value(rhs)?;
        if float_operands {
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
                Ok(match arith {
                    ArithBinOp::Add => self.fb.insert_inst(Add::new(is, lhs, rhs), ty),
                    ArithBinOp::Sub => self.fb.insert_inst(Sub::new(is, lhs, rhs), ty),
                    ArithBinOp::Mul => self.fb.insert_inst(Mul::new(is, lhs, rhs), ty),
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
                        if self.operand_signedness(lhs_local, rhs_local)? {
                            self.fb.insert_inst(Sar::new(is, rhs, lhs), ty)
                        } else {
                            self.fb.insert_inst(Shr::new(is, rhs, lhs), ty)
                        }
                    }
                    other => {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target (R1) arithmetic op `{other:?}` is not supported \
                             (div/rem/pow/`<<`/bitwise/unsigned `>>` are R2)"
                        )));
                    }
                })
            }
            BinOp::Comp(comp) => Ok(match comp {
                CompBinOp::Lt => {
                    // Sign-aware (M2): i32 -> Slt, u32 -> Lt. Signedness comes from
                    // the operand CLASS, not the sonatina type (signless).
                    if self.operand_signedness(lhs_local, rhs_local)? {
                        self.fb.insert_inst(Slt::new(is, lhs, rhs), Type::I1)
                    } else {
                        self.fb.insert_inst(Lt::new(is, lhs, rhs), Type::I1)
                    }
                }
                CompBinOp::Eq => self.fb.insert_inst(CmpEq::new(is, lhs, rhs), Type::I1),
                other => {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target (R1) comparison `{other:?}` is not supported \
                         (only `<` and `==`; LtEq/Gt/GtEq need an IsZero lowering the SPIR-V \
                         translator does not map yet, so the full compare matrix is R2)"
                    )));
                }
            }),
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) binary op `{other:?}` is not supported"
            ))),
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
        Err(LowerError::Unsupported(format!(
            "wasm target (R1) unary op `{op:?}` is not supported"
        )))
    }

    fn lower_call(
        &mut self,
        callee: RuntimeInstance<'db>,
        args: &[RLocalId],
    ) -> Result<ValueId, LowerError> {
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
                    | "__floor_f32"
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
        let ret_class = callee.body(self.module.db).signature.ret.clone();
        let ret_ty = match ret_class {
            Some(class) => self.module.ty_for_class(&class)?,
            None => {
                return Err(LowerError::Unsupported(
                    "wasm target (R1) does not support calling a unit-returning function \
                     as a value expression"
                        .to_string(),
                ));
            }
        };
        let arg_vals = self.call_arg_values(args)?;
        Ok(self.fb.insert_inst(
            Call::new(is, callee_ref, arg_vals.into_iter().collect()),
            ret_ty,
        ))
    }

    fn lower_terminator(&mut self, terminator: &RTerminator<'db>) -> Result<(), LowerError> {
        let is = self.inst_set();
        match terminator {
            RTerminator::Return(Some(value)) => {
                // R2.1: returning a flattened scalar tuple is a wasm MULTI-VALUE
                // return of its element words (the host reads the N results). Every
                // other return is the single-value form.
                if let Some(elem_vars) = self.tuple_vars.get(value).cloned() {
                    let values: Vec<ValueId> =
                        elem_vars.iter().map(|var| self.fb.use_var(*var)).collect();
                    self.fb.insert_return_values(&values);
                } else {
                    let value = self.local_value(*value)?;
                    self.fb.insert_inst_no_result(Return::new_single(is, value));
                }
            }
            // A unit return and a `Stop` (the synthetic main-root exit) both
            // become a plain wasm return.
            RTerminator::Return(None) | RTerminator::Stop => {
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

    fn local_value(&mut self, local: RLocalId) -> Result<ValueId, LowerError> {
        let var = self.var_for(local)?;
        Ok(self.fb.use_var(var))
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
        self.module.ty_for_class(&class)
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
    use mir::ScalarRole;
    use url::Url;

    #[test]
    fn aggregate_param_reification_excludes_mutable_object_refs() {
        assert!(is_reifiable_aggregate_ref(&RefKind::Const));
        assert!(!is_reifiable_aggregate_ref(&RefKind::Object));
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
}
