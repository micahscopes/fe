pub(crate) mod code_region;
pub mod ir;
pub mod layout_utils;
pub mod lower;
pub(crate) mod package;
pub mod pretty;
pub(crate) mod root_effects;
pub mod stable_key;
pub(crate) mod synthetic;
pub mod transform;

pub use ir::*;
pub use layout_utils::*;
pub use lower::*;
pub use package::{
    LowerError, build_runtime_package, build_test_runtime_package, build_wasm_runtime_package,
    runtime_instance_stable_key, runtime_instance_symbol_key,
};
pub use pretty::{
    format_runtime_body, format_runtime_body_excerpt, format_runtime_package,
    format_runtime_verify_failure,
};
pub use transform::{
    RuntimeAggregateFacts, RuntimeArgFact, RuntimeArgShapeKey, RuntimeScalarConstFacts,
    runtime_arg_shape_key, specialize_pure_inline_stmts,
};
