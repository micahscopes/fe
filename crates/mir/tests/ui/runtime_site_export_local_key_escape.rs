use fe_mir::{RBlockId, RuntimeStmtIndex, RuntimeStmtSite, RuntimeTerminatorSite};

fn runtime_stmt_site_does_not_expose_export_local_key() {
    let site = RuntimeStmtSite::new(RBlockId::from_u32(0), RuntimeStmtIndex::from_u32(0));
    let _ = site.export_local_key();
}

fn runtime_terminator_site_does_not_expose_export_local_key() {
    let site = RuntimeTerminatorSite::new(RBlockId::from_u32(0));
    let _ = site.export_local_key();
}

fn main() {}
