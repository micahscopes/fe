use super::{
    OriginExportKey, OriginExportKeyError, OriginExportKind, OriginExportLocalKey, OriginGraph,
    OriginKey, OriginKeyTextError, OriginLink, OriginLinkKind,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
struct TestOwner(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
struct TestLocal(u32);

crate::define_origin_string_key! {
    struct TestStringKey;
}

crate::define_origin_owner_key! {
    struct TestOwnerKey;
}

crate::define_origin_local_key! {
    struct TestLocalKey;
}

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
    enum TestClosedKind {
        Alpha => "alpha",
        BetaValue => "beta_value",
    }
}

fn raw_export_key(kind: OriginExportKind, owner: &str, local: &str) -> OriginExportKey {
    OriginExportKey::try_from_raw_parts(kind, owner, local).unwrap()
}

crate::define_origin_key_type! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
    struct TestOrigin {
        owner: TestOwner => owner,
        local: TestLocal => local
    }
}

#[test]
fn same_local_id_in_different_owners_does_not_collide() {
    let first = OriginKey::new(TestOwner(0), TestLocal(7));
    let second = OriginKey::new(TestOwner(1), TestLocal(7));

    assert_ne!(first, second);
}

#[test]
fn same_owner_with_different_local_ids_does_not_collide() {
    let first = OriginKey::new(TestOwner(0), TestLocal(7));
    let second = OriginKey::new(TestOwner(0), TestLocal(8));

    assert_ne!(first, second);
}

#[test]
fn owner_and_local_id_round_trip() {
    let key = OriginKey::new(TestOwner(2), TestLocal(3));

    assert_eq!(key.owner(), &TestOwner(2));
    assert_eq!(key.local(), &TestLocal(3));
    assert_eq!(key.into_parts(), (TestOwner(2), TestLocal(3)));
}

#[test]
fn export_key_keeps_kind_owner_and_local_separate() {
    let expr = raw_export_key(OriginExportKind::HirExpr, "body:a", "0");
    let stmt = raw_export_key(OriginExportKind::HirStmt, "body:a", "0");
    let other_body_expr = raw_export_key(OriginExportKind::HirExpr, "body:b", "0");

    assert_ne!(expr, stmt);
    assert_ne!(expr, other_body_expr);
    assert_eq!(expr.kind(), OriginExportKind::HirExpr);
    assert_eq!(expr.owner_key(), "body:a");
    assert_eq!(expr.local_key(), "0");
    assert_eq!(OriginExportKind::HirExpr.as_str(), "hir.expr");
}

#[test]
fn export_key_constructor_requires_typed_owner_and_local_key_parts() {
    let key = OriginExportKey::new(
        OriginExportKind::Semantic,
        &TestOwnerKey::new("semantic:test"),
        &TestLocalKey::new("expr:0"),
    );

    assert_eq!(key.owner_key(), "semantic:test");
    assert_eq!(key.local_key(), "expr:0");
}

#[test]
fn export_key_formats_canonical_storage_key_and_display_label() {
    let key = raw_export_key(
        OriginExportKind::BytecodePc,
        "object:Foo:section:runtime",
        "pc:4..8",
    );

    assert_eq!(
        key.canonical_storage_key(),
        "bytecode.pc\u{1f}object:Foo:section:runtime\u{1f}pc:4..8"
    );
    assert_eq!(
        key.display_label(),
        "bytecode.pc:object:Foo:section:runtime:pc:4..8"
    );
}

#[test]
fn export_key_rejects_empty_owner_and_local_parts() {
    assert_eq!(
        OriginExportKey::try_from_raw_parts(OriginExportKind::Semantic, "", "expr:0"),
        Err(OriginExportKeyError::EmptyOwnerKey)
    );
    assert_eq!(
        OriginExportKey::try_from_raw_parts(OriginExportKind::Semantic, "semantic:test", ""),
        Err(OriginExportKeyError::EmptyLocalKey)
    );
}

#[test]
fn export_key_rejects_reserved_storage_separator() {
    assert_eq!(
        OriginExportKey::try_from_raw_parts(
            OriginExportKind::Semantic,
            "semantic\u{1f}test",
            "expr:0",
        ),
        Err(OriginExportKeyError::ReservedStorageSeparator { field: "owner_key" })
    );
    assert_eq!(
        OriginExportKey::try_from_raw_parts(
            OriginExportKind::Semantic,
            "semantic:test",
            "expr\u{1f}0",
        ),
        Err(OriginExportKeyError::ReservedStorageSeparator { field: "local_key" })
    );
}

#[test]
fn export_key_deserialization_validates_parts() {
    let json = r#"{
        "kind": "semantic",
        "owner_key": "",
        "local_key": "expr:0"
    }"#;

    let err = serde_json::from_str::<OriginExportKey>(json)
        .expect_err("origin export key decoding should validate owner/local parts");
    assert!(
        err.to_string()
            .contains("origin export owner key must not be empty")
    );
}

#[test]
fn origin_string_key_macro_defines_nominal_string_wrappers() {
    let key = TestStringKey::new("runtime:test");

    assert_eq!(key.as_str(), "runtime:test");
    assert_eq!(
        TestStringKey::try_new("runtime:test").expect("valid origin string key should construct"),
        key
    );
    assert_eq!(key, TestStringKey::new("runtime:test"));
    assert_ne!(key, TestStringKey::new("semantic:test"));
}

#[test]
fn origin_string_key_macro_rejects_empty_keys() {
    assert_eq!(
        TestStringKey::try_new(""),
        Err(OriginKeyTextError::Empty {
            kind: "origin string key"
        })
    );
}

#[test]
fn origin_string_key_macro_rejects_reserved_storage_separators() {
    assert_eq!(
        TestStringKey::try_new("runtime\u{1f}test"),
        Err(OriginKeyTextError::ReservedStorageSeparator {
            kind: "origin string key"
        })
    );
}

#[test]
fn origin_owner_key_macro_defines_export_owner_wrappers() {
    fn accepts_owner_key(key: &impl super::OriginExportOwnerKey) -> &str {
        key.as_str()
    }

    let key = TestOwnerKey::new("runtime:test");

    assert_eq!(accepts_owner_key(&key), "runtime:test");
    assert_eq!(
        TestOwnerKey::try_new("runtime:test").expect("valid origin owner key should construct"),
        key
    );
    assert_eq!(key, TestOwnerKey::new("runtime:test"));
    assert_ne!(key, TestOwnerKey::new("semantic:test"));
}

#[test]
fn origin_owner_key_macro_rejects_empty_keys() {
    assert_eq!(
        TestOwnerKey::try_new(""),
        Err(OriginKeyTextError::Empty {
            kind: "origin owner key"
        })
    );
}

#[test]
fn origin_owner_key_macro_rejects_reserved_storage_separators() {
    assert_eq!(
        TestOwnerKey::try_new("runtime\u{1f}test"),
        Err(OriginKeyTextError::ReservedStorageSeparator {
            kind: "origin owner key"
        })
    );
}

#[test]
fn origin_local_key_macro_defines_export_local_wrappers() {
    let key = TestLocalKey::new("expr:0");

    assert_eq!(key.to_export_local_key(), "expr:0");
    assert_eq!(
        TestLocalKey::try_new("expr:0").expect("valid origin local key should construct"),
        key
    );
    assert_ne!(key, TestLocalKey::new("stmt:0"));
}

#[test]
fn origin_local_key_macro_rejects_empty_keys() {
    assert_eq!(
        TestLocalKey::try_new(""),
        Err(OriginKeyTextError::Empty {
            kind: "origin local key"
        })
    );
}

#[test]
fn origin_local_key_macro_rejects_reserved_storage_separators() {
    assert_eq!(
        TestLocalKey::try_new("expr\u{1f}0"),
        Err(OriginKeyTextError::ReservedStorageSeparator {
            kind: "origin local key"
        })
    );
}

#[test]
fn closed_string_enum_macro_defines_string_and_serde_policy() {
    assert_eq!(TestClosedKind::STRINGS, &["alpha", "beta_value"]);
    assert_eq!(TestClosedKind::BetaValue.as_str(), "beta_value");
    assert_eq!(
        TestClosedKind::from_str("alpha"),
        Some(TestClosedKind::Alpha)
    );
    assert_eq!(TestClosedKind::from_str("missing"), None);
    assert_eq!(
        serde_json::to_string(&TestClosedKind::BetaValue).unwrap(),
        "\"beta_value\""
    );
    assert_eq!(TestClosedKind::BetaValue.to_string(), "beta_value");
    assert_eq!(
        serde_json::from_str::<TestClosedKind>("\"alpha\"").unwrap(),
        TestClosedKind::Alpha
    );

    let err = serde_json::from_str::<TestClosedKind>("\"missing\"")
        .expect_err("unknown closed string enum value should fail");
    assert!(err.to_string().contains("unknown variant `missing`"));
}

#[test]
fn origin_key_type_macro_defines_nominal_owner_local_wrappers() {
    let origin = TestOrigin::new(TestOwner(4), TestLocal(9));

    assert_eq!(origin.owner(), TestOwner(4));
    assert_eq!(origin.local(), TestLocal(9));
}

#[test]
fn shared_origin_identity_types_derive_salsa_update() {
    fn assert_update<T: salsa::Update>() {}

    assert_update::<OriginKey<TestOwner, TestLocal>>();
    assert_update::<OriginLink<OriginKey<TestOwner, TestLocal>>>();
    assert_update::<OriginGraph<OriginKey<TestOwner, TestLocal>>>();
    assert_update::<TestStringKey>();
    assert_update::<TestOwnerKey>();
    assert_update::<TestLocalKey>();
    assert_update::<TestClosedKind>();
    assert_update::<TestOrigin>();
}

unsafe fn maybe_update<T: salsa::Update>(old: &mut T, new: T) -> bool {
    unsafe { T::maybe_update(old as *mut T, new) }
}

#[test]
fn origin_key_update_is_fieldwise_and_precise() {
    let mut key = OriginKey::new(TestOwner(1), TestLocal(2));

    assert!(!unsafe { maybe_update(&mut key, OriginKey::new(TestOwner(1), TestLocal(2))) });
    assert_eq!(key.into_parts(), (TestOwner(1), TestLocal(2)));

    let mut key = OriginKey::new(TestOwner(1), TestLocal(2));
    assert!(unsafe { maybe_update(&mut key, OriginKey::new(TestOwner(1), TestLocal(3))) });
    assert_eq!(key.into_parts(), (TestOwner(1), TestLocal(3)));
}

#[test]
fn origin_link_update_is_fieldwise_and_precise() {
    let mut link = OriginLink::new(TestOwner(1), TestOwner(2), OriginLinkKind::Lowered);

    assert!(!unsafe {
        maybe_update(
            &mut link,
            OriginLink::new(TestOwner(1), TestOwner(2), OriginLinkKind::Lowered),
        )
    });
    assert_eq!(
        link.into_parts(),
        (TestOwner(1), TestOwner(2), OriginLinkKind::Lowered)
    );

    let mut link = OriginLink::new(TestOwner(1), TestOwner(2), OriginLinkKind::Lowered);
    assert!(unsafe {
        maybe_update(
            &mut link,
            OriginLink::new(TestOwner(1), TestOwner(3), OriginLinkKind::Alias),
        )
    });
    assert_eq!(
        link.into_parts(),
        (TestOwner(1), TestOwner(3), OriginLinkKind::Alias)
    );
}

#[test]
fn origin_graph_update_is_fieldwise_and_precise() {
    let mut graph = OriginGraph::from_links(vec![OriginLink::new(
        TestOwner(1),
        TestOwner(2),
        OriginLinkKind::Lowered,
    )]);

    assert!(!unsafe {
        maybe_update(
            &mut graph,
            OriginGraph::from_links(vec![OriginLink::new(
                TestOwner(1),
                TestOwner(2),
                OriginLinkKind::Lowered,
            )]),
        )
    });
    assert_eq!(graph.links().len(), 1);

    assert!(unsafe {
        maybe_update(
            &mut graph,
            OriginGraph::from_links(vec![
                OriginLink::new(TestOwner(1), TestOwner(3), OriginLinkKind::Alias),
                OriginLink::new(TestOwner(3), TestOwner(4), OriginLinkKind::Transformed),
            ]),
        )
    });
    let links = graph.links();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].to(), &TestOwner(3));
    assert_eq!(links[0].kind(), OriginLinkKind::Alias);
    assert_eq!(links[1].from(), &TestOwner(3));
    assert_eq!(links[1].to(), &TestOwner(4));
    assert_eq!(links[1].kind(), OriginLinkKind::Transformed);
}

#[test]
fn origin_link_preserves_direction_and_kind() {
    let link = OriginLink::new(1u32, 2u32, OriginLinkKind::Lowered);

    assert_eq!(link.from(), &1);
    assert_eq!(link.to(), &2);
    assert_eq!(link.kind(), OriginLinkKind::Lowered);
    assert_eq!(link.into_parts(), (1, 2, OriginLinkKind::Lowered));
}

#[test]
fn origin_graph_supports_many_to_many_links() {
    let mut graph = OriginGraph::new();

    graph.push("hir_expr", "mir_stmt_0", OriginLinkKind::Lowered);
    graph.push("hir_expr", "mir_stmt_1", OriginLinkKind::Lowered);
    graph.push("hir_stmt", "mir_stmt_1", OriginLinkKind::Expanded);

    assert_eq!(graph.len(), 3);
    assert_eq!(graph.outgoing_from(&"hir_expr").count(), 2);
    assert_eq!(graph.incoming_to(&"mir_stmt_1").count(), 2);
}

#[test]
fn origin_graph_can_be_built_from_links_and_consumed() {
    let links = vec![
        OriginLink::new(0u32, 1u32, OriginLinkKind::Alias),
        OriginLink::new(1u32, 2u32, OriginLinkKind::Transformed),
    ];

    let graph = OriginGraph::from_links(links.clone());

    assert!(!graph.is_empty());
    assert_eq!(graph.links(), links.as_slice());
    assert_eq!(graph.into_links(), links);
}
