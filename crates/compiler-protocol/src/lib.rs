//! Versioned data contract for invoking an Fe compiler facade.
//!
//! This crate intentionally has no compiler, filesystem, terminal, network, or
//! web-platform dependencies. Browser Workers and native build tools use the
//! same requests, structured diagnostics, and content-addressed artifacts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROTOCOL_NAME: &str = "fe-compiler";
pub const PROTOCOL_MAJOR: u16 = 1;
pub const PROTOCOL_MINOR: u16 = 1;
pub const SOURCE_DEPENDENCY_INVENTORY_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const CURRENT: Self = Self {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
    };

    pub fn ensure_compatible(self) -> Result<(), ProtocolError> {
        if self.major != PROTOCOL_MAJOR {
            return Err(ProtocolError::IncompatibleMajor {
                expected: PROTOCOL_MAJOR,
                received: self.major,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileRequest {
    pub protocol: ProtocolVersion,
    pub root: String,
    pub sources: Vec<VirtualSource>,
    pub target: CompileTarget,
    #[serde(default)]
    pub entries: Vec<String>,
    #[serde(default)]
    pub options: CompileOptions,
}

impl CompileRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.protocol.ensure_compatible()?;
        if self.sources.is_empty() {
            return Err(ProtocolError::NoSources);
        }
        let mut previous = None;
        let mut root_found = false;
        for source in &self.sources {
            source.validate()?;
            if previous.is_some_and(|value: &str| value >= source.url.as_str()) {
                return Err(ProtocolError::SourcesNotStrictlySorted);
            }
            root_found |= source.url == self.root;
            previous = Some(&source.url);
        }
        if !root_found {
            return Err(ProtocolError::RootNotFound(self.root.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VirtualSource {
    /// Stable absolute virtual URL, for example `fe-memory:///app/src/lib.fe`.
    pub url: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl VirtualSource {
    pub fn new(url: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            url: url.into(),
            sha256: Some(sha256_hex(text.as_bytes())),
            text,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !self.url.contains("://") {
            return Err(ProtocolError::InvalidVirtualUrl(self.url.clone()));
        }
        if let Some(expected) = &self.sha256 {
            let actual = sha256_hex(self.text.as_bytes());
            if *expected != actual {
                return Err(ProtocolError::SourceDigestMismatch {
                    url: self.url.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDependencyInventory {
    pub version: u16,
    pub root: String,
    /// Structurally participating supplied sources, sorted by URL.
    pub sources: Vec<SourceDependency>,
}

impl SourceDependencyInventory {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != SOURCE_DEPENDENCY_INVENTORY_VERSION {
            return Err(ProtocolError::UnsupportedSourceDependencyInventoryVersion(
                self.version,
            ));
        }
        ensure_sorted_unique(
            self.sources.iter().map(|source| source.url.clone()),
            "source dependencies",
        )?;
        for source in &self.sources {
            source.validate()?;
        }
        if !self.sources.iter().any(|source| source.url == self.root) {
            return Err(ProtocolError::SourceDependencyRootMissing(
                self.root.clone(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDependency {
    pub url: String,
    pub sha256: String,
}

impl SourceDependency {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !self.url.contains("://") {
            return Err(ProtocolError::InvalidVirtualUrl(self.url.clone()));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ProtocolError::InvalidSourceDependencyDigest {
                url: self.url.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileTarget {
    Evm,
    Wasm,
    Webgpu,
    Native,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileOptions {
    #[serde(default)]
    pub optimization: OptimizationLevel,
    #[serde(default)]
    pub debug_info: bool,
    /// Stable extension point. Keys are namespaced and sorted by construction.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationLevel {
    #[default]
    None,
    Size,
    Speed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileResult {
    pub protocol: ProtocolVersion,
    pub compiler: CompilerIdentity,
    pub target: CompileTarget,
    pub source_set_sha256: String,
    /// Compiler-database-proven source closure. `None` remains accepted for
    /// older protocol-v1 producers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_dependencies: Option<SourceDependencyInventory>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    pub interface: InterfaceManifest,
}

impl CompileResult {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.protocol.ensure_compatible()?;
        let mut previous = None;
        for artifact in &self.artifacts {
            artifact.validate()?;
            if previous.is_some_and(|value: &str| value >= artifact.name.as_str()) {
                return Err(ProtocolError::ArtifactsNotStrictlySorted);
            }
            previous = Some(&artifact.name);
        }
        if let Some(dependencies) = &self.source_dependencies {
            dependencies.validate()?;
        }
        self.interface.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerIdentity {
    pub name: String,
    pub version: String,
    pub build: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub name: String,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// Content-addressed artifact after publication. Unlike [`Artifact`], this is
/// safe to embed in a JSON manifest because payload bytes live at `url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedArtifact {
    pub name: String,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub sha256: String,
    pub byte_len: u64,
    pub url: String,
}

impl PublishedArtifact {
    pub fn from_artifact(artifact: &Artifact, url: impl Into<String>) -> Self {
        Self {
            name: artifact.name.clone(),
            kind: artifact.kind,
            media_type: artifact.media_type.clone(),
            sha256: artifact.sha256.clone(),
            byte_len: artifact.bytes.len() as u64,
            url: url.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedModuleManifest {
    pub protocol: ProtocolVersion,
    pub compiler: CompilerIdentity,
    pub target: CompileTarget,
    pub source_set_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_dependencies: Option<SourceDependencyInventory>,
    pub entry: String,
    pub interface: InterfaceManifest,
    pub artifacts: Vec<PublishedArtifact>,
}

impl PublishedModuleManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.protocol.ensure_compatible()?;
        if let Some(dependencies) = &self.source_dependencies {
            dependencies.validate()?;
        }
        ensure_sorted_unique(
            self.artifacts.iter().map(|artifact| artifact.name.clone()),
            "published artifacts",
        )?;
        self.interface.validate()
    }
}

impl Artifact {
    pub fn new(
        name: impl Into<String>,
        kind: ArtifactKind,
        media_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            media_type: media_type.into(),
            sha256: sha256_hex(&bytes),
            bytes,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        let actual = sha256_hex(&self.bytes);
        if actual != self.sha256 {
            return Err(ProtocolError::ArtifactDigestMismatch {
                name: self.name.clone(),
                expected: self.sha256.clone(),
                actual,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    WasmModule,
    EvmBytecode,
    WgslModule,
    NativeObject,
    InterfaceManifest,
    SourceMap,
    HostAdapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<DiagnosticLabel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticLabel {
    pub source_url: String,
    pub start: u32,
    pub end: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceManifest {
    /// Target-neutral semantic interface, before core-Wasm/JS/native lowering.
    ///
    /// The physical imports and exports below remain an artifact inventory:
    /// they may be incomplete when recovered from a finished binary. Generated
    /// bindings and rich adapters consume this semantic world instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_world: Option<fe_host_abi::World>,
    #[serde(default)]
    pub imports: Vec<InterfaceFunction>,
    #[serde(default)]
    pub exports: Vec<InterfaceFunction>,
    #[serde(default)]
    pub resources: Vec<ResourceType>,
}

impl InterfaceManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(world) = &self.host_world {
            world
                .validate()
                .map_err(|error| ProtocolError::InvalidHostInterface(error.to_string()))?;
        }
        ensure_sorted_unique(
            self.imports.iter().map(InterfaceFunction::sort_key),
            "interface imports",
        )?;
        ensure_sorted_unique(
            self.exports.iter().map(InterfaceFunction::sort_key),
            "interface exports",
        )?;
        ensure_sorted_unique(
            self.resources.iter().map(|resource| resource.name.clone()),
            "interface resources",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceFunction {
    pub module: String,
    pub name: String,
    /// False when an artifact inventory knows the symbol but has not decoded
    /// its target ABI signature. Empty params/results are only authoritative
    /// when this is true.
    #[serde(default)]
    pub signature_complete: bool,
    #[serde(default)]
    pub params: Vec<InterfaceType>,
    #[serde(default)]
    pub results: Vec<InterfaceType>,
}

impl InterfaceFunction {
    fn sort_key(&self) -> String {
        format!("{}\0{}", self.module, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceType {
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Resource(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceType {
    pub name: String,
    pub ownership: ResourceOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwnership {
    Owned,
    Borrowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    IncompatibleMajor {
        expected: u16,
        received: u16,
    },
    NoSources,
    InvalidVirtualUrl(String),
    RootNotFound(String),
    SourcesNotStrictlySorted,
    SourceDigestMismatch {
        url: String,
        expected: String,
        actual: String,
    },
    ArtifactsNotStrictlySorted,
    ArtifactDigestMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    NotStrictlySorted(&'static str),
    InvalidHostInterface(String),
    UnsupportedSourceDependencyInventoryVersion(u16),
    SourceDependencyRootMissing(String),
    InvalidSourceDependencyDigest {
        url: String,
    },
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProtocolError {}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn source_set_sha256(sources: &[VirtualSource]) -> String {
    let mut hash = Sha256::new();
    for source in sources {
        hash.update((source.url.len() as u64).to_le_bytes());
        hash.update(source.url.as_bytes());
        hash.update((source.text.len() as u64).to_le_bytes());
        hash.update(source.text.as_bytes());
    }
    hex::encode(hash.finalize())
}

fn ensure_sorted_unique(
    values: impl IntoIterator<Item = String>,
    context: &'static str,
) -> Result<(), ProtocolError> {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|previous| previous >= &value) {
            return Err(ProtocolError::NotStrictlySorted(context));
        }
        previous = Some(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CompileRequest {
        CompileRequest {
            protocol: ProtocolVersion::CURRENT,
            root: "fe-memory:///app/src/lib.fe".to_owned(),
            sources: vec![VirtualSource::new(
                "fe-memory:///app/src/lib.fe",
                "pub fn main() -> u32 { 42 }",
            )],
            target: CompileTarget::Wasm,
            entries: vec!["main".to_owned()],
            options: CompileOptions::default(),
        }
    }

    #[test]
    fn request_json_is_deterministic_and_round_trips() {
        let request = request();
        request.validate().unwrap();
        let first = serde_json::to_string_pretty(&request).unwrap();
        assert_eq!(
            first,
            include_str!("../tests/fixtures/compile-request-v1.json").trim_end()
        );
        let decoded: CompileRequest = serde_json::from_str(&first).unwrap();
        let second = serde_json::to_string_pretty(&decoded).unwrap();
        assert_eq!(first, second);
        assert_eq!(decoded, request);
        assert!(first.contains("\"target\": \"wasm\""));
    }

    #[test]
    fn incompatible_major_is_rejected() {
        let mut request = request();
        request.protocol.major += 1;
        assert!(matches!(
            request.validate(),
            Err(ProtocolError::IncompatibleMajor { .. })
        ));
    }

    #[test]
    fn tampered_source_and_artifact_are_rejected() {
        let mut request = request();
        request.sources[0].text.push_str("\n// tampered");
        assert!(matches!(
            request.validate(),
            Err(ProtocolError::SourceDigestMismatch { .. })
        ));

        let mut artifact = Artifact::new(
            "module.wasm",
            ArtifactKind::WasmModule,
            "application/wasm",
            vec![0, 97, 115, 109],
        );
        artifact.bytes.push(1);
        assert!(matches!(
            artifact.validate(),
            Err(ProtocolError::ArtifactDigestMismatch { .. })
        ));
    }

    #[test]
    fn ordering_is_part_of_the_canonical_contract() {
        let mut request = request();
        request.sources.push(VirtualSource::new(
            "fe-memory:///app/src/a.fe",
            "pub fn a() {}",
        ));
        assert_eq!(
            request.validate(),
            Err(ProtocolError::SourcesNotStrictlySorted)
        );

        let manifest = InterfaceManifest {
            imports: vec![
                InterfaceFunction {
                    module: "z".to_owned(),
                    name: "last".to_owned(),
                    signature_complete: true,
                    params: vec![],
                    results: vec![],
                },
                InterfaceFunction {
                    module: "a".to_owned(),
                    name: "first".to_owned(),
                    signature_complete: true,
                    params: vec![],
                    results: vec![],
                },
            ],
            ..InterfaceManifest::default()
        };
        assert_eq!(
            manifest.validate(),
            Err(ProtocolError::NotStrictlySorted("interface imports"))
        );
    }

    #[test]
    fn semantic_host_world_is_validated_and_round_trips() {
        let manifest = InterfaceManifest {
            host_world: Some(fe_host_abi::World {
                name: "example-host".to_owned(),
                ..fe_host_abi::World::default()
            }),
            ..InterfaceManifest::default()
        };
        manifest.validate().unwrap();
        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(
            serde_json::from_str::<InterfaceManifest>(&json).unwrap(),
            manifest
        );

        let invalid = InterfaceManifest {
            host_world: Some(fe_host_abi::World::default()),
            ..InterfaceManifest::default()
        };
        assert!(matches!(
            invalid.validate(),
            Err(ProtocolError::InvalidHostInterface(_))
        ));
    }

    #[test]
    fn source_set_hash_frames_urls_and_contents() {
        let sources = request().sources;
        assert_eq!(source_set_sha256(&sources), source_set_sha256(&sources));
        assert_ne!(
            source_set_sha256(&sources),
            source_set_sha256(&[VirtualSource::new(
                "fe-memory:///other/src/lib.fe",
                &sources[0].text,
            )])
        );
    }

    #[test]
    fn source_dependency_inventory_is_versioned_sorted_and_rooted() {
        let root = &request().sources[0];
        let inventory = SourceDependencyInventory {
            version: SOURCE_DEPENDENCY_INVENTORY_VERSION,
            root: root.url.clone(),
            sources: vec![SourceDependency {
                url: root.url.clone(),
                sha256: root.sha256.clone().unwrap(),
            }],
        };
        inventory.validate().unwrap();
        let json = serde_json::to_string(&inventory).unwrap();
        assert_eq!(
            serde_json::from_str::<SourceDependencyInventory>(&json).unwrap(),
            inventory
        );

        let mut invalid = inventory.clone();
        invalid.root = "fe-memory:///missing.fe".to_owned();
        assert!(matches!(
            invalid.validate(),
            Err(ProtocolError::SourceDependencyRootMissing(_))
        ));

        let mut noncanonical = inventory;
        noncanonical.sources[0].sha256.make_ascii_uppercase();
        assert!(matches!(
            noncanonical.validate(),
            Err(ProtocolError::InvalidSourceDependencyDigest { .. })
        ));
    }

    #[test]
    fn unknown_fields_fail_closed() {
        let mut value = serde_json::to_value(request()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("browser_magic".to_owned(), serde_json::json!(true));
        assert!(serde_json::from_value::<CompileRequest>(value).is_err());
    }
}
