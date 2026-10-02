//! Owned error kinds crossing the elaboration input/output boundary.

use hir::{analysis::semantic::ctfe::ConstDemandKind, lower::GeneratedQuoteFailure};

use super::{InvocationErrorKind, RecordInputError, construction::ConstructionError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezingProtocolFault {
    BinderFieldIndexIsOutOfBounds,
    BinderVariantIndexIsOutOfBounds,
    DuplicateCoreMetaField,
    ExpectedCoreMetaEnumDefinition,
    ExpectedCoreMetaRecordDefinition,
    GenerationOwnerFileIsMissing,
    GenerationOwnerPathIsMissing,
    InvalidCheckedBodyCodeOrigin,
    InvalidCoreMetaArray,
    InvalidCoreMetaEnumVariant,
    InvalidCoreMetaRecordFields,
    InvalidCoreMetaRecordType,
    InvalidCoreMetaTuple,
    InvalidCoreMetaVariantType,
    QuoteBinderIsClassifiedTwice,
    QuoteFunctionScopeIsMissing,
    QuoteHasNoCheckedNameClassification,
    QuoteLocalUseHasNoBinder,
    QuoteOwnerBodyIsMissing,
    QuoteOwnerDoesNotTypeCheck,
    QuoteOwnerIsNotAFunction,
    QuoteOwnerLocatorIsMissing,
    QuoteSiteNoLongerNamesAQuote,
    SelfConstructionOutsideAMethodNeedsTransplantation,
    SelfPathNeedsCheckedTransplantation,
    AssociatedTypeHasExtraCheckedNames,
    CastNeedsACheckedPrimitiveTypeBeforeTransplantation,
    ConstWhereLocalOrValueUseNeedsCheckedReification,
    EffectUseNeedsASimpleBoundPath,
    FragmentConstPredicateNeedsCheckedReification,
    FunctionArgumentNeedsAType,
    FunctionGenericIdentityIsMissing,
    FunctionGenericOwnerIsMissing,
    FunctionHasNoCheckedGenericOwner,
    FunctionIntermediateTypeArgumentsNeedReification,
    FunctionPathSegmentIsMissing,
    FunctionRootChangedShape,
    FunctionRootHasNoAuthoredFunction,
    FunctionTypeArgumentHasExtraCheckedNames,
    FunctionTypeArgumentIsMissing,
    FunctionTypeArgumentNeedsCheckedReification,
    GenericBodyUseNeedsASimplePath,
    GenericBodyUseNeedsCheckedReification,
    GenericBodyUseOutsideAFunctionNeedsTransplantation,
    GenericSignatureBindingChanged,
    ImplTraitArgumentHasExtraCheckedNames,
    ImplTraitArgumentNeedsACheckedType,
    ImplTraitArgumentNeedsCheckedReification,
    ImplTraitTypeArgumentIsMissing,
    LocalTypeHasExtraCheckedNames,
    LocalTypeNeedsCheckedReification,
    LocalUseChangedFunctionOwner,
    MethodWhereBoundTypeIsMissing,
    MethodWhereBoundTypeNeedsCheckedReification,
    MethodWhereFragmentNeedsCheckedReification,
    NominalTypeFileIsMissing,
    ParameterUseChangedFunctionOwner,
    QualifiedConstantHasNoCheckedTrait,
    QualifiedConstantHasNoPath,
    QualifiedConstantHasNoReceiverType,
    QualifiedConstantHasNoRoot,
    QualifiedConstantNeedsAPrimitiveReceiver,
    QualifiedConstantNeedsARootedTrait,
    QualifiedConstantNeedsASimpleMember,
    QualifiedConstantOutsideAFunctionNeedsTransplantation,
    RootedFunctionOutsideAFunctionNeedsTransplantation,
    SelfOutsideAMethodNeedsTransplantation,
    SignatureHasExtraCheckedNames,
    SignatureNameChangedSite,
    SignaturePathSegmentIsMissing,
    SignatureTypeHasNoCheckedName,
    TraitFileIsMissing,
    TraitSignatureNameChangedSite,
    WhereTypeHasExtraCheckedNames,
    WhereTypeIsMissing,
    WhereTypeNeedsCheckedReification,
    TargetConstParameterHasNoType,
    TargetConstParameterTypeIsNotScalar,
    TypeLevelValuesAreNotGeneratedDeclarations,
    UnexpectedCoreMetaRecord,
    UnexpectedCoreMetaVariant,
    UnexpectedDeclarationNameVariant,
    UnnamedCoreMetaField,
}

impl std::fmt::Display for FreezingProtocolFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BinderFieldIndexIsOutOfBounds => "binder field index is out of bounds",
            Self::BinderVariantIndexIsOutOfBounds => "binder variant index is out of bounds",
            Self::DuplicateCoreMetaField => "duplicate core meta field",
            Self::ExpectedCoreMetaEnumDefinition => "expected core meta enum definition",
            Self::ExpectedCoreMetaRecordDefinition => "expected core meta record definition",
            Self::GenerationOwnerFileIsMissing => "generation owner file is missing",
            Self::GenerationOwnerPathIsMissing => "generation owner path is missing",
            Self::InvalidCheckedBodyCodeOrigin => "invalid checked-body code origin",
            Self::InvalidCoreMetaArray => "invalid core meta array",
            Self::InvalidCoreMetaEnumVariant => "invalid core meta enum variant",
            Self::InvalidCoreMetaRecordFields => "invalid core meta record fields",
            Self::InvalidCoreMetaRecordType => "invalid core meta record type",
            Self::InvalidCoreMetaTuple => "invalid core meta tuple",
            Self::InvalidCoreMetaVariantType => "invalid core meta variant type",
            Self::QuoteBinderIsClassifiedTwice => "quote binder is classified twice",
            Self::QuoteFunctionScopeIsMissing => "quote function scope is missing",
            Self::QuoteHasNoCheckedNameClassification => "quote has no checked name classification",
            Self::QuoteLocalUseHasNoBinder => "quote local use has no binder",
            Self::QuoteOwnerBodyIsMissing => "quote owner body is missing",
            Self::QuoteOwnerDoesNotTypeCheck => "quote owner does not type check",
            Self::QuoteOwnerIsNotAFunction => "quote owner is not a function",
            Self::QuoteOwnerLocatorIsMissing => "quote owner locator is missing",
            Self::QuoteSiteNoLongerNamesAQuote => "quote site no longer names a quote",
            Self::SelfConstructionOutsideAMethodNeedsTransplantation => {
                "quoted Self construction outside a method needs transplantation"
            }
            Self::SelfPathNeedsCheckedTransplantation => {
                "quoted Self path needs checked transplantation"
            }
            Self::AssociatedTypeHasExtraCheckedNames => {
                "quoted associated type has extra checked names"
            }
            Self::CastNeedsACheckedPrimitiveTypeBeforeTransplantation => {
                "quoted cast needs a checked primitive type before transplantation"
            }
            Self::ConstWhereLocalOrValueUseNeedsCheckedReification => {
                "quoted const where local or value use needs checked reification"
            }
            Self::EffectUseNeedsASimpleBoundPath => "quoted effect use needs a simple bound path",
            Self::FragmentConstPredicateNeedsCheckedReification => {
                "quoted fragment const predicate needs checked reification"
            }
            Self::FunctionArgumentNeedsAType => "quoted function argument needs a type",
            Self::FunctionGenericIdentityIsMissing => "quoted function generic identity is missing",
            Self::FunctionGenericOwnerIsMissing => "quoted function generic owner is missing",
            Self::FunctionHasNoCheckedGenericOwner => {
                "quoted function has no checked generic owner"
            }
            Self::FunctionIntermediateTypeArgumentsNeedReification => {
                "quoted function intermediate type arguments need reification"
            }
            Self::FunctionPathSegmentIsMissing => "quoted function path segment is missing",
            Self::FunctionRootChangedShape => "quoted function root changed shape",
            Self::FunctionRootHasNoAuthoredFunction => {
                "quoted function root has no authored function"
            }
            Self::FunctionTypeArgumentHasExtraCheckedNames => {
                "quoted function type argument has extra checked names"
            }
            Self::FunctionTypeArgumentIsMissing => "quoted function type argument is missing",
            Self::FunctionTypeArgumentNeedsCheckedReification => {
                "quoted function type argument needs checked reification"
            }
            Self::GenericBodyUseNeedsASimplePath => "quoted generic body use needs a simple path",
            Self::GenericBodyUseNeedsCheckedReification => {
                "quoted generic body use needs checked reification"
            }
            Self::GenericBodyUseOutsideAFunctionNeedsTransplantation => {
                "quoted generic body use outside a function needs transplantation"
            }
            Self::GenericSignatureBindingChanged => "quoted generic signature binding changed",
            Self::ImplTraitArgumentHasExtraCheckedNames => {
                "quoted impl trait argument has extra checked names"
            }
            Self::ImplTraitArgumentNeedsACheckedType => {
                "quoted impl trait argument needs a checked type"
            }
            Self::ImplTraitArgumentNeedsCheckedReification => {
                "quoted impl trait argument needs checked reification"
            }
            Self::ImplTraitTypeArgumentIsMissing => "quoted impl trait type argument is missing",
            Self::LocalTypeHasExtraCheckedNames => "quoted local type has extra checked names",
            Self::LocalTypeNeedsCheckedReification => "quoted local type needs checked reification",
            Self::LocalUseChangedFunctionOwner => "quoted local use changed function owner",
            Self::MethodWhereBoundTypeIsMissing => "quoted method where bound type is missing",
            Self::MethodWhereBoundTypeNeedsCheckedReification => {
                "quoted method where bound type needs checked reification"
            }
            Self::MethodWhereFragmentNeedsCheckedReification => {
                "quoted method where fragment needs checked reification"
            }
            Self::NominalTypeFileIsMissing => "quoted nominal type file is missing",
            Self::ParameterUseChangedFunctionOwner => "quoted parameter use changed function owner",
            Self::QualifiedConstantHasNoCheckedTrait => {
                "quoted qualified constant has no checked trait"
            }
            Self::QualifiedConstantHasNoPath => "quoted qualified constant has no path",
            Self::QualifiedConstantHasNoReceiverType => {
                "quoted qualified constant has no receiver type"
            }
            Self::QualifiedConstantHasNoRoot => "quoted qualified constant has no root",
            Self::QualifiedConstantNeedsAPrimitiveReceiver => {
                "quoted qualified constant needs a primitive receiver"
            }
            Self::QualifiedConstantNeedsARootedTrait => {
                "quoted qualified constant needs a rooted trait"
            }
            Self::QualifiedConstantNeedsASimpleMember => {
                "quoted qualified constant needs a simple member"
            }
            Self::QualifiedConstantOutsideAFunctionNeedsTransplantation => {
                "quoted qualified constant outside a function needs transplantation"
            }
            Self::RootedFunctionOutsideAFunctionNeedsTransplantation => {
                "quoted rooted function outside a function needs transplantation"
            }
            Self::SelfOutsideAMethodNeedsTransplantation => {
                "quoted self outside a method needs transplantation"
            }
            Self::SignatureHasExtraCheckedNames => "quoted signature has extra checked names",
            Self::SignatureNameChangedSite => "quoted signature name changed site",
            Self::SignaturePathSegmentIsMissing => "quoted signature path segment is missing",
            Self::SignatureTypeHasNoCheckedName => "quoted signature type has no checked name",
            Self::TraitFileIsMissing => "quoted trait file is missing",
            Self::TraitSignatureNameChangedSite => "quoted trait signature name changed site",
            Self::WhereTypeHasExtraCheckedNames => "quoted where type has extra checked names",
            Self::WhereTypeIsMissing => "quoted where type is missing",
            Self::WhereTypeNeedsCheckedReification => "quoted where type needs checked reification",
            Self::TargetConstParameterHasNoType => "target const parameter has no type",
            Self::TargetConstParameterTypeIsNotScalar => {
                "target const parameter type is not scalar"
            }
            Self::TypeLevelValuesAreNotGeneratedDeclarations => {
                "type-level values are not generated declarations"
            }
            Self::UnexpectedCoreMetaRecord => "unexpected core meta record",
            Self::UnexpectedCoreMetaVariant => "unexpected core meta variant",
            Self::UnexpectedDeclarationNameVariant => "unexpected declaration name variant",
            Self::UnnamedCoreMetaField => "unnamed core meta field",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezingLimitKind {
    MetaNodes,
    TypeDepth,
    TypeNodes,
    MetaDepth,
}
impl std::fmt::Display for FreezingLimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MetaNodes => "frozen meta nodes",
            Self::TypeDepth => "quoted type nesting",
            Self::TypeNodes => "quoted type nodes",
            Self::MetaDepth => "frozen meta depth",
        })
    }
}

/// Evaluation faults retain authored messages but never render Salsa IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationFault {
    NotConstEvaluable,
    AssertionFailed(Option<String>),
    InvalidOperation(String),
    InvalidBorrow,
    InvalidProviderUse,
    NonConstCall,
    InvalidBody,
    DivisionByZero,
    ArithmeticOverflow,
    NegativeExponent,
    OutOfBounds,
    VariantMismatch,
    UninitializedLocal,
    StepLimitExceeded,
    RecursionLimitExceeded,
    RecursiveConst,
    Invariant(String),
}
impl std::fmt::Display for EvaluationFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::AssertionFailed(Some(message)) => {
                return write!(f, "assertion failed: {message}");
            }
            Self::InvalidOperation(message) => return f.write_str(message),
            Self::Invariant(message) => {
                return write!(f, "constant evaluation invariant failed: {message}");
            }
            Self::NotConstEvaluable => "expression cannot be evaluated as a constant",
            Self::AssertionFailed(None) => "assertion failed",
            Self::InvalidBorrow => "invalid borrow during constant evaluation",
            Self::InvalidProviderUse => "invalid provider use during constant evaluation",
            Self::NonConstCall => "constant evaluation called a non-const function",
            Self::InvalidBody => "constant evaluation encountered an invalid body",
            Self::DivisionByZero => "division by zero",
            Self::ArithmeticOverflow => "arithmetic overflow",
            Self::NegativeExponent => "negative exponent",
            Self::OutOfBounds => "index out of bounds",
            Self::VariantMismatch => "enum variant mismatch",
            Self::UninitializedLocal => "read of an uninitialized local",
            Self::StepLimitExceeded => "evaluation step limit exceeded",
            Self::RecursionLimitExceeded => "evaluation recursion limit exceeded",
            Self::RecursiveConst => "recursive constant dependency",
        };
        f.write_str(message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElaborationErrorKind {
    MissingTarget,
    Invocation(InvocationErrorKind),
    Input(RecordInputError),
    RequiresConstFunction,
    MissingRecord,
    InvalidEntry,
    UnsupportedOutputShape,
    ProviderDiagnostics(String),
    IssueInput,
    EvaluationBlocked(ConstDemandKind),
    EvaluationFailed {
        fault: EvaluationFault,
        calls: Vec<String>,
    },
    Operation(String),
    Freezing(ConstructionError),
    DescriptionLimit {
        actual: usize,
        maximum: usize,
    },
    ProjectDescriptionLimit {
        actual: usize,
        maximum: usize,
    },
    ProviderFileMissing,
    ProviderPathMissing,
    OwnerSpanMissing,
    Owner(ConstructionError),
    Lowering(GeneratedQuoteFailure),
    OutputDiagnostics(String),
    Hygiene(GeneratedQuoteFailure),
}
impl ElaborationErrorKind {
    /// Preserve the six generation diagnostic classes in one exhaustive mapping.
    pub fn code(&self) -> u16 {
        match self {
            Self::Freezing(ConstructionError::Protocol(_)) => 2,
            Self::Freezing(_) => 3,
            Self::Lowering(_) => 4,
            Self::OutputDiagnostics(_) => 5,
            Self::Hygiene(_) => 6,
            Self::MissingTarget
            | Self::Invocation(_)
            | Self::Input(_)
            | Self::RequiresConstFunction
            | Self::MissingRecord
            | Self::InvalidEntry
            | Self::UnsupportedOutputShape
            | Self::ProviderDiagnostics(_)
            | Self::IssueInput
            | Self::EvaluationBlocked(_)
            | Self::EvaluationFailed { .. }
            | Self::Operation(_)
            | Self::DescriptionLimit { .. }
            | Self::ProjectDescriptionLimit { .. }
            | Self::ProviderFileMissing
            | Self::ProviderPathMissing
            | Self::OwnerSpanMissing
            | Self::Owner(_) => 1,
        }
    }
}
impl std::fmt::Display for ElaborationErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTarget => f.write_str("could not resolve generation target"),
            Self::Invocation(kind) => f.write_str(kind.message()),
            Self::Input(kind) => write!(f, "invalid generation input: {kind}"),
            Self::RequiresConstFunction => f.write_str("generation requires a const function"),
            Self::MissingRecord => f.write_str("builtin Record is missing"),
            Self::InvalidEntry => f.write_str("generation entry must be a safe nongeneric module const function taking Record, without entry requirements or effects; use ordinary checked helper calls and scoped witnesses inside it"),
            Self::UnsupportedOutputShape => f.write_str("generation result has an unsupported shape"),
            Self::ProviderDiagnostics(rendered) | Self::OutputDiagnostics(rendered) => f.write_str(rendered),
            Self::IssueInput => f.write_str("could not issue record input"),
            Self::EvaluationBlocked(demand) => {
                let demand = match demand {
                    ConstDemandKind::Value => "value", ConstDemandKind::Boolean => "boolean",
                    ConstDemandKind::Integer => "integer", ConstDemandKind::Index => "index",
                    ConstDemandKind::ArrayLength => "array length", ConstDemandKind::EnumVariant => "enum variant",
                    ConstDemandKind::Layout => "layout", ConstDemandKind::CallableSelection => "callable selection",
                };
                write!(f, "generation evaluation remained dependent: waiting for a constant {demand}")
            }
            Self::EvaluationFailed { fault, calls } => {
                write!(f, "generation evaluation failed: {fault}")?;
                for call in calls { write!(f, "
  while evaluating `{call}`")?; }
                Ok(())
            }
            Self::Operation(message) => f.write_str(message),
            Self::Freezing(error) => write!(f, "invalid generated declaration: {error}"),
            Self::DescriptionLimit { actual, maximum } => write!(f, "invalid generated declaration: generated description units limit exceeded: {actual} > {maximum}"),
            Self::ProjectDescriptionLimit { actual, maximum } => write!(f, "project generated description units limit exceeded: {actual} > {maximum}"),
            Self::ProviderFileMissing => f.write_str("generation provider file is missing"),
            Self::ProviderPathMissing => f.write_str("generation provider path is missing"),
            Self::OwnerSpanMissing => f.write_str("generation owner has no source span"),
            Self::Owner(error) => write!(f, "invalid generation owner: {error}"),
            Self::Lowering(error) | Self::Hygiene(error) => write!(f, "invalid generated declaration: {error}"),
        }
    }
}
