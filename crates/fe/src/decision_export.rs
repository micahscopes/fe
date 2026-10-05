//! Format-v1 projections of retained compiler decisions. No decision is solved
//! again here: unsupported terms must be reported instead of silently omitted.
use hir::core::semantic::Provision;
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
    hir_def::{
        ArithBinOp, BinOp, Body, Expr, ItemKind, LitKind, LogicalBinOp, Partial, UnOp,
        WhereClauseOwner,
    },
    span::{DynLazySpan, LazySpan},
};
mod program;
pub mod records;
pub use program::declaration_records;
use records::{Goal, Kind, OpeningOutcome, Record, Site, Term, Via};

/// Why a construct cannot be written in format v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportGap(pub &'static str);

impl From<&'static str> for ExportGap {
    fn from(reason: &'static str) -> Self {
        Self(reason)
    }
}

impl std::fmt::Display for ExportGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

type Result<T, E = ExportGap> = std::result::Result<T, E>;

/// A decision that format v1 cannot express, at its site when it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionGap {
    pub site: Option<Site>,
    pub reason: &'static str,
}

/// One decision record, or why the decision cannot be written.
pub type Decision = Result<Record, DecisionGap>;

/// Build the record of a decision taken at `site`, keeping the site of a gap.
fn locate(site: Result<Site>, record: impl FnOnce(Site) -> Result<Record>) -> Decision {
    let site = site.map_err(|ExportGap(reason)| DecisionGap { site: None, reason })?;
    record(site).map_err(|ExportGap(reason)| DecisionGap {
        site: Some(site),
        reason,
    })
}

/// Encode the body's accepted goal answers, including their authored use spans.
/// The answer's implementation identity comes only from the retained record.
pub fn goal_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: &TypedBody<'db>,
) -> Vec<Decision> {
    body.decisions()
        .choice_entries(db)
        .map(|choice| {
            locate(source_site(db, choice.span()), |site| {
                let span = choice.span().resolve(db).ok_or("goal has no source span")?;
                let via = match choice.answer_origin(db) {
                    ImplementorOrigin::Assumption => Via::Bound,
                    ImplementorOrigin::Hir(impl_) => {
                        if impl_
                            .span()
                            .resolve(db)
                            .is_none_or(|declaration| declaration.file != span.file)
                        {
                            return Err(
                                "implementation from another file is outside format v1".into()
                            );
                        }
                        match choice.via() {
                            TraitChoiceVia::Implementation(_) => {
                                Via::Impl(impl_.declaration_index(db))
                            }
                            TraitChoiceVia::Selected(_) => {
                                Via::Selected(impl_.declaration_index(db))
                            }
                        }
                    }
                    ImplementorOrigin::VirtualContract(_) => {
                        return Err("virtual contract answer is outside format v1".into());
                    }
                };
                Ok(Record::Goal {
                    site,
                    goal: goal(db, choice.goal())?,
                    via,
                })
            })
        })
        .collect()
}

fn goal<'db>(db: &'db dyn SpannedHirAnalysisDb, instance: TraitInstId<'db>) -> Result<Goal> {
    let name = instance.def(db).name(db).to_opt().ok_or("unnamed trait")?;
    Ok(Goal {
        self_ty: term(db, instance.self_ty(db))?,
        trait_: name.data(db).clone(),
        args: instance
            .args(db)
            .iter()
            .skip(1)
            .map(|ty| term(db, *ty))
            .collect::<Result<_>>()?,
    })
}

fn term<'db>(db: &'db dyn SpannedHirAnalysisDb, ty: TyId<'db>) -> Result<Term> {
    match ty.data(db) {
        TyData::TyParam(param) => Ok(Term::Var {
            var: param.name.data(db).clone(),
            kind: Some(Kind::Type),
        }),
        TyData::ConstTy(constant) => match constant.data(db) {
            ConstTyData::TyParam(param, ty) => {
                let kind = match ty.data(db) {
                    TyData::TyBase(TyBase::Prim(PrimTy::Bool)) => Kind::Bool,
                    TyData::TyBase(TyBase::Prim(PrimTy::Usize)) => Kind::Nat,
                    _ => return Err("constant kind is outside the v1 fragment (bool/usize)".into()),
                };
                Ok(Term::Var {
                    var: param.name.data(db).clone(),
                    kind: Some(kind),
                })
            }
            ConstTyData::Value(value) => match value.value().value(db) {
                SemConstValue::Scalar {
                    value: SemConstScalar::Bool(value),
                    ..
                } => Ok(Term::Bool { bool: *value }),
                SemConstValue::Scalar {
                    value: SemConstScalar::Int { value },
                    ..
                } => Ok(Term::Nat {
                    nat: value
                        .to_string()
                        .parse()
                        .map_err(|_| "integer is outside the v1 encoder's natural-number range")?,
                }),
                _ => Err("aggregate constant is outside format v1".into()),
            },
            _ => Err("unevaluated type argument needs a format-v1 term projection".into()),
        },
        TyData::TyBase(_) | TyData::TyApp(_, _) => {
            let base = ty.base_ty(db);
            if !matches!(base.data(db), TyData::TyBase(_)) {
                return Err("higher-kinded type argument is outside format v1".into());
            }
            Ok(Term::Con {
                con: base.pretty_print(db).to_string(),
                args: ty
                    .generic_args(db)
                    .iter()
                    .map(|ty| term(db, *ty))
                    .collect::<Result<_>>()?,
            })
        }
        _ => Err("type argument is outside format v1".into()),
    }
}

/// Project the predicates, selection context and actual discharge retained at uses.
pub fn condition_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: &TypedBody<'db>,
) -> Vec<Decision> {
    body.decisions()
        .condition_entries()
        .map(|(_, use_)| {
            locate(source_site(db, use_.span()), |site| {
                let file = use_
                    .span()
                    .resolve(db)
                    .ok_or("condition has no source span")?
                    .file;
                let in_other_file = |item: ItemKind<'db>| {
                    item.span()
                        .resolve(db)
                        .is_none_or(|declaration| declaration.file != file)
                };
                if in_other_file(ItemKind::from(use_.declaration())) {
                    return Err(
                        "condition declaration from another file is outside format v1".into(),
                    );
                }
                let source = condition_source(db, use_.declaration())?;
                let pred = predicate(
                    db,
                    use_.predicate()
                        .ok_or("condition has no retained predicate")?,
                )?;
                let outcome = match use_.outcome() {
                    Discharge::Holds(RequirementProof::Evaluated) => "eval-true".to_owned(),
                    Discharge::Holds(RequirementProof::Premise(_)) => format!(
                        "premise:{}",
                        use_.premise_index(db)
                            .ok_or("inherited premise has no local format-v1 index")?
                    ),
                    Discharge::Fails(RequirementFailure::False) => "eval-false".to_owned(),
                    Discharge::Fails(
                        RequirementFailure::NoMatchingPremise | RequirementFailure::NotForwardable,
                    ) => "no-premise".to_owned(),
                    Discharge::Fails(RequirementFailure::Evaluation(
                        EvaluationStop::DivisionByZero,
                    )) => "fault:div-zero".to_owned(),
                    Discharge::Fails(
                        RequirementFailure::NotEstablished | RequirementFailure::NotInstantiable,
                    ) => "fault:ill-typed".to_owned(),
                    _ => {
                        return Err("condition failure needs a format-v1 outcome projection".into());
                    }
                };
                let sels =
                    use_.selections(db)
                        .iter()
                        .map(|selection| match selection.origin(db) {
                            ImplementorOrigin::Hir(impl_) => {
                                if impl_
                                    .span()
                                    .resolve(db)
                                    .is_none_or(|declaration| declaration.file != file)
                                {
                                    return Err(
                                "condition selection from another file is outside format v1".into(),
                            );
                                }
                                Ok(impl_.declaration_index(db))
                            }
                            _ => Err("non-HIR selection is outside format v1".into()),
                        })
                        .collect::<Result<_>>()?;
                Ok(Record::Condition {
                    site,
                    source,
                    pred,
                    sels,
                    outcome,
                })
            })
        })
        .collect()
}

/// The declaration a condition comes from: `impl #i`, `struct S`, `fn f` or `trait Tr::m`.
fn condition_source<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    owner: WhereClauseOwner<'db>,
) -> Result<String> {
    let item = match owner {
        WhereClauseOwner::ImplTrait(impl_) => {
            return Ok(format!("impl #{}", impl_.declaration_index(db)));
        }
        WhereClauseOwner::Func(_) | WhereClauseOwner::Struct(_) | WhereClauseOwner::Trait(_) => {
            ItemKind::from(owner)
        }
        _ => return Err("condition declaration is outside format v1".into()),
    };
    let name = item
        .name(db)
        .ok_or("condition declaration has no name")?
        .data(db);
    match item {
        ItemKind::Func(func) => match func.scope().parent_item(db) {
            Some(ItemKind::Trait(trait_)) => {
                let trait_name = trait_.name(db).to_opt().ok_or("unnamed trait")?;
                Ok(format!("trait {}::{name}", trait_name.data(db)))
            }
            Some(ItemKind::Impl(_) | ItemKind::ImplTrait(_)) => {
                Err("implementation method condition is outside format v1".into())
            }
            _ => Ok(format!("fn {name}")),
        },
        ItemKind::Struct(_) => Ok(format!("struct {name}")),
        _ => Err("trait-level condition is outside format v1".into()),
    }
}

fn source_site<'db>(db: &'db dyn SpannedHirAnalysisDb, span: &DynLazySpan<'db>) -> Result<Site> {
    let span = span.resolve(db).ok_or("decision has no source span")?;
    span_site(db, &span)
}

fn span_site(db: &dyn SpannedHirAnalysisDb, span: &common::diagnostics::Span) -> Result<Site> {
    let prefix = span
        .file
        .text(db)
        .get(..usize::from(span.range.start()))
        .ok_or("invalid decision source offset")?;
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    Ok(Site {
        line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        col: prefix[line_start..].chars().count() + 1,
    })
}

fn predicate<'db>(db: &'db dyn SpannedHirAnalysisDb, key: &PredicateKey<'db>) -> Result<Term> {
    match key.term() {
        PredicateTerm::Literal(LitKind::Bool(value)) => Ok(Term::Bool { bool: *value }),
        PredicateTerm::Literal(LitKind::Int(value)) => Ok(Term::Nat {
            nat: value
                .data(db)
                .to_string()
                .parse()
                .map_err(|_| "integer outside natural-number encoder range")?,
        }),
        PredicateTerm::TypeConst(ty) => term(db, *ty),
        PredicateTerm::TraitConst(instance, name) => {
            Ok(Term::assoc(goal(db, *instance)?, name.data(db).clone()))
        }
        PredicateTerm::Binary(op, a, b) => Ok(Term::Op {
            op: binary_operator(*op)?,
            a: Box::new(predicate(db, a)?),
            b: Box::new(predicate(db, b)?),
        }),
        PredicateTerm::Unary(UnOp::Not, value) => Ok(Term::Not {
            not: Box::new(predicate(db, value)?),
        }),
        _ => Err("predicate term is outside format v1".into()),
    }
}

/// Read recorded named openings. Evidence openings remain outside format v1.
pub fn with_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    owner: Body<'db>,
    body: &TypedBody<'db>,
) -> Vec<Decision> {
    body.with_choices()
        .map(|choice| {
            let expression = choice.expression();
            let span: DynLazySpan<'db> = expression.span(owner).into();
            locate(source_site(db, &span), |site| {
                let Partial::Present(Expr::Path(path)) = expression.data(db, owner) else {
                    return Err("non-path opening is outside format v1".into());
                };
                let name = path
                    .to_opt()
                    .and_then(|path| path.as_ident(db))
                    .ok_or("qualified opening is outside format v1")?;
                let file = span.resolve(db).ok_or("opening has no source span")?.file;
                let outcome = match choice.outcome() {
                    Ok(Provision::Named(selection)) => match selection.origin(db) {
                        ImplementorOrigin::Hir(impl_) => {
                            if impl_
                                .span()
                                .resolve(db)
                                .is_none_or(|declaration| declaration.file != file)
                            {
                                return Err("opening from another file is outside format v1".into());
                            }
                            OpeningOutcome::Impl(impl_.declaration_index(db))
                        }
                        _ => return Err("non-HIR opening is outside format v1".into()),
                    },
                    Ok(Provision::Bound(_)) => {
                        unreachable!("root premises cannot be lexical opening records")
                    }
                    Ok(Provision::EvidenceLocal(..) | Provision::EvidenceParameter(..)) => {
                        return Err("evidence opening is outside format v1".into());
                    }
                    Err(_) => OpeningOutcome::Error(format!(
                        "8-{:04}",
                        choice.error_code().ok_or("opening failure has no code")?
                    )),
                };
                Ok(Record::With {
                    site,
                    name: name.data(db).clone(),
                    outcome,
                })
            })
        })
        .collect()
}

/// Build every record of the checked module, in output order. A decision v1
/// cannot express becomes an `out_of_fragment` record at its place.
pub fn module_records(
    db: &driver::DriverDataBase,
    module: hir::hir_def::TopLevelMod<'_>,
    file: common::file::File,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
) -> Vec<Record> {
    let mut records = preamble(db, file);
    records.extend(declaration_records(db, module, diagnostics));
    let outside = |what, name: Option<String>| {
        move |decision: Decision| {
            decision.unwrap_or_else(|gap| Record::outside(what, name.clone(), gap.site, gap.reason))
        }
    };
    records.extend(
        diagnostic_records(db, file, diagnostics)
            .into_iter()
            .map(outside("diagnostic", None)),
    );
    records.extend(
        overlap_records(db, module, diagnostics)
            .into_iter()
            .map(outside("overlap", None)),
    );
    for func in module.all_funcs(db) {
        let Some(owner) = func.body(db) else { continue };
        let name = func.name(db).to_opt().map(|name| name.data(db).clone());
        let (_, body) = hir::analysis::ty::ty_check::check_func_body(db, *func);
        records.extend(
            goal_records(db, body)
                .into_iter()
                .map(outside("goal", name.clone())),
        );
        records.extend(
            condition_records(db, body)
                .into_iter()
                .map(outside("condition", name.clone())),
        );
        records.extend(
            with_records(db, owner, body)
                .into_iter()
                .map(outside("with", name)),
        );
    }
    records
}

/// The records of a file whose declarations could not be generated: the error,
/// and nothing about the program.
pub fn generation_failure_records(
    db: &driver::DriverDataBase,
    file: common::file::File,
    error: Option<&common::diagnostics::CompleteDiagnostic>,
) -> Vec<Record> {
    let mut records = preamble(db, file);
    let errors = error.map_or_else(Vec::new, |error| {
        diagnostic_records(db, file, std::slice::from_ref(error))
    });
    if errors.is_empty() {
        let reason = "generation error has no source span";
        records.push(Record::outside("diagnostic", None, None, reason));
    }
    records.extend(errors.into_iter().map(|decision| {
        decision.unwrap_or_else(|gap| Record::outside("diagnostic", None, gap.site, gap.reason))
    }));
    records
}

/// The header and the note on what v1 leaves out.
fn preamble(db: &driver::DriverDataBase, file: common::file::File) -> Vec<Record> {
    vec![
        header(db, file),
        Record::Note {
            text: "Unsupported program shapes are explicit out_of_fragment items; corpus acceptance is pending.",
        },
    ]
}

/// The provenance record: compiler build and a digest of the exact source.
fn header(db: &driver::DriverDataBase, file: common::file::File) -> Record {
    use sha2::{Digest, Sha256};
    Record::Header {
        schema: "fe-decisions/1",
        compiler_commit: option_env!("FE_GIT_COMMIT").unwrap_or("unknown"),
        source_sha256: format!("{:x}", Sha256::digest(file.text(db).as_bytes())),
    }
}

/// Write `records`, one JSON object per line. The file is written next to
/// `output` and renamed over it, so readers never see a partial export.
pub fn write_records(
    records: &[Record],
    output: &std::path::Path,
) -> std::result::Result<(), String> {
    use std::io::Write;
    let fail = |error: std::io::Error| {
        format!("cannot write decision export {}: {error}", output.display())
    };
    let directory = match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => std::path::Path::new("."),
    };
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(fail)?;
    for record in records {
        serde_json::to_writer(&mut file, &record.to_json())
            .map_err(std::io::Error::from)
            .map_err(fail)?;
        file.write_all(b"\n").map_err(fail)?;
    }
    file.persist(output).map_err(|error| fail(error.error))?;
    Ok(())
}

/// Encode the same completed errors that the check command reports, including
/// errors in dependencies, whose sites name their file.
pub fn diagnostic_records(
    db: &dyn SpannedHirAnalysisDb,
    checked: common::file::File,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
) -> Vec<Decision> {
    diagnostics
        .iter()
        .filter(|d| d.severity == common::diagnostics::Severity::Error)
        .map(|d| {
            let span = d.primary_span();
            let site = span
                .as_ref()
                .ok_or("reported error has no source span".into())
                .and_then(|span| span_site(db, span));
            locate(site, |site| {
                let file = span
                    .filter(|span| span.file != checked)
                    .map(|span| span.file.url(db).ok_or("dependency error has no file URL"))
                    .transpose()?
                    .map(|url| url.to_string());
                Ok(Record::Diagnostic {
                    code: d.error_code.to_string(),
                    site: records::DiagnosticSite { site, file },
                })
            })
        })
        .collect()
}

/// Pair declarations without repeating the compiler's overlap search.
/// A pair conflicts precisely when the checked diagnostics report that pair.
pub fn overlap_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    module: hir::hir_def::TopLevelMod<'db>,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
) -> Vec<Decision> {
    let impls = module
        .children_non_nested(db)
        .filter_map(|item| {
            let ItemKind::ImplTrait(impl_) = item else {
                return None;
            };
            let name = impl_
                .hir_trait_ref(db)
                .to_opt()?
                .path(db)
                .to_opt()?
                .ident(db)
                .to_opt()?;
            Some((impl_, name))
        })
        .collect::<Vec<_>>();
    let mut records = vec![];
    for (i, (a, trait_a)) in impls.iter().enumerate() {
        for (b, trait_b) in &impls[i + 1..] {
            if trait_a != trait_b {
                continue;
            }
            let (Some(span_a), Some(span_b)) = (a.span().resolve(db), b.span().resolve(db)) else {
                records.push(Err(DecisionGap {
                    site: None,
                    reason: "implementation has no source span",
                }));
                continue;
            };
            let conflict = diagnostics
                .iter()
                .filter(|d| d.error_code.to_string() == "5-0001")
                .any(|d| {
                    let contains = |declaration: &common::diagnostics::Span| {
                        d.sub_diagnostics
                            .iter()
                            .filter_map(|s| s.span.as_ref())
                            .any(|s| {
                                s.file == declaration.file
                                    && declaration.range.contains_range(s.range)
                            })
                    };
                    contains(&span_a) && contains(&span_b)
                });
            let a = a.declaration_index(db);
            let b = b.declaration_index(db);
            records.push(Ok(Record::Overlap {
                a: a.min(b),
                b: a.max(b),
                conflict,
            }));
        }
    }
    records
}

fn binary_operator(op: BinOp) -> Result<&'static str> {
    match op {
        BinOp::Comp(op) => Ok(op.symbol()),
        BinOp::Logical(LogicalBinOp::And) => Ok("&&"),
        BinOp::Logical(LogicalBinOp::Or) => Ok("||"),
        BinOp::Arith(ArithBinOp::Add) => Ok("+"),
        BinOp::Arith(ArithBinOp::Sub) => Ok("-"),
        BinOp::Arith(ArithBinOp::Mul) => Ok("*"),
        BinOp::Arith(ArithBinOp::Div) => Ok("/"),
        _ => Err("operator is outside format v1".into()),
    }
}
