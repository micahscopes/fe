use crate::{analysis::ty::derive_provider::DeriveProviderId, span::DynLazySpan};

use super::{BuilderCommandListId, ElaborationCtfeContextId, ElaborationRequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ProviderFailureReason {
    MissingBuilderCapability,
    MissingReflectCapability,
    MissingFinish,
    DuplicateFinish,
    CommandAfterFinish,
    InvalidBuilderRequirement,
    InvalidGeneratedMethodName,
    InvalidGeneratedMethodBody,
    InvalidBuilderState,
    UnsupportedProviderBody,
}

impl ProviderFailureReason {
    pub(super) fn diagnostic_message(self) -> &'static str {
        match self {
            Self::MissingBuilderCapability => {
                "provider does not declare mutable ImplBuilder capability"
            }
            Self::MissingReflectCapability => "provider body requires Reflect capability",
            Self::MissingFinish => "provider did not call builder.finish()",
            Self::DuplicateFinish => "provider called builder.finish() more than once",
            Self::CommandAfterFinish => "provider emitted a builder command after finish",
            Self::InvalidBuilderRequirement => "provider emitted an invalid builder requirement",
            Self::InvalidGeneratedMethodName => "provider emitted an invalid generated method name",
            Self::InvalidGeneratedMethodBody => "provider emitted an invalid generated method body",
            Self::InvalidBuilderState => "provider left the impl builder in an invalid state",
            Self::UnsupportedProviderBody => "provider body uses unsupported elaboration CTFE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ProviderOutputStatus<'db> {
    Succeeded {
        commands: BuilderCommandListId<'db>,
    },
    Failed {
        reason: ProviderFailureReason,
        span: DynLazySpan<'db>,
    },
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ProviderOutputId<'db> {
    pub(crate) request: ElaborationRequestId<'db>,
    pub(crate) provider: DeriveProviderId<'db>,
    pub(crate) context: ElaborationCtfeContextId<'db>,
    pub(crate) status: ProviderOutputStatus<'db>,
}
