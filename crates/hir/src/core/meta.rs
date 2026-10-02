//! Compiler-designated types in the checked `core::meta` generation protocol.
//!
//! A matching user-defined spelling does not establish protocol identity.

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
    /// The kinds of `Code<K>`.
    Expr,
    Item,
    Where,
    DeclarationName,
}

impl MetaItem {
    pub const ALL: [Self; 8] = [
        Self::Record,
        Self::Field,
        Self::FieldType,
        Self::Code,
        Self::Expr,
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
            Self::Expr => "core::meta::Expr",
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

    pub fn named_path(&self) -> Option<&str> {
        match self {
            Self::Named(owner) => Some(&owner.path),
            _ => None,
        }
    }
}

/// Locate a quote in authored HIR after the output database is forked. The
/// expression index is local to the named body's arena, not to the file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenTemplateKey {
    pub owner: FrozenItemLocator,
    pub expr: ExprId,
    /// Authored quote position, retained across the output database fork.
    pub span: Option<std::ops::Range<usize>>,
}

/// A flat graph preserves sharing when code fragments splice other fragments.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenCode {
    pub root: usize,
    pub nodes: Vec<FrozenCodeNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenCodeNode {
    /// `None` is the empty fragment produced by `empty<K>()`.
    pub template: Option<FrozenTemplateKey>,
    pub holes: Vec<FrozenCodeHole>,
    /// Authored expression paths that were resolved before generation.
    pub rooted_funcs: Vec<FrozenQuoteFunction>,
    /// Authored trait paths, keyed by the type-site order in the quote.
    pub rooted_traits: Vec<FrozenQuoteTrait>,
    /// Qualified constant uses with a checked provider-side trait selection.
    pub qualified_consts: Vec<FrozenQuoteQualifiedConst>,
    /// Checked type arguments of the quoted impl trait, if explicitly given.
    pub impl_trait_type_arguments: Option<Vec<FrozenQuoteSignatureType>>,
    /// Pattern binders and their checked uses in the quoted template.
    pub locals: Vec<FrozenQuoteLocal>,
    /// Provider-checked types of local annotations in quoted bodies.
    pub local_types: Vec<FrozenQuoteLocalType>,
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
    /// Checked primitive types of associated constants in the sole quoted impl.
    pub assoc_const_primitives: Vec<Option<PrimTy>>,
    /// Checked associated-type values in the sole quoted impl.
    pub assoc_type_values: Vec<Option<FrozenQuoteSignatureType>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteLocal {
    pub func: u32,
    pub binder: PatId,
    pub uses: Vec<ExprId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteLocalType {
    pub func: u32,
    pub stmt: StmtId,
    pub ty: FrozenQuoteSignatureType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteSelfUse {
    pub func: u32,
    pub site: ExprId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteOpenUse {
    pub site: ExprId,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteQualifiedConst {
    pub func: u32,
    pub site: ExprId,
    pub trait_site: u32,
    pub receiver: PrimTy,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteParamUse {
    pub site: ExprId,
    pub func: u32,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteEffectUse {
    pub site: ExprId,
    pub func: u32,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteGenericUse {
    pub site: ExprId,
    pub func: u32,
    pub index: u32,
    /// A const predicate owns a separate expression body from the method.
    pub where_predicate: Option<u32>,
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

/// A nominal signature type selected while checking the quote's provider.
/// The output resolves its ingot path and confirms the authored item identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteNominal {
    pub ingot: Url,
    pub path: String,
    pub owner: FrozenOwner,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteFunction {
    /// A function index for item quotes, or `None` for an expression fragment.
    pub func: Option<u32>,
    /// An associated-constant index for item quotes, or `None` otherwise.
    pub assoc_const: Option<u32>,
    /// Type argument slots checked as quote holes at the provider site.
    pub type_holes: Option<Vec<u32>>,
    pub site: ExprId,
    pub where_predicate: Option<u32>,
    pub ingot: Url,
    /// Item path relative to the defining ingot's top-level module.
    pub path: String,
    /// The authored declaration selected while checking the quote.
    pub owner: FrozenItemLocator,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenQuoteTrait {
    pub site: u32,
    pub ingot: Url,
    pub path: String,
    pub owner: FrozenOwner,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenCodeHole {
    Code(usize),
    Value(Box<FrozenMetaValue>, Option<FrozenQuoteFieldType>),
}

/// Checked meaning of a field type used by a quote over a generic record.
/// The output view rebinds target parameters by index, never by authored name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenQuoteFieldType {
    TargetParam(usize),
    Primitive(PrimTy),
    Tuple(Vec<FrozenQuoteFieldType>),
    Ptr(Box<FrozenQuoteFieldType>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrozenHandleKind {
    Record,
    Field(usize),
    FieldType(usize),
}

/// Owned transport for an already checked `core::meta` result.
///
/// This is a copy of the value, not a second generated syntax tree. Record
/// fields retain their declared names. Selected enum cases have a checked
/// semantic tag, independent of their declaration order. No database-lifetime
/// ID crosses this boundary.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenMetaValue {
    Unit,
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
    Tuple(Vec<Self>),
    Array(Vec<Self>),
    Record {
        item: MetaItem,
        fields: BTreeMap<String, Self>,
    },
    Variant {
        item: MetaItem,
        name: String,
        case: FrozenMetaVariantCase,
        fields: Vec<Self>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrozenMetaVariantCase {
    Other,
    DeclarationNameLiteral,
    DeclarationNameFieldPrefix,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenGeneration {
    pub target: FrozenOwner,
    /// Checked scalar types for target const parameters, aligned with all
    /// target parameters. Type parameters have no entry.
    pub target_const_param_types: Vec<Option<crate::analysis::ty::ty_def::PrimTy>>,
    /// Source position anchors diagnostics. Item identity uses the resolved
    /// nominal path, so inserting an earlier sibling does not change it.
    pub target_span_start: usize,
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
