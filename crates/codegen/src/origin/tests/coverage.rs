use super::*;

#[test]
fn bytecode_origin_coverage_constructor_derives_partitioned_total() {
    let coverage = BytecodeOriginCoverage::new(2, 3, 5);

    assert_eq!(coverage.total(), 10);
    assert_eq!(coverage.classified_total(), 10);
    assert!(coverage.is_partitioned());
    assert!(!coverage.is_empty());
}

#[test]
fn sonatina_coverage_constructors_derive_partitioned_totals() {
    let pre_opt = SonatinaOriginCoverage::new(2, 3, 5, 7);

    assert_eq!(pre_opt.total(), 17);
    assert_eq!(pre_opt.runtime_stmt(), 2);
    assert_eq!(pre_opt.runtime_terminator(), 3);
    assert_eq!(pre_opt.synthetic(), 5);
    assert_eq!(pre_opt.unmapped(), 7);
    assert_eq!(pre_opt.classified_total(), 17);
    assert!(pre_opt.is_partitioned());
    assert!(!pre_opt.is_empty());

    let post_opt = SonatinaPostOptOriginCoverage::new(11, 13, 17);

    assert_eq!(post_opt.total(), 24);
    assert_eq!(post_opt.same_inst_id(), 11);
    assert_eq!(post_opt.created_or_unmatched_after_preopt_snapshot(), 13);
    assert_eq!(post_opt.pre_opt_snapshot_losses(), 17);
    assert_eq!(post_opt.post_opt_classified_total(), 24);
    assert_eq!(post_opt.observed_pre_opt_total(), 28);
    assert!(post_opt.is_post_opt_partitioned());
    assert!(!post_opt.is_empty());
}
