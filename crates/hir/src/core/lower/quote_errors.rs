//! Typed quotation faults. Messages are rendered only at diagnostic boundaries.

macro_rules! quotation_faults {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $message:literal,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, salsa::Update)]
        pub enum $name { $($variant,)* }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(match self { $(Self::$variant => $message,)* })
            }
        }
    };
}

quotation_faults! {
    /// Failures while locating, reifying and copying checked quotation data.
    QuoteConstructionFault {
        AuthoredItemLocatorIsAmbiguous => "authored item locator is ambiguous",
        ConstPredicateNeedsCheckedNameTransplantation => "this const predicate is not supported in a quote yet",
        NameBelongsToAnotherTemplateMember => "quoted name belongs to another template member",
        BinderGroupAlreadyBoundInAnEnclosingPattern => "binder group already bound in an enclosing pattern",
        BinderGroupHasNoHygieneRecord => "binder group has no hygiene record",
        BinderGroupIsNotBoundInThisScope => "binder group is not bound in this scope",
        FieldReferenceCodeCannotStandForAPattern => "field reference code cannot stand for a pattern",
        PatternCodeCannotStandForAnExpression => "pattern code cannot stand for an expression",
        PatternSpliceNeedsACompilerIssuedBinder => "pattern splice needs a compiler-issued binder",
        PatternSpliceRequiresCodePat => "pattern splice requires Code<Pat>",
        PatternSpliceSlotIsMissing => "pattern splice slot is missing",
        VariantBinderNeedsAnEnumOwner => "variant binder needs an enum owner",
        ArmSpliceHasNoFrozenCode => "arm splice has no frozen code",
        ArmSpliceHasNoFrozenNode => "arm splice has no frozen node",
        ArmSpliceNeedsCodeArm => "arm splice needs Code<Arm>",
        ArmSpliceNeedsAQuotedArmList => "arm splice needs a quoted arm list",
        ArmListSpliceNeedsCodeArm => "arm-list splice needs Code<Arm>",
        ArmListSpliceNeedsAFragmentSource => "arm-list splice needs a fragment source",
        AuthorizedRecordNameIsMissing => "authorized record name is missing",
        BinderFieldIndexIsOutOfBounds => "binder field index is out of bounds",
        CheckedBodyChangedAfterReadBack => "checked body changed after read-back",
        CompilerIssuedBinderCodeCannotStandForItems => "compiler-issued binder code cannot stand for items",
        EmptyItemCodeHasSplices => "empty item code has splices",
        ExpectedAQuotedGenerationResult => "expected a quoted generation result",
        ExpectedAnAuthorizedFieldHandle => "expected an authorized field handle",
        ExpectedFixedStringData => "expected fixed string data",
        FieldHandleHasAnInvalidOwnerVariantOrFieldCoordinate => "field handle has an invalid owner, variant or field coordinate",
        FieldNameRequiresASelectedFieldList => "field name requires a selected field list",
        GeneratedNameMustBeAnIdentifier => "generated name must be an identifier that is not a keyword, at most 256 bytes long",
        GeneratedTargetHasNoLexicalParent => "generated target has no lexical parent",
        GeneratedTargetIsAbsentFromAuthoredHIR => "generated target is absent from authored HIR",
        GenerationFieldNameIsMissing => "generation field name is missing",
        GenerationRequiresItemCode => "generation requires item code",
        GenerationStringIsNotUTF8 => "generation string is not UTF-8",
        InvalidGeneratedDeclarationName => "invalid generated declaration name",
        NegativeQuotedScalarRequiresSignedLiteralLowering => "negative quoted scalar requires signed literal lowering",
        QualifiedFragmentConstantNeedsACheckedBody => "qualified fragment constant needs a checked body",
        QuoteOwnerBodyIsMissing => "quote owner body is missing",
        QuoteOwnerFileIsMissing => "quote owner file is missing",
        QuoteSiteNoLongerNamesAQuote => "quote site no longer names a quote",
        AssociatedConstantBodyWasNotCopied => "quoted associated constant body was not copied",
        AssociatedConstantCountExceedsU32 => "quoted associated constant count exceeds u32",
        AssociatedConstantHeaderNeedsRewriting => "this associated constant header is not supported in a quote yet",
        AssociatedConstantIsNotACheckedPrimitiveType => "quoted associated constant is not a checked primitive type",
        AssociatedConstantTypeIsMissing => "quoted associated constant type is missing",
        AssociatedConstantValueIsMissing => "quoted associated constant value is missing",
        AssociatedMemberTypesChangedAfterFreezing => "quoted associated member types changed after freezing",
        AssociatedTypeNeedsALiteralName => "quoted associated type needs a literal name",
        AssociatedTypeNeedsCheckedReification => "this associated type is not supported in a quote yet",
        AssociatedTypeValueIsMissing => "quoted associated type value is missing",
        AuthoredFunctionLocatorHasNoMatch => "quoted authored function locator has no match",
        AuthoredFunctionLocatorIsAmbiguous => "quoted authored function locator is ambiguous",
        BinderIsClassifiedTwice => "quoted binder is classified twice",
        BodyCopyIsMissing => "quoted body copy is missing",
        CastHasNoFrozenType => "quoted cast has no recorded target type",
        CodeRootIsMissing => "quoted code root is missing",
        CodeSpliceHasNoFrozenNode => "quoted code splice has no frozen node",
        CodeSpliceRequiresFragmentTransplantation => "this code splice is not supported in a quote yet",
        ConstParameterDefaultNeedsCheckedReification => "this const parameter default is not supported in a quote yet",
        ConstParameterTypeIsNotACheckedScalar => "quoted const parameter type is not a checked scalar",
        ConstPredicateCountExceedsU32 => "quoted const predicate count exceeds u32",
        ConstructorKindDisagreesWithItsNominalOwner => "quoted constructor kind disagrees with its nominal owner",
        ConstructorOwnerIsAbsentFromAuthoredHIR => "quoted constructor owner is absent from authored HIR",
        DeclarationNameIsMissing => "quoted declaration name is missing",
        EffectKeyNeedsCheckedReification => "this effect key is not supported in a quote yet",
        EffectUseHasNoBoundName => "quoted effect use has no bound name",
        ExpressionFragmentNeedsCheckedNameTransplantation => "this expression fragment is not supported in a quote yet",
        ExpressionSpliceNeedsExpressionCode => "quoted expression splice needs expression code",
        FieldLabelHasNoCheckedField => "quoted field label has no checked field",
        FieldTypeIsMissing => "quoted field type is missing",
        FragmentGenericUseNeedsACheckedBody => "quoted fragment generic use needs a checked body",
        FragmentVariantIsClassifiedTwice => "quoted fragment variant is classified twice",
        FunctionBodyIsMissing => "quoted function body is missing",
        FunctionBodyRequiresASpliceOrNameRewrite => "quoted expression form was not refused by the quote checker",
        FunctionContainsAnInvalidCondition => "quoted function contains an invalid condition",
        FunctionContainsAnInvalidExpression => "quoted function contains an invalid expression",
        FunctionContainsAnInvalidPattern => "quoted function contains an invalid pattern",
        FunctionContainsAnInvalidStatement => "quoted function contains an invalid statement",
        FunctionGenericBoundsAndDefaultsNeedCheckedReification => "these function generic bounds and defaults are not supported in a quote yet",
        FunctionGenericBoundsChangedAfterFreezing => "quoted function generic bounds changed after freezing",
        FunctionGenericIndexIsMissing => "quoted function generic index is missing",
        FunctionHasNoCheckedSignature => "quoted function has no checked signature",
        FunctionHeaderNeedsNameAndTypeRewriting => "this function header is not supported in a quote yet",
        FunctionParametersAreMissing => "quoted function parameters are missing",
        FunctionPathHasNoCheckedRootOrBinder => "this path is not supported in a quote yet",
        FunctionPathHasNoFinalItem => "quoted function path has no final item",
        FunctionPathNeedsGenericArgumentRewriting => "this function path is not supported in a quote yet",
        FunctionResultTypeIsMissing => "quoted function result type is missing",
        FunctionResultTypeNeedsCheckedRewriting => "this function result type is not supported in a quote yet",
        GenerationCurrentlyRequiresAFunctionOrTraitImpl => "quoted generation currently requires a function or trait impl",
        GenericFieldTypeNeedsCheckedReification => "this generic field type is not supported in a quote yet",
        GenericPathWasNotCopied => "quoted generic path was not copied",
        GenericUseHasNoParameter => "quoted generic use has no parameter",
        ImplHeaderNeedsCheckedTransplantation => "this impl header is not supported in a quote yet",
        ImplMemberNamesNeedCheckedTransplantation => "these impl member names are not supported in a quote yet",
        ImplMemberNeedsCheckedTransplantation => "this impl member is not supported in a quote yet",
        ImplRequiresATrait => "quoted impl requires a trait",
        ImplSelfTypeIsMissing => "quoted impl self type is missing",
        ImplSelfTypeMustSpliceTheTargetRecord => "quoted impl self type must splice the target record",
        ImplTraitArgumentsChangedAfterFreezing => "quoted impl trait arguments changed after freezing",
        ImplTraitArgumentsOnAnIntermediatePathNeedReification => "these impl trait arguments on an intermediate path are not supported in a quote yet",
        ImplTraitHasNoCheckedRoot => "quoted impl trait has no checked root",
        ImplTraitPathHasNoFinalItem => "quoted impl trait path has no final item",
        ImplTraitPathIsMissing => "quoted impl trait path is missing",
        ImplTraitPathSegmentIsMissing => "quoted impl trait path segment is missing",
        ImplTraitRootIsAmbiguous => "quoted impl trait root is ambiguous",
        ItemCountExceedsUsize => "quoted item count exceeds usize",
        ItemListNeedsCheckedTransplantation => "this item list is not supported in a quote yet",
        ItemPathHasNoItem => "quoted item path has no item",
        ItemPathIsNotASimpleItemPath => "quoted item path is not a simple item path",
        ItemSpliceIsNotItemCode => "quoted item splice is not item code",
        ItemSplicePositionChanged => "quoted item splice position changed",
        KindBoundNeedsCheckedReification => "this kind bound is not supported in a quote yet",
        LetConditionNeedsPatternTransplantation => "this let condition is not supported in a quote yet",
        LetTypeHasNoCheckedRoot => "quoted let type has no checked root",
        LocalBinderWasNotCopied => "quoted local binder was not copied",
        LocalTypeIsClassifiedTwice => "quoted local type is classified twice",
        LocalUseWasNotCopied => "quoted local use was not copied",
        MemberHoleHasNoCheckedField => "quoted member hole has no checked field",
        MemberHoleIsClassifiedTwice => "quoted member hole is classified twice",
        MemberHoleIsMissing => "quoted member hole is missing",
        MemberSlotExceedsU32 => "quoted member slot exceeds u32",
        MethodCallNeedsCheckedGenericArgumentRewriting => "this method call is not supported in a quote yet",
        MethodCountExceedsU32 => "quoted method count exceeds u32",
        MethodEffectsChangedAfterFreezing => "quoted method effects changed after freezing",
        MethodSignaturesChangedAfterFreezing => "quoted method signatures changed after freezing",
        MethodWhereBoundChangedAfterFreezing => "quoted method where bound changed after freezing",
        MethodWherePredicateChangedAfterFreezing => "quoted method where predicate changed after freezing",
        MethodWherePredicatesChangedAfterFreezing => "quoted method where predicates changed after freezing",
        NameSpliceHasNoCheckedValue => "quoted name splice has no checked value",
        NominalTypeIsAbsentFromAuthoredHIR => "quoted nominal type is absent from authored HIR",
        OpenNameHasNoDeclaration => "quoted open name has no declaration",
        OpenNameWasNotCopied => "quoted open name was not copied",
        ParameterIndexIsMissing => "quoted parameter index is missing",
        ParameterNeedsCheckedRewriting => "this parameter is not supported in a quote yet",
        ParameterPathWasNotCopied => "quoted parameter path was not copied",
        ParameterTypeCountChangedAfterFreezing => "quoted parameter type count changed after freezing",
        ParameterTypeIsMissing => "quoted parameter type is missing",
        ParameterUseHasNoName => "quoted parameter use has no name",
        PathIsClassifiedTwice => "quoted path is classified twice",
        PatternIsNotACheckedBinder => "quoted pattern is not a checked binder",
        PatternNeedsACheckedBinder => "quoted pattern needs a checked binder",
        PatternNeedsClassificationBeforeTransplantation => "this pattern form is not supported in a quote yet",
        PrimitiveSignatureTypeIsUnsupported => "quoted primitive signature type is unsupported",
        QualifiedConstantLostItsCheckedTrait => "quoted qualified constant lost its checked trait",
        QualifiedConstantMemberIsMissing => "quoted qualified constant member is missing",
        QualifiedConstantPathDisappeared => "quoted qualified constant path disappeared",
        QualifiedConstantReceiverIsUnsupported => "quoted qualified constant receiver is unsupported",
        QualifiedConstantWasNotCopied => "quoted qualified constant was not copied",
        QualifiedTraitIsAbsentFromAuthoredHIR => "quoted qualified trait is absent from authored HIR",
        RecordConstructionHasNoCheckedNominalOwner => "quoted record construction has no checked nominal owner",
        RecordPatternHasNoCheckedNominalConstructor => "quoted record pattern has no checked nominal constructor",
        RootFileIsMissing => "quoted root file is missing",
        RootedFunctionHasNoFrozenIdentity => "quoted rooted function has no frozen identity",
        RootedFunctionWasNotCopied => "quoted rooted function was not copied",
        RootedPathWasNotCopied => "quoted rooted path was not copied",
        RootedTypePathHasNoFinalItem => "quoted rooted type path has no final item",
        ScalarDoesNotMatchItsPrimitiveType => "quoted scalar does not match its primitive type",
        ScalarHasNoSupportedPrimitiveType => "quoted scalar has no supported primitive type",
        SelfNameRequiresAMethod => "quoted self name requires a method",
        SelfNeedsADestinationMethod => "quoted self needs a destination method",
        SelfParameterNeedsAMethod => "quoted self parameter needs a method",
        SpliceCountChangedAfterFreezing => "quoted splice count changed after freezing",
        SpliceHasNoCheckedFragmentSource => "quoted splice has no checked fragment source",
        SpliceMustBeAScalarValue => "quoted splice must be a scalar value",
        SpliceSlotIsMissing => "quoted splice slot is missing",
        TargetAssociatedBoundArgumentIsMissing => "quoted target associated bound argument is missing",
        TargetBoundArgumentIsMissing => "quoted target bound argument is missing",
        TargetBoundPathNeedsCheckedReification => "this target bound path is not supported in a quote yet",
        TargetBoundTypeIsMissing => "quoted target bound type is missing",
        TargetBoundTypeNeedsCheckedReification => "this target bound type is not supported in a quote yet",
        TargetConstBinderIsMissing => "quoted target const binder is missing",
        TargetConstBoundArgumentNeedsCheckedReification => "this target const bound argument is not supported in a quote yet",
        TargetConstParameterTypeIsUnsupported => "quoted target const parameter type is unsupported",
        TargetConstPathNeedsCheckedReification => "this target const path is not supported in a quote yet",
        TargetConstPredicateCountExceedsU32 => "quoted target const predicate count exceeds u32",
        TargetConstPredicateNeedsCheckedReification => "this target const predicate is not supported in a quote yet",
        TargetParameterCountChanged => "quoted target parameter count changed",
        TargetParameterCountChangedAfterFreezing => "quoted target parameter count changed after freezing",
        TargetParameterHasNoName => "quoted target parameter has no name",
        TargetQualifiedBoundTraitIsMissing => "quoted target qualified bound trait is missing",
        TargetTypeParameterHasAConstType => "quoted target type parameter has a const type",
        TargetWhereBinderIsMissing => "quoted target where binder is missing",
        TargetWhereBoundIsMissing => "quoted target where bound is missing",
        TargetWhereBoundIsNotATrait => "quoted target where bound is not a trait",
        TargetWhereTypeIsMissing => "quoted target where type is missing",
        TargetWhereTypeIsNotAnOwnerBinder => "quoted target where type is not an owner binder",
        TargetWhereTypeNeedsCheckedReification => "this target where type is not supported in a quote yet",
        TraitBoundArgumentsNeedCheckedReification => "these trait bound arguments are not supported in a quote yet",
        TraitBoundHasNoCheckedRoot => "quoted trait bound has no checked root",
        TraitBoundPathIsMissing => "quoted trait bound path is missing",
        TraitBoundRootIsAmbiguous => "quoted trait bound root is ambiguous",
        TraitBoundRootIsMissing => "quoted trait bound root is missing",
        TraitIsAbsentFromAuthoredHIR => "quoted trait is absent from authored HIR",
        TraitTypeIsAbsentFromAuthoredHIR => "quoted trait type is absent from authored HIR",
        TuplePatternHasNoCheckedNominalConstructor => "quoted tuple pattern has no checked nominal constructor",
        TypeHoleIsMissing => "quoted type hole is missing",
        TypeLacksTargetAuthority => "quoted type lacks target authority",
        TypeParameterHasAConstType => "quoted type parameter has a const type",
        TypeSpliceIsNotATypeHole => "quoted type splice is not a type hole",
        TypeSpliceNeedsACheckedTargetContext => "quoted type splice needs a checked target context",
        VariantHasNoAuthoredName => "quoted variant has no authored name",
        VariantIndexIsOutsideItsAuthoredEnum => "quoted variant index is outside its authored enum",
        VariantSiteIsClassifiedTwice => "quoted variant site is classified twice",
        VariantSiteWasNotCopied => "quoted variant site was not copied",
        WhereBoundChangedAfterFreezing => "quoted where bound changed after freezing",
        WhereCompositionHoleIsNotCode => "quoted where composition hole is not code",
        WhereFragmentHasNoTemplate => "quoted where fragment has no template",
        WhereFragmentHasTheWrongKind => "quoted where fragment has the wrong kind",
        WhereFragmentIsNotAQuote => "quoted where fragment is not a quote",
        WhereGraphEdgeIsMissing => "quoted where graph edge is missing",
        WhereHoleIsNotCode => "quoted where hole is not code",
        WhereHolesChangedAfterFreezing => "quoted where holes changed after freezing",
        WhereOwnerBodyIsMissing => "quoted where owner body is missing",
        WhereOwnerFileIsMissing => "quoted where owner file is missing",
        WherePredicateNeedsCheckedReification => "this where predicate is not supported in a quote yet",
        WherePredicatesChangedAfterFreezing => "quoted where predicates changed after freezing",
        WhereSiteChanged => "quoted where site changed",
        ReadBackCastPrimitiveIsUnsupported => "read-back cast primitive is unsupported",
        ReadBackGenericHasNoDestinationBinding => "read-back generic has no destination binding",
        ReadBackGenericWasNotCopied => "read-back generic was not copied",
        ReadBackMethodFileIsMissing => "read-back method file is missing",
        ReadBackMethodHasNoCheckedSelection => "read-back method has no checked selection",
        ReadBackMethodIsClassifiedTwice => "read-back method is classified twice",
        ReadBackNominalTypeIsAbsentFromAuthoredHIR => "read-back nominal type is absent from authored HIR",
        ReadBackOwnerGenericHasNoDestinationBinding => "read-back owner generic has no destination binding",
        ReadBackParameterHasNoDestinationBinding => "read-back parameter has no destination binding",
        ReadBackQualifiedConstantWasNotCopied => "read-back qualified constant was not copied",
        ReadBackRecordConstructorNeedsANominalType => "read-back record constructor needs a nominal type",
        ReadBackSelectedMethodWasNotCopied => "read-back selected method was not copied",
        ReadBackTypeNeedsANominalIdentity => "read-back type needs a nominal identity",
        RecordGenericParameterCountChanged => "record generic parameter count changed",
        VariantFieldAccessRequiresAMatchingPatternBinder => "variant field access requires a matching pattern binder",
    }
}

quotation_faults! {
    /// Checked identities that changed after quotation transport.
    QuoteHygieneFault {
        ConstWhereBodyChangedInOutput => "quoted const where body changed in output",
        ConstWhereGenericChangedMeaningInOutput => "quoted const where generic changed meaning in output",
        ConstructorChangedOwnerOrVariantIndex => "quoted constructor changed owner or variant index",
        EffectBindingChangedMeaningInOutput => "quoted effect binding changed meaning in output",
        FunctionBodyDisappearedBeforeHygieneChecking => "quoted function body disappeared before hygiene checking",
        GenericBodyUseChangedMeaningInOutput => "quoted generic body use changed meaning in output",
        LocalBinderChangedMeaningInOutput => "quoted local binder changed meaning in output",
        MethodCallChangedSelectionInOutput => "quoted method call changed selection in output",
        MethodCallDisappearedBeforeHygieneChecking => "quoted method call disappeared before hygiene checking",
        ParameterChangedMeaningInOutput => "quoted parameter changed meaning in output",
    }
}

impl QuoteConstructionFault {
    /// Whether an author can reach this fault from a provider that type
    /// checks. Every other fault breaks the compiler's own invariants and is
    /// reported as an internal error.
    pub fn is_user_facing(self) -> bool {
        use QuoteConstructionFault::*;
        matches!(
            self,
            AssociatedConstantHeaderNeedsRewriting
                | AssociatedConstantIsNotACheckedPrimitiveType
                | AssociatedTypeNeedsALiteralName
                | AssociatedTypeNeedsCheckedReification
                | BinderGroupAlreadyBoundInAnEnclosingPattern
                | BinderGroupIsNotBoundInThisScope
                | CodeSpliceRequiresFragmentTransplantation
                | CompilerIssuedBinderCodeCannotStandForItems
                | ConstParameterDefaultNeedsCheckedReification
                | ConstParameterTypeIsNotACheckedScalar
                | ConstPredicateNeedsCheckedNameTransplantation
                | ConstructorOwnerIsAbsentFromAuthoredHIR
                | EffectKeyNeedsCheckedReification
                | ExpressionFragmentNeedsCheckedNameTransplantation
                | FieldNameRequiresASelectedFieldList
                | FieldReferenceCodeCannotStandForAPattern
                | FunctionGenericBoundsAndDefaultsNeedCheckedReification
                | FunctionHeaderNeedsNameAndTypeRewriting
                | FunctionPathHasNoCheckedRootOrBinder
                | FunctionPathNeedsGenericArgumentRewriting
                | FunctionResultTypeNeedsCheckedRewriting
                | GeneratedNameMustBeAnIdentifier
                | GenerationCurrentlyRequiresAFunctionOrTraitImpl
                | GenerationRequiresItemCode
                | GenerationStringIsNotUTF8
                | GenericFieldTypeNeedsCheckedReification
                | ImplHeaderNeedsCheckedTransplantation
                | ImplMemberNamesNeedCheckedTransplantation
                | ImplMemberNeedsCheckedTransplantation
                | ImplRequiresATrait
                | ImplSelfTypeMustSpliceTheTargetRecord
                | ImplTraitArgumentsOnAnIntermediatePathNeedReification
                | InvalidGeneratedDeclarationName
                | ItemListNeedsCheckedTransplantation
                | KindBoundNeedsCheckedReification
                | NegativeQuotedScalarRequiresSignedLiteralLowering
                | NominalTypeIsAbsentFromAuthoredHIR
                | ParameterNeedsCheckedRewriting
                | PatternCodeCannotStandForAnExpression
                | PatternNeedsClassificationBeforeTransplantation
                | PatternSpliceNeedsACompilerIssuedBinder
                | PrimitiveSignatureTypeIsUnsupported
                | QualifiedConstantReceiverIsUnsupported
                | QualifiedTraitIsAbsentFromAuthoredHIR
                | ScalarDoesNotMatchItsPrimitiveType
                | ScalarHasNoSupportedPrimitiveType
                | SelfNameRequiresAMethod
                | SelfNeedsADestinationMethod
                | SelfParameterNeedsAMethod
                | SpliceMustBeAScalarValue
                | TargetBoundPathNeedsCheckedReification
                | TargetBoundTypeNeedsCheckedReification
                | TargetConstBoundArgumentNeedsCheckedReification
                | TargetConstParameterTypeIsUnsupported
                | TargetConstPathNeedsCheckedReification
                | TargetConstPredicateNeedsCheckedReification
                | TargetWhereTypeNeedsCheckedReification
                | TraitBoundArgumentsNeedCheckedReification
                | TraitIsAbsentFromAuthoredHIR
                | TraitTypeIsAbsentFromAuthoredHIR
                | TypeLacksTargetAuthority
                | TypeSpliceNeedsACheckedTargetContext
                | VariantFieldAccessRequiresAMatchingPatternBinder
                | WherePredicateNeedsCheckedReification
        )
    }
}
