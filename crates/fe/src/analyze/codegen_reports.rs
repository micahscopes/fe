use codegen::{
    SonatinaContractBytecode, SonatinaTestOptions, TestMetadata,
    debug::{
        BytecodeSourceMapEntry, BytecodeSourceMapSummary, bytecode_source_map_entries_summary,
    },
    emit_runtime_package_sonatina_bytecode_with_source_maps, emit_test_module_sonatina,
    origin::{BytecodeOriginCoverage, SonatinaPostOptOriginCoverage},
};
use driver::DriverDataBase;
use hir::hir_def::TopLevelMod;

use super::{
    AnalyzeOptions, origin_fact_report,
    report::{AnalyzeOriginFactReport, AnalyzeSourceMapReport, AnalyzeSourceMapReportError},
};

pub(super) struct AnalyzeCodegenReports {
    pub(super) source_maps: Vec<AnalyzeSourceMapReport>,
    pub(super) origin_facts: Vec<AnalyzeOriginFactReport>,
}

pub(super) fn summarize_runtime_codegen_reports(
    db: &DriverDataBase,
    package: mir::RuntimePackage<'_>,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeCodegenReports, String> {
    let outputs = emit_runtime_package_sonatina_bytecode_with_source_maps(
        db,
        &package,
        codegen::EVM_LAYOUT,
        options.opt_level,
    )
    .map_err(|err| err.to_string())?;
    let source_maps = if options.include_source_maps {
        outputs
            .iter()
            .map(|(contract, output)| {
                source_map_report_for_runtime_contract(
                    contract,
                    output,
                    options.include_source_map_entries,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?
            .into_iter()
            .flatten()
            .collect()
    } else {
        Vec::new()
    };
    let origin_facts = options
        .include_origin_facts
        .then(|| {
            outputs
                .iter()
                .flat_map(|(contract, output)| {
                    origin_fact_reports_for_runtime_contract(
                        contract,
                        output,
                        options.include_fact_relation_tables,
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AnalyzeCodegenReports {
        source_maps,
        origin_facts,
    })
}

pub(super) fn summarize_test_codegen_reports(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeCodegenReports, String> {
    let output = emit_test_module_sonatina(
        db,
        top_mod,
        options.opt_level,
        SonatinaTestOptions {
            emit_observability: true,
        },
        None,
    )
    .map_err(|err| err.to_string())?;
    let source_maps = if options.include_source_maps {
        output
            .tests
            .iter()
            .map(|test| source_map_report_for_test(test, options.include_source_map_entries))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?
            .into_iter()
            .flatten()
            .collect()
    } else {
        Vec::new()
    };
    let origin_facts = options
        .include_origin_facts
        .then(|| {
            output
                .tests
                .iter()
                .flat_map(|test| {
                    origin_fact_reports_for_test(test, options.include_fact_relation_tables)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AnalyzeCodegenReports {
        source_maps,
        origin_facts,
    })
}

fn source_map_report_for_test(
    test: &TestMetadata,
    include_source_map_entries: bool,
) -> Result<Option<AnalyzeSourceMapReport>, AnalyzeSourceMapReportError> {
    let Some(summary) = test.sonatina_source_map_summary.as_ref() else {
        return Ok(None);
    };
    let entries = include_source_map_entries
        .then(|| test.sonatina_source_map_entries.clone())
        .unwrap_or_default();
    source_map_report_from_summary(
        &test.display_name,
        &test.object_name,
        summary,
        test.sonatina_bytecode_origin_coverage,
        test.sonatina_post_opt_origin_coverage,
        entries,
    )
    .map(Some)
}

fn source_map_report_for_runtime_contract(
    contract: &str,
    output: &SonatinaContractBytecode,
    include_source_map_entries: bool,
) -> Result<Option<AnalyzeSourceMapReport>, AnalyzeSourceMapReportError> {
    source_map_report_from_entries(
        "runtime_bytecode",
        contract.to_string(),
        contract,
        &output.source_map_entries,
        output.bytecode_origin_coverage,
        output.post_opt_origin_coverage,
        include_source_map_entries,
    )
}

fn source_map_report_from_summary(
    test: &str,
    fallback_object: &str,
    summary: &BytecodeSourceMapSummary,
    bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
    entries: Vec<BytecodeSourceMapEntry>,
) -> Result<AnalyzeSourceMapReport, AnalyzeSourceMapReportError> {
    AnalyzeSourceMapReport::try_from_summary(
        "test_bytecode",
        test.to_string(),
        Some(test.to_string()),
        summary.object().unwrap_or(fallback_object).to_string(),
        summary.section().unwrap_or("<all>").to_string(),
        summary,
        bytecode_origin_coverage,
        post_opt_origin_coverage,
        entries,
    )
}

fn source_map_report_from_entries(
    scope: &'static str,
    label: String,
    fallback_object: &str,
    all_entries: &[BytecodeSourceMapEntry],
    bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
    include_entries: bool,
) -> Result<Option<AnalyzeSourceMapReport>, AnalyzeSourceMapReportError> {
    let Some(summary) = bytecode_source_map_entries_summary(all_entries, None) else {
        return Ok(None);
    };

    AnalyzeSourceMapReport::try_from_summary(
        scope,
        label,
        None,
        all_entries
            .first()
            .map(|entry| entry.object().to_string())
            .unwrap_or_else(|| fallback_object.to_string()),
        "<all>".to_string(),
        &summary,
        bytecode_origin_coverage,
        post_opt_origin_coverage,
        include_entries
            .then(|| all_entries.to_vec())
            .unwrap_or_default(),
    )
    .map(Some)
}

fn origin_fact_reports_for_test(
    test: &TestMetadata,
    include_relation_tables: bool,
) -> Vec<AnalyzeOriginFactReport> {
    let mut reports = Vec::new();
    if let Some(facts) = test.sonatina_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "test_bytecode",
            test.display_name.clone(),
            Some(test.object_name.clone()),
            facts,
            include_relation_tables,
        ));
    }
    if let Some(facts) = test.sonatina_snapshot_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "test_sonatina_snapshot",
            test.display_name.clone(),
            Some(test.object_name.clone()),
            facts,
            include_relation_tables,
        ));
    }
    reports
}

fn origin_fact_reports_for_runtime_contract(
    contract: &str,
    output: &SonatinaContractBytecode,
    include_relation_tables: bool,
) -> Vec<AnalyzeOriginFactReport> {
    let mut reports = Vec::new();
    if let Some(facts) = output.origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "runtime_bytecode",
            contract.to_string(),
            Some(contract.to_string()),
            facts,
            include_relation_tables,
        ));
    }
    if let Some(facts) = output.snapshot_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "runtime_sonatina_snapshot",
            contract.to_string(),
            Some(contract.to_string()),
            facts,
            include_relation_tables,
        ));
    }
    reports
}
