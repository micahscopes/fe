use common::{
    InputDb,
    diagnostics::Severity,
    file::File,
    stdlib::{HasBuiltinCore, HasBuiltinStd},
};
use fe_driver::DriverDataBase;
use hir::{
    analysis::semantic::eval_body_owner_const_with_args, analysis::ty::ty_check::BodyOwner,
    hir_def::Func,
};
use url::Url;

fn database() -> DriverDataBase {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    db.initialize_builtin_std();
    db
}

fn input(db: &mut DriverDataBase, name: &str, source: &str) -> File {
    use salsa::Setter;
    let file = db.workspace().touch(
        db,
        Url::parse(&format!("file:///const-where/{name}.fe")).unwrap(),
        Some(source.to_owned()),
    );
    // touch reuses an existing input without replacing its source.
    file.set_text(db).to(source.to_owned());
    file
}

fn diagnostics(db: &DriverDataBase, file: File) -> String {
    let ingot = db.top_mod(file).ingot(db);
    let hir = db.run_on_ingot(ingot);
    let mut errors = if hir.has_errors(db) {
        hir.format_diags(db)
    } else {
        String::new()
    };
    let semantic = db.mir_diagnostics_for_ingot(ingot);
    if semantic
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        errors.push_str(&db.format_complete_diagnostics(&semantic));
    }
    errors
}

fn checked(db: &mut DriverDataBase, name: &str, source: &str) -> File {
    let file = input(db, name, source);
    let errors = diagnostics(db, file);
    assert!(errors.is_empty(), "unexpected diagnostics:\n{errors}");
    file
}

fn named<'db>(db: &'db DriverDataBase, file: File, name: &str) -> Func<'db> {
    db.top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == name)
        })
        .unwrap_or_else(|| panic!("missing function `{name}`"))
}

fn evaluate(db: &DriverDataBase, file: File, name: &str) -> String {
    eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(named(db, file, name)),
        hir::analysis::semantic::GenericSubst::for_body_owner(
            db,
            BodyOwner::Func(named(db, file, name)),
            Vec::new(),
        ),
        Vec::new(),
    )
    .into_ready()
    .unwrap_or_else(|| panic!("failed to evaluate `{name}`"))
    .pretty_print(db)
}

#[test]
#[ignore = "Step 2: pass the selected bound to both the impl condition and body"]
fn selected_impl_bound_agrees_between_condition_and_body() {
    let mut db = database();
    let file = checked(
        &mut db,
        "selected-bound-condition-body",
        "trait Compute { const OK: bool }
struct Subject {}
impl Compute for Subject { const OK: bool = false }
impl Compute for Subject as Relaxed { const OK: bool = true }
trait Value { const fn value() -> u256 }
struct Gate<T> {}
impl<T: Compute> Value for Gate<T> where T::OK {
    const fn value() -> u256 { if T::OK { 1 } else { 2 } }
}
const fn run() -> u256 { with (Relaxed) { <Gate<Subject> as Value>::value() } }",
    );
    assert_eq!(evaluate(&db, file, "run"), "1");
}

// These are ordinary Fe carrier probes, not a public evidence primitive.
const BOOLEAN_CARRIER: &str = "struct Evidence<const P: bool> where P {}";

#[test]
fn boolean_carrier_checks_introduction_and_explicit_forwarding() {
    let mut db = database();
    let source = format!(
        "{BOOLEAN_CARRIER}\nconst fn consume<const P: bool>(_ evidence: Evidence<P>) -> u256 where P {{ 42 }}\nconst fn answer() -> u256 {{ consume<true>(Evidence<true> {{}}) }}"
    );
    let file = checked(&mut db, "boolean-introduction", &source);
    assert_eq!(evaluate(&db, file, "answer"), "42");
    let file = input(
        &mut db,
        "boolean-introduction",
        &source.replace("<true>", "<false>"),
    );
    let errors = diagnostics(&db, file);
    assert!(
        errors.contains("const requirement") && errors.contains("false"),
        "{errors}"
    );
}

#[test]
fn boolean_carrier_does_not_implicitly_open_its_requirement() {
    let mut db = database();
    let source = format!(
        "{BOOLEAN_CARRIER}\nconst fn consume<const P: bool>(_ evidence: Evidence<P>) -> u256 {{ 42 }}"
    );
    let file = input(&mut db, "boolean-elimination", &source);
    let errors = diagnostics(&db, file);
    assert!(
        !errors.is_empty(),
        "an ordinary carrier unexpectedly established P"
    );
    assert!(errors.contains("const requirement"), "{errors}");
}

const TRAIT_CARRIER: &str = "trait Compute { const fn compute() -> u256 }\nstruct Evidence<T: Compute> {}\nstruct Subject {}\nimpl Compute for Subject { const fn compute() -> u256 { 42 } }";

#[test]
fn trait_carrier_checks_introduction_and_explicit_forwarding() {
    let mut db = database();
    let source = format!(
        "{TRAIT_CARRIER}\nconst fn consume<T: Compute>(_ evidence: Evidence<T>) -> u256 {{ T::compute() }}\nconst fn answer() -> u256 {{ consume<Subject>(Evidence<Subject> {{}}) }}"
    );
    let file = checked(&mut db, "trait-introduction", &source);
    assert_eq!(evaluate(&db, file, "answer"), "42");
    let file = input(
        &mut db,
        "trait-introduction",
        &source.replace(
            "impl Compute for Subject { const fn compute() -> u256 { 42 } }",
            "",
        ),
    );
    let errors = diagnostics(&db, file);
    assert!(!errors.is_empty() && errors.contains("Compute"), "{errors}");
}

#[test]
fn trait_carrier_does_not_implicitly_open_its_requirement() {
    let mut db = database();
    let source = format!(
        "{TRAIT_CARRIER}\nconst fn consume<T>(_ evidence: Evidence<T>) -> u256 {{ T::compute() }}"
    );
    let file = input(&mut db, "trait-elimination", &source);
    let errors = diagnostics(&db, file);
    assert!(
        !errors.is_empty(),
        "an ordinary carrier unexpectedly established Compute"
    );
    assert!(errors.contains("Compute"), "{errors}");
}

#[test]
fn trait_carrier_keeps_nominal_type_but_does_not_select_an_alternate_impl() {
    let mut db = database();
    let source = "trait Compute { const fn compute() -> u256 }\nstruct Evidence<T: Compute> {}\nstruct A {}\nstruct B {}\nimpl Compute for A { const fn compute() -> u256 { 11 } }\nimpl Compute for B { const fn compute() -> u256 { 22 } }\nconst fn consume<T: Compute>(_ evidence: Evidence<T>) -> u256 { T::compute() }\nconst fn answer() -> u256 { consume<A>(Evidence<A> {}) + consume<B>(Evidence<B> {}) }";
    let file = checked(&mut db, "nominal-carriers", source);
    assert_eq!(evaluate(&db, file, "answer"), "33");
    let file = input(
        &mut db,
        "nominal-carriers",
        &source.replace("consume<A>(Evidence<A>", "consume<A>(Evidence<B>"),
    );
    let errors = diagnostics(&db, file);
    assert!(errors.contains("type mismatch"), "{errors}");

    // A marker carries no selection identity beyond its type parameter.
    let file = input(
        &mut db,
        "nominal-carriers",
        &source.replace("impl Compute for B", "impl Compute for A"),
    );
    let errors = diagnostics(&db, file);
    assert!(
        errors.contains("conflicting trait implementations"),
        "{errors}"
    );
}

#[test]
fn carrier_creation_cannot_assume_the_requirement_it_is_establishing() {
    let mut db = database();
    let source = format!(
        "{BOOLEAN_CARRIER}\nconst fn manufacture<const P: bool>() -> Evidence<P> {{ Evidence<P> {{}} }}"
    );
    let file = input(&mut db, "manufacture", &source);
    let errors = diagnostics(&db, file);
    assert!(errors.contains("const requirement"), "{errors}");

    let source =
        format!("{TRAIT_CARRIER}\nconst fn manufacture<T>() -> Evidence<T> {{ Evidence<T> {{}} }}");
    let file = input(&mut db, "manufacture-trait", &source);
    let errors = diagnostics(&db, file);
    assert!(!errors.is_empty() && errors.contains("Compute"), "{errors}");
}

#[test]
fn carrier_validity_edits_do_not_survive_from_a_warm_database() {
    let mut db = database();
    for allowed in [true, false, true] {
        let source = format!(
            "{BOOLEAN_CARRIER}\nconst fn answer() -> u256 {{ let evidence = Evidence<{allowed}> {{}}\n42 }}"
        );
        let file = input(&mut db, "carrier-edits", &source);
        let warm = diagnostics(&db, file);
        assert_eq!(warm.is_empty(), allowed, "{warm}");
        let mut fresh = database();
        let fresh_file = input(&mut fresh, "carrier-edits", &source);
        assert_eq!(warm, diagnostics(&fresh, fresh_file));
        if allowed {
            assert_eq!(evaluate(&db, file, "answer"), "42");
        }
    }
}

#[test]
fn opening_unproven_evidence_in_a_constant_reports_the_missing_impl() {
    let mut db = database();
    let source = include_str!("fixtures/evidence_boundary/open_unproven_const.fe");
    for source in [
        source.to_owned(),
        source.replace("const X: u64 = run()", ""),
    ] {
        let file = input(&mut db, "open-unproven", &source);
        let errors = diagnostics(&db, file);
        assert!(errors.contains("error[6-0003]"), "{errors}");
        assert!(
            errors.contains("`Subject` doesn't implement `Produce`"),
            "{errors}"
        );
    }
}

#[test]
fn selected_associated_output_cannot_escape_as_the_ordinary_field_type() {
    let mut db = database();
    let file = input(
        &mut db,
        "associated-output-escape",
        include_str!("fixtures/evidence_boundary/associated_output_escape.fe"),
    );
    let errors = diagnostics(&db, file);
    assert!(
        errors.contains("error[6-0025]"),
        "a selected bool field escaped as an ordinary u256 field: {errors}"
    );
}

#[test]
fn named_associated_types_must_match_the_ordinary_definition() {
    for source in [
        "trait Value { type Output }\nstruct Subject {}\nimpl Value for Subject { type Output = u256 }\nimpl Value for Subject as Flag { type Output = bool }",
        "trait Value { type Output }\nstruct Holder<T> {}\nimpl<T> Value for Holder<T> { type Output = T }\nimpl<U> Value for Holder<U> as Flag { type Output = u8 }",
        "trait Value { type Output<T> }\nstruct Subject {}\nimpl Value for Subject { type Output<T> = T }\nimpl Value for Subject as Flag { type Output<U> = bool }",
    ] {
        let mut db = database();
        let file = input(&mut db, "named-output-change", source);
        let errors = diagnostics(&db, file);
        assert!(errors.contains("error[6-0025]"), "{errors}");
        assert!(
            errors.contains("named implementation changes associated type"),
            "{errors}"
        );
    }
}

#[test]
fn matching_or_named_only_associated_types_remain_allowed() {
    for source in [
        "trait Value { type Output }\nstruct Subject {}\nimpl Value for Subject as Flag { type Output = bool }",
        "trait Value { type Output }\nimpl<T: Value> Value for T as Forward { type Output = T::Output }",
        "type Word = u256\ntrait Value { type Output }\nstruct Subject {}\nimpl Value for Subject { type Output = Word }\nimpl Value for Subject as Flag { type Output = u256 }",
        "trait Value { type Output }\nstruct Holder<T> {}\nimpl<T> Value for Holder<T> { type Output = T }\nimpl<U> Value for Holder<U> as Flag { type Output = U }",
        "trait Value { type Output<T> }\nstruct Subject {}\nimpl Value for Subject { type Output<T> = T }\nimpl Value for Subject as Flag { type Output<U> = U }",
    ] {
        let mut db = database();
        let file = input(&mut db, "matching-output", source);
        let errors = diagnostics(&db, file);
        assert!(errors.is_empty(), "{errors}");
    }
}

#[test]
fn read_only_evm_cannot_construct_writable_storage_handles() {
    let source = include_str!("fixtures/evidence_boundary/readonly_evm_storage.fe");
    for source in [
        source.to_owned(),
        source
            .replace("StorPtr", "TStorPtr")
            .replace(".stor_ptr(", ".tstor_ptr("),
    ] {
        let mut db = database();
        let file = input(&mut db, "readonly-storage", &source);
        let errors = diagnostics(&db, file);
        assert!(
            errors.contains("error[8-0117]: storage mutation requires the write capability"),
            "read-only EVM access wrote through an opened storage handle: {errors}"
        );
    }
}

#[test]
fn storage_write_capability_and_read_only_operations_remain_available() {
    let source = include_str!("fixtures/evidence_boundary/readonly_evm_storage.fe")
        .replace("uses (evm: Evm)", "uses (evm: mut Evm)");
    for source in [
        source.clone(),
        source.replace("StorPtr", "TStorPtr").replace(".stor_ptr(", ".tstor_ptr("),
        "use std::evm::{Evm, CalldataPtr}\nstruct Cell { value: u256 }\nfn read(_ slot: u256) -> u256 uses (evm: Evm) { evm.sload(slot) }\nfn input(_ offset: u256) -> CalldataPtr<Cell> uses (evm: Evm) { evm.calldata_ptr(offset) }".to_owned(),
    ] {
        let mut db = database();
        let file = input(&mut db, "storage-controls", &source);
        let errors = diagnostics(&db, file);
        assert!(errors.is_empty(), "{errors}");
    }
}

#[test]
fn mutable_contract_storage_keeps_its_write_capability() {
    let source = include_str!("fixtures/evidence_boundary/mutable_contract_storage.fe");
    let mut db = database();
    let file = input(&mut db, "mutable-contract-storage", source);
    let errors = diagnostics(&db, file);
    assert!(errors.is_empty(), "{errors}");
}

#[test]
fn read_only_evm_can_construct_and_read_storage_handles() {
    for source in [
        "use std::evm::{Evm, StorPtr, RawStorage}\nfn peek() -> u256 uses (evm: Evm) { let p: StorPtr<u256> = evm.stor_ptr(0)\np.read() }",
        "use std::evm::{Evm, TStorPtr, RawOps}\nfn peek() -> u256 uses (evm: Evm) { let p: TStorPtr<u256> = evm.tstor_ptr(0)\np.read() }",
    ] {
        let mut db = database();
        let file = input(&mut db, "readonly-handle-construction", source);
        let errors = diagnostics(&db, file);
        assert!(errors.is_empty(), "{errors}");
    }
}

#[test]
fn read_only_storage_map_copy_requires_the_write_capability() {
    let source = include_str!("fixtures/evidence_boundary/readonly_storage_map_copy.fe");
    let mut db = database();
    let file = input(&mut db, "readonly-storage-map-copy", source);
    let errors = diagnostics(&db, file);
    assert!(
        errors.contains("RawStorage"),
        "copied storage handle wrote without the write capability: {errors}"
    );
}

#[test]
fn copied_storage_handle_writes_require_mutable_storage_capability() {
    for (typ, operation) in [
        ("StorageMap<u256, u256, 0>", "copy.set(key: 1, value: 99)"),
        ("StorPtr<u256>", "copy.write(99)"),
        ("TStorPtr<u256>", "copy.write(99)"),
        ("TSlot<u256, 0>", "copy.write(99)"),
        ("SolArray<u256>", "copy.set(index: 0, value: 99)"),
        ("SolSlot", "copy.write(offset: 0, value: 99 as u256)"),
        (
            "SolMapping",
            "SolSlot::at(copy.slot_of(1 as u256)).write(offset: 0, value: 99 as u256)",
        ),
        ("SolEnumerableSet<u256>", "let inserted = copy.add(99)"),
    ] {
        let source = format!(
            "use std::evm::*\nfn write(_ original: {typ}) uses (evm: Evm) {{\n    let mut copy = original\n    with (RawStorage = evm) {{\n        {operation}\n    }}\n}}"
        );
        let mut db = database();
        let file = input(&mut db, "copied-storage-write", &source);
        let errors = diagnostics(&db, file);
        assert!(
            errors.contains("mutable") || errors.contains("RawStorage"),
            "{typ} write did not require mutable capability: {errors}"
        );
        let authorized = source.replace("evm: Evm", "evm: mut Evm");
        let file = input(&mut db, "authorized-storage-write", &authorized);
        let errors = diagnostics(&db, file);
        assert!(
            errors.is_empty(),
            "{typ} authorized write rejected: {errors}"
        );
    }
}

#[test]
fn copied_storage_descriptor_cannot_supply_a_mutable_helper_effect() {
    let base = include_str!("fixtures/evidence_boundary/copied_descriptor_mutable_helper.fe");
    for binding in ["copy", "mut copy"] {
        let source = base.replace("with (copy)", &format!("with ({binding})"));
        let mut db = database();
        let file = input(&mut db, "copied-storage-helper", &source);
        let errors = diagnostics(&db, file);
        assert!(
            errors.contains("storage mutation requires the write capability"),
            "{errors}"
        );
        let authorized = source.replace(
            "uses (balances: StorageMap",
            "uses (balances: mut StorageMap",
        );
        let file = input(&mut db, "authorized-storage-helper", &authorized);
        let errors = diagnostics(&db, file);
        assert!(errors.is_empty(), "{errors}");
    }
}

#[test]
fn mutable_memory_effect_does_not_authorize_storage_writes() {
    let source = "use std::evm::StorageMap\nfn write(_ original: StorageMap<u256, u256, 0>) uses (memory: mut u256) { let mut copy = original\ncopy.set(key: 1, value: 99) }";
    let mut db = database();
    let file = input(&mut db, "memory-is-not-storage-authority", source);
    let errors = diagnostics(&db, file);
    assert!(errors.contains("RawStorage"), "{errors}");
}

#[test]
fn mutable_storage_receiver_checks_the_callers_write_effects() {
    let source = "use std::evm::StorageMap\nstruct Store { map: StorageMap<u256, u256, 0> }\nimpl Store { fn write(mut self) { self.map.set(key: 1, value: 99) } }\nfn caller() uses (map: StorageMap<u256, u256, 0>) { let mut copy = Store { map }\ncopy.write() }";
    let mut db = database();
    let file = input(&mut db, "storage-receiver-readonly-caller", source);
    let errors = diagnostics(&db, file);
    assert!(
        errors.contains("storage mutation requires the write capability"),
        "{errors}"
    );
    assert!(errors.contains("copy.write()"), "{errors}");
    let authorized = source.replace("uses (map: StorageMap", "uses (map: mut StorageMap");
    let file = input(&mut db, "storage-receiver-authorized-caller", &authorized);
    let errors = diagnostics(&db, file);
    assert!(errors.is_empty(), "{errors}");
}

#[test]
fn mutable_storage_receiver_transports_authority_across_a_loop() {
    let source = "use std::evm::StorageMap\nstruct Store { map: StorageMap<u256, u256, 0> }\nimpl Store { fn write(mut self, _ value: u256) { let mut i = 0\nwhile i < 2 { self.map.set(key: i, value)\ni += 1 }\nself.map.set(key: i, value) } }\nfn caller() uses (store: mut Store) { store.write(99) }";
    let mut db = database();
    let file = input(&mut db, "storage-receiver-loop", source);
    let errors = diagnostics(&db, file);
    assert!(errors.is_empty(), "{errors}");
}

#[test]
fn generic_storage_receiver_transports_authority_across_a_loop() {
    let source = "use std::evm::{StorageMap, WordRepr}\nuse core::ops::Ord\nstruct Store<T> { map: StorageMap<u256, T, 0> }\nimpl<T> Store<T> where T: WordRepr + Ord + Copy { fn write(mut self, _ value: T) { let mut i = 0\nwhile i < 2 { let previous = self.map.get(i)\nif !value.lt(previous) { break }\nself.map.set(key: i, value: previous)\ni += 1 }\nself.map.set(key: i, value) } }\nfn caller() uses (store: mut Store<u256>) { store.write(99) }";
    for source in [
        source.to_owned(),
        source.replace("StorageMap<u256, T, 0>", "StorageMap<u256, T>"),
    ] {
        let mut db = database();
        let file = input(&mut db, "generic-storage-receiver-loop", &source);
        let errors = diagnostics(&db, file);
        assert!(errors.is_empty(), "{errors}");
    }
}

#[test]
fn storage_write_root_is_registered_inside_the_standard_library() {
    let mut db = database();
    let file = input(&mut db, "std-storage-root", "fn caller() {}");
    let scope = named(&db, file, "caller").scope();
    let raw = hir::analysis::ty::corelib::resolve_lib_trait_path(
        &db,
        scope,
        "std::evm::effects::RawStorage",
    )
    .unwrap();
    let root = hir::analysis::ty::resolve_default_root_effect_ty(
        &db,
        raw.scope(),
        hir::analysis::ty::trait_resolution::PredicateListId::empty_list(&db),
    );
    assert!(
        root.is_some(),
        "standard-library storage methods need their registered root provider"
    );
}

#[test]
fn mutable_aggregate_effect_transports_storage_write_authority() {
    let source = "use std::evm::StorageMap\nstruct Store { map: StorageMap<u256, u256, 0> }\nfn write() uses (store: mut Store) { store.map.set(key: 1, value: 99) }";
    let mut db = database();
    let file = input(&mut db, "mutable-storage-aggregate", source);
    let errors = diagnostics(&db, file);
    assert!(errors.is_empty(), "{errors}");
}

#[test]
fn storage_effect_marker_requires_its_seal() {
    let source = "use std::evm::StorageEffect\nstruct Memory {}\nimpl StorageEffect for Memory {}";
    let mut db = database();
    let file = input(&mut db, "storage-effect-seal", source);
    let errors = diagnostics(&db, file);
    assert!(errors.contains("StorageEffectSeal"), "{errors}");
}

#[test]
fn copied_storage_handles_keep_read_only_access() {
    for (typ, operation) in [
        ("StorageMap<u256, u256, 0>", "copy.get(1)"),
        ("StorPtr<u256>", "copy.read()"),
        ("TStorPtr<u256>", "copy.read()"),
        ("TSlot<u256, 0>", "copy.read()"),
        ("TSlot<u256, 0>", "copy.ptr().read()"),
        ("SolArray<u256>", "copy.get(0)"),
        ("SolSlot", "copy.read(offset: 0)"),
        ("SolMapping", "copy.slot_of(1 as u256)"),
    ] {
        let source = format!(
            "use std::evm::*\nfn read(_ original: {typ}) -> u256 uses (evm: Evm) {{\n    let copy = original\n    with (RawStorage = evm) {{\n        {operation}\n    }}\n}}"
        );
        let mut db = database();
        let file = input(&mut db, "copied-storage-read", &source);
        let errors = diagnostics(&db, file);
        assert!(
            errors.is_empty(),
            "{typ} read-only access rejected: {errors}"
        );
    }
}

#[test]
fn one_frame_rejects_duplicate_trait_provisions_in_both_orders() {
    let source = include_str!("fixtures/evidence_boundary/same_frame_duplicate.fe");
    let mut db = database();
    let file = input(&mut db, "same-frame-duplicates", source);
    let errors = diagnostics(&db, file);
    assert_eq!(
        errors.matches("error[8-0097]").count(),
        2,
        "each ordering must reject its second provision: {errors}"
    );
}

const PROVISION_CONTROLS: &str = "
use core::obligations::prove
trait Compute { const fn compute() -> u256 }
struct Subject {}
impl Compute for Subject { const fn compute() -> u256 { 1 } }
impl Compute for Subject as Flag { const fn compute() -> u256 { 10 } }
impl Compute for Subject as Flag2 { const fn compute() -> u256 { 20 } }
const fn h<T: Compute>() -> u256 { T::compute() }
";

#[test]
fn same_frame_rejects_evidence_and_mixed_provision_duplicates() {
    for bindings in [
        "e1, e2",
        "e2, e1",
        "e1, e1",
        "Flag, Flag",
        "e1, Flag",
        "Flag, e1",
    ] {
        let source = format!("{PROVISION_CONTROLS}
const fn answer() -> u256 {{ let e1 = prove<Compute<Subject>>()\n let e2 = prove<Compute<Subject>>()\n with ({bindings}) {{ h<Subject>() }} }}");
        let mut db = database();
        let file = input(&mut db, "duplicate-spellings", &source);
        let errors = diagnostics(&db, file);
        assert_eq!(
            errors.matches("error[8-0097]").count(),
            1,
            "{bindings}: {errors}"
        );
    }
}

#[test]
fn nested_provision_frames_choose_inner_and_restore_outer() {
    for (expression, expected) in [
        (
            "with (Flag) { let inner = with (Flag2) { h<Subject>() }\n inner * 100 + h<Subject>() }",
            "2010",
        ),
        (
            "with (Flag2) { let inner = with (Flag) { h<Subject>() }\n inner * 100 + h<Subject>() }",
            "1020",
        ),
        ("with (Flag) { with (e) { h<Subject>() } }", "1"),
        ("with (e) { with (Flag) { h<Subject>() } }", "10"),
        (
            "with (Flag) { let inner = with (e) { h<Subject>() }\n inner * 100 + h<Subject>() }",
            "110",
        ),
        (
            "with (e) { let inner = with (Flag) { h<Subject>() }\n inner * 100 + h<Subject>() }",
            "1001",
        ),
        ("with (e) { with (e) { h<Subject>() } }", "1"),
    ] {
        let source = format!(
            "{PROVISION_CONTROLS}
const fn answer() -> u256 {{ let e = prove<Compute<Subject>>()\n {expression} }}"
        );
        let mut db = database();
        let file = checked(&mut db, "nested-provision-spellings", &source);
        assert_eq!(evaluate(&db, file, "answer"), expected, "{expression}");
    }
}

#[test]
fn one_frame_keeps_distinct_trait_arguments_in_separate_slots() {
    let source = "
trait Compute<T> { const fn compute() -> u256 }
struct Subject {}
impl Compute<u8> for Subject { const fn compute() -> u256 { 1 } }
impl Compute<u16> for Subject { const fn compute() -> u256 { 2 } }
impl Compute<u8> for Subject as Narrow { const fn compute() -> u256 { 10 } }
impl Compute<u16> for Subject as Wide { const fn compute() -> u256 { 20 } }
const fn answer() -> u256 {
    with (Narrow, Wide) {
        <Subject as Compute<u8>>::compute() * 100 + <Subject as Compute<u16>>::compute()
    }
}
";
    let mut db = database();
    let file = checked(&mut db, "distinct-provision-slots", source);
    assert_eq!(evaluate(&db, file, "answer"), "1020");
}

#[test]
fn trait_and_effect_provisions_share_nested_frame_lifetimes() {
    for (outer, inner, expected) in [
        ("Flag, outer", "inner, Flag2", "2312"),
        ("outer, Flag2", "Flag, inner", "1322"),
    ] {
        let source = format!(
            "{PROVISION_CONTROLS}
struct Counter {{ value: u256 }}
trait View {{ const fn get(ref self) -> u256 }}
impl View for Counter {{ const fn get(ref self) -> u256 {{ self.value }} }}
const fn read<T: Compute>() -> u256 uses (counter: View) {{ T::compute() + counter.get() }}
const fn answer() -> u256 {{
    let outer = Counter {{ value: 2 }}
    let inner = Counter {{ value: 3 }}
    with ({outer}) {{
        let nested = with ({inner}) {{ read<Subject>() }}
        nested * 100 + read<Subject>()
    }}
}}"
        );
        let mut db = database();
        let file = checked(&mut db, "combined-provision-frames", &source);
        assert_eq!(evaluate(&db, file, "answer"), expected);
    }
}

#[test]
fn both_evidence_opening_spellings_retain_their_successful_record() {
    let source = format!(
        "{PROVISION_CONTROLS}
use core::obligations::Evidence
const fn parameter<T>(_ e: Evidence<Compute<T>>) -> u256 {{ with (e) {{ T::compute() }} }}
const fn local() -> u256 {{
    let e = prove<Compute<Subject>>()
    with (e) {{ h<Subject>() }}
}}"
    );
    let mut db = database();
    let file = checked(&mut db, "retained-evidence-openings", &source);
    for name in ["parameter", "local"] {
        let (errors, typed) =
            hir::analysis::ty::ty_check::check_func_body(&db, named(&db, file, name));
        assert!(errors.is_empty(), "{name}: {errors:?}");
        assert_eq!(
            typed.with_choices().len(),
            1,
            "{name}: opening was not retained"
        );
        let record = typed.with_choices().next().unwrap();
        let hir::core::semantic::Provision::Evidence(value, goal) = record.outcome().unwrap()
        else {
            panic!("{name}: expected evidence provision");
        };
        assert_eq!(value, record.expression());
        assert_eq!(
            goal.def(&db).name(&db).to_opt().unwrap().data(&db),
            "Compute"
        );
        if name == "parameter" {
            assert!(matches!(
                goal.self_ty(&db).data(&db),
                hir::analysis::ty::ty_def::TyData::TyParam(_)
            ));
        } else {
            assert_eq!(goal.self_ty(&db).pretty_print(&db), "Subject");
        }
    }
}
