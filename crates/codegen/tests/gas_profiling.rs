mod test_helpers;
use test_helpers::*;

use fe_codegen::gas_profile::format_gas_profile;

const TOKEN_CONTRACT: &str = r#"
msg Msg {
    #[selector = 0x11111111]
    Transfer { to: Address, amount: u256 } -> bool,
    #[selector = 0x22222222]
    Balance { account: Address } -> u256,
}

struct Store {
    balances: StorageMap<Address, u256>,
}

pub contract Token uses (ctx: Ctx) {
    mut store: Store

    recv Msg {
        Transfer { to, amount } -> bool uses (mut store, ctx) {
            let sender = ctx.caller()
            let bal = store.balances.get(key: sender)
            if bal < amount {
                return false
            }
            store.balances.set(key: sender, value: bal - amount)
            store.balances.set(key: to, value: store.balances.get(key: to) + amount)
            return true
        }

        Balance { account } -> u256 uses store {
            store.balances.get(key: account)
        }
    }
}
"#;

#[test]
fn gas_profile_erc20_balance() {
    let a = analyze(TOKEN_CONTRACT);

    let mut calldata = vec![0x22, 0x22, 0x22, 0x22];
    calldata.extend_from_slice(&[0u8; 12]);
    calldata.extend_from_slice(&[0u8; 19]);
    calldata.push(0x01);

    let profile = gas_profile(&a, &calldata, 0);
    eprintln!("{}", format_gas_profile(&profile));

    assert!(profile.tx_gas > 20_000, "tx gas should include base cost");
    assert!(profile.total_gas > 2000, "opcode gas should include SLOAD");
    assert!(profile.total_steps > 50);

    let mapped_pct = (profile.total_gas - profile.unmapped_gas) as f64
        / profile.total_gas as f64 * 100.0;
    assert!(mapped_pct > 80.0, "should map >80% at O1, got {mapped_pct:.1}%");
}

#[test]
fn gas_profile_deposit_contract() {
    let source = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../fe/tests/fixtures/fe_test/deposit_contract.fe")
    ).expect("read deposit contract");

    let a = analyze(&source);

    fn mk_bytes(seed: u8, len: usize) -> Vec<u8> {
        (0..len).map(|i| seed.wrapping_add(i as u8)).collect()
    }

    let pubkey = mk_bytes(0x01, 48);
    let withdrawal_creds = mk_bytes(0xaa, 32);
    let signature = mk_bytes(0x11, 96);

    use sha2::{Digest, Sha256};

    fn sha256_pair(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(a);
        h.update(b);
        h.finalize().into()
    }

    // SSZ deposit_data_root — exact logic from differential_deposit.rs
    let pubkey_arr: [u8; 48] = pubkey.clone().try_into().unwrap();
    let wc_arr: [u8; 32] = withdrawal_creds.clone().try_into().unwrap();
    let sig_arr: [u8; 96] = signature.clone().try_into().unwrap();
    let one_eth_gwei: u64 = 1_000_000_000;

    let mut pk_pad = [0u8; 64];
    pk_pad[..48].copy_from_slice(&pubkey_arr);
    let pubkey_root: [u8; 32] = Sha256::digest(pk_pad).into();

    let mut sig_tail_pad = [0u8; 64];
    sig_tail_pad[..32].copy_from_slice(&sig_arr[64..96]);
    let sig_tail: [u8; 32] = Sha256::digest(sig_tail_pad).into();
    let sig_head: [u8; 32] = Sha256::digest(&sig_arr[..64]).into();
    let sig_root = sha256_pair(&sig_head, &sig_tail);

    let mut amount_sig_pad = [0u8; 64];
    amount_sig_pad[..8].copy_from_slice(&one_eth_gwei.to_le_bytes());
    amount_sig_pad[32..64].copy_from_slice(&sig_root);
    let right: [u8; 32] = Sha256::digest(amount_sig_pad).into();

    let left = sha256_pair(&pubkey_root, &wc_arr);
    let deposit_data_root = sha256_pair(&left, &right);

    let function = ethers_core::abi::AbiParser::default()
        .parse_function("deposit(bytes,bytes,bytes,bytes32)")
        .expect("parse");
    let calldata = function.encode_input(&[
        ethers_core::abi::Token::Bytes(pubkey),
        ethers_core::abi::Token::Bytes(withdrawal_creds),
        ethers_core::abi::Token::Bytes(signature),
        ethers_core::abi::Token::FixedBytes(deposit_data_root.to_vec()),
    ]).expect("encode");

    // Debug: check what sections exist
    let package = a.package();
    let (artifacts, _) = fe_codegen::compile_with_frontend_provenance(
        &a.db, &package, fe_codegen::EVM_LAYOUT, fe_codegen::OptLevel::O1,
    ).expect("compile");
    for art in &artifacts {
        for (name, sec) in &art.sections {
            eprintln!("  section: {} ({} bytes)", name.0, sec.bytes.len());
        }
    }

    let one_eth: u128 = 1_000_000_000_000_000_000;

    // Also do a plain call to compare gas with Christoph's numbers
    {
        let init_hex = artifacts.iter()
            .flat_map(|art| art.sections.iter())
            .find(|(name, _)| name.0 == "init")
            .map(|(_, s)| hex::encode(&s.bytes))
            .expect("init");
        let mut inst = fe_contract_harness::RuntimeInstance::deploy(&init_hex).expect("deploy");
        inst.fund_account(fe_contract_harness::Address::ZERO, fe_contract_harness::U256::from(u128::MAX / 2));
        let mut opts = fe_contract_harness::ExecutionOptions::default();
        opts.value = fe_contract_harness::U256::from(one_eth);
        let result = inst.call_raw(&calldata, opts).expect("deposit call");
        eprintln!("Direct call gas_used: {}", result.gas_used);
    }

    let profile = gas_profile(&a, &calldata, one_eth);
    eprintln!("{}", format_gas_profile(&profile));

    assert!(profile.tx_gas > 25_000,
        "deposit tx should cost >25k total gas, got {}", profile.tx_gas);
    assert!(profile.total_steps > 500,
        "deposit should execute >500 opcode steps, got {}", profile.total_steps);
}
