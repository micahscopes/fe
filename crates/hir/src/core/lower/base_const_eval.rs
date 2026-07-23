//! Analysis-neutral scalar CTFE for the pre-expansion world.
//!
//! This is the single interpreter used by provider ordinary const-helper
//! steering and recursive-type-function staged payloads. It intentionally
//! supports only pure integer/bool computation over base-graph functions.

use num_bigint::BigUint;
use num_traits::{CheckedSub, One, ToPrimitive};

use crate::{
    HirDb,
    core::{
        hir_def::{
            ArithBinOp, BinOp, Body, CompBinOp, Cond, CondId, Expr, ExprId, Func, IdentId,
            IntegerId, ItemKind, LitKind, LogicalBinOp, Partial, Pat, Stmt, StmtId, TopLevelMod,
            UnOp,
        },
        lower::provider::resolve_base_item,
    },
};

const STEP_LIMIT: usize = 100_000;
const CALL_LIMIT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseConstValue<'db> {
    UInt {
        value: IntegerId<'db>,
        kind: BaseUIntKind,
    },
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseUIntKind {
    U32,
    Usize,
    Inferred,
}

impl BaseUIntKind {
    fn bits(self) -> usize {
        match self {
            Self::U32 => 32,
            Self::Usize | Self::Inferred => 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseConstEvalError {
    Unsupported,
    Limit,
}

pub(super) fn eval_base_const_body<'db>(
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
    body: Body<'db>,
    bindings: Vec<(IdentId<'db>, BaseConstValue<'db>)>,
    inherited_consts: &[(IdentId<'db>, BaseConstValue<'db>)],
    expected: Option<BaseUIntKind>,
) -> Result<BaseConstValue<'db>, BaseConstEvalError> {
    BaseConstEvaluator {
        db,
        top_mod,
        inherited_consts,
        body: Some(body),
        scopes: vec![bindings],
        steps: 0,
        call_stack: Vec::new(),
    }
    .eval_body(body, expected)
}

pub(super) fn eval_base_const_func<'db>(
    db: &'db dyn HirDb,
    func: Func<'db>,
    values: Vec<BaseConstValue<'db>>,
) -> Result<BaseConstValue<'db>, BaseConstEvalError> {
    if !func.is_const(db)
        || func.modifiers(db).is_extern
        || !func.generic_params(db).data(db).is_empty()
        || !func.effects(db).data(db).is_empty()
    {
        return Err(BaseConstEvalError::Unsupported);
    }
    let body = func.body(db).ok_or(BaseConstEvalError::Unsupported)?;
    let params = func
        .params_list(db)
        .to_opt()
        .ok_or(BaseConstEvalError::Unsupported)?;
    if params.data(db).len() != values.len() {
        return Err(BaseConstEvalError::Unsupported);
    }
    let bindings = params
        .data(db)
        .iter()
        .zip(values)
        .map(|(param, value)| {
            let name = param.name().ok_or(BaseConstEvalError::Unsupported)?;
            let ty = param.ty.to_opt();
            let value = if let Some(kind) = raw_uint_kind(db, ty) {
                coerce_uint(db, value, kind, false)?
            } else if raw_is_bool(db, ty) && matches!(value, BaseConstValue::Bool(_)) {
                value
            } else {
                return Err(BaseConstEvalError::Unsupported);
            };
            Ok((name, value))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected = raw_uint_kind(db, func.ret_type_ref(db));
    if expected.is_none() && !raw_is_bool(db, func.ret_type_ref(db)) {
        return Err(BaseConstEvalError::Unsupported);
    }
    let value = eval_base_const_body(db, func.top_mod(db), body, bindings, &[], expected)?;
    if let Some(kind) = expected {
        coerce_uint(db, value, kind, false)
    } else if matches!(value, BaseConstValue::Bool(_)) {
        Ok(value)
    } else {
        Err(BaseConstEvalError::Unsupported)
    }
}

pub(super) fn raw_uint_kind(
    db: &dyn HirDb,
    ty: Option<crate::hir_def::TypeId>,
) -> Option<BaseUIntKind> {
    let crate::hir_def::TypeKind::Path(Partial::Present(path)) = ty?.data(db) else {
        return None;
    };
    match path.as_ident(db)?.data(db).as_str() {
        "u32" => Some(BaseUIntKind::U32),
        "usize" => Some(BaseUIntKind::Usize),
        _ => None,
    }
}

fn raw_is_bool(db: &dyn HirDb, ty: Option<crate::hir_def::TypeId>) -> bool {
    let Some(crate::hir_def::TypeKind::Path(Partial::Present(path))) = ty.map(|ty| ty.data(db))
    else {
        return false;
    };
    path.as_ident(db)
        .is_some_and(|name| name.data(db).as_str() == "bool")
}

fn coerce_uint<'db>(
    db: &'db dyn HirDb,
    value: BaseConstValue<'db>,
    kind: BaseUIntKind,
    truncate: bool,
) -> Result<BaseConstValue<'db>, BaseConstEvalError> {
    let BaseConstValue::UInt { value, .. } = value else {
        return Err(BaseConstEvalError::Unsupported);
    };
    let modulus = BigUint::one() << kind.bits();
    let raw = if truncate {
        value.data(db) % &modulus
    } else {
        if value.data(db) >= &modulus {
            return Err(BaseConstEvalError::Unsupported);
        }
        value.data(db).clone()
    };
    Ok(BaseConstValue::UInt {
        value: IntegerId::new(db, raw),
        kind,
    })
}

pub(super) fn eval_base_uint_binop<'db>(
    db: &'db dyn HirDb,
    op: ArithBinOp,
    lhs: IntegerId<'db>,
    rhs: IntegerId<'db>,
    kind: BaseUIntKind,
) -> Result<IntegerId<'db>, BaseConstEvalError> {
    let (lhs, rhs) = (lhs.data(db), rhs.data(db));
    let bits = kind.bits();
    let value = match op {
        ArithBinOp::Add => lhs + rhs,
        ArithBinOp::Sub => lhs
            .checked_sub(rhs)
            .ok_or(BaseConstEvalError::Unsupported)?,
        ArithBinOp::Mul => lhs * rhs,
        ArithBinOp::Div if rhs.bits() != 0 => lhs / rhs,
        ArithBinOp::Rem if rhs.bits() != 0 => lhs % rhs,
        ArithBinOp::LShift => match rhs.to_usize() {
            Some(shift) if shift < bits => (lhs << shift) % (BigUint::one() << bits),
            _ => BigUint::default(),
        },
        ArithBinOp::RShift => match rhs.to_usize() {
            Some(shift) if shift < bits => lhs >> shift,
            _ => BigUint::default(),
        },
        ArithBinOp::BitAnd => lhs & rhs,
        ArithBinOp::BitOr => lhs | rhs,
        ArithBinOp::BitXor => lhs ^ rhs,
        ArithBinOp::Pow | ArithBinOp::Range | ArithBinOp::Div | ArithBinOp::Rem => {
            return Err(BaseConstEvalError::Unsupported);
        }
    };
    let value = IntegerId::new(db, value);
    let BaseConstValue::UInt { value, .. } =
        coerce_uint(db, BaseConstValue::UInt { value, kind }, kind, false)?
    else {
        unreachable!()
    };
    Ok(value)
}

enum Flow<'db> {
    Next(Option<BaseConstValue<'db>>),
    Return(BaseConstValue<'db>),
}

struct BaseConstEvaluator<'a, 'db> {
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
    inherited_consts: &'a [(IdentId<'db>, BaseConstValue<'db>)],
    body: Option<Body<'db>>,
    scopes: Vec<Vec<(IdentId<'db>, BaseConstValue<'db>)>>,
    steps: usize,
    call_stack: Vec<Func<'db>>,
}

impl<'db> BaseConstEvaluator<'_, 'db> {
    fn tick(&mut self) -> Result<(), BaseConstEvalError> {
        self.steps += 1;
        (self.steps <= STEP_LIMIT)
            .then_some(())
            .ok_or(BaseConstEvalError::Limit)
    }

    fn eval_body(
        &mut self,
        body: Body<'db>,
        expected: Option<BaseUIntKind>,
    ) -> Result<BaseConstValue<'db>, BaseConstEvalError> {
        let old = self.body.replace(body);
        let result = match self.eval_expr(body.expr(self.db), expected)? {
            Flow::Next(Some(value)) | Flow::Return(value) => Ok(value),
            Flow::Next(None) => Err(BaseConstEvalError::Unsupported),
        };
        self.body = old;
        result
    }

    fn body(&self) -> Result<Body<'db>, BaseConstEvalError> {
        self.body.ok_or(BaseConstEvalError::Unsupported)
    }

    fn bind(&mut self, name: IdentId<'db>, value: BaseConstValue<'db>) {
        self.scopes.last_mut().unwrap().push((name, value));
    }

    fn lookup(&self, name: IdentId<'db>) -> Option<BaseConstValue<'db>> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .find_map(|(bound, value)| (*bound == name).then_some(*value))
            .or_else(|| {
                self.inherited_consts
                    .iter()
                    .rev()
                    .find_map(|(bound, value)| (*bound == name).then_some(*value))
            })
    }

    fn assign(&mut self, name: IdentId<'db>, value: BaseConstValue<'db>) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if let Some((_, slot)) = scope.iter_mut().rev().find(|(bound, _)| *bound == name) {
                *slot = value;
                return true;
            }
        }
        false
    }

    fn pat_name(&self, pat: crate::hir_def::PatId) -> Option<IdentId<'db>> {
        let body = self.body?;
        let Partial::Present(Pat::Path(Partial::Present(path), _)) = pat.data(self.db, body) else {
            return None;
        };
        path.as_ident(self.db)
    }

    fn expr_name(&self, expr: ExprId) -> Option<IdentId<'db>> {
        let body = self.body?;
        let Partial::Present(Expr::Path(Partial::Present(path))) = expr.data(self.db, body) else {
            return None;
        };
        path.as_ident(self.db)
    }

    fn eval_stmt(&mut self, stmt: StmtId) -> Result<Flow<'db>, BaseConstEvalError> {
        self.tick()?;
        let body = self.body()?;
        let Partial::Present(stmt) = stmt.data(self.db, body) else {
            return Err(BaseConstEvalError::Unsupported);
        };
        match stmt {
            Stmt::Let(pat, ty, Some(init)) => {
                let name = self.pat_name(*pat).ok_or(BaseConstEvalError::Unsupported)?;
                let expected = raw_uint_kind(self.db, *ty);
                let value = self.value(*init, expected)?;
                self.bind(name, value);
                Ok(Flow::Next(None))
            }
            Stmt::Expr(expr) => self.eval_expr(*expr, None),
            Stmt::Return(Some(expr)) => Ok(Flow::Return(self.value(*expr, None)?)),
            Stmt::While(cond, loop_body) => {
                while self.eval_cond(*cond)? {
                    match self.eval_expr(*loop_body, None)? {
                        Flow::Next(_) => {}
                        flow @ Flow::Return(_) => return Ok(flow),
                    }
                    self.tick()?;
                }
                Ok(Flow::Next(None))
            }
            _ => Err(BaseConstEvalError::Unsupported),
        }
    }

    fn eval_expr(
        &mut self,
        expr: ExprId,
        expected: Option<BaseUIntKind>,
    ) -> Result<Flow<'db>, BaseConstEvalError> {
        self.tick()?;
        let body = self.body()?;
        let data = expr
            .data(self.db, body)
            .clone()
            .to_opt()
            .ok_or(BaseConstEvalError::Unsupported)?;
        match data {
            Expr::Block(stmts) => {
                self.scopes.push(Vec::new());
                let mut tail = None;
                for stmt in stmts {
                    match self.eval_stmt(stmt)? {
                        Flow::Next(value) => tail = value,
                        flow @ Flow::Return(_) => {
                            self.scopes.pop();
                            return Ok(flow);
                        }
                    }
                }
                self.scopes.pop();
                Ok(Flow::Next(tail))
            }
            Expr::If(cond, then_expr, else_expr) => {
                if self.eval_cond(cond)? {
                    self.eval_expr(then_expr, expected)
                } else if let Some(else_expr) = else_expr {
                    self.eval_expr(else_expr, expected)
                } else {
                    Ok(Flow::Next(None))
                }
            }
            Expr::Assign(lhs, rhs) => {
                let name = self.expr_name(lhs).ok_or(BaseConstEvalError::Unsupported)?;
                let value = self.value(rhs, expected)?;
                if !self.assign(name, value) {
                    return Err(BaseConstEvalError::Unsupported);
                }
                Ok(Flow::Next(Some(value)))
            }
            _ => Ok(Flow::Next(Some(self.eval_scalar(data, expected)?))),
        }
    }

    fn value(
        &mut self,
        expr: ExprId,
        expected: Option<BaseUIntKind>,
    ) -> Result<BaseConstValue<'db>, BaseConstEvalError> {
        match self.eval_expr(expr, expected)? {
            Flow::Next(Some(value)) | Flow::Return(value) => Ok(value),
            Flow::Next(None) => Err(BaseConstEvalError::Unsupported),
        }
    }

    fn uint(
        &mut self,
        expr: ExprId,
        expected: Option<BaseUIntKind>,
    ) -> Result<(IntegerId<'db>, BaseUIntKind), BaseConstEvalError> {
        match self.value(expr, expected)? {
            BaseConstValue::UInt { value, kind } => Ok((value, kind)),
            BaseConstValue::Bool(_) => Err(BaseConstEvalError::Unsupported),
        }
    }

    fn eval_scalar(
        &mut self,
        data: Expr<'db>,
        expected: Option<BaseUIntKind>,
    ) -> Result<BaseConstValue<'db>, BaseConstEvalError> {
        match data {
            Expr::Lit(LitKind::Int(value)) => coerce_uint(
                self.db,
                BaseConstValue::UInt {
                    value,
                    kind: BaseUIntKind::Inferred,
                },
                expected.unwrap_or(BaseUIntKind::Inferred),
                false,
            ),
            Expr::Lit(LitKind::Bool(value)) => Ok(BaseConstValue::Bool(value)),
            Expr::Path(Partial::Present(path)) => {
                let name = path
                    .as_ident(self.db)
                    .ok_or(BaseConstEvalError::Unsupported)?;
                self.lookup(name).ok_or(BaseConstEvalError::Unsupported)
            }
            Expr::Bin(lhs, rhs, BinOp::Comp(op)) => {
                let (lhs, kind) = self.uint(lhs, expected)?;
                let (rhs, _) = self.uint(rhs, Some(kind))?;
                let ordering = lhs.data(self.db).cmp(rhs.data(self.db));
                Ok(BaseConstValue::Bool(match op {
                    CompBinOp::Eq => ordering.is_eq(),
                    CompBinOp::NotEq => !ordering.is_eq(),
                    CompBinOp::Lt => ordering.is_lt(),
                    CompBinOp::LtEq => !ordering.is_gt(),
                    CompBinOp::Gt => ordering.is_gt(),
                    CompBinOp::GtEq => !ordering.is_lt(),
                }))
            }
            Expr::Bin(lhs, rhs, BinOp::Arith(op)) => {
                let (lhs, kind) = self.uint(lhs, expected)?;
                let (rhs, _) = self.uint(rhs, Some(kind))?;
                Ok(BaseConstValue::UInt {
                    value: eval_base_uint_binop(self.db, op, lhs, rhs, kind)?,
                    kind,
                })
            }
            Expr::Un(inner, UnOp::Not) => match self.value(inner, None)? {
                BaseConstValue::Bool(value) => Ok(BaseConstValue::Bool(!value)),
                BaseConstValue::UInt { .. } => Err(BaseConstEvalError::Unsupported),
            },
            Expr::Call(callee, args) => {
                let body = self.body()?;
                let Partial::Present(Expr::Path(Partial::Present(path))) =
                    callee.data(self.db, body)
                else {
                    return Err(BaseConstEvalError::Unsupported);
                };
                let ItemKind::Func(func) = resolve_base_item(self.db, self.top_mod, *path)
                    .ok_or(BaseConstEvalError::Unsupported)?
                else {
                    return Err(BaseConstEvalError::Unsupported);
                };
                let values = args
                    .iter()
                    .map(|arg| self.value(arg.expr, None))
                    .collect::<Result<Vec<_>, _>>()?;
                self.call(func, values)
            }
            Expr::MethodCall(receiver, method, generic_args, args)
                if args.is_empty()
                    && generic_args.is_empty(self.db)
                    && method
                        .to_opt()
                        .is_some_and(|name| name.data(self.db) == "downcast_truncate") =>
            {
                let target = expected.ok_or(BaseConstEvalError::Unsupported)?;
                coerce_uint(self.db, self.value(receiver, None)?, target, true)
            }
            Expr::Cast(inner, ty) => {
                let target = raw_uint_kind(self.db, ty.to_opt())
                    .or(expected)
                    .ok_or(BaseConstEvalError::Unsupported)?;
                coerce_uint(self.db, self.value(inner, None)?, target, false)
            }
            _ => Err(BaseConstEvalError::Unsupported),
        }
    }

    fn eval_cond(&mut self, cond: CondId) -> Result<bool, BaseConstEvalError> {
        let body = self.body()?;
        let data = cond
            .data(self.db, body)
            .clone()
            .to_opt()
            .ok_or(BaseConstEvalError::Unsupported)?;
        match data {
            Cond::Expr(expr) => match self.value(expr, None)? {
                BaseConstValue::Bool(value) => Ok(value),
                BaseConstValue::UInt { .. } => Err(BaseConstEvalError::Unsupported),
            },
            Cond::Bin(lhs, rhs, LogicalBinOp::And) => {
                Ok(self.eval_cond(lhs)? && self.eval_cond(rhs)?)
            }
            Cond::Bin(lhs, rhs, LogicalBinOp::Or) => {
                Ok(self.eval_cond(lhs)? || self.eval_cond(rhs)?)
            }
            Cond::Let(..) => Err(BaseConstEvalError::Unsupported),
        }
    }

    fn call(
        &mut self,
        func: Func<'db>,
        values: Vec<BaseConstValue<'db>>,
    ) -> Result<BaseConstValue<'db>, BaseConstEvalError> {
        if !func.is_const(self.db)
            || func.modifiers(self.db).is_extern
            || !func.generic_params(self.db).data(self.db).is_empty()
            || !func.effects(self.db).data(self.db).is_empty()
            || self.call_stack.len() == CALL_LIMIT
            || self.call_stack.contains(&func)
        {
            return Err(BaseConstEvalError::Unsupported);
        }
        let params = func
            .params_list(self.db)
            .to_opt()
            .ok_or(BaseConstEvalError::Unsupported)?;
        if params.data(self.db).len() != values.len() {
            return Err(BaseConstEvalError::Unsupported);
        }
        let bindings = params
            .data(self.db)
            .iter()
            .zip(values)
            .map(|(param, value)| {
                let name = param.name().ok_or(BaseConstEvalError::Unsupported)?;
                let ty = param.ty.to_opt();
                let value = if let Some(kind) = raw_uint_kind(self.db, ty) {
                    coerce_uint(self.db, value, kind, false)?
                } else if raw_is_bool(self.db, ty) && matches!(value, BaseConstValue::Bool(_)) {
                    value
                } else {
                    return Err(BaseConstEvalError::Unsupported);
                };
                Ok((name, value))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let body = func.body(self.db).ok_or(BaseConstEvalError::Unsupported)?;
        let expected = raw_uint_kind(self.db, func.ret_type_ref(self.db));
        let expects_bool = raw_is_bool(self.db, func.ret_type_ref(self.db));
        if expected.is_none() && !expects_bool {
            return Err(BaseConstEvalError::Unsupported);
        }
        let old_body = self.body.replace(body);
        let old_top_mod = std::mem::replace(&mut self.top_mod, func.top_mod(self.db));
        let old_scopes = std::mem::replace(&mut self.scopes, vec![bindings]);
        self.call_stack.push(func);
        let result = (|| match self.eval_expr(body.expr(self.db), expected)? {
            Flow::Next(Some(value)) | Flow::Return(value) => {
                if let Some(kind) = expected {
                    coerce_uint(self.db, value, kind, false)
                } else if expects_bool && matches!(value, BaseConstValue::Bool(_)) {
                    Ok(value)
                } else {
                    Err(BaseConstEvalError::Unsupported)
                }
            }
            Flow::Next(None) => Err(BaseConstEvalError::Unsupported),
        })();
        self.scopes = old_scopes;
        self.call_stack.pop();
        self.top_mod = old_top_mod;
        self.body = old_body;
        result
    }
}
