//! Format-v1 projections of retained compiler decisions. No decision is solved
//! again here: unsupported terms must be reported instead of silently omitted.
use hir::{
    analysis::{
        diagnostics::SpannedHirAnalysisDb,
        semantic::{SemConstScalar, SemConstValue},
        ty::{
            const_ty::ConstTyData,
            trait_def::{ImplementorOrigin, TraitInstId},
            ty_check::{
                Discharge, EvaluationStop, PredicateKey, PredicateTerm, RequirementFailure,
                RequirementProof, TraitChoiceVia, TypedBody,
            },
            ty_def::{PrimTy, TyBase, TyData, TyId},
        },
    },
    hir_def::{ArithBinOp, BinOp, ItemKind, LitKind, LogicalBinOp, UnOp, WhereClauseOwner},
    span::{DynLazySpan, LazySpan},
};
use serde_json::{Value, json};

/// Encode the body's accepted goal answers, including their authored use spans.
/// The answer's implementation identity comes only from the retained record.
pub fn goal_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: &TypedBody<'db>,
) -> Result<Vec<Value>, String> {
    body.decisions()
        .choice_entries(db)
        .map(|choice| {
            let span = choice.span().resolve(db).ok_or("goal has no source span")?;
            let site = source_site(db, choice.span())?;
            let via = match choice.answer_origin(db) {
                ImplementorOrigin::Assumption => json!("bound"),
                ImplementorOrigin::Hir(impl_) => {
                    if impl_
                        .span()
                        .resolve(db)
                        .is_none_or(|declaration| declaration.file != span.file)
                    {
                        return Err("implementation from another file is outside format v1".into());
                    }
                    match choice.via() {
                        TraitChoiceVia::Implementation(_) => {
                            json!({"impl": impl_.declaration_index(db)})
                        }
                        TraitChoiceVia::Selected(_) => {
                            json!({"selected": impl_.declaration_index(db)})
                        }
                    }
                }
                ImplementorOrigin::VirtualContract(_) => {
                    return Err("virtual contract answer is outside format v1".into());
                }
            };
            Ok(json!({
                "v": 1, "kind": "goal", "site": site,
                "goal": goal(db, choice.goal())?, "via": via,
            }))
        })
        .collect()
}

fn goal<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    instance: TraitInstId<'db>,
) -> Result<Value, String> {
    let name = instance.def(db).name(db).to_opt().ok_or("unnamed trait")?;
    let args = instance.args(db);
    Ok(json!({
        "self": term(db, instance.self_ty(db))?, "trait": name.data(db),
        "args": args.iter().skip(1).map(|ty| term(db, *ty)).collect::<Result<Vec<_>, _>>()?,
    }))
}

fn term<'db>(db: &'db dyn SpannedHirAnalysisDb, ty: TyId<'db>) -> Result<Value, String> {
    match ty.data(db) {
        TyData::TyParam(param) => Ok(json!({"var": param.name.data(db), "kind": "type"})),
        TyData::ConstTy(constant) => match constant.data(db) {
            ConstTyData::TyParam(param, ty) => {
                let kind = if matches!(ty.data(db), TyData::TyBase(TyBase::Prim(PrimTy::Bool))) {
                    "bool"
                } else {
                    "nat"
                };
                Ok(json!({"var": param.name.data(db), "kind": kind}))
            }
            ConstTyData::Value(value) => match value.value().value(db) {
                SemConstValue::Scalar {
                    value: SemConstScalar::Bool(value),
                    ..
                } => Ok(json!({"bool": value})),
                SemConstValue::Scalar {
                    value: SemConstScalar::Int { value },
                    ..
                } => {
                    let value = value
                        .to_string()
                        .parse::<u64>()
                        .map_err(|_| "integer is outside the v1 encoder's natural-number range")?;
                    Ok(json!({"nat": value}))
                }
                _ => Err("aggregate constant is outside format v1".into()),
            },
            _ => Err("unevaluated type argument needs a format-v1 term projection".into()),
        },
        TyData::TyBase(_) | TyData::TyApp(_, _) => {
            let base = ty.base_ty(db);
            if !matches!(base.data(db), TyData::TyBase(_)) {
                return Err("higher-kinded type argument is outside format v1".into());
            }
            Ok(json!({
                "con": base.pretty_print(db),
                "args": ty.generic_args(db).iter().map(|ty| term(db, *ty)).collect::<Result<Vec<_>, _>>()?,
            }))
        }
        _ => Err("type argument is outside format v1".into()),
    }
}

/// Project the predicates, selection context and actual discharge retained at uses.
pub fn condition_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: &TypedBody<'db>,
) -> Result<Vec<Value>, String> {
    body.decisions().condition_entries().map(|(_, use_)| {
        let file = use_.span().resolve(db).ok_or("condition has no source span")?.file;
        if ItemKind::from(use_.declaration()).span().resolve(db).is_none_or(|declaration| declaration.file != file) {
            return Err("condition declaration from another file is outside format v1".into());
        }
        let source = match use_.declaration() {
            WhereClauseOwner::ImplTrait(impl_) => format!("impl #{}", impl_.declaration_index(db)),
            owner @ (WhereClauseOwner::Func(_) | WhereClauseOwner::Struct(_) | WhereClauseOwner::Trait(_)) => {
                let item = ItemKind::from(owner);
                let name = item.name(db).ok_or("condition declaration has no name")?.data(db);
                match item {
                    ItemKind::Func(func) => match func.scope().parent_item(db) {
                        Some(ItemKind::Trait(trait_)) => format!("trait {}::{name}", trait_.name(db).to_opt().ok_or("unnamed trait")?.data(db)),
                        Some(ItemKind::Impl(_) | ItemKind::ImplTrait(_)) => return Err("implementation method condition is outside format v1".into()),
                        _ => format!("fn {name}"),
                    },
                    ItemKind::Struct(_) => format!("struct {name}"),
                    _ => return Err("trait-level condition is outside format v1".into()),
                }
            }
            _ => return Err("condition declaration is outside format v1".into()),
        };
        let pred = predicate(db, use_.predicate().ok_or("condition has no retained predicate")?)?;
        let outcome = match use_.outcome() {
            Discharge::Holds(RequirementProof::Evaluated) => "eval-true".to_owned(),
            Discharge::Holds(RequirementProof::Premise(_)) => format!("premise:{}", use_.premise_index(db).ok_or("inherited premise has no local format-v1 index")?),
            Discharge::Fails(RequirementFailure::False) => "eval-false".to_owned(),
            Discharge::Fails(RequirementFailure::NoMatchingPremise | RequirementFailure::NotForwardable) => "no-premise".to_owned(),
            Discharge::Fails(RequirementFailure::Evaluation(EvaluationStop::DivisionByZero)) => "fault:div-zero".to_owned(),
            Discharge::Fails(RequirementFailure::NotEstablished | RequirementFailure::NotInstantiable) => "fault:ill-typed".to_owned(),
            _ => return Err("condition failure needs a format-v1 outcome projection".into()),
        };
        let sels = use_.selections(db).iter().map(|selection| match selection.origin(db) {
            ImplementorOrigin::Hir(impl_) => {
                if impl_.span().resolve(db).is_none_or(|declaration| declaration.file != file) {
                    return Err("condition selection from another file is outside format v1");
                }
                Ok(impl_.declaration_index(db))
            },
            _ => Err("non-HIR selection is outside format v1"),
        }).collect::<Result<Vec<_>, _>>()?;
        Ok(json!({"v":1,"kind":"condition","site":source_site(db,use_.span())?,"source":source,"pred":pred,"sels":sels,"outcome":outcome}))
    }).collect()
}

fn source_site<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    span: &DynLazySpan<'db>,
) -> Result<Value, String> {
    let span = span.resolve(db).ok_or("decision has no source span")?;
    let prefix = span
        .file
        .text(db)
        .get(..usize::from(span.range.start()))
        .ok_or("invalid decision source offset")?;
    Ok(
        json!({"line":prefix.bytes().filter(|byte| *byte == b'\n').count()+1,"col":prefix.rsplit('\n').next().unwrap().chars().count()+1}),
    )
}

fn predicate<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    key: &PredicateKey<'db>,
) -> Result<Value, String> {
    match key.term() {
        PredicateTerm::Literal(LitKind::Bool(value)) => Ok(json!({"bool":value})),
        PredicateTerm::Literal(LitKind::Int(value)) => Ok(
            json!({"nat":value.data(db).to_string().parse::<u64>().map_err(|_| "integer outside natural-number encoder range")?}),
        ),
        PredicateTerm::TypeConst(ty) => term(db, *ty),
        PredicateTerm::TraitConst(instance, name) => {
            let goal = goal(db, *instance)?;
            Ok(
                json!({"assoc":goal["self"],"trait":goal["trait"],"targs":goal["args"],"c":name.data(db)}),
            )
        }
        PredicateTerm::Binary(op, a, b) => {
            let op = match op {
                BinOp::Comp(op) => op.symbol(),
                BinOp::Logical(LogicalBinOp::And) => "&&",
                BinOp::Logical(LogicalBinOp::Or) => "||",
                BinOp::Arith(ArithBinOp::Add) => "+",
                BinOp::Arith(ArithBinOp::Sub) => "-",
                BinOp::Arith(ArithBinOp::Mul) => "*",
                BinOp::Arith(ArithBinOp::Div) => "/",
                _ => return Err("predicate operator is outside format v1".into()),
            };
            Ok(json!({"op":op,"a":predicate(db,a)?,"b":predicate(db,b)?}))
        }
        PredicateTerm::Unary(UnOp::Not, value) => Ok(json!({"not":predicate(db,value)?})),
        _ => Err("predicate term is outside format v1".into()),
    }
}
