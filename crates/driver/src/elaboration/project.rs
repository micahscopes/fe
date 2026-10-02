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
    core::meta::{
        FrozenGeneration, FrozenItemLocator, FrozenMetaValue, FrozenOwner, FrozenOwnerError,
        FrozenQuoteSpan, MetaItem,
    },
    hir_def::{CallableDef, Expr, HirIngot, ItemKind, Partial, WhereClauseOwner},
    span::{DesugaredOrigin, HirOrigin, LazySpan},
};
use parser::TextRange;
use url::Url;

use super::{ElaborationErrorKind, EvaluationFault, RecordInput, invocations};
use crate::DriverDataBase;

#[derive(Debug)]
pub struct ElaborationError {
    pub url: Box<Url>,
    pub kind: ElaborationErrorKind,
    /// Byte range in the authored file, usually the `#[generate]` attribute.
    pub span: Option<std::ops::Range<usize>>,
    pub quote: Option<Box<FrozenQuoteSpan>>,
}

fn invocation_span(
    db: &DriverDataBase,
    item: ItemKind<'_>,
    attribute_index: usize,
) -> Option<std::ops::Range<usize>> {
    match item {
        ItemKind::Struct(owner) => owner.span().attributes().attr(attribute_index).resolve(db),
        ItemKind::Enum(owner) => owner.span().attributes().attr(attribute_index).resolve(db),
        item => item.span().resolve(db),
    }
    .map(|span| span.range.into())
}

fn evaluation_error(
    db: &DriverDataBase,
    failure: &hir::analysis::semantic::ctfe::EvalFailure<'_>,
) -> ElaborationErrorKind {
    use hir::analysis::semantic::ctfe::{CtfeError, EvalFailure};
    let error = match failure {
        EvalFailure::Invariant { message, .. } => {
            return ElaborationErrorKind::EvaluationFailed {
                fault: EvaluationFault::Invariant(message.clone()),
                calls: Vec::new(),
            };
        }
        EvalFailure::Ctfe(error) => error,
    };
    let mut root = error;
    let mut calls = Vec::new();
    while let CtfeError::CalleeError { callee, source, .. } = root {
        if let Some(name) = callee.key(db).owner(db).scope().pretty_path(db) {
            calls.push(name);
        }
        root = source;
    }
    let fault = root.kind();
    ElaborationErrorKind::EvaluationFailed { fault, calls }
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
) -> Option<(String, FrozenQuoteSpan)> {
    use hir::analysis::semantic::{
        SemOrigin,
        ctfe::{CtfeError, EvalFailure},
    };
    let EvalFailure::Ctfe(error) = failure else {
        return None;
    };
    let origin_span = |owner: BodyOwner<'db>, origin: SemOrigin<'db>| {
        let body = owner.body(db)?;
        match origin {
            SemOrigin::Expr(expr) => expr.span(body).resolve(db),
            SemOrigin::Stmt(stmt) => stmt.span(body).resolve(db),
            SemOrigin::Body(_) | SemOrigin::Synthetic => None,
        }
    };
    let in_core = |owner: BodyOwner<'db>| {
        owner.scope().top_mod(db).ingot(db).kind(db) == common::ingot::IngotKind::Core
    };
    let mut error = error;
    let mut caller_span = None;
    while let CtfeError::CalleeError {
        origin,
        callee,
        source,
    } = error
    {
        if !in_core(owner) {
            caller_span = origin_span(owner, *origin);
        }
        owner = callee.key(db).owner(db);
        error = source;
    }
    let CtfeError::InvalidOperation { origin, message } = error else {
        return None;
    };
    // Core wrappers implement structural operations; authors act on the call
    // in their own provider. Ordinary helper failures retain their leaf span.
    let span = if in_core(owner) {
        caller_span.or_else(|| origin_span(owner, *origin))?
    } else {
        origin_span(owner, *origin)?
    };
    Some((
        message.clone(),
        FrozenQuoteSpan {
            file: span.file.url(db)?,
            range: span.range.into(),
        },
    ))
}

impl ElaborationError {
    pub fn message(&self) -> String {
        self.kind.to_string()
    }
    pub fn code(&self) -> u16 {
        self.kind.code()
    }
    pub fn rendered(&self) -> Option<String> {
        match &self.kind {
            ElaborationErrorKind::ProviderDiagnostics(message) => {
                Some(format!("{}: {message}", self.url))
            }
            ElaborationErrorKind::OutputDiagnostics(rendered) => Some(rendered.clone()),
            _ => None,
        }
    }

    pub fn position(&self, db: &DriverDataBase) -> Option<(usize, usize)> {
        let source = db.workspace().get(db, &self.url)?.text(db);
        let prefix = source.get(..self.span.as_ref()?.start)?;
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next()?.chars().count() + 1;
        Some((line, column))
    }

    pub fn render(&self, db: &DriverDataBase) -> String {
        if let Some(rendered) = self.rendered() {
            return rendered;
        }
        let message = self.message();
        let quote = self
            .quote
            .as_ref()
            .and_then(|quote| source_span(db, &quote.file, &quote.range));
        let invocation = self
            .span
            .as_ref()
            .and_then(|range| source_span(db, &self.url, range));
        if let Some(primary) = quote.clone().or_else(|| invocation.clone()) {
            let mut labels = vec![SubDiagnostic::new(
                LabelStyle::Primary,
                message.clone(),
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
                message,
                labels,
                Vec::new(),
                GlobalErrorCode::new(DiagnosticPass::Generation, self.code()),
            )]);
        }
        match self.position(db) {
            Some((line, column)) => {
                format!("{}:{line}:{column}: {}", self.url, message)
            }
            None => format!("{}: {}", self.url, message),
        }
    }
}

fn request_at<'a>(
    requests: &'a [FrozenGeneration],
    url: &Url,
    range: Option<&std::ops::Range<usize>>,
) -> Option<&'a FrozenGeneration> {
    requests
        .iter()
        .find(|request| request.target.file == *url && request.site_span.as_ref() == range)
}

fn unique_quote_location(value: &FrozenMetaValue) -> Option<FrozenQuoteSpan> {
    fn collect(value: &FrozenMetaValue, locations: &mut Vec<FrozenQuoteSpan>) {
        match value {
            FrozenMetaValue::Code(code) => {
                for node in &code.nodes {
                    if let Some(template) = &node.template
                        && let Some(range) = &template.span
                    {
                        locations.push(FrozenQuoteSpan {
                            file: template.owner.file().clone(),
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
            FrozenMetaValue::DeclarationName { fields, .. } => {
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
        .all(|location| location.file == first.file && location.range == first.range)
        .then_some(first)
}

fn provider_quote_location(
    db: &DriverDataBase,
    provider: hir::hir_def::Func<'_>,
) -> Option<FrozenQuoteSpan> {
    let body = provider.body(db)?;
    let mut locations = body.exprs(db).iter().filter_map(|(expr, data)| {
        matches!(data, Partial::Present(Expr::Quote(_)))
            .then(|| expr.span(body).resolve(db))
            .flatten()
            .and_then(|span| {
                Some(FrozenQuoteSpan {
                    file: span.file.url(db)?,
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
        if let SpanKind::Generation { file, range } = primary.kind {
            annotated = true;
            diagnostic.sub_diagnostics.push(SubDiagnostic::new(
                LabelStyle::Secondary,
                "generated here".into(),
                Some(Span::new(file, range, SpanKind::Original)),
            ));
            // Retain the existing generated-item note for a single quote.
            let site_range: std::ops::Range<usize> = range.into();
            if let Some(url) = file.url(db)
                && requests.iter().any(|request| {
                    request.target.file == url
                        && request.site_span.as_ref() == Some(&site_range)
                        && unique_quote_location(&request.value).is_some()
                })
                && let Some(item) = generated_item_at(db, &url, &site_range)
            {
                diagnostic.notes.push(format!("generated item:\n{item}"));
            }
            continue;
        }
        let Some(url) = primary.file.url(db) else {
            continue;
        };
        let range: std::ops::Range<usize> = primary.range.into();
        let Some(request) = request_at(requests, &url, Some(&range)) else {
            continue;
        };
        let unique = unique_quote_location(&request.value);
        // Composed fragments have several quote sites. The emitted item's root
        // still identifies its author without guessing which child caused the error.
        let root = match &request.value {
            FrozenMetaValue::Code(code) => code
                .nodes
                .get(code.root)
                .and_then(|node| node.template.as_ref())
                .and_then(|template| {
                    template.span.as_ref().map(|range| FrozenQuoteSpan {
                        file: template.owner.file().clone(),
                        range: range.clone(),
                    })
                }),
            _ => None,
        };
        let Some(quote) = unique.as_ref().or(root.as_ref()) else {
            continue;
        };
        let Some(quote_span) = source_span(db, &quote.file, &quote.range) else {
            continue;
        };
        annotated = true;
        if unique.is_none() {
            diagnostic.sub_diagnostics.push(SubDiagnostic::new(
                LabelStyle::Secondary,
                "generated from this quote".into(),
                Some(quote_span),
            ));
            continue;
        }
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
fn check_output_shape<'db>(
    input: &RecordInput<'db>,
    ty: TyId<'db>,
) -> Result<(), ElaborationErrorKind> {
    let db = input.db;
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
    Err(ElaborationErrorKind::UnsupportedOutputShape)
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
        kind: ElaborationErrorKind::MissingTarget,
        span: None,
        quote: None,
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
        let error_at = |kind: ElaborationErrorKind, span| ElaborationError {
            url: Box::new(url.clone()),
            kind,
            span,
            quote: None,
        };
        let requests = invocations(db, file).map_err(|e| {
            let span = invocation_span(db, e.item, e.attribute_index);
            error_at(ElaborationErrorKind::Invocation(e.kind), span)
        })?;
        for request in requests {
            let site = invocation_span(db, request.owner.as_item(), request.attribute_index);
            let error = |kind: ElaborationErrorKind| error_at(kind, site.clone());
            let input = request
                .record_input(db)
                .map_err(|e| error(ElaborationErrorKind::Input(e)))?;
            let TyData::TyBase(TyBase::Func(CallableDef::Func(provider))) =
                request.provider.data(db)
            else {
                return Err(error(ElaborationErrorKind::RequiresConstFunction));
            };
            let parameters: Vec<_> = provider.params(db).collect();
            let record_ty = MetaItem::Record
                .resolve(db, provider.scope())
                .ok_or_else(|| error(ElaborationErrorKind::MissingRecord))?;
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
                return Err(error(ElaborationErrorKind::InvalidEntry));
            }
            check_output_shape(&input, normalize(provider.return_ty(db))).map_err(error)?;
            let diagnostics = &check_func_body(db, *provider).0;
            if !diagnostics.is_empty() {
                let complete = diagnostics
                    .iter()
                    .map(|d| d.to_complete(db))
                    .collect::<Vec<_>>();
                let rendered = db.format_complete_diagnostics(&complete);
                return Err(error(ElaborationErrorKind::ProviderDiagnostics(rendered)));
            }
            let argument = input
                .const_argument()
                .ok_or_else(|| error(ElaborationErrorKind::IssueInput))?;
            let value = match eval_body_owner_const_with_args(
                db,
                BodyOwner::Func(*provider),
                GenericSubst::for_body_owner(db, BodyOwner::Func(*provider), vec![]),
                vec![argument],
            ) {
                EvalOutcome::Ready(value) => value,
                EvalOutcome::Blocked(info) => {
                    return Err(error(ElaborationErrorKind::EvaluationBlocked(info.demand)));
                }
                EvalOutcome::Failed(failure) => {
                    if let Some((message, location)) =
                        evaluation_operation_diagnostic(db, BodyOwner::Func(*provider), &failure)
                    {
                        let mut diagnostic = error(ElaborationErrorKind::Operation(message));
                        diagnostic.quote = Some(Box::new(location));
                        return Err(diagnostic);
                    }
                    return Err(error(evaluation_error(db, &failure)));
                }
            };
            // Freeze the checked value before the output HIR query reads it.
            let (frozen, units) = input.freeze_value_counted(value).map_err(|e| {
                let mut diagnostic = error(ElaborationErrorKind::Freezing(e));
                diagnostic.quote = provider_quote_location(db, *provider).map(Box::new);
                diagnostic
            })?;
            total_units = total_units.saturating_add(units);
            if total_units > 1048576 {
                return Err(error(ElaborationErrorKind::ProjectDescriptionLimit {
                    actual: total_units,
                    maximum: 1048576,
                }));
            }
            let provider_identity = FrozenItemLocator::Named(
                FrozenOwner::of_item(db, (*provider).into()).map_err(|kind| {
                    error(match kind {
                        FrozenOwnerError::MissingFile => ElaborationErrorKind::ProviderFileMissing,
                        FrozenOwnerError::MissingPath => ElaborationErrorKind::ProviderPathMissing,
                    })
                })?,
            );
            request
                .owner
                .as_item()
                .span()
                .resolve(db)
                .ok_or_else(|| error(ElaborationErrorKind::OwnerSpanMissing))?;
            frozen_requests.push(FrozenGeneration {
                target: input
                    .frozen_owner()
                    .map_err(|e| error(ElaborationErrorKind::Owner(e)))?,
                target_const_param_types: input
                    .frozen_const_param_types()
                    .map_err(|e| error(ElaborationErrorKind::Owner(e)))?,
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
            let quote = error.quote_source.clone().or_else(|| {
                request_at(&frozen_requests, &url, error.site_span.as_ref())
                    .and_then(|request| unique_quote_location(&request.value))
            });
            return Err(ElaborationError {
                url: Box::new(url),
                kind: ElaborationErrorKind::Lowering(error.failure.clone()),
                span: error.site_span.clone(),
                quote: quote.map(Box::new),
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
                kind: ElaborationErrorKind::OutputDiagnostics(rendered),
                span: None,
                quote: None,
            });
        }
        if let Some(error) =
            hir::lower::generated_quote_hygiene_errors(&output, output.top_mod(file)).first()
        {
            let quote = request_at(&frozen_requests, &url, error.site_span.as_ref())
                .and_then(|request| unique_quote_location(&request.value));
            return Err(ElaborationError {
                url: Box::new(url),
                kind: ElaborationErrorKind::Hygiene(error.failure.clone()),
                span: error.site_span.clone(),
                quote: quote.map(Box::new),
            });
        }
    }
    Ok(Some(output))
}
