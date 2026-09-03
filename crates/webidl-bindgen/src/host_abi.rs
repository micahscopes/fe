//! Lowering from normalized Web IDL into the target-neutral host ABI.

use std::collections::BTreeSet;

use fe_host_abi as abi;

use crate::{
    ArgumentDef, BindgenError, BufferKind, CollectionKind, ConstructorDef, DefaultValueDef,
    ExtendedAttributesDef, Member, NamespaceMember, OperationDef, OperationSpecial, StringKind,
    TypeRef, World, inherited_dictionary_members,
};

/// Names that are transport policy rather than Web IDL semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAbiOptions {
    pub world_name: String,
}

impl HostAbiOptions {
    pub fn new(world_name: impl Into<String>) -> Self {
        Self {
            world_name: world_name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostAbiLowering {
    pub world: abi::World,
    pub resource_inheritance: Vec<ResourceInheritanceBinding>,
    pub iterators: Vec<IteratorBinding>,
    pub async_iterators: Vec<AsyncIteratorBinding>,
    pub defaults: Vec<DefaultBinding>,
    pub exposures: Vec<ExposureBinding>,
    pub variadics: Vec<VariadicBinding>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AsyncIteratorBinding {
    pub interface: String,
    pub resource: String,
    pub item: IteratorItemBinding,
    pub token_owner: AsyncIteratorTokenOwner,
    pub cancellation: AsyncIteratorCancellation,
    pub backpressure: AsyncIteratorBackpressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncIteratorTokenOwner {
    CallerRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncIteratorCancellation {
    OwnedSubscription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncIteratorBackpressure {
    SequentialOneInFlight,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IteratorBinding {
    pub interface: String,
    pub resource: String,
    pub item: IteratorItemBinding,
    /// Optional protocol methods. Plain Web IDL `iterable<T>` exposes none;
    /// future mutable collection lowerings must opt in explicitly.
    pub mutations: Vec<IteratorMutation>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IteratorItemBinding {
    Value(TypeRef),
    Entry {
        record: String,
        key: TypeRef,
        value: TypeRef,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IteratorMutation {
    MapSet,
    SetAdd,
    Delete,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceInheritanceBinding {
    pub resource: String,
    pub parent: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultBinding {
    pub path: String,
    pub value: DefaultValueDef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposureBinding {
    pub definition: String,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariadicBinding {
    pub callable: String,
    pub parameter: String,
}

/// Lower the ABI plus Web IDL adapter metadata which has no representation in
/// the stable host ABI core. The metadata is required to apply defaults,
/// exposure gates, and variadic spreading without changing the core ABI model.
pub fn lower_host_abi_with_metadata(
    world: &World,
    options: &HostAbiOptions,
) -> Result<HostAbiLowering, BindgenError> {
    let mut abi_world = world.clone();
    let mut defaults = Vec::new();
    let mut exposures = Vec::new();
    let mut variadics = Vec::new();

    for dictionary in abi_world.dictionaries.values_mut() {
        for member in &mut dictionary.members {
            if let Some(value) = member.default_.take() {
                defaults.push(DefaultBinding {
                    path: format!("dictionary/{}/{}", dictionary.name, member.name),
                    value,
                });
                // Web IDL dictionary conversion materializes a defaulted
                // member before the value crosses the normalized host ABI.
                // It is therefore a concrete record field, not an Option.
                member.required = true;
            }
        }
    }
    for callback in abi_world.callbacks.values_mut() {
        collect_argument_metadata(
            &format!("callback/{}", callback.name),
            &mut callback.arguments,
            &mut defaults,
            &mut variadics,
        );
        if callback.attributes != ExtendedAttributesDef::default() {
            exposures.push(ExposureBinding {
                definition: format!("callback/{}", callback.name),
                attributes: callback.attributes.clone(),
            });
        }
    }
    for interface in abi_world.interfaces.values_mut() {
        if interface.attributes != ExtendedAttributesDef::default() {
            exposures.push(ExposureBinding {
                definition: format!("interface/{}", interface.name),
                attributes: interface.attributes.clone(),
            });
        }
        for member in &mut interface.members {
            match member {
                Member::Const(_) => {}
                Member::Collection(collection) => {
                    if collection.attributes != ExtendedAttributesDef::default() {
                        exposures.push(ExposureBinding {
                            definition: format!(
                                "interface/{}/{}",
                                interface.name,
                                crate::collection_kind_name(&collection.kind)
                            ),
                            attributes: collection.attributes.clone(),
                        });
                    }
                    if let CollectionKind::AsyncIterable { arguments, .. } = &mut collection.kind {
                        collect_argument_metadata(
                            &format!("interface/{}/async-iterable", interface.name),
                            arguments,
                            &mut defaults,
                            &mut variadics,
                        );
                    }
                }
                Member::Constructor(constructor) => {
                    let base = constructor.name.as_deref().unwrap_or("constructor");
                    let label = if constructor.overload > 0 {
                        format!("{base}-{}", constructor.overload)
                    } else {
                        base.to_owned()
                    };
                    let path = format!("interface/{}/{label}", interface.name);
                    collect_argument_metadata(
                        &path,
                        &mut constructor.arguments,
                        &mut defaults,
                        &mut variadics,
                    );
                    if constructor.attributes != ExtendedAttributesDef::default() {
                        exposures.push(ExposureBinding {
                            definition: path,
                            attributes: constructor.attributes.clone(),
                        });
                    }
                }
                Member::Attribute(attribute) => {
                    if attribute.attributes != ExtendedAttributesDef::default() {
                        exposures.push(ExposureBinding {
                            definition: format!(
                                "interface/{}/attribute/{}",
                                interface.name, attribute.name
                            ),
                            attributes: attribute.attributes.clone(),
                        });
                    }
                }
                Member::Operation(operation) => {
                    let operation_path = format!("interface/{}/{}", interface.name, operation.name);
                    collect_argument_metadata(
                        &operation_path,
                        &mut operation.arguments,
                        &mut defaults,
                        &mut variadics,
                    );
                    if operation.attributes != ExtendedAttributesDef::default() {
                        exposures.push(ExposureBinding {
                            definition: operation_path,
                            attributes: operation.attributes.clone(),
                        });
                    }
                }
            }
        }
    }
    for namespace in abi_world.namespaces.values_mut() {
        if namespace.attributes != ExtendedAttributesDef::default() {
            exposures.push(ExposureBinding {
                definition: format!("namespace/{}", namespace.name),
                attributes: namespace.attributes.clone(),
            });
        }
        for member in &mut namespace.members {
            match member {
                NamespaceMember::Const(_) => {}
                NamespaceMember::Attribute(attribute) => {
                    if attribute.attributes != ExtendedAttributesDef::default() {
                        exposures.push(ExposureBinding {
                            definition: format!(
                                "namespace/{}/attribute/{}",
                                namespace.name, attribute.name
                            ),
                            attributes: attribute.attributes.clone(),
                        });
                    }
                }
                NamespaceMember::Operation(operation) => {
                    let suffix = if operation.overload > 0 {
                        format!("-{}", operation.overload)
                    } else {
                        String::new()
                    };
                    let path = format!("namespace/{}/{}{suffix}", namespace.name, operation.name);
                    collect_argument_metadata(
                        &path,
                        &mut operation.arguments,
                        &mut defaults,
                        &mut variadics,
                    );
                    if operation.attributes != ExtendedAttributesDef::default() {
                        exposures.push(ExposureBinding {
                            definition: path,
                            attributes: operation.attributes.clone(),
                        });
                    }
                }
            }
        }
    }
    defaults.sort_by(|left, right| left.path.cmp(&right.path));
    exposures.sort_by(|left, right| left.definition.cmp(&right.definition));
    variadics.sort_by(|left, right| {
        (&left.callable, &left.parameter).cmp(&(&right.callable, &right.parameter))
    });
    let resource_inheritance = abi_world
        .interfaces
        .values()
        .filter_map(|interface| {
            interface
                .inherits
                .as_ref()
                .map(|parent| ResourceInheritanceBinding {
                    resource: interface.name.clone(),
                    parent: parent.clone(),
                })
        })
        .collect();
    let iterators = abi_world
        .interfaces
        .values()
        .filter_map(|interface| {
            interface.members.iter().find_map(|member| {
                let Member::Collection(collection) = member else {
                    return None;
                };
                match &collection.kind {
                    CollectionKind::Iterable { key, value } => Some(IteratorBinding {
                        interface: interface.name.clone(),
                        resource: format!("{}Iterator", interface.name),
                        item: match key {
                            None => IteratorItemBinding::Value(value.clone()),
                            Some(key) => IteratorItemBinding::Entry {
                                record: format!("{}IteratorEntry", interface.name),
                                key: key.clone(),
                                value: value.clone(),
                            },
                        },
                        mutations: Vec::new(),
                    }),
                    CollectionKind::Maplike {
                        key,
                        value,
                        read_only,
                    } => Some(IteratorBinding {
                        interface: interface.name.clone(),
                        resource: format!("{}Iterator", interface.name),
                        item: IteratorItemBinding::Entry {
                            record: format!("{}IteratorEntry", interface.name),
                            key: key.clone(),
                            value: value.clone(),
                        },
                        mutations: if *read_only {
                            Vec::new()
                        } else {
                            vec![
                                IteratorMutation::MapSet,
                                IteratorMutation::Delete,
                                IteratorMutation::Clear,
                            ]
                        },
                    }),
                    CollectionKind::Setlike { value, read_only } => Some(IteratorBinding {
                        interface: interface.name.clone(),
                        resource: format!("{}Iterator", interface.name),
                        item: IteratorItemBinding::Value(value.clone()),
                        mutations: if *read_only {
                            Vec::new()
                        } else {
                            vec![
                                IteratorMutation::SetAdd,
                                IteratorMutation::Delete,
                                IteratorMutation::Clear,
                            ]
                        },
                    }),
                    CollectionKind::AsyncIterable { .. } => None,
                }
            })
        })
        .collect();
    let async_iterators = abi_world
        .interfaces
        .values()
        .filter_map(|interface| {
            interface.members.iter().find_map(|member| {
                let Member::Collection(collection) = member else {
                    return None;
                };
                let CollectionKind::AsyncIterable { key, value, .. } = &collection.kind else {
                    return None;
                };
                Some(AsyncIteratorBinding {
                    interface: interface.name.clone(),
                    resource: format!("{}AsyncIterator", interface.name),
                    item: match key {
                        None => IteratorItemBinding::Value(value.clone()),
                        Some(key) => IteratorItemBinding::Entry {
                            record: format!("{}AsyncIteratorEntry", interface.name),
                            key: key.clone(),
                            value: value.clone(),
                        },
                    },
                    token_owner: AsyncIteratorTokenOwner::CallerRuntime,
                    cancellation: AsyncIteratorCancellation::OwnedSubscription,
                    backpressure: AsyncIteratorBackpressure::SequentialOneInFlight,
                })
            })
        })
        .collect();
    Ok(HostAbiLowering {
        world: lower_host_abi(&abi_world, options)?,
        resource_inheritance,
        iterators,
        async_iterators,
        defaults,
        exposures,
        variadics,
    })
}

fn collect_argument_metadata(
    callable: &str,
    arguments: &mut [ArgumentDef],
    defaults: &mut Vec<DefaultBinding>,
    variadics: &mut Vec<VariadicBinding>,
) {
    for argument in arguments {
        if let Some(value) = argument.default_.take() {
            defaults.push(DefaultBinding {
                path: format!("{callable}/{}", argument.name),
                value,
            });
        }
        if argument.variadic {
            variadics.push(VariadicBinding {
                callable: callable.to_owned(),
                parameter: argument.name.clone(),
            });
        }
    }
}

/// Lower a linked Web IDL world into the generic host ABI model.
///
/// This does not choose a Wasm, JavaScript, component-model, or native
/// transport. If the host model cannot retain an IDL distinction, lowering
/// fails with the definition/member path instead of guessing.
pub fn lower_host_abi(world: &World, options: &HostAbiOptions) -> Result<abi::World, BindgenError> {
    let mut types = Vec::new();

    let unions = collect_world_unions(world);
    for (fingerprint, union) in &unions {
        let name = union_name(fingerprint);
        if world.typedefs.contains_key(&name)
            || world.enums.contains_key(&name)
            || world.dictionaries.contains_key(&name)
            || world.callbacks.contains_key(&name)
            || world.interfaces.contains_key(&name)
        {
            return Err(BindgenError::new(
                format!("anonymous union `{fingerprint}`"),
                format!("generated stable name `{name}` collides with an IDL definition"),
            ));
        }
        let TypeRef::Union(members) = union else {
            unreachable!("union registry only contains unions");
        };
        let mut case_names = BTreeSet::new();
        let cases = members
            .iter()
            .map(|member| {
                let case_name = union_case_name(member);
                if !case_names.insert(case_name.clone()) {
                    return Err(BindgenError::new(
                        format!("anonymous union `{fingerprint}`"),
                        format!("ambiguous duplicate case identity `{case_name}`"),
                    ));
                }
                Ok(abi::Case {
                    name: case_name,
                    payload: Some(lower_value_type(
                        world,
                        member,
                        TypePosition::Nested,
                        &format!("anonymous union `{fingerprint}`"),
                    )?),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        types.push(abi::TypeDef {
            name,
            kind: abi::TypeDefKind::Variant { cases },
        });
    }

    for typedef in world.typedefs.values() {
        let target = lower_value_type(
            world,
            &typedef.type_,
            TypePosition::Nested,
            &format!("typedef `{}`", typedef.name),
        )?;
        types.push(abi::TypeDef {
            name: typedef.name.clone(),
            kind: abi::TypeDefKind::Alias { target },
        });
    }
    for enum_ in world.enums.values() {
        types.push(abi::TypeDef {
            name: enum_.name.clone(),
            kind: abi::TypeDefKind::Enum {
                cases: enum_.values.clone(),
            },
        });
    }
    for dictionary in world.dictionaries.values() {
        let members = inherited_dictionary_members(world, dictionary)?;
        let fields = members
            .into_iter()
            .map(|member| {
                let context = format!("dictionary `{}` member `{}`", dictionary.name, member.name);
                if member.default_.is_some() {
                    return Err(BindgenError::new(
                        context,
                        "default values are not retained by the normalized host ABI",
                    ));
                }
                let mut type_ =
                    lower_value_type(world, &member.type_, TypePosition::Nested, &context)?;
                if !member.required {
                    type_ = abi::Type::Option(Box::new(type_));
                }
                Ok(abi::Field {
                    name: member.name.clone(),
                    type_,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        types.push(abi::TypeDef {
            name: dictionary.name.clone(),
            kind: abi::TypeDefKind::Record { fields },
        });
    }
    for callback in world.callbacks.values() {
        let context = format!("callback `{}`", callback.name);
        let params = callback
            .arguments
            .iter()
            .map(|argument| lower_argument(world, &context, argument))
            .collect::<Result<Vec<_>, _>>()?;
        let (result, async_) = match &callback.result {
            TypeRef::Unit => (None, false),
            TypeRef::Promise(payload) => {
                let result = match payload.as_ref() {
                    TypeRef::Unit => None,
                    payload => Some(lower_value_type(
                        world,
                        payload,
                        TypePosition::Result,
                        &context,
                    )?),
                };
                (result, true)
            }
            result => (
                Some(lower_value_type(
                    world,
                    result,
                    TypePosition::Result,
                    &context,
                )?),
                false,
            ),
        };
        types.push(abi::TypeDef {
            name: callback.name.clone(),
            kind: abi::TypeDefKind::Callback {
                signature: abi::FunctionType {
                    params,
                    result,
                    async_,
                },
            },
        });
    }
    for interface in world.interfaces.values() {
        let Some(collection) = interface.members.iter().find_map(|member| match member {
            Member::Collection(collection) => Some(&collection.kind),
            _ => None,
        }) else {
            continue;
        };
        let (key, value) = match collection {
            CollectionKind::Iterable {
                key: Some(key),
                value,
            }
            | CollectionKind::Maplike { key, value, .. } => (key, value),
            CollectionKind::Iterable { key: None, .. }
            | CollectionKind::AsyncIterable { .. }
            | CollectionKind::Setlike { .. } => continue,
        };
        let name = format!("{}IteratorEntry", interface.name);
        if world.typedefs.contains_key(&name)
            || world.enums.contains_key(&name)
            || world.dictionaries.contains_key(&name)
            || world.callbacks.contains_key(&name)
            || world.interfaces.contains_key(&name)
            || world.namespaces.contains_key(&name)
        {
            return Err(BindgenError::new(
                format!("interface `{}` iterable", interface.name),
                format!("generated iterator entry record `{name}` collides with an IDL definition"),
            ));
        }
        let context = format!("interface `{}` iterable entry", interface.name);
        types.push(abi::TypeDef {
            name,
            kind: abi::TypeDefKind::Record {
                fields: vec![
                    abi::Field {
                        name: "key".to_owned(),
                        type_: lower_value_type(world, key, TypePosition::Result, &context)?,
                    },
                    abi::Field {
                        name: "value".to_owned(),
                        type_: lower_value_type(world, value, TypePosition::Result, &context)?,
                    },
                ],
            },
        });
    }
    types.sort_by(|left, right| left.name.cmp(&right.name));

    let mut resources = Vec::new();
    let mut iterator_resources = Vec::new();
    for interface in world.interfaces.values() {
        let mut methods = Vec::new();
        for member in &interface.members {
            match member {
                Member::Const(_) => {}
                Member::Collection(collection) => match &collection.kind {
                    CollectionKind::Iterable { key, value } => {
                        let item = match key {
                            None => lower_value_type(
                                world,
                                value,
                                TypePosition::Result,
                                &format!("interface `{}` iterable item", interface.name),
                            )?,
                            Some(_) => abi::Type::Named(format!("{}IteratorEntry", interface.name)),
                        };
                        let (create, iterator) = lower_iterator_protocol(&interface.name, item);
                        methods.push(create);
                        iterator_resources.push(iterator);
                    }
                    CollectionKind::Maplike {
                        key,
                        value,
                        read_only,
                    } => {
                        let context = format!("interface `{}` readonly maplike", interface.name);
                        let key_param =
                            lower_value_type(world, key, TypePosition::Param, &context)?;
                        let value_result =
                            lower_value_type(world, value, TypePosition::Result, &context)?;
                        methods.extend([
                            abi::ResourceMethod {
                                name: "collection-get".to_owned(),
                                receiver: abi::Receiver::Borrow,
                                signature: abi::FunctionType {
                                    params: vec![abi::Param {
                                        name: "key".to_owned(),
                                        type_: key_param.clone(),
                                    }],
                                    result: Some(abi::Type::Option(Box::new(value_result))),
                                    async_: false,
                                },
                            },
                            abi::ResourceMethod {
                                name: "collection-has".to_owned(),
                                receiver: abi::Receiver::Borrow,
                                signature: abi::FunctionType {
                                    params: vec![abi::Param {
                                        name: "key".to_owned(),
                                        type_: key_param,
                                    }],
                                    result: Some(abi::Type::Bool),
                                    async_: false,
                                },
                            },
                            collection_size_method(),
                        ]);
                        let (create, iterator) = lower_iterator_protocol(
                            &interface.name,
                            abi::Type::Named(format!("{}IteratorEntry", interface.name)),
                        );
                        methods.push(create);
                        iterator_resources.push(iterator);
                        if !*read_only {
                            let own_self = abi::Type::Handle(abi::Handle {
                                resource: interface.name.clone(),
                                ownership: abi::HandleOwnership::Own,
                            });
                            methods.extend([
                                mutation_method(
                                    "collection-set",
                                    abi::Receiver::Own,
                                    vec![
                                        abi::Param {
                                            name: "key".to_owned(),
                                            type_: lower_value_type(
                                                world,
                                                key,
                                                TypePosition::Param,
                                                &context,
                                            )?,
                                        },
                                        abi::Param {
                                            name: "value".to_owned(),
                                            type_: lower_value_type(
                                                world,
                                                value,
                                                TypePosition::Param,
                                                &context,
                                            )?,
                                        },
                                    ],
                                    Some(own_self),
                                ),
                                mutation_method(
                                    "collection-delete",
                                    abi::Receiver::Borrow,
                                    vec![abi::Param {
                                        name: "key".to_owned(),
                                        type_: lower_value_type(
                                            world,
                                            key,
                                            TypePosition::Param,
                                            &context,
                                        )?,
                                    }],
                                    Some(abi::Type::Bool),
                                ),
                                mutation_method(
                                    "collection-clear",
                                    abi::Receiver::Borrow,
                                    Vec::new(),
                                    None,
                                ),
                            ]);
                        }
                    }
                    CollectionKind::Setlike { value, read_only } => {
                        let context = format!("interface `{}` readonly setlike", interface.name);
                        methods.extend([
                            abi::ResourceMethod {
                                name: "collection-has".to_owned(),
                                receiver: abi::Receiver::Borrow,
                                signature: abi::FunctionType {
                                    params: vec![abi::Param {
                                        name: "value".to_owned(),
                                        type_: lower_value_type(
                                            world,
                                            value,
                                            TypePosition::Param,
                                            &context,
                                        )?,
                                    }],
                                    result: Some(abi::Type::Bool),
                                    async_: false,
                                },
                            },
                            collection_size_method(),
                        ]);
                        let item = lower_value_type(world, value, TypePosition::Result, &context)?;
                        let (create, iterator) = lower_iterator_protocol(&interface.name, item);
                        methods.push(create);
                        iterator_resources.push(iterator);
                        if !*read_only {
                            methods.extend([
                                mutation_method(
                                    "collection-add",
                                    abi::Receiver::Own,
                                    vec![abi::Param {
                                        name: "value".to_owned(),
                                        type_: lower_value_type(
                                            world,
                                            value,
                                            TypePosition::Param,
                                            &context,
                                        )?,
                                    }],
                                    Some(abi::Type::Handle(abi::Handle {
                                        resource: interface.name.clone(),
                                        ownership: abi::HandleOwnership::Own,
                                    })),
                                ),
                                mutation_method(
                                    "collection-delete",
                                    abi::Receiver::Borrow,
                                    vec![abi::Param {
                                        name: "value".to_owned(),
                                        type_: lower_value_type(
                                            world,
                                            value,
                                            TypePosition::Param,
                                            &context,
                                        )?,
                                    }],
                                    Some(abi::Type::Bool),
                                ),
                                mutation_method(
                                    "collection-clear",
                                    abi::Receiver::Borrow,
                                    Vec::new(),
                                    None,
                                ),
                            ]);
                        }
                    }
                    CollectionKind::AsyncIterable {
                        key,
                        value,
                        arguments,
                    } => {
                        let context = format!("interface `{}` async iterable", interface.name);
                        let item = match key {
                            None => lower_value_type(world, value, TypePosition::Result, &context)?,
                            Some(key) => {
                                let record = format!("{}AsyncIteratorEntry", interface.name);
                                types.push(abi::TypeDef {
                                    name: record.clone(),
                                    kind: abi::TypeDefKind::Record {
                                        fields: vec![
                                            abi::Field {
                                                name: "key".to_owned(),
                                                type_: lower_value_type(
                                                    world,
                                                    key,
                                                    TypePosition::Result,
                                                    &context,
                                                )?,
                                            },
                                            abi::Field {
                                                name: "value".to_owned(),
                                                type_: lower_value_type(
                                                    world,
                                                    value,
                                                    TypePosition::Result,
                                                    &context,
                                                )?,
                                            },
                                        ],
                                    },
                                });
                                abi::Type::Named(record)
                            }
                        };
                        let params = arguments
                            .iter()
                            .map(|argument| lower_argument(world, &context, argument))
                            .collect::<Result<Vec<_>, _>>()?;
                        let (create, iterator) =
                            lower_async_iterator_protocol(&interface.name, params, item);
                        methods.push(create);
                        iterator_resources.push(iterator);
                    }
                },
                Member::Constructor(constructor) => {
                    methods.push(lower_constructor(world, &interface.name, constructor)?);
                }
                Member::Attribute(attribute) => {
                    if attribute.stringifier {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` stringifier attribute `{}`",
                                interface.name, attribute.name
                            ),
                            "stringifier coercion semantics are not representable in the current host ABI",
                        ));
                    }
                    let context = format!(
                        "interface `{}` attribute `{}`",
                        interface.name, attribute.name
                    );
                    let result =
                        lower_value_type(world, &attribute.type_, TypePosition::Result, &context)?;
                    methods.push(abi::ResourceMethod {
                        name: format!("get-{}", attribute.name),
                        receiver: receiver(attribute.static_ || interface.attributes.global),
                        signature: abi::FunctionType {
                            params: Vec::new(),
                            result: Some(result.clone()),
                            async_: false,
                        },
                    });
                    if !attribute.read_only {
                        methods.push(abi::ResourceMethod {
                            name: format!("set-{}", attribute.name),
                            receiver: receiver(attribute.static_ || interface.attributes.global),
                            signature: abi::FunctionType {
                                params: vec![abi::Param {
                                    name: "value".to_owned(),
                                    type_: lower_value_type(
                                        world,
                                        &attribute.type_,
                                        TypePosition::Param,
                                        &context,
                                    )?,
                                }],
                                result: None,
                                async_: false,
                            },
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
                                    Some(&candidate.type_)
                                }
                                _ => None,
                            })
                            .expect("validated PutForwards member");
                        methods.push(abi::ResourceMethod {
                            name: format!("set-{}", attribute.name),
                            receiver: abi::Receiver::Borrow,
                            signature: abi::FunctionType {
                                params: vec![abi::Param {
                                    name: "value".to_owned(),
                                    type_: lower_value_type(
                                        world,
                                        forwarded_type,
                                        TypePosition::Param,
                                        &context,
                                    )?,
                                }],
                                result: None,
                                async_: false,
                            },
                        });
                    }
                }
                Member::Operation(operation) => {
                    if operation.special != OperationSpecial::Regular {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` {}",
                                interface.name,
                                crate::operation_special_name(operation.special)
                            ),
                            "property/index/string coercion semantics are not representable in the current host ABI",
                        ));
                    }
                    methods.push(lower_operation(
                        world,
                        &format!("interface `{}`", interface.name),
                        operation,
                        interface.attributes.global,
                    )?);
                }
            }
        }
        if !interface.attributes.global {
            methods.push(abi::ResourceMethod {
                name: "resource-drop".to_owned(),
                receiver: abi::Receiver::Own,
                signature: abi::FunctionType {
                    params: Vec::new(),
                    result: None,
                    async_: false,
                },
            });
        }
        methods.sort_by(|left, right| left.name.cmp(&right.name));
        resources.push(abi::Resource {
            name: interface.name.clone(),
            methods,
        });
    }
    resources.append(&mut iterator_resources);
    resources.sort_by(|left, right| left.name.cmp(&right.name));

    let mut imports = Vec::new();
    for namespace in world.namespaces.values() {
        for member in &namespace.members {
            match member {
                NamespaceMember::Const(_) => {}
                NamespaceMember::Attribute(attribute) => {
                    let context = format!(
                        "namespace `{}` attribute `{}`",
                        namespace.name, attribute.name
                    );
                    imports.push(abi::Function {
                        namespace: namespace.name.clone(),
                        name: format!("get-{}", attribute.name),
                        signature: abi::FunctionType {
                            params: Vec::new(),
                            result: Some(lower_value_type(
                                world,
                                &attribute.type_,
                                TypePosition::Result,
                                &context,
                            )?),
                            async_: false,
                        },
                    });
                }
                NamespaceMember::Operation(operation) => {
                    let method = lower_operation(
                        world,
                        &format!("namespace `{}`", namespace.name),
                        operation,
                        true,
                    )?;
                    imports.push(abi::Function {
                        namespace: namespace.name.clone(),
                        name: method.name,
                        signature: method.signature,
                    });
                }
            }
        }
    }
    imports
        .sort_by(|left, right| (&left.namespace, &left.name).cmp(&(&right.namespace, &right.name)));

    let lowered = abi::World {
        name: options.world_name.clone(),
        types,
        resources,
        imports,
        exports: Vec::new(),
    };
    lowered.validate().map_err(|error| {
        BindgenError::new(
            format!("lower host ABI `{}`", options.world_name),
            error.to_string(),
        )
    })?;
    Ok(lowered)
}

fn collection_size_method() -> abi::ResourceMethod {
    abi::ResourceMethod {
        name: "collection-size".to_owned(),
        receiver: abi::Receiver::Borrow,
        signature: abi::FunctionType {
            params: Vec::new(),
            result: Some(abi::Type::U32),
            async_: false,
        },
    }
}

fn mutation_method(
    name: &str,
    receiver: abi::Receiver,
    params: Vec<abi::Param>,
    ok: Option<abi::Type>,
) -> abi::ResourceMethod {
    abi::ResourceMethod {
        name: name.to_owned(),
        receiver,
        signature: abi::FunctionType {
            params,
            result: Some(abi::Type::Result(abi::ResultType {
                ok: ok.map(Box::new),
                error: Some(Box::new(abi::Type::String(abi::StringEncoding::Utf8))),
            })),
            async_: false,
        },
    }
}

fn lower_iterator_protocol(
    interface: &str,
    item: abi::Type,
) -> (abi::ResourceMethod, abi::Resource) {
    let iterator_name = format!("{interface}Iterator");
    (
        abi::ResourceMethod {
            name: "iterator".to_owned(),
            receiver: abi::Receiver::Borrow,
            signature: abi::FunctionType {
                params: Vec::new(),
                result: Some(abi::Type::Handle(abi::Handle {
                    resource: iterator_name.clone(),
                    ownership: abi::HandleOwnership::Own,
                })),
                async_: false,
            },
        },
        abi::Resource {
            name: iterator_name,
            methods: vec![abi::ResourceMethod {
                name: "next".to_owned(),
                receiver: abi::Receiver::Borrow,
                signature: abi::FunctionType {
                    params: Vec::new(),
                    result: Some(abi::Type::Result(abi::ResultType {
                        ok: Some(Box::new(abi::Type::Option(Box::new(item)))),
                        error: Some(Box::new(abi::Type::String(abi::StringEncoding::Utf8))),
                    })),
                    async_: false,
                },
            }],
        },
    )
}

fn lower_async_iterator_protocol(
    interface: &str,
    params: Vec<abi::Param>,
    item: abi::Type,
) -> (abi::ResourceMethod, abi::Resource) {
    let iterator_name = format!("{interface}AsyncIterator");
    (
        abi::ResourceMethod {
            name: "async-iterator".to_owned(),
            receiver: abi::Receiver::Borrow,
            signature: abi::FunctionType {
                params,
                result: Some(abi::Type::Handle(abi::Handle {
                    resource: iterator_name.clone(),
                    ownership: abi::HandleOwnership::Own,
                })),
                async_: false,
            },
        },
        abi::Resource {
            name: iterator_name,
            methods: vec![abi::ResourceMethod {
                name: "next".to_owned(),
                receiver: abi::Receiver::Borrow,
                signature: abi::FunctionType {
                    params: Vec::new(),
                    result: Some(abi::Type::Result(abi::ResultType {
                        ok: Some(Box::new(abi::Type::Option(Box::new(item)))),
                        error: Some(Box::new(abi::Type::String(abi::StringEncoding::Utf8))),
                    })),
                    async_: true,
                },
            }],
        },
    )
}

fn lower_constructor(
    world: &World,
    interface: &str,
    constructor: &ConstructorDef,
) -> Result<abi::ResourceMethod, BindgenError> {
    let base = match &constructor.name {
        Some(name) => format!("named-constructor-{name}"),
        None => "constructor".to_owned(),
    };
    let name = if constructor.overload > 0 {
        format!("{base}-{}", constructor.overload)
    } else {
        base
    };
    let context = format!("interface `{interface}` constructor `{name}`");
    let params = constructor
        .arguments
        .iter()
        .map(|argument| lower_argument(world, &context, argument))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(abi::ResourceMethod {
        name,
        receiver: abi::Receiver::Static,
        signature: abi::FunctionType {
            params,
            result: Some(abi::Type::Handle(abi::Handle {
                resource: interface.to_owned(),
                ownership: abi::HandleOwnership::Own,
            })),
            async_: false,
        },
    })
}

fn lower_operation(
    world: &World,
    definition: &str,
    operation: &OperationDef,
    force_static: bool,
) -> Result<abi::ResourceMethod, BindgenError> {
    let suffix = if operation.overload > 0 {
        format!("-{}", operation.overload)
    } else {
        String::new()
    };
    let name = format!("{}{suffix}", operation.name);
    let context = format!("{definition} operation `{name}`");
    let params = operation
        .arguments
        .iter()
        .map(|argument| lower_argument(world, &context, argument))
        .collect::<Result<Vec<_>, _>>()?;

    let (result, async_) = match &operation.result {
        TypeRef::Unit => (None, false),
        TypeRef::Promise(payload) => {
            let result = match payload.as_ref() {
                TypeRef::Unit => None,
                payload => Some(lower_value_type(
                    world,
                    payload,
                    TypePosition::Result,
                    &context,
                )?),
            };
            (result, true)
        }
        result => (
            Some(lower_value_type(
                world,
                result,
                TypePosition::Result,
                &context,
            )?),
            false,
        ),
    };
    Ok(abi::ResourceMethod {
        name,
        receiver: receiver(operation.static_ || force_static),
        signature: abi::FunctionType {
            params,
            result,
            async_,
        },
    })
}

fn lower_argument(
    world: &World,
    operation: &str,
    argument: &ArgumentDef,
) -> Result<abi::Param, BindgenError> {
    let context = format!("{operation} argument `{}`", argument.name);
    if argument.default_.is_some() {
        return Err(BindgenError::new(
            context,
            "argument default values are not retained by the normalized host ABI",
        ));
    }
    let mut type_ = lower_value_type(world, &argument.type_, TypePosition::Param, &context)?;
    if argument.variadic {
        type_ = abi::Type::List(Box::new(type_));
    } else if argument.optional {
        type_ = abi::Type::Option(Box::new(type_));
    }
    Ok(abi::Param {
        name: argument.name.clone(),
        type_,
    })
}

#[derive(Clone, Copy)]
enum TypePosition {
    Param,
    Result,
    ParamNested,
    ResultNested,
    Nested,
}

fn lower_value_type(
    world: &World,
    type_: &TypeRef,
    position: TypePosition,
    context: &str,
) -> Result<abi::Type, BindgenError> {
    let lowered = match type_ {
        TypeRef::Unit => {
            return Err(BindgenError::new(
                context,
                "`undefined` is only representable as an operation result",
            ));
        }
        TypeRef::Bool => abi::Type::Bool,
        TypeRef::I8 => abi::Type::I8,
        TypeRef::U8 => abi::Type::U8,
        TypeRef::I16 => abi::Type::I16,
        TypeRef::U16 => abi::Type::U16,
        TypeRef::I32 => abi::Type::I32,
        TypeRef::U32 => abi::Type::U32,
        TypeRef::I64 => abi::Type::I64,
        TypeRef::U64 => abi::Type::U64,
        TypeRef::F32 => abi::Type::F32,
        TypeRef::F64 => abi::Type::F64,
        TypeRef::String(StringKind::Byte) => abi::Type::String(abi::StringEncoding::Latin1),
        TypeRef::String(StringKind::Dom) => abi::Type::String(abi::StringEncoding::Utf16),
        TypeRef::String(StringKind::Usv) => abi::Type::String(abi::StringEncoding::Utf8),
        TypeRef::Named(name) if world.interfaces.contains_key(name) => {
            let ownership = match position {
                TypePosition::Param => abi::HandleOwnership::Borrow,
                TypePosition::Result | TypePosition::ResultNested => abi::HandleOwnership::Own,
                TypePosition::ParamNested => {
                    return Err(BindgenError::new(
                        context,
                        format!(
                            "borrowed resource `{name}` is nested in a parameter, which the host ABI cannot express"
                        ),
                    ));
                }
                TypePosition::Nested => {
                    return Err(BindgenError::new(
                        context,
                        format!(
                            "resource `{name}` appears in a reusable value type without an ownership direction"
                        ),
                    ));
                }
            };
            abi::Type::Handle(abi::Handle {
                resource: name.clone(),
                ownership,
            })
        }
        TypeRef::Named(name)
            if world.typedefs.contains_key(name)
                || world.enums.contains_key(name)
                || world.dictionaries.contains_key(name)
                || world.callbacks.contains_key(name) =>
        {
            abi::Type::Named(name.clone())
        }
        TypeRef::Named(name) => {
            return Err(BindgenError::new(
                context,
                format!("unknown named Web IDL type `{name}`"),
            ));
        }
        TypeRef::Nullable(inner) => abi::Type::Option(Box::new(lower_value_type(
            world,
            inner,
            nested_position(position),
            context,
        )?)),
        TypeRef::Sequence(inner) => abi::Type::List(Box::new(lower_value_type(
            world,
            inner,
            nested_position(position),
            context,
        )?)),
        TypeRef::Buffer(kind) => abi::Type::Buffer(lower_buffer(*kind, position, context)?),
        TypeRef::Promise(_) => {
            return Err(BindgenError::new(
                context,
                "`Promise` is only representable as an operation result",
            ));
        }
        TypeRef::Record(_) => {
            return Err(BindgenError::new(
                context,
                "Web IDL record key type was erased by the current normalized model",
            ));
        }
        TypeRef::Union(_) => abi::Type::Named(union_name(&type_fingerprint(type_))),
        TypeRef::Any | TypeRef::Object | TypeRef::Symbol | TypeRef::Error => {
            return Err(BindgenError::new(
                context,
                format!("Web IDL type `{type_:?}` has no target-neutral host ABI semantics"),
            ));
        }
    };
    Ok(lowered)
}

fn collect_world_unions(world: &World) -> std::collections::BTreeMap<String, TypeRef> {
    let mut unions = std::collections::BTreeMap::new();
    let mut collect = |type_: &TypeRef| collect_unions(type_, &mut unions);
    for typedef in world.typedefs.values() {
        collect(&typedef.type_);
    }
    for dictionary in world.dictionaries.values() {
        for member in &dictionary.members {
            collect(&member.type_);
        }
    }
    for callback in world.callbacks.values() {
        for argument in &callback.arguments {
            collect(&argument.type_);
        }
        collect(&callback.result);
    }
    for interface in world.interfaces.values() {
        for member in &interface.members {
            match member {
                Member::Const(_) => {}
                Member::Collection(collection) => match &collection.kind {
                    CollectionKind::Iterable { key, value }
                    | CollectionKind::AsyncIterable { key, value, .. } => {
                        if let Some(key) = key {
                            collect(key);
                        }
                        collect(value);
                    }
                    CollectionKind::Maplike { key, value, .. } => {
                        collect(key);
                        collect(value);
                    }
                    CollectionKind::Setlike { value, .. } => collect(value),
                },
                Member::Constructor(constructor) => {
                    for argument in &constructor.arguments {
                        collect(&argument.type_);
                    }
                }
                Member::Attribute(attribute) => collect(&attribute.type_),
                Member::Operation(operation) => {
                    for argument in &operation.arguments {
                        collect(&argument.type_);
                    }
                    collect(&operation.result);
                }
            }
        }
    }
    for namespace in world.namespaces.values() {
        for member in &namespace.members {
            match member {
                NamespaceMember::Const(_) => {}
                NamespaceMember::Attribute(attribute) => collect(&attribute.type_),
                NamespaceMember::Operation(operation) => {
                    for argument in &operation.arguments {
                        collect(&argument.type_);
                    }
                    collect(&operation.result);
                }
            }
        }
    }
    unions
}

fn collect_unions(type_: &TypeRef, unions: &mut std::collections::BTreeMap<String, TypeRef>) {
    match type_ {
        TypeRef::Union(members) => {
            unions
                .entry(type_fingerprint(type_))
                .or_insert_with(|| type_.clone());
            for member in members {
                collect_unions(member, unions);
            }
        }
        TypeRef::Nullable(inner)
        | TypeRef::Sequence(inner)
        | TypeRef::Promise(inner)
        | TypeRef::Record(inner) => collect_unions(inner, unions),
        _ => {}
    }
}

fn type_fingerprint(type_: &TypeRef) -> String {
    match type_ {
        TypeRef::Unit => "unit".to_owned(),
        TypeRef::Bool => "bool".to_owned(),
        TypeRef::I8 => "i8".to_owned(),
        TypeRef::U8 => "u8".to_owned(),
        TypeRef::I16 => "i16".to_owned(),
        TypeRef::U16 => "u16".to_owned(),
        TypeRef::I32 => "i32".to_owned(),
        TypeRef::U32 => "u32".to_owned(),
        TypeRef::I64 => "i64".to_owned(),
        TypeRef::U64 => "u64".to_owned(),
        TypeRef::F32 => "f32".to_owned(),
        TypeRef::F64 => "f64".to_owned(),
        TypeRef::String(kind) => format!("string-{kind:?}"),
        TypeRef::Named(name) => format!("named-{name}"),
        TypeRef::Nullable(inner) => format!("nullable-{}", type_fingerprint(inner)),
        TypeRef::Sequence(inner) => format!("sequence-{}", type_fingerprint(inner)),
        TypeRef::Promise(inner) => format!("promise-{}", type_fingerprint(inner)),
        TypeRef::Record(inner) => format!("record-{}", type_fingerprint(inner)),
        TypeRef::Union(members) => format!(
            "union-{}",
            members
                .iter()
                .map(type_fingerprint)
                .collect::<Vec<_>>()
                .join("-or-")
        ),
        TypeRef::Buffer(kind) => format!("buffer-{kind:?}"),
        TypeRef::Any => "any".to_owned(),
        TypeRef::Object => "object".to_owned(),
        TypeRef::Symbol => "symbol".to_owned(),
        TypeRef::Error => "error".to_owned(),
    }
}

fn union_name(fingerprint: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in fingerprint.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("webidl-union-{hash:016x}")
}

pub(crate) fn stable_union_name(type_: &TypeRef) -> String {
    union_name(&type_fingerprint(type_))
}

fn union_case_name(type_: &TypeRef) -> String {
    type_fingerprint(type_)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

pub(crate) fn stable_union_case_name(type_: &TypeRef) -> String {
    union_case_name(type_)
}

fn nested_position(position: TypePosition) -> TypePosition {
    match position {
        TypePosition::Param | TypePosition::ParamNested => TypePosition::ParamNested,
        TypePosition::Result | TypePosition::ResultNested => TypePosition::ResultNested,
        TypePosition::Nested => TypePosition::Nested,
    }
}

fn lower_buffer(
    kind: BufferKind,
    position: TypePosition,
    context: &str,
) -> Result<abi::Buffer, BindgenError> {
    let element = match kind {
        BufferKind::ArrayBuffer => abi::BufferElement::U8,
        BufferKind::I8 => abi::BufferElement::I8,
        BufferKind::U8 | BufferKind::U8Clamped => abi::BufferElement::U8,
        BufferKind::I16 => abi::BufferElement::I16,
        BufferKind::U16 => abi::BufferElement::U16,
        BufferKind::I32 => abi::BufferElement::I32,
        BufferKind::U32 => abi::BufferElement::U32,
        BufferKind::F32 => abi::BufferElement::F32,
        BufferKind::F64 => abi::BufferElement::F64,
        BufferKind::ArrayBufferView | BufferKind::BufferSource | BufferKind::DataView => {
            return Err(BindgenError::new(
                context,
                format!(
                    "`{kind:?}` does not identify a single element representation for the host ABI"
                ),
            ));
        }
    };
    Ok(abi::Buffer {
        element,
        ownership: match position {
            TypePosition::Param => abi::BufferOwnership::Borrow,
            TypePosition::Result | TypePosition::ResultNested => abi::BufferOwnership::Own,
            TypePosition::ParamNested => {
                return Err(BindgenError::new(
                    context,
                    "a borrowed buffer nested in a parameter is not representable by the host ABI",
                ));
            }
            TypePosition::Nested => {
                return Err(BindgenError::new(
                    context,
                    "a buffer in a reusable named value type has no ownership direction",
                ));
            }
        },
    })
}

fn receiver(static_: bool) -> abi::Receiver {
    if static_ {
        abi::Receiver::Static
    } else {
        abi::Receiver::Borrow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn lower(source: &str) -> Result<abi::World, BindgenError> {
        let world = parse(source)?;
        lower_host_abi(&world, &HostAbiOptions::new("web-test"))
    }

    #[test]
    fn lowers_named_values_resources_async_and_buffers_deterministically() {
        let lowered = lower(
            r#"
                typedef unsigned long Identifier;
                enum Direction { "up", "down-left" };
                dictionary Point {
                    Identifier id;
                    required long x;
                };
                interface mixin Named {
                    readonly attribute DOMString label;
                };
                interface Registry {
                    attribute boolean active;
                    undefined draw(long y, long x);
                    Promise<DOMString> lookup(optional Identifier id);
                    undefined replace(Registry next);
                    Uint8Array snapshot();
                    undefined write(Uint8Array bytes);
                    static undefined clear();
                };
                Registry includes Named;
            "#,
        )
        .unwrap();

        lowered.validate().unwrap();
        assert_eq!(
            lowered
                .types
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["Direction", "Identifier", "Point"]
        );
        assert_eq!(
            lowered.types[0].kind,
            abi::TypeDefKind::Enum {
                cases: vec!["up".to_owned(), "down-left".to_owned()]
            }
        );
        assert_eq!(
            lowered.types[2].kind,
            abi::TypeDefKind::Record {
                fields: vec![
                    abi::Field {
                        name: "id".to_owned(),
                        type_: abi::Type::Option(Box::new(abi::Type::Named(
                            "Identifier".to_owned()
                        ))),
                    },
                    abi::Field {
                        name: "x".to_owned(),
                        type_: abi::Type::I32,
                    },
                ],
            }
        );

        let registry = &lowered.resources[0];
        assert_eq!(registry.name, "Registry");
        assert_eq!(
            registry
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            [
                "clear",
                "draw",
                "get-active",
                "get-label",
                "lookup",
                "replace",
                "resource-drop",
                "set-active",
                "snapshot",
                "write",
            ]
        );
        let draw = registry
            .methods
            .iter()
            .find(|method| method.name == "draw")
            .unwrap();
        assert_eq!(
            draw.signature
                .params
                .iter()
                .map(|param| param.name.as_str())
                .collect::<Vec<_>>(),
            ["y", "x"]
        );
        let lookup = registry
            .methods
            .iter()
            .find(|method| method.name == "lookup")
            .unwrap();
        assert!(lookup.signature.async_);
        assert_eq!(
            lookup.signature.result,
            Some(abi::Type::String(abi::StringEncoding::Utf16))
        );
        assert_eq!(
            lookup.signature.params[0].type_,
            abi::Type::Option(Box::new(abi::Type::Named("Identifier".to_owned())))
        );
        let replace = registry
            .methods
            .iter()
            .find(|method| method.name == "replace")
            .unwrap();
        assert_eq!(
            replace.signature.params[0].type_,
            abi::Type::Handle(abi::Handle {
                resource: "Registry".to_owned(),
                ownership: abi::HandleOwnership::Borrow,
            })
        );
        let snapshot = registry
            .methods
            .iter()
            .find(|method| method.name == "snapshot")
            .unwrap();
        assert_eq!(
            snapshot.signature.result,
            Some(abi::Type::Buffer(abi::Buffer {
                element: abi::BufferElement::U8,
                ownership: abi::BufferOwnership::Own,
            }))
        );
        let write = registry
            .methods
            .iter()
            .find(|method| method.name == "write")
            .unwrap();
        assert_eq!(
            write.signature.params[0].type_,
            abi::Type::Buffer(abi::Buffer {
                element: abi::BufferElement::U8,
                ownership: abi::BufferOwnership::Borrow,
            })
        );
    }

    #[test]
    fn flattens_dictionary_inheritance_but_rejects_shadowing() {
        let lowered = lower(
            r#"
                dictionary Position { required long x; };
                dictionary Point : Position { required long y; };
            "#,
        )
        .unwrap();
        assert_eq!(
            lowered.types[0].kind,
            abi::TypeDefKind::Record {
                fields: vec![
                    abi::Field {
                        name: "x".to_owned(),
                        type_: abi::Type::I32,
                    },
                    abi::Field {
                        name: "y".to_owned(),
                        type_: abi::Type::I32,
                    },
                ]
            }
        );

        let error = lower(
            r#"
                dictionary Position { required long x; };
                dictionary Point : Position { required long x; };
            "#,
        )
        .unwrap_err();
        assert!(error.context.contains("dictionary `Point` member `x`"));
        assert!(error.detail.contains("shadows"));
    }

    #[test]
    fn rejects_web_idl_semantics_the_host_model_cannot_retain() {
        let cases = [
            ("dictionary Config { long size = 4; };", "default values"),
            (
                "interface Value { record<DOMString, long> read(); };",
                "key type was erased",
            ),
            (
                "interface Node { undefined attach(Node? parent); };",
                "nested in a parameter",
            ),
            (
                "interface Writer { undefined write(sequence<Uint8Array> chunks); };",
                "buffer nested in a parameter",
            ),
            (
                "typedef Uint8Array Bytes; interface Writer { undefined write(Bytes bytes); };",
                "no ownership direction",
            ),
            (
                "interface Config { undefined set(optional long size = 4); };",
                "argument default values",
            ),
        ];
        for (source, expected) in cases {
            let error = lower(source).unwrap_err();
            assert!(
                error.detail.contains(expected),
                "expected {expected:?} in {error}"
            );
        }
    }

    #[test]
    fn overload_names_and_output_are_stable() {
        let source = r#"
            interface Convert {
                long value(long input);
                DOMString value(DOMString input);
            };
        "#;
        let first = lower(source).unwrap();
        let second = lower(source).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.resources[0]
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            ["resource-drop", "value", "value-1"]
        );
    }

    #[test]
    fn lowers_callbacks_variadic_tails_and_stable_anonymous_unions() {
        let source = r#"
            callback Mapper = Promise<DOMString> (long value);
            callback interface EventListener {
                undefined handleEvent(long event);
            };
            interface Console {
                undefined log(DOMString... values);
                (long or DOMString) parse(DOMString value);
            };
        "#;
        let first = lower(source).unwrap();
        let second = lower(source).unwrap();
        assert_eq!(first, second);

        let mapper = first
            .types
            .iter()
            .find(|definition| definition.name == "Mapper")
            .unwrap();
        let abi::TypeDefKind::Callback { signature } = &mapper.kind else {
            panic!("Mapper should lower as callback");
        };
        assert!(signature.async_);
        assert_eq!(
            signature.result,
            Some(abi::Type::String(abi::StringEncoding::Utf16))
        );

        let listener = first
            .types
            .iter()
            .find(|definition| definition.name == "EventListener")
            .unwrap();
        assert!(matches!(listener.kind, abi::TypeDefKind::Callback { .. }));

        let console = &first.resources[0];
        let log = console
            .methods
            .iter()
            .find(|method| method.name == "log")
            .unwrap();
        assert_eq!(
            log.signature.params[0].type_,
            abi::Type::List(Box::new(abi::Type::String(abi::StringEncoding::Utf16)))
        );
        let parse = console
            .methods
            .iter()
            .find(|method| method.name == "parse")
            .unwrap();
        let Some(abi::Type::Named(union_name)) = &parse.signature.result else {
            panic!("union result should reference a synthetic variant");
        };
        let union = first
            .types
            .iter()
            .find(|definition| definition.name == *union_name)
            .unwrap();
        let abi::TypeDefKind::Variant { cases } = &union.kind else {
            panic!("synthetic union should be a variant");
        };
        assert_eq!(
            cases
                .iter()
                .map(|case| case.name.as_str())
                .collect::<Vec<_>>(),
            ["i32", "string-dom"]
        );
    }

    #[test]
    fn adapter_metadata_retains_defaults_exposure_and_variadic_semantics() {
        let world = parse(
            r#"
                [Exposed=(Window,Worker), SecureContext]
                interface Console {
                    undefined configure(
                        optional DOMString mode = "fast",
                        DOMString... values
                    );
                };
                dictionary Config {
                    boolean enabled = true;
                };
            "#,
        )
        .unwrap();
        assert!(lower_host_abi(&world, &HostAbiOptions::new("web-test")).is_err());
        let lowered =
            lower_host_abi_with_metadata(&world, &HostAbiOptions::new("web-test")).unwrap();
        lowered.world.validate().unwrap();
        assert_eq!(
            lowered.defaults,
            [
                DefaultBinding {
                    path: "dictionary/Config/enabled".to_owned(),
                    value: DefaultValueDef::Bool(true),
                },
                DefaultBinding {
                    path: "interface/Console/configure/mode".to_owned(),
                    value: DefaultValueDef::String("fast".to_owned()),
                },
            ]
        );
        assert_eq!(
            lowered.exposures,
            [ExposureBinding {
                definition: "interface/Console".to_owned(),
                attributes: ExtendedAttributesDef {
                    exposed: Some(vec!["Window".to_owned(), "Worker".to_owned()]),
                    secure_context: true,
                    same_object: false,
                    legacy_unforgeable: false,
                    put_forwards: None,
                    global: false,
                    unmodeled: Vec::new(),
                },
            }]
        );
        assert_eq!(
            lowered.variadics,
            [VariadicBinding {
                callable: "interface/Console/configure".to_owned(),
                parameter: "values".to_owned(),
            }]
        );
    }
}
