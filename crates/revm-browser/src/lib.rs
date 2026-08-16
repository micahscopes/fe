//! A generic browser boundary for executing Fe-generated EVM bytecode.
//!
//! This crate deliberately knows nothing about Fe contracts, ABIs, proofs, or
//! application state. It accepts raw EVM runtime bytecode and raw calldata,
//! then returns the exact status, output bytes, and gas use reported by revm.

use revm::{
    bytecode::Bytecode,
    context::{
        Context, TxEnv,
        result::{ExecutionResult, Output},
    },
    database::InMemoryDB,
    handler::{ExecuteCommitEvm, MainBuilder, MainContext, MainnetContext, MainnetEvm},
    primitives::{Address, Bytes, U256},
    state::AccountInfo,
};
use wasm_bindgen::prelude::*;

const RUNTIME_ADDRESS: Address = Address::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff,
]);
const DEFAULT_GAS_LIMIT: u64 = 1_000_000_000;
const TEST_CONTRACT_CODE_SIZE_LIMIT: usize = 1024 * 1024;
const TEST_CONTRACT_INITCODE_SIZE_LIMIT: usize = 2 * TEST_CONTRACT_CODE_SIZE_LIMIT;

/// Stable status codes crossing the Wasm boundary.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevmStatus {
    Success = 0,
    Revert = 1,
    Halt = 2,
    EngineError = 3,
}

/// Raw result of one EVM call.
#[wasm_bindgen]
pub struct RevmCallOutcome {
    status: RevmStatus,
    output: Vec<u8>,
    gas_used: u64,
    detail: String,
}

#[wasm_bindgen]
impl RevmCallOutcome {
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> RevmStatus {
        self.status
    }

    #[wasm_bindgen(getter)]
    pub fn output(&self) -> Vec<u8> {
        self.output.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn gas_used(&self) -> u64 {
        self.gas_used
    }

    #[wasm_bindgen(getter)]
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
}

impl RevmCallOutcome {
    fn success(output: Vec<u8>, gas_used: u64) -> Self {
        Self {
            status: RevmStatus::Success,
            output,
            gas_used,
            detail: String::new(),
        }
    }

    fn revert(output: Vec<u8>, gas_used: u64) -> Self {
        Self {
            status: RevmStatus::Revert,
            output,
            gas_used,
            detail: String::new(),
        }
    }

    fn halt(gas_used: u64, detail: String) -> Self {
        Self {
            status: RevmStatus::Halt,
            output: Vec::new(),
            gas_used,
            detail,
        }
    }

    fn engine_error(detail: String) -> Self {
        Self {
            status: RevmStatus::EngineError,
            output: Vec::new(),
            gas_used: 0,
            detail,
        }
    }
}

/// A persistent EVM session backed by revm's in-memory database.
///
/// Successive calls share contract storage. The fixed zero caller and monotonically
/// increasing nonce make browser and native executions deterministic.
#[wasm_bindgen]
pub struct RevmSession {
    evm: MainnetEvm<MainnetContext<InMemoryDB>>,
    next_nonce: u64,
}

#[wasm_bindgen]
impl RevmSession {
    #[wasm_bindgen(constructor)]
    pub fn new(runtime_bytecode: &[u8]) -> RevmSession {
        let bytecode = Bytecode::new_raw(Bytes::copy_from_slice(runtime_bytecode));
        let code_hash = bytecode.hash_slow();
        let mut db = InMemoryDB::default();
        db.insert_account_info(
            RUNTIME_ADDRESS,
            AccountInfo::new(U256::ZERO, 0, code_hash, bytecode),
        );

        let context = Context::mainnet().with_db(db).modify_cfg_chained(|cfg| {
            cfg.limit_contract_code_size = Some(TEST_CONTRACT_CODE_SIZE_LIMIT);
            cfg.limit_contract_initcode_size = Some(TEST_CONTRACT_INITCODE_SIZE_LIMIT);
        });

        RevmSession {
            evm: context.build_mainnet(),
            next_nonce: 0,
        }
    }

    /// Execute one raw call while preserving storage for later calls.
    pub fn call(&mut self, calldata: &[u8]) -> RevmCallOutcome {
        let nonce = self.next_nonce;
        self.next_nonce += 1;

        let transaction = match TxEnv::builder()
            .caller(Address::ZERO)
            .gas_limit(DEFAULT_GAS_LIMIT)
            .gas_price(0)
            .to(RUNTIME_ADDRESS)
            .value(U256::ZERO)
            .data(Bytes::copy_from_slice(calldata))
            .nonce(nonce)
            .build()
        {
            Ok(transaction) => transaction,
            Err(error) => return RevmCallOutcome::engine_error(format!("{error:?}")),
        };

        match self.evm.transact_commit(transaction) {
            Ok(ExecutionResult::Success {
                output: Output::Call(output),
                gas_used,
                ..
            }) => RevmCallOutcome::success(output.to_vec(), gas_used),
            Ok(ExecutionResult::Success {
                output: Output::Create(..),
                gas_used,
                ..
            }) => RevmCallOutcome::engine_error(format!(
                "runtime call unexpectedly produced create output after {gas_used} gas"
            )),
            Ok(ExecutionResult::Revert {
                output, gas_used, ..
            }) => RevmCallOutcome::revert(output.to_vec(), gas_used),
            Ok(ExecutionResult::Halt { reason, gas_used }) => {
                RevmCallOutcome::halt(gas_used, format!("{reason:?}"))
            }
            Err(error) => RevmCallOutcome::engine_error(error.to_string()),
        }
    }
}
