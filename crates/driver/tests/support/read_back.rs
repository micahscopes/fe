use std::{fs, path::Path};

use common::InputDb;
use hir::{
    HirDb,
    core::meta::{FrozenCode, FrozenCodeHole, FrozenMetaValue},
    hir_def::Func,
};
use salsa::Setter;
use url::Url;

use super::{DriverDataBase, read_back};

pub struct TransplantedFixture {
    // The structural oracle needs both views; the execution oracle uses output.
    #[allow(dead_code)]
    pub authored: DriverDataBase,
    pub output: DriverDataBase,
    pub url: Url,
    pub bodies: Vec<(String, FrozenCode)>,
}

pub fn named_function<'db>(db: &'db DriverDataBase, url: &Url, name: &str) -> Func<'db> {
    let file = db.workspace().get(db, url).expect("fixture file");
    db.top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.body(db).is_some()
                && func
                    .name(db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(db) == name)
        })
        .unwrap_or_else(|| panic!("missing fixture function {name}"))
}

pub fn transplant_fixture(
    path: impl AsRef<Path>,
    source: &str,
    adapter: &Path,
) -> TransplantedFixture {
    let adapter = fs::read_to_string(adapter).expect("read Fe execution adapter");
    let url = Url::from_file_path(path.as_ref()).expect("fixture URL");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), format!("{source}\n{adapter}"));
    let mut output = super::prepare(&authored)
        .expect("fixture generation succeeds")
        .expect("fixture has generation requests");
    let input = HirDb::generated_module(&output, &url).expect("frozen generation");
    let mut requests = input.requests(&output).clone();
    assert!(!requests.is_empty(), "execution adapter must emit bodies");
    let mut bodies = Vec::new();
    for request in &mut requests {
        let name = request
            .provider
            .named_path()
            .expect("adapter provider is a named function")
            .rsplit("::")
            .next()
            .and_then(|name| name.strip_prefix("read_back_"))
            .expect("adapter providers name the body they copy");
        let original = read_back(&authored, named_function(&authored, &url, name))
            .unwrap_or_else(|error| panic!("{name} did not read back: {error:?}"));
        assert_eq!(original.nodes.len(), 1, "checked body has no splice graph");
        let FrozenMetaValue::Code(code) = &mut request.value else {
            panic!("adapter must emit an item quote");
        };
        let mut children = code.nodes[code.root]
            .holes
            .iter()
            .filter_map(|hole| match hole {
                FrozenCodeHole::Code(child, ..) => Some(*child),
                FrozenCodeHole::Value(_, _) => None,
            });
        let child = children.next().expect("adapter has a body fragment");
        assert!(
            children.next().is_none(),
            "adapter has exactly one body fragment"
        );
        code.nodes[child] = original.nodes[original.root].clone();
        bodies.push((name.to_owned(), original));
    }
    input.set_requests(&mut output).to(requests);
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let errors = hir::lower::generated_hir_errors(&output, module);
    assert!(errors.is_empty(), "copied HIR errors: {errors:?}");
    let errors = hir::lower::generated_quote_hygiene_errors(&output, module);
    assert!(errors.is_empty(), "copied hygiene errors: {errors:?}");
    for (name, _) in &bodies {
        let copied = named_function(&output, &url, &format!("copied_{name}"));
        let (errors, _) = hir::analysis::ty::ty_check::check_func_body(&output, copied);
        assert!(errors.is_empty(), "{name} did not type check: {errors:?}");
    }
    TransplantedFixture {
        authored,
        output,
        url,
        bodies,
    }
}
