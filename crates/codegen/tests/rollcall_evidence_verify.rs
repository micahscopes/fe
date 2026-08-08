//! Rung 4 receipts verifier: re-reads `demos/rollcall/evidence.json` and the
//! artifact paths it names, and confirms the ledger has not drifted from
//! what is actually on disk.
//!
//! Deliberately reads the ledger as untyped JSON (`serde_json::Value`)
//! rather than adding `Deserialize` to `capstone_evidence::CapstoneEvidenceManifest`:
//! that schema's fields are `&'static str` by design (every existing writer,
//! including `gen_mandelbrot_demo.rs`, constructs it from Rust literals), so
//! widening it to support deserialization would be a real, rippling schema
//! change well outside this rung's "wire, don't rebuild" scope. Reading the
//! JSON generically is sufficient for tamper-evidence.
//!
//! Three checks, matching the rung brief exactly ("a verifier re-reads the
//! paths and confirms the roots agree across legs + are
//! source-digest-identical"):
//!   1. tamper-evidence: re-hash `source.sha256` and the wasm artifact's
//!      `sha256` against the files actually on disk.
//!   2. cross-leg agreement: the wasm leg's plain root-hex result must
//!      appear inside the EVM leg's result string (both legs record the SAME
//!      root, computed by SEPARATE code paths).
//!   3. honesty: any `not_run` leg must carry a non-empty explanatory note
//!      and no fabricated result; any `verified`/`validated` leg must carry
//!      a result.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)")
        .to_path_buf()
}

fn sha256_hex_of_file(path: &Path) -> String {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|e| panic!("could not read {} for tamper-evidence: {e}", path.display()));
    format!("{:x}", Sha256::digest(&bytes))
}

fn load_evidence() -> serde_json::Value {
    let path = repo_root().join("demos/rollcall/evidence.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "could not read {} (run `cargo run -p fe-codegen --features native-backend \
             --example gen_rollcall_evidence` first): {e}",
            path.display()
        )
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

fn target<'a>(evidence: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    evidence["targets"]
        .as_array()
        .expect("evidence.targets should be an array")
        .iter()
        .find(|t| t["target"] == name)
        .unwrap_or_else(|| panic!("evidence.json has no `{name}` target"))
}

fn as_str<'a>(value: &'a serde_json::Value, path: &str) -> &'a str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("expected a JSON string at `{path}`, got {value:?}"))
}

#[test]
fn rollcall_evidence_protocol_and_target_order_are_correct() {
    let evidence = load_evidence();
    assert_eq!(
        evidence["protocol"],
        fe_codegen::capstone_evidence::CAPSTONE_EVIDENCE_PROTOCOL
    );
    assert_eq!(
        evidence["version"],
        fe_codegen::capstone_evidence::CAPSTONE_EVIDENCE_VERSION
    );
    let names: Vec<&str> = evidence["targets"]
        .as_array()
        .expect("targets should be an array")
        .iter()
        .map(|t| as_str(&t["target"], "targets[].target"))
        .collect();
    assert_eq!(
        names,
        ["evm", "native", "wasm", "webgpu"],
        "the ledger's leg order is part of the protocol"
    );
}

/// Tamper-evidence: the kernel source and the wasm artifact's recorded
/// SHA-256s must match what is actually on disk right now. A tampered or
/// stale file changes its digest and this test catches it.
#[test]
fn rollcall_evidence_source_and_wasm_artifact_are_source_digest_identical() {
    let evidence = load_evidence();
    let root = repo_root();

    let source_path = root.join(as_str(&evidence["source"]["path"], "source.path"));
    let recorded_source_sha256 = as_str(&evidence["source"]["sha256"], "source.sha256");
    assert_eq!(
        sha256_hex_of_file(&source_path),
        recorded_source_sha256,
        "demos/rollcall/gen/kernel.fe on disk must match the SHA-256 recorded in evidence.json \
         (tamper-evidence)"
    );

    let wasm_target = target(&evidence, "wasm");
    let artifact = &wasm_target["artifact"];
    assert!(
        !artifact.is_null(),
        "the wasm leg must carry a real artifact record"
    );
    let wasm_path = root.join(as_str(&artifact["path"], "targets[wasm].artifact.path"));
    let recorded_wasm_sha256 = as_str(&artifact["sha256"], "targets[wasm].artifact.sha256");
    assert_eq!(
        sha256_hex_of_file(&wasm_path),
        recorded_wasm_sha256,
        "demos/rollcall/gen/kernel.wasm on disk must match the SHA-256 recorded in \
         evidence.json (tamper-evidence)"
    );
}

/// Cross-leg agreement: the wasm leg's plain root-hex result and the EVM
/// leg's result string (which embeds the same root hex, computed by a
/// SEPARATE code path: an on-chain hash2 probe, not the wasm builder) must
/// name the SAME root.
#[test]
fn rollcall_evidence_wasm_and_evm_roots_agree() {
    let evidence = load_evidence();

    let wasm_result = as_str(
        &target(&evidence, "wasm")["verification"]["result"],
        "targets[wasm].verification.result",
    );
    assert!(
        wasm_result.starts_with("0x") && wasm_result.len() > 2,
        "the wasm leg's result should be a plain 0x-prefixed root hex, got {wasm_result}"
    );

    let evm_result = as_str(
        &target(&evidence, "evm")["verification"]["result"],
        "targets[evm].verification.result",
    );
    assert!(
        evm_result.contains(wasm_result),
        "the EVM leg's result ({evm_result}) must embed the SAME root the wasm leg computed \
         ({wasm_result}) -- these are two independent code paths (an off-chain wasm builder vs. \
         an on-chain hash2 probe) that must agree on the value, not just both succeed"
    );
}

/// Honesty: a `not_run` leg must explain itself and record no fabricated
/// result; a `verified`/`validated` leg must record a result (and, if it
/// names a root, that root must agree with the wasm leg's).
#[test]
fn rollcall_evidence_native_and_webgpu_legs_are_honestly_reported() {
    let evidence = load_evidence();
    let wasm_result = as_str(
        &target(&evidence, "wasm")["verification"]["result"],
        "targets[wasm].verification.result",
    )
    .to_string();

    for leg in ["native", "webgpu"] {
        let entry = target(&evidence, leg);
        let verification = &entry["verification"];
        let status = as_str(&verification["status"], "verification.status");
        match status {
            "not_run" => {
                assert!(
                    verification["result"].is_null(),
                    "`{leg}` is not_run but records a result: {:?}",
                    verification["result"]
                );
                let note = as_str(&verification["note"], "verification.note");
                assert!(
                    !note.trim().is_empty(),
                    "`{leg}` is not_run and must explain why in `note`"
                );
            }
            "verified" | "validated" => {
                let result = as_str(&verification["result"], "verification.result");
                assert!(
                    !result.trim().is_empty(),
                    "`{leg}` claims {status} but has an empty result"
                );
                if result.contains("0x") {
                    assert!(
                        result.contains(&wasm_result),
                        "`{leg}` result ({result}) claims a root but it does not match the \
                         wasm leg's root ({wasm_result})"
                    );
                }
            }
            other => panic!("`{leg}` has an unrecognized verification status: {other}"),
        }
    }
}
