use super::*;

#[test]
fn bytecode_pc_origin_includes_object_and_section_owner() {
    let range = BytecodePcRange::new(4, 8).expect("valid range");

    let first = BytecodePcOrigin::new(bytecode_section_key("Foo", "init"), range);
    let second = BytecodePcOrigin::new(bytecode_section_key("Foo", "runtime"), range);

    assert_ne!(first, second);
}

#[test]
#[should_panic(expected = "origin string key must not be empty")]
fn bytecode_object_key_rejects_empty_keys() {
    BytecodeObjectKey::new("");
}

#[test]
#[should_panic(expected = "origin string key must not be empty")]
fn bytecode_section_key_rejects_empty_sections() {
    BytecodeSectionNameKey::new("");
}

#[test]
fn bytecode_pc_range_rejects_inverted_offsets() {
    assert_eq!(BytecodePcRange::new(8, 4), None);
}

#[test]
fn bytecode_pc_range_rejects_empty_ranges() {
    assert_eq!(BytecodePcRange::new(4, 4), None);
}

#[test]
fn bytecode_package_origins_from_artifacts_are_deterministically_ordered() {
    let artifacts = vec![
        object_artifact("B", [("runtime", 20, 24), ("init", 10, 14)]),
        object_artifact("A", [("runtime", 8, 12), ("init", 4, 6)]),
    ];
    let post_opt_origins = SonatinaPostOptPackageOrigins {
        functions: Vec::new(),
        pre_opt_snapshot_losses: Vec::new(),
    };

    let origins = BytecodePackageOrigins::from_artifacts(&artifacts, &post_opt_origins);
    let record_keys = origins
        .records()
        .iter()
        .map(|record| {
            (
                record.origin().section().object().as_str().to_string(),
                record.origin().section().section().to_string(),
                record.origin().range().start(),
                record.origin().range().end(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        record_keys,
        vec![
            ("A".to_string(), "init".to_string(), 4, 6),
            ("A".to_string(), "runtime".to_string(), 8, 12),
            ("B".to_string(), "init".to_string(), 10, 14),
            ("B".to_string(), "runtime".to_string(), 20, 24),
        ]
    );
    assert!(origins.records().iter().all(|record| {
        matches!(
            record.source(),
            BytecodeOriginSource::Unmapped(super::BytecodeUnmappedReason::Unknown)
        )
    }));
}

#[test]
fn bytecode_package_origins_reject_overlapping_pc_ranges_in_one_section() {
    let mut artifact = object_artifact("A", [("runtime", 4, 8)]);
    push_pc_map_entry(&mut artifact, "runtime", 7, 10);
    let post_opt_origins = SonatinaPostOptPackageOrigins {
        functions: Vec::new(),
        pre_opt_snapshot_losses: Vec::new(),
    };

    let err = BytecodePackageOrigins::try_from_artifacts(&[artifact], &post_opt_origins)
        .expect_err("overlapping bytecode PC ranges should fail");

    assert_eq!(
        err,
        BytecodePackageOriginsError::OverlappingPcRange {
            object: "A".to_string(),
            section: "runtime".to_string(),
            previous_start: 4,
            previous_end: 8,
            current_start: 7,
            current_end: 10,
        }
    );
    assert_eq!(
        err.to_string(),
        "bytecode origin PC ranges must not overlap within one object section: object `A` section `runtime` range 4..8 overlaps 7..10"
    );
}

#[test]
fn bytecode_package_origins_reject_empty_pc_map_ranges() {
    let artifact = object_artifact("A", [("runtime", 4, 4)]);
    let post_opt_origins = SonatinaPostOptPackageOrigins {
        functions: Vec::new(),
        pre_opt_snapshot_losses: Vec::new(),
    };

    let err = BytecodePackageOrigins::try_from_artifacts(&[artifact], &post_opt_origins)
        .expect_err("empty bytecode PC ranges should fail");

    assert_eq!(
        err,
        BytecodePackageOriginsError::InvalidPcRange {
            object: "A".to_string(),
            section: "runtime".to_string(),
            pc_start: 4,
            pc_end: 4,
        }
    );
    assert_eq!(
        err.to_string(),
        "bytecode origin PC-map ranges must be non-empty and ordered: object `A` section `runtime` range 4..4"
    );
}

#[test]
fn bytecode_package_origins_reject_inverted_pc_map_ranges() {
    let artifact = object_artifact("A", [("runtime", 8, 4)]);
    let post_opt_origins = SonatinaPostOptPackageOrigins {
        functions: Vec::new(),
        pre_opt_snapshot_losses: Vec::new(),
    };

    let err = BytecodePackageOrigins::try_from_artifacts(&[artifact], &post_opt_origins)
        .expect_err("inverted bytecode PC ranges should fail");

    assert_eq!(
        err,
        BytecodePackageOriginsError::InvalidPcRange {
            object: "A".to_string(),
            section: "runtime".to_string(),
            pc_start: 8,
            pc_end: 4,
        }
    );
    assert_eq!(
        err.to_string(),
        "bytecode origin PC-map ranges must be non-empty and ordered: object `A` section `runtime` range 8..4"
    );
}

#[test]
fn bytecode_package_origins_allow_adjacent_pc_ranges_in_one_section() {
    let mut artifact = object_artifact("A", [("runtime", 4, 8)]);
    push_pc_map_entry(&mut artifact, "runtime", 8, 12);
    let post_opt_origins = SonatinaPostOptPackageOrigins {
        functions: Vec::new(),
        pre_opt_snapshot_losses: Vec::new(),
    };

    let origins = BytecodePackageOrigins::from_artifacts(&[artifact], &post_opt_origins);

    assert_eq!(
        origins
            .records()
            .iter()
            .map(|record| (
                record.origin().range().start(),
                record.origin().range().end()
            ))
            .collect::<Vec<_>>(),
        vec![(4, 8), (8, 12)]
    );
}
