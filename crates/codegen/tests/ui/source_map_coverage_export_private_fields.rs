use fe_codegen::debug::{BytecodeOriginCoverageExport, SonatinaPostOptOriginCoverageExport};

fn main() {
    let _ = BytecodeOriginCoverageExport {
        total: 1,
        sonatina_post_opt: 1,
        sonatina_backend_prepared: 0,
        unmapped: 0,
    };

    let _ = SonatinaPostOptOriginCoverageExport {
        total: 1,
        same_inst_id: 1,
        created_or_unmatched_after_preopt_snapshot: 0,
        pre_opt_snapshot_losses: 0,
        observed_pre_opt_total: 1,
    };
}
