use common::ingot::IngotKind;

use crate::{
    analysis::{
        HirAnalysisDb,
        name_resolution::{NameDomain, PathRes, resolve_ident_to_bucket, resolve_path},
        ty::{
            trait_resolution::PredicateListId,
            ty_def::{TyBase, TyData, TyId},
        },
    },
    hir_def::{
        ArithBinOp, BinOp, CallableDef, CompBinOp, Func, IdentId, ItemKind, PathId, Trait, UnOp,
        scope_graph::ScopeId,
    },
};

/// Resolve a trait in the core library by an explicit trait path, excluding the "core" root segment.
pub fn resolve_core_trait<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    segments: &[&str],
) -> Option<Trait<'db>> {
    let (module_segments, [trait_name]) = segments.split_last_chunk::<1>()?;
    let mut module_path = lib_root_path(db, scope, "core");

    for segment in module_segments {
        module_path = module_path.push_str(db, segment);
    }

    let assumptions = PredicateListId::empty_list(db);
    let Ok(PathRes::Mod(module_scope)) = resolve_path(db, module_path, scope, assumptions, false)
    else {
        return None;
    };

    let trait_name = IdentId::new(db, trait_name.to_string());
    let bucket = resolve_ident_to_bucket(db, PathId::from_ident(db, trait_name), module_scope);
    bucket.pick(NameDomain::TYPE).as_ref().ok()?.trait_()
}

#[salsa::interned]
#[derive(Debug)]
pub struct LibPath<'db> {
    #[return_ref]
    pub string: String,
}

/// Resolve a type by a fully-qualified `core::...` or `std::...` path string.
///
/// This is a cached wrapper around `resolve_path` intended for backend consumers (e.g. MIR)
/// that need stable access to a small set of core/std helper types.
pub fn resolve_lib_type_path<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    path: &str,
) -> Option<TyId<'db>> {
    let path_id = LibPath::new(db, path.to_string());
    resolve_lib_path(db, scope, path_id)
}

/// Resolve a function by a fully-qualified `core::...` or `std::...` path string.
///
/// Returns the `Func` HIR item for the resolved function.
pub fn resolve_lib_func_path<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    path: &str,
) -> Option<crate::hir_def::Func<'db>> {
    let path_id = LibPath::new(db, path.to_string());
    resolve_lib_func(db, scope, path_id)
}

/// Returns `true` if `func` is the library function at the fully-qualified
/// `core::...` or `std::...` path.
///
/// This resolves from the owning ingot root instead of an arbitrary caller or
/// nested module scope, so backend consumers can classify already-resolved
/// library callees without reintroducing lookup drift.
pub fn lib_func_matches<'db>(db: &'db dyn HirAnalysisDb, func: Func<'db>, path: &str) -> bool {
    let Some((root, target_suffix)) = path.split_once("::") else {
        return false;
    };
    let expected_kind = match root {
        "core" => IngotKind::Core,
        "std" => IngotKind::Std,
        _ => return false,
    };
    if func.top_mod(db).ingot(db).kind(db) != expected_kind {
        return false;
    }
    let Some(actual_path) = func.scope().pretty_path(db) else {
        return false;
    };
    let Some((_, actual_suffix)) = actual_path.split_once("::") else {
        return false;
    };
    actual_suffix == target_suffix
}

/// Returns `true` if `trait_` is the library trait at the fully-qualified path.
pub fn lib_trait_matches<'db>(db: &'db dyn HirAnalysisDb, trait_: Trait<'db>, path: &str) -> bool {
    let Some((root, target_suffix)) = path.split_once("::") else {
        return false;
    };
    let expected_kind = match root {
        "core" => IngotKind::Core,
        "std" => IngotKind::Std,
        _ => return false,
    };
    if trait_.top_mod(db).ingot(db).kind(db) != expected_kind {
        return false;
    }
    let Some(actual_path) = trait_.scope().pretty_path(db) else {
        return false;
    };
    let Some((_, actual_suffix)) = actual_path.split_once("::") else {
        return false;
    };
    actual_suffix == target_suffix
}

/// Identity of the generic numeric intrinsic declarations. The declaration
/// vocabulary is shared by CTFE and runtime lowering, not target policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum GenericNumericIntrinsicKind {
    Bitcast,
    IntTruncate,
    SaturatingAdd,
    SaturatingSub,
    SaturatingMul,
    CheckedBinary(ArithBinOp),
    CheckedNeg,
}

impl GenericNumericIntrinsicKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "__bitcast" => Self::Bitcast,
            "__int_truncate" => Self::IntTruncate,
            "__saturating_add" => Self::SaturatingAdd,
            "__saturating_sub" => Self::SaturatingSub,
            "__saturating_mul" => Self::SaturatingMul,
            "__checked_add" => Self::CheckedBinary(ArithBinOp::Add),
            "__checked_sub" => Self::CheckedBinary(ArithBinOp::Sub),
            "__checked_mul" => Self::CheckedBinary(ArithBinOp::Mul),
            "__checked_div" => Self::CheckedBinary(ArithBinOp::Div),
            "__checked_rem" => Self::CheckedBinary(ArithBinOp::Rem),
            "__checked_pow" => Self::CheckedBinary(ArithBinOp::Pow),
            "__checked_neg" => Self::CheckedNeg,
            _ => return None,
        })
    }
}

#[salsa::tracked]
pub fn generic_numeric_intrinsic_func_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Option<GenericNumericIntrinsicKind> {
    if func.body(db).is_some() {
        return None;
    }
    GenericNumericIntrinsicKind::from_name(func.name(db).to_opt()?.data(db).as_str())
}

/// Operation identity for the scalar-suffixed numeric declaration vocabulary.
/// Target legality and declaration type checking remain separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum ScalarNumericIntrinsicOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Shl,
    Shr,
    Bitand,
    Bitor,
    Bitxor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Bitnot,
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub struct ScalarNumericIntrinsic {
    pub op: ScalarNumericIntrinsicOp,
    pub scalar: crate::hir_def::prim_ty::PrimTy,
}

impl ScalarNumericIntrinsic {
    pub fn from_name(name: &str) -> Option<Self> {
        use crate::hir_def::prim_ty::{FloatTy, IntTy, PrimTy, UintTy};
        let (op, scalar) = name.strip_prefix("__")?.rsplit_once('_')?;
        let op = match op {
            "add" => ScalarNumericIntrinsicOp::Add,
            "sub" => ScalarNumericIntrinsicOp::Sub,
            "mul" => ScalarNumericIntrinsicOp::Mul,
            "div" => ScalarNumericIntrinsicOp::Div,
            "rem" => ScalarNumericIntrinsicOp::Rem,
            "pow" => ScalarNumericIntrinsicOp::Pow,
            "shl" => ScalarNumericIntrinsicOp::Shl,
            "shr" => ScalarNumericIntrinsicOp::Shr,
            "bitand" => ScalarNumericIntrinsicOp::Bitand,
            "bitor" => ScalarNumericIntrinsicOp::Bitor,
            "bitxor" => ScalarNumericIntrinsicOp::Bitxor,
            "eq" => ScalarNumericIntrinsicOp::Eq,
            "ne" => ScalarNumericIntrinsicOp::Ne,
            "lt" => ScalarNumericIntrinsicOp::Lt,
            "le" => ScalarNumericIntrinsicOp::Le,
            "gt" => ScalarNumericIntrinsicOp::Gt,
            "ge" => ScalarNumericIntrinsicOp::Ge,
            "bitnot" => ScalarNumericIntrinsicOp::Bitnot,
            "not" => ScalarNumericIntrinsicOp::Not,
            "neg" => ScalarNumericIntrinsicOp::Neg,
            _ => return None,
        };
        let scalar = match scalar {
            "u8" => PrimTy::Uint(UintTy::U8),
            "u16" => PrimTy::Uint(UintTy::U16),
            "u32" => PrimTy::Uint(UintTy::U32),
            "u64" => PrimTy::Uint(UintTy::U64),
            "u128" => PrimTy::Uint(UintTy::U128),
            "u256" => PrimTy::Uint(UintTy::U256),
            "usize" => PrimTy::Uint(UintTy::Usize),
            "i8" => PrimTy::Int(IntTy::I8),
            "i16" => PrimTy::Int(IntTy::I16),
            "i32" => PrimTy::Int(IntTy::I32),
            "i64" => PrimTy::Int(IntTy::I64),
            "i128" => PrimTy::Int(IntTy::I128),
            "i256" => PrimTy::Int(IntTy::I256),
            "isize" => PrimTy::Int(IntTy::Isize),
            "bool" => PrimTy::Bool,
            "f32" => PrimTy::Float(FloatTy::F32),
            _ => return None,
        };
        Some(Self { op, scalar })
    }
}

#[salsa::tracked]
pub fn scalar_numeric_intrinsic_func_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Option<ScalarNumericIntrinsic> {
    if func.body(db).is_some() {
        return None;
    }
    ScalarNumericIntrinsic::from_name(func.name(db).to_opt()?.data(db).as_str())
}

/// Identity of the existing bodyless floating-point intrinsic declarations.
/// Recognition lives here; evaluation and target support remain separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum F32IntrinsicKind {
    FromI32,
    ToI32,
    Sqrt,
    Rsqrt,
    Abs,
    Min,
    Max,
    MinRelaxed,
    MaxRelaxed,
    Clamp,
    Floor,
    Ceil,
    Trunc,
    Round,
}

impl F32IntrinsicKind {
    /// Decode the current declaration vocabulary, not an arbitrary call target.
    /// Callers with a function identity should use `f32_intrinsic_func_kind`.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "__f32_from_i32" => Self::FromI32,
            "__i32_from_f32" => Self::ToI32,
            "__sqrt_f32" => Self::Sqrt,
            "__rsqrt_f32" => Self::Rsqrt,
            "__abs_f32" => Self::Abs,
            "__min_f32" => Self::Min,
            "__max_f32" => Self::Max,
            "__min_relaxed_f32" => Self::MinRelaxed,
            "__max_relaxed_f32" => Self::MaxRelaxed,
            "__clamp_f32" => Self::Clamp,
            "__floor_f32" => Self::Floor,
            "__ceil_f32" => Self::Ceil,
            "__trunc_f32" => Self::Trunc,
            "__round_f32" => Self::Round,
            _ => return None,
        })
    }
}

#[salsa::tracked]
pub fn f32_intrinsic_func_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Option<F32IntrinsicKind> {
    if func.body(db).is_some() {
        return None;
    }
    F32IntrinsicKind::from_name(func.name(db).to_opt()?.data(db).as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum RuntimeBuiltinFuncKind {
    Malloc,
    Mload,
    Mstore,
    Mstore8,
    Mcopy,
    Msize,
    Sload,
    Sstore,
    CallDataLoad,
    CallDataCopy,
    CallDataSize,
    ReturnDataCopy,
    ReturnDataSize,
    CodeCopy,
    CodeSize,
    ExtCodeCopy,
    ExtCodeSize,
    ExtCodeHash,
    Keccak256,
    AddMod,
    MulMod,
    Byte,
    SignExtend,
    Address,
    Caller,
    CallValue,
    Origin,
    GasPrice,
    CoinBase,
    Balance,
    Timestamp,
    Number,
    PrevRandao,
    GasLimit,
    ChainId,
    BaseFee,
    SelfBalance,
    BlockHash,
    BlobHash,
    BlobBaseFee,
    Gas,
    Call,
    StaticCall,
    DelegateCall,
    Create,
    Create2,
    Log0,
    Log1,
    Log2,
    Log3,
    Log4,
    Revert,
    ReturnData,
    SelfDestruct,
    Stop,
    Panic,
    PanicWithValue,
    Todo,
    IntrinsicKeccak256,
}

/// Nominal control operations which remain ordinary typed Fe functions through
/// semantic analysis but are consumed by resumable MIR materialization before
/// backend import lowering. Kept separate from EVM/runtime builtins: these
/// operations alter control flow and require continuation planning rather than
/// producing one target instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum RuntimeControlEffectFuncKind {
    Suspend,
}

/// Nominal actor-scope operations which remain ordinary host imports but need
/// compiler validation against the resident actor contract. Kept distinct
/// from [`RuntimeControlEffectFuncKind`]: these operations do not split control
/// flow themselves; their returned `Pending` value may subsequently be passed
/// to the ordinary suspension operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum RuntimeActorEffectFuncKind {
    SendBegin,
    AskBegin,
    ChildSpawnBegin,
    ChildFailureBegin,
    ChildClose,
}

#[salsa::tracked]
pub fn runtime_actor_effect_func_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Option<RuntimeActorEffectFuncKind> {
    let kind = func.top_mod(db).ingot(db).kind(db);
    let path = runtime_builtin_func_path(db, func)?;
    match (kind, path.as_slice()) {
        (IngotKind::Std, ["actor", "raw", "send_begin"]) => {
            Some(RuntimeActorEffectFuncKind::SendBegin)
        }
        (IngotKind::Std, ["runtime", "raw", "ask_begin"]) => {
            Some(RuntimeActorEffectFuncKind::AskBegin)
        }
        (IngotKind::Std, ["runtime", "raw", "spawn_begin"]) => {
            Some(RuntimeActorEffectFuncKind::ChildSpawnBegin)
        }
        (IngotKind::Std, ["runtime", "raw", "failure_begin"]) => {
            Some(RuntimeActorEffectFuncKind::ChildFailureBegin)
        }
        (IngotKind::Std, ["runtime", "raw", "close"]) => {
            Some(RuntimeActorEffectFuncKind::ChildClose)
        }
        _ => None,
    }
}

#[salsa::tracked]
pub fn runtime_control_effect_func_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Option<RuntimeControlEffectFuncKind> {
    let kind = func.top_mod(db).ingot(db).kind(db);
    let path = runtime_builtin_func_path(db, func)?;
    match (kind, path.as_slice()) {
        (IngotKind::Std, ["host", "raw", "suspend"]) => Some(RuntimeControlEffectFuncKind::Suspend),
        _ => None,
    }
}

#[salsa::tracked]
pub fn runtime_builtin_func_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Option<RuntimeBuiltinFuncKind> {
    let kind = func.top_mod(db).ingot(db).kind(db);
    let path = runtime_builtin_func_path(db, func)?;
    Some(match (kind, path.as_slice()) {
        (IngotKind::Core, ["effect_ref", "alloc_bytes"]) => RuntimeBuiltinFuncKind::Malloc,
        (IngotKind::Core, ["browser", "alloc_browser_bytes"]) => RuntimeBuiltinFuncKind::Malloc,
        (IngotKind::Core, ["browser", "alloc_browser_object"]) => RuntimeBuiltinFuncKind::Malloc,
        (IngotKind::Std, ["evm", "mem", "alloc"]) => RuntimeBuiltinFuncKind::Malloc,
        (IngotKind::Std, ["evm", "ops", "mload"]) => RuntimeBuiltinFuncKind::Mload,
        (IngotKind::Std, ["evm", "ops", "mstore"]) => RuntimeBuiltinFuncKind::Mstore,
        (IngotKind::Std, ["evm", "ops", "mstore8"]) => RuntimeBuiltinFuncKind::Mstore8,
        (IngotKind::Core, ["abi", "mcopy"]) => RuntimeBuiltinFuncKind::Mcopy,
        (IngotKind::Std, ["evm", "ops", "msize"]) => RuntimeBuiltinFuncKind::Msize,
        (IngotKind::Std, ["evm", "ops", "sload"]) => RuntimeBuiltinFuncKind::Sload,
        (IngotKind::Std, ["evm", "ops", "sstore"]) => RuntimeBuiltinFuncKind::Sstore,
        (IngotKind::Std, ["evm", "ops", "calldataload"]) => RuntimeBuiltinFuncKind::CallDataLoad,
        (IngotKind::Std, ["evm", "ops", "calldatacopy"]) => RuntimeBuiltinFuncKind::CallDataCopy,
        (IngotKind::Std, ["evm", "ops", "calldatasize"]) => RuntimeBuiltinFuncKind::CallDataSize,
        (IngotKind::Std, ["evm", "ops", "returndatacopy"]) => {
            RuntimeBuiltinFuncKind::ReturnDataCopy
        }
        (IngotKind::Std, ["evm", "ops", "returndatasize"]) => {
            RuntimeBuiltinFuncKind::ReturnDataSize
        }
        (IngotKind::Std, ["evm", "ops", "codecopy"]) => RuntimeBuiltinFuncKind::CodeCopy,
        (IngotKind::Std, ["evm", "ops", "codesize"]) => RuntimeBuiltinFuncKind::CodeSize,
        (IngotKind::Std, ["evm", "ops", "extcodecopy"]) => RuntimeBuiltinFuncKind::ExtCodeCopy,
        (IngotKind::Std, ["evm", "ops", "extcodesize"]) => RuntimeBuiltinFuncKind::ExtCodeSize,
        (IngotKind::Std, ["evm", "ops", "extcodehash"]) => RuntimeBuiltinFuncKind::ExtCodeHash,
        (IngotKind::Std, ["evm", "ops", "keccak256"]) => RuntimeBuiltinFuncKind::Keccak256,
        (IngotKind::Core, ["num", "__addmod_u256"]) => RuntimeBuiltinFuncKind::AddMod,
        (IngotKind::Core, ["num", "__mulmod_u256"]) => RuntimeBuiltinFuncKind::MulMod,
        (IngotKind::Std, ["evm", "ops", "addmod"]) => RuntimeBuiltinFuncKind::AddMod,
        (IngotKind::Std, ["evm", "ops", "mulmod"]) => RuntimeBuiltinFuncKind::MulMod,
        (IngotKind::Std, ["evm", "ops", "byte"]) => RuntimeBuiltinFuncKind::Byte,
        (IngotKind::Std, ["evm", "ops", "signextend"]) => RuntimeBuiltinFuncKind::SignExtend,
        (IngotKind::Std, ["evm", "ops", "address"]) => RuntimeBuiltinFuncKind::Address,
        (IngotKind::Std, ["evm", "ops", "caller"]) => RuntimeBuiltinFuncKind::Caller,
        (IngotKind::Std, ["evm", "ops", "callvalue"]) => RuntimeBuiltinFuncKind::CallValue,
        (IngotKind::Std, ["evm", "ops", "origin"]) => RuntimeBuiltinFuncKind::Origin,
        (IngotKind::Std, ["evm", "ops", "gasprice"]) => RuntimeBuiltinFuncKind::GasPrice,
        (IngotKind::Std, ["evm", "ops", "coinbase"]) => RuntimeBuiltinFuncKind::CoinBase,
        (IngotKind::Std, ["evm", "ops", "balance"]) => RuntimeBuiltinFuncKind::Balance,
        (IngotKind::Std, ["evm", "ops", "timestamp"]) => RuntimeBuiltinFuncKind::Timestamp,
        (IngotKind::Std, ["evm", "ops", "number"]) => RuntimeBuiltinFuncKind::Number,
        (IngotKind::Std, ["evm", "ops", "prevrandao"]) => RuntimeBuiltinFuncKind::PrevRandao,
        (IngotKind::Std, ["evm", "ops", "gaslimit"]) => RuntimeBuiltinFuncKind::GasLimit,
        (IngotKind::Std, ["evm", "ops", "chainid"]) => RuntimeBuiltinFuncKind::ChainId,
        (IngotKind::Std, ["evm", "ops", "basefee"]) => RuntimeBuiltinFuncKind::BaseFee,
        (IngotKind::Std, ["evm", "ops", "selfbalance"]) => RuntimeBuiltinFuncKind::SelfBalance,
        (IngotKind::Std, ["evm", "ops", "blockhash"]) => RuntimeBuiltinFuncKind::BlockHash,
        (IngotKind::Std, ["evm", "ops", "blobhash"]) => RuntimeBuiltinFuncKind::BlobHash,
        (IngotKind::Std, ["evm", "ops", "blobbasefee"]) => RuntimeBuiltinFuncKind::BlobBaseFee,
        (IngotKind::Std, ["evm", "ops", "gas"]) => RuntimeBuiltinFuncKind::Gas,
        (IngotKind::Std, ["evm", "ops", "call"]) => RuntimeBuiltinFuncKind::Call,
        (IngotKind::Std, ["evm", "ops", "staticcall"]) => RuntimeBuiltinFuncKind::StaticCall,
        (IngotKind::Std, ["evm", "ops", "delegatecall"]) => RuntimeBuiltinFuncKind::DelegateCall,
        (IngotKind::Std, ["evm", "ops", "create"]) => RuntimeBuiltinFuncKind::Create,
        (IngotKind::Std, ["evm", "ops", "create2"]) => RuntimeBuiltinFuncKind::Create2,
        (IngotKind::Std, ["evm", "ops", "log0"]) => RuntimeBuiltinFuncKind::Log0,
        (IngotKind::Std, ["evm", "ops", "log1"]) => RuntimeBuiltinFuncKind::Log1,
        (IngotKind::Std, ["evm", "ops", "log2"]) => RuntimeBuiltinFuncKind::Log2,
        (IngotKind::Std, ["evm", "ops", "log3"]) => RuntimeBuiltinFuncKind::Log3,
        (IngotKind::Std, ["evm", "ops", "log4"]) => RuntimeBuiltinFuncKind::Log4,
        (IngotKind::Std, ["evm", "ops", "revert"]) => RuntimeBuiltinFuncKind::Revert,
        (IngotKind::Std, ["evm", "ops", "return_data"]) => RuntimeBuiltinFuncKind::ReturnData,
        (IngotKind::Std, ["evm", "ops", "selfdestruct"]) => RuntimeBuiltinFuncKind::SelfDestruct,
        (IngotKind::Std, ["evm", "ops", "stop"]) => RuntimeBuiltinFuncKind::Stop,
        (IngotKind::Core, ["panic"]) => RuntimeBuiltinFuncKind::Panic,
        (IngotKind::Core, ["panic_with_value"]) => RuntimeBuiltinFuncKind::PanicWithValue,
        (IngotKind::Core, ["todo"]) => RuntimeBuiltinFuncKind::Todo,
        (IngotKind::Core, ["intrinsic", "__keccak256"]) => {
            RuntimeBuiltinFuncKind::IntrinsicKeccak256
        }
        _ => return None,
    })
}

fn runtime_builtin_func_path<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Option<Vec<&'db str>> {
    let mut segments = Vec::new();
    let mut scope = Some(func.scope());
    while let Some(current) = scope {
        let name = current.name(db)?;
        segments.push(name.data(db).as_str());
        scope = current.parent(db);
    }
    segments.reverse();
    if segments.first() == Some(&"lib") {
        segments.remove(0);
    }
    Some(segments)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveWrapperCallKind {
    Unary(UnOp),
    Binary(BinOp),
    Assign(BinOp),
}

pub fn core_primitive_wrapper_call_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    result_ty: TyId<'db>,
) -> Option<PrimitiveWrapperCallKind> {
    if func.top_mod(db).ingot(db).kind(db) != IngotKind::Core {
        return None;
    }
    let Some(ItemKind::ImplTrait(impl_trait)) = func.scope().parent_item(db) else {
        return None;
    };
    let method = func.name(db).to_opt()?.data(db);
    let matches_trait = |segments: &[&str]| {
        impl_trait.trait_def(db) == resolve_core_trait(db, func.scope(), segments)
    };
    Some(if method == "add" && matches_trait(&["ops", "Add"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::Add))
    } else if method == "sub" && matches_trait(&["ops", "Sub"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::Sub))
    } else if method == "mul" && matches_trait(&["ops", "Mul"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::Mul))
    } else if method == "div" && matches_trait(&["ops", "Div"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::Div))
    } else if method == "rem" && matches_trait(&["ops", "Rem"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::Rem))
    } else if method == "pow" && matches_trait(&["ops", "Pow"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::Pow))
    } else if method == "shl" && matches_trait(&["ops", "Shl"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::LShift))
    } else if method == "shr" && matches_trait(&["ops", "Shr"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::RShift))
    } else if method == "bitand" && matches_trait(&["ops", "BitAnd"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::BitAnd))
    } else if method == "bitor" && matches_trait(&["ops", "BitOr"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::BitOr))
    } else if method == "bitxor" && matches_trait(&["ops", "BitXor"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Arith(ArithBinOp::BitXor))
    } else if method == "eq" && matches_trait(&["ops", "Eq"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Comp(CompBinOp::Eq))
    } else if method == "ne" && matches_trait(&["ops", "Eq"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Comp(CompBinOp::NotEq))
    } else if method == "lt" && matches_trait(&["ops", "Ord"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Comp(CompBinOp::Lt))
    } else if method == "le" && matches_trait(&["ops", "Ord"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Comp(CompBinOp::LtEq))
    } else if method == "gt" && matches_trait(&["ops", "Ord"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Comp(CompBinOp::Gt))
    } else if method == "ge" && matches_trait(&["ops", "Ord"]) {
        PrimitiveWrapperCallKind::Binary(BinOp::Comp(CompBinOp::GtEq))
    } else if method == "neg" && matches_trait(&["ops", "Neg"]) {
        PrimitiveWrapperCallKind::Unary(UnOp::Minus)
    } else if method == "bit_not" && matches_trait(&["ops", "BitNot"]) {
        PrimitiveWrapperCallKind::Unary(UnOp::BitNot)
    } else if method == "not" && matches_trait(&["ops", "Not"]) {
        PrimitiveWrapperCallKind::Unary(if result_ty == TyId::bool(db) {
            UnOp::Not
        } else {
            UnOp::BitNot
        })
    } else if method == "add_assign" && matches_trait(&["ops", "AddAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::Add))
    } else if method == "sub_assign" && matches_trait(&["ops", "SubAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::Sub))
    } else if method == "mul_assign" && matches_trait(&["ops", "MulAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::Mul))
    } else if method == "div_assign" && matches_trait(&["ops", "DivAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::Div))
    } else if method == "rem_assign" && matches_trait(&["ops", "RemAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::Rem))
    } else if method == "pow_assign" && matches_trait(&["ops", "PowAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::Pow))
    } else if method == "shl_assign" && matches_trait(&["ops", "ShlAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::LShift))
    } else if method == "shr_assign" && matches_trait(&["ops", "ShrAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::RShift))
    } else if method == "bitand_assign" && matches_trait(&["ops", "BitAndAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::BitAnd))
    } else if method == "bitor_assign" && matches_trait(&["ops", "BitOrAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::BitOr))
    } else if method == "bitxor_assign" && matches_trait(&["ops", "BitXorAssign"]) {
        PrimitiveWrapperCallKind::Assign(BinOp::Arith(ArithBinOp::BitXor))
    } else {
        return None;
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreRangeTypes<'db> {
    pub range: TyId<'db>,
    pub known: TyId<'db>,
    pub unknown: TyId<'db>,
}

pub fn resolve_core_range_types<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
) -> Option<CoreRangeTypes<'db>> {
    let range = resolve_lib_type_path(db, scope, "core::range::Range")?;
    let known = resolve_lib_type_path(db, scope, "core::range::Known")?;
    let unknown = resolve_lib_type_path(db, scope, "core::range::Unknown")?;
    Some(CoreRangeTypes {
        range,
        known,
        unknown,
    })
}

#[salsa::tracked]
fn resolve_lib_path<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    path: LibPath<'db>,
) -> Option<TyId<'db>> {
    let mut segments = path.string(db).split("::");
    let mut path = lib_root_path(db, scope, segments.next()?);

    for segment in segments {
        path = path.push_str(db, segment);
    }

    let assumptions = PredicateListId::empty_list(db);
    match resolve_path(db, path, scope, assumptions, true).ok()? {
        PathRes::Ty(ty) | PathRes::TyAlias(_, ty) => Some(ty),
        _ => None,
    }
}

#[salsa::tracked]
fn resolve_lib_func<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    path: LibPath<'db>,
) -> Option<Func<'db>> {
    let mut segments = path.string(db).split("::");
    let mut path = lib_root_path(db, scope, segments.next()?);

    for segment in segments {
        path = path.push_str(db, segment);
    }

    let assumptions = PredicateListId::empty_list(db);
    match resolve_path(db, path, scope, assumptions, true).ok()? {
        PathRes::Func(ty) => {
            let TyData::TyBase(TyBase::Func(CallableDef::Func(func))) = ty.data(db) else {
                return None;
            };
            Some(*func)
        }
        _ => None,
    }
}

pub(crate) fn lib_root_path<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    root: &str,
) -> PathId<'db> {
    let ingot_kind = scope.top_mod(db).ingot(db).kind(db);
    if (ingot_kind == IngotKind::Core && root == "core")
        || (ingot_kind == IngotKind::Std && root == "std")
    {
        PathId::from_ident(db, IdentId::make_ingot(db))
    } else {
        PathId::from_str(db, root)
    }
}
