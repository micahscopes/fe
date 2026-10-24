use std::collections::{HashMap, HashSet};

use common::ingot::IngotKind;
use driver::DriverDataBase;
use hir::{
    analysis::{
        name_resolution::{PathRes, resolve_path},
        semantic::{
            CallSiteId, EvalOutcome, SExpr, SStmtKind, SemConstScalar, SemConstValue, SemOrigin,
            SemanticInstanceKey, eval_const_ref, get_or_build_semantic_instance,
            resolve_semantic_const_ref, root_semantic_instance_key,
        },
        ty::{
            abi_ty::{self, AbiComponent, AbiTypeDesc, ParsedFunctionSignature},
            adt_def::AdtRef,
            assoc_const::AssocConstUse,
            corelib::{RuntimeBuiltinFuncKind, runtime_builtin_func_kind},
            evidence::UseEvidence,
            trait_def::TraitInstId,
            trait_resolution::PredicateListId,
            ty_check::{BodyOwner, ConstRef},
            ty_def::{PrimTy, TyBase, TyData, TyId},
        },
    },
    hir_def::{FieldDefListId, IdentId, PathId, Struct, TopLevelMod, Trait, scope_graph::ScopeId},
};
use serde::Serialize;
use tiny_keccak::{Hasher, Keccak};

#[derive(Serialize)]
pub struct AbiEntry {
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<AbiParam>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<AbiParam>>,
    #[serde(rename = "stateMutability", skip_serializing_if = "Option::is_none")]
    pub state_mutability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anonymous: Option<bool>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AbiParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<AbiParam>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NamedAbiParamDesc {
    name: String,
    indexed: Option<bool>,
    desc: AbiTypeDesc,
}

fn component_to_param(component: AbiComponent) -> AbiParam {
    AbiParam {
        name: component.name,
        ty: component.ty,
        indexed: None,
        components: component
            .components
            .map(|components| components.into_iter().map(component_to_param).collect()),
    }
}

impl NamedAbiParamDesc {
    fn into_param(self) -> AbiParam {
        AbiParam {
            name: self.name,
            ty: self.desc.abi_type,
            indexed: self.indexed,
            components: self
                .desc
                .components
                .map(|components| components.into_iter().map(component_to_param).collect()),
        }
    }
}

pub struct AbiResult {
    pub json: String,
    pub entry_count: usize,
    pub warnings: Vec<String>,
}

enum RecvArmAbiEmission {
    Emit(AbiEntry),
    Skip(String),
}

/// Generate an Ethereum-compatible JSON ABI for a given contract inside `top_mod`.
///
/// Returns `Ok(None)` if the contract is not found in this module (useful for
/// ingot builds where a contract may live in a different module).
pub fn generate_contract_abi(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    contract_name: &str,
) -> Result<Option<AbiResult>, String> {
    let Some(contract) = top_mod
        .all_contracts(db)
        .iter()
        .find(|c| {
            c.name(db)
                .to_opt()
                .is_some_and(|n| n.data(db).as_str() == contract_name)
        })
        .copied()
    else {
        return Ok(None);
    };

    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    if let Some(init) = contract.init(db) {
        let inputs = init_params_to_abi_params(db, contract)?
            .into_iter()
            .map(NamedAbiParamDesc::into_param)
            .collect();
        let state_mutability = if init.is_payable(db) {
            "payable"
        } else {
            "nonpayable"
        };
        entries.push(AbiEntry {
            entry_type: "constructor".to_string(),
            name: None,
            inputs: Some(inputs),
            outputs: None,
            state_mutability: Some(state_mutability.to_string()),
            anonymous: None,
        });
    }

    let sol_ty = resolve_sol_abi_ty(db, contract.scope())?;
    for recv in contract.recv_views(db) {
        for arm_view in recv.arms(db) {
            match recv_arm_to_abi_entry(db, arm_view, sol_ty) {
                Ok(RecvArmAbiEmission::Emit(entry)) => entries.push(entry),
                Ok(RecvArmAbiEmission::Skip(warning)) => warnings.push(warning),
                Err(e) => return Err(e),
            }
        }
    }

    // Error discovery admits the contract's call graph and checks each
    // reachable selector before any event or error entry is emitted.
    let error_types = collect_contract_error_types(db, contract)?;
    for event_ty in collect_contract_event_types(db, contract)? {
        entries.push(event_type_to_abi_entry(db, contract.scope(), event_ty)?);
    }

    for error_ty in error_types {
        let entry = error_type_to_abi_entry(db, error_ty)?;
        // Solidity's built-in errors are implicit. Declaring them as custom
        // errors makes consumers such as ethers reject the entire ABI.
        if let Some([input]) = entry.inputs.as_deref()
            && matches!(
                (entry.name.as_deref(), input.ty.as_str()),
                (Some("Panic"), "uint256") | (Some("Error"), "string")
            )
        {
            continue;
        }
        entries.push(entry);
    }

    let entry_count = entries.len();
    let json = serde_json::to_string_pretty(&entries)
        .map_err(|e| format!("JSON serialization error: {e}"))?;
    Ok(Some(AbiResult {
        json,
        entry_count,
        warnings,
    }))
}

fn recv_arm_to_abi_entry(
    db: &DriverDataBase,
    arm_view: hir::semantic::RecvArmView<'_>,
    sol_ty: TyId<'_>,
) -> Result<RecvArmAbiEmission, String> {
    let arm = arm_view
        .arm(db)
        .ok_or_else(|| "missing recv arm during ABI generation".to_string())?;
    if arm_view.is_fallback(db) {
        return Ok(RecvArmAbiEmission::Emit(AbiEntry {
            entry_type: "fallback".to_string(),
            name: None,
            inputs: None,
            outputs: None,
            state_mutability: Some(if arm.is_payable(db) {
                "payable".to_string()
            } else {
                "nonpayable".to_string()
            }),
            anonymous: None,
        }));
    }

    let variant_ty = arm_view.variant_ty(db);
    let variant_struct = struct_from_ty(db, variant_ty).ok_or_else(|| {
        format!(
            "recv arm type `{}` is not a struct",
            variant_ty.pretty_print(db)
        )
    })?;
    let variant_name = variant_struct
        .name(db)
        .to_opt()
        .map_or_else(|| "<unknown>".to_string(), |name| name.data(db).to_string());
    if !variant_has_canonical_json_abi_shape(db, variant_struct) {
        return Ok(RecvArmAbiEmission::Skip(format!(
            "skipping recv arm `{variant_name}`: ABI shape is not compiler-known for manual `MsgVariant` impls; only `msg`-generated variants are emitted"
        )));
    }
    let abi_info = arm_view.abi_info(db, sol_ty);
    let Some(selector_signature) = abi_info.selector_signature.as_deref() else {
        return Ok(RecvArmAbiEmission::Skip(format!(
            "skipping recv arm `{variant_name}`: selector signature is unknown; \
             use `#[selector = sol(\"name(types)\")]` to include it in the ABI"
        )));
    };

    let input_descs = struct_ty_to_abi_param_descs(db, variant_struct, variant_ty, |_| None)?;
    // A tuple return is a parameter list (Solidity multi-value `returns`), so
    // each element becomes its own output, matching its runtime encoding.
    let output_tys = match abi_info.ret_ty {
        Some(ret_ty) if ret_ty.is_tuple(db) => ret_ty.field_types(db),
        Some(ret_ty) => vec![ret_ty],
        None => Vec::new(),
    };
    let outputs = output_tys
        .into_iter()
        .map(|ty| {
            Ok(NamedAbiParamDesc {
                name: String::new(),
                indexed: None,
                desc: semantic_ty_to_abi_desc(db, ty)?,
            }
            .into_param())
        })
        .collect::<Result<Vec<_>, String>>()?;

    let selector_value = abi_info.selector_value.ok_or_else(|| {
        format!(
            "cannot emit JSON ABI for `{selector_signature}`: selector value could not be resolved"
        )
    })?;
    let parsed_signature = parse_function_signature(selector_signature)?;
    ensure_selector_matches_signature(
        selector_signature,
        &parsed_signature,
        &input_descs,
        selector_value,
    )?;

    Ok(RecvArmAbiEmission::Emit(AbiEntry {
        entry_type: "function".to_string(),
        name: Some(parsed_signature.name),
        inputs: Some(
            input_descs
                .into_iter()
                .map(NamedAbiParamDesc::into_param)
                .collect(),
        ),
        outputs: Some(outputs),
        state_mutability: Some(derive_state_mutability(db, arm_view)),
        anonymous: None,
    }))
}

/// The inputs of an event in declaration order. A field is indexed when it
/// carries the compiler's `#[indexed]` mark or has the canonical
/// `std::evm::Indexed<T>` type, whose Solidity type is that of `T`. The owner
/// is identified by its resolved declaration, never by spelling.
fn event_param_descs<'db>(
    db: &'db DriverDataBase,
    scope: ScopeId<'db>,
    struct_: Struct<'db>,
    ty: TyId<'db>,
) -> Result<Vec<NamedAbiParamDesc>, String> {
    let indexed_owner = hir::analysis::ty::corelib::resolve_lib_type_path(
        db,
        scope,
        "std::evm::event::Indexed",
    )
    .and_then(|owner| owner.adt_def(db))
    .ok_or("cannot resolve canonical `std::evm::Indexed` type")?;
    let hir_fields = struct_.hir_fields(db).data(db);
    let field_tys = ty.field_types(db);
    if hir_fields.len() != field_tys.len() {
        return Err(format!(
            "field count mismatch: {} HIR fields vs {} semantic fields",
            hir_fields.len(),
            field_tys.len()
        ));
    }
    let mut params = Vec::with_capacity(field_tys.len());
    for (field, field_ty) in hir_fields.iter().zip(field_tys) {
        let wrapped = field_ty.adt_def(db) == Some(indexed_owner);
        let value_ty = if wrapped {
            *field_ty.generic_args(db).first().ok_or(
                "canonical `Indexed` type is missing its value type",
            )?
        } else {
            field_ty
        };
        params.push(NamedAbiParamDesc {
            name: field
                .name
                .to_opt()
                .map(|ident| ident.data(db).to_string())
                .unwrap_or_default(),
            indexed: Some(wrapped || field.is_event_indexed),
            desc: semantic_ty_to_abi_desc(db, value_ty)?,
        });
    }
    let indexed = params.iter().filter(|param| param.indexed == Some(true)).count();
    if indexed > MAX_INDEXED_EVENT_FIELDS {
        return Err(format!(
            "event `{}` has {indexed} indexed fields; at most {MAX_INDEXED_EVENT_FIELDS} fit in a log",
            ty.pretty_print(db)
        ));
    }
    Ok(params)
}

/// Solidity allows three indexed topics after `topic0` in a non-anonymous event.
const MAX_INDEXED_EVENT_FIELDS: usize = 3;

fn event_type_to_abi_entry<'db>(
    db: &'db DriverDataBase,
    scope: ScopeId<'db>,
    ty: TyId<'db>,
) -> Result<AbiEntry, String> {
    let struct_ = struct_from_ty(db, ty).ok_or("event type is not a struct")?;
    let name = struct_
        .name(db)
        .to_opt()
        .ok_or("event struct is missing a name")?;
    let inputs = event_param_descs(db, scope, struct_, ty)?;
    Ok(AbiEntry {
        entry_type: "event".to_string(),
        name: Some(name.data(db).to_string()),
        inputs: Some(
            inputs
                .into_iter()
                .map(NamedAbiParamDesc::into_param)
                .collect(),
        ),
        outputs: None,
        state_mutability: None,
        anonymous: Some(false),
    })
}

fn error_type_to_abi_entry(db: &DriverDataBase, ty: TyId<'_>) -> Result<AbiEntry, String> {
    let struct_ = struct_from_ty(db, ty).ok_or("error type is not a struct")?;
    let name = struct_
        .name(db)
        .to_opt()
        .ok_or("error struct is missing a name")?;
    let inputs = struct_ty_to_abi_param_descs(db, struct_, ty, |_| None)?;
    Ok(AbiEntry {
        entry_type: "error".to_string(),
        name: Some(name.data(db).to_string()),
        inputs: Some(
            inputs
                .into_iter()
                .map(NamedAbiParamDesc::into_param)
                .collect(),
        ),
        outputs: None,
        state_mutability: None,
        anonymous: None,
    })
}

/// The instances execution enters a contract through: its init and each
/// receive arm.
fn contract_root_instances<'db>(
    db: &'db DriverDataBase,
    contract: hir::hir_def::Contract<'db>,
) -> Result<Vec<SemanticInstanceKey<'db>>, String> {
    let mut owners = Vec::new();
    if contract.init(db).is_some() {
        owners.push(BodyOwner::ContractInit { contract });
    }
    for recv in contract.recv_views(db) {
        for arm in recv.arms(db) {
            owners.push(BodyOwner::ContractRecvArm {
                contract,
                recv_idx: recv.index(db),
                arm_idx: arm.index(db),
            });
        }
    }
    owners
        .into_iter()
        .map(|owner| root_semantic_instance_key(db, owner).map_err(|error| format!("{error:?}")))
        .collect()
}

/// Evaluate the associated constant `name` of a goal the checker has already
/// proved, through that proof, as execution does. `what` names the constant
/// and `subject` the declaration it belongs to in diagnostics.
#[allow(clippy::too_many_arguments)]
fn eval_proved_trait_const<'db>(
    db: &'db DriverDataBase,
    body: hir::hir_def::Body<'db>,
    typed: &hir::analysis::ty::ty_check::TypedBody<'db>,
    expr: hir::hir_def::ExprId,
    goal: TraitInstId<'db>,
    proof: hir::analysis::ty::evidence::EvidenceId<'db>,
    name: &str,
    ty: TyId<'db>,
    what: &str,
    subject: &str,
) -> Result<hir::analysis::semantic::SemConstId<'db>, String> {
    let mut evidence = UseEvidence::default();
    evidence.record_dispatch(goal, Some(proof));
    let reference = ConstRef::TraitConst {
        use_: AssocConstUse::new(
            body.scope(),
            typed.assumptions(),
            goal,
            IdentId::new(db, name.to_string()),
        ),
        evidence,
    };
    let reference = resolve_semantic_const_ref(db, reference, ty, SemOrigin::Expr(expr))
        .ok_or_else(|| format!("missing checked {what} for `{subject}`"))?;
    match eval_const_ref(db, reference) {
        EvalOutcome::Ready(value) => Ok(value),
        failure => Err(format!(
            "{what} evaluation failed for `{subject}`: {failure:?}"
        )),
    }
}

/// Error reachability follows the same applied instances and proof transport
/// used by execution. The complete type remains attached to its field layout.
fn collect_contract_error_types<'db>(
    db: &'db DriverDataBase,
    contract: hir::hir_def::Contract<'db>,
) -> Result<Vec<TyId<'db>>, String> {
    let error_func = hir::analysis::ty::corelib::resolve_lib_func_path(
        db,
        contract.scope(),
        "std::evm::effects::revert_error",
    );
    let error_trait = hir::analysis::ty::corelib::resolve_lib_trait_path(
        db,
        contract.scope(),
        "core::error::ErrorVariant",
    );
    let sol_ty = resolve_sol_abi_ty(db, contract.scope())?;
    let mut pending = contract_root_instances(db, contract)
        .map_err(|error| format!("cannot prepare error ABI reachability: {error}"))?;
    let mut visited = HashSet::new();
    let mut seen = HashSet::new();
    let mut errors = Vec::new();
    while let Some(key) = pending.pop() {
        if !visited.insert(key) {
            continue;
        }
        let instance = get_or_build_semantic_instance(db, key);
        let admitted = instance
            .admitted_body(db)
            .map_err(|error| format!("cannot admit error ABI call graph: {error:?}"))?;
        let typed = key.typed_body(db);
        let Some(body) = typed.body() else { continue };
        // Lowering owns closure regions and implicit protocol calls. Inspect
        // only expression calls actually present in this admitted instance.
        let expression_calls = admitted
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter_map(|stmt| match &stmt.kind {
                SStmtKind::Assign {
                    expr:
                        SExpr::Call {
                            call_site: CallSiteId::Expr(expr),
                            callee,
                            ..
                        },
                    ..
                } => Some((*expr, callee.key)),
                _ => None,
            });
        for (expr, callee) in expression_calls {
            let partial = expr.data(db, body);
            let hir::hir_def::Partial::Present(hir::hir_def::Expr::Call(_, args)) = partial else {
                continue;
            };
            let Some(callable) = typed.callable_expr(expr) else {
                continue;
            };
            let hir::hir_def::CallableDef::Func(func) = callable.callable_def else {
                continue;
            };
            let is_revert = Some(func) == error_func;
            let is_panic =
                runtime_builtin_func_kind(db, func) == Some(RuntimeBuiltinFuncKind::PanicWithValue);
            if !is_revert && !is_panic {
                continue;
            }
            let Some(arg) = args.first() else { continue };
            let mut ty = typed.expr_ty(db, arg.expr);
            while let Some((_, inner)) = ty.as_capability(db) {
                ty = inner;
            }
            let Some(struct_) = struct_from_ty(db, ty) else {
                continue;
            };
            if is_panic {
                // This intrinsic declares no ErrorVariant bound. Its existing
                // compiler-issued error origin remains the supported contract.
                if !matches!(
                    hir::span::struct_ast(db, struct_),
                    hir::span::HirOrigin::Desugared(hir::span::DesugaredOrigin::Error(_))
                ) {
                    continue;
                }
            } else {
                let error_trait =
                    error_trait.ok_or("missing canonical ErrorVariant declaration")?;
                let goal = TraitInstId::new_simple(db, error_trait, vec![ty, sol_ty]);
                // The callee instance answers a closed slot the call does not
                // transport; its normalization scope gives that proof's ingot.
                let instance_scope = callee.impl_env(db).normalization_scope(db);
                let proved = callable
                    .proved_bound(db, instance_scope, goal)
                    .ok_or_else(|| {
                        format!(
                            "missing declaration-owned checked ErrorVariant proof for `{}`",
                            ty.pretty_print(db)
                        )
                    })?;
                if !proved.proof.is_closed(db) || proved.proof.answering(db, goal).is_none() {
                    return Err(format!(
                        "error ABI requires a closed checked ErrorVariant proof for `{}`",
                        ty.pretty_print(db)
                    ));
                }
                let inputs = struct_ty_to_abi_param_descs(db, struct_, ty, |_| None)?;
                let name = struct_
                    .name(db)
                    .to_opt()
                    .ok_or("error struct is missing a name")?;
                let signature = format!(
                    "{}({})",
                    name.data(db),
                    inputs
                        .iter()
                        .map(|input| input.desc.canonical_type.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                let value = eval_proved_trait_const(
                    db,
                    body,
                    typed,
                    expr,
                    goal,
                    proved.proof,
                    "SELECTOR",
                    TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::U32))),
                    "error SELECTOR",
                    &signature,
                )?;
                let selector = match value.value(db) {
                    SemConstValue::Scalar {
                        value: SemConstScalar::Int { value },
                        ..
                    } => value.to_string().parse::<u32>().ok(),
                    _ => None,
                }
                .ok_or_else(|| format!("error SELECTOR is not u32 for `{signature}`"))?;
                if selector != selector_for_signature(&signature) {
                    return Err(format!(
                        "error SELECTOR does not match canonical signature `{signature}`"
                    ));
                }
            }
            if seen.insert(ty) {
                errors.push(ty);
            }
        }
        pending.extend(admitted.callees().into_iter().map(|callee| callee.key));
    }
    Ok(errors)
}

/// Event reachability follows the admitted instance graph execution uses, as
/// error reachability does, so instance identity and the shared admission
/// limits bound it. Each body contributes its events in expression order,
/// before the instances its calls at that expression reach.
///
/// An emitted type is reported only with the checked `Event` proof of the call
/// that emits it. Its `TOPIC0` is evaluated through that proof and must be the
/// hash of the signature the JSON entry states, so a name or a hash written in
/// the source never decides what the ABI says.
fn collect_contract_event_types<'db>(
    db: &'db DriverDataBase,
    contract: hir::hir_def::Contract<'db>,
) -> Result<Vec<TyId<'db>>, String> {
    let mut events = Vec::new();
    let mut seen = HashSet::new();
    let mut visited = HashSet::new();
    let emit_traits = resolve_event_emit_traits(db, contract.scope());
    let roots = contract_root_instances(db, contract)
        .map_err(|error| format!("cannot prepare event ABI reachability: {error}"))?;
    for root in roots {
        collect_instance_event_types(
            db,
            contract.scope(),
            root,
            &emit_traits,
            &mut events,
            &mut seen,
            &mut visited,
        )?;
    }
    Ok(events)
}

fn collect_instance_event_types<'db>(
    db: &'db DriverDataBase,
    contract_scope: ScopeId<'db>,
    key: SemanticInstanceKey<'db>,
    emit_traits: &EventEmitTraits<'db>,
    out: &mut Vec<TyId<'db>>,
    seen: &mut HashSet<TyId<'db>>,
    visited: &mut HashSet<SemanticInstanceKey<'db>>,
) -> Result<(), String> {
    if !visited.insert(key) {
        return Ok(());
    }
    let admitted = get_or_build_semantic_instance(db, key)
        .admitted_body(db)
        .map_err(|error| format!("cannot admit event ABI call graph: {error:?}"))?;
    let mut expression_callees: HashMap<hir::hir_def::ExprId, Vec<SemanticInstanceKey<'db>>> =
        HashMap::new();
    let mut protocol_callees = Vec::new();
    for stmt in admitted.blocks.iter().flat_map(|block| &block.stmts) {
        let SStmtKind::Assign {
            expr: SExpr::Call {
                call_site, callee, ..
            },
            ..
        } = &stmt.kind
        else {
            continue;
        };
        match call_site {
            CallSiteId::Expr(expr) => expression_callees
                .entry(*expr)
                .or_default()
                .push(callee.key),
            CallSiteId::ForLoop(..) => protocol_callees.push(callee.key),
        }
    }
    let typed_body = key.typed_body(db);
    if let Some(body) = typed_body.body() {
        for (expr_id, partial_expr) in body.exprs(db).iter() {
            let hir::hir_def::Partial::Present(_) = partial_expr else {
                continue;
            };
            // Only calls this admitted instance performs can emit; a call in
            // an uninvoked closure region has no callee here.
            let Some(callees) = expression_callees.get(&expr_id) else {
                continue;
            };
            if let Some(&callee) = callees.first()
                && let Some(ty) =
                    checked_emitted_event(db, contract_scope, typed_body, body, expr_id, callee, emit_traits)?
                && seen.insert(ty)
            {
                out.push(ty);
            }
            for &callee in callees {
                collect_instance_event_types(
                    db,
                    contract_scope,
                    callee,
                    emit_traits,
                    out,
                    seen,
                    visited,
                )?;
            }
        }
    }
    for callee in protocol_callees {
        collect_instance_event_types(
            db,
            contract_scope,
            callee,
            emit_traits,
            out,
            seen,
            visited,
        )?;
    }
    Ok(())
}

struct EventEmitTraits<'db> {
    log_trait: Option<Trait<'db>>,
    event_trait: Option<Trait<'db>>,
}

fn resolve_event_emit_traits<'db>(
    db: &'db DriverDataBase,
    scope: ScopeId<'db>,
) -> EventEmitTraits<'db> {
    EventEmitTraits {
        log_trait: resolve_lib_trait_path(db, scope, "std::evm::effects::Log"),
        event_trait: resolve_lib_trait_path(db, scope, "std::evm::event::Event"),
    }
}

/// The event type a call emits, once its `Event` proof is checked, or `None`
/// when the call is not the canonical `Log::emit` or `Event::emit`.
///
/// `Log::emit<E: Event>` carries the proof as its declaration-owned bound for
/// `E`; a direct `Event::emit` call carries it as its dispatch proof.
fn checked_emitted_event<'db>(
    db: &'db DriverDataBase,
    contract_scope: ScopeId<'db>,
    typed_body: &hir::analysis::ty::ty_check::TypedBody<'db>,
    body: hir::hir_def::Body<'db>,
    expr_id: hir::hir_def::ExprId,
    callee: SemanticInstanceKey<'db>,
    emit_traits: &EventEmitTraits<'db>,
) -> Result<Option<TyId<'db>>, String> {
    let hir::hir_def::Partial::Present(hir::hir_def::Expr::MethodCall(
        receiver,
        method_name,
        _,
        args,
    )) = expr_id.data(db, body)
    else {
        return Ok(None);
    };
    let Some(method_name) = method_name.to_opt() else {
        return Ok(None);
    };
    if method_name.data(db) != "emit" {
        return Ok(None);
    }
    let Some(callable) = typed_body.callable_expr(expr_id) else {
        return Ok(None);
    };
    let hir::hir_def::CallableDef::Func(func) = callable.callable_def else {
        return Ok(None);
    };
    if func.name(db).to_opt() != Some(method_name) {
        return Ok(None);
    }
    let Some(trait_inst) = callable.trait_inst() else {
        return Ok(None);
    };
    let trait_def = trait_inst.def(db);
    let through_log = emit_traits
        .log_trait
        .is_some_and(|log_trait| trait_def == log_trait);
    let through_event = emit_traits
        .event_trait
        .is_some_and(|event_trait| trait_def == event_trait);
    let emitted = if through_log {
        let Some(arg) = args.first() else {
            return Ok(None);
        };
        arg.expr
    } else if through_event {
        *receiver
    } else {
        return Ok(None);
    };
    let mut ty = typed_body.expr_ty(db, emitted);
    while let Some((_, inner)) = ty.as_capability(db) {
        ty = inner;
    }
    let Some(struct_) = struct_from_ty(db, ty) else {
        return Ok(None);
    };
    let event_trait = emit_traits
        .event_trait
        .ok_or("missing canonical Event declaration")?;
    let goal = TraitInstId::new_simple(db, event_trait, vec![ty]);
    let proof = if through_log {
        let instance_scope = callee.impl_env(db).normalization_scope(db);
        callable
            .proved_bound(db, instance_scope, goal)
            .map(|proved| proved.proof)
    } else {
        callable
            .use_evidence()
            .dispatch
            .filter(|dispatch| dispatch.goal == goal)
            .map(|dispatch| dispatch.proof)
    }
    .ok_or_else(|| {
        format!(
            "missing declaration-owned checked Event proof for `{}`",
            ty.pretty_print(db)
        )
    })?;
    if !proof.is_closed(db) || proof.answering(db, goal).is_none() {
        return Err(format!(
            "event ABI requires a closed checked Event proof for `{}`",
            ty.pretty_print(db)
        ));
    }
    let name = struct_
        .name(db)
        .to_opt()
        .ok_or("event struct is missing a name")?;
    let inputs = event_param_descs(db, contract_scope, struct_, ty)?;
    let signature = canonical_function_signature(name.data(db), &inputs);
    let value = eval_proved_trait_const(
        db,
        body,
        typed_body,
        expr_id,
        goal,
        proof,
        "TOPIC0",
        TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::U256))),
        "event TOPIC0",
        &signature,
    )?;
    let topic0 = match value.value(db) {
        SemConstValue::Scalar {
            value: SemConstScalar::Int { value },
            ..
        } => Some(value.to_str_radix(16)),
        _ => None,
    }
    .ok_or_else(|| format!("event TOPIC0 is not u256 for `{signature}`"))?;
    if topic0 != keccak_hex_without_leading_zeros(&signature) {
        return Err(format!(
            "event TOPIC0 does not match canonical signature `{signature}`"
        ));
    }
    Ok(Some(ty))
}

fn keccak_hex_without_leading_zeros(signature: &str) -> String {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut output);
    let hex: String = output.iter().map(|byte| format!("{byte:02x}")).collect();
    let trimmed = hex.trim_start_matches('0');
    if trimmed.is_empty() { "0" } else { trimmed }.to_string()
}

fn variant_has_canonical_json_abi_shape(db: &DriverDataBase, struct_: Struct<'_>) -> bool {
    matches!(
        hir::span::struct_ast(db, struct_),
        hir::span::HirOrigin::Desugared(hir::span::DesugaredOrigin::Msg(_))
    )
}

fn resolve_lib_trait_path<'db>(
    db: &'db DriverDataBase,
    scope: ScopeId<'db>,
    path: &str,
) -> Option<Trait<'db>> {
    let mut segments = path.split("::");
    let root = segments.next()?;

    let ingot = scope.ingot(db);
    let mut path = if (ingot.kind(db) == IngotKind::Std && root == "std")
        || (ingot.kind(db) == IngotKind::Core && root == "core")
    {
        PathId::from_ident(db, IdentId::make_ingot(db))
    } else {
        PathId::from_str(db, root)
    };

    for segment in segments {
        path = path.push_str(db, segment);
    }

    let assumptions = PredicateListId::empty_list(db);
    match resolve_path(db, path, scope, assumptions, true).ok()? {
        PathRes::Trait(inst) => Some(inst.def(db)),
        _ => None,
    }
}

fn init_params_to_abi_params(
    db: &DriverDataBase,
    contract: hir::hir_def::Contract<'_>,
) -> Result<Vec<NamedAbiParamDesc>, String> {
    let Some(init) = contract.init(db) else {
        return Ok(Vec::new());
    };

    let params: Vec<_> = init
        .params(db)
        .data(db)
        .iter()
        .filter(|param| !param.is_self_param(db))
        .collect();
    let param_tys = contract.init_args_ty(db).field_types(db);

    if params.len() != param_tys.len() {
        return Err(format!(
            "constructor parameter count mismatch: {} names vs {} semantic types",
            params.len(),
            param_tys.len()
        ));
    }

    params
        .into_iter()
        .zip(param_tys)
        .map(|(param, ty)| {
            let name = param
                .name()
                .map(|ident| ident.data(db).to_string())
                .unwrap_or_default();
            Ok(NamedAbiParamDesc {
                name,
                indexed: None,
                desc: semantic_ty_to_abi_desc(db, ty)?,
            })
        })
        .collect()
}

fn struct_ty_to_abi_param_descs(
    db: &DriverDataBase,
    struct_: Struct<'_>,
    ty: TyId<'_>,
    indexed: impl Fn(&hir::hir_def::FieldDef<'_>) -> Option<bool>,
) -> Result<Vec<NamedAbiParamDesc>, String> {
    let field_tys = ty.field_types(db);
    named_field_param_descs(db, struct_.hir_fields(db), &field_tys, indexed)
}

fn named_field_param_descs(
    db: &DriverDataBase,
    fields: FieldDefListId<'_>,
    field_tys: &[TyId<'_>],
    indexed: impl Fn(&hir::hir_def::FieldDef<'_>) -> Option<bool>,
) -> Result<Vec<NamedAbiParamDesc>, String> {
    let hir_fields = fields.data(db);
    if hir_fields.len() != field_tys.len() {
        return Err(format!(
            "field count mismatch: {} HIR fields vs {} semantic fields",
            hir_fields.len(),
            field_tys.len()
        ));
    }

    hir_fields
        .iter()
        .zip(field_tys.iter().copied())
        .map(|(field, ty)| {
            let name = field
                .name
                .to_opt()
                .map(|ident| ident.data(db).to_string())
                .unwrap_or_default();
            Ok(NamedAbiParamDesc {
                name,
                indexed: indexed(field),
                desc: semantic_ty_to_abi_desc(db, ty)?,
            })
        })
        .collect()
}

fn semantic_ty_to_abi_desc(db: &DriverDataBase, ty: TyId<'_>) -> Result<AbiTypeDesc, String> {
    abi_ty::semantic_ty_to_abi_desc(db, ty).map_err(|err| err.to_string())
}

fn struct_from_ty<'db>(db: &'db DriverDataBase, ty: TyId<'db>) -> Option<Struct<'db>> {
    if let Some((_, inner)) = ty.as_capability(db) {
        return struct_from_ty(db, inner);
    }
    match ty.base_ty(db).data(db) {
        TyData::TyBase(TyBase::Adt(adt)) => match adt.adt_ref(db) {
            AdtRef::Struct(struct_) => Some(struct_),
            AdtRef::Enum(_) => None,
        },
        TyData::QualifiedTy(trait_inst) => struct_from_ty(db, trait_inst.self_ty(db)),
        _ => None,
    }
}

fn parse_function_signature(signature: &str) -> Result<ParsedFunctionSignature, String> {
    abi_ty::parse_function_signature(signature)
        .map_err(|err| format!("cannot emit JSON ABI for `{signature}`: {err}"))
}

fn ensure_selector_matches_signature(
    source_signature: &str,
    parsed_signature: &ParsedFunctionSignature,
    inputs: &[NamedAbiParamDesc],
    actual_selector: u32,
) -> Result<(), String> {
    if parsed_signature.arg_types.len() != inputs.len() {
        return Err(format!(
            "cannot emit JSON ABI for `{source_signature}`: selector arity {} does not match semantic arity {}",
            parsed_signature.arg_types.len(),
            inputs.len()
        ));
    }

    for (selector_ty, input) in parsed_signature.arg_types.iter().zip(inputs) {
        if selector_ty != &input.desc.canonical_type {
            let mut message = format!(
                "cannot emit JSON ABI for `{source_signature}`: selector argument type `{selector_ty}` does not match semantic ABI type `{}`",
                input.desc.canonical_type
            );
            if let Some(suggestion) = abi_ty::suggested_fe_type_for_sol_type(selector_ty) {
                message.push_str(&format!(
                    "; `{suggestion}` decodes and encodes Solidity `{selector_ty}`"
                ));
            }
            return Err(message);
        }
    }

    let canonical_signature = canonical_function_signature(&parsed_signature.name, inputs);
    let expected_selector = selector_for_signature(&canonical_signature);
    if actual_selector != expected_selector {
        return Err(format!(
            "cannot emit JSON ABI for `{source_signature}`: non-canonical selector 0x{actual_selector:08x} does not match canonical signature `{canonical_signature}` (0x{expected_selector:08x})"
        ));
    }
    Ok(())
}

fn canonical_function_signature(fn_name: &str, inputs: &[NamedAbiParamDesc]) -> String {
    let mut signature = String::new();
    signature.push_str(fn_name);
    signature.push('(');
    for (idx, input) in inputs.iter().enumerate() {
        if idx > 0 {
            signature.push(',');
        }
        signature.push_str(&input.desc.canonical_type);
    }
    signature.push(')');
    signature
}

fn selector_for_signature(signature: &str) -> u32 {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut output);
    u32::from_be_bytes([output[0], output[1], output[2], output[3]])
}

fn resolve_sol_abi_ty<'db>(
    db: &'db DriverDataBase,
    scope: ScopeId<'db>,
) -> Result<TyId<'db>, String> {
    hir::analysis::ty::corelib::resolve_lib_type_path(db, scope, "std::abi::Sol")
        .ok_or_else(|| "cannot resolve canonical `std::abi::Sol` type".to_string())
}

/// Derive ABI state mutability from the effective recv-arm effect requirement set.
fn derive_state_mutability(
    db: &DriverDataBase,
    arm_view: hir::semantic::RecvArmView<'_>,
) -> String {
    let arm = arm_view
        .arm(db)
        .expect("recv arm should exist during ABI generation");
    if arm.is_payable(db) {
        return "payable".to_string();
    }

    let effects = arm_view.effective_effect_requirements(db);
    if effects.is_empty() {
        "pure".to_string()
    } else if effects.iter().any(|effect| effect.is_mut) {
        "nonpayable".to_string()
    } else {
        "view".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::InputDb;
    use driver::DriverDataBase;
    use serde_json::Value;

    fn generate_test_abi_result(code: &str, contract_name: &str) -> Result<AbiResult, String> {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file_path = temp.path().join("test.fe");
        let url = url::Url::from_file_path(&file_path).expect("file path to url");
        let mut db = DriverDataBase::default();
        let file = db.workspace().touch(&mut db, url, Some(code.to_string()));
        let prepared = driver::generation::prepare_file(&db, file)
            .map_err(|error| format!("generation failed: {error:?}"))?;
        if let Some(output) = prepared {
            let output_file = output
                .workspace()
                .get(&output, &file.url(&db).unwrap())
                .unwrap();
            generate_contract_abi(&output, output.top_mod(output_file), contract_name)?
                .ok_or_else(|| format!("contract `{contract_name}` not found"))
        } else {
            generate_contract_abi(&db, db.top_mod(file), contract_name)?
                .ok_or_else(|| format!("contract `{contract_name}` not found"))
        }
    }

    fn generate_test_abi(code: &str, contract_name: &str) -> Result<String, String> {
        generate_test_abi_result(code, contract_name).map(|r| r.json)
    }

    fn abi_entries(code: &str, contract_name: &str) -> Vec<Value> {
        let abi = generate_test_abi(code, contract_name).expect("generate abi");
        serde_json::from_str(&abi).expect("parse abi json")
    }

    #[test]
    fn hex_selector_skips_arm_with_warning() {
        let code = r#"
msg FooMsg {
    #[selector = 0x12345678]
    Ping -> u256,
}

pub contract Foo {
    recv FooMsg {
        Ping -> u256 {
            return 1
        }
    }
}
"#;

        let result = generate_test_abi_result(code, "Foo").expect("ABI generation should succeed");
        let entries: Vec<Value> = serde_json::from_str(&result.json).expect("parse abi json");
        assert!(
            entries.iter().all(|e| e["type"] != "function"),
            "hex-selector arm should be skipped"
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0].contains("selector signature is unknown"),
            "unexpected warning: {}",
            result.warnings[0]
        );
    }

    #[test]
    fn sol_selector_alias_uses_source_function_name() {
        let code = r#"
use std::abi::sol

msg FooMsg {
    #[selector = sol("foo(uint256)")]
    Bar { value: u256 } -> u256,
}

pub contract Foo {
    recv FooMsg {
        Bar { value } -> u256 {
            value
        }
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "foo");
        assert_eq!(function["inputs"][0]["type"], "uint256");
    }

    #[test]
    fn sol_fixed_bytes_wrappers_emit_canonical_types() {
        let code = r#"
use std::abi::{FixedBytes, sol}

msg FooMsg {
    #[selector = sol("check(bytes4,bytes32)")]
    Check { interface_id: FixedBytes<4>, root: FixedBytes<32> } -> bool,
}

pub contract Foo {
    recv FooMsg {
        Check { interface_id, root } -> bool {
            interface_id.val == 0x01ffc9a7 && root.val != 0
        }
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "check");
        assert_eq!(function["inputs"][0]["type"], "bytes4");
        assert_eq!(function["inputs"][1]["type"], "bytes32");
        assert_eq!(function["outputs"][0]["type"], "bool");
    }

    #[test]
    fn sol_fixed_bytes_aliases_emit_canonical_types() {
        let code = r#"
use std::abi::{Bytes1, Bytes4, Bytes32, sol}

msg FooMsg {
    #[selector = sol("check(bytes1,bytes4,bytes32)")]
    Check { prefix: Bytes1, interface_id: Bytes4, root: Bytes32 } -> bool,
}

pub contract Foo {
    recv FooMsg {
        Check { prefix, interface_id, root } -> bool {
            prefix.val == 0xab && interface_id.val == 0x01ffc9a7 && root.val != 0
        }
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "check");
        assert_eq!(function["inputs"][0]["type"], "bytes1");
        assert_eq!(function["inputs"][1]["type"], "bytes4");
        assert_eq!(function["inputs"][2]["type"], "bytes32");
        assert_eq!(function["outputs"][0]["type"], "bool");
    }

    #[test]
    fn selector_const_alias_preserves_signature() {
        let code = r#"
use std::abi::sol

const PING: u32 = sol("ping()")

msg FooMsg {
    #[selector = PING]
    Ping,
}

pub contract Foo {
    recv FooMsg {
        Ping {}
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "ping");
    }

    #[test]
    fn selector_const_block_preserves_signature() {
        let code = r#"
use std::abi::sol

const PING: u32 = { sol("ping()") }

msg FooMsg {
    #[selector = PING]
    Ping,
}

pub contract Foo {
    recv FooMsg {
        Ping {}
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "ping");
    }

    #[test]
    fn selector_const_block_local_alias_preserves_signature() {
        let code = r#"
use std::abi::sol

const PING: u32 = {
    let x = sol("ping()");
    x
}

msg FooMsg {
    #[selector = PING]
    Ping,
}

pub contract Foo {
    recv FooMsg {
        Ping {}
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "ping");
    }

    #[test]
    fn selector_const_block_local_alias_chain_preserves_signature() {
        let code = r#"
use std::abi::sol

const PING: u32 = {
    let x = sol("ping()");
    let y = x;
    y
}

msg FooMsg {
    #[selector = PING]
    Ping,
}

pub contract Foo {
    recv FooMsg {
        Ping {}
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "ping");
    }

    #[test]
    fn selector_const_fn_preserves_signature() {
        let code = r#"
use std::abi::sol

const fn ping_selector() -> u32 { sol("ping()") }

msg FooMsg {
    #[selector = ping_selector()]
    Ping,
}

pub contract Foo {
    recv FooMsg {
        Ping {}
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "ping");
    }

    #[test]
    fn sol_selector_preserves_source_casing() {
        let code = r#"
use std::abi::sol

msg FooMsg {
    #[selector = sol("urlValue()")]
    URLValue,
}

pub contract Foo {
    recv FooMsg {
        URLValue {
        }
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "urlValue");
        assert_eq!(
            function["inputs"].as_array().expect("inputs array").len(),
            0
        );
    }

    #[test]
    fn manual_generic_recv_variants_are_skipped_with_warning() {
        let code = r#"
use std::abi::sol

struct GenericMsg<T> {
    pub value: T,
}

impl<T> core::abi::AbiSize for GenericMsg<T>
    where T: core::abi::AbiSize
{
    const HEAD_SIZE: u256 = T::HEAD_SIZE
    const IS_DYNAMIC: bool = T::IS_DYNAMIC

    fn payload_size(self) -> u256 {
        Self::HEAD_SIZE + core::abi::dynamic_payload_size(self.value)
    }
}

impl<T> core::abi::Encode<std::abi::Sol> for GenericMsg<T>
    where T: core::abi::Encode<std::abi::Sol>
{
    fn encode(own self, _ out: core::abi::AbiFrame) {
        let Self { value } = self
        value.encode(out)
    }
}

impl<T> core::abi::Decode<std::abi::Sol> for GenericMsg<T>
    where T: core::abi::Decode<std::abi::Sol>
{
    fn decode_payload<D: core::abi::AbiDecoder<std::abi::Sol>, I: core::abi::ByteInput>(_ d: mut D, _ input: I) -> Self {
        let value = T::decode_payload(mut d, input)
        Self { value }
    }
}

impl core::message::MsgVariant<std::abi::Sol> for GenericMsg<u8> {
    const SELECTOR: u32 = sol("genericMsg(uint8)")
    type Return = u8
}

impl core::message::MsgVariant<std::abi::Sol> for GenericMsg<u16> {
    const SELECTOR: u32 = sol("genericMsg(uint16)")
    type Return = u16
}

pub contract GenericRecvContract {
    recv {
        GenericMsg<u8> { value } -> u8 uses () {
            value
        }
        GenericMsg<u16> { value } -> u16 uses () {
            value
        }
    }
}
"#;

        let result = generate_test_abi_result(code, "GenericRecvContract")
            .expect("ABI generation should succeed");
        let entries: Vec<Value> = serde_json::from_str(&result.json).expect("parse abi json");
        assert!(
            entries.iter().all(|entry| entry["type"] != "function"),
            "manual generic MsgVariant impls should be skipped"
        );
        assert_eq!(result.warnings.len(), 2);
        assert!(
            result
                .warnings
                .iter()
                .all(|warning| warning.contains("manual `MsgVariant` impls")),
            "unexpected warnings: {:?}",
            result.warnings
        );
    }

    #[test]
    fn manual_msg_variant_impls_are_skipped_with_warning() {
        let code = r#"
use std::abi::sol

struct Weird {
    pub amount: u64,
    pub flag: bool,
}

impl core::abi::Encode<std::abi::Sol> for Weird {
    fn encode(own self, _ out: core::abi::AbiFrame) {
        self.flag.encode(out.slice(offset: 0, len: 32))
        self.amount.encode(out.slice(offset: 32, len: 32))
    }
}

impl core::abi::Decode<std::abi::Sol> for Weird {
    fn decode_payload<D: core::abi::AbiDecoder<std::abi::Sol>, I: core::abi::ByteInput>(_ d: mut D, _ input: I) -> Self {
        let flag = bool::decode_payload(mut d, input)
        let amount = u64::decode_payload(mut d, input)
        Self { amount, flag }
    }
}

impl core::message::MsgVariant<std::abi::Sol> for Weird {
    const SELECTOR: u32 = sol("foo(bool,uint64)")
    type Return = ()
}

pub contract Foo {
    recv {
        Weird { amount, flag } uses () {
            let _ = amount
            let _ = flag
        }
    }
}
"#;

        let result = generate_test_abi_result(code, "Foo").expect("ABI generation should succeed");
        let entries: Vec<Value> = serde_json::from_str(&result.json).expect("parse abi json");
        assert!(
            entries.iter().all(|e| e["type"] != "function"),
            "manual MsgVariant should be skipped"
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0].contains("manual `MsgVariant` impls"),
            "unexpected warning: {}",
            result.warnings[0]
        );
    }

    #[test]
    fn manual_module_msg_variants_are_skipped_with_warning() {
        let code = r#"
mod TokenMsg {
    use std::abi::sol

    pub struct Transfer {
        pub to: u64,
        pub amount: u64,
    }

    impl core::abi::AbiSize for Transfer {
        const HEAD_SIZE: u256 = 64
        const IS_DYNAMIC: bool = false
    }

    impl core::abi::Encode<std::abi::Sol> for Transfer {
        fn encode(own self, _ out: core::abi::AbiFrame) {
            self.to.encode(out.slice(offset: 0, len: 32))
            self.amount.encode(out.slice(offset: 32, len: 32))
        }
    }

    impl core::abi::Decode<std::abi::Sol> for Transfer {
        fn decode_payload<D: core::abi::AbiDecoder<std::abi::Sol>, I: core::abi::ByteInput>(_ d: mut D, _ input: I) -> Self {
            let to = u64::decode_payload(mut d, input)
            let amount = u64::decode_payload(mut d, input)
            Self { to, amount }
        }
    }

    impl core::message::MsgVariant<std::abi::Sol> for Transfer {
        const SELECTOR: u32 = sol("transfer(uint64,uint64)")
        type Return = bool
    }
}

pub contract Foo {
    recv TokenMsg {
        Transfer { to, amount } -> bool uses () {
            let _ = to
            let _ = amount
            true
        }
    }
}
"#;

        let result = generate_test_abi_result(code, "Foo").expect("ABI generation should succeed");
        let entries: Vec<Value> = serde_json::from_str(&result.json).expect("parse abi json");
        assert!(
            entries.iter().all(|e| e["type"] != "function"),
            "manual MsgVariant module should be skipped"
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0].contains("manual `MsgVariant` impls"),
            "unexpected warning: {}",
            result.warnings[0]
        );
    }

    #[test]
    fn constructed_events_without_emit_are_not_included() {
        let code = r#"
use std::abi::sol

#[event]
struct Transfer {
    value: u256,
}

msg FooMsg {
    #[selector = sol("ping()")]
    Ping,
}

pub contract Foo {
    recv FooMsg {
        Ping uses () {
            helper()
        }
    }
}

fn helper() {
    let _ = Transfer { value: 1 }
}
"#;

        let entries = abi_entries(code, "Foo");
        assert!(
            entries
                .iter()
                .all(|entry| !(entry["type"] == "event" && entry["name"] == "Transfer")),
            "constructed-but-not-emitted event should be absent: {entries:?}"
        );
    }

    #[test]
    fn recv_variant_resolution_survives_same_name_event_structs() {
        let code = r#"
use std::abi::sol

msg Erc20 {
    #[selector = sol("transfer(uint256,uint256)")]
    Transfer { to: u256, amount: u256 } -> bool,
}

#[event]
struct Transfer {
    #[indexed]
    from: u256,
    #[indexed]
    to: u256,
    value: u256,
}

pub contract C {
    recv Erc20 {
        Transfer { to, amount } -> bool {
            let _ = to
            let _ = amount
            true
        }
    }
}
"#;

        let entries = abi_entries(code, "C");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");
        assert_eq!(function["name"], "transfer");
        assert_eq!(function["inputs"][0]["name"], "to");
        assert_eq!(function["inputs"][1]["name"], "amount");
    }

    #[test]
    fn event_abi_preserves_indexed_fields() {
        let code = r#"
use std::abi::sol
use std::evm::Log

msg FooMsg {
    #[selector = sol("ping()")]
    Ping,
}

#[event]
struct Transfer {
    #[indexed]
    from: u256,
    #[indexed]
    to: u256,
    value: u256,
}

pub contract C uses (log: mut Log) {
    recv FooMsg {
        Ping uses (mut log) {
            log.emit(Transfer { from: 1, to: 2, value: 3 })
        }
    }
}
"#;

        let entries = abi_entries(code, "C");
        let event = entries
            .iter()
            .find(|entry| entry["type"] == "event" && entry["name"] == "Transfer")
            .expect("event entry");

        assert_eq!(event["anonymous"], false);
        assert_eq!(event["inputs"][0]["name"], "from");
        assert_eq!(event["inputs"][0]["indexed"], true);
        assert_eq!(event["inputs"][1]["name"], "to");
        assert_eq!(event["inputs"][1]["indexed"], true);
        assert_eq!(event["inputs"][2]["name"], "value");
        assert_eq!(event["inputs"][2]["indexed"], false);
    }

    #[test]
    fn tuple_and_fixed_array_types_emit_components_and_lengths() {
        let code = r#"
use std::abi::sol

msg TupleMsg {
    #[selector = sol("setPair((uint64,bool),uint256[2])")]
    SetPair { pair: (u64, bool), values: [u256; 2] } -> (u64, bool),
}

pub contract TupleContract {
    recv TupleMsg {
        SetPair { pair, values } -> (u64, bool) uses () {
            let _ = values
            pair
        }
    }
}
"#;

        let entries = abi_entries(code, "TupleContract");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["inputs"][0]["type"], "tuple");
        assert_eq!(function["inputs"][0]["components"][0]["type"], "uint64");
        assert_eq!(function["inputs"][0]["components"][1]["type"], "bool");
        assert_eq!(function["inputs"][1]["type"], "uint256[2]");
        assert_eq!(function["outputs"].as_array().unwrap().len(), 2);
        assert_eq!(function["outputs"][0]["type"], "uint64");
        assert_eq!(function["outputs"][1]["type"], "bool");
    }

    #[test]
    fn tuple_return_emits_one_output_per_element() {
        let code = r#"
use std::abi::{Bytes32, DynString}

msg InfoMsg {
    #[selector = sol("information()")]
    Information {} -> (DynString, Bytes32, Address),
    #[selector = sol("nested()")]
    Nested {} -> (u256, (DynString, bool)),
    #[selector = sol("name()")]
    Name {} -> DynString,
}

pub contract Info {
    recv InfoMsg {
        Information {} -> (DynString, Bytes32, Address) {
            (DynString::empty(), Bytes32 { val: 0 }, Address::new(0))
        }
        Nested {} -> (u256, (DynString, bool)) {
            (0, (DynString::empty(), false))
        }
        Name {} -> DynString {
            DynString::empty()
        }
    }
}
"#;

        let entries = abi_entries(code, "Info");
        let function = |name: &str| {
            entries
                .iter()
                .find(|entry| entry["type"] == "function" && entry["name"] == name)
                .unwrap_or_else(|| panic!("missing function `{name}`"))
                .clone()
        };

        let information = function("information");
        assert_eq!(
            information["outputs"],
            serde_json::json!([
                { "name": "", "type": "string" },
                { "name": "", "type": "bytes32" },
                { "name": "", "type": "address" },
            ])
        );

        let nested = function("nested");
        assert_eq!(
            nested["outputs"],
            serde_json::json!([
                { "name": "", "type": "uint256" },
                {
                    "name": "",
                    "type": "tuple",
                    "components": [
                        { "name": "", "type": "string" },
                        { "name": "", "type": "bool" },
                    ],
                },
            ])
        );

        let name = function("name");
        assert_eq!(
            name["outputs"],
            serde_json::json!([{ "name": "", "type": "string" }])
        );
    }

    #[test]
    fn recv_mutability_uses_effective_contract_scoped_bindings() {
        let code = r#"
use std::abi::sol
use std::evm::{Address, Call, Ctx}

msg FooMsg {
    #[selector = sol("who()")]
    Who -> u256,
    #[selector = sol("ping(address)")]
    Ping { b: Address },
}

msg BarMsg {
    #[selector = sol("pong()")]
    Pong,
}

pub contract Foo uses (ctx: Ctx, call: mut Call) {
    recv FooMsg {
        Who -> u256 uses (ctx) {
            ctx.caller().word()
        }

        Ping { b } uses (call) {
            call.call(addr: b, gas: 100000, value: Wei::zero(), message: BarMsg::Pong {})
        }
    }
}

pub contract Bar {
    recv BarMsg {
        Pong {}
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let who = entries
            .iter()
            .find(|entry| entry["type"] == "function" && entry["name"] == "who")
            .expect("who entry");
        let ping = entries
            .iter()
            .find(|entry| entry["type"] == "function" && entry["name"] == "ping")
            .expect("ping entry");

        assert_eq!(who["stateMutability"], "view");
        assert_eq!(ping["stateMutability"], "nonpayable");
    }

    #[test]
    fn raw_staticcall_supports_view_abi_with_readonly_call_effect() {
        let code = r#"
use core::ptr::MemBuffer
use std::abi::sol
use std::evm::{Address, Call}
msg ProbeMsg {
    #[selector = sol("probe(address)")]
    Probe { target: Address } -> bool,
}
pub contract Probe {
    recv ProbeMsg {
        Probe { target } -> bool uses (call: Call) {
            let args = MemBuffer::alloc(0)
            var ret = MemBuffer::with_capacity(64)
            call.raw_staticcall(addr: target, gas: 30000, args: args.span(), ret: mut ret).success()
        }
    }
}
"#;
        let entries = abi_entries(code, "Probe");
        let probe = entries
            .iter()
            .find(|e| e["name"] == "probe")
            .expect("probe entry");
        assert_eq!(probe["stateMutability"], "view");
    }

    #[test]
    fn payable_constructor_and_recv_arms_preserve_abi_mutability_and_array_inputs() {
        let code = r#"
use std::abi::sol

msg WalletMsg {
    #[selector = sol("fund()")]
    Fund,

    #[selector = sol("peek()")]
    Peek -> u256,
}

pub contract Wallet {
    #[payable]
    init(seed: u256, values: [u256; 2]) {}

    recv WalletMsg {
        #[payable]
        Fund {} {}

        Peek -> u256 {
            7
        }
    }
}
"#;

        let entries = abi_entries(code, "Wallet");
        let constructor = entries
            .iter()
            .find(|entry| entry["type"] == "constructor")
            .expect("constructor entry");
        let fund = entries
            .iter()
            .find(|entry| entry["type"] == "function" && entry["name"] == "fund")
            .expect("fund entry");
        let peek = entries
            .iter()
            .find(|entry| entry["type"] == "function" && entry["name"] == "peek")
            .expect("peek entry");

        assert_eq!(constructor["stateMutability"], "payable");
        assert_eq!(constructor["inputs"][0]["name"], "seed");
        assert_eq!(constructor["inputs"][0]["type"], "uint256");
        assert_eq!(constructor["inputs"][1]["name"], "values");
        assert_eq!(constructor["inputs"][1]["type"], "uint256[2]");

        assert_eq!(fund["stateMutability"], "payable");
        assert_eq!(peek["stateMutability"], "pure");
        assert_eq!(peek["outputs"][0]["type"], "uint256");
    }

    #[test]
    fn fallback_recv_arm_emits_standard_fallback_abi_entry() {
        let code = r#"
use std::abi::sol

msg WalletMsg {
    #[selector = sol("peek()")]
    Peek -> u256,
}

pub contract Wallet {
    recv {
        WalletMsg::Peek {} -> u256 {
            7
        }

        #[payable]
        _ {}
    }
}
"#;

        let entries = abi_entries(code, "Wallet");
        let fallback = entries
            .iter()
            .find(|entry| entry["type"] == "fallback")
            .expect("fallback entry");

        assert_eq!(fallback["stateMutability"], "payable");
        assert!(fallback.get("name").is_none());
        assert!(fallback.get("inputs").is_none());
        assert!(fallback.get("outputs").is_none());
    }

    #[test]
    fn generic_event_helpers_preserve_concrete_event_types() {
        let code = r#"
use std::abi::sol
use std::evm::effects::Log
use std::evm::event::Event

#[event]
struct Transfer {
    value: u256,
}

#[event]
struct Approval {
    value: u256,
}

msg FooMsg {
    #[selector = sol("ping()")]
    Ping,
}

fn emit_event<E: Event>(event: E) uses (log: mut Log) {
    log.emit(event)
}

pub contract Foo uses (log: mut Log) {
    recv FooMsg {
        Ping uses (mut log) {
            emit_event(Transfer { value: 1 })
            emit_event(Approval { value: 2 })
        }
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        assert!(
            entries
                .iter()
                .any(|entry| entry["type"] == "event" && entry["name"] == "Transfer"),
            "generic helper should preserve Transfer event: {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry["type"] == "event" && entry["name"] == "Approval"),
            "generic helper should preserve Approval event: {entries:?}"
        );
    }

    #[test]
    fn sol_compat_wrapper_types_emit_correct_abi_type() {
        let code = r#"
use std::abi::sol
use std::abi::sol::Uint160
use std::abi::sol::Int24

msg FooMsg {
    #[selector = sol("set(uint160,int24)")]
    Set { addr: Uint160, value: Int24 },
}

pub contract Foo {
    recv FooMsg {
        Set { addr, value } uses () {
            let _ = addr
            let _ = value
        }
    }
}
"#;

        let entries = abi_entries(code, "Foo");
        let function = entries
            .iter()
            .find(|entry| entry["type"] == "function")
            .expect("function entry");

        assert_eq!(function["name"], "set");
        assert_eq!(function["inputs"][0]["type"], "uint160");
        assert_eq!(function["inputs"][0]["name"], "addr");
        assert_eq!(function["inputs"][1]["type"], "int24");
        assert_eq!(function["inputs"][1]["name"], "value");
    }
    const APPLIED_ERROR_SOURCE: &str = r#"
use core::abi::{AbiFrame, AbiSize, Encode, abi_field_head_size, dynamic_payload_size, encode_field}
use core::error::ErrorVariant
use std::abi::{Sol, sol}
use std::evm::revert_error

struct Failure<T> { value: T }
impl<T: AbiSize> AbiSize for Failure<T> {
    const HEAD_SIZE: u256 = abi_field_head_size<T>()
    const IS_DYNAMIC: bool = <T as AbiSize>::IS_DYNAMIC
    fn payload_size(self) -> u256 { Self::HEAD_SIZE + dynamic_payload_size(self.value) }
}
impl<T: AbiSize + Encode<Sol>> Encode<Sol> for Failure<T> {
    fn encode(own self, _ out: AbiFrame) {
        let tails = out.tail(<Self as AbiSize>::HEAD_SIZE)
        let _ = encode_field<Sol, T>(self.value, out, head_pos: 0, tails: tails)
    }
}
impl ErrorVariant<Sol> for Failure<u256> {
    const SELECTOR: u32 = sol("Failure(uint256)")
}
impl ErrorVariant<Sol> for Failure<bool> {
    const SELECTOR: u32 = sol("Failure(bool)")
}
struct NonError { value: u256 }
mod lookalike {
    pub fn revert_error<T>(_ value: own T) { let _ = value }
}
fn fail<T: ErrorVariant<Sol> + Encode<Sol>>(_ error: own T) { revert_error(error) }
msg M {
    #[selector = sol("fail(bool)")]
    Fail { boolean: bool }
}
pub contract C {
    recv M {
        Fail { boolean } {
            lookalike::revert_error(NonError { value: 7 })
            if boolean { fail(Failure<bool> { value: true }) }
            else { fail(Failure<u256> { value: 7 }) }
        }
    }
}
"#;

    #[test]
    fn handwritten_errors_retain_applied_fields_through_generic_calls() {
        let entries = abi_entries(APPLIED_ERROR_SOURCE, "C");
        let errors = entries
            .iter()
            .filter(|entry| entry["type"] == "error")
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 2, "{entries:?}");
        assert!(errors.iter().all(|entry| entry["name"] == "Failure"));
        assert!(
            errors
                .iter()
                .any(|entry| entry["inputs"]
                    == serde_json::json!([{"name":"value","type":"uint256"}]))
        );
        assert!(errors.iter().any(|entry| entry["inputs"] == serde_json::json!([{"name":"value","type":"bool"}])));
        assert!(!entries.iter().any(|entry| entry["name"] == "NonError"));
    }

    #[test]
    fn handwritten_error_noncanonical_selector_is_refused() {
        let source = APPLIED_ERROR_SOURCE.replace("sol(\"Failure(bool)\")", "0");
        let error = generate_test_abi_result(&source, "C")
            .err()
            .expect("noncanonical error selector");
        assert!(
            error.contains("error SELECTOR does not match canonical signature `Failure(bool)`"),
            "{error}"
        );
    }

    #[test]
    fn generated_error_uses_checked_selector_and_reachability() {
        let source = r#"
use std::abi::sol
use std::derive::error_struct
use std::evm::revert_error
#[generate(error_struct)]
struct GeneratedFailure { first: u256, second: u256 }
#[generate(error_struct)]
struct UnusedFailure { first: u256 }
msg M {
    #[selector = sol("fail()")]
    Fail {}
}
pub contract C {
    recv M { Fail {} { revert_error(GeneratedFailure { first: 1, second: 2 }) } }
}
"#;
        let entries = abi_entries(source, "C");
        let errors = entries
            .iter()
            .filter(|entry| entry["type"] == "error")
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{entries:?}");
        assert_eq!(errors[0]["name"], "GeneratedFailure");
        assert_eq!(
            errors[0]["inputs"],
            serde_json::json!([
                {"name":"first","type":"uint256"}, {"name":"second","type":"uint256"}
            ])
        );
    }

    #[test]
    fn error_discovery_respects_unused_invoked_and_nested_closure_regions() {
        for (body, expected) in [
            (
                "let f = || revert_error(ClosureFailure {})\nlet _ = f",
                false,
            ),
            ("let f = || revert_error(ClosureFailure {})\nf()", true),
            (
                "let outer = || { let inner = || revert_error(ClosureFailure {})\nlet _ = inner }\nouter()",
                false,
            ),
            (
                "let outer = || { let inner = || revert_error(ClosureFailure {})\ninner() }\nouter()",
                true,
            ),
        ] {
            let code = format!(
                r#"
use std::abi::sol
use std::evm::revert_error
#[error]
struct ClosureFailure {{}}
msg M {{
    #[selector = sol("fail()")]
    Fail {{}}
}}
pub contract C {{ recv M {{ Fail {{}} {{ {body} }} }} }}
"#
            );
            let entries = abi_entries(&code, "C");
            let errors = entries
                .iter()
                .filter(|entry| entry["type"] == "error")
                .collect::<Vec<_>>();
            assert_eq!(
                errors.len(),
                usize::from(expected),
                "body={body} entries={entries:?}"
            );
            if expected {
                assert_eq!(errors[0]["name"], "ClosureFailure");
            }
        }
    }

    #[test]
    fn error_discovery_includes_collection_protocol_calls_and_direct_controls() {
        for (start, next, direct, name) in [
            (
                "revert_error(StartFailure {})",
                "c + 1",
                "xs.start()",
                "StartFailure",
            ),
            (
                "0",
                "revert_error(NextFailure {})",
                "xs.next(0)",
                "NextFailure",
            ),
        ] {
            for body in [
                "for x in xs { let _ = x }".to_string(),
                format!("let _ = {direct}"),
            ] {
                let code = format!(
                    r#"
use core::iter::Collection
use std::abi::sol
use std::evm::revert_error
#[error]
struct StartFailure {{}}
#[error]
struct NextFailure {{}}
struct ErrorCollection {{}}
impl Collection for ErrorCollection {{
    type Cursor = usize
    type Item = u256
    fn start(self) -> usize {{ {start} }}
    fn has(self, _ c: usize) -> bool {{ c < 1 }}
    fn next(self, _ c: usize) -> usize {{ {next} }}
    fn at(self, _ c: usize) -> ref u256 {{
        let value: u256 = 7
        yield ref value
    }}
}}
msg M {{
    #[selector = sol("fail()")]
    Fail {{}}
}}
pub contract C {{ recv M {{ Fail {{}} {{
    let xs = ErrorCollection {{}}
    {body}
}} }} }}
"#
                );
                let entries = abi_entries(&code, "C");
                let errors = entries
                    .iter()
                    .filter(|entry| entry["type"] == "error")
                    .collect::<Vec<_>>();
                assert_eq!(errors.len(), 1, "body={body} entries={entries:?}");
                assert_eq!(errors[0]["name"], name);
            }
        }
    }

    #[test]
    fn error_protocol_identity_ignores_nested_core_and_std_lookalikes() {
        for shadows in [
            "",
            "mod core { pub mod error { pub trait ErrorVariant<A> {} } }\nmod std { pub mod abi { pub struct Sol {} } }",
        ] {
            let source = format!(
                r#"
use std::abi::sol
use std::evm::revert_error
#[error]
struct Failure {{ code: u256 }}
msg M {{
    #[selector = sol("fail()")]
    Fail {{}}
}}
mod client {{
    use super::{{revert_error, M, Failure}}
    {shadows}
    pub contract C {{ recv M {{ Fail {{}} {{ revert_error(Failure {{ code: 7 }}) }} }} }}
}}
"#
            );
            let entries = abi_entries(&source, "C");
            let errors = entries
                .iter()
                .filter(|entry| entry["type"] == "error")
                .collect::<Vec<_>>();
            assert_eq!(errors.len(), 1, "shadows={shadows} entries={entries:?}");
            assert_eq!(errors[0]["name"], "Failure");
            assert_eq!(
                errors[0]["inputs"],
                serde_json::json!([{"name":"code","type":"uint256"}])
            );
        }
    }

    #[test]
    fn abi_error_runtime_and_json_match_literal_two_word_oracle() {
        use contract_harness::{ExecutionOptions, FeContractHarness, HarnessError, U256};

        let declarations = [
            r#"#[generate(std::derive::error_struct)]
struct InsufficientBalance { available: u256, required: u256 }"#,
            r#"#[error]
struct InsufficientBalance { available: u256, required: u256 }"#,
            r#"struct InsufficientBalance { available: u256, required: u256 }
impl core::abi::AbiSize for InsufficientBalance {
    const HEAD_SIZE: u256 = 64
    const IS_DYNAMIC: bool = false
}
impl core::abi::Encode<std::abi::Sol> for InsufficientBalance {
    fn encode(own self, _ out: core::abi::AbiFrame) {
        out.store_word(at: 0, value: self.available)
        out.store_word(at: 32, value: self.required)
    }
}
impl core::error::ErrorVariant<std::abi::Sol> for InsufficientBalance {
    const SELECTOR: u32 = 0xcf479181
}"#,
        ];
        // Literal selector and two big-endian words are independent of both
        // the generator and the compiler's legacy error construction.
        let mut expected = vec![0xcf, 0x47, 0x91, 0x81];
        let mut payload = [0u8; 64];
        payload[31] = 10;
        payload[63] = 25;
        expected.extend_from_slice(&payload);
        assert_eq!(expected.len(), 68);
        assert_eq!(
            selector_for_signature("InsufficientBalance(uint256,uint256)"),
            0xcf479181
        );
        for declaration in declarations {
            let source = format!(
                r#"
use std::abi::sol
use std::evm::revert_error
{declaration}
msg M {{
    #[selector = sol("fail()")]
    Fail {{}},
    #[selector = sol("headSize()")]
    HeadSize {{}} -> u256,
}}
pub contract C {{
    recv M {{
        Fail {{}} {{ revert_error(InsufficientBalance {{ available: 10, required: 25 }}) }}
        HeadSize {{}} -> u256 {{ <InsufficientBalance as core::abi::AbiSize>::HEAD_SIZE }}
    }}
}}
"#
            );
            let entries = abi_entries(&source, "C");
            let errors = entries
                .iter()
                .filter(|entry| entry["type"] == "error")
                .collect::<Vec<_>>();
            assert_eq!(errors.len(), 1, "{entries:?}");
            assert_eq!(errors[0]["name"], "InsufficientBalance");
            assert_eq!(
                errors[0]["inputs"],
                serde_json::json!([
                    {"name":"available","type":"uint256"},
                    {"name":"required","type":"uint256"}
                ])
            );
            let harness = FeContractHarness::compile("C", &source).expect("compile oracle twin");
            let mut instance = harness.deploy_instance().expect("deploy oracle twin");
            let options = ExecutionOptions::default();
            let head = instance
                .call_function("headSize()", &[], options)
                .expect("head size");
            assert_eq!(U256::from_be_slice(&head.return_data), U256::from(64u64));
            let failure = instance
                .call_function("fail()", &[], options)
                .expect_err("custom error revert");
            let HarnessError::Revert(bytes) = failure else {
                panic!("{failure:?}")
            };
            assert_eq!(bytes.0, expected);
        }
    }

    #[test]
    fn error_selector_evaluation_keeps_division_fault() {
        let source = APPLIED_ERROR_SOURCE
            .replace("sol(\"Failure(bool)\")", "{ let zero: u32 = 0\n1 / zero }");
        let error = generate_test_abi_result(&source, "C")
            .err()
            .expect("selector evaluation fault");
        assert!(error.contains("SELECTOR evaluation failed"), "{error}");
        assert!(error.contains("DivisionByZero"), "{error}");
    }

    #[test]
    fn error_abi_discovery_keeps_shared_growing_instance_refusal() {
        let source = r#"
use std::abi::sol
fn recur<T: Copy>(_ value: T, _ n: u256) -> u256 {
    if n == 0 { 0 } else { 1 + recur(value, n - 1) }
}
msg M {
    #[selector = sol("run()")]
    Run {} -> u256
}
pub contract C { recv M { Run {} -> u256 { recur(1 as u256, 0) } } }
"#;
        generate_test_abi_result(source, "C")
            .expect("same-instance recursion is bounded by key identity");
        let growing = source.replace("recur(value, n - 1)", "recur((value, value), n - 1)");
        let error = generate_test_abi_result(&growing, "C")
            .err()
            .expect("shared instance growth refusal");
        assert!(
            error.contains("cannot admit error ABI call graph"),
            "{error}"
        );
        assert!(error.contains("InstanceLimit"), "{error}");
    }

    #[test]
    fn abi_runtime_payloads_match_empty_nested_and_dynamic_literal_oracles() {
        use contract_harness::{ExecutionOptions, FeContractHarness, HarnessError};
        let mut nested = vec![0u8; 96];
        nested[31] = 1;
        nested[63] = 2;
        nested[95] = 3;
        let mut dynamic = vec![0u8; 128];
        dynamic[31] = 7;
        dynamic[63] = 64;
        dynamic[95] = 3;
        dynamic[96..99].copy_from_slice(&[0xaa, 0xbb, 0xcc]);
        let cases = [
            ("", "Payload {}", 0, false, "let _ = out", Vec::new()),
            (
                "pair: (u256, u256), third: u256",
                "Payload { pair: (1, 2), third: 3 }",
                96,
                false,
                "out.store_word(at: 0, value: self.pair.0)\nout.store_word(at: 32, value: self.pair.1)\nout.store_word(at: 64, value: self.third)",
                nested,
            ),
            (
                "id: u256, bytes: Bytes",
                "Payload { id: 7, bytes: std::abi::bytes_from_words_prefix<3, 1>([0xaabbcc0000000000000000000000000000000000000000000000000000000000]) }",
                64,
                true,
                "out.store_word(at: 0, value: self.id)\nout.store_word(at: 32, value: 64)\nself.bytes.encode(out.tail(64))",
                dynamic,
            ),
        ];
        for (fields, value, head, is_dynamic, handwritten_encode, expected) in cases {
            let record = format!("struct Payload {{ {fields} }}");
            let handwritten_size = if is_dynamic {
                "Self::HEAD_SIZE + self.bytes.payload_size()"
            } else {
                "Self::HEAD_SIZE"
            };
            let manual = format!(
                r#"
{record}
impl AbiSize for Payload {{
    const HEAD_SIZE: u256 = {head}
    const IS_DYNAMIC: bool = {is_dynamic}
    fn payload_size(self) -> u256 {{ {handwritten_size} }}
}}
impl Encode<Sol> for Payload {{
    fn encode(own self, _ out: AbiFrame) {{ {handwritten_encode} }}
}}
"#
            );
            for declaration in [
                format!(
                    "#[generate(std::derive::abi_size)]\n#[generate(std::derive::abi_encode)]\n{record}"
                ),
                format!("#[abi]\n{record}"),
                manual,
            ] {
                let source =
                    include_str!("../../driver/tests/fixtures/four_clients_runtime_payload.fe.in")
                        .replace("DECLARATION", &declaration)
                        .replace("VALUE", value);
                let harness =
                    FeContractHarness::compile("C", &source).expect("compile payload twin");
                let mut instance = harness.deploy_instance().expect("deploy payload twin");
                let error = instance
                    .call_function("payload()", &[], ExecutionOptions::default())
                    .expect_err("raw encoded payload revert");
                let HarnessError::Revert(bytes) = error else {
                    panic!("{error:?}")
                };
                assert_eq!(
                    bytes.0, expected,
                    "fields={fields} declaration={declaration}"
                );
            }
        }
    }

    #[test]
    fn custom_errors_include_reachable_reverts_only() {
        let code = r#"
use std::abi::sol
use std::evm::revert_error
#[error]
struct Failure { code: u256 }
#[error]
struct InitFailure {}
#[error]
struct Unused { code: u256 }
fn fail() { revert_error(Failure { code: 7 }) }
msg M {
    #[selector = sol("fail()")]
    Fail {},
}
pub contract C {
    init(fail: bool) { if fail { revert_error(InitFailure {}) } }
    recv M {
        Fail {} {
            let _ = Unused { code: 3 }
            fail()
        }
    }
}
"#;
        let entries = abi_entries(code, "C");
        let errors: Vec<_> = entries
            .iter()
            .filter(|entry| entry["type"] == "error")
            .collect();
        assert_eq!(errors.len(), 2, "{entries:?}");
        let failure = errors
            .iter()
            .find(|entry| entry["name"] == "Failure")
            .unwrap();
        assert_eq!(
            failure["inputs"],
            serde_json::json!([{"name":"code","type":"uint256"}])
        );
        assert!(failure.get("anonymous").is_none());
        assert!(failure.get("stateMutability").is_none());
    }
    #[test]
    fn custom_errors_include_panic_with_value() {
        let entries = abi_entries(
            r#"
use std::abi::sol
#[error]
struct Failure { code: u256 }
msg M {
    #[selector = sol("fail()")]
    Fail {}
}
pub contract C {
    recv M { Fail {} { core::panic_with_value(Failure { code: 7 }) } }
}
"#,
            "C",
        );
        let errors: Vec<_> = entries
            .iter()
            .filter(|entry| entry["type"] == "error")
            .collect();
        assert_eq!(errors.len(), 1, "{entries:?}");
        assert_eq!(errors[0]["name"], "Failure");
        assert_eq!(
            errors[0]["inputs"],
            serde_json::json!([{"name":"code","type":"uint256"}])
        );
    }

    #[test]
    fn custom_errors_include_result_unwrap() {
        let entries = abi_entries(
            r#"
use core::Result
use std::abi::sol
#[error]
struct Failure { code: u256 }
fn result() -> Result<Failure, u256> { Result::Err(Failure { code: 7 }) }
msg M {
    #[selector = sol("fail()")]
    Fail {} -> u256
}
pub contract C {
    recv M { Fail {} -> u256 { result().unwrap() } }
}
"#,
            "C",
        );
        let errors: Vec<_> = entries
            .iter()
            .filter(|entry| entry["type"] == "error")
            .collect();
        assert_eq!(errors.len(), 1, "{entries:?}");
        assert_eq!(errors[0]["name"], "Failure");
        assert_eq!(
            errors[0]["inputs"],
            serde_json::json!([{"name":"code","type":"uint256"}])
        );
    }

    #[test]
    fn custom_errors_include_overloaded_operators() {
        let entries = abi_entries(
            r#"
use core::ops::Add
use std::abi::sol
use std::evm::revert_error
#[error]
struct AddFailure { code: u256 }
struct Number { value: u256 }
impl Add for Number {
    fn add(own self, _ other: own Number) -> Number {
        revert_error(AddFailure { code: self.value })
    }
}
msg M {
    #[selector = sol("fail()")]
    Fail {} -> u256
}
pub contract C {
    recv M {
        Fail {} -> u256 {
            let a = Number { value: 1 }
            let b = Number { value: 2 }
            (a + b).value
        }
    }
}
"#,
            "C",
        );
        let errors: Vec<_> = entries
            .iter()
            .filter(|entry| entry["type"] == "error")
            .collect();
        assert_eq!(errors.len(), 1, "{entries:?}");
        assert_eq!(errors[0]["name"], "AddFailure");
        assert_eq!(
            errors[0]["inputs"],
            serde_json::json!([{"name":"code","type":"uint256"}])
        );
    }

    #[test]
    fn custom_errors_include_generic_operator_helpers() {
        let entries = abi_entries(
            r#"
use core::ops::Add
use std::abi::sol
use std::evm::revert_error
#[error]
struct AddFailure { code: u256 }
struct Number<T> { value: u256, tag: T }
impl<T> Add for Number<T> {
    fn add(own self, _ other: own Number<T>) -> Number<T> {
        revert_error(AddFailure { code: self.value })
    }
}
fn add<T: Add>(_ lhs: own T, _ rhs: own T) -> T::Output { lhs + rhs }
msg M {
    #[selector = sol("fail()")]
    Fail {} -> u256
}
pub contract C {
    recv M {
        Fail {} -> u256 {
            let a = Number { value: 1, tag: true }
            let b = Number { value: 2, tag: false }
            add(a, b).value
        }
    }
}
"#,
            "C",
        );
        let errors: Vec<_> = entries
            .iter()
            .filter(|entry| entry["type"] == "error")
            .collect();
        assert_eq!(errors.len(), 1, "{entries:?}");
        assert_eq!(errors[0]["name"], "AddFailure");
        assert_eq!(
            errors[0]["inputs"],
            serde_json::json!([{"name":"code","type":"uint256"}])
        );
    }

    #[test]
    fn custom_errors_ignore_non_error_panics_and_user_named_helpers() {
        let entries = abi_entries(
            r#"
use std::abi::sol
#[error]
struct Unused {}
fn panic_with_value(_ value: own Unused) {}
msg M {
    #[selector = sol("fail()")]
    Fail {}
}
pub contract C {
    recv M {
        Fail {} {
            panic_with_value(Unused {})
            core::panic_with_value(7 as u256)
        }
    }
}
"#,
            "C",
        );
        assert!(
            !entries.iter().any(|entry| entry["type"] == "error"),
            "{entries:?}"
        );
    }
    #[test]
    fn raw_memory_does_not_change_abi_state_mutability() {
        let code = r#"
use core::abi::store_word
use core::ptr
use std::abi::sol
use std::evm::RawStorage
msg M {
    #[selector = sol("memoryOnly()")]
    MemoryOnly {},
    #[selector = sol("readStorage()")]
    ReadStorage {},
    #[selector = sol("writeStorage()")]
    WriteStorage {},
}
pub contract C {
    recv M {
        MemoryOnly {} {
            unsafe { store_word(ptr: ptr::alloc_bytes(32), value: 1) }
        }
        ReadStorage {} uses (storage: RawStorage) {}
        WriteStorage {} uses (storage: mut RawStorage) {}
    }
}
"#;
        let entries = abi_entries(code, "C");
        for (name, expected) in [
            ("memoryOnly", "pure"),
            ("readStorage", "view"),
            ("writeStorage", "nonpayable"),
        ] {
            let entry = entries.iter().find(|entry| entry["name"] == name).unwrap();
            assert_eq!(entry["stateMutability"], expected);
        }
    }

    #[test]
    fn abi_export_traverses_typed_calldata_encoding() {
        let entries = abi_entries(
            r#"// ABI-only export panics in instantiate_callable_typed_body on Fe aad737010.
use std::abi::sol
use std::evm::{Address, encode_msg_calldata}
msg TokenMsg {
    #[selector = sol("transfer(address,uint256)")]
    Transfer { to: Address, amount: u256 } -> bool,
}
msg ProbeMsg {
    #[selector = sol("probe()")]
    Probe {},
}
pub contract EncodeMsgAbiProbe {
    recv ProbeMsg {
        Probe {} {
            let encoded = encode_msg_calldata(
                TokenMsg::Transfer { to: Address::new(1), amount: 1 },
            )
        }
    }
}
"#,
            "EncodeMsgAbiProbe",
        );
        assert!(entries.iter().any(|e| e["name"] == "probe"));
    }

    const EVENT_TOPIC_TRANSFER: &str =
        "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
    const EVENT_TOPIC_PAUSED: &str =
        "9e87fac88ff661f02d44f95383c817fece4bce600a3dab7a54406878b965e752";
    const EVENT_TOPIC_NOTE: &str =
        "91314f3d0f410da7dc2d8bd091ec2d373bdd4d9e26f30ada1a7131407c996b03";
    const EVENT_TOPIC_MOVED: &str =
        "a7782439b83e57044d60e67405bf17689b58e4edba6110dd5a0b65d532972d87";
    const EVENT_TOPIC_LEDGER: &str =
        "dbd8ea414a07837829ae10d7902b1399b3279692fe22434c3b62ea7b45cf594e";

    /// Event contract shared by the generated, compiler and handwritten twins.
    /// `INDEX(x)` wraps a value for an indexed field of the generated twin.
    fn event_contract(declarations: &str, indexed_wrapper: bool) -> String {
        let idx = |value: &str| {
            if indexed_wrapper {
                format!("std::evm::Indexed::new({value})")
            } else {
                value.to_string()
            }
        };
        format!(
            r#"
use std::abi::{{Bytes, sol}}
use std::evm::{{Address, Log, TopicValue}}
{declarations}
msg M {{
    #[selector = sol("emitPaused()")]
    EmitPaused {{}},
    #[selector = sol("emitTransfer()")]
    EmitTransfer {{}},
    #[selector = sol("emitNote()")]
    EmitNote {{}},
    #[selector = sol("emitMoved()")]
    EmitMoved {{}},
    #[selector = sol("emitLedger()")]
    EmitLedger {{}},
}}
pub contract C uses (log: mut Log) {{
    recv M {{
        EmitPaused {{}} uses (mut log) {{ log.emit(Paused {{}}) }}
        EmitTransfer {{}} uses (mut log) {{
            log.emit(Transfer {{ from: {from}, to: {to}, value: 500 }})
        }}
        EmitNote {{}} uses (mut log) {{
            log.emit(Note {{
                sender: {from},
                id: 7,
                body: std::abi::bytes_from_words_prefix<3, 1>([0xaabbcc0000000000000000000000000000000000000000000000000000000000]),
            }})
        }}
        EmitMoved {{}} uses (mut log) {{
            log.emit(Moved {{ who: {from}, n: 1, to: Point {{ x: 2, y: 3 }} }})
        }}
        EmitLedger {{}} uses (mut log) {{
            log.emit(Ledger {{ a: {from}, b: {to}, c: {third}, amount: 9 }})
        }}
    }}
}}
"#,
            from = idx("Address::new(0x11)"),
            to = idx("Address::new(0x22)"),
            third = idx("Address::new(0x33)"),
        )
    }

    const GENERATED_EVENTS: &str = r#"
#[abi]
struct Point { x: u256, y: u256 }
#[generate(std::derive::event_struct)]
struct Paused {}
#[generate(std::derive::event_struct)]
struct Transfer { from: std::evm::Indexed<Address>, to: std::evm::Indexed<Address>, value: u256 }
#[generate(std::derive::event_struct)]
struct Note { sender: std::evm::Indexed<Address>, id: u256, body: Bytes }
#[generate(std::derive::event_struct)]
struct Moved { who: std::evm::Indexed<Address>, n: u256, to: Point }
#[generate(std::derive::event_struct)]
struct Ledger {
    a: std::evm::Indexed<Address>,
    b: std::evm::Indexed<Address>,
    c: std::evm::Indexed<Address>,
    amount: u256,
}
"#;

    const COMPILER_EVENTS: &str = r#"
#[abi]
struct Point { x: u256, y: u256 }
#[event]
struct Paused {}
#[event]
struct Transfer {
    #[indexed]
    from: Address,
    #[indexed]
    to: Address,
    value: u256,
}
#[event]
struct Note {
    #[indexed]
    sender: Address,
    id: u256,
    body: Bytes,
}
#[event]
struct Moved {
    #[indexed]
    who: Address,
    n: u256,
    to: Point,
}
#[event]
struct Ledger {
    #[indexed]
    a: Address,
    #[indexed]
    b: Address,
    #[indexed]
    c: Address,
    amount: u256,
}
"#;

    // Every topic0 and data word is written out directly; nothing here uses
    // the generator, the `#[event]` desugaring or the ABI encoder. Indexing is
    // declared the way the JSON reads it: by the field's `Indexed` type.
    fn handwritten_events() -> String {
        format!(
            r#"
#[abi]
struct Point {{ x: u256, y: u256 }}
struct Paused {{}}
impl std::evm::Event for Paused {{
    const TOPIC0: u256 = 0x{EVENT_TOPIC_PAUSED}
    fn emit<L: Log>(own self, _ log: mut L) {{
        log.log1(data: core::ptr::MemBuffer::empty().span(), topic0: Self::TOPIC0)
    }}
}}
struct Transfer {{ from: std::evm::Indexed<Address>, to: std::evm::Indexed<Address>, value: u256 }}
impl std::evm::Event for Transfer {{
    const TOPIC0: u256 = 0x{EVENT_TOPIC_TRANSFER}
    fn emit<L: Log>(own self, _ log: mut L) {{
        let buf = core::ptr::MemBuffer::alloc(32)
        unsafe {{ core::abi::store_word(ptr: buf.ptr(), value: self.value) }}
        log.log3(data: buf.span(), topic0: Self::TOPIC0, topic1: self.from.as_topic(), topic2: self.to.as_topic())
    }}
}}
struct Note {{ sender: std::evm::Indexed<Address>, id: u256, body: Bytes }}
impl std::evm::Event for Note {{
    const TOPIC0: u256 = 0x{EVENT_TOPIC_NOTE}
    fn emit<L: Log>(own self, _ log: mut L) {{
        let buf = core::ptr::MemBuffer::alloc(128)
        unsafe {{
            core::abi::store_word(ptr: buf.ptr(), value: self.id)
            core::abi::store_word(ptr: core::ptr::offset_bytes(buf.ptr(), 32), value: 64)
            core::abi::store_word(ptr: core::ptr::offset_bytes(buf.ptr(), 64), value: 3)
            core::abi::store_word(ptr: core::ptr::offset_bytes(buf.ptr(), 96), value: 0xaabbcc0000000000000000000000000000000000000000000000000000000000)
        }}
        log.log2(data: buf.span(), topic0: Self::TOPIC0, topic1: self.sender.as_topic())
    }}
}}
struct Moved {{ who: std::evm::Indexed<Address>, n: u256, to: Point }}
impl std::evm::Event for Moved {{
    const TOPIC0: u256 = 0x{EVENT_TOPIC_MOVED}
    fn emit<L: Log>(own self, _ log: mut L) {{
        let buf = core::ptr::MemBuffer::alloc(96)
        unsafe {{
            core::abi::store_word(ptr: buf.ptr(), value: self.n)
            core::abi::store_word(ptr: core::ptr::offset_bytes(buf.ptr(), 32), value: self.to.x)
            core::abi::store_word(ptr: core::ptr::offset_bytes(buf.ptr(), 64), value: self.to.y)
        }}
        log.log2(data: buf.span(), topic0: Self::TOPIC0, topic1: self.who.as_topic())
    }}
}}
struct Ledger {{ a: std::evm::Indexed<Address>, b: std::evm::Indexed<Address>, c: std::evm::Indexed<Address>, amount: u256 }}
impl std::evm::Event for Ledger {{
    const TOPIC0: u256 = 0x{EVENT_TOPIC_LEDGER}
    fn emit<L: Log>(own self, _ log: mut L) {{
        let buf = core::ptr::MemBuffer::alloc(32)
        unsafe {{ core::abi::store_word(ptr: buf.ptr(), value: self.amount) }}
        log.log4(
            data: buf.span(),
            topic0: Self::TOPIC0,
            topic1: self.a.as_topic(),
            topic2: self.b.as_topic(),
            topic3: self.c.as_topic(),
        )
    }}
}}
"#
        )
    }

    fn event_json_inputs(entries: &[Value], name: &str) -> Value {
        let matching = entries
            .iter()
            .filter(|entry| entry["type"] == "event" && entry["name"] == name)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "event `{name}` in {entries:?}");
        assert_eq!(matching[0]["anonymous"], false);
        matching[0]["inputs"].clone()
    }

    fn address_word(low: u8) -> Vec<u8> {
        let mut word = vec![0u8; 32];
        word[31] = low;
        word
    }

    fn u256_word(value: u8) -> Vec<u8> {
        address_word(value)
    }

    #[test]
    fn abi_event_runtime_and_json_match_literal_oracles() {
        use contract_harness::{ExecutionOptions, FeContractHarness};

        let handwritten = handwritten_events();
        let twins = [
            ("generated", event_contract(GENERATED_EVENTS, true)),
            ("compiler", event_contract(COMPILER_EVENTS, false)),
            ("handwritten", event_contract(&handwritten, true)),
        ];
        let topic = |hex_text: &str| hex::decode(hex_text).unwrap();
        let mut note_data = Vec::new();
        for word in [7u8, 64, 3] {
            note_data.extend(u256_word(word));
        }
        let mut tail = vec![0u8; 32];
        tail[..3].copy_from_slice(&[0xaa, 0xbb, 0xcc]);
        note_data.extend(tail);
        let moved_data = [1u8, 2, 3].iter().flat_map(|w| u256_word(*w)).collect::<Vec<_>>();
        // (call, topics, data), each written out from `cast keccak`/`cast abi-encode`.
        let expected: Vec<(&str, Vec<Vec<u8>>, Vec<u8>)> = vec![
            ("emitPaused()", vec![topic(EVENT_TOPIC_PAUSED)], Vec::new()),
            (
                "emitTransfer()",
                vec![
                    topic(EVENT_TOPIC_TRANSFER),
                    address_word(0x11),
                    address_word(0x22),
                ],
                u256_word_u16(500),
            ),
            (
                "emitNote()",
                vec![topic(EVENT_TOPIC_NOTE), address_word(0x11)],
                note_data,
            ),
            (
                "emitMoved()",
                vec![topic(EVENT_TOPIC_MOVED), address_word(0x11)],
                moved_data,
            ),
            (
                "emitLedger()",
                vec![
                    topic(EVENT_TOPIC_LEDGER),
                    address_word(0x11),
                    address_word(0x22),
                    address_word(0x33),
                ],
                u256_word(9),
            ),
        ];
        let expected_json = serde_json::json!({
            "Paused": [],
            "Transfer": [
                {"name":"from","type":"address","indexed":true},
                {"name":"to","type":"address","indexed":true},
                {"name":"value","type":"uint256","indexed":false}
            ],
            "Note": [
                {"name":"sender","type":"address","indexed":true},
                {"name":"id","type":"uint256","indexed":false},
                {"name":"body","type":"bytes","indexed":false}
            ],
            "Moved": [
                {"name":"who","type":"address","indexed":true},
                {"name":"n","type":"uint256","indexed":false},
                {"name":"to","type":"tuple","indexed":false,"components":[
                    {"name":"x","type":"uint256"},{"name":"y","type":"uint256"}
                ]}
            ],
            "Ledger": [
                {"name":"a","type":"address","indexed":true},
                {"name":"b","type":"address","indexed":true},
                {"name":"c","type":"address","indexed":true},
                {"name":"amount","type":"uint256","indexed":false}
            ],
        });
        for (label, source) in &twins {
            let entries = abi_entries(source, "C");
            assert_eq!(
                entries.iter().filter(|e| e["type"] == "event").count(),
                5,
                "{label}: {entries:?}"
            );
            for (name, inputs) in expected_json.as_object().unwrap() {
                assert_eq!(
                    &event_json_inputs(&entries, name),
                    inputs,
                    "{label}: event {name}"
                );
            }
            let harness = FeContractHarness::compile("C", source)
                .unwrap_or_else(|error| panic!("{label}: {error:?}"));
            let mut instance = harness.deploy_instance().expect("deploy event twin");
            for (call, topics, data) in &expected {
                let calldata = selector_for_signature(call).to_be_bytes();
                let outcome = instance
                    .call_raw_with_logs(&calldata, ExecutionOptions::default())
                    .unwrap_or_else(|error| panic!("{label} {call}: {error:?}"));
                assert_eq!(outcome.raw_logs.len(), 1, "{label} {call}");
                let log = &outcome.raw_logs[0];
                let actual_topics = log
                    .data
                    .topics()
                    .iter()
                    .map(|topic| topic.as_slice().to_vec())
                    .collect::<Vec<_>>();
                assert_eq!(&actual_topics, topics, "{label} {call} topics");
                assert_eq!(log.data.data.as_ref(), data.as_slice(), "{label} {call} data");
            }
        }
    }

    fn u256_word_u16(value: u16) -> Vec<u8> {
        let mut word = vec![0u8; 32];
        word[30..].copy_from_slice(&value.to_be_bytes());
        word
    }

    fn event_names(entries: &[Value]) -> Vec<String> {
        entries
            .iter()
            .filter(|entry| entry["type"] == "event")
            .map(|entry| entry["name"].as_str().unwrap().to_string())
            .collect()
    }

    const GENERATED_EMIT_SOURCE: &str = r#"
use std::abi::sol
use std::evm::{Address, Event, Indexed, Log}
#[generate(std::derive::event_struct)]
struct Direct { who: Indexed<Address>, value: u256 }
#[generate(std::derive::event_struct)]
struct ViaLog { value: u256 }
#[generate(std::derive::event_struct)]
struct ViaHelper { value: u256 }
#[generate(std::derive::event_struct)]
struct Never { value: u256 }
#[generate(std::derive::event_struct)]
struct Constructed { value: u256 }
fn emit_event<E: Event>(_ event: own E) uses (log: mut Log) { log.emit(event) }
fn construct_only() { let _ = Constructed { value: 1 } }
msg M {
    #[selector = sol("go()")]
    Go {},
}
pub contract C uses (log: mut Log) {
    recv M {
        Go {} uses (mut log) {
            construct_only()
            Direct { who: Indexed::new(Address::new(1)), value: 2 }.emit(mut log)
            log.emit(ViaLog { value: 3 })
            emit_event(ViaHelper { value: 4 })
        }
    }
}
"#;

    #[test]
    fn event_abi_discovery_uses_proofs_for_generated_direct_log_and_generic_emits() {
        let entries = abi_entries(GENERATED_EMIT_SOURCE, "C");
        assert_eq!(
            event_names(&entries),
            ["Direct", "ViaLog", "ViaHelper"],
            "{entries:?}"
        );
        assert_eq!(
            event_json_inputs(&entries, "Direct"),
            serde_json::json!([
                {"name":"who","type":"address","indexed":true},
                {"name":"value","type":"uint256","indexed":false}
            ])
        );
    }

    #[test]
    fn event_topic0_that_disagrees_with_the_signature_is_refused() {
        let source = format!(
            r#"
use std::abi::sol
use std::evm::{{Address, Log}}
struct Transfer {{ value: u256 }}
impl std::evm::Event for Transfer {{
    const TOPIC0: u256 = 0x{EVENT_TOPIC_TRANSFER}
    fn emit<L: Log>(own self, _ log: mut L) {{
        log.log1(data: core::ptr::MemBuffer::empty().span(), topic0: Self::TOPIC0)
    }}
}}
msg M {{
    #[selector = sol("go()")]
    Go {{}},
}}
pub contract C uses (log: mut Log) {{
    recv M {{ Go {{}} uses (mut log) {{ log.emit(Transfer {{ value: 1 }}) }} }}
}}
"#
        );
        let error = generate_test_abi_result(&source, "C")
            .err()
            .expect("noncanonical event topic");
        assert!(
            error.contains("event TOPIC0 does not match canonical signature `Transfer(uint256)`"),
            "{error}"
        );
        // The same body with the matching hash is accepted.
        let accepted = source.replace(EVENT_TOPIC_TRANSFER, &cast_topic("Transfer(uint256)"));
        let entries = abi_entries(&accepted, "C");
        assert_eq!(event_names(&entries), ["Transfer"]);
    }

    fn cast_topic(signature: &str) -> String {
        let mut hasher = Keccak::v256();
        let mut output = [0u8; 32];
        hasher.update(signature.as_bytes());
        hasher.finalize(&mut output);
        hex::encode(output)
    }

    #[test]
    fn event_discovery_ignores_lookalike_emit_traits_and_unindexed_lookalike_wrappers() {
        let source = r#"
use std::abi::sol
use std::evm::{Address, Log}
mod lookalike {
    pub trait Event { fn emit<L>(own self, _ log: mut L) }
    pub struct Indexed<T> { pub value: T }
}
use lookalike::Event
struct Fake { value: u256 }
impl Event for Fake { fn emit<L>(own self, _ log: mut L) { let _ = self.value } }
msg M {
    #[selector = sol("go()")]
    Go {},
}
pub contract C uses (log: mut Log) {
    recv M { Go {} uses (mut log) { Fake { value: 1 }.emit(mut log) } }
}
"#;
        assert!(event_names(&abi_entries(source, "C")).is_empty());

        // A same-named wrapper does not become the canonical indexed owner:
        // its field stays an ordinary tuple-shaped data field.
        let wrapper = format!(
            r#"
use std::abi::sol
use std::evm::{{Address, Log}}
mod lookalike {{ pub struct Indexed<T> {{ pub value: T }} }}
struct Trade {{ who: lookalike::Indexed<u256> }}
impl std::evm::Event for Trade {{
    const TOPIC0: u256 = 0x{}
    fn emit<L: Log>(own self, _ log: mut L) {{
        log.log1(data: core::ptr::MemBuffer::empty().span(), topic0: Self::TOPIC0)
    }}
}}
msg M {{
    #[selector = sol("go()")]
    Go {{}},
}}
pub contract C uses (log: mut Log) {{
    recv M {{ Go {{}} uses (mut log) {{ log.emit(Trade {{ who: lookalike::Indexed {{ value: 1 }} }}) }} }}
}}
"#,
            cast_topic("Trade((uint256))")
        );
        let entries = abi_entries(&wrapper, "C");
        let inputs = event_json_inputs(&entries, "Trade");
        assert_eq!(inputs[0]["type"], "tuple", "{inputs}");
        assert_eq!(inputs[0]["indexed"], false, "{inputs}");
    }

    #[test]
    fn event_field_proof_is_sealed_against_user_claims_of_indexing() {
        let source = r#"
use std::derive::EventField
struct Claim { value: u256 }
impl EventField for Claim {
    const IS_INDEXED: bool = true
    const DATA_HEAD_SIZE: u256 = 0
    fn data_dynamic_size(self) -> u256 { 0 }
    fn emit_field(
        own self,
        _ out: core::abi::AbiFrame,
        head_pos: u256,
        tails: own core::abi::AbiFrame,
    ) -> (core::abi::AbiFrame, u256) {
        let _ = out
        let _ = head_pos
        (tails, self.value)
    }
}
"#;
        let mut db = DriverDataBase::default();
        let url = url::Url::parse("file:///tmp/event-field-seal.fe").unwrap();
        let file = db.workspace().touch(&mut db, url, Some(source.to_string()));
        let diagnostics = db.run_on_top_mod(db.top_mod(file));
        let rendered = diagnostics.format_diags(&db);
        assert!(
            rendered.contains("`Claim` doesn't implement `EventFieldSeal`"),
            "{rendered}"
        );
    }

    #[test]
    fn event_discovery_respects_unused_and_invoked_closure_regions() {
        for (body, expected) in [
            ("let f = || log.emit(Quiet { value: 1 })\nlet _ = f", false),
            ("let f = || log.emit(Quiet { value: 1 })\nf()", true),
        ] {
            let code = format!(
                r#"
use std::abi::sol
use std::evm::Log
#[generate(std::derive::event_struct)]
struct Quiet {{ value: u256 }}
msg M {{
    #[selector = sol("go()")]
    Go {{}},
}}
pub contract C uses (log: mut Log) {{
    recv M {{ Go {{}} uses (mut log) {{ {body} }} }}
}}
"#
            );
            let entries = abi_entries(&code, "C");
            assert_eq!(
                event_names(&entries),
                if expected { vec!["Quiet"] } else { Vec::<&str>::new() },
                "body={body} entries={entries:?}"
            );
        }
    }

    #[test]
    fn event_json_refuses_more_than_three_indexed_fields() {
        let source = format!(
            r#"
use std::abi::sol
use std::evm::{{Indexed, Log}}
struct Four {{
    a: Indexed<u256>,
    b: Indexed<u256>,
    c: Indexed<u256>,
    d: Indexed<u256>,
}}
impl std::evm::Event for Four {{
    const TOPIC0: u256 = 0x{}
    fn emit<L: Log>(own self, _ log: mut L) {{
        log.log1(data: core::ptr::MemBuffer::empty().span(), topic0: Self::TOPIC0)
    }}
}}
msg M {{
    #[selector = sol("go()")]
    Go {{}},
}}
pub contract C uses (log: mut Log) {{
    recv M {{
        Go {{}} uses (mut log) {{
            log.emit(Four {{
                a: Indexed::new(1),
                b: Indexed::new(2),
                c: Indexed::new(3),
                d: Indexed::new(4),
            }})
        }}
    }}
}}
"#,
            cast_topic("Four(uint256,uint256,uint256,uint256)")
        );
        let error = generate_test_abi_result(&source, "C")
            .err()
            .expect("four indexed fields have no log encoding");
        assert!(error.contains("has 4 indexed fields; at most 3"), "{error}");
    }
}
