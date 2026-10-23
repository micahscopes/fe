//! Object emission for the explicit scalar task ABI.

use driver::DriverDataBase;
use hir::hir_def::TopLevelMod;
use mir::{
    RuntimeInstance,
    runtime::{
        build_native_task_adapter_package,
        suspension::{plan_native_scalar_abi, plan_native_scalar_task},
    },
};
use rustc_hash::FxHashSet;
use sonatina_codegen::{Compile, isa::cranelift::CraneliftObjectBackend};
use sonatina_ir::{
    Linkage, Signature, Type,
    builder::ModuleBuilder,
    func_cursor::InstInserter,
    inst::{
        cast::PtrToInt,
        control_flow::{Call, Return},
    },
    ir_writer::ModuleWriter,
    isa::Isa,
};

use super::{
    LowerError, create_native_isa, ensure_module_sonatina_ir_valid, format_cranelift_errors,
    lower_runtime, to_sonatina_opt_level,
};
use crate::OptLevel;

/// Symbols are ordered start, then one resume per cut. Every symbol has C ABI
/// `(uint64_t *frame, uint64_t kind, uint64_t payload) -> uint64_t next_step`.
/// The object imports `native_task_begin`, supplied by the task runtime.
#[derive(Debug)]
pub struct NativeTaskArtifact {
    pub symbols: Vec<String>,
    pub frame_words: usize,
    pub object: Vec<u8>,
    pub ir: String,
}

pub fn emit_native_scalar_task<'db>(
    db: &'db DriverDataBase,
    top_mod: TopLevelMod<'db>,
    source: RuntimeInstance<'db>,
    opt_level: OptLevel,
) -> Result<NativeTaskArtifact, LowerError> {
    let task = plan_native_scalar_task(db, source)?;
    let plan = plan_native_scalar_abi(db, task)?;
    let package = build_native_task_adapter_package(db, top_mod, plan)?;
    let isa = create_native_isa()?;
    let (module, functions) =
        lower_runtime::compile_runtime_package_sonatina_for_isa(db, &package, &isa, false, None)?;
    let mut names = module
        .funcs()
        .into_iter()
        .map(|function| module.ctx.func_sig(function, |sig| sig.name().to_owned()))
        .collect::<FxHashSet<_>>();
    let builder = ModuleBuilder::from_module(module);
    let pointer = builder.ptr_type(Type::I64);
    let mut symbols = Vec::new();
    for (index, instance) in plan.instances(db).into_iter().enumerate() {
        let callee = functions[&instance];
        builder.ctx.func_sig(callee, |signature| {
            if signature.args() != [Type::I256, Type::I64, Type::I64]
                || signature.ret_tys() != [Type::I64]
            {
                return Err(LowerError::Internal(
                    "native task adapter has an unexpected physical signature".into(),
                ));
            }
            Ok(())
        })?;
        let mut symbol = format!("__fe_native_task_{index}");
        while !names.insert(symbol.clone()) {
            symbol.push('_');
        }
        let wrapper = builder
            .declare_function(Signature::new_single(
                &symbol,
                Linkage::Public,
                &[pointer, Type::I64, Type::I64],
                Type::I64,
            ))
            .map_err(|error| {
                LowerError::Internal(format!("declare native task bridge: {error}"))
            })?;
        let mut fb = builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        fb.switch_to_block(entry);
        let frame = fb.func.arg_values[0];
        let kind = fb.func.arg_values[1];
        let payload = fb.func.arg_values[2];
        let frame = fb.insert_inst(PtrToInt::new(isa.inst_set(), frame, Type::I256), Type::I256);
        let result = fb.insert_inst(
            Call::new(isa.inst_set(), callee, vec![frame, kind, payload].into()),
            Type::I64,
        );
        fb.insert_inst_no_result(Return::new_single(isa.inst_set(), result));
        fb.seal_all();
        fb.finish();
        symbols.push(symbol);
    }
    let mut compile = Compile::new(builder.build(), CraneliftObjectBackend::new())
        .with_opt_level(to_sonatina_opt_level(opt_level));
    compile.optimize();
    ensure_module_sonatina_ir_valid(compile.module())?;
    let ir = ModuleWriter::new(compile.module()).dump_string();
    let object = compile
        .compile()
        .map(|artifact| artifact.into_bytes())
        .map_err(|errors| LowerError::Internal(format_cranelift_errors(&errors)))?;
    Ok(NativeTaskArtifact {
        symbols,
        frame_words: plan.frame_words(db),
        object,
        ir,
    })
}
