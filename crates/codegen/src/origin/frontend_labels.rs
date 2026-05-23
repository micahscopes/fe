use mir::{runtime_stmt_export_key, runtime_terminator_export_key};
use sonatina_ir::{InstId, module::FuncRef};

use super::{function_keys::SonatinaFunctionExportKey, sonatina_pre_opt::SonatinaOriginSource};

common::define_origin_string_key! {
    /// Frontend label attached to Sonatina observability rows.
    ///
    /// Fe derives these labels from stable origin export keys before handing
    /// them to Sonatina's external frontend-provenance map.
    pub struct FrontendOriginLabel;
}

/// Fe-owned wrapper around Sonatina's frontend provenance label map.
///
/// Sonatina's external observability API still uses "provenance"; the origin
/// overhaul keeps that spelling at the dependency boundary and uses origin
/// terminology inside Fe-owned APIs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrontendOriginLabelMap {
    inner: sonatina_codegen::object::FrontendProvenanceMap,
}

impl FrontendOriginLabelMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_if_absent(
        &mut self,
        function: FuncRef,
        inst: InstId,
        label: FrontendOriginLabel,
    ) {
        self.inner
            .entry((function, inst))
            .or_insert_with(|| label.as_str().to_string());
    }

    pub fn as_sonatina_frontend_provenance(
        &self,
    ) -> &sonatina_codegen::object::FrontendProvenanceMap {
        &self.inner
    }
}

pub(super) fn pre_opt_source_has_frontend_label(source: SonatinaOriginSource<'_>) -> bool {
    matches!(
        source,
        SonatinaOriginSource::RuntimeStmt(_) | SonatinaOriginSource::RuntimeTerminator(_)
    )
}

pub(super) fn frontend_label_for_pre_opt_source(
    source: SonatinaOriginSource<'_>,
    function_key: &SonatinaFunctionExportKey,
) -> Option<FrontendOriginLabel> {
    let key = match source {
        SonatinaOriginSource::RuntimeStmt(origin) => runtime_stmt_export_key(origin, function_key),
        SonatinaOriginSource::RuntimeTerminator(origin) => {
            runtime_terminator_export_key(origin, function_key)
        }
        SonatinaOriginSource::Synthetic(_) | SonatinaOriginSource::Unmapped(_) => return None,
    };
    Some(FrontendOriginLabel::new(key.display_label()))
}
