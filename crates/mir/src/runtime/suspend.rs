//! Compiler-derived suspension sites and live-state plans.
//!
//! This pass is deliberately target-neutral. It recognizes the nominal Fe
//! control operation, assigns deterministic continuation states, and computes
//! the exact runtime locals live across each boundary. Wasm materialization is
//! a later consumer; no manifest, export-name list, or host-authored lane table
//! participates in this analysis.

use std::collections::BTreeSet;

use cranelift_entity::EntityRef;
use hir::projection::IndexSource;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{MirDb, RuntimeControlEffectFuncKind, RuntimeInstance, runtime_control_effect_kind};

use super::{
    EnumLayoutKey, EnumVariantLayout, LayoutId, LayoutKey, PlaceElem, PlaceRoot, RBlock, RBlockId,
    RExpr, RLocal, RLocalId, RStmt, RTerminator, RuntimeBody, RuntimeBuiltin, RuntimeCarrier,
    RuntimeClass, RuntimeLocalRoot, RuntimePackage, RuntimeParam, RuntimePlace, VariantId,
};

const RESUMABLE_FLATTEN_STMT_BUDGET: usize = 65_536;
const RESUMABLE_FLATTEN_BLOCK_BUDGET: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSuspensionCause<'db> {
    /// The typed control operation which hands one pending token to the
    /// executor. This is the deepest frame in a suspended call chain.
    Effect { pending: RLocalId },
    /// An ordinary Fe call whose callee can suspend. The caller retains its own
    /// live frame while the callee owns the pending token and nested frame.
    Callee { callee: RuntimeInstance<'db> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspensionPoint<'db> {
    /// Compiler-owned state zero is the initial entry; suspension points begin
    /// at one in stable block/statement order.
    pub continuation_state: u32,
    pub block: RBlockId,
    pub statement: u32,
    pub cause: RuntimeSuspensionCause<'db>,
    /// The call destination populated by typed re-entry.
    pub delivery: RLocalId,
    /// Runtime values which must survive while the stack is absent. The
    /// delivery local is intentionally excluded because re-entry creates it.
    pub live_values: Box<[RLocalId]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendingTail<'db> {
    pub block: RBlockId,
    pub callee: RuntimeInstance<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeContinuationFrameSlot<'db> {
    pub local: RLocalId,
    pub class: RuntimeClass<'db>,
    /// Retain whether the live word is a direct value, slot, reference, or raw
    /// pointer carrier. Re-entry must reconstruct the same MIR root semantics,
    /// not merely a byte-compatible scalar.
    pub root: RuntimeLocalRoot<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeResumableBodyPlan<'db> {
    pub body: RuntimeInstance<'db>,
    /// Backend-independent body after non-recursive resumable Fe calls have
    /// been expanded into this body's CFG. The authored instance remains the
    /// identity and public ABI; this private compiler form makes every
    /// suspension leaf direct before liveness and state splitting.
    pub flattened_body: RuntimeBody<'db>,
    /// Union of the body's live values in stable local-id order. Individual
    /// point masks are the `live_values` lists; the materializer allocates this
    /// frame once and stores only the slots live at the selected state.
    pub frame: Box<[RuntimeContinuationFrameSlot<'db>]>,
    pub points: Box<[RuntimeSuspensionPoint<'db>]>,
    /// Tail calls need no additional caller frame, but remain explicit so the
    /// materializer forwards their terminal/suspended state.
    pub suspending_tails: Box<[RuntimeSuspendingTail<'db>]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSuspensionPlanError {
    InvalidSuspendArity {
        block: RBlockId,
        statement: u32,
        actual: usize,
    },
    LiveValueHasNoRuntimeClass {
        local: RLocalId,
    },
}

/// One executable target-neutral state-machine body. State zero is the normal
/// authored entry. Every nonzero segment receives exactly the frame values
/// live at its suspension point plus the typed delivery local populated on
/// re-entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeContinuationSegment<'db> {
    pub continuation_state: u32,
    pub body: RuntimeBody<'db>,
}

/// Compiler-created payload-enum protocol between continuation segments and a
/// backend materializer. Variant zero is `Complete`; variant N is the Nth
/// suspension point and carries its pending token followed by that point's
/// exact live frame. This is internal MIR, not a host manifest or authoring
/// surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeResumableMachine<'db> {
    pub source: RuntimeInstance<'db>,
    pub step_layout: LayoutId<'db>,
    pub entry: RuntimeContinuationSegment<'db>,
    pub continuations: Box<[RuntimeContinuationSegment<'db>]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSuspensionMaterializeError<'db> {
    CalleeFrameRequired {
        body: RuntimeInstance<'db>,
        callee: RuntimeInstance<'db>,
    },
    SuspendingTailFrameRequired {
        body: RuntimeInstance<'db>,
        callee: RuntimeInstance<'db>,
    },
    MissingRuntimeClass {
        local: RLocalId,
    },
    UnsupportedTerminalCall {
        block: RBlockId,
        callee: RuntimeInstance<'db>,
    },
}

/// Derive the direct suspension points in one lowered Fe body. Transitive call
/// propagation and frame materialization consume this result in the next pass;
/// this function already owns site identity and liveness, the two facts which
/// must never be duplicated by a host-side description.
pub fn derive_runtime_suspension_points<'db>(
    db: &'db dyn MirDb,
    body: &RuntimeBody<'db>,
) -> Result<Vec<RuntimeSuspensionPoint<'db>>, RuntimeSuspensionPlanError> {
    let mut sites = Vec::new();
    for (block_index, block) in body.blocks.iter().enumerate() {
        for (statement_index, statement) in block.stmts.iter().enumerate() {
            let RStmt::Assign {
                dst,
                expr: RExpr::Call { callee, args },
            } = statement
            else {
                continue;
            };
            if runtime_control_effect_kind(db, *callee)
                != Some(RuntimeControlEffectFuncKind::Suspend)
            {
                continue;
            }
            let [pending] = args.as_ref() else {
                return Err(RuntimeSuspensionPlanError::InvalidSuspendArity {
                    block: RBlockId::new(block_index),
                    statement: statement_index as u32,
                    actual: args.len(),
                });
            };
            sites.push((
                RBlockId::new(block_index),
                statement_index,
                RuntimeSuspensionCause::Effect { pending: *pending },
                *dst,
            ));
        }
    }

    let positions = sites
        .iter()
        .map(|(block, statement, _, _)| (*block, *statement))
        .collect::<BTreeSet<_>>();
    let live = live_values_across_sites(&body.blocks, &body.provider_bindings, &positions);
    Ok(sites
        .into_iter()
        .enumerate()
        .map(|(index, (block, statement, cause, delivery))| {
            let mut live_values = live.get(&(block, statement)).cloned().unwrap_or_default();
            live_values.remove(&delivery);
            RuntimeSuspensionPoint {
                continuation_state: index as u32 + 1,
                block,
                statement: statement as u32,
                cause,
                delivery,
                live_values: live_values.into_iter().collect(),
            }
        })
        .collect())
}

/// Derive the complete resumable call graph for one runtime package. A body is
/// resumable when it directly performs the nominal suspend operation or calls
/// another resumable body. This fixed point lets normal Fe effect providers
/// and helpers remain ordinary functions: authors do not repeat a `resumable`
/// annotation up the stack.
pub fn derive_runtime_resumable_plans<'db>(
    db: &'db dyn MirDb,
    package: RuntimePackage<'db>,
) -> Result<Vec<RuntimeResumableBodyPlan<'db>>, RuntimeSuspensionPlanError> {
    let bodies = package
        .functions(db)
        .iter()
        .map(|function| (function.instance(db), function.instance(db).body(db)))
        .filter(|(_, body)| !body.blocks.is_empty())
        .collect::<Vec<_>>();

    let mut resumable = bodies
        .iter()
        .filter_map(|(instance, body)| body_directly_suspends(db, body).then_some(*instance))
        .collect::<FxHashSet<_>>();
    loop {
        let mut changed = false;
        for (instance, body) in &bodies {
            if resumable.contains(instance) || !body_calls_any(body, &resumable) {
                continue;
            }
            resumable.insert(*instance);
            changed = true;
        }
        if !changed {
            break;
        }
    }

    let body_map = bodies.iter().cloned().collect::<FxHashMap<_, _>>();
    let recursive = resumable
        .iter()
        .copied()
        .filter(|instance| {
            resumable_reaches(
                *instance,
                *instance,
                &body_map,
                &resumable,
                &mut FxHashSet::default(),
            )
        })
        .collect::<FxHashSet<_>>();
    let mut flattened_cache = FxHashMap::default();
    let mut plans = Vec::new();
    for (instance, _) in bodies {
        if !resumable.contains(&instance) {
            continue;
        }
        let mut visiting = FxHashSet::default();
        let body = flatten_resumable_body(
            instance,
            &body_map,
            &resumable,
            &recursive,
            &mut visiting,
            &mut flattened_cache,
        );
        let mut sites = Vec::new();
        let mut tails = Vec::new();
        for (block_index, block) in body.blocks.iter().enumerate() {
            let block_id = RBlockId::new(block_index);
            for (statement_index, statement) in block.stmts.iter().enumerate() {
                let RStmt::Assign {
                    dst,
                    expr: RExpr::Call { callee, args },
                } = statement
                else {
                    continue;
                };
                let cause = if runtime_control_effect_kind(db, *callee)
                    == Some(RuntimeControlEffectFuncKind::Suspend)
                {
                    let [pending] = args.as_ref() else {
                        return Err(RuntimeSuspensionPlanError::InvalidSuspendArity {
                            block: block_id,
                            statement: statement_index as u32,
                            actual: args.len(),
                        });
                    };
                    Some(RuntimeSuspensionCause::Effect { pending: *pending })
                } else {
                    resumable
                        .contains(callee)
                        .then_some(RuntimeSuspensionCause::Callee { callee: *callee })
                };
                if let Some(cause) = cause {
                    sites.push((block_id, statement_index, cause, *dst));
                }
            }
            if let RTerminator::TerminalCall { callee, .. } = block.terminator
                && (resumable.contains(&callee)
                    || runtime_control_effect_kind(db, callee)
                        == Some(RuntimeControlEffectFuncKind::Suspend))
            {
                tails.push(RuntimeSuspendingTail {
                    block: block_id,
                    callee,
                });
            }
        }
        let positions = sites
            .iter()
            .map(|(block, statement, _, _)| (*block, *statement))
            .collect::<BTreeSet<_>>();
        let live = live_values_across_sites(&body.blocks, &body.provider_bindings, &positions);
        let points = sites
            .into_iter()
            .enumerate()
            .map(|(index, (block, statement, cause, delivery))| {
                let mut live_values = live.get(&(block, statement)).cloned().unwrap_or_default();
                live_values.remove(&delivery);
                RuntimeSuspensionPoint {
                    continuation_state: index as u32 + 1,
                    block,
                    statement: statement as u32,
                    cause,
                    delivery,
                    live_values: live_values.into_iter().collect(),
                }
            })
            .collect::<Box<[_]>>();
        let frame = continuation_frame(&body, &points)?;
        plans.push(RuntimeResumableBodyPlan {
            body: instance,
            flattened_body: body,
            frame,
            points,
            suspending_tails: tails.into_boxed_slice(),
        });
    }
    Ok(plans)
}

/// Recursively inline every non-recursive call into another resumable Fe body.
///
/// This is deliberately a CFG transform, not the Wasm backend's scalar
/// `#[inline(always)]` optimization. Suspension semantics may cross ordinary
/// helpers, branches, places, and effect-provider methods regardless of
/// optimization hints. A recursive edge is retained so the later materializer
/// rejects the genuinely linked/recursive stack explicitly.
fn flatten_resumable_body<'db>(
    instance: RuntimeInstance<'db>,
    bodies: &FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
    resumable: &FxHashSet<RuntimeInstance<'db>>,
    recursive: &FxHashSet<RuntimeInstance<'db>>,
    visiting: &mut FxHashSet<RuntimeInstance<'db>>,
    cache: &mut FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
) -> RuntimeBody<'db> {
    if let Some(body) = cache.get(&instance) {
        return body.clone();
    }
    let mut body = bodies
        .get(&instance)
        .cloned()
        .expect("every resumable instance was collected with a runtime body");
    if !visiting.insert(instance) {
        return body;
    }

    let mut refused = BTreeSet::new();
    loop {
        let site = body.blocks.iter().enumerate().find_map(|(block, data)| {
            data.stmts.iter().enumerate().find_map(|(statement, stmt)| {
                let RStmt::Assign {
                    expr: RExpr::Call { callee, .. },
                    ..
                } = stmt
                else {
                    return None;
                };
                (resumable.contains(callee)
                    && !recursive.contains(callee)
                    && !visiting.contains(callee)
                    && !refused.contains(&(block, statement)))
                .then_some((RBlockId::new(block), statement, *callee))
            })
        });
        let Some((block, statement, callee)) = site else {
            break;
        };
        let callee_body =
            flatten_resumable_body(callee, bodies, resumable, recursive, visiting, cache);
        if inline_resumable_call(&mut body, block, statement, &callee_body) {
            refused.clear();
        } else {
            refused.insert((block.index(), statement));
        }
    }

    visiting.remove(&instance);
    cache.insert(instance, body.clone());
    body
}

fn resumable_reaches<'db>(
    target: RuntimeInstance<'db>,
    current: RuntimeInstance<'db>,
    bodies: &FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
    resumable: &FxHashSet<RuntimeInstance<'db>>,
    seen: &mut FxHashSet<RuntimeInstance<'db>>,
) -> bool {
    if !seen.insert(current) {
        return false;
    }
    bodies.get(&current).is_some_and(|body| {
        body_callees(body).into_iter().any(|callee| {
            resumable.contains(&callee)
                && (callee == target || resumable_reaches(target, callee, bodies, resumable, seen))
        })
    })
}

fn body_callees<'db>(body: &RuntimeBody<'db>) -> Vec<RuntimeInstance<'db>> {
    body.blocks
        .iter()
        .flat_map(|block| {
            block
                .stmts
                .iter()
                .filter_map(|stmt| match stmt {
                    RStmt::Assign {
                        expr: RExpr::Call { callee, .. },
                        ..
                    } => Some(*callee),
                    _ => None,
                })
                .chain(match block.terminator {
                    RTerminator::TerminalCall { callee, .. } => Some(callee),
                    _ => None,
                })
        })
        .collect()
}

/// Replace one value call with a cloned callee CFG and a continuation block.
/// Existing caller block ids never move; appended callee blocks remap their
/// internal edges by offset. Callee parameters receive ordinary value copies,
/// and every return writes the original call destination before continuing.
fn inline_resumable_call<'db>(
    caller: &mut RuntimeBody<'db>,
    block: RBlockId,
    statement: usize,
    callee: &RuntimeBody<'db>,
) -> bool {
    let Some(call_block) = caller.blocks.get(block.index()) else {
        return false;
    };
    let Some(RStmt::Assign {
        dst,
        expr: RExpr::Call { args, .. },
    }) = call_block.stmts.get(statement)
    else {
        return false;
    };
    if callee.blocks.is_empty() || callee.signature.params.len() != args.len() {
        return false;
    }
    let caller_statements = caller
        .blocks
        .iter()
        .map(|block| block.stmts.len())
        .sum::<usize>();
    let callee_statements = callee
        .blocks
        .iter()
        .map(|block| block.stmts.len())
        .sum::<usize>();
    if caller_statements
        .checked_add(callee_statements)
        .and_then(|count| count.checked_add(callee.signature.params.len()))
        .is_none_or(|count| count > RESUMABLE_FLATTEN_STMT_BUDGET)
        || caller
            .blocks
            .len()
            .checked_add(callee.blocks.len())
            .and_then(|count| count.checked_add(1))
            .is_none_or(|count| count > RESUMABLE_FLATTEN_BLOCK_BUDGET)
        || caller
            .locals
            .len()
            .checked_add(callee.locals.len())
            .is_none_or(|count| count > u32::MAX as usize)
    {
        // Leave this call visible. The materializer reports its ordinary
        // CalleeFrameRequired boundary instead of risking compiler resource
        // exhaustion or silently changing suspension semantics.
        return false;
    }
    let dst = *dst;
    let args = args.clone();
    let original = call_block.clone();
    let local_base = caller.locals.len();
    let provider_base = caller.provider_bindings.len();
    let block_base = caller.blocks.len();
    let continuation = RBlockId::new(block_base + callee.blocks.len());

    caller.locals.extend(callee.locals.iter().cloned());
    caller
        .provider_bindings
        .extend(callee.provider_bindings.iter().cloned().map(|mut binding| {
            binding.value = offset_local(binding.value, local_base);
            binding
        }));

    let mut cloned_blocks = callee
        .blocks
        .iter()
        .map(|source| {
            let mut cloned = RBlock {
                stmts: source
                    .stmts
                    .iter()
                    .map(|stmt| remap_stmt(stmt, local_base, provider_base))
                    .collect(),
                terminator: remap_terminator(&source.terminator, local_base, block_base),
            };
            match source.terminator {
                RTerminator::Return(value) => {
                    if let Some(value) = value {
                        cloned.stmts.push(RStmt::Assign {
                            dst,
                            expr: RExpr::Use(offset_local(value, local_base)),
                        });
                    }
                    cloned.terminator = RTerminator::Goto(continuation);
                }
                _ => {}
            }
            cloned
        })
        .collect::<Vec<_>>();
    let entry = &mut cloned_blocks[0];
    let param_copies = callee
        .signature
        .params
        .iter()
        .zip(args.iter())
        .map(|(param, arg)| RStmt::Assign {
            dst: offset_local(param.local, local_base),
            expr: RExpr::Use(*arg),
        })
        .collect::<Vec<_>>();
    entry.stmts.splice(0..0, param_copies);

    caller.blocks[block.index()] = RBlock {
        stmts: original.stmts[..statement].to_vec(),
        terminator: RTerminator::Goto(RBlockId::new(block_base)),
    };
    caller.blocks.extend(cloned_blocks);
    caller.blocks.push(RBlock {
        stmts: original.stmts[statement + 1..].to_vec(),
        terminator: original.terminator,
    });
    true
}

fn offset_local(local: RLocalId, base: usize) -> RLocalId {
    RLocalId::from_u32(base as u32 + local.as_u32())
}

fn offset_provider(
    provider: super::RuntimeProviderBindingId,
    base: usize,
) -> super::RuntimeProviderBindingId {
    super::RuntimeProviderBindingId::from_u32(base as u32 + provider.as_u32())
}

fn offset_block(block: RBlockId, base: usize) -> RBlockId {
    RBlockId::from_u32(base as u32 + block.as_u32())
}

fn remap_place<'db>(
    place: &RuntimePlace<'db>,
    local_base: usize,
    provider_base: usize,
) -> RuntimePlace<'db> {
    let mut place = place.clone();
    match &mut place.root {
        PlaceRoot::Slot(local) | PlaceRoot::Ref(local) => *local = offset_local(*local, local_base),
        PlaceRoot::Provider(provider) => *provider = offset_provider(*provider, provider_base),
        PlaceRoot::Ptr { addr, .. } => *addr = offset_local(*addr, local_base),
    }
    for element in &mut place.path {
        if let PlaceElem::Index(IndexSource::Dynamic(value)) = element {
            *value = offset_local(*value, local_base);
        }
    }
    place
}

fn remap_expr<'db>(expr: &RExpr<'db>, local_base: usize, provider_base: usize) -> RExpr<'db> {
    let mut expr = expr.clone();
    match &mut expr {
        RExpr::Use(value)
        | RExpr::Unary { value, .. }
        | RExpr::Cast { value, .. }
        | RExpr::Bitcast { value, .. }
        | RExpr::MaterializeToObject { src: value }
        | RExpr::ProviderToRaw { value }
        | RExpr::RetagRef { value }
        | RExpr::AggregateExtract { value, .. }
        | RExpr::EnumTagOfValue { value }
        | RExpr::EnumIsVariant { value, .. }
        | RExpr::EnumExtract { value, .. }
        | RExpr::EnumGetTag { root: value }
        | RExpr::EnumAssertVariantRef { root: value, .. } => {
            *value = offset_local(*value, local_base)
        }
        RExpr::Binary { lhs, rhs, .. } => {
            *lhs = offset_local(*lhs, local_base);
            *rhs = offset_local(*rhs, local_base);
        }
        RExpr::ProviderFromRaw { raw, .. } => *raw = offset_local(*raw, local_base),
        RExpr::WordToRawAddr { value, .. } => *value = offset_local(*value, local_base),
        RExpr::MaterializePlaceToObject { place }
        | RExpr::AddrOf { place }
        | RExpr::Load { place } => *place = remap_place(place, local_base, provider_base),
        RExpr::AggregateMake { fields, .. }
        | RExpr::Call { args: fields, .. }
        | RExpr::EnumMake { fields, .. } => {
            for field in fields {
                *field = offset_local(*field, local_base);
            }
        }
        RExpr::Builtin(builtin) => remap_builtin(builtin, local_base),
        RExpr::ConstScalar(_)
        | RExpr::Placeholder { .. }
        | RExpr::ConstRef { .. }
        | RExpr::AllocObject { .. } => {}
    }
    expr
}

fn remap_stmt<'db>(stmt: &RStmt<'db>, local_base: usize, provider_base: usize) -> RStmt<'db> {
    let mut stmt = stmt.clone();
    match &mut stmt {
        RStmt::Assign { dst, expr } => {
            *dst = offset_local(*dst, local_base);
            *expr = remap_expr(expr, local_base, provider_base);
        }
        RStmt::EnumAssertVariant { value, .. } => *value = offset_local(*value, local_base),
        RStmt::Store { dst, src } | RStmt::CopyInto { dst, src } => {
            *dst = remap_place(dst, local_base, provider_base);
            *src = offset_local(*src, local_base);
        }
        RStmt::EnumSetTag { root, .. } => *root = offset_local(*root, local_base),
        RStmt::EnumWriteVariant { root, fields, .. } => {
            *root = offset_local(*root, local_base);
            for field in fields {
                *field = offset_local(*field, local_base);
            }
        }
    }
    stmt
}

fn remap_terminator<'db>(
    terminator: &RTerminator<'db>,
    local_base: usize,
    block_base: usize,
) -> RTerminator<'db> {
    let mut terminator = terminator.clone();
    match &mut terminator {
        RTerminator::Goto(block) => *block = offset_block(*block, block_base),
        RTerminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => {
            *cond = offset_local(*cond, local_base);
            *then_bb = offset_block(*then_bb, block_base);
            *else_bb = offset_block(*else_bb, block_base);
        }
        RTerminator::SwitchScalar {
            discr,
            cases,
            default,
        } => {
            *discr = offset_local(*discr, local_base);
            for (_, block) in cases {
                *block = offset_block(*block, block_base);
            }
            *default = offset_block(*default, block_base);
        }
        RTerminator::MatchEnumTag {
            tag,
            cases,
            default,
            ..
        } => {
            *tag = offset_local(*tag, local_base);
            for (_, block) in cases {
                *block = offset_block(*block, block_base);
            }
            if let Some(block) = default {
                *block = offset_block(*block, block_base);
            }
        }
        RTerminator::TerminalCall { args, .. } => {
            for arg in args {
                *arg = offset_local(*arg, local_base);
            }
        }
        RTerminator::ReturnData { offset, len } | RTerminator::Revert { offset, len } => {
            *offset = offset_local(*offset, local_base);
            *len = offset_local(*len, local_base);
        }
        RTerminator::SelfDestruct { beneficiary } => {
            *beneficiary = offset_local(*beneficiary, local_base)
        }
        RTerminator::Return(Some(value)) => *value = offset_local(*value, local_base),
        RTerminator::Trap | RTerminator::Return(None) | RTerminator::Stop => {}
    }
    terminator
}

fn remap_builtin(builtin: &mut RuntimeBuiltin<'_>, local_base: usize) {
    macro_rules! remap {
        ($($value:ident),* $(,)?) => {{
            $(
                *$value = offset_local(*$value, local_base);
            )*
        }};
    }
    match builtin {
        RuntimeBuiltin::IntTruncate { value, .. }
        | RuntimeBuiltin::Mload { addr: value }
        | RuntimeBuiltin::Sload { slot: value }
        | RuntimeBuiltin::CallDataLoad { offset: value }
        | RuntimeBuiltin::ExtCodeSize { addr: value }
        | RuntimeBuiltin::ExtCodeHash { addr: value }
        | RuntimeBuiltin::Balance { addr: value }
        | RuntimeBuiltin::BlockHash { block: value }
        | RuntimeBuiltin::BlobHash { index: value }
        | RuntimeBuiltin::Malloc { size: value }
        | RuntimeBuiltin::F32FromI32 { value }
        | RuntimeBuiltin::I32FromF32 { value }
        | RuntimeBuiltin::F32Sqrt { value }
        | RuntimeBuiltin::F32Abs { value }
        | RuntimeBuiltin::F32Floor { value }
        | RuntimeBuiltin::F32Ceil { value }
        | RuntimeBuiltin::F32Trunc { value }
        | RuntimeBuiltin::F32Round { value } => remap!(value),
        RuntimeBuiltin::Mstore { addr, value } | RuntimeBuiltin::Mstore8 { addr, value } => {
            remap!(addr, value)
        }
        RuntimeBuiltin::Sstore { slot, value } => remap!(slot, value),
        RuntimeBuiltin::Mcopy { dst, src, len } => remap!(dst, src, len),
        RuntimeBuiltin::ReturnDataCopy { dst, offset, len }
        | RuntimeBuiltin::CallDataCopy { dst, offset, len }
        | RuntimeBuiltin::CodeCopy { dst, offset, len } => remap!(dst, offset, len),
        RuntimeBuiltin::ExtCodeCopy {
            addr,
            dst,
            offset,
            len,
        } => {
            remap!(addr, dst, offset, len)
        }
        RuntimeBuiltin::Keccak256 { offset, len } | RuntimeBuiltin::Log0 { offset, len } => {
            remap!(offset, len)
        }
        RuntimeBuiltin::AddMod { lhs, rhs, modulus }
        | RuntimeBuiltin::MulMod { lhs, rhs, modulus } => remap!(lhs, rhs, modulus),
        RuntimeBuiltin::Byte { pos, value } => remap!(pos, value),
        RuntimeBuiltin::SignExtend { byte, value } => remap!(byte, value),
        RuntimeBuiltin::IntrinsicArith { lhs, rhs, .. }
        | RuntimeBuiltin::Saturating { lhs, rhs, .. }
        | RuntimeBuiltin::F32Min { lhs, rhs }
        | RuntimeBuiltin::F32Max { lhs, rhs }
        | RuntimeBuiltin::F32MinRelaxed { lhs, rhs }
        | RuntimeBuiltin::F32MaxRelaxed { lhs, rhs } => remap!(lhs, rhs),
        RuntimeBuiltin::F32Clamp { value, lo, hi } => remap!(value, lo, hi),
        RuntimeBuiltin::Call {
            gas,
            addr,
            value,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        } => remap!(gas, addr, value, args_offset, args_len, ret_offset, ret_len),
        RuntimeBuiltin::StaticCall {
            gas,
            addr,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        }
        | RuntimeBuiltin::DelegateCall {
            gas,
            addr,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        } => remap!(gas, addr, args_offset, args_len, ret_offset, ret_len),
        RuntimeBuiltin::Create { value, offset, len } => remap!(value, offset, len),
        RuntimeBuiltin::Create2 {
            value,
            offset,
            len,
            salt,
        } => {
            remap!(value, offset, len, salt)
        }
        RuntimeBuiltin::Log1 {
            offset,
            len,
            topic0,
        } => remap!(offset, len, topic0),
        RuntimeBuiltin::Log2 {
            offset,
            len,
            topic0,
            topic1,
        } => {
            remap!(offset, len, topic0, topic1)
        }
        RuntimeBuiltin::Log3 {
            offset,
            len,
            topic0,
            topic1,
            topic2,
        } => {
            remap!(offset, len, topic0, topic1, topic2)
        }
        RuntimeBuiltin::Log4 {
            offset,
            len,
            topic0,
            topic1,
            topic2,
            topic3,
        } => {
            remap!(offset, len, topic0, topic1, topic2, topic3)
        }
        RuntimeBuiltin::Msize
        | RuntimeBuiltin::CallValue
        | RuntimeBuiltin::ReturnDataSize
        | RuntimeBuiltin::CallDataSize
        | RuntimeBuiltin::CodeSize
        | RuntimeBuiltin::Address
        | RuntimeBuiltin::Caller
        | RuntimeBuiltin::Origin
        | RuntimeBuiltin::GasPrice
        | RuntimeBuiltin::CoinBase
        | RuntimeBuiltin::Timestamp
        | RuntimeBuiltin::Number
        | RuntimeBuiltin::PrevRandao
        | RuntimeBuiltin::GasLimit
        | RuntimeBuiltin::ChainId
        | RuntimeBuiltin::BaseFee
        | RuntimeBuiltin::SelfBalance
        | RuntimeBuiltin::BlobBaseFee
        | RuntimeBuiltin::Gas
        | RuntimeBuiltin::CurrentCodeRegionLen
        | RuntimeBuiltin::CodeRegionOffset { .. }
        | RuntimeBuiltin::CodeRegionLen { .. }
        | RuntimeBuiltin::CallDataSelector
        | RuntimeBuiltin::MakeContractFieldRef { .. } => {}
    }
}

/// Split one directly-suspending body into executable target-neutral segments.
///
/// Acyclic suspending callees were already expanded as complete CFGs by the
/// planner. Any remaining suspending callee is recursive and is deliberately
/// refused: it requires a linked stack of per-body frames, not unbounded
/// compile-time unrolling. Direct sites, including sites reached again through
/// loops, are fully split. Backends can persist the returned enum's typed lanes
/// and invoke the selected continuation with its exact variant payload plus one
/// typed `TaskOutcome` delivery.
pub fn materialize_runtime_resumable_machine<'db>(
    db: &'db dyn MirDb,
    plan: &RuntimeResumableBodyPlan<'db>,
) -> Result<RuntimeResumableMachine<'db>, RuntimeSuspensionMaterializeError<'db>> {
    for point in &plan.points {
        if let RuntimeSuspensionCause::Callee { callee } = point.cause {
            return Err(RuntimeSuspensionMaterializeError::CalleeFrameRequired {
                body: plan.body,
                callee,
            });
        }
    }
    if let Some(tail) = plan.suspending_tails.first() {
        return Err(
            RuntimeSuspensionMaterializeError::SuspendingTailFrameRequired {
                body: plan.body,
                callee: tail.callee,
            },
        );
    }

    let source_body = plan.flattened_body.clone();
    let complete_fields = source_body
        .signature
        .ret
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let mut variants = Vec::with_capacity(plan.points.len() + 1);
    variants.push(EnumVariantLayout {
        name: "Complete".to_owned(),
        fields: complete_fields,
    });
    for point in &plan.points {
        let RuntimeSuspensionCause::Effect { pending } = point.cause else {
            unreachable!("callee suspension was rejected above")
        };
        let mut fields = Vec::with_capacity(point.live_values.len() + 1);
        fields.push(
            source_body
                .value_class(pending)
                .cloned()
                .ok_or(RuntimeSuspensionMaterializeError::MissingRuntimeClass { local: pending })?,
        );
        for local in &point.live_values {
            fields.push(
                source_body.value_class(*local).cloned().ok_or(
                    RuntimeSuspensionMaterializeError::MissingRuntimeClass { local: *local },
                )?,
            );
        }
        variants.push(EnumVariantLayout {
            name: format!("Suspended{}", point.continuation_state),
            fields: fields.into_boxed_slice(),
        });
    }
    let step_layout = LayoutId::new(
        db,
        LayoutKey::Enum(EnumLayoutKey {
            // This layout is compiler-owned and has no distinct authored type.
            // Unit is provenance only; exact runtime fields above are the ABI.
            source_ty: hir::analysis::ty::ty_def::TyId::unit(db),
            variants: variants.into_boxed_slice(),
        }),
    );
    let point_by_position = plan
        .points
        .iter()
        .map(|point| ((point.block, point.statement as usize), point))
        .collect::<std::collections::BTreeMap<_, _>>();

    let entry_body =
        materialize_segment_body(db, &source_body, step_layout, &point_by_position, None)?;
    let entry = RuntimeContinuationSegment {
        continuation_state: 0,
        body: entry_body,
    };
    let continuations = plan
        .points
        .iter()
        .map(|point| {
            materialize_segment_body(
                db,
                &source_body,
                step_layout,
                &point_by_position,
                Some(point),
            )
            .map(|body| RuntimeContinuationSegment {
                continuation_state: point.continuation_state,
                body,
            })
        })
        .collect::<Result<Box<[_]>, _>>()?;
    Ok(RuntimeResumableMachine {
        source: plan.body,
        step_layout,
        entry,
        continuations,
    })
}

fn materialize_segment_body<'db>(
    db: &'db dyn MirDb,
    source: &RuntimeBody<'db>,
    step_layout: LayoutId<'db>,
    points: &std::collections::BTreeMap<(RBlockId, usize), &RuntimeSuspensionPoint<'db>>,
    resume_from: Option<&RuntimeSuspensionPoint<'db>>,
) -> Result<RuntimeBody<'db>, RuntimeSuspensionMaterializeError<'db>> {
    let mut body = source.clone();
    let step_class = RuntimeClass::AggregateValue {
        layout: step_layout,
    };
    let step_local = RLocalId::from_u32(body.locals.len() as u32);
    body.locals.push(RLocal {
        semantic_ty: hir::analysis::ty::ty_def::TyId::unit(db),
        carrier: RuntimeCarrier::Value(step_class.clone()),
        root: RuntimeLocalRoot::None,
    });
    body.signature.ret = Some(step_class);

    let block_shift = usize::from(resume_from.is_some());
    let mut blocks = Vec::with_capacity(source.blocks.len() + block_shift);
    if let Some(point) = resume_from {
        body.signature.params = point
            .live_values
            .iter()
            .copied()
            .chain(std::iter::once(point.delivery))
            .map(|local| {
                let class = source
                    .value_class(local)
                    .cloned()
                    .ok_or(RuntimeSuspensionMaterializeError::MissingRuntimeClass { local })?;
                Ok(RuntimeParam { local, class })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let original = &source.blocks[point.block.index()];
        blocks.push(materialize_block(
            original,
            point.block,
            point.statement as usize + 1,
            step_layout,
            step_local,
            points,
            1,
        )?);
    }
    for (index, block) in source.blocks.iter().enumerate() {
        blocks.push(materialize_block(
            block,
            RBlockId::new(index),
            0,
            step_layout,
            step_local,
            points,
            block_shift,
        )?);
    }
    body.blocks = blocks;
    Ok(body)
}

#[allow(clippy::too_many_arguments)]
fn materialize_block<'db>(
    source: &RBlock<'db>,
    original_block: RBlockId,
    start_statement: usize,
    step_layout: LayoutId<'db>,
    step_local: RLocalId,
    points: &std::collections::BTreeMap<(RBlockId, usize), &RuntimeSuspensionPoint<'db>>,
    block_shift: usize,
) -> Result<RBlock<'db>, RuntimeSuspensionMaterializeError<'db>> {
    let mut stmts = Vec::new();
    for (relative, statement) in source.stmts[start_statement..].iter().enumerate() {
        let statement_index = start_statement + relative;
        let Some(point) = points.get(&(original_block, statement_index)).copied() else {
            stmts.push(statement.clone());
            continue;
        };
        let RuntimeSuspensionCause::Effect { pending } = point.cause else {
            unreachable!("callee suspension was rejected before segment construction")
        };
        let variant = VariantId {
            enum_layout: step_layout,
            index: point.continuation_state as u16,
        };
        let fields = std::iter::once(pending)
            .chain(point.live_values.iter().copied())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        stmts.push(RStmt::Assign {
            dst: step_local,
            expr: RExpr::EnumMake {
                layout: step_layout,
                variant,
                fields,
            },
        });
        return Ok(RBlock {
            stmts,
            terminator: RTerminator::Return(Some(step_local)),
        });
    }

    let terminator = match &source.terminator {
        RTerminator::Return(value) => {
            stmts.push(RStmt::Assign {
                dst: step_local,
                expr: RExpr::EnumMake {
                    layout: step_layout,
                    variant: VariantId {
                        enum_layout: step_layout,
                        index: 0,
                    },
                    fields: value.iter().copied().collect::<Vec<_>>().into_boxed_slice(),
                },
            });
            RTerminator::Return(Some(step_local))
        }
        RTerminator::TerminalCall { callee, .. } => {
            return Err(RuntimeSuspensionMaterializeError::UnsupportedTerminalCall {
                block: original_block,
                callee: *callee,
            });
        }
        other => shift_terminator(other, block_shift),
    };
    Ok(RBlock { stmts, terminator })
}

fn shift_block(block: RBlockId, by: usize) -> RBlockId {
    RBlockId::new(block.index() + by)
}

fn shift_terminator<'db>(terminator: &RTerminator<'db>, by: usize) -> RTerminator<'db> {
    match terminator {
        RTerminator::Goto(block) => RTerminator::Goto(shift_block(*block, by)),
        RTerminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => RTerminator::Branch {
            cond: *cond,
            then_bb: shift_block(*then_bb, by),
            else_bb: shift_block(*else_bb, by),
        },
        RTerminator::SwitchScalar {
            discr,
            cases,
            default,
        } => RTerminator::SwitchScalar {
            discr: *discr,
            cases: cases
                .iter()
                .map(|(value, block)| (value.clone(), shift_block(*block, by)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            default: shift_block(*default, by),
        },
        RTerminator::MatchEnumTag {
            tag,
            enum_layout,
            cases,
            default,
        } => RTerminator::MatchEnumTag {
            tag: *tag,
            enum_layout: *enum_layout,
            cases: cases
                .iter()
                .map(|(variant, block)| (*variant, shift_block(*block, by)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            default: default.map(|block| shift_block(block, by)),
        },
        other => other.clone(),
    }
}

fn body_directly_suspends(db: &dyn MirDb, body: &RuntimeBody<'_>) -> bool {
    body.blocks.iter().any(|block| {
        block.stmts.iter().any(|statement| {
            matches!(
                statement,
                RStmt::Assign {
                    expr: RExpr::Call { callee, .. },
                    ..
                } if runtime_control_effect_kind(db, *callee)
                    == Some(RuntimeControlEffectFuncKind::Suspend)
            )
        }) || matches!(
            block.terminator,
            RTerminator::TerminalCall { callee, .. }
                if runtime_control_effect_kind(db, callee)
                    == Some(RuntimeControlEffectFuncKind::Suspend)
        )
    })
}

fn body_calls_any(body: &RuntimeBody<'_>, candidates: &FxHashSet<RuntimeInstance<'_>>) -> bool {
    body.blocks.iter().any(|block| {
        block.stmts.iter().any(|statement| {
            matches!(
                statement,
                RStmt::Assign {
                    expr: RExpr::Call { callee, .. },
                    ..
                } if candidates.contains(callee)
            )
        }) || matches!(
            block.terminator,
            RTerminator::TerminalCall { callee, .. } if candidates.contains(&callee)
        )
    })
}

fn continuation_frame<'db>(
    body: &RuntimeBody<'db>,
    points: &[RuntimeSuspensionPoint<'db>],
) -> Result<Box<[RuntimeContinuationFrameSlot<'db>]>, RuntimeSuspensionPlanError> {
    let locals = points
        .iter()
        .flat_map(|point| point.live_values.iter().copied())
        .collect::<BTreeSet<_>>();
    locals
        .into_iter()
        .map(|local| {
            let runtime_local = body
                .local(local)
                .ok_or(RuntimeSuspensionPlanError::LiveValueHasNoRuntimeClass { local })?;
            let class = body
                .value_class(local)
                .cloned()
                .ok_or(RuntimeSuspensionPlanError::LiveValueHasNoRuntimeClass { local })?;
            Ok(RuntimeContinuationFrameSlot {
                local,
                class,
                root: runtime_local.root.clone(),
            })
        })
        .collect::<Result<Box<[_]>, _>>()
}

fn block_successors(terminator: &RTerminator<'_>) -> Vec<RBlockId> {
    match terminator {
        RTerminator::Goto(block) => vec![*block],
        RTerminator::Branch {
            then_bb, else_bb, ..
        } => vec![*then_bb, *else_bb],
        RTerminator::SwitchScalar { cases, default, .. } => cases
            .iter()
            .map(|(_, block)| *block)
            .chain(std::iter::once(*default))
            .collect(),
        RTerminator::MatchEnumTag { cases, default, .. } => cases
            .iter()
            .map(|(_, block)| *block)
            .chain(default.iter().copied())
            .collect(),
        RTerminator::TerminalCall { .. }
        | RTerminator::ReturnData { .. }
        | RTerminator::Revert { .. }
        | RTerminator::SelfDestruct { .. }
        | RTerminator::Trap
        | RTerminator::Return(_)
        | RTerminator::Stop => Vec::new(),
    }
}

fn live_values_across_sites(
    blocks: &[RBlock<'_>],
    provider_bindings: &[super::RuntimeProviderBinding<'_>],
    sites: &BTreeSet<(RBlockId, usize)>,
) -> std::collections::BTreeMap<(RBlockId, usize), BTreeSet<RLocalId>> {
    let mut block_uses = vec![BTreeSet::new(); blocks.len()];
    let mut block_defs = vec![BTreeSet::new(); blocks.len()];
    for (index, block) in blocks.iter().enumerate() {
        for statement in &block.stmts {
            let mut used = BTreeSet::new();
            stmt_uses(provider_bindings, statement, &mut used);
            block_uses[index].extend(
                used.into_iter()
                    .filter(|value| !block_defs[index].contains(value)),
            );
            if let RStmt::Assign { dst, .. } = statement {
                block_defs[index].insert(*dst);
            }
        }
        let mut used = BTreeSet::new();
        terminator_uses(&block.terminator, &mut used);
        block_uses[index].extend(
            used.into_iter()
                .filter(|value| !block_defs[index].contains(value)),
        );
    }

    let mut live_in = vec![BTreeSet::new(); blocks.len()];
    let mut live_out = vec![BTreeSet::new(); blocks.len()];
    loop {
        let mut changed = false;
        for index in (0..blocks.len()).rev() {
            let next_out = block_successors(&blocks[index].terminator)
                .into_iter()
                .flat_map(|successor| live_in[successor.index()].iter().copied())
                .collect::<BTreeSet<_>>();
            let mut next_in = block_uses[index].clone();
            next_in.extend(
                next_out
                    .iter()
                    .copied()
                    .filter(|value| !block_defs[index].contains(value)),
            );
            if next_out != live_out[index] || next_in != live_in[index] {
                live_out[index] = next_out;
                live_in[index] = next_in;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut result = std::collections::BTreeMap::new();
    for (block_index, block) in blocks.iter().enumerate() {
        let block_id = RBlockId::new(block_index);
        let mut live = live_out[block_index].clone();
        terminator_uses(&block.terminator, &mut live);
        for (statement_index, statement) in block.stmts.iter().enumerate().rev() {
            if sites.contains(&(block_id, statement_index)) {
                result.insert((block_id, statement_index), live.clone());
            }
            if let RStmt::Assign { dst, .. } = statement {
                live.remove(dst);
            }
            stmt_uses(provider_bindings, statement, &mut live);
        }
    }
    result
}

fn place_uses(
    provider_bindings: &[super::RuntimeProviderBinding<'_>],
    place: &RuntimePlace<'_>,
    used: &mut BTreeSet<RLocalId>,
) {
    match &place.root {
        PlaceRoot::Slot(local) | PlaceRoot::Ref(local) => {
            used.insert(*local);
        }
        PlaceRoot::Provider(binding) => {
            if let Some(binding) = provider_bindings.get(binding.index()) {
                used.insert(binding.value);
            }
        }
        PlaceRoot::Ptr { addr, .. } => {
            used.insert(*addr);
        }
    }
    for element in &place.path {
        if let PlaceElem::Index(IndexSource::Dynamic(value)) = element {
            used.insert(*value);
        }
    }
}

fn expr_uses(
    provider_bindings: &[super::RuntimeProviderBinding<'_>],
    expr: &RExpr<'_>,
    used: &mut BTreeSet<RLocalId>,
) {
    match expr {
        RExpr::Use(value)
        | RExpr::Unary { value, .. }
        | RExpr::Cast { value, .. }
        | RExpr::Bitcast { value, .. }
        | RExpr::MaterializeToObject { src: value }
        | RExpr::ProviderToRaw { value }
        | RExpr::RetagRef { value }
        | RExpr::AggregateExtract { value, .. }
        | RExpr::EnumTagOfValue { value }
        | RExpr::EnumIsVariant { value, .. }
        | RExpr::EnumExtract { value, .. }
        | RExpr::EnumGetTag { root: value }
        | RExpr::EnumAssertVariantRef { root: value, .. } => {
            used.insert(*value);
        }
        RExpr::Binary { lhs, rhs, .. } => {
            used.insert(*lhs);
            used.insert(*rhs);
        }
        RExpr::ProviderFromRaw { raw, .. } => {
            used.insert(*raw);
        }
        RExpr::WordToRawAddr { value, .. } => {
            used.insert(*value);
        }
        RExpr::MaterializePlaceToObject { place }
        | RExpr::AddrOf { place }
        | RExpr::Load { place } => place_uses(provider_bindings, place, used),
        RExpr::AggregateMake { fields, .. }
        | RExpr::Call { args: fields, .. }
        | RExpr::EnumMake { fields, .. } => used.extend(fields.iter().copied()),
        RExpr::Builtin(builtin) => builtin_uses(builtin, used),
        RExpr::ConstScalar(_)
        | RExpr::Placeholder { .. }
        | RExpr::ConstRef { .. }
        | RExpr::AllocObject { .. } => {}
    }
}

fn stmt_uses(
    provider_bindings: &[super::RuntimeProviderBinding<'_>],
    statement: &RStmt<'_>,
    used: &mut BTreeSet<RLocalId>,
) {
    match statement {
        RStmt::Assign { expr, .. } => expr_uses(provider_bindings, expr, used),
        RStmt::EnumAssertVariant { value, .. } => {
            used.insert(*value);
        }
        RStmt::Store { dst, src } | RStmt::CopyInto { dst, src } => {
            place_uses(provider_bindings, dst, used);
            used.insert(*src);
        }
        RStmt::EnumSetTag { root, .. } => {
            used.insert(*root);
        }
        RStmt::EnumWriteVariant { root, fields, .. } => {
            used.insert(*root);
            used.extend(fields.iter().copied());
        }
    }
}

fn terminator_uses(terminator: &RTerminator<'_>, used: &mut BTreeSet<RLocalId>) {
    match terminator {
        RTerminator::Branch { cond, .. } => {
            used.insert(*cond);
        }
        RTerminator::SwitchScalar { discr, .. } => {
            used.insert(*discr);
        }
        RTerminator::MatchEnumTag { tag, .. } => {
            used.insert(*tag);
        }
        RTerminator::TerminalCall { args, .. } => used.extend(args.iter().copied()),
        RTerminator::ReturnData { offset, len } | RTerminator::Revert { offset, len } => {
            used.insert(*offset);
            used.insert(*len);
        }
        RTerminator::SelfDestruct { beneficiary } => {
            used.insert(*beneficiary);
        }
        RTerminator::Return(Some(value)) => {
            used.insert(*value);
        }
        RTerminator::Goto(_)
        | RTerminator::Trap
        | RTerminator::Return(None)
        | RTerminator::Stop => {}
    }
}

fn builtin_uses(builtin: &RuntimeBuiltin<'_>, used: &mut BTreeSet<RLocalId>) {
    macro_rules! mark {
        ($($value:expr),* $(,)?) => {{ $(let _ = used.insert(*$value);)* }};
    }
    match builtin {
        RuntimeBuiltin::IntTruncate { value, .. }
        | RuntimeBuiltin::Mload { addr: value }
        | RuntimeBuiltin::Sload { slot: value }
        | RuntimeBuiltin::CallDataLoad { offset: value }
        | RuntimeBuiltin::ExtCodeSize { addr: value }
        | RuntimeBuiltin::ExtCodeHash { addr: value }
        | RuntimeBuiltin::Balance { addr: value }
        | RuntimeBuiltin::BlockHash { block: value }
        | RuntimeBuiltin::BlobHash { index: value }
        | RuntimeBuiltin::Malloc { size: value }
        | RuntimeBuiltin::F32FromI32 { value }
        | RuntimeBuiltin::I32FromF32 { value }
        | RuntimeBuiltin::F32Sqrt { value }
        | RuntimeBuiltin::F32Abs { value }
        | RuntimeBuiltin::F32Floor { value }
        | RuntimeBuiltin::F32Ceil { value }
        | RuntimeBuiltin::F32Trunc { value }
        | RuntimeBuiltin::F32Round { value } => mark!(value),
        RuntimeBuiltin::Mstore { addr, value } | RuntimeBuiltin::Mstore8 { addr, value } => {
            mark!(addr, value)
        }
        RuntimeBuiltin::Sstore { slot, value } => mark!(slot, value),
        RuntimeBuiltin::Mcopy { dst, src, len } => mark!(dst, src, len),
        RuntimeBuiltin::ReturnDataCopy { dst, offset, len }
        | RuntimeBuiltin::CallDataCopy { dst, offset, len }
        | RuntimeBuiltin::CodeCopy { dst, offset, len } => mark!(dst, offset, len),
        RuntimeBuiltin::ExtCodeCopy {
            addr,
            dst,
            offset,
            len,
        } => mark!(addr, dst, offset, len),
        RuntimeBuiltin::Keccak256 { offset, len } | RuntimeBuiltin::Log0 { offset, len } => {
            mark!(offset, len)
        }
        RuntimeBuiltin::AddMod { lhs, rhs, modulus }
        | RuntimeBuiltin::MulMod { lhs, rhs, modulus } => mark!(lhs, rhs, modulus),
        RuntimeBuiltin::Byte { pos, value } => mark!(pos, value),
        RuntimeBuiltin::SignExtend { byte, value } => mark!(byte, value),
        RuntimeBuiltin::IntrinsicArith { lhs, rhs, .. }
        | RuntimeBuiltin::Saturating { lhs, rhs, .. }
        | RuntimeBuiltin::F32Min { lhs, rhs }
        | RuntimeBuiltin::F32Max { lhs, rhs }
        | RuntimeBuiltin::F32MinRelaxed { lhs, rhs }
        | RuntimeBuiltin::F32MaxRelaxed { lhs, rhs } => mark!(lhs, rhs),
        RuntimeBuiltin::F32Clamp { value, lo, hi } => mark!(value, lo, hi),
        RuntimeBuiltin::Call {
            gas,
            addr,
            value,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        } => mark!(gas, addr, value, args_offset, args_len, ret_offset, ret_len),
        RuntimeBuiltin::StaticCall {
            gas,
            addr,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        }
        | RuntimeBuiltin::DelegateCall {
            gas,
            addr,
            args_offset,
            args_len,
            ret_offset,
            ret_len,
        } => mark!(gas, addr, args_offset, args_len, ret_offset, ret_len),
        RuntimeBuiltin::Create { value, offset, len } => mark!(value, offset, len),
        RuntimeBuiltin::Create2 {
            value,
            offset,
            len,
            salt,
        } => mark!(value, offset, len, salt),
        RuntimeBuiltin::Log1 {
            offset,
            len,
            topic0,
        } => mark!(offset, len, topic0),
        RuntimeBuiltin::Log2 {
            offset,
            len,
            topic0,
            topic1,
        } => mark!(offset, len, topic0, topic1),
        RuntimeBuiltin::Log3 {
            offset,
            len,
            topic0,
            topic1,
            topic2,
        } => mark!(offset, len, topic0, topic1, topic2),
        RuntimeBuiltin::Log4 {
            offset,
            len,
            topic0,
            topic1,
            topic2,
            topic3,
        } => mark!(offset, len, topic0, topic1, topic2, topic3),
        RuntimeBuiltin::Msize
        | RuntimeBuiltin::CallValue
        | RuntimeBuiltin::ReturnDataSize
        | RuntimeBuiltin::CallDataSize
        | RuntimeBuiltin::CodeSize
        | RuntimeBuiltin::Address
        | RuntimeBuiltin::Caller
        | RuntimeBuiltin::Origin
        | RuntimeBuiltin::GasPrice
        | RuntimeBuiltin::CoinBase
        | RuntimeBuiltin::Timestamp
        | RuntimeBuiltin::Number
        | RuntimeBuiltin::PrevRandao
        | RuntimeBuiltin::GasLimit
        | RuntimeBuiltin::ChainId
        | RuntimeBuiltin::BaseFee
        | RuntimeBuiltin::SelfBalance
        | RuntimeBuiltin::BlobBaseFee
        | RuntimeBuiltin::Gas
        | RuntimeBuiltin::CurrentCodeRegionLen
        | RuntimeBuiltin::CodeRegionOffset { .. }
        | RuntimeBuiltin::CodeRegionLen { .. }
        | RuntimeBuiltin::CallDataSelector
        | RuntimeBuiltin::MakeContractFieldRef { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConstScalar;

    fn local(index: u32) -> RLocalId {
        RLocalId::from_u32(index)
    }

    #[test]
    fn liveness_crosses_branches_but_excludes_dead_prefix_values() {
        let blocks = vec![
            RBlock {
                stmts: vec![
                    RStmt::Assign {
                        dst: local(3),
                        expr: RExpr::Binary {
                            op: hir::hir_def::BinOp::Arith(hir::hir_def::ArithBinOp::Add),
                            lhs: local(0),
                            rhs: local(1),
                        },
                    },
                    RStmt::Assign {
                        dst: local(4),
                        expr: RExpr::ConstScalar(ConstScalar::Int {
                            bits: 32,
                            signed: false,
                            words: vec![9],
                        }),
                    },
                    // The site marker need not be a real call for this pure
                    // dataflow oracle; nominal-call recognition is tested by
                    // the public planner.
                    RStmt::Assign {
                        dst: local(5),
                        expr: RExpr::Use(local(2)),
                    },
                ],
                terminator: RTerminator::Branch {
                    cond: local(6),
                    then_bb: RBlockId::new(1),
                    else_bb: RBlockId::new(2),
                },
            },
            RBlock {
                stmts: vec![RStmt::Assign {
                    dst: local(7),
                    expr: RExpr::Use(local(3)),
                }],
                terminator: RTerminator::Return(Some(local(7))),
            },
            RBlock {
                stmts: vec![RStmt::Assign {
                    dst: local(8),
                    expr: RExpr::Use(local(5)),
                }],
                terminator: RTerminator::Return(Some(local(8))),
            },
        ];
        let site = (RBlockId::new(0), 2usize);
        let live = live_values_across_sites(&blocks, &[], &BTreeSet::from([site]));
        assert_eq!(
            live[&site],
            BTreeSet::from([local(3), local(5), local(6)]),
            "the branch condition and either-branch payload survive; dead constant local 4 does not"
        );
    }
}
