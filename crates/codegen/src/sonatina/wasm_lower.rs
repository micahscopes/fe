//! Narrow MIR -> Sonatina IR lowering for the wasm target (R1).
//!
//! This is the first genuinely-Fe-compiled wasm path: `MIR runtime package ->
//! Sonatina IR (portable vocabulary, Wasm32 ISA) -> WAFFLE -> wasm bytes`. It is
//! deliberately narrow. It lowers only the scalar-arithmetic + control-flow +
//! call subset needed for `add`, `sum_to` (a loop/phi), and a two-function call
//! pair, and it FAILS CLOSED on everything else (aggregates, memory builtins,
//! checked-overflow reverts, u128/u256, EVM host ops). Those are R2.
//!
//! Why a separate path rather than parameterizing the EVM lowerer
//! (`lower_runtime.rs`): the EVM path hardcodes `Type::I256` as the word in ~90
//! places and lowers Fe's checked arithmetic to `uaddo` + an EVM `revert` panic
//! block (see `emit_panic_revert`), which is EVM-native. A faithful portable
//! rewrite of that lowerer is R2-scale. For R1 we lower the clean MIR directly:
//! at the MIR level `a + b` is `RExpr::Binary { op: Arith(Add), .. }` with no
//! overflow machinery attached, so we emit a plain portable `arith::Add`. The
//! WAFFLE translator currently fakes overflow flags as 0, so R1 is only correct
//! for non-overflowing values; real checked semantics are R2.
//!
//! It reuses Sonatina's `FunctionBuilder` SSA-variable machinery (declare/def/
//! use + `seal_all`) exactly as the EVM lowerer does, so loop-carried values
//! (`sum_to`'s accumulator) get their phis inserted automatically. MIR runtime
//! locals in this subset are value-carried (`RuntimeLocalRoot::None`); any
//! address-taken (`Slot`) local is out of R1 scope and fails closed.

use std::collections::HashMap;

use driver::DriverDataBase;
use hir::hir_def::{ArithBinOp, BinOp, CompBinOp};
use mir::{
    AddressSpaceKind, ConstScalar, IntrinsicArithBinOp, Layout, LayoutId, RBlockId, RExpr,
    RLocalId, RStmt, RTerminator, RefKind, RuntimeBody, RuntimeBuiltin, RuntimeCarrier,
    RuntimeClass, RuntimeFunction, RuntimeInstance, RuntimeLinkage, RuntimeLocalRoot,
    RuntimePackage, ScalarClass,
};
use rustc_hash::FxHashMap;
use sonatina_ir::{
    BlockId, Immediate, Module, Signature, Type, ValueId,
    builder::{FunctionBuilder, ModuleBuilder, Variable},
    func_cursor::InstInserter,
    inst::{
        arith::{Add, Mul, Sub},
        cmp::{Eq as CmpEq, Lt},
        control_flow::{Br, Call, Jump, Return, Unreachable},
    },
    isa::{Isa, wasm32::Wasm32},
    module::{FuncRef, ModuleCtx},
};
use sonatina_triple::{Architecture, OperatingSystem, TargetTriple, Vendor};

use super::LowerError;
use super::lower_runtime::{
    assign_sonatina_function_symbols, bytes_to_i256, linkage_for_runtime, scalar_ty,
};

/// The Wasm32 ISA the wasm lowering targets (little-endian, 32-bit pointers,
/// portable `NativeInstSet` vocabulary).
pub(crate) fn create_wasm32_isa() -> Wasm32 {
    Wasm32::new(TargetTriple::new(
        Architecture::Wasm32,
        Vendor::Unknown,
        OperatingSystem::Native,
    ))
}

/// Lower a MIR runtime package to a Sonatina IR `Module` built under the Wasm32
/// ISA. The resulting module is handed to the Sonatina WAFFLE backend to emit
/// wasm bytes. The second return value is the symbol -> wasm-import-module side
/// table (R3.3): the WAFFLE backend consults it to name each external
/// declaration's import module, defaulting to the flat `"fe"` convention for
/// symbols it does not list.
pub fn compile_runtime_package_wasm(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
) -> Result<(Module, HashMap<String, String>), LowerError> {
    let isa = create_wasm32_isa();
    let builder = ModuleBuilder::new(ModuleCtx::new(&isa));
    let mut lowerer = WasmModuleLowerer::new(db, builder, &isa, package);
    lowerer.declare_functions()?;
    lowerer.lower_bodies()?;
    let import_modules = lowerer.import_modules();
    Ok((lowerer.finish(), import_modules))
}

struct WasmModuleLowerer<'db, 'a> {
    db: &'db DriverDataBase,
    builder: ModuleBuilder,
    isa: &'a Wasm32,
    package: &'a RuntimePackage<'db>,
    func_symbols: FxHashMap<RuntimeInstance<'db>, String>,
    func_map: FxHashMap<RuntimeInstance<'db>, FuncRef>,
}

impl<'db, 'a> WasmModuleLowerer<'db, 'a> {
    fn new(
        db: &'db DriverDataBase,
        builder: ModuleBuilder,
        isa: &'a Wasm32,
        package: &'a RuntimePackage<'db>,
    ) -> Self {
        Self {
            db,
            builder,
            isa,
            package,
            func_symbols: assign_sonatina_function_symbols(db, package),
            func_map: FxHashMap::default(),
        }
    }

    fn finish(self) -> Module {
        self.builder.build()
    }

    /// The symbol -> wasm-import-module side table for external declarations
    /// (R3.3). Each non-builtin `extern` whose block carries
    /// `#[wasm_import(module = "...")]` maps its Sonatina symbol (which becomes
    /// the import's field name) to that module string. Attribute-less externs are
    /// omitted, so the WAFFLE backend falls back to the flat `"fe"` convention for
    /// them. Keyed by the same symbol the lowering assigns, so it matches the
    /// import name the backend reads.
    fn import_modules(&self) -> HashMap<String, String> {
        let mut modules = HashMap::new();
        for function in self.package.functions(self.db) {
            if function.linkage(self.db) != RuntimeLinkage::External {
                continue;
            }
            let instance = function.instance(self.db);
            if let Some(module) = mir::wasm_import_module(self.db, instance) {
                modules.insert(self.function_symbol(instance), module);
            }
        }
        modules
    }

    fn function_symbol(&self, instance: RuntimeInstance<'db>) -> String {
        // A DECLARED-EXTERNAL host import is named by its BASE op identifier (the
        // stable name the broker binds - "the import table IS the op set"), NOT the
        // internal Sonatina symbol, which is mangled per instance
        // (`std__lib__webgpu__raw__gpu_buffer_create_HASH`). The fork's WAFFLE emitter
        // uses this string verbatim as the wasm import field name.
        if let Some(name) = mir::wasm_import_name(self.db, instance) {
            return name;
        }
        self.func_symbols
            .get(&instance)
            .cloned()
            .or_else(|| {
                self.package
                    .functions(self.db)
                    .into_iter()
                    .find(|function| function.instance(self.db) == instance)
                    .map(|function| function.symbol(self.db).clone())
            })
            .unwrap_or_else(|| format!("{:?}", instance.key(self.db)))
    }

    fn declare_functions(&mut self) -> Result<(), LowerError> {
        // DECLARED-EXTERNAL host imports dedup to ONE import per `(module, op-name)`
        // identity: the same `extern` reached through two effect-provider scopes
        // (e.g. `main`'s `Dispatch` + `Wait` vs `main_begin`'s `Dispatch`) mints two
        // instances, but they name the SAME broker op, so the import table must carry
        // it once. Each instance maps to the shared import `FuncRef`; bodyless imports
        // never lower a body (`lower_bodies` skips block-empty functions).
        let mut import_refs: FxHashMap<(String, String), FuncRef> = FxHashMap::default();
        for function in self.package.functions(self.db) {
            let instance = function.instance(self.db);
            if let Some(name) = mir::wasm_import_name(self.db, instance) {
                let module =
                    mir::wasm_import_module(self.db, instance).unwrap_or_else(|| "fe".to_string());
                let key = (module, name);
                if let Some(func_ref) = import_refs.get(&key) {
                    self.func_map.insert(instance, *func_ref);
                    continue;
                }
                let signature = self.lower_signature(function)?;
                let func_ref = self.builder.declare_function(signature).map_err(|err| {
                    LowerError::Internal(format!("failed to declare wasm function: {err}"))
                })?;
                import_refs.insert(key, func_ref);
                self.func_map.insert(instance, func_ref);
                continue;
            }
            let signature = self.lower_signature(function)?;
            let func_ref = self.builder.declare_function(signature).map_err(|err| {
                LowerError::Internal(format!("failed to declare wasm function: {err}"))
            })?;
            self.func_map.insert(instance, func_ref);
        }
        Ok(())
    }

    fn lower_signature(&mut self, function: RuntimeFunction<'db>) -> Result<Signature, LowerError> {
        let body = function.instance(self.db).body(self.db);
        let args = body
            .signature
            .params
            .iter()
            .map(|param| self.ty_for_class(&param.class))
            .collect::<Result<Vec<_>, _>>()?;
        let ret = body
            .signature
            .ret
            .as_ref()
            .map(|class| self.ty_for_class(class))
            .transpose()?;
        let symbol = self.function_symbol(function.instance(self.db));
        let linkage = linkage_for_runtime(function.linkage(self.db));
        Ok(match ret {
            Some(ret) => Signature::new_single(&symbol, linkage, &args, ret),
            None => Signature::new_unit(&symbol, linkage, &args),
        })
    }

    fn lower_bodies(&mut self) -> Result<(), LowerError> {
        for function in self.package.functions(self.db) {
            let instance = function.instance(self.db);
            let body = instance.body(self.db);
            if body.blocks.is_empty() {
                continue;
            }
            let func_ref = *self.func_map.get(&instance).ok_or_else(|| {
                LowerError::Internal("wasm function lowered before it was declared".to_string())
            })?;
            WasmFunctionLowerer::new(self, body, func_ref)?.lower()?;
        }
        Ok(())
    }

    /// The Sonatina `Type` for a runtime class. R1 covered the scalar envelope
    /// (bool, u8..u64 / i8..i64); R3.4b adds the FIRST per-backend ABI facts: a
    /// memory-space provider reference is the backend pointer word (i32), and a
    /// single-scalar-field aggregate is its one field's scalar. Wider scalars
    /// (u128/u256/address), non-memory provider refs, object/const refs, raw
    /// addresses, and multi-field / empty aggregates all fail closed.
    fn ty_for_class(&self, class: &RuntimeClass<'db>) -> Result<Type, LowerError> {
        match class {
            RuntimeClass::Scalar(scalar) => scalar_ty_r1(scalar),
            // R3.4b step 1: the `MemPtr<B::Word>` transport class is the backend
            // pointer word: i32 on wasm32 (exactly the linear-memory offset the JS
            // broker already sees). A `MemPtr<u32>` classifies as a memory-space
            // `RawAddr` (a raw memory address, the class the runtime classifier
            // actually assigns every host-minted region pointer and every region
            // pointer crossing the WebGPU import boundary); a memory-space provider
            // reference is admitted on the same footing. This is the wasm mirror of
            // the EVM lowerer's I256 transport repr (`lower_runtime.rs`
            // `ty_for_class`). Non-memory addresses/provider refs and object/const
            // refs are R2 memory lowering and stay fail-closed (the catch-all below).
            RuntimeClass::RawAddr {
                space: AddressSpaceKind::Memory,
                ..
            }
            | RuntimeClass::Ref {
                kind:
                    RefKind::Provider {
                        space: AddressSpaceKind::Memory,
                        ..
                    },
                ..
            } => Ok(Type::I32),
            // R3.4b step 2: a single-scalar-field aggregate (a `u32` newtype such
            // as `PendingId` / `KernelId` / `WebGpuRef`) is represented as its one
            // field's scalar. Multi-field and empty aggregates fail closed.
            RuntimeClass::AggregateValue { layout } => {
                let scalar = self.single_scalar_field(*layout).ok_or_else(|| {
                    LowerError::Unsupported(format!(
                        "wasm target (R3.4b) supports only single-scalar-field aggregates; \
                         `{class:?}` is not a one-field scalar newtype"
                    ))
                })?;
                scalar_ty_r1(&scalar)
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) supports only scalar values; `{other:?}` \
                 (aggregate/ref/raw-addr) is not lowered yet"
            ))),
        }
    }

    /// If `layout` is a struct with EXACTLY ONE field that is an R1-envelope
    /// scalar, return that field's scalar class; otherwise `None` (multi-field,
    /// empty, array, and enum layouts stay fail-closed). This is what lets the
    /// `u32` newtypes (`PendingId` / `KernelId` / `WebGpuRef`) execute: their
    /// runtime representation IS their single field's word.
    fn single_scalar_field(&self, layout: LayoutId<'db>) -> Option<ScalarClass<'db>> {
        match layout.data(self.db) {
            Layout::Struct(struct_layout) => match &*struct_layout.fields {
                [RuntimeClass::Scalar(scalar)] => Some(scalar.clone()),
                _ => None,
            },
            Layout::Array(_) | Layout::Enum(_) => None,
        }
    }
}

/// R1 scalar type mapping: reuses the EVM path's `scalar_ty` but rejects
/// anything wider than i64 (and anything address-shaped), which fails closed
/// per the ratified "u256-on-wasm is out of scope" decision.
fn scalar_ty_r1<'db>(scalar: &ScalarClass<'db>) -> Result<Type, LowerError> {
    let ty = scalar_ty(scalar);
    match ty {
        Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 => Ok(ty),
        wide => Err(LowerError::Unsupported(format!(
            "wasm target (R1) scalar envelope is bool / u8..u64 / i8..i64; \
             `{wide:?}` (u128/u256/address) is out of scope"
        ))),
    }
}

struct WasmFunctionLowerer<'ctx, 'db, 'a> {
    module: &'ctx mut WasmModuleLowerer<'db, 'a>,
    body: RuntimeBody<'db>,
    fb: FunctionBuilder<InstInserter>,
    prologue_block: BlockId,
    block_map: Vec<BlockId>,
    vars: FxHashMap<RLocalId, Variable>,
}

impl<'ctx, 'db, 'a> WasmFunctionLowerer<'ctx, 'db, 'a> {
    fn new(
        module: &'ctx mut WasmModuleLowerer<'db, 'a>,
        body: RuntimeBody<'db>,
        func_ref: FuncRef,
    ) -> Result<Self, LowerError> {
        let mut fb = module.builder.func_builder::<InstInserter>(func_ref);
        let prologue_block = fb.append_block();
        let block_map = body.blocks.iter().map(|_| fb.append_block()).collect();

        // Declare one SSA variable per value-carried local. Address-taken
        // (`Slot`) locals are R2; their reads/writes fail closed if reached.
        let mut vars = FxHashMap::default();
        for (idx, local) in body.locals.iter().enumerate() {
            if matches!(local.root, RuntimeLocalRoot::Slot(_)) {
                continue;
            }
            if let RuntimeCarrier::Value(class) = &local.carrier {
                let ty = module.ty_for_class(class)?;
                vars.insert(RLocalId::from_u32(idx as u32), fb.declare_var(ty));
            }
        }

        Ok(Self {
            module,
            body,
            fb,
            prologue_block,
            block_map,
            vars,
        })
    }

    fn inst_set(&self) -> &'static sonatina_ir::inst::native::inst_set::NativeInstSet {
        self.module.isa.inst_set()
    }

    fn lower(mut self) -> Result<(), LowerError> {
        let is = self.inst_set();

        // Prologue: bind incoming argument values to their parameter locals,
        // then jump to the MIR entry block (block 0).
        self.fb.switch_to_block(self.prologue_block);
        let params = self.body.signature.params.clone();
        for (idx, param) in params.iter().enumerate() {
            let arg = self.fb.args()[idx];
            let var = self.var_for(param.local)?;
            self.fb.def_var(var, arg);
        }
        let entry = self.block_map[0];
        self.fb.insert_inst_no_result(Jump::new(is, entry));

        let blocks = self.body.blocks.clone();
        for (idx, block) in blocks.iter().enumerate() {
            self.fb.switch_to_block(self.block_map[idx]);
            for stmt in &block.stmts {
                self.lower_stmt(stmt)?;
            }
            self.lower_terminator(&block.terminator)?;
        }

        self.fb.seal_all();
        self.fb.finish();
        Ok(())
    }

    fn lower_stmt(&mut self, stmt: &RStmt<'db>) -> Result<(), LowerError> {
        match stmt {
            RStmt::Assign { dst, expr } => {
                // R3.4b step 3: a unit-returning call lowered as a statement. MIR
                // emits `Assign { dst: <Erased unit temp>, expr: Call }` for a call
                // whose callee returns unit (`body.rs` ~2972-2984); the Erased temp
                // carries no value class, so there is no destination variable to
                // define. Emit a no-result call instruction (the EVM precedent is
                // `lower_runtime.rs` ~1276-1283).
                if let RExpr::Call { callee, args } = expr
                    && callee.body(self.module.db).signature.ret.is_none()
                {
                    return self.lower_call_stmt(*callee, args);
                }
                let value = self.lower_expr(expr, *dst)?;
                let var = self.var_for(*dst)?;
                self.fb.def_var(var, value);
                Ok(())
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) statement `{other:?}` is not supported"
            ))),
        }
    }

    /// Lower a unit-returning call as a statement: a no-result `Call` instruction.
    /// Mirrors `lower_call` but for the unit case (`lower_call` rejects unit-return
    /// callees as value expressions; here the result is discarded by construction).
    fn lower_call_stmt(
        &mut self,
        callee: RuntimeInstance<'db>,
        args: &[RLocalId],
    ) -> Result<(), LowerError> {
        let is = self.inst_set();
        let callee_ref = *self.module.func_map.get(&callee).ok_or_else(|| {
            LowerError::Internal("wasm call target was not declared".to_string())
        })?;
        let mut arg_vals = Vec::with_capacity(args.len());
        for arg in args {
            arg_vals.push(self.local_value(*arg)?);
        }
        self.fb
            .insert_inst_no_result(Call::new(is, callee_ref, arg_vals.into_iter().collect()));
        Ok(())
    }

    fn lower_expr(&mut self, expr: &RExpr<'db>, dst: RLocalId) -> Result<ValueId, LowerError> {
        match expr {
            RExpr::Use(src) => self.local_value(*src),
            RExpr::ConstScalar(constant) => {
                let ty = self.local_ty(dst)?;
                let imm = immediate_for_const_scalar(constant, ty)?;
                Ok(self.fb.make_imm_value(imm))
            }
            RExpr::Binary { op, lhs, rhs } => self.lower_binary(*op, *lhs, *rhs, dst),
            RExpr::Call { callee, args } => self.lower_call(*callee, args),
            RExpr::Builtin(builtin) => self.lower_builtin(builtin),
            // R3.4b step 2: single-scalar-field newtype construction/projection is
            // a no-op on the represented word. `AggregateMake` of one field yields
            // that field's value; `AggregateExtract` at index 0 yields the
            // aggregate's value (which IS the field's word). This is what executes
            // the `u32` newtypes (`PendingId` / `KernelId` / `WebGpuRef`).
            RExpr::AggregateMake { layout, fields } => self.lower_scalar_newtype_make(*layout, fields),
            RExpr::AggregateExtract { value, index } => {
                self.lower_scalar_newtype_extract(*value, *index)
            }
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) expression `{other:?}` is not supported"
            ))),
        }
    }

    /// `AggregateMake` of a single-scalar-field newtype: the aggregate IS its one
    /// field's word, so construction is `Use` of that field. Multi-field (and
    /// empty) constructions fail closed.
    fn lower_scalar_newtype_make(
        &mut self,
        layout: LayoutId<'db>,
        fields: &[RLocalId],
    ) -> Result<ValueId, LowerError> {
        if self.module.single_scalar_field(layout).is_none() {
            return Err(LowerError::Unsupported(format!(
                "wasm target (R3.4b) aggregate construction of `{layout:?}` is not a \
                 single-scalar-field newtype (multi-field aggregates are R2)"
            )));
        }
        let [field] = fields else {
            return Err(LowerError::Internal(
                "single-scalar-field aggregate must have exactly one field".to_string(),
            ));
        };
        self.local_value(*field)
    }

    /// `AggregateExtract` of field 0 from a single-scalar-field newtype: the
    /// extracted word IS the aggregate's value. Any other field index fails closed
    /// (a single-scalar-field newtype has only field 0).
    fn lower_scalar_newtype_extract(
        &mut self,
        value: RLocalId,
        index: u32,
    ) -> Result<ValueId, LowerError> {
        if index != 0 {
            return Err(LowerError::Unsupported(format!(
                "wasm target (R3.4b) aggregate extract at index {index} is not supported \
                 (only field 0 of a single-scalar-field newtype)"
            )));
        }
        self.local_value(value)
    }

    /// Fe's primitive `+`/`-`/`*` lower to `IntrinsicArith` builtins (not
    /// `Binary`). R1 emits plain (unchecked) arithmetic and IGNORES the
    /// `checked` flag: Fe's checked-overflow semantics need real wasm overflow
    /// flags/traps, which are R2 (and the WAFFLE translator currently fakes the
    /// flag as 0), so R1 requires non-overflowing values. Every other builtin
    /// (memory, EVM host, addmod/mulmod, saturating, byte/sign-extend) fails
    /// closed.
    fn lower_builtin(&mut self, builtin: &RuntimeBuiltin<'db>) -> Result<ValueId, LowerError> {
        match builtin {
            RuntimeBuiltin::IntrinsicArith {
                op, lhs, rhs, class, ..
            } => self.lower_intrinsic_arith(*op, *lhs, *rhs, class),
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) builtin `{other:?}` is not supported \
                 (memory/EVM-host/addmod/saturating builtins are R2)"
            ))),
        }
    }

    fn lower_intrinsic_arith(
        &mut self,
        op: IntrinsicArithBinOp,
        lhs: RLocalId,
        rhs: RLocalId,
        class: &ScalarClass<'db>,
    ) -> Result<ValueId, LowerError> {
        let is = self.inst_set();
        let ty = scalar_ty_r1(class)?;
        let lhs = self.local_value(lhs)?;
        let rhs = self.local_value(rhs)?;
        Ok(match op {
            IntrinsicArithBinOp::Add => self.fb.insert_inst(Add::new(is, lhs, rhs), ty),
            IntrinsicArithBinOp::Sub => self.fb.insert_inst(Sub::new(is, lhs, rhs), ty),
            IntrinsicArithBinOp::Mul => self.fb.insert_inst(Mul::new(is, lhs, rhs), ty),
            other => {
                return Err(LowerError::Unsupported(format!(
                    "wasm target (R1) intrinsic arithmetic `{other:?}` is not supported \
                     (div/rem/pow are R2)"
                )));
            }
        })
    }

    fn lower_binary(
        &mut self,
        op: BinOp,
        lhs: RLocalId,
        rhs: RLocalId,
        dst: RLocalId,
    ) -> Result<ValueId, LowerError> {
        let is = self.inst_set();
        let lhs = self.local_value(lhs)?;
        let rhs = self.local_value(rhs)?;
        match op {
            // R1 emits plain (unchecked) arithmetic. Fe's checked-overflow
            // semantics lower to an EVM revert on the EVM path; the wasm
            // equivalent (real overflow flags / traps) is R2, so R1 requires
            // non-overflowing values.
            BinOp::Arith(arith) => {
                let ty = self.local_ty(dst)?;
                Ok(match arith {
                    ArithBinOp::Add => self.fb.insert_inst(Add::new(is, lhs, rhs), ty),
                    ArithBinOp::Sub => self.fb.insert_inst(Sub::new(is, lhs, rhs), ty),
                    ArithBinOp::Mul => self.fb.insert_inst(Mul::new(is, lhs, rhs), ty),
                    other => {
                        return Err(LowerError::Unsupported(format!(
                            "wasm target (R1) arithmetic op `{other:?}` is not supported \
                             (div/rem/pow/shifts/bitwise are R2)"
                        )));
                    }
                })
            }
            BinOp::Comp(comp) => Ok(match comp {
                CompBinOp::Lt => self.fb.insert_inst(Lt::new(is, lhs, rhs), Type::I1),
                CompBinOp::Eq => self.fb.insert_inst(CmpEq::new(is, lhs, rhs), Type::I1),
                other => {
                    return Err(LowerError::Unsupported(format!(
                        "wasm target (R1) comparison `{other:?}` is not supported \
                         (only `<` and `==`; the full compare matrix is R2)"
                    )));
                }
            }),
            other => Err(LowerError::Unsupported(format!(
                "wasm target (R1) binary op `{other:?}` is not supported"
            ))),
        }
    }

    fn lower_call(
        &mut self,
        callee: RuntimeInstance<'db>,
        args: &[RLocalId],
    ) -> Result<ValueId, LowerError> {
        let is = self.inst_set();
        let callee_ref = *self.module.func_map.get(&callee).ok_or_else(|| {
            LowerError::Internal("wasm call target was not declared".to_string())
        })?;
        let ret_class = callee.body(self.module.db).signature.ret.clone();
        let ret_ty = match ret_class {
            Some(class) => self.module.ty_for_class(&class)?,
            None => {
                return Err(LowerError::Unsupported(
                    "wasm target (R1) does not support calling a unit-returning function \
                     as a value expression"
                        .to_string(),
                ));
            }
        };
        let mut arg_vals = Vec::with_capacity(args.len());
        for arg in args {
            arg_vals.push(self.local_value(*arg)?);
        }
        Ok(self
            .fb
            .insert_inst(Call::new(is, callee_ref, arg_vals.into_iter().collect()), ret_ty))
    }

    fn lower_terminator(&mut self, terminator: &RTerminator<'db>) -> Result<(), LowerError> {
        let is = self.inst_set();
        match terminator {
            RTerminator::Return(Some(value)) => {
                let value = self.local_value(*value)?;
                self.fb
                    .insert_inst_no_result(Return::new_single(is, value));
            }
            // A unit return and a `Stop` (the synthetic main-root exit) both
            // become a plain wasm return.
            RTerminator::Return(None) | RTerminator::Stop => {
                self.fb.insert_inst_no_result(Return::new_unit(is));
            }
            RTerminator::Goto(target) => {
                let block = self.block_for(*target)?;
                self.fb.insert_inst_no_result(Jump::new(is, block));
            }
            RTerminator::Branch {
                cond,
                then_bb,
                else_bb,
            } => {
                let cond = self.local_value(*cond)?;
                let then_block = self.block_for(*then_bb)?;
                let else_block = self.block_for(*else_bb)?;
                self.fb
                    .insert_inst_no_result(Br::new(is, cond, then_block, else_block));
            }
            RTerminator::Trap => {
                self.fb.insert_inst_no_result(Unreachable::new(is));
            }
            other => {
                return Err(LowerError::Unsupported(format!(
                    "wasm target (R1) terminator `{other:?}` is not supported \
                     (return-data/revert/switch/match/self-destruct are R2)"
                )));
            }
        }
        Ok(())
    }

    fn local_value(&mut self, local: RLocalId) -> Result<ValueId, LowerError> {
        let var = self.var_for(local)?;
        Ok(self.fb.use_var(var))
    }

    fn var_for(&self, local: RLocalId) -> Result<Variable, LowerError> {
        self.vars.get(&local).copied().ok_or_else(|| {
            LowerError::Unsupported(format!(
                "wasm target (R1): local {local:?} is not a value-carried scalar \
                 (address-taken/aggregate locals are R2)"
            ))
        })
    }

    fn local_ty(&self, local: RLocalId) -> Result<Type, LowerError> {
        let class = self
            .body
            .value_class(local)
            .cloned()
            .ok_or_else(|| LowerError::Internal(format!("local {local:?} carries no value class")))?;
        self.module.ty_for_class(&class)
    }

    fn block_for(&self, block: RBlockId) -> Result<BlockId, LowerError> {
        self.block_map
            .get(block.as_u32() as usize)
            .copied()
            .ok_or_else(|| LowerError::Internal(format!("unknown runtime block {block:?}")))
    }
}

fn immediate_for_const_scalar(
    constant: &ConstScalar,
    ty: Type,
) -> Result<Immediate, LowerError> {
    match constant {
        ConstScalar::Bool(value) => Ok(Immediate::from(*value)),
        ConstScalar::Int { words, signed, .. } => {
            Ok(Immediate::from_i256(bytes_to_i256(words, *signed), ty))
        }
        other => Err(LowerError::Unsupported(format!(
            "wasm target (R1) constant `{other:?}` is not supported"
        ))),
    }
}
