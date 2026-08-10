//! Rung 4 evidence generator: the Rollcall four-leg capstone ledger.
//!
//! Compiles the SAME authored Poseidon-Merkle root kernel
//! (`poseidon_merkle_root_loop.fe`, N=4/depth-2, the exact fixture
//! `rollcall_e2e.rs` proves) across wasm, native/Cranelift, EVM/revm, and
//! GPU/SPIR-V, and writes:
//!
//!   - `demos/rollcall/gen/{kernel.fe,kernel.wasm,kernel.manifest.json,
//!     reference.json}`: the browser app's real, executed artifacts (wasm
//!     leg only -- the leg the browser app actually loads).
//!   - `demos/rollcall/evidence.json`: the `fe-capstone-evidence` v1 ledger
//!     (the SAME schema `crates/codegen/src/capstone_evidence.rs` and
//!     `gen_mandelbrot_demo.rs` already established; reused, not
//!     reinvented).
//!
//! HONESTY, not optimism: every leg's evidence entry states what actually
//! happened when this generator ran, never an assumed outcome. As of this
//! rung, on the pinned Sonatina rev, wasm and EVM genuinely EXECUTE the real
//! kernel and cross-check equal; native/Cranelift and GPU/SPIR-V both
//! currently fail closed with the SAME root cause (function-local
//! `[u32; N]` arrays lower to `MemAllocDynamic`, which only the wasm
//! backend's canonical arena currently supports on this pin) -- see
//! `RUNG4_ASSEMBLY_PLAN.md`. If a future run of this generator finds either
//! leg now succeeds (the fork re-pin, Decision 5), it records that instead;
//! nothing here is hand-typed to force a particular verdict.
//!
//! Run: `cargo run -p fe-codegen --features native-backend --example
//! gen_rollcall_evidence`

use std::path::PathBuf;

use common::InputDb;
use driver::DriverDataBase;
use ethers_core::abi::Token;
use ethers_core::types::U256 as AbiU256;
use fe_codegen::capstone_evidence::{
    ArtifactEvidence, CAPSTONE_EVIDENCE_PROTOCOL, CAPSTONE_EVIDENCE_VERSION,
    CapstoneEvidenceManifest, InterfaceSnapshot, SourceEvidence, TargetEvidence,
    VerificationEvidence, VerificationStatus, sha256_hex,
};
use fe_codegen::{BackendKind, OptLevel, layout_for};
use fe_contract_harness::{Address, ExecutionOptions, FeContractHarness, U256, bytes_to_u256};
use num_bigint::BigUint;
use url::Url;

/// The SSOT kernel: the exact fixture `rollcall_e2e.rs` `include_str!`s, so
/// the tested source and the shipped browser artifact are byte-identical by
/// construction.
const MERKLE4_SRC: &str = include_str!("../tests/fixtures/spirv/poseidon_merkle_root_loop.fe");
/// `const_poseidon.fe`'s pinned `hash2`, reused VERBATIM for the EVM leg
/// (same file `rollcall_e2e.rs` and the RollcallRegistry contract test use).
const CONST_POSEIDON_FULL_SOURCE: &str =
    include_str!("../../fe/tests/fixtures/fe_test/const_poseidon.fe");

const KERNEL_NAME: &str = "poseidon_merkle_root_loop";
const MERKLE_LIMB_BITS: usize = 13;
const MERKLE_N_LIMBS: usize = 20;
const DEPTH: usize = 2; // N = 4 leaves, matches rollcall_e2e.rs's end-to-end gate.

/// The canonical member list: the SAME 4 leaves `rollcall_e2e.rs`'s
/// wasm+EVM end-to-end gate uses, so this generator's evidence and that
/// test's proof are about the identical scenario.
const CANONICAL_LEAVES: [u64; 4] = [111, 222, 333, 444];

fn const_poseidon_source() -> &'static str {
    CONST_POSEIDON_FULL_SOURCE
        .split_once("\n#[test]\n")
        .expect("const_poseidon.fe should still contain its pinned #[test] marker")
        .0
}

/// The RollcallRegistry + Hash2Exec wrapper, verbatim in spirit with
/// `rollcall_e2e.rs::rollcall_wrapper` (kept independent per this
/// repository's test/example duplication convention -- see
/// `gen_mandelbrot_demo.rs`'s own `mandel_oracle_q12`).
fn rollcall_wrapper(depth: usize) -> String {
    format!(
        r#"
use std::evm::Ctx
use std::abi::sol

const ROLLCALL_DEPTH: usize = {depth}

msg RollcallMsg {{
    #[selector = sol("commit(uint256)")]
    Commit {{ root: u256 }},
    #[selector = sol("getRoot()")]
    GetRoot -> u256,
    #[selector = sol("verifyMembership(uint256,uint256,uint256[{depth}])")]
    VerifyMembership {{ leaf: u256, index: u256, path: [u256; ROLLCALL_DEPTH] }} -> bool,
}}

struct RollcallStore {{
    owner_inner: u256,
    root: u256,
}}

pub contract RollcallRegistry uses (ctx: Ctx) {{
    mut store: RollcallStore

    init() uses (mut store, ctx) {{
        store.owner_inner = ctx.caller().inner
    }}

    recv RollcallMsg {{
        Commit {{ root }} uses (mut store, ctx) {{
            assert!(
                ctx.caller().inner == store.owner_inner,
                "RollcallRegistry: only owner may commit",
            )
            store.root = root
        }}

        GetRoot -> u256 uses store {{
            store.root
        }}

        VerifyMembership {{ leaf, index, path }} -> bool uses store {{
            fold_rollcall_path(leaf, index, path) == store.root
        }}
    }}
}}

fn fold_rollcall_path(leaf: u256, index: u256, path: [u256; ROLLCALL_DEPTH]) -> u256 {{
    let mut node: u256 = leaf
    let mut idx: u256 = index
    for level in 0 .. ROLLCALL_DEPTH {{
        let sibling: u256 = path[level]
        if idx & 1 == 0 {{
            node = hash2(node, sibling)
        }} else {{
            node = hash2(sibling, node)
        }}
        idx = idx / 2
    }}
    node
}}

msg Hash2Msg {{
    #[selector = sol("hash2Probe(uint256,uint256)")]
    Hash2 {{ a: u256, b: u256 }} -> u256,
}}

pub contract Hash2Exec {{
    recv Hash2Msg {{
        Hash2 {{ a, b }} -> u256 {{
            hash2(a, b)
        }}
    }}
}}
"#,
        depth = depth,
    )
}

fn rollcall_source(depth: usize) -> String {
    format!(
        "{poseidon}\n{wrapper}",
        poseidon = const_poseidon_source(),
        wrapper = rollcall_wrapper(depth)
    )
}

fn to_abi_u256(value: U256) -> AbiU256 {
    AbiU256::from_big_endian(&value.to_be_bytes::<32>())
}

fn decode_bool(return_data: &[u8]) -> bool {
    let raw = bytes_to_u256(return_data).expect("bool return should be one word");
    raw != U256::ZERO
}

fn merkle_to_limbs(x: &BigUint, n: usize) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..n)
        .map(|j| {
            let limb = (x >> (MERKLE_LIMB_BITS * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

fn merkle_limbs_to_biguint(limbs: &[u32]) -> BigUint {
    let mut acc = BigUint::from(0u32);
    for (j, &l) in limbs.iter().enumerate() {
        acc |= BigUint::from(l) << (MERKLE_LIMB_BITS * j);
    }
    acc
}

fn biguint_to_abi_u256(x: &BigUint) -> AbiU256 {
    let bytes = x.to_bytes_be();
    assert!(bytes.len() <= 32, "field element must fit in 32 bytes");
    let mut buf = [0u8; 32];
    buf[32 - bytes.len()..].copy_from_slice(&bytes);
    AbiU256::from_big_endian(&buf)
}

fn root_hex(root: &BigUint) -> String {
    format!("0x{}", root.to_str_radix(16))
}

// --- WASM leg: compile + execute (the leg the browser app also runs). -----

fn compile_wasm(source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_rollcall_merkle.fe").expect("gen URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("gen file should load");
    let top_mod = db.top_mod(file);
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("merkle builder should compile Fe -> wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode")
}

fn merkle_root_on_wasm(bytes: &[u8], leaves: &[BigUint]) -> BigUint {
    let n = MERKLE_N_LIMBS;
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_func(&mut store, KERNEL_NAME)
        .unwrap_or_else(|| panic!("`{KERNEL_NAME}` export should exist"));

    let leaf_limbs: Vec<Vec<u32>> = leaves.iter().map(|x| merkle_to_limbs(x, n)).collect();
    let mut root_limbs = Vec::with_capacity(n);
    for k in 0..n {
        let mut params: Vec<wasmtime::Val> = Vec::with_capacity(1 + leaf_limbs.len() * n);
        params.push(wasmtime::Val::I32(k as i32));
        for leaf in &leaf_limbs {
            for &l in leaf {
                params.push(wasmtime::Val::I32(l as i32));
            }
        }
        let mut results = [wasmtime::Val::I32(0)];
        f.call(&mut store, &params, &mut results)
            .unwrap_or_else(|e| panic!("merkle builder (k={k}) should run under wasmtime: {e:?}"));
        root_limbs.push(match results[0] {
            wasmtime::Val::I32(v) => v as u32,
            other => panic!("merkle builder result must be i32, got {other:?}"),
        });
    }
    merkle_limbs_to_biguint(&root_limbs)
}

// --- Native/Cranelift leg: attempt, report honestly either way. -----------

#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn merkle_root_on_native(source: &str, leaves: &[BigUint]) -> Result<BigUint, String> {
    let n = MERKLE_N_LIMBS;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_rollcall_merkle_native.fe").expect("gen URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("gen file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, KERNEL_NAME)
        .map_err(|error| error.to_string())?;
    let artifact =
        fe_codegen::compile_runtime_package_native_merkle_root_entry(&db, &package, KERNEL_NAME)
            .map_err(|error| error.to_string())?;

    let leaf_limbs: Vec<Vec<u32>> = leaves.iter().map(|x| merkle_to_limbs(x, n)).collect();
    let mut root_limbs = Vec::with_capacity(n);
    for k in 0..n {
        let mut args = [0i32; fe_codegen::MERKLE_ROOT_NATIVE_ENTRY_ARITY];
        args[0] = k as i32;
        let mut idx = 1;
        for leaf in &leaf_limbs {
            for &l in leaf {
                args[idx] = l as i32;
                idx += 1;
            }
        }
        root_limbs.push(artifact.call(&args) as u32);
    }
    Ok(merkle_limbs_to_biguint(&root_limbs))
}

// --- GPU/SPIR-V leg: compile + naga-validate attempt, never executed. -----

fn spirv_validation_attempt(source: &str) -> Result<usize, String> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_rollcall_merkle_spirv.fe").expect("gen URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("gen file should load");
    let top_mod = db.top_mod(file);
    let package =
        mir::build_wasm_runtime_package(&db, top_mod).map_err(|error| error.to_string())?;
    fe_codegen::compile_runtime_package_spirv(&db, &package)
        .map(|artifact| artifact.words.len())
        .map_err(|error| error.to_string())
}

fn write_file(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes)
        .unwrap_or_else(|e| panic!("could not write {}: {e}", path.display()));
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)");
    let demo_dir = repo_root.join("demos/rollcall");
    let gen_dir = demo_dir.join("gen");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", gen_dir.display()));

    let leaves_big: Vec<BigUint> = CANONICAL_LEAVES.iter().map(|&v| BigUint::from(v)).collect();
    let leaves_abi: Vec<AbiU256> = CANONICAL_LEAVES.iter().map(|&v| AbiU256::from(v)).collect();

    eprintln!("gen_rollcall_evidence: compiling `{KERNEL_NAME}` (N=4/depth-2) -> wasm");
    let wasm_bytes = compile_wasm(MERKLE4_SRC);
    let wasm_root = merkle_root_on_wasm(&wasm_bytes, &leaves_big);
    eprintln!(
        "  wasm leg EXECUTED: root(111,222,333,444) = {}",
        root_hex(&wasm_root)
    );

    // --- Native/Cranelift leg. ------------------------------------------
    #[cfg(all(
        feature = "native-backend",
        not(target_arch = "wasm32"),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    let native_result: Result<BigUint, String> = merkle_root_on_native(MERKLE4_SRC, &leaves_big);
    #[cfg(not(all(
        feature = "native-backend",
        not(target_arch = "wasm32"),
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    let native_result: Result<BigUint, String> =
        Err("generator built without the native-backend feature".to_string());
    match &native_result {
        Ok(root) => {
            assert_eq!(
                root, &wasm_root,
                "native/Cranelift root must equal the wasm root when native execution succeeds"
            );
            eprintln!(
                "  native/Cranelift leg EXECUTED: root == wasm root ({})",
                root_hex(root)
            );
        }
        Err(message) => eprintln!("  native/Cranelift leg NOT RUN: {message}"),
    }

    // --- GPU/SPIR-V leg. --------------------------------------------------
    let spirv_result = spirv_validation_attempt(MERKLE4_SRC);
    match &spirv_result {
        Ok(words) => eprintln!("  GPU/SPIR-V leg VALIDATED: {words} naga-validated SPIR-V words"),
        Err(message) => eprintln!("  GPU/SPIR-V leg NOT RUN: {message}"),
    }

    // --- EVM leg: build off-chain tree via the SAME hash2, commit, verify. -
    eprintln!("  EVM leg: building RollcallRegistry + Hash2Exec, committing the wasm-built root");
    let source = rollcall_source(DEPTH);
    let hash2_harness =
        FeContractHarness::compile("Hash2Exec", &source).expect("Hash2Exec should compile");
    let mut hash2_instance = hash2_harness
        .deploy_with_init()
        .expect("Hash2Exec should deploy under revm");
    let mut levels: Vec<Vec<AbiU256>> = vec![leaves_abi.clone()];
    for _ in 0..DEPTH {
        let prev = levels.last().expect("at least one level");
        let mut next = Vec::with_capacity(prev.len() / 2);
        for pair in prev.chunks(2) {
            let result = hash2_instance
                .call_function(
                    "hash2Probe(uint256,uint256)",
                    &[Token::Uint(pair[0]), Token::Uint(pair[1])],
                    ExecutionOptions::default(),
                )
                .expect("hash2Probe should execute under revm");
            let raw = bytes_to_u256(&result.return_data).expect("hash2Probe returns one word");
            next.push(to_abi_u256(raw));
        }
        levels.push(next);
    }
    let probe_root = levels[DEPTH][0];
    let wasm_root_abi = biguint_to_abi_u256(&wasm_root);
    assert_eq!(
        wasm_root_abi, probe_root,
        "the wasm-built root must equal the EVM hash2-probe-built root (same circomlib Poseidon \
         on both legs)"
    );

    let registry_harness = FeContractHarness::compile("RollcallRegistry", &source)
        .expect("RollcallRegistry should compile");
    let mut registry = registry_harness
        .deploy_with_init()
        .expect("RollcallRegistry should deploy under revm");
    registry
        .call_function(
            "commit(uint256)",
            &[Token::Uint(wasm_root_abi)],
            ExecutionOptions::default(),
        )
        .expect("owner commit of the wasm-built root should succeed");

    let target_index = 2usize; // leaf value 333
    let member_leaf = leaves_abi[target_index];
    let mut path = Vec::with_capacity(DEPTH);
    {
        let mut index = target_index;
        for level in &levels[..DEPTH] {
            path.push(level[index ^ 1]);
            index /= 2;
        }
    }
    let path_tokens = || Token::FixedArray(path.iter().copied().map(Token::Uint).collect());

    let accept = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[2])",
            &[
                Token::Uint(member_leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute");
    assert!(
        decode_bool(&accept.return_data),
        "a real member must verify"
    );
    let accept_gas = accept.gas_used;

    let mut tampered = path.clone();
    tampered[0] = tampered[0] + AbiU256::from(1u64);
    let reject_tampered = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[2])",
            &[
                Token::Uint(member_leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                Token::FixedArray(tampered.iter().copied().map(Token::Uint).collect()),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute for a tampered path");
    assert!(
        !decode_bool(&reject_tampered.return_data),
        "a tampered path must be rejected"
    );

    let reject_nonmember = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[2])",
            &[
                Token::Uint(AbiU256::from(999u64)),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute for a non-member leaf");
    assert!(
        !decode_bool(&reject_nonmember.return_data),
        "a non-member leaf must be rejected"
    );

    // Unauthorized commit must revert (fail-closed access control).
    let unauthorized_commit = FeContractHarness::compile("RollcallRegistry", &source)
        .expect("RollcallRegistry should compile")
        .deploy_with_init()
        .expect("RollcallRegistry should deploy under revm")
        .call_function(
            "commit(uint256)",
            &[Token::Uint(wasm_root_abi)],
            ExecutionOptions {
                caller: Address::with_last_byte(0xab),
                ..ExecutionOptions::default()
            },
        );
    assert!(
        unauthorized_commit.is_err(),
        "a non-owner commit must revert"
    );

    eprintln!(
        "  EVM leg EXECUTED (revm, local, NOT a testnet): commit ok; verifyMembership ACCEPT \
         (member, gas_used={accept_gas}), REJECT (tampered path), REJECT (non-member), REJECT \
         (unauthorized commit)"
    );

    // --- Write demos/rollcall/gen/*. --------------------------------------
    let kernel_param_types: Vec<&str> = std::iter::repeat_n("u32", 81).collect();
    let kernel_manifest_json = serde_json::to_string_pretty(&serde_json::json!({
        "protocol": { "major": 1, "minor": 1 },
        "entry": KERNEL_NAME,
        "source": {
            "path": "demos/rollcall/gen/kernel.fe",
            "sha256": sha256_hex(MERKLE4_SRC.as_bytes()),
        },
        "interface": {
            "imports": [],
            "exports": [{
                "name": KERNEL_NAME,
                "params": kernel_param_types,
                "result": "u32",
            }],
            "resources": [],
        },
        "artifacts": [{
            "kind": "wasm_module",
            "byte_len": wasm_bytes.len(),
            "sha256": sha256_hex(&wasm_bytes),
        }],
    }))
    .expect("kernel.manifest.json should serialize");

    let reference_json = serde_json::to_string_pretty(&serde_json::json!({
        "kernel": KERNEL_NAME,
        "depth": DEPTH,
        "leaves": CANONICAL_LEAVES,
        "root_hex": root_hex(&wasm_root),
        "sample_membership": {
            "index": target_index,
            "leaf": CANONICAL_LEAVES[target_index],
            "accept": true,
        },
        "runtime": "wasmtime (Fe -> wasm), executed at generation time",
    }))
    .expect("reference.json should serialize");

    write_file(&gen_dir.join("kernel.fe"), MERKLE4_SRC.as_bytes());
    write_file(&gen_dir.join("kernel.wasm"), &wasm_bytes);
    write_file(
        &gen_dir.join("kernel.manifest.json"),
        kernel_manifest_json.as_bytes(),
    );
    write_file(&gen_dir.join("reference.json"), reference_json.as_bytes());

    // --- Write demos/rollcall/evidence.json (fe-capstone-evidence v1). ----
    let source_evidence = SourceEvidence {
        path: "demos/rollcall/gen/kernel.fe",
        sha256: sha256_hex(MERKLE4_SRC.as_bytes()),
    };
    let interface = InterfaceSnapshot {
        version: 1,
        export: KERNEL_NAME,
        parameters: {
            let mut params = vec!["u32"]; // k, the output-limb index
            params.extend(std::iter::repeat_n("u32", 80)); // 4 leaves x 20 limbs
            params
        },
        result: "u32",
    };

    let native_target = match &native_result {
        Ok(root) => TargetEvidence {
            target: "native",
            runtime: "Cranelift JIT",
            imports: vec![],
            exports: vec![KERNEL_NAME],
            artifact: None,
            verification: VerificationEvidence {
                status: VerificationStatus::Verified,
                scope: "root(111,222,333,444), all 20 output limbs",
                command: "cargo run -p fe-codegen --features native-backend --example gen_rollcall_evidence",
                test: "generator native/Cranelift execution",
                result: Some(format!("root == wasm root ({})", root_hex(root))),
                note: None,
            },
        },
        Err(message) => TargetEvidence {
            target: "native",
            runtime: "Cranelift JIT",
            imports: vec![],
            exports: vec![KERNEL_NAME],
            artifact: None,
            verification: VerificationEvidence {
                status: VerificationStatus::NotRun,
                scope: "root(111,222,333,444), all 20 output limbs",
                command: "cargo run -p fe-codegen --features native-backend --example gen_rollcall_evidence",
                test: "generator native/Cranelift execution",
                result: None,
                note: Some(Box::leak(
                    format!(
                        "Native execution is not currently possible on the pinned Sonatina rev for \
                     this array-using kernel: {message} Same root cause as the SPIR-V leg \
                     (function-local array lowering via MemAllocDynamic is wasm-only on this \
                     pin); re-lands with the fork re-pin (Decision 5). See \
                     RUNG4_ASSEMBLY_PLAN.md."
                    )
                    .into_boxed_str(),
                )),
            },
        },
    };

    let webgpu_target = match &spirv_result {
        Ok(_words) => TargetEvidence {
            target: "webgpu",
            runtime: "browser WebGPU (naga SPIR-V -> WGSL)",
            imports: vec![],
            exports: vec![KERNEL_NAME],
            artifact: None,
            verification: VerificationEvidence {
                status: VerificationStatus::Validated,
                scope: "naga SPIR-V translation + validation",
                command: "cargo run -p fe-codegen --features native-backend --example gen_rollcall_evidence",
                test: "generator SPIR-V compile + naga validation",
                result: Some("naga-validated SPIR-V module".to_string()),
                note: Some(
                    "This is validation only, never a live GPU execution claim. No Vulkan \
                     adapter (lavapipe) is available in this sandbox, so GPU EXECUTION is \
                     pending lavapipe on a host that has one.",
                ),
            },
        },
        Err(message) => TargetEvidence {
            target: "webgpu",
            runtime: "browser WebGPU (naga SPIR-V -> WGSL)",
            imports: vec![],
            exports: vec![KERNEL_NAME],
            artifact: None,
            verification: VerificationEvidence {
                status: VerificationStatus::NotRun,
                scope: "naga SPIR-V translation + validation",
                command: "cargo run -p fe-codegen --features native-backend --example gen_rollcall_evidence",
                test: "generator SPIR-V compile + naga validation",
                result: None,
                note: Some(Box::leak(
                    format!(
                        "GPU validation is not currently possible on the pinned Sonatina rev for \
                     this array-using kernel: {message} The private-storage heap emulation it \
                     needs (SpirvLayout.trap) exists only on the unpushed fork branch \
                     rung3-spirv-arrays-v2 (same-day revert b55f051e9 -> 40f8a1f27 on this \
                     branch); re-lands with the fork re-pin (Decision 5). GPU EXECUTION would \
                     additionally need lavapipe, unavailable in this sandbox regardless. See \
                     RUNG4_ASSEMBLY_PLAN.md."
                    )
                    .into_boxed_str(),
                )),
            },
        },
    };

    let evidence = CapstoneEvidenceManifest {
        protocol: CAPSTONE_EVIDENCE_PROTOCOL,
        version: CAPSTONE_EVIDENCE_VERSION,
        capstone: "rollcall-poseidon-merkle",
        source: source_evidence,
        interface,
        targets: vec![
            TargetEvidence {
                target: "evm",
                runtime: "revm (local harness, NOT a testnet deploy)",
                imports: vec![],
                exports: vec![
                    "RollcallRegistry.commit(uint256)",
                    "RollcallRegistry.verifyMembership(uint256,uint256,uint256[2])",
                ],
                artifact: None,
                verification: VerificationEvidence {
                    status: VerificationStatus::Verified,
                    scope: "commit + accept(member) + reject(tampered) + reject(non-member) + \
                            reject(unauthorized commit)",
                    command: "cargo run -p fe-codegen --features native-backend --example gen_rollcall_evidence",
                    test: "generator EVM/revm execution",
                    result: Some(format!(
                        "root == wasm root ({}); verifyMembership gas_used={accept_gas}",
                        root_hex(&wasm_root)
                    )),
                    note: Some(
                        "revm local harness (FeContractHarness), the same harness path \
                         rollcall_e2e.rs uses. This is NOT a testnet or mainnet deploy.",
                    ),
                },
            },
            native_target,
            TargetEvidence {
                target: "wasm",
                runtime: "wasmtime",
                imports: vec![],
                exports: vec![KERNEL_NAME],
                artifact: Some(ArtifactEvidence::from_bytes(
                    "wasm-module",
                    "demos/rollcall/gen/kernel.wasm",
                    &wasm_bytes,
                )),
                verification: VerificationEvidence {
                    status: VerificationStatus::Verified,
                    scope: "root(111,222,333,444), all 20 output limbs",
                    command: "cargo run -p fe-codegen --features native-backend --example gen_rollcall_evidence",
                    test: "generator wasm/wasmtime execution",
                    result: Some(root_hex(&wasm_root)),
                    note: None,
                },
            },
            webgpu_target,
        ],
    };
    evidence
        .validate()
        .expect("the generated Rollcall capstone evidence must satisfy protocol v1");
    write_file(
        &demo_dir.join("evidence.json"),
        evidence.to_pretty_json().as_bytes(),
    );

    eprintln!(
        "gen_rollcall_evidence: wrote demos/rollcall/gen/{{kernel.fe,kernel.wasm,\
         kernel.manifest.json,reference.json}} and demos/rollcall/evidence.json"
    );
}
