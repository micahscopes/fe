use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

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
