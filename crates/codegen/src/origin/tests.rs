use common::{
    InputDb,
    origin::{OriginExportKey, OriginExportKind, OriginLinkKind},
};
use driver::DriverDataBase;
use hir::{
    analysis::{
        semantic::{get_or_build_semantic_instance, root_semantic_instance_key},
        ty::ty_check::BodyOwner,
    },
    hir_def::{Func, TopLevelMod},
};
use mir::{
    RBlockId, RuntimeInstanceKey, RuntimeStmtIndex, RuntimeStmtOrigin, RuntimeStmtSite,
    get_or_build_runtime_instance, instance::RuntimeInstanceSource,
};
use sonatina_codegen::{
    machinst::vcode::VCodeInst,
    object::{
        ObjectArtifact, PcMapEntry, SectionArtifact, SectionObservability, UnmappedReasonCoverage,
    },
};
use sonatina_ir::{
    BlockId, InstId,
    module::FuncRef,
    object::{ObjectName, SectionName},
};
use url::Url;

use super::{
    BytecodeObjectKey, BytecodeOriginCoverage, BytecodeOriginRecord, BytecodeOriginSource,
    BytecodePackageOrigins, BytecodePackageOriginsError, BytecodePcOrigin, BytecodePcRange,
    BytecodeSectionKey, BytecodeSectionNameKey, BytecodeUnmappedReason, CodegenOriginGraph,
    CodegenOriginNode, EndToEndOriginGraph, EndToEndOriginNode, EndToEndOriginOwnerKeys,
    EndToEndRuntimeOwnerKey, EndToEndRuntimeSyntheticLocalKey, FrontendOriginLabel,
    FrontendOriginLabelMap, SonatinaBackendPreparedOriginRecord,
    SonatinaBackendPreparedOriginSource, SonatinaFunctionExportKey, SonatinaInstOrigin,
    SonatinaInstOriginRecord, SonatinaInstStage, SonatinaOriginCoverage, SonatinaOriginSource,
    SonatinaPostOptFunctionOrigins, SonatinaPostOptOriginCoverage, SonatinaPostOptOriginRecord,
    SonatinaPostOptOriginSource, SonatinaPostOptPackageOrigins, SonatinaPreOptSnapshotLossReason,
    SonatinaPreOptSnapshotLossRecord, SonatinaSyntheticOrigin, bytecode_pc_export_key,
    bytecode_source_from_pc_entry, bytecode_unmapped_export_key, codegen_origin_graph_facts,
    codegen_origin_node_export_key, end_to_end_origin_graph_facts,
    end_to_end_origin_node_export_key, sonatina_inst_export_key, sonatina_synthetic_export_key,
};

fn bytecode_section_key(object: &str, section: &str) -> BytecodeSectionKey {
    BytecodeSectionKey::new(
        BytecodeObjectKey::new(object),
        BytecodeSectionNameKey::new(section),
    )
}

fn origin_key(kind: OriginExportKind, owner: &str, local: &str) -> OriginExportKey {
    OriginExportKey::try_from_raw_parts(kind, owner, local).unwrap()
}

fn find_func<'db>(db: &'db DriverDataBase, top_mod: TopLevelMod<'db>, name: &str) -> Func<'db> {
    top_mod
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == name)
        })
        .unwrap_or_else(|| panic!("missing function `{name}`"))
}

fn runtime_stmt_origin_for_fixture<'db>(db: &'db mut DriverDataBase) -> RuntimeStmtOrigin<'db> {
    let file_url = Url::parse("file:///codegen_frontend_origin_label_keys.fe").unwrap();
    let file = db.workspace().touch(
        db,
        file_url,
        Some(
            r#"
fn label_source() -> u256 {
    1
}
"#
            .to_string(),
        ),
    );
    let top_mod = db.top_mod(file);
    let func = find_func(db, top_mod, "label_source");
    let semantic_key = root_semantic_instance_key(db, BodyOwner::Func(func))
        .expect("fixture function should have a root semantic instance key");
    let semantic = get_or_build_semantic_instance(db, semantic_key);
    let runtime_key =
        RuntimeInstanceKey::new(db, RuntimeInstanceSource::Semantic(semantic), Vec::new());
    let instance = get_or_build_runtime_instance(db, runtime_key);
    RuntimeStmtOrigin::new(
        instance,
        RuntimeStmtSite::new(RBlockId::from_u32(0), RuntimeStmtIndex::from_u32(0)),
    )
}

fn bytecode_package_with_same_id_source<'db>(
    function: FuncRef,
    inst: InstId,
    source: SonatinaOriginSource<'db>,
) -> BytecodePackageOrigins<'db> {
    let pre_opt =
        SonatinaInstOriginRecord::new(SonatinaInstOrigin::pre_opt(function, inst), source);
    let post_opt = SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(function, inst),
        SonatinaPostOptOriginSource::SameInstId(pre_opt),
    );
    BytecodePackageOrigins {
        records: vec![BytecodeOriginRecord::new(
            BytecodePcOrigin::new(
                bytecode_section_key("Foo", "runtime"),
                BytecodePcRange::new(4, 8).expect("valid PC range"),
            ),
            BytecodeOriginSource::SonatinaPostOpt(post_opt),
        )],
    }
}

fn pc_map_entry(pc_start: u32, pc_end: u32) -> PcMapEntry {
    PcMapEntry {
        pc_start,
        pc_end,
        func: FuncRef::from_u32(0),
        func_name: "test_func".to_string(),
        block: BlockId::from_u32(0),
        vcode_inst: VCodeInst(0),
        ir_inst: None,
        frontend_provenance: None,
        unmapped_reason: None,
    }
}

fn section_observability(
    section: impl Into<SectionName>,
    pc_start: u32,
    pc_end: u32,
) -> SectionObservability {
    SectionObservability {
        schema_version: "test",
        section: section.into(),
        section_bytes: pc_end,
        code_bytes: pc_end,
        data_bytes: 0,
        embed_bytes: 0,
        mapped_code_bytes: 0,
        unmapped_code_bytes: pc_end.saturating_sub(pc_start),
        unmapped_reason_coverage: UnmappedReasonCoverage::default(),
        pc_map: vec![pc_map_entry(pc_start, pc_end)],
    }
}

fn object_artifact(
    object: impl Into<ObjectName>,
    sections: impl IntoIterator<Item = (&'static str, u32, u32)>,
) -> ObjectArtifact {
    ObjectArtifact {
        object: object.into(),
        sections: sections
            .into_iter()
            .map(|(section, pc_start, pc_end)| {
                (
                    SectionName::from(section),
                    SectionArtifact {
                        bytes: Vec::new(),
                        symtab: Default::default(),
                        observability: Some(section_observability(section, pc_start, pc_end)),
                    },
                )
            })
            .collect(),
    }
}

fn push_pc_map_entry(
    artifact: &mut ObjectArtifact,
    section: &'static str,
    pc_start: u32,
    pc_end: u32,
) {
    artifact
        .sections
        .get_mut(&SectionName::from(section))
        .expect("test section should exist")
        .observability
        .as_mut()
        .expect("test section should have observability")
        .pc_map
        .push(pc_map_entry(pc_start, pc_end));
}

mod backend_prepared;
mod bytecode_origins;
mod coverage;
mod export_keys;
mod fact_export;
mod frontend_labels;
mod graph;
mod post_opt_snapshot;
mod sonatina_records;
