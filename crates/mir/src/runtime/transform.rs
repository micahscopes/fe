use rustc_hash::{FxHashMap, FxHashSet};

use super::{RExpr, RLocalId, RStmt, RuntimeBuiltin};

/// Structurally known aggregate fields at a callsite. This records identity,
/// not scalar values, so it makes no arithmetic or floating-point assumptions.
pub type RuntimeAggregateFacts = FxHashMap<RLocalId, Box<[RLocalId]>>;

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
    let mut aggregates = external.clone();
    let mut aliases = FxHashMap::default();
    let mut rewritten = Vec::with_capacity(stmts.len());

    for stmt in stmts {
        let RStmt::Assign { dst, mut expr } = stmt else {
            return None;
        };
        if let RExpr::AggregateExtract { value, index } = expr {
            let value = resolve_alias(value, &aliases);
            if let Some(fields) = aggregates.get(&value) {
                expr = RExpr::Use(*fields.get(index as usize)?);
            } else {
                expr = RExpr::AggregateExtract { value, index };
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

fn is_pure_inline_expr(expr: &RExpr<'_>) -> bool {
    matches!(
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
}

fn add_expr_inputs(expr: &RExpr<'_>, live: &mut FxHashSet<RLocalId>) -> Option<()> {
    match expr {
        RExpr::Use(value) | RExpr::Unary { value, .. } | RExpr::Cast { value, .. } => {
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
            | RuntimeBuiltin::F32Sqrt { value } => {
                live.insert(*value);
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
        assert_eq!(got.len(), 2);
        assert!(matches!(got[0], RStmt::Assign { expr: RExpr::Use(v), .. } if v == id(4)));
        assert!(matches!(got[1], RStmt::Assign { expr: RExpr::Use(v), .. } if v == id(6)));
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
}
