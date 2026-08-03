//! Standards-driven Web IDL binding generation for Fe.
//!
//! This crate is deliberately independent of Fe's compiler internals. It turns
//! Web IDL into a normalized interface model and emits ordinary Fe `extern`
//! declarations plus JavaScript import adapters. Higher-level `std::web` and
//! FRP libraries are separate consumers.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use weedle::{
    Definition, Parse,
    argument::Argument,
    attribute::{ExtendedAttribute, ExtendedAttributeList, IdentifierOrString},
    dictionary::DictionaryMember,
    interface::{
        AsyncIterableInterfaceMember, InterfaceMember, IterableInterfaceMember, Special,
        StringifierOrInheritOrStatic, StringifierOrStatic,
    },
    literal::{ConstValue, DefaultValue, FloatLit, IntegerLit},
    mixin::MixinMember,
    namespace::NamespaceMember as WeedleNamespaceMember,
    types::{
        ConstType, FloatingPointType, IntegerType, NonAnyType, ReturnType, SingleType, Type,
        UnionMemberType,
    },
};

mod adapter_plan;
mod host_abi;
mod selection;
mod transport_plan;

pub use adapter_plan::{
    AdapterAsyncIterator, AdapterCallback, AdapterCollection, AdapterCollectionKind,
    AdapterFunction, AdapterInvocation, AdapterIterator, AdapterNamespace, AdapterParam,
    AdapterPlan, AdapterResource, build_adapter_plan, emit_js_canonical_adapter,
    emit_js_selected_adapter,
};
pub use host_abi::{
    AsyncIteratorBackpressure, AsyncIteratorBinding, AsyncIteratorCancellation,
    AsyncIteratorTokenOwner, DefaultBinding, ExposureBinding, HostAbiLowering, HostAbiOptions,
    IteratorBinding, IteratorItemBinding, IteratorMutation, ResourceInheritanceBinding,
    VariadicBinding, lower_host_abi, lower_host_abi_with_metadata,
};
pub use selection::{
    ADAPTER_SELECTION_VERSION, AdapterOperationMetadata, AdapterSelectionError,
    AdapterSelectionManifest, ImportKind, RequiredImport, adapter_operation_metadata,
    select_adapter_operations,
};
pub use transport_plan::{
    CallbackTransport, CoreSignature, CoreValueType, FutureTransport, MemorySurfacePlan,
    TransportFunction, TransportKind, TransportPlan, build_transport_plan,
    emit_js_core_wasm_transport,
};

/// A linked, deterministic subset of one Web IDL definition graph.
#[derive(Debug, Clone, PartialEq)]
pub struct World {
    pub interfaces: BTreeMap<String, InterfaceDef>,
    pub namespaces: BTreeMap<String, NamespaceDef>,
    pub typedefs: BTreeMap<String, TypedefDef>,
    pub enums: BTreeMap<String, EnumDef>,
    pub dictionaries: BTreeMap<String, DictionaryDef>,
    pub mixins: BTreeMap<String, MixinDef>,
    /// Included mixin names per interface, in statement order.
    pub includes: BTreeMap<String, Vec<String>>,
    pub callbacks: BTreeMap<String, CallbackDef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamespaceDef {
    pub name: String,
    pub members: Vec<NamespaceMember>,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NamespaceMember {
    Attribute(AttributeDef),
    Operation(OperationDef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDef {
    pub name: String,
    pub inherits: Option<String>,
    pub members: Vec<Member>,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtendedAttributesDef {
    /// `None` means the IDL omitted `[Exposed]`; an explicit list preserves
    /// declaration order.
    pub exposed: Option<Vec<String>>,
    pub secure_context: bool,
    pub same_object: bool,
    pub legacy_unforgeable: bool,
    pub put_forwards: Option<String>,
    /// Attribute names whose semantics are not modeled by this rung. Keeping
    /// them visible prevents generators from silently claiming full fidelity.
    pub unmodeled: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallbackDef {
    pub name: String,
    pub arguments: Vec<ArgumentDef>,
    pub result: TypeRef,
    /// Present for legacy callback-interface declarations such as
    /// `EventListener.handleEvent`.
    pub interface_operation: Option<String>,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedefDef {
    pub name: String,
    pub type_: TypeRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DictionaryDef {
    pub name: String,
    pub inherits: Option<String>,
    pub members: Vec<DictionaryMemberDef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DictionaryMemberDef {
    pub name: String,
    pub type_: TypeRef,
    pub required: bool,
    pub default_: Option<DefaultValueDef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MixinDef {
    pub name: String,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Member {
    Const(ConstDef),
    Constructor(ConstructorDef),
    Collection(CollectionDef),
    Attribute(AttributeDef),
    Operation(OperationDef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollectionDef {
    pub kind: CollectionKind,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CollectionKind {
    Iterable {
        key: Option<TypeRef>,
        value: TypeRef,
    },
    AsyncIterable {
        key: Option<TypeRef>,
        value: TypeRef,
        arguments: Vec<ArgumentDef>,
    },
    Maplike {
        key: TypeRef,
        value: TypeRef,
        read_only: bool,
    },
    Setlike {
        value: TypeRef,
        read_only: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDef {
    pub name: String,
    pub type_: TypeRef,
    pub value: DefaultValueDef,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorDef {
    /// `None` is the interface's default constructor. Legacy
    /// `[NamedConstructor=Name(...)]` declarations retain `Name`.
    pub name: Option<String>,
    pub arguments: Vec<ArgumentDef>,
    /// Zero-based index within the default or same-named overload set.
    pub overload: usize,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttributeDef {
    pub name: String,
    pub type_: TypeRef,
    pub read_only: bool,
    pub static_: bool,
    pub stringifier: bool,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationDef {
    pub name: String,
    pub arguments: Vec<ArgumentDef>,
    pub result: TypeRef,
    pub static_: bool,
    pub special: OperationSpecial,
    /// Zero-based index within an overload set, assigned deterministically.
    pub overload: usize,
    pub attributes: ExtendedAttributesDef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OperationSpecial {
    Regular,
    Getter,
    Setter,
    Deleter,
    LegacyCaller,
    Stringifier,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArgumentDef {
    pub name: String,
    pub type_: TypeRef,
    pub optional: bool,
    pub default_: Option<DefaultValueDef>,
    pub variadic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultValueDef {
    Bool(bool),
    Integer(String),
    Float(String),
    String(String),
    Null,
    EmptySequence,
    EmptyDictionary,
}

/// Web IDL values before ABI lowering. Unsupported values stay explicit and
/// cause a generation error rather than silently degrading to JavaScript `any`.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    Unit,
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    String(StringKind),
    Named(String),
    Nullable(Box<TypeRef>),
    Sequence(Box<TypeRef>),
    Promise(Box<TypeRef>),
    Record(Box<TypeRef>),
    Union(Vec<TypeRef>),
    Buffer(BufferKind),
    Any,
    Object,
    Symbol,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringKind {
    Byte,
    Dom,
    Usv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferKind {
    ArrayBuffer,
    ArrayBufferView,
    BufferSource,
    DataView,
    I8,
    U8,
    U8Clamped,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindgenError {
    pub context: String,
    pub detail: String,
}

impl BindgenError {
    fn new(context: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for BindgenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.detail)
    }
}

impl std::error::Error for BindgenError {}

/// Parse and link one Web IDL source.
///
/// Non-partial interfaces establish identity and inheritance; partial
/// interfaces are merged in source order. The final map is name-sorted so code
/// generation does not depend on hash iteration.
pub fn parse(source: &str) -> Result<World, BindgenError> {
    // weedle2 5.0.0 does not accept Web IDL's `Exposed=*` token even though it
    // models ordinary exposure identifiers. Preserve the standards spelling
    // semantically through a parser-only identifier.
    let wildcard_normalized;
    let source = if source.contains("[Exposed=*]") {
        wildcard_normalized = source.replace("[Exposed=*]", "[Exposed=FeExposedWildcard]");
        wildcard_normalized.as_str()
    } else {
        source
    };
    // Use the root parser directly because weedle2's convenience function
    // asserts when unsupported syntax remains unparsed.
    let (remaining, ast) = weedle::Definitions::parse(source)
        .map_err(|error| BindgenError::new("parse Web IDL", format!("{error:?}")))?;
    if !remaining.trim().is_empty() {
        return Err(BindgenError::new(
            "parse Web IDL",
            format!("unsupported syntax begins at `{}`", remaining.trim()),
        ));
    }
    let mut interfaces = BTreeMap::<String, InterfaceDef>::new();
    let mut namespaces = BTreeMap::<String, NamespaceDef>::new();
    let mut typedefs = BTreeMap::<String, TypedefDef>::new();
    let mut enums = BTreeMap::<String, EnumDef>::new();
    let mut dictionaries = BTreeMap::<String, DictionaryDef>::new();
    let mut mixins = BTreeMap::<String, MixinDef>::new();
    let mut callbacks = BTreeMap::<String, CallbackDef>::new();
    let mut interface_partials = Vec::new();
    let mut namespace_partials = Vec::new();
    let mut dictionary_partials = Vec::new();
    let mut mixin_partials = Vec::new();
    let mut include_statements = Vec::new();

    for definition in ast {
        match definition {
            Definition::Interface(interface) => {
                let name = interface.identifier.0.to_owned();
                if interfaces.contains_key(&name) {
                    return Err(BindgenError::new(
                        format!("interface `{name}`"),
                        "duplicate non-partial definition",
                    ));
                }
                let (attributes, mut legacy_constructors) =
                    normalize_interface_attributes(interface.attributes, &name)?;
                let mut members = normalize_members(&name, interface.members.body)?;
                legacy_constructors.append(&mut members);
                interfaces.insert(
                    name.clone(),
                    InterfaceDef {
                        name,
                        inherits: interface
                            .inheritance
                            .map(|inheritance| inheritance.identifier.0.to_owned()),
                        members: legacy_constructors,
                        attributes,
                    },
                );
            }
            Definition::PartialInterface(interface) => {
                interface_partials
                    .push((interface.identifier.0.to_owned(), interface.members.body));
            }
            Definition::Typedef(typedef) => {
                let name = typedef.identifier.0.to_owned();
                insert_unique(
                    &mut typedefs,
                    name.clone(),
                    TypedefDef {
                        name,
                        type_: normalize_type(typedef.type_.type_),
                    },
                    "typedef",
                )?;
            }
            Definition::Enum(enum_) => {
                let name = enum_.identifier.0.to_owned();
                let values = enum_
                    .values
                    .body
                    .list
                    .into_iter()
                    .map(|variant| variant.value.0.to_owned())
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    return Err(BindgenError::new(
                        format!("enum `{name}`"),
                        "must contain at least one value",
                    ));
                }
                let mut unique = BTreeSet::new();
                if let Some(value) = values.iter().find(|value| !unique.insert(*value)) {
                    return Err(BindgenError::new(
                        format!("enum `{name}`"),
                        format!("duplicate value `{value}`"),
                    ));
                }
                insert_unique(&mut enums, name.clone(), EnumDef { name, values }, "enum")?;
            }
            Definition::Dictionary(dictionary) => {
                let name = dictionary.identifier.0.to_owned();
                let definition = DictionaryDef {
                    name: name.clone(),
                    inherits: dictionary
                        .inheritance
                        .map(|inheritance| inheritance.identifier.0.to_owned()),
                    members: normalize_dictionary_members(&name, dictionary.members.body)?,
                };
                insert_unique(&mut dictionaries, name, definition, "dictionary")?;
            }
            Definition::PartialDictionary(dictionary) => {
                dictionary_partials
                    .push((dictionary.identifier.0.to_owned(), dictionary.members.body));
            }
            Definition::InterfaceMixin(mixin) => {
                let name = mixin.identifier.0.to_owned();
                let definition = MixinDef {
                    name: name.clone(),
                    members: normalize_mixin_members(&name, mixin.members.body)?,
                };
                insert_unique(&mut mixins, name, definition, "interface mixin")?;
            }
            Definition::PartialInterfaceMixin(mixin) => {
                mixin_partials.push((mixin.identifier.0.to_owned(), mixin.members.body));
            }
            Definition::IncludesStatement(statement) => {
                include_statements.push((
                    statement.lhs_identifier.0.to_owned(),
                    statement.rhs_identifier.0.to_owned(),
                ));
            }
            Definition::Callback(callback) => {
                let name = callback.identifier.0.to_owned();
                let definition = CallbackDef {
                    name: name.clone(),
                    arguments: normalize_arguments(callback.arguments.body.list),
                    result: normalize_return_type(callback.return_type),
                    interface_operation: None,
                    attributes: normalize_extended_attributes(callback.attributes, "callback")?,
                };
                insert_unique(&mut callbacks, name, definition, "callback")?;
            }
            Definition::CallbackInterface(interface) => {
                let name = interface.identifier.0.to_owned();
                if interface.inheritance.is_some() {
                    return Err(BindgenError::new(
                        format!("callback interface `{name}`"),
                        "callback-interface inheritance is not representable as one callback signature",
                    ));
                }
                let members = normalize_members(&name, interface.members.body)?;
                let mut operations = members.iter().filter_map(|member| match member {
                    Member::Operation(operation) => Some(operation),
                    Member::Const(_)
                    | Member::Constructor(_)
                    | Member::Collection(_)
                    | Member::Attribute(_) => None,
                });
                let Some(operation) = operations.next() else {
                    return Err(BindgenError::new(
                        format!("callback interface `{name}`"),
                        "must contain exactly one regular operation",
                    ));
                };
                if operations.next().is_some()
                    || members.iter().any(|member| {
                        matches!(
                            member,
                            Member::Constructor(_) | Member::Collection(_) | Member::Attribute(_)
                        )
                    })
                {
                    return Err(BindgenError::new(
                        format!("callback interface `{name}`"),
                        "must contain exactly one regular operation; constants are the only additional supported members",
                    ));
                }
                let definition = CallbackDef {
                    name: name.clone(),
                    arguments: operation.arguments.clone(),
                    result: operation.result.clone(),
                    interface_operation: Some(operation.name.clone()),
                    attributes: normalize_extended_attributes(
                        interface.attributes,
                        "callback interface",
                    )?,
                };
                insert_unique(&mut callbacks, name, definition, "callback interface")?;
            }
            Definition::Namespace(namespace) => {
                let name = namespace.identifier.0.to_owned();
                let definition = NamespaceDef {
                    name: name.clone(),
                    members: normalize_namespace_members(&name, namespace.members.body)?,
                    attributes: normalize_extended_attributes(
                        namespace.attributes,
                        &format!("namespace `{name}`"),
                    )?,
                };
                insert_unique(&mut namespaces, name, definition, "namespace")?;
            }
            Definition::PartialNamespace(namespace) => {
                namespace_partials
                    .push((namespace.identifier.0.to_owned(), namespace.members.body));
            }
            Definition::Implements(_) => {
                return Err(BindgenError::new(
                    "link Web IDL",
                    format!(
                        "definition kind `{}` is not implemented yet",
                        definition_kind(&definition)
                    ),
                ));
            }
        }
    }

    for (name, members) in namespace_partials {
        let normalized = normalize_namespace_members(&name, members)?;
        let namespace = namespaces.get_mut(&name).ok_or_else(|| {
            BindgenError::new(
                format!("partial namespace `{name}`"),
                "has no non-partial definition",
            )
        })?;
        namespace.members.extend(normalized);
    }

    for (name, members) in interface_partials {
        let normalized = normalize_members(&name, members)?;
        if normalized
            .iter()
            .any(|member| matches!(member, Member::Constructor(_)))
        {
            return Err(BindgenError::new(
                format!("partial interface `{name}` constructor"),
                "Web IDL constructors must be declared on the non-partial interface",
            ));
        }
        let interface = interfaces.get_mut(&name).ok_or_else(|| {
            BindgenError::new(
                format!("partial interface `{name}`"),
                "has no non-partial definition",
            )
        })?;
        interface.members.extend(normalized);
    }

    for (name, members) in dictionary_partials {
        let normalized = normalize_dictionary_members(&name, members)?;
        let dictionary = dictionaries.get_mut(&name).ok_or_else(|| {
            BindgenError::new(
                format!("partial dictionary `{name}`"),
                "has no non-partial definition",
            )
        })?;
        dictionary.members.extend(normalized);
        validate_dictionary_member_names(dictionary)?;
    }

    for (name, members) in mixin_partials {
        let normalized = normalize_mixin_members(&name, members)?;
        let mixin = mixins.get_mut(&name).ok_or_else(|| {
            BindgenError::new(
                format!("partial interface mixin `{name}`"),
                "has no non-partial definition",
            )
        })?;
        mixin.members.extend(normalized);
    }

    let mut includes = BTreeMap::<String, Vec<String>>::new();
    for (interface_name, mixin_name) in include_statements {
        if !interfaces.contains_key(&interface_name) {
            return Err(BindgenError::new(
                format!("includes `{interface_name} includes {mixin_name}`"),
                format!("target `{interface_name}` is not an interface"),
            ));
        }
        let mixin = mixins.get(&mixin_name).ok_or_else(|| {
            BindgenError::new(
                format!("includes `{interface_name} includes {mixin_name}`"),
                format!("source `{mixin_name}` is not an interface mixin"),
            )
        })?;
        let target_includes = includes.entry(interface_name.clone()).or_default();
        if target_includes.contains(&mixin_name) {
            return Err(BindgenError::new(
                format!("includes `{interface_name} includes {mixin_name}`"),
                "duplicate includes statement",
            ));
        }
        target_includes.push(mixin_name.clone());
        interfaces
            .get_mut(&interface_name)
            .expect("validated above")
            .members
            .extend(mixin.members.clone());
    }

    for interface in interfaces.values() {
        if let Some(parent) = &interface.inherits
            && !interfaces.contains_key(parent)
        {
            return Err(BindgenError::new(
                format!("interface `{}`", interface.name),
                format!("inherits unknown interface `{parent}`"),
            ));
        }
    }
    for dictionary in dictionaries.values() {
        validate_dictionary_member_names(dictionary)?;
        if let Some(parent) = &dictionary.inherits
            && !dictionaries.contains_key(parent)
        {
            return Err(BindgenError::new(
                format!("dictionary `{}`", dictionary.name),
                format!("inherits unknown dictionary `{parent}`"),
            ));
        }
    }

    validate_inheritance_cycles(&interfaces, "interface")?;
    validate_inheritance_cycles(&dictionaries, "dictionary")?;
    validate_typedef_cycles(&typedefs)?;
    validate_definition_names(
        &interfaces,
        &namespaces,
        &typedefs,
        &enums,
        &dictionaries,
        &mixins,
        &callbacks,
    )?;

    assign_overload_indexes(&mut interfaces)?;
    assign_namespace_overload_indexes(&mut namespaces)?;
    validate_inherited_member_collisions(&interfaces)?;
    let world = World {
        interfaces,
        namespaces,
        typedefs,
        enums,
        dictionaries,
        mixins,
        includes,
        callbacks,
    };
    validate_typed_defaults(&world)?;
    validate_special_operations(&world)?;
    validate_property_policies(&world)?;
    Ok(world)
}

fn validate_property_policies(world: &World) -> Result<(), BindgenError> {
    for interface in world.interfaces.values() {
        for member in &interface.members {
            let Member::Attribute(attribute) = member else {
                continue;
            };
            let context = format!(
                "interface `{}` attribute `{}`",
                interface.name, attribute.name
            );
            if attribute.attributes.same_object && (!attribute.read_only || attribute.static_) {
                return Err(BindgenError::new(
                    &context,
                    "`[SameObject]` requires a non-static readonly attribute",
                ));
            }
            if attribute.attributes.legacy_unforgeable && attribute.static_ {
                return Err(BindgenError::new(
                    &context,
                    "`[LegacyUnforgeable]` is not valid on a static attribute",
                ));
            }
            let Some(forwarded) = &attribute.attributes.put_forwards else {
                continue;
            };
            if !attribute.read_only || attribute.static_ {
                return Err(BindgenError::new(
                    &context,
                    "`[PutForwards]` requires a non-static readonly attribute",
                ));
            }
            let TypeRef::Named(target_name) = &attribute.type_ else {
                return Err(BindgenError::new(
                    &context,
                    "`[PutForwards]` requires an interface-valued attribute",
                ));
            };
            let Some(target) = world.interfaces.get(target_name) else {
                return Err(BindgenError::new(
                    &context,
                    "`[PutForwards]` target type must be a linked interface",
                ));
            };
            if !target.members.iter().any(|member| {
                matches!(member, Member::Attribute(candidate) if candidate.name == *forwarded && !candidate.read_only)
            }) {
                return Err(BindgenError::new(
                    &context,
                    format!(
                        "`[PutForwards={forwarded}]` does not name a writable attribute on `{}`",
                        target.name
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn validate_inherited_member_collisions(
    interfaces: &BTreeMap<String, InterfaceDef>,
) -> Result<(), BindgenError> {
    for interface in interfaces.values() {
        let mut inherited_names = BTreeMap::<String, &'static str>::new();
        let mut inherited_operations = BTreeSet::new();
        let mut inherited_collection = None;
        let mut parent = interface.inherits.as_deref();
        while let Some(name) = parent {
            let ancestor = &interfaces[name];
            for member in &ancestor.members {
                match member {
                    Member::Const(constant) => {
                        inherited_names
                            .entry(constant.name.clone())
                            .or_insert("constant");
                    }
                    Member::Attribute(attribute) => {
                        inherited_names
                            .entry(attribute.name.clone())
                            .or_insert("attribute");
                    }
                    Member::Operation(operation) => {
                        inherited_operations.insert((
                            operation.name.clone(),
                            operation.special,
                            callable_signature(&operation.arguments),
                        ));
                        if operation.special == OperationSpecial::Regular {
                            inherited_names
                                .entry(operation.name.clone())
                                .or_insert("operation");
                        }
                    }
                    Member::Collection(collection) => {
                        inherited_collection.get_or_insert(collection_kind_name(&collection.kind));
                    }
                    Member::Constructor(_) => {}
                }
            }
            parent = ancestor.inherits.as_deref();
        }

        for member in &interface.members {
            match member {
                Member::Const(constant) => {
                    reject_inherited_name(interface, &constant.name, "constant", &inherited_names)?
                }
                Member::Attribute(attribute) => reject_inherited_name(
                    interface,
                    &attribute.name,
                    "attribute",
                    &inherited_names,
                )?,
                Member::Operation(operation) => {
                    let signature = callable_signature(&operation.arguments);
                    if inherited_operations.contains(&(
                        operation.name.clone(),
                        operation.special,
                        signature,
                    )) {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` operation `{}`",
                                interface.name, operation.name
                            ),
                            "duplicates an operation signature declared by an ancestor interface",
                        ));
                    }
                    if operation.special == OperationSpecial::Regular
                        && inherited_names
                            .get(&operation.name)
                            .is_some_and(|kind| *kind != "operation")
                    {
                        reject_inherited_name(
                            interface,
                            &operation.name,
                            "operation",
                            &inherited_names,
                        )?;
                    }
                }
                Member::Collection(collection) => {
                    if let Some(previous) = inherited_collection {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` {}",
                                interface.name,
                                collection_kind_name(&collection.kind)
                            ),
                            format!("collection declaration conflicts with inherited {previous}"),
                        ));
                    }
                }
                Member::Constructor(_) => {}
            }
        }
    }
    Ok(())
}

fn reject_inherited_name(
    interface: &InterfaceDef,
    name: &str,
    kind: &str,
    inherited_names: &BTreeMap<String, &'static str>,
) -> Result<(), BindgenError> {
    if let Some(inherited_kind) = inherited_names.get(name) {
        Err(BindgenError::new(
            format!("interface `{}` {kind} `{name}`", interface.name),
            format!("name conflicts with inherited {inherited_kind} `{name}`"),
        ))
    } else {
        Ok(())
    }
}

fn validate_special_operations(world: &World) -> Result<(), BindgenError> {
    for interface in world.interfaces.values() {
        let mut identities = BTreeSet::new();
        let mut has_stringifier = false;
        for member in &interface.members {
            match member {
                Member::Attribute(attribute) if attribute.stringifier => {
                    let context = format!(
                        "interface `{}` stringifier attribute `{}`",
                        interface.name, attribute.name
                    );
                    if !matches!(resolve_typedef(world, &attribute.type_), TypeRef::String(_)) {
                        return Err(BindgenError::new(
                            context,
                            "stringifier attribute must have a Web IDL string type",
                        ));
                    }
                    if has_stringifier {
                        return Err(BindgenError::new(
                            context,
                            "multiple stringifiers are not allowed",
                        ));
                    }
                    has_stringifier = true;
                }
                Member::Operation(operation) if operation.special != OperationSpecial::Regular => {
                    let context = format!(
                        "interface `{}` {}",
                        interface.name,
                        operation_special_name(operation.special)
                    );
                    validate_special_signature(world, operation, &context)?;
                    if operation.special == OperationSpecial::Stringifier {
                        if has_stringifier {
                            return Err(BindgenError::new(
                                context,
                                "multiple stringifiers are not allowed",
                            ));
                        }
                        has_stringifier = true;
                        continue;
                    }
                    let key_kind = if matches!(
                        operation.special,
                        OperationSpecial::Getter
                            | OperationSpecial::Setter
                            | OperationSpecial::Deleter
                    ) {
                        special_key_kind(world, &operation.arguments[0].type_, &context)?
                    } else {
                        "call"
                    };
                    if !identities.insert((operation.special, key_kind)) {
                        return Err(BindgenError::new(
                            context,
                            format!(
                                "duplicate {} operation for {key_kind} keys",
                                operation_special_name(operation.special)
                            ),
                        ));
                    }
                }
                Member::Const(_)
                | Member::Constructor(_)
                | Member::Collection(_)
                | Member::Attribute(_)
                | Member::Operation(_) => {}
            }
        }
    }
    Ok(())
}

fn validate_special_signature(
    world: &World,
    operation: &OperationDef,
    context: &str,
) -> Result<(), BindgenError> {
    if operation
        .arguments
        .iter()
        .any(|argument| argument.optional || argument.variadic)
    {
        return Err(BindgenError::new(
            context,
            "special-operation arguments must be required and non-variadic",
        ));
    }
    let valid = match operation.special {
        OperationSpecial::Regular => true,
        OperationSpecial::Getter => {
            operation.arguments.len() == 1 && operation.result != TypeRef::Unit
        }
        OperationSpecial::Setter => {
            operation.arguments.len() == 2 && operation.result == TypeRef::Unit
        }
        OperationSpecial::Deleter => {
            operation.arguments.len() == 1
                && matches!(
                    resolve_typedef(world, &operation.result),
                    TypeRef::Unit | TypeRef::Bool
                )
        }
        OperationSpecial::LegacyCaller => true,
        OperationSpecial::Stringifier => {
            operation.arguments.is_empty()
                && matches!(
                    resolve_typedef(world, &operation.result),
                    TypeRef::String(_)
                )
        }
    };
    if valid {
        Ok(())
    } else {
        Err(BindgenError::new(
            context,
            "signature does not satisfy Web IDL special-operation requirements",
        ))
    }
}

fn special_key_kind<'a>(
    world: &'a World,
    type_: &'a TypeRef,
    context: &str,
) -> Result<&'static str, BindgenError> {
    match resolve_typedef(world, type_) {
        TypeRef::String(_) => Ok("named"),
        TypeRef::I8
        | TypeRef::U8
        | TypeRef::I16
        | TypeRef::U16
        | TypeRef::I32
        | TypeRef::U32
        | TypeRef::I64
        | TypeRef::U64 => Ok("indexed"),
        type_ => Err(BindgenError::new(
            context,
            format!("special-operation key type `{type_:?}` is neither string nor integer"),
        )),
    }
}

fn validate_typed_defaults(world: &World) -> Result<(), BindgenError> {
    for dictionary in world.dictionaries.values() {
        for member in &dictionary.members {
            if let Some(default) = &member.default_ {
                validate_default_type(
                    world,
                    &member.type_,
                    default,
                    &format!("dictionary `{}` member `{}`", dictionary.name, member.name),
                )?;
            }
        }
    }
    for callback in world.callbacks.values() {
        for argument in &callback.arguments {
            if let Some(default) = &argument.default_ {
                validate_default_type(
                    world,
                    &argument.type_,
                    default,
                    &format!("callback `{}` argument `{}`", callback.name, argument.name),
                )?;
            }
        }
    }
    for interface in world.interfaces.values() {
        for member in &interface.members {
            match member {
                Member::Const(constant) => validate_default_type(
                    world,
                    &constant.type_,
                    &constant.value,
                    &format!("interface `{}` const `{}`", interface.name, constant.name),
                )?,
                Member::Constructor(constructor) => {
                    for argument in &constructor.arguments {
                        if let Some(default) = &argument.default_ {
                            validate_default_type(
                                world,
                                &argument.type_,
                                default,
                                &format!(
                                    "interface `{}` constructor argument `{}`",
                                    interface.name, argument.name
                                ),
                            )?;
                        }
                    }
                }
                Member::Collection(collection) => {
                    if let CollectionKind::AsyncIterable { arguments, .. } = &collection.kind {
                        for argument in arguments {
                            if let Some(default) = &argument.default_ {
                                validate_default_type(
                                    world,
                                    &argument.type_,
                                    default,
                                    &format!(
                                        "interface `{}` async iterable argument `{}`",
                                        interface.name, argument.name
                                    ),
                                )?;
                            }
                        }
                    }
                }
                Member::Operation(operation) => {
                    for argument in &operation.arguments {
                        if let Some(default) = &argument.default_ {
                            validate_default_type(
                                world,
                                &argument.type_,
                                default,
                                &format!(
                                    "interface `{}` operation `{}` argument `{}`",
                                    interface.name, operation.name, argument.name
                                ),
                            )?;
                        }
                    }
                }
                Member::Attribute(_) => {}
            }
        }
    }
    for namespace in world.namespaces.values() {
        for member in &namespace.members {
            if let NamespaceMember::Operation(operation) = member {
                for argument in &operation.arguments {
                    if let Some(default) = &argument.default_ {
                        validate_default_type(
                            world,
                            &argument.type_,
                            default,
                            &format!(
                                "namespace `{}` operation `{}` argument `{}`",
                                namespace.name, operation.name, argument.name
                            ),
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_default_type(
    world: &World,
    type_: &TypeRef,
    default: &DefaultValueDef,
    context: &str,
) -> Result<(), BindgenError> {
    let resolved = resolve_typedef(world, type_);
    let compatible = match (resolved, default) {
        (TypeRef::Bool, DefaultValueDef::Bool(_)) => true,
        (
            TypeRef::I8
            | TypeRef::U8
            | TypeRef::I16
            | TypeRef::U16
            | TypeRef::I32
            | TypeRef::U32
            | TypeRef::I64
            | TypeRef::U64,
            DefaultValueDef::Integer(_),
        ) => true,
        (TypeRef::F32 | TypeRef::F64, DefaultValueDef::Integer(_) | DefaultValueDef::Float(_)) => {
            true
        }
        (TypeRef::String(_), DefaultValueDef::String(_)) => true,
        (TypeRef::Nullable(_), DefaultValueDef::Null) => true,
        (TypeRef::Sequence(_), DefaultValueDef::EmptySequence) => true,
        (TypeRef::Named(name), DefaultValueDef::String(value)) => world
            .enums
            .get(name)
            .is_some_and(|definition| definition.values.contains(value)),
        (TypeRef::Named(name), DefaultValueDef::EmptyDictionary) => {
            world.dictionaries.contains_key(name)
        }
        (TypeRef::Union(members), _) => members
            .iter()
            .any(|member| validate_default_type(world, member, default, context).is_ok()),
        _ => false,
    };
    if compatible {
        Ok(())
    } else {
        Err(BindgenError::new(
            context,
            format!("default `{default:?}` is incompatible with Web IDL type `{type_:?}`"),
        ))
    }
}

fn validate_definition_names(
    interfaces: &BTreeMap<String, InterfaceDef>,
    namespaces: &BTreeMap<String, NamespaceDef>,
    typedefs: &BTreeMap<String, TypedefDef>,
    enums: &BTreeMap<String, EnumDef>,
    dictionaries: &BTreeMap<String, DictionaryDef>,
    mixins: &BTreeMap<String, MixinDef>,
    callbacks: &BTreeMap<String, CallbackDef>,
) -> Result<(), BindgenError> {
    let mut names = BTreeMap::<&str, &str>::new();
    for (kind, definitions) in [
        ("interface", interfaces.keys().collect::<Vec<_>>()),
        ("namespace", namespaces.keys().collect::<Vec<_>>()),
        ("typedef", typedefs.keys().collect()),
        ("enum", enums.keys().collect()),
        ("dictionary", dictionaries.keys().collect()),
        ("interface mixin", mixins.keys().collect()),
        ("callback", callbacks.keys().collect()),
    ] {
        for name in definitions {
            if let Some(previous) = names.insert(name, kind) {
                return Err(BindgenError::new(
                    format!("{kind} `{name}`"),
                    format!("name is already defined as a {previous}"),
                ));
            }
        }
    }
    Ok(())
}

fn definition_kind(definition: &Definition) -> &'static str {
    match definition {
        Definition::Callback(_) => "callback",
        Definition::Dictionary(_) => "dictionary",
        Definition::Enum(_) => "enum",
        Definition::Implements(_) => "implements",
        Definition::IncludesStatement(_) => "includes",
        Definition::Interface(_) => "interface",
        Definition::InterfaceMixin(_) | Definition::PartialInterfaceMixin(_) => "mixin",
        Definition::Namespace(_) => "namespace",
        Definition::PartialNamespace(_) => "partial namespace",
        Definition::PartialDictionary(_) => "partial dictionary",
        Definition::PartialInterface(_) => "partial interface",
        Definition::CallbackInterface(_) => "callback interface",
        Definition::Typedef(_) => "typedef",
    }
}

fn insert_unique<T>(
    definitions: &mut BTreeMap<String, T>,
    name: String,
    definition: T,
    kind: &str,
) -> Result<(), BindgenError> {
    if definitions.insert(name.clone(), definition).is_some() {
        return Err(BindgenError::new(
            format!("{kind} `{name}`"),
            "duplicate non-partial definition",
        ));
    }
    Ok(())
}

fn normalize_extended_attributes(
    attributes: Option<ExtendedAttributeList<'_>>,
    context: &str,
) -> Result<ExtendedAttributesDef, BindgenError> {
    normalize_extended_attribute_items(
        attributes
            .map(|attributes| attributes.body.list)
            .unwrap_or_default(),
        context,
    )
}

fn normalize_interface_attributes(
    attributes: Option<ExtendedAttributeList<'_>>,
    interface: &str,
) -> Result<(ExtendedAttributesDef, Vec<Member>), BindgenError> {
    let mut ordinary = Vec::new();
    let mut constructors = Vec::new();
    for attribute in attributes
        .map(|attributes| attributes.body.list)
        .unwrap_or_default()
    {
        match attribute {
            ExtendedAttribute::ArgList(attribute) if attribute.identifier.0 == "Constructor" => {
                constructors.push(Member::Constructor(ConstructorDef {
                    name: None,
                    arguments: normalize_arguments(attribute.args.body.list),
                    overload: 0,
                    attributes: ExtendedAttributesDef::default(),
                }));
            }
            ExtendedAttribute::NamedArgList(attribute)
                if attribute.lhs_identifier.0 == "NamedConstructor" =>
            {
                constructors.push(Member::Constructor(ConstructorDef {
                    name: Some(attribute.rhs_identifier.0.to_owned()),
                    arguments: normalize_arguments(attribute.args.body.list),
                    overload: 0,
                    attributes: ExtendedAttributesDef::default(),
                }));
            }
            attribute => ordinary.push(attribute),
        }
    }
    let attributes =
        normalize_extended_attribute_items(ordinary, &format!("interface `{interface}`"))?;
    Ok((attributes, constructors))
}

fn normalize_extended_attribute_items(
    attributes: Vec<ExtendedAttribute<'_>>,
    context: &str,
) -> Result<ExtendedAttributesDef, BindgenError> {
    let mut result = ExtendedAttributesDef::default();
    for attribute in attributes {
        let attribute_name = match &attribute {
            ExtendedAttribute::ArgList(value) => value.identifier.0,
            ExtendedAttribute::NamedArgList(value) => value.lhs_identifier.0,
            ExtendedAttribute::IdentList(value) => value.identifier.0,
            ExtendedAttribute::Ident(value) => value.lhs_identifier.0,
            ExtendedAttribute::NoArgs(value) => value.0.0,
        }
        .to_owned();
        match attribute {
            ExtendedAttribute::Ident(attribute) if attribute.lhs_identifier.0 == "Exposed" => {
                if result.exposed.is_some() {
                    return Err(BindgenError::new(
                        context,
                        "duplicate `[Exposed]` extended attribute",
                    ));
                }
                let value = match attribute.rhs {
                    IdentifierOrString::Identifier(value) => value.0.to_owned(),
                    IdentifierOrString::String(_) => {
                        return Err(BindgenError::new(
                            context,
                            "`[Exposed]` requires global identifiers, not a string",
                        ));
                    }
                };
                result.exposed = Some(vec![if value == "FeExposedWildcard" {
                    "*".to_owned()
                } else {
                    value
                }]);
            }
            ExtendedAttribute::IdentList(attribute) if attribute.identifier.0 == "Exposed" => {
                if result.exposed.is_some() {
                    return Err(BindgenError::new(
                        context,
                        "duplicate `[Exposed]` extended attribute",
                    ));
                }
                let values = attribute
                    .list
                    .body
                    .list
                    .into_iter()
                    .map(|value| value.0.to_owned())
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    return Err(BindgenError::new(
                        context,
                        "`[Exposed]` global list must not be empty",
                    ));
                }
                result.exposed = Some(values);
            }
            ExtendedAttribute::NoArgs(attribute) if attribute.0.0 == "SecureContext" => {
                if result.secure_context {
                    return Err(BindgenError::new(
                        context,
                        "duplicate `[SecureContext]` extended attribute",
                    ));
                }
                result.secure_context = true;
            }
            ExtendedAttribute::NoArgs(attribute) if attribute.0.0 == "SameObject" => {
                if result.same_object {
                    return Err(BindgenError::new(context, "duplicate `[SameObject]`"));
                }
                result.same_object = true;
            }
            ExtendedAttribute::NoArgs(attribute) if attribute.0.0 == "LegacyUnforgeable" => {
                if result.legacy_unforgeable {
                    return Err(BindgenError::new(
                        context,
                        "duplicate `[LegacyUnforgeable]`",
                    ));
                }
                result.legacy_unforgeable = true;
            }
            ExtendedAttribute::Ident(attribute) if attribute.lhs_identifier.0 == "PutForwards" => {
                if result.put_forwards.is_some() {
                    return Err(BindgenError::new(context, "duplicate `[PutForwards]`"));
                }
                let IdentifierOrString::Identifier(target) = attribute.rhs else {
                    return Err(BindgenError::new(
                        context,
                        "`[PutForwards]` requires an identifier",
                    ));
                };
                result.put_forwards = Some(target.0.to_owned());
            }
            // Other extended attributes remain in the Weedle input boundary.
            // They do not affect this normalized rung until their semantics
            // have an explicit representation.
            _ => result.unmodeled.push(attribute_name),
        }
    }
    Ok(result)
}

fn normalize_default(value: DefaultValue<'_>) -> DefaultValueDef {
    match value {
        DefaultValue::Boolean(value) => DefaultValueDef::Bool(value.0),
        DefaultValue::Integer(value) => DefaultValueDef::Integer(match value {
            IntegerLit::Dec(value) => value.0.to_owned(),
            IntegerLit::Hex(value) => value.0.to_owned(),
            IntegerLit::Oct(value) => value.0.to_owned(),
        }),
        DefaultValue::Float(value) => DefaultValueDef::Float(match value {
            FloatLit::Value(value) => value.0.to_owned(),
            FloatLit::NegInfinity(_) => "-Infinity".to_owned(),
            FloatLit::Infinity(_) => "Infinity".to_owned(),
            FloatLit::NaN(_) => "NaN".to_owned(),
        }),
        DefaultValue::String(value) => DefaultValueDef::String(value.0.to_owned()),
        DefaultValue::Null(_) => DefaultValueDef::Null,
        DefaultValue::EmptyArray(_) => DefaultValueDef::EmptySequence,
        DefaultValue::EmptyDictionary(_) => DefaultValueDef::EmptyDictionary,
    }
}

fn normalize_const_value(value: ConstValue<'_>) -> DefaultValueDef {
    match value {
        ConstValue::Boolean(value) => DefaultValueDef::Bool(value.0),
        ConstValue::Integer(value) => DefaultValueDef::Integer(match value {
            IntegerLit::Dec(value) => value.0.to_owned(),
            IntegerLit::Hex(value) => value.0.to_owned(),
            IntegerLit::Oct(value) => value.0.to_owned(),
        }),
        ConstValue::Float(value) => DefaultValueDef::Float(match value {
            FloatLit::Value(value) => value.0.to_owned(),
            FloatLit::NegInfinity(_) => "-Infinity".to_owned(),
            FloatLit::Infinity(_) => "Infinity".to_owned(),
            FloatLit::NaN(_) => "NaN".to_owned(),
        }),
        ConstValue::Null(_) => DefaultValueDef::Null,
    }
}

fn normalize_const_type(type_: ConstType<'_>, context: &str) -> Result<TypeRef, BindgenError> {
    let (type_, nullable) = match type_ {
        ConstType::Integer(value) => (
            match value.type_ {
                IntegerType::Short(value) if value.unsigned.is_some() => TypeRef::U16,
                IntegerType::Short(_) => TypeRef::I16,
                IntegerType::Long(value) if value.unsigned.is_some() => TypeRef::U32,
                IntegerType::Long(_) => TypeRef::I32,
                IntegerType::LongLong(value) if value.unsigned.is_some() => TypeRef::U64,
                IntegerType::LongLong(_) => TypeRef::I64,
            },
            value.q_mark.is_some(),
        ),
        ConstType::FloatingPoint(value) => (
            match value.type_ {
                FloatingPointType::Float(_) => TypeRef::F32,
                FloatingPointType::Double(_) => TypeRef::F64,
            },
            value.q_mark.is_some(),
        ),
        ConstType::Boolean(value) => (TypeRef::Bool, value.q_mark.is_some()),
        ConstType::Byte(value) => (TypeRef::I8, value.q_mark.is_some()),
        ConstType::Octet(value) => (TypeRef::U8, value.q_mark.is_some()),
        ConstType::Identifier(value) => (
            TypeRef::Named(value.type_.0.to_owned()),
            value.q_mark.is_some(),
        ),
    };
    if nullable {
        return Err(BindgenError::new(
            context,
            "nullable constants are retained by Web IDL but are not representable as Fe constants",
        ));
    }
    Ok(type_)
}

fn normalize_dictionary_members(
    dictionary: &str,
    members: Vec<DictionaryMember<'_>>,
) -> Result<Vec<DictionaryMemberDef>, BindgenError> {
    let normalized = members
        .into_iter()
        .map(|member| DictionaryMemberDef {
            name: member.identifier.0.to_owned(),
            type_: normalize_type(member.type_),
            required: member.required.is_some(),
            default_: member
                .default
                .map(|default| normalize_default(default.value)),
        })
        .collect::<Vec<_>>();
    let definition = DictionaryDef {
        name: dictionary.to_owned(),
        inherits: None,
        members: normalized,
    };
    validate_dictionary_member_names(&definition)?;
    Ok(definition.members)
}

fn validate_dictionary_member_names(dictionary: &DictionaryDef) -> Result<(), BindgenError> {
    let mut names = BTreeSet::new();
    if let Some(member) = dictionary
        .members
        .iter()
        .find(|member| !names.insert(&member.name))
    {
        return Err(BindgenError::new(
            format!("dictionary `{}` member `{}`", dictionary.name, member.name),
            "duplicate dictionary member",
        ));
    }
    Ok(())
}

fn normalize_mixin_members(
    mixin: &str,
    members: Vec<MixinMember<'_>>,
) -> Result<Vec<Member>, BindgenError> {
    members
        .into_iter()
        .map(|member| match member {
            MixinMember::Attribute(attribute) => {
                if attribute.stringifier.is_some() {
                    return Err(BindgenError::new(
                        format!("interface mixin `{mixin}`"),
                        "stringifier attributes are not implemented yet",
                    ));
                }
                Ok(Member::Attribute(AttributeDef {
                    name: attribute.identifier.0.to_owned(),
                    type_: normalize_type(attribute.type_.type_),
                    read_only: attribute.readonly.is_some(),
                    static_: false,
                    stringifier: false,
                    attributes: normalize_extended_attributes(
                        attribute.attributes,
                        "mixin attribute",
                    )?,
                }))
            }
            MixinMember::Operation(operation) => {
                if operation.stringifier.is_some() {
                    return Err(BindgenError::new(
                        format!("interface mixin `{mixin}`"),
                        "stringifier operations are not implemented yet",
                    ));
                }
                let name = operation.identifier.ok_or_else(|| {
                    BindgenError::new(
                        format!("interface mixin `{mixin}`"),
                        "anonymous regular operation is not representable",
                    )
                })?;
                Ok(Member::Operation(OperationDef {
                    name: name.0.to_owned(),
                    arguments: normalize_arguments(operation.args.body.list),
                    result: normalize_return_type(operation.return_type),
                    static_: false,
                    special: OperationSpecial::Regular,
                    overload: 0,
                    attributes: normalize_extended_attributes(
                        operation.attributes,
                        "mixin operation",
                    )?,
                }))
            }
            MixinMember::Const(constant) => {
                let name = constant.identifier.0.to_owned();
                Err(BindgenError::new(
                    format!("interface mixin `{mixin}` const `{name}`"),
                    "constants on interface mixins are not implemented yet",
                ))
            }
            MixinMember::Stringifier(_) => Err(BindgenError::new(
                format!("interface mixin `{mixin}`"),
                "stringifier members are not implemented yet",
            )),
        })
        .collect()
}

fn normalize_arguments(arguments: Vec<Argument<'_>>) -> Vec<ArgumentDef> {
    arguments
        .into_iter()
        .map(|argument| match argument {
            Argument::Single(argument) => ArgumentDef {
                name: argument.identifier.0.to_owned(),
                type_: normalize_type(argument.type_.type_),
                optional: argument.optional.is_some(),
                default_: argument
                    .default
                    .map(|default| normalize_default(default.value)),
                variadic: false,
            },
            Argument::Variadic(argument) => ArgumentDef {
                name: argument.identifier.0.to_owned(),
                type_: normalize_type(argument.type_),
                optional: false,
                default_: None,
                variadic: true,
            },
        })
        .collect()
}

fn validate_inheritance_cycles<T>(
    definitions: &BTreeMap<String, T>,
    kind: &str,
) -> Result<(), BindgenError>
where
    T: Inherits,
{
    for name in definitions.keys() {
        let mut chain = BTreeSet::new();
        let mut cursor = name.as_str();
        loop {
            if !chain.insert(cursor.to_owned()) {
                return Err(BindgenError::new(
                    format!("{kind} `{name}`"),
                    format!("inheritance cycle reaches `{cursor}`"),
                ));
            }
            let Some(parent) = definitions.get(cursor).and_then(Inherits::parent) else {
                break;
            };
            cursor = parent;
        }
    }
    Ok(())
}

trait Inherits {
    fn parent(&self) -> Option<&str>;
}

impl Inherits for InterfaceDef {
    fn parent(&self) -> Option<&str> {
        self.inherits.as_deref()
    }
}

impl Inherits for DictionaryDef {
    fn parent(&self) -> Option<&str> {
        self.inherits.as_deref()
    }
}

fn validate_typedef_cycles(typedefs: &BTreeMap<String, TypedefDef>) -> Result<(), BindgenError> {
    for name in typedefs.keys() {
        let mut path = BTreeSet::new();
        if let Some(cycle) = typedef_cycle_from(name, name, typedefs, &mut path) {
            return Err(BindgenError::new(
                format!("typedef `{name}`"),
                format!("typedef cycle reaches `{cycle}`"),
            ));
        }
    }
    Ok(())
}

fn typedef_cycle_from<'a>(
    start: &'a str,
    current: &'a str,
    typedefs: &'a BTreeMap<String, TypedefDef>,
    path: &mut BTreeSet<String>,
) -> Option<&'a str> {
    if !path.insert(current.to_owned()) {
        return None;
    }
    let mut dependencies = BTreeSet::new();
    collect_named_types(&typedefs[current].type_, &mut dependencies);
    for dependency in dependencies {
        if dependency == start {
            return Some(dependency);
        }
        if typedefs.contains_key(dependency)
            && let Some(cycle) = typedef_cycle_from(start, dependency, typedefs, path)
        {
            return Some(cycle);
        }
    }
    path.remove(current);
    None
}

fn collect_named_types<'a>(type_: &'a TypeRef, names: &mut BTreeSet<&'a str>) {
    match type_ {
        TypeRef::Named(name) => {
            names.insert(name);
        }
        TypeRef::Nullable(inner)
        | TypeRef::Sequence(inner)
        | TypeRef::Promise(inner)
        | TypeRef::Record(inner) => collect_named_types(inner, names),
        TypeRef::Union(members) => {
            for member in members {
                collect_named_types(member, names);
            }
        }
        _ => {}
    }
}

fn normalize_members(
    interface: &str,
    members: Vec<InterfaceMember>,
) -> Result<Vec<Member>, BindgenError> {
    members
        .into_iter()
        .map(|member| match member {
            InterfaceMember::Const(constant) => {
                let name = constant.identifier.0.to_owned();
                let context = format!("interface `{interface}` const `{name}`");
                let type_ = normalize_const_type(constant.const_type, &context)?;
                let value = normalize_const_value(constant.const_value);
                Ok(Member::Const(ConstDef {
                    name,
                    type_,
                    value,
                    attributes: normalize_extended_attributes(constant.attributes, &context)?,
                }))
            }
            InterfaceMember::Attribute(attribute) => {
                if matches!(
                    attribute.modifier,
                    Some(StringifierOrInheritOrStatic::Inherit(_))
                ) {
                    return Err(BindgenError::new(
                        format!(
                            "interface `{interface}` attribute `{}`",
                            attribute.identifier.0
                        ),
                        "inherited attributes are not implemented yet",
                    ));
                }
                let static_ = matches!(
                    attribute.modifier,
                    Some(StringifierOrInheritOrStatic::Static(_))
                );
                Ok(Member::Attribute(AttributeDef {
                    name: attribute.identifier.0.to_owned(),
                    type_: normalize_type(attribute.type_.type_),
                    read_only: attribute.readonly.is_some(),
                    static_,
                    stringifier: matches!(
                        attribute.modifier,
                        Some(StringifierOrInheritOrStatic::Stringifier(_))
                    ),
                    attributes: normalize_extended_attributes(
                        attribute.attributes,
                        "interface attribute",
                    )?,
                }))
            }
            InterfaceMember::Operation(operation) => {
                let stringifier = matches!(
                    operation.modifier,
                    Some(StringifierOrStatic::Stringifier(_))
                );
                let special = if stringifier {
                    OperationSpecial::Stringifier
                } else {
                    match operation.special {
                        Some(Special::Getter(_)) => OperationSpecial::Getter,
                        Some(Special::Setter(_)) => OperationSpecial::Setter,
                        Some(Special::Deleter(_)) => OperationSpecial::Deleter,
                        Some(Special::LegacyCaller(_)) => OperationSpecial::LegacyCaller,
                        None => OperationSpecial::Regular,
                    }
                };
                let name = operation
                    .identifier
                    .map(|identifier| identifier.0.to_owned())
                    .or_else(|| {
                        (special != OperationSpecial::Regular)
                            .then(|| operation_special_name(special).to_owned())
                    })
                    .ok_or_else(|| {
                        BindgenError::new(
                            format!("interface `{interface}`"),
                            "anonymous regular operation is not representable",
                        )
                    })?;
                let arguments = normalize_arguments(operation.args.body.list);
                Ok(Member::Operation(OperationDef {
                    name,
                    arguments,
                    result: normalize_return_type(operation.return_type),
                    static_: matches!(operation.modifier, Some(StringifierOrStatic::Static(_))),
                    special,
                    overload: 0,
                    attributes: normalize_extended_attributes(
                        operation.attributes,
                        "interface operation",
                    )?,
                }))
            }
            InterfaceMember::Constructor(constructor) => Ok(Member::Constructor(ConstructorDef {
                name: None,
                arguments: normalize_arguments(constructor.args.body.list),
                overload: 0,
                attributes: normalize_extended_attributes(
                    constructor.attributes,
                    &format!("interface `{interface}` constructor"),
                )?,
            })),
            InterfaceMember::Iterable(iterable) => {
                let (key, value, attributes) = match iterable {
                    IterableInterfaceMember::Single(iterable) => (
                        None,
                        normalize_type(iterable.generics.body.type_),
                        iterable.attributes,
                    ),
                    IterableInterfaceMember::Double(iterable) => (
                        Some(normalize_type(iterable.generics.body.0.type_)),
                        normalize_type(iterable.generics.body.2.type_),
                        iterable.attributes,
                    ),
                };
                Ok(Member::Collection(CollectionDef {
                    kind: CollectionKind::Iterable { key, value },
                    attributes: normalize_extended_attributes(
                        attributes,
                        &format!("interface `{interface}` iterable"),
                    )?,
                }))
            }
            InterfaceMember::AsyncIterable(iterable) => {
                let (key, value, arguments, attributes) = match iterable {
                    AsyncIterableInterfaceMember::Single(iterable) => (
                        None,
                        normalize_type(iterable.generics.body.type_),
                        iterable
                            .args
                            .map(|args| normalize_arguments(args.body.list))
                            .unwrap_or_default(),
                        iterable.attributes,
                    ),
                    AsyncIterableInterfaceMember::Double(iterable) => (
                        Some(normalize_type(iterable.generics.body.0.type_)),
                        normalize_type(iterable.generics.body.2.type_),
                        iterable
                            .args
                            .map(|args| normalize_arguments(args.body.list))
                            .unwrap_or_default(),
                        iterable.attributes,
                    ),
                };
                Ok(Member::Collection(CollectionDef {
                    kind: CollectionKind::AsyncIterable {
                        key,
                        value,
                        arguments,
                    },
                    attributes: normalize_extended_attributes(
                        attributes,
                        &format!("interface `{interface}` async iterable"),
                    )?,
                }))
            }
            InterfaceMember::Maplike(maplike) => Ok(Member::Collection(CollectionDef {
                kind: CollectionKind::Maplike {
                    key: normalize_type(maplike.generics.body.0.type_),
                    value: normalize_type(maplike.generics.body.2.type_),
                    read_only: maplike.readonly.is_some(),
                },
                attributes: normalize_extended_attributes(
                    maplike.attributes,
                    &format!("interface `{interface}` maplike"),
                )?,
            })),
            InterfaceMember::Setlike(setlike) => Ok(Member::Collection(CollectionDef {
                kind: CollectionKind::Setlike {
                    value: normalize_type(setlike.generics.body.type_),
                    read_only: setlike.readonly.is_some(),
                },
                attributes: normalize_extended_attributes(
                    setlike.attributes,
                    &format!("interface `{interface}` setlike"),
                )?,
            })),
            InterfaceMember::Stringifier(stringifier) => Ok(Member::Operation(OperationDef {
                name: "stringifier".to_owned(),
                arguments: Vec::new(),
                result: TypeRef::String(StringKind::Dom),
                static_: false,
                special: OperationSpecial::Stringifier,
                overload: 0,
                attributes: normalize_extended_attributes(
                    stringifier.attributes,
                    &format!("interface `{interface}` stringifier"),
                )?,
            })),
        })
        .collect()
}

fn normalize_namespace_members(
    namespace: &str,
    members: Vec<WeedleNamespaceMember<'_>>,
) -> Result<Vec<NamespaceMember>, BindgenError> {
    members
        .into_iter()
        .map(|member| match member {
            WeedleNamespaceMember::Attribute(attribute) => {
                Ok(NamespaceMember::Attribute(AttributeDef {
                    name: attribute.identifier.0.to_owned(),
                    type_: normalize_type(attribute.type_.type_),
                    read_only: true,
                    static_: true,
                    stringifier: false,
                    attributes: normalize_extended_attributes(
                        attribute.attributes,
                        &format!(
                            "namespace `{namespace}` attribute `{}`",
                            attribute.identifier.0
                        ),
                    )?,
                }))
            }
            WeedleNamespaceMember::Operation(operation) => {
                let name = operation.identifier.ok_or_else(|| {
                    BindgenError::new(
                        format!("namespace `{namespace}`"),
                        "anonymous namespace operation is not representable",
                    )
                })?;
                Ok(NamespaceMember::Operation(OperationDef {
                    name: name.0.to_owned(),
                    arguments: normalize_arguments(operation.args.body.list),
                    result: normalize_return_type(operation.return_type),
                    static_: true,
                    special: OperationSpecial::Regular,
                    overload: 0,
                    attributes: normalize_extended_attributes(
                        operation.attributes,
                        &format!("namespace `{namespace}` operation `{}`", name.0),
                    )?,
                }))
            }
        })
        .collect()
}

fn assign_overload_indexes(
    interfaces: &mut BTreeMap<String, InterfaceDef>,
) -> Result<(), BindgenError> {
    for interface in interfaces.values_mut() {
        let mut counts = BTreeMap::<String, usize>::new();
        let mut signatures = BTreeSet::new();
        let mut attributes = BTreeSet::new();
        let mut constants = BTreeSet::new();
        let mut constructor_signatures = BTreeSet::new();
        let mut constructor_counts = BTreeMap::<Option<String>, usize>::new();
        let mut collection = None;
        for member in &interface.members {
            match member {
                Member::Const(constant) if !constants.insert(constant.name.clone()) => {
                    return Err(BindgenError::new(
                        format!("interface `{}` const `{}`", interface.name, constant.name),
                        "duplicate constant, possibly introduced by a partial",
                    ));
                }
                Member::Attribute(attribute) if !attributes.insert(attribute.name.clone()) => {
                    return Err(BindgenError::new(
                        format!(
                            "interface `{}` attribute `{}`",
                            interface.name, attribute.name
                        ),
                        "duplicate attribute, possibly introduced by a partial or included mixin",
                    ));
                }
                Member::Const(_)
                | Member::Constructor(_)
                | Member::Collection(_)
                | Member::Attribute(_)
                | Member::Operation(_) => {}
            }
            if let Member::Collection(candidate) = member {
                let label = collection_kind_name(&candidate.kind);
                if let Some(previous) = collection.replace(label) {
                    return Err(BindgenError::new(
                        format!("interface `{}` {label}", interface.name),
                        format!(
                            "multiple collection declarations are not allowed (already has {previous})"
                        ),
                    ));
                }
            }
        }
        for member in &mut interface.members {
            let Member::Constructor(constructor) = member else {
                continue;
            };
            let signature = callable_signature(&constructor.arguments);
            if !constructor_signatures.insert((constructor.name.clone(), signature)) {
                let label = constructor.name.as_deref().unwrap_or("constructor");
                return Err(BindgenError::new(
                    format!("interface `{}` constructor `{label}`", interface.name),
                    "duplicate constructor overload signature",
                ));
            }
            let next = constructor_counts
                .entry(constructor.name.clone())
                .or_default();
            constructor.overload = *next;
            *next += 1;
        }
        for member in &mut interface.members {
            let Member::Operation(operation) = member else {
                continue;
            };
            let signature = callable_signature(&operation.arguments);
            if operation.special == OperationSpecial::Regular
                && !signatures.insert((operation.name.clone(), signature))
            {
                return Err(BindgenError::new(
                    format!(
                        "interface `{}` operation `{}`",
                        interface.name, operation.name
                    ),
                    "duplicate overload signature",
                ));
            }
            let next = counts.entry(operation.name.clone()).or_default();
            operation.overload = *next;
            *next += 1;
        }
    }
    Ok(())
}

fn collection_kind_name(kind: &CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Iterable { .. } => "iterable",
        CollectionKind::AsyncIterable { .. } => "async iterable",
        CollectionKind::Maplike { .. } => "maplike",
        CollectionKind::Setlike { .. } => "setlike",
    }
}

fn operation_special_name(special: OperationSpecial) -> &'static str {
    match special {
        OperationSpecial::Regular => "operation",
        OperationSpecial::Getter => "getter",
        OperationSpecial::Setter => "setter",
        OperationSpecial::Deleter => "deleter",
        OperationSpecial::LegacyCaller => "legacy caller",
        OperationSpecial::Stringifier => "stringifier",
    }
}

fn assign_namespace_overload_indexes(
    namespaces: &mut BTreeMap<String, NamespaceDef>,
) -> Result<(), BindgenError> {
    for namespace in namespaces.values_mut() {
        let mut attributes = BTreeSet::new();
        let mut signatures = BTreeSet::new();
        let mut counts = BTreeMap::<String, usize>::new();
        for member in &namespace.members {
            if let NamespaceMember::Attribute(attribute) = member
                && !attributes.insert(attribute.name.clone())
            {
                return Err(BindgenError::new(
                    format!(
                        "namespace `{}` attribute `{}`",
                        namespace.name, attribute.name
                    ),
                    "duplicate readonly attribute, possibly introduced by a partial namespace",
                ));
            }
        }
        for member in &mut namespace.members {
            let NamespaceMember::Operation(operation) = member else {
                continue;
            };
            let signature = callable_signature(&operation.arguments);
            if !signatures.insert((operation.name.clone(), signature)) {
                return Err(BindgenError::new(
                    format!(
                        "namespace `{}` operation `{}`",
                        namespace.name, operation.name
                    ),
                    "duplicate overload signature",
                ));
            }
            let next = counts.entry(operation.name.clone()).or_default();
            operation.overload = *next;
            *next += 1;
        }
    }
    Ok(())
}

fn callable_signature(arguments: &[ArgumentDef]) -> String {
    format!(
        "{:?}",
        arguments
            .iter()
            .map(|argument| (&argument.type_, argument.optional, argument.variadic))
            .collect::<Vec<_>>()
    )
}

fn normalize_return_type(type_: ReturnType) -> TypeRef {
    match type_ {
        ReturnType::Undefined(_) => TypeRef::Unit,
        ReturnType::Type(type_) => normalize_type(type_),
    }
}

fn normalize_type(type_: Type) -> TypeRef {
    match type_ {
        Type::Single(SingleType::Any(_)) => TypeRef::Any,
        Type::Single(SingleType::NonAny(type_)) => normalize_non_any(type_),
        Type::Union(type_) => {
            let kind = TypeRef::Union(
                type_
                    .type_
                    .body
                    .list
                    .into_iter()
                    .map(normalize_union_member)
                    .collect(),
            );
            nullable(kind, type_.q_mark.is_some())
        }
    }
}

fn normalize_union_member(type_: UnionMemberType<'_>) -> TypeRef {
    match type_ {
        UnionMemberType::Single(type_) => normalize_non_any(type_.type_),
        UnionMemberType::Union(type_) => {
            let kind = TypeRef::Union(
                type_
                    .type_
                    .body
                    .list
                    .into_iter()
                    .map(normalize_union_member)
                    .collect(),
            );
            nullable(kind, type_.q_mark.is_some())
        }
    }
}

fn normalize_non_any(type_: NonAnyType<'_>) -> TypeRef {
    match type_ {
        NonAnyType::Promise(type_) => {
            TypeRef::Promise(Box::new(normalize_return_type(*type_.generics.body)))
        }
        NonAnyType::Integer(type_) => {
            let kind = match type_.type_ {
                IntegerType::Short(value) if value.unsigned.is_some() => TypeRef::U16,
                IntegerType::Short(_) => TypeRef::I16,
                IntegerType::Long(value) if value.unsigned.is_some() => TypeRef::U32,
                IntegerType::Long(_) => TypeRef::I32,
                IntegerType::LongLong(value) if value.unsigned.is_some() => TypeRef::U64,
                IntegerType::LongLong(_) => TypeRef::I64,
            };
            nullable(kind, type_.q_mark.is_some())
        }
        NonAnyType::FloatingPoint(type_) => {
            let kind = match type_.type_ {
                FloatingPointType::Float(_) => TypeRef::F32,
                FloatingPointType::Double(_) => TypeRef::F64,
            };
            nullable(kind, type_.q_mark.is_some())
        }
        NonAnyType::Boolean(type_) => nullable(TypeRef::Bool, type_.q_mark.is_some()),
        NonAnyType::Byte(type_) => nullable(TypeRef::I8, type_.q_mark.is_some()),
        NonAnyType::Octet(type_) => nullable(TypeRef::U8, type_.q_mark.is_some()),
        NonAnyType::ByteString(type_) => {
            nullable(TypeRef::String(StringKind::Byte), type_.q_mark.is_some())
        }
        NonAnyType::DOMString(type_) => {
            nullable(TypeRef::String(StringKind::Dom), type_.q_mark.is_some())
        }
        NonAnyType::USVString(type_) => {
            nullable(TypeRef::String(StringKind::Usv), type_.q_mark.is_some())
        }
        NonAnyType::Sequence(type_) => nullable(
            TypeRef::Sequence(Box::new(normalize_type(*type_.type_.generics.body))),
            type_.q_mark.is_some(),
        ),
        NonAnyType::FrozenArrayType(type_) => nullable(
            TypeRef::Sequence(Box::new(normalize_type(*type_.type_.generics.body))),
            type_.q_mark.is_some(),
        ),
        NonAnyType::RecordType(type_) => nullable(
            TypeRef::Record(Box::new(normalize_type(*type_.type_.generics.body.2))),
            type_.q_mark.is_some(),
        ),
        NonAnyType::Identifier(type_) => nullable(
            TypeRef::Named(type_.type_.0.to_owned()),
            type_.q_mark.is_some(),
        ),
        NonAnyType::Object(type_) => nullable(TypeRef::Object, type_.q_mark.is_some()),
        NonAnyType::Symbol(type_) => nullable(TypeRef::Symbol, type_.q_mark.is_some()),
        NonAnyType::Error(type_) => nullable(TypeRef::Error, type_.q_mark.is_some()),
        NonAnyType::ArrayBuffer(type_) => nullable(
            TypeRef::Buffer(BufferKind::ArrayBuffer),
            type_.q_mark.is_some(),
        ),
        NonAnyType::ArrayBufferView(type_) => nullable(
            TypeRef::Buffer(BufferKind::ArrayBufferView),
            type_.q_mark.is_some(),
        ),
        NonAnyType::BufferSource(type_) => nullable(
            TypeRef::Buffer(BufferKind::BufferSource),
            type_.q_mark.is_some(),
        ),
        NonAnyType::DataView(type_) => nullable(
            TypeRef::Buffer(BufferKind::DataView),
            type_.q_mark.is_some(),
        ),
        NonAnyType::Int8Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::I8), type_.q_mark.is_some())
        }
        NonAnyType::Int16Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::I16), type_.q_mark.is_some())
        }
        NonAnyType::Int32Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::I32), type_.q_mark.is_some())
        }
        NonAnyType::Uint8Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::U8), type_.q_mark.is_some())
        }
        NonAnyType::Uint16Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::U16), type_.q_mark.is_some())
        }
        NonAnyType::Uint32Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::U32), type_.q_mark.is_some())
        }
        NonAnyType::Uint8ClampedArray(type_) => nullable(
            TypeRef::Buffer(BufferKind::U8Clamped),
            type_.q_mark.is_some(),
        ),
        NonAnyType::Float32Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::F32), type_.q_mark.is_some())
        }
        NonAnyType::Float64Array(type_) => {
            nullable(TypeRef::Buffer(BufferKind::F64), type_.q_mark.is_some())
        }
    }
}

fn nullable(kind: TypeRef, is_nullable: bool) -> TypeRef {
    if is_nullable {
        TypeRef::Nullable(Box::new(kind))
    } else {
        kind
    }
}

/// Emit the currently representable raw Fe import layer.
///
/// This v0 emitter intentionally accepts only Wasm scalars and non-null
/// interface handles, matching Fe's existing general host-import ABI. Richer
/// Web IDL values remain in [`World`] but fail here with an exact diagnostic.
pub fn emit_fe_raw(world: &World, module: &str) -> Result<String, BindgenError> {
    emit_fe_import_layer(world, module, false)
}

/// Emit target-neutral flat host imports, including canonical UTF-8
/// `(pointer, byte-length)` string descriptors.
///
/// `BrowserString` is a borrowed descriptor. This layer deliberately emits
/// only the unsafe low-level declarations; a safe wrapper requires validated
/// guest memory and explicit result cleanup ownership.
pub fn emit_fe_flat_host_imports(world: &World, module: &str) -> Result<String, BindgenError> {
    emit_fe_import_layer(world, module, true)
}

fn emit_fe_import_layer(
    world: &World,
    module: &str,
    rich_flat_values: bool,
) -> Result<String, BindgenError> {
    let mut output = String::from("// @generated by fe-webidl-bindgen; do not edit.\n\n");
    if rich_flat_values {
        output.push_str(
            "use core::{BrowserLatin1String, BrowserList, BrowserString, BrowserUtf16String}\n\n",
        );
    }
    for interface in world.interfaces.values() {
        output.push_str(&format!(
            "pub struct {} {{ handle: u32 }}\n\n",
            fe_ident(&interface.name)
        ));
        if rich_flat_values
            && interface.members.iter().any(|member| {
                matches!(
                    member,
                    Member::Collection(CollectionDef {
                        kind: CollectionKind::Iterable {
                            key: None,
                            value: TypeRef::U32,
                        },
                        ..
                    })
                )
            })
        {
            let iterator = format!("{}Iterator", fe_ident(&interface.name));
            output.push_str(&format!(
                "pub struct {iterator} {{ handle: u32 }}\n\
                 pub enum {iterator}Option {{\n    None,\n    Some {{ value: u32 }}\n}}\n\
                 pub enum {iterator}Next {{\n    Ok {{ value: {iterator}Option }},\n    \
                 Error {{ error: BrowserString }}\n}}\n\n"
            ));
        }
        for member in &interface.members {
            let Member::Const(constant) = member else {
                continue;
            };
            let context = format!("interface `{}` const `{}`", interface.name, constant.name);
            let type_ = fe_import_type(world, &constant.type_, &context, rich_flat_values)?;
            let value = fe_const_literal(&constant.value, &context)?;
            output.push_str(&format!(
                "pub const {}_{}: {type_} = {value}\n",
                screaming_snake_case(&interface.name),
                screaming_snake_case(&constant.name),
            ));
        }
        if interface
            .members
            .iter()
            .any(|member| matches!(member, Member::Const(_)))
        {
            output.push('\n');
        }
    }
    for interface in world.interfaces.values() {
        let Some(parent) = &interface.inherits else {
            continue;
        };
        output.push_str(&format!(
            "pub fn {}_into_{}(self_: {}) -> {} {{\n    {} {{ handle: self_.handle }}\n}}\n\n",
            snake_case(&interface.name),
            snake_case(parent),
            fe_ident(&interface.name),
            fe_ident(parent),
            fe_ident(parent),
        ));
    }
    output.push_str(&format!("#[host_import(module = {module:?})]\nextern {{\n"));
    for interface in world.interfaces.values() {
        for member in &interface.members {
            match member {
                Member::Const(_) => {}
                Member::Collection(collection) => {
                    if matches!(collection.kind, CollectionKind::AsyncIterable { .. }) {
                        return Err(BindgenError::new(
                            format!("interface `{}` async iterable", interface.name),
                            "semantic host adapters support async iteration, but raw Fe emission \
                             remains gated on compiler-generated Future/await state machines",
                        ));
                    }
                    let CollectionKind::Iterable { key: None, value } = &collection.kind else {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` {}",
                                interface.name,
                                collection_kind_name(&collection.kind)
                            ),
                            "raw rich collection imports currently require single-valued synchronous iterable<T>",
                        ));
                    };
                    if !rich_flat_values || !matches!(resolve_typedef(world, value), TypeRef::U32) {
                        return Err(BindgenError::new(
                            format!("interface `{}` iterable", interface.name),
                            "raw rich iterator emission currently requires an exact u32 item type",
                        ));
                    }
                    let iterator = format!("{}Iterator", fe_ident(&interface.name));
                    output.push_str(&format!(
                        "    #[host_result(codec = \"fe:host-wasm-codec/v1\")]\n    \
                         pub unsafe fn {}_iterator_next(self_: {}) -> {}Next\n",
                        snake_case(&interface.name),
                        iterator,
                        iterator,
                    ));
                }
                Member::Constructor(constructor) => {
                    let function = constructor_import_name(interface, constructor);
                    let mut args = Vec::new();
                    for argument in &constructor.arguments {
                        if argument.optional || argument.variadic {
                            return Err(BindgenError::new(
                                format!("constructor `{function}` argument `{}`", argument.name),
                                "optional and variadic arguments need the rich adapter ABI",
                            ));
                        }
                        args.push(format!(
                            "{}: {}",
                            fe_ident(&argument.name),
                            fe_import_param_type(
                                world,
                                &argument.type_,
                                &function,
                                rich_flat_values,
                            )?
                        ));
                    }
                    output.push_str(&format!(
                        "    pub unsafe fn {function}({}) -> {}\n",
                        args.join(", "),
                        fe_ident(&interface.name),
                    ));
                }
                Member::Attribute(attribute) => {
                    if attribute.stringifier {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` stringifier attribute `{}`",
                                interface.name, attribute.name
                            ),
                            "stringifier coercion semantics require a dedicated host ABI lowering",
                        ));
                    }
                    let function = format!(
                        "{}_get_{}",
                        snake_case(&interface.name),
                        snake_case(&attribute.name)
                    );
                    let mut args = Vec::new();
                    if !attribute.static_ {
                        args.push(format!("self_: {}", fe_ident(&interface.name)));
                    }
                    let result =
                        fe_import_type(world, &attribute.type_, &function, rich_flat_values)?;
                    output.push_str(&format!(
                        "    pub unsafe fn {function}({}) -> {result}\n",
                        args.join(", ")
                    ));
                    if !attribute.read_only {
                        let value =
                            fe_import_type(world, &attribute.type_, &function, rich_flat_values)?;
                        let mut args = Vec::new();
                        if !attribute.static_ {
                            args.push(format!("self_: {}", fe_ident(&interface.name)));
                        }
                        args.push(format!("value: {value}"));
                        let setter = format!(
                            "{}_set_{}",
                            snake_case(&interface.name),
                            snake_case(&attribute.name)
                        );
                        output.push_str(&format!(
                            "    pub unsafe fn {setter}({})\n",
                            args.join(", ")
                        ));
                    }
                }
                Member::Operation(operation) => {
                    if operation.special != OperationSpecial::Regular {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` {}",
                                interface.name,
                                operation_special_name(operation.special)
                            ),
                            "property/index/string coercion semantics require a dedicated host ABI lowering",
                        ));
                    }
                    let suffix = if operation.overload > 0 {
                        format!("_{}", operation.overload)
                    } else {
                        String::new()
                    };
                    let function = format!(
                        "{}_{}{}",
                        snake_case(&interface.name),
                        snake_case(&operation.name),
                        suffix
                    );
                    let mut args = Vec::new();
                    if !operation.static_ {
                        args.push(format!("self_: {}", fe_ident(&interface.name)));
                    }
                    for argument in &operation.arguments {
                        if argument.optional || argument.variadic {
                            return Err(BindgenError::new(
                                format!("operation `{function}` argument `{}`", argument.name),
                                "optional and variadic arguments need the rich adapter ABI",
                            ));
                        }
                        args.push(format!(
                            "{}: {}",
                            fe_ident(&argument.name),
                            fe_import_param_type(
                                world,
                                &argument.type_,
                                &function,
                                rich_flat_values,
                            )?
                        ));
                    }
                    let result =
                        fe_import_type(world, &operation.result, &function, rich_flat_values)?;
                    let arrow = if result != "()" {
                        format!(" -> {result}")
                    } else {
                        String::new()
                    };
                    if rich_flat_values
                        && matches!(
                            resolve_typedef(world, &operation.result),
                            TypeRef::Sequence(_)
                        )
                    {
                        output.push_str("    #[host_result(codec = \"fe:host-wasm-codec/v1\")]\n");
                    }
                    output.push_str(&format!(
                        "    pub unsafe fn {function}({}){arrow}\n",
                        args.join(", ")
                    ));
                }
            }
        }
    }
    for namespace in world.namespaces.values() {
        for member in &namespace.members {
            match member {
                NamespaceMember::Attribute(attribute) => {
                    let function = format!(
                        "{}_get_{}",
                        snake_case(&namespace.name),
                        snake_case(&attribute.name)
                    );
                    let result =
                        fe_import_type(world, &attribute.type_, &function, rich_flat_values)?;
                    output.push_str(&format!("    pub unsafe fn {function}() -> {result}\n"));
                }
                NamespaceMember::Operation(operation) => {
                    let suffix = if operation.overload > 0 {
                        format!("_{}", operation.overload)
                    } else {
                        String::new()
                    };
                    let function = format!(
                        "{}_{}{}",
                        snake_case(&namespace.name),
                        snake_case(&operation.name),
                        suffix
                    );
                    let mut args = Vec::new();
                    for argument in &operation.arguments {
                        if argument.optional || argument.variadic {
                            return Err(BindgenError::new(
                                format!(
                                    "namespace operation `{function}` argument `{}`",
                                    argument.name
                                ),
                                "optional and variadic arguments need the rich adapter ABI",
                            ));
                        }
                        args.push(format!(
                            "{}: {}",
                            fe_ident(&argument.name),
                            fe_import_param_type(
                                world,
                                &argument.type_,
                                &function,
                                rich_flat_values,
                            )?
                        ));
                    }
                    let result =
                        fe_import_type(world, &operation.result, &function, rich_flat_values)?;
                    let arrow = if result != "()" {
                        format!(" -> {result}")
                    } else {
                        String::new()
                    };
                    output.push_str(&format!(
                        "    pub unsafe fn {function}({}){arrow}\n",
                        args.join(", ")
                    ));
                }
            }
        }
    }
    output.push_str("}\n");
    Ok(output)
}

/// Emit a JavaScript import adapter for the v0 scalar/handle ABI.
pub fn emit_js_adapter(world: &World, module: &str) -> Result<String, BindgenError> {
    // Reuse the Fe-side gate so the two generated halves cannot disagree about
    // what the current ABI represents.
    let _ = emit_fe_raw(world, module)?;
    let mut operations = Vec::new();
    for interface in world.interfaces.values() {
        for member in &interface.members {
            match member {
                Member::Const(_) => {}
                Member::Collection(collection) => {
                    return Err(BindgenError::new(
                        format!(
                            "interface `{}` {}",
                            interface.name,
                            collection_kind_name(&collection.kind)
                        ),
                        "collection iterator and mutation semantics require a dedicated JavaScript adapter lowering",
                    ));
                }
                Member::Constructor(constructor) => {
                    let function = constructor_import_name(interface, constructor);
                    let mut js_params = Vec::new();
                    let mut js_args = Vec::new();
                    for argument in &constructor.arguments {
                        if argument.optional || argument.variadic {
                            return Err(BindgenError::new(
                                format!("constructor `{function}` argument `{}`", argument.name),
                                "optional and variadic arguments need the rich adapter ABI",
                            ));
                        }
                        let name = js_ident(&argument.name);
                        js_params.push(name.clone());
                        js_args.push(js_unwrap_argument(world, &argument.type_, &name)?);
                    }
                    let host_name = constructor.name.as_deref().unwrap_or(&interface.name);
                    operations.push(format!(
                        "    {function}({}) {{ return handles.insert(new {host_name}({})); }}",
                        js_params.join(", "),
                        js_args.join(", "),
                    ));
                }
                Member::Attribute(attribute) => {
                    if attribute.stringifier {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` stringifier attribute `{}`",
                                interface.name, attribute.name
                            ),
                            "stringifier coercion semantics require a dedicated JavaScript adapter lowering",
                        ));
                    }
                    let getter = format!(
                        "{}_get_{}",
                        snake_case(&interface.name),
                        snake_case(&attribute.name)
                    );
                    operations.push(format!(
                        "    {getter}({receiver}) {{ return {wrapped}; }}",
                        receiver = if attribute.static_ { "" } else { "selfHandle" },
                        wrapped = js_wrap_result(
                            world,
                            &attribute.type_,
                            &format!(
                                "{}.{}",
                                if attribute.static_ {
                                    interface.name.clone()
                                } else {
                                    "handles.get(selfHandle)".to_owned()
                                },
                                attribute.name
                            )
                        )?,
                    ));
                    if !attribute.read_only {
                        let setter = format!(
                            "{}_set_{}",
                            snake_case(&interface.name),
                            snake_case(&attribute.name)
                        );
                        operations.push(format!(
                            "    {setter}({receiver}value) {{ {target}.{} = {}; }}",
                            attribute.name,
                            js_unwrap_argument(world, &attribute.type_, "value")?,
                            receiver = if attribute.static_ {
                                ""
                            } else {
                                "selfHandle, "
                            },
                            target = if attribute.static_ {
                                interface.name.clone()
                            } else {
                                "handles.get(selfHandle)".to_owned()
                            },
                        ));
                    }
                }
                Member::Operation(operation) => {
                    if operation.special != OperationSpecial::Regular {
                        return Err(BindgenError::new(
                            format!(
                                "interface `{}` {}",
                                interface.name,
                                operation_special_name(operation.special)
                            ),
                            "property/index/string coercion semantics require a dedicated JavaScript adapter lowering",
                        ));
                    }
                    let suffix = if operation.overload > 0 {
                        format!("_{}", operation.overload)
                    } else {
                        String::new()
                    };
                    let function = format!(
                        "{}_{}{}",
                        snake_case(&interface.name),
                        snake_case(&operation.name),
                        suffix
                    );
                    let mut js_params = Vec::new();
                    let mut js_args = Vec::new();
                    if !operation.static_ {
                        js_params.push("selfHandle".to_owned());
                    }
                    for argument in &operation.arguments {
                        if argument.optional || argument.variadic {
                            return Err(BindgenError::new(
                                format!("operation `{function}` argument `{}`", argument.name),
                                "optional and variadic arguments need the rich adapter ABI",
                            ));
                        }
                        let name = js_ident(&argument.name);
                        js_params.push(name.clone());
                        js_args.push(js_unwrap_argument(world, &argument.type_, &name)?);
                    }
                    let target = if operation.static_ {
                        interface.name.clone()
                    } else {
                        "handles.get(selfHandle)".to_owned()
                    };
                    let call = format!("{target}.{}({})", operation.name, js_args.join(", "));
                    let body = if operation.result == TypeRef::Unit {
                        format!("{call};")
                    } else {
                        format!(
                            "return {};",
                            js_wrap_result(world, &operation.result, &call)?
                        )
                    };
                    operations.push(format!(
                        "    {function}({}) {{ {body} }}",
                        js_params.join(", ")
                    ));
                }
            }
        }
    }
    for namespace in world.namespaces.values() {
        for member in &namespace.members {
            match member {
                NamespaceMember::Attribute(attribute) => {
                    let function = format!(
                        "{}_get_{}",
                        snake_case(&namespace.name),
                        snake_case(&attribute.name)
                    );
                    operations.push(format!(
                        "    {function}() {{ return {}; }}",
                        js_wrap_result(
                            world,
                            &attribute.type_,
                            &format!("{}.{name}", namespace.name, name = attribute.name),
                        )?
                    ));
                }
                NamespaceMember::Operation(operation) => {
                    let suffix = if operation.overload > 0 {
                        format!("_{}", operation.overload)
                    } else {
                        String::new()
                    };
                    let function = format!(
                        "{}_{}{}",
                        snake_case(&namespace.name),
                        snake_case(&operation.name),
                        suffix
                    );
                    let mut params = Vec::new();
                    let mut args = Vec::new();
                    for argument in &operation.arguments {
                        if argument.optional || argument.variadic {
                            return Err(BindgenError::new(
                                format!(
                                    "namespace operation `{function}` argument `{}`",
                                    argument.name
                                ),
                                "optional and variadic arguments need the rich adapter ABI",
                            ));
                        }
                        let name = js_ident(&argument.name);
                        params.push(name.clone());
                        args.push(js_unwrap_argument(world, &argument.type_, &name)?);
                    }
                    let call =
                        format!("{}.{}({})", namespace.name, operation.name, args.join(", "));
                    let body = if operation.result == TypeRef::Unit {
                        format!("{call};")
                    } else {
                        format!(
                            "return {};",
                            js_wrap_result(world, &operation.result, &call)?
                        )
                    };
                    operations.push(format!(
                        "    {function}({}) {{ {body} }}",
                        params.join(", ")
                    ));
                }
            }
        }
    }
    Ok(format!(
        "// @generated by fe-webidl-bindgen; do not edit.\n\
         export class FeResourceTable {{\n  \
         #next = 1;\n  #values = new Map();\n  \
         insert(value) {{ const handle = this.#next++; this.#values.set(handle, value); return handle; }}\n  \
         get(handle) {{ const value = this.#values.get(handle); if (value === undefined) throw new TypeError(`invalid Fe host handle ${{handle}}`); return value; }}\n  \
         drop(handle) {{ if (!this.#values.delete(handle)) throw new TypeError(`invalid Fe host handle ${{handle}}`); }}\n\
         }}\n\n\
         export function createFeWebImports(handles = new FeResourceTable()) {{\n  \
         return {{\n  {module:?}: {{\n{}\n  }}\n  }};\n\
         }}\n",
        operations.join(",\n")
    ))
}

fn fe_abi_type(world: &World, type_: &TypeRef, context: &str) -> Result<String, BindgenError> {
    fe_import_type(world, type_, context, false)
}

fn fe_import_type(
    world: &World,
    type_: &TypeRef,
    context: &str,
    rich_flat_values: bool,
) -> Result<String, BindgenError> {
    let type_ = resolve_typedef(world, type_);
    let result = match type_ {
        TypeRef::Unit => "()",
        TypeRef::Bool => "bool",
        TypeRef::I8 => "i8",
        TypeRef::U8 => "u8",
        TypeRef::I16 => "i16",
        TypeRef::U16 => "u16",
        TypeRef::I32 => "i32",
        TypeRef::U32 => "u32",
        TypeRef::I64 => "i64",
        TypeRef::U64 => "u64",
        TypeRef::F32 => "f32",
        TypeRef::F64 => {
            return Err(BindgenError::new(
                context,
                "Web IDL `double` is not supported by Fe's Wasm host-import ABI",
            ));
        }
        TypeRef::String(StringKind::Byte) if rich_flat_values => "BrowserLatin1String",
        TypeRef::String(StringKind::Dom) if rich_flat_values => "BrowserUtf16String",
        TypeRef::String(StringKind::Usv) if rich_flat_values => "BrowserString",
        TypeRef::Named(name) if world.interfaces.contains_key(name) => return Ok(fe_ident(name)),
        TypeRef::Sequence(inner) if rich_flat_values => {
            let item = fe_import_type(world, inner, context, rich_flat_values)?;
            return Ok(format!("BrowserList<{item}, 0>"));
        }
        other => {
            return Err(BindgenError::new(
                context,
                format!("Web IDL type `{other:?}` needs the rich adapter ABI"),
            ));
        }
    };
    Ok(result.to_owned())
}

fn fe_import_param_type(
    world: &World,
    type_: &TypeRef,
    context: &str,
    rich_flat_values: bool,
) -> Result<String, BindgenError> {
    let value = fe_import_type(world, type_, context, rich_flat_values)?;
    if rich_flat_values && matches!(resolve_typedef(world, type_), TypeRef::String(_)) {
        Ok(format!("own {value}"))
    } else {
        Ok(value)
    }
}

fn resolve_typedef<'a>(world: &'a World, mut type_: &'a TypeRef) -> &'a TypeRef {
    // Cycles are rejected by `parse`, so following direct aliases terminates.
    while let TypeRef::Named(name) = type_ {
        let Some(typedef) = world.typedefs.get(name) else {
            break;
        };
        type_ = &typedef.type_;
    }
    type_
}

fn js_unwrap_argument(
    world: &World,
    type_: &TypeRef,
    expression: &str,
) -> Result<String, BindgenError> {
    let _ = fe_abi_type(world, type_, "emit JavaScript argument")?;
    Ok(match resolve_typedef(world, type_) {
        TypeRef::Named(name) if world.interfaces.contains_key(name) => {
            format!("handles.get({expression})")
        }
        _ => expression.to_owned(),
    })
}

fn js_wrap_result(
    world: &World,
    type_: &TypeRef,
    expression: &str,
) -> Result<String, BindgenError> {
    let _ = fe_abi_type(world, type_, "emit JavaScript result")?;
    Ok(match resolve_typedef(world, type_) {
        TypeRef::Named(name) if world.interfaces.contains_key(name) => {
            format!("handles.insert({expression})")
        }
        _ => expression.to_owned(),
    })
}

fn fe_const_literal(value: &DefaultValueDef, context: &str) -> Result<String, BindgenError> {
    match value {
        DefaultValueDef::Bool(value) => Ok(value.to_string()),
        DefaultValueDef::Integer(value) => Ok(value.clone()),
        DefaultValueDef::Float(value)
            if !matches!(value.as_str(), "Infinity" | "-Infinity" | "NaN") =>
        {
            Ok(value.clone())
        }
        DefaultValueDef::Float(_) => Err(BindgenError::new(
            context,
            "non-finite Web IDL constants have no portable Fe literal",
        )),
        DefaultValueDef::String(_)
        | DefaultValueDef::Null
        | DefaultValueDef::EmptySequence
        | DefaultValueDef::EmptyDictionary => Err(BindgenError::new(
            context,
            "constant value is not representable as a Fe literal",
        )),
    }
}

fn screaming_snake_case(name: &str) -> String {
    if name
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        name.to_owned()
    } else {
        snake_case(name).to_ascii_uppercase()
    }
}

fn constructor_import_name(interface: &InterfaceDef, constructor: &ConstructorDef) -> String {
    let base = match &constructor.name {
        Some(name) => format!("{}_new_{}", snake_case(&interface.name), snake_case(name)),
        None => format!("{}_new", snake_case(&interface.name)),
    };
    if constructor.overload > 0 {
        format!("{base}_{}", constructor.overload)
    } else {
        base
    }
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

fn fe_ident(name: &str) -> String {
    let ident = if name == "self" {
        "self_".to_owned()
    } else {
        name.replace('-', "_")
    };
    match ident.as_str() {
        "actor" | "const" | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl"
        | "let" | "match" | "mod" | "pub" | "return" | "struct" | "trait" | "true" | "type"
        | "unsafe" | "use" | "while" => format!("{ident}_"),
        _ => ident,
    }
}

fn js_ident(name: &str) -> String {
    match name {
        "delete" | "function" | "new" | "return" | "this" | "var" => format!("{name}_"),
        _ => name.replace('-', "_"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &str = r#"
        interface EventTarget {
            undefined dispatchEvent(boolean trusted);
        };

        [Exposed=Window]
        interface Window : EventTarget {
            readonly attribute unsigned long innerWidth;
            EventTarget parentTarget();
            undefined resizeTo(long width, long height);
            undefined resizeTo(unsigned long width, unsigned long height);
        };

        partial interface Window {
            attribute boolean closed;
        };
    "#;

    #[test]
    fn links_partials_inheritance_and_overloads_deterministically() {
        let world = parse(BASIC).unwrap();
        assert_eq!(
            world.interfaces.keys().cloned().collect::<Vec<_>>(),
            ["EventTarget", "Window"]
        );
        let window = &world.interfaces["Window"];
        assert_eq!(window.inherits.as_deref(), Some("EventTarget"));
        assert_eq!(window.members.len(), 5);
        let overloads = window
            .members
            .iter()
            .filter_map(|member| match member {
                Member::Operation(operation) if operation.name == "resizeTo" => {
                    Some(operation.overload)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(overloads, [0, 1]);
    }

    #[test]
    fn emits_raw_fe_imports_and_js_handle_adapters() {
        let world = parse(BASIC).unwrap();
        let fe = emit_fe_raw(&world, "fe:web").unwrap();
        assert!(fe.contains("pub struct Window { handle: u32 }"));
        assert!(fe.contains("#[host_import(module = \"fe:web\")]"));
        assert!(!fe.contains("wasm_import"));
        assert!(fe.contains("pub unsafe fn window_get_inner_width(self_: Window) -> u32"));
        assert!(fe.contains("pub unsafe fn window_parent_target(self_: Window) -> EventTarget"));
        assert!(
            fe.contains("pub unsafe fn window_resize_to_1(self_: Window, width: u32, height: u32)")
        );

        let js = emit_js_adapter(&world, "fe:web").unwrap();
        assert!(js.contains("\"fe:web\": {"));
        assert!(js.contains(
            "window_parent_target(selfHandle) { return handles.insert(handles.get(selfHandle).parentTarget()); }"
        ));
        assert!(js.contains(
            "window_set_closed(selfHandle, value) { handles.get(selfHandle).closed = value; }"
        ));
    }

    #[test]
    fn checked_in_std_web_raw_is_generated_scalar_resource_subset() {
        let source = include_str!("../../../ingots/std/web-minimal.webidl");
        let world = parse(source).expect("minimal std Web IDL should parse");
        let fe = emit_fe_raw(&world, "fe:web").expect("scalar/resource subset should emit");
        assert_eq!(
            fe,
            include_str!("../../../ingots/std/src/web/raw.fe"),
            "the std facade's raw layer must remain generator-owned"
        );
        assert!(fe.contains("#[host_import(module = \"fe:web\")]"));
        assert!(fe.contains("window_get_document(self_: Window) -> Document"));
        assert!(fe.contains("element_get_child_element_count(self_: Element) -> u32"));
        assert!(fe.contains("element_get_hidden(self_: Element) -> bool"));
        for unsupported in ["BrowserString", "Callback", "Pending", "Wait"] {
            assert!(
                !fe.contains(unsupported),
                "minimal executable facade must fail closed before `{unsupported}` enters it"
            );
        }

        let js = emit_js_adapter(&world, "fe:web").expect("v0 adapter should emit");
        assert!(js.contains(
            "window_get_document(selfHandle) { return handles.insert(handles.get(selfHandle).document); }"
        ));
        assert!(js.contains(
            "element_get_child_element_count(selfHandle) { return handles.get(selfHandle).childElementCount; }"
        ));
    }

    #[test]
    fn emits_target_neutral_flat_string_host_imports() {
        let world = parse("interface Channel { DOMString echo(DOMString value); };").unwrap();
        let fe = emit_fe_flat_host_imports(&world, "fe:web").unwrap();
        assert!(fe.contains("BrowserUtf16String"));
        assert!(fe.contains("#[host_import(module = \"fe:web\")]"));
        assert!(fe.contains(
            "pub unsafe fn channel_echo(self_: Channel, value: own BrowserUtf16String) -> BrowserUtf16String"
        ));
        assert!(!fe.contains("wasm_import"));
        assert!(!fe.contains("WebAssembly"));
    }

    #[test]
    fn retains_and_emits_event_constants_without_creating_host_calls() {
        let source = include_str!("../tests/fixtures/event-constants.webidl");
        let world = parse(source).unwrap();
        let event = &world.interfaces["Event"];
        assert_eq!(
            event
                .members
                .iter()
                .filter_map(|member| match member {
                    Member::Const(constant) => {
                        Some((&constant.name, &constant.type_, &constant.value))
                    }
                    Member::Constructor(_)
                    | Member::Collection(_)
                    | Member::Attribute(_)
                    | Member::Operation(_) => None,
                })
                .collect::<Vec<_>>(),
            [
                (
                    &"NONE".to_owned(),
                    &TypeRef::U16,
                    &DefaultValueDef::Integer("0".to_owned()),
                ),
                (
                    &"CAPTURING_PHASE".to_owned(),
                    &TypeRef::U16,
                    &DefaultValueDef::Integer("1".to_owned()),
                ),
                (
                    &"AT_TARGET".to_owned(),
                    &TypeRef::U16,
                    &DefaultValueDef::Integer("2".to_owned()),
                ),
                (
                    &"BUBBLING_PHASE".to_owned(),
                    &TypeRef::U16,
                    &DefaultValueDef::Integer("3".to_owned()),
                ),
            ]
        );

        let first = emit_fe_flat_host_imports(&world, "fe:web").unwrap();
        let second = emit_fe_flat_host_imports(&parse(source).unwrap(), "fe:web").unwrap();
        assert_eq!(first, second);
        for declaration in [
            "pub const EVENT_NONE: u16 = 0",
            "pub const EVENT_CAPTURING_PHASE: u16 = 1",
            "pub const EVENT_AT_TARGET: u16 = 2",
            "pub const EVENT_BUBBLING_PHASE: u16 = 3",
        ] {
            assert!(
                first.contains(declaration),
                "missing `{declaration}` in:\n{first}"
            );
        }

        let abi = lower_host_abi(&world, &HostAbiOptions::new("web-test")).unwrap();
        assert_eq!(abi.resources[0].name, "Event");
        assert_eq!(abi.resources[0].methods.len(), 1);
        assert_eq!(abi.resources[0].methods[0].name, "get-type");
    }

    #[test]
    fn constants_fail_with_exact_member_context_when_not_representable() {
        let error = parse("interface Event { const unsigned short BAD = null; };").unwrap_err();
        assert_eq!(error.context, "interface `Event` const `BAD`");
        assert!(error.detail.contains("incompatible"), "{error}");

        let world = parse("interface Metrics { const float BAD = NaN; };").unwrap();
        let error = emit_fe_raw(&world, "fe:web").unwrap_err();
        assert_eq!(error.context, "interface `Metrics` const `BAD`");
        assert!(error.detail.contains("non-finite"), "{error}");

        let error = parse(
            "interface Event { const unsigned short SAME = 1; }; partial interface Event { const unsigned short SAME = 2; };",
        )
        .unwrap_err();
        assert_eq!(error.context, "interface `Event` const `SAME`");
        assert!(error.detail.contains("duplicate constant"), "{error}");
    }

    #[test]
    fn constructors_retain_overloads_exposure_and_named_identity() {
        let source = include_str!("../tests/fixtures/event-constructors.webidl");
        let world = parse(source).unwrap();
        let constructors = world.interfaces["Event"]
            .members
            .iter()
            .filter_map(|member| match member {
                Member::Constructor(constructor) => Some(constructor),
                Member::Const(_)
                | Member::Collection(_)
                | Member::Attribute(_)
                | Member::Operation(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(constructors.len(), 3);
        assert_eq!(constructors[0].name.as_deref(), Some("LegacyEvent"));
        assert_eq!(constructors[0].overload, 0);
        assert_eq!(constructors[1].name, None);
        assert_eq!(constructors[1].overload, 0);
        assert_eq!(constructors[2].name, None);
        assert_eq!(constructors[2].overload, 1);
        assert!(constructors[2].attributes.secure_context);

        let fe = emit_fe_flat_host_imports(&world, "fe:web").unwrap();
        for declaration in [
            "event_new_legacy_event(type_: own BrowserUtf16String) -> Event",
            "event_new(type_: own BrowserUtf16String) -> Event",
            "event_new_1(type_: own BrowserUtf16String, bubbles: bool) -> Event",
        ] {
            assert!(
                fe.contains(declaration),
                "missing `{declaration}` in:\n{fe}"
            );
        }

        let lowering =
            lower_host_abi_with_metadata(&world, &HostAbiOptions::new("web-test")).unwrap();
        assert_eq!(
            lowering.world.resources[0]
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            [
                "constructor",
                "constructor-1",
                "get-type",
                "named-constructor-LegacyEvent"
            ]
        );
        assert!(
            lowering.world.resources[0]
                .methods
                .iter()
                .filter(|method| method.name.starts_with("constructor")
                    || method.name.starts_with("named-constructor"))
                .all(|method| method.receiver == fe_host_abi::Receiver::Static)
        );
        assert!(lowering.exposures.iter().any(|binding| {
            binding.definition == "interface/Event/constructor-1"
                && binding.attributes.secure_context
        }));

        let plan = build_adapter_plan(&world, "web-test", "fe:web").unwrap();
        let js = emit_js_canonical_adapter(&world, &plan).unwrap();
        assert!(js.contains("new host.interfaces[\"Event\"]("), "{js}");
        assert!(js.contains("new host.interfaces[\"LegacyEvent\"]("), "{js}");
        assert!(js.contains("runtime.resources.insert"), "{js}");

        let raw_error = emit_fe_raw(&world, "fe:web").unwrap_err();
        assert!(
            raw_error.context.contains("event_new_legacy_event"),
            "{raw_error}"
        );
        assert!(raw_error.detail.contains("rich adapter ABI"), "{raw_error}");
    }

    #[test]
    fn duplicate_constructor_signatures_fail_with_interface_context() {
        let error = parse(
            "interface Event { constructor(DOMString type); constructor(DOMString other); };",
        )
        .unwrap_err();
        assert_eq!(error.context, "interface `Event` constructor `constructor`");
        assert!(
            error
                .detail
                .contains("duplicate constructor overload signature"),
            "{error}"
        );

        let error =
            parse("interface Event {}; partial interface Event { constructor(DOMString type); };")
                .unwrap_err();
        assert_eq!(error.context, "partial interface `Event` constructor");
        assert!(error.detail.contains("non-partial interface"), "{error}");
    }

    #[test]
    fn links_namespaces_as_free_host_functions_without_resource_semantics() {
        let source = include_str!("../tests/fixtures/console-namespace.webidl");
        let world = parse(source).unwrap();
        let namespace = &world.namespaces["console"];
        assert_eq!(namespace.members.len(), 4);
        assert_eq!(
            namespace.attributes.exposed,
            Some(vec!["Window".to_owned(), "Worker".to_owned()])
        );
        let log_overloads = namespace
            .members
            .iter()
            .filter_map(|member| match member {
                NamespaceMember::Operation(operation) if operation.name == "log" => {
                    Some((operation.overload, operation.attributes.secure_context))
                }
                NamespaceMember::Attribute(_) | NamespaceMember::Operation(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(log_overloads, [(0, false), (1, true)]);

        let fe = emit_fe_flat_host_imports(&world, "fe:web").unwrap();
        for declaration in [
            "console_get_level() -> u32",
            "console_log(message: own BrowserUtf16String)",
            "console_log_1(value: bool)",
            "console_time_stamp(label: own BrowserUtf16String) -> u32",
        ] {
            assert!(
                fe.contains(declaration),
                "missing `{declaration}` in:\n{fe}"
            );
        }

        let lowering =
            lower_host_abi_with_metadata(&world, &HostAbiOptions::new("web-test")).unwrap();
        assert!(lowering.world.resources.is_empty());
        assert_eq!(
            lowering
                .world
                .imports
                .iter()
                .map(|function| { (function.namespace.as_str(), function.name.as_str()) })
                .collect::<Vec<_>>(),
            [
                ("console", "get-level"),
                ("console", "log"),
                ("console", "log-1"),
                ("console", "timeStamp"),
            ]
        );
        assert!(lowering.exposures.iter().any(|binding| {
            binding.definition == "namespace/console/log-1" && binding.attributes.secure_context
        }));

        let plan = build_adapter_plan(&world, "web-test", "fe:web").unwrap();
        assert!(plan.resources.is_empty());
        assert_eq!(plan.namespaces.len(), 1);
        let js = emit_js_canonical_adapter(&world, &plan).unwrap();
        assert!(
            js.contains("host.namespaces[\"console\"][\"level\"]"),
            "{js}"
        );
        assert!(
            js.contains("host.namespaces[\"console\"][\"log\"]("),
            "{js}"
        );
        assert!(!js.contains("resources.borrow(selfHandle)"), "{js}");
    }

    #[test]
    fn namespace_linking_rejects_orphans_and_duplicate_members_contextually() {
        let error = parse("partial namespace console { undefined log(); };").unwrap_err();
        assert_eq!(error.context, "partial namespace `console`");
        assert!(
            error.detail.contains("no non-partial definition"),
            "{error}"
        );

        let error = parse(
            "namespace console { readonly attribute boolean enabled; }; partial namespace console { readonly attribute boolean enabled; };",
        )
        .unwrap_err();
        assert_eq!(error.context, "namespace `console` attribute `enabled`");
        assert!(error.detail.contains("partial namespace"), "{error}");

        let error =
            parse("namespace console { undefined log(long value); undefined log(long other); };")
                .unwrap_err();
        assert_eq!(error.context, "namespace `console` operation `log`");
        assert!(
            error.detail.contains("duplicate overload signature"),
            "{error}"
        );
    }

    #[test]
    fn retains_collections_and_lowers_single_iterable_to_iterator_resource() {
        let source = include_str!("../tests/fixtures/web-collections.webidl");
        let world = parse(source).unwrap();

        let collection = |interface: &str| {
            world.interfaces[interface]
                .members
                .iter()
                .find_map(|member| match member {
                    Member::Collection(collection) => Some(collection),
                    Member::Const(_)
                    | Member::Constructor(_)
                    | Member::Attribute(_)
                    | Member::Operation(_) => None,
                })
                .unwrap()
        };
        assert_eq!(
            collection("URLSearchParams").kind,
            CollectionKind::Iterable {
                key: Some(TypeRef::String(StringKind::Dom)),
                value: TypeRef::String(StringKind::Dom),
            }
        );
        assert_eq!(
            collection("DOMTokenList").kind,
            CollectionKind::Iterable {
                key: None,
                value: TypeRef::String(StringKind::Dom),
            }
        );
        assert_eq!(
            collection("ReadonlyRegistry").kind,
            CollectionKind::Maplike {
                key: TypeRef::String(StringKind::Dom),
                value: TypeRef::String(StringKind::Dom),
                read_only: true,
            }
        );
        assert_eq!(
            collection("MutableFeatureSet").kind,
            CollectionKind::Setlike {
                value: TypeRef::String(StringKind::Dom),
                read_only: false,
            }
        );
        assert_eq!(world, parse(source).unwrap());

        let one = parse(
            "[Exposed=Window] interface DOMTokenList { [SecureContext] iterable<DOMString>; };",
        )
        .unwrap();
        let Member::Collection(retained) = &one.interfaces["DOMTokenList"].members[0] else {
            panic!("expected retained collection");
        };
        assert!(retained.attributes.secure_context);
        let lowering =
            lower_host_abi_with_metadata(&one, &HostAbiOptions::new("web-test")).unwrap();
        assert_eq!(
            lowering.iterators,
            [IteratorBinding {
                interface: "DOMTokenList".to_owned(),
                resource: "DOMTokenListIterator".to_owned(),
                item: IteratorItemBinding::Value(TypeRef::String(StringKind::Dom)),
                mutations: Vec::new(),
            }]
        );
        let interface = lowering
            .world
            .resources
            .iter()
            .find(|resource| resource.name == "DOMTokenList")
            .unwrap();
        assert_eq!(interface.methods[0].name, "iterator");
        assert_eq!(interface.methods[0].receiver, fe_host_abi::Receiver::Borrow);
        let iterator = lowering
            .world
            .resources
            .iter()
            .find(|resource| resource.name == "DOMTokenListIterator")
            .unwrap();
        assert_eq!(iterator.methods[0].name, "next");
        assert_eq!(iterator.methods[0].receiver, fe_host_abi::Receiver::Borrow);
        assert!(matches!(
            iterator.methods[0].signature.result,
            Some(fe_host_abi::Type::Result(_))
        ));
        let error = emit_fe_flat_host_imports(&one, "fe:web").unwrap_err();
        assert_eq!(error.context, "interface `DOMTokenList` iterable");
        assert!(error.detail.contains("exact u32 item type"), "{error}");
        let scalar = parse("interface Counters { iterable<unsigned long>; };").unwrap();
        let scalar_lowering =
            lower_host_abi_with_metadata(&scalar, &HostAbiOptions::new("web-test")).unwrap();
        let iterator = scalar_lowering
            .world
            .resources
            .iter()
            .find(|resource| resource.name == "CountersIterator")
            .unwrap();
        let mut signature = iterator.methods[0].signature.clone();
        signature.params.insert(
            0,
            fe_host_abi::Param {
                name: "self".to_owned(),
                type_: fe_host_abi::Type::Handle(fe_host_abi::Handle {
                    resource: "CountersIterator".to_owned(),
                    ownership: fe_host_abi::HandleOwnership::Borrow,
                }),
            },
        );
        let codec = fe_host_wasm_codec::function_plan(
            &scalar_lowering.world,
            &fe_host_abi::Function {
                namespace: "CountersIterator".to_owned(),
                name: "next".to_owned(),
                signature,
            },
            fe_host_wasm_codec::BoundaryDirection::GuestToHost,
        )
        .unwrap();
        assert!(matches!(
            codec.result.as_ref().unwrap().layout.flat,
            fe_host_wasm_codec::Flattening::Indirect
        ));
        assert!(
            codec
                .requirements
                .contains(&fe_host_wasm_codec::PlanRequirement::Realloc)
        );
        assert!(
            codec
                .requirements
                .contains(&fe_host_wasm_codec::PlanRequirement::PostReturn)
        );
        let generated = emit_fe_flat_host_imports(&scalar, "fe:web").unwrap();
        assert!(
            generated.contains("#[host_result(codec = \"fe:host-wasm-codec/v1\")]"),
            "{generated}"
        );
        assert!(
            generated.contains(
                "pub unsafe fn counters_iterator_next(self_: CountersIterator) -> \
                 CountersIteratorNext"
            ),
            "{generated}"
        );
        let plan = build_adapter_plan(&one, "web-test", "fe:web").unwrap();
        assert_eq!(plan.iterators.len(), 1);
        let js = emit_js_canonical_adapter(&one, &plan).unwrap();
        assert!(js.contains("[Symbol.iterator]()"), "{js}");
        assert!(
            js.contains("runtime.resources.borrow(selfHandle).next()"),
            "{js}"
        );
        assert!(js.contains("step.done ? null"), "{js}");
        assert!(js.contains("error: String(error)"), "{js}");

        let pair = parse("interface Entries { iterable<DOMString, DOMString>; };").unwrap();
        let pair_lowering =
            lower_host_abi_with_metadata(&pair, &HostAbiOptions::new("web-test")).unwrap();
        assert_eq!(
            pair_lowering.iterators[0].item,
            IteratorItemBinding::Entry {
                record: "EntriesIteratorEntry".to_owned(),
                key: TypeRef::String(StringKind::Dom),
                value: TypeRef::String(StringKind::Dom),
            }
        );
        let entry = pair_lowering
            .world
            .types
            .iter()
            .find(|type_| type_.name == "EntriesIteratorEntry")
            .unwrap();
        let fe_host_abi::TypeDefKind::Record { fields } = &entry.kind else {
            panic!("expected named iterator entry record");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["key", "value"]
        );
        let pair_plan = build_adapter_plan(&pair, "web-test", "fe:web").unwrap();
        let pair_js = emit_js_canonical_adapter(&pair, &pair_plan).unwrap();
        assert!(pair_js.contains("entry.length !== 2"), "{pair_js}");
        assert!(pair_js.contains("return { key:"), "{pair_js}");

        let readonly = parse(
            "interface Registry { readonly maplike<DOMString, unsigned long>; }; interface FeatureSet { readonly setlike<DOMString>; };",
        )
        .unwrap();
        let readonly_lowering =
            lower_host_abi_with_metadata(&readonly, &HostAbiOptions::new("web-test")).unwrap();
        assert_eq!(readonly_lowering.iterators.len(), 2);
        assert_eq!(
            readonly_lowering
                .world
                .resources
                .iter()
                .find(|resource| resource.name == "Registry")
                .unwrap()
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            [
                "collection-get",
                "collection-has",
                "collection-size",
                "iterator"
            ]
        );
        assert_eq!(
            readonly_lowering
                .world
                .resources
                .iter()
                .find(|resource| resource.name == "FeatureSet")
                .unwrap()
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            ["collection-has", "collection-size", "iterator"]
        );
        let readonly_plan = build_adapter_plan(&readonly, "web-test", "fe:web").unwrap();
        assert_eq!(readonly_plan.collections.len(), 2);
        let readonly_js = emit_js_canonical_adapter(&readonly, &readonly_plan).unwrap();
        assert!(readonly_js.contains(".size;"), "{readonly_js}");
        assert!(readonly_js.contains(".has("), "{readonly_js}");
        assert!(readonly_js.contains(".get("), "{readonly_js}");

        let mutable = parse(
            "interface MutableMap { maplike<DOMString, DOMString>; }; interface MutableSet { setlike<DOMString>; };",
        )
        .unwrap();
        let mutable_lowering =
            lower_host_abi_with_metadata(&mutable, &HostAbiOptions::new("web-test")).unwrap();
        assert_eq!(
            mutable_lowering.iterators[0].mutations,
            [
                IteratorMutation::MapSet,
                IteratorMutation::Delete,
                IteratorMutation::Clear,
            ]
        );
        assert_eq!(
            mutable_lowering
                .world
                .resources
                .iter()
                .find(|resource| resource.name == "MutableMap")
                .unwrap()
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            [
                "collection-clear",
                "collection-delete",
                "collection-get",
                "collection-has",
                "collection-set",
                "collection-size",
                "iterator",
            ]
        );
        let set = mutable_lowering
            .world
            .resources
            .iter()
            .find(|resource| resource.name == "MutableSet")
            .unwrap();
        assert_eq!(
            set.methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            [
                "collection-add",
                "collection-clear",
                "collection-delete",
                "collection-has",
                "collection-size",
                "iterator",
            ]
        );
        assert_eq!(
            set.methods
                .iter()
                .find(|method| method.name == "collection-add")
                .unwrap()
                .receiver,
            fe_host_abi::Receiver::Own
        );
        let mutable_plan = build_adapter_plan(&mutable, "web-test", "fe:web").unwrap();
        let mutable_js = emit_js_canonical_adapter(&mutable, &mutable_plan).unwrap();
        assert!(
            mutable_js.contains("runtime.resources.take(selfHandle)"),
            "{mutable_js}"
        );
        assert!(mutable_js.contains("must return this"), "{mutable_js}");
        assert!(mutable_js.contains("error: String(error)"), "{mutable_js}");
        let error = emit_fe_flat_host_imports(&mutable, "fe:web").unwrap_err();
        assert_eq!(error.context, "interface `MutableMap` maplike");

        let asynchronous = parse(
            "interface Updates { async iterable<DOMString, unsigned long>(optional boolean fresh = true); };",
        )
        .unwrap();
        let Member::Collection(collection) = &asynchronous.interfaces["Updates"].members[0] else {
            panic!("expected async iterable");
        };
        let CollectionKind::AsyncIterable {
            key,
            value,
            arguments,
        } = &collection.kind
        else {
            panic!("expected async iterable shape");
        };
        assert_eq!(key, &Some(TypeRef::String(StringKind::Dom)));
        assert_eq!(value, &TypeRef::U32);
        assert_eq!(arguments[0].default_, Some(DefaultValueDef::Bool(true)));
    }

    #[test]
    fn rejects_multiple_collection_declarations_with_member_context() {
        let error = parse(
            "interface Bag { iterable<DOMString>; }; partial interface Bag { setlike<DOMString>; };",
        )
        .unwrap_err();
        assert_eq!(error.context, "interface `Bag` setlike");
        assert!(error.detail.contains("already has iterable"), "{error}");
    }

    #[test]
    fn retains_special_operation_and_stringifier_identity() {
        let source = include_str!("../tests/fixtures/special-operations.webidl");
        let world = parse(source).unwrap();
        let specials = |interface: &str| {
            world.interfaces[interface]
                .members
                .iter()
                .filter_map(|member| match member {
                    Member::Operation(operation) => Some((
                        operation.name.as_str(),
                        operation.special,
                        operation.arguments.len(),
                    )),
                    Member::Const(_)
                    | Member::Constructor(_)
                    | Member::Collection(_)
                    | Member::Attribute(_) => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            specials("DOMStringMap"),
            [
                ("getter", OperationSpecial::Getter, 1),
                ("setter", OperationSpecial::Setter, 2),
                ("deleter", OperationSpecial::Deleter, 1),
            ]
        );
        assert_eq!(
            specials("Storage"),
            [
                ("getItem", OperationSpecial::Getter, 1),
                ("setItem", OperationSpecial::Setter, 2),
                ("removeItem", OperationSpecial::Deleter, 1),
            ]
        );
        assert_eq!(
            specials("URL"),
            [("stringifier", OperationSpecial::Stringifier, 0)]
        );
        assert_eq!(world, parse(source).unwrap());

        let dom_string_map = parse(
            "interface DOMStringMap { [SecureContext] getter DOMString? (DOMString name); };",
        )
        .unwrap();
        let error = lower_host_abi(&dom_string_map, &HostAbiOptions::new("web-test")).unwrap_err();
        assert_eq!(error.context, "interface `DOMStringMap` getter");
        assert!(error.detail.contains("property/index"), "{error}");
        let error = emit_fe_flat_host_imports(&dom_string_map, "fe:web").unwrap_err();
        assert_eq!(error.context, "interface `DOMStringMap` getter");
        let error = build_adapter_plan(&dom_string_map, "web-test", "fe:web").unwrap_err();
        assert_eq!(error.context, "interface `DOMStringMap` getter");
    }

    #[test]
    fn validates_special_operation_signatures_and_uniqueness() {
        let error = parse("interface Bad { setter DOMString (DOMString key, DOMString value); };")
            .unwrap_err();
        assert_eq!(error.context, "interface `Bad` setter");
        assert!(error.detail.contains("signature"), "{error}");

        let error = parse("interface Bad { getter DOMString (boolean key); };").unwrap_err();
        assert_eq!(error.context, "interface `Bad` getter");
        assert!(
            error.detail.contains("neither string nor integer"),
            "{error}"
        );

        let error = parse(
            "interface Bad { getter DOMString (DOMString first); getter DOMString (DOMString second); };",
        )
        .unwrap_err();
        assert_eq!(error.context, "interface `Bad` getter");
        assert!(error.detail.contains("duplicate getter"), "{error}");

        // Named and indexed property getters are distinct Web IDL identities.
        parse(
            "interface Both { getter DOMString (DOMString name); getter DOMString (unsigned long index); };",
        )
        .unwrap();

        let error = parse("interface Bad { stringifier; stringifier; };").unwrap_err();
        assert_eq!(error.context, "interface `Bad` stringifier");
        assert!(error.detail.contains("multiple stringifiers"), "{error}");

        let attributed =
            parse("interface URLValue { stringifier attribute DOMString href; };").unwrap();
        let Member::Attribute(attribute) = &attributed.interfaces["URLValue"].members[0] else {
            panic!("expected stringifier attribute");
        };
        assert!(attribute.stringifier);
        let error = lower_host_abi(&attributed, &HostAbiOptions::new("web-test")).unwrap_err();
        assert_eq!(
            error.context,
            "interface `URLValue` stringifier attribute `href`"
        );

        let error = parse("interface Bad { stringifier attribute boolean value; };").unwrap_err();
        assert_eq!(
            error.context,
            "interface `Bad` stringifier attribute `value`"
        );
        assert!(error.detail.contains("string type"), "{error}");
    }

    #[test]
    fn preserves_declaring_interface_identity_and_emits_safe_upcasts() {
        let source = include_str!("../tests/fixtures/dom-inheritance.webidl");
        let world = parse(source).unwrap();
        assert_eq!(
            world.interfaces["Node"].inherits.as_deref(),
            Some("EventTarget")
        );
        assert_eq!(
            world.interfaces["Element"].inherits.as_deref(),
            Some("Node")
        );
        assert_eq!(world.interfaces["Element"].members.len(), 2);

        let fe = emit_fe_raw(&world, "fe:web").unwrap();
        assert!(
            fe.contains("pub fn element_into_node(self_: Element) -> Node"),
            "{fe}"
        );
        assert!(
            fe.contains("pub fn node_into_event_target(self_: Node) -> EventTarget"),
            "{fe}"
        );
        assert!(
            fe.contains("pub unsafe fn node_append_child(self_: Node"),
            "{fe}"
        );
        assert!(!fe.contains("element_append_child"), "{fe}");
        assert!(!fe.contains("element_dispatch_event"), "{fe}");

        let lowering =
            lower_host_abi_with_metadata(&world, &HostAbiOptions::new("web-test")).unwrap();
        assert_eq!(
            lowering.resource_inheritance,
            [
                ResourceInheritanceBinding {
                    resource: "Element".to_owned(),
                    parent: "Node".to_owned(),
                },
                ResourceInheritanceBinding {
                    resource: "Node".to_owned(),
                    parent: "EventTarget".to_owned(),
                },
            ]
        );
        let resources = &lowering.world.resources;
        assert_eq!(
            resources
                .iter()
                .find(|resource| resource.name == "Element")
                .unwrap()
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            ["focus", "get-hidden", "set-hidden"]
        );

        let plan = build_adapter_plan(&world, "web-test", "fe:web").unwrap();
        assert_eq!(
            plan.lowering.resource_inheritance,
            lowering.resource_inheritance
        );
        let js = emit_js_canonical_adapter(&world, &plan).unwrap();
        assert!(js.contains("\"node_append_child\""), "{js}");
        assert!(!js.contains("\"element_append_child\""), "{js}");
    }

    #[test]
    fn validates_inherited_member_collisions_without_rejecting_new_overloads() {
        let error = parse(
            "interface Parent { undefined update(long value); }; interface Child : Parent { undefined update(long other); };",
        )
        .unwrap_err();
        assert_eq!(error.context, "interface `Child` operation `update`");
        assert!(error.detail.contains("ancestor interface"), "{error}");

        parse(
            "interface Parent { undefined update(long value); }; interface Child : Parent { undefined update(DOMString value); };",
        )
        .unwrap();

        let error = parse(
            "interface Parent { readonly attribute boolean active; }; interface Child : Parent { attribute boolean active; };",
        )
        .unwrap_err();
        assert_eq!(error.context, "interface `Child` attribute `active`");
        assert!(error.detail.contains("inherited attribute"), "{error}");

        let error = parse(
            "interface Parent { readonly attribute boolean active; }; interface Child : Parent { undefined active(); };",
        )
        .unwrap_err();
        assert_eq!(error.context, "interface `Child` operation `active`");
        assert!(error.detail.contains("inherited attribute"), "{error}");
    }

    #[test]
    fn rich_web_idl_types_fail_closed_at_the_v0_abi() {
        let world = parse(
            r#"
                interface Window {
                    DOMString title();
                };
            "#,
        )
        .unwrap();
        let error = emit_fe_raw(&world, "fe:web").unwrap_err();
        assert!(error.detail.contains("rich adapter ABI"), "{error}");
        assert!(error.context.contains("window_title"), "{error}");
    }

    #[test]
    fn unsupported_member_kinds_fail_during_linking() {
        let error = parse("interface Host { inherit attribute DOMString name; };").unwrap_err();
        assert_eq!(error.context, "interface `Host` attribute `name`");
        assert!(error.detail.contains("inherited attributes"), "{error}");
    }

    #[test]
    fn unknown_parent_is_rejected() {
        let error = parse("interface Window : Missing {};").unwrap_err();
        assert!(
            error.detail.contains("unknown interface `Missing`"),
            "{error}"
        );
    }

    #[test]
    fn normalizes_typedefs_enums_and_dictionary_partials() {
        let world = parse(
            r#"
                typedef unsigned long Identifier;
                enum Direction { "up", "down-left" };
                dictionary Point {
                    required long x;
                    DOMString label = "";
                };
                partial dictionary Point {
                    Identifier id;
                };
            "#,
        )
        .unwrap();

        assert_eq!(world.typedefs["Identifier"].type_, TypeRef::U32);
        assert_eq!(
            world.enums["Direction"].values,
            ["up".to_owned(), "down-left".to_owned()]
        );
        assert_eq!(
            world.dictionaries["Point"].members,
            [
                DictionaryMemberDef {
                    name: "x".to_owned(),
                    type_: TypeRef::I32,
                    required: true,
                    default_: None,
                },
                DictionaryMemberDef {
                    name: "label".to_owned(),
                    type_: TypeRef::String(StringKind::Dom),
                    required: false,
                    default_: Some(DefaultValueDef::String(String::new())),
                },
                DictionaryMemberDef {
                    name: "id".to_owned(),
                    type_: TypeRef::Named("Identifier".to_owned()),
                    required: false,
                    default_: None,
                },
            ]
        );

        let world = parse(
            r#"
                typedef unsigned long Identifier;
                interface Registry { Identifier size(); };
            "#,
        )
        .unwrap();
        let fe = emit_fe_raw(&world, "fe:web").unwrap();
        assert!(fe.contains("registry_size(self_: Registry) -> u32"));
    }

    #[test]
    fn links_partial_mixins_and_includes_in_statement_order() {
        let world = parse(
            r#"
                interface Element {};
                interface mixin ParentNode {
                    readonly attribute unsigned long childElementCount;
                };
                partial interface mixin ParentNode {
                    Element firstElementChild();
                };
                Element includes ParentNode;
            "#,
        )
        .unwrap();

        assert_eq!(world.includes["Element"], ["ParentNode"]);
        assert_eq!(world.mixins["ParentNode"].members.len(), 2);
        assert_eq!(world.interfaces["Element"].members.len(), 2);
        let fe = emit_fe_raw(&world, "fe:web").unwrap();
        assert!(fe.contains("element_get_child_element_count(self_: Element) -> u32"));
        assert!(fe.contains("element_first_element_child(self_: Element) -> Element"));
    }

    #[test]
    fn rich_named_definitions_remain_fail_closed_at_v0_abi() {
        let world = parse(
            r#"
                dictionary Point { required long x; };
                interface Geometry { Point origin(); };
            "#,
        )
        .unwrap();
        let error = emit_fe_raw(&world, "fe:web").unwrap_err();
        assert!(error.detail.contains("rich adapter ABI"), "{error}");
        assert!(error.detail.contains("Named(\"Point\")"), "{error}");
    }

    #[test]
    fn rejects_orphan_partials_and_invalid_includes_with_context() {
        let error = parse("partial dictionary Missing { long x; };").unwrap_err();
        assert_eq!(error.context, "partial dictionary `Missing`");
        assert!(
            error.detail.contains("no non-partial definition"),
            "{error}"
        );

        let error = parse(
            r#"
                interface Element {};
                Element includes Missing;
            "#,
        )
        .unwrap_err();
        assert!(
            error.context.contains("Element includes Missing"),
            "{error}"
        );
        assert!(error.detail.contains("not an interface mixin"), "{error}");
    }

    #[test]
    fn rejects_cycles_duplicates_and_cross_kind_name_collisions() {
        let error = parse(
            r#"
                typedef Second First;
                typedef First Second;
            "#,
        )
        .unwrap_err();
        assert!(error.detail.contains("typedef cycle"), "{error}");

        let error = parse(
            r#"
                typedef sequence<Recursive> Recursive;
            "#,
        )
        .unwrap_err();
        assert!(error.detail.contains("typedef cycle"), "{error}");

        let error = parse(
            r#"
                dictionary Base : Child {};
                dictionary Child : Base {};
            "#,
        )
        .unwrap_err();
        assert!(error.detail.contains("inheritance cycle"), "{error}");

        let error = parse(r#"enum Direction { "up", "up" };"#).unwrap_err();
        assert!(error.detail.contains("duplicate value `up`"), "{error}");

        let error = parse(
            r#"
                interface Item {};
                typedef long Item;
            "#,
        )
        .unwrap_err();
        assert!(error.detail.contains("already defined"), "{error}");
    }

    #[test]
    fn duplicate_members_introduced_by_partials_or_mixins_fail_closed() {
        let error = parse(
            r#"
                interface Element { attribute boolean hidden; };
                interface mixin Hidden { attribute boolean hidden; };
                Element includes Hidden;
            "#,
        )
        .unwrap_err();
        assert!(error.context.contains("attribute `hidden`"), "{error}");
        assert!(error.detail.contains("included mixin"), "{error}");

        let error = parse(
            r#"
                dictionary Point { long x; };
                partial dictionary Point { long x; };
            "#,
        )
        .unwrap_err();
        assert!(error.context.contains("member `x`"), "{error}");
    }

    #[test]
    fn retains_callbacks_typed_defaults_and_web_exposure_metadata() {
        let world = parse(
            r#"
                [Exposed=(Window,Worker), SecureContext]
                callback Mapper = Promise<DOMString> (
                    optional boolean enabled = true,
                    long... values
                );
                [Exposed=Window]
                callback interface EventListener {
                    undefined handleEvent(long event);
                };
                [Exposed=(Window,Worker), SecureContext]
                interface Registry {
                    [SecureContext]
                    undefined configure(
                        optional DOMString mode = "fast",
                        optional double scale = Infinity
                    );
                };
                dictionary Config {
                    boolean enabled = false;
                    sequence<long> values = [];
                    DOMString? label = null;
                };
            "#,
        )
        .unwrap();

        let mapper = &world.callbacks["Mapper"];
        assert_eq!(
            mapper.attributes.exposed,
            Some(vec!["Window".to_owned(), "Worker".to_owned()])
        );
        assert!(mapper.attributes.secure_context);
        assert_eq!(
            mapper.arguments[0].default_,
            Some(DefaultValueDef::Bool(true))
        );
        assert!(mapper.arguments[1].variadic);

        let listener = &world.callbacks["EventListener"];
        assert_eq!(listener.interface_operation.as_deref(), Some("handleEvent"));
        assert_eq!(listener.attributes.exposed, Some(vec!["Window".to_owned()]));

        let registry = &world.interfaces["Registry"];
        assert_eq!(
            registry.attributes.exposed,
            Some(vec!["Window".to_owned(), "Worker".to_owned()])
        );
        assert!(registry.attributes.secure_context);
        let Member::Operation(configure) = &registry.members[0] else {
            panic!("expected configure operation");
        };
        assert!(configure.attributes.secure_context);
        assert_eq!(
            configure.arguments[0].default_,
            Some(DefaultValueDef::String("fast".to_owned()))
        );
        assert_eq!(
            configure.arguments[1].default_,
            Some(DefaultValueDef::Float("Infinity".to_owned()))
        );

        assert_eq!(
            world.dictionaries["Config"]
                .members
                .iter()
                .map(|member| member.default_.clone())
                .collect::<Vec<_>>(),
            [
                Some(DefaultValueDef::Bool(false)),
                Some(DefaultValueDef::EmptySequence),
                Some(DefaultValueDef::Null),
            ]
        );
    }

    #[test]
    fn callback_interfaces_fail_closed_when_not_function_shaped() {
        let error = parse(
            r#"
                callback interface Listener {
                    undefined first();
                    undefined second();
                };
            "#,
        )
        .unwrap_err();
        assert_eq!(error.context, "callback interface `Listener`");
        assert!(error.detail.contains("exactly one regular operation"));
    }

    #[test]
    fn invalid_typed_defaults_and_wildcard_exposure_is_preserved() {
        let error = parse("interface Config { undefined set(optional boolean enabled = 1); };")
            .unwrap_err();
        assert!(error.context.contains("argument `enabled`"), "{error}");
        assert!(error.detail.contains("incompatible"), "{error}");

        let wildcard = parse("[Exposed=*] interface Everywhere {};").unwrap();
        assert_eq!(
            wildcard.interfaces["Everywhere"].attributes.exposed,
            Some(vec!["*".to_owned()])
        );
    }
}
