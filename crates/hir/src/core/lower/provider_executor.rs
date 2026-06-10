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
    hir_def::{
        Body, Cond, CondId, Expr, ExprId, GenericArg, GenericArgListId, IdentId, LitKind,
        LogicalBinOp, Partial, Pat, PatId, Stmt, StmtId, TypeId,
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
    /// The generated method's `self` value.
    SelfRef,
    /// A reference to a generated method parameter by name.
    ArgRef(IdentId<'db>),
    /// `base.field`
    FieldGet(GenExprId, FieldKey),
    /// `lhs == rhs`
    EqCmp(GenExprId, GenExprId),
    /// `<ty as GoalishTrait>::method()` — a qualified call of the request's
    /// goal trait method on `ty`.
    TraitCall {
        ty: TypeId<'db>,
        method: IdentId<'db>,
    },
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
pub(super) struct SigId(pub(super) usize);

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
}

/// The successful result of running a provider body: the recorded commands
/// plus the arenas the generated expression/pattern/signature ids index
/// into.
#[derive(Debug)]
pub(super) struct ProviderOutput<'db> {
    pub(super) exprs: Vec<GenExpr<'db>>,
    pub(super) pats: Vec<GenPat<'db>>,
    pub(super) sigs: Vec<GenMethodSig<'db>>,
    pub(super) commands: Vec<BuilderCommand<'db>>,
}

/// A compile-time value in the provider command language.
#[derive(Debug, Clone, Copy)]
enum Value<'db> {
    Bool(bool),
    /// A reflected field handle.
    Field(FieldKey),
    /// A reflected variant handle (index into the target's variants).
    Variant(usize),
    /// A type witness (e.g. the result of `field.ty()`).
    Ty(TypeId<'db>),
    /// A generated expression.
    Expr(GenExprId),
    /// A generated pattern.
    Pat(GenPatId),
    /// A generated method signature.
    Sig(SigId),
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
    /// The provider's module, for canonicalizing `require<Trait>` paths.
    provider_top_mod: crate::hir_def::TopLevelMod<'db>,
    /// Lexically scoped value bindings; the innermost binding of a name
    /// shadows outer ones (including the capability params).
    scopes: Vec<Vec<(IdentId<'db>, Value<'db>)>>,

    exprs: Vec<GenExpr<'db>>,
    pats: Vec<GenPat<'db>>,
    sigs: Vec<GenMethodSig<'db>>,
    commands: Vec<BuilderCommand<'db>>,
    emitted_methods: Vec<IdentId<'db>>,
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
    ) -> Result<ProviderOutput<'db>, ExecError> {
        let mut initial_scope = Vec::new();
        for &name in &provider.param_names {
            initial_scope.push((name, Value::Evidence));
        }
        for &name in &provider.reflect_names {
            initial_scope.push((name, Value::Reflect));
        }
        for &name in &provider.builder_names {
            initial_scope.push((name, Value::Builder));
        }

        let fallback_range = super::provider::provider_name_range(db, provider.provider);
        let mut executor = ProviderExecutor {
            db,
            body: provider.body,
            reflection,
            target_ty,
            provider_top_mod: provider.provider.top_mod(db),
            scopes: vec![initial_scope],
            exprs: Vec::new(),
            pats: Vec::new(),
            sigs: Vec::new(),
            commands: Vec::new(),
            emitted_methods: Vec::new(),
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
            _ => Err(self.unsupported_expr(expr)),
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
                _ => Err(self.unsupported_expr(expr)),
            },
            Value::Variant(variant) => match (method_name.as_str(), args.as_slice()) {
                ("is_default", []) => {
                    let Some(reflected) = self.reflection.variant(variant) else {
                        return Err(self.unsupported_expr(expr));
                    };
                    Ok(Value::Bool(reflected.is_default))
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
                let Value::Expr(body) = self.eval_expr(body_arg.expr)? else {
                    return Err(
                        self.invalid_method(body_arg.expr, "expected a generated expression")
                    );
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
            ("self_ref", []) => Ok(self.push_expr(GenExpr::SelfRef)),
            ("arg_ref", [arg]) => {
                let name = self.string_literal_ident(arg.expr)?;
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
            ("trait_call", [ty_arg, method_arg]) => {
                let Value::Ty(ty) = self.eval_expr(ty_arg.expr)? else {
                    return Err(self.unsupported_expr(ty_arg.expr));
                };
                let method = self.string_literal_ident(method_arg.expr)?;
                Ok(self.push_expr(GenExpr::TraitCall { ty, method }))
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
                let prefix = self.string_literal_ident(prefix_arg.expr)?;
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
                let prefix = self.string_literal_ident(prefix_arg.expr)?;
                Ok(self.push_expr(GenExpr::VariantBinder {
                    variant,
                    field: field.index,
                    prefix,
                }))
            }
            ("method", [name_arg]) => {
                let name = self.string_literal_ident(name_arg.expr)?;
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
                let name = self.string_literal_ident(name_arg.expr)?;
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

    fn push_expr(&mut self, expr: GenExpr<'db>) -> Value<'db> {
        self.exprs.push(expr);
        Value::Expr(GenExprId(self.exprs.len() - 1))
    }

    fn push_pat(&mut self, pat: GenPat<'db>) -> Value<'db> {
        self.pats.push(pat);
        Value::Pat(GenPatId(self.pats.len() - 1))
    }

    fn gen_expr_arg(&mut self, expr: ExprId) -> Result<GenExprId, ExecError> {
        match self.eval_expr(expr)? {
            Value::Expr(id) => Ok(id),
            _ => Err(self.unsupported_expr(expr)),
        }
    }

    fn sig_arg(&mut self, expr: ExprId) -> Result<SigId, ExecError> {
        match self.eval_expr(expr)? {
            Value::Sig(id) => Ok(id),
            _ => Err(self.invalid_method(expr, "expected a method signature")),
        }
    }

    fn string_literal_ident(&mut self, expr: ExprId) -> Result<IdentId<'db>, ExecError> {
        let Partial::Present(Expr::Lit(LitKind::String(value))) = expr.data(self.db, self.body)
        else {
            return Err(self.unsupported_expr(expr));
        };
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
