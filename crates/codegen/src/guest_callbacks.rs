//! Compiler-visible gate for target-neutral guest callback registrations.
//!
//! This module intentionally stops before backend materialization. It validates
//! the manifest shared with host tooling, then requires each mechanism needed
//! for a truthful token-dispatch trampoline.

use std::collections::BTreeSet;

use compiler_db::DriverDataBase;
use fe_host_abi::{
    CoreType, GuestCallbackRegistration, GuestCallbackRegistrationManifest, GuestFunctionIdentity,
    World,
};
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
    InvalidManifest(String),
    MissingCapabilities {
        profile: &'static str,
        missing: BTreeSet<GuestCallbackMaterializerCapability>,
    },
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

/// Opaque core-i32 authority for one guest callback registration.
///
/// The low 16 bits encode `slot + 1` (zero is never a valid token) and the high
/// 16 bits encode the nonzero generation. The fields remain private so callers
/// cannot forge a typed token from slot arithmetic.
#[derive(Debug, PartialEq, Eq)]
pub struct GuestCallbackToken {
    core: u32,
}

impl GuestCallbackToken {
    pub fn to_core(&self) -> i32 {
        self.core as i32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GuestCallbackSlot {
    Vacant {
        generation: u16,
    },
    Occupied {
        generation: u16,
        callback: ResolvedGuestCallback,
    },
    Exhausted,
}

/// Target-neutral registration arena. It models token ownership and generation
/// checks only; it contains no backend function table and performs no dispatch.
#[derive(Debug, Default)]
pub struct GuestCallbackRegistrationTable {
    slots: Vec<GuestCallbackSlot>,
    free: BTreeSet<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestCallbackTableError {
    InvalidToken {
        core: i32,
    },
    Released {
        slot: u16,
        generation: u16,
    },
    Stale {
        slot: u16,
        expected_generation: u16,
        received_generation: u16,
    },
    SlotCapacityExhausted,
    GenerationExhausted {
        slot: u16,
    },
}

impl GuestCallbackRegistrationTable {
    pub fn register(
        &mut self,
        callback: ResolvedGuestCallback,
    ) -> Result<GuestCallbackToken, GuestCallbackTableError> {
        let (slot, generation) = if let Some(slot) = self.free.pop_first() {
            let entry = &mut self.slots[usize::from(slot)];
            let GuestCallbackSlot::Vacant { generation } = entry else {
                unreachable!("only vacant callback slots enter the free set")
            };
            let next = generation
                .checked_add(1)
                .ok_or(GuestCallbackTableError::GenerationExhausted { slot })?;
            (slot, next)
        } else {
            let slot = u16::try_from(self.slots.len())
                .map_err(|_| GuestCallbackTableError::SlotCapacityExhausted)?;
            self.slots.push(GuestCallbackSlot::Vacant { generation: 0 });
            (slot, 1)
        };
        self.slots[usize::from(slot)] = GuestCallbackSlot::Occupied {
            generation,
            callback,
        };
        Ok(GuestCallbackToken {
            core: encode_guest_callback_token(slot, generation),
        })
    }

    pub fn resolve(&self, core: i32) -> Result<&ResolvedGuestCallback, GuestCallbackTableError> {
        let (slot, received_generation) = decode_guest_callback_token(core)?;
        let Some(entry) = self.slots.get(usize::from(slot)) else {
            return Err(GuestCallbackTableError::InvalidToken { core });
        };
        match entry {
            GuestCallbackSlot::Occupied {
                generation,
                callback,
            } if *generation == received_generation => Ok(callback),
            GuestCallbackSlot::Occupied { generation, .. }
            | GuestCallbackSlot::Vacant { generation }
                if *generation != received_generation =>
            {
                Err(GuestCallbackTableError::Stale {
                    slot,
                    expected_generation: *generation,
                    received_generation,
                })
            }
            GuestCallbackSlot::Vacant { generation } => Err(GuestCallbackTableError::Released {
                slot,
                generation: *generation,
            }),
            GuestCallbackSlot::Exhausted => {
                Err(GuestCallbackTableError::GenerationExhausted { slot })
            }
            GuestCallbackSlot::Occupied { .. } => unreachable!("generation checked above"),
        }
    }

    /// Consume the sole rooted token and retire its binding.
    pub fn release(
        &mut self,
        token: GuestCallbackToken,
    ) -> Result<ResolvedGuestCallback, GuestCallbackTableError> {
        let core = token.to_core();
        let (slot, received_generation) = decode_guest_callback_token(core)?;
        let Some(entry) = self.slots.get_mut(usize::from(slot)) else {
            return Err(GuestCallbackTableError::InvalidToken { core });
        };
        let generation = match entry {
            GuestCallbackSlot::Occupied { generation, .. }
                if *generation == received_generation =>
            {
                *generation
            }
            GuestCallbackSlot::Occupied { generation, .. }
            | GuestCallbackSlot::Vacant { generation } => {
                return Err(GuestCallbackTableError::Stale {
                    slot,
                    expected_generation: *generation,
                    received_generation,
                });
            }
            GuestCallbackSlot::Exhausted => {
                return Err(GuestCallbackTableError::GenerationExhausted { slot });
            }
        };
        let GuestCallbackSlot::Occupied { callback, .. } =
            std::mem::replace(entry, GuestCallbackSlot::Vacant { generation })
        else {
            unreachable!("release admitted only an occupied slot")
        };
        if generation == u16::MAX {
            *entry = GuestCallbackSlot::Exhausted;
        } else {
            self.free.insert(slot);
        }
        Ok(callback)
    }

    pub fn live_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| matches!(slot, GuestCallbackSlot::Occupied { .. }))
            .count()
    }
}

fn encode_guest_callback_token(slot: u16, generation: u16) -> u32 {
    (u32::from(generation) << 16) | (u32::from(slot) + 1)
}

fn decode_guest_callback_token(core: i32) -> Result<(u16, u16), GuestCallbackTableError> {
    let raw = core as u32;
    let encoded_slot = (raw & 0xffff) as u16;
    let generation = (raw >> 16) as u16;
    if encoded_slot == 0 || generation == 0 {
        return Err(GuestCallbackTableError::InvalidToken { core });
    }
    Ok((encoded_slot - 1, generation))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestCallbackResolutionError {
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
/// explicit ambiguity until a manifest version can name type arguments.
pub fn resolve_guest_callbacks(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    manifest: &GuestCallbackRegistrationManifest,
) -> Result<Vec<ResolvedGuestCallback>, GuestCallbackResolutionError> {
    let functions = package.functions(db);
    let mut resolved = Vec::with_capacity(manifest.registrations.len());
    for registration in &manifest.registrations {
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
        if actual_params != registration.core_params || actual_results != registration.core_results
        {
            return Err(GuestCallbackResolutionError::SignatureMismatch {
                detail: Box::new(GuestCallbackSignatureMismatch {
                    body: registration.body.clone(),
                    expected_params: registration.core_params.clone(),
                    actual_params,
                    expected_results: registration.core_results.clone(),
                    actual_results,
                }),
            });
        }
        resolved.push(ResolvedGuestCallback {
            signature_id: registration.signature_id.clone(),
            body: registration.body.clone(),
            runtime_instance_key: mir::runtime_instance_stable_key(db, function.instance(db)),
            runtime_symbol: function.symbol(db).clone(),
            core_params: registration.core_params.clone(),
            core_results: registration.core_results.clone(),
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
    manifest: &GuestCallbackRegistrationManifest,
    profile: &GuestCallbackMaterializerProfile,
) -> Result<Vec<GuestCallbackRegistration>, GuestCallbackMaterializationError> {
    manifest
        .validate(world)
        .map_err(|error| GuestCallbackMaterializationError::InvalidManifest(error.to_string()))?;
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
    Ok(manifest.registrations.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::InputDb;
    use driver::DriverDataBase;
    use fe_host_abi::{
        CoreType, FunctionType, GUEST_CALLBACK_REGISTRATION_PROTOCOL,
        GUEST_CALLBACK_REGISTRATION_VERSION, GuestCallbackGenerationPolicy,
        GuestCallbackRegistration, GuestCallbackReleasePolicy, GuestCallbackTokenOwner,
        GuestFunctionIdentity, Param, Type, TypeDef, TypeDefKind,
    };
    use url::Url;

    fn fixture() -> (World, GuestCallbackRegistrationManifest) {
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
        let manifest = GuestCallbackRegistrationManifest {
            protocol: GUEST_CALLBACK_REGISTRATION_PROTOCOL.into(),
            version: GUEST_CALLBACK_REGISTRATION_VERSION,
            registrations: vec![GuestCallbackRegistration {
                signature_id: "listener".into(),
                body: GuestFunctionIdentity {
                    ingot: "app".into(),
                    module_path: vec!["handlers".into()],
                    function: "on_value".into(),
                },
                core_params: vec![CoreType::I32],
                core_results: vec![CoreType::I32],
                token_owner: GuestCallbackTokenOwner::GuestRegistry,
                generation: GuestCallbackGenerationPolicy::ValidateExactAndBumpOnReuse,
                release: GuestCallbackReleasePolicy::ConsumeRootAndRejectStale,
            }],
        };
        (world, manifest)
    }

    #[test]
    fn current_wasm_fails_before_emitting_a_fake_dispatcher() {
        let (world, manifest) = fixture();
        let error = prepare_guest_callback_materialization(
            &world,
            &manifest,
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
    fn resolves_public_authored_body_and_rejects_missing_and_signature_drift() {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///guest_callback_resolution.fe").unwrap();
        db.workspace().touch(
            &mut db,
            url.clone(),
            Some(
                "pub fn on_value(value: i32) -> i32 { value + 1 }\n\
                 fn private_value(value: i32) -> i32 { value }\n\
                 pub fn keep_private_reachable(value: i32) -> i32 { private_value(value) }\n"
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
        let (_world, mut manifest) = fixture();
        manifest.registrations[0].body = identity.clone();
        let resolved = resolve_guest_callbacks(&db, &package, &manifest).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].body, identity);
        assert!(!resolved[0].runtime_instance_key.is_empty());
        assert!(!resolved[0].runtime_symbol.is_empty());
        assert_eq!(resolved[0].core_params, [CoreType::I32]);
        assert_eq!(resolved[0].core_results, [CoreType::I32]);

        manifest.registrations[0].body.function = "missing".into();
        assert!(matches!(
            resolve_guest_callbacks(&db, &package, &manifest),
            Err(GuestCallbackResolutionError::Missing { .. })
        ));

        manifest.registrations[0].body = identity;
        manifest.registrations[0].core_params = vec![CoreType::I64];
        assert!(matches!(
            resolve_guest_callbacks(&db, &package, &manifest),
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

    fn resolved_fixture(name: &str) -> ResolvedGuestCallback {
        ResolvedGuestCallback {
            signature_id: "listener".into(),
            body: GuestFunctionIdentity {
                ingot: "app".into(),
                module_path: vec!["lib".into()],
                function: name.into(),
            },
            runtime_instance_key: format!("app$lib$fn${name}"),
            runtime_symbol: name.into(),
            core_params: vec![CoreType::I32],
            core_results: vec![CoreType::I32],
        }
    }

    #[test]
    fn registration_table_rejects_released_and_reused_generations() {
        let mut table = GuestCallbackRegistrationTable::default();
        let first = table.register(resolved_fixture("first")).unwrap();
        let stale_core = first.to_core();
        assert_eq!(table.resolve(stale_core).unwrap().runtime_symbol, "first");
        assert_eq!(table.live_count(), 1);

        let released = table.release(first).unwrap();
        assert_eq!(released.runtime_symbol, "first");
        assert_eq!(table.live_count(), 0);
        assert!(matches!(
            table.resolve(stale_core),
            Err(GuestCallbackTableError::Released {
                slot: 0,
                generation: 1
            })
        ));

        let second = table.register(resolved_fixture("second")).unwrap();
        let second_core = second.to_core();
        assert_ne!(stale_core, second_core);
        assert_eq!(table.resolve(second_core).unwrap().runtime_symbol, "second");
        assert!(matches!(
            table.resolve(stale_core),
            Err(GuestCallbackTableError::Stale {
                slot: 0,
                expected_generation: 2,
                received_generation: 1,
            })
        ));
    }

    #[test]
    fn registration_table_reuses_lowest_free_slot_deterministically() {
        let mut table = GuestCallbackRegistrationTable::default();
        let zero = table.register(resolved_fixture("zero")).unwrap();
        let one = table.register(resolved_fixture("one")).unwrap();
        let zero_core = zero.to_core();
        let one_core = one.to_core();
        table.release(one).unwrap();
        table.release(zero).unwrap();

        let reused_zero = table.register(resolved_fixture("reused-zero")).unwrap();
        let reused_one = table.register(resolved_fixture("reused-one")).unwrap();
        assert_eq!((reused_zero.to_core() as u32) & 0xffff, 1);
        assert_eq!((reused_one.to_core() as u32) & 0xffff, 2);
        assert_ne!(reused_zero.to_core(), zero_core);
        assert_ne!(reused_one.to_core(), one_core);
    }

    #[test]
    fn zero_tokens_are_invalid_and_exhausted_generations_retire_slots() {
        let table = GuestCallbackRegistrationTable::default();
        assert_eq!(
            table.resolve(0).unwrap_err(),
            GuestCallbackTableError::InvalidToken { core: 0 }
        );

        let mut table = GuestCallbackRegistrationTable {
            slots: vec![GuestCallbackSlot::Occupied {
                generation: u16::MAX,
                callback: resolved_fixture("last-generation"),
            }],
            free: BTreeSet::new(),
        };
        let token = GuestCallbackToken {
            core: encode_guest_callback_token(0, u16::MAX),
        };
        let stale_core = token.to_core();
        table.release(token).unwrap();
        assert!(matches!(
            table.resolve(stale_core),
            Err(GuestCallbackTableError::GenerationExhausted { slot: 0 })
        ));
        let next = table.register(resolved_fixture("next-slot")).unwrap();
        assert_eq!((next.to_core() as u32) & 0xffff, 2);
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
        let manifest = GuestCallbackRegistrationManifest {
            protocol: GUEST_CALLBACK_REGISTRATION_PROTOCOL.into(),
            version: GUEST_CALLBACK_REGISTRATION_VERSION,
            registrations: vec![
                GuestCallbackRegistration {
                    signature_id: "EventListener".into(),
                    body: identity("on_event"),
                    core_params: vec![CoreType::I32],
                    core_results: vec![CoreType::I32],
                    token_owner: GuestCallbackTokenOwner::GuestRegistry,
                    generation: GuestCallbackGenerationPolicy::ValidateExactAndBumpOnReuse,
                    release: GuestCallbackReleasePolicy::ConsumeRootAndRejectStale,
                },
                GuestCallbackRegistration {
                    signature_id: "i64-listener".into(),
                    body: identity("on_i64"),
                    core_params: vec![CoreType::I64],
                    core_results: vec![CoreType::I64],
                    token_owner: GuestCallbackTokenOwner::GuestRegistry,
                    generation: GuestCallbackGenerationPolicy::ValidateExactAndBumpOnReuse,
                    release: GuestCallbackReleasePolicy::ConsumeRootAndRejectStale,
                },
            ],
        };
        let callbacks = resolve_guest_callbacks(&db, &package, &manifest).unwrap();
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
    return wasm.fe_guest_callback_0_invoke(token, eventHandle);
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
