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

use ethers_core::abi::Token;
use ethers_core::types::U256 as AbiU256;
use fe_contract_harness::{
    Address, ExecutionOptions, FeContractHarness, RuntimeInstance, U256, bytes_to_u256,
};

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
