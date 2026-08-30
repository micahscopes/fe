use rustc_hash::{FxHashMap, FxHashSet};

use super::{ConstScalar, RExpr, RLocalId, RStmt, RuntimeBuiltin};

/// Structurally known aggregate fields at a callsite. This records identity,
/// not scalar values, so it makes no arithmetic or floating-point assumptions.
pub type RuntimeAggregateFacts = FxHashMap<RLocalId, Box<[RLocalId]>>;
pub type RuntimeScalarConstFacts = FxHashMap<RLocalId, ConstScalar>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeArgFact {
    Unknown,
    ScalarConst(ConstScalar),
    Aggregate(Box<[RuntimeArgFact]>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeArgShapeKey(pub Box<[RuntimeArgFact]>);

impl RuntimeArgShapeKey {
    pub fn has_known_facts(&self) -> bool {
        fn known(fact: &RuntimeArgFact) -> bool {
            match fact {
                RuntimeArgFact::Unknown => false,
                RuntimeArgFact::ScalarConst(_) => true,
                RuntimeArgFact::Aggregate(_) => true,
            }
        }
        self.0.iter().any(known)
    }
}

pub fn runtime_arg_shape_key(
    args: &[RLocalId],
    aggregates: &RuntimeAggregateFacts,
    constants: &RuntimeScalarConstFacts,
) -> RuntimeArgShapeKey {
    fn fact(
        value: RLocalId,
        aggregates: &RuntimeAggregateFacts,
        constants: &RuntimeScalarConstFacts,
        visiting: &mut FxHashSet<RLocalId>,
        fuel: u8,
    ) -> RuntimeArgFact {
        if fuel == 0 || !visiting.insert(value) {
            return RuntimeArgFact::Unknown;
        }
        let result = if let Some(value) = constants.get(&value) {
            RuntimeArgFact::ScalarConst(value.clone())
        } else if let Some(fields) = aggregates.get(&value) {
            RuntimeArgFact::Aggregate(
                fields
                    .iter()
                    .map(|field| fact(*field, aggregates, constants, visiting, fuel - 1))
                    .collect(),
            )
        } else {
            RuntimeArgFact::Unknown
        };
        visiting.remove(&value);
        result
    }
    RuntimeArgShapeKey(
        args.iter()
            .map(|arg| fact(*arg, aggregates, constants, &mut FxHashSet::default(), 32))
            .collect(),
    )
}

/// Forward projections through structurally known aggregate values, then drop
/// pure assignments that do not contribute to `root`.
///
/// This deliberately accepts only the value-only expression vocabulary used
/// by the restricted Runtime MIR inliner. Effects fail closed. No arithmetic
/// identity or constant folding is performed.
pub fn specialize_pure_inline_stmts<'db>(
    stmts: Vec<RStmt<'db>>,
    external: &RuntimeAggregateFacts,
    root: RLocalId,
) -> Option<Vec<RStmt<'db>>> {
    // The projection forwarding and reverse liveness pass below operate on
    // value identities. Runtime MIR locals are variables rather than SSA
    // values, so a repeated destination would make both assumptions false:
    // an earlier aggregate snapshot could be confused with its reassigned
    // value, and reverse liveness could retain the wrong definition. This is
    // an optional inline specialization, so fail closed and leave the call in
    // the program when a body contains mutation.
    let mut assigned = FxHashSet::default();
    for stmt in &stmts {
        let RStmt::Assign { dst, .. } = stmt else {
            return None;
        };
        if !assigned.insert(*dst) {
            return None;
        }
    }

    let mut aggregates = external.clone();
    let mut aliases = FxHashMap::default();
    let mut projections = FxHashMap::default();
    let mut rewritten = Vec::with_capacity(stmts.len());

    for stmt in stmts {
        let RStmt::Assign { dst, mut expr } = stmt else {
            return None;
        };
        rewrite_alias_inputs(&mut expr, &aliases)?;
        if let RExpr::AggregateExtract { value, index } = expr {
            if let Some(fields) = aggregates.get(&value) {
                expr = RExpr::Use(*fields.get(index as usize)?);
            } else if let Some(previous) = projections.get(&(value, index)).copied() {
                expr = RExpr::Use(previous);
            } else {
                expr = RExpr::AggregateExtract { value, index };
                projections.insert((value, index), dst);
            }
        }
        match &expr {
            RExpr::Use(value) => {
                aliases.insert(dst, resolve_alias(*value, &aliases));
            }
            RExpr::AggregateMake { fields, .. } => {
                aggregates.insert(dst, fields.clone());
            }
            _ => {}
        }
        if !is_pure_inline_expr(&expr) {
            return None;
        }
        rewritten.push(RStmt::Assign { dst, expr });
    }

    let mut live = FxHashSet::default();
    live.insert(root);
    let mut kept = Vec::new();
    for stmt in rewritten.into_iter().rev() {
        let RStmt::Assign { dst, expr } = &stmt else {
            return None;
        };
        if live.remove(dst) || !safe_to_drop_when_dead(expr) {
            add_expr_inputs(expr, &mut live)?;
            kept.push(stmt);
        }
    }
    kept.reverse();
    Some(kept)
}

/// Only structural value plumbing is known non-trapping. Arithmetic, casts,
/// and builtins remain in the program even when their result is dead.
fn safe_to_drop_when_dead(expr: &RExpr<'_>) -> bool {
    matches!(
        expr,
        RExpr::Use(_)
            | RExpr::ConstScalar(_)
            | RExpr::AggregateMake { .. }
            | RExpr::AggregateExtract { .. }
    )
}

fn resolve_alias(mut value: RLocalId, aliases: &FxHashMap<RLocalId, RLocalId>) -> RLocalId {
    let mut seen = FxHashSet::default();
    while seen.insert(value) {
        let Some(next) = aliases.get(&value).copied() else {
            break;
        };
        value = next;
    }
    value
}

fn rewrite_alias_inputs(
    expr: &mut RExpr<'_>,
    aliases: &FxHashMap<RLocalId, RLocalId>,
) -> Option<()> {
    let resolve = |value: &mut RLocalId| *value = resolve_alias(*value, aliases);
    match expr {
        RExpr::Use(value)
        | RExpr::Unary { value, .. }
        | RExpr::Cast { value, .. }
        | RExpr::Bitcast { value, .. } => resolve(value),
        RExpr::Binary { lhs, rhs, .. } => {
            resolve(lhs);
            resolve(rhs);
        }
        RExpr::Builtin(builtin) => match builtin {
            RuntimeBuiltin::IntrinsicArith { lhs, rhs, .. } => {
                resolve(lhs);
                resolve(rhs);
            }
            RuntimeBuiltin::F32FromI32 { value }
            | RuntimeBuiltin::I32FromF32 { value }
            | RuntimeBuiltin::F32Sqrt { value }
            | RuntimeBuiltin::F32Abs { value }
            | RuntimeBuiltin::F32Floor { value }
            | RuntimeBuiltin::F32Ceil { value }
            | RuntimeBuiltin::F32Trunc { value }
            | RuntimeBuiltin::F32Round { value } => resolve(value),
            RuntimeBuiltin::F32Min { lhs, rhs }
            | RuntimeBuiltin::F32Max { lhs, rhs }
            | RuntimeBuiltin::F32MinRelaxed { lhs, rhs }
            | RuntimeBuiltin::F32MaxRelaxed { lhs, rhs } => {
                resolve(lhs);
                resolve(rhs);
            }
            RuntimeBuiltin::F32Clamp { value, lo, hi } => {
                resolve(value);
                resolve(lo);
                resolve(hi);
            }
            _ => return None,
        },
        RExpr::AggregateMake { fields, .. } => fields.iter_mut().for_each(resolve),
        RExpr::AggregateExtract { value, .. } => resolve(value),
        RExpr::ConstScalar(_) => {}
        _ => return None,
    }
    Some(())
}

fn is_pure_inline_expr(expr: &RExpr<'_>) -> bool {
    matches!(
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
            )
            | RExpr::AggregateMake { .. }
            | RExpr::AggregateExtract { .. }
    )
}

fn add_expr_inputs(expr: &RExpr<'_>, live: &mut FxHashSet<RLocalId>) -> Option<()> {
    match expr {
        RExpr::Use(value)
        | RExpr::Unary { value, .. }
        | RExpr::Cast { value, .. }
        | RExpr::Bitcast { value, .. } => {
            live.insert(*value);
        }
        RExpr::Binary { lhs, rhs, .. } => {
            live.insert(*lhs);
            live.insert(*rhs);
        }
        RExpr::Builtin(builtin) => match builtin {
            RuntimeBuiltin::IntrinsicArith { lhs, rhs, .. } => {
                live.insert(*lhs);
                live.insert(*rhs);
            }
            RuntimeBuiltin::F32FromI32 { value }
            | RuntimeBuiltin::I32FromF32 { value }
            | RuntimeBuiltin::F32Sqrt { value }
            | RuntimeBuiltin::F32Abs { value } => {
                live.insert(*value);
            }
            RuntimeBuiltin::F32Min { lhs, rhs }
            | RuntimeBuiltin::F32Max { lhs, rhs }
            | RuntimeBuiltin::F32MinRelaxed { lhs, rhs }
            | RuntimeBuiltin::F32MaxRelaxed { lhs, rhs } => {
                live.insert(*lhs);
                live.insert(*rhs);
            }
            RuntimeBuiltin::F32Clamp { value, lo, hi } => {
                live.insert(*value);
                live.insert(*lo);
                live.insert(*hi);
            }
            _ => return None,
        },
        RExpr::AggregateMake { fields, .. } => live.extend(fields.iter().copied()),
        RExpr::AggregateExtract { value, .. } => {
            live.insert(*value);
        }
        RExpr::ConstScalar(_) => {}
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> RLocalId {
        RLocalId::from_u32(n)
    }

    #[test]
    fn forwards_nested_aggregate_projection_and_keeps_only_dynamic_leaf() {
        let mut facts = RuntimeAggregateFacts::default();
        facts.insert(id(0), vec![id(1), id(2)].into_boxed_slice());
        facts.insert(id(1), vec![id(3), id(4)].into_boxed_slice());
        let stmts = vec![
            RStmt::Assign {
                dst: id(5),
                expr: RExpr::AggregateExtract {
                    value: id(0),
                    index: 0,
                },
            },
            RStmt::Assign {
                dst: id(6),
                expr: RExpr::AggregateExtract {
                    value: id(5),
                    index: 1,
                },
            },
            RStmt::Assign {
                dst: id(7),
                expr: RExpr::Use(id(6)),
            },
            RStmt::Assign {
                dst: id(8),
                expr: RExpr::Use(id(2)),
            },
        ];
        let got = specialize_pure_inline_stmts(stmts, &facts, id(7)).unwrap();
        assert_eq!(got.len(), 1);
        assert!(
            matches!(got[0], RStmt::Assign { dst, expr: RExpr::Use(v) } if dst == id(7) && v == id(4))
        );
    }

    #[test]
    fn effect_statement_fails_closed() {
        let stmts = vec![RStmt::Store {
            dst: super::super::RuntimePlace {
                root: super::super::PlaceRoot::Slot(id(1)),
                path: Box::new([]),
            },
            src: id(0),
        }];
        assert!(
            specialize_pure_inline_stmts(stmts, &RuntimeAggregateFacts::default(), id(0)).is_none()
        );
    }

    #[test]
    fn reuses_identical_unknown_aggregate_projection() {
        let stmts = vec![
            RStmt::Assign {
                dst: id(1),
                expr: RExpr::AggregateExtract {
                    value: id(0),
                    index: 3,
                },
            },
            RStmt::Assign {
                dst: id(2),
                expr: RExpr::AggregateExtract {
                    value: id(0),
                    index: 3,
                },
            },
        ];
        let got = specialize_pure_inline_stmts(stmts, &RuntimeAggregateFacts::default(), id(2))
            .expect("pure duplicate projections should specialize");
        assert_eq!(got.len(), 2);
        assert!(
            matches!(got[0], RStmt::Assign { dst, expr: RExpr::AggregateExtract { .. } } if dst == id(1))
        );
        assert!(
            matches!(got[1], RStmt::Assign { dst, expr: RExpr::Use(value) } if dst == id(2) && value == id(1))
        );
    }

    #[test]
    fn repeated_destination_fails_closed_instead_of_reusing_a_stale_value() {
        let stmts = vec![
            RStmt::Assign {
                dst: id(0),
                expr: RExpr::ConstScalar(ConstScalar::Bool(false)),
            },
            RStmt::Assign {
                dst: id(1),
                expr: RExpr::Use(id(0)),
            },
            RStmt::Assign {
                dst: id(0),
                expr: RExpr::ConstScalar(ConstScalar::Bool(true)),
            },
            RStmt::Assign {
                dst: id(2),
                expr: RExpr::Use(id(1)),
            },
        ];

        assert!(
            specialize_pure_inline_stmts(stmts, &RuntimeAggregateFacts::default(), id(2)).is_none(),
            "mutation must retain the unspecialized Runtime MIR body",
        );
    }

    #[test]
    fn retains_dead_checked_arithmetic_division_and_conversion() {
        let i32_class = super::super::ScalarClass {
            repr: super::super::ScalarRepr::Int {
                bits: 32,
                signed: true,
            },
            role: super::super::ScalarRole::Plain,
        };
        let stmts = vec![
            RStmt::Assign {
                dst: id(10),
                expr: RExpr::Builtin(RuntimeBuiltin::IntrinsicArith {
                    op: super::super::IntrinsicArithBinOp::Add,
                    checked: true,
                    lhs: id(1),
                    rhs: id(2),
                    class: i32_class.clone(),
                }),
            },
            RStmt::Assign {
                dst: id(11),
                expr: RExpr::Builtin(RuntimeBuiltin::IntrinsicArith {
                    op: super::super::IntrinsicArithBinOp::Div,
                    checked: false,
                    lhs: id(3),
                    rhs: id(4),
                    class: i32_class,
                }),
            },
            RStmt::Assign {
                dst: id(12),
                expr: RExpr::Builtin(RuntimeBuiltin::I32FromF32 { value: id(5) }),
            },
            RStmt::Assign {
                dst: id(20),
                expr: RExpr::Use(id(0)),
            },
        ];
        let got =
            specialize_pure_inline_stmts(stmts, &RuntimeAggregateFacts::default(), id(20)).unwrap();
        assert_eq!(
            got.len(),
            4,
            "potentially trapping dead expressions must remain"
        );
        assert!(matches!(
            got[0],
            RStmt::Assign {
                expr: RExpr::Builtin(RuntimeBuiltin::IntrinsicArith { checked: true, .. }),
                ..
            }
        ));
        assert!(matches!(
            got[1],
            RStmt::Assign {
                expr: RExpr::Builtin(RuntimeBuiltin::IntrinsicArith {
                    op: super::super::IntrinsicArithBinOp::Div,
                    ..
                }),
                ..
            }
        ));
        assert!(matches!(
            got[2],
            RStmt::Assign {
                expr: RExpr::Builtin(RuntimeBuiltin::I32FromF32 { .. }),
                ..
            }
        ));
    }

    #[test]
    fn argument_shape_keys_separate_nested_support_and_scalar_constants() {
        let mut aggregates = RuntimeAggregateFacts::default();
        aggregates.insert(id(0), vec![id(1), id(2)].into_boxed_slice());
        aggregates.insert(id(3), vec![id(1), id(4)].into_boxed_slice());
        let mut constants = RuntimeScalarConstFacts::default();
        constants.insert(id(2), super::super::ConstScalar::Float { bits: 0 });
        constants.insert(
            id(4),
            super::super::ConstScalar::Float {
                bits: 1.0f32.to_bits(),
            },
        );
        let zero = runtime_arg_shape_key(&[id(0)], &aggregates, &constants);
        let one = runtime_arg_shape_key(&[id(3)], &aggregates, &constants);
        let unknown = runtime_arg_shape_key(&[id(9)], &aggregates, &constants);
        assert_ne!(zero, one);
        assert_ne!(zero, unknown);
        assert_eq!(
            unknown,
            RuntimeArgShapeKey(vec![RuntimeArgFact::Unknown].into_boxed_slice())
        );
    }

    #[test]
    fn cyclic_argument_facts_terminate_at_unknown() {
        let mut aggregates = RuntimeAggregateFacts::default();
        aggregates.insert(id(0), vec![id(1)].into_boxed_slice());
        aggregates.insert(id(1), vec![id(0)].into_boxed_slice());

        assert_eq!(
            runtime_arg_shape_key(&[id(0)], &aggregates, &RuntimeScalarConstFacts::default()),
            RuntimeArgShapeKey(
                vec![RuntimeArgFact::Aggregate(
                    vec![RuntimeArgFact::Aggregate(
                        vec![RuntimeArgFact::Unknown].into_boxed_slice(),
                    )]
                    .into_boxed_slice(),
                )]
                .into_boxed_slice(),
            )
        );
    }

    #[test]
    fn deeply_nested_argument_facts_stop_at_fuel_boundary() {
        let mut aggregates = RuntimeAggregateFacts::default();
        for index in 0..40 {
            aggregates.insert(id(index), vec![id(index + 1)].into_boxed_slice());
        }
        let key = runtime_arg_shape_key(&[id(0)], &aggregates, &RuntimeScalarConstFacts::default());
        let mut fact = &key.0[0];
        for _ in 0..32 {
            let RuntimeArgFact::Aggregate(fields) = fact else {
                panic!("expected aggregate before the fuel boundary, got {fact:?}");
            };
            assert_eq!(fields.len(), 1);
            fact = &fields[0];
        }
        assert_eq!(fact, &RuntimeArgFact::Unknown);
    }
}
