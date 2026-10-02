use super::*;
use fe_driver::generation::{GenerationErrorKind, imports::ImportErrorKind};

fn selected(db: &mut DriverDataBase, source: &str) -> FrozenFunction {
    let file = provider(db, source);
    FrozenArtifact::new(generate(db, file, grants()).unwrap())
        .export("accepts")
        .unwrap()
}

fn outcomes(bound: &BoundFunction) -> Vec<String> {
    ["below", "edge", "above"]
        .into_iter()
        .map(|name| evaluate(bound, name))
        .collect()
}

#[test]
fn policy_selects_behavior_used_by_a_separate_checked_consumer() {
    let mut db = database();
    for (strict, target, expected) in [
        (true, 7, ["true", "false", "false"]),
        (false, 9, ["true", "true", "false"]),
    ] {
        let source = PROVIDER.replace("strict: true", &format!("strict: {strict}"));
        let export = selected(&mut db, &source);
        let bound = bind(&export, CONSUMER).unwrap();
        assert_eq!(outcomes(&bound), expected);
        assert_eq!(
            bound.receipt().original_provenance.emission_target,
            Some(target)
        );
        assert_eq!(bound.receipt().selected_name, "accepts");
        assert_eq!(
            &bound.source()[bound.receipt().emitted_call.clone()],
            "__frozen_import::accepts(value: value)"
        );
    }
}

#[test]
fn policy_value_and_template_edits_match_fresh_and_do_not_rebind_old_handles() {
    let mut db = database();
    let old = selected(&mut db, PROVIDER);
    let old_bound = bind(&old, CONSUMER).unwrap();
    let original_file = provider(&mut db, PROVIDER);
    // Policy edit, value edit, template edit at the same target, then restoration.
    for (strict, maximum, template_edit, expected) in [
        (false, 20, false, ["true", "true", "false"]),
        (true, 21, false, ["true", "true", "false"]),
        (true, 20, true, ["true", "true", "false"]),
        (true, 20, false, ["true", "false", "false"]),
    ] {
        let source = PROVIDER
            .replace("strict: true", &format!("strict: {strict}"))
            .replace("maximum: 20", &format!("maximum: {maximum}"));
        let edited_grants = || {
            let mut result = grants();
            if template_edit {
                result[0].template.source = INCLUSIVE.to_owned();
            }
            result
        };
        let file = provider(&mut db, &source);
        assert_eq!(file, original_file);
        assert_eq!(file.text(&db), &source);
        let warm = FrozenArtifact::new(generate(&db, file, edited_grants()).unwrap());
        let bound = bind(&warm.export("accepts").unwrap(), CONSUMER).unwrap();
        let mut fresh_db = database();
        let fresh_file = provider(&mut fresh_db, &source);
        let fresh = FrozenArtifact::new(generate(&fresh_db, fresh_file, edited_grants()).unwrap());
        let fresh_bound = bind(&fresh.export("accepts").unwrap(), CONSUMER).unwrap();
        assert_eq!(outcomes(&bound), expected);
        assert_eq!(outcomes(&bound), outcomes(&fresh_bound));
        assert_eq!(bound.source(), fresh_bound.source());
        assert_eq!(
            bound.receipt().original_source,
            fresh_bound.receipt().original_source
        );
        assert_eq!(
            bound.receipt().original_provenance,
            fresh_bound.receipt().original_provenance
        );
        assert_eq!(
            bound.receipt().original_provenance.request_identity,
            old_bound.receipt().original_provenance.request_identity
        );
        assert_eq!(
            outcomes(&bind(&old, CONSUMER).unwrap()),
            ["true", "false", "false"]
        );
    }
    drop(db);
    assert_eq!(
        outcomes(&bind(&old, CONSUMER).unwrap()),
        ["true", "false", "false"]
    );
}

#[test]
fn failed_emission_cannot_supply_a_new_completion_or_replace_a_retained_one() {
    let mut db = database();
    let old = selected(&mut db, PROVIDER);
    for (source, invalid_output, expected) in [
        (
            PROVIDER.replace("{ 7 }", "{ 123 }"),
            false,
            GenerationErrorKind::Authority,
        ),
        (
            PROVIDER.replace("value: policy.limit()", "value: true"),
            false,
            GenerationErrorKind::Provider,
        ),
        (PROVIDER.to_owned(), true, GenerationErrorKind::Output),
    ] {
        let file = provider(&mut db, &source);
        let mut allowed = grants();
        if invalid_output {
            allowed[0]
                .template
                .source
                .push_str("\nfn broken() -> bool { 42 }");
        }
        // Export and binding are reachable only from the successful artifact arm.
        let result = generate(&db, file, allowed).map(|artifact| {
            let frozen = FrozenArtifact::new(artifact);
            bind(&frozen.export("accepts").unwrap(), CONSUMER).unwrap()
        });
        let error = result
            .err()
            .expect("failed emission must not reach consumption");
        assert_eq!(error.kind, expected, "{error}");
        assert_eq!(error.provenance.is_some(), invalid_output);
        assert_eq!(
            outcomes(&bind(&old, CONSUMER).unwrap()),
            ["true", "false", "false"]
        );
    }
}

#[test]
fn same_shaped_replacement_cannot_shadow_the_selected_export() {
    let mut db = database();
    let export = selected(&mut db, PROVIDER);
    let shadowed = format!(
        "mod __frozen_import {{ pub const fn accepts(value: u256) -> bool {{ true }} }}\n{CONSUMER}"
    );
    let error = bind(&export, &shadowed).err().expect("shadow must reject");
    assert_eq!(error.kind, ImportErrorKind::Binding, "{error}");
    let receipt = error.receipt.unwrap();
    assert_eq!(receipt.original_provenance.emission_target, Some(7));
    assert!(receipt.original_source.contains("value < ingot::limit()"));
    // An unrelated same-named function cannot redirect the qualified binding.
    let unrelated = format!("const fn accepts(value: u256) -> bool {{ true }}\n{CONSUMER}");
    assert_eq!(
        outcomes(&bind(&export, &unrelated).unwrap()),
        ["true", "false", "false"]
    );
}

#[test]
fn checked_emission_does_not_certify_the_consumer() {
    let mut db = database();
    let export = selected(&mut db, PROVIDER);
    for source in [
        CONSUMER.replace("fn validate(value: u256)", "fn validate(value: bool)"),
        format!("{CONSUMER}\nfn invalid() -> bool {{ 42 }}"),
    ] {
        let error = bind(&export, &source)
            .err()
            .expect("invalid consumer must reject");
        assert_eq!(error.kind, ImportErrorKind::Output, "{error}");
        assert_eq!(
            error.receipt.unwrap().original_provenance.emission_target,
            Some(7)
        );
    }
}
