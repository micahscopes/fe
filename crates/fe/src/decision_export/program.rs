//! Authored program declarations for the v1 model fragment, without resolution.
use super::{ExportGap, Result, binary_operator, goal, records::*, source_site, span_site};
use hir::{
    analysis::{diagnostics::SpannedHirAnalysisDb, ty::ty_check::TypedBody},
    hir_def::{
        Body, ConstGenericArgValue, Expr, ExprId, Func, GenericArg, GenericParam,
        GenericParamOwner, ItemKind, LitKind, Partial, PathId, PathKind, Stmt, TopLevelMod,
        TraitRefId, TypeBound, TypeId, TypeKind, UnOp,
    },
    span::{DynLazySpan, LazySpan},
};
use std::collections::BTreeMap;

type Parameters = BTreeMap<String, Kind>;
type Diagnostics = [common::diagnostics::CompleteDiagnostic];
const MAX_TERM_DEPTH: usize = 64;

/// Read declaration shapes. Function use records are encoded separately.
pub fn declaration_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    module: TopLevelMod<'db>,
    diagnostics: &Diagnostics,
) -> Vec<Record> {
    module
        .children_non_nested(db)
        .filter(|item| !matches!(item, ItemKind::Use(_)))
        .map(|item| {
            let record = declaration(db, item, diagnostics).and_then(|item_record| {
                let span_aliases = declaration_aliases(db, item, diagnostics)?;
                Ok(ItemRecord {
                    item: item_record,
                    span_aliases: Some(span_aliases),
                })
            });
            record.map_or_else(
                |ExportGap(reason)| {
                    let name = item.name(db).map(|name| name.data(db).clone());
                    Record::outside(item.kind_name(), name, None, reason)
                },
                Record::Item,
            )
        })
        .collect()
}

/// Reported coordinates inside a declaration, other than those of its method bodies.
fn declaration_aliases<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    item: ItemKind<'db>,
    diagnostics: &Diagnostics,
) -> Result<Vec<Site>> {
    let region: DynLazySpan = match item {
        ItemKind::Func(function) => function.span().sig().into(),
        _ => item.span().into(),
    };
    let excluded = match item {
        ItemKind::Trait(trait_) => trait_.methods(db).map(|m| m.span().into()).collect(),
        ItemKind::ImplTrait(impl_) => impl_
            .methods(db)
            .filter_map(|m| m.body(db).map(|b| b.span().into()))
            .collect(),
        _ => vec![],
    };
    span_aliases(db, region, &excluded, diagnostics, None)
}

fn declaration<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    item: ItemKind<'db>,
    diagnostics: &Diagnostics,
) -> Result<Item> {
    if let ItemKind::Const(value) = item {
        let name = value.name(db).to_opt().ok_or("unnamed constant")?;
        return Ok(Item::Value {
            name: name.data(db).clone(),
        });
    }
    let owner = GenericParamOwner::from_item_opt(item).ok_or("declaration is outside format v1")?;
    let (params, env) = parameters(db, owner)?;
    let (bounds, conds) = clause(db, owner, &env)?;
    match item {
        ItemKind::Func(function) => {
            if function.params(db).next().is_some() {
                return Err("function value parameters are outside this v1 projection".into());
            }
            let body = function.body(db).ok_or("function has no body")?;
            let (_, typed) = hir::analysis::ty::ty_check::check_func_body(db, function);
            let uses = body_uses(db, body, body.expr(db), &env, typed, diagnostics, 0)?;
            let name = function.name(db).to_opt().ok_or("unnamed function")?;
            Ok(Item::Fn {
                name: name.data(db).clone(),
                params,
                is_const: function.is_const(db),
                bounds,
                conds,
                body: uses,
            })
        }
        ItemKind::Struct(struct_) => {
            if !struct_.hir_fields(db).data(db).is_empty() {
                return Err("struct fields are outside the v1 model fragment".into());
            }
            let name = struct_.name(db).to_opt().ok_or("unnamed struct")?;
            Ok(Item::Struct {
                name: name.data(db).clone(),
                params,
                bounds,
                conds,
            })
        }
        ItemKind::Trait(trait_) => {
            if trait_.super_trait_refs(db).next().is_some() {
                return Err("super-traits are outside the v1 model fragment".into());
            }
            if trait_.assoc_types(db).next().is_some() {
                return Err("associated types are outside format v1".into());
            }
            let assoc_consts = trait_
                .assoc_consts(db)
                .map(|constant| {
                    if constant.has_default(db) {
                        return Err("associated constant defaults are outside format v1".into());
                    }
                    let ty = constant
                        .hir_ty(db)
                        .ok_or("associated constant has no type")?;
                    let kind = constant_kind(db, ty)?;
                    let name = constant.name(db).ok_or("unnamed associated constant")?;
                    Ok(Param {
                        name: name.data(db).clone(),
                        kind,
                    })
                })
                .collect::<Result<_>>()?;
            let methods = trait_
                .methods(db)
                .map(|method| method_record(db, method, diagnostics))
                .collect::<Result<_>>()?;
            if !bounds.is_empty() || !conds.is_empty() {
                return Err("trait-level requirements are outside format v1".into());
            }
            let name = trait_.name(db).to_opt().ok_or("unnamed trait")?;
            Ok(Item::Trait {
                name: name.data(db).clone(),
                params,
                assoc_consts,
                methods,
                fixed: false,
            })
        }
        ItemKind::ImplTrait(impl_) => {
            let reference = impl_
                .hir_trait_ref(db)
                .to_opt()
                .ok_or("missing trait reference")?;
            let (trait_, targs) = trait_reference(db, reference, &env, 0)?;
            let self_ty = syntax_type(
                db,
                impl_.hir_type_ref(db).to_opt().ok_or("missing impl type")?,
                &env,
                0,
            )?;
            let consts = impl_
                .hir_consts(db)
                .iter()
                .map(|constant| {
                    let body = constant
                        .value
                        .to_opt()
                        .ok_or("missing associated constant value")?;
                    let name = constant
                        .name
                        .to_opt()
                        .ok_or("unnamed associated constant")?;
                    Ok(ConstValue {
                        name: name.data(db).clone(),
                        value: syntax_expr(db, body, body.expr(db), &env, 0)?,
                    })
                })
                .collect::<Result<_>>()?;
            let methods = impl_
                .methods(db)
                .map(|method| method_record(db, method, diagnostics))
                .collect::<Result<_>>()?;
            Ok(Item::Impl {
                index: impl_.declaration_index(db),
                name: impl_.selection_name(db).map(|name| name.data(db).clone()),
                params,
                trait_,
                targs,
                self_ty,
                bounds,
                conds,
                consts,
                methods,
            })
        }
        _ => Err("declaration is outside format v1".into()),
    }
}

fn method_record<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    method: Func<'db>,
    diagnostics: &Diagnostics,
) -> Result<Method> {
    let owner = GenericParamOwner::Func(method);
    let (params, env) = parameters(db, owner)?;
    if !params.is_empty() {
        return Err("generic methods are outside format v1".into());
    }
    let (bounds, conds) = clause(db, owner, &env)?;
    if !bounds.is_empty() {
        return Err("method bounds are outside format v1".into());
    }
    let name = method.name(db).to_opt().ok_or("unnamed method")?;
    Ok(Method {
        name: name.data(db).clone(),
        conds,
        span_aliases: span_aliases(db, method.span().sig().into(), &[], diagnostics, None)?,
    })
}

/// The owner's own parameters, and the kinds of every parameter in scope.
fn parameters<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    owner: GenericParamOwner<'db>,
) -> Result<(Vec<Param>, Parameters)> {
    let mut chain = vec![owner];
    while let Some(parent) = chain.last().and_then(|scope| scope.parent(db)) {
        chain.push(parent);
    }
    let mut env = Parameters::new();
    for scope in chain.iter().rev() {
        for parameter in scope.params(db) {
            let name = parameter.name().to_opt().ok_or("unnamed parameter")?;
            let kind = match parameter.param {
                GenericParam::Type(_) => Kind::Type,
                GenericParam::Const(constant) => constant_kind(
                    db,
                    constant
                        .ty
                        .to_opt()
                        .ok_or("constant parameter has no type")?,
                )?,
            };
            env.insert(name.data(db).clone(), kind);
        }
    }
    let params = owner
        .params(db)
        .map(|parameter| {
            let name = parameter
                .name()
                .to_opt()
                .ok_or("unnamed parameter")?
                .data(db);
            Ok(Param {
                name: name.clone(),
                kind: env[name],
            })
        })
        .collect::<Result<_>>()?;
    Ok((params, env))
}

fn constant_kind<'db>(db: &'db dyn SpannedHirAnalysisDb, ty: TypeId<'db>) -> Result<Kind> {
    match ty.pretty_print(db).as_str() {
        "bool" => Ok(Kind::Bool),
        "usize" => Ok(Kind::Nat),
        _ => Err("constant kind is outside the v1 fragment (bool/usize)".into()),
    }
}

/// The owner's trait bounds (inline and in its where clause) and const conditions.
fn clause<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    owner: GenericParamOwner<'db>,
    env: &Parameters,
) -> Result<(Vec<Goal>, Vec<Term>)> {
    let clause = owner
        .where_clause_owner()
        .ok_or("owner has no where clause")?
        .where_clause(db);
    let mut bounds = vec![];
    for parameter in owner.params(db) {
        if let GenericParam::Type(parameter) = parameter.param {
            let name = parameter.name.to_opt().ok_or("unnamed parameter")?;
            let self_ty = Term::Var {
                var: name.data(db).clone(),
                kind: Some(Kind::Type),
            };
            append_bounds(db, self_ty, &parameter.bounds, env, &mut bounds)?;
        }
    }
    for (_, predicate) in clause.type_predicates(db) {
        let self_ty = syntax_type(
            db,
            predicate.ty.to_opt().ok_or("missing bound subject")?,
            env,
            0,
        )?;
        append_bounds(db, self_ty, &predicate.bounds, env, &mut bounds)?;
    }
    let conds = clause
        .const_predicates(db)
        .iter()
        .map(|body| syntax_expr(db, *body, body.expr(db), env, 0))
        .collect::<Result<_>>()?;
    Ok((bounds, conds))
}

fn append_bounds<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    self_ty: Term,
    bounds: &[TypeBound<'db>],
    env: &Parameters,
    output: &mut Vec<Goal>,
) -> Result<()> {
    for bound in bounds {
        let TypeBound::Trait(reference) = bound else {
            return Err("kind bound is outside format v1".into());
        };
        let (trait_, args) = trait_reference(db, *reference, env, 0)?;
        output.push(Goal {
            self_ty: self_ty.clone(),
            trait_,
            args,
        });
    }
    Ok(())
}

fn trait_reference<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    reference: TraitRefId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<(String, Vec<Term>)> {
    let path = reference.path(db).to_opt().ok_or("missing trait path")?;
    simple_path(db, path, env, depth)
}

/// An unqualified name with its generic arguments.
fn simple_path<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    path: PathId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<(String, Vec<Term>)> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 source term exceeds depth 64".into());
    }
    if path.parent(db).is_some() {
        return Err("qualified name is outside this v1 source projection".into());
    }
    let PathKind::Ident {
        ident,
        generic_args,
    } = path.kind(db)
    else {
        return Err("generated path is outside format v1".into());
    };
    let name = ident.to_opt().ok_or("missing path name")?.data(db).clone();
    let args = generic_args
        .data(db)
        .iter()
        .map(|argument| match argument {
            GenericArg::Type(argument) => syntax_type(
                db,
                argument
                    .ty
                    .to_opt()
                    .ok_or("missing generic type argument")?,
                env,
                depth + 1,
            ),
            GenericArg::Const(argument) => match argument.value {
                ConstGenericArgValue::Expr(body) => {
                    let body = body.to_opt().ok_or("missing constant argument")?;
                    syntax_expr(db, body, body.expr(db), env, depth + 1)
                }
                _ => Err("constant hole is outside format v1".into()),
            },
            _ => Err("associated type argument is outside format v1".into()),
        })
        .collect::<Result<_>>()?;
    Ok((name, args))
}

fn syntax_type<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    ty: TypeId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<Term> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 source term exceeds depth 64".into());
    }
    match ty.data(db) {
        TypeKind::Path(path) => {
            let path = path.to_opt().ok_or("missing type path")?;
            if let Some((goal, name)) = qualified_member(db, path, env, depth + 1)? {
                return Ok(Term::assoc(goal, name));
            }
            let (name, args) = simple_path(db, path, env, depth + 1)?;
            if let Some(kind) = env.get(&name) {
                if !args.is_empty() {
                    return Err("applied parameter is outside format v1".into());
                }
                Ok(Term::Var {
                    var: name,
                    kind: Some(*kind),
                })
            } else {
                Ok(Term::Con { con: name, args })
            }
        }
        TypeKind::Primitive(primitive) => Ok(Term::Con {
            con: primitive.name(db).data(db).clone(),
            args: vec![],
        }),
        TypeKind::Tuple(types) if types.data(db).is_empty() => Ok(Term::Con {
            con: "()".into(),
            args: vec![],
        }),
        _ => Err("type is outside the v1 source fragment".into()),
    }
}

fn syntax_expr<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: Body<'db>,
    expr: ExprId,
    env: &Parameters,
    depth: usize,
) -> Result<Term> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 source term exceeds depth 64".into());
    }
    let Partial::Present(expression) = expr.data(db, body) else {
        return Err("expression is outside the v1 source fragment".into());
    };
    match expression {
        Expr::Block(statements) if statements.len() == 1 => {
            let Partial::Present(Stmt::Expr(value)) = statements[0].data(db, body) else {
                return Err("constant block is outside the v1 source fragment".into());
            };
            syntax_expr(db, body, *value, env, depth + 1)
        }
        Expr::Lit(LitKind::Bool(value)) => Ok(Term::Bool { bool: *value }),
        Expr::Lit(LitKind::Int(value)) => Ok(Term::Nat {
            nat: value
                .data(db)
                .to_string()
                .parse()
                .map_err(|_| "natural literal exceeds encoder range")?,
        }),
        Expr::Path(path) => {
            let path = path.to_opt().ok_or("missing expression path")?;
            if let Some((goal, name)) = qualified_member(db, path, env, depth + 1)? {
                return Ok(Term::assoc(goal, name));
            }
            let (name, args) = simple_path(db, path, env, depth + 1)?;
            if !args.is_empty() {
                return Err("applied expression name is outside format v1".into());
            }
            let kind = env.get(&name).copied();
            Ok(Term::Var { var: name, kind })
        }
        Expr::Bin(a, b, op) => Ok(Term::Op {
            op: binary_operator(*op)?,
            a: Box::new(syntax_expr(db, body, *a, env, depth + 1)?),
            b: Box::new(syntax_expr(db, body, *b, env, depth + 1)?),
        }),
        Expr::Un(value, UnOp::Not) => Ok(Term::Not {
            not: Box::new(syntax_expr(db, body, *value, env, depth + 1)?),
        }),
        _ => Err("expression is outside the v1 source fragment".into()),
    }
}

/// `<T as Tr<..>>::NAME`, as the goal and the member name.
fn qualified_member<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    path: PathId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<Option<(Goal, String)>> {
    let Some(parent) = path.parent(db) else {
        return Ok(None);
    };
    let PathKind::QualifiedType { type_, trait_ } = parent.kind(db) else {
        return Ok(None);
    };
    let PathKind::Ident {
        ident,
        generic_args,
    } = path.kind(db)
    else {
        return Err("missing qualified member".into());
    };
    if !generic_args.data(db).is_empty() {
        return Err("generic member is outside format v1".into());
    }
    let (trait_name, args) = trait_reference(db, trait_, env, depth)?;
    let self_ty = syntax_type(db, type_, env, depth)?;
    let name = ident.to_opt().ok_or("unnamed qualified member")?;
    Ok(Some((
        Goal {
            self_ty,
            trait_: trait_name,
            args,
        },
        name.data(db).clone(),
    )))
}

/// The uses of a body expression, in source order.
fn body_uses<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: Body<'db>,
    expr: ExprId,
    env: &Parameters,
    typed: &TypedBody<'db>,
    diagnostics: &Diagnostics,
    depth: usize,
) -> Result<Vec<Use>> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 use nesting exceeds depth 64".into());
    }
    let Partial::Present(expression) = expr.data(db, body) else {
        return Err("expression is outside the v1 use fragment".into());
    };
    if let Expr::Block(statements) = expression {
        let mut uses = vec![];
        for statement in statements {
            let Partial::Present(statement_data) = statement.data(db, body) else {
                return Err("statement is outside the v1 use fragment".into());
            };
            let value = match statement_data {
                Stmt::Let(_, _, Some(value)) | Stmt::Expr(value) => *value,
                _ => return Err("statement is outside the v1 use fragment".into()),
            };
            let mut children = body_uses(db, body, value, env, typed, diagnostics, depth + 1)?;
            // A written type on a single-use `let` belongs to that use.
            if matches!(statement_data, Stmt::Let(_, Some(_), _))
                && let [use_] = children.as_mut_slice()
            {
                let aliases = span_aliases(
                    db,
                    statement.span(body).into_let_stmt().ty().into(),
                    &[],
                    diagnostics,
                    Some(typed),
                )?;
                for alias in aliases {
                    if !use_.span_aliases.contains(&alias) {
                        use_.span_aliases.push(alias);
                    }
                }
            }
            uses.extend(children);
        }
        return Ok(uses);
    }
    let kind = match expression {
        Expr::Lit(_) => return Ok(vec![]),
        Expr::Call(callee, args) => {
            if !args.is_empty() {
                return Err("value arguments are outside the v1 use fragment".into());
            }
            let Partial::Present(Expr::Path(path)) = callee.data(db, body) else {
                return Err("non-path call is outside format v1".into());
            };
            let path = path.to_opt().ok_or("missing call path")?;
            if let Some((goal, method)) = qualified_member(db, path, env, 0)? {
                UseKind::Method {
                    goal,
                    method,
                    qualified: true,
                }
            } else if path.parent(db).is_some() {
                let instance = typed
                    .callable_expr(expr)
                    .and_then(|callable| callable.trait_inst)
                    .ok_or("unqualified call has no checked trait instance")?;
                let method = path.ident(db).to_opt().ok_or("unnamed method")?;
                UseKind::Method {
                    goal: goal(db, instance)?,
                    method: method.data(db).clone(),
                    qualified: false,
                }
            } else {
                let (function, args) = simple_path(db, path, env, 0)?;
                UseKind::Call { function, args }
            }
        }
        Expr::Path(path) => {
            let (goal, c) =
                qualified_member(db, path.to_opt().ok_or("missing constant path")?, env, 0)?
                    .ok_or("unqualified value use is outside this v1 projection")?;
            UseKind::Const { goal, c }
        }
        Expr::RecordInit(path, fields) => {
            if !fields.is_empty() {
                return Err("record fields are outside format v1".into());
            }
            let (con, args) = simple_path(db, path.to_opt().ok_or("missing record path")?, env, 0)?;
            UseKind::Type {
                ty: Term::Con { con, args },
            }
        }
        Expr::With(bindings, inner) => {
            let mut names = bindings
                .iter()
                .map(|binding| {
                    if binding.key_path.is_some() {
                        return Err("keyed opening is outside format v1".into());
                    }
                    let Partial::Present(Expr::Path(path)) = binding.value.data(db, body) else {
                        return Err("evidence opening requires the Step 2 record".into());
                    };
                    let (name, args) =
                        simple_path(db, path.to_opt().ok_or("missing opening path")?, env, 0)?;
                    if !args.is_empty() {
                        return Err("applied opening is outside format v1".into());
                    }
                    Ok(name)
                })
                .collect::<Result<Vec<_>>>()?;
            let body = body_uses(db, body, *inner, env, typed, diagnostics, depth + 1)?;
            if names.len() == 1 {
                UseKind::With {
                    name: names.remove(0),
                    body,
                }
            } else {
                UseKind::WithList { names, body }
            }
        }
        _ => return Err("expression is outside the v1 use fragment".into()),
    };
    let construct_span = source_site(db, &expr.span(body).into())?;
    let (span, excluded) = match expression {
        Expr::With(bindings, inner) => {
            let span = match bindings.as_slice() {
                [binding] => source_site(db, &binding.value.span(body).into())?,
                _ => construct_span,
            };
            (span, vec![inner.span(body).into()])
        }
        _ => (construct_span, vec![]),
    };
    let span_aliases = span_aliases(
        db,
        expr.span(body).into(),
        &excluded,
        diagnostics,
        Some(typed),
    )?;
    let construct_span = matches!(kind, UseKind::With { .. }).then_some(construct_span);
    Ok(vec![Use {
        kind,
        span,
        span_aliases,
        construct_span,
    }])
}

// Alias actual reported/retained coordinates to the smallest authored use
// region. Nested with bodies are handled by their own use records.
fn span_aliases<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    region: DynLazySpan<'db>,
    excluded: &[DynLazySpan<'db>],
    diagnostics: &Diagnostics,
    typed: Option<&TypedBody<'db>>,
) -> Result<Vec<Site>> {
    let region = region
        .resolve(db)
        .ok_or("authored region has no source span")?;
    let excluded = excluded
        .iter()
        .filter_map(|s| s.resolve(db))
        .collect::<Vec<_>>();
    let mut points = diagnostics
        .iter()
        .filter_map(|d| d.primary_span())
        .collect::<Vec<_>>();
    if let Some(typed) = typed {
        points.extend(
            typed
                .decisions()
                .condition_entries()
                .filter_map(|(_, c)| c.span().resolve(db)),
        );
        points.extend(
            typed
                .decisions()
                .choice_entries(db)
                .filter_map(|c| c.span().resolve(db)),
        );
        if let Some(body) = typed.body() {
            points.extend(
                typed
                    .with_choices()
                    .filter_map(|c| c.expression().span(body).resolve(db)),
            );
        }
    }
    let mut offsets = std::collections::BTreeSet::new();
    let mut result = vec![];
    for point in points {
        let inside = |r: &common::diagnostics::Span| {
            point.file == r.file && r.range.contains(point.range.start())
        };
        if inside(&region) && !excluded.iter().any(inside) && offsets.insert(point.range.start()) {
            result.push(span_site(db, &point)?);
        }
    }
    Ok(result)
}
