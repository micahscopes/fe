//! Compiler-designated types in the checked `core::meta` generation protocol.
//!
//! A matching user-defined spelling does not establish protocol identity.

use crate::span::LazySpan;
use crate::{
    analysis::{
        HirAnalysisDb,
        semantic::SemConstScalar,
        ty::{
            corelib::resolve_lib_type_path,
            ty_def::{PrimTy, TyId},
        },
    },
    hir_def::{ExprId, PatId, StmtId, TypeMode, scope_graph::ScopeId},
};
use std::collections::BTreeMap;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetaItem {
    Record,
    Field,
    FieldType,
    Code,
    CodeNode,
    /// The kinds of `Code<K>`.
    Expr,
    Pat,
    Arm,
    Item,
    Where,
    DeclarationName,
}

impl MetaItem {
    /// Registry order matches the enum discriminants for indexed type tables.
    pub const ALL: [Self; 11] = [
        Self::Record,
        Self::Field,
        Self::FieldType,
        Self::Code,
        Self::CodeNode,
        Self::Expr,
        Self::Pat,
        Self::Arm,
        Self::Item,
        Self::Where,
        Self::DeclarationName,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Record => "core::meta::Record",
            Self::Field => "core::meta::Field",
            Self::FieldType => "core::meta::FieldType",
            Self::Code => "core::meta::Code",
            Self::CodeNode => "core::meta::CodeNode",
            Self::Expr => "core::meta::Expr",
            Self::Arm => "core::meta::Arm",
            Self::Pat => "core::meta::Pat",
            Self::Item => "core::meta::Item",
            Self::Where => "core::meta::Where",
            Self::DeclarationName => "core::meta::DeclarationName",
        }
    }

    pub fn resolve<'db>(
        self,
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
    ) -> Option<TyId<'db>> {
        resolve_lib_type_path(db, scope, self.path())
    }
}

/// A structural handle remains qualified by its authored owner after CTFE.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenOwner {
    pub file: Url,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenOwnerError {
    MissingFile,
    MissingPath,
}

impl FrozenOwner {
    pub fn of_item<'db>(
        db: &'db dyn crate::SpannedHirDb,
        item: crate::hir_def::ItemKind<'db>,
    ) -> Result<Self, FrozenOwnerError> {
        let file = item
            .span()
            .resolve(db)
            .and_then(|span| span.file.url(db))
            .ok_or(FrozenOwnerError::MissingFile)?;
        let path = item
            .scope()
            .pretty_path(db)
            .ok_or(FrozenOwnerError::MissingPath)?;
        Ok(Self { file, path })
    }
}

/// Stable authored item address shared by provider identity and quote lookup.
/// Impl members use the declaration header instead of an unnamed impl scope
/// or its source-order index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenItemLocator {
    Named(FrozenOwner),
    ImplMember {
        file: Url,
        module_path: String,
        trait_path: Option<String>,
        self_type_path: String,
        selection_name: Option<String>,
        member_name: String,
    },
    TraitMember {
        file: Url,
        trait_path: String,
        member_name: String,
    },
}

impl FrozenItemLocator {
    pub fn file(&self) -> &Url {
        match self {
            Self::Named(owner) => &owner.file,
            Self::ImplMember { file, .. } | Self::TraitMember { file, .. } => file,
        }
    }

    pub fn path_nodes(&self) -> usize {
        match self {
            Self::Named(owner) => authored_path_nodes(&owner.path),
            Self::ImplMember {
                module_path,
                member_name,
                ..
            } => authored_path_nodes(module_path) + authored_path_nodes(member_name),
            Self::TraitMember {
                trait_path,
                member_name,
                ..
            } => authored_path_nodes(trait_path) + authored_path_nodes(member_name),
        }
    }

    pub fn named_path(&self) -> Option<&str> {
        match self {
            Self::Named(owner) => Some(&owner.path),
            _ => None,
        }
    }
}

/// The source of a frozen template. A checked body uses its function body root
/// while a quote uses the authored quote expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum FrozenTemplateSource {
    Quote,
    CheckedBody,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenTemplateKey {
    pub owner: FrozenItemLocator,
    /// The expression index is local to the owner's body arena.
    pub expr: ExprId,
    pub source: FrozenTemplateSource,
    /// Authored quote position, retained across the output database fork.
    pub span: Option<std::ops::Range<usize>>,
}

/// A flat graph preserves sharing when code fragments splice other fragments.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenCode {
    pub root: usize,
    pub nodes: Vec<FrozenCodeNode>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrozenCodeSource {
    #[default]
    Empty,
    Template(FrozenTemplateKey),
    Pattern(FrozenVariantBinding),
    Field(FrozenVariantBinding, usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FrozenCodeNode {
    pub source: FrozenCodeSource,
    pub holes: Vec<FrozenCodeHole>,
    /// Authored expression paths that were resolved before generation.
    pub rooted_funcs: Vec<FrozenQuoteFunction>,
    /// Nominal constructors resolved where the template is authored.
    pub rooted_constructors: Vec<FrozenQuoteConstructor>,
    /// Method declarations selected while checking a read-back body.
    pub selected_methods: Vec<(ExprId, FrozenItemLocator)>,
    /// Authored trait paths, keyed by the type-site order in the quote.
    pub rooted_traits: Vec<FrozenQuoteTrait>,
    /// Qualified constant uses with a checked provider-side trait selection.
    pub qualified_consts: Vec<FrozenQuoteQualifiedConst>,
    /// Checked type arguments of the quoted impl trait, if explicitly given.
    pub impl_trait_type_arguments: Option<Vec<FrozenQuoteSignatureType>>,
    /// Pattern binders and their checked uses in the quoted template.
    pub locals: Vec<FrozenQuoteLocal>,
    /// Checked primitive targets of casts in quoted fragments or checked bodies.
    pub cast_primitives: Vec<(ExprId, PrimTy)>,
    /// Provider-checked types of local annotations in quoted bodies.
    pub local_types: Vec<FrozenQuoteLocalType>,
    /// Checked nominal constructor types in a read-back body.
    pub record_types: Vec<(ExprId, FrozenQuoteSignatureType)>,
    /// Uses of parameters declared by a quoted function.
    pub param_uses: Vec<FrozenQuoteParamUse>,
    pub effect_uses: Vec<FrozenQuoteEffectUse>,
    /// Uses of method-local const parameters in quoted function bodies.
    pub generic_uses: Vec<FrozenQuoteGenericUse>,
    /// Uses of `self` inside quoted method bodies.
    pub self_uses: Vec<FrozenQuoteSelfUse>,
    /// `Self { ... }` constructors in quoted method bodies.
    pub self_record_inits: Vec<FrozenQuoteSelfUse>,
    /// Open parameter uses in an expression fragment, by its declared open-name index.
    pub open_uses: Vec<FrozenQuoteOpenUse>,
    /// `self` uses in an expression fragment, bound by the destination method.
    pub free_self_uses: Vec<ExprId>,
    /// Checked function signatures in the quote's visiting order.
    pub function_types: Vec<FrozenQuoteFunctionTypes>,
    /// Checked fragment/impl predicates; holes retain their graph edge in holes.
    pub where_predicates: Vec<Option<FrozenQuoteWherePredicate>>,
    /// Checked primitive types of associated constants in the sole quoted impl.
    pub assoc_const_primitives: Vec<Option<PrimTy>>,
    /// Checked associated-type values in the sole quoted impl.
    pub assoc_type_values: Vec<Option<FrozenQuoteSignatureType>>,
}

/// The defining ingot replaces the display root; it is not an extra path node.
pub fn authored_path_nodes(path: &str) -> usize {
    path.split("::").count()
}

impl FrozenCodeNode {
    /// Ownership for every classified site, without allocating another table.
    pub fn members(&self) -> impl Iterator<Item = QuoteMember> + '_ {
        self.rooted_funcs
            .iter()
            .map(|site| site.member)
            .chain(self.rooted_constructors.iter().map(|site| site.member))
            .chain(self.qualified_consts.iter().map(|site| site.member))
            .chain(self.locals.iter().map(|site| site.member))
            .chain(self.local_types.iter().map(|site| site.member))
            .chain(self.param_uses.iter().map(|site| site.member))
            .chain(self.effect_uses.iter().map(|site| site.member))
            .chain(self.generic_uses.iter().map(|site| site.member))
            .chain(self.self_uses.iter().map(|site| site.member))
            .chain(self.self_record_inits.iter().map(|site| site.member))
    }

    /// Paths prepared from captured roots, separate from indexing metadata.
    pub fn rooted_path_nodes(&self, function: Option<u32>) -> usize {
        let arguments = |args: &Option<Vec<FrozenQuoteSignatureType>>| {
            args.as_ref().map_or(0, |args| 2 + args.len())
        };
        self.rooted_funcs
            .iter()
            .filter(|root| function.is_none() || root.member.function() == function)
            .map(|root| root.owner.path_nodes() + arguments(&root.type_arguments))
            .sum::<usize>()
            + if function.is_none() {
                self.rooted_traits
                    .iter()
                    .map(|root| authored_path_nodes(&root.owner.path))
                    .sum::<usize>()
            } else {
                0
            }
            + self
                .rooted_constructors
                .iter()
                .filter(|root| function.is_none() || root.member.function() == function)
                .map(|root| {
                    authored_path_nodes(&root.owner.owner.path)
                        + usize::from(root.variant_index.is_some())
                            * (1 + usize::from(root.self_target))
                        + usize::from(root.self_target)
                        + arguments(&root.type_arguments)
                })
                .sum::<usize>()
    }

    /// Retained indexing/binder metadata, excluding type-tree children and values.
    /// Destructure exhaustively so adding a carrier field requires an accounting decision.
    pub fn metadata_nodes(&self) -> usize {
        let Self {
            source: _,
            holes,
            rooted_funcs,
            rooted_constructors,
            selected_methods,
            rooted_traits,
            qualified_consts,
            impl_trait_type_arguments: _,
            locals,
            cast_primitives,
            local_types,
            record_types,
            param_uses,
            effect_uses,
            generic_uses,
            self_uses,
            self_record_inits,
            open_uses,
            free_self_uses,
            function_types,
            where_predicates,
            assoc_const_primitives,
            assoc_type_values,
        } = self;
        let mut count = [
            holes.len(),
            rooted_funcs.len(),
            rooted_constructors.len(),
            selected_methods.len(),
            rooted_traits.len(),
            qualified_consts.len(),
            locals.len(),
            cast_primitives.len(),
            local_types.len(),
            record_types.len(),
            param_uses.len(),
            effect_uses.len(),
            generic_uses.len(),
            self_uses.len(),
            self_record_inits.len(),
            open_uses.len(),
            free_self_uses.len(),
            function_types.len(),
            where_predicates.len(),
            assoc_const_primitives.len(),
            assoc_type_values.len(),
        ]
        .into_iter()
        .fold(0usize, usize::saturating_add);
        for local in locals {
            count = count.saturating_add(local.uses.len());
        }
        for function in function_types {
            let FrozenQuoteFunctionTypes {
                params,
                effects,
                result: _,
                generic_bound_sites,
                generic_type_defaults,
                generic_const_types,
                where_predicates,
            } = function;
            for size in [
                params.len(),
                effects.len(),
                generic_bound_sites.len(),
                generic_type_defaults.len(),
                generic_const_types.len(),
                where_predicates.len(),
            ] {
                count = count.saturating_add(size);
            }
            for bounds in generic_bound_sites {
                count = count.saturating_add(bounds.len());
            }
            for predicate in where_predicates {
                if let FrozenQuoteWherePredicate::Bound { trait_sites, .. } = predicate {
                    count = count.saturating_add(trait_sites.len());
                }
            }
        }
        for predicate in where_predicates.iter().flatten() {
            if let FrozenQuoteWherePredicate::Bound { trait_sites, .. } = predicate {
                count = count.saturating_add(trait_sites.len());
            }
        }
        count
    }
    pub fn template(&self) -> Option<&FrozenTemplateKey> {
        match &self.source {
            FrozenCodeSource::Template(key) => Some(key),
            _ => None,
        }
    }
    pub fn empty(template: Option<FrozenTemplateKey>) -> Self {
        Self {
            source: template.map_or(FrozenCodeSource::Empty, FrozenCodeSource::Template),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenVariantBinding {
    pub owner: FrozenQuoteNominal,
    pub variant_index: u16,
    pub group: usize,
}

/// The expression body or quoted item owning a classified quotation site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteMember {
    Fragment,
    Function(u32),
    FunctionWhere(u32, u32),
    AssocConst(u32),
    CheckedBody,
}

impl QuoteMember {
    pub fn function(self) -> Option<u32> {
        match self {
            Self::Function(index) | Self::FunctionWhere(index, _) => Some(index),
            Self::Fragment | Self::AssocConst(_) | Self::CheckedBody => None,
        }
    }

    pub fn where_predicate(self) -> Option<u32> {
        match self {
            Self::FunctionWhere(_, predicate) => Some(predicate),
            _ => None,
        }
    }

    pub fn assoc_const(self) -> Option<u32> {
        match self {
            Self::AssocConst(index) => Some(index),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteLocal {
    pub member: QuoteMember,
    pub binder: PatId,
    pub uses: Vec<ExprId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum FrozenQuoteConstructorSite {
    Expr(ExprId),
    Pat(PatId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteConstructor {
    pub member: QuoteMember,
    pub site: FrozenQuoteConstructorSite,
    pub owner: FrozenQuoteNominal,
    /// `None` denotes the struct itself; `Some` indexes the authored enum.
    pub variant_index: Option<u16>,
    /// Checked instantiation when the authored spelling fixes owner arguments.
    pub type_arguments: Option<Vec<FrozenQuoteSignatureType>>,
    /// Preserve the impl receiver binding instead of spelling its nominal path.
    pub self_target: bool,
}

impl FrozenQuoteConstructor {
    pub fn new(
        member: QuoteMember,
        site: FrozenQuoteConstructorSite,
        owner: FrozenQuoteNominal,
        variant_index: Option<u16>,
        type_arguments: Option<Vec<FrozenQuoteSignatureType>>,
    ) -> Self {
        Self {
            member,
            site,
            owner,
            variant_index,
            type_arguments,
            self_target: false,
        }
    }

    pub fn for_self(mut self) -> Self {
        self.self_target = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteLocalType {
    pub member: QuoteMember,
    pub stmt: StmtId,
    pub ty: FrozenQuoteSignatureType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteSelfUse {
    pub member: QuoteMember,
    pub site: ExprId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteOpenUse {
    pub site: ExprId,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteQualifiedConst {
    pub member: QuoteMember,
    pub site: ExprId,
    pub trait_site: u32,
    pub receiver: PrimTy,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteParamUse {
    pub site: ExprId,
    pub member: QuoteMember,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteEffectUse {
    pub site: ExprId,
    pub member: QuoteMember,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteGenericUse {
    pub site: ExprId,
    pub member: QuoteMember,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteFunctionTypes {
    pub params: Vec<Option<FrozenQuoteSignatureType>>,
    pub effects: Vec<Option<FrozenQuoteSignatureType>>,
    pub result: Option<FrozenQuoteSignatureType>,
    /// Rooted trait site for each method-local generic bound.
    pub generic_bound_sites: Vec<Vec<Option<u32>>>,
    /// Checked default type for each method-local type parameter.
    pub generic_type_defaults: Vec<Option<FrozenQuoteSignatureType>>,
    /// Checked scalar type for each method-local const parameter.
    pub generic_const_types: Vec<Option<PrimTy>>,
    /// Checked method-local where predicates in their authored order.
    pub where_predicates: Vec<FrozenQuoteWherePredicate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenQuoteWherePredicate {
    Bound {
        ty: FrozenQuoteSignatureType,
        trait_sites: Vec<Option<u32>>,
    },
    Const,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenQuoteSignatureType {
    Primitive(PrimTy),
    /// A checked type splice, reified only where the target record is known.
    Hole(u32),
    SelfType,
    FunctionGeneric(u32),
    /// An inherited declaration binder of a checked body. Rebinding happens
    /// only when the destination owner context is available.
    OwnerGeneric(u32),
    Nominal {
        root: FrozenQuoteNominal,
        arguments: Vec<FrozenQuoteSignatureType>,
        arguments_given: bool,
    },
    Trait {
        root: FrozenQuoteTrait,
        arguments: Vec<FrozenQuoteSignatureType>,
        arguments_given: bool,
    },
    Mode(TypeMode, Box<FrozenQuoteSignatureType>),
    Ptr(Box<FrozenQuoteSignatureType>),
    Tuple(Vec<FrozenQuoteSignatureType>),
    Never,
}

impl FrozenQuoteSignatureType {
    /// HIR nodes emitted for this type, excluding recursively emitted children.
    pub fn reification_nodes(&self) -> usize {
        let root_nodes = match self {
            Self::Nominal { root, .. } => authored_path_nodes(&root.owner.path),
            Self::Trait { root, .. } => authored_path_nodes(&root.owner.path),
            Self::Tuple(_) | Self::SelfType | Self::FunctionGeneric(_) | Self::OwnerGeneric(_) => {
                return 2;
            }
            _ => return 1,
        };
        let arguments_given = matches!(
            self,
            Self::Nominal {
                arguments_given: true,
                ..
            } | Self::Trait {
                arguments_given: true,
                ..
            }
        );
        let arguments = self.children();
        2 + root_nodes + arguments.len() + 2 * usize::from(arguments_given || !arguments.is_empty())
    }

    /// Direct type-syntax children, shared by construction budget walks.
    pub fn children(&self) -> &[Self] {
        match self {
            Self::Nominal { arguments, .. }
            | Self::Trait { arguments, .. }
            | Self::Tuple(arguments) => arguments,
            Self::Mode(_, inner) | Self::Ptr(inner) => std::slice::from_ref(inner.as_ref()),
            _ => &[],
        }
    }
}

/// A nominal signature type selected while checking the quote's provider.
/// Replay locates this declaration before constructing a resolved HIR path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteNominal {
    pub owner: FrozenOwner,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteFunction {
    pub member: QuoteMember,
    /// Explicit call type arguments checked at the provider site.
    pub type_arguments: Option<Vec<FrozenQuoteSignatureType>>,
    pub site: ExprId,
    /// The authored declaration selected while checking the quote.
    pub owner: FrozenItemLocator,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteTrait {
    pub site: u32,
    pub owner: FrozenOwner,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenCodeHole {
    Code(usize, Option<FrozenQuoteSpan>),
    /// Reflected field hints share checked quotation types and owner binders.
    Value(Box<FrozenMetaValue>, Option<FrozenQuoteSignatureType>),
}

/// An authored source location carried through quotation transport.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteSpan {
    pub file: url::Url,
    pub range: std::ops::Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrozenHandleKind {
    Record,
    Variant(u16),
    Field {
        variant_index: Option<u16>,
        index: usize,
    },
    FieldType {
        variant_index: Option<u16>,
        index: usize,
    },
}

/// Owned transport for an already checked `core::meta` result.
///
/// This is a copy of the value, not a second generated syntax tree. Record
/// fields retain their declared names. Selected enum cases have a checked
/// semantic tag, independent of their declaration order. No database-lifetime
/// ID crosses this boundary.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenMetaValue {
    Code(FrozenCode),
    Scalar {
        value: SemConstScalar,
        primitive: Option<PrimTy>,
    },
    Structural {
        item: MetaItem,
        owner: FrozenOwner,
        kind: FrozenHandleKind,
    },
    DeclarationName {
        case: FrozenMetaVariantCase,
        fields: Vec<Self>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrozenMetaVariantCase {
    DeclarationNameLiteral,
    DeclarationNameFieldPrefix,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenGeneration {
    pub target: FrozenOwner,
    /// Checked scalar types for target const parameters, aligned with all
    /// target parameters. Type parameters have no entry.
    pub target_const_param_types: Vec<Option<crate::analysis::ty::ty_def::PrimTy>>,
    /// The authored generation attribute, for output-stage construction errors.
    pub site_span: Option<std::ops::Range<usize>>,
    pub provider: FrozenItemLocator,
    pub attribute_index: usize,
    pub value: FrozenMetaValue,
}

/// One module's completed provider results, published after the authored pass.
#[salsa::input]
#[derive(Debug)]
pub struct GeneratedModuleInput {
    #[return_ref]
    pub requests: Vec<FrozenGeneration>,
}

/// The set of modules that have a published generation stage.
#[salsa::input]
#[derive(Debug)]
pub struct GenerationManifest {
    #[return_ref]
    pub modules: BTreeMap<Url, GeneratedModuleInput>,
}
