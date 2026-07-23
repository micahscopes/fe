use fe_hir::test_db::HirAnalysisTestDb;

#[test]
fn ground_type_inspection_exposes_constructor_and_ordered_args() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_ground_type_inspection.fe".into(),
        include_str!("fixtures/provider_ground_type_inspection/nominal_args.fe"),
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn method_quote_supports_hygienic_local_let_block() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_quote_local_let.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Compute {
    fn run(self, _ value: i32) -> i32
}

struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Compute<T>>,
        )
    {
        builder.emit_method(quote {
            fn run(self, _ value: i32) -> i32 {
                let value = value + value
                let shared = value + value
                shared + shared
            }
        })
        builder.finish()
        ev
    }
}

struct Target {}
derive Compute for Target using Provider

fn use_it(value: Target) -> i32 {
    value.run(2)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn method_quote_local_let_rejects_typed_binding_fail_closed() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_quote_local_let_reject.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self, _ value: bool) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method(quote {
            fn run(self, _ value: bool) -> bool {
                let shared: bool = value
                shared
            }
        })
        builder.finish()
        ev
    }
}

struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = db.run_on_top_mod(top_mod);
    assert!(
        !diags.is_empty(),
        "typed quote-local bindings must be rejected rather than silently replayed"
    );
}

#[test]
fn provider_const_helper_also_drives_recursive_type_plan() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_const_helper_bridge.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

const fn keep(_ i: usize) -> bool { i == 1 }

struct Zero {}
struct Add<const I: usize, R> {}
struct Select<const Keep: usize, const I: usize, R> {}
trait SelectOut { type Out }
impl<const I: usize, R> SelectOut for Select<0, I, R> { type Out = R }
impl<const I: usize, R> SelectOut for Select<1, I, R> { type Out = Add<I, R> }
recursive type fn Plan<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => <Select<
            { if keep(N - 1) { 1 } else { 0 } },
            {N - 1},
            Plan<{N - 1}>,
        > as SelectOut>::Out
    }
}
type Exact = Plan<3>
type Expected = Add<1, Zero>
fn exact(value: Exact) -> Expected { value }

trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if keep(1) {
            builder.emit_method(quote { fn run(self) -> bool { true } })
        } else {
            builder.emit_method(quote { fn run(self) -> bool { false } })
        }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_const_helpers_may_nest() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_nested_const_helpers.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn inner(_ x: bool) -> bool { !x }
const fn outer(_ x: bool) -> bool { inner(inner(x)) }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if outer(true) {
            builder.emit_method(quote { fn run(self) -> bool { true } })
        } else {
            builder.emit_method(quote { fn run(self) -> bool { false } })
        }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

fn assert_provider_helper_rejected(name: &str, source: &str) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(name.into(), source);
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("this construct is not supported in derive provider bodies"),
        "unexpected diagnostics:\n{rendered}"
    );
}

#[test]
fn provider_const_helper_recursion_is_rejected() {
    assert_provider_helper_rejected(
        "provider_recursive_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn recurse(_ x: bool) -> bool { recurse(x) }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if recurse(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}

#[test]
fn provider_const_helper_unsupported_body_is_rejected() {
    assert_provider_helper_rejected(
        "provider_unsupported_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn unsupported(_ x: bool) -> bool { [x][0] }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if unsupported(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}

#[test]
fn provider_const_helper_depth_is_rejected() {
    let mut helpers = String::new();
    for i in 0..33 {
        let body = if i == 32 {
            "x".to_string()
        } else {
            format!("h{}(x)", i + 1)
        };
        helpers.push_str(&format!("const fn h{i}(_ x: bool) -> bool {{ {body} }}\n"));
    }
    let source = format!(
        r#"
use core::derive::{{Derive, Evidence, ImplBuilder, Reflect}}
{helpers}
trait Compute {{ fn run(self) -> bool }}
struct Provider {{}}
impl Derive<Compute> for Provider {{
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {{
        if h0(true) {{ builder.emit_method(quote {{ fn run(self) -> bool {{ true }} }}) }}
        builder.finish()
        ev
    }}
}}
struct Target {{}}
derive Compute for Target using Provider
"#
    );
    assert_provider_helper_rejected("provider_deep_const_helper.fe", &source);
}

#[test]
fn provider_non_const_helper_is_rejected() {
    assert_provider_helper_rejected(
        "provider_non_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
fn effectful(_ x: bool) -> bool { x }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if effectful(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}

#[test]
fn provider_effectful_const_helper_is_rejected() {
    assert_provider_helper_rejected(
        "provider_effectful_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn effectful(_ x: bool) -> bool uses (reflect: Reflect<bool>) { x }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if effectful(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}
