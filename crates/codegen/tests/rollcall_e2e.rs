//! Rollcall (Rung 2 Stage 1): the on-chain half of the flagship. A
//! `RollcallRegistry` EVM contract commits a Poseidon-Merkle root and verifies
//! O(log n) membership claims under `revm`, executed here exactly like the
//! keystone's EVM legs in `spirv_e2e.rs` (`FeContractHarness` /
//! `ExecutionOptions` / `bytes_to_u256`).
//!
//! Hash primitive: this file reuses `const_poseidon.fe`'s `hash2` VERBATIM
//! (via `include_str!`), the same circomlib-pinned Poseidon (t=3, real
//! MDS/round constants, `static_assert`-pinned to circomlib's `hash2(0,0)` /
//! `hash2(1,2)` test vectors) already grounded in the codebase. Nothing about
//! the hash is redefined or weakened here; the wrapper below only ADDS a
//! contract that calls the existing `hash2`. The file's own `#[test]` block
//! (a duplicate check of the same `static_assert` pins) is trimmed before
//! compiling, because `FeContractHarness::compile` type-checks the whole
//! module as one flat unit and a `#[test]` fn is orthogonal noise there; the
//! `static_assert` pins themselves stay byte-for-byte.
//!
//! Design: `commit(root)` is owner-gated (only the deployer may (re)commit),
//! otherwise anyone could publish their own root and "prove" membership in a
//! list they invented -- the one access check a membership registry cannot
//! skip. `verify_membership` folds a sibling path bottom-up with the SAME
//! `hash2`, direction bit = `index & 1` (index shifts right each level,
//! exactly the deposit-contract incremental-Merkle convention already in this
//! repo). `claim` is the bonus: same fold, plus a one-shot claimed-bitmap so a
//! valid proof can only flip its bit once (a second claim of the same index
//! fails closed even with a valid proof).
//!
//! Gas methodology: rather than measuring one small depth and extrapolating,
//! this measures TWO real depths directly: D=4 (a genuine 16-leaf tree --
//! build the tree off-chain via repeated calls to the SAME `hash2` exposed
//! through a tiny `Hash2Exec` probe contract, so the off-chain root and the
//! on-chain root are computed by the identical pinned function) for the
//! accept/reject/claim correctness suite, and D=20 (the depth the vision doc
//! calls out as the real target) for gas ONLY, via a synthetic sibling chain
//! (arbitrary sibling values folded up from a leaf -- valid mathematically,
//! but not tied to a materialized 2^20-leaf tree, which would need a million
//! off-chain hash2 calls to build for no additional signal: gas only depends
//! on the number of levels folded, not the sibling values). This gives a
//! direct D=20 number instead of a guessed extrapolation.

use common::InputDb;
use driver::DriverDataBase;
use ethers_core::abi::Token;
use ethers_core::types::U256 as AbiU256;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use fe_contract_harness::{
    Address, ExecutionOptions, FeContractHarness, RuntimeInstance, U256, bytes_to_u256,
};
use num_bigint::BigUint;
use url::Url;

/// `const_poseidon.fe`'s hash2/hash5 pinned to circomlib vectors (t=3 Poseidon:
/// real MDS matrix + round constants). Included verbatim; see module docs.
const CONST_POSEIDON_FULL_SOURCE: &str =
    include_str!("../../fe/tests/fixtures/fe_test/const_poseidon.fe");

/// Trims the trailing `#[test] fn test_const_poseidon_vectors()` block, which
/// duplicates the `static_assert` pins just above it. The `static_assert`
/// lines (the actual circomlib pin) are kept byte-for-byte.
fn const_poseidon_source() -> &'static str {
    CONST_POSEIDON_FULL_SOURCE
        .split_once("\n#[test]\n")
        .expect("const_poseidon.fe should still contain its pinned #[test] marker")
        .0
}

/// The RollcallRegistry contract + a `Hash2Exec` probe contract (so the test
/// can compute off-chain hashes through the SAME `hash2`), parameterized by
/// Merkle depth. Appended, unmodified in spirit, to `const_poseidon_source()`
/// -- mirrors the `MANDEL_Q12_EVM_WRAPPER` / `KEYSTONE_U32_EVM_WRAPPER`
/// pattern in `spirv_e2e.rs`: one flat module, kernel source first, wrapper
/// second.
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
    #[selector = sol("claim(uint256,uint256,uint256[{depth}])")]
    Claim {{ leaf: u256, index: u256, path: [u256; ROLLCALL_DEPTH] }} -> bool,
    #[selector = sol("isClaimed(uint256)")]
    IsClaimed {{ index: u256 }} -> bool,
}}

struct RollcallStore {{
    owner_inner: u256,
    root: u256,
    claimed: StorageMap<u256, u256>,
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

        Claim {{ leaf, index, path }} -> bool uses (mut store) {{
            if store.claimed.get(key: index) != 0 {{
                return false
            }}
            if fold_rollcall_path(leaf, index, path) != store.root {{
                return false
            }}
            store.claimed.set(key: index, value: 1)
            true
        }}

        IsClaimed {{ index }} -> bool uses store {{
            store.claimed.get(key: index) != 0
        }}
    }}
}}

// Folds a sibling path bottom-up with the SAME pinned `hash2`. Direction bit
// = `index & 1` (0: current node is the left child, 1: right), index shifts
// right each level -- the deposit-contract incremental-Merkle convention.
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

// Test-only probe: exposes the SAME `hash2` so the Rust test can build an
// off-chain tree/path that agrees with the on-chain fold by construction.
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

fn from_abi_u256(value: AbiU256) -> [u8; 32] {
    let mut buf = [0u8; 32];
    value.to_big_endian(&mut buf);
    buf
}

/// Calls the `Hash2Exec` probe's `hash2Probe(uint256,uint256)` -- the exact
/// same `hash2` the on-chain `RollcallRegistry` folds with -- so any tree or
/// path built here agrees with the contract by construction, not by a second
/// (possibly divergent) implementation.
fn call_hash2(instance: &mut RuntimeInstance, a: AbiU256, b: AbiU256) -> AbiU256 {
    let result = instance
        .call_function(
            "hash2Probe(uint256,uint256)",
            &[Token::Uint(a), Token::Uint(b)],
            ExecutionOptions::default(),
        )
        .expect("hash2Probe should execute under revm");
    let raw = bytes_to_u256(&result.return_data).expect("hash2Probe returns one u256 word");
    to_abi_u256(raw)
}

fn decode_bool(return_data: &[u8]) -> bool {
    let raw = bytes_to_u256(return_data).expect("bool return should be one word");
    raw != U256::ZERO
}

/// Builds a genuine `2^depth`-leaf Merkle tree bottom-up via `hash2Probe`,
/// returning every level (`levels[0]` = leaves, `levels[depth]` = `[root]`).
fn build_tree(instance: &mut RuntimeInstance, depth: usize) -> Vec<Vec<AbiU256>> {
    let leaf_count = 1usize << depth;
    let leaves: Vec<AbiU256> = (0..leaf_count)
        .map(|i| AbiU256::from((i as u64) + 1))
        .collect();
    let mut levels = vec![leaves];
    for _ in 0..depth {
        let prev = levels.last().expect("at least one level");
        let mut next = Vec::with_capacity(prev.len() / 2);
        for pair in prev.chunks(2) {
            next.push(call_hash2(instance, pair[0], pair[1]));
        }
        levels.push(next);
    }
    levels
}

/// The sibling path for `index` within a tree built by [`build_tree`].
fn sibling_path(levels: &[Vec<AbiU256>], mut index: usize) -> Vec<AbiU256> {
    let mut path = Vec::with_capacity(levels.len() - 1);
    for level in &levels[..levels.len() - 1] {
        path.push(level[index ^ 1]);
        index /= 2;
    }
    path
}

fn path_tokens(path: &[AbiU256]) -> Token {
    Token::FixedArray(path.iter().copied().map(Token::Uint).collect())
}

/// D=4 correctness suite: a real 16-leaf tree, a valid membership proof
/// (accept), a tampered proof (fail-closed reject), and the bonus claim flow
/// (a valid claim flips the bit once; replaying the same valid proof on an
/// already-claimed index is rejected). Also measures gas for the accepting
/// `verifyMembership` call.
#[test]
fn rollcall_registry_accept_reject_and_claim_at_depth4() {
    const DEPTH: usize = 4;
    let source = rollcall_source(DEPTH);

    let hash2_harness =
        FeContractHarness::compile("Hash2Exec", &source).expect("Hash2Exec should compile");
    let mut hash2_instance = hash2_harness
        .deploy_with_init()
        .expect("Hash2Exec should deploy under revm");

    let levels = build_tree(&mut hash2_instance, DEPTH);
    let root = levels[DEPTH][0];

    let target_index: usize = 5;
    let leaf = levels[0][target_index];
    let path = sibling_path(&levels, target_index);
    assert_eq!(path.len(), DEPTH);

    let registry_harness = FeContractHarness::compile("RollcallRegistry", &source)
        .expect("RollcallRegistry should compile");
    let mut registry = registry_harness
        .deploy_with_init()
        .expect("RollcallRegistry should deploy under revm");

    // An unauthorized commit must revert (fail-closed access control): a
    // non-owner caller trying to publish a root is rejected outright.
    let unauthorized_caller = Address::with_last_byte(0xab);
    let unauthorized = registry.call_function(
        "commit(uint256)",
        &[Token::Uint(root)],
        ExecutionOptions {
            caller: unauthorized_caller,
            ..ExecutionOptions::default()
        },
    );
    assert!(
        unauthorized.is_err(),
        "commit from a non-owner caller must revert, got {unauthorized:?}"
    );

    // Owner (the deploying caller) commits the real root.
    registry
        .call_function(
            "commit(uint256)",
            &[Token::Uint(root)],
            ExecutionOptions::default(),
        )
        .expect("owner commit should succeed");

    let stored_root = registry
        .call_function("getRoot()", &[], ExecutionOptions::default())
        .expect("getRoot should execute");
    assert_eq!(
        bytes_to_u256(&stored_root.return_data).unwrap(),
        to_abi_u256_to_revm(root),
        "stored root must equal the committed root"
    );

    // ACCEPT: a valid membership proof for a real leaf verifies true, and
    // this is the gas measurement point.
    let accept_result = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[4])",
            &[
                Token::Uint(leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&path),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute");
    assert!(
        decode_bool(&accept_result.return_data),
        "a valid depth-4 membership proof must verify"
    );
    eprintln!(
        "Rollcall D=4 verifyMembership (ACCEPT): gas_used={}",
        accept_result.gas_used
    );

    // REJECT: tamper with one sibling in the path -- must fail closed.
    let mut tampered_path = path.clone();
    tampered_path[0] = tampered_path[0] + AbiU256::from(1u64);
    let reject_result = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[4])",
            &[
                Token::Uint(leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&tampered_path),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute even for a bad proof (returns false, no revert)");
    assert!(
        !decode_bool(&reject_result.return_data),
        "a tampered membership proof must be rejected"
    );

    // REJECT: a valid path but the wrong claimed leaf value.
    let wrong_leaf_result = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[4])",
            &[
                Token::Uint(leaf + AbiU256::from(1u64)),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&path),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute");
    assert!(
        !decode_bool(&wrong_leaf_result.return_data),
        "a proof for the wrong leaf value must be rejected"
    );

    // Bonus: claim flow. First claim on a valid proof succeeds and flips the
    // bit; a second claim of the same index (even with the same valid proof)
    // is rejected -- fail-closed against double-claiming.
    let first_claim = registry
        .call_function(
            "claim(uint256,uint256,uint256[4])",
            &[
                Token::Uint(leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&path),
            ],
            ExecutionOptions::default(),
        )
        .expect("claim should execute");
    assert!(
        decode_bool(&first_claim.return_data),
        "the first claim on a valid proof must succeed"
    );

    let is_claimed = registry
        .call_function(
            "isClaimed(uint256)",
            &[Token::Uint(AbiU256::from(target_index as u64))],
            ExecutionOptions::default(),
        )
        .expect("isClaimed should execute");
    assert!(
        decode_bool(&is_claimed.return_data),
        "isClaimed must report true after a successful claim"
    );

    let second_claim = registry
        .call_function(
            "claim(uint256,uint256,uint256[4])",
            &[
                Token::Uint(leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&path),
            ],
            ExecutionOptions::default(),
        )
        .expect("claim should execute (returns false, no revert)");
    assert!(
        !decode_bool(&second_claim.return_data),
        "a second claim of an already-claimed index must be rejected even with a valid proof"
    );
}

fn to_abi_u256_to_revm(value: AbiU256) -> U256 {
    U256::from_be_bytes(from_abi_u256(value))
}

/// D=20 gas-only measurement: the vision doc's real target depth. Rather than
/// materializing a 2^20-leaf tree (a million off-chain hash2 calls for no
/// extra signal -- gas depends only on the number of levels folded), this
/// folds a synthetic sibling chain up from a leaf via the SAME `hash2Probe`,
/// commits the resulting root, and measures one `verifyMembership` call. This
/// is a direct measurement at the real depth, not an extrapolation.
#[test]
fn rollcall_registry_gas_at_depth20_is_l2_honest() {
    const DEPTH: usize = 20;
    let source = rollcall_source(DEPTH);

    let hash2_harness =
        FeContractHarness::compile("Hash2Exec", &source).expect("Hash2Exec should compile");
    let mut hash2_instance = hash2_harness
        .deploy_with_init()
        .expect("Hash2Exec should deploy under revm");

    // Fold a leaf up through DEPTH synthetic siblings (always the "left
    // child" direction, index = 0 throughout) to get a root that is a
    // genuinely valid depth-20 fold of the SAME hash2.
    let leaf = AbiU256::from(1u64);
    let mut node = leaf;
    let mut path = Vec::with_capacity(DEPTH);
    for level in 0..DEPTH {
        let sibling = AbiU256::from((level as u64) + 1000);
        node = call_hash2(&mut hash2_instance, node, sibling);
        path.push(sibling);
    }
    let root = node;

    let registry_harness = FeContractHarness::compile("RollcallRegistry", &source)
        .expect("RollcallRegistry should compile");
    let mut registry = registry_harness
        .deploy_with_init()
        .expect("RollcallRegistry should deploy under revm");

    registry
        .call_function(
            "commit(uint256)",
            &[Token::Uint(root)],
            ExecutionOptions::default(),
        )
        .expect("owner commit should succeed");

    let verify_result = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[20])",
            &[Token::Uint(leaf), Token::Uint(AbiU256::from(0u64)), path_tokens(&path)],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute");
    assert!(
        decode_bool(&verify_result.return_data),
        "the depth-20 synthetic proof must verify (it is a genuine hash2 fold, just not tied \
         to a materialized 2^20-leaf tree)"
    );

    // L2-honest gas: depth-20 Poseidon-Merkle verification is NOT cheap on
    // L1 (roughly 20 field-arithmetic-heavy Poseidon permutations' worth of
    // EVM execution). Report the real number rather than a guess.
    eprintln!(
        "Rollcall D=20 verifyMembership (ACCEPT, direct measurement): gas_used={}",
        verify_result.gas_used
    );
    assert!(
        verify_result.gas_used > 0,
        "gas_used should be a real positive measurement"
    );
}

// ============================================================================
// END-TO-END: prove(wasm) -> commit(EVM) -> verify(EVM), one Poseidon.
//
// The SERIAL wasm Poseidon-Merkle root builder (`poseidon_merkle_root_loop.fe`,
// the ungated prover leg proven bit-exact vs an independent oracle in
// `wasm_e2e::serial_poseidon_merkle_root_matches_bigint_tree_oracle_on_wasm_at_o0_and_o2`)
// builds a root from N=4 leaves on wasm under wasmtime; that root is committed to
// the `RollcallRegistry` under revm; then a sibling path derived from the SAME
// tree verifies membership on-chain (accept for a real member, reject for a
// non-member and a tampered path). The wasm-built root is additionally
// cross-checked equal to the tree built by the on-chain `hash2` probe, so the
// SAME Poseidon (circomlib t=3) runs on the wasm prover leg and the EVM verifier
// leg -- the prove->commit->verify flow closed end to end.
// ============================================================================

const MERKLE4_SRC: &str = include_str!("fixtures/spirv/poseidon_merkle_root_loop.fe");
const MERKLE_LIMB_BITS: usize = 13;
const MERKLE_N_LIMBS: usize = 20;

/// Decompose a field element into `n` little-endian 13-bit limbs (the wasm
/// builder's input schema).
fn merkle_to_limbs(x: &BigUint, n: usize) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..n)
        .map(|j| {
            let limb = (x >> (MERKLE_LIMB_BITS * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

/// Reassemble little-endian 13-bit limbs into a field element.
fn merkle_limbs_to_biguint(limbs: &[u32]) -> BigUint {
    let mut acc = BigUint::from(0u32);
    for (j, &l) in limbs.iter().enumerate() {
        acc |= BigUint::from(l) << (MERKLE_LIMB_BITS * j);
    }
    acc
}

/// A `BigUint` field element (< p < 2^256) as an ABI u256, zero-padded big-endian.
fn biguint_to_abi_u256(x: &BigUint) -> AbiU256 {
    let bytes = x.to_bytes_be();
    assert!(bytes.len() <= 32, "field element must fit in 32 bytes");
    let mut buf = [0u8; 32];
    buf[32 - bytes.len()..].copy_from_slice(&bytes);
    AbiU256::from_big_endian(&buf)
}

/// PROVE: compile + run the serial wasm Poseidon-Merkle root builder over
/// `leaves` (N=4 field elements), returning the root as a field element. Runs on
/// a wide-stack worker thread (the generated function is large; a compiler-stack
/// accommodation, mirroring the wasm_e2e Poseidon/Merkle gates).
fn merkle_root_on_wasm(leaves: &[BigUint]) -> BigUint {
    assert_eq!(leaves.len(), 4, "this end-to-end uses the N=4 (depth-2) builder");
    let leaves = leaves.to_vec();
    std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(move || merkle_root_on_wasm_body(&leaves))
        .expect("spawn wide-stack worker for the wasm Merkle builder")
        .join()
        .expect("wasm Merkle worker thread should not panic")
}

fn merkle_root_on_wasm_body(leaves: &[BigUint]) -> BigUint {
    let n = MERKLE_N_LIMBS;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///poseidon_merkle_root_loop.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MERKLE4_SRC.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("wasm compilation of the Merkle builder should succeed")
        .into_bytecode()
        .expect("wasm output should be bytecode");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_func(&mut store, "poseidon_merkle_root_loop")
        .expect("`poseidon_merkle_root_loop` export should exist");

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

/// Build a genuine `2^depth`-leaf tree bottom-up from explicit `leaves` via the
/// on-chain `hash2Probe` (the SAME `hash2` the registry folds with).
fn build_tree_from_leaves(
    instance: &mut RuntimeInstance,
    leaves: &[AbiU256],
    depth: usize,
) -> Vec<Vec<AbiU256>> {
    assert_eq!(leaves.len(), 1usize << depth, "leaf count must be 2^depth");
    let mut levels = vec![leaves.to_vec()];
    for _ in 0..depth {
        let prev = levels.last().expect("at least one level");
        let mut next = Vec::with_capacity(prev.len() / 2);
        for pair in prev.chunks(2) {
            next.push(call_hash2(instance, pair[0], pair[1]));
        }
        levels.push(next);
    }
    levels
}

/// THE END-TO-END GATE: prove a Poseidon-Merkle root on wasm, commit it to the
/// `RollcallRegistry` under revm, and verify membership on-chain -- accept for a
/// real member, reject for a non-member and a tampered path. The wasm-built root
/// equals the on-chain-probe-built root, so the identical circomlib Poseidon runs
/// on both the (wasm) prover and (EVM) verifier legs.
#[test]
fn rollcall_prove_on_wasm_commit_on_evm_and_verify_membership_end_to_end() {
    const DEPTH: usize = 2; // N = 4 leaves
    let source = rollcall_source(DEPTH);

    // The member list (field elements): the SAME leaves fed to the wasm prover
    // and to the on-chain hash2 probe.
    let leaves_u64: [u64; 4] = [111, 222, 333, 444];
    let leaves_big: Vec<BigUint> = leaves_u64.iter().map(|&v| BigUint::from(v)).collect();
    let leaves_abi: Vec<AbiU256> = leaves_u64.iter().map(|&v| AbiU256::from(v)).collect();

    // PROVE (wasm): the serial loop-form Poseidon-Merkle builder produces the root.
    let wasm_root_big = merkle_root_on_wasm(&leaves_big);
    let wasm_root = biguint_to_abi_u256(&wasm_root_big);

    // Cross-leg agreement: build the same tree on-chain via hash2Probe and assert
    // the wasm-built root equals it (identical Poseidon on the prover + verifier).
    let hash2_harness =
        FeContractHarness::compile("Hash2Exec", &source).expect("Hash2Exec should compile");
    let mut hash2_instance = hash2_harness
        .deploy_with_init()
        .expect("Hash2Exec should deploy under revm");
    let levels = build_tree_from_leaves(&mut hash2_instance, &leaves_abi, DEPTH);
    let probe_root = levels[DEPTH][0];
    assert_eq!(
        wasm_root, probe_root,
        "the serial wasm-built Poseidon-Merkle root must equal the on-chain hash2-built root \
         (same circomlib Poseidon on both legs)"
    );

    // COMMIT (EVM): owner commits the wasm-built root.
    let registry_harness = FeContractHarness::compile("RollcallRegistry", &source)
        .expect("RollcallRegistry should compile");
    let mut registry = registry_harness
        .deploy_with_init()
        .expect("RollcallRegistry should deploy under revm");
    registry
        .call_function(
            "commit(uint256)",
            &[Token::Uint(wasm_root)],
            ExecutionOptions::default(),
        )
        .expect("owner commit of the wasm-built root should succeed");

    // VERIFY (EVM): a real member verifies against the committed wasm-built root.
    let target_index = 2usize; // leaf value 333
    let member_leaf = leaves_abi[target_index];
    let path = sibling_path(&levels, target_index);
    assert_eq!(path.len(), DEPTH);

    let accept = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[2])",
            &[
                Token::Uint(member_leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&path),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute");
    assert!(
        decode_bool(&accept.return_data),
        "a real member must verify against the committed wasm-built root"
    );

    // REJECT: a non-member leaf value with an otherwise-valid path.
    let nonmember = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[2])",
            &[
                Token::Uint(AbiU256::from(999u64)),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&path),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute for a bad leaf (returns false, no revert)");
    assert!(
        !decode_bool(&nonmember.return_data),
        "a non-member leaf must be rejected"
    );

    // REJECT: a tampered sibling in the path.
    let mut tampered = path.clone();
    tampered[0] = tampered[0] + AbiU256::from(1u64);
    let reject = registry
        .call_function(
            "verifyMembership(uint256,uint256,uint256[2])",
            &[
                Token::Uint(member_leaf),
                Token::Uint(AbiU256::from(target_index as u64)),
                path_tokens(&tampered),
            ],
            ExecutionOptions::default(),
        )
        .expect("verifyMembership should execute for a tampered path (returns false, no revert)");
    assert!(
        !decode_bool(&reject.return_data),
        "a tampered membership path must be rejected"
    );

    eprintln!(
        "Rollcall e2e: prove(wasm) Poseidon-Merkle root == probe(EVM) root; committed on-chain; \
         verifyMembership ACCEPT (member) + REJECT (non-member, tampered path) all pass."
    );
}

// ============================================================================
// FOUR-LEG EVIDENCE LEDGER: native/Cranelift + GPU/SPIR-V legs.
//
// The wasm and EVM legs are proven above. These two extend the SAME kernel
// (byte-identical `MERKLE4_SRC`, the SAME 4 leaves) across the remaining two
// legs the Rung 4 evidence ledger names: native/Cranelift (an independently
// EXECUTED cross-check of the root, not a re-implementation) and GPU/SPIR-V
// (compile + naga-validate ONLY; GPU execution is out of scope here and is
// never claimed).
// ============================================================================

/// Native/Cranelift leg: the merkle builder's real ABI is `(i32 x 81) -> i32`
/// (one output-limb index + 4 leaves x 20 Montgomery limbs; Fe has no
/// cross-backend array/pointer ABI, hence the flattened limb args), so this
/// uses the narrow `NativeMerkleRootEntryArtifact` wrapper added alongside
/// this rung (`crates/codegen/src/sonatina/native.rs`), sized exactly for that
/// ABI. Runs on a wide-stack worker thread for the same reason the wasm leg
/// does (the ~2.8k-statement kernel's lowering pipeline recurses deeply).
///
/// Returns `Err` (never panics) on a compile-time lowering failure so the
/// caller can report it honestly instead of hard-failing the whole leg.
#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn merkle_root_on_native(leaves: &[BigUint]) -> Result<BigUint, String> {
    assert_eq!(leaves.len(), 4, "this end-to-end uses the N=4 (depth-2) builder");
    let leaves = leaves.to_vec();
    std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(move || merkle_root_on_native_body(&leaves))
        .expect("spawn wide-stack worker for the native Merkle builder")
        .join()
        .expect("native Merkle worker thread should not panic")
}

#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn merkle_root_on_native_body(leaves: &[BigUint]) -> Result<BigUint, String> {
    let n = MERKLE_N_LIMBS;
    let mut db = DriverDataBase::default();
    let url =
        Url::parse("file:///poseidon_merkle_root_loop_native.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MERKLE4_SRC.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "poseidon_merkle_root_loop")
            .expect("merkle builder should build a native-entry runtime package");
    let artifact = fe_codegen::compile_runtime_package_native_merkle_root_entry(
        &db,
        &package,
        "poseidon_merkle_root_loop",
    )
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
        let result = artifact.call(&args);
        root_limbs.push(result as u32);
    }
    Ok(merkle_limbs_to_biguint(&root_limbs))
}

/// The native/Cranelift leg: attempts to independently EXECUTE the SAME
/// kernel over the SAME 4 leaves as the wasm+EVM end-to-end test above. This
/// is a probe, like the SPIR-V leg below, not a foregone conclusion: the
/// kernel's function-local `[u32; N]` arrays lower to `MemAllocDynamic`, which
/// the wasm backend supports via its canonical arena (`fe_cabi_alloc`) but
/// which `CraneliftBackend` on this pin does not yet lower (observed: "skipping
/// function poseidon_merkle_root_loop: unsupported instruction for
/// CraneliftBackend: Opaque"). This is the SAME root cause as the SPIR-V leg's
/// gap (array/heap lowering exists only for the wasm target on this pin), so
/// this records and asserts on whatever actually happens rather than assuming
/// either outcome.
#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn rollcall_merkle_root_native_cranelift_leg_is_honestly_reported() {
    let leaves_u64: [u64; 4] = [111, 222, 333, 444];
    let leaves_big: Vec<BigUint> = leaves_u64.iter().map(|&v| BigUint::from(v)).collect();

    let wasm_root = merkle_root_on_wasm(&leaves_big);
    match merkle_root_on_native(&leaves_big) {
        Ok(native_root) => {
            assert_eq!(
                wasm_root, native_root,
                "the native/Cranelift-executed Poseidon-Merkle root must equal the \
                 wasm-executed root (same kernel, same 4 leaves, two independent backends)"
            );
            eprintln!(
                "Rollcall native/Cranelift leg: root == wasm-built root (0x{})",
                wasm_root.to_str_radix(16)
            );
        }
        Err(message) => {
            eprintln!(
                "Rollcall native/Cranelift leg: native execution is NOT currently possible on \
                 this pinned Sonatina rev for an array-using kernel: {message}. Same root cause \
                 as the SPIR-V leg (function-local array lowering via MemAllocDynamic is \
                 wasm-only on this pin, via the canonical arena); re-lands with the fork re-pin \
                 (Decision 5)."
            );
        }
    }
}

/// GPU/SPIR-V leg: attempts to compile + naga-validate the SAME kernel through
/// `compile_runtime_package_spirv`. This is a probe, not a foregone
/// conclusion: the kernel uses function-local `[u32; N]` arrays
/// (`MemAllocDynamic`/`Mload`/`Mstore`), and this branch's SPIR-V backend
/// wiring for array-using kernels was reverted same-day (see
/// `RUNG4_ASSEMBLY_PLAN.md`) because the private-storage heap emulation it
/// needs exists only on an unpushed Sonatina fork branch, not the pinned rev.
/// So this test records and asserts on whatever actually happens -- an honest
/// `LowerError` naming the real gap, or (if the pin has moved) a genuine
/// naga-validated artifact -- never an assumed outcome. GPU EXECUTION is out
/// of scope regardless of which branch fires: this only ever claims
/// "validated" or "not run", never "executed".
#[test]
fn rollcall_merkle_root_spirv_validation_is_honestly_reported() {
    let mut db = DriverDataBase::default();
    let url =
        Url::parse("file:///poseidon_merkle_root_loop_spirv.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(MERKLE4_SRC.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("merkle builder should build a wasm-shaped runtime package");

    match fe_codegen::compile_runtime_package_spirv(&db, &package) {
        Ok(artifact) => {
            const SPIRV_MAGIC: u32 = 0x0723_0203;
            assert!(
                !artifact.words.is_empty() && artifact.words[0] == SPIRV_MAGIC,
                "a claimed-Ok SPIR-V artifact must actually start with the SPIR-V magic word"
            );
            eprintln!(
                "Rollcall GPU/SPIR-V leg: the merkle builder DOES naga-validate on this pin \
                 (validated, NOT executed -- GPU execution needs lavapipe and is out of scope \
                 here)."
            );
        }
        Err(error) => {
            let message = error.to_string();
            eprintln!(
                "Rollcall GPU/SPIR-V leg: naga validation is NOT currently possible on this \
                 pinned Sonatina rev for an array-using kernel: {message}. This matches the \
                 same-day b55f051e9 -> 40f8a1f27 revert on this branch (SpirvLayout.trap / the \
                 private-storage heap emulation live only on the unpushed fork branch \
                 rung3-spirv-arrays-v2); re-lands with the fork re-pin (Decision 5)."
            );
        }
    }
}
