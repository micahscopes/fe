//! The derive-provider command-language executor.
//!
//! Provider bodies are written in a restricted subset of Fe: boolean locals,
//! `if`/`else`, `for` loops over reflection lists, and method calls on the
//! capability values bound by the `uses` clause. This module interprets such
//! a body at compile time (inside the expansion stage) and records the
//! builder commands it issues. The commands are *transient command data*:
//! they are replayed into real HIR by [`super::provider_synthesis`] and
//! never persisted as a semantic artifact.
//!
//! Compared to the metaprogramming prototype this interpreter:
//! * dispatches method calls on the *resolved value* of the receiver, so
//!   `let builder = ..` shadowing behaves like ordinary Fe scoping instead
//!   of matching receiver identifier text;
//! * makes `return` actually stop execution;
//! * enforces an explicit step budget and a command-count cap, so a buggy
//!   provider degrades into a diagnostic instead of a hang.

use parser::TextRange;
use parser::ast::prelude::*;

use super::{
    provider::{TargetReflection, ValidatedProvider, canonical_trait_path},
    top_mod_ast,
};
use crate::{
    HirDb,
    analysis::ty::ty_def::MAX_INLINE_STRING_BYTES,
    hir_def::{
        BinOp, Body, CompBinOp, Cond, CondId, Expr, ExprId, GenericArg, GenericArgListId, IdentId,
        LitKind, LogicalBinOp, MatchArm, Partial, Pat, PatId, PathId, QuoteBody, Stmt, StmtId,
        StringId, TypeId, TypeKind,
    },
    span::HirOrigin,
};

/// Maximum number of statement/expression evaluations for one provider run.
const STEP_BUDGET: usize = 100_000;
/// Maximum number of builder commands (and generated expression nodes) for
/// one provider run.
const COMMAND_BUDGET: usize = 10_000;

/// Why a provider execution failed. Rendered into a derive diagnostic at the
/// request site (with the failing provider expression as the primary span
/// when it lies in the same file, see `expansion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailureKind {
    /// The body uses a construct outside the provider command language.
    UnsupportedBody,
    /// The provider returned without calling `builder.finish()`.
    MissingFinish,
    /// `builder.finish()` was called more than once.
    DuplicateFinish,
    /// A builder command was issued after `builder.finish()`.
    CommandAfterFinish,
    /// A `require` command had a malformed trait argument or operand.
    InvalidRequirement,
    /// A method-emission command was malformed (bad signature value, body
    /// value, or duplicate method name).
    InvalidMethod { detail: String },
    /// An associated-item emission command (`emit_const`, `emit_assoc_ty`)
    /// was malformed (bad name, type, or value, or a duplicate name).
    InvalidAssoc { detail: String },
    /// A compile-time string operation was malformed (non-string operand,
    /// or a piece exceeding the inline string capacity).
    InvalidString { detail: String },
    /// A quote template was malformed: an unsupported construct in the
    /// body, an open name the destination method does not bind, or a hole
    /// filled with a wrong-kind value.
    InvalidQuote { detail: String },
    /// The interpreter step budget or command cap was exceeded.
    BudgetExceeded,
}

impl ProviderFailureKind {
    pub fn message(&self) -> String {
        match self {
            Self::UnsupportedBody => {
                "this construct is not supported in derive provider bodies".into()
            }
            Self::MissingFinish => "the provider returned without calling `finish`".into(),
            Self::DuplicateFinish => "the provider called `finish` more than once".into(),
            Self::CommandAfterFinish => {
                "the provider issued a builder command after `finish`".into()
            }
            Self::InvalidRequirement => "invalid `require` command".into(),
            Self::InvalidMethod { detail } => format!("invalid method emission: {detail}"),
            Self::InvalidAssoc { detail } => {
                format!("invalid associated item emission: {detail}")
            }
            Self::InvalidString { detail } => {
                format!("invalid compile-time string operation: {detail}")
            }
            Self::InvalidQuote { detail } => format!("invalid quote: {detail}"),
            Self::BudgetExceeded => {
                "the provider exceeded its compile-time execution budget".into()
            }
        }
    }
}

/// An execution failure, with the source range of the offending construct
/// *inside the provider's file*.
#[derive(Debug, Clone)]
pub(super) struct ExecError {
    pub(super) kind: ProviderFailureKind,
    pub(super) range: TextRange,
}

/// A generated expression node, built by builder expression commands and
/// replayed into real HIR expressions by the synthesis module.
#[derive(Debug, Clone)]
pub(super) enum GenExpr<'db> {
    Bool(bool),
    /// `lhs && rhs`
    And(GenExprId, GenExprId),
    /// `lhs || rhs`
    Or(GenExprId, GenExprId),
    /// `lhs + rhs` (integer addition; used by layout-fact providers folding
    /// per-field size consts, e.g. ABI `HEAD_WORDS`).
    Add(GenExprId, GenExprId),
    /// The generated method's `self` value.
    SelfRef,
    /// A reference to a generated method parameter by name.
    ArgRef(IdentId<'db>),
    /// `base.field`
    FieldGet(GenExprId, FieldKey),
    /// `lhs == rhs`
    EqCmp(GenExprId, GenExprId),
    /// `lhs < rhs`
    LtCmp(GenExprId, GenExprId),
    /// `lhs > rhs`
    GtCmp(GenExprId, GenExprId),
    /// `<ty as GoalishTrait>::method(args..)` — a qualified call of the
    /// request's goal trait method on `ty` (`Self::method(args..)` when `ty`
    /// is the `Self` type).
    TraitCall {
        ty: TypeId<'db>,
        method: IdentId<'db>,
        args: Vec<GenExprId>,
    },
    /// `<ty as GoalishTrait>::NAME` — a qualified reference to an associated
    /// const of the request's goal trait on `ty` (`Self::NAME` when `ty` is
    /// the `Self` type).
    TraitConst {
        ty: TypeId<'db>,
        name: IdentId<'db>,
    },
    /// `receiver.method(args..)` — a method call on a generated expression.
    MethodCall {
        receiver: GenExprId,
        method: IdentId<'db>,
        args: Vec<GenExprId>,
    },
    /// `path(args..)` — a call through a path built from a type as written
    /// with an associated-function name appended (e.g. `Hash712::new()`).
    StaticCall {
        path: PathId<'db>,
        args: Vec<GenExprId>,
    },
    /// A string literal with the exact inline width of its text.
    StrLit(StringId<'db>),
    /// `(elem0, elem1, ..)`
    Tuple(Vec<GenExprId>),
    /// `core::keccak(arg)`
    Keccak(GenExprId),
    /// `Self { field: value, .. }`
    StructInit {
        fields: Vec<(FieldKey, GenExprId)>,
    },
    /// `Enum::Variant` / `Enum::Variant(..)` / `Enum::Variant { .. }`
    VariantInit {
        variant: usize,
        fields: Vec<(FieldKey, GenExprId)>,
    },
    /// `match scrutinee { arms }`
    Match {
        scrutinee: GenExprId,
        arms: Vec<(GenPatId, GenExprId)>,
    },
    /// A reference to the binder introduced for `field` by a
    /// [`GenPat::Variant`] pattern with the same `prefix`.
    VariantBinder {
        variant: usize,
        field: usize,
        prefix: IdentId<'db>,
    },
}

/// A generated type, built by builder type commands and materialized into a
/// real [`TypeId`] by the synthesis module (some forms, like exact-width
/// string types, need a lowering context to build their const-argument
/// bodies, so materialization cannot happen during execution).
#[derive(Debug, Clone)]
pub(super) enum GenTy<'db> {
    /// `String<LEN>` — an exact-width inline string type.
    StringN(usize),
    /// A tuple of generated types.
    Tuple(Vec<GenTyId>),
    /// `<ty as GoalishTrait>::name` — a projection of the goal trait's
    /// associated type on `ty`.
    Projection { ty: TypeId<'db>, name: IdentId<'db> },
    /// A type as written (e.g. from `ty<T>()` or `field.ty()`).
    Concrete(TypeId<'db>),
}

#[derive(Debug, Clone)]
pub(super) enum GenPat<'db> {
    Wildcard,
    /// A pattern matching `variant`, binding every payload field to
    /// `{prefix}_{field-name-or-index}`.
    Variant {
        variant: usize,
        prefix: IdentId<'db>,
    },
}

/// A generated method signature, built with `builder.method("name")` +
/// `with_self` / `with_arg` / `returns`.
#[derive(Debug, Clone)]
pub(super) struct GenMethodSig<'db> {
    pub(super) name: IdentId<'db>,
    pub(super) takes_self: bool,
    pub(super) args: Vec<(IdentId<'db>, TypeId<'db>)>,
    pub(super) ret: Option<TypeId<'db>>,
}

/// A field reference: `(variant index, field index)` into the target
/// reflection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FieldKey {
    pub(super) variant: Option<usize>,
    pub(super) index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GenExprId(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GenPatId(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GenTyId(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SigId(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QuoteId(usize);

/// A quote template value: the inert body of a `quote(open, ..) { .. }`
/// expression plus the hole values captured when the quote expression was
/// evaluated. Templates only become generated expressions when a builder
/// emission command elaborates them (see [`ProviderExecutor::elaborate_quote`]).
#[derive(Debug, Clone)]
struct QuoteTemplate<'db> {
    /// The `Expr::Quote` expression, for error spans.
    origin: ExprId,
    /// Declared open names (`quote(other) { .. }`); `self` is implicitly
    /// open.
    open: Vec<IdentId<'db>>,
    /// The template body: a block expression in the provider's body, or a
    /// match-arm sequence.
    body: QuoteBody,
    /// Captured hole values, keyed by the hole expression
    /// (`Expr::QuoteHole` / `Expr::QuoteFieldHole`) for expression holes,
    /// and by the hole's inner expression for pattern holes
    /// (`Pat::QuoteHole`).
    holes: Vec<(ExprId, Value<'db>)>,
}

/// A binder group introduced by a `${variant}(group)` arm pattern enclosing
/// the elaboration point: the group name and the matched variant's index.
type BinderGroup<'db> = (IdentId<'db>, usize);

/// A builder command recorded during provider execution.
#[derive(Debug, Clone)]
pub(super) enum BuilderCommand<'db> {
    /// `builder.require<Trait>(ty)`: the generated impl requires
    /// `ty: Trait`. `trait_path` is the canonical path of the required
    /// trait (resolved against the provider module's imports).
    Require {
        ty: TypeId<'db>,
        trait_path: crate::hir_def::PathId<'db>,
    },
    /// `builder.emit_method(sig, body)`.
    EmitMethod { sig: SigId, body: GenExprId },
    /// `builder.emit_assoc_ty(name, ty)`: `type name = ty` in the generated
    /// impl.
    EmitAssocTy { name: IdentId<'db>, ty: GenTyId },
    /// `builder.emit_const(name, ty, value)`: `const name: ty = value` in
    /// the generated impl.
    EmitConst {
        name: IdentId<'db>,
        ty: GenTyId,
        value: GenExprId,
    },
}

/// The successful result of running a provider body: the recorded commands
/// plus the arenas the generated expression/pattern/signature ids index
/// into.
#[derive(Debug)]
pub(super) struct ProviderOutput<'db> {
    pub(super) exprs: Vec<GenExpr<'db>>,
    pub(super) pats: Vec<GenPat<'db>>,
    pub(super) tys: Vec<GenTy<'db>>,
    pub(super) sigs: Vec<GenMethodSig<'db>>,
    pub(super) commands: Vec<BuilderCommand<'db>>,
}

/// A compile-time value in the provider command language.
#[derive(Debug, Clone, Copy)]
enum Value<'db> {
    Bool(bool),
    /// A compile-time string (string literals, reflected names, and
    /// `concat` results).
    Str(StringId<'db>),
    /// A reflected field handle.
    Field(FieldKey),
    /// A reflected variant handle (index into the target's variants).
    Variant(usize),
    /// A type witness (e.g. the result of `field.ty()`).
    Ty(TypeId<'db>),
    /// A generated type (e.g. the result of `str_ty` / `tuple_ty`).
    GenTy(GenTyId),
    /// A generated expression.
    Expr(GenExprId),
    /// A generated pattern.
    Pat(GenPatId),
    /// A generated method signature.
    Sig(SigId),
    /// A quote template (`quote { .. }`).
    Quote(QuoteId),
    /// The `builder: mut ImplBuilder<..>` capability.
    Builder,
    /// The `reflect: Reflect<..>` capability.
    Reflect,
    /// An opaque evidence value (the provider's ordinary parameters).
    Evidence,
    /// The result of a command call; carries no data.
    Unit,
}

enum Flow {
    Continue,
    Return,
}

enum Iterable {
    StructFields,
    Variants,
    VariantFields(usize),
}

pub(super) struct ProviderExecutor<'a, 'db> {
    db: &'db dyn HirDb,
    body: Body<'db>,
    reflection: &'a TargetReflection<'db>,
    /// The impl self type with generic args applied (`Pair<A, B>`), exposed
    /// as `builder.target_ty()`.
    target_ty: TypeId<'db>,
    /// The target item's bare name (`Mail`), exposed as
    /// `reflect.target_name()`.
    target_name: IdentId<'db>,
    /// The provider's module, for canonicalizing `require<Trait>` paths.
    provider_top_mod: crate::hir_def::TopLevelMod<'db>,
    /// Lexically scoped value bindings; the innermost binding of a name
    /// shadows outer ones (including the capability params).
    scopes: Vec<Vec<(IdentId<'db>, Value<'db>)>>,

    exprs: Vec<GenExpr<'db>>,
    pats: Vec<GenPat<'db>>,
    tys: Vec<GenTy<'db>>,
    sigs: Vec<GenMethodSig<'db>>,
    quotes: Vec<QuoteTemplate<'db>>,
    commands: Vec<BuilderCommand<'db>>,
    emitted_methods: Vec<IdentId<'db>>,
    emitted_assocs: Vec<IdentId<'db>>,
    finished: bool,

    steps: usize,
    /// Lazily resolved syntax root of the provider's file, for error spans.
    root: Option<parser::SyntaxNode>,
    fallback_range: TextRange,
}

impl<'a, 'db> ProviderExecutor<'a, 'db> {
    pub(super) fn run(
        db: &'db dyn HirDb,
        provider: &ValidatedProvider<'db>,
        reflection: &'a TargetReflection<'db>,
        target_ty: TypeId<'db>,
        target_name: IdentId<'db>,
    ) -> Result<ProviderOutput<'db>, ExecError> {
        let mut initial_scope = Vec::new();
        for &name in &provider.param_names {
            initial_scope.push((name, Value::Evidence));
        }
        for &capability in &provider.capabilities {
            let value = match capability {
                super::provider::Capability::Reflect(_) => Value::Reflect,
                super::provider::Capability::ImplBuilder(_) => Value::Builder,
            };
            initial_scope.push((capability.binding(), value));
        }

        let fallback_range = super::provider::provider_name_range(db, provider.provider);
        let mut executor = ProviderExecutor {
            db,
            body: provider.body,
            reflection,
            target_ty,
            target_name,
            provider_top_mod: provider.provider.top_mod(db),
            scopes: vec![initial_scope],
            exprs: Vec::new(),
            pats: Vec::new(),
            tys: Vec::new(),
            sigs: Vec::new(),
            quotes: Vec::new(),
            commands: Vec::new(),
            emitted_methods: Vec::new(),
            emitted_assocs: Vec::new(),
            finished: false,
            steps: 0,
            root: None,
            fallback_range,
        };

        let root_expr = executor.body.expr(db);
        executor.execute_expr(root_expr)?;
        if !executor.finished {
            return Err(ExecError {
                kind: ProviderFailureKind::MissingFinish,
                range: executor.expr_range(root_expr),
            });
        }
        Ok(ProviderOutput {
            exprs: executor.exprs,
            pats: executor.pats,
            tys: executor.tys,
            sigs: executor.sigs,
            commands: executor.commands,
        })
    }

    // --- spans ----------------------------------------------------------

    fn syntax_root(&mut self) -> parser::SyntaxNode {
        if self.root.is_none() {
            self.root = Some(
                top_mod_ast(self.db, self.body.top_mod(self.db))
                    .syntax()
                    .clone(),
            );
        }
        self.root.clone().unwrap()
    }

    fn expr_range(&mut self, expr: ExprId) -> TextRange {
        let origin = self
            .body
            .source_map(self.db)
            .expr_map
            .node_to_source(expr)
            .clone();
        self.origin_range(origin)
    }

    fn stmt_range(&mut self, stmt: StmtId) -> TextRange {
        let origin = self
            .body
            .source_map(self.db)
            .stmt_map
            .node_to_source(stmt)
            .clone();
        self.origin_range(origin)
    }

    fn origin_range<T>(&mut self, origin: HirOrigin<T>) -> TextRange
    where
        T: parser::ast::prelude::AstNode<Language = parser::FeLang> + Clone + std::hash::Hash + Eq,
    {
        match origin {
            HirOrigin::Raw(ptr) => {
                let root = self.syntax_root();
                ptr.syntax_node_ptr()
                    .try_to_node(&root)
                    .map(|node| node.text_range())
                    .unwrap_or(self.fallback_range)
            }
            _ => self.fallback_range,
        }
    }

    fn unsupported_expr(&mut self, expr: ExprId) -> ExecError {
        ExecError {
            kind: ProviderFailureKind::UnsupportedBody,
            range: self.expr_range(expr),
        }
    }

    fn unsupported_stmt(&mut self, stmt: StmtId) -> ExecError {
        ExecError {
            kind: ProviderFailureKind::UnsupportedBody,
            range: self.stmt_range(stmt),
        }
    }

    // --- budget ---------------------------------------------------------

    fn tick(&mut self, range: TextRange) -> Result<(), ExecError> {
        self.steps += 1;
        if self.steps > STEP_BUDGET || self.commands.len() + self.exprs.len() > COMMAND_BUDGET {
            return Err(ExecError {
                kind: ProviderFailureKind::BudgetExceeded,
                range,
            });
        }
        Ok(())
    }

    // --- environment ----------------------------------------------------

    fn bind(&mut self, name: IdentId<'db>, value: Value<'db>) {
        self.scopes
            .last_mut()
            .expect("executor scope stack is never empty")
            .push((name, value));
    }

    fn lookup(&self, name: IdentId<'db>) -> Option<Value<'db>> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .find_map(|(bound, value)| (*bound == name).then_some(*value))
    }

    fn assign(&mut self, name: IdentId<'db>, value: Value<'db>) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if let Some((_, slot)) = scope.iter_mut().rev().find(|(bound, _)| *bound == name) {
                *slot = value;
                return true;
            }
        }
        false
    }

    // --- execution ------------------------------------------------------

    fn execute_stmt(&mut self, stmt: StmtId) -> Result<Flow, ExecError> {
        let range = self.stmt_range(stmt);
        self.tick(range)?;
        let Partial::Present(stmt_data) = stmt.data(self.db, self.body) else {
            return Ok(Flow::Continue);
        };
        match stmt_data {
            Stmt::Let(pat, _ty, init) => {
                let Some(init) = init else {
                    return Err(self.unsupported_stmt(stmt));
                };
                let value = self.eval_expr(*init)?;
                let Some(name) = self.simple_pat_binding(*pat) else {
                    return Err(self.unsupported_stmt(stmt));
                };
                self.bind(name, value);
                Ok(Flow::Continue)
            }
            Stmt::For(pat, iterable, loop_body, _unroll) => {
                let Some(binding) = self.simple_pat_binding(*pat) else {
                    return Err(self.unsupported_stmt(stmt));
                };
                let iterable_kind = self.eval_iterable(*iterable)?;
                let items: Vec<Value<'db>> = match iterable_kind {
                    Iterable::StructFields => self
                        .reflection
                        .struct_fields()
                        .iter()
                        .map(|field| {
                            Value::Field(FieldKey {
                                variant: field.variant,
                                index: field.index,
                            })
                        })
                        .collect(),
                    Iterable::Variants => self
                        .reflection
                        .variants()
                        .iter()
                        .map(|variant| Value::Variant(variant.index))
                        .collect(),
                    Iterable::VariantFields(variant) => self
                        .reflection
                        .variant(variant)
                        .map(|variant| {
                            variant
                                .fields
                                .iter()
                                .map(|field| {
                                    Value::Field(FieldKey {
                                        variant: field.variant,
                                        index: field.index,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                };
                for item in items {
                    self.scopes.push(vec![(binding, item)]);
                    let flow = self.execute_expr(*loop_body);
                    self.scopes.pop();
                    if matches!(flow?, Flow::Return) {
                        return Ok(Flow::Return);
                    }
                }
                Ok(Flow::Continue)
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    self.eval_expr(*expr)?;
                }
                Ok(Flow::Return)
            }
            Stmt::Expr(expr) => self.execute_expr(*expr),
            Stmt::While(..) | Stmt::Continue | Stmt::Break => Err(self.unsupported_stmt(stmt)),
        }
    }

    /// Executes `expr` for effect, threading control flow. Used for block
    /// and `if` bodies; values are discarded.
    fn execute_expr(&mut self, expr: ExprId) -> Result<Flow, ExecError> {
        let range = self.expr_range(expr);
        self.tick(range)?;
        let Partial::Present(expr_data) = expr.data(self.db, self.body) else {
            return Ok(Flow::Continue);
        };
        match expr_data {
            Expr::Block(stmts) => {
                self.scopes.push(Vec::new());
                let mut flow = Flow::Continue;
                let mut error = None;
                for &stmt in stmts {
                    match self.execute_stmt(stmt) {
                        Ok(Flow::Continue) => {}
                        Ok(Flow::Return) => {
                            flow = Flow::Return;
                            break;
                        }
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                self.scopes.pop();
                match error {
                    Some(err) => Err(err),
                    None => Ok(flow),
                }
            }
            Expr::If(cond, then_expr, else_expr) => {
                if self.eval_cond(*cond)? {
                    self.execute_expr(*then_expr)
                } else if let Some(else_expr) = else_expr {
                    self.execute_expr(*else_expr)
                } else {
                    Ok(Flow::Continue)
                }
            }
            Expr::Assign(lhs, rhs) => {
                let value = self.eval_expr(*rhs)?;
                let Some(name) = self.simple_expr_path_ident(*lhs) else {
                    return Err(self.unsupported_expr(*lhs));
                };
                if !self.assign(name, value) {
                    return Err(self.unsupported_expr(*lhs));
                }
                Ok(Flow::Continue)
            }
            _ => {
                self.eval_expr(expr)?;
                Ok(Flow::Continue)
            }
        }
    }

    fn eval_cond(&mut self, cond: CondId) -> Result<bool, ExecError> {
        let Partial::Present(cond_data) = cond.data(self.db, self.body) else {
            return Err(ExecError {
                kind: ProviderFailureKind::UnsupportedBody,
                range: self.fallback_range,
            });
        };
        match cond_data {
            Cond::Expr(expr) => match self.eval_expr(*expr)? {
                Value::Bool(value) => Ok(value),
                _ => Err(self.unsupported_expr(*expr)),
            },
            Cond::Bin(lhs, rhs, LogicalBinOp::And) => {
                Ok(self.eval_cond(*lhs)? && self.eval_cond(*rhs)?)
            }
            Cond::Bin(lhs, rhs, LogicalBinOp::Or) => {
                Ok(self.eval_cond(*lhs)? || self.eval_cond(*rhs)?)
            }
            Cond::Let(..) => Err(ExecError {
                kind: ProviderFailureKind::UnsupportedBody,
                range: self.fallback_range,
            }),
        }
    }

    fn eval_expr(&mut self, expr: ExprId) -> Result<Value<'db>, ExecError> {
        let range = self.expr_range(expr);
        self.tick(range)?;
        let Partial::Present(expr_data) = expr.data(self.db, self.body) else {
            return Err(self.unsupported_expr(expr));
        };
        match expr_data {
            Expr::Lit(LitKind::Bool(value)) => Ok(Value::Bool(*value)),
            Expr::Lit(LitKind::String(value)) => Ok(Value::Str(*value)),
            Expr::Path(_) => {
                let Some(name) = self.simple_expr_path_ident(expr) else {
                    return Err(self.unsupported_expr(expr));
                };
                self.lookup(name).ok_or_else(|| self.unsupported_expr(expr))
            }
            Expr::MethodCall(receiver, method, generic_args, args) => {
                self.eval_method_call(expr, *receiver, *method, *generic_args, args.clone())
            }
            Expr::Un(inner, crate::hir_def::UnOp::Not) => match self.eval_expr(*inner)? {
                Value::Bool(value) => Ok(Value::Bool(!value)),
                _ => Err(self.unsupported_expr(expr)),
            },
            Expr::Quote { open, body } => {
                let open = open.clone();
                let body = body.clone();
                let mut holes = Vec::new();
                match &body {
                    QuoteBody::Expr(root) => self.capture_quote_holes(*root, &mut holes)?,
                    QuoteBody::Arms(arms) => {
                        let arms = arms.clone();
                        self.capture_arm_holes(&arms, &mut holes)?;
                    }
                }
                self.quotes.push(QuoteTemplate {
                    origin: expr,
                    open,
                    body,
                    holes,
                });
                Ok(Value::Quote(QuoteId(self.quotes.len() - 1)))
            }
            Expr::QuoteHole(..) | Expr::QuoteFieldHole(..) => Err(self.invalid_quote(
                expr,
                "`${...}` splice holes are only meaningful inside a `quote` body",
            )),
            _ => Err(self.unsupported_expr(expr)),
        }
    }

    /// Walks a quote template at quote-construction time, evaluating every
    /// hole's inner expression in the current environment and recording the
    /// captured values. Only the v1 template vocabulary is walked; constructs
    /// outside it cannot carry live holes because elaboration rejects them
    /// before any hole beneath them is reached.
    fn capture_quote_holes(
        &mut self,
        expr: ExprId,
        holes: &mut Vec<(ExprId, Value<'db>)>,
    ) -> Result<(), ExecError> {
        let range = self.expr_range(expr);
        self.tick(range)?;
        let Partial::Present(data) = expr.data(self.db, self.body) else {
            return Ok(());
        };
        match data {
            Expr::QuoteHole(inner) => {
                let value = self.eval_expr(*inner)?;
                holes.push((expr, value));
                Ok(())
            }
            Expr::QuoteFieldHole(base, inner) => {
                let (base, inner) = (*base, *inner);
                self.capture_quote_holes(base, holes)?;
                let value = self.eval_expr(inner)?;
                holes.push((expr, value));
                Ok(())
            }
            Expr::Quote { .. } => Err(self.invalid_quote(
                expr,
                "`quote` inside a quote body is not supported; build the inner \
                 fragment in a separate `let` and splice it with `${...}`",
            )),
            Expr::Block(stmts) => {
                let stmts = stmts.clone();
                for stmt in stmts {
                    if let Partial::Present(Stmt::Expr(stmt_expr)) = stmt.data(self.db, self.body) {
                        self.capture_quote_holes(*stmt_expr, holes)?;
                    }
                }
                Ok(())
            }
            Expr::Bin(lhs, rhs, _) => {
                let (lhs, rhs) = (*lhs, *rhs);
                self.capture_quote_holes(lhs, holes)?;
                self.capture_quote_holes(rhs, holes)
            }
            Expr::MethodCall(receiver, _, _, args) => {
                let receiver = *receiver;
                let args: Vec<ExprId> = args.iter().map(|arg| arg.expr).collect();
                self.capture_quote_holes(receiver, holes)?;
                for arg in args {
                    self.capture_quote_holes(arg, holes)?;
                }
                Ok(())
            }
            Expr::Match(scrutinee, arms) => {
                let scrutinee = *scrutinee;
                let arms = match arms {
                    Partial::Present(arms) => arms.clone(),
                    Partial::Absent => Vec::new(),
                };
                self.capture_quote_holes(scrutinee, holes)?;
                self.capture_arm_holes(&arms, holes)
            }
            // Leaves carry no holes; the remaining constructs are outside
            // the template vocabulary and are rejected at elaboration.
            Expr::Lit(..)
            | Expr::Path(..)
            | Expr::Un(..)
            | Expr::Cast(..)
            | Expr::Call(..)
            | Expr::Assert(..)
            | Expr::RecordInit(..)
            | Expr::Field(..)
            | Expr::Tuple(..)
            | Expr::Array(..)
            | Expr::ArrayRep(..)
            | Expr::If(..)
            | Expr::Assign(..)
            | Expr::AugAssign(..)
            | Expr::With(..) => Ok(()),
        }
    }

    /// Captures hole values from a match-arm sequence: pattern holes in arm
    /// patterns (keyed by the hole's inner expression) and expression holes
    /// in arm bodies. Arm splices (`${arms}` standing alone) have an absent
    /// pattern and their hole as the arm body, so the body walk covers them.
    fn capture_arm_holes(
        &mut self,
        arms: &[MatchArm],
        holes: &mut Vec<(ExprId, Value<'db>)>,
    ) -> Result<(), ExecError> {
        for arm in arms {
            self.capture_pat_holes(arm.pat, holes)?;
            self.capture_quote_holes(arm.body, holes)?;
        }
        Ok(())
    }

    fn capture_pat_holes(
        &mut self,
        pat: PatId,
        holes: &mut Vec<(ExprId, Value<'db>)>,
    ) -> Result<(), ExecError> {
        let Partial::Present(data) = pat.data(self.db, self.body) else {
            return Ok(());
        };
        match data {
            Pat::QuoteHole(inner, _binders) => {
                let inner = *inner;
                let value = self.eval_expr(inner)?;
                holes.push((inner, value));
                Ok(())
            }
            // Other patterns carry no live holes; the ones outside the
            // template vocabulary are rejected at elaboration.
            Pat::WildCard
            | Pat::Rest
            | Pat::Lit(..)
            | Pat::Tuple(..)
            | Pat::Path(..)
            | Pat::PathTuple(..)
            | Pat::Record(..)
            | Pat::Or(..) => Ok(()),
        }
    }

    /// Validates a template's declared open names against the destination:
    /// the emitted method's parameter names, plus the binder groups of the
    /// match arms enclosing the splice point.
    fn validate_open_names(
        &mut self,
        template: &QuoteTemplate<'db>,
        sig: SigId,
        binders: &[BinderGroup<'db>],
    ) -> Result<(), ExecError> {
        let sig_data = &self.sigs[sig.0];
        let method_name = sig_data.name;
        let takes_self = sig_data.takes_self;
        let params: Vec<IdentId<'db>> = sig_data.args.iter().map(|(name, _)| *name).collect();
        for open in &template.open {
            if params.contains(open) || binders.iter().any(|(group, _)| group == open) {
                continue;
            }
            let mut binds = Vec::new();
            if takes_self {
                binds.push("`self`".to_string());
            }
            binds.extend(
                params
                    .iter()
                    .map(|param| format!("`{}`", param.data(self.db))),
            );
            let binds = if binds.is_empty() {
                "no names".to_string()
            } else {
                binds.join(", ")
            };
            let detail = if binders.is_empty() {
                format!(
                    "the quote declares open name `{}`, but the emitted method `{}` binds {}; \
                     open names bind against the destination method's parameter names",
                    open.data(self.db),
                    method_name.data(self.db),
                    binds,
                )
            } else {
                let groups = binders
                    .iter()
                    .map(|(group, _)| format!("`{}`", group.data(self.db)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "the quote declares open name `{}`, but the emitted method `{}` binds {} \
                     and the enclosing match arms bind {}; open names bind against the \
                     destination method's parameters or enclosing arm binder groups",
                    open.data(self.db),
                    method_name.data(self.db),
                    binds,
                    groups,
                )
            };
            return Err(self.invalid_quote(template.origin, &detail));
        }
        Ok(())
    }

    /// Elaborates a quote template into a generated expression for emission
    /// under `sig`. Open names (the quote's own and those of every spliced
    /// quote) must match the destination signature's parameter names or the
    /// binder groups of enclosing match arms.
    fn elaborate_quote(
        &mut self,
        quote: QuoteId,
        sig: SigId,
        binders: &[BinderGroup<'db>],
    ) -> Result<GenExprId, ExecError> {
        let template = self.quotes[quote.0].clone();
        self.validate_open_names(&template, sig, binders)?;

        // Expression quotes: the body block must hold exactly one
        // expression.
        let block = match &template.body {
            QuoteBody::Expr(block) => *block,
            QuoteBody::Arms(_) => {
                return Err(self.invalid_quote(
                    template.origin,
                    "the quote holds match arms; expression positions need an expression \
                     template (wrap the arms in a `match`)",
                ));
            }
        };
        let Partial::Present(Expr::Block(stmts)) = block.data(self.db, self.body) else {
            return Err(self.invalid_quote(template.origin, "malformed quote body"));
        };
        let root = match stmts.as_slice() {
            [stmt] => match stmt.data(self.db, self.body) {
                Partial::Present(Stmt::Expr(root)) => *root,
                _ => {
                    return Err(self.invalid_quote(
                        template.origin,
                        "quote bodies must be a single expression",
                    ));
                }
            },
            [] => {
                return Err(self.invalid_quote(
                    template.origin,
                    "the quote is empty; an empty quote only splices in match-arm position",
                ));
            }
            _ => {
                return Err(
                    self.invalid_quote(template.origin, "quote bodies must be a single expression")
                );
            }
        };
        self.elab_template_expr(root, &template, sig, binders)
    }

    /// Elaborates a quote spliced in match-arm position, appending its arms
    /// to `out`. The spliced quote must be a match-arm template
    /// (`quote { pat => expr, .. }`) or the empty quote (`quote { }`).
    fn elaborate_quote_arms(
        &mut self,
        quote: QuoteId,
        sig: SigId,
        binders: &[BinderGroup<'db>],
        out: &mut Vec<(GenPatId, GenExprId)>,
    ) -> Result<(), ExecError> {
        let template = self.quotes[quote.0].clone();
        self.validate_open_names(&template, sig, binders)?;
        match &template.body {
            QuoteBody::Arms(arms) => {
                let arms = arms.clone();
                self.elab_arm_items(&arms, &template, sig, binders, out)
            }
            QuoteBody::Expr(block) => {
                // The empty quote is the empty arm sequence — the natural
                // seed for arm folds.
                if let Partial::Present(Expr::Block(stmts)) = block.data(self.db, self.body)
                    && stmts.is_empty()
                {
                    return Ok(());
                }
                Err(self.invalid_quote(
                    template.origin,
                    "the quote holds an expression template; arm splices need match arms \
                     (`pat => expr` items) or an empty `quote { }`",
                ))
            }
        }
    }

    /// Elaborates a match-arm sequence (the arms of a template `match` or
    /// the items of a match-arm template) into generated (pattern, body)
    /// pairs.
    fn elab_arm_items(
        &mut self,
        arms: &[MatchArm],
        template: &QuoteTemplate<'db>,
        sig: SigId,
        binders: &[BinderGroup<'db>],
        out: &mut Vec<(GenPatId, GenExprId)>,
    ) -> Result<(), ExecError> {
        for arm in arms {
            let range = self.expr_range(arm.body);
            self.tick(range)?;
            match arm.pat.data(self.db, self.body) {
                // An arm splice: `${arms}` standing alone has no pattern
                // and carries its hole as the arm body.
                Partial::Absent => {
                    let Partial::Present(Expr::QuoteHole(_)) = arm.body.data(self.db, self.body)
                    else {
                        return Err(
                            self.invalid_quote(arm.body, "malformed match arm in quote body")
                        );
                    };
                    let value = self.quote_hole_value(arm.body, template)?;
                    let Value::Quote(inner) = value else {
                        let detail = format!(
                            "a {} cannot fill an arm splice; arm splices accept quote values \
                             holding match arms",
                            value_kind_name(&value),
                        );
                        return Err(self.invalid_quote(arm.body, &detail));
                    };
                    self.elaborate_quote_arms(inner, sig, binders, out)?;
                }
                Partial::Present(Pat::WildCard) => {
                    let body = self.elab_template_expr(arm.body, template, sig, binders)?;
                    let pat = self.push_gen_pat(GenPat::Wildcard);
                    out.push((pat, body));
                }
                Partial::Present(Pat::QuoteHole(inner, groups)) => {
                    let (inner, groups) = (*inner, groups.clone());
                    let value = self.quote_hole_value(inner, template)?;
                    let Value::Variant(variant) = value else {
                        let detail = format!(
                            "a {} cannot fill a pattern hole; pattern holes accept `Variant` \
                             handles",
                            value_kind_name(&value),
                        );
                        return Err(self.invalid_quote(inner, &detail));
                    };
                    let group = self.binder_group_name(inner, &groups)?;
                    let pat = self.push_gen_pat(GenPat::Variant {
                        variant,
                        prefix: group,
                    });
                    let mut arm_binders = binders.to_vec();
                    arm_binders.push((group, variant));
                    let body = self.elab_template_expr(arm.body, template, sig, &arm_binders)?;
                    out.push((pat, body));
                }
                Partial::Present(_) => {
                    return Err(self.invalid_quote(
                        arm.body,
                        "this pattern is not supported in quote match arms (arms take \
                         `${variant}(group)` pattern holes and `_`)",
                    ));
                }
            }
        }
        Ok(())
    }

    /// The binder-group name of a `${variant}(group)` pattern hole.
    fn binder_group_name(
        &mut self,
        hole_expr: ExprId,
        groups: &[PatId],
    ) -> Result<IdentId<'db>, ExecError> {
        let group = match groups {
            [group] => self.simple_pat_binding(*group),
            _ => None,
        };
        let Some(name) = group else {
            return Err(self.invalid_quote(
                hole_expr,
                "variant pattern holes bind their payload under a single group name: \
                 `${variant}(group)`",
            ));
        };
        if name.is_self(self.db) {
            return Err(self.invalid_quote(hole_expr, "a binder group cannot be named `self`"));
        }
        Ok(name)
    }

    fn elab_template_expr(
        &mut self,
        expr: ExprId,
        template: &QuoteTemplate<'db>,
        sig: SigId,
        binders: &[BinderGroup<'db>],
    ) -> Result<GenExprId, ExecError> {
        let range = self.expr_range(expr);
        self.tick(range)?;
        let Partial::Present(data) = expr.data(self.db, self.body) else {
            return Err(self.invalid_quote(expr, "malformed quote body"));
        };
        match data {
            Expr::Lit(LitKind::Bool(value)) => Ok(self.push_gen(GenExpr::Bool(*value))),
            Expr::Lit(LitKind::String(value)) => {
                let value = *value;
                self.check_inline_capacity(expr, value)?;
                Ok(self.push_gen(GenExpr::StrLit(value)))
            }
            Expr::Lit(LitKind::Int(_)) => {
                Err(self.invalid_quote(expr, "integer literals are not supported in quote bodies"))
            }
            Expr::Bin(lhs, rhs, BinOp::Logical(LogicalBinOp::And)) => {
                let (lhs, rhs) = (*lhs, *rhs);
                let lhs = self.elab_template_expr(lhs, template, sig, binders)?;
                let rhs = self.elab_template_expr(rhs, template, sig, binders)?;
                Ok(self.push_gen(GenExpr::And(lhs, rhs)))
            }
            Expr::Bin(lhs, rhs, BinOp::Logical(LogicalBinOp::Or)) => {
                let (lhs, rhs) = (*lhs, *rhs);
                let lhs = self.elab_template_expr(lhs, template, sig, binders)?;
                let rhs = self.elab_template_expr(rhs, template, sig, binders)?;
                Ok(self.push_gen(GenExpr::Or(lhs, rhs)))
            }
            Expr::Bin(lhs, rhs, BinOp::Comp(CompBinOp::Eq)) => {
                let (lhs, rhs) = (*lhs, *rhs);
                let lhs = self.elab_template_expr(lhs, template, sig, binders)?;
                let rhs = self.elab_template_expr(rhs, template, sig, binders)?;
                Ok(self.push_gen(GenExpr::EqCmp(lhs, rhs)))
            }
            Expr::Bin(lhs, rhs, BinOp::Comp(CompBinOp::Lt)) => {
                let (lhs, rhs) = (*lhs, *rhs);
                let lhs = self.elab_template_expr(lhs, template, sig, binders)?;
                let rhs = self.elab_template_expr(rhs, template, sig, binders)?;
                Ok(self.push_gen(GenExpr::LtCmp(lhs, rhs)))
            }
            Expr::Bin(lhs, rhs, BinOp::Comp(CompBinOp::Gt)) => {
                let (lhs, rhs) = (*lhs, *rhs);
                let lhs = self.elab_template_expr(lhs, template, sig, binders)?;
                let rhs = self.elab_template_expr(rhs, template, sig, binders)?;
                Ok(self.push_gen(GenExpr::GtCmp(lhs, rhs)))
            }
            Expr::Bin(..) => Err(self.invalid_quote(
                expr,
                "this operator is not supported in quote bodies (quotes support `&&`, `||`, \
                 `==`, `<`, `>`, and method calls)",
            )),
            Expr::Path(_) => {
                let Some(name) = self.simple_expr_path_ident(expr) else {
                    return Err(
                        self.invalid_quote(expr, "paths in quote bodies must be a single name")
                    );
                };
                self.elab_template_name(expr, name, template, sig, binders)
            }
            Expr::QuoteHole(_) => {
                let value = self.quote_hole_value(expr, template)?;
                match value {
                    Value::Quote(inner) => self.elaborate_quote(inner, sig, binders),
                    Value::Bool(value) => Ok(self.push_gen(GenExpr::Bool(value))),
                    Value::Str(value) => {
                        self.check_inline_capacity(expr, value)?;
                        Ok(self.push_gen(GenExpr::StrLit(value)))
                    }
                    Value::Field(_) => Err(self.invalid_quote(
                        expr,
                        "a `Field` handle only fills member-access holes (`self.${field}`); \
                         expression holes accept quote values and compile-time bool/string \
                         values",
                    )),
                    other => {
                        let detail = format!(
                            "a {} cannot fill an expression hole; expression holes accept \
                             quote values and compile-time bool/string values",
                            value_kind_name(&other),
                        );
                        Err(self.invalid_quote(expr, &detail))
                    }
                }
            }
            Expr::QuoteFieldHole(base, _) => {
                let base = *base;
                let value = self.quote_hole_value(expr, template)?;
                // `group.${field}` — a payload-binder reference when the
                // base is an open name bound by an enclosing arm pattern.
                if let Some(name) = self.simple_expr_path_ident(base)
                    && !name.is_self(self.db)
                    && let Some(variant) = binders
                        .iter()
                        .rev()
                        .find_map(|(group, variant)| (*group == name).then_some(*variant))
                {
                    if !template.open.contains(&name) {
                        let detail = format!(
                            "`{}` matches a binder group of an enclosing match arm but is \
                             not declared open; declare it with `quote({}) {{ .. }}`",
                            name.data(self.db),
                            name.data(self.db),
                        );
                        return Err(self.invalid_quote(expr, &detail));
                    }
                    let Value::Field(field) = value else {
                        let detail = format!(
                            "member-access holes (`base.${{...}}`) accept `Field` handles, \
                             found a {}",
                            value_kind_name(&value),
                        );
                        return Err(self.invalid_quote(expr, &detail));
                    };
                    if field.variant != Some(variant) {
                        let detail = format!(
                            "the field does not belong to the variant matched by `{}`",
                            name.data(self.db),
                        );
                        return Err(self.invalid_quote(expr, &detail));
                    }
                    return Ok(self.push_gen(GenExpr::VariantBinder {
                        variant,
                        field: field.index,
                        prefix: name,
                    }));
                }
                let Value::Field(field) = value else {
                    let detail = format!(
                        "member-access holes (`base.${{...}}`) accept `Field` handles, \
                         found a {}",
                        value_kind_name(&value),
                    );
                    return Err(self.invalid_quote(expr, &detail));
                };
                let base = self.elab_template_expr(base, template, sig, binders)?;
                Ok(self.push_gen(GenExpr::FieldGet(base, field)))
            }
            Expr::MethodCall(receiver, method, generic_args, args) => {
                let Some(method) = method.to_opt() else {
                    return Err(self.invalid_quote(expr, "malformed method call in quote body"));
                };
                if !generic_args.data(self.db).is_empty() {
                    return Err(self.invalid_quote(
                        expr,
                        "generic method calls are not supported in quote bodies",
                    ));
                }
                let receiver = *receiver;
                let arg_exprs: Vec<ExprId> = args.iter().map(|arg| arg.expr).collect();
                let receiver = self.elab_template_expr(receiver, template, sig, binders)?;
                let mut call_args = Vec::with_capacity(arg_exprs.len());
                for arg in arg_exprs {
                    call_args.push(self.elab_template_expr(arg, template, sig, binders)?);
                }
                Ok(self.push_gen(GenExpr::MethodCall {
                    receiver,
                    method,
                    args: call_args,
                }))
            }
            Expr::Match(scrutinee, arms) => {
                let scrutinee = *scrutinee;
                let arms = match arms {
                    Partial::Present(arms) => arms.clone(),
                    Partial::Absent => {
                        return Err(self.invalid_quote(expr, "malformed quote body"));
                    }
                };
                self.elab_template_match(expr, scrutinee, &arms, template, sig, binders)
            }
            Expr::Field(..) => Err(self.invalid_quote(
                expr,
                "field access in quote bodies goes through a member-access hole \
                 (`self.${field}`)",
            )),
            Expr::Quote { .. } => Err(self.invalid_quote(
                expr,
                "`quote` inside a quote body is not supported; build the inner \
                 fragment in a separate `let` and splice it with `${...}`",
            )),
            Expr::Block(..)
            | Expr::Un(..)
            | Expr::Cast(..)
            | Expr::Call(..)
            | Expr::Assert(..)
            | Expr::RecordInit(..)
            | Expr::Tuple(..)
            | Expr::Array(..)
            | Expr::ArrayRep(..)
            | Expr::If(..)
            | Expr::Assign(..)
            | Expr::AugAssign(..)
            | Expr::With(..) => Err(self.invalid_quote(
                expr,
                "this construct is not supported in quote bodies (quotes support literals, \
                 `&&`, `==`, `self`, declared open names, method calls, `match`, and \
                 `${...}` holes)",
            )),
        }
    }

    /// Elaborates a `match` inside a quote body. Variant arm patterns name
    /// the derive target's variants, so the scrutinee must be the target
    /// value (`self` or a target-typed parameter), and the arms must cover
    /// every reflected variant unless a `_` arm is present.
    fn elab_template_match(
        &mut self,
        expr: ExprId,
        scrutinee: ExprId,
        arms: &[MatchArm],
        template: &QuoteTemplate<'db>,
        sig: SigId,
        binders: &[BinderGroup<'db>],
    ) -> Result<GenExprId, ExecError> {
        let scrutinee_gen = self.elab_template_expr(scrutinee, template, sig, binders)?;
        let is_target_value = match &self.exprs[scrutinee_gen.0] {
            GenExpr::SelfRef => true,
            GenExpr::ArgRef(name) => {
                let name = *name;
                self.sigs[sig.0].args.iter().any(|(arg, ty)| {
                    *arg == name
                        && (*ty == self.target_ty || *ty == TypeId::fallback_self_ty(self.db))
                })
            }
            _ => false,
        };
        if !is_target_value {
            return Err(self.invalid_quote(
                scrutinee,
                "match scrutinees in quote bodies must be `self` or a parameter of the \
                 derive target's type (variant patterns name the target's variants)",
            ));
        }

        let mut gen_arms = Vec::new();
        self.elab_arm_items(arms, template, sig, binders, &mut gen_arms)?;

        // Exhaustiveness over the target's variants, checked at the
        // template so the failure names the provider's match rather than
        // surfacing later from the generated code.
        let has_wildcard = gen_arms
            .iter()
            .any(|(pat, _)| matches!(self.pats[pat.0], GenPat::Wildcard));
        if !has_wildcard {
            let mut uncovered = None;
            for variant in self.reflection.variants() {
                let covered = gen_arms.iter().any(|(pat, _)| {
                    matches!(
                        &self.pats[pat.0],
                        GenPat::Variant { variant: v, .. } if *v == variant.index
                    )
                });
                if !covered {
                    uncovered = Some(variant.name);
                    break;
                }
            }
            if let Some(name) = uncovered {
                let detail = format!(
                    "the template match does not cover variant `{}`; add a \
                     `${{variant}}(group)` arm for it or a `_` arm",
                    name.data(self.db),
                );
                return Err(self.invalid_quote(expr, &detail));
            }
        }

        Ok(self.push_gen(GenExpr::Match {
            scrutinee: scrutinee_gen,
            arms: gen_arms,
        }))
    }

    /// Resolves a bare name in a quote template: `self`, or a declared open
    /// name bound at the destination — by the innermost enclosing arm's
    /// binder group of that name, or by the emitted method's parameters.
    fn elab_template_name(
        &mut self,
        expr: ExprId,
        name: IdentId<'db>,
        template: &QuoteTemplate<'db>,
        sig: SigId,
        binders: &[BinderGroup<'db>],
    ) -> Result<GenExprId, ExecError> {
        let sig_data = &self.sigs[sig.0];
        let method_name = sig_data.name;
        let takes_self = sig_data.takes_self;
        let is_param = sig_data.args.iter().any(|(arg, _)| *arg == name);
        let is_group = binders.iter().any(|(group, _)| *group == name);
        if name.is_self(self.db) {
            if !takes_self {
                let detail = format!(
                    "the quote uses `self`, but the emitted method `{}` does not bind `self`",
                    method_name.data(self.db),
                );
                return Err(self.invalid_quote(expr, &detail));
            }
            return Ok(self.push_gen(GenExpr::SelfRef));
        }
        if template.open.contains(&name) {
            // Binder groups shadow method parameters; a bare group never
            // names a value — its payload is reached field by field.
            if is_group {
                let detail = format!(
                    "`{}` is bound by an enclosing `${{variant}}({})` arm pattern as a \
                     binder group; payload fields are reached with `{}.${{field}}`",
                    name.data(self.db),
                    name.data(self.db),
                    name.data(self.db),
                );
                return Err(self.invalid_quote(expr, &detail));
            }
            // Validated against the destination signature when elaboration
            // started.
            return Ok(self.push_gen(GenExpr::ArgRef(name)));
        }
        let detail = if is_param {
            format!(
                "`{}` matches a parameter of the emitted method but is not declared open; \
                 declare it with `quote({}) {{ .. }}`",
                name.data(self.db),
                name.data(self.db),
            )
        } else if is_group {
            format!(
                "`{}` matches a binder group of an enclosing match arm but is not declared \
                 open; declare it with `quote({}) {{ .. }}`",
                name.data(self.db),
                name.data(self.db),
            )
        } else {
            format!(
                "cannot resolve `{}` in a quote body; quotes support `self`, declared \
                 open names, and `${{...}}` holes",
                name.data(self.db),
            )
        };
        Err(self.invalid_quote(expr, &detail))
    }

    /// The value captured for a hole expression when its quote was built.
    fn quote_hole_value(
        &mut self,
        expr: ExprId,
        template: &QuoteTemplate<'db>,
    ) -> Result<Value<'db>, ExecError> {
        match template
            .holes
            .iter()
            .find_map(|(hole, value)| (*hole == expr).then_some(*value))
        {
            Some(value) => Ok(value),
            None => Err(self.invalid_quote(
                expr,
                "splice hole was not captured when the quote was built",
            )),
        }
    }

    fn eval_method_call(
        &mut self,
        expr: ExprId,
        receiver: ExprId,
        method: Partial<IdentId<'db>>,
        generic_args: GenericArgListId<'db>,
        args: Vec<crate::hir_def::expr::CallArg<'db>>,
    ) -> Result<Value<'db>, ExecError> {
        let Some(method) = method.to_opt() else {
            return Err(self.unsupported_expr(expr));
        };
        let receiver_value = self.eval_expr(receiver)?;
        let method_name = method.data(self.db).clone();
        match receiver_value {
            Value::Builder => self.eval_builder_method(expr, &method_name, generic_args, &args),
            Value::Reflect => match (method_name.as_str(), args.as_slice()) {
                ("is_struct", []) => Ok(Value::Bool(self.reflection.is_struct())),
                ("is_enum", []) => Ok(Value::Bool(self.reflection.is_enum())),
                ("target_name", []) => Ok(Value::Str(StringId::new(
                    self.db,
                    self.target_name.data(self.db).clone(),
                ))),
                // `fields()` / `variants()` are only meaningful as `for`
                // iterables, which are intercepted before evaluation.
                _ => Err(self.unsupported_expr(expr)),
            },
            Value::Field(field) => match (method_name.as_str(), args.as_slice()) {
                ("ty", []) => {
                    let Some(reflected) = self.reflection.field(field.variant, field.index) else {
                        return Err(self.unsupported_expr(expr));
                    };
                    Ok(Value::Ty(reflected.ty))
                }
                ("name", []) => {
                    let Some(reflected) = self.reflection.field(field.variant, field.index) else {
                        return Err(self.unsupported_expr(expr));
                    };
                    let text = match reflected.name {
                        super::provider::FieldName::Named(name) => name.data(self.db).clone(),
                        super::provider::FieldName::Positional(idx) => idx.to_string(),
                    };
                    Ok(Value::Str(StringId::new(self.db, text)))
                }
                _ => Err(self.unsupported_expr(expr)),
            },
            Value::Variant(variant) => match (method_name.as_str(), args.as_slice()) {
                ("is_default", []) => {
                    let Some(reflected) = self.reflection.variant(variant) else {
                        return Err(self.unsupported_expr(expr));
                    };
                    Ok(Value::Bool(reflected.is_default))
                }
                ("precedes", [other]) => {
                    let Value::Variant(other) = self.eval_expr(other.expr)? else {
                        return Err(self.unsupported_expr(other.expr));
                    };
                    // Declaration-order precedence: a variant handle is its index
                    // into the target's variant list (declaration order), so `<`
                    // means "declared earlier". This is the minimal reflection
                    // primitive an enum `Ord` derive needs to order variants — the
                    // variant index is otherwise sealed inside the opaque handle
                    // (there is no integer value in the provider command language).
                    Ok(Value::Bool(variant < other))
                }
                // `fields()` is only meaningful as a `for` iterable.
                _ => Err(self.unsupported_expr(expr)),
            },
            _ => Err(self.unsupported_expr(expr)),
        }
    }

    fn eval_builder_method(
        &mut self,
        expr: ExprId,
        method: &str,
        generic_args: GenericArgListId<'db>,
        args: &[crate::hir_def::expr::CallArg<'db>],
    ) -> Result<Value<'db>, ExecError> {
        // Commands check the finish flag; pure expression builders do not.
        match (method, args) {
            // --- commands ---------------------------------------------
            ("require", [arg]) => {
                self.check_not_finished(expr)?;
                let Some(trait_path) = self.single_type_generic_arg_path(generic_args) else {
                    return Err(ExecError {
                        kind: ProviderFailureKind::InvalidRequirement,
                        range: self.expr_range(expr),
                    });
                };
                let Value::Ty(ty) = self.eval_expr(arg.expr)? else {
                    return Err(ExecError {
                        kind: ProviderFailureKind::InvalidRequirement,
                        range: self.expr_range(arg.expr),
                    });
                };
                let trait_path = canonical_trait_path(self.db, self.provider_top_mod, trait_path);
                self.commands
                    .push(BuilderCommand::Require { ty, trait_path });
                Ok(Value::Unit)
            }
            ("emit_method", [sig_arg, body_arg]) => {
                self.check_not_finished(expr)?;
                let Value::Sig(sig) = self.eval_expr(sig_arg.expr)? else {
                    return Err(self.invalid_method(sig_arg.expr, "expected a method signature"));
                };
                let body = match self.eval_expr(body_arg.expr)? {
                    Value::Expr(body) => body,
                    // Quotes land here: the template elaborates into the
                    // same generated-expression layer the explicit builder
                    // calls produce.
                    Value::Quote(quote) => self.elaborate_quote(quote, sig, &[])?,
                    _ => {
                        return Err(self.invalid_method(
                            body_arg.expr,
                            "expected a generated expression or a quote",
                        ));
                    }
                };
                let name = self.sigs[sig.0].name;
                if self.emitted_methods.contains(&name) {
                    return Err(self.invalid_method(
                        sig_arg.expr,
                        &format!("duplicate generated method `{}`", name.data(self.db)),
                    ));
                }
                self.emitted_methods.push(name);
                self.commands.push(BuilderCommand::EmitMethod { sig, body });
                Ok(Value::Unit)
            }
            ("emit_assoc_ty", [name_arg, ty_arg]) => {
                self.check_not_finished(expr)?;
                let name = self.string_value_ident(name_arg.expr)?;
                let ty = self.gen_ty_arg(ty_arg.expr)?;
                self.check_fresh_assoc(name_arg.expr, name)?;
                self.commands.push(BuilderCommand::EmitAssocTy { name, ty });
                Ok(Value::Unit)
            }
            ("emit_const", [name_arg, ty_arg, value_arg]) => {
                self.check_not_finished(expr)?;
                let name = self.string_value_ident(name_arg.expr)?;
                let ty = self.gen_ty_arg(ty_arg.expr)?;
                let Value::Expr(value) = self.eval_expr(value_arg.expr)? else {
                    return Err(
                        self.invalid_assoc(value_arg.expr, "expected a generated expression")
                    );
                };
                self.check_fresh_assoc(name_arg.expr, name)?;
                self.commands
                    .push(BuilderCommand::EmitConst { name, ty, value });
                Ok(Value::Unit)
            }
            ("finish", []) => {
                if self.finished {
                    return Err(ExecError {
                        kind: ProviderFailureKind::DuplicateFinish,
                        range: self.expr_range(expr),
                    });
                }
                self.finished = true;
                Ok(Value::Unit)
            }

            // --- expression builders -----------------------------------
            ("bool", [arg]) => {
                let Value::Bool(value) = self.eval_expr(arg.expr)? else {
                    return Err(self.unsupported_expr(arg.expr));
                };
                Ok(self.push_expr(GenExpr::Bool(value)))
            }
            ("and", [lhs, rhs]) => {
                let lhs = self.gen_expr_arg(lhs.expr)?;
                let rhs = self.gen_expr_arg(rhs.expr)?;
                Ok(self.push_expr(GenExpr::And(lhs, rhs)))
            }
            ("add", [lhs, rhs]) => {
                let lhs = self.gen_expr_arg(lhs.expr)?;
                let rhs = self.gen_expr_arg(rhs.expr)?;
                Ok(self.push_expr(GenExpr::Add(lhs, rhs)))
            }
            ("or", [lhs, rhs]) => {
                let lhs = self.gen_expr_arg(lhs.expr)?;
                let rhs = self.gen_expr_arg(rhs.expr)?;
                Ok(self.push_expr(GenExpr::Or(lhs, rhs)))
            }
            ("self_ref", []) => Ok(self.push_expr(GenExpr::SelfRef)),
            ("arg_ref", [arg]) => {
                let name = self.string_value_ident(arg.expr)?;
                Ok(self.push_expr(GenExpr::ArgRef(name)))
            }
            ("field_get", [base, field]) => {
                let base = self.gen_expr_arg(base.expr)?;
                let Value::Field(field) = self.eval_expr(field.expr)? else {
                    return Err(self.unsupported_expr(expr));
                };
                Ok(self.push_expr(GenExpr::FieldGet(base, field)))
            }
            ("eq", [lhs, rhs]) => {
                let lhs = self.gen_expr_arg(lhs.expr)?;
                let rhs = self.gen_expr_arg(rhs.expr)?;
                Ok(self.push_expr(GenExpr::EqCmp(lhs, rhs)))
            }
            ("lt", [lhs, rhs]) => {
                let lhs = self.gen_expr_arg(lhs.expr)?;
                let rhs = self.gen_expr_arg(rhs.expr)?;
                Ok(self.push_expr(GenExpr::LtCmp(lhs, rhs)))
            }
            ("gt", [lhs, rhs]) => {
                let lhs = self.gen_expr_arg(lhs.expr)?;
                let rhs = self.gen_expr_arg(rhs.expr)?;
                Ok(self.push_expr(GenExpr::GtCmp(lhs, rhs)))
            }
            ("trait_call", [ty_arg, method_arg, extra @ ..]) => {
                let Value::Ty(ty) = self.eval_expr(ty_arg.expr)? else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                let method = self.string_value_ident(method_arg.expr)?;
                let mut call_args = Vec::with_capacity(extra.len());
                for arg in extra {
                    call_args.push(self.gen_expr_arg(arg.expr)?);
                }
                Ok(self.push_expr(GenExpr::TraitCall {
                    ty,
                    method,
                    args: call_args,
                }))
            }
            ("trait_const", [ty_arg, name_arg]) => {
                let Value::Ty(ty) = self.eval_expr(ty_arg.expr)? else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                let name = self.string_value_ident(name_arg.expr)?;
                Ok(self.push_expr(GenExpr::TraitConst { ty, name }))
            }
            ("call", [receiver_arg, method_arg, extra @ ..]) => {
                let receiver = self.gen_expr_arg(receiver_arg.expr)?;
                let method = self.string_value_ident(method_arg.expr)?;
                let mut call_args = Vec::with_capacity(extra.len());
                for arg in extra {
                    call_args.push(self.gen_expr_arg(arg.expr)?);
                }
                Ok(self.push_expr(GenExpr::MethodCall {
                    receiver,
                    method,
                    args: call_args,
                }))
            }
            ("static_call", [ty_arg, method_arg, extra @ ..]) => {
                let Value::Ty(ty) = self.eval_expr(ty_arg.expr)? else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                // The callee path is the type as written with the function
                // name appended, so only path types can be call targets.
                let TypeKind::Path(Partial::Present(ty_path)) = ty.data(self.db) else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                let method = self.string_value_ident(method_arg.expr)?;
                let path = ty_path.push_ident(self.db, method);
                let mut call_args = Vec::with_capacity(extra.len());
                for arg in extra {
                    call_args.push(self.gen_expr_arg(arg.expr)?);
                }
                Ok(self.push_expr(GenExpr::StaticCall {
                    path,
                    args: call_args,
                }))
            }
            // --- compile-time strings ----------------------------------
            ("concat", [lhs, rhs]) => {
                let lhs = self.str_value(lhs.expr)?;
                let rhs = self.str_value(rhs.expr)?;
                let joined = format!("{}{}", lhs.data(self.db), rhs.data(self.db));
                Ok(Value::Str(StringId::new(self.db, joined)))
            }
            ("str", [arg]) => {
                let value = self.checked_inline_str(arg.expr)?;
                Ok(self.push_expr(GenExpr::StrLit(value)))
            }
            ("str_ty", [arg]) => {
                let value = self.checked_inline_str(arg.expr)?;
                Ok(self.push_ty(GenTy::StringN(value.data(self.db).len())))
            }
            // --- tuples ------------------------------------------------
            ("tuple_expr", []) => Ok(self.push_expr(GenExpr::Tuple(Vec::new()))),
            ("with_elem", [tuple_arg, elem_arg]) => {
                let tuple = self.gen_expr_arg(tuple_arg.expr)?;
                let elem = self.gen_expr_arg(elem_arg.expr)?;
                let GenExpr::Tuple(elems) = &self.exprs[tuple.0] else {
                    return Err(self.invalid_method(tuple_arg.expr, "`with_elem` expects a tuple"));
                };
                let mut elems = elems.clone();
                elems.push(elem);
                Ok(self.push_expr(GenExpr::Tuple(elems)))
            }
            ("tuple_ty", []) => Ok(self.push_ty(GenTy::Tuple(Vec::new()))),
            ("with_elem_ty", [tuple_arg, elem_arg]) => {
                let tuple = self.gen_ty_arg(tuple_arg.expr)?;
                let elem = self.gen_ty_arg(elem_arg.expr)?;
                let GenTy::Tuple(elems) = &self.tys[tuple.0] else {
                    return Err(
                        self.invalid_method(tuple_arg.expr, "`with_elem_ty` expects a tuple type")
                    );
                };
                let mut elems = elems.clone();
                elems.push(elem);
                Ok(self.push_ty(GenTy::Tuple(elems)))
            }
            ("trait_assoc_ty", [ty_arg, name_arg]) => {
                let Value::Ty(ty) = self.eval_expr(ty_arg.expr)? else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                let name = self.string_value_ident(name_arg.expr)?;
                Ok(self.push_ty(GenTy::Projection { ty, name }))
            }
            // --- misc --------------------------------------------------
            ("keccak", [arg]) => {
                let arg = self.gen_expr_arg(arg.expr)?;
                Ok(self.push_expr(GenExpr::Keccak(arg)))
            }
            // Syntactic type identity (types are compared as written, after
            // interning); used e.g. to deduplicate referenced struct types.
            ("same_ty", [lhs, rhs]) => {
                let Value::Ty(lhs) = self.eval_expr(lhs.expr)? else {
                    return Err(self.unsupported_expr(lhs.expr));
                };
                let Value::Ty(rhs) = self.eval_expr(rhs.expr)? else {
                    return Err(self.unsupported_expr(rhs.expr));
                };
                Ok(Value::Bool(lhs == rhs))
            }
            ("same_field", [lhs, rhs]) => {
                let Value::Field(lhs) = self.eval_expr(lhs.expr)? else {
                    return Err(self.unsupported_expr(lhs.expr));
                };
                let Value::Field(rhs) = self.eval_expr(rhs.expr)? else {
                    return Err(self.unsupported_expr(rhs.expr));
                };
                Ok(Value::Bool(lhs == rhs))
            }
            ("struct_init", []) => {
                if !self.reflection.is_struct() {
                    return Err(self.invalid_method(expr, "`struct_init` on a non-struct target"));
                }
                Ok(self.push_expr(GenExpr::StructInit { fields: Vec::new() }))
            }
            ("variant_init", [variant_arg]) => {
                let Value::Variant(variant) = self.eval_expr(variant_arg.expr)? else {
                    return Err(self.unsupported_expr(variant_arg.expr));
                };
                Ok(self.push_expr(GenExpr::VariantInit {
                    variant,
                    fields: Vec::new(),
                }))
            }
            ("with_field", [init_arg, field_arg, value_arg]) => {
                let init = self.gen_expr_arg(init_arg.expr)?;
                let Value::Field(field) = self.eval_expr(field_arg.expr)? else {
                    return Err(self.unsupported_expr(field_arg.expr));
                };
                let value = self.gen_expr_arg(value_arg.expr)?;
                let extended = match &self.exprs[init.0] {
                    GenExpr::StructInit { fields } => {
                        if field.variant.is_some() {
                            return Err(self.invalid_method(
                                field_arg.expr,
                                "variant field used in a struct initializer",
                            ));
                        }
                        let mut fields = fields.clone();
                        fields.push((field, value));
                        GenExpr::StructInit { fields }
                    }
                    GenExpr::VariantInit { variant, fields } => {
                        if field.variant != Some(*variant) {
                            return Err(self.invalid_method(
                                field_arg.expr,
                                "field does not belong to the initialized variant",
                            ));
                        }
                        let mut fields = fields.clone();
                        fields.push((field, value));
                        GenExpr::VariantInit {
                            variant: *variant,
                            fields,
                        }
                    }
                    _ => {
                        return Err(self.invalid_method(
                            init_arg.expr,
                            "`with_field` expects a struct or variant initializer",
                        ));
                    }
                };
                Ok(self.push_expr(extended))
            }
            ("match_expr", [scrutinee_arg]) => {
                let scrutinee = self.gen_expr_arg(scrutinee_arg.expr)?;
                Ok(self.push_expr(GenExpr::Match {
                    scrutinee,
                    arms: Vec::new(),
                }))
            }
            ("with_arm", [match_arg, pat_arg, body_arg]) => {
                let match_ = self.gen_expr_arg(match_arg.expr)?;
                let Value::Pat(pat) = self.eval_expr(pat_arg.expr)? else {
                    return Err(self.unsupported_expr(pat_arg.expr));
                };
                let body = self.gen_expr_arg(body_arg.expr)?;
                let GenExpr::Match { scrutinee, arms } = &self.exprs[match_.0] else {
                    return Err(self
                        .invalid_method(match_arg.expr, "`with_arm` expects a match expression"));
                };
                let scrutinee = *scrutinee;
                let mut arms = arms.clone();
                arms.push((pat, body));
                Ok(self.push_expr(GenExpr::Match { scrutinee, arms }))
            }
            ("wildcard_pat", []) => Ok(self.push_pat(GenPat::Wildcard)),
            ("variant_pat", [variant_arg, prefix_arg]) => {
                let Value::Variant(variant) = self.eval_expr(variant_arg.expr)? else {
                    return Err(self.unsupported_expr(variant_arg.expr));
                };
                let prefix = self.string_value_ident(prefix_arg.expr)?;
                Ok(self.push_pat(GenPat::Variant { variant, prefix }))
            }
            ("variant_binder", [variant_arg, field_arg, prefix_arg]) => {
                let Value::Variant(variant) = self.eval_expr(variant_arg.expr)? else {
                    return Err(self.unsupported_expr(variant_arg.expr));
                };
                let Value::Field(field) = self.eval_expr(field_arg.expr)? else {
                    return Err(self.unsupported_expr(field_arg.expr));
                };
                if field.variant != Some(variant) {
                    return Err(self.invalid_method(
                        field_arg.expr,
                        "field does not belong to the named variant",
                    ));
                }
                let prefix = self.string_value_ident(prefix_arg.expr)?;
                Ok(self.push_expr(GenExpr::VariantBinder {
                    variant,
                    field: field.index,
                    prefix,
                }))
            }
            ("method", [name_arg]) => {
                let name = self.string_value_ident(name_arg.expr)?;
                self.sigs.push(GenMethodSig {
                    name,
                    takes_self: false,
                    args: Vec::new(),
                    ret: None,
                });
                Ok(Value::Sig(SigId(self.sigs.len() - 1)))
            }
            ("with_self", [sig_arg]) => {
                let sig = self.sig_arg(sig_arg.expr)?;
                let mut new_sig = self.sigs[sig.0].clone();
                new_sig.takes_self = true;
                self.sigs.push(new_sig);
                Ok(Value::Sig(SigId(self.sigs.len() - 1)))
            }
            ("with_arg", [sig_arg, name_arg, ty_arg]) => {
                let sig = self.sig_arg(sig_arg.expr)?;
                let name = self.string_value_ident(name_arg.expr)?;
                let Value::Ty(ty) = self.eval_expr(ty_arg.expr)? else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                let mut new_sig = self.sigs[sig.0].clone();
                new_sig.args.push((name, ty));
                self.sigs.push(new_sig);
                Ok(Value::Sig(SigId(self.sigs.len() - 1)))
            }
            ("returns", [sig_arg, ty_arg]) => {
                let sig = self.sig_arg(sig_arg.expr)?;
                let Value::Ty(ty) = self.eval_expr(ty_arg.expr)? else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                let mut new_sig = self.sigs[sig.0].clone();
                new_sig.ret = Some(ty);
                self.sigs.push(new_sig);
                Ok(Value::Sig(SigId(self.sigs.len() - 1)))
            }
            ("target_ty", []) => Ok(Value::Ty(self.target_ty)),
            ("self_ty", []) => Ok(Value::Ty(TypeId::fallback_self_ty(self.db))),
            ("ty", []) => {
                let Some(path) = self.single_type_generic_arg(generic_args) else {
                    return Err(self.unsupported_expr(expr));
                };
                Ok(Value::Ty(path))
            }
            _ => Err(self.unsupported_expr(expr)),
        }
    }

    fn check_not_finished(&mut self, expr: ExprId) -> Result<(), ExecError> {
        if self.finished {
            return Err(ExecError {
                kind: ProviderFailureKind::CommandAfterFinish,
                range: self.expr_range(expr),
            });
        }
        Ok(())
    }

    fn invalid_method(&mut self, expr: ExprId, detail: &str) -> ExecError {
        ExecError {
            kind: ProviderFailureKind::InvalidMethod {
                detail: detail.to_string(),
            },
            range: self.expr_range(expr),
        }
    }

    fn invalid_assoc(&mut self, expr: ExprId, detail: &str) -> ExecError {
        ExecError {
            kind: ProviderFailureKind::InvalidAssoc {
                detail: detail.to_string(),
            },
            range: self.expr_range(expr),
        }
    }

    fn invalid_string(&mut self, expr: ExprId, detail: &str) -> ExecError {
        ExecError {
            kind: ProviderFailureKind::InvalidString {
                detail: detail.to_string(),
            },
            range: self.expr_range(expr),
        }
    }

    fn invalid_quote(&mut self, expr: ExprId, detail: &str) -> ExecError {
        ExecError {
            kind: ProviderFailureKind::InvalidQuote {
                detail: detail.to_string(),
            },
            range: self.expr_range(expr),
        }
    }

    /// Rejects a second `emit_const` / `emit_assoc_ty` / method-name reuse
    /// for `name` (the generated impl namespaces consts, types, and methods
    /// together for simplicity; EIP-712-style providers never collide).
    fn check_fresh_assoc(&mut self, expr: ExprId, name: IdentId<'db>) -> Result<(), ExecError> {
        if self.emitted_assocs.contains(&name) {
            return Err(self.invalid_assoc(
                expr,
                &format!(
                    "duplicate generated associated item `{}`",
                    name.data(self.db)
                ),
            ));
        }
        self.emitted_assocs.push(name);
        Ok(())
    }

    fn push_expr(&mut self, expr: GenExpr<'db>) -> Value<'db> {
        Value::Expr(self.push_gen(expr))
    }

    fn push_gen(&mut self, expr: GenExpr<'db>) -> GenExprId {
        self.exprs.push(expr);
        GenExprId(self.exprs.len() - 1)
    }

    fn push_pat(&mut self, pat: GenPat<'db>) -> Value<'db> {
        Value::Pat(self.push_gen_pat(pat))
    }

    fn push_gen_pat(&mut self, pat: GenPat<'db>) -> GenPatId {
        self.pats.push(pat);
        GenPatId(self.pats.len() - 1)
    }

    fn push_ty(&mut self, ty: GenTy<'db>) -> Value<'db> {
        self.tys.push(ty);
        Value::GenTy(GenTyId(self.tys.len() - 1))
    }

    fn gen_expr_arg(&mut self, expr: ExprId) -> Result<GenExprId, ExecError> {
        match self.eval_expr(expr)? {
            Value::Expr(id) => Ok(id),
            _ => Err(self.unsupported_expr(expr)),
        }
    }

    /// A generated-type argument. Concrete `Ty` witnesses (from `ty<T>()` /
    /// `field.ty()` / `target_ty()`) are accepted and wrapped, so type
    /// commands take either currency.
    fn gen_ty_arg(&mut self, expr: ExprId) -> Result<GenTyId, ExecError> {
        match self.eval_expr(expr)? {
            Value::GenTy(id) => Ok(id),
            Value::Ty(ty) => {
                let Value::GenTy(id) = self.push_ty(GenTy::Concrete(ty)) else {
                    unreachable!("push_ty returns a GenTy value");
                };
                Ok(id)
            }
            _ => Err(self.unsupported_expr(expr)),
        }
    }

    fn sig_arg(&mut self, expr: ExprId) -> Result<SigId, ExecError> {
        match self.eval_expr(expr)? {
            Value::Sig(id) => Ok(id),
            _ => Err(self.invalid_method(expr, "expected a method signature")),
        }
    }

    /// A compile-time string operand: a string literal, a reflected name,
    /// or a `concat` result.
    fn str_value(&mut self, expr: ExprId) -> Result<StringId<'db>, ExecError> {
        match self.eval_expr(expr)? {
            Value::Str(value) => Ok(value),
            _ => Err(self.invalid_string(expr, "expected a compile-time string")),
        }
    }

    /// A compile-time string destined for a generated string literal or
    /// exact-width string type; enforces the inline string capacity.
    fn checked_inline_str(&mut self, expr: ExprId) -> Result<StringId<'db>, ExecError> {
        let value = self.str_value(expr)?;
        self.check_inline_capacity(expr, value)?;
        Ok(value)
    }

    fn check_inline_capacity(
        &mut self,
        expr: ExprId,
        value: StringId<'db>,
    ) -> Result<(), ExecError> {
        let len = value.data(self.db).len();
        if len > MAX_INLINE_STRING_BYTES {
            return Err(self.invalid_string(
                expr,
                &format!(
                    "string piece is {len} bytes; inline strings hold at most \
                     {MAX_INLINE_STRING_BYTES}"
                ),
            ));
        }
        Ok(())
    }

    fn string_value_ident(&mut self, expr: ExprId) -> Result<IdentId<'db>, ExecError> {
        let value = self.str_value(expr)?;
        Ok(IdentId::new(self.db, value.data(self.db).to_string()))
    }

    /// The single type argument of `require<Trait>(..)` / `ty<T>()`, as a
    /// path. Returns `None` for malformed argument lists.
    fn single_type_generic_arg_path(
        &self,
        generic_args: GenericArgListId<'db>,
    ) -> Option<crate::hir_def::PathId<'db>> {
        let [GenericArg::Type(type_arg)] = generic_args.data(self.db).as_slice() else {
            return None;
        };
        let ty = type_arg.ty.to_opt()?;
        match ty.data(self.db) {
            crate::hir_def::TypeKind::Path(path) => path.to_opt(),
            _ => None,
        }
    }

    /// The single type argument of `ty<T>()`, as written.
    fn single_type_generic_arg(&self, generic_args: GenericArgListId<'db>) -> Option<TypeId<'db>> {
        let [GenericArg::Type(type_arg)] = generic_args.data(self.db).as_slice() else {
            return None;
        };
        type_arg.ty.to_opt()
    }

    /// Detects the supported `for` iterables: `reflect.fields()`,
    /// `reflect.variants()`, and `variant.fields()`.
    fn eval_iterable(&mut self, iterable: ExprId) -> Result<Iterable, ExecError> {
        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            iterable.data(self.db, self.body)
        else {
            return Err(self.unsupported_expr(iterable));
        };
        let (receiver, args_empty) = (*receiver, args.is_empty());
        let Some(method) = method.to_opt() else {
            return Err(self.unsupported_expr(iterable));
        };
        if !args_empty {
            return Err(self.unsupported_expr(iterable));
        }
        let receiver_value = self.eval_expr(receiver)?;
        match (receiver_value, method.data(self.db).as_str()) {
            (Value::Reflect, "fields") => Ok(Iterable::StructFields),
            (Value::Reflect, "variants") => Ok(Iterable::Variants),
            (Value::Variant(variant), "fields") => Ok(Iterable::VariantFields(variant)),
            _ => Err(self.unsupported_expr(iterable)),
        }
    }

    fn simple_pat_binding(&self, pat: PatId) -> Option<IdentId<'db>> {
        let Partial::Present(Pat::Path(Partial::Present(path), _)) = pat.data(self.db, self.body)
        else {
            return None;
        };
        path.as_ident(self.db)
    }

    fn simple_expr_path_ident(&self, expr: ExprId) -> Option<IdentId<'db>> {
        let Partial::Present(Expr::Path(Partial::Present(path))) = expr.data(self.db, self.body)
        else {
            return None;
        };
        path.as_ident(self.db)
    }
}

/// A human-readable kind name for hole-value diagnostics.
fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "compile-time bool",
        Value::Str(_) => "compile-time string",
        Value::Field(_) => "`Field` handle",
        Value::Variant(_) => "`Variant` handle",
        Value::Ty(_) | Value::GenTy(_) => "type value",
        Value::Expr(_) => "generated expression",
        Value::Pat(_) => "generated pattern",
        Value::Sig(_) => "method signature",
        Value::Quote(_) => "quote value",
        Value::Builder => "builder capability",
        Value::Reflect => "reflect capability",
        Value::Evidence => "evidence value",
        Value::Unit => "unit value",
    }
}
