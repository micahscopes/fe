use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::{HirDb, core::meta::FrozenMetaValue};
use salsa::Setter;
use url::Url;

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generated_const_generic_function", glob: "identity.fe")]
fn construction_error_uses_generation_site(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture path is absolute");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), fixture.content().to_string());
    let mut output = fe_driver::elaboration::prepare(&authored)
        .unwrap()
        .expect("generation request");
    let module_input = output
        .generated_module(&url)
        .expect("frozen generation input");
    let mut requests = module_input.requests(&output).clone();
    let site = requests[0]
        .site_span
        .clone()
        .expect("generation attribute span");
    requests[0].value = FrozenMetaValue::Array(vec![]);
    module_input.set_requests(&mut output).to(requests);

    let file = output.workspace().get(&output, &url).expect("fixture file");
    let error = hir::lower::generated_hir_errors(&output, output.top_mod(file))
        .first()
        .expect("construction error");
    assert_eq!(error.site_span.as_ref(), Some(&site));
    assert_eq!(
        error.failure,
        hir::lower::GeneratedQuoteFailure::Construction(
            hir::lower::QuoteConstructionFault::ExpectedAQuotedGenerationResult,
        ),
    );
}
