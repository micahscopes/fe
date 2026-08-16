//! Core-Wasm transport blueprint derived from the generic host ABI.
//!
//! Layout decisions come from `fe-host-wasm-codec`. This module names transport
//! entry points and codec operations; it does not duplicate memory layout or
//! lift/lower algorithms.

use std::collections::BTreeSet;

use fe_host_abi::{
    Function, FunctionType, Handle, HandleOwnership, Param, Receiver, Type, TypeDefKind,
};
use fe_host_wasm_codec::{
    BoundaryDirection, CoreType as CodecCoreType, Flattening, FunctionPlan as CodecFunctionPlan,
    JS_CODEC_CONTRACT, PlanRequirement,
};

use crate::{AdapterPlan, BindgenError};

pub const HOST_WASM_CODEC_CONTRACT: &str = JS_CODEC_CONTRACT;
pub const GENERATED_COMPLETION_CONTRACT: &str = "fe:generated-completion/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportPlan {
    pub codec_contract: &'static str,
    pub module: String,
    pub memory: MemorySurfacePlan,
    pub functions: Vec<TransportFunction>,
    pub callbacks: Vec<CallbackTransport>,
    pub futures: Vec<FutureTransport>,
    pub required_codec_features: BTreeSet<PlanRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySurfacePlan {
    pub memory_export: String,
    pub alloc_export: String,
    pub realloc_export: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportFunction {
    pub identity: String,
    pub module: String,
    pub import_name: String,
    pub kind: TransportKind,
    pub core: Option<CoreSignature>,
    pub requirements: BTreeSet<PlanRequirement>,
    pub post_return_export: Option<String>,
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    ResourceMethod,
    CallbackExport,
    FutureCompletionExport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackTransport {
    pub signature_id: String,
    pub export_name: String,
    /// Generic, non-Web callback ABI. The transport remains blocked until a
    /// compiler or runtime adapter materializes this exact export.
    pub export_plan: fe_host_abi::CallbackExportPlan,
    pub core: Option<CoreSignature>,
    pub requirements: BTreeSet<PlanRequirement>,
    pub post_return_export: Option<String>,
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FutureTransport {
    pub operation_identity: String,
    pub success: Vec<CoreValueType>,
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSignature {
    pub params: Vec<CoreValueType>,
    pub results: Vec<CoreValueType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreValueType {
    I32,
    I64,
    F32,
    F64,
}

pub fn build_transport_plan(plan: &AdapterPlan) -> Result<TransportPlan, BindgenError> {
    let world = &plan.host_abi;
    let mut functions = Vec::new();
    let mut callbacks = Vec::new();
    let mut futures = Vec::new();
    let mut required_codec_features = BTreeSet::new();

    for resource in &plan.resources {
        let abi_resource = world
            .resources
            .iter()
            .find(|candidate| candidate.name == resource.name)
            .ok_or_else(|| {
                BindgenError::new(
                    format!("transport resource `{}`", resource.name),
                    "missing normalized host ABI resource",
                )
            })?;
        for function in &resource.functions {
            let method = abi_resource
                .methods
                .iter()
                .find(|candidate| candidate.name == function.abi_method_name)
                .ok_or_else(|| {
                    BindgenError::new(
                        format!(
                            "transport method `{}::{}`",
                            resource.name, function.abi_method_name
                        ),
                        "missing normalized host ABI method",
                    )
                })?;
            let identity = format!("resource/{}/{}", resource.name, function.import_name);
            let is_async = method.signature.async_;
            let mut signature = method.signature.clone();
            if method.receiver != Receiver::Static {
                signature.params.insert(
                    0,
                    Param {
                        name: "self".to_owned(),
                        type_: Type::Handle(Handle {
                            resource: resource.name.clone(),
                            ownership: HandleOwnership::Borrow,
                        }),
                    },
                );
            }
            let mut transport_signature = signature;
            transport_signature.async_ = false;
            let codec_plan = codec_function_plan(
                world,
                &resource.name,
                &function.abi_method_name,
                transport_signature,
                BoundaryDirection::GuestToHost,
            )?;
            let requirements = codec_plan.requirements.clone();
            let mut core = codec_core_signature(&codec_plan);
            let blocker = if is_async
                && (requirements.contains(&PlanRequirement::PostReturn)
                    || codec_plan
                        .result
                        .as_ref()
                        .is_some_and(|result| matches!(result.layout.flat, Flattening::Indirect)))
            {
                Some(
                    "generated Promise transport requires continuation-scoped post-return for rich results"
                        .to_owned(),
                )
            } else {
                None
            };
            let post_return_export = requirements
                .contains(&PlanRequirement::PostReturn)
                .then(|| post_return_name(&identity));
            required_codec_features.extend(requirements.iter().copied());
            if is_async {
                let success = core.results.clone();
                core.results = vec![CoreValueType::I32];
                futures.push(FutureTransport {
                    operation_identity: identity.clone(),
                    success,
                    blocker: blocker.clone(),
                });
            }
            functions.push(TransportFunction {
                identity,
                module: plan.module.clone(),
                import_name: function.import_name.clone(),
                kind: TransportKind::ResourceMethod,
                core: Some(core),
                requirements,
                post_return_export,
                blocker,
            });
        }
    }

    for callback in &plan.callbacks {
        let definition = world
            .types
            .iter()
            .find(|definition| definition.name == callback.name)
            .ok_or_else(|| {
                BindgenError::new(
                    format!("transport callback `{}`", callback.name),
                    "missing host ABI callback type",
                )
            })?;
        let TypeDefKind::Callback { signature } = &definition.kind else {
            return Err(BindgenError::new(
                format!("transport callback `{}`", callback.name),
                "named type is not a callback",
            ));
        };
        let identity = format!("callback/{}", callback.name);
        let export_plan = world
            .callback_export_plan(&callback.name)
            .map_err(|error| {
                BindgenError::new(
                    format!("transport callback `{}`", callback.name),
                    error.to_string(),
                )
            })?;
        let blocker = export_plan.blocker.clone().unwrap_or_else(|| {
            "scalar callback export requires guest callback registration: an opaque token must \
             resolve to a lowered Fe callback body. Runtime packages currently expose only \
             statically named functions, and Wasm lowering has no guest callback table, \
             token-to-RuntimeInstance mapping, or indirect-call registration seam"
                .to_owned()
        });
        let mut trampoline = signature.clone();
        trampoline.params.insert(
            0,
            Param {
                name: "callback".to_owned(),
                type_: Type::Named(callback.name.clone()),
            },
        );
        let codec_plan = if signature.async_ {
            None
        } else {
            Some(codec_function_plan(
                world,
                "callback",
                &callback.name,
                trampoline,
                BoundaryDirection::HostToGuest,
            )?)
        };
        let requirements = codec_plan
            .as_ref()
            .map(|plan| plan.requirements.clone())
            .unwrap_or_else(|| {
                BTreeSet::from([PlanRequirement::CallbackTable, PlanRequirement::FutureTable])
            });
        let core = codec_plan.as_ref().map(codec_core_signature);
        let post_return_export = requirements
            .contains(&PlanRequirement::PostReturn)
            .then(|| post_return_name(&identity));
        required_codec_features.extend(requirements.iter().copied());
        callbacks.push(CallbackTransport {
            signature_id: callback.name.clone(),
            export_name: stable_entry("__fe_callback", &identity),
            export_plan,
            core,
            requirements,
            post_return_export,
            blocker: Some(blocker),
        });
    }

    functions.sort_by(|left, right| left.identity.cmp(&right.identity));
    callbacks.sort_by(|left, right| left.signature_id.cmp(&right.signature_id));
    futures.sort_by(|left, right| left.operation_identity.cmp(&right.operation_identity));
    Ok(TransportPlan {
        codec_contract: HOST_WASM_CODEC_CONTRACT,
        module: plan.module.clone(),
        memory: MemorySurfacePlan {
            memory_export: "memory".to_owned(),
            alloc_export: "cabi_alloc".to_owned(),
            realloc_export: "cabi_realloc".to_owned(),
        },
        functions,
        callbacks,
        futures,
        required_codec_features,
    })
}

fn codec_function_plan(
    world: &fe_host_abi::World,
    namespace: &str,
    name: &str,
    signature: FunctionType,
    direction: BoundaryDirection,
) -> Result<CodecFunctionPlan, BindgenError> {
    let function = Function {
        namespace: namespace.to_owned(),
        name: name.to_owned(),
        signature,
    };
    fe_host_wasm_codec::function_plan(world, &function, direction).map_err(|error| {
        BindgenError::new(
            format!("codec plan `{namespace}::{name}`"),
            error.to_string(),
        )
    })
}

fn codec_core_signature(plan: &CodecFunctionPlan) -> CoreSignature {
    let params = plan
        .params
        .iter()
        .flat_map(|param| layout_types(&param.layout.flat))
        .collect();
    let results = plan
        .result
        .as_ref()
        .into_iter()
        .flat_map(|result| layout_types(&result.layout.flat))
        .collect();
    CoreSignature { params, results }
}

fn layout_types(flattening: &Flattening) -> Vec<CoreValueType> {
    match flattening {
        Flattening::Direct(types) => types.iter().copied().map(codec_core_type).collect(),
        Flattening::Indirect => vec![CoreValueType::I32],
    }
}

fn codec_core_type(type_: CodecCoreType) -> CoreValueType {
    match type_ {
        CodecCoreType::I32 => CoreValueType::I32,
        CodecCoreType::I64 => CoreValueType::I64,
        CodecCoreType::F32 => CoreValueType::F32,
        CodecCoreType::F64 => CoreValueType::F64,
    }
}

fn stable_entry(prefix: &str, identity: &str) -> String {
    format!("{prefix}_{:016x}", stable_hash(identity))
}

fn post_return_name(identity: &str) -> String {
    stable_entry("cabi_post", identity)
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Emit a two-phase Instance binder against a separately supplied codec.
///
/// No codec ships in this crate. The binder fails before exposing imports when
/// the codec lacks a requirement. Direct scalar Promise results use the shared
/// generated-completion rail; rich Promise results and callback trampolines
/// remain blocked until their continuation-scoped ownership mechanics exist.
pub fn emit_js_core_wasm_transport(plan: &TransportPlan) -> String {
    let has_futures = !plan.futures.is_empty();
    let needs_realloc = plan
        .required_codec_features
        .contains(&PlanRequirement::Realloc);
    let mut output = format!(
        "// @generated transport blueprint; layout is delegated to the codec.\n\
         export const FE_HOST_WASM_CODEC_CONTRACT = {:?};\n\
         {}export function createFeCoreWasmTransport(codec, semanticAdapter{}) {{\n\
         \x20 if (codec.protocol !== FE_HOST_WASM_CODEC_CONTRACT) throw new TypeError(`expected ${{FE_HOST_WASM_CODEC_CONTRACT}} codec`);\n\
         {}\
         \x20 const required = [",
        HOST_WASM_CODEC_CONTRACT,
        if has_futures {
            format!(
                "export const FE_GENERATED_COMPLETION_CONTRACT = {:?};\n\n",
                GENERATED_COMPLETION_CONTRACT
            )
        } else {
            String::new()
        },
        if has_futures { ", completions" } else { "" },
        if has_futures {
            "  if (completions?.protocol !== FE_GENERATED_COMPLETION_CONTRACT || typeof completions.begin !== \"function\") throw new TypeError(`expected ${FE_GENERATED_COMPLETION_CONTRACT} completion rail`);\n"
        } else {
            ""
        },
    );
    for requirement in &plan.required_codec_features {
        output.push_str(&format!("{:?},", requirement_name(*requirement)));
    }
    output.push_str(
        "];\n\
         \x20 const unsupported = required.filter(feature => !codec.supports(feature));\n\
         \x20 if (unsupported.length) throw new TypeError(`codec lacks ${unsupported.join(\", \")}`);\n\
         \x20 const mechanicsBlockers = [",
    );
    for function in &plan.functions {
        if let Some(blocker) = &function.blocker {
            output.push_str(&format!("{blocker:?},"));
        }
    }
    for callback in &plan.callbacks {
        if let Some(blocker) = &callback.blocker {
            output.push_str(&format!("{blocker:?},"));
        }
    }
    output.push_str(
        "];\n\
         \x20 if (mechanicsBlockers.length) throw new TypeError(`transport blueprint is not executable: ${mechanicsBlockers.join(\"; \")}`);\n\
         \x20 const session = codec.createSession();\n\
         \x20 const imports = {\n",
    );
    for function in &plan.functions {
        if let Some(blocker) = &function.blocker {
            output.push_str(&format!(
                "    {:?}: () => {{ throw new TypeError({blocker:?}); }},\n",
                function.import_name
            ));
            continue;
        }
        if let Some(future) = plan
            .futures
            .iter()
            .find(|future| future.operation_identity == function.identity)
        {
            output.push_str(&format!(
                "    {:?}: (...coreArgs) => {{\n\
                 \x20     const args = session.liftArguments({:?}, coreArgs);\n\
                 \x20     return completions.begin(\n\
                 \x20       {:?},\n\
                 \x20       _signal => semanticAdapter.imports[{:?}][{:?}](...args),\n\
                 \x20       {},\n\
                 \x20       value => session.lowerResult({:?}, value),\n\
                 \x20     );\n\
                 \x20   }},\n",
                function.import_name,
                function.identity,
                function.identity,
                function.module,
                function.import_name,
                future.success.len(),
                function.identity,
            ));
        } else {
            output.push_str(&format!(
                "    {:?}: (...coreArgs) => {{\n\
                 \x20     const args = session.liftArguments({:?}, coreArgs);\n\
                 \x20     const result = semanticAdapter.imports[{:?}][{:?}](...args);\n\
                 \x20     return session.lowerResult({:?}, result);\n\
                 \x20   }},\n",
                function.import_name,
                function.identity,
                function.module,
                function.import_name,
                function.identity,
            ));
        }
    }
    output.push_str("  };\n");
    output.push_str("  const postReturnNames = {");
    for function in &plan.functions {
        if let Some(name) = &function.post_return_export {
            output.push_str(&format!("{:?}: {:?},", function.identity, name));
        }
    }
    for callback in &plan.callbacks {
        if let Some(name) = &callback.post_return_export {
            output.push_str(&format!(
                "{:?}: {:?},",
                format!("callback/{}", callback.signature_id),
                name
            ));
        }
    }
    output.push_str("};\n");
    output.push_str(&format!(
        "  const attach = instance => {{\n\
         \x20   const exports = instance.exports;\n\
         \x20   const memory = exports[{:?}];\n\
         \x20   const alloc = exports[{:?}];\n\
         \x20   const realloc = exports[{:?}];\n\
         \x20   if (!(memory instanceof WebAssembly.Memory)) throw new TypeError(\"missing canonical memory export\");\n\
         {}\
         \x20   const postReturns = Object.fromEntries(Object.entries(postReturnNames).map(([identity, name]) => {{\n\
         \x20     const cleanup = exports[name];\n\
         \x20     if (typeof cleanup !== \"function\") throw new TypeError(`missing post-return export ${{name}} for ${{identity}}`);\n\
         \x20     return [identity, cleanup];\n\
         \x20   }}));\n\
         \x20   session.attach({{ instance, memory, alloc, realloc, postReturns }});\n\
         \x20   return instance;\n\
         \x20 }};\n",
        plan.memory.memory_export,
        plan.memory.alloc_export,
        plan.memory.realloc_export,
        if needs_realloc {
            "    if (typeof alloc !== \"function\" || typeof realloc !== \"function\") throw new TypeError(\"missing canonical allocator exports\");\n"
        } else {
            ""
        },
    ));
    output.push_str(&format!(
        "  return {{ imports: {{ {:?}: imports }}, attach, session }};\n}}\n",
        plan.module
    ));
    output
}

fn requirement_name(requirement: PlanRequirement) -> &'static str {
    match requirement {
        PlanRequirement::Realloc => "realloc",
        PlanRequirement::PostReturn => "post_return",
        PlanRequirement::ResourceTransfer => "resource_transfer",
        PlanRequirement::BorrowScope => "borrow_scope",
        PlanRequirement::CallbackTable => "callback_table",
        PlanRequirement::FutureTable => "future_table",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_adapter_plan, parse};

    const FIXTURE: &str = r#"
        interface Event {};
        callback EventMapper = DOMString (Event event);
        interface Channel {
            DOMString echo(DOMString value);
            Promise<DOMString> receive();
        };
    "#;

    #[test]
    fn transport_plan_is_stable_and_pairs_semantics_with_core_signatures() {
        let world = parse(FIXTURE).unwrap();
        let adapter = build_adapter_plan(&world, "transport-test", "fe:host").unwrap();
        let first = build_transport_plan(&adapter).unwrap();
        let second = build_transport_plan(&adapter).unwrap();
        assert_eq!(first, second);
        let echo = first
            .functions
            .iter()
            .find(|function| function.import_name == "channel_echo")
            .unwrap();
        assert_eq!(
            echo.core,
            Some(CoreSignature {
                // Receiver handle, then canonical string pointer/length.
                params: vec![CoreValueType::I32, CoreValueType::I32, CoreValueType::I32,],
                results: vec![CoreValueType::I32, CoreValueType::I32],
            })
        );
        assert!(echo.post_return_export.is_some());
        let receive = first
            .functions
            .iter()
            .find(|function| function.import_name == "channel_receive")
            .unwrap();
        assert_eq!(
            receive.core,
            Some(CoreSignature {
                params: vec![CoreValueType::I32],
                results: vec![CoreValueType::I32],
            })
        );
        assert!(receive.blocker.as_deref().unwrap().contains("post-return"));
        assert_eq!(first.futures.len(), 1);
        assert_eq!(
            first.futures[0].success,
            [CoreValueType::I32, CoreValueType::I32]
        );
        assert!(first.futures[0].blocker.is_some());
        assert_eq!(first.callbacks.len(), 1);
        assert!(first.callbacks[0].export_name.starts_with("__fe_callback_"));
        assert_eq!(
            first.callbacks[0].core.as_ref().unwrap().params[0],
            CoreValueType::I32
        );
    }

    #[test]
    fn event_listener_callback_uses_generic_scalar_plan_but_fails_closed_without_export() {
        let world = parse(
            r#"
                callback EventListener = undefined (long event);
                interface EventTarget {
                    undefined addEventListener(EventListener listener);
                };
            "#,
        )
        .unwrap();
        let adapter = build_adapter_plan(&world, "event-test", "fe:host").unwrap();
        let plan = build_transport_plan(&adapter).unwrap();
        let callback = plan
            .callbacks
            .iter()
            .find(|callback| callback.signature_id == "EventListener")
            .unwrap();
        assert_eq!(
            callback.export_plan.params,
            [fe_host_abi::CoreType::I32, fe_host_abi::CoreType::I32]
        );
        assert!(callback.export_plan.results.is_empty());
        assert!(callback.export_plan.blocker.is_none());
        assert!(
            callback
                .blocker
                .as_deref()
                .unwrap()
                .contains("token-to-RuntimeInstance mapping")
        );
        assert!(
            callback
                .blocker
                .as_deref()
                .unwrap()
                .contains("indirect-call registration seam")
        );
    }

    #[test]
    fn javascript_binder_delegates_layout_and_fails_closed_for_rich_async_results() {
        let world = parse(FIXTURE).unwrap();
        let adapter = build_adapter_plan(&world, "transport-test", "fe:host").unwrap();
        let plan = build_transport_plan(&adapter).unwrap();
        let js = emit_js_core_wasm_transport(&plan);
        assert!(js.contains(JS_CODEC_CONTRACT));
        assert!(js.contains("codec.supports"));
        assert!(js.contains("session.liftArguments"));
        assert!(js.contains("session.lowerResult"));
        assert!(js.contains("missing canonical memory export"));
        assert!(js.contains("missing canonical allocator exports"));
        assert!(js.contains("postReturns"));
        assert!(js.contains("continuation-scoped post-return"));
        assert!(js.contains("transport blueprint is not executable"));
        // Memory is a mandatory transport surface checked by `attach`, not an
        // optional codec capability advertised through `supports`.
        assert!(!js.contains("\"canonical_memory\""));
        assert!(js.contains("const memory = exports[\"memory\"]"));
        assert!(!js.contains("core::browser"));
    }

    #[test]
    fn scalar_promises_use_generated_pending_and_the_completion_rail() {
        let world = parse("interface Channel { Promise<unsigned long> receive(); };").unwrap();
        let adapter = build_adapter_plan(&world, "scalar-promise", "fe:host").unwrap();
        let plan = build_transport_plan(&adapter).unwrap();
        let receive = plan
            .functions
            .iter()
            .find(|function| function.import_name == "channel_receive")
            .unwrap();
        assert!(receive.blocker.is_none());
        assert_eq!(
            receive.core,
            Some(CoreSignature {
                params: vec![CoreValueType::I32],
                results: vec![CoreValueType::I32],
            })
        );
        assert_eq!(plan.futures[0].success, [CoreValueType::I32]);
        assert!(plan.futures[0].blocker.is_none());
        assert!(
            !plan
                .required_codec_features
                .contains(&PlanRequirement::FutureTable)
        );

        let fe = crate::emit_fe_flat_host_imports(&world, "fe:host").unwrap();
        assert!(fe.contains("use core::pending::Pending"), "{fe}");
        assert!(fe.contains("use std::wasm::WasmBackend"), "{fe}");
        assert!(fe.contains("channel_receive(self_: Channel) -> Pending<WasmBackend, u32>"));

        let js = emit_js_core_wasm_transport(&plan);
        assert!(js.contains(GENERATED_COMPLETION_CONTRACT), "{js}");
        assert!(js.contains("return completions.begin("), "{js}");
        assert!(js.contains("value => session.lowerResult("), "{js}");
        assert!(!js.contains("transport blueprint is not executable: generated"));
        assert!(!js.contains("missing canonical allocator exports"));
    }

    #[test]
    fn generated_scalar_promise_transport_executes_its_semantic_completion() {
        if !std::process::Command::new("bun")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let world = parse("interface Channel { Promise<unsigned long> receive(); };").unwrap();
        let adapter = build_adapter_plan(&world, "scalar-promise", "fe:host").unwrap();
        let plan = build_transport_plan(&adapter).unwrap();
        let transport = emit_js_core_wasm_transport(&plan);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("fe-webidl-promise-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let transport_path = directory.join("transport.mjs");
        let script_path = directory.join("execute.mjs");
        std::fs::write(&transport_path, transport).unwrap();
        let script = format!(
            r#"
import {{ createFeCoreWasmTransport }} from {transport_url:?};

let settled;
const completions = {{
  protocol: "fe:generated-completion/v1",
  begin(identity, invoke, width, lower) {{
    if (identity !== "resource/Channel/channel_receive" || width !== 1) {{
      throw new Error(`wrong generated completion ${{identity}}/${{width}}`);
    }}
    settled = Promise.resolve().then(() => invoke(new AbortController().signal)).then(lower);
    return 73;
  }},
}};
const codec = {{
  protocol: "fe:host-wasm-codec/v1",
  supports: () => true,
  createSession() {{
    return {{
      attach() {{}},
      liftArguments(identity, lanes) {{
        if (identity !== "resource/Channel/channel_receive") throw new Error("wrong identity");
        return lanes;
      }},
      lowerResult(_identity, value) {{ return value; }},
    }};
  }},
}};
const semanticAdapter = {{ imports: {{ "fe:host": {{
  channel_receive: handle => Promise.resolve(handle + 35),
}} }} }};
const transport = createFeCoreWasmTransport(codec, semanticAdapter, completions);
transport.attach({{ exports: {{
  memory: new WebAssembly.Memory({{ initial: 1 }}),
  cabi_alloc() {{ return 0; }},
  cabi_realloc() {{ return 0; }},
}} }});
const token = transport.imports["fe:host"].channel_receive(7);
if (token !== 73) throw new Error(`wrong pending token ${{token}}`);
const value = await settled;
if (value !== 42) throw new Error(`wrong scalar completion ${{value}}`);
"#,
            transport_url = format!("file://{}", transport_path.display()),
        );
        std::fs::write(&script_path, script).unwrap();
        let execution = std::process::Command::new("bun")
            .arg("run")
            .arg(&script_path)
            .output()
            .unwrap();
        let cleanup = std::fs::remove_dir_all(&directory);
        assert!(
            execution.status.success(),
            "generated scalar Promise transport failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&execution.stdout),
            String::from_utf8_lossy(&execution.stderr),
        );
        cleanup.unwrap();
    }

    #[test]
    fn handles_are_i32_tokens_in_host_codec_and_transport_plans() {
        let world = parse(FIXTURE).unwrap();
        let adapter = build_adapter_plan(&world, "transport-test", "fe:host").unwrap();
        let handle = Type::Handle(Handle {
            resource: "Channel".to_owned(),
            ownership: HandleOwnership::Borrow,
        });
        let signature = FunctionType {
            params: vec![Param {
                name: "self".to_owned(),
                type_: handle.clone(),
            }],
            result: None,
            async_: false,
        };

        let host = adapter
            .host_abi
            .signature_lowering_plan(
                "Channel",
                "wire-handle",
                &signature,
                fe_host_abi::LoweringProfile::CanonicalV1Blueprint,
            )
            .unwrap();
        assert_eq!(
            host.params[0].mode,
            fe_host_abi::PassMode::Direct(vec![fe_host_abi::CoreType::I32])
        );

        let codec = fe_host_wasm_codec::layout(&adapter.host_abi, &handle).unwrap();
        assert_eq!((codec.size, codec.align), (4, 4));
        assert_eq!(codec.flat, Flattening::Direct(vec![CodecCoreType::I32]));

        let transport = build_transport_plan(&adapter).unwrap();
        let echo = transport
            .functions
            .iter()
            .find(|function| function.import_name == "channel_echo")
            .unwrap();
        assert_eq!(echo.core.as_ref().unwrap().params[0], CoreValueType::I32);
        assert_eq!(
            HOST_WASM_CODEC_CONTRACT,
            fe_host_wasm_codec::JS_CODEC_CONTRACT
        );
    }

    #[test]
    fn checked_in_sync_binder_is_exactly_the_rust_emission() {
        let plan = TransportPlan {
            codec_contract: HOST_WASM_CODEC_CONTRACT,
            module: "fe:fixture".to_owned(),
            memory: MemorySurfacePlan {
                memory_export: "memory".to_owned(),
                alloc_export: "cabi_alloc".to_owned(),
                realloc_export: "cabi_realloc".to_owned(),
            },
            functions: vec![TransportFunction {
                identity: "fixture/send".to_owned(),
                module: "fe:fixture".to_owned(),
                import_name: "send".to_owned(),
                kind: TransportKind::ResourceMethod,
                core: Some(CoreSignature {
                    params: vec![CoreValueType::I32; 5],
                    results: vec![CoreValueType::I32],
                }),
                requirements: BTreeSet::from([
                    PlanRequirement::Realloc,
                    PlanRequirement::PostReturn,
                    PlanRequirement::ResourceTransfer,
                ]),
                post_return_export: Some("cabi_post_fixture_send".to_owned()),
                blocker: None,
            }],
            callbacks: vec![],
            futures: vec![],
            required_codec_features: BTreeSet::from([
                PlanRequirement::Realloc,
                PlanRequirement::PostReturn,
                PlanRequirement::ResourceTransfer,
            ]),
        };
        assert_eq!(HOST_WASM_CODEC_CONTRACT, JS_CODEC_CONTRACT);
        assert_eq!(
            emit_js_core_wasm_transport(&plan),
            include_str!("../../../demos/shared/host-wasm-transport-v1.fixture.js")
        );
    }
}
