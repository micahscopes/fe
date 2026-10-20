//! Bounded structural input for checked generation.
//!
//! These handles retain existing semantic type and declaration identities.
//! They are not a context, a proof record, or a second reflection database.
//! Output publication must happen after provider evaluation has finished.

use parser::ast::{self, prelude::*};
use salsa::Update;

use crate::{
    analysis::{HirAnalysisDb, ty::ty_def::TyId},
    hir_def::IdentId,
    span::HirOrigin,
};

/// Search existing semantic type arguments and instantiated fields. Repeated
/// nominal expansion is stopped independently from argument traversal.
pub fn ty_contains_meta_carrier<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<TyId<'db>> {
    let mut pending = vec![ty];
    let mut seen = rustc_hash::FxHashSet::default();
    let mut expanded = rustc_hash::FxHashSet::default();
    while let Some(ty) = pending.pop() {
        if !seen.insert(ty) {
            continue;
        }
        if is_canonical_meta_carrier_ty(db, ty) {
            return Some(ty);
        }
        pending.extend(ty.generic_args(db).iter().copied());
        if let Some(adt) = ty.adt_def(db)
            && expanded.insert(adt)
        {
            for (variant, fields) in adt.fields(db).iter().enumerate() {
                pending.extend((0..fields.num_types()).map(|field| {
                    crate::analysis::ty::adt_def::instantiate_adt_field_shape(
                        db,
                        adt,
                        variant,
                        field,
                        ty.generic_args(db),
                    )
                }));
            }
        }
    }
    None
}

/// Completed compile-time aggregates must contain genuine issued tokens at
/// every metadata leaf. Ordinary seal-shaped structs never substitute for them.
pub(crate) fn issued_metadata_leaves<'db>(
    db: &'db dyn HirAnalysisDb,
    value: crate::analysis::semantic::SemConstId<'db>,
) -> bool {
    use crate::analysis::semantic::SemConstValue;
    if is_canonical_meta_carrier_ty(db, crate::analysis::semantic::sem_const_ty(db, value)) {
        return matches!(
            value.value(db),
            SemConstValue::Meta { .. } | SemConstValue::ItemCode { .. }
        );
    }
    match value.value(db) {
        SemConstValue::Tuple { elems, .. } | SemConstValue::Array { elems, .. } => {
            elems.iter().all(|value| issued_metadata_leaves(db, *value))
        }
        SemConstValue::Struct { fields, .. } | SemConstValue::Enum { fields, .. } => fields
            .iter()
            .all(|value| issued_metadata_leaves(db, *value)),
        SemConstValue::Description(..) => false,
        _ => true,
    }
}

/// Canonical compile-time carriers are opaque checking tokens, not their
/// private source seal representation. Authority is the actual library ingot.
pub fn is_canonical_meta_carrier_ty<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
    let ty = ty.as_view(db).unwrap_or(ty);
    let (base, _) = ty.decompose_ty_app(db);
    let crate::analysis::ty::ty_def::TyData::TyBase(crate::analysis::ty::ty_def::TyBase::Adt(adt)) =
        base.data(db)
    else {
        return false;
    };
    let scope = adt.scope(db);
    [
        "core::meta::Record",
        "core::meta::Field",
        "core::meta::FieldType",
        "core::meta::Code",
    ]
    .iter()
    .any(|path| crate::analysis::ty::corelib::resolve_lib_type_path(db, scope, path) == Some(base))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub struct RecordInput<'db> {
    ty: TyId<'db>,
    declaration: crate::analysis::ty::adt_def::AdtRef<'db>,
    variant: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub struct RecordField<'db> {
    record: RecordInput<'db>,
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum MetaValue<'db> {
    Record(RecordInput<'db>),
    Field(RecordField<'db>),
    FieldType(RecordField<'db>),
}

impl<'db> MetaValue<'db> {
    pub fn input(self) -> RecordInput<'db> {
        match self {
            Self::Record(record) => record,
            Self::Field(field) | Self::FieldType(field) => field.record(),
        }
    }

    pub(crate) fn carrier_path(self) -> &'static str {
        match self {
            Self::Record(_) => "core::meta::Record",
            Self::Field(_) => "core::meta::Field",
            Self::FieldType(_) => "core::meta::FieldType",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordInputError {
    NotConcrete,
    NotRecord,
    NotAuthored,
    NotEnum,
    WholeEnumHasNoFields,
    InvalidField,
    FieldOutsideRecord,
    VariantOutsideEnum,
    SelectedPayloadIsNotWholeEnum,
}

impl<'db> RecordInput<'db> {
    /// Identify the annotation's actual declaration in its owning source module.
    pub fn for_authored_syntax(
        db: &'db dyn HirAnalysisDb,
        module: crate::hir_def::TopLevelMod<'db>,
        syntax: &ast::Struct,
    ) -> Result<Self, RecordInputError> {
        let origin = HirOrigin::raw(syntax);
        let declaration = module
            .all_structs(db)
            .iter()
            .find(|decl| *decl.origin(db) == origin)
            .ok_or(RecordInputError::NotAuthored)?;
        let adt = crate::analysis::ty::adt_def::AdtRef::Struct(*declaration).as_adt(db);
        Self::for_authored_type(db, TyId::adt(db, adt))
    }

    /// Issue a view of an already resolved, concrete authored record.
    /// The driver supplies the target of the authored generation request.
    /// No Fe constructor issues this handle.
    pub fn for_authored_type(
        db: &'db dyn HirAnalysisDb,
        ty: TyId<'db>,
    ) -> Result<Self, RecordInputError> {
        if !ty.is_star_kind(db)
            || !crate::analysis::ty::const_ty::ty_is_fully_ground(db, ty)
            || ty.has_invalid(db)
        {
            return Err(RecordInputError::NotConcrete);
        }
        let adt = ty.adt_def(db).ok_or(RecordInputError::NotRecord)?;
        let declaration = adt.adt_ref(db);
        let authored = match declaration {
            crate::analysis::ty::adt_def::AdtRef::Struct(item) => {
                matches!(item.origin(db), HirOrigin::Raw(_))
            }
            crate::analysis::ty::adt_def::AdtRef::Enum(item) => {
                matches!(item.origin(db), HirOrigin::Raw(_))
            }
        };
        if !authored {
            return Err(RecordInputError::NotAuthored);
        }
        if adt.fields(db).iter().enumerate().any(|(variant, fields)| {
            (0..fields.num_types()).any(|field| {
                crate::analysis::ty::adt_def::instantiate_adt_field_shape(
                    db,
                    adt,
                    variant,
                    field,
                    ty.generic_args(db),
                )
                .has_invalid(db)
            })
        }) {
            return Err(RecordInputError::InvalidField);
        }
        Ok(Self {
            ty,
            declaration,
            variant: None,
        })
    }

    pub fn ty(self) -> TyId<'db> {
        self.ty
    }

    pub fn declaration(self) -> crate::analysis::ty::adt_def::AdtRef<'db> {
        self.declaration
    }

    pub fn field_count(self, db: &'db dyn HirAnalysisDb) -> Result<usize, RecordInputError> {
        match (self.declaration, self.variant) {
            (crate::analysis::ty::adt_def::AdtRef::Struct(declaration), None) => {
                Ok(declaration.hir_fields(db).data(db).len())
            }
            (crate::analysis::ty::adt_def::AdtRef::Enum(_), Some(variant)) => {
                Ok(self.declaration.as_adt(db).fields(db)[variant].num_types())
            }
            _ => Err(RecordInputError::WholeEnumHasNoFields),
        }
    }

    /// Whole authored enum count; selected payloads are outside this checkpoint.
    pub fn variant_count(self, db: &'db dyn HirAnalysisDb) -> Result<usize, RecordInputError> {
        let crate::analysis::ty::adt_def::AdtRef::Enum(declaration) = self.declaration else {
            return Err(RecordInputError::NotEnum);
        };
        if self.variant.is_some() {
            return Err(RecordInputError::SelectedPayloadIsNotWholeEnum);
        }
        Ok(declaration.variants(db).count())
    }

    /// Select an authored payload in declaration order; retain nominal owner.
    pub fn variant(
        self,
        db: &'db dyn HirAnalysisDb,
        index: usize,
    ) -> Result<Self, RecordInputError> {
        if index >= self.variant_count(db)? {
            return Err(RecordInputError::VariantOutsideEnum);
        }
        Ok(Self {
            variant: Some(index),
            ..self
        })
    }

    pub fn selected_variant(self) -> Option<usize> {
        self.variant
    }

    pub(crate) fn validate(self, db: &'db dyn HirAnalysisDb) -> bool {
        let root = Self {
            variant: None,
            ..self
        };
        Self::for_authored_type(db, self.ty) == Ok(root)
            && self
                .variant
                .is_none_or(|index| root.variant(db, index) == Ok(self))
    }

    pub fn for_authored_enum_syntax(
        db: &'db dyn HirAnalysisDb,
        module: crate::hir_def::TopLevelMod<'db>,
        syntax: &ast::Enum,
    ) -> Result<Self, RecordInputError> {
        let origin = HirOrigin::raw(syntax);
        let declaration = module
            .all_enums(db)
            .iter()
            .find(|decl| *decl.origin(db) == origin)
            .ok_or(RecordInputError::NotAuthored)?;
        let adt = crate::analysis::ty::adt_def::AdtRef::Enum(*declaration).as_adt(db);
        Self::for_authored_type(db, TyId::adt(db, adt))
    }

    pub fn field(
        self,
        db: &'db dyn HirAnalysisDb,
        index: usize,
    ) -> Result<RecordField<'db>, RecordInputError> {
        if index >= self.field_count(db)? {
            return Err(RecordInputError::FieldOutsideRecord);
        }
        Ok(RecordField {
            record: self,
            index,
        })
    }
}

impl<'db> RecordField<'db> {
    pub fn record(self) -> RecordInput<'db> {
        self.record
    }

    pub fn index(self) -> usize {
        self.index
    }

    pub fn name(self, db: &'db dyn HirAnalysisDb) -> Option<IdentId<'db>> {
        match self.record.declaration {
            crate::analysis::ty::adt_def::AdtRef::Struct(declaration) => {
                declaration.hir_fields(db).data(db)[self.index]
                    .name
                    .to_opt()
            }
            crate::analysis::ty::adt_def::AdtRef::Enum(declaration) => {
                let variant = declaration
                    .variants(db)
                    .nth(self.record.variant.expect("selected payload field"))
                    .unwrap();
                match variant.kind(db) {
                    crate::hir_def::VariantKind::Record(fields) => {
                        fields.data(db)[self.index].name.to_opt()
                    }
                    _ => None,
                }
            }
        }
    }

    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        crate::analysis::ty::adt_def::instantiate_adt_field_shape(
            db,
            self.record.declaration.as_adt(db),
            self.record.variant.unwrap_or(0),
            self.index,
            self.record.ty.generic_args(db),
        )
    }
}

/// A checked item template in the existing producer instance, with completed
/// scalar splice values. No generated declaration has been published yet.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct ItemCode<'db> {
    key: crate::analysis::semantic::SemanticInstanceKey<'db>,
    expression: crate::hir_def::ExprId,
    holes: Box<[crate::analysis::semantic::SemConstId<'db>]>,
}

/// One bounded source walk shared by producer lowering, construction and
/// output copying. Unsupported nested constructors never lower missing holes.
pub(crate) fn quote_holes(quote: &ast::QuoteExpr) -> Result<Vec<ast::QuoteHoleExpr>, &'static str> {
    let mut holes = Vec::new();
    for (index, node) in quote.syntax().descendants().take(4097).enumerate() {
        if index == 4096 {
            return Err("item quotation exceeds the construction node limit of 4096");
        }
        if ast::QuoteExpr::can_cast(node.kind()) && node != *quote.syntax() {
            return Err(
                "nested quote expressions are not supported by this bounded item quotation",
            );
        }
        if let Some(hole) = ast::QuoteHoleExpr::cast(node.clone()) {
            if node
                .ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor != quote.syntax())
                .any(|ancestor| ast::QuoteHoleExpr::can_cast(ancestor.kind()))
            {
                return Err("nested quotation holes are not supported");
            }
            holes.push(hole);
        }
    }
    Ok(holes)
}

impl<'db> ItemCode<'db> {
    pub(crate) fn for_quote(
        db: &'db dyn HirAnalysisDb,
        key: crate::analysis::semantic::SemanticInstanceKey<'db>,
        expression: crate::hir_def::ExprId,
        holes: Box<[crate::analysis::semantic::SemConstId<'db>]>,
    ) -> Result<Self, &'static str> {
        let value = Self {
            key,
            expression,
            holes,
        };
        value.validate(db)?;
        Ok(value)
    }

    pub fn producer(&self) -> crate::analysis::semantic::SemanticInstanceKey<'db> {
        self.key
    }
    pub fn holes(&self) -> &[crate::analysis::semantic::SemConstId<'db>] {
        &self.holes
    }

    /// Freeze this bounded same-file template for an output database. Scalar
    /// values retain their primitive type; source remains the authored AST.
    pub fn freeze_function(
        &self,
        db: &'db dyn HirAnalysisDb,
        target: crate::File,
    ) -> Result<common::generation::FrozenFunctionTemplate, &'static str> {
        use crate::analysis::semantic::{SemConstScalar, SemConstValue};
        use common::generation::{FrozenFunctionTemplate, FrozenScalar};
        self.validate(db)?;
        let body = self
            .key
            .owner(db)
            .body(db)
            .ok_or("quote producer has no body")?;
        if body.top_mod(db).file(db) != target {
            return Err("bounded generation requires quote and target in the same source file");
        }
        let quote = self.syntax(db)?;
        let ast::ItemKind::Func(function) = quote
            .body()
            .unwrap()
            .items()
            .next()
            .unwrap()
            .kind()
            .unwrap()
        else {
            unreachable!()
        };
        let signature = function.sig();
        let params_valid = match signature.params() {
            None => true,
            Some(params) => {
                let mut iter = params.into_iter();
                match (iter.next(), iter.next()) {
                    (None, _) => true,
                    (Some(param), None) => {
                        matches!(param.name(), Some(ast::FuncParamName::SelfParam(_)))
                            && param.ty().is_none()
                    }
                    _ => false,
                }
            }
        };
        if !params_valid
            || signature.generic_params().is_some()
            || signature.where_clause().is_some()
            || signature.uses_clause().is_some()
        {
            return Err(
                "bounded output copying requires a zero-argument nongeneric function without requirements or effects",
            );
        }
        let producer_block = quote
            .syntax()
            .ancestors()
            .find_map(ast::BlockExpr::cast)
            .map(|block| {
                block
                    .syntax()
                    .parent()
                    .and_then(ast::UnsafeExpr::cast)
                    .map_or_else(
                        || block.syntax().text_range(),
                        |unsafe_expr| unsafe_expr.syntax().text_range(),
                    )
            });
        let producer_module = quote
            .syntax()
            .ancestors()
            .find_map(ast::Mod::cast)
            .map(|module| module.syntax().text_range());
        let holes = self
            .holes
            .iter()
            .map(|value| match value.value(db) {
                SemConstValue::Scalar {
                    value: SemConstScalar::Bool(value),
                    ..
                } => Ok(FrozenScalar::Bool(*value)),
                SemConstValue::Scalar {
                    ty,
                    value: SemConstScalar::Int { value },
                } => {
                    let crate::analysis::ty::ty_def::TyData::TyBase(
                        crate::analysis::ty::ty_def::TyBase::Prim(primitive),
                    ) = ty.data(db)
                    else {
                        return Err("integer splice must have a completed primitive type");
                    };
                    if !primitive.is_integral() {
                        return Err("integer splice must have a completed primitive type");
                    }
                    let hir_primitive = crate::hir_def::prim_ty::PrimTy::all_types()
                        .iter()
                        .find(|candidate| {
                            crate::analysis::ty::ty_def::TyBase::from(**candidate)
                                == crate::analysis::ty::ty_def::TyBase::Prim(*primitive)
                        })
                        .expect("integral semantic primitive has a HIR primitive");
                    let primitive = hir_primitive.name(db).data(db).to_string();
                    let (sign, bytes) = value.to_bytes_be();
                    Ok(FrozenScalar::Int {
                        negative: sign == num_bigint::Sign::Minus,
                        bytes,
                        primitive: primitive.clone(),
                    })
                }
                _ => Err("bounded output copying supports boolean and integer splices"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FrozenFunctionTemplate {
            target_file: target
                .url(db)
                .ok_or("generation target has no source URL")?,
            quote_range: quote.syntax().text_range(),
            request_impl: None,
            scoped_impl: None,
            producer_module,
            producer_block,
            holes,
        })
    }

    pub fn syntax(&self, db: &'db dyn HirAnalysisDb) -> Result<ast::QuoteExpr, &'static str> {
        let body = self
            .key
            .owner(db)
            .body(db)
            .ok_or("quote producer has no body")?;
        let HirOrigin::Raw(ptr) = body.source_map(db).expr_map.node_to_source(self.expression)
        else {
            return Err("item code must retain its authored quote origin");
        };
        let root = crate::lower::top_mod_ast(db, body.top_mod(db));
        let ast::ExprKind::Quote(quote) = ptr.to_node(root.syntax()).kind() else {
            return Err("item code source is not a quotation");
        };
        Ok(quote)
    }

    pub(crate) fn validate(&self, db: &'db dyn HirAnalysisDb) -> Result<(), &'static str> {
        let body = self
            .key
            .owner(db)
            .body(db)
            .ok_or("quote producer has no body")?;
        let crate::hir_def::Partial::Present(crate::hir_def::Expr::ItemQuote(holes)) =
            self.expression.data(db, body)
        else {
            return Err("item code does not identify a checked quote expression");
        };
        if holes.len() != self.holes.len() {
            return Err("item code splice arity differs from its source");
        }
        for value in &self.holes {
            crate::analysis::semantic::consts::verify_sem_const_shape(db, *value)?;
            if !matches!(
                value.value(db),
                crate::analysis::semantic::SemConstValue::Scalar { .. }
            ) {
                return Err("this bounded item quotation requires scalar splice values");
            }
        }
        let quote = self.syntax(db)?;
        quote_holes(&quote)?;
        let block = quote.body().ok_or("item quote has no body")?;
        if block.stmts().next().is_some() {
            return Err("item quotation requires item declarations");
        }
        let items = block.items().collect::<Vec<_>>();
        let [item] = items.as_slice() else {
            return Err("this bounded item quotation requires one function declaration");
        };
        let Some(ast::ItemKind::Func(function)) = item.kind() else {
            return Err("this bounded item quotation requires one function declaration");
        };
        if function
            .attr_list()
            .is_some_and(|attrs| attrs.normal_attrs_named("generate").next().is_some())
        {
            return Err("quoted functions cannot carry #[generate] attributes");
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum NativeDeriveProviderError<'db> {
    Message(String),
    Target(crate::analysis::ty::ty_check::GroundTypeAdmissionError<'db>),
}
impl From<String> for NativeDeriveProviderError<'_> {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}
impl From<&str> for NativeDeriveProviderError<'_> {
    fn from(message: &str) -> Self {
        Self::Message(message.into())
    }
}

/// Explicit bounded Derive request through the ordinary trait selection path.
pub fn native_derive_provider<'db>(
    db: &'db dyn HirAnalysisDb,
    request: crate::hir_def::ImplTrait<'db>,
    provider_ty: TyId<'db>,
) -> Result<(crate::hir_def::Func<'db>, RecordInput<'db>), NativeDeriveProviderError<'db>> {
    use crate::analysis::ty::{
        corelib::resolve_core_trait,
        trait_choice::Resolved,
        trait_def::{TraitInstId, chosen_method, prove_selected_hir_application},
        trait_resolution::{PredicateListId, Selection, TraitSolveCx},
        ty_def::{TyBase, TyData},
    };
    if !request.impl_params(db).is_empty() || !request.where_clause(db).predicates(db).is_empty() {
        return Err(
            "bounded native derive requires a concrete header without binders or where conditions"
                .into(),
        );
    }
    let goal = request
        .candidate_trait_inst_result(db)
        .map_err(|_| "cannot resolve requested trait and target")?;
    // Validate authority against the authored goal and scope before any provider runs.
    // Ok(None) means the goal is outside the canonical gate, not a grant.
    // No establishment receipt or old-database proof enters the output template.
    crate::analysis::ty::permits::establishment_for_goal(db, request, goal)
        .map_err(|cause| format!("canonical root establishment failed: {cause}"))?;
    if goal.args(db).len() != 1
        || !goal.assoc_type_bindings(db).is_empty()
        || !goal.def(db).types(db).is_empty()
        || !goal.def(db).consts(db).is_empty()
    {
        return Err(
            "bounded native derive requires an unparameterized goal without associated equalities"
                .into(),
        );
    }
    if goal.def(db).ingot(db) != request.top_mod(db).ingot(db) {
        return Err("library/canonical goal establishment is pending the permits owner API".into());
    }
    let input = RecordInput::for_authored_type(db, goal.args(db)[0])
        .map_err(|cause| format!("native derive requires a concrete authored record: {cause:?}"))?;
    crate::analysis::ty::ty_check::admit_ground_type_application(
        db,
        input.ty(),
        request.into(),
        request.span().ty().into(),
    )
    .map_err(NativeDeriveProviderError::Target)?;
    if provider_ty.has_param(db) || provider_ty.has_invalid(db) || !provider_ty.is_star_kind(db) {
        return Err("native derive requires a concrete provider type".into());
    }
    crate::analysis::ty::ty_check::admit_ground_type_application(
        db,
        provider_ty,
        request.into(),
        request.span().into(),
    )
    .map_err(NativeDeriveProviderError::Target)?;
    let derive = resolve_core_trait(db, request.scope(), &["derive", "Derive"])
        .ok_or("missing canonical Derive protocol")?;
    let constructor = TyId::new(db, TyData::TyBase(TyBase::TraitCtor(goal.def(db))));
    let inst = TraitInstId::new_simple(db, derive, vec![provider_ty, constructor]);
    let selected = chosen_method(
        db,
        Resolved::Ordinary,
        inst,
        IdentId::new(db, "derive"),
        request.scope(),
        PredicateListId::empty_list(db),
        None,
    );
    match selected {
        Selection::Unique(method) => {
            if !method.body_args().is_empty() {
                return Err(
                    "generic provider method substitution remains unsupported in this checkpoint"
                        .into(),
                );
            }
            // Method discovery is not admission. Check this exact retained
            // implementation with the ordinary declaration-condition machinery
            // before its body can execute. Ground conditions remain ambient at
            // their declaration; no caller selection is transported here.
            let crate::analysis::ty::trait_def::ImplementorOrigin::Hir(implementation) =
                method.origin(db)
            else {
                return Err("native derive provider requires an authored implementation".into());
            };
            let checked = prove_selected_hir_application(
                db,
                TraitSolveCx::new(db, request.scope()),
                implementation,
                inst,
            )
            .map_err(|cause| {
                format!("native derive provider application not admitted: {cause:?}")
            })?;
            let Selection::Unique(method) = checked.method(IdentId::new(db, "derive")) else {
                return Err("admitted native derive provider has no unique method".into());
            };
            Ok((
                method
                    .body()
                    .ok_or("selected Derive method has no authored body")?,
                input,
            ))
        }
        Selection::NotFound => {
            Err("no ordinary Derive<requested goal> implementation for the provider".into())
        }
        Selection::Ambiguous(_) => {
            Err("ambiguous ordinary Derive<requested goal> provider implementation".into())
        }
    }
}

/// Recover the exact authored header, never a first-by-name implementation.
pub fn authored_derive_request<'db>(
    db: &'db dyn HirAnalysisDb,
    module: crate::hir_def::TopLevelMod<'db>,
    ast: &ast::ImplTrait,
) -> Option<crate::hir_def::ImplTrait<'db>> {
    let root = crate::core::lower::top_mod_ast(db, module);
    module
        .all_impl_traits(db)
        .iter()
        .copied()
        .find(|implementation| {
            let HirOrigin::Raw(ptr) = implementation.origin(db) else {
                return false;
            };
            ptr.to_node(root.syntax()).syntax().text_range() == ast.syntax().text_range()
        })
}

/// A header-only request is not an implementation before its output plan exists.
pub(crate) fn is_unfulfilled_derive_request(
    db: &dyn HirAnalysisDb,
    request: crate::hir_def::ImplTrait<'_>,
) -> bool {
    let HirOrigin::Raw(ptr) = request.origin(db) else {
        return false;
    };
    let root = crate::core::lower::top_mod_ast(db, request.top_mod(db));
    let ast = ptr.to_node(root.syntax());
    ast.derive_provider().is_some()
        && !db.generation_inputs().functions(db).iter().any(|plan| {
            plan.request_impl == Some(ast.syntax().text_range())
                && Some(plan.target_file.clone()) == request.top_mod(db).file(db).url(db)
        })
}

pub fn item_module_relative_path<'db>(
    db: &'db dyn HirAnalysisDb,
    item: crate::hir_def::ItemKind<'db>,
) -> Option<Vec<String>> {
    let mut segments = vec![item.name(db)?.data(db).to_string()];
    let mut scope = item.scope();
    while let Some(parent) = scope.parent(db) {
        match parent {
            crate::hir_def::scope_graph::ScopeId::Item(crate::hir_def::ItemKind::TopMod(_)) => {
                segments.reverse();
                return Some(segments);
            }
            crate::hir_def::scope_graph::ScopeId::Item(crate::hir_def::ItemKind::Mod(m)) => {
                segments.push(m.name(db).to_opt()?.data(db).to_string());
                scope = parent;
            }
            _ => return None,
        }
    }
    None
}

fn ty_selection_component<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> String {
    if let Some(adt) = ty.adt_def(db)
        && let Some(item) = adt.adt_ref(db).scope().to_item()
        && let Some(segments) = item_module_relative_path(db, item)
    {
        return segments.join("_");
    }
    ty.pretty_print(db).to_string()
}

pub fn scoped_derive_selection_name<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: crate::hir_def::Trait<'db>,
    target_ty: TyId<'db>,
    provider_ty: TyId<'db>,
) -> IdentId<'db> {
    let trait_comp = item_module_relative_path(db, trait_.into())
        .map(|segments| segments.join("_"))
        .or_else(|| trait_.name(db).to_opt().map(|id| id.data(db).to_string()))
        .unwrap_or_else(|| "Trait".to_string());
    let target_comp = ty_selection_component(db, target_ty);
    let provider_comp = ty_selection_component(db, provider_ty);
    IdentId::new(
        db,
        format!("__fe_scoped_derive${trait_comp}${target_comp}${provider_comp}"),
    )
}

/// Returns the traits `T` for which an authored ordinary `impl Derive<T> for Provider`
/// exists in the ingot.
pub fn derive_provider_traits_for_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: crate::hir_def::scope_graph::ScopeId<'db>,
    provider_ty: TyId<'db>,
) -> Vec<crate::hir_def::Trait<'db>> {
    use crate::analysis::ty::{
        canonical::Canonical,
        corelib::resolve_core_trait,
        trait_def::impls_for_trait_and_ty,
        ty_def::{TyBase, TyData},
    };
    if provider_ty.has_param(db) || provider_ty.has_invalid(db) || !provider_ty.is_star_kind(db) {
        return Vec::new();
    }
    let Some(derive) = resolve_core_trait(db, scope, &["derive", "Derive"]) else {
        return Vec::new();
    };
    let ingot = scope.top_mod(db).ingot(db);
    let mut traits = Vec::new();
    for implementor in impls_for_trait_and_ty(db, ingot, derive, Canonical::new(db, provider_ty)) {
        let args = implementor.trait_(db).args(db);
        if args.len() == 2
            && let TyData::TyBase(TyBase::TraitCtor(trait_)) = args[1].data(db)
            && !traits.contains(trait_)
        {
            traits.push(*trait_);
        }
    }
    traits
}

/// Validate a scoped on-demand derive opening `with (Provider)` at its opening site.
pub fn validate_scoped_derive_opening<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: crate::hir_def::scope_graph::ScopeId<'db>,
    trait_: crate::hir_def::Trait<'db>,
    provider_ty: TyId<'db>,
) -> Result<(), NativeDeriveProviderError<'db>> {
    use crate::analysis::ty::{
        corelib::resolve_core_trait,
        trait_choice::Resolved,
        trait_def::{TraitInstId, chosen_method, prove_selected_hir_application},
        trait_resolution::{PredicateListId, Selection, TraitSolveCx},
        ty_def::{TyBase, TyData},
    };
    let span: crate::span::DynLazySpan<'db> = scope
        .name_span(db)
        .unwrap_or_else(|| scope.top_mod(db).span().into());
    if crate::analysis::ty::permits::is_canonical_trait(db, trait_) {
        return Err(
            "canonical root establishment failed: a canonical goal accepts only its ordinary home provision"
                .into(),
        );
    }
    if !trait_.types(db).is_empty() || !trait_.consts(db).is_empty() {
        return Err(
            "bounded native derive requires an unparameterized goal without associated equalities"
                .into(),
        );
    }
    if trait_.top_mod(db).ingot(db) != scope.top_mod(db).ingot(db) {
        return Err("library/canonical goal establishment is pending the permits owner API".into());
    }
    if provider_ty.has_param(db) || provider_ty.has_invalid(db) || !provider_ty.is_star_kind(db) {
        return Err("native derive requires a concrete provider type".into());
    }
    crate::analysis::ty::ty_check::admit_ground_type_application(
        db,
        provider_ty,
        scope.item(),
        span,
    )
    .map_err(NativeDeriveProviderError::Target)?;
    let derive = resolve_core_trait(db, scope, &["derive", "Derive"])
        .ok_or("missing canonical Derive protocol")?;
    let constructor = TyId::new(db, TyData::TyBase(TyBase::TraitCtor(trait_)));
    let inst = TraitInstId::new_simple(db, derive, vec![provider_ty, constructor]);
    let selected = chosen_method(
        db,
        Resolved::Ordinary,
        inst,
        IdentId::new(db, "derive"),
        scope,
        PredicateListId::empty_list(db),
        None,
    );
    match selected {
        Selection::Unique(method) => {
            if !method.body_args().is_empty() {
                return Err(
                    "generic provider method substitution remains unsupported in this checkpoint"
                        .into(),
                );
            }
            let crate::analysis::ty::trait_def::ImplementorOrigin::Hir(implementation) =
                method.origin(db)
            else {
                return Err("native derive provider requires an authored implementation".into());
            };
            let checked = prove_selected_hir_application(
                db,
                TraitSolveCx::new(db, scope),
                implementation,
                inst,
            )
            .map_err(|cause| {
                format!("native derive provider application not admitted: {cause:?}")
            })?;
            let Selection::Unique(method) = checked.method(IdentId::new(db, "derive")) else {
                return Err("admitted native derive provider has no unique method".into());
            };
            if method.body().is_none() {
                return Err("selected Derive method has no authored body".into());
            }
            Ok(())
        }
        Selection::NotFound => {
            Err("no ordinary Derive<requested goal> implementation for the provider".into())
        }
        Selection::Ambiguous(_) => {
            Err("ambiguous ordinary Derive<requested goal> provider implementation".into())
        }
    }
}

/// Validate and resolve a concrete scoped on-demand derive demand `(Trait, Target, Provider)`.
pub fn scoped_derive_provider<'db>(
    db: &'db dyn HirAnalysisDb,
    _module: crate::hir_def::TopLevelMod<'db>,
    scope: crate::hir_def::scope_graph::ScopeId<'db>,
    trait_def: crate::hir_def::Trait<'db>,
    target_ty: TyId<'db>,
    provider_ty: TyId<'db>,
) -> Result<
    (
        crate::hir_def::Func<'db>,
        RecordInput<'db>,
        common::generation::FrozenScopedImpl,
    ),
    NativeDeriveProviderError<'db>,
> {
    use crate::analysis::ty::{
        corelib::resolve_core_trait,
        trait_choice::Resolved,
        trait_def::{TraitInstId, chosen_method, prove_selected_hir_application},
        trait_resolution::{PredicateListId, Selection, TraitSolveCx},
        ty_def::{TyBase, TyData},
    };
    let span: crate::span::DynLazySpan<'db> = scope
        .name_span(db)
        .unwrap_or_else(|| scope.top_mod(db).span().into());
    let goal = TraitInstId::new_simple(db, trait_def, vec![target_ty]);
    if crate::analysis::ty::permits::is_canonical_trait(db, goal.def(db)) {
        return Err(
            "canonical root establishment failed: a canonical goal accepts only its ordinary home provision"
                .into(),
        );
    }
    if goal.args(db).len() != 1
        || !goal.assoc_type_bindings(db).is_empty()
        || !goal.def(db).types(db).is_empty()
        || !goal.def(db).consts(db).is_empty()
    {
        return Err(
            "bounded native derive requires an unparameterized goal without associated equalities"
                .into(),
        );
    }
    if goal.def(db).ingot(db) != scope.top_mod(db).ingot(db) {
        return Err("library/canonical goal establishment is pending the permits owner API".into());
    }
    let input = RecordInput::for_authored_type(db, goal.args(db)[0])
        .map_err(|cause| format!("native derive requires a concrete authored record: {cause:?}"))?;
    crate::analysis::ty::ty_check::admit_ground_type_application(
        db,
        input.ty(),
        scope.item(),
        span.clone(),
    )
    .map_err(NativeDeriveProviderError::Target)?;
    if provider_ty.has_param(db) || provider_ty.has_invalid(db) || !provider_ty.is_star_kind(db) {
        return Err("native derive requires a concrete provider type".into());
    }
    crate::analysis::ty::ty_check::admit_ground_type_application(
        db,
        provider_ty,
        scope.item(),
        span,
    )
    .map_err(NativeDeriveProviderError::Target)?;
    let trait_path = item_module_relative_path(db, trait_def.into())
        .ok_or("cannot resolve module-relative path for scoped derive trait")?;
    let target_item = input
        .declaration()
        .scope()
        .to_item()
        .ok_or("cannot resolve item for scoped derive target")?;
    let target_path = item_module_relative_path(db, target_item)
        .ok_or("cannot resolve module-relative path for scoped derive target")?;
    let selection_name = scoped_derive_selection_name(db, trait_def, target_ty, provider_ty)
        .data(db)
        .to_string();
    let scoped_impl = common::generation::FrozenScopedImpl {
        trait_path,
        target_path,
        selection_name,
    };
    let derive = resolve_core_trait(db, scope, &["derive", "Derive"])
        .ok_or("missing canonical Derive protocol")?;
    let constructor = TyId::new(db, TyData::TyBase(TyBase::TraitCtor(goal.def(db))));
    let inst = TraitInstId::new_simple(db, derive, vec![provider_ty, constructor]);
    let selected = chosen_method(
        db,
        Resolved::Ordinary,
        inst,
        IdentId::new(db, "derive"),
        scope,
        PredicateListId::empty_list(db),
        None,
    );
    match selected {
        Selection::Unique(method) => {
            if !method.body_args().is_empty() {
                return Err(
                    "generic provider method substitution remains unsupported in this checkpoint"
                        .into(),
                );
            }
            let crate::analysis::ty::trait_def::ImplementorOrigin::Hir(implementation) =
                method.origin(db)
            else {
                return Err("native derive provider requires an authored implementation".into());
            };
            let checked = prove_selected_hir_application(
                db,
                TraitSolveCx::new(db, scope),
                implementation,
                inst,
            )
            .map_err(|cause| {
                format!("native derive provider application not admitted: {cause:?}")
            })?;
            let Selection::Unique(method) = checked.method(IdentId::new(db, "derive")) else {
                return Err("admitted native derive provider has no unique method".into());
            };
            Ok((
                method
                    .body()
                    .ok_or("selected Derive method has no authored body")?,
                input,
                scoped_impl,
            ))
        }
        Selection::NotFound => {
            Err("no ordinary Derive<requested goal> implementation for the provider".into())
        }
        Selection::Ambiguous(_) => {
            Err("ambiguous ordinary Derive<requested goal> provider implementation".into())
        }
    }
}
