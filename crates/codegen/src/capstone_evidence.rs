//! Deterministic, machine-readable evidence for multi-backend capstones.
//!
//! This is deliberately an evidence manifest, not a compiler interface. It
//! describes artifacts emitted through normal backend APIs and names the
//! independent verification which earned each claim.

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CAPSTONE_EVIDENCE_PROTOCOL: &str = "fe-capstone-evidence";
pub const CAPSTONE_EVIDENCE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CapstoneEvidenceManifest {
    pub protocol: &'static str,
    pub version: u32,
    pub capstone: &'static str,
    pub source: SourceEvidence,
    pub interface: InterfaceSnapshot,
    pub targets: Vec<TargetEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceEvidence {
    pub path: &'static str,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InterfaceSnapshot {
    pub version: u32,
    pub export: &'static str,
    pub parameters: Vec<&'static str>,
    pub result: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TargetEvidence {
    pub target: &'static str,
    pub runtime: &'static str,
    pub imports: Vec<&'static str>,
    pub exports: Vec<&'static str>,
    pub artifact: Option<ArtifactEvidence>,
    pub verification: VerificationEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArtifactEvidence {
    pub kind: &'static str,
    pub path: &'static str,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerificationEvidence {
    pub status: VerificationStatus,
    pub scope: &'static str,
    pub command: &'static str,
    pub test: &'static str,
    pub result: Option<String>,
    pub note: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    Validated,
    NotRun,
}

impl ArtifactEvidence {
    pub fn from_bytes(kind: &'static str, path: &'static str, bytes: &[u8]) -> Self {
        Self {
            kind,
            path,
            bytes: bytes.len(),
            sha256: sha256_hex(bytes),
        }
    }
}

impl CapstoneEvidenceManifest {
    /// Serialize with stable field and target ordering and no ambient metadata.
    pub fn to_pretty_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("capstone evidence should serialize")
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol != CAPSTONE_EVIDENCE_PROTOCOL || self.version != CAPSTONE_EVIDENCE_VERSION
        {
            return Err("unsupported capstone evidence protocol");
        }
        if !is_lowercase_sha256(&self.source.sha256) {
            return Err("source SHA-256 must contain 64 lowercase hexadecimal characters");
        }
        let target_names: Vec<_> = self.targets.iter().map(|item| item.target).collect();
        if target_names != ["evm", "native", "wasm", "webgpu"] {
            return Err("targets must be unique and sorted as evm, native, wasm, webgpu");
        }
        for item in &self.targets {
            if let Some(artifact) = &item.artifact {
                if !is_lowercase_sha256(&artifact.sha256) {
                    return Err(
                        "artifact SHA-256 must contain 64 lowercase hexadecimal characters",
                    );
                }
                if artifact.bytes == 0 {
                    return Err("an artifact must contain at least one byte");
                }
            }
            match (item.verification.status, &item.verification.result) {
                (VerificationStatus::Verified | VerificationStatus::Validated, None) => {
                    return Err("verified or validated evidence must record a result");
                }
                (VerificationStatus::NotRun, Some(_)) => {
                    return Err("not-run evidence cannot record a result");
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> CapstoneEvidenceManifest {
        CapstoneEvidenceManifest {
            protocol: CAPSTONE_EVIDENCE_PROTOCOL,
            version: CAPSTONE_EVIDENCE_VERSION,
            capstone: "fixture",
            source: SourceEvidence {
                path: "kernel.fe",
                sha256: sha256_hex(b"one source"),
            },
            interface: InterfaceSnapshot {
                version: 1,
                export: "kernel",
                parameters: vec!["i32"],
                result: "u32",
            },
            targets: ["evm", "native", "wasm", "webgpu"]
                .into_iter()
                .map(|target| TargetEvidence {
                    target,
                    runtime: "fixture",
                    imports: vec![],
                    exports: vec!["kernel"],
                    artifact: Some(ArtifactEvidence::from_bytes(
                        "fixture",
                        "kernel.bin",
                        b"artifact",
                    )),
                    verification: VerificationEvidence {
                        status: VerificationStatus::Validated,
                        scope: "fixture",
                        command: "cargo test",
                        test: "fixture",
                        result: Some("fixture validated".to_string()),
                        note: None,
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn serialization_is_byte_for_byte_deterministic() {
        let manifest = fixture();
        assert_eq!(manifest.to_pretty_json(), manifest.to_pretty_json());
        assert!(!manifest.to_pretty_json().contains("timestamp"));
        assert_eq!(
            manifest.source.sha256,
            sha256_hex(b"one source"),
            "the source is hashed once rather than copied into the evidence"
        );
    }

    #[test]
    fn target_order_is_part_of_the_protocol() {
        let mut manifest = fixture();
        manifest.targets.swap(0, 1);
        assert_eq!(
            manifest.validate(),
            Err("targets must be unique and sorted as evm, native, wasm, webgpu")
        );
    }

    #[test]
    fn mandelbrot_source_is_the_canonical_file_not_a_copied_kernel() {
        let source = include_bytes!("../../../demos/capstones/mandelbrot/kernel.fe");
        assert_eq!(
            sha256_hex(source),
            "dd9edf593b8477f2afeea3c2e4e51669d67a1a1e8f37782f2c43e1b124f8d871"
        );
    }

    #[test]
    fn rejects_non_lowercase_or_non_hex_digests() {
        let mut manifest = fixture();
        manifest.source.sha256 = "G".repeat(64);
        assert_eq!(
            manifest.validate(),
            Err("source SHA-256 must contain 64 lowercase hexadecimal characters")
        );

        let mut manifest = fixture();
        manifest.targets[0].artifact.as_mut().unwrap().sha256 = "A".repeat(64);
        assert_eq!(
            manifest.validate(),
            Err("artifact SHA-256 must contain 64 lowercase hexadecimal characters")
        );
    }

    #[test]
    fn verification_status_and_result_cannot_contradict_each_other() {
        let mut manifest = fixture();
        manifest.targets[0].verification.result = None;
        assert_eq!(
            manifest.validate(),
            Err("verified or validated evidence must record a result")
        );

        let mut manifest = fixture();
        manifest.targets[0].verification.status = VerificationStatus::NotRun;
        assert_eq!(
            manifest.validate(),
            Err("not-run evidence cannot record a result")
        );
    }

    #[test]
    fn verified_ephemeral_runtime_does_not_require_an_artifact() {
        let mut manifest = fixture();
        manifest.targets[0].artifact = None;
        manifest.targets[0].verification.status = VerificationStatus::Verified;
        assert_eq!(manifest.validate(), Ok(()));
    }
}
