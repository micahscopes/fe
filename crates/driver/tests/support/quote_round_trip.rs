//! Independent comparison of frozen quote templates with checked output.
//! This walks the input graph and output HIR together. It neither invokes the
//! copier nor prints and reparses code, and unexpected forms fail closed.

use std::collections::{HashMap, HashSet};

use common::InputDb;
use fe_driver::{
    DriverDataBase,
    elaboration::read_back::{ReadBackError, read_back},
};
use hir::{
    HirDb,
    analysis::semantic::SemConstScalar,
    core::meta::{
        FrozenCode, FrozenCodeHole, FrozenCodeNode, FrozenGeneration, FrozenHandleKind,
        FrozenItemLocator, FrozenMetaValue, FrozenMetaVariantCase, FrozenQuoteConstructorSite,
        FrozenQuoteFieldType, FrozenQuoteSignatureType, FrozenTemplateSource, MetaItem,
    },
    hir_def::{
        Body, Cond, CondId, Expr, ExprId, FieldIndex, Func, GenericParamOwner, IdentId, ItemKind,
        LitKind, Partial, Pat, PatId, Quote, QuoteFragment, QuoteFunc, QuoteItem, QuoteName, Stmt,
        StmtId, TypeKind,
    },
    span::{DesugaredOrigin, HirOrigin, LazySpan},
};
use url::Url;

fn owner<'db>(db: &'db DriverDataBase, locator: &FrozenItemLocator) -> Func<'db> {
    let file = db
        .workspace()
        .get(db, locator.file())
        .expect("template file");
    let matches = db
        .top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .filter(|func| func.frozen_locator(db).as_ref() == Some(locator))
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "unique frozen template owner: {locator:?}"
    );
    matches[0]
}

fn quote<'db>(db: &'db DriverDataBase, node: &FrozenCodeNode) -> Option<&'db Quote<'db>> {
    let key = node.template.as_ref()?;
    assert_eq!(key.source, FrozenTemplateSource::Quote);
    let body = owner(db, &key.owner).body(db).expect("template owner body");
    let Expr::Quote(quote) = body.exprs(db)[key.expr].unwrap_ref() else {
        panic!("frozen template must still be a quote");
    };
    Some(quote)
}

fn unwrapped<'db>(db: &'db DriverDataBase, body: Body<'db>, mut expr: ExprId) -> ExprId {
    // Generated bodies add an expression-statement wrapper. Single expression
    // blocks have no binders; retain every block containing a declaration.
    loop {
        let Expr::Block(stmts) = body.exprs(db)[expr].unwrap_ref() else {
            return expr;
        };
        let [stmt] = stmts.as_slice() else {
            return expr;
        };
        let Stmt::Expr(inner) = body.stmts(db)[*stmt].unwrap_ref() else {
            return expr;
        };
        expr = *inner;
    }
}

struct Frame<'db> {
    node: usize,
    body: Body<'db>,
    owner: Func<'db>,
    function: u32,
    open: Vec<IdentId<'db>>,
    locals: HashMap<PatId, PatId>,
}

struct Comparison<'a, 'db> {
    authored: &'db DriverDataBase,
    output: &'db DriverDataBase,
    request: &'a FrozenGeneration,
    code: &'a FrozenCode,
    target: hir::analysis::ty::adt_def::AdtRef<'db>,
    body: Body<'db>,
    actual: &'a FrozenCodeNode,
    parameters: Vec<Option<IdentId<'db>>>,
    is_method: bool,
}

impl<'a, 'db> Comparison<'a, 'db> {
    fn frame(&self, node: usize, function: u32) -> Frame<'db> {
        let frozen = &self.code.nodes[node];
        let key = frozen.template.as_ref().expect("nonempty body fragment");
        let owner = owner(self.authored, &key.owner);
        let source = owner.body(self.authored).expect("source owner body");
        let (body, open) = match key.source {
            FrozenTemplateSource::Quote => {
                let quote = quote(self.authored, frozen).expect("quote");
                (quote.template, quote.open.clone())
            }
            FrozenTemplateSource::CheckedBody => (source, Vec::new()),
        };
        Frame {
            node,
            body,
            owner,
            function,
            open,
            locals: HashMap::new(),
        }
    }

    fn field_name(&self, value: &FrozenMetaValue) -> String {
        let FrozenMetaValue::Structural {
            owner,
            kind:
                FrozenHandleKind::Field {
                    variant_index: None,
                    index,
                },
            item: MetaItem::Field,
        } = value
        else {
            panic!("expected field handle: {value:?}");
        };
        assert_eq!(owner, &self.request.target, "field authority");
        let hir::analysis::ty::adt_def::AdtRef::Struct(target) = self.target else {
            panic!("field name requires a selected field list");
        };
        target.hir_fields(self.authored).data(self.authored)[*index]
            .name
            .unwrap_ref()
            .data(self.authored)
            .clone()
    }

    fn field_hole(&self, node: usize, slot: u32) -> String {
        let FrozenCodeHole::Value(value, _) = &self.code.nodes[node].holes[slot as usize] else {
            panic!("field splice is a value");
        };
        self.field_name(value)
    }

    fn parameter(&self, actual: ExprId, expected: usize) {
        let actual = self
            .actual
            .open_uses
            .iter()
            .find(|use_| use_.site == actual)
            .expect("checked parameter use");
        assert_eq!(actual.index as usize, expected, "parameter binding");
    }

    fn generic_offset(&self) -> u32 {
        if self.is_method {
            0
        } else {
            match self.target {
                hir::analysis::ty::adt_def::AdtRef::Struct(owner) => {
                    GenericParamOwner::Struct(owner)
                }
                hir::analysis::ty::adt_def::AdtRef::Enum(owner) => GenericParamOwner::Enum(owner),
            }
            .params(self.authored)
            .count() as u32
        }
    }

    fn type_argument(
        &self,
        frame: &Frame<'db>,
        expected: &FrozenQuoteSignatureType,
        actual: &FrozenQuoteSignatureType,
    ) {
        use FrozenQuoteSignatureType as T;
        match expected {
            T::Hole(slot) => {
                let FrozenCodeHole::Value(value, field_type) =
                    &self.code.nodes[frame.node].holes[*slot as usize]
                else {
                    panic!("type splice is a value");
                };
                if let Some(field_type) = field_type {
                    self.field_type(field_type, actual);
                } else if let FrozenMetaValue::Structural {
                    item: MetaItem::Record,
                    owner,
                    kind: FrozenHandleKind::Record,
                } = value.as_ref()
                {
                    assert_eq!(owner, &self.request.target);
                    self.self_type(actual);
                } else {
                    panic!("uncovered type splice: {value:?}");
                }
            }
            T::SelfType => self.self_type(actual),
            T::FunctionGeneric(index) => {
                assert_eq!(actual, &T::FunctionGeneric(index + self.generic_offset()))
            }
            T::OwnerGeneric(index) => assert_eq!(
                actual,
                &if self.is_method {
                    T::OwnerGeneric(*index)
                } else {
                    T::FunctionGeneric(*index)
                }
            ),
            T::Tuple(parts) => {
                let T::Tuple(actual) = actual else {
                    panic!("expected tuple type")
                };
                assert_eq!(parts.len(), actual.len());
                for (part, actual) in parts.iter().zip(actual) {
                    self.type_argument(frame, part, actual);
                }
            }
            T::Nominal {
                root, arguments, ..
            } => {
                let T::Nominal {
                    root: actual_root,
                    arguments: actual_args,
                    ..
                } = actual
                else {
                    panic!("expected nominal type")
                };
                assert_eq!(root, actual_root, "nominal identity");
                assert_eq!(arguments.len(), actual_args.len());
                for (part, actual) in arguments.iter().zip(actual_args) {
                    self.type_argument(frame, part, actual);
                }
            }
            T::Ptr(inner) => {
                let T::Ptr(actual) = actual else {
                    panic!("expected pointer type")
                };
                self.type_argument(frame, inner, actual);
            }
            T::Mode(mode, inner) => {
                let T::Mode(actual_mode, actual) = actual else {
                    panic!("expected mode type")
                };
                assert_eq!(mode, actual_mode);
                self.type_argument(frame, inner, actual);
            }
            T::Primitive(_) | T::Never | T::Trait { .. } => {
                assert_eq!(expected, actual, "checked type argument")
            }
        }
    }

    fn field_type(&self, expected: &FrozenQuoteFieldType, actual: &FrozenQuoteSignatureType) {
        use FrozenQuoteFieldType as F;
        use FrozenQuoteSignatureType as T;
        match expected {
            F::TargetParam(index) => assert_eq!(
                actual,
                &if self.is_method {
                    T::OwnerGeneric(*index as u32)
                } else {
                    T::FunctionGeneric(*index as u32)
                }
            ),
            F::Primitive(primitive) => assert_eq!(actual, &T::Primitive(*primitive)),
            F::Tuple(parts) => {
                let T::Tuple(actual) = actual else {
                    panic!("expected tuple field type")
                };
                assert_eq!(parts.len(), actual.len());
                for (part, actual) in parts.iter().zip(actual) {
                    self.field_type(part, actual);
                }
            }
            F::Ptr(inner) => {
                let T::Ptr(actual) = actual else {
                    panic!("expected pointer field type")
                };
                self.field_type(inner, actual);
            }
        }
    }

    fn self_type(&self, actual: &FrozenQuoteSignatureType) {
        let FrozenQuoteSignatureType::Nominal {
            root, arguments, ..
        } = actual
        else {
            panic!("Self must denote target record")
        };
        assert_eq!(root.owner.file, self.request.target.file);
        assert!(
            root.owner.path == self.request.target.path
                || root
                    .owner
                    .path
                    .ends_with(&format!("::{}", self.request.target.path)),
            "target nominal identity"
        );
        let count = match self.target {
            hir::analysis::ty::adt_def::AdtRef::Struct(owner) => GenericParamOwner::Struct(owner),
            hir::analysis::ty::adt_def::AdtRef::Enum(owner) => GenericParamOwner::Enum(owner),
        }
        .params(self.authored)
        .count();
        assert_eq!(count, arguments.len());
        for (index, actual) in arguments.iter().enumerate() {
            self.field_type(&FrozenQuoteFieldType::TargetParam(index), actual);
        }
    }

    fn variant(
        &self,
        frame: &Frame<'db>,
        expected: FrozenQuoteConstructorSite,
        actual: FrozenQuoteConstructorSite,
    ) {
        let expected = self.code.nodes[frame.node]
            .rooted_constructors
            .iter()
            .find(|root| {
                root.site == expected && (root.func.is_none() || root.func == Some(frame.function))
            })
            .expect("captured enum variant");
        let actual = self
            .actual
            .rooted_constructors
            .iter()
            .find(|root| root.site == actual)
            .expect("checked output enum variant");
        assert_eq!(
            expected.owner.owner, actual.owner.owner,
            "selected constructor owner identity"
        );
        assert_eq!(
            expected.variant_index, actual.variant_index,
            "selected variant index"
        );
    }

    fn path(&self, frame: &Frame<'db>, expected: ExprId, actual: ExprId) {
        let node = &self.code.nodes[frame.node];
        if node.rooted_constructors.iter().any(|root| {
            root.site == FrozenQuoteConstructorSite::Expr(expected)
                && (root.func.is_none() || root.func == Some(frame.function))
        }) {
            self.variant(
                frame,
                FrozenQuoteConstructorSite::Expr(expected),
                FrozenQuoteConstructorSite::Expr(actual),
            );
            return;
        }
        if let Some(use_) = node
            .param_uses
            .iter()
            .find(|use_| use_.site == expected && use_.func == frame.function)
        {
            self.parameter(actual, use_.index as usize);
        } else if node
            .self_uses
            .iter()
            .any(|use_| use_.site == expected && use_.func == frame.function)
            || node.free_self_uses.contains(&expected)
        {
            self.parameter(actual, 0);
        } else if let Some(use_) = node.open_uses.iter().find(|use_| use_.site == expected) {
            let key = node.template.as_ref().unwrap();
            let index = if key.source == FrozenTemplateSource::CheckedBody {
                use_.index as usize
            } else {
                let name = frame.open[use_.index as usize];
                self.parameters
                    .iter()
                    .position(|param| param == &Some(name))
                    .expect("explicit open name binds a destination parameter")
            };
            self.parameter(actual, index);
        } else if let Some(use_) = node.generic_uses.iter().find(|use_| {
            use_.site == expected && use_.func == frame.function && use_.where_predicate.is_none()
        }) {
            let actual = self
                .actual
                .generic_uses
                .iter()
                .find(|use_| use_.site == actual)
                .expect("checked generic use");
            assert_eq!(actual.index, use_.index + self.generic_offset());
        } else if let Some(local) = node
            .locals
            .iter()
            .find(|local| local.func == frame.function && local.uses.contains(&expected))
        {
            let binder = frame
                .locals
                .get(&local.binder)
                .expect("source use follows its binder");
            assert!(
                self.actual
                    .locals
                    .iter()
                    .any(|local| local.binder == *binder && local.uses.contains(&actual)),
                "local use retains its binder"
            );
        } else if let Some(root) = node
            .rooted_funcs
            .iter()
            .find(|root| root.site == expected && root.where_predicate.is_none())
        {
            let actual = self
                .actual
                .rooted_funcs
                .iter()
                .find(|root| root.site == actual)
                .expect("checked rooted function");
            assert_eq!(root.owner, actual.owner, "selected function identity");
            match (&root.type_arguments, &actual.type_arguments) {
                (None, None) => {}
                (Some(expected), Some(actual)) => {
                    assert_eq!(expected.len(), actual.len());
                    for (expected, actual) in expected.iter().zip(actual) {
                        self.type_argument(frame, expected, actual);
                    }
                }
                other => panic!("explicit type arguments changed: {other:?}"),
            }
        } else if let Some(constant) = node
            .qualified_consts
            .iter()
            .find(|use_| use_.site == expected && use_.func == frame.function)
        {
            let actual_constant = self
                .actual
                .qualified_consts
                .iter()
                .find(|use_| use_.site == actual)
                .expect("checked qualified constant");
            let expected_trait = node
                .rooted_traits
                .iter()
                .find(|root| root.site == constant.trait_site)
                .unwrap();
            let actual_trait = self
                .actual
                .rooted_traits
                .iter()
                .find(|root| root.site == actual_constant.trait_site)
                .unwrap();
            assert_eq!(
                expected_trait.owner, actual_trait.owner,
                "constant trait identity"
            );
            assert_eq!(constant.receiver, actual_constant.receiver);
            let Expr::Path(Partial::Present(expected)) =
                frame.body.exprs(self.authored)[expected].unwrap_ref()
            else {
                unreachable!()
            };
            let Expr::Path(Partial::Present(actual)) =
                self.body.exprs(self.output)[actual].unwrap_ref()
            else {
                unreachable!()
            };
            assert_eq!(
                expected
                    .ident(self.authored)
                    .unwrap_ref()
                    .data(self.authored),
                actual.ident(self.output).unwrap_ref().data(self.output)
            );
        } else {
            panic!("unclassified template path: {expected:?}");
        }
    }

    fn scalar(&self, value: &FrozenMetaValue, actual: ExprId) {
        let FrozenMetaValue::Scalar { value, primitive } = value else {
            panic!("scalar expression splice")
        };
        let actual = unwrapped(self.output, self.body, actual);
        let literal = match value {
            SemConstScalar::Bool(_) => actual,
            SemConstScalar::Int { .. } => {
                let Expr::Cast(inner, _) = self.body.exprs(self.output)[actual].unwrap_ref() else {
                    panic!("integer splice must preserve its type")
                };
                let target = self
                    .actual
                    .cast_primitives
                    .iter()
                    .find(|(site, _)| *site == actual)
                    .unwrap();
                assert_eq!(Some(target.1), *primitive, "lifted scalar type");
                *inner
            }
            other => panic!("unsupported expression scalar: {other:?}"),
        };
        match (value, self.body.exprs(self.output)[literal].unwrap_ref()) {
            (SemConstScalar::Bool(expected), Expr::Lit(LitKind::Bool(actual))) => {
                assert_eq!(expected, actual)
            }
            (SemConstScalar::Int { value }, Expr::Lit(LitKind::Int(actual))) => {
                assert_eq!(value.to_string(), actual.data(self.output).to_string())
            }
            other => panic!("scalar splice changed kind: {other:?}"),
        }
    }

    fn expr(&self, frame: &mut Frame<'db>, expected: ExprId, actual: ExprId) {
        let expected = unwrapped(self.authored, frame.body, expected);
        let actual = unwrapped(self.output, self.body, actual);
        let source = frame.body.exprs(self.authored)[expected].unwrap_ref();
        let output = self.body.exprs(self.output)[actual].unwrap_ref();
        if let Expr::Hole(slot) = source {
            match &self.code.nodes[frame.node].holes[*slot as usize] {
                FrozenCodeHole::Code(child) => {
                    let mut child_frame = self.frame(*child, 0);
                    let root = child_frame.body.expr(self.authored);
                    self.expr(&mut child_frame, root, actual);
                }
                FrozenCodeHole::Value(value, _) => self.scalar(value, actual),
            }
            return;
        }
        match (source, output) {
            (Expr::Lit(expected), Expr::Lit(actual)) => assert_eq!(
                expected.pretty_print(self.authored),
                actual.pretty_print(self.output)
            ),
            (Expr::Path(_), Expr::Path(_)) => self.path(frame, expected, actual),
            (Expr::Bin(lhs, rhs, op), Expr::Bin(a, b, actual_op)) => {
                assert_eq!(op, actual_op);
                self.expr(frame, *lhs, *a);
                self.expr(frame, *rhs, *b);
            }
            (Expr::AugAssign(lhs, rhs, op), Expr::AugAssign(a, b, actual_op)) => {
                assert_eq!(op, actual_op);
                self.expr(frame, *lhs, *a);
                self.expr(frame, *rhs, *b);
            }
            (Expr::Assign(lhs, rhs), Expr::Assign(a, b)) => {
                self.expr(frame, *lhs, *a);
                self.expr(frame, *rhs, *b);
            }
            (Expr::Un(value, op), Expr::Un(actual, actual_op)) => {
                assert_eq!(op, actual_op);
                self.expr(frame, *value, *actual);
            }
            (Expr::Cast(value, Partial::Present(ty)), Expr::Cast(actual_value, _)) => {
                let TypeKind::Primitive(primitive) = ty.data(self.authored) else {
                    panic!("cast requires a checked primitive")
                };
                let target = self
                    .actual
                    .cast_primitives
                    .iter()
                    .find(|(site, _)| *site == actual)
                    .unwrap();
                assert_eq!(
                    primitive.name(self.authored).data(self.authored),
                    &format!("{:?}", target.1).to_ascii_lowercase()
                );
                self.expr(frame, *value, *actual_value);
            }
            (Expr::Call(callee, args), Expr::Call(actual_callee, actual_args)) => {
                self.expr(frame, *callee, *actual_callee);
                assert_eq!(args.len(), actual_args.len());
                for (arg, actual) in args.iter().zip(actual_args) {
                    assert_eq!(
                        arg.label_eagerly(self.authored, frame.body)
                            .map(|name| name.data(self.authored)),
                        actual
                            .label_eagerly(self.output, self.body)
                            .map(|name| name.data(self.output))
                    );
                    self.expr(frame, arg.expr, actual.expr);
                }
            }
            (
                Expr::MethodCall(receiver, name, args, values),
                Expr::MethodCall(actual_receiver, actual_name, actual_args, actual_values),
            ) => {
                assert_eq!(
                    name.unwrap_ref().data(self.authored),
                    actual_name.unwrap_ref().data(self.output)
                );
                assert_eq!(
                    args.is_given(self.authored),
                    actual_args.is_given(self.output)
                );
                let (_, selected) = self
                    .actual
                    .selected_methods
                    .iter()
                    .find(|(site, _)| *site == actual)
                    .expect("read-back selected method");
                if let Some((_, expected)) = self.code.nodes[frame.node]
                    .selected_methods
                    .iter()
                    .find(|(site, _)| *site == expected)
                {
                    assert_eq!(expected, selected, "selected method identity");
                }
                let (_, scope) = self
                    .body
                    .quote_method_scopes(self.output)
                    .iter()
                    .find(|(site, _)| *site == actual)
                    .expect("retained lexical method scope");
                let ItemKind::Func(scope_owner) = scope.item() else {
                    panic!("method scope is the template owner")
                };
                assert_eq!(
                    scope_owner.frozen_locator(self.output),
                    frame.owner.frozen_locator(self.authored)
                );
                self.expr(frame, *receiver, *actual_receiver);
                assert_eq!(values.len(), actual_values.len());
                for (value, actual) in values.iter().zip(actual_values) {
                    assert_eq!(
                        value
                            .label_eagerly(self.authored, frame.body)
                            .map(|name| name.data(self.authored)),
                        actual
                            .label_eagerly(self.output, self.body)
                            .map(|name| name.data(self.output))
                    );
                    self.expr(frame, value.expr, actual.expr);
                }
            }
            (Expr::Field(base, field), Expr::Field(actual_base, actual_field)) => {
                let expected = match field.unwrap_ref() {
                    FieldIndex::Ident(name) => name.data(self.authored).clone(),
                    FieldIndex::Index(index) => index.data(self.authored).to_string(),
                    FieldIndex::Hole(slot) => self.field_hole(frame.node, *slot),
                };
                let actual = match actual_field.unwrap_ref() {
                    FieldIndex::Ident(name) => name.data(self.output).clone(),
                    FieldIndex::Index(index) => index.data(self.output).to_string(),
                    FieldIndex::Hole(_) => panic!("output still contains field hole"),
                };
                assert_eq!(expected, actual, "field identity");
                self.expr(frame, *base, *actual_base);
            }
            (Expr::Tuple(values), Expr::Tuple(actual))
            | (Expr::Array(values), Expr::Array(actual)) => {
                assert_eq!(values.len(), actual.len());
                for (value, actual) in values.iter().zip(actual) {
                    self.expr(frame, *value, *actual);
                }
            }
            (Expr::RecordInit(_, fields), Expr::RecordInit(_, actual_fields)) => {
                if self.code.nodes[frame.node]
                    .rooted_constructors
                    .iter()
                    .any(|root| {
                        root.site == FrozenQuoteConstructorSite::Expr(expected)
                            && (root.func.is_none() || root.func == Some(frame.function))
                    })
                {
                    self.variant(
                        frame,
                        FrozenQuoteConstructorSite::Expr(expected),
                        FrozenQuoteConstructorSite::Expr(actual),
                    );
                } else {
                    let (_, actual_type) = self
                        .actual
                        .record_types
                        .iter()
                        .find(|(site, _)| *site == actual)
                        .expect("checked constructor type");
                    let node = &self.code.nodes[frame.node];
                    if node
                        .self_record_inits
                        .iter()
                        .any(|use_| use_.site == expected && use_.func == frame.function)
                    {
                        self.self_type(actual_type);
                    } else {
                        let (_, expected_type) = node
                            .record_types
                            .iter()
                            .find(|(site, _)| *site == expected)
                            .expect("captured constructor type");
                        self.type_argument(frame, expected_type, actual_type);
                    }
                }
                assert_eq!(fields.len(), actual_fields.len());
                for (field, actual) in fields.iter().zip(actual_fields) {
                    let expected = field
                        .label_hole
                        .map(|slot| self.field_hole(frame.node, slot))
                        .or_else(|| {
                            field
                                .label_eagerly(self.authored, frame.body)
                                .map(|name| name.data(self.authored).clone())
                        });
                    assert_eq!(
                        expected,
                        actual
                            .label_eagerly(self.output, self.body)
                            .map(|name| name.data(self.output).clone())
                    );
                    self.expr(frame, field.expr, actual.expr);
                }
            }
            (Expr::If(condition, then, otherwise), Expr::If(a, b, c)) => {
                self.cond(frame, *condition, *a);
                self.expr(frame, *then, *b);
                match (otherwise, c) {
                    (Some(value), Some(actual)) => self.expr(frame, *value, *actual),
                    (None, None) => {}
                    _ => panic!("if alternatives changed"),
                }
            }
            (
                Expr::Match(scrutinee, Partial::Present(arms)),
                Expr::Match(actual_scrutinee, Partial::Present(actual_arms)),
            ) => {
                self.expr(frame, *scrutinee, *actual_scrutinee);
                assert_eq!(arms.len(), actual_arms.len());
                for (arm, actual) in arms.iter().zip(actual_arms) {
                    self.pat(frame, arm.pat, actual.pat);
                    self.expr(frame, arm.body, actual.body);
                }
            }
            (Expr::Block(stmts), Expr::Block(actual)) => {
                assert_eq!(stmts.len(), actual.len());
                for (stmt, actual) in stmts.iter().zip(actual) {
                    self.stmt(frame, *stmt, *actual);
                }
            }
            other => panic!("template/output expression mismatch or uncovered form: {other:?}"),
        }
    }

    fn cond(&self, frame: &mut Frame<'db>, expected: CondId, actual: CondId) {
        match (
            frame.body.conds(self.authored)[expected].unwrap_ref(),
            self.body.conds(self.output)[actual].unwrap_ref(),
        ) {
            (Cond::Expr(expected), Cond::Expr(actual)) => self.expr(frame, *expected, *actual),
            (Cond::Bin(lhs, rhs, op), Cond::Bin(a, b, actual_op)) => {
                assert_eq!(op, actual_op);
                self.cond(frame, *lhs, *a);
                self.cond(frame, *rhs, *b);
            }
            other => panic!("condition changed or uncovered form: {other:?}"),
        }
    }

    fn stmt(&self, frame: &mut Frame<'db>, expected: StmtId, actual: StmtId) {
        match (
            frame.body.stmts(self.authored)[expected].unwrap_ref(),
            self.body.stmts(self.output)[actual].unwrap_ref(),
        ) {
            (Stmt::Let(pat, ty, value), Stmt::Let(actual_pat, actual_ty, actual_value)) => {
                assert_eq!(ty.is_some(), actual_ty.is_some(), "annotation presence");
                if ty.is_some() {
                    let expected_type = &self.code.nodes[frame.node]
                        .local_types
                        .iter()
                        .find(|local| local.stmt == expected && local.func == frame.function)
                        .expect("frozen annotation")
                        .ty;
                    let actual_type = &self
                        .actual
                        .local_types
                        .iter()
                        .find(|local| local.stmt == actual)
                        .expect("checked annotation")
                        .ty;
                    self.type_argument(frame, expected_type, actual_type);
                }
                self.pat(frame, *pat, *actual_pat);
                match (value, actual_value) {
                    (Some(value), Some(actual)) => self.expr(frame, *value, *actual),
                    (None, None) => {}
                    _ => panic!("let initializer changed"),
                }
            }
            (Stmt::Expr(expected), Stmt::Expr(actual)) => self.expr(frame, *expected, *actual),
            (Stmt::Return(expected), Stmt::Return(actual)) => match (expected, actual) {
                (Some(value), Some(actual)) => self.expr(frame, *value, *actual),
                (None, None) => {}
                _ => panic!("return changed"),
            },
            (Stmt::While(condition, block), Stmt::While(actual_condition, actual_block)) => {
                self.cond(frame, *condition, *actual_condition);
                self.expr(frame, *block, *actual_block);
            }
            (
                Stmt::For(pat, iterable, block, unroll),
                Stmt::For(actual_pat, actual_iterable, actual_block, actual_unroll),
            ) => {
                assert_eq!(unroll, actual_unroll);
                self.expr(frame, *iterable, *actual_iterable);
                self.pat(frame, *pat, *actual_pat);
                self.expr(frame, *block, *actual_block);
            }
            (Stmt::Break, Stmt::Break) | (Stmt::Continue, Stmt::Continue) => {}
            other => panic!("statement changed or uncovered form: {other:?}"),
        }
    }

    fn pat(&self, frame: &mut Frame<'db>, expected: PatId, actual: PatId) {
        match (
            frame.body.pats(self.authored)[expected].unwrap_ref(),
            self.body.pats(self.output)[actual].unwrap_ref(),
        ) {
            (Pat::Path(_, mutable), Pat::Path(_, actual_mutable))
                if self.code.nodes[frame.node]
                    .rooted_constructors
                    .iter()
                    .any(|root| {
                        root.site == FrozenQuoteConstructorSite::Pat(expected)
                            && (root.func.is_none() || root.func == Some(frame.function))
                    }) =>
            {
                assert_eq!(mutable, actual_mutable);
                self.variant(
                    frame,
                    FrozenQuoteConstructorSite::Pat(expected),
                    FrozenQuoteConstructorSite::Pat(actual),
                );
            }
            (Pat::PathTuple(_, parts), Pat::PathTuple(_, actual_parts)) => {
                self.variant(
                    frame,
                    FrozenQuoteConstructorSite::Pat(expected),
                    FrozenQuoteConstructorSite::Pat(actual),
                );
                assert_eq!(parts.len(), actual_parts.len());
                for (part, actual) in parts.iter().zip(actual_parts) {
                    self.pat(frame, *part, *actual);
                }
            }
            (Pat::Record(_, fields), Pat::Record(_, actual_fields)) => {
                self.variant(
                    frame,
                    FrozenQuoteConstructorSite::Pat(expected),
                    FrozenQuoteConstructorSite::Pat(actual),
                );
                assert_eq!(fields.len(), actual_fields.len());
                for (field, actual) in fields.iter().zip(actual_fields) {
                    assert_eq!(
                        field
                            .label(self.authored, frame.body)
                            .map(|name| name.data(self.authored)),
                        actual
                            .label(self.output, self.body)
                            .map(|name| name.data(self.output))
                    );
                    self.pat(frame, field.pat, actual.pat);
                }
            }
            (Pat::Path(_, mutable), Pat::Path(_, actual_mutable)) => {
                assert_eq!(mutable, actual_mutable);
                assert!(
                    self.code.nodes[frame.node]
                        .locals
                        .iter()
                        .any(|local| local.func == frame.function && local.binder == expected)
                );
                assert!(
                    self.actual
                        .locals
                        .iter()
                        .any(|local| local.binder == actual)
                );
                assert!(
                    frame.locals.insert(expected, actual).is_none(),
                    "unique source binder"
                );
            }
            (Pat::Tuple(parts), Pat::Tuple(actual)) => {
                assert_eq!(parts.len(), actual.len());
                for (part, actual) in parts.iter().zip(actual) {
                    self.pat(frame, *part, *actual);
                }
            }
            (Pat::WildCard, Pat::WildCard) | (Pat::Rest, Pat::Rest) => {}
            (Pat::Lit(Partial::Present(literal)), Pat::Lit(Partial::Present(actual))) => {
                assert_eq!(
                    literal.pretty_print(self.authored),
                    actual.pretty_print(self.output)
                );
            }
            (Pat::Or(lhs, rhs), Pat::Or(actual_lhs, actual_rhs)) => {
                self.pat(frame, *lhs, *actual_lhs);
                self.pat(frame, *rhs, *actual_rhs);
            }
            other => panic!("pattern changed or uncovered form: {other:?}"),
        }
    }
}

fn text(value: &FrozenMetaValue) -> String {
    let FrozenMetaValue::Scalar {
        value: SemConstScalar::Bytes(bytes),
        ..
    } = value
    else {
        panic!("name is a fixed string")
    };
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[first..]).unwrap().to_owned()
}

fn name<'db>(
    db: &'db DriverDataBase,
    target: hir::analysis::ty::adt_def::AdtRef<'db>,
    request: &FrozenGeneration,
    node: &FrozenCodeNode,
    name: QuoteName<'db>,
) -> String {
    match name {
        QuoteName::Ident(name) => name.unwrap_ref().data(db).clone(),
        QuoteName::Hole(slot) => {
            let FrozenCodeHole::Value(value, _) = &node.holes[slot as usize] else {
                panic!("name splice")
            };
            let field = |value: &FrozenMetaValue| {
                let FrozenMetaValue::Structural {
                    owner,
                    kind:
                        FrozenHandleKind::Field {
                            variant_index: None,
                            index,
                        },
                    item: MetaItem::Field,
                } = value
                else {
                    panic!("field name")
                };
                assert_eq!(owner, &request.target);
                let hir::analysis::ty::adt_def::AdtRef::Struct(target) = target else {
                    panic!("field name requires a selected field list");
                };
                target.hir_fields(db).data(db)[*index]
                    .name
                    .unwrap_ref()
                    .data(db)
                    .clone()
            };
            match value.as_ref() {
                FrozenMetaValue::Structural {
                    item: MetaItem::Field,
                    ..
                } => field(value),
                FrozenMetaValue::Variant {
                    item: MetaItem::DeclarationName,
                    case: FrozenMetaVariantCase::DeclarationNameLiteral,
                    fields,
                    ..
                } => text(&fields[0]),
                FrozenMetaValue::Variant {
                    item: MetaItem::DeclarationName,
                    case: FrozenMetaVariantCase::DeclarationNameFieldPrefix,
                    fields,
                    ..
                } => format!("{}{}", text(&fields[0]), field(&fields[1])),
                other => panic!("uncovered generated name: {other:?}"),
            }
        }
    }
}

pub fn assert_round_trip<'db>(
    authored: &'db DriverDataBase,
    output: &'db DriverDataBase,
    url: &Url,
) {
    let input = HirDb::generated_module(output, url).expect("frozen generation");
    assert_round_trip_against(authored, output, url, input.requests(output));
}

pub fn assert_round_trip_against<'db>(
    authored: &'db DriverDataBase,
    output: &'db DriverDataBase,
    url: &Url,
    requests: &[FrozenGeneration],
) {
    let file = output.workspace().get(output, url).expect("output file");
    let module = output.top_mod(file);
    let generated = module
        .all_funcs(output)
        .iter()
        .copied()
        .filter(|func| {
            matches!(
                func.origin(output),
                HirOrigin::Desugared(DesugaredOrigin::Generation(_))
            )
        })
        .collect::<Vec<_>>();
    let mut compared = HashSet::new();
    for request in requests {
        let FrozenMetaValue::Code(code) = &request.value else {
            panic!("quotation corpus uses code results")
        };
        let source_file = authored
            .workspace()
            .get(authored, &request.target.file)
            .unwrap();
        let targets = authored
            .top_mod(source_file)
            .all_items(authored)
            .iter()
            .filter_map(|item| {
                let target = hir::analysis::ty::adt_def::AdtRef::try_from_item(*item)?;
                let path = target.scope().pretty_path(authored)?;
                (path == request.target.path
                    || path.ends_with(&format!("::{}", request.target.path)))
                .then_some(target)
            })
            .collect::<Vec<_>>();
        assert_eq!(targets.len(), 1, "unique frozen target");
        let target = targets[0];
        let attribute_range = match target {
            hir::analysis::ty::adt_def::AdtRef::Struct(target) => target
                .span()
                .attributes()
                .attr(request.attribute_index)
                .resolve(authored),
            hir::analysis::ty::adt_def::AdtRef::Enum(target) => target
                .span()
                .attributes()
                .attr(request.attribute_index)
                .resolve(authored),
        }
        .expect("authored generation attribute")
        .range;
        let mut functions = Vec::new();
        collect_functions(authored, code, code.root, &mut functions);
        for (node_index, function_index, quoted, is_method) in functions {
            let expected_name = name(
                authored,
                target,
                request,
                &code.nodes[node_index],
                quoted.name,
            );
            let candidates = generated
                .iter()
                .copied()
                .filter(|func| {
                    let HirOrigin::Desugared(DesugaredOrigin::Generation(origin)) =
                        func.origin(output)
                    else {
                        return false;
                    };
                    origin.attribute.syntax_node_ptr().text_range() == attribute_range
                        && func.name(output).unwrap_ref().data(output) == &expected_name
                })
                .collect::<Vec<_>>();
            assert_eq!(
                candidates.len(),
                1,
                "unique generated function {expected_name}"
            );
            let function = candidates[0];
            assert!(
                compared.insert(function),
                "every generated body compared once"
            );
            assert_eq!(
                quoted.effects.data(authored).len(),
                function.effect_params(output).count(),
                "effect header"
            );
            if function.has_effects(output) {
                assert_eq!(read_back(output, function), Err(ReadBackError::Effectful));
                continue;
            }
            let reflected = read_back(output, function).expect("generated body reads back");
            let actual = &reflected.nodes[reflected.root];
            let parameters = quoted
                .params
                .unwrap_ref()
                .data(authored)
                .iter()
                .map(|param| param.name())
                .collect();
            let comparison = Comparison {
                authored,
                output,
                request,
                code,
                target,
                body: function.body(output).unwrap(),
                actual,
                parameters,
                is_method,
            };
            let mut frame = comparison.frame(node_index, function_index);
            comparison.expr(
                &mut frame,
                quoted.body.expect("quoted function body"),
                comparison.body.expr(output),
            );
        }
    }
    assert_eq!(
        compared.len(),
        generated.len(),
        "all generated bodies were compared or explicitly rejected as effectful"
    );
    println!("{} generated bodies compared", compared.len());
}

fn collect_functions<'db>(
    db: &'db DriverDataBase,
    code: &FrozenCode,
    index: usize,
    functions: &mut Vec<(usize, u32, &'db QuoteFunc<'db>, bool)>,
) {
    let Some(quote) = quote(db, &code.nodes[index]) else {
        return;
    };
    let items = match &quote.fragment {
        QuoteFragment::Items(items) => items,
        QuoteFragment::Expr => {
            for slot in quote.splice_only_body(db).expect("item-only fragment") {
                let FrozenCodeHole::Code(child) = code.nodes[index].holes[slot as usize] else {
                    panic!("item splice is code");
                };
                collect_functions(db, code, child, functions);
            }
            return;
        }
        QuoteFragment::Where(_) => panic!("item corpus cannot emit a where fragment"),
    };
    let mut function = 0;
    for item in items {
        match item {
            QuoteItem::Func(quoted) => {
                functions.push((index, function, quoted, false));
                function += 1;
            }
            QuoteItem::Impl(implementation) => {
                for (function, quoted) in implementation
                    .items
                    .iter()
                    .filter_map(|item| {
                        if let QuoteItem::Func(func) = item {
                            Some(func)
                        } else {
                            None
                        }
                    })
                    .enumerate()
                {
                    functions.push((index, function as u32, quoted, true));
                }
            }
            QuoteItem::Hole(slot) => {
                let FrozenCodeHole::Code(child) = code.nodes[index].holes[*slot as usize] else {
                    panic!("item splice is code");
                };
                collect_functions(db, code, child, functions);
            }
            QuoteItem::Const(_) | QuoteItem::Type(_) => panic!("standalone associated item"),
        }
    }
}
