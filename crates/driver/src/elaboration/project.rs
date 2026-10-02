//! One explicit input phase followed by one complete output phase.
//! Generated declarations are not visible while providers are evaluated.

use std::collections::{BTreeSet, HashSet};

use common::{
    InputDb,
    diagnostics::{
        CompleteDiagnostic, DiagnosticPass, GlobalErrorCode, LabelStyle, Severity, Span, SpanKind,
        SubDiagnostic,
    },
};
use hir::{
    analysis::{
        diagnostics::DiagnosticVoucher,
        semantic::{EvalOutcome, GenericSubst, SemConstScalar, eval_body_owner_const_with_args},
        ty::{
            normalize::normalize_ty,
            trait_resolution::PredicateListId,
            ty_check::{BodyOwner, check_func_body},
            ty_def::{TyBase, TyData, TyId},
        },
    },
    core::meta::{FrozenGeneration, FrozenItemLocator, FrozenMetaValue, FrozenOwner, MetaItem},
    hir_def::{CallableDef, Expr, HirIngot, ItemKind, Partial, WhereClauseOwner},
    span::{DesugaredOrigin, HirOrigin, LazySpan},
};
use parser::TextRange;
use url::Url;

use super::{RecordInput, construction::ConstructionError, invocations};
use crate::DriverDataBase;

#[derive(Debug)]
pub struct ElaborationError {
    pub url: Box<Url>,
    pub message: String,
    /// Byte range in the authored file, usually the `#[generate]` attribute.
    pub span: Option<std::ops::Range<usize>>,
    pub quote: Option<QuoteLocation>,
    pub code: u16,
    pub rendered: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QuoteLocation {
    pub url: Url,
    pub range: std::ops::Range<usize>,
}

fn source_span(db: &DriverDataBase, url: &Url, range: &std::ops::Range<usize>) -> Option<Span> {
    let file = db.workspace().get(db, url)?;
    if range.end > file.text(db).len() || range.start > range.end {
        return None;
    }
    Some(Span::new(
        file,
        TextRange::new(
            u32::try_from(range.start).ok()?.into(),
            u32::try_from(range.end).ok()?.into(),
        ),
        SpanKind::Original,
    ))
}

fn evaluation_operation_diagnostic<'db>(
    db: &'db DriverDataBase,
    mut owner: BodyOwner<'db>,
    failure: &hir::analysis::semantic::ctfe::EvalFailure<'db>,
) -> Option<(String, QuoteLocation)> {
    use hir::analysis::semantic::{
        SemOrigin,
        ctfe::{CtfeError, EvalFailure},
    };
    let EvalFailure::Ctfe(error) = failure else {
        return None;
    };
    let mut error = error;
    while let CtfeError::CalleeError { callee, source, .. } = error {
        owner = callee.key(db).owner(db);
        error = source;
    }
    let CtfeError::InvalidOperation { origin, message } = error else {
        return None;
    };
    let body = owner.body(db)?;
    let span = match origin {
        SemOrigin::Expr(expr) => expr.span(body).resolve(db)?,
        SemOrigin::Stmt(stmt) => stmt.span(body).resolve(db)?,
        SemOrigin::Body(_) | SemOrigin::Synthetic => return None,
    };
    Some((
        message.clone(),
        QuoteLocation {
            url: span.file.url(db)?,
            range: span.range.into(),
        },
    ))
}

impl ElaborationError {
    pub fn position(&self, db: &DriverDataBase) -> Option<(usize, usize)> {
        let source = db.workspace().get(db, &self.url)?.text(db);
        let prefix = source.get(..self.span.as_ref()?.start)?;
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next()?.chars().count() + 1;
        Some((line, column))
    }

    pub fn render(&self, db: &DriverDataBase) -> String {
        if let Some(rendered) = &self.rendered {
            return rendered.clone();
        }
        let quote = self
            .quote
            .as_ref()
            .and_then(|quote| source_span(db, &quote.url, &quote.range));
        let invocation = self
            .span
            .as_ref()
            .and_then(|range| source_span(db, &self.url, range));
        if let Some(primary) = quote.clone().or_else(|| invocation.clone()) {
            let mut labels = vec![SubDiagnostic::new(
                LabelStyle::Primary,
                self.message.clone(),
                Some(primary),
            )];
            if quote.is_some()
                && let Some(invocation) = invocation
            {
                labels.push(SubDiagnostic::new(
                    LabelStyle::Secondary,
                    "generated here".into(),
                    Some(invocation),
                ));
            }
            return db.format_complete_diagnostics(&[CompleteDiagnostic::new(
                Severity::Error,
                self.message.clone(),
                labels,
                Vec::new(),
                GlobalErrorCode::new(DiagnosticPass::Generation, self.code),
            )]);
        }
        match self.position(db) {
            Some((line, column)) => {
                format!("{}:{line}:{column}: {}", self.url, self.message)
            }
            None => format!("{}: {}", self.url, self.message),
        }
    }
}

fn unique_quote_location(value: &FrozenMetaValue) -> Option<QuoteLocation> {
    fn collect(value: &FrozenMetaValue, locations: &mut Vec<QuoteLocation>) {
        match value {
            FrozenMetaValue::Code(code) => {
                for node in &code.nodes {
                    if let Some(template) = &node.template
                        && let Some(range) = &template.span
                    {
                        locations.push(QuoteLocation {
                            url: template.owner.file().clone(),
                            range: range.clone(),
                        });
                    }
                    for hole in &node.holes {
                        if let hir::core::meta::FrozenCodeHole::Value(value, _) = hole {
                            collect(value, locations);
                        }
                    }
                }
            }
            FrozenMetaValue::Tuple(values) | FrozenMetaValue::Array(values) => {
                for value in values {
                    collect(value, locations);
                }
            }
            FrozenMetaValue::Record { fields, .. } => {
                for value in fields.values() {
                    collect(value, locations);
                }
            }
            FrozenMetaValue::Variant { fields, .. } => {
                for value in fields {
                    collect(value, locations);
                }
            }
            _ => {}
        }
    }
    let mut locations = Vec::new();
    collect(value, &mut locations);
    let first = locations.first()?.clone();
    locations
        .iter()
        .all(|location| location.url == first.url && location.range == first.range)
        .then_some(first)
}

fn provider_quote_location(
    db: &DriverDataBase,
    provider: hir::hir_def::Func<'_>,
) -> Option<QuoteLocation> {
    let body = provider.body(db)?;
    let mut locations = body.exprs(db).iter().filter_map(|(expr, data)| {
        matches!(data, Partial::Present(Expr::Quote(_)))
            .then(|| expr.span(body).resolve(db))
            .flatten()
            .and_then(|span| {
                Some(QuoteLocation {
                    url: span.file.url(db)?,
                    range: span.range.into(),
                })
            })
    });
    let location = locations.next()?;
    locations.next().is_none().then_some(location)
}

fn generated_item_at(
    db: &DriverDataBase,
    url: &Url,
    site: &std::ops::Range<usize>,
) -> Option<String> {
    let file = db.workspace().get(db, url)?;
    db.top_mod(file)
        .scope_graph(db)
        .items_dfs(db)
        .find_map(|item| {
            let generated = match item {
                ItemKind::Func(func) => matches!(
                    func.origin(db),
                    HirOrigin::Desugared(DesugaredOrigin::Generation(_))
                ),
                ItemKind::Impl(_) | ItemKind::ImplTrait(_) => true,
                _ => false,
            };
            if !generated
                || item
                    .span()
                    .resolve(db)
                    .is_none_or(|span| std::ops::Range::<usize>::from(span.range) != *site)
            {
                return None;
            }
            Some(item.pretty_print(db))
        })
}

fn annotate_generated_diagnostics(
    db: &DriverDataBase,
    requests: &[FrozenGeneration],
    diagnostics: &mut [CompleteDiagnostic],
) -> bool {
    let mut annotated = false;
    for diagnostic in diagnostics {
        let Some(primary) = diagnostic.primary_span() else {
            continue;
        };
        let Some(url) = primary.file.url(db) else {
            continue;
        };
        let range: std::ops::Range<usize> = primary.range.into();
        let Some(request) = requests.iter().find(|request| {
            request.target.file == url && request.site_span.as_ref() == Some(&range)
        }) else {
            continue;
        };
        let Some(quote) = unique_quote_location(&request.value) else {
            continue;
        };
        let Some(quote_span) = source_span(db, &quote.url, &quote.range) else {
            continue;
        };
        annotated = true;
        for label in &mut diagnostic.sub_diagnostics {
            if label.style == LabelStyle::Primary && label.span.as_ref() == Some(&primary) {
                label.span = Some(quote_span.clone());
            }
        }
        diagnostic.sub_diagnostics.push(SubDiagnostic::new(
            LabelStyle::Secondary,
            "generated here".into(),
            Some(primary),
        ));
        if let Some(item) = generated_item_at(db, &url, &range) {
            diagnostic.notes.push(format!("generated item:\n{item}"));
        }
    }
    annotated
}

/// Only checked quoted items may cross the generation output boundary.
fn check_output_shape<'db>(input: &RecordInput<'db>, ty: TyId<'db>) -> Result<(), String> {
    let db = input.db;
    let missing = || "generation result has an unsupported shape".to_owned();
    let (base, args) = ty.decompose_ty_app(db);
    if input
        .meta_type(MetaItem::Code)
        .is_some_and(|code| base == code.base_ty(db))
        && args.len() == 1
        && input
            .meta_type(MetaItem::Item)
            .is_some_and(|item| args[0].base_ty(db) == item.base_ty(db))
    {
        return Ok(());
    }
    Err(missing())
}

/// One structural budget for the frozen provider result. Schema names and
/// pretty-printer formatting do not affect whether a declaration is admitted.
/// Each value node costs eight units, plus its scalar payload bytes.
fn description_units(value: &FrozenMetaValue) -> usize {
    let payload = match value {
        FrozenMetaValue::Unit | FrozenMetaValue::Structural { .. } => 0,
        FrozenMetaValue::Code(code) => code.nodes.iter().fold(0usize, |sum, node| {
            let holes = node.holes.iter().fold(0usize, |sum, hole| {
                sum.saturating_add(match hole {
                    hir::core::meta::FrozenCodeHole::Code(_) => 8,
                    hir::core::meta::FrozenCodeHole::Value(value, _) => description_units(value),
                })
            });
            sum.saturating_add(16).saturating_add(holes)
        }),
        FrozenMetaValue::Scalar { value, .. } => match value {
            SemConstScalar::Bool(_) => 1,
            SemConstScalar::Int { value } => value.to_signed_bytes_le().len(),
            SemConstScalar::Bytes(bytes) => bytes.len(),
        },
        FrozenMetaValue::Tuple(values) | FrozenMetaValue::Array(values) => {
            values.iter().fold(0usize, |sum, value| {
                sum.saturating_add(description_units(value))
            })
        }
        FrozenMetaValue::Record { fields, .. } => fields.values().fold(0usize, |sum, value| {
            sum.saturating_add(description_units(value))
        }),
        FrozenMetaValue::Variant { fields, .. } => fields.iter().fold(0usize, |sum, value| {
            sum.saturating_add(description_units(value))
        }),
    };
    8usize.saturating_add(payload)
}

/// Prepare all explicit invocations in the resolved input set. The authored
/// database remains unchanged, including on failure. None avoids copying when
/// there are no invocations. Resolve semantic identities again in the result.
pub fn prepare(db: &DriverDataBase) -> Result<Option<DriverDataBase>, ElaborationError> {
    prepare_selected(db, None)
}

/// Prepare a selected file or ingot and its dependency closure. Other loaded
/// workspace members stay authored and their providers are not evaluated.
pub fn prepare_target(
    db: &DriverDataBase,
    target: &Url,
) -> Result<Option<DriverDataBase>, ElaborationError> {
    let root = if let Some(file) = db.workspace().get(db, target) {
        Some(db.top_mod(file).ingot(db))
    } else {
        db.workspace().containing_ingot(db, target.clone())
    }
    .ok_or_else(|| ElaborationError {
        url: Box::new(target.clone()),
        message: "could not resolve generation target".into(),
        span: None,
        quote: None,
        code: 1,
        rendered: None,
    })?;
    let mut selected = HashSet::new();
    let mut pending = vec![root];
    while let Some(ingot) = pending.pop() {
        if selected.insert(ingot) {
            pending.extend(
                ingot
                    .resolved_external_ingots(db)
                    .iter()
                    .map(|(_, dependency)| *dependency),
            );
        }
    }
    prepare_selected(db, Some(&selected))
}

fn prepare_selected<'db>(
    db: &'db DriverDataBase,
    selected: Option<&HashSet<common::ingot::Ingot<'db>>>,
) -> Result<Option<DriverDataBase>, ElaborationError> {
    let mut check_urls = BTreeSet::new();
    let mut total_units = 0usize;
    let mut frozen_requests = Vec::new();
    for (url, file) in db.workspace().all_files(db).iter() {
        // This is only a negative lexical filter; all matching source goes
        // through the parser and ordinary lexical invocation resolution.
        if !url.path().ends_with(".fe") || !file.text(db).contains("generate") {
            continue;
        }
        if selected.is_some_and(|selected| !selected.contains(&db.top_mod(file).ingot(db))) {
            continue;
        }
        let error_at = |message: String, span| ElaborationError {
            url: Box::new(url.clone()),
            message,
            span,
            quote: None,
            code: 1,
            rendered: None,
        };
        let requests = invocations(db, file).map_err(|e| {
            let span = match e.item {
                ItemKind::Struct(owner) => owner
                    .span()
                    .attributes()
                    .attr(e.attribute_index)
                    .resolve(db),
                item => item.span().resolve(db),
            }
            .map(|span| span.range.into());
            error_at(format!("invalid generation invocation: {:?}", e.kind), span)
        })?;
        for request in requests {
            let site = match request.owner {
                hir::analysis::ty::adt_def::AdtRef::Struct(owner) => owner
                    .span()
                    .attributes()
                    .attr(request.attribute_index)
                    .resolve(db),
                hir::analysis::ty::adt_def::AdtRef::Enum(owner) => owner
                    .span()
                    .attributes()
                    .attr(request.attribute_index)
                    .resolve(db),
            }
            .map(|span| span.range.into());
            let error = |message: String| error_at(message, site.clone());
            let input = request
                .record_input(db)
                .map_err(|e| error(format!("invalid generation input: {e:?}")))?;
            let TyData::TyBase(TyBase::Func(CallableDef::Func(provider))) =
                request.provider.data(db)
            else {
                return Err(error("generation requires a const function".into()));
            };
            let parameters: Vec<_> = provider.params(db).collect();
            let record_ty = MetaItem::Record
                .resolve(db, provider.scope())
                .ok_or_else(|| error("builtin Record is missing".into()))?;
            let normalize =
                |ty| normalize_ty(db, ty, provider.scope(), PredicateListId::empty_list(db));
            let argument_ty = parameters.first().map(|parameter| {
                let ty = normalize(parameter.ty(db));
                ty.as_view(db).unwrap_or(ty)
            });
            let clause = WhereClauseOwner::Func(*provider).clause(db).id;
            if !provider.is_const(db)
                || provider.is_unsafe(db)
                || provider.is_method(db)
                || provider.has_effects(db)
                || !clause.predicates(db).is_empty()
                || !clause.const_predicates(db).is_empty()
                || !provider
                    .as_callable(db)
                    .is_some_and(|c| c.params(db).is_empty())
                || parameters.len() != 1
                || argument_ty != Some(record_ty)
            {
                return Err(error("generation entry must be a safe nongeneric module const function taking Record, without entry requirements or effects; use ordinary checked helper calls and scoped witnesses inside it".into()));
            }
            check_output_shape(&input, normalize(provider.return_ty(db))).map_err(error)?;
            let diagnostics = &check_func_body(db, *provider).0;
            if !diagnostics.is_empty() {
                let complete = diagnostics
                    .iter()
                    .map(|d| d.to_complete(db))
                    .collect::<Vec<_>>();
                let rendered = db.format_complete_diagnostics(&complete);
                let mut diagnostic = error(rendered.clone());
                diagnostic.rendered = Some(format!("{url}: {rendered}"));
                return Err(diagnostic);
            }
            let argument = input
                .const_argument()
                .ok_or_else(|| error("could not issue record input".into()))?;
            let value = match eval_body_owner_const_with_args(
                db,
                BodyOwner::Func(*provider),
                GenericSubst::for_body_owner(db, BodyOwner::Func(*provider), vec![]),
                vec![argument],
            ) {
                EvalOutcome::Ready(value) => value,
                EvalOutcome::Blocked(info) => {
                    return Err(error(format!(
                        "generation evaluation remained dependent: {info:?}"
                    )));
                }
                EvalOutcome::Failed(failure) => {
                    if let Some((message, location)) =
                        evaluation_operation_diagnostic(db, BodyOwner::Func(*provider), &failure)
                    {
                        let mut diagnostic = error(message);
                        diagnostic.quote = Some(location);
                        return Err(diagnostic);
                    }
                    return Err(error(format!("generation evaluation failed: {failure:?}")));
                }
            };
            // Freeze the checked value before the output HIR query reads it.
            let frozen = input.freeze_value(value).map_err(|e| {
                let mut diagnostic = error(format!("invalid generated declaration: {e}"));
                diagnostic.code = if matches!(e, ConstructionError::Protocol(_)) {
                    2
                } else {
                    3
                };
                diagnostic.quote = provider_quote_location(db, *provider);
                diagnostic
            })?;
            let units = description_units(&frozen);
            if units > 65536 {
                return Err(error(format!(
                    "invalid generated declaration: generated description units limit exceeded: {units} > 65536"
                )));
            }
            total_units = total_units.saturating_add(units);
            if total_units > 1048576 {
                return Err(error(format!(
                    "project generated description units limit exceeded: {total_units} > 1048576"
                )));
            }
            let provider_identity = FrozenItemLocator::Named(FrozenOwner {
                file: provider
                    .span()
                    .resolve(db)
                    .and_then(|span| span.file.url(db))
                    .ok_or_else(|| error("generation provider file is missing".into()))?,
                path: provider
                    .scope()
                    .pretty_path(db)
                    .ok_or_else(|| error("generation provider path is missing".into()))?,
            });
            let span = request
                .owner
                .as_item()
                .span()
                .resolve(db)
                .ok_or_else(|| error("generation owner has no source span".into()))?;
            let range: std::ops::Range<usize> = span.range.into();
            frozen_requests.push(FrozenGeneration {
                target: input
                    .frozen_owner()
                    .map_err(|e| error(format!("invalid generation owner: {e}")))?,
                target_const_param_types: input
                    .frozen_const_param_types()
                    .map_err(|e| error(format!("invalid generation owner: {e}")))?,
                target_span_start: range.start,
                site_span: site,
                provider: provider_identity,
                attribute_index: request.attribute_index,
                value: frozen,
            });
            check_urls.insert(url.clone());
            if let Some(provider_url) = provider.span().resolve(db).and_then(|s| s.file.url(db)) {
                check_urls.insert(provider_url);
            }
        }
    }
    if frozen_requests.is_empty() {
        return Ok(None);
    }
    let mut output = db.fork_inputs();
    output.publish_generation(frozen_requests.clone());
    let mut checked = HashSet::new();
    for url in check_urls {
        let file = output
            .workspace()
            .get(&output, &url)
            .expect("copied source file");
        if let Some(error) = hir::lower::generated_hir_errors(&output, output.top_mod(file)).first()
        {
            let quote = frozen_requests
                .iter()
                .find(|request| request.target.file == url && request.site_span == error.site_span)
                .and_then(|request| unique_quote_location(&request.value));
            return Err(ElaborationError {
                url: Box::new(url),
                message: format!("invalid generated declaration: {}", error.message),
                span: error.site_span.clone(),
                quote,
                code: 4,
                rendered: None,
            });
        }
        if checked.insert(output.top_mod(file).ingot(&output))
            && let Some(mut diagnostics) =
                crate::checked_source::complete_diagnostics(&output, file)
        {
            let annotated =
                annotate_generated_diagnostics(&output, &frozen_requests, &mut diagnostics);
            let rendered = output.format_complete_diagnostics(&diagnostics);
            let rendered = if annotated {
                rendered
            } else {
                format!("{url}: {rendered}")
            };
            return Err(ElaborationError {
                url: Box::new(url),
                message: rendered.clone(),
                span: None,
                quote: None,
                code: 5,
                rendered: Some(rendered),
            });
        }
        if let Some(error) =
            hir::lower::generated_quote_hygiene_errors(&output, output.top_mod(file)).first()
        {
            let quote = frozen_requests
                .iter()
                .find(|request| request.target.file == url && request.site_span == error.site_span)
                .and_then(|request| unique_quote_location(&request.value));
            return Err(ElaborationError {
                url: Box::new(url),
                message: format!("invalid generated declaration: {}", error.message),
                span: error.site_span.clone(),
                quote,
                code: 6,
                rendered: None,
            });
        }
    }
    Ok(Some(output))
}
