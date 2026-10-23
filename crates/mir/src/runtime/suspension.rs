//! Resolved native cuts and scalar frame requirements. Only the privileged
//! operation is a cut; a capability call must first have its continuation
//! composed with the caller. Projection yields remain ordinary MIR control.

mod segments;
pub use segments::{
    NativeScalarSegments, NativeScalarTaskPlan, build_native_scalar_segments,
    plan_native_scalar_task,
};

use std::collections::BTreeSet;

use cranelift_entity::EntityRef;
use hir::analysis::{
    semantic::{SemOrigin, suspension::suspension_contract},
    ty::{corelib::native_suspension_primitive, ty_check::BodyOwner},
};
use rustc_hash::FxHashMap;

use crate::{
    MirDb,
    instance::{RuntimeInstance, runtime::runtime_instance_lowered_body},
    runtime::{
        LowerError, RBlock, RBlockId, RExpr, RLocalId, RStmt, RTerminator, RuntimeBody,
        RuntimeCarrier, RuntimeClass, RuntimeLocalRoot, ScalarClass,
    },
    verify::verify_runtime_body,
};

/// Compose nonrecursive suspension-capable calls before constructing frame
/// requirements. Ordinary calls without that footprint retain their ABI.
/// Recursive task calls need a dynamic frame chain and remain an explicit
/// unsupported case of this first scalar transform.
pub fn compose_native_scalar_task<'db>(
    db: &'db dyn MirDb,
    root: RuntimeInstance<'db>,
) -> Result<RuntimeBody<'db>, LowerError> {
    fn compose<'db>(
        db: &'db dyn MirDb,
        current: RuntimeInstance<'db>,
        active: &mut Vec<RuntimeInstance<'db>>,
        cache: &mut FxHashMap<RuntimeInstance<'db>, RuntimeBody<'db>>,
    ) -> Result<RuntimeBody<'db>, LowerError> {
        if active.contains(&current) {
            return Err(LowerError::Unsupported(
                "recursive native task calls require a dynamic frame chain".into(),
            ));
        }
        if let Some(body) = cache.get(&current) {
            return Ok(body.clone());
        }
        active.push(current);
        let mut body = runtime_instance_lowered_body(db, current)?.body(db).clone();
        loop {
            let next = body.blocks.iter().enumerate().find_map(|(block, data)| {
                data.stmts.iter().enumerate().find_map(|(statement, stmt)| {
                    let RStmt::Assign { expr: RExpr::Call { callee, .. }, .. } = stmt else { return None; };
                    let semantic = callee.key(db).semantic(db)?;
                    if matches!(semantic.key(db).owner(db), BodyOwner::Func(func, _) if native_suspension_primitive(db, func))
                        || suspension_contract(db, semantic.key(db)).entries.is_empty()
                    { return None; }
                    Some((RBlockId::new(block), statement, *callee))
                })
            });
            let Some((block, statement, callee)) = next else {
                break;
            };
            if callee.is_external_declaration(db) {
                return Err(LowerError::Unsupported(
                    "a suspension-capable extern needs a native task adapter".into(),
                ));
            }
            let callee_body = compose(db, callee, active, cache)?;
            crate::runtime::lower::inline::inline_task_call(
                &mut body,
                block,
                statement,
                &callee_body,
            )?;
        }
        verify_runtime_body(db, &db, &body).map_err(|error| {
            LowerError::Unsupported(format!("invalid composed task MIR: {error:?}"))
        })?;
        active.pop();
        cache.insert(current, body.clone());
        Ok(body)
    }
    compose(db, root, &mut Vec::new(), &mut FxHashMap::default())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScalarCapture<'db> {
    /// An SSA-like scalar value; its bits are saved directly.
    Value {
        local: RLocalId,
        class: ScalarClass<'db>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSuspensionCut<'db> {
    pub block: RBlockId,
    pub statement: usize,
    pub origin: SemOrigin<'db>,
    pub pending: RLocalId,
    /// Defined by delivery, so it must not be captured before suspension.
    pub outcome: RLocalId,
    pub captures: Vec<ScalarCapture<'db>>,
}

/// Identify direct raw cuts in verified MIR and calculate what each resume
/// needs. Remaining calls with suspension contracts are refused: retaining an
/// ordinary machine stack while claiming a heap-owned frame is not lowering.
/// This preparation does not emit start/resume functions.
pub fn prepare_native_scalar_cuts<'db>(
    db: &'db dyn MirDb,
    body: &RuntimeBody<'db>,
) -> Result<Vec<NativeSuspensionCut<'db>>, LowerError> {
    verify_runtime_body(db, &db, body)
        .map_err(|error| LowerError::Unsupported(format!("invalid native task MIR: {error:?}")))?;
    if body.stmt_origins.len() != body.blocks.len()
        || body
            .blocks
            .iter()
            .zip(&body.stmt_origins)
            .any(|(block, origins)| block.stmts.len() != origins.len())
    {
        return Err(LowerError::Unsupported(
            "native task MIR has incomplete statement origins".into(),
        ));
    }
    let entries = live_on_entry(&body.blocks);
    let mut cuts = Vec::new();
    for (index, block) in body.blocks.iter().enumerate() {
        if let RTerminator::TerminalCall { callee, .. } = &block.terminator
            && let Some(semantic) = callee.key(db).semantic(db)
            && !suspension_contract(db, semantic.key(db)).entries.is_empty()
        {
            return Err(LowerError::Unsupported(
                "native task preparation requires composition of a suspending terminal callee"
                    .into(),
            ));
        }
        let mut live = live_at_exit(block, &entries);
        for (statement, stmt) in block.stmts.iter().enumerate().rev() {
            if let RStmt::Assign {
                dst,
                expr: RExpr::Call { callee, args },
            } = stmt
                && let Some(semantic) = callee.key(db).semantic(db)
            {
                let raw = matches!(semantic.key(db).owner(db), BodyOwner::Func(func, _) if native_suspension_primitive(db, func));
                if raw {
                    let [pending] = args.as_ref() else {
                        return Err(LowerError::Unsupported(
                            "native suspension must consume one pending value".into(),
                        ));
                    };
                    let mut captures = Vec::new();
                    for &local in live.iter().filter(|local| **local != *dst) {
                        let data = &body.locals[local.index()];
                        let capture = match (&data.root, &data.carrier) {
                            (RuntimeLocalRoot::None, RuntimeCarrier::Erased) => continue,
                            (
                                RuntimeLocalRoot::None,
                                RuntimeCarrier::Value(RuntimeClass::Scalar(class)),
                            ) => ScalarCapture::Value {
                                local,
                                class: class.clone(),
                            },
                            _ => {
                                return Err(LowerError::Unsupported(format!(
                                    "native scalar suspension cannot yet retain {local:?}: {:?}, {:?}",
                                    data.root, data.carrier
                                )));
                            }
                        };
                        captures.push(capture);
                    }
                    cuts.push(NativeSuspensionCut {
                        block: RBlockId::new(index),
                        statement,
                        origin: body.stmt_origins[index][statement],
                        pending: *pending,
                        outcome: *dst,
                        captures,
                    });
                } else if !suspension_contract(db, semantic.key(db)).entries.is_empty() {
                    return Err(LowerError::Unsupported("native task preparation requires composition of a suspending callee before frame construction".into()));
                }
            }
            transfer_backwards(stmt, &mut live);
        }
    }
    cuts.sort_by_key(|cut| (cut.block, cut.statement));
    Ok(cuts)
}

fn live_at_exit(block: &RBlock<'_>, entries: &[BTreeSet<RLocalId>]) -> BTreeSet<RLocalId> {
    let mut live = BTreeSet::new();
    for next in block.terminator.successors() {
        live.extend(entries[next.index()].iter().copied());
    }
    live.extend(
        block
            .terminator
            .clone()
            .values_mut()
            .into_iter()
            .map(|value| *value),
    );
    live
}

fn transfer_backwards(stmt: &RStmt<'_>, live: &mut BTreeSet<RLocalId>) {
    let mut stmt = stmt.clone();
    match &mut stmt {
        RStmt::Assign { dst, expr } => {
            live.remove(dst);
            live.extend(expr.values_mut().into_iter().map(|value| *value));
        }
        // Place writes read the identity/address of their destination. Slot
        // contents are conservatively retained until frame reconstruction.
        _ => live.extend(stmt.locals_mut().into_iter().map(|value| *value)),
    }
}

fn live_on_entry(blocks: &[RBlock<'_>]) -> Vec<BTreeSet<RLocalId>> {
    let mut entries = vec![BTreeSet::new(); blocks.len()];
    loop {
        let mut changed = false;
        for (index, block) in blocks.iter().enumerate().rev() {
            let mut live = live_at_exit(block, &entries);
            for stmt in block.stmts.iter().rev() {
                transfer_backwards(stmt, &mut live);
            }
            if live != entries[index] {
                entries[index] = live;
                changed = true;
            }
        }
        if !changed {
            return entries;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liveness_keeps_loop_carried_values_and_kills_redefined_results() {
        let keep = RLocalId::new(0);
        let result = RLocalId::new(1);
        let condition = RLocalId::new(2);
        let blocks = vec![
            RBlock {
                stmts: vec![],
                terminator: RTerminator::Goto(RBlockId::new(1)),
            },
            RBlock {
                stmts: vec![RStmt::Assign {
                    dst: result,
                    expr: RExpr::Use(keep),
                }],
                terminator: RTerminator::Branch {
                    cond: condition,
                    then_bb: RBlockId::new(1),
                    else_bb: RBlockId::new(2),
                },
            },
            RBlock {
                stmts: vec![],
                terminator: RTerminator::Return(Some(result)),
            },
        ];
        let live = live_on_entry(&blocks);
        assert_eq!(live[0], BTreeSet::from([keep, condition]));
        assert_eq!(live[1], BTreeSet::from([keep, condition]));
        assert_eq!(live[2], BTreeSet::from([result]));
    }
}
