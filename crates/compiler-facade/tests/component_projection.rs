use fe_compiler_facade::{CompileFacadeError, compile_resident_component};
use fe_compiler_protocol::{
    CompileOptions, CompileRequest, CompileTarget, ProtocolVersion, VirtualSource,
};
use url::Url;

#[test]
fn component_view_without_the_same_resident_actor_fails_closed() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen/tests/fixtures/web_page_actor/src/lib.fe")
        .canonicalize()
        .expect("page fixture source");
    let url = Url::from_file_path(&path).expect("fixture URL");
    let source = std::fs::read_to_string(&path).expect("fixture source");
    let request = CompileRequest {
        protocol: ProtocolVersion::CURRENT,
        root: url.to_string(),
        sources: vec![VirtualSource::new(url.as_str(), source)],
        target: CompileTarget::Wasm,
        entries: vec!["component".to_owned()],
        options: CompileOptions::default(),
    };

    let error = compile_resident_component(&request).unwrap_err();
    assert!(matches!(error, CompileFacadeError::Backend(_)), "{error}");
    assert!(
        error.to_string().contains("has no resident transition"),
        "{error}"
    );
}
