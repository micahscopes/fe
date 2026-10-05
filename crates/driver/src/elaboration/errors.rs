//! Owned error kinds crossing the elaboration input/output boundary.

use hir::{analysis::semantic::ctfe::ConstDemandKind, lower::GeneratedQuoteFailure};

use super::{InvocationErrorKind, RecordInputError, construction::ConstructionError};

// Every protocol fault owns its typed variant and diagnostic text in one table.
macro_rules! freezing_protocol_faults {
    ($($kind:ident => $message:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum FreezingProtocolFault { $($kind),+ }
        impl std::fmt::Display for FreezingProtocolFault {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(match self { $(Self::$kind => $message),+ })
            }
        }
    };
}

freezing_protocol_faults! {
    BinderFieldIndexIsOutOfBounds => "binder field index is out of bounds",
    BinderVariantIndexIsOutOfBounds => "binder variant index is out of bounds",
    ExpectedCoreMetaEnumDefinition => "expected core meta enum definition",
    GenerationOwnerFileIsMissing => "generation owner file is missing",
    GenerationOwnerPathIsMissing => "generation owner path is missing",
    InvalidCheckedBodyCodeOrigin => "invalid checked-body code origin",
    InvalidCoreMetaEnumVariant => "invalid core meta enum variant",
    InvalidCoreMetaVariantType => "invalid core meta variant type",
    QuoteBinderIsClassifiedTwice => "quote binder is classified twice",
    QuoteHasNoCheckedNameClassification => "quote has no checked name classification",
    QuoteLocalUseHasNoBinder => "quote local use has no binder",
    QuoteOwnerBodyIsMissing => "quote owner body is missing",
    QuoteOwnerDoesNotTypeCheck => "quote owner does not type check",
    QuoteOwnerIsNotAFunction => "quote owner is not a function",
    QuoteHoleSlotIsOutOfBounds => "quote hole slot is out of bounds",
    QuoteOwnerLocatorIsMissing => "quote owner locator is missing",
    QuoteSiteNoLongerNamesAQuote => "quote site no longer names a quote",
    SelfConstructionOutsideAMethodNeedsTransplantation => "quoted Self construction outside a method needs transplantation",
    SelfPathNeedsCheckedTransplantation => "quoted Self path needs checked transplantation",
    AssociatedTypeHasExtraCheckedNames => "quoted associated type has extra checked names",
    CastNeedsACheckedPrimitiveType => "a cast inside a quote must name a primitive type",
    ConstWhereLocalOrValueUseNeedsCheckedReification => "quoted const where local or value use needs checked reification",
    EffectUseNeedsASimpleBoundPath => "quoted effect use needs a simple bound path",
    FragmentConstPredicateNeedsCheckedReification => "quoted fragment const predicate needs checked reification",
    FunctionArgumentNeedsAType => "quoted function argument needs a type",
    FunctionGenericIdentityIsMissing => "quoted function generic identity is missing",
    FunctionGenericOwnerIsMissing => "quoted function generic owner is missing",
    FunctionHasNoCheckedGenericOwner => "quoted function has no checked generic owner",
    FunctionIntermediateTypeArgumentsNeedReification => "quoted function intermediate type arguments need reification",
    FunctionPathSegmentIsMissing => "quoted function path segment is missing",
    FunctionRootChangedShape => "quoted function root changed shape",
    FunctionRootHasNoAuthoredFunction => "quoted function root has no authored function",
    FunctionTypeArgumentHasExtraCheckedNames => "quoted function type argument has extra checked names",
    FunctionTypeArgumentIsMissing => "quoted function type argument is missing",
    FunctionTypeArgumentNeedsCheckedReification => "quoted function type argument needs checked reification",
    GenericBodyUseNeedsASimplePath => "quoted generic body use needs a simple path",
    GenericBodyUseNeedsCheckedReification => "quoted generic body use needs checked reification",
    GenericBodyUseOutsideAFunctionNeedsTransplantation => "quoted generic body use outside a function needs transplantation",
    GenericSignatureBindingChanged => "quoted generic signature binding changed",
    ImplTraitArgumentHasExtraCheckedNames => "quoted impl trait argument has extra checked names",
    ImplTraitArgumentNeedsACheckedType => "quoted impl trait argument needs a checked type",
    ImplTraitArgumentNeedsCheckedReification => "quoted impl trait argument needs checked reification",
    ImplTraitTypeArgumentIsMissing => "quoted impl trait type argument is missing",
    LocalTypeHasExtraCheckedNames => "quoted local type has extra checked names",
    LocalTypeNeedsCheckedReification => "quoted local type needs checked reification",
    LocalUseChangedFunctionOwner => "quoted local use changed function owner",
    MethodWhereBoundTypeIsMissing => "quoted method where bound type is missing",
    MethodWhereBoundTypeNeedsCheckedReification => "quoted method where bound type needs checked reification",
    MethodWhereFragmentNeedsCheckedReification => "quoted method where fragment needs checked reification",
    NominalTypeFileIsMissing => "quoted nominal type file is missing",
    ParameterUseChangedFunctionOwner => "quoted parameter use changed function owner",
    QualifiedConstantHasNoCheckedTrait => "quoted qualified constant has no checked trait",
    QualifiedConstantHasNoPath => "quoted qualified constant has no path",
    QualifiedConstantHasNoReceiverType => "quoted qualified constant has no receiver type",
    QualifiedConstantHasNoRoot => "quoted qualified constant has no root",
    QualifiedConstantNeedsAPrimitiveReceiver => "quoted qualified constant needs a primitive receiver",
    QualifiedConstantNeedsARootedTrait => "quoted qualified constant needs a rooted trait",
    QualifiedConstantNeedsASimpleMember => "quoted qualified constant needs a simple member",
    QualifiedConstantOutsideAFunctionNeedsTransplantation => "quoted qualified constant outside a function needs transplantation",
    RootedFunctionOutsideAFunctionNeedsTransplantation => "quoted rooted function outside a function needs transplantation",
    SelfOutsideAMethodNeedsTransplantation => "quoted self outside a method needs transplantation",
    SignatureHasExtraCheckedNames => "quoted signature has extra checked names",
    SignatureNameChangedSite => "quoted signature name changed site",
    SignaturePathSegmentIsMissing => "quoted signature path segment is missing",
    SignatureTypeHasNoCheckedName => "quoted signature type has no checked name",
    TraitFileIsMissing => "quoted trait file is missing",
    TraitSignatureNameChangedSite => "quoted trait signature name changed site",
    WhereTypeHasExtraCheckedNames => "quoted where type has extra checked names",
    WhereTypeIsMissing => "quoted where type is missing",
    WhereTypeNeedsCheckedReification => "quoted where type needs checked reification",
    TargetConstParameterHasNoType => "target const parameter has no type",
    TargetConstParameterTypeIsNotScalar => "target const parameter type is not scalar",
    TypeLevelValuesAreNotGeneratedDeclarations => "type-level values are not generated declarations",
    UnsupportedGeneratedValueShape => "generation transport accepts quoted code, scalars, structural handles and declaration names",
    UnexpectedCoreMetaVariant => "unexpected core meta variant",
    UnexpectedDeclarationNameVariant => "unexpected declaration name variant",
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

pub use hir::analysis::semantic::ctfe::CtfeErrorKind as EvaluationFault;

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
            Self::ProjectDescriptionLimit { actual, maximum } => write!(f, "project generated nodes limit exceeded: {actual} > {maximum}"),
            Self::ProviderFileMissing => f.write_str("generation provider file is missing"),
            Self::ProviderPathMissing => f.write_str("generation provider path is missing"),
            Self::OwnerSpanMissing => f.write_str("generation owner has no source span"),
            Self::Owner(error) => write!(f, "invalid generation owner: {error}"),
            Self::Lowering(error) | Self::Hygiene(error) => write!(f, "invalid generated declaration: {error}"),
        }
    }
}
