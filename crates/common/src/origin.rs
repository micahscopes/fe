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

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
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
    pub fn new<Owner, Local>(kind: OriginExportKind, owner_key: &Owner, local_key: &Local) -> Self
    where
        Owner: OriginExportOwnerKey + ?Sized,
        Local: OriginExportLocalKey + ?Sized,
    {
        Self::try_new(kind, owner_key, local_key)
            .unwrap_or_else(|err| panic!("invalid origin export key: {err}"))
    }

    pub fn try_new<Owner, Local>(
        kind: OriginExportKind,
        owner_key: &Owner,
        local_key: &Local,
    ) -> Result<Self, OriginExportKeyError>
    where
        Owner: OriginExportOwnerKey + ?Sized,
        Local: OriginExportLocalKey + ?Sized,
    {
        Self::try_from_raw_parts(kind, owner_key.as_str(), local_key.to_export_local_key())
    }

    /// Build an export key from decoded or imported wire fields.
    ///
    /// Prefer [`OriginExportKey::new`] at compiler construction sites so owner
    /// and local-key namespaces stay nominal. This raw path exists for serde and
    /// relation-table import boundaries where the stable strings are already the
    /// data being validated.
    pub fn try_from_raw_parts(
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
        Self::try_from_raw_parts(raw.kind, raw.owner_key, raw.local_key).map_err(de::Error::custom)
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

pub trait OriginExportLocalKey {
    fn to_export_local_key(&self) -> String;
}

#[doc(hidden)]
pub fn assert_origin_key_text(kind: &'static str, value: &str) {
    validate_origin_key_text(kind, value).unwrap_or_else(|err| panic!("{err}"));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginKeyTextError {
    Empty { kind: &'static str },
    ReservedStorageSeparator { kind: &'static str },
}

impl fmt::Display for OriginKeyTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { kind } => write!(f, "{kind} must not be empty"),
            Self::ReservedStorageSeparator { kind } => write!(
                f,
                "{kind} must not contain reserved origin storage separator"
            ),
        }
    }
}

impl std::error::Error for OriginKeyTextError {}

#[doc(hidden)]
pub fn validate_origin_key_text(kind: &'static str, value: &str) -> Result<(), OriginKeyTextError> {
    if value.is_empty() {
        return Err(OriginKeyTextError::Empty { kind });
    }
    if value.contains(ORIGIN_EXPORT_KEY_STORAGE_SEPARATOR) {
        return Err(OriginKeyTextError::ReservedStorageSeparator { kind });
    }
    Ok(())
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
                Self::try_new(key).unwrap_or_else(|err| panic!("{err}"))
            }

            pub fn try_new(
                key: impl Into<::std::string::String>
            ) -> ::std::result::Result<Self, $crate::origin::OriginKeyTextError> {
                let key = key.into();
                $crate::origin::validate_origin_key_text("origin owner key", &key)?;
                Ok(Self(key))
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
macro_rules! define_origin_local_key {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, $crate::salsa::Update)]
        $vis struct $name(::std::string::String);

        impl $name {
            pub fn new(key: impl Into<::std::string::String>) -> Self {
                Self::try_new(key).unwrap_or_else(|err| panic!("{err}"))
            }

            pub fn try_new(
                key: impl Into<::std::string::String>
            ) -> ::std::result::Result<Self, $crate::origin::OriginKeyTextError> {
                let key = key.into();
                $crate::origin::validate_origin_key_text("origin local key", &key)?;
                Ok(Self(key))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl $crate::origin::OriginExportLocalKey for $name {
            fn to_export_local_key(&self) -> ::std::string::String {
                self.as_str().to_string()
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
                Self::try_new(key).unwrap_or_else(|err| panic!("{err}"))
            }

            pub fn try_new(
                key: impl Into<::std::string::String>
            ) -> ::std::result::Result<Self, $crate::origin::OriginKeyTextError> {
                let key = key.into();
                $crate::origin::validate_origin_key_text("origin string key", &key)?;
                Ok(Self(key))
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
mod tests;
