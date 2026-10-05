use std::{collections::BTreeMap, ops::Range};

use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::{
    HirDb,
    analysis::ty::ty_check::{LocalBinding, TypedBody, check_func_body},
    hir_def::{
        Body, CallArg, CallableDef, Cond, CondId, Expr, ExprId, FieldIndex, Func, ItemKind,
        Partial, Pat, PatId, Stmt, StmtId, scope_graph::ScopeId,
    },
    span::LazySpan,
};
use parser::{
    SyntaxNode,
    ast::{self, prelude::*},
};
use url::Url;

/// What a body's names resolved to, so the comparison checks items, not
/// spellings: a printed path that re-resolves to another item differs here.
struct Resolved<'a, 'db> {
    db: &'db DriverDataBase,
    body: Body<'db>,
    typed: Option<&'a TypedBody<'db>>,
}

impl<'db> Resolved<'_, 'db> {
    fn callable(&self, expr: ExprId) -> String {
        let Some(callable) = self.typed.and_then(|typed| typed.callable_expr(expr)) else {
            return "unresolved".into();
        };
        let scope = match callable.callable_def() {
            CallableDef::Func(func) => ItemKind::Func(func).scope(),
            CallableDef::VariantCtor(variant) => ScopeId::Variant(variant),
        };
        scope.pretty_path(self.db).unwrap_or_default()
    }

    fn value(&self, expr: ExprId) -> String {
        let Some(typed) = self.typed else {
            return String::new();
        };
        match typed.expr_binding(expr) {
            Some(LocalBinding::Local { .. }) => "local".into(),
            Some(LocalBinding::Param { idx, .. }) => format!("param {idx}"),
            Some(_) => "binding".into(),
            None => typed
                .expr_ty(self.db, expr)
                .pretty_print(self.db)
                .to_string(),
        }
    }

    fn exprs(&self, ids: &[ExprId]) -> Vec<String> {
        ids.iter().map(|id| self.expr(*id)).collect()
    }

    fn args(&self, args: &[CallArg<'db>]) -> Vec<(Option<String>, String)> {
        args.iter()
            .map(|arg| {
                (
                    arg.label.map(|label| label.data(self.db).clone()),
                    self.expr(arg.expr),
                )
            })
            .collect()
    }

    fn expr(&self, id: ExprId) -> String {
        let db = self.db;
        match id.data(db, self.body).unwrap_ref() {
            Expr::Lit(value) => format!("Lit({:?})", value.pretty_print(db)),
            Expr::Path(Partial::Present(path)) => {
                format!("Path({:?}, {:?})", path.pretty_print(db), self.value(id))
            }
            Expr::Cast(value, ty) => format!(
                "Cast({}, {:?})",
                self.expr(*value),
                ty.to_opt().map(|ty| ty.pretty_print(db))
            ),
            Expr::Bin(lhs, rhs, op) => {
                format!("Bin({op:?}, {}, {})", self.expr(*lhs), self.expr(*rhs))
            }
            Expr::Un(value, op) => format!("Un({op:?}, {})", self.expr(*value)),
            Expr::Call(callee, args) => format!(
                "Call({}, {:?}, {:?})",
                self.expr(*callee),
                self.callable(id),
                self.args(args)
            ),
            Expr::Assert(args) => format!("Assert({:?})", self.args(args)),
            Expr::MethodCall(receiver, name, generic_args, args) => format!(
                "MethodCall({}, {:?}, {:?}, {:?}, {:?})",
                self.expr(*receiver),
                name.to_opt().map(|name| name.data(db).clone()),
                generic_args.pretty_print(db),
                self.callable(id),
                self.args(args)
            ),
            Expr::RecordInit(Partial::Present(path), fields) => format!(
                "RecordInit({:?}, {:?}, {:?})",
                path.pretty_print(db),
                self.value(id),
                fields
                    .iter()
                    .map(|field| (
                        field
                            .label_eagerly(db, self.body)
                            .map(|label| label.data(db).clone()),
                        self.expr(field.expr)
                    ))
                    .collect::<Vec<_>>()
            ),
            Expr::Field(base, Partial::Present(field)) => {
                let field = match field {
                    FieldIndex::Ident(name) => name.data(db).clone(),
                    FieldIndex::Index(index) => index.data(db).to_string(),
                    FieldIndex::Hole(slot) => {
                        panic!("generated HIR still contains quote field hole {slot}")
                    }
                };
                format!("Field({}, {:?})", self.expr(*base), field)
            }
            Expr::Tuple(values) => format!("Tuple({:?})", self.exprs(values)),
            Expr::Array(values) => format!("Array({:?})", self.exprs(values)),
            Expr::If(cond, then, otherwise) => format!(
                "If({}, {}, {:?})",
                self.cond(*cond),
                self.expr(*then),
                otherwise.map(|otherwise| self.expr(otherwise))
            ),
            Expr::Match(scrutinee, Partial::Present(arms)) => format!(
                "Match({}, {:?})",
                self.expr(*scrutinee),
                arms.iter()
                    .filter_map(|arm| arm.parts())
                    .map(|(pat, body)| (self.pat(pat), self.expr(body)))
                    .collect::<Vec<_>>()
            ),
            Expr::Assign(lhs, rhs) => format!("Assign({}, {})", self.expr(*lhs), self.expr(*rhs)),
            Expr::AugAssign(lhs, rhs, op) => {
                format!(
                    "AugAssign({op:?}, {}, {})",
                    self.expr(*lhs),
                    self.expr(*rhs)
                )
            }
            Expr::Block(stmts) => format!(
                "Block({:?})",
                stmts
                    .iter()
                    .map(|stmt| self.stmt(*stmt))
                    .collect::<Vec<_>>()
            ),
            other => panic!("generated expression needs parity normalization: {other:?}"),
        }
    }

    fn cond(&self, id: CondId) -> String {
        match &self.body.conds(self.db)[id] {
            Partial::Present(Cond::Expr(expr)) => self.expr(*expr),
            Partial::Present(Cond::Let(pat, expr)) => {
                format!("Let({}, {})", self.pat(*pat), self.expr(*expr))
            }
            Partial::Present(Cond::Bin(lhs, rhs, op)) => {
                format!("CondBin({op:?}, {}, {})", self.cond(*lhs), self.cond(*rhs))
            }
            Partial::Absent => panic!("generated condition is absent"),
        }
    }

    fn pat(&self, id: PatId) -> String {
        let db = self.db;
        match id.data(db, self.body).unwrap_ref() {
            Pat::Path(Partial::Present(path), mutable) => {
                format!("Path({:?}, {mutable})", path.pretty_print(db))
            }
            Pat::WildCard => "WildCard".into(),
            Pat::Rest => "Rest".into(),
            Pat::Lit(literal) => format!(
                "Lit({:?})",
                literal.to_opt().map(|lit| lit.pretty_print(db))
            ),
            Pat::Tuple(pats) => format!(
                "Tuple({:?})",
                pats.iter().map(|pat| self.pat(*pat)).collect::<Vec<_>>()
            ),
            Pat::PathTuple(Partial::Present(path), pats) => format!(
                "PathTuple({:?}, {:?})",
                path.pretty_print(db),
                pats.iter().map(|pat| self.pat(*pat)).collect::<Vec<_>>()
            ),
            Pat::Record(Partial::Present(path), fields) => format!(
                "Record({:?}, {:?})",
                path.pretty_print(db),
                fields
                    .iter()
                    .map(|field| (
                        field
                            .label(db, self.body)
                            .map(|label| label.data(db).clone()),
                        self.pat(field.pat)
                    ))
                    .collect::<Vec<_>>()
            ),
            Pat::Or(lhs, rhs) => format!("Or({}, {})", self.pat(*lhs), self.pat(*rhs)),
            other => panic!("generated pattern needs parity normalization: {other:?}"),
        }
    }

    fn stmt(&self, id: StmtId) -> String {
        let db = self.db;
        match id.data(db, self.body).unwrap_ref() {
            Stmt::Let(pat, ty, value) => format!(
                "Let({}, {:?}, {:?})",
                self.pat(*pat),
                ty.map(|ty| ty.pretty_print(db)),
                value.map(|value| self.expr(value))
            ),
            Stmt::For(pat, iterable, body, unroll) => format!(
                "For({}, {}, {}, {unroll:?})",
                self.pat(*pat),
                self.expr(*iterable),
                self.expr(*body)
            ),
            Stmt::While(cond, body) => format!("While({}, {})", self.cond(*cond), self.expr(*body)),
            Stmt::Continue => "Continue".into(),
            Stmt::Break => "Break".into(),
            Stmt::Return(value) => format!("Return({:?})", value.map(|value| self.expr(value))),
            Stmt::Expr(value) => format!("Expr({})", self.expr(*value)),
        }
    }
}

fn normalized_function(db: &DriverDataBase, function: Func<'_>) -> (String, String) {
    let body = function.body(db).expect("generated function body");
    let resolved = Resolved {
        db,
        body,
        typed: Some(&check_func_body(db, function).1),
    };
    (
        function.pretty_print_signature(db),
        resolved.expr(body.expr(db)),
    )
}

/// Generated local binders keep their authored name marked with a trailing
/// `'`, which source code cannot spell. Give them stable source spellings only
/// for this print-and-reparse oracle.
fn replayable_names(source: &str) -> String {
    let mut replayable = String::with_capacity(source.len());
    let mut previous = None;
    for ch in source.chars() {
        if ch == '\'' && previous.is_some_and(|p: char| p == '_' || p.is_ascii_alphanumeric()) {
            replayable.push_str("__fe_quoted");
        } else {
            replayable.push(ch);
        }
        previous = Some(ch);
    }
    replayable
}

fn normalized_item(db: &DriverDataBase, item: ItemKind<'_>) -> String {
    let normalized = match item {
        ItemKind::Func(function) => format!("Function({:?})", normalized_function(db, function)),
        ItemKind::ImplTrait(implementation) => {
            let printed = implementation.pretty_print(db);
            let header = printed
                .split_once(" {\n")
                .expect("printed impl header")
                .0
                .replace("< ", "<")
                .replace(" >", ">");
            let constants = implementation
                .hir_consts(db)
                .iter()
                .map(|constant| {
                    let name = constant.name.to_opt().expect("const name").data(db);
                    let ty = constant.ty.to_opt().expect("const type").pretty_print(db);
                    let body = constant.value.to_opt().expect("const value");
                    let resolved = Resolved {
                        db,
                        body,
                        typed: None,
                    };
                    (name.clone(), ty, resolved.expr(body.expr(db)))
                })
                .collect::<Vec<_>>();
            let methods = implementation
                .methods(db)
                .map(|method| normalized_function(db, method))
                .collect::<Vec<_>>();
            format!("Impl({header:?}, {constants:?}, {methods:?})")
        }
        other => panic!("unexpected generated item: {other:?}"),
    };
    assert!(
        !normalized.contains("<quote-record-"),
        "generated generic names leaked into printed output: {normalized}"
    );
    replayable_names(&normalized)
}

fn is_top_level_generated_item(db: &DriverDataBase, item: ItemKind<'_>) -> bool {
    if !matches!(item, ItemKind::Func(_) | ItemKind::ImplTrait(_)) {
        return false;
    }
    !item
        .scope()
        .parent(db)
        .is_some_and(|parent| matches!(parent.item(), ItemKind::ImplTrait(_)))
}

fn assert_parity(fixture: Fixture<&str>, expected_private_calls: Option<usize>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture path is absolute");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), fixture.content().to_string());
    let output = fe_driver::elaboration::prepare(&authored)
        .unwrap()
        .expect("generation request");
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let root = ast::Root::cast(SyntaxNode::new_root(hir::lower::parse_file_impl(
        &output, module,
    )))
    .unwrap();
    let requests = output
        .generated_module(&url)
        .expect("frozen generation input");
    let targets = requests
        .requests(&output)
        .iter()
        .map(|request| {
            let target = root
                .syntax()
                .descendants()
                .filter_map(ast::Struct::cast)
                .find(|target| {
                    let span: Range<usize> = target.syntax().text_range().into();
                    request
                        .site_span
                        .as_ref()
                        .is_some_and(|site| span.start <= site.start && site.end <= span.end)
                })
                .expect("authored generation target");
            let span: Range<usize> = target.syntax().text_range().into();
            span
        })
        .collect::<Vec<_>>();
    let mut generated = Vec::new();
    for item in module.all_items(&output).iter().copied() {
        if !is_top_level_generated_item(&output, item) {
            continue;
        }
        let Some(span) = item.span().resolve(&output) else {
            continue;
        };
        if span.file.url(&output).as_ref() != Some(&url) {
            continue;
        }
        let span: Range<usize> = span.range.into();
        let Some(target) = targets
            .iter()
            .find(|target| target.start <= span.start && span.start < target.end)
        else {
            continue;
        };
        generated.push((
            item,
            target.end,
            replayable_names(&item.pretty_print(&output)),
        ));
    }
    assert!(
        !generated.is_empty(),
        "fixture produced no generated HIR items"
    );

    let mut insertions = BTreeMap::<usize, Vec<(usize, String)>>::new();
    for (index, (_, offset, printed)) in generated.iter().enumerate() {
        insertions
            .entry(*offset)
            .or_default()
            .push((index, printed.clone()));
    }
    let mut replay_source = fixture.content().to_string();
    for (offset, entries) in insertions.into_iter().rev() {
        let inserted = entries
            .into_iter()
            .map(|(index, printed)| format!("\n// generated_hir_parity_{index}\n{printed}\n"))
            .collect::<String>();
        replay_source.insert_str(offset, &inserted);
    }
    let mut replay = DriverDataBase::default();
    replay
        .workspace()
        .update(&mut replay, url.clone(), replay_source.clone());
    let replay_file = replay.workspace().get(&replay, &url).unwrap();
    let replay_module = replay.top_mod(replay_file);
    let items = replay_module.all_items(&replay);
    for (index, (original, _, _)) in generated.iter().enumerate() {
        let marker = format!("// generated_hir_parity_{index}\n");
        let start = replay_source.find(&marker).expect("printed item marker") + marker.len();
        let parsed = items
            .iter()
            .copied()
            .find(|item| {
                item.span().resolve(&replay).is_some_and(|span| {
                    let range: Range<usize> = span.range.into();
                    span.file.url(&replay).as_ref() == Some(&url) && range.start == start
                })
            })
            .unwrap_or_else(|| panic!("re-lowered item {index} is absent"));
        assert_eq!(
            normalized_item(&output, *original),
            normalized_item(&replay, parsed),
            "item {index}"
        );
    }
    let diagnostics = replay.run_on_ingot(replay_module.ingot(&replay));
    if let Some(expected) = expected_private_calls {
        // Printed source cannot carry a provider's authority to call its
        // private helper. The HIR comparison above still checks each call.
        let errors = diagnostics
            .complete_diags(&replay)
            .into_iter()
            .filter(|diag| diag.severity == common::diagnostics::Severity::Error)
            .collect::<Vec<_>>();
        assert_eq!(
            errors.len(),
            expected,
            "{}",
            diagnostics.format_diags(&replay)
        );
        for error in errors {
            assert_eq!(error.error_code.to_string(), "2-0003");
            assert_eq!(error.message, "`encode_sol` is not visible");
        }
    } else {
        assert!(
            !diagnostics.has_errors(&replay),
            "{}",
            diagnostics.format_diags(&replay)
        );
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generated_const_generic_function", glob: "*.fe")]
fn generated_function_roundtrips(fixture: Fixture<&str>) {
    assert_parity(fixture, None);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generic_derive", glob: "*.fe")]
fn generated_implementation_roundtrips(fixture: Fixture<&str>) {
    assert_parity(fixture, None);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/structural_generation", glob: "abi.fe")]
fn generated_abi_roundtrips(fixture: Fixture<&str>) {
    assert_parity(fixture, Some(7));
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generated_hir_parity", glob: "*.fe")]
fn generated_forms_roundtrip(fixture: Fixture<&str>) {
    assert_parity(fixture, None);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_forms/allowed", glob: "*.fe")]
fn quoted_forms_roundtrip(fixture: Fixture<&str>) {
    assert_parity(fixture, None);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_item_composition", glob: "*.fe")]
fn composed_items_roundtrip(fixture: Fixture<&str>) {
    assert_parity(fixture, None);
}
