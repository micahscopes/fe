use std::{fmt, slice};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Owner-aware identity for origin nodes whose local IDs are scoped.
///
/// The fields are intentionally private: callers must provide an owner and a
/// local ID together, so a body-local ID cannot masquerade as a global origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OriginKey<Owner, Local> {
    owner: Owner,
    local: Local,
}

// SAFETY: `OriginKey` is a transparent owner/local product. Updating it by
// delegating to each field's `salsa::Update` impl preserves Salsa's revision
// semantics without comparing database-tied references directly. The Salsa
// derive currently does not add the required bounds for this generic type.
unsafe impl<Owner, Local> salsa::Update for OriginKey<Owner, Local>
where
    Owner: salsa::Update,
    Local: salsa::Update,
{
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let mut changed = false;
        unsafe {
            changed |= Owner::maybe_update(&mut (*old_pointer).owner, new_value.owner);
            changed |= Local::maybe_update(&mut (*old_pointer).local, new_value.local);
        }
        changed
    }
}

impl<Owner, Local> OriginKey<Owner, Local> {
    pub const fn new(owner: Owner, local: Local) -> Self {
        Self { owner, local }
    }

    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    pub fn local(&self) -> &Local {
        &self.local
    }

    pub fn into_parts(self) -> (Owner, Local) {
        (self.owner, self.local)
    }
}

#[macro_export]
macro_rules! define_closed_string_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $value:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
        }

        impl $name {
            pub const STRINGS: &'static [&'static str] = &[$($value),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value,)+
                }
            }

            pub fn from_str(raw: &str) -> Option<Self> {
                match raw {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let raw = <::std::string::String as ::serde::Deserialize>::deserialize(
                    deserializer,
                )?;
                Self::from_str(&raw).ok_or_else(|| {
                    <D::Error as ::serde::de::Error>::unknown_variant(&raw, Self::STRINGS)
                })
            }
        }
    };
}

crate::define_closed_string_enum! {
    /// Export-facing origin node kind.
    ///
    /// These kinds are deliberately separate from compiler-internal node keys. A
    /// boundary exporter can assign compact fact IDs, but the stable key remains a
    /// structured string pair tagged with this kind.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
    pub enum OriginExportKind {
        HirExpr => "hir.expr",
        HirStmt => "hir.stmt",
        Semantic => "semantic",
        RuntimeStmt => "runtime.stmt",
        RuntimeTerminator => "runtime.terminator",
        RuntimeCodeRegion => "runtime.code_region",
        RuntimeSynthetic => "runtime.synthetic",
        SonatinaInst => "sonatina.inst",
        SonatinaSynthetic => "sonatina.synthetic",
        BytecodeUnmapped => "bytecode.unmapped",
        BytecodePc => "bytecode.pc",
    }
}

const ORIGIN_EXPORT_KEY_STORAGE_SEPARATOR: char = '\u{1f}';

/// Error returned when building an invalid export-facing origin key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginExportKeyError {
    EmptyOwnerKey,
    EmptyLocalKey,
    ReservedStorageSeparator { field: &'static str },
}

impl fmt::Display for OriginExportKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyOwnerKey => write!(f, "origin export owner key must not be empty"),
            Self::EmptyLocalKey => write!(f, "origin export local key must not be empty"),
            Self::ReservedStorageSeparator { field } => write!(
                f,
                "origin export {field} must not contain the reserved storage separator"
            ),
        }
    }
}

impl std::error::Error for OriginExportKeyError {}

/// Stable key for an origin node that leaves the compiler.
///
/// `owner_key` is the stable identity of the containing object, while
/// `local_key` identifies the node inside that owner. Keeping these as separate
/// fields avoids recreating the old raw-ID namespace problem at export time.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
pub struct OriginExportKey {
    kind: OriginExportKind,
    owner_key: String,
    local_key: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginExportKeySerde {
    kind: OriginExportKind,
    owner_key: String,
    local_key: String,
}

impl OriginExportKey {
    pub fn new(
        kind: OriginExportKind,
        owner_key: impl Into<String>,
        local_key: impl Into<String>,
    ) -> Self {
        Self::try_new(kind, owner_key, local_key)
            .unwrap_or_else(|err| panic!("invalid origin export key: {err}"))
    }

    pub fn try_new(
        kind: OriginExportKind,
        owner_key: impl Into<String>,
        local_key: impl Into<String>,
    ) -> Result<Self, OriginExportKeyError> {
        let owner_key = owner_key.into();
        let local_key = local_key.into();
        validate_origin_export_key_part("owner_key", &owner_key)?;
        validate_origin_export_key_part("local_key", &local_key)?;
        Ok(Self {
            kind,
            owner_key,
            local_key,
        })
    }

    pub const fn kind(&self) -> OriginExportKind {
        self.kind
    }

    pub fn owner_key(&self) -> &str {
        &self.owner_key
    }

    pub fn local_key(&self) -> &str {
        &self.local_key
    }

    pub fn into_parts(self) -> (OriginExportKind, String, String) {
        (self.kind, self.owner_key, self.local_key)
    }

    /// Collision-resistant string for internal maps and fact ID allocation.
    ///
    /// This is intentionally not user-facing: the unit separator keeps
    /// kind/owner/local fields unambiguous even when owner keys contain `:`.
    pub fn canonical_storage_key(&self) -> String {
        format!(
            "{}{}{}{}{}",
            self.kind.as_str(),
            ORIGIN_EXPORT_KEY_STORAGE_SEPARATOR,
            self.owner_key,
            ORIGIN_EXPORT_KEY_STORAGE_SEPARATOR,
            self.local_key
        )
    }

    /// Human-readable label for diagnostics and frontend origin labels.
    pub fn display_label(&self) -> String {
        format!(
            "{}:{}:{}",
            self.kind.as_str(),
            self.owner_key,
            self.local_key
        )
    }
}

impl Serialize for OriginExportKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        OriginExportKeySerde {
            kind: self.kind,
            owner_key: self.owner_key.clone(),
            local_key: self.local_key.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OriginExportKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = OriginExportKeySerde::deserialize(deserializer)?;
        Self::try_new(raw.kind, raw.owner_key, raw.local_key).map_err(de::Error::custom)
    }
}

fn validate_origin_export_key_part(
    field: &'static str,
    value: &str,
) -> Result<(), OriginExportKeyError> {
    if value.is_empty() {
        return match field {
            "owner_key" => Err(OriginExportKeyError::EmptyOwnerKey),
            "local_key" => Err(OriginExportKeyError::EmptyLocalKey),
            _ => Err(OriginExportKeyError::ReservedStorageSeparator { field }),
        };
    }
    if value.contains(ORIGIN_EXPORT_KEY_STORAGE_SEPARATOR) {
        return Err(OriginExportKeyError::ReservedStorageSeparator { field });
    }
    Ok(())
}

pub trait OriginExportOwnerKey {
    fn as_str(&self) -> &str;
}

#[doc(hidden)]
pub fn assert_origin_key_text(kind: &'static str, value: &str) {
    assert!(!value.is_empty(), "{kind} must not be empty");
    assert!(
        !value.contains(ORIGIN_EXPORT_KEY_STORAGE_SEPARATOR),
        "{kind} must not contain reserved origin storage separator"
    );
}

#[macro_export]
macro_rules! define_origin_owner_key {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, $crate::salsa::Update)]
        $vis struct $name(::std::string::String);

        impl $name {
            pub fn new(key: impl Into<::std::string::String>) -> Self {
                let key = key.into();
                $crate::origin::assert_origin_key_text("origin owner key", &key);
                Self(key)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl $crate::origin::OriginExportOwnerKey for $name {
            fn as_str(&self) -> &str {
                self.as_str()
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_string_key {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, $crate::salsa::Update)]
        $vis struct $name(::std::string::String);

        impl $name {
            pub fn new(key: impl Into<::std::string::String>) -> Self {
                let key = key.into();
                $crate::origin::assert_origin_key_text("origin string key", &key);
                Self(key)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_key_type {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $(<$lt:lifetime>)? {
            owner: $owner_ty:ty => $owner:ident,
            local: $local_ty:ty => $local:ident
        }
    ) => {
        $(#[$meta])*
        $vis struct $name $(<$lt>)? {
            key: $crate::origin::OriginKey<$owner_ty, $local_ty>,
        }

        impl $(<$lt>)? $name $(<$lt>)? {
            pub const fn new($owner: $owner_ty, $local: $local_ty) -> Self {
                Self {
                    key: $crate::origin::OriginKey::new($owner, $local),
                }
            }

            pub fn $owner(self) -> $owner_ty {
                self.key.into_parts().0
            }

            pub fn $local(self) -> $local_ty {
                self.key.into_parts().1
            }

            pub fn key(self) -> $crate::origin::OriginKey<$owner_ty, $local_ty> {
                self.key
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_graph_type {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $(<$lt:lifetime>)? ($node:ty);
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        $vis struct $name $(<$lt>)?($crate::origin::OriginGraph<$node>);

        impl $(<$lt>)? ::std::default::Default for $name $(<$lt>)? {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $(<$lt>)? $name $(<$lt>)? {
            pub const fn new() -> Self {
                Self($crate::origin::OriginGraph::new())
            }

            pub fn from_links(
                links: ::std::vec::Vec<$crate::origin::OriginLink<$node>>,
            ) -> Self {
                Self($crate::origin::OriginGraph::from_links(links))
            }

            pub fn push(
                &mut self,
                from: $node,
                to: $node,
                kind: $crate::origin::OriginLinkKind,
            ) {
                self.0.push(from, to, kind);
            }

            pub fn push_link(&mut self, link: $crate::origin::OriginLink<$node>) {
                self.0.push_link(link);
            }

            pub fn extend(
                &mut self,
                links: impl ::std::iter::IntoIterator<
                    Item = $crate::origin::OriginLink<$node>,
                >,
            ) {
                self.0.extend(links);
            }

            pub fn links(&self) -> &[$crate::origin::OriginLink<$node>] {
                self.0.links()
            }

            pub fn into_links(self) -> ::std::vec::Vec<$crate::origin::OriginLink<$node>> {
                self.0.into_links()
            }

            pub fn iter(
                &self,
            ) -> ::std::slice::Iter<'_, $crate::origin::OriginLink<$node>> {
                self.0.iter()
            }

            pub fn len(&self) -> usize {
                self.0.len()
            }

            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }

            pub fn as_origin_graph(&self) -> &$crate::origin::OriginGraph<$node> {
                &self.0
            }

            pub fn into_origin_graph(self) -> $crate::origin::OriginGraph<$node> {
                self.0
            }
        }

        impl $(<$lt>)? $name $(<$lt>)?
        where
            $node: ::std::cmp::PartialEq,
        {
            pub fn outgoing_from<'a>(
                &'a self,
                node: &'a $node,
            ) -> impl ::std::iter::Iterator<
                Item = &'a $crate::origin::OriginLink<$node>,
            > + 'a {
                self.0.outgoing_from(node)
            }

            pub fn incoming_to<'a>(
                &'a self,
                node: &'a $node,
            ) -> impl ::std::iter::Iterator<
                Item = &'a $crate::origin::OriginLink<$node>,
            > + 'a {
                self.0.incoming_to(node)
            }
        }
    };
}

crate::define_closed_string_enum! {
    /// Coarse reason an origin edge exists.
    ///
    /// Exporters can later project these into more specific relation names, but the
    /// core graph should preserve enough intent that a link is not just "related".
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
    pub enum OriginLinkKind {
        /// The target is the next compiler representation of the source.
        Lowered => "lowered",
        /// The target was created while expanding or desugaring the source.
        Expanded => "expanded",
        /// The target was created by a transform or optimization pass.
        Transformed => "transformed",
        /// The target was introduced by the compiler and has no direct source node.
        Synthetic => "synthetic",
        /// The target preserves the source identity in another namespace.
        Alias => "alias",
    }
}

/// Directed edge in an origin graph.
///
/// Direction is earlier artifact to later artifact:
///
/// ```text
/// HIR -> semantic -> MIR -> backend -> bytecode
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OriginLink<Node> {
    from: Node,
    to: Node,
    kind: OriginLinkKind,
}

// SAFETY: `OriginLink` contains only two origin nodes and a link-kind enum.
// Fieldwise `salsa::Update` keeps cached graphs precise while preserving each
// node type's own update invariants.
unsafe impl<Node> salsa::Update for OriginLink<Node>
where
    Node: salsa::Update,
{
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let mut changed = false;
        unsafe {
            changed |= Node::maybe_update(&mut (*old_pointer).from, new_value.from);
            changed |= Node::maybe_update(&mut (*old_pointer).to, new_value.to);
            changed |= OriginLinkKind::maybe_update(&mut (*old_pointer).kind, new_value.kind);
        }
        changed
    }
}

impl<Node> OriginLink<Node> {
    pub const fn new(from: Node, to: Node, kind: OriginLinkKind) -> Self {
        Self { from, to, kind }
    }

    pub fn from(&self) -> &Node {
        &self.from
    }

    pub fn to(&self) -> &Node {
        &self.to
    }

    pub const fn kind(&self) -> OriginLinkKind {
        self.kind
    }

    pub fn into_parts(self) -> (Node, Node, OriginLinkKind) {
        (self.from, self.to, self.kind)
    }
}

/// Immutable origin graph container returned by queries and consumed by exports.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OriginGraph<Node> {
    links: Vec<OriginLink<Node>>,
}

// SAFETY: `OriginGraph` is an owned vector of `OriginLink<Node>`. The vector
// update implementation delegates element updates to `OriginLink<Node>`, so no
// graph-specific aliasing or database lifetime assumptions are introduced here.
unsafe impl<Node> salsa::Update for OriginGraph<Node>
where
    Node: salsa::Update,
{
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        unsafe { Vec::<OriginLink<Node>>::maybe_update(&mut (*old_pointer).links, new_value.links) }
    }
}

impl<Node> Default for OriginGraph<Node> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Node> OriginGraph<Node> {
    pub const fn new() -> Self {
        Self { links: Vec::new() }
    }

    pub fn from_links(links: Vec<OriginLink<Node>>) -> Self {
        Self { links }
    }

    pub fn push(&mut self, from: Node, to: Node, kind: OriginLinkKind) {
        self.links.push(OriginLink::new(from, to, kind));
    }

    pub fn push_link(&mut self, link: OriginLink<Node>) {
        self.links.push(link);
    }

    pub fn extend(&mut self, links: impl IntoIterator<Item = OriginLink<Node>>) {
        self.links.extend(links);
    }

    pub fn links(&self) -> &[OriginLink<Node>] {
        &self.links
    }

    pub fn into_links(self) -> Vec<OriginLink<Node>> {
        self.links
    }

    pub fn iter(&self) -> slice::Iter<'_, OriginLink<Node>> {
        self.links.iter()
    }

    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

impl<Node> OriginGraph<Node>
where
    Node: PartialEq,
{
    pub fn outgoing_from<'a>(
        &'a self,
        node: &'a Node,
    ) -> impl Iterator<Item = &'a OriginLink<Node>> + 'a {
        self.links.iter().filter(move |link| link.from() == node)
    }

    pub fn incoming_to<'a>(
        &'a self,
        node: &'a Node,
    ) -> impl Iterator<Item = &'a OriginLink<Node>> + 'a {
        self.links.iter().filter(move |link| link.to() == node)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        OriginExportKey, OriginExportKeyError, OriginExportKind, OriginGraph, OriginKey,
        OriginLink, OriginLinkKind,
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

    crate::define_closed_string_enum! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
        enum TestClosedKind {
            Alpha => "alpha",
            BetaValue => "beta_value",
        }
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
        let expr = OriginExportKey::new(OriginExportKind::HirExpr, "body:a", "0");
        let stmt = OriginExportKey::new(OriginExportKind::HirStmt, "body:a", "0");
        let other_body_expr = OriginExportKey::new(OriginExportKind::HirExpr, "body:b", "0");

        assert_ne!(expr, stmt);
        assert_ne!(expr, other_body_expr);
        assert_eq!(expr.kind(), OriginExportKind::HirExpr);
        assert_eq!(expr.owner_key(), "body:a");
        assert_eq!(expr.local_key(), "0");
        assert_eq!(OriginExportKind::HirExpr.as_str(), "hir.expr");
    }

    #[test]
    fn export_key_formats_canonical_storage_key_and_display_label() {
        let key = OriginExportKey::new(
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
            OriginExportKey::try_new(OriginExportKind::Semantic, "", "expr:0"),
            Err(OriginExportKeyError::EmptyOwnerKey)
        );
        assert_eq!(
            OriginExportKey::try_new(OriginExportKind::Semantic, "semantic:test", ""),
            Err(OriginExportKeyError::EmptyLocalKey)
        );
    }

    #[test]
    fn export_key_rejects_reserved_storage_separator() {
        assert_eq!(
            OriginExportKey::try_new(OriginExportKind::Semantic, "semantic\u{1f}test", "expr:0"),
            Err(OriginExportKeyError::ReservedStorageSeparator { field: "owner_key" })
        );
        assert_eq!(
            OriginExportKey::try_new(OriginExportKind::Semantic, "semantic:test", "expr\u{1f}0"),
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
        assert_eq!(key, TestStringKey::new("runtime:test"));
        assert_ne!(key, TestStringKey::new("semantic:test"));
    }

    #[test]
    #[should_panic(expected = "origin string key must not be empty")]
    fn origin_string_key_macro_rejects_empty_keys() {
        TestStringKey::new("");
    }

    #[test]
    #[should_panic(
        expected = "origin string key must not contain reserved origin storage separator"
    )]
    fn origin_string_key_macro_rejects_reserved_storage_separators() {
        TestStringKey::new("runtime\u{1f}test");
    }

    #[test]
    fn origin_owner_key_macro_defines_export_owner_wrappers() {
        fn accepts_owner_key(key: &impl super::OriginExportOwnerKey) -> &str {
            key.as_str()
        }

        let key = TestOwnerKey::new("runtime:test");

        assert_eq!(accepts_owner_key(&key), "runtime:test");
        assert_eq!(key, TestOwnerKey::new("runtime:test"));
        assert_ne!(key, TestOwnerKey::new("semantic:test"));
    }

    #[test]
    #[should_panic(expected = "origin owner key must not be empty")]
    fn origin_owner_key_macro_rejects_empty_keys() {
        TestOwnerKey::new("");
    }

    #[test]
    #[should_panic(
        expected = "origin owner key must not contain reserved origin storage separator"
    )]
    fn origin_owner_key_macro_rejects_reserved_storage_separators() {
        TestOwnerKey::new("runtime\u{1f}test");
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
        assert_eq!(origin.key().into_parts(), (TestOwner(4), TestLocal(9)));
    }

    #[test]
    fn shared_origin_identity_types_derive_salsa_update() {
        fn assert_update<T: salsa::Update>() {}

        assert_update::<OriginKey<TestOwner, TestLocal>>();
        assert_update::<OriginLink<OriginKey<TestOwner, TestLocal>>>();
        assert_update::<OriginGraph<OriginKey<TestOwner, TestLocal>>>();
        assert_update::<TestStringKey>();
        assert_update::<TestOwnerKey>();
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
}
