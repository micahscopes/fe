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
    fn run(self, _ value: bool) -> bool
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
            fn run(self, _ value: bool) -> bool {
                let value = value && true
                let shared = value || false
                shared && shared
            }
        })
        builder.finish()
        ev
    }
}

struct Target {}
derive Compute for Target using Provider

fn use_it(value: Target) -> bool {
    value.run(true)
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
