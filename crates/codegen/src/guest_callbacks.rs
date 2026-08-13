//! Compiler-visible gate for target-neutral guest callbacks.
//!
//! Callback signature shape comes from the normalized interface world and
//! authored-body shape comes from Fe semantic/MIR types. The compiler joins
//! those sources directly; there is no callback-registration JSON manifest and
//! no caller-authored scalar signature or lifetime-policy table.

use std::collections::BTreeSet;

use compiler_db::DriverDataBase;
use fe_host_abi::{CoreType, World};
use hir::{analysis::ty::ty_check::BodyOwner, hir_def::Visibility};
use mir::{
    AddressSpaceKind, Layout, RefKind, RuntimeClass, RuntimeFunctionOwner, RuntimeLinkage,
    RuntimePackage, ScalarRepr,
    runtime::stable_key::{ingot_component_for_scope, module_path_components_for_scope},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuestCallbackMaterializerCapability {
    ExportTrampoline,
    /// State which remains available across separate Wasm export calls.
    PersistentGuestState,
    GuestRegistrationTable,
    GenerationValidation,
    IndirectCall,
    AuthoredBodyResolution,
}

/// Why callback registration exports cannot yet be emitted by the Wasm
/// backend. Keeping this machine-readable prevents callers from mistaking the
/// target-neutral table model for an embedded implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WasmGuestCallbackEmbeddingBlocker {
    pub missing_sonatina_global_lowering: bool,
    pub missing_checked_memory_reservation: bool,
    pub required_backend_contract: &'static str,
}

pub const WASM_GUEST_CALLBACK_EMBEDDING_BLOCKER: WasmGuestCallbackEmbeddingBlocker =
    WasmGuestCallbackEmbeddingBlocker {
        missing_sonatina_global_lowering: true,
        missing_checked_memory_reservation: true,
        required_backend_contract: "lower mutable Sonatina globals, or reserve a checked \
            compiler-owned linear-memory range whose base and length are shared with the \
            canonical allocator",
    };

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestCallbackMaterializerProfile {
    pub name: &'static str,
    pub capabilities: BTreeSet<GuestCallbackMaterializerCapability>,
}

impl GuestCallbackMaterializerProfile {
    /// What the Wasm backend implements today. It can synthesize a statically
    /// linked public wrapper, but cannot register or indirectly dispatch guest
    /// callback bodies.
    pub fn current_wasm() -> Self {
        Self {
            name: "fe-wasm-guest-callbacks-v0",
            capabilities: BTreeSet::from([
                GuestCallbackMaterializerCapability::ExportTrampoline,
                GuestCallbackMaterializerCapability::AuthoredBodyResolution,
            ]),
        }
    }

    /// Scalar synchronous callback materialization available only when the
    /// reviewed Sonatina indirect-call overlay is selected.
    #[cfg(feature = "sonatina-indirect-calls")]
    pub fn overlay_wasm() -> Self {
        Self {
            name: "fe-wasm-guest-callbacks-overlay-v1",
            capabilities: BTreeSet::from([
                GuestCallbackMaterializerCapability::ExportTrampoline,
                GuestCallbackMaterializerCapability::PersistentGuestState,
                GuestCallbackMaterializerCapability::GuestRegistrationTable,
                GuestCallbackMaterializerCapability::GenerationValidation,
                GuestCallbackMaterializerCapability::IndirectCall,
                GuestCallbackMaterializerCapability::AuthoredBodyResolution,
            ]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestCallbackMaterializationError {
    InvalidBinding(String),
    MissingCapabilities {
        profile: &'static str,
        missing: BTreeSet<GuestCallbackMaterializerCapability>,
    },
}

/// Internal compiler identity for one authored Fe callback body.
///
/// This is semantic compiler data, not a serializable host ABI. The future
/// Fe-facing authoring role will derive it without spelling paths at the call
/// site; keeping it internal prevents an interim JSON protocol from becoming
/// runtime architecture.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GuestFunctionIdentity {
    pub ingot: String,
    pub module_path: Vec<String>,
    pub function: String,
}

/// One compiler-internal association between an interface callback signature
/// and an authored Fe body. Core lanes and token policy are derived, not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestCallbackBinding {
    pub signature_id: String,
    pub body: GuestFunctionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGuestCallback {
    pub signature_id: String,
    pub body: GuestFunctionIdentity,
    pub runtime_instance_key: String,
    pub runtime_symbol: String,
    pub core_params: Vec<CoreType>,
    pub core_results: Vec<CoreType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestCallbackResolutionError {
    InvalidBinding(String),
    Missing {
        body: GuestFunctionIdentity,
    },
    Ambiguous {
        body: GuestFunctionIdentity,
        candidates: Vec<String>,
    },
    NotPublic {
        body: GuestFunctionIdentity,
    },
    NotCallable {
        body: GuestFunctionIdentity,
    },
    SignatureMismatch {
        detail: Box<GuestCallbackSignatureMismatch>,
    },
    UnsupportedScalar {
        body: GuestFunctionIdentity,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestCallbackSignatureMismatch {
    pub body: GuestFunctionIdentity,
    pub expected_params: Vec<CoreType>,
    pub actual_params: Vec<CoreType>,
    pub expected_results: Vec<CoreType>,
    pub actual_results: Vec<CoreType>,
}

/// Resolve authored identities against semantic functions already present in a
/// MIR package. Multiple monomorphizations of one authored function are an
/// explicit ambiguity until Fe authoring metadata can name type arguments.
pub fn resolve_guest_callbacks(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    world: &World,
    bindings: &[GuestCallbackBinding],
) -> Result<Vec<ResolvedGuestCallback>, GuestCallbackResolutionError> {
    let bindings = normalize_guest_callback_bindings(world, bindings)
        .map_err(GuestCallbackResolutionError::InvalidBinding)?;
    let functions = package.functions(db);
    let mut resolved = Vec::with_capacity(bindings.len());
    for registration in &bindings {
        let plan = world
            .callback_export_plan(&registration.signature_id)
            .map_err(|error| GuestCallbackResolutionError::InvalidBinding(error.to_string()))?;
        let expected_params = plan.params[1..].to_vec();
        let expected_results = plan.results;
        let mut matches = functions
            .iter()
            .filter_map(|function| {
                let RuntimeFunctionOwner::Semantic(semantic) = function.owner(db) else {
                    return None;
                };
                let BodyOwner::Func(func) = semantic.key(db).owner(db) else {
                    return None;
                };
                let identity = GuestFunctionIdentity {
                    ingot: ingot_component_for_scope(db, func.scope()),
                    module_path: module_path_components_for_scope(db, func.scope()),
                    function: func.name(db).to_opt()?.data(db).to_string(),
                };
                (identity == registration.body).then_some((*function, func))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(function, _)| {
            mir::runtime_instance_stable_key(db, function.instance(db))
        });
        let candidate_keys = matches
            .iter()
            .map(|(function, _)| mir::runtime_instance_stable_key(db, function.instance(db)))
            .collect::<Vec<_>>();
        ensure_unique_candidate(&registration.body, &candidate_keys)?;
        let (function, func) = matches[0];
        if func.vis(db) != Visibility::Public {
            return Err(GuestCallbackResolutionError::NotPublic {
                body: registration.body.clone(),
            });
        }
        if function.linkage(db) == RuntimeLinkage::External {
            return Err(GuestCallbackResolutionError::NotCallable {
                body: registration.body.clone(),
            });
        }
        let runtime_body = function.instance(db).body(db);
        if runtime_body.blocks.is_empty() {
            return Err(GuestCallbackResolutionError::NotCallable {
                body: registration.body.clone(),
            });
        }
        let actual_params = runtime_body
            .signature
            .params
            .iter()
            .map(|param| flat_core_types(db, &param.class))
            .collect::<Result<Vec<_>, _>>()
            .map(|lanes| lanes.into_iter().flatten().collect())
            .map_err(|detail| GuestCallbackResolutionError::UnsupportedScalar {
                body: registration.body.clone(),
                detail,
            })?;
        let actual_results = runtime_body
            .signature
            .ret
            .as_ref()
            .map(|class| flat_core_types(db, class))
            .transpose()
            .map_err(|detail| GuestCallbackResolutionError::UnsupportedScalar {
                body: registration.body.clone(),
                detail,
            })?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if actual_params != expected_params || actual_results != expected_results {
            return Err(GuestCallbackResolutionError::SignatureMismatch {
                detail: Box::new(GuestCallbackSignatureMismatch {
                    body: registration.body.clone(),
                    expected_params,
                    actual_params,
                    expected_results,
                    actual_results,
                }),
            });
        }
        resolved.push(ResolvedGuestCallback {
            signature_id: registration.signature_id.clone(),
            body: registration.body.clone(),
            runtime_instance_key: mir::runtime_instance_stable_key(db, function.instance(db)),
            runtime_symbol: function.symbol(db).clone(),
            core_params: expected_params,
            core_results: expected_results,
        });
    }
    Ok(resolved)
}

fn ensure_unique_candidate(
    body: &GuestFunctionIdentity,
    candidates: &[String],
) -> Result<(), GuestCallbackResolutionError> {
    if candidates.is_empty() {
        return Err(GuestCallbackResolutionError::Missing { body: body.clone() });
    }
    if candidates.len() > 1 {
        return Err(GuestCallbackResolutionError::Ambiguous {
            body: body.clone(),
            candidates: candidates.to_vec(),
        });
    }
    Ok(())
}

fn scalar_core_type(class: &RuntimeClass<'_>) -> Result<CoreType, String> {
    let RuntimeClass::Scalar(scalar) = class else {
        return Err(format!("non-scalar runtime class {class:?}"));
    };
    match scalar.repr {
        ScalarRepr::Bool | ScalarRepr::Int { bits: 1..=32, .. } => Ok(CoreType::I32),
        ScalarRepr::Int { bits: 33..=64, .. } => Ok(CoreType::I64),
        ScalarRepr::Float { bits: 32 } => Ok(CoreType::F32),
        ScalarRepr::Float { bits: 64 } => Ok(CoreType::F64),
        _ => Err(format!(
            "unsupported scalar representation {:?}",
            scalar.repr
        )),
    }
}

/// Flatten the same closed scalar-product envelope used by the Wasm function
/// boundary. This admits generated resource newtypes such as
/// `struct Event { handle: u32 }` without assigning any meaning to `Event`:
/// it is simply a one-lane aggregate. Arrays, enums, references, strings, and
/// other address-shaped values remain gated.
fn flat_core_types(db: &DriverDataBase, class: &RuntimeClass<'_>) -> Result<Vec<CoreType>, String> {
    match class {
        RuntimeClass::Scalar(_) => scalar_core_type(class).map(|lane| vec![lane]),
        RuntimeClass::AggregateValue { layout } => match layout.data(db) {
            Layout::Struct(struct_) => {
                struct_
                    .fields
                    .iter()
                    .try_fold(Vec::new(), |mut lanes, field| {
                        lanes.extend(flat_core_types(db, field)?);
                        Ok(lanes)
                    })
            }
            Layout::Array(_) | Layout::Enum(_) => {
                Err(format!("non-flat aggregate runtime class {class:?}"))
            }
        },
        RuntimeClass::Ref {
            pointee,
            kind:
                RefKind::Provider {
                    space: AddressSpaceKind::Memory,
                    ..
                },
            ..
        } => {
            let lanes = flat_core_types(db, pointee)?;
            if lanes.len() == 1 {
                Ok(lanes)
            } else {
                Err(format!(
                    "provider-wrapped callback parameter must flatten to exactly one lane, found {} in {class:?}",
                    lanes.len()
                ))
            }
        }
        RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => {
            Err(format!("address-shaped runtime class {class:?}"))
        }
    }
}

/// Validate registrations and admit them to backend materialization only when
/// the selected backend declares every required mechanism.
pub fn prepare_guest_callback_materialization(
    world: &World,
    bindings: &[GuestCallbackBinding],
    profile: &GuestCallbackMaterializerProfile,
) -> Result<Vec<GuestCallbackBinding>, GuestCallbackMaterializationError> {
    let bindings = normalize_guest_callback_bindings(world, bindings)
        .map_err(GuestCallbackMaterializationError::InvalidBinding)?;
    let required = BTreeSet::from([
        GuestCallbackMaterializerCapability::ExportTrampoline,
        GuestCallbackMaterializerCapability::PersistentGuestState,
        GuestCallbackMaterializerCapability::GuestRegistrationTable,
        GuestCallbackMaterializerCapability::GenerationValidation,
        GuestCallbackMaterializerCapability::IndirectCall,
        GuestCallbackMaterializerCapability::AuthoredBodyResolution,
    ]);
    let missing = required
        .difference(&profile.capabilities)
        .copied()
        .collect::<BTreeSet<_>>();
    if !missing.is_empty() {
        return Err(GuestCallbackMaterializationError::MissingCapabilities {
            profile: profile.name,
            missing,
        });
    }
    Ok(bindings)
}

fn normalize_guest_callback_bindings(
    world: &World,
    bindings: &[GuestCallbackBinding],
) -> Result<Vec<GuestCallbackBinding>, String> {
    world.validate().map_err(|error| error.to_string())?;
    let mut normalized = bindings.to_vec();
    normalized.sort_by(|left, right| left.signature_id.cmp(&right.signature_id));
    let mut signatures = BTreeSet::new();
    let mut bodies = BTreeSet::new();
    for binding in &normalized {
        if !signatures.insert(binding.signature_id.as_str()) {
            return Err(format!(
                "duplicate callback signature `{}`",
                binding.signature_id
            ));
        }
        if !bodies.insert(&binding.body) {
            return Err(format!(
                "authored Fe callback body `{}::{}` is bound more than once",
                binding.body.module_path.join("::"),
                binding.body.function
            ));
        }
        let plan = world
            .callback_export_plan(&binding.signature_id)
            .map_err(|error| error.to_string())?;
        if let Some(blocker) = plan.blocker {
            return Err(blocker);
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::InputDb;
    use driver::DriverDataBase;
    use fe_host_abi::{CoreType, FunctionType, Param, Type, TypeDef, TypeDefKind};
    use url::Url;

    fn fixture() -> (World, Vec<GuestCallbackBinding>) {
        let world = World {
            name: "callbacks".into(),
            types: vec![TypeDef {
                name: "listener".into(),
                kind: TypeDefKind::Callback {
                    signature: FunctionType {
                        params: vec![Param {
                            name: "value".into(),
                            type_: Type::I32,
                        }],
                        result: Some(Type::I32),
                        async_: false,
                    },
                },
            }],
            ..World::default()
        };
        let bindings = vec![GuestCallbackBinding {
            signature_id: "listener".into(),
            body: GuestFunctionIdentity {
                ingot: "app".into(),
                module_path: vec!["handlers".into()],
                function: "on_value".into(),
            },
        }];
        (world, bindings)
    }

    #[test]
    fn current_wasm_fails_before_emitting_a_fake_dispatcher() {
        let (world, bindings) = fixture();
        let error = prepare_guest_callback_materialization(
            &world,
            &bindings,
            &GuestCallbackMaterializerProfile::current_wasm(),
        )
        .unwrap_err();
        let GuestCallbackMaterializationError::MissingCapabilities { missing, .. } = error else {
            panic!("expected capability gate")
        };
        assert!(missing.contains(&GuestCallbackMaterializerCapability::GuestRegistrationTable));
        assert!(missing.contains(&GuestCallbackMaterializerCapability::PersistentGuestState));
        assert!(missing.contains(&GuestCallbackMaterializerCapability::GenerationValidation));
        assert!(missing.contains(&GuestCallbackMaterializerCapability::IndirectCall));
        assert!(!missing.contains(&GuestCallbackMaterializerCapability::AuthoredBodyResolution));
        assert!(!missing.contains(&GuestCallbackMaterializerCapability::ExportTrampoline));
    }

    #[test]
    fn wasm_embedding_blocker_requires_backend_owned_persistent_storage() {
        let blocker = WASM_GUEST_CALLBACK_EMBEDDING_BLOCKER;
        assert!(blocker.missing_sonatina_global_lowering);
        assert!(blocker.missing_checked_memory_reservation);
        assert!(blocker.required_backend_contract.contains("compiler-owned"));
        assert!(
            blocker
                .required_backend_contract
                .contains("canonical allocator")
        );
    }

    #[test]
    fn callback_bindings_are_normalized_and_lanes_are_interface_derived() {
        let (mut world, bindings) = fixture();
        world.types.push(TypeDef {
            name: "listener-z".into(),
            kind: TypeDefKind::Callback {
                signature: FunctionType {
                    params: vec![Param {
                        name: "value".into(),
                        type_: Type::I64,
                    }],
                    result: Some(Type::I64),
                    async_: false,
                },
            },
        });
        let mut reversed = vec![
            GuestCallbackBinding {
                signature_id: "listener-z".into(),
                body: GuestFunctionIdentity {
                    ingot: "app".into(),
                    module_path: vec!["handlers".into()],
                    function: "on_i64".into(),
                },
            },
            bindings[0].clone(),
        ];
        let normalized = normalize_guest_callback_bindings(&world, &reversed).unwrap();
        assert_eq!(
            normalized
                .iter()
                .map(|binding| binding.signature_id.as_str())
                .collect::<Vec<_>>(),
            ["listener", "listener-z"]
        );
        let plan = world.callback_export_plan("listener-z").unwrap();
        assert_eq!(plan.params[1..], [CoreType::I64]);
        assert_eq!(plan.results, [CoreType::I64]);

        reversed[0].body = reversed[1].body.clone();
        assert!(
            normalize_guest_callback_bindings(&world, &reversed)
                .unwrap_err()
                .contains("bound more than once")
        );
    }

    #[test]
    fn resolves_public_authored_body_and_rejects_missing_and_signature_drift() {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///guest_callback_resolution.fe").unwrap();
        db.workspace().touch(
            &mut db,
            url.clone(),
            Some(
                "pub fn on_value(value: i32) -> i32 { value + 1 }\n\
                 fn private_value(value: i32) -> i32 { value }\n\
                 pub fn keep_private_reachable(value: i32) -> i32 { private_value(value) }\n\
                 pub fn on_i64(value: i64) -> i64 { value }\n"
                    .to_owned(),
            ),
        );
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let package = mir::build_wasm_runtime_package(&db, top_mod).unwrap();
        let function = package
            .functions(&db)
            .into_iter()
            .find(|function| {
                let RuntimeFunctionOwner::Semantic(semantic) = function.owner(&db) else {
                    return false;
                };
                matches!(
                    semantic.key(&db).owner(&db),
                    BodyOwner::Func(func)
                        if func.name(&db).to_opt().is_some_and(|name| name.data(&db) == "on_value")
                )
            })
            .unwrap();
        let RuntimeFunctionOwner::Semantic(semantic) = function.owner(&db) else {
            unreachable!()
        };
        let BodyOwner::Func(func) = semantic.key(&db).owner(&db) else {
            unreachable!()
        };
        let identity = GuestFunctionIdentity {
            ingot: ingot_component_for_scope(&db, func.scope()),
            module_path: module_path_components_for_scope(&db, func.scope()),
            function: "on_value".into(),
        };
        let (world, mut bindings) = fixture();
        bindings[0].body = identity.clone();
        let resolved = resolve_guest_callbacks(&db, &package, &world, &bindings).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].body, identity);
        assert!(!resolved[0].runtime_instance_key.is_empty());
        assert!(!resolved[0].runtime_symbol.is_empty());
        assert_eq!(resolved[0].core_params, [CoreType::I32]);
        assert_eq!(resolved[0].core_results, [CoreType::I32]);

        bindings[0].body.function = "missing".into();
        assert!(matches!(
            resolve_guest_callbacks(&db, &package, &world, &bindings),
            Err(GuestCallbackResolutionError::Missing { .. })
        ));

        bindings[0].body = identity;
        bindings[0].body.function = "on_i64".into();
        assert!(matches!(
            resolve_guest_callbacks(&db, &package, &world, &bindings),
            Err(GuestCallbackResolutionError::SignatureMismatch { .. })
        ));
    }

    #[test]
    fn ambiguous_authored_body_reports_sorted_stable_candidates() {
        let body = GuestFunctionIdentity {
            ingot: "app".into(),
            module_path: vec!["lib".into()],
            function: "generic_callback".into(),
        };
        let candidates = vec!["instance-a".to_owned(), "instance-b".to_owned()];
        assert_eq!(
            ensure_unique_candidate(&body, &candidates),
            Err(GuestCallbackResolutionError::Ambiguous { body, candidates })
        );
    }

    #[cfg(feature = "sonatina-indirect-calls")]
    #[test]
    fn overlay_wasm_guest_callback_registration_dispatch_and_release_capstone() {
        use sonatina_codegen::{Backend as _, isa::wasm::WasmBackend as SonatinaWasmBackend};

        let event_idl = r#"
            interface Event {
                readonly attribute long code;
            };
            callback EventListener = long (Event event);
            interface ScalarEventTarget {
                undefined addEventListener(DOMString type, EventListener listener);
                undefined removeEventListener(DOMString type, EventListener listener);
            };
        "#;
        let event_world = fe_webidl_bindgen::parse(event_idl).unwrap();
        let callback_world = World {
            name: "event-callbacks".into(),
            types: vec![
                TypeDef {
                    name: "EventListener".into(),
                    kind: TypeDefKind::Callback {
                        signature: FunctionType {
                            params: vec![Param {
                                name: "event".into(),
                                type_: Type::I32,
                            }],
                            result: Some(Type::I32),
                            async_: false,
                        },
                    },
                },
                TypeDef {
                    name: "i64-listener".into(),
                    kind: TypeDefKind::Callback {
                        signature: FunctionType {
                            params: vec![Param {
                                name: "value".into(),
                                type_: Type::I64,
                            }],
                            result: Some(Type::I64),
                            async_: false,
                        },
                    },
                },
            ],
            ..World::default()
        };
        // Only the Event resource/property imports enter the Fe core-Wasm
        // compilation unit. DOMString remains on the generated semantic
        // adapter side until canonical string memory is available here.
        let event_import_world =
            fe_webidl_bindgen::parse("interface Event { readonly attribute long code; };").unwrap();
        let mut source = fe_webidl_bindgen::emit_fe_raw(&event_import_world, "fe:web").unwrap();
        source.push_str(
            "\npub fn on_event(event: Event) -> i32 {\n\
             \x20   event_get_code(event)\n\
             }\n\
             pub fn on_i64(value: i64) -> i64 { value }\n",
        );
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///guest_callback_capstone.fe").unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let package = mir::build_wasm_runtime_package(&db, top_mod).unwrap();

        let identity = |name: &str| {
            let function = package
                .functions(&db)
                .into_iter()
                .find(|function| {
                    let RuntimeFunctionOwner::Semantic(semantic) = function.owner(&db) else {
                        return false;
                    };
                    matches!(
                        semantic.key(&db).owner(&db),
                        BodyOwner::Func(func)
                            if func.name(&db).to_opt().is_some_and(|candidate| {
                                candidate.data(&db) == name
                            })
                    )
                })
                .unwrap();
            let RuntimeFunctionOwner::Semantic(semantic) = function.owner(&db) else {
                unreachable!()
            };
            let BodyOwner::Func(func) = semantic.key(&db).owner(&db) else {
                unreachable!()
            };
            GuestFunctionIdentity {
                ingot: ingot_component_for_scope(&db, func.scope()),
                module_path: module_path_components_for_scope(&db, func.scope()),
                function: name.into(),
            }
        };
        let bindings = vec![
            GuestCallbackBinding {
                signature_id: "EventListener".into(),
                body: identity("on_event"),
            },
            GuestCallbackBinding {
                signature_id: "i64-listener".into(),
                body: identity("on_i64"),
            },
        ];
        let callbacks = resolve_guest_callbacks(&db, &package, &callback_world, &bindings).unwrap();
        let (module, imports) = crate::sonatina::compile_runtime_package_wasm_with_guest_callbacks(
            &db, &package, &callbacks,
        )
        .unwrap();
        let artifact = SonatinaWasmBackend::new()
            .with_import_modules(imports)
            .compile_module(&module)
            .unwrap();
        wasmparser::validate(&artifact.bytes).unwrap();

        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let mut linker = wasmtime::Linker::new(&engine);
        linker
            .func_wrap("fe:web", "event_get_code", |event_handle: i32| -> i32 {
                assert_eq!(event_handle, 17);
                35
            })
            .unwrap();
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let register = instance
            .get_typed_func::<(), i32>(&mut store, "fe_guest_callback_0_register")
            .unwrap();
        let invoke = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "fe_guest_callback_0_invoke")
            .unwrap();
        let release = instance
            .get_typed_func::<i32, ()>(&mut store, "fe_guest_callback_0_release")
            .unwrap();
        let raw = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "fe_guest_callback_0_invoke_raw")
            .unwrap();
        let slot0 = instance
            .get_typed_func::<(), i32>(&mut store, "fe_guest_callback_0_table_slot")
            .unwrap()
            .call(&mut store, ())
            .unwrap();
        let slot1 = instance
            .get_typed_func::<(), i32>(&mut store, "fe_guest_callback_1_table_slot")
            .unwrap()
            .call(&mut store, ())
            .unwrap();

        // The Rust host stands in for JS here: it retains only the opaque core
        // token and later re-enters the Wasm export. There is no JS loopback.
        let host_held_token = register.call(&mut store, ()).unwrap();
        assert_eq!(invoke.call(&mut store, (host_held_token, 17)).unwrap(), 35);
        assert_eq!(raw.call(&mut store, (slot0, 17)).unwrap(), 35);
        release.call(&mut store, host_held_token).unwrap();
        assert!(
            invoke.call(&mut store, (host_held_token, 17)).is_err(),
            "released token must be rejected"
        );

        let reused = register.call(&mut store, ()).unwrap();
        assert_ne!(reused, host_held_token);
        assert_eq!((reused as u32) & 0xffff, (host_held_token as u32) & 0xffff);
        assert!(
            invoke.call(&mut store, (host_held_token, 17)).is_err(),
            "old generation must stay stale after slot reuse"
        );
        assert_eq!(invoke.call(&mut store, (reused, 17)).unwrap(), 35);

        assert!(raw.call(&mut store, (0, 17)).is_err(), "null must trap");
        assert!(
            raw.call(&mut store, (i32::MAX, 17)).is_err(),
            "out-of-bounds table index must trap"
        );
        assert!(
            raw.call(&mut store, (slot1, 17)).is_err(),
            "table entry with an i64 signature must trap in the i32 trampoline"
        );
        release.call(&mut store, reused).unwrap();

        // Exercise the same artifact through the real generated Web IDL
        // semantic adapter and generic browser host runtime. Event is a
        // generated one-i32 resource wrapper; richer members such as strings
        // and async callbacks remain gated.
        if std::process::Command::new("bun")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            let plan = fe_webidl_bindgen::build_adapter_plan(
                &event_world,
                "borrowed-event-listener",
                "fe:web",
            )
            .unwrap();
            let adapter =
                fe_webidl_bindgen::emit_js_canonical_adapter(&event_world, &plan).unwrap();
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "fe-webidl-wasm-listener-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir(&directory).unwrap();
            let wasm_path = directory.join("listener.wasm");
            let adapter_path = directory.join("adapter.mjs");
            let test_path = directory.join("event-listener.mjs");
            std::fs::write(&wasm_path, &artifact.bytes).unwrap();
            std::fs::write(&adapter_path, adapter).unwrap();
            let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../demos/shared/host-runtime.js")
                .canonicalize()
                .unwrap();
            let script = format!(
                r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};

const bytes = await Bun.file({wasm_path:?}).arrayBuffer();
const runtime = createFeHostRuntime();

class ScalarEventTarget {{
  #listeners = new Map();
  addEventListener(type, listener) {{
    let listeners = this.#listeners.get(type);
    if (listeners === undefined) this.#listeners.set(type, listeners = new Set());
    listeners.add(listener);
  }}
  removeEventListener(type, listener) {{
    this.#listeners.get(type)?.delete(listener);
  }}
  dispatch(type, value) {{
    return Array.from(this.#listeners.get(type) ?? [], listener => listener(value));
  }}
  listenerCount(type) {{ return this.#listeners.get(type)?.size ?? 0; }}
}}

const target = new ScalarEventTarget();
const targetHandle = runtime.resources.insert(target);
const adapter = createFeHostAdapter({{ interfaces: {{}} }}, runtime);
const {{ instance }} = await WebAssembly.instantiate(bytes, adapter.imports);
const wasm = instance.exports;
const token = wasm.fe_guest_callback_0_register();
let borrowedEventHandle;
const callbackHandle = adapter.registerCallback(
  "EventListener",
  eventHandle => {{
    borrowedEventHandle = eventHandle;
    return wasm.fe_guest_callback_0_invoke(
      token,
      runtime.resources.toCore(eventHandle),
    );
  }},
);
const imports = adapter.imports["fe:web"];
imports.scalar_event_target_add_event_listener(
  targetHandle, "tick", callbackHandle,
);
if (target.listenerCount("tick") !== 1) throw new Error("listener was not installed");
const hostEvent = {{ code: 35 }};
const delivered = target.dispatch("tick", hostEvent);
if (delivered.length !== 1 || delivered[0] !== 35) {{
  throw new Error(`Fe listener returned ${{JSON.stringify(delivered)}}`);
}}
let borrowExpired = false;
try {{ runtime.resources.borrow(borrowedEventHandle); }}
catch (error) {{ borrowExpired = error.code === "stale_handle"; }}
if (!borrowExpired) throw new Error("borrowed Event resource escaped callback invocation");

let live = true;
const unsubscribe = () => {{
  if (!live) throw new Error("subscription ownership was already consumed");
  live = false;
  imports.scalar_event_target_remove_event_listener(
    targetHandle, "tick", callbackHandle,
  );
  adapter.releaseCallback(callbackHandle);
  wasm.fe_guest_callback_0_release(token);
}};
unsubscribe();
if (target.listenerCount("tick") !== 0) throw new Error("listener leaked after unsubscribe");
if (target.dispatch("tick", 99).length !== 0) throw new Error("delivery survived unsubscribe");

let staleRejected = false;
try {{ wasm.fe_guest_callback_0_invoke(token, 99); }}
catch (error) {{ staleRejected = error instanceof WebAssembly.RuntimeError; }}
if (!staleRejected) throw new Error("released Wasm callback token remained callable");
let doubleUnsubscribeRejected = false;
try {{ unsubscribe(); }}
catch (error) {{ doubleUnsubscribeRejected = /consumed/.test(String(error)); }}
if (!doubleUnsubscribeRejected) throw new Error("unsubscribe was not consuming");

runtime.resources.drop(targetHandle);
const inventory = runtime.inventory();
if (inventory.resources !== 0 || inventory.callbacks !== 0 || inventory.futures !== 0) {{
  throw new Error(`host handles leaked: ${{JSON.stringify(inventory)}}`);
}}
"#,
                adapter_url = format!("file://{}", adapter_path.display()),
                runtime_url = format!("file://{}", runtime_path.display()),
                wasm_path = wasm_path.display().to_string(),
            );
            std::fs::write(&test_path, script).unwrap();
            let output = std::process::Command::new("bun")
                .arg("run")
                .arg(&test_path)
                .output()
                .unwrap();
            let _ = std::fs::remove_dir_all(&directory);
            assert!(
                output.status.success(),
                "Bun generated Web IDL + Fe Wasm listener capstone failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
