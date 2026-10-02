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
    hir_def::scope_graph::ScopeId,
};
use std::collections::BTreeMap;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetaItem {
    Option,
    Record,
    Field,
    FieldType,
    DeclarationType,
    DeclarationName,
    BodyExpr,
    FunctionCall,
    CallArgument,
    FunctionDefinition,
    AssociatedConstant,
    RecordImplementation,
}

impl MetaItem {
    pub const ALL: [Self; 12] = [
        Self::Option,
        Self::Record,
        Self::Field,
        Self::FieldType,
        Self::DeclarationType,
        Self::DeclarationName,
        Self::BodyExpr,
        Self::FunctionCall,
        Self::CallArgument,
        Self::FunctionDefinition,
        Self::AssociatedConstant,
        Self::RecordImplementation,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Option => "core::Option",
            Self::Record => "core::meta::Record",
            Self::Field => "core::meta::Field",
            Self::FieldType => "core::meta::FieldType",
            Self::DeclarationType => "core::meta::DeclarationType",
            Self::DeclarationName => "core::meta::DeclarationName",
            Self::BodyExpr => "core::meta::BodyExpr",
            Self::FunctionCall => "core::meta::FunctionCall",
            Self::CallArgument => "core::meta::CallArgument",
            Self::FunctionDefinition => "core::meta::FunctionDefinition",
            Self::AssociatedConstant => "core::meta::AssociatedConstant",
            Self::RecordImplementation => "core::meta::RecordImplementation",
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrozenHandleKind {
    Record,
    Field(usize),
    FieldType(usize),
}

/// Owned transport for an already checked `core::meta` result.
///
/// This is a copy of the value, not a second generated syntax tree. Record
/// fields and enum cases retain their declared names, so core source order is
/// irrelevant to decoding. No database-lifetime ID crosses this boundary.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FrozenMetaValue {
    Unit,
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
        fields: Vec<Self>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrozenOutputShape {
    Function,
    Functions,
    Implementation,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenGeneration {
    pub target: FrozenOwner,
    /// Source position anchors diagnostics. Item identity uses the resolved
    /// nominal path, so inserting an earlier sibling does not change it.
    pub target_span_start: usize,
    /// The authored generation attribute, for output-stage construction errors.
    pub site_span: Option<std::ops::Range<usize>>,
    pub provider: FrozenOwner,
    pub attribute_index: usize,
    pub shape: FrozenOutputShape,
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
