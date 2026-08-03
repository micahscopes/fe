//! Deterministic semantic adapter planning and JavaScript host emission.
//!
//! The emitted adapter is executable when values have already crossed a
//! semantic host-ABI boundary. Core-Wasm canonical memory marshalling is a
//! separate transport implementation and is not claimed here.

use std::collections::BTreeSet;

use fe_host_abi as abi;

use crate::host_abi::{stable_union_case_name, stable_union_name};
use crate::selection::{AdapterSelectionManifest, ImportKind};
use crate::{
    ArgumentDef, BindgenError, CallbackDef, CollectionKind, ConstructorDef, DefaultValueDef,
    ExtendedAttributesDef, HostAbiLowering, HostAbiOptions, InterfaceDef, IteratorItemBinding,
    Member, NamespaceDef, NamespaceMember, OperationDef, TypeRef, World, constructor_import_name,
    lower_host_abi_with_metadata,
};

pub const HOST_RUNTIME_CONTRACT: &str = "fe:host-runtime/v1";

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterPlan {
    pub contract: &'static str,
    pub module: String,
    pub host_abi: abi::World,
    pub resources: Vec<AdapterResource>,
    pub namespaces: Vec<AdapterNamespace>,
    pub iterators: Vec<AdapterIterator>,
    pub async_iterators: Vec<AdapterAsyncIterator>,
    pub collections: Vec<AdapterCollection>,
    pub callbacks: Vec<AdapterCallback>,
    pub lowering: HostAbiLowering,
    pub runtime_operations: BTreeSet<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterAsyncIterator {
    pub interface: String,
    pub resource: String,
    pub item: IteratorItemBinding,
    pub create_import: String,
    pub next_import: String,
    pub cancel_import: String,
    pub drop_import: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterCollection {
    pub interface: String,
    pub kind: AdapterCollectionKind,
    pub size_import: String,
    pub has_import: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdapterCollectionKind {
    ReadonlyMaplike {
        key: TypeRef,
        value: TypeRef,
        get_import: String,
    },
    ReadonlySetlike {
        value: TypeRef,
    },
    MutableMaplike {
        key: TypeRef,
        value: TypeRef,
        get_import: String,
        set_import: String,
        delete_import: String,
        clear_import: String,
    },
    MutableSetlike {
        value: TypeRef,
        add_import: String,
        delete_import: String,
        clear_import: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterIterator {
    pub interface: String,
    pub resource: String,
    pub item: IteratorItemBinding,
    pub create_import: String,
    pub next_import: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterNamespace {
    pub name: String,
    pub attributes: ExtendedAttributesDef,
    pub functions: Vec<AdapterFunction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterResource {
    pub name: String,
    pub attributes: ExtendedAttributesDef,
    pub functions: Vec<AdapterFunction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterCallback {
    pub name: String,
    pub interface_operation: Option<String>,
    pub params: Vec<AdapterParam>,
    pub result: TypeRef,
    pub async_: bool,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterFunction {
    pub import_name: String,
    pub abi_method_name: String,
    pub member_name: String,
    pub invocation: AdapterInvocation,
    pub static_: bool,
    pub params: Vec<AdapterParam>,
    pub result: TypeRef,
    pub async_: bool,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterInvocation {
    Constructor,
    AttributeGet,
    AttributeSet,
    AttributeForwardSet,
    Operation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterParam {
    pub name: String,
    pub type_: TypeRef,
    pub optional: bool,
    pub default_: Option<DefaultValueDef>,
    pub variadic: bool,
}

pub fn build_adapter_plan(
    world: &World,
    world_name: &str,
    module: &str,
) -> Result<AdapterPlan, BindgenError> {
    let lowering = lower_host_abi_with_metadata(world, &HostAbiOptions::new(world_name))?;
    let mut resources = world
        .interfaces
        .values()
        .map(|interface| plan_resource(world, interface))
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| left.name.cmp(&right.name));
    let mut callbacks = world
        .callbacks
        .values()
        .map(plan_callback)
        .collect::<Vec<_>>();
    callbacks.sort_by(|left, right| left.name.cmp(&right.name));
    let mut namespaces = world
        .namespaces
        .values()
        .map(plan_namespace)
        .collect::<Vec<_>>();
    namespaces.sort_by(|left, right| left.name.cmp(&right.name));
    let iterators = lowering
        .iterators
        .iter()
        .map(|iterator| {
            let prefix = snake_case(&iterator.interface);
            let collection_prefix = world.interfaces[&iterator.interface]
                .members
                .iter()
                .find_map(|member| match member {
                    Member::Collection(collection) => Some(match &collection.kind {
                        CollectionKind::Maplike { .. } | CollectionKind::Setlike { .. } => {
                            format!("{prefix}_collection")
                        }
                        CollectionKind::Iterable { .. } | CollectionKind::AsyncIterable { .. } => {
                            prefix.clone()
                        }
                    }),
                    _ => None,
                })
                .expect("iterator metadata originates from a collection member");
            AdapterIterator {
                interface: iterator.interface.clone(),
                resource: iterator.resource.clone(),
                item: iterator.item.clone(),
                create_import: format!("{collection_prefix}_iterator"),
                next_import: format!("{collection_prefix}_iterator_next"),
            }
        })
        .collect();
    let async_iterators = lowering
        .async_iterators
        .iter()
        .map(|iterator| {
            let prefix = snake_case(&iterator.interface);
            AdapterAsyncIterator {
                interface: iterator.interface.clone(),
                resource: iterator.resource.clone(),
                item: iterator.item.clone(),
                create_import: format!("{prefix}_async_iterator"),
                next_import: format!("{prefix}_async_iterator_next"),
                cancel_import: format!("{prefix}_async_iterator_cancel"),
                drop_import: format!("{prefix}_async_iterator_drop"),
            }
        })
        .collect();
    let collections = world
        .interfaces
        .values()
        .filter_map(|interface| {
            interface.members.iter().find_map(|member| {
                let Member::Collection(collection) = member else {
                    return None;
                };
                let prefix = snake_case(&interface.name);
                let kind = match &collection.kind {
                    CollectionKind::Maplike {
                        key,
                        value,
                        read_only: true,
                    } => AdapterCollectionKind::ReadonlyMaplike {
                        key: key.clone(),
                        value: value.clone(),
                        get_import: format!("{prefix}_collection_get"),
                    },
                    CollectionKind::Setlike {
                        value,
                        read_only: true,
                    } => AdapterCollectionKind::ReadonlySetlike {
                        value: value.clone(),
                    },
                    CollectionKind::Maplike {
                        key,
                        value,
                        read_only: false,
                    } => AdapterCollectionKind::MutableMaplike {
                        key: key.clone(),
                        value: value.clone(),
                        get_import: format!("{prefix}_collection_get"),
                        set_import: format!("{prefix}_collection_set"),
                        delete_import: format!("{prefix}_collection_delete"),
                        clear_import: format!("{prefix}_collection_clear"),
                    },
                    CollectionKind::Setlike {
                        value,
                        read_only: false,
                    } => AdapterCollectionKind::MutableSetlike {
                        value: value.clone(),
                        add_import: format!("{prefix}_collection_add"),
                        delete_import: format!("{prefix}_collection_delete"),
                        clear_import: format!("{prefix}_collection_clear"),
                    },
                    CollectionKind::Iterable { .. } | CollectionKind::AsyncIterable { .. } => {
                        return None;
                    }
                };
                Some(AdapterCollection {
                    interface: interface.name.clone(),
                    kind,
                    size_import: format!("{prefix}_collection_size"),
                    has_import: format!("{prefix}_collection_has"),
                })
            })
        })
        .collect();
    Ok(AdapterPlan {
        contract: HOST_RUNTIME_CONTRACT,
        module: module.to_owned(),
        host_abi: lowering.world.clone(),
        resources,
        namespaces,
        iterators,
        async_iterators,
        collections,
        callbacks,
        lowering,
        runtime_operations: BTreeSet::from([
            "resources.insert",
            "resources.borrow",
            "resources.take",
            "resources.drop",
            "resources.withBorrowed",
            "callbacks.register",
            "callbacks.invoke",
            "callbacks.release",
            "futures.settle",
            "futures.cancel",
            "async_iterators.next",
            "async_iterators.cancel",
            "async_iterators.drop",
        ]),
    })
}

fn plan_namespace(namespace: &NamespaceDef) -> AdapterNamespace {
    let mut functions = Vec::new();
    for member in &namespace.members {
        match member {
            NamespaceMember::Attribute(attribute) => {
                functions.push(AdapterFunction {
                    import_name: format!(
                        "{}_get_{}",
                        snake_case(&namespace.name),
                        snake_case(&attribute.name)
                    ),
                    abi_method_name: format!("get-{}", attribute.name),
                    member_name: attribute.name.clone(),
                    invocation: AdapterInvocation::AttributeGet,
                    static_: true,
                    params: Vec::new(),
                    result: attribute.type_.clone(),
                    async_: false,
                    attributes: attribute.attributes.clone(),
                });
            }
            NamespaceMember::Operation(operation) => {
                let mut function = plan_operation_like(&namespace.name, operation);
                function.static_ = true;
                functions.push(function);
            }
        }
    }
    functions.sort_by(|left, right| left.import_name.cmp(&right.import_name));
    AdapterNamespace {
        name: namespace.name.clone(),
        attributes: namespace.attributes.clone(),
        functions,
    }
}

fn plan_resource(world: &World, interface: &InterfaceDef) -> AdapterResource {
    let mut functions = Vec::new();
    for member in &interface.members {
        match member {
            Member::Const(_) => {}
            // `build_adapter_plan` lowers the host ABI before reaching this
            // planner, and collection declarations fail there until an
            // iterator ownership protocol exists.
            Member::Collection(_) => {}
            Member::Constructor(constructor) => {
                functions.push(plan_constructor(interface, constructor));
            }
            Member::Attribute(attribute) => {
                functions.push(AdapterFunction {
                    import_name: format!(
                        "{}_get_{}",
                        snake_case(&interface.name),
                        snake_case(&attribute.name)
                    ),
                    abi_method_name: format!("get-{}", attribute.name),
                    member_name: attribute.name.clone(),
                    invocation: AdapterInvocation::AttributeGet,
                    static_: attribute.static_,
                    params: Vec::new(),
                    result: attribute.type_.clone(),
                    async_: false,
                    attributes: attribute.attributes.clone(),
                });
                if !attribute.read_only {
                    functions.push(AdapterFunction {
                        import_name: format!(
                            "{}_set_{}",
                            snake_case(&interface.name),
                            snake_case(&attribute.name)
                        ),
                        abi_method_name: format!("set-{}", attribute.name),
                        member_name: attribute.name.clone(),
                        invocation: AdapterInvocation::AttributeSet,
                        static_: attribute.static_,
                        params: vec![AdapterParam {
                            name: "value".to_owned(),
                            type_: attribute.type_.clone(),
                            optional: false,
                            default_: None,
                            variadic: false,
                        }],
                        result: TypeRef::Unit,
                        async_: false,
                        attributes: attribute.attributes.clone(),
                    });
                } else if let Some(forwarded) = &attribute.attributes.put_forwards {
                    let TypeRef::Named(target) = &attribute.type_ else {
                        unreachable!("validated PutForwards target");
                    };
                    let forwarded_type = world.interfaces[target]
                        .members
                        .iter()
                        .find_map(|member| match member {
                            Member::Attribute(candidate) if candidate.name == *forwarded => {
                                Some(candidate.type_.clone())
                            }
                            _ => None,
                        })
                        .expect("validated PutForwards member");
                    functions.push(AdapterFunction {
                        import_name: format!(
                            "{}_set_{}",
                            snake_case(&interface.name),
                            snake_case(&attribute.name)
                        ),
                        abi_method_name: format!("set-{}", attribute.name),
                        member_name: attribute.name.clone(),
                        invocation: AdapterInvocation::AttributeForwardSet,
                        static_: false,
                        params: vec![AdapterParam {
                            name: "value".to_owned(),
                            type_: forwarded_type,
                            optional: false,
                            default_: None,
                            variadic: false,
                        }],
                        result: TypeRef::Unit,
                        async_: false,
                        attributes: attribute.attributes.clone(),
                    });
                }
            }
            Member::Operation(operation) => {
                functions.push(plan_operation(interface, operation));
            }
        }
    }
    functions.sort_by(|left, right| left.import_name.cmp(&right.import_name));
    AdapterResource {
        name: interface.name.clone(),
        attributes: interface.attributes.clone(),
        functions,
    }
}

fn plan_constructor(interface: &InterfaceDef, constructor: &ConstructorDef) -> AdapterFunction {
    let abi_base = match &constructor.name {
        Some(name) => format!("named-constructor-{name}"),
        None => "constructor".to_owned(),
    };
    let abi_method_name = if constructor.overload > 0 {
        format!("{abi_base}-{}", constructor.overload)
    } else {
        abi_base
    };
    AdapterFunction {
        import_name: constructor_import_name(interface, constructor),
        abi_method_name,
        member_name: constructor
            .name
            .clone()
            .unwrap_or_else(|| interface.name.clone()),
        invocation: AdapterInvocation::Constructor,
        static_: true,
        params: constructor.arguments.iter().map(plan_param).collect(),
        result: TypeRef::Named(interface.name.clone()),
        async_: false,
        attributes: constructor.attributes.clone(),
    }
}

fn plan_operation(interface: &InterfaceDef, operation: &OperationDef) -> AdapterFunction {
    plan_operation_like(&interface.name, operation)
}

fn plan_operation_like(owner: &str, operation: &OperationDef) -> AdapterFunction {
    let suffix = if operation.overload > 0 {
        format!("_{}", operation.overload)
    } else {
        String::new()
    };
    let abi_suffix = if operation.overload > 0 {
        format!("-{}", operation.overload)
    } else {
        String::new()
    };
    let (result, async_) = match &operation.result {
        TypeRef::Promise(payload) => ((*payload.clone()), true),
        result => (result.clone(), false),
    };
    AdapterFunction {
        import_name: format!(
            "{}_{}{}",
            snake_case(owner),
            snake_case(&operation.name),
            suffix
        ),
        abi_method_name: format!("{}{abi_suffix}", operation.name),
        member_name: operation.name.clone(),
        invocation: AdapterInvocation::Operation,
        static_: operation.static_,
        params: operation.arguments.iter().map(plan_param).collect(),
        result,
        async_,
        attributes: operation.attributes.clone(),
    }
}

fn plan_callback(callback: &CallbackDef) -> AdapterCallback {
    let (result, async_) = match &callback.result {
        TypeRef::Promise(payload) => ((*payload.clone()), true),
        result => (result.clone(), false),
    };
    AdapterCallback {
        name: callback.name.clone(),
        interface_operation: callback.interface_operation.clone(),
        params: callback.arguments.iter().map(plan_param).collect(),
        result,
        async_,
        attributes: callback.attributes.clone(),
    }
}

fn plan_param(argument: &ArgumentDef) -> AdapterParam {
    AdapterParam {
        name: argument.name.clone(),
        type_: argument.type_.clone(),
        optional: argument.optional,
        default_: argument.default_.clone(),
        variadic: argument.variadic,
    }
}

/// Emit an executable JavaScript semantic adapter.
///
/// Function arguments/results here are semantic host values. The emitter does
/// not claim to decode canonical-memory pointers from a core Wasm module.
pub fn emit_js_canonical_adapter(
    world: &World,
    plan: &AdapterPlan,
) -> Result<String, BindgenError> {
    let has_callbacks = !plan.callbacks.is_empty();
    let has_resources = !plan.resources.is_empty()
        || !plan.iterators.is_empty()
        || !plan.async_iterators.is_empty()
        || !plan.collections.is_empty();
    let has_async = !plan.async_iterators.is_empty()
        || plan
            .resources
            .iter()
            .flat_map(|resource| &resource.functions)
            .chain(
                plan.namespaces
                    .iter()
                    .flat_map(|namespace| &namespace.functions),
            )
            .any(|function| function.async_);
    let mut output = format!(
        "// @generated by fe-webidl-bindgen; semantic adapter boundary.\n\
         export const FE_HOST_RUNTIME_CONTRACT = {:?};\n\n\
         export function createFeHostAdapter(host, runtime) {{\n\
         \x20 if (runtime.protocol !== FE_HOST_RUNTIME_CONTRACT) throw new TypeError(`expected ${{FE_HOST_RUNTIME_CONTRACT}} runtime`);\n\
         \x20 const withBorrowedList = (values, convert, callback, index = 0, output = []) => {{\n\
         \x20   if (index === values.length) return callback(output);\n\
         \x20   return convert(values[index], converted => {{ output.push(converted); return withBorrowedList(values, convert, callback, index + 1, output); }});\n\
         \x20 }};\n",
        HOST_RUNTIME_CONTRACT
    );
    output.push_str(
        "  const sameObjectCache = new WeakMap();\n\
         \x20 const requireLegacyUnforgeable = (target, name) => { const descriptor = Object.getOwnPropertyDescriptor(target, name); if (!descriptor || descriptor.configurable) throw new TypeError(`LegacyUnforgeable property ${name} must be own and non-configurable`); };\n",
    );
    if has_callbacks {
        output.push_str(
            "  const callbackFunctions = new Map();\n\
             \x20 const callbackFactories = Object.create(null);\n\
             \x20 const borrowCallback = (handle, signature) => {\n\
             \x20   let callback = callbackFunctions.get(handle);\n\
             \x20   if (callback === undefined) {\n\
             \x20     const factory = callbackFactories[signature];\n\
             \x20     if (factory === undefined) throw new TypeError(`unknown callback signature ${signature}`);\n\
             \x20     callback = factory(handle);\n\
             \x20     callbackFunctions.set(handle, callback);\n\
             \x20   }\n\
             \x20   return callback;\n\
             \x20 };\n\
             \x20 const releaseCallback = (handle) => { callbackFunctions.delete(handle); runtime.callbacks.release(handle); };\n",
        );
    }
    emit_dictionary_helpers(world, &mut output)?;
    emit_union_helpers(world, plan, &mut output)?;
    emit_callback_factories(world, plan, &mut output)?;
    output.push_str("  const imports = {\n");
    for resource in &plan.resources {
        for function in &resource.functions {
            emit_function(world, &resource.name, "interfaces", function, &mut output)?;
        }
    }
    for namespace in &plan.namespaces {
        for function in &namespace.functions {
            emit_function(world, &namespace.name, "namespaces", function, &mut output)?;
        }
    }
    for iterator in &plan.iterators {
        emit_iterator_functions(world, iterator, &mut output)?;
    }
    for iterator in &plan.async_iterators {
        emit_async_iterator_functions(world, iterator, &mut output)?;
    }
    for collection in &plan.collections {
        emit_collection_functions(world, collection, &mut output)?;
    }
    output.push_str("  };\n");
    if has_async {
        output.push_str(
            "  const settleFuture = (token, promise) => Promise.resolve(promise).then(\n\
             \x20   value => runtime.futures.settle(token, { ok: value }),\n\
             \x20   error => runtime.futures.settle(token, { error })\n\
             \x20 );\n",
        );
    }
    output.push_str("  return {\n    imports: { ");
    output.push_str(&format!("{:?}: imports", plan.module));
    output.push_str(" },\n");
    if has_resources {
        output.push_str("    resources: runtime.resources,\n");
    }
    if has_callbacks {
        output.push_str(
            "    registerCallback: (signature, callback) => runtime.callbacks.register(signature, callback),\n\
             \x20   releaseCallback,\n",
        );
    }
    if has_async {
        output.push_str(
            "    settleFuture,\n\
             \x20   cancelFuture: (token, reason) => runtime.futures.cancel(token, reason),\n",
        );
    }
    output.push_str("  };\n}\n");
    Ok(output)
}

/// Emit only operations selected for one generated adapter provider.
///
/// Synthetic iterator/collection groups are currently emitted atomically
/// because their helpers share state and ownership invariants. A partial group
/// fails closed instead of reintroducing omitted operations.
pub fn emit_js_selected_adapter(
    world: &World,
    plan: &AdapterPlan,
    provider: &str,
    selection: &AdapterSelectionManifest,
) -> Result<String, BindgenError> {
    if !selection.providers.is_empty() && selection.providers != [provider] {
        return Err(BindgenError::new(
            "adapter selection",
            format!(
                "selection providers {:?} do not identify exactly `{provider}`",
                selection.providers
            ),
        ));
    }
    let selected = selection
        .operations
        .iter()
        .filter(|operation| {
            operation.module == plan.module && operation.kind == ImportKind::Function
        })
        .map(|operation| operation.name.as_str())
        .collect::<BTreeSet<_>>();
    if selected.len() != selection.operations.len() {
        return Err(BindgenError::new(
            "adapter selection",
            "selection contains an operation for another module or unsupported kind",
        ));
    }

    let mut sliced = plan.clone();
    for resource in &mut sliced.resources {
        resource
            .functions
            .retain(|function| selected.contains(function.import_name.as_str()));
    }
    sliced.resources.retain(|resource| {
        !resource.functions.is_empty() || selection.resources.contains(&resource.name)
    });
    for namespace in &mut sliced.namespaces {
        namespace
            .functions
            .retain(|function| selected.contains(function.import_name.as_str()));
    }
    sliced
        .namespaces
        .retain(|namespace| !namespace.functions.is_empty());
    for iterator in &plan.iterators {
        selected_group(
            &selected,
            [&iterator.create_import, &iterator.next_import],
            "iterator",
        )?;
    }
    sliced
        .iterators
        .retain(|iterator| selected.contains(iterator.create_import.as_str()));
    for iterator in &plan.async_iterators {
        selected_group(
            &selected,
            [
                &iterator.create_import,
                &iterator.next_import,
                &iterator.cancel_import,
                &iterator.drop_import,
            ],
            "async iterator",
        )?;
    }
    sliced
        .async_iterators
        .retain(|iterator| selected.contains(iterator.create_import.as_str()));
    for collection in &plan.collections {
        let names = collection_import_names(collection);
        selected_group(&selected, names, "collection")?;
    }
    sliced
        .collections
        .retain(|collection| selected.contains(collection.size_import.as_str()));
    sliced
        .callbacks
        .retain(|callback| selection.types.contains(&callback.name));

    let mut sliced_world = world.clone();
    let resources = selection.resources.iter().collect::<BTreeSet<_>>();
    let types = selection.types.iter().collect::<BTreeSet<_>>();
    sliced_world
        .interfaces
        .retain(|name, _| resources.contains(name));
    sliced_world.namespaces.retain(|name, _| {
        sliced
            .namespaces
            .iter()
            .any(|namespace| &namespace.name == name)
    });
    sliced_world.typedefs.retain(|name, _| types.contains(name));
    sliced_world.enums.retain(|name, _| types.contains(name));
    sliced_world
        .dictionaries
        .retain(|name, _| types.contains(name));
    sliced_world
        .callbacks
        .retain(|name, _| types.contains(name));
    sliced_world.mixins.retain(|name, _| types.contains(name));
    sliced_world.includes.retain(|interface, mixins| {
        if !resources.contains(interface) {
            return false;
        }
        mixins.retain(|mixin| types.contains(mixin));
        true
    });
    emit_js_canonical_adapter(&sliced_world, &sliced)
}

fn selected_group<'a>(
    selected: &BTreeSet<&str>,
    names: impl IntoIterator<Item = &'a String>,
    context: &str,
) -> Result<bool, BindgenError> {
    let names = names.into_iter().map(String::as_str).collect::<Vec<_>>();
    let count = names
        .iter()
        .filter(|name| selected.contains(**name))
        .count();
    if count != 0 && count != names.len() {
        return Err(BindgenError::new(
            "adapter selection",
            format!("{context} operations must be selected as one ownership group"),
        ));
    }
    Ok(count != 0)
}

fn collection_import_names(collection: &AdapterCollection) -> Vec<&String> {
    let mut names = vec![&collection.size_import, &collection.has_import];
    match &collection.kind {
        AdapterCollectionKind::ReadonlyMaplike { get_import, .. } => names.push(get_import),
        AdapterCollectionKind::ReadonlySetlike { .. } => {}
        AdapterCollectionKind::MutableMaplike {
            get_import,
            set_import,
            delete_import,
            clear_import,
            ..
        } => names.extend([get_import, set_import, delete_import, clear_import]),
        AdapterCollectionKind::MutableSetlike {
            add_import,
            delete_import,
            clear_import,
            ..
        } => names.extend([add_import, delete_import, clear_import]),
    }
    names
}

fn emit_collection_functions(
    world: &World,
    collection: &AdapterCollection,
    output: &mut String,
) -> Result<(), BindgenError> {
    output.push_str(&format!(
        "    {:?}: function(selfHandle) {{ return runtime.resources.borrow(selfHandle).size; }},\n",
        collection.size_import
    ));
    let (argument_name, argument_type) = match &collection.kind {
        AdapterCollectionKind::ReadonlyMaplike { key, .. }
        | AdapterCollectionKind::MutableMaplike { key, .. } => ("key", key),
        AdapterCollectionKind::ReadonlySetlike { value }
        | AdapterCollectionKind::MutableSetlike { value, .. } => ("value", value),
    };
    let argument = from_fe(world, argument_type, argument_name)?;
    output.push_str(&format!(
        "    {:?}: function(selfHandle, {argument_name}) {{ return runtime.resources.borrow(selfHandle).has({argument}); }},\n",
        collection.has_import
    ));
    if let AdapterCollectionKind::ReadonlyMaplike {
        key,
        value,
        get_import,
    }
    | AdapterCollectionKind::MutableMaplike {
        key,
        value,
        get_import,
        ..
    } = &collection.kind
    {
        let key = from_fe(world, key, "key")?;
        let value = to_fe(world, value, "value")?;
        output.push_str(&format!(
            "    {get_import:?}: function(selfHandle, key) {{ const value = runtime.resources.borrow(selfHandle).get({key}); return value === undefined ? null : {value}; }},\n"
        ));
    }
    match &collection.kind {
        AdapterCollectionKind::MutableMaplike {
            key,
            value,
            set_import,
            delete_import,
            clear_import,
            ..
        } => {
            emit_fluent_mutation(
                world,
                output,
                set_import,
                "set",
                &[("key", key), ("value", value)],
            )?;
            emit_delete_mutation(world, output, delete_import, "key", key)?;
            emit_clear_mutation(output, clear_import);
        }
        AdapterCollectionKind::MutableSetlike {
            value,
            add_import,
            delete_import,
            clear_import,
        } => {
            emit_fluent_mutation(world, output, add_import, "add", &[("value", value)])?;
            emit_delete_mutation(world, output, delete_import, "value", value)?;
            emit_clear_mutation(output, clear_import);
        }
        AdapterCollectionKind::ReadonlyMaplike { .. }
        | AdapterCollectionKind::ReadonlySetlike { .. } => {}
    }
    Ok(())
}

fn emit_fluent_mutation(
    world: &World,
    output: &mut String,
    import: &str,
    method: &str,
    params: &[(&str, &TypeRef)],
) -> Result<(), BindgenError> {
    let names = params.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let converted = params
        .iter()
        .map(|(name, type_)| from_fe(world, type_, name))
        .collect::<Result<Vec<_>, _>>()?;
    output.push_str(&format!(
        "    {import:?}: function(selfHandle, {}) {{ try {{ const target = runtime.resources.take(selfHandle); const returned = target[{method:?}]({}); if (returned !== target) throw new TypeError(\"Web IDL {method} mutation must return this\"); return {{ ok: runtime.resources.insert(target) }}; }} catch (error) {{ return {{ error: String(error) }}; }} }},\n",
        names.join(", "),
        converted.join(", "),
    ));
    Ok(())
}

fn emit_delete_mutation(
    world: &World,
    output: &mut String,
    import: &str,
    name: &str,
    type_: &TypeRef,
) -> Result<(), BindgenError> {
    let converted = from_fe(world, type_, name)?;
    output.push_str(&format!(
        "    {import:?}: function(selfHandle, {name}) {{ try {{ return {{ ok: runtime.resources.borrow(selfHandle).delete({converted}) }}; }} catch (error) {{ return {{ error: String(error) }}; }} }},\n"
    ));
    Ok(())
}

fn emit_clear_mutation(output: &mut String, import: &str) {
    output.push_str(&format!(
        "    {import:?}: function(selfHandle) {{ try {{ runtime.resources.borrow(selfHandle).clear(); return {{ ok: undefined }}; }} catch (error) {{ return {{ error: String(error) }}; }} }},\n"
    ));
}

fn emit_iterator_functions(
    world: &World,
    iterator: &AdapterIterator,
    output: &mut String,
) -> Result<(), BindgenError> {
    output.push_str(&format!(
        "    {:?}: function(selfHandle) {{ return runtime.resources.insert(runtime.resources.borrow(selfHandle)[Symbol.iterator]()); }},\n",
        iterator.create_import
    ));
    let converted = match &iterator.item {
        IteratorItemBinding::Value(item) => to_fe(world, item, "step.value")?,
        IteratorItemBinding::Entry { key, value, .. } => {
            let key = to_fe(world, key, "entry[0]")?;
            let value = to_fe(world, value, "entry[1]")?;
            format!(
                "((entry) => {{ if (!Array.isArray(entry) || entry.length !== 2) throw new TypeError(\"Web IDL pair iterator must yield a two-element array\"); return {{ key: {key}, value: {value} }}; }})(step.value)"
            )
        }
    };
    output.push_str(&format!(
        "    {:?}: function(selfHandle) {{ try {{ const step = runtime.resources.borrow(selfHandle).next(); return {{ ok: step.done ? null : {converted} }}; }} catch (error) {{ return {{ error: String(error) }}; }} }},\n",
        iterator.next_import
    ));
    Ok(())
}

fn emit_async_iterator_functions(
    world: &World,
    iterator: &AdapterAsyncIterator,
    output: &mut String,
) -> Result<(), BindgenError> {
    let converted = match &iterator.item {
        IteratorItemBinding::Value(item) => to_fe(world, item, "step.value")?,
        IteratorItemBinding::Entry { key, value, .. } => {
            let key = to_fe(world, key, "entry[0]")?;
            let value = to_fe(world, value, "entry[1]")?;
            format!(
                "((entry) => {{ if (!Array.isArray(entry) || entry.length !== 2) throw new TypeError(\"Web IDL async pair iterator must yield a two-element array\"); return {{ key: {key}, value: {value} }}; }})(step.value)"
            )
        }
    };
    output.push_str(&format!(
        "    {:?}: function(selfHandle, ...args) {{ const source = runtime.resources.borrow(selfHandle); const iterator = source[Symbol.asyncIterator](...args); if (!iterator || typeof iterator.next !== \"function\") throw new TypeError(\"Symbol.asyncIterator must return an async iterator\"); const state = {{ iterator, pending: null, closed: false }}; return runtime.resources.insert(state, () => {{ const token = state.pending; state.pending = null; state.closed = true; if (token !== null) {{ try {{ runtime.futures.cancel(token, new Error(\"async iterator dropped\")); }} catch (_) {{}} }} if (typeof iterator.return === \"function\") Promise.resolve(iterator.return()).catch(() => {{}}); }}); }},\n",
        iterator.create_import
    ));
    output.push_str(&format!(
        "    {:?}: function(selfHandle, token) {{ const state = runtime.resources.borrow(selfHandle); if (state.pending !== null) throw new TypeError(\"async iterator backpressure permits exactly one in-flight next\"); if (state.closed) {{ runtime.futures.settle(token, {{ ok: null }}); return token; }} state.pending = token; Promise.resolve().then(() => state.iterator.next()).then(step => {{ if (state.pending !== token) return; state.pending = null; if (!step || typeof step.done !== \"boolean\") throw new TypeError(\"async iterator next must return an iterator result\"); if (step.done) state.closed = true; runtime.futures.settle(token, {{ ok: step.done ? null : {converted} }}); }}, error => {{ if (state.pending !== token) return; state.pending = null; state.closed = true; runtime.futures.settle(token, {{ error: String(error) }}); }}); return token; }},\n",
        iterator.next_import
    ));
    output.push_str(&format!(
        "    {:?}: function(selfHandle, token, reason) {{ const state = runtime.resources.borrow(selfHandle); if (state.pending !== token) return false; state.pending = null; runtime.futures.cancel(token, reason); if (typeof state.iterator.return === \"function\") Promise.resolve(state.iterator.return()).catch(() => {{}}); return true; }},\n",
        iterator.cancel_import
    ));
    output.push_str(&format!(
        "    {:?}: function(selfHandle) {{ runtime.resources.drop(selfHandle); }},\n",
        iterator.drop_import
    ));
    Ok(())
}

fn emit_callback_factories(
    world: &World,
    plan: &AdapterPlan,
    output: &mut String,
) -> Result<(), BindgenError> {
    for callback in &plan.callbacks {
        let converted_names = (0..callback.params.len())
            .map(|index| format!("converted{index}"))
            .collect::<Vec<_>>();
        let invocation = format!(
            "runtime.callbacks.invoke(handle, {:?}, [{}])",
            callback.name,
            converted_names.join(", ")
        );
        let result = if callback.async_ {
            format!(
                "Promise.resolve({invocation}).then(value => {})",
                from_fe_owned(world, &callback.result, "value")?
            )
        } else if callback.result == TypeRef::Unit {
            format!(
                "((result) => result && typeof result.then === \"function\" ? Promise.resolve(result).then(() => undefined) : undefined)({invocation})"
            )
        } else {
            format!(
                "((result) => result && typeof result.then === \"function\" ? Promise.resolve(result).then(value => {}) : {})({invocation})",
                from_fe_owned(world, &callback.result, "value")?,
                from_fe_owned(world, &callback.result, "result")?
            )
        };
        let body = scope_callback_params(world, &callback.params, 0, &result)?;
        output.push_str(&format!(
            "  callbackFactories[{:?}] = handle => (...hostArgs) => {};\n",
            callback.name, body
        ));
    }
    Ok(())
}

fn scope_callback_params(
    world: &World,
    params: &[AdapterParam],
    index: usize,
    body: &str,
) -> Result<String, BindgenError> {
    if index == params.len() {
        return Ok(body.to_owned());
    }
    let param = &params[index];
    let raw = if param.variadic {
        format!("hostArgs.slice({index})")
    } else {
        format!("hostArgs[{index}]")
    };
    let supplied = if let Some(default) = &param.default_ {
        format!(
            "({raw} === undefined ? {} : {raw})",
            js_default_for_type(world, &param.type_, default)?
        )
    } else {
        raw.clone()
    };
    let rest = scope_callback_params(world, params, index + 1, body)?;
    let variadic_type = param
        .variadic
        .then(|| TypeRef::Sequence(Box::new(param.type_.clone())));
    let scoped = scope_host_to_fe(
        world,
        variadic_type.as_ref().unwrap_or(&param.type_),
        &supplied,
        &format!("converted{index}"),
        &rest,
    )?;
    if param.optional && param.default_.is_none() {
        Ok(format!(
            "({raw} === undefined ? ((converted{index}) => {rest})(undefined) : {scoped})"
        ))
    } else {
        Ok(scoped)
    }
}

/// Convert a host callback argument to Fe while keeping every borrowed
/// resource scope alive until `body` (including a returned Promise) settles.
fn scope_host_to_fe(
    world: &World,
    type_: &TypeRef,
    expression: &str,
    binding: &str,
    body: &str,
) -> Result<String, BindgenError> {
    Ok(match type_ {
        TypeRef::Named(name) if world.interfaces.contains_key(name) => {
            format!("runtime.resources.withBorrowed({expression}, {binding} => {body})")
        }
        TypeRef::Named(name) if world.callbacks.contains_key(name) => {
            format!("(({binding}) => {body})(runtime.callbacks.register({name:?}, {expression}))")
        }
        TypeRef::Named(name) if world.dictionaries.contains_key(name) => {
            scope_dictionary_to_fe(world, name, expression, binding, body)?
        }
        TypeRef::Nullable(inner) => format!(
            "({expression} == null ? (({binding}) => {body})(null) : {})",
            scope_host_to_fe(world, inner, expression, binding, body)?
        ),
        TypeRef::Sequence(inner) => {
            let item_scope =
                scope_host_to_fe(world, inner, "item", "convertedItem", "next(convertedItem)")?;
            format!(
                "withBorrowedList(Array.from({expression}), (item, next) => {item_scope}, {binding} => {body})"
            )
        }
        TypeRef::Union(members) => {
            let mut branches = String::new();
            for member in members {
                let case = stable_union_case_name(member);
                let wrapped_body = format!(
                    "(({binding}) => {body})({{ case: {:?}, value: convertedUnion }})",
                    case
                );
                branches.push_str(&format!(
                    "case {:?}: return {};",
                    case,
                    scope_host_to_fe(
                        world,
                        member,
                        "unionValue.value",
                        "convertedUnion",
                        &wrapped_body,
                    )?
                ));
            }
            format!(
                "((unionValue) => {{ switch (unionValue.case) {{ {branches} default: throw new TypeError(`invalid Web IDL union case ${{unionValue.case}}`); }} }})({expression})"
            )
        }
        TypeRef::Promise(_) => {
            return Err(BindgenError::new(
                "emit callback adapter",
                "`Promise` is not valid in a callback argument position",
            ));
        }
        _ => format!("(({binding}) => {body})({expression})"),
    })
}

fn scope_dictionary_to_fe(
    world: &World,
    name: &str,
    expression: &str,
    binding: &str,
    body: &str,
) -> Result<String, BindgenError> {
    let dictionary = &world.dictionaries[name];
    let members = dictionary_members(world, dictionary);
    let mut converted = body.to_owned();
    let object = members
        .iter()
        .map(|member| format!("{:?}: converted_{}", member.name, member.name))
        .collect::<Vec<_>>()
        .join(", ");
    converted = format!("(({binding}) => {converted})({{ {object} }})");
    for member in members.into_iter().rev() {
        let raw = format!("{expression}[{:?}]", member.name);
        let supplied = if let Some(default) = &member.default_ {
            format!(
                "({raw} === undefined ? {} : {raw})",
                js_default_for_type(world, &member.type_, default)?
            )
        } else {
            raw
        };
        converted = scope_host_to_fe(
            world,
            &member.type_,
            &supplied,
            &format!("converted_{}", member.name),
            &converted,
        )?;
    }
    Ok(converted)
}

fn emit_function(
    world: &World,
    owner_name: &str,
    host_collection: &str,
    function: &AdapterFunction,
    output: &mut String,
) -> Result<(), BindgenError> {
    let mut params = function
        .params
        .iter()
        .map(|param| js_ident(&param.name))
        .collect::<Vec<_>>();
    if !function.static_ {
        params.insert(0, "selfHandle".to_owned());
    }
    let target = if function.static_ {
        format!("host.{host_collection}[{owner_name:?}]")
    } else {
        "runtime.resources.borrow(selfHandle)".to_owned()
    };
    let body = match function.invocation {
        AdapterInvocation::Constructor => {
            let mut args = Vec::new();
            for param in &function.params {
                let name = js_ident(&param.name);
                let supplied = if let Some(default) = &param.default_ {
                    format!(
                        "({name} === undefined ? {} : {name})",
                        js_default_for_type(world, &param.type_, default)?
                    )
                } else {
                    name.clone()
                };
                let converted = from_fe(world, &param.type_, &supplied)?;
                if param.variadic {
                    args.push(format!("...{converted}"));
                } else {
                    args.push(converted);
                }
            }
            let call = format!(
                "new host.interfaces[{:?}]({})",
                function.member_name,
                args.join(", ")
            );
            format!("return {};", to_fe(world, &function.result, &call)?)
        }
        AdapterInvocation::AttributeGet => {
            let check = if function.attributes.legacy_unforgeable {
                format!(
                    "requireLegacyUnforgeable({target}, {:?}); ",
                    function.member_name
                )
            } else {
                String::new()
            };
            if function.attributes.same_object {
                let converted = to_fe(world, &function.result, "value")?;
                format!(
                    "{check}const value = {target}[{:?}]; let ownerCache = sameObjectCache.get({target}); if (ownerCache === undefined) {{ ownerCache = new Map(); sameObjectCache.set({target}, ownerCache); }} const cached = ownerCache.get({:?}); if (cached !== undefined) {{ if (cached.value !== value) throw new TypeError(\"SameObject getter changed identity\"); return cached.converted; }} const converted = {converted}; ownerCache.set({:?}, {{ value, converted }}); return converted;",
                    function.member_name, function.member_name, function.member_name
                )
            } else {
                format!(
                    "{check}return {};",
                    to_fe(
                        world,
                        &function.result,
                        &format!("{target}[{:?}]", function.member_name)
                    )?
                )
            }
        }
        AdapterInvocation::AttributeSet => {
            let value = from_fe(world, &function.params[0].type_, "value")?;
            let check = if function.attributes.legacy_unforgeable {
                format!(
                    "requireLegacyUnforgeable({target}, {:?}); ",
                    function.member_name
                )
            } else {
                String::new()
            };
            format!("{check}{target}[{:?}] = {value};", function.member_name)
        }
        AdapterInvocation::AttributeForwardSet => {
            let value = from_fe(world, &function.params[0].type_, "value")?;
            let forwarded = function
                .attributes
                .put_forwards
                .as_deref()
                .expect("AttributeForwardSet carries PutForwards metadata");
            format!(
                "const forwardedTarget = {target}[{:?}]; if (forwardedTarget == null) throw new TypeError(\"PutForwards target is null\"); forwardedTarget[{forwarded:?}] = {value};",
                function.member_name
            )
        }
        AdapterInvocation::Operation => {
            let mut args = Vec::new();
            for param in &function.params {
                let name = js_ident(&param.name);
                let supplied = if let Some(default) = &param.default_ {
                    format!(
                        "({name} === undefined ? {} : {name})",
                        js_default_for_type(world, &param.type_, default)?
                    )
                } else {
                    name.clone()
                };
                let converted = from_fe(world, &param.type_, &supplied)?;
                let converted = if param.optional && param.default_.is_none() {
                    format!("({name} === undefined ? undefined : {converted})")
                } else {
                    converted
                };
                if param.variadic {
                    args.push(format!("...{converted}"));
                } else {
                    args.push(converted);
                }
            }
            let call = format!("{target}[{:?}]({})", function.member_name, args.join(", "));
            if function.async_ {
                let conversion = to_fe(world, &function.result, "value")?;
                format!("return Promise.resolve({call}).then(value => {conversion});")
            } else if function.result == TypeRef::Unit {
                format!("{call};")
            } else {
                format!("return {};", to_fe(world, &function.result, &call)?)
            }
        }
    };
    output.push_str(&format!(
        "    {:?}: function({}) {{ {} }},\n",
        function.import_name,
        params.join(", "),
        body
    ));
    Ok(())
}

fn from_fe(world: &World, type_: &TypeRef, expression: &str) -> Result<String, BindgenError> {
    Ok(match type_ {
        TypeRef::Named(name) if world.interfaces.contains_key(name) => {
            format!("runtime.resources.borrow({expression})")
        }
        TypeRef::Named(name) if world.callbacks.contains_key(name) => {
            format!("borrowCallback({expression}, {name:?})")
        }
        TypeRef::Named(name) if world.dictionaries.contains_key(name) => {
            format!("fromFeDictionary_{name}({expression})")
        }
        TypeRef::Nullable(inner) => format!(
            "({expression} == null ? null : {})",
            from_fe(world, inner, expression)?
        ),
        TypeRef::Sequence(inner) => format!(
            "Array.from({expression}, value => {})",
            from_fe(world, inner, "value")?
        ),
        TypeRef::Union(_) => format!("fromFeUnion_{}({expression})", union_suffix(type_)),
        TypeRef::Promise(_) => {
            return Err(BindgenError::new(
                "emit semantic adapter",
                "`Promise` is not valid in an adapter argument position",
            ));
        }
        _ => expression.to_owned(),
    })
}

fn from_fe_owned(world: &World, type_: &TypeRef, expression: &str) -> Result<String, BindgenError> {
    Ok(match type_ {
        TypeRef::Unit => "undefined".to_owned(),
        TypeRef::Named(name) if world.interfaces.contains_key(name) => {
            format!("runtime.resources.take({expression})")
        }
        TypeRef::Named(name) if world.callbacks.contains_key(name) => {
            format!("borrowCallback({expression}, {name:?})")
        }
        TypeRef::Named(name) if world.dictionaries.contains_key(name) => {
            format!("takeFeDictionary_{name}({expression})")
        }
        TypeRef::Nullable(inner) => format!(
            "({expression} == null ? null : {})",
            from_fe_owned(world, inner, expression)?
        ),
        TypeRef::Sequence(inner) => format!(
            "Array.from({expression}, value => {})",
            from_fe_owned(world, inner, "value")?
        ),
        TypeRef::Union(_) => {
            format!("takeFeUnion_{}({expression})", union_suffix(type_))
        }
        TypeRef::Promise(_) => {
            return Err(BindgenError::new(
                "emit callback adapter",
                "nested `Promise` callback result is not supported",
            ));
        }
        _ => expression.to_owned(),
    })
}

fn to_fe(world: &World, type_: &TypeRef, expression: &str) -> Result<String, BindgenError> {
    Ok(match type_ {
        TypeRef::Unit => "undefined".to_owned(),
        TypeRef::Named(name) if world.interfaces.contains_key(name) => {
            format!("runtime.resources.insert({expression})")
        }
        TypeRef::Named(name) if world.callbacks.contains_key(name) => {
            format!("runtime.callbacks.register({name:?}, {expression})")
        }
        TypeRef::Named(name) if world.dictionaries.contains_key(name) => {
            format!("toFeDictionary_{name}({expression})")
        }
        TypeRef::Nullable(inner) => format!(
            "({expression} == null ? null : {})",
            to_fe(world, inner, expression)?
        ),
        TypeRef::Sequence(inner) => format!(
            "Array.from({expression}, value => {})",
            to_fe(world, inner, "value")?
        ),
        TypeRef::Union(_) => format!("toFeUnion_{}({expression})", union_suffix(type_)),
        TypeRef::Promise(_) => {
            return Err(BindgenError::new(
                "emit semantic adapter",
                "nested `Promise` result is not supported",
            ));
        }
        _ => expression.to_owned(),
    })
}

fn emit_dictionary_helpers(world: &World, output: &mut String) -> Result<(), BindgenError> {
    for dictionary in world.dictionaries.values() {
        let members = dictionary_members(world, dictionary);
        output.push_str(&format!(
            "  const fromFeDictionary_{} = value => ({{",
            dictionary.name
        ));
        for member in &members {
            let source = format!("value[{:?}]", member.name);
            let supplied = if let Some(default) = &member.default_ {
                format!(
                    "({source} === undefined ? {} : {source})",
                    js_default(default)
                )
            } else {
                source
            };
            output.push_str(&format!(
                "{:?}: {},",
                member.name,
                from_fe(world, &member.type_, &supplied)?
            ));
        }
        output.push_str("});\n");
        output.push_str(&format!(
            "  const takeFeDictionary_{} = value => ({{",
            dictionary.name
        ));
        for member in &members {
            output.push_str(&format!(
                "{:?}: {},",
                member.name,
                from_fe_owned(world, &member.type_, &format!("value[{:?}]", member.name))?
            ));
        }
        output.push_str("});\n");
        output.push_str(&format!(
            "  const toFeDictionary_{} = value => ({{",
            dictionary.name
        ));
        for member in &members {
            output.push_str(&format!(
                "{:?}: {},",
                member.name,
                to_fe(world, &member.type_, &format!("value[{:?}]", member.name))?
            ));
        }
        output.push_str("});\n");
    }
    Ok(())
}

fn dictionary_members<'a>(
    world: &'a World,
    dictionary: &'a crate::DictionaryDef,
) -> Vec<&'a crate::DictionaryMemberDef> {
    let mut lineage = Vec::new();
    let mut cursor = Some(dictionary);
    while let Some(current) = cursor {
        lineage.push(current);
        cursor = current
            .inherits
            .as_ref()
            .and_then(|parent| world.dictionaries.get(parent));
    }
    lineage.reverse();
    lineage
        .into_iter()
        .flat_map(|definition| definition.members.iter())
        .collect()
}

fn emit_union_helpers(
    world: &World,
    plan: &AdapterPlan,
    output: &mut String,
) -> Result<(), BindgenError> {
    for definition in &plan.host_abi.types {
        let abi::TypeDefKind::Variant { cases } = &definition.kind else {
            continue;
        };
        if !definition.name.starts_with("webidl-union-") {
            continue;
        }
        let Some(union) = find_union_by_suffix(world, &definition.name) else {
            return Err(BindgenError::new(
                format!("adapter union `{}`", definition.name),
                "could not recover normalized Web IDL union",
            ));
        };
        let TypeRef::Union(members) = union else {
            unreachable!();
        };
        for direction in ["fromFe", "toFe", "takeFe"] {
            output.push_str(&format!(
                "  const {direction}Union_{} = value => {{ switch (value.case) {{",
                union_suffix(union)
            ));
            for (case, member) in cases.iter().zip(members) {
                let converted = match direction {
                    "fromFe" => from_fe(world, member, "value.value")?,
                    "toFe" => to_fe(world, member, "value.value")?,
                    "takeFe" => from_fe_owned(world, member, "value.value")?,
                    _ => unreachable!(),
                };
                if direction == "toFe" {
                    output.push_str(&format!(
                        "case {:?}: return {{ case: {:?}, value: {} }};",
                        case.name, case.name, converted
                    ));
                } else {
                    output.push_str(&format!("case {:?}: return {};", case.name, converted));
                }
            }
            output.push_str(
                "default: throw new TypeError(`invalid Web IDL union case ${value.case}`); } };\n",
            );
        }
    }
    Ok(())
}

fn find_union_by_suffix<'a>(world: &'a World, name: &str) -> Option<&'a TypeRef> {
    let mut found = None;
    visit_world_types(world, &mut |type_| {
        if matches!(type_, TypeRef::Union(_)) && stable_union_name(type_) == name {
            found = Some(type_);
        }
    });
    found
}

fn visit_world_types<'a>(world: &'a World, visitor: &mut impl FnMut(&'a TypeRef)) {
    for typedef in world.typedefs.values() {
        visit_type(&typedef.type_, visitor);
    }
    for dictionary in world.dictionaries.values() {
        for member in &dictionary.members {
            visit_type(&member.type_, visitor);
        }
    }
    for callback in world.callbacks.values() {
        for argument in &callback.arguments {
            visit_type(&argument.type_, visitor);
        }
        visit_type(&callback.result, visitor);
    }
    for interface in world.interfaces.values() {
        for member in &interface.members {
            match member {
                Member::Const(_) => {}
                Member::Collection(collection) => match &collection.kind {
                    CollectionKind::Iterable { key, value }
                    | CollectionKind::AsyncIterable { key, value, .. } => {
                        if let Some(key) = key {
                            visit_type(key, visitor);
                        }
                        visit_type(value, visitor);
                    }
                    CollectionKind::Maplike { key, value, .. } => {
                        visit_type(key, visitor);
                        visit_type(value, visitor);
                    }
                    CollectionKind::Setlike { value, .. } => visit_type(value, visitor),
                },
                Member::Constructor(constructor) => {
                    for argument in &constructor.arguments {
                        visit_type(&argument.type_, visitor);
                    }
                }
                Member::Attribute(attribute) => visit_type(&attribute.type_, visitor),
                Member::Operation(operation) => {
                    for argument in &operation.arguments {
                        visit_type(&argument.type_, visitor);
                    }
                    visit_type(&operation.result, visitor);
                }
            }
        }
    }
    for namespace in world.namespaces.values() {
        for member in &namespace.members {
            match member {
                NamespaceMember::Attribute(attribute) => visit_type(&attribute.type_, visitor),
                NamespaceMember::Operation(operation) => {
                    for argument in &operation.arguments {
                        visit_type(&argument.type_, visitor);
                    }
                    visit_type(&operation.result, visitor);
                }
            }
        }
    }
}

fn visit_type<'a>(type_: &'a TypeRef, visitor: &mut impl FnMut(&'a TypeRef)) {
    visitor(type_);
    match type_ {
        TypeRef::Nullable(inner)
        | TypeRef::Sequence(inner)
        | TypeRef::Promise(inner)
        | TypeRef::Record(inner) => visit_type(inner, visitor),
        TypeRef::Union(members) => {
            for member in members {
                visit_type(member, visitor);
            }
        }
        _ => {}
    }
}

fn union_suffix(type_: &TypeRef) -> String {
    stable_union_name(type_)
        .strip_prefix("webidl-union-")
        .expect("stable union names have the documented prefix")
        .to_owned()
}

fn js_default(value: &DefaultValueDef) -> String {
    match value {
        DefaultValueDef::Bool(value) => value.to_string(),
        DefaultValueDef::Integer(value) | DefaultValueDef::Float(value) => match value.as_str() {
            "Infinity" => "Infinity".to_owned(),
            "-Infinity" => "-Infinity".to_owned(),
            "NaN" => "NaN".to_owned(),
            _ => value.clone(),
        },
        DefaultValueDef::String(value) => format!("{value:?}"),
        DefaultValueDef::Null => "null".to_owned(),
        DefaultValueDef::EmptySequence => "[]".to_owned(),
        DefaultValueDef::EmptyDictionary => "{}".to_owned(),
    }
}

fn js_default_for_type(
    world: &World,
    type_: &TypeRef,
    value: &DefaultValueDef,
) -> Result<String, BindgenError> {
    if let TypeRef::Union(members) = type_ {
        let member = members
            .iter()
            .find(|member| {
                crate::validate_default_type(world, member, value, "adapter default").is_ok()
            })
            .expect("operation defaults were validated during linking");
        return Ok(format!(
            "{{ case: {:?}, value: {} }}",
            stable_union_case_name(member),
            js_default_for_type(world, member, value)?
        ));
    }
    Ok(js_default(value))
}

fn snake_case(name: &str) -> String {
    let mut result = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push('_');
        }
    }
    result
}

fn js_ident(name: &str) -> String {
    match name {
        "delete" | "function" | "interface" | "new" | "return" | "this" | "var" => {
            format!("{name}_")
        }
        _ => name.replace('-', "_"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use std::time::{SystemTime, UNIX_EPOCH};

    const WEB_FIXTURE: &str = r#"
        dictionary AddEventListenerOptions {
            boolean capture = false;
            boolean once = false;
        };
        interface Event {};
        callback EventListener = undefined (Event event);
        interface EventTarget {
            undefined addEventListener(
                DOMString type,
                EventListener callback,
                optional AddEventListenerOptions options
            );
            undefined removeEventListener(
                DOMString type,
                EventListener callback
            );
        };
        interface AbortSignal {
            readonly attribute boolean aborted;
        };
        interface Fetcher {
            Promise<DOMString> fetch(DOMString url, AbortSignal signal);
        };
        interface Worker {
            attribute EventListener onmessage;
            undefined postMessage(
                (DOMString or sequence<unsigned long>) message
            );
        };
        interface MessagePort {
            undefined postMessage(DOMString message);
            Promise<DOMString> receive();
            undefined start();
        };
    "#;

    #[test]
    fn plans_event_callbacks_async_abort_workers_and_message_ports() {
        let world = parse(WEB_FIXTURE).unwrap();
        let first = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let second = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.contract, "fe:host-runtime/v1");
        assert_eq!(
            first
                .resources
                .iter()
                .map(|resource| resource.name.as_str())
                .collect::<Vec<_>>(),
            [
                "AbortSignal",
                "Event",
                "EventTarget",
                "Fetcher",
                "MessagePort",
                "Worker",
            ]
        );
        let event_target = first
            .resources
            .iter()
            .find(|resource| resource.name == "EventTarget")
            .unwrap();
        let add = event_target
            .functions
            .iter()
            .find(|function| function.member_name == "addEventListener")
            .unwrap();
        assert_eq!(
            add.params
                .iter()
                .map(|param| param.name.as_str())
                .collect::<Vec<_>>(),
            ["type", "callback", "options"]
        );
        let fetch = first
            .resources
            .iter()
            .find(|resource| resource.name == "Fetcher")
            .unwrap()
            .functions
            .iter()
            .find(|function| function.member_name == "fetch")
            .unwrap();
        assert!(fetch.async_);
        assert_eq!(fetch.result, TypeRef::String(crate::StringKind::Dom));
        assert_eq!(first.callbacks[0].name, "EventListener");
        assert!(first.runtime_operations.contains("callbacks.invoke"));
        assert!(first.runtime_operations.contains("futures.settle"));
    }

    #[test]
    fn emits_semantic_javascript_with_stable_runtime_contract() {
        let world = parse(WEB_FIXTURE).unwrap();
        let plan = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let js = emit_js_canonical_adapter(&world, &plan).unwrap();
        assert!(js.contains("fe:host-runtime/v1"));
        assert!(js.contains("runtime.protocol !== FE_HOST_RUNTIME_CONTRACT"));
        assert!(js.contains("runtime.resources.borrow(selfHandle)"));
        assert!(js.contains("runtime.callbacks.invoke(handle, \"EventListener\""));
        assert!(js.contains("runtime.callbacks.register"));
        assert!(js.contains("runtime.callbacks.release"));
        assert!(js.contains("runtime.futures.settle"));
        assert!(js.contains("runtime.futures.cancel"));
        assert!(js.contains("event_target_add_event_listener"));
        assert!(js.contains("borrowCallback(callback, \"EventListener\")"));
        assert!(js.contains("Promise.resolve("));
        assert!(js.contains("..."));
        assert!(js.contains("fromFeUnion_"));
        assert!(js.contains("\"fe:web\": imports"));
        assert!(!js.contains("core::browser"));
    }

    #[test]
    fn adapter_plan_remains_a_blueprint_for_core_wasm_marshalling() {
        let world = parse(WEB_FIXTURE).unwrap();
        let plan = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let error = fe_host_abi::SupportProfile::current_fe_wasm_imports()
            .check(&plan.host_abi)
            .unwrap_err();
        assert!(error.missing.contains(&abi::AbiFeature::Resource));
        assert!(error.missing.contains(&abi::AbiFeature::Callback));
        assert!(error.missing.contains(&abi::AbiFeature::Future));
    }

    #[test]
    fn async_iterable_plan_is_bounded_and_raw_fe_stays_gated() {
        let world = parse("interface Updates { async iterable<DOMString>; };").unwrap();
        let plan = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let [iterator] = plan.async_iterators.as_slice() else {
            panic!("expected one async iterator");
        };
        assert_eq!(iterator.resource, "UpdatesAsyncIterator");
        assert_eq!(iterator.create_import, "updates_async_iterator");
        assert_eq!(iterator.next_import, "updates_async_iterator_next");
        assert_eq!(iterator.cancel_import, "updates_async_iterator_cancel");
        assert_eq!(iterator.drop_import, "updates_async_iterator_drop");
        let metadata = &plan.lowering.async_iterators[0];
        assert_eq!(
            metadata.token_owner,
            crate::AsyncIteratorTokenOwner::CallerRuntime
        );
        assert_eq!(
            metadata.backpressure,
            crate::AsyncIteratorBackpressure::SequentialOneInFlight
        );
        assert_eq!(
            metadata.cancellation,
            crate::AsyncIteratorCancellation::OwnedSubscription
        );
        let js = emit_js_canonical_adapter(&world, &plan).unwrap();
        assert!(js.contains("[Symbol.asyncIterator](...args)"), "{js}");
        assert!(js.contains("exactly one in-flight next"), "{js}");
        assert!(js.contains("state.pending !== token"), "{js}");
        assert!(js.contains("runtime.futures.cancel"), "{js}");
        let error = crate::emit_fe_flat_host_imports(&world, "fe:web").unwrap_err();
        assert!(
            error.detail.contains("Future/await state machines"),
            "{error}"
        );
    }

    #[test]
    fn bun_async_iterator_enforces_backpressure_cancel_drop_and_late_suppression() {
        if !std::process::Command::new("bun")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let world = parse("interface Updates { async iterable<DOMString>; };").unwrap();
        let plan = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let iterator = &plan.async_iterators[0];
        let adapter = emit_js_canonical_adapter(&world, &plan).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "fe-webidl-async-iterator-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let adapter_path = directory.join("adapter.mjs");
        let test_path = directory.join("test.mjs");
        std::fs::write(&adapter_path, adapter).unwrap();
        let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../demos/shared/host-runtime.js")
            .canonicalize()
            .unwrap();
        let script = format!(
            r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};
const deferred = () => {{
  let resolve, reject;
  const promise = new Promise((a, b) => {{ resolve = a; reject = b; }});
  return {{ promise, resolve, reject }};
}};
const queue = [];
let returns = 0;
const source = {{
  [Symbol.asyncIterator]() {{
    return {{
      next() {{ const item = deferred(); queue.push(item); return item.promise; }},
      return() {{ returns += 1; return Promise.resolve({{ done: true }}); }},
    }};
  }},
}};
const runtime = createFeHostRuntime();
const adapter = createFeHostAdapter({{ interfaces: {{}} }}, runtime);
const imports = adapter.imports["fe:web"];
const sourceHandle = runtime.resources.insert(source);
const iteratorHandle = imports[{create:?}](sourceHandle);

const first = runtime.futures.create();
imports[{next:?}](iteratorHandle, first.token);
const blocked = runtime.futures.create();
let backpressure = false;
try {{ imports[{next:?}](iteratorHandle, blocked.token); }}
catch (error) {{ backpressure = /one in-flight/.test(error.message); }}
if (!backpressure) throw new Error("missing sequential backpressure");
runtime.futures.cancel(blocked.token);
runtime.futures.release(blocked.token);
await Promise.resolve();
queue.shift().resolve({{ done: false, value: "one" }});
if (await first.promise !== "one") throw new Error("wrong first item");
runtime.futures.release(first.token);

const done = runtime.futures.create();
imports[{next:?}](iteratorHandle, done.token);
await Promise.resolve();
queue.shift().resolve({{ done: true }});
if (await done.promise !== null) throw new Error("completion must resolve null");
runtime.futures.release(done.token);

const cancelledHandle = imports[{create:?}](sourceHandle);
const cancelled = runtime.futures.create();
imports[{next:?}](cancelledHandle, cancelled.token);
await Promise.resolve();
const late = queue.shift();
if (!imports[{cancel:?}](cancelledHandle, cancelled.token, new Error("stop")))
  throw new Error("cancel did not own pending subscription");
await cancelled.promise.catch(() => {{}});
late.resolve({{ done: false, value: "late" }});
await Promise.resolve(); await Promise.resolve();
if (runtime.futures.inspect(cancelled.token).state !== "cancelled")
  throw new Error("late resolution changed cancellation");
runtime.futures.release(cancelled.token);

const droppedHandle = imports[{create:?}](sourceHandle);
const dropped = runtime.futures.create();
imports[{next:?}](droppedHandle, dropped.token);
await Promise.resolve();
const droppedLate = queue.shift();
imports[{drop:?}](droppedHandle);
await dropped.promise.catch(() => {{}});
droppedLate.resolve({{ done: false, value: "stale" }});
await Promise.resolve(); await Promise.resolve();
if (runtime.futures.inspect(dropped.token).state !== "cancelled")
  throw new Error("drop did not cancel pending next");
runtime.futures.release(dropped.token);
let stale = false;
try {{ imports[{next:?}](droppedHandle, runtime.futures.create().token); }}
catch (error) {{ stale = error.code === "stale_handle"; }}
if (!stale) throw new Error("dropped iterator handle was not stale");
if (returns < 2) throw new Error("cancel/drop did not close owned iterators");
imports[{drop:?}](iteratorHandle);
runtime.resources.drop(sourceHandle);
"#,
            adapter_url = format!("file://{}", adapter_path.display()),
            runtime_url = format!("file://{}", runtime_path.display()),
            create = iterator.create_import,
            next = iterator.next_import,
            cancel = iterator.cancel_import,
            drop = iterator.drop_import,
        );
        std::fs::write(&test_path, script).unwrap();
        let output = std::process::Command::new("bun")
            .arg("run")
            .arg(&test_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Bun async iterator integration failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn nested_callback_conversion_is_direction_aware_and_scoped() {
        let world = parse(
            r#"
                interface Event {};
                callback Listener = undefined (Event event);
            "#,
        )
        .unwrap();
        let nested = TypeRef::Sequence(Box::new(TypeRef::Union(vec![
            TypeRef::Named("Event".to_owned()),
            TypeRef::String(crate::StringKind::Dom),
        ])));
        let emitted = scope_host_to_fe(
            &world,
            &nested,
            "hostEvents",
            "convertedEvents",
            "invoke(convertedEvents)",
        )
        .unwrap();
        assert!(emitted.contains("withBorrowedList"));
        assert!(emitted.contains("runtime.resources.withBorrowed"));
        assert!(emitted.contains("switch (unionValue.case)"));
        assert!(emitted.contains("invoke(convertedEvents)"));
    }

    #[test]
    fn bun_event_callback_borrow_is_live_through_async_and_stale_afterward() {
        if !std::process::Command::new("bun")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let world = parse(WEB_FIXTURE).unwrap();
        let plan = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let adapter = emit_js_canonical_adapter(&world, &plan).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("fe-webidl-callback-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let adapter_path = directory.join("adapter.mjs");
        let test_path = directory.join("test.mjs");
        std::fs::write(&adapter_path, adapter).unwrap();
        let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../demos/shared/host-runtime.js")
            .canonicalize()
            .unwrap();
        let script = format!(
            r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};

const runtime = createFeHostRuntime();
let installed;
const target = {{
  addEventListener(_type, callback) {{ installed = callback; }},
  removeEventListener() {{}},
}};
const targetHandle = runtime.resources.insert(target);
const hostEvent = {{ type: "message" }};
let borrowedHandle;
const callbackHandle = runtime.callbacks.register(
  "EventListener",
  async eventHandle => {{
    borrowedHandle = eventHandle;
    if (runtime.resources.borrow(eventHandle) !== hostEvent) throw new Error("wrong callback resource");
    await Promise.resolve();
    if (runtime.resources.borrow(eventHandle) !== hostEvent) throw new Error("borrow ended before Promise");
  }},
);
const adapter = createFeHostAdapter({{ interfaces: {{}} }}, runtime);
adapter.imports["fe:web"].event_target_add_event_listener(
  targetHandle,
  "message",
  callbackHandle,
  undefined,
);
await installed(hostEvent);
let stale = false;
try {{ runtime.resources.borrow(borrowedHandle); }}
catch (error) {{ stale = error.code === "stale_handle"; }}
if (!stale) throw new Error("callback resource handle escaped its borrow scope");
adapter.releaseCallback(callbackHandle);
runtime.resources.drop(targetHandle);
"#,
            adapter_url = format!("file://{}", adapter_path.display()),
            runtime_url = format!("file://{}", runtime_path.display()),
        );
        std::fs::write(&test_path, script).unwrap();
        let output = std::process::Command::new("bun")
            .arg("run")
            .arg(&test_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Bun callback integration failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn bun_iterator_resource_is_incremental_and_stale_after_drop() {
        if !std::process::Command::new("bun")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let world = parse("interface DOMTokenList { iterable<DOMString>; };").unwrap();
        let plan = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let create_import = plan.iterators[0].create_import.clone();
        let next_import = plan.iterators[0].next_import.clone();
        let adapter = emit_js_canonical_adapter(&world, &plan).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("fe-webidl-iterator-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let adapter_path = directory.join("adapter.mjs");
        let test_path = directory.join("test.mjs");
        std::fs::write(&adapter_path, adapter).unwrap();
        let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../demos/shared/host-runtime.js")
            .canonicalize()
            .unwrap();
        let script = format!(
            r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};

const runtime = createFeHostRuntime();
const collectionHandle = runtime.resources.insert(["alpha", "beta"]);
const adapter = createFeHostAdapter({{ interfaces: {{}}, namespaces: {{}} }}, runtime);
const imports = adapter.imports["fe:web"];
const iteratorHandle = imports[{create_import:?}](collectionHandle);
const first = imports[{next_import:?}](iteratorHandle);
const second = imports[{next_import:?}](iteratorHandle);
const done = imports[{next_import:?}](iteratorHandle);
if (first.ok !== "alpha" || second.ok !== "beta" || done.ok !== null)
  throw new Error("iterator was copied or completion was lost");
runtime.resources.drop(iteratorHandle);
let stale = false;
try {{ runtime.resources.borrow(iteratorHandle); }}
catch (error) {{ stale = error.code === "stale_handle"; }}
if (!stale) throw new Error("dropped iterator handle remained live");
const staleNext = imports[{next_import:?}](iteratorHandle);
if (typeof staleNext.error !== "string") throw new Error("stale next did not become owned error");
runtime.resources.drop(collectionHandle);
"#,
            adapter_url = format!("file://{}", adapter_path.display()),
            runtime_url = format!("file://{}", runtime_path.display()),
            create_import = create_import,
            next_import = next_import,
        );
        std::fs::write(&test_path, script).unwrap();
        let output = std::process::Command::new("bun")
            .arg("run")
            .arg(&test_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Bun iterator integration failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn bun_pair_iterator_validates_entries_and_preserves_lifecycle() {
        if !std::process::Command::new("bun")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let world = parse(
            "interface Registry { maplike<DOMString, unsigned long>; }; interface FeatureSet { setlike<DOMString>; };",
        )
        .unwrap();
        let plan = build_adapter_plan(&world, "web-fixture", "fe:web").unwrap();
        let registry_iterator = plan
            .iterators
            .iter()
            .find(|iterator| iterator.interface == "Registry")
            .unwrap();
        let create_import = registry_iterator.create_import.clone();
        let next_import = registry_iterator.next_import.clone();
        let registry = plan
            .collections
            .iter()
            .find(|collection| collection.interface == "Registry")
            .unwrap();
        let AdapterCollectionKind::MutableMaplike {
            get_import,
            set_import,
            delete_import: map_delete,
            clear_import: map_clear,
            ..
        } = &registry.kind
        else {
            panic!("expected mutable maplike");
        };
        let map_size = registry.size_import.clone();
        let map_has = registry.has_import.clone();
        let map_get = get_import.clone();
        let map_set = set_import.clone();
        let map_delete = map_delete.clone();
        let map_clear = map_clear.clone();
        let feature_set = plan
            .collections
            .iter()
            .find(|collection| collection.interface == "FeatureSet")
            .unwrap();
        let set_size = feature_set.size_import.clone();
        let set_has = feature_set.has_import.clone();
        let AdapterCollectionKind::MutableSetlike {
            add_import: set_add,
            delete_import: set_delete,
            clear_import: set_clear,
            ..
        } = &feature_set.kind
        else {
            panic!("expected mutable setlike");
        };
        let set_add = set_add.clone();
        let set_delete = set_delete.clone();
        let set_clear = set_clear.clone();
        let set_iterator = plan
            .iterators
            .iter()
            .find(|iterator| iterator.interface == "FeatureSet")
            .unwrap();
        let set_create = set_iterator.create_import.clone();
        let set_next = set_iterator.next_import.clone();
        let adapter = emit_js_canonical_adapter(&world, &plan).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "fe-webidl-pair-iterator-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let adapter_path = directory.join("adapter.mjs");
        let test_path = directory.join("test.mjs");
        std::fs::write(&adapter_path, adapter).unwrap();
        let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../demos/shared/host-runtime.js")
            .canonicalize()
            .unwrap();
        let script = format!(
            r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};

const runtime = createFeHostRuntime();
const adapter = createFeHostAdapter({{ interfaces: {{}}, namespaces: {{}} }}, runtime);
const imports = adapter.imports["fe:web"];
let collectionHandle = runtime.resources.insert(new Map([["first", 1], ["second", 2]]));
if (imports[{map_size:?}](collectionHandle) !== 2) throw new Error("maplike size changed");
if (!imports[{map_has:?}](collectionHandle, "second")) throw new Error("maplike has failed");
if (imports[{map_get:?}](collectionHandle, "first") !== 1) throw new Error("maplike get failed");
if (imports[{map_get:?}](collectionHandle, "missing") !== null) throw new Error("missing map key was not option-none");
const iteratorHandle = imports[{create_import:?}](collectionHandle);
const first = imports[{next_import:?}](iteratorHandle);
const oldCollectionHandle = collectionHandle;
const setResult = imports[{map_set:?}](collectionHandle, "third", 3);
if (typeof setResult.ok !== "object" || setResult.ok === null) throw new Error("map set did not return owned self");
collectionHandle = setResult.ok;
let oldStale = false;
try {{ runtime.resources.borrow(oldCollectionHandle); }}
catch (error) {{ oldStale = error.code === "stale_handle"; }}
if (!oldStale) throw new Error("fluent map mutation duplicated ownership");
if (imports[{map_delete:?}](collectionHandle, "second").ok !== true) throw new Error("map delete presence changed");
if (imports[{map_delete:?}](collectionHandle, "missing").ok !== false) throw new Error("map delete absence changed");
const second = imports[{next_import:?}](iteratorHandle);
const done = imports[{next_import:?}](iteratorHandle);
if (first.ok.key !== "first" || first.ok.value !== 1) throw new Error("first pair changed");
if (second.ok.key !== "third" || second.ok.value !== 3) throw new Error("live map iterator missed mutation");
if (done.ok !== null) throw new Error("pair completion was lost");
runtime.resources.drop(iteratorHandle);
let stale = false;
try {{ runtime.resources.borrow(iteratorHandle); }}
catch (error) {{ stale = error.code === "stale_handle"; }}
if (!stale) throw new Error("dropped pair iterator remained live");

let setHandle = runtime.resources.insert(new Set(["red", "blue"]));
if (imports[{set_size:?}](setHandle) !== 2) throw new Error("setlike size changed");
if (!imports[{set_has:?}](setHandle, "blue")) throw new Error("setlike has failed");
const setIterator = imports[{set_create:?}](setHandle);
if (imports[{set_next:?}](setIterator).ok !== "red") throw new Error("setlike order changed");
const addResult = imports[{set_add:?}](setHandle, "green");
if (typeof addResult.ok !== "object" || addResult.ok === null) throw new Error("set add did not return owned self");
setHandle = addResult.ok;
if (imports[{set_delete:?}](setHandle, "blue").ok !== true) throw new Error("set delete failed");
if (imports[{set_next:?}](setIterator).ok !== "green") throw new Error("live set iterator missed mutation");
if (imports[{set_next:?}](setIterator).ok !== null) throw new Error("setlike completion changed");
runtime.resources.drop(setIterator);
if (!("ok" in imports[{set_clear:?}](setHandle))) throw new Error("set clear lost unit success");
if (imports[{set_size:?}](setHandle) !== 0) throw new Error("set clear failed");

if (!("ok" in imports[{map_clear:?}](collectionHandle))) throw new Error("map clear lost unit success");
if (imports[{map_size:?}](collectionHandle) !== 0) throw new Error("map clear failed");

const malformed = {{ [Symbol.iterator]() {{ return {{ next() {{ return {{ done: false, value: ["only-key"] }}; }} }}; }} }};
const malformedHandle = runtime.resources.insert(malformed);
const malformedIterator = imports[{create_import:?}](malformedHandle);
const invalid = imports[{next_import:?}](malformedIterator);
if (typeof invalid.error !== "string" || !invalid.error.includes("two-element"))
  throw new Error("malformed pair was not an owned iterator error");
runtime.resources.drop(malformedIterator);
runtime.resources.drop(malformedHandle);

const invalidMutationHandle = runtime.resources.insert({{ set() {{ return {{}}; }} }});
const invalidMutation = imports[{map_set:?}](invalidMutationHandle, "key", 1);
if (typeof invalidMutation.error !== "string" || !invalidMutation.error.includes("return this"))
  throw new Error("mutation contract failure was not an owned error");
let consumedOnError = false;
try {{ runtime.resources.borrow(invalidMutationHandle); }}
catch (error) {{ consumedOnError = error.code === "stale_handle"; }}
if (!consumedOnError) throw new Error("failed owned mutation duplicated the receiver");

runtime.resources.drop(setHandle);
runtime.resources.drop(collectionHandle);
"#,
            adapter_url = format!("file://{}", adapter_path.display()),
            runtime_url = format!("file://{}", runtime_path.display()),
            create_import = create_import,
            next_import = next_import,
            map_size = map_size,
            map_has = map_has,
            map_get = map_get,
            map_set = map_set,
            map_delete = map_delete,
            map_clear = map_clear,
            set_size = set_size,
            set_has = set_has,
            set_add = set_add,
            set_delete = set_delete,
            set_clear = set_clear,
            set_create = set_create,
            set_next = set_next,
        );
        std::fs::write(&test_path, script).unwrap();
        let output = std::process::Command::new("bun")
            .arg("run")
            .arg(&test_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Bun pair iterator integration failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
