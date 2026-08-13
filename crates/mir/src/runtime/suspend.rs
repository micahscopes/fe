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
use rustc_hash::FxHashSet;

use crate::{MirDb, RuntimeControlEffectFuncKind, RuntimeInstance, runtime_control_effect_kind};

use super::{
    PlaceElem, PlaceRoot, RBlock, RBlockId, RExpr, RLocalId, RStmt, RTerminator, RuntimeBody,
    RuntimeBuiltin, RuntimeClass, RuntimeLocalRoot, RuntimePackage, RuntimePlace,
};

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
    let live = live_values_across_sites(&body.blocks, &positions);
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

    let mut plans = Vec::new();
    for (instance, body) in bodies {
        if !resumable.contains(&instance) {
            continue;
        }
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
        let live = live_values_across_sites(&body.blocks, &positions);
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
            frame,
            points,
            suspending_tails: tails.into_boxed_slice(),
        });
    }
    Ok(plans)
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
    sites: &BTreeSet<(RBlockId, usize)>,
) -> std::collections::BTreeMap<(RBlockId, usize), BTreeSet<RLocalId>> {
    let mut block_uses = vec![BTreeSet::new(); blocks.len()];
    let mut block_defs = vec![BTreeSet::new(); blocks.len()];
    for (index, block) in blocks.iter().enumerate() {
        for statement in &block.stmts {
            let mut used = BTreeSet::new();
            stmt_uses(statement, &mut used);
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
            stmt_uses(statement, &mut live);
        }
    }
    result
}

fn place_uses(place: &RuntimePlace<'_>, used: &mut BTreeSet<RLocalId>) {
    match &place.root {
        PlaceRoot::Slot(local) | PlaceRoot::Ref(local) => {
            used.insert(*local);
        }
        PlaceRoot::Provider(_) => {}
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

fn expr_uses(expr: &RExpr<'_>, used: &mut BTreeSet<RLocalId>) {
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
        | RExpr::Load { place } => place_uses(place, used),
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

fn stmt_uses(statement: &RStmt<'_>, used: &mut BTreeSet<RLocalId>) {
    match statement {
        RStmt::Assign { expr, .. } => expr_uses(expr, used),
        RStmt::EnumAssertVariant { value, .. } => {
            used.insert(*value);
        }
        RStmt::Store { dst, src } | RStmt::CopyInto { dst, src } => {
            place_uses(dst, used);
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
        let live = live_values_across_sites(&blocks, &BTreeSet::from([site]));
        assert_eq!(
            live[&site],
            BTreeSet::from([local(3), local(5), local(6)]),
            "the branch condition and either-branch payload survive; dead constant local 4 does not"
        );
    }
}
