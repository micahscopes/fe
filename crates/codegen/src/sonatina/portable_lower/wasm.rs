//! Wasm32 entry orchestration and canonical host wrapper selection.
//!
//! Shader and native entry points use the shared portable builder directly.
//! This module is the only entry that requests Wasm host-interface synthesis.

use super::*;
use sonatina_ir::isa::wasm32::Wasm32;
#[cfg(feature = "sonatina-indirect-calls")]
use sonatina_ir::inst::{control_flow::CallIndirect, data::GetFunctionPtr};

impl<'db, 'a> PortableModuleLowerer<'db, 'a, Wasm32> {
    #[cfg(feature = "sonatina-indirect-calls")]
    fn synthesize_guest_callbacks(
        &mut self,
        callbacks: &[crate::guest_callbacks::ResolvedGuestCallback],
    ) -> Result<(), LowerError> {
        use smallvec1::smallvec;

        fn core_ty(ty: fe_host_abi::CoreType) -> Result<Type, LowerError> {
            Ok(match ty {
                fe_host_abi::CoreType::I32 => Type::I32,
                fe_host_abi::CoreType::I64 => Type::I64,
                fe_host_abi::CoreType::F32 => Type::F32,
                fe_host_abi::CoreType::F64 => {
                    return Err(LowerError::Unsupported(
                        "f64 guest callbacks await an f64 Sonatina carrier".into(),
                    ));
                }
            })
        }

        for (index, callback) in callbacks.iter().enumerate() {
            // Rich and async lanes never reach this resolved scalar manifest.
            // Keep this first materializer deliberately single-result.
            if callback.core_params.len() != 1 || callback.core_results.len() > 1 {
                return Err(LowerError::Unsupported(
                    "guest callback trampolines currently require one scalar parameter and at \
                     most one scalar result"
                        .into(),
                ));
            }
            let target = self
                .func_symbols
                .iter()
                .find_map(|(instance, symbol)| {
                    (symbol == &callback.runtime_symbol)
                        .then(|| self.func_map.get(instance).copied())
                        .flatten()
                })
                .ok_or_else(|| {
                    LowerError::Internal(format!(
                        "resolved guest callback target `{}` was not declared",
                        callback.runtime_symbol
                    ))
                })?;
            let params = callback
                .core_params
                .iter()
                .copied()
                .map(core_ty)
                .collect::<Result<Vec<_>, _>>()?;
            let results = callback
                .core_results
                .iter()
                .copied()
                .map(core_ty)
                .collect::<Result<Vec<_>, _>>()?;
            let function_ty = self
                .builder
                .ctx
                .with_ty_store_mut(|types| types.make_func(&params, &results));
            let function_ptr_ty = function_ty.to_ptr(&self.builder.ctx);
            let prefix = format!("fe_guest_callback_{index}");
            let generation = self.builder.declare_gv(GlobalVariableData::new(
                format!("{prefix}_generation"),
                Type::I32,
                Linkage::Private,
                false,
                Some(GvInitializer::make_imm(0i32)),
            ));
            let occupied = self.builder.declare_gv(GlobalVariableData::new(
                format!("{prefix}_occupied"),
                Type::I32,
                Linkage::Private,
                false,
                Some(GvInitializer::make_imm(0i32)),
            ));

            // register() -> token. One manifest entry owns one reusable slot;
            // generation is bumped before every re-registration.
            let register = self
                .builder
                .declare_function(Signature::new_single(
                    &format!("{prefix}_register"),
                    Linkage::Public,
                    &[],
                    Type::I32,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(register);
                let entry = fb.append_block();
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let occupied_addr = fb.make_global_value(occupied);
                let occupied_value = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), occupied_addr, Type::I32),
                    Type::I32,
                );
                let zero = fb.make_imm_value(Immediate::I32(0));
                let vacant = fb.insert_inst(
                    CmpEq::new(self.isa.inst_set(), occupied_value, zero),
                    Type::I1,
                );
                fb.insert_inst_no_result(Br::new(self.isa.inst_set(), vacant, valid, invalid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(self.isa.inst_set()));
                fb.switch_to_block(valid);
                let generation_addr = fb.make_global_value(generation);
                let old_generation = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), generation_addr, Type::I32),
                    Type::I32,
                );
                let one = fb.make_imm_value(Immediate::I32(1));
                let next = fb.insert_inst(
                    Add::new(self.isa.inst_set(), old_generation, one),
                    Type::I32,
                );
                fb.insert_inst_no_result(Mstore::new(
                    self.isa.inst_set(),
                    generation_addr,
                    next,
                    Type::I32,
                ));
                fb.insert_inst_no_result(Mstore::new(
                    self.isa.inst_set(),
                    occupied_addr,
                    one,
                    Type::I32,
                ));
                let scale = fb.make_imm_value(Immediate::I32(1 << 16));
                let generation_bits =
                    fb.insert_inst(Mul::new(self.isa.inst_set(), next, scale), Type::I32);
                let token = fb.insert_inst(
                    Add::new(self.isa.inst_set(), generation_bits, one),
                    Type::I32,
                );
                fb.insert_return(token);
                fb.seal_all();
                fb.finish();
            }

            let mut trampoline_params = vec![Type::I32];
            trampoline_params.extend_from_slice(&params);

            // Expose the opaque Wasm table index separately from the lifetime
            // token. Hosts may retain it as dispatch metadata, but registration
            // authority is still the generation-checked token below.
            let pointer_export = self
                .builder
                .declare_function(Signature::new_single(
                    &format!("{prefix}_table_slot"),
                    Linkage::Public,
                    &[],
                    function_ptr_ty,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(pointer_export);
                let entry = fb.append_block();
                fb.switch_to_block(entry);
                let pointer = fb.insert_inst(
                    GetFunctionPtr::new(self.isa.inst_set(), target),
                    function_ptr_ty,
                );
                fb.insert_return(pointer);
                fb.seal_all();
                fb.finish();
            }

            let mut raw_params = vec![function_ptr_ty];
            raw_params.extend_from_slice(&params);
            let raw_invoke = self
                .builder
                .declare_function(Signature::new(
                    &format!("{prefix}_invoke_raw"),
                    Linkage::Public,
                    &raw_params,
                    &results,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(raw_invoke);
                let entry = fb.append_block();
                fb.switch_to_block(entry);
                let args = fb.args().to_vec();
                let call = CallIndirect::new(
                    self.isa.inst_set(),
                    args[0],
                    function_ptr_ty,
                    smallvec![args[1]],
                );
                if results.is_empty() {
                    fb.insert_inst_no_result(call);
                    fb.insert_return_unit();
                } else {
                    let result = fb.insert_inst(call, results[0]);
                    fb.insert_return(result);
                }
                fb.seal_all();
                fb.finish();
            }

            let invoke = self
                .builder
                .declare_function(Signature::new(
                    &format!("{prefix}_invoke"),
                    Linkage::Public,
                    &trampoline_params,
                    &results,
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(invoke);
                let entry = fb.append_block();
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let args = fb.args().to_vec();
                let occupied_addr = fb.make_global_value(occupied);
                let generation_addr = fb.make_global_value(generation);
                let occupied_value = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), occupied_addr, Type::I32),
                    Type::I32,
                );
                let current_generation = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), generation_addr, Type::I32),
                    Type::I32,
                );
                let one = fb.make_imm_value(Immediate::I32(1));
                let scale = fb.make_imm_value(Immediate::I32(1 << 16));
                let generation_bits = fb.insert_inst(
                    Mul::new(self.isa.inst_set(), current_generation, scale),
                    Type::I32,
                );
                let expected = fb.insert_inst(
                    Add::new(self.isa.inst_set(), generation_bits, one),
                    Type::I32,
                );
                let token_ok =
                    fb.insert_inst(CmpEq::new(self.isa.inst_set(), args[0], expected), Type::I1);
                let occupied_ok = fb.insert_inst(
                    CmpEq::new(self.isa.inst_set(), occupied_value, one),
                    Type::I1,
                );
                let valid_token = fb.insert_inst(
                    And::new(self.isa.inst_set(), token_ok, occupied_ok),
                    Type::I1,
                );
                fb.insert_inst_no_result(Br::new(self.isa.inst_set(), valid_token, valid, invalid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(self.isa.inst_set()));
                fb.switch_to_block(valid);
                let callee = fb.insert_inst(
                    GetFunctionPtr::new(self.isa.inst_set(), target),
                    function_ptr_ty,
                );
                let call = CallIndirect::new(
                    self.isa.inst_set(),
                    callee,
                    function_ptr_ty,
                    smallvec![args[1]],
                );
                if results.is_empty() {
                    fb.insert_inst_no_result(call);
                    fb.insert_return_unit();
                } else {
                    let result = fb.insert_inst(call, results[0]);
                    fb.insert_return(result);
                }
                fb.seal_all();
                fb.finish();
            }

            let release = self
                .builder
                .declare_function(Signature::new(
                    &format!("{prefix}_release"),
                    Linkage::Public,
                    &[Type::I32],
                    &[],
                ))
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            {
                let mut fb = self.builder.func_builder::<InstInserter>(release);
                let entry = fb.append_block();
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let token = fb.args()[0];
                let occupied_addr = fb.make_global_value(occupied);
                let generation_addr = fb.make_global_value(generation);
                let occupied_value = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), occupied_addr, Type::I32),
                    Type::I32,
                );
                let current_generation = fb.insert_inst(
                    Mload::new(self.isa.inst_set(), generation_addr, Type::I32),
                    Type::I32,
                );
                let one = fb.make_imm_value(Immediate::I32(1));
                let scale = fb.make_imm_value(Immediate::I32(1 << 16));
                let generation_bits = fb.insert_inst(
                    Mul::new(self.isa.inst_set(), current_generation, scale),
                    Type::I32,
                );
                let expected = fb.insert_inst(
                    Add::new(self.isa.inst_set(), generation_bits, one),
                    Type::I32,
                );
                let token_ok =
                    fb.insert_inst(CmpEq::new(self.isa.inst_set(), token, expected), Type::I1);
                let occupied_ok = fb.insert_inst(
                    CmpEq::new(self.isa.inst_set(), occupied_value, one),
                    Type::I1,
                );
                let valid_token = fb.insert_inst(
                    And::new(self.isa.inst_set(), token_ok, occupied_ok),
                    Type::I1,
                );
                fb.insert_inst_no_result(Br::new(self.isa.inst_set(), valid_token, valid, invalid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(self.isa.inst_set()));
                fb.switch_to_block(valid);
                let zero = fb.make_imm_value(Immediate::I32(0));
                fb.insert_inst_no_result(Mstore::new(
                    self.isa.inst_set(),
                    occupied_addr,
                    zero,
                    Type::I32,
                ));
                fb.insert_return_unit();
                fb.seal_all();
                fb.finish();
            }
        }
        Ok(())
    }

    fn synthesize_resident_transition(
        &mut self,
        transition: &super::super::WasmResidentTransition,
        initializer: Option<&super::super::WasmResidentInitializer>,
        projection: Option<&super::super::WasmResidentProjection>,
    ) -> Result<ResidentActorStorage, LowerError> {
        match &transition.transport {
            super::super::WasmResidentEventTransport::Direct { event_fields } => self
                .synthesize_direct_resident_transition(
                    transition,
                    initializer,
                    projection,
                    *event_fields,
                ),
            super::super::WasmResidentEventTransport::Batch {
                event_fields,
                event_stride,
                accumulate_f32_fields,
                coalesce_tag_field,
                coalesce_tag_variant,
            } => {
                if initializer.is_some() || projection.is_some() {
                    return Err(LowerError::Unsupported(
                        "batched resident transitions do not yet admit an authored initializer/projection"
                            .to_owned(),
                    ));
                }
                self.synthesize_batched_resident_transition(
                    transition,
                    *event_fields,
                    *event_stride,
                    accumulate_f32_fields,
                    *coalesce_tag_field,
                    *coalesce_tag_variant,
                )
            }
        }
    }

    /// Synthesize the target-neutral one-event resident actor ABI. The host
    /// passes one flattened event plus inert resources. Complete actor state
    /// stays in private Wasm globals and the authored Fe transition returns the
    /// next complete state.
    fn synthesize_direct_resident_transition(
        &mut self,
        transition: &super::super::WasmResidentTransition,
        initializer: Option<&super::super::WasmResidentInitializer>,
        projection: Option<&super::super::WasmResidentProjection>,
        event_fields: usize,
    ) -> Result<ResidentActorStorage, LowerError> {
        if event_fields == 0 {
            return Err(LowerError::Unsupported(
                "resident actor transition must declare at least one event field".to_owned(),
            ));
        }

        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| self.function_symbol(**instance) == transition.source)
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must select exactly one lowered Fe behavior (found {})",
                transition.source,
                candidates.len()
            )));
        };
        let (callee_args, result_tys) = self.builder.sig(*callee, |signature| {
            (signature.args().to_vec(), signature.ret_tys().to_vec())
        });
        if callee_args.len() < event_fields {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} arguments, fewer than its {} event leaves",
                transition.source,
                callee_args.len(),
                event_fields
            )));
        }
        if result_tys.is_empty() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return complete actor state",
                transition.source
            )));
        }

        let event_tys = &callee_args[..event_fields];
        let actor_tys = &callee_args[event_fields..];
        if actor_tys.len() != transition.actor_param_is_resource.len() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} flattened actor arguments but its resource mask has {} entries",
                transition.source,
                actor_tys.len(),
                transition.actor_param_is_resource.len()
            )));
        }
        let state_tys = actor_tys
            .iter()
            .zip(&transition.actor_param_is_resource)
            .filter_map(|(ty, is_resource)| (!is_resource).then_some(*ty))
            .collect::<Vec<_>>();
        if state_tys != result_tys {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return the complete flattened non-resource actor state: arguments {state_tys:?}, results {result_tys:?}",
                transition.source
            )));
        }
        for (index, limit) in &transition.event_tag_limits {
            if *limit == 0
                || event_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid event enum constraint ({index}, {limit}) for {event_tys:?}",
                    transition.source
                )));
            }
        }
        for (index, limit) in &transition.state_tag_limits {
            if *limit == 0
                || result_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid state enum constraint ({index}, {limit}) for {result_tys:?}",
                    transition.source
                )));
            }
        }

        let state_globals = result_tys
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                self.builder.declare_gv(GlobalVariableData::new(
                    format!("__fe_actor_state_v1_{index}"),
                    *ty,
                    Linkage::Private,
                    false,
                    None,
                ))
            })
            .collect::<Vec<_>>();
        let state_initialized = self.builder.declare_gv(GlobalVariableData::new(
            "__fe_actor_state_v1_initialized".to_owned(),
            Type::I32,
            Linkage::Private,
            false,
            Some(GvInitializer::make_imm(0i32)),
        ));

        if let Some(initializer) = initializer {
            let candidates = self
                .func_map
                .iter()
                .filter(|(instance, _)| self.function_symbol(**instance) == initializer.source)
                .map(|(_, func_ref)| *func_ref)
                .collect::<Vec<_>>();
            let [initializer_callee] = candidates.as_slice() else {
                return Err(LowerError::Unsupported(format!(
                    "resident initializer `{}` must select exactly one lowered Fe behavior (found {})",
                    initializer.source,
                    candidates.len()
                )));
            };
            let (initializer_args, initializer_results) =
                self.builder.sig(*initializer_callee, |signature| {
                    (signature.args().to_vec(), signature.ret_tys().to_vec())
                });
            if !initializer_args.is_empty() || initializer_results != result_tys {
                return Err(LowerError::Unsupported(format!(
                    "resident initializer `{}` must take no arguments and return complete actor state {result_tys:?}; got {initializer_args:?} -> {initializer_results:?}",
                    initializer.source
                )));
            }
            let initialize = self
                .builder
                .declare_function(Signature::new(
                    &initializer.export,
                    Linkage::Public,
                    &[],
                    &result_tys,
                ))
                .map_err(|error| {
                    LowerError::Internal(format!(
                        "failed to declare resident initializer `{}`: {error}",
                        initializer.export
                    ))
                })?;
            {
                let is = self.isa.inst_set();
                let mut fb = self.builder.func_builder::<InstInserter>(initialize);
                let entry = fb.append_block();
                fb.switch_to_block(entry);
                let results = fb.insert_call_results(
                    *initializer_callee,
                    smallvec1::SmallVec::<[ValueId; 8]>::new(),
                );
                for ((global, ty), value) in state_globals
                    .iter()
                    .copied()
                    .zip(result_tys.iter().copied())
                    .zip(results.iter().copied())
                {
                    let address = fb.make_global_value(global);
                    fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
                }
                let initialized_address = fb.make_global_value(state_initialized);
                let one = fb.make_imm_value(Immediate::I32(1));
                fb.insert_inst_no_result(Mstore::new(is, initialized_address, one, Type::I32));
                fb.insert_return_values(&results);
                fb.seal_all();
                fb.finish();
            }
        }

        if let Some(projection) = projection {
            let candidates = self
                .func_map
                .iter()
                .filter(|(instance, _)| self.function_symbol(**instance) == projection.source)
                .map(|(_, func_ref)| *func_ref)
                .collect::<Vec<_>>();
            let [projection_callee] = candidates.as_slice() else {
                return Err(LowerError::Unsupported(format!(
                    "resident projection `{}` must select exactly one lowered Fe behavior (found {})",
                    projection.source,
                    candidates.len()
                )));
            };
            let (projection_args, projection_results) =
                self.builder.sig(*projection_callee, |signature| {
                    (signature.args().to_vec(), signature.ret_tys().to_vec())
                });
            if projection_args != result_tys || projection_results.is_empty() {
                return Err(LowerError::Unsupported(format!(
                    "resident projection `{}` must take complete actor state {result_tys:?} and return a non-empty closed value; got {projection_args:?} -> {projection_results:?}",
                    projection.source
                )));
            }
            let project = self
                .builder
                .declare_function(Signature::new(
                    &projection.export,
                    Linkage::Public,
                    &[],
                    &projection_results,
                ))
                .map_err(|error| {
                    LowerError::Internal(format!(
                        "failed to declare resident projection `{}`: {error}",
                        projection.export
                    ))
                })?;
            {
                let is = self.isa.inst_set();
                let mut fb = self.builder.func_builder::<InstInserter>(project);
                let entry = fb.append_block();
                let invoke = fb.append_block();
                let invalid = fb.append_block();
                fb.switch_to_block(entry);
                let initialized_address = fb.make_global_value(state_initialized);
                let initialized_value =
                    fb.insert_inst(Mload::new(is, initialized_address, Type::I32), Type::I32);
                let one = fb.make_imm_value(Immediate::I32(1));
                let initialized = fb.insert_inst(CmpEq::new(is, initialized_value, one), Type::I1);
                fb.insert_inst_no_result(Br::new(is, initialized, invoke, invalid));

                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(is));

                fb.switch_to_block(invoke);
                let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
                for (global, ty) in state_globals
                    .iter()
                    .copied()
                    .zip(result_tys.iter().copied())
                {
                    let address = fb.make_global_value(global);
                    args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                }
                let results = fb.insert_call_results(*projection_callee, args);
                fb.insert_return_values(&results);
                fb.seal_all();
                fb.finish();
            }
        }

        let state_replace = self
            .builder
            .declare_function(Signature::new(
                &transition.state_replace_export,
                Linkage::Public,
                &result_tys,
                &[],
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident state replacement `{}`: {error}",
                    transition.state_replace_export
                ))
            })?;
        {
            let is = self.isa.inst_set();
            let mut fb = self.builder.func_builder::<InstInserter>(state_replace);
            let entry = fb.append_block();
            let valid = fb.append_block();
            let invalid = fb.append_block();
            fb.switch_to_block(entry);
            let values = fb.args().to_vec();
            let mut all_valid = None;
            for (index, limit) in &transition.state_tag_limits {
                let mut tag_valid = None;
                for tag in 0..*limit {
                    let expected = fb.make_imm_value(
                        fieldless_tag_immediate(result_tys[*index], tag)
                            .expect("enum tag type validated above"),
                    );
                    let equal = fb.insert_inst(CmpEq::new(is, values[*index], expected), Type::I1);
                    tag_valid = Some(match tag_valid {
                        Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                        None => equal,
                    });
                }
                let tag_valid = tag_valid.expect("nonzero enum limit validated above");
                all_valid = Some(match all_valid {
                    Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                    None => tag_valid,
                });
            }
            if let Some(all_valid) = all_valid {
                fb.insert_inst_no_result(Br::new(is, all_valid, valid, invalid));
            } else {
                fb.insert_inst_no_result(Jump::new(is, valid));
            }

            fb.switch_to_block(invalid);
            fb.insert_inst_no_result(Unreachable::new(is));

            fb.switch_to_block(valid);
            for ((global, ty), value) in state_globals
                .iter()
                .copied()
                .zip(result_tys.iter().copied())
                .zip(values)
            {
                let address = fb.make_global_value(global);
                fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
            }
            let initialized_address = fb.make_global_value(state_initialized);
            let one = fb.make_imm_value(Immediate::I32(1));
            fb.insert_inst_no_result(Mstore::new(is, initialized_address, one, Type::I32));
            fb.insert_inst_no_result(Return::new_unit(is));
            fb.seal_all();
            fb.finish();
        }

        let mut wrapper_args = event_tys.to_vec();
        wrapper_args.extend(
            actor_tys
                .iter()
                .zip(&transition.actor_param_is_resource)
                .filter_map(|(ty, is_resource)| is_resource.then_some(*ty)),
        );
        let wrapper = self
            .builder
            .declare_function(Signature::new(
                &transition.export,
                Linkage::Public,
                &wrapper_args,
                &result_tys,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident transition wrapper `{}`: {error}",
                    transition.export
                ))
            })?;

        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        let invoke = fb.append_block();
        let invalid = fb.append_block();
        fb.switch_to_block(entry);
        let wrapper_values = fb.args().to_vec();
        let initialized_address = fb.make_global_value(state_initialized);
        let initialized_value =
            fb.insert_inst(Mload::new(is, initialized_address, Type::I32), Type::I32);
        let one = fb.make_imm_value(Immediate::I32(1));
        let mut ready = fb.insert_inst(CmpEq::new(is, initialized_value, one), Type::I1);
        for (index, limit) in &transition.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("enum tag type validated above"),
                );
                let equal =
                    fb.insert_inst(CmpEq::new(is, wrapper_values[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            ready = fb.insert_inst(
                And::new(
                    is,
                    ready,
                    tag_valid.expect("nonzero enum limit validated above"),
                ),
                Type::I1,
            );
        }
        fb.insert_inst_no_result(Br::new(is, ready, invoke, invalid));

        fb.switch_to_block(invalid);
        fb.insert_inst_no_result(Unreachable::new(is));

        fb.switch_to_block(invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        args.extend(wrapper_values[..event_fields].iter().copied());
        let mut resource_index = event_fields;
        let mut state_index = 0usize;
        for is_resource in &transition.actor_param_is_resource {
            if *is_resource {
                args.push(wrapper_values[resource_index]);
                resource_index += 1;
            } else {
                let global = state_globals[state_index];
                let ty = result_tys[state_index];
                let address = fb.make_global_value(global);
                args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                state_index += 1;
            }
        }
        let results = fb.insert_call_results(*callee, args);
        if results.len() != result_tys.len() {
            return Err(LowerError::Internal(
                "resident transition call result arity changed after signature inspection"
                    .to_owned(),
            ));
        }
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
            .zip(results.iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        fb.insert_return_values(&results);
        fb.seal_all();
        fb.finish();
        Ok(ResidentActorStorage {
            result_tys,
            state_globals,
            state_initialized,
            state_tag_limits: transition.state_tag_limits.clone(),
        })
    }

    /// Add one direct event entry to an already-materialized resident actor.
    /// The auxiliary behavior is admitted only when its complete non-resource
    /// argument and result shapes exactly match the primary transition. It
    /// therefore cannot acquire a second state store or drift into a parallel
    /// application protocol.
    fn synthesize_aux_resident_transition(
        &mut self,
        transition: &super::super::WasmResidentTransition,
        state: &ResidentActorStorage,
    ) -> Result<(), LowerError> {
        let super::super::WasmResidentEventTransport::Direct { event_fields } = transition.transport
        else {
            return Err(LowerError::Unsupported(format!(
                "auxiliary resident transition `{}` must use direct event transport",
                transition.source
            )));
        };
        if event_fields == 0 {
            return Err(LowerError::Unsupported(
                "auxiliary resident actor transition must declare at least one event field"
                    .to_owned(),
            ));
        }
        if transition.state_tag_limits != state.state_tag_limits {
            return Err(LowerError::Unsupported(format!(
                "auxiliary resident transition `{}` has a different complete-state enum contract",
                transition.source
            )));
        }

        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| self.function_symbol(**instance) == transition.source)
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "auxiliary resident transition `{}` must select exactly one lowered Fe behavior (found {})",
                transition.source,
                candidates.len()
            )));
        };
        let (callee_args, result_tys) = self.builder.sig(*callee, |signature| {
            (signature.args().to_vec(), signature.ret_tys().to_vec())
        });
        if callee_args.len() < event_fields {
            return Err(LowerError::Unsupported(format!(
                "auxiliary resident transition `{}` has {} arguments, fewer than its {} event leaves",
                transition.source,
                callee_args.len(),
                event_fields
            )));
        }
        let event_tys = &callee_args[..event_fields];
        let actor_tys = &callee_args[event_fields..];
        if actor_tys.len() != transition.actor_param_is_resource.len() {
            return Err(LowerError::Unsupported(format!(
                "auxiliary resident transition `{}` has {} flattened actor arguments but its resource mask has {} entries",
                transition.source,
                actor_tys.len(),
                transition.actor_param_is_resource.len()
            )));
        }
        let state_tys = actor_tys
            .iter()
            .zip(&transition.actor_param_is_resource)
            .filter_map(|(ty, is_resource)| (!is_resource).then_some(*ty))
            .collect::<Vec<_>>();
        if state_tys != state.result_tys || result_tys != state.result_tys {
            return Err(LowerError::Unsupported(format!(
                "auxiliary resident transition `{}` must consume and return the primary transition's complete non-resource state {:?}; got {state_tys:?} -> {result_tys:?}",
                transition.source, state.result_tys
            )));
        }
        for (index, limit) in &transition.event_tag_limits {
            if *limit == 0
                || event_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "auxiliary resident transition `{}` has invalid event enum constraint ({index}, {limit}) for {event_tys:?}",
                    transition.source
                )));
            }
        }

        let mut wrapper_args = event_tys.to_vec();
        wrapper_args.extend(
            actor_tys
                .iter()
                .zip(&transition.actor_param_is_resource)
                .filter_map(|(ty, is_resource)| is_resource.then_some(*ty)),
        );
        let wrapper = self
            .builder
            .declare_function(Signature::new(
                &transition.export,
                Linkage::Public,
                &wrapper_args,
                &state.result_tys,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare auxiliary resident transition wrapper `{}`: {error}",
                    transition.export
                ))
            })?;

        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        let invoke = fb.append_block();
        let invalid = fb.append_block();
        fb.switch_to_block(entry);
        let wrapper_values = fb.args().to_vec();
        let initialized_address = fb.make_global_value(state.state_initialized);
        let initialized_value =
            fb.insert_inst(Mload::new(is, initialized_address, Type::I32), Type::I32);
        let one = fb.make_imm_value(Immediate::I32(1));
        let mut ready = fb.insert_inst(CmpEq::new(is, initialized_value, one), Type::I1);
        for (index, limit) in &transition.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("auxiliary enum tag type validated above"),
                );
                let equal =
                    fb.insert_inst(CmpEq::new(is, wrapper_values[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            ready = fb.insert_inst(
                And::new(
                    is,
                    ready,
                    tag_valid.expect("nonzero auxiliary enum limit validated above"),
                ),
                Type::I1,
            );
        }
        fb.insert_inst_no_result(Br::new(is, ready, invoke, invalid));

        fb.switch_to_block(invalid);
        fb.insert_inst_no_result(Unreachable::new(is));

        fb.switch_to_block(invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        args.extend(wrapper_values[..event_fields].iter().copied());
        let mut resource_index = event_fields;
        let mut state_index = 0usize;
        for is_resource in &transition.actor_param_is_resource {
            if *is_resource {
                args.push(wrapper_values[resource_index]);
                resource_index += 1;
            } else {
                let global = state.state_globals[state_index];
                let ty = state.result_tys[state_index];
                let address = fb.make_global_value(global);
                args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                state_index += 1;
            }
        }
        let results = fb.insert_call_results(*callee, args);
        for ((global, ty), value) in state
            .state_globals
            .iter()
            .copied()
            .zip(state.result_tys.iter().copied())
            .zip(results.iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        fb.insert_return_values(&results);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    /// Lower an ordinary Fe decision function into a resident policy export.
    /// Its event prefix is host supplied, its state segment is loaded from and
    /// committed to private globals, and only the decision suffix is returned
    /// to the host. This is the generic mechanism used by presentation policy;
    /// it contains no browser or scheduling algorithm.
    fn synthesize_resident_policy(
        &mut self,
        policy: &super::super::WasmResidentPolicy,
        policy_index: usize,
    ) -> Result<(), LowerError> {
        if policy.event_fields == 0 || policy.decision_fields == 0 {
            return Err(LowerError::Unsupported(
                "derived policy requires non-empty fact and decision records".to_owned(),
            ));
        }
        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| {
                mir::runtime_instance_symbol_key(self.db, **instance) == policy.callee_instance_key
            })
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident policy `{}` must select exactly one lowered Fe behavior (found {})",
                policy.callee_instance_key,
                candidates.len()
            )));
        };
        let (callee_args, result_tys) = self.builder.sig(*callee, |signature| {
            (signature.args().to_vec(), signature.ret_tys().to_vec())
        });
        let expected_args = policy.event_fields + policy.state_fields;
        let expected_results = policy.state_fields + policy.decision_fields;
        if callee_args.len() != expected_args || result_tys.len() != expected_results {
            return Err(LowerError::Unsupported(format!(
                "resident policy `{}` must flatten to {expected_args} arguments and {expected_results} results; got {} -> {}",
                policy.callee_instance_key,
                callee_args.len(),
                result_tys.len()
            )));
        }
        let (event_tys, state_tys) = if policy.event_first {
            (
                &callee_args[..policy.event_fields],
                &callee_args[policy.event_fields..],
            )
        } else {
            (
                &callee_args[policy.state_fields..],
                &callee_args[..policy.state_fields],
            )
        };
        if result_tys[..policy.state_fields] != *state_tys {
            return Err(LowerError::Unsupported(format!(
                "resident policy `{}` must return its complete state as the leading result prefix: arguments {state_tys:?}, results {:?}",
                policy.callee_instance_key,
                &result_tys[..policy.state_fields]
            )));
        }
        for (index, limit) in &policy.event_tag_limits {
            if *limit == 0
                || event_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident policy `{}` has invalid event enum constraint ({index}, {limit}) for {event_tys:?}",
                    policy.callee_instance_key
                )));
            }
        }
        for (index, limit) in &policy.state_tag_limits {
            if *limit == 0
                || state_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident policy `{}` has invalid state enum constraint ({index}, {limit}) for {state_tys:?}",
                    policy.callee_instance_key
                )));
            }
        }

        // Wasm globals without an explicit initializer are zero-initialized.
        // The nominal Fe state contract deliberately defines that value as its
        // initial state, so no host-authored seed or policy JSON is required.
        let state_globals = state_tys
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                self.builder.declare_gv(GlobalVariableData::new(
                    format!("__fe_resident_policy_{policy_index}_state_v1_{index}"),
                    *ty,
                    Linkage::Private,
                    false,
                    None,
                ))
            })
            .collect::<Vec<_>>();
        let decision_tys = &result_tys[policy.state_fields..];
        let wrapper = self
            .builder
            .declare_function(Signature::new(
                &policy.export,
                Linkage::Public,
                event_tys,
                decision_tys,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident policy wrapper `{}`: {error}",
                    policy.export
                ))
            })?;

        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        let invoke = fb.append_block();
        let invalid = fb.append_block();
        fb.switch_to_block(entry);
        let event_values = fb.args().to_vec();
        let mut all_valid = None;
        for (index, limit) in &policy.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("resident policy enum tag type validated above"),
                );
                let equal =
                    fb.insert_inst(CmpEq::new(is, event_values[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            let tag_valid = tag_valid.expect("nonzero resident policy enum limit validated above");
            all_valid = Some(match all_valid {
                Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                None => tag_valid,
            });
        }
        if let Some(all_valid) = all_valid {
            fb.insert_inst_no_result(Br::new(is, all_valid, invoke, invalid));
        } else {
            fb.insert_inst_no_result(Jump::new(is, invoke));
        }

        fb.switch_to_block(invalid);
        fb.insert_inst_no_result(Unreachable::new(is));

        fb.switch_to_block(invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        if policy.event_first {
            args.extend(event_values.iter().copied());
        }
        for (global, ty) in state_globals.iter().copied().zip(state_tys.iter().copied()) {
            let address = fb.make_global_value(global);
            args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        if !policy.event_first {
            args.extend(event_values);
        }
        let results = fb.insert_call_results(*callee, args);
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(state_tys.iter().copied())
            .zip(results[..policy.state_fields].iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        fb.insert_return_values(&results[policy.state_fields..]);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    /// Publish one compiler-derived scalar fact through the module's fixed
    /// binary ABI. It is deliberately a function rather than a manifest field
    /// or mutable global, so discovery and validation use the same ordinary
    /// Wasm export machinery as the rest of the host contract.
    fn synthesize_fixed_i32_export(&mut self, export: &str, value: i32) -> Result<(), LowerError> {
        let function = self
            .builder
            .declare_function(Signature::new_single(
                export,
                Linkage::Public,
                &[],
                Type::I32,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare fixed i32 export `{export}`: {error}"
                ))
            })?;
        let mut fb = self.builder.func_builder::<InstInserter>(function);
        let entry = fb.append_block();
        fb.switch_to_block(entry);
        let value = fb.make_imm_value(Immediate::I32(value));
        fb.insert_return(value);
        fb.seal_all();
        fb.finish();
        Ok(())
    }

    /// Lower Fe's batched resident transition policy. The host writes fixed-
    /// stride scalar event records into exported linear memory. Coalescing
    /// executes in generated Wasm: selected f32 leaves accumulate while every
    /// other fact comes from the newest record. The authored Fe transition is
    /// invoked exactly once and its full state reply is committed atomically.
    fn synthesize_batched_resident_transition(
        &mut self,
        transition: &super::super::WasmResidentTransition,
        event_fields: usize,
        event_stride: i32,
        accumulate_f32_fields: &[usize],
        coalesce_tag_field: usize,
        coalesce_tag_variant: u32,
    ) -> Result<ResidentActorStorage, LowerError> {
        if event_fields == 0 || event_stride <= 0 {
            return Err(LowerError::Unsupported(
                "batched resident transition requires a non-empty, positive-stride event"
                    .to_owned(),
            ));
        }
        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| self.function_symbol(**instance) == transition.source)
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must select exactly one lowered Fe behavior (found {})",
                transition.source,
                candidates.len()
            )));
        };
        let (callee_args, result_tys) = self.builder.sig(*callee, |signature| {
            (signature.args().to_vec(), signature.ret_tys().to_vec())
        });
        if callee_args.len() < event_fields {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} arguments, fewer than its {} event leaves",
                transition.source,
                callee_args.len(),
                event_fields
            )));
        }
        if result_tys.is_empty() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return complete actor state",
                transition.source
            )));
        }

        let event_tys = callee_args[..event_fields].to_vec();
        for (index, limit) in &transition.event_tag_limits {
            if *limit == 0
                || event_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid batched event enum constraint ({index}, {limit}) for {event_tys:?}",
                    transition.source
                )));
            }
        }
        for index in accumulate_f32_fields {
            if event_tys.get(*index) != Some(&Type::F32) {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` accumulation leaf {index} is not f32",
                    transition.source
                )));
            }
        }
        let coalesce_limit = transition
            .event_tag_limits
            .iter()
            .find_map(|(index, limit)| (*index == coalesce_tag_field).then_some(*limit))
            .ok_or_else(|| {
                LowerError::Unsupported(format!(
                    "resident transition `{}` coalescing field {coalesce_tag_field} is not a validated fieldless enum leaf",
                    transition.source
                ))
            })?;
        if coalesce_tag_variant >= coalesce_limit {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` coalescing variant {coalesce_tag_variant} exceeds enum bound {coalesce_limit}",
                transition.source
            )));
        }
        let coalesce_tag = event_tys
            .get(coalesce_tag_field)
            .and_then(|ty| fieldless_tag_immediate(*ty, coalesce_tag_variant))
            .ok_or_else(|| {
                LowerError::Unsupported(format!(
                    "resident transition `{}` cannot represent coalescing variant {coalesce_tag_variant} in event leaf {coalesce_tag_field}",
                    transition.source
                ))
            })?;
        let actor_tys = &callee_args[event_fields..];
        if actor_tys.len() != transition.actor_param_is_resource.len() {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` has {} flattened actor arguments but its resource mask has {} entries",
                transition.source,
                actor_tys.len(),
                transition.actor_param_is_resource.len()
            )));
        }
        let state_tys = actor_tys
            .iter()
            .zip(&transition.actor_param_is_resource)
            .filter_map(|(ty, is_resource)| (!is_resource).then_some(*ty))
            .collect::<Vec<_>>();
        if state_tys != result_tys {
            return Err(LowerError::Unsupported(format!(
                "resident transition `{}` must return the complete flattened non-resource actor state: arguments {state_tys:?}, results {result_tys:?}",
                transition.source
            )));
        }
        for (index, limit) in &transition.state_tag_limits {
            if *limit == 0
                || result_tys
                    .get(*index)
                    .and_then(|ty| fieldless_tag_immediate(*ty, *limit - 1))
                    .is_none()
            {
                return Err(LowerError::Unsupported(format!(
                    "resident transition `{}` has invalid batched state enum constraint ({index}, {limit}) for {result_tys:?}",
                    transition.source
                )));
            }
        }

        let state_globals = result_tys
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                self.builder.declare_gv(GlobalVariableData::new(
                    format!("__fe_actor_state_v1_{index}"),
                    *ty,
                    Linkage::Private,
                    false,
                    None,
                ))
            })
            .collect::<Vec<_>>();
        let state_initialized = self.builder.declare_gv(GlobalVariableData::new(
            "__fe_actor_state_v1_initialized".to_owned(),
            Type::I32,
            Linkage::Private,
            false,
            Some(GvInitializer::make_imm(0i32)),
        ));

        let state_replace = self
            .builder
            .declare_function(Signature::new(
                &transition.state_replace_export,
                Linkage::Public,
                &result_tys,
                &[],
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident state replacement `{}`: {error}",
                    transition.state_replace_export
                ))
            })?;
        {
            let is = self.isa.inst_set();
            let mut fb = self.builder.func_builder::<InstInserter>(state_replace);
            let entry = fb.append_block();
            let valid = fb.append_block();
            let invalid = fb.append_block();
            fb.switch_to_block(entry);
            let values = fb.args().to_vec();
            let mut all_valid = None;
            for (index, limit) in &transition.state_tag_limits {
                let mut tag_valid = None;
                for tag in 0..*limit {
                    let expected = fb.make_imm_value(
                        fieldless_tag_immediate(result_tys[*index], tag)
                            .expect("enum tag type validated above"),
                    );
                    let equal = fb.insert_inst(CmpEq::new(is, values[*index], expected), Type::I1);
                    tag_valid = Some(match tag_valid {
                        Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                        None => equal,
                    });
                }
                let tag_valid = tag_valid.expect("nonzero enum limit validated above");
                all_valid = Some(match all_valid {
                    Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                    None => tag_valid,
                });
            }
            if let Some(all_valid) = all_valid {
                fb.insert_inst_no_result(Br::new(is, all_valid, valid, invalid));
            } else {
                fb.insert_inst_no_result(Jump::new(is, valid));
            }

            fb.switch_to_block(invalid);
            fb.insert_inst_no_result(Unreachable::new(is));

            fb.switch_to_block(valid);
            for ((global, ty), value) in state_globals
                .iter()
                .copied()
                .zip(result_tys.iter().copied())
                .zip(values)
            {
                let address = fb.make_global_value(global);
                fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
            }
            let initialized_address = fb.make_global_value(state_initialized);
            let one = fb.make_imm_value(Immediate::I32(1));
            fb.insert_inst_no_result(Mstore::new(is, initialized_address, one, Type::I32));
            fb.insert_inst_no_result(Return::new_unit(is));
            fb.seal_all();
            fb.finish();
        }

        let mut wrapper_args = vec![Type::I32, Type::I32];
        wrapper_args.extend(
            actor_tys
                .iter()
                .zip(&transition.actor_param_is_resource)
                .filter_map(|(ty, is_resource)| is_resource.then_some(*ty)),
        );
        let wrapper = self
            .builder
            .declare_function(Signature::new(
                &transition.export,
                Linkage::Public,
                &wrapper_args,
                &result_tys,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare resident transition wrapper `{}`: {error}",
                    transition.export
                ))
            })?;

        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        let classify_header = fb.append_block();
        let classify_body = fb.append_block();
        let classify_done = fb.append_block();
        let initialize = fb.append_block();
        let invalid = fb.append_block();
        let header = fb.append_block();
        let body = fb.append_block();
        let done = fb.append_block();
        let invoke = fb.append_block();
        let sequential_header = fb.append_block();
        let sequential_body = fb.append_block();
        let sequential_invoke = fb.append_block();
        let sequential_done = fb.append_block();
        fb.switch_to_block(entry);
        let wrapper_values = fb.args().to_vec();
        let events_ptr = wrapper_values[0];
        let event_count = wrapper_values[1];
        let zero = fb.make_imm_value(Immediate::I32(0));
        let has_events = fb.insert_inst(Lt::new(is, zero, event_count), Type::I1);
        let initialized_address = fb.make_global_value(state_initialized);
        let initialized_value =
            fb.insert_inst(Mload::new(is, initialized_address, Type::I32), Type::I32);
        let initialized_one = fb.make_imm_value(Immediate::I32(1));
        let initialized =
            fb.insert_inst(CmpEq::new(is, initialized_value, initialized_one), Type::I1);
        let ready = fb.insert_inst(And::new(is, has_events, initialized), Type::I1);
        fb.insert_inst_no_result(Br::new(is, ready, classify_header, invalid));

        // Preserve the one-transition hot path for a homogeneous gesture
        // burst. Any heterogeneous batch is folded in source order below, so
        // a direct parameter edit can never swallow an older gesture merely
        // because its event tag is the newest record.
        fb.switch_to_block(classify_header);
        let classify_index = fb.insert_inst(Phi::new(is, vec![(zero, entry)]), Type::I32);
        let true_value = fb.make_imm_value(Immediate::I1(true));
        let all_coalescible = fb.insert_inst(Phi::new(is, vec![(true_value, entry)]), Type::I1);
        let classify_more = fb.insert_inst(Lt::new(is, classify_index, event_count), Type::I1);
        fb.insert_inst_no_result(Br::new(is, classify_more, classify_body, classify_done));

        fb.switch_to_block(classify_body);
        let stride = fb.make_imm_value(Immediate::I32(event_stride));
        let byte_offset = fb.insert_inst(Mul::new(is, classify_index, stride), Type::I32);
        let event_ptr = fb.insert_inst(Add::new(is, events_ptr, byte_offset), Type::I32);
        let tag_offset = fb.make_imm_value(Immediate::I32((coalesce_tag_field as i32) * 4));
        let tag_ptr = fb.insert_inst(Add::new(is, event_ptr, tag_offset), Type::I32);
        let tag_ty = event_tys[coalesce_tag_field];
        let tag = fb.insert_inst(Mload::new(is, tag_ptr, tag_ty), tag_ty);
        let expected_tag = fb.make_imm_value(coalesce_tag.clone());
        let is_coalescible = fb.insert_inst(CmpEq::new(is, tag, expected_tag), Type::I1);
        let next_all = fb.insert_inst(And::new(is, all_coalescible, is_coalescible), Type::I1);
        let one = fb.make_imm_value(Immediate::I32(1));
        let next_classify_index = fb.insert_inst(Add::new(is, classify_index, one), Type::I32);
        let classify_body_block = fb
            .current_block()
            .expect("resident batch classification has a current block");
        fb.append_phi_arg(classify_index, next_classify_index, classify_body_block);
        fb.append_phi_arg(all_coalescible, next_all, classify_body_block);
        fb.insert_inst_no_result(Jump::new(is, classify_header));

        fb.switch_to_block(classify_done);
        fb.insert_inst_no_result(Br::new(is, all_coalescible, initialize, sequential_header));

        fb.switch_to_block(invalid);
        fb.insert_inst_no_result(Unreachable::new(is));

        fb.switch_to_block(initialize);
        let mut initial = Vec::with_capacity(event_fields);
        for (index, ty) in event_tys.iter().copied().enumerate() {
            let address = if index == 0 {
                events_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32((index as i32) * 4));
                fb.insert_inst(Add::new(is, events_ptr, offset), Type::I32)
            };
            initial.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        let initialize_block = fb
            .current_block()
            .expect("resident batch initialization has a current block");
        fb.insert_inst_no_result(Jump::new(is, header));

        fb.switch_to_block(header);
        let one = fb.make_imm_value(Immediate::I32(1));
        let event_index = fb.insert_inst(Phi::new(is, vec![(one, initialize_block)]), Type::I32);
        let mut coalesced = Vec::with_capacity(event_fields);
        for (value, ty) in initial.into_iter().zip(event_tys.iter().copied()) {
            coalesced.push(fb.insert_inst(Phi::new(is, vec![(value, initialize_block)]), ty));
        }
        let more = fb.insert_inst(Lt::new(is, event_index, event_count), Type::I1);
        fb.insert_inst_no_result(Br::new(is, more, body, done));

        fb.switch_to_block(body);
        let stride = fb.make_imm_value(Immediate::I32(event_stride));
        let byte_offset = fb.insert_inst(Mul::new(is, event_index, stride), Type::I32);
        let event_ptr = fb.insert_inst(Add::new(is, events_ptr, byte_offset), Type::I32);
        let mut incoming = Vec::with_capacity(event_fields);
        for (index, ty) in event_tys.iter().copied().enumerate() {
            let address = if index == 0 {
                event_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32((index as i32) * 4));
                fb.insert_inst(Add::new(is, event_ptr, offset), Type::I32)
            };
            incoming.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        let mut next = incoming;
        for index in accumulate_f32_fields {
            next[*index] =
                fb.insert_inst(Fadd::new(is, coalesced[*index], next[*index]), Type::F32);
        }
        let next_index = fb.insert_inst(Add::new(is, event_index, one), Type::I32);
        let body_block = fb
            .current_block()
            .expect("resident batch body has a current block");
        fb.append_phi_arg(event_index, next_index, body_block);
        for (phi, value) in coalesced.iter().copied().zip(next) {
            fb.append_phi_arg(phi, value, body_block);
        }
        fb.insert_inst_no_result(Jump::new(is, header));

        fb.switch_to_block(done);
        let mut tags_valid = None;
        for (index, limit) in &transition.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("batched enum tag type validated above"),
                );
                let equal = fb.insert_inst(CmpEq::new(is, coalesced[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            let tag_valid = tag_valid.expect("nonzero batched enum limit validated above");
            tags_valid = Some(match tags_valid {
                Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                None => tag_valid,
            });
        }
        if let Some(tags_valid) = tags_valid {
            fb.insert_inst_no_result(Br::new(is, tags_valid, invoke, invalid));
        } else {
            fb.insert_inst_no_result(Jump::new(is, invoke));
        }

        fb.switch_to_block(invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        args.extend(coalesced);
        let mut resource_index = 2usize;
        let mut state_index = 0usize;
        for is_resource in &transition.actor_param_is_resource {
            if *is_resource {
                args.push(wrapper_values[resource_index]);
                resource_index += 1;
            } else {
                let global = state_globals[state_index];
                let ty = result_tys[state_index];
                let address = fb.make_global_value(global);
                args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                state_index += 1;
            }
        }
        let results = fb.insert_call_results(*callee, args);
        if results.len() != result_tys.len() {
            return Err(LowerError::Internal(
                "resident transition call result arity changed after signature inspection"
                    .to_owned(),
            ));
        }
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
            .zip(results.iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        fb.insert_return_values(&results);

        // Slow path for a heterogeneous batch. The browser still crosses the
        // Wasm boundary once, but the generated wrapper folds each untouched
        // event through the same authored Fe transition in source order. This
        // is deliberately generic over event meaning; only the semantically
        // derived homogeneous coalescing tag chooses the fast path above.
        fb.switch_to_block(sequential_header);
        let sequential_index = fb.insert_inst(Phi::new(is, vec![(zero, classify_done)]), Type::I32);
        let sequential_more = fb.insert_inst(Lt::new(is, sequential_index, event_count), Type::I1);
        fb.insert_inst_no_result(Br::new(
            is,
            sequential_more,
            sequential_body,
            sequential_done,
        ));

        fb.switch_to_block(sequential_body);
        let stride = fb.make_imm_value(Immediate::I32(event_stride));
        let byte_offset = fb.insert_inst(Mul::new(is, sequential_index, stride), Type::I32);
        let event_ptr = fb.insert_inst(Add::new(is, events_ptr, byte_offset), Type::I32);
        let mut sequential_event = Vec::with_capacity(event_fields);
        for (index, ty) in event_tys.iter().copied().enumerate() {
            let address = if index == 0 {
                event_ptr
            } else {
                let offset = fb.make_imm_value(Immediate::I32((index as i32) * 4));
                fb.insert_inst(Add::new(is, event_ptr, offset), Type::I32)
            };
            sequential_event.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        let mut sequential_tags_valid = None;
        for (index, limit) in &transition.event_tag_limits {
            let mut tag_valid = None;
            for tag in 0..*limit {
                let expected = fb.make_imm_value(
                    fieldless_tag_immediate(event_tys[*index], tag)
                        .expect("batched enum tag type validated above"),
                );
                let equal =
                    fb.insert_inst(CmpEq::new(is, sequential_event[*index], expected), Type::I1);
                tag_valid = Some(match tag_valid {
                    Some(previous) => fb.insert_inst(Or::new(is, previous, equal), Type::I1),
                    None => equal,
                });
            }
            let tag_valid = tag_valid.expect("nonzero batched enum limit validated above");
            sequential_tags_valid = Some(match sequential_tags_valid {
                Some(previous) => fb.insert_inst(And::new(is, previous, tag_valid), Type::I1),
                None => tag_valid,
            });
        }
        if let Some(tags_valid) = sequential_tags_valid {
            fb.insert_inst_no_result(Br::new(is, tags_valid, sequential_invoke, invalid));
        } else {
            fb.insert_inst_no_result(Jump::new(is, sequential_invoke));
        }

        fb.switch_to_block(sequential_invoke);
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        args.extend(sequential_event);
        let mut resource_index = 2usize;
        let mut state_index = 0usize;
        for is_resource in &transition.actor_param_is_resource {
            if *is_resource {
                args.push(wrapper_values[resource_index]);
                resource_index += 1;
            } else {
                let global = state_globals[state_index];
                let ty = result_tys[state_index];
                let address = fb.make_global_value(global);
                args.push(fb.insert_inst(Mload::new(is, address, ty), ty));
                state_index += 1;
            }
        }
        let results = fb.insert_call_results(*callee, args);
        for ((global, ty), value) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
            .zip(results.iter().copied())
        {
            let address = fb.make_global_value(global);
            fb.insert_inst_no_result(Mstore::new(is, address, value, ty));
        }
        let one = fb.make_imm_value(Immediate::I32(1));
        let next_sequential_index = fb.insert_inst(Add::new(is, sequential_index, one), Type::I32);
        let sequential_invoke_block = fb
            .current_block()
            .expect("resident sequential batch invocation has a current block");
        fb.append_phi_arg(
            sequential_index,
            next_sequential_index,
            sequential_invoke_block,
        );
        fb.insert_inst_no_result(Jump::new(is, sequential_header));

        fb.switch_to_block(sequential_done);
        let mut final_state = Vec::with_capacity(result_tys.len());
        for (global, ty) in state_globals
            .iter()
            .copied()
            .zip(result_tys.iter().copied())
        {
            let address = fb.make_global_value(global);
            final_state.push(fb.insert_inst(Mload::new(is, address, ty), ty));
        }
        fb.insert_return_values(&final_state);
        fb.seal_all();
        fb.finish();
        Ok(ResidentActorStorage {
            result_tys,
            state_globals,
            state_initialized,
            state_tag_limits: transition.state_tag_limits.clone(),
        })
    }

    fn synthesize_canonical_lane(&mut self, lane: &crate::CanonicalLane) -> Result<(), LowerError> {
        let export = lane.export.as_deref().ok_or_else(|| {
            LowerError::Internal(format!(
                "host-effect lane `{}` reached canonical Wasm lowering",
                lane.name
            ))
        })?;
        fn flatten(
            layout: &crate::CanonicalLayout,
            base: u32,
            leaves: &mut Vec<(u32, Type)>,
            descriptors: &mut Vec<(u32, u32, u32, u32, u32)>,
        ) -> Result<(), LowerError> {
            use crate::CanonicalShape;
            let ty = match &layout.shape {
                CanonicalShape::Bool => Some(Type::I1),
                CanonicalShape::U8 => Some(Type::I8),
                CanonicalShape::I32 | CanonicalShape::U32 => Some(Type::I32),
                CanonicalShape::I64 | CanonicalShape::U64 => Some(Type::I64),
                CanonicalShape::F32 => Some(Type::F32),
                CanonicalShape::Record { fields } => {
                    for field in fields {
                        let offset = base.checked_add(field.offset).ok_or_else(|| {
                            LowerError::Unsupported(
                                "canonical record field offset overflow".to_owned(),
                            )
                        })?;
                        flatten(&field.layout, offset, leaves, descriptors)?;
                    }
                    None
                }
                CanonicalShape::Variant { .. } => {
                    return Err(LowerError::Unsupported(
                        "canonical variants require wasm32 enum runtime-class lowering".to_owned(),
                    ));
                }
                CanonicalShape::Bytes {
                    pointer_offset,
                    length_offset,
                }
                | CanonicalShape::String {
                    pointer_offset,
                    length_offset,
                    ..
                } => {
                    let pointer_offset = base.checked_add(*pointer_offset).ok_or_else(|| {
                        LowerError::Unsupported(
                            "canonical descriptor pointer offset overflow".to_owned(),
                        )
                    })?;
                    let length_offset = base.checked_add(*length_offset).ok_or_else(|| {
                        LowerError::Unsupported(
                            "canonical descriptor length offset overflow".to_owned(),
                        )
                    })?;
                    leaves.push((pointer_offset, Type::I32));
                    leaves.push((length_offset, Type::I32));
                    descriptors.push((pointer_offset, length_offset, 1, u32::MAX, 1));
                    None
                }
                CanonicalShape::List {
                    max,
                    stride,
                    pointer_offset,
                    length_offset,
                    ..
                } => {
                    let pointer_offset = base.checked_add(*pointer_offset).ok_or_else(|| {
                        LowerError::Unsupported("canonical list pointer offset overflow".to_owned())
                    })?;
                    let length_offset = base.checked_add(*length_offset).ok_or_else(|| {
                        LowerError::Unsupported("canonical list length offset overflow".to_owned())
                    })?;
                    if *stride != 4 || *max > u32::MAX / *stride {
                        return Err(LowerError::Unsupported(
                            "canonical list has unsafe stride or maximum".to_owned(),
                        ));
                    }
                    leaves.push((pointer_offset, Type::I32));
                    leaves.push((length_offset, Type::I32));
                    descriptors.push((pointer_offset, length_offset, *stride, *max, *stride));
                    None
                }
            };
            if let Some(ty) = ty {
                leaves.push((base, ty));
            }
            Ok(())
        }

        let has_variant = canonical_layout_contains_variant(&lane.request)
            || canonical_layout_contains_variant(&lane.response);
        let (request_plan, response_plan) = if has_variant {
            (
                Some(canonical_scalar_value_plan(
                    &lane.request,
                    0,
                    "canonical_request",
                )?),
                Some(canonical_scalar_value_plan(
                    &lane.response,
                    0,
                    "canonical_response",
                )?),
            )
        } else {
            (None, None)
        };
        let mut request = Vec::new();
        let mut response = Vec::new();
        let mut request_descriptors = Vec::new();
        let mut response_descriptors = Vec::new();
        if !has_variant {
            flatten(&lane.request, 0, &mut request, &mut request_descriptors)?;
            flatten(&lane.response, 0, &mut response, &mut response_descriptors)?;
        }
        // Input descriptors remain borrowed views into caller-owned memory.
        let _ = request_descriptors;
        let mut request_tys = Vec::new();
        let mut response_tys = Vec::new();
        if let Some(plan) = &request_plan {
            plan.append_flat_types(&mut request_tys);
        } else {
            request_tys.extend(request.iter().map(|(_, ty)| *ty));
        }
        if let Some(plan) = &response_plan {
            plan.append_flat_types(&mut response_tys);
        } else {
            response_tys.extend(response.iter().map(|(_, ty)| *ty));
        }
        if request_tys.is_empty() || response_tys.is_empty() {
            return Err(LowerError::Unsupported(
                "canonical wrapper records must contain scalar leaves".to_owned(),
            ));
        }

        let candidates = self
            .func_map
            .iter()
            .filter(|(instance, _)| self.function_symbol(**instance) == lane.name)
            .map(|(_, func_ref)| *func_ref)
            .collect::<Vec<_>>();
        let [callee] = candidates.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "canonical lane `{}` must select exactly one lowered Fe entry (found {})",
                lane.name,
                candidates.len()
            )));
        };
        let signature_matches = self.builder.sig(*callee, |signature| {
            signature.args() == request_tys && signature.ret_tys() == response_tys
        });
        if !signature_matches {
            return Err(LowerError::Unsupported(format!(
                "canonical lane `{}` flattened signature does not match selected Fe entry",
                lane.name
            )));
        }

        let wrapper = self
            .builder
            .declare_function(Signature::new_single(
                export,
                Linkage::Public,
                &[Type::I32],
                Type::I32,
            ))
            .map_err(|error| {
                LowerError::Internal(format!(
                    "failed to declare canonical wrapper `{}`: {error}",
                    export
                ))
            })?;
        let is = self.isa.inst_set();
        let mut fb = self.builder.func_builder::<InstInserter>(wrapper);
        let entry = fb.append_block();
        fb.switch_to_block(entry);
        let request_ptr = fb.args()[0];
        let mut args = smallvec1::SmallVec::<[ValueId; 8]>::new();
        if let Some(plan) = &request_plan {
            args.extend(load_canonical_scalar_value(&mut fb, is, request_ptr, plan)?);
        } else {
            for (offset, ty) in &request {
                let addr = canonical_offset_address(&mut fb, is, request_ptr, *offset);
                args.push(fb.insert_inst(Mload::new(is, addr, *ty), *ty));
            }
        }
        let results = fb.insert_call_results(*callee, args);
        if results.len() != response_tys.len() {
            return Err(LowerError::Internal(
                "canonical wrapper call result arity changed after signature check".to_owned(),
            ));
        }
        let allocation_size = lane
            .response
            .size
            .checked_add(lane.response.align - 1)
            .and_then(|size| i32::try_from(size).ok())
            .ok_or_else(|| {
                LowerError::Unsupported(
                    "canonical aligned response allocation exceeds Wasm i32".to_owned(),
                )
            })?;
        let allocation_size = fb.make_imm_value(Immediate::I32(allocation_size));
        let raw_response_ptr = fb.insert_inst(MemAllocDynamic::new(is, allocation_size), Type::I32);
        let response_ptr = if lane.response.align == 1 {
            raw_response_ptr
        } else {
            // MemAllocDynamic's Wasm bridge requests byte alignment. Allocate
            // enough slack and align the exposed canonical response pointer.
            // Canonical layout construction already guarantees power-of-two
            // alignment; the arena itself is bounded well below i32::MAX.
            let align_minus_one =
                fb.make_imm_value(Immediate::I32((lane.response.align - 1) as i32));
            let biased = fb.insert_inst(Add::new(is, raw_response_ptr, align_minus_one), Type::I32);
            let mask = fb.make_imm_value(Immediate::I32(-(lane.response.align as i32)));
            fb.insert_inst(And::new(is, biased, mask), Type::I32)
        };
        if response_plan.is_some() {
            // The canonical tagged-union contract makes inactive storage
            // deterministic. Arena reset reuses bytes, so zero the complete
            // response record before storing the validated active payload.
            zero_canonical_memory(&mut fb, is, response_ptr, lane.response.size)?;
        }
        let mut copied_pointers = HashMap::new();
        for (pointer_offset, length_offset, stride, max, alignment) in response_descriptors {
            let pointer_index = response
                .iter()
                .position(|(offset, _)| *offset == pointer_offset)
                .ok_or_else(|| {
                    LowerError::Internal(
                        "canonical descriptor pointer leaf was not flattened".to_owned(),
                    )
                })?;
            let length_index = response
                .iter()
                .position(|(offset, _)| *offset == length_offset)
                .ok_or_else(|| {
                    LowerError::Internal(
                        "canonical descriptor length leaf was not flattened".to_owned(),
                    )
                })?;
            let source = results[pointer_index];
            let length = results[length_index];
            if stride > 1 {
                let maximum = fb.make_imm_value(Immediate::I32(max as i32));
                let too_long = fb.insert_inst(Lt::new(is, maximum, length), Type::I1);
                let valid = fb.append_block();
                let invalid = fb.append_block();
                fb.insert_inst_no_result(Br::new(is, too_long, invalid, valid));
                fb.switch_to_block(invalid);
                fb.insert_inst_no_result(Unreachable::new(is));
                fb.switch_to_block(valid);
                // The pointer is semantically ignored for an empty list, just
                // as it is by the JS decoder. Non-empty typed payloads must be
                // naturally aligned before the byte-copy loop can read them.
                let zero = fb.make_imm_value(Immediate::I32(0));
                let empty = fb.insert_inst(CmpEq::new(is, length, zero), Type::I1);
                let aligned_block = fb.append_block();
                let check_alignment = fb.append_block();
                fb.insert_inst_no_result(Br::new(is, empty, aligned_block, check_alignment));
                fb.switch_to_block(check_alignment);
                let mask = fb.make_imm_value(Immediate::I32((alignment - 1) as i32));
                let low_bits = fb.insert_inst(And::new(is, source, mask), Type::I32);
                let aligned = fb.insert_inst(CmpEq::new(is, low_bits, zero), Type::I1);
                let misaligned = fb.append_block();
                fb.insert_inst_no_result(Br::new(is, aligned, aligned_block, misaligned));
                fb.switch_to_block(misaligned);
                fb.insert_inst_no_result(Unreachable::new(is));
                fb.switch_to_block(aligned_block);
            }
            let byte_length = if stride == 1 {
                length
            } else {
                let stride = fb.make_imm_value(Immediate::I32(stride as i32));
                fb.insert_inst(Mul::new(is, length, stride), Type::I32)
            };
            let allocation_length = if alignment == 1 {
                byte_length
            } else {
                let slack = fb.make_imm_value(Immediate::I32((alignment - 1) as i32));
                fb.insert_inst(Add::new(is, byte_length, slack), Type::I32)
            };
            let raw_destination =
                fb.insert_inst(MemAllocDynamic::new(is, allocation_length), Type::I32);
            let destination = if alignment == 1 {
                raw_destination
            } else {
                let slack = fb.make_imm_value(Immediate::I32((alignment - 1) as i32));
                let biased = fb.insert_inst(Add::new(is, raw_destination, slack), Type::I32);
                let mask = fb.make_imm_value(Immediate::I32(-(alignment as i32)));
                fb.insert_inst(And::new(is, biased, mask), Type::I32)
            };

            // Returned descriptors are borrowed inside Fe. The canonical
            // wrapper establishes explicit host ownership by copying exactly
            // `len * stride` bytes into its arena before publishing the
            // descriptor. The length leaf remains an element count.
            let copy_entry = fb
                .current_block()
                .expect("canonical wrapper copy requires a current block");
            let copy_header = fb.append_block();
            let copy_body = fb.append_block();
            let copy_done = fb.append_block();
            fb.insert_inst_no_result(Jump::new(is, copy_header));
            fb.switch_to_block(copy_header);
            let zero = fb.make_imm_value(Immediate::I32(0));
            let index = fb.insert_inst(Phi::new(is, vec![(zero, copy_entry)]), Type::I32);
            let more = fb.insert_inst(Lt::new(is, index, byte_length), Type::I1);
            fb.insert_inst_no_result(Br::new(is, more, copy_body, copy_done));

            fb.switch_to_block(copy_body);
            let source_byte = fb.insert_inst(Add::new(is, source, index), Type::I32);
            let destination_byte = fb.insert_inst(Add::new(is, destination, index), Type::I32);
            let byte = fb.insert_inst(Mload::new(is, source_byte, Type::I8), Type::I8);
            fb.insert_inst_no_result(Mstore::new(is, destination_byte, byte, Type::I8));
            let one = fb.make_imm_value(Immediate::I32(1));
            let next = fb.insert_inst(Add::new(is, index, one), Type::I32);
            let copy_back = fb
                .current_block()
                .expect("canonical wrapper copy body requires a current block");
            fb.append_phi_arg(index, next, copy_back);
            fb.insert_inst_no_result(Jump::new(is, copy_header));
            fb.switch_to_block(copy_done);
            copied_pointers.insert(pointer_offset, destination);
        }
        if let Some(plan) = &response_plan {
            let mut cursor = 0;
            store_canonical_scalar_value(&mut fb, is, response_ptr, plan, &results, &mut cursor)?;
            if cursor != results.len() {
                return Err(LowerError::Internal(
                    "canonical scalar response left unconsumed value lanes".to_owned(),
                ));
            }
        } else {
            for ((offset, ty), value) in response.into_iter().zip(results) {
                let value = copied_pointers.get(&offset).copied().unwrap_or(value);
                let addr = canonical_offset_address(&mut fb, is, response_ptr, offset);
                fb.insert_inst_no_result(Mstore::new(is, addr, value, ty));
            }
        }
        fb.insert_return(response_ptr);
        fb.seal_all();
        fb.finish();
        Ok(())
    }
}

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
    compile_runtime_package_wasm_with_canonical_lanes(
        db,
        package,
        &[],
        &[],
        None,
        &[],
        None,
        None,
        &[],
        &[],
    )
}


#[cfg(feature = "sonatina-indirect-calls")]
pub fn compile_runtime_package_wasm_with_guest_callbacks(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    callbacks: &[crate::guest_callbacks::ResolvedGuestCallback],
) -> Result<(Module, HashMap<String, String>), LowerError> {
    let isa = create_wasm32_isa();
    let mut lowerer = lower_portable_bodies(
        &isa,
        db,
        package,
        HashSet::new(),
        &[],
    )?;
    lowerer.synthesize_guest_callbacks(callbacks)?;
    let import_modules = lowerer.import_modules();
    Ok((lowerer.finish(), import_modules))
}

pub(crate) fn compile_runtime_package_wasm_with_canonical_lanes(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    canonical_lanes: &[crate::CanonicalLane],
    export_aliases: &[(String, String)],
    resident_transition: Option<&super::super::WasmResidentTransition>,
    resident_aux_transitions: &[super::super::WasmResidentTransition],
    resident_initializer: Option<&super::super::WasmResidentInitializer>,
    resident_projection: Option<&super::super::WasmResidentProjection>,
    resident_policies: &[super::super::WasmResidentPolicy],
    fixed_i32_exports: &[(String, i32)],
) -> Result<(Module, HashMap<String, String>), LowerError> {
    debug_assert!({
        let kind = crate::dispatch::DispatchKind::for_backend(crate::BackendKind::Wasm);
        matches!(kind, crate::dispatch::DispatchKind::Export) && kind.entries_invoked_directly()
    }, "wasm lowering must realize the Export DispatchKind (entries invoked directly)");
    validate_wasm_host_results(db, package)?;
    let isa = create_wasm32_isa();
    let mut wrapped_lane_names: HashSet<String> = canonical_lanes
        .iter()
        .map(|lane| lane.name.clone())
        .collect();
    if let Some(transition) = resident_transition {
        wrapped_lane_names.insert(transition.source.clone());
    }
    for transition in resident_aux_transitions {
        wrapped_lane_names.insert(transition.source.clone());
    }
    if let Some(initializer) = resident_initializer {
        wrapped_lane_names.insert(initializer.source.clone());
    }
    if let Some(projection) = resident_projection {
        wrapped_lane_names.insert(projection.source.clone());
    }
    for policy in resident_policies {
        let assigned = assign_sonatina_function_symbols(db, package);
        let policy_symbols = assigned
            .into_iter()
            .filter_map(|(instance, symbol)| {
                (mir::runtime_instance_symbol_key(db, instance) == policy.callee_instance_key)
                    .then_some(symbol)
            })
            .collect::<Vec<_>>();
        let [symbol] = policy_symbols.as_slice() else {
            return Err(LowerError::Unsupported(format!(
                "resident policy instance `{}` must select exactly one assigned Fe symbol (found {})",
                policy.callee_instance_key,
                policy_symbols.len()
            )));
        };
        wrapped_lane_names.insert(symbol.clone());
    }
    let mut lowerer = lower_portable_bodies(
        &isa, db, package, wrapped_lane_names, export_aliases,
    )?;
    for lane in canonical_lanes {
        lowerer.synthesize_canonical_lane(lane)?;
    }
    if let Some(transition) = resident_transition {
        let state = lowerer.synthesize_resident_transition(
            transition,
            resident_initializer,
            resident_projection,
        )?;
        for auxiliary in resident_aux_transitions {
            lowerer.synthesize_aux_resident_transition(auxiliary, &state)?;
        }
    } else if resident_initializer.is_some()
        || resident_projection.is_some()
        || !resident_aux_transitions.is_empty()
    {
        return Err(LowerError::Unsupported(
            "resident actor initializer, projection, and auxiliary transitions require a primary resident transition".to_owned(),
        ));
    }
    for (policy_index, policy) in resident_policies.iter().enumerate() {
        lowerer.synthesize_resident_policy(policy, policy_index)?;
    }
    for (export, value) in fixed_i32_exports {
        lowerer.synthesize_fixed_i32_export(export, *value)?;
    }
    let import_modules = lowerer.import_modules();
    Ok((lowerer.finish(), import_modules))
}


fn validate_wasm_host_results(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
) -> Result<(), LowerError> {
    // Reject unsupported indirect host results before constructing any
    // Sonatina signatures. A local wrapper may itself return the authored enum
    // and appear before the import in package order, so gating inside
    // `declare_functions` is already too late.
    for function in package.functions(db) {
        let instance = function.instance(db);
        let Some(name) = mir::host_import_name(db, instance) else {
            continue;
        };
        let Some(descriptor) = mir::indirect_host_result(db, instance) else {
            continue;
        };
        let mut missing = Vec::new();
        if descriptor.requires_realloc {
            missing.push("realloc");
        }
        if descriptor.requires_post_return {
            missing.push("post-return");
        }
        if !missing.is_empty() {
            return Err(LowerError::Unsupported(format!(
                "extern host import `{name}` uses indirect host result codec `{}`, but the Wasm \
                 backend is missing required capabilities: {}",
                mir::IndirectHostResult::FE_HOST_WASM_PROTOCOL,
                missing.join(", ")
            )));
        }
    }

    Ok(())
}
