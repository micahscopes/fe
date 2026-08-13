//! Target-neutral host interface model.
//!
//! This crate describes interfaces before they are lowered to a concrete
//! transport such as core Wasm, the Component Model, JavaScript, or a native
//! embedding. It deliberately contains no browser vocabulary.
//!
//! The data model is close to WIT where that makes interfaces portable:
//! records, variants, options, results, lists, resources, `own`/`borrow`,
//! futures, and streams retain their semantic identity. Callbacks and typed
//! buffers are included because source interface languages need them even when
//! a particular backend cannot lower them yet.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// A complete, deterministic host interface world.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct World {
    pub name: String,
    #[serde(default)]
    pub types: Vec<TypeDef>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub imports: Vec<Function>,
    #[serde(default)]
    pub exports: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDef {
    pub name: String,
    pub kind: TypeDefKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDefKind {
    Alias { target: Type },
    Record { fields: Vec<Field> },
    Tuple { fields: Vec<Type> },
    Enum { cases: Vec<String> },
    Flags { flags: Vec<String> },
    Variant { cases: Vec<Case> },
    Callback { signature: FunctionType },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub type_: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Type>,
}

/// A host-managed resource.
///
/// `own<R>` transfers the obligation to eventually invoke the resource's
/// canonical drop operation. `borrow<R>` is valid only for the duration of the
/// host call. Backends may realize this using tables, capabilities, references,
/// or another unforgeable representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub name: String,
    #[serde(default)]
    pub methods: Vec<ResourceMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMethod {
    pub name: String,
    pub receiver: Receiver,
    pub signature: FunctionType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Receiver {
    Borrow,
    Own,
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    /// Host namespace, analogous to a WIT interface or Wasm import module.
    pub namespace: String,
    pub name: String,
    pub signature: FunctionType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionType {
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Type>,
    #[serde(default)]
    pub async_: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    pub type_: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Type {
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
    Char,
    String(StringEncoding),
    /// A canonical owned sequence.
    List(Box<Type>),
    /// A contiguous typed memory view. Unlike `list`, it may be borrowed.
    Buffer(Buffer),
    Option(Box<Type>),
    Result(ResultType),
    Future(Option<Box<Type>>),
    Stream(Option<Box<Type>>),
    Handle(Handle),
    /// Reference to a named type definition.
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StringEncoding {
    Utf8,
    Utf16,
    Latin1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Buffer {
    pub element: BufferElement,
    pub ownership: BufferOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferElement {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferOwnership {
    Own,
    Borrow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultType {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<Box<Type>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Box<Type>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handle {
    pub resource: String,
    pub ownership: HandleOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleOwnership {
    Own,
    Borrow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ValidationError {}

impl World {
    /// Validate semantic invariants and deterministic ordering.
    pub fn validate(&self) -> Result<(), ValidationError> {
        valid_name(&self.name, "world")?;
        sorted_unique(&self.types, "types", |value| &value.name)?;
        sorted_unique(&self.resources, "resources", |value| &value.name)?;
        sorted_functions(&self.imports, "imports")?;
        sorted_functions(&self.exports, "exports")?;

        let type_names = self
            .types
            .iter()
            .map(|value| value.name.as_str())
            .collect::<BTreeSet<_>>();
        let resource_names = self
            .resources
            .iter()
            .map(|value| value.name.as_str())
            .collect::<BTreeSet<_>>();
        if let Some(name) = type_names.intersection(&resource_names).next() {
            return error("world", format!("`{name}` is both a type and a resource"));
        }

        for definition in &self.types {
            valid_name(&definition.name, &format!("type {}", definition.name))?;
            validate_type_def(definition, &type_names, &resource_names)?;
        }
        reject_type_cycles(self, &type_names)?;

        for resource in &self.resources {
            let path = format!("resource {}", resource.name);
            valid_name(&resource.name, &path)?;
            sorted_unique(&resource.methods, &format!("{path}.methods"), |value| {
                &value.name
            })?;
            for method in &resource.methods {
                valid_name(&method.name, &format!("{path}.{}", method.name))?;
                validate_signature(
                    &method.signature,
                    &format!("{path}.{}", method.name),
                    &type_names,
                    &resource_names,
                )?;
            }
        }
        for (direction, functions) in [("import", &self.imports), ("export", &self.exports)] {
            for function in functions {
                let path = format!("{direction} {}::{}", function.namespace, function.name);
                valid_namespace(&function.namespace, &path)?;
                valid_name(&function.name, &path)?;
                validate_signature(&function.signature, &path, &type_names, &resource_names)?;
            }
        }
        Ok(())
    }
}

fn validate_type_def(
    definition: &TypeDef,
    types: &BTreeSet<&str>,
    resources: &BTreeSet<&str>,
) -> Result<(), ValidationError> {
    let path = format!("type {}", definition.name);
    match &definition.kind {
        TypeDefKind::Alias { target } => {
            validate_type(target, &path, Position::Nested, types, resources)
        }
        TypeDefKind::Record { fields } => {
            unique(fields, &format!("{path}.fields"), |field| &field.name)?;
            if fields.is_empty() {
                return error(path, "record must contain at least one field");
            }
            for field in fields {
                valid_name(&field.name, &format!("{path}.{}", field.name))?;
                validate_type(
                    &field.type_,
                    &format!("{path}.{}", field.name),
                    Position::Nested,
                    types,
                    resources,
                )?;
            }
            Ok(())
        }
        TypeDefKind::Tuple { fields } => {
            if fields.is_empty() {
                return error(path, "tuple must contain at least one field");
            }
            for (index, type_) in fields.iter().enumerate() {
                validate_type(
                    type_,
                    &format!("{path}[{index}]"),
                    Position::Nested,
                    types,
                    resources,
                )?;
            }
            Ok(())
        }
        TypeDefKind::Enum { cases } => validate_names(cases, &format!("{path}.cases")),
        TypeDefKind::Flags { flags } => validate_names(flags, &format!("{path}.flags")),
        TypeDefKind::Variant { cases } => {
            unique(cases, &format!("{path}.cases"), |case| &case.name)?;
            if cases.is_empty() {
                return error(path, "variant must contain at least one case");
            }
            for case in cases {
                valid_name(&case.name, &format!("{path}.{}", case.name))?;
                if let Some(type_) = &case.payload {
                    validate_type(
                        type_,
                        &format!("{path}.{}", case.name),
                        Position::Nested,
                        types,
                        resources,
                    )?;
                }
            }
            Ok(())
        }
        TypeDefKind::Callback { signature } => {
            validate_signature(signature, &path, types, resources)
        }
    }
}

fn validate_signature(
    signature: &FunctionType,
    path: &str,
    types: &BTreeSet<&str>,
    resources: &BTreeSet<&str>,
) -> Result<(), ValidationError> {
    unique(&signature.params, &format!("{path}.params"), |param| {
        &param.name
    })?;
    for param in &signature.params {
        valid_name(&param.name, &format!("{path}.param.{}", param.name))?;
        validate_type(
            &param.type_,
            &format!("{path}.param.{}", param.name),
            Position::Param,
            types,
            resources,
        )?;
    }
    if let Some(result) = &signature.result {
        validate_type(
            result,
            &format!("{path}.result"),
            Position::Result,
            types,
            resources,
        )?;
        if signature.async_ && matches!(result, Type::Future(_)) {
            return error(
                path,
                "an async function result must not be wrapped in `future` twice",
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Position {
    Param,
    Result,
    Nested,
}

fn validate_type(
    type_: &Type,
    path: &str,
    position: Position,
    types: &BTreeSet<&str>,
    resources: &BTreeSet<&str>,
) -> Result<(), ValidationError> {
    match type_ {
        Type::Handle(handle) => {
            if !resources.contains(handle.resource.as_str()) {
                return error(path, format!("unknown resource `{}`", handle.resource));
            }
            if handle.ownership == HandleOwnership::Borrow && !matches!(position, Position::Param) {
                return error(path, "`borrow` is only valid as a top-level parameter");
            }
        }
        Type::Buffer(buffer)
            if buffer.ownership == BufferOwnership::Borrow
                && !matches!(position, Position::Param) =>
        {
            return error(
                path,
                "a borrowed buffer is only valid as a top-level parameter",
            );
        }
        Type::Named(name) if !types.contains(name.as_str()) => {
            return error(path, format!("unknown named type `{name}`"));
        }
        Type::List(inner) | Type::Option(inner) => {
            validate_type(inner, path, Position::Nested, types, resources)?;
        }
        Type::Result(result) => {
            if let Some(ok) = &result.ok {
                validate_type(ok, path, Position::Nested, types, resources)?;
            }
            if let Some(error_) = &result.error {
                validate_type(error_, path, Position::Nested, types, resources)?;
            }
        }
        Type::Future(payload) | Type::Stream(payload) => {
            if let Some(payload) = payload {
                validate_type(payload, path, Position::Nested, types, resources)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn reject_type_cycles(world: &World, names: &BTreeSet<&str>) -> Result<(), ValidationError> {
    fn visit<'a>(
        name: &'a str,
        definitions: &BTreeMap<&'a str, &'a TypeDef>,
        names: &BTreeSet<&str>,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> Result<(), ValidationError> {
        if visited.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name) {
            return error(
                format!("type {name}"),
                "recursive value types are not supported",
            );
        }
        let definition = definitions[name];
        let mut dependencies = BTreeSet::new();
        collect_named(&definition.kind, &mut dependencies);
        for dependency in dependencies {
            if names.contains(dependency) {
                visit(dependency, definitions, names, visiting, visited)?;
            }
        }
        visiting.remove(name);
        visited.insert(name);
        Ok(())
    }

    let definitions = world
        .types
        .iter()
        .map(|definition| (definition.name.as_str(), definition))
        .collect::<BTreeMap<_, _>>();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for name in names {
        visit(name, &definitions, names, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn collect_named<'a>(kind: &'a TypeDefKind, output: &mut BTreeSet<&'a str>) {
    fn collect<'a>(type_: &'a Type, output: &mut BTreeSet<&'a str>) {
        match type_ {
            Type::Named(name) => {
                output.insert(name);
            }
            Type::List(inner) | Type::Option(inner) => collect(inner, output),
            Type::Result(result) => {
                if let Some(ok) = &result.ok {
                    collect(ok, output);
                }
                if let Some(error) = &result.error {
                    collect(error, output);
                }
            }
            Type::Future(payload) | Type::Stream(payload) => {
                if let Some(payload) = payload {
                    collect(payload, output);
                }
            }
            _ => {}
        }
    }
    match kind {
        TypeDefKind::Alias { target } => collect(target, output),
        TypeDefKind::Record { fields } => {
            for field in fields {
                collect(&field.type_, output);
            }
        }
        TypeDefKind::Tuple { fields } => {
            for field in fields {
                collect(field, output);
            }
        }
        TypeDefKind::Variant { cases } => {
            for case in cases {
                if let Some(payload) = &case.payload {
                    collect(payload, output);
                }
            }
        }
        TypeDefKind::Callback { signature } => {
            for param in &signature.params {
                collect(&param.type_, output);
            }
            if let Some(result) = &signature.result {
                collect(result, output);
            }
        }
        TypeDefKind::Enum { .. } | TypeDefKind::Flags { .. } => {}
    }
}

fn validate_names(names: &[String], path: &str) -> Result<(), ValidationError> {
    if names.is_empty() {
        return error(path, "must contain at least one name");
    }
    unique(names, path, |name| name)?;
    for name in names {
        valid_name(name, path)?;
    }
    Ok(())
}

/// Validate identity without changing declaration order. Parameter, field,
/// discriminant, flag-bit, and variant-case positions are ABI semantics.
fn unique<T>(
    values: &[T],
    path: &str,
    name: impl Fn(&T) -> &String,
) -> Result<(), ValidationError> {
    let mut seen = BTreeSet::new();
    for value in values {
        let current = name(value);
        if !seen.insert(current) {
            return error(path, "entries must be unique");
        }
    }
    Ok(())
}

fn sorted_functions(functions: &[Function], path: &str) -> Result<(), ValidationError> {
    let mut previous = None;
    for function in functions {
        let key = (&function.namespace, &function.name);
        if previous.is_some_and(|previous| previous >= key) {
            return error(
                path,
                "functions must be strictly sorted by namespace and name",
            );
        }
        previous = Some(key);
    }
    Ok(())
}

fn sorted_unique<T>(
    values: &[T],
    path: &str,
    name: impl Fn(&T) -> &String,
) -> Result<(), ValidationError> {
    let mut previous: Option<&str> = None;
    for value in values {
        let current = name(value);
        if previous.is_some_and(|previous| previous >= current) {
            return error(path, "entries must be strictly sorted and unique");
        }
        previous = Some(current);
    }
    Ok(())
}

fn valid_namespace(name: &str, path: &str) -> Result<(), ValidationError> {
    if name.split([':', '/', '.']).all(is_identifier) {
        Ok(())
    } else {
        error(path, format!("invalid namespace `{name}`"))
    }
}

fn valid_name(name: &str, path: &str) -> Result<(), ValidationError> {
    if is_identifier(name) {
        Ok(())
    } else {
        error(path, format!("invalid identifier `{name}`"))
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn error<T>(path: impl Into<String>, message: impl Into<String>) -> Result<T, ValidationError> {
    Err(ValidationError {
        path: path.into(),
        message: message.into(),
    })
}

/// Features a concrete transport must implement to preserve interface semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbiFeature {
    Scalar,
    Resource,
    ResourceDrop,
    Borrow,
    String,
    List,
    Buffer,
    Record,
    Variant,
    Callback,
    Future,
    Stream,
}

impl World {
    pub fn required_features(&self) -> BTreeSet<AbiFeature> {
        let mut features = BTreeSet::new();
        for definition in &self.types {
            match &definition.kind {
                TypeDefKind::Record { .. } | TypeDefKind::Tuple { .. } => {
                    features.insert(AbiFeature::Record);
                }
                TypeDefKind::Enum { .. }
                | TypeDefKind::Flags { .. }
                | TypeDefKind::Variant { .. } => {
                    features.insert(AbiFeature::Variant);
                }
                TypeDefKind::Callback { .. } => {
                    features.insert(AbiFeature::Callback);
                }
                TypeDefKind::Alias { .. } => {}
            }
            walk_definition(definition, &mut features);
        }
        if !self.resources.is_empty() {
            features.extend([AbiFeature::Resource, AbiFeature::ResourceDrop]);
        }
        for function in self.imports.iter().chain(&self.exports) {
            if function.signature.async_ {
                features.insert(AbiFeature::Future);
            }
            walk_signature(&function.signature, &mut features);
        }
        for resource in &self.resources {
            for method in &resource.methods {
                if method.receiver != Receiver::Static {
                    features.insert(AbiFeature::Borrow);
                }
                if method.signature.async_ {
                    features.insert(AbiFeature::Future);
                }
                walk_signature(&method.signature, &mut features);
            }
        }
        features
    }
}

fn walk_definition(definition: &TypeDef, features: &mut BTreeSet<AbiFeature>) {
    match &definition.kind {
        TypeDefKind::Alias { target } => walk_type(target, features),
        TypeDefKind::Record { fields } => {
            fields
                .iter()
                .for_each(|field| walk_type(&field.type_, features));
        }
        TypeDefKind::Tuple { fields } => {
            fields.iter().for_each(|type_| walk_type(type_, features));
        }
        TypeDefKind::Variant { cases } => {
            cases
                .iter()
                .filter_map(|case| case.payload.as_ref())
                .for_each(|type_| walk_type(type_, features));
        }
        TypeDefKind::Callback { signature } => walk_signature(signature, features),
        TypeDefKind::Enum { .. } | TypeDefKind::Flags { .. } => {}
    }
}

fn walk_signature(signature: &FunctionType, features: &mut BTreeSet<AbiFeature>) {
    signature
        .params
        .iter()
        .for_each(|param| walk_type(&param.type_, features));
    if let Some(result) = &signature.result {
        walk_type(result, features);
    }
}

fn walk_type(type_: &Type, features: &mut BTreeSet<AbiFeature>) {
    match type_ {
        Type::Bool
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::I64
        | Type::U64
        | Type::F32
        | Type::F64
        | Type::Char => {
            features.insert(AbiFeature::Scalar);
        }
        Type::String(_) => {
            features.insert(AbiFeature::String);
        }
        Type::List(inner) => {
            features.insert(AbiFeature::List);
            walk_type(inner, features);
        }
        Type::Buffer(buffer) => {
            features.insert(AbiFeature::Buffer);
            if buffer.ownership == BufferOwnership::Borrow {
                features.insert(AbiFeature::Borrow);
            }
        }
        Type::Option(inner) => {
            features.insert(AbiFeature::Variant);
            walk_type(inner, features);
        }
        Type::Result(result) => {
            features.insert(AbiFeature::Variant);
            result
                .ok
                .as_deref()
                .into_iter()
                .for_each(|type_| walk_type(type_, features));
            result
                .error
                .as_deref()
                .into_iter()
                .for_each(|type_| walk_type(type_, features));
        }
        Type::Future(payload) => {
            features.insert(AbiFeature::Future);
            payload
                .as_deref()
                .into_iter()
                .for_each(|type_| walk_type(type_, features));
        }
        Type::Stream(payload) => {
            features.insert(AbiFeature::Stream);
            payload
                .as_deref()
                .into_iter()
                .for_each(|type_| walk_type(type_, features));
        }
        Type::Handle(handle) => {
            features.extend([AbiFeature::Resource, AbiFeature::ResourceDrop]);
            if handle.ownership == HandleOwnership::Borrow {
                features.insert(AbiFeature::Borrow);
            }
        }
        Type::Named(_) => {}
    }
}

/// Honest support declaration for an executable ABI implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportProfile {
    pub name: &'static str,
    pub features: BTreeSet<AbiFeature>,
}

impl SupportProfile {
    /// The currently implemented `#[wasm_import]` boundary.
    ///
    /// Scalar newtypes and pointers are transport conveniences, not semantic
    /// resource/string/list support, so they are intentionally not advertised.
    pub fn current_fe_wasm_imports() -> Self {
        Self {
            name: "fe-wasm-import-v0",
            features: BTreeSet::from([AbiFeature::Scalar]),
        }
    }

    /// Intended canonical host ABI contract. This describes the implementation
    /// target; it must not be used as evidence that a backend supports it.
    pub fn canonical_v1() -> Self {
        Self {
            name: "fe-host-abi-v1",
            features: BTreeSet::from([
                AbiFeature::Scalar,
                AbiFeature::Resource,
                AbiFeature::ResourceDrop,
                AbiFeature::Borrow,
                AbiFeature::String,
                AbiFeature::List,
                AbiFeature::Buffer,
                AbiFeature::Record,
                AbiFeature::Variant,
                AbiFeature::Callback,
                AbiFeature::Future,
                AbiFeature::Stream,
            ]),
        }
    }

    pub fn check(&self, world: &World) -> Result<(), UnsupportedFeatures> {
        let missing = world
            .required_features()
            .difference(&self.features)
            .copied()
            .collect::<BTreeSet<_>>();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(UnsupportedFeatures {
                profile: self.name,
                missing,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedFeatures {
    pub profile: &'static str,
    pub missing: BTreeSet<AbiFeature>,
}

impl std::fmt::Display for UnsupportedFeatures {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} does not implement {:?}",
            self.profile, self.missing
        )
    }
}

impl std::error::Error for UnsupportedFeatures {}

/// Core value types used by a transport lowering plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreType {
    I32,
    I64,
    F32,
    F64,
}

/// How one semantic value is carried by a core function signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassMode {
    /// Flattened directly into core values.
    Direct(Vec<CoreType>),
    /// Address of canonical memory storage. Allocation and cleanup obligations
    /// are described by [`LoweringRequirement`].
    Indirect(CoreType),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredParam {
    pub name: String,
    pub semantic: Type,
    pub mode: PassMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweringPlan {
    pub namespace: String,
    pub name: String,
    pub params: Vec<LoweredParam>,
    pub result: Option<PassMode>,
    pub requirements: BTreeSet<LoweringRequirement>,
    /// Whether this plan corresponds to code that exists in the Fe compiler
    /// today, rather than a versioned implementation blueprint.
    pub executable_today: bool,
}

/// Transport-neutral shape of a synchronous callback export trampoline.
///
/// Parameter zero is always the opaque callback-table token. Remaining values
/// are the callback's flattened scalar arguments. This is a generic core-Wasm
/// contract; naming the export or connecting it to Web IDL is adapter policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackExportPlan {
    pub signature_id: String,
    pub params: Vec<CoreType>,
    pub results: Vec<CoreType>,
    pub requirements: BTreeSet<LoweringRequirement>,
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutureTokenOwner {
    /// The caller allocates the token before starting the import and retains it
    /// until one terminal outcome is consumed.
    CallerRuntime,
}

/// Transport-neutral protocol for one asynchronous import.
///
/// The start import receives `[future_token, ...arguments]`. Promise settlement
/// calls exactly one guest export: resolve receives `[token, ...result]`, reject
/// receives `[token, error_token]`, and cancellation receives `[token]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncImportPlan {
    pub operation_id: String,
    pub token_owner: FutureTokenOwner,
    pub start_import_params: Vec<CoreType>,
    pub resolve_export_params: Vec<CoreType>,
    pub reject_export_params: Vec<CoreType>,
    pub cancel_export_params: Vec<CoreType>,
    pub requirements: BTreeSet<LoweringRequirement>,
    pub blocker: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoweringRequirement {
    CanonicalMemory,
    Realloc,
    PostReturn,
    ResourceTable,
    ResourceDrop,
    BorrowScope,
    CallbackTable,
    AsyncRuntime,
    StreamRuntime,
    Utf16Transcode,
    Latin1Transcode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoweringProfile {
    /// The compiler's currently executable scalar-only import boundary.
    CurrentFeWasmImports,
    /// The proposed canonical-memory/resource-table ABI. Producing a plan is
    /// design validation, not a claim that compiler lowering is implemented.
    CanonicalV1Blueprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweringError {
    InvalidWorld(ValidationError),
    Unsupported(UnsupportedFeatures),
    UnknownFunction { namespace: String, name: String },
}

impl std::fmt::Display for LoweringError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LoweringError {}

impl World {
    /// Construct a deterministic transport plan for an import or export.
    pub fn lowering_plan(
        &self,
        namespace: &str,
        name: &str,
        profile: LoweringProfile,
    ) -> Result<LoweringPlan, LoweringError> {
        self.validate().map_err(LoweringError::InvalidWorld)?;
        let function = self
            .imports
            .iter()
            .chain(&self.exports)
            .find(|function| function.namespace == namespace && function.name == name)
            .ok_or_else(|| LoweringError::UnknownFunction {
                namespace: namespace.to_owned(),
                name: name.to_owned(),
            })?;
        self.signature_lowering_plan(namespace, name, &function.signature, profile)
    }

    /// Plan a resource method without pretending it is a free host function.
    pub fn resource_method_lowering_plan(
        &self,
        resource: &str,
        method: &str,
        profile: LoweringProfile,
    ) -> Result<LoweringPlan, LoweringError> {
        self.validate().map_err(LoweringError::InvalidWorld)?;
        let signature = self
            .resources
            .iter()
            .find(|candidate| candidate.name == resource)
            .and_then(|candidate| {
                candidate
                    .methods
                    .iter()
                    .find(|candidate| candidate.name == method)
            })
            .map(|method| &method.signature)
            .ok_or_else(|| LoweringError::UnknownFunction {
                namespace: resource.to_owned(),
                name: method.to_owned(),
            })?;
        self.signature_lowering_plan(resource, method, signature, profile)
    }

    /// Plan an already-validated signature such as a callback trampoline.
    pub fn signature_lowering_plan(
        &self,
        namespace: &str,
        name: &str,
        signature: &FunctionType,
        profile: LoweringProfile,
    ) -> Result<LoweringPlan, LoweringError> {
        self.validate().map_err(LoweringError::InvalidWorld)?;
        let support = match profile {
            LoweringProfile::CurrentFeWasmImports => SupportProfile::current_fe_wasm_imports(),
            LoweringProfile::CanonicalV1Blueprint => SupportProfile::canonical_v1(),
        };
        support.check(self).map_err(LoweringError::Unsupported)?;
        let mut requirements = BTreeSet::new();
        let params = signature
            .params
            .iter()
            .map(|param| LoweredParam {
                name: param.name.clone(),
                semantic: param.type_.clone(),
                mode: lower_type(self, &param.type_, &mut requirements),
            })
            .collect();
        let result = signature
            .result
            .as_ref()
            .map(|type_| lower_type(self, type_, &mut requirements));
        if signature.async_ {
            requirements.insert(LoweringRequirement::AsyncRuntime);
        }
        Ok(LoweringPlan {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            params,
            result,
            requirements,
            executable_today: profile == LoweringProfile::CurrentFeWasmImports,
        })
    }

    /// Plan the generic core-Wasm export used by a host to invoke a callback.
    ///
    /// This rung is intentionally limited to synchronous, directly flattened
    /// scalar values. Canonical-memory and async callbacks remain explicit
    /// blockers rather than silently acquiring partial semantics.
    pub fn callback_export_plan(
        &self,
        signature_id: &str,
    ) -> Result<CallbackExportPlan, LoweringError> {
        self.validate().map_err(LoweringError::InvalidWorld)?;
        let signature = self
            .types
            .iter()
            .find_map(|definition| (definition.name == signature_id).then_some(&definition.kind))
            .and_then(|kind| match kind {
                TypeDefKind::Callback { signature } => Some(signature),
                _ => None,
            })
            .ok_or_else(|| LoweringError::UnknownFunction {
                namespace: "callback".to_owned(),
                name: signature_id.to_owned(),
            })?;

        let mut requirements = BTreeSet::from([
            LoweringRequirement::ResourceTable,
            LoweringRequirement::ResourceDrop,
            LoweringRequirement::CallbackTable,
        ]);
        let mut params = vec![CoreType::I32];
        let mut blocker = signature
            .async_
            .then(|| "async callback export trampolines are not executable yet".to_owned());
        for param in &signature.params {
            match lower_type(self, &param.type_, &mut requirements) {
                PassMode::Direct(types) => params.extend(types),
                PassMode::Indirect(_) => {
                    blocker.get_or_insert_with(|| {
                        "callback export requires canonical-memory argument lowering".to_owned()
                    });
                }
            }
        }
        let results = match signature
            .result
            .as_ref()
            .map(|result| lower_type(self, result, &mut requirements))
        {
            None => Vec::new(),
            Some(PassMode::Direct(types)) => types,
            Some(PassMode::Indirect(_)) => {
                blocker.get_or_insert_with(|| {
                    "callback export requires canonical-memory result lowering".to_owned()
                });
                Vec::new()
            }
        };
        if signature.async_ {
            requirements.insert(LoweringRequirement::AsyncRuntime);
        }
        if requirements.contains(&LoweringRequirement::CanonicalMemory) {
            blocker.get_or_insert_with(|| {
                "callback export requires canonical-memory value lowering".to_owned()
            });
        }
        Ok(CallbackExportPlan {
            signature_id: signature_id.to_owned(),
            params,
            results,
            requirements,
            blocker,
        })
    }

    /// Plan the token protocol around one asynchronous import without claiming
    /// that Fe can yet suspend and resume a function body.
    pub fn async_import_plan(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<AsyncImportPlan, LoweringError> {
        self.validate().map_err(LoweringError::InvalidWorld)?;
        let signature = self
            .imports
            .iter()
            .find(|function| function.namespace == namespace && function.name == name)
            .map(|function| &function.signature)
            .ok_or_else(|| LoweringError::UnknownFunction {
                namespace: namespace.to_owned(),
                name: name.to_owned(),
            })?;

        let mut requirements = BTreeSet::from([
            LoweringRequirement::ResourceTable,
            LoweringRequirement::ResourceDrop,
            LoweringRequirement::AsyncRuntime,
        ]);
        let mut rich_values = false;
        let mut start_import_params = vec![CoreType::I32];
        for param in &signature.params {
            match lower_type(self, &param.type_, &mut requirements) {
                PassMode::Direct(types) => start_import_params.extend(types),
                PassMode::Indirect(_) => rich_values = true,
            }
        }
        let mut resolve_export_params = vec![CoreType::I32];
        if let Some(result) = &signature.result {
            match lower_type(self, result, &mut requirements) {
                PassMode::Direct(types) => resolve_export_params.extend(types),
                PassMode::Indirect(_) => rich_values = true,
            }
        }
        if requirements.contains(&LoweringRequirement::CanonicalMemory) {
            rich_values = true;
        }
        let blocker = if !signature.async_ {
            "operation is synchronous and must not use the async token protocol".to_owned()
        } else if rich_values {
            "compiler resumable state machines and canonical-memory async values are not executable yet"
                .to_owned()
        } else {
            "compiler resumable state machines and Fe Future/await are not executable yet"
                .to_owned()
        };
        Ok(AsyncImportPlan {
            operation_id: format!("{namespace}/{name}"),
            token_owner: FutureTokenOwner::CallerRuntime,
            start_import_params,
            resolve_export_params,
            // Rejection is an opaque host-runtime error token at this rung.
            reject_export_params: vec![CoreType::I32, CoreType::I32],
            cancel_export_params: vec![CoreType::I32],
            requirements,
            blocker,
        })
    }
}

fn lower_type(
    world: &World,
    type_: &Type,
    requirements: &mut BTreeSet<LoweringRequirement>,
) -> PassMode {
    let direct = |type_| PassMode::Direct(vec![type_]);
    match type_ {
        Type::Bool
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::Char => direct(CoreType::I32),
        Type::I64 | Type::U64 => direct(CoreType::I64),
        Type::F32 => direct(CoreType::F32),
        Type::F64 => direct(CoreType::F64),
        Type::String(encoding) => {
            requirements.extend([
                LoweringRequirement::CanonicalMemory,
                LoweringRequirement::Realloc,
                LoweringRequirement::PostReturn,
            ]);
            match encoding {
                StringEncoding::Utf8 => {}
                StringEncoding::Utf16 => {
                    requirements.insert(LoweringRequirement::Utf16Transcode);
                }
                StringEncoding::Latin1 => {
                    requirements.insert(LoweringRequirement::Latin1Transcode);
                }
            }
            PassMode::Direct(vec![CoreType::I32, CoreType::I32])
        }
        Type::List(_) | Type::Buffer(_) => {
            requirements.extend([
                LoweringRequirement::CanonicalMemory,
                LoweringRequirement::Realloc,
                LoweringRequirement::PostReturn,
            ]);
            if matches!(
                type_,
                Type::Buffer(Buffer {
                    ownership: BufferOwnership::Borrow,
                    ..
                })
            ) {
                requirements.insert(LoweringRequirement::BorrowScope);
            }
            PassMode::Direct(vec![CoreType::I32, CoreType::I32])
        }
        Type::Handle(handle) => {
            requirements.extend([
                LoweringRequirement::ResourceTable,
                LoweringRequirement::ResourceDrop,
            ]);
            if handle.ownership == HandleOwnership::Borrow {
                requirements.insert(LoweringRequirement::BorrowScope);
            }
            direct(CoreType::I32)
        }
        Type::Future(_) => {
            requirements.extend([
                LoweringRequirement::ResourceTable,
                LoweringRequirement::ResourceDrop,
                LoweringRequirement::AsyncRuntime,
            ]);
            direct(CoreType::I32)
        }
        Type::Stream(_) => {
            requirements.extend([
                LoweringRequirement::ResourceTable,
                LoweringRequirement::ResourceDrop,
                LoweringRequirement::StreamRuntime,
            ]);
            direct(CoreType::I32)
        }
        Type::Option(_) | Type::Result(_) => indirect(requirements),
        Type::Named(name) => {
            let definition = world
                .types
                .iter()
                .find(|definition| definition.name == *name)
                .expect("validated named type");
            match &definition.kind {
                TypeDefKind::Alias { target } => lower_type(world, target, requirements),
                TypeDefKind::Enum { .. } | TypeDefKind::Flags { .. } => direct(CoreType::I32),
                TypeDefKind::Callback { .. } => {
                    requirements.extend([
                        LoweringRequirement::ResourceTable,
                        LoweringRequirement::ResourceDrop,
                        LoweringRequirement::CallbackTable,
                    ]);
                    direct(CoreType::I32)
                }
                TypeDefKind::Record { .. }
                | TypeDefKind::Tuple { .. }
                | TypeDefKind::Variant { .. } => indirect(requirements),
            }
        }
    }
}

fn indirect(requirements: &mut BTreeSet<LoweringRequirement>) -> PassMode {
    requirements.extend([
        LoweringRequirement::CanonicalMemory,
        LoweringRequirement::Realloc,
        LoweringRequirement::PostReturn,
    ]);
    PassMode::Indirect(CoreType::I32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich_world() -> World {
        World {
            name: "sandbox".into(),
            types: vec![
                TypeDef {
                    name: "completion".into(),
                    kind: TypeDefKind::Callback {
                        signature: FunctionType {
                            params: vec![Param {
                                name: "value".into(),
                                type_: Type::Result(ResultType {
                                    ok: Some(Box::new(Type::String(StringEncoding::Utf8))),
                                    error: Some(Box::new(Type::Named("host-error".into()))),
                                }),
                            }],
                            result: None,
                            async_: false,
                        },
                    },
                },
                TypeDef {
                    name: "host-error".into(),
                    kind: TypeDefKind::Variant {
                        cases: vec![
                            Case {
                                name: "closed".into(),
                                payload: None,
                            },
                            Case {
                                name: "message".into(),
                                payload: Some(Type::String(StringEncoding::Utf8)),
                            },
                        ],
                    },
                },
            ],
            resources: vec![Resource {
                name: "channel".into(),
                methods: vec![ResourceMethod {
                    name: "send".into(),
                    receiver: Receiver::Borrow,
                    signature: FunctionType {
                        params: vec![Param {
                            name: "bytes".into(),
                            type_: Type::Buffer(Buffer {
                                element: BufferElement::U8,
                                ownership: BufferOwnership::Borrow,
                            }),
                        }],
                        result: Some(Type::Future(Some(Box::new(Type::Result(ResultType {
                            ok: None,
                            error: Some(Box::new(Type::Named("host-error".into()))),
                        }))))),
                        async_: false,
                    },
                }],
            }],
            imports: vec![Function {
                namespace: "fe:host".into(),
                name: "open-channel".into(),
                signature: FunctionType {
                    params: vec![Param {
                        name: "name".into(),
                        type_: Type::String(StringEncoding::Utf8),
                    }],
                    result: Some(Type::Handle(Handle {
                        resource: "channel".into(),
                        ownership: HandleOwnership::Own,
                    })),
                    async_: true,
                },
            }],
            exports: vec![],
        }
    }

    #[test]
    fn rich_world_is_valid_and_declares_semantic_requirements() {
        let world = rich_world();
        world.validate().unwrap();
        assert_eq!(
            world.required_features(),
            BTreeSet::from([
                AbiFeature::Resource,
                AbiFeature::ResourceDrop,
                AbiFeature::Borrow,
                AbiFeature::String,
                AbiFeature::Buffer,
                AbiFeature::Variant,
                AbiFeature::Callback,
                AbiFeature::Future,
            ])
        );
        SupportProfile::canonical_v1().check(&world).unwrap();
    }

    #[test]
    fn current_wasm_boundary_fails_closed_for_rich_interface() {
        let error = SupportProfile::current_fe_wasm_imports()
            .check(&rich_world())
            .unwrap_err();
        assert!(error.missing.contains(&AbiFeature::Resource));
        assert!(error.missing.contains(&AbiFeature::ResourceDrop));
        assert!(error.missing.contains(&AbiFeature::String));
        assert!(error.missing.contains(&AbiFeature::Callback));
        assert!(error.missing.contains(&AbiFeature::Future));
    }

    #[test]
    fn scalar_interface_matches_current_wasm_boundary() {
        let world = World {
            name: "math".into(),
            imports: vec![Function {
                namespace: "fe:math".into(),
                name: "add".into(),
                signature: FunctionType {
                    params: vec![
                        Param {
                            name: "left".into(),
                            type_: Type::I32,
                        },
                        Param {
                            name: "right".into(),
                            type_: Type::I32,
                        },
                    ],
                    result: Some(Type::I32),
                    async_: false,
                },
            }],
            ..World::default()
        };
        world.validate().unwrap();
        SupportProfile::current_fe_wasm_imports()
            .check(&world)
            .unwrap();
    }

    #[test]
    fn declaration_order_is_preserved_for_abi_bearing_sequences() {
        let world = World {
            name: "ordered".into(),
            types: vec![
                TypeDef {
                    name: "a-enum".into(),
                    kind: TypeDefKind::Enum {
                        cases: vec!["z".into(), "a".into()],
                    },
                },
                TypeDef {
                    name: "b-flags".into(),
                    kind: TypeDefKind::Flags {
                        flags: vec!["high-bit".into(), "low-bit".into()],
                    },
                },
                TypeDef {
                    name: "c-record".into(),
                    kind: TypeDefKind::Record {
                        fields: vec![
                            Field {
                                name: "y".into(),
                                type_: Type::I32,
                            },
                            Field {
                                name: "x".into(),
                                type_: Type::I32,
                            },
                        ],
                    },
                },
                TypeDef {
                    name: "d-variant".into(),
                    kind: TypeDefKind::Variant {
                        cases: vec![
                            Case {
                                name: "some".into(),
                                payload: Some(Type::I32),
                            },
                            Case {
                                name: "none".into(),
                                payload: None,
                            },
                        ],
                    },
                },
            ],
            imports: vec![Function {
                namespace: "fe:ordered".into(),
                name: "point".into(),
                signature: FunctionType {
                    params: vec![
                        Param {
                            name: "y".into(),
                            type_: Type::I32,
                        },
                        Param {
                            name: "x".into(),
                            type_: Type::I32,
                        },
                    ],
                    result: None,
                    async_: false,
                },
            }],
            ..World::default()
        };
        world.validate().unwrap();
        assert_eq!(
            world.imports[0]
                .signature
                .params
                .iter()
                .map(|param| param.name.as_str())
                .collect::<Vec<_>>(),
            ["y", "x"]
        );
        assert_eq!(
            match &world.types[0].kind {
                TypeDefKind::Enum { cases } => cases.as_slice(),
                _ => unreachable!(),
            },
            ["z", "a"]
        );
    }

    #[test]
    fn borrow_cannot_escape_a_call() {
        let mut world = rich_world();
        world.imports[0].signature.result = Some(Type::Handle(Handle {
            resource: "channel".into(),
            ownership: HandleOwnership::Borrow,
        }));
        let error = world.validate().unwrap_err();
        assert!(error.message.contains("top-level parameter"));
    }

    #[test]
    fn unknown_resource_and_named_type_are_rejected() {
        let mut world = rich_world();
        world.imports[0].signature.result = Some(Type::Handle(Handle {
            resource: "missing".into(),
            ownership: HandleOwnership::Own,
        }));
        assert!(
            world
                .validate()
                .unwrap_err()
                .message
                .contains("unknown resource")
        );

        let mut world = rich_world();
        world.types[0] = TypeDef {
            name: "completion".into(),
            kind: TypeDefKind::Alias {
                target: Type::Named("missing".into()),
            },
        };
        assert!(
            world
                .validate()
                .unwrap_err()
                .message
                .contains("unknown named type")
        );
    }

    #[test]
    fn recursive_value_types_are_rejected() {
        let world = World {
            name: "cycles".into(),
            types: vec![
                TypeDef {
                    name: "a".into(),
                    kind: TypeDefKind::Alias {
                        target: Type::Named("b".into()),
                    },
                },
                TypeDef {
                    name: "b".into(),
                    kind: TypeDefKind::Alias {
                        target: Type::Option(Box::new(Type::Named("a".into()))),
                    },
                },
            ],
            ..World::default()
        };
        assert!(world.validate().unwrap_err().message.contains("recursive"));
    }

    #[test]
    fn json_round_trip_preserves_ownership_and_async() {
        let world = rich_world();
        let json = serde_json::to_string(&world).unwrap();
        let decoded: World = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, world);
    }

    #[test]
    fn current_lowering_plan_is_executable_and_scalar() {
        let world = World {
            name: "math".into(),
            imports: vec![Function {
                namespace: "fe:math".into(),
                name: "dot".into(),
                signature: FunctionType {
                    params: vec![Param {
                        name: "value".into(),
                        type_: Type::F64,
                    }],
                    result: Some(Type::I64),
                    async_: false,
                },
            }],
            ..World::default()
        };
        let plan = world
            .lowering_plan("fe:math", "dot", LoweringProfile::CurrentFeWasmImports)
            .unwrap();
        assert!(plan.executable_today);
        assert_eq!(plan.params[0].mode, PassMode::Direct(vec![CoreType::F64]));
        assert_eq!(plan.result, Some(PassMode::Direct(vec![CoreType::I64])));
        assert!(plan.requirements.is_empty());
    }

    #[test]
    fn canonical_blueprint_records_runtime_obligations() {
        let world = rich_world();
        let plan = world
            .lowering_plan(
                "fe:host",
                "open-channel",
                LoweringProfile::CanonicalV1Blueprint,
            )
            .unwrap();
        assert!(!plan.executable_today);
        assert_eq!(
            plan.params[0].mode,
            PassMode::Direct(vec![CoreType::I32, CoreType::I32])
        );
        assert_eq!(plan.result, Some(PassMode::Direct(vec![CoreType::I32])));
        assert!(
            plan.requirements
                .contains(&LoweringRequirement::CanonicalMemory)
        );
        assert!(
            plan.requirements
                .contains(&LoweringRequirement::ResourceTable)
        );
        assert!(
            plan.requirements
                .contains(&LoweringRequirement::ResourceDrop)
        );
        assert!(
            plan.requirements
                .contains(&LoweringRequirement::AsyncRuntime)
        );
    }

    #[test]
    fn current_lowering_refuses_before_planning_rich_values() {
        let error = rich_world()
            .lowering_plan(
                "fe:host",
                "open-channel",
                LoweringProfile::CurrentFeWasmImports,
            )
            .unwrap_err();
        assert!(matches!(error, LoweringError::Unsupported(_)));
    }

    #[test]
    fn callback_export_plan_is_generic_scalar_token_abi() {
        let world = World {
            name: "events".into(),
            types: vec![TypeDef {
                name: "event-listener".into(),
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
            }],
            ..World::default()
        };
        let plan = world.callback_export_plan("event-listener").unwrap();
        assert_eq!(plan.params, [CoreType::I32, CoreType::I32]);
        assert_eq!(plan.results, [CoreType::I32]);
        assert!(plan.blocker.is_none());
        assert!(
            plan.requirements
                .contains(&LoweringRequirement::CallbackTable)
        );
    }

    #[test]
    fn callback_export_plan_keeps_rich_and_async_shapes_blocked() {
        let world = World {
            name: "events".into(),
            types: vec![
                TypeDef {
                    name: "async-listener".into(),
                    kind: TypeDefKind::Callback {
                        signature: FunctionType {
                            params: vec![Param {
                                name: "event".into(),
                                type_: Type::I32,
                            }],
                            result: Some(Type::I32),
                            async_: true,
                        },
                    },
                },
                TypeDef {
                    name: "rich-listener".into(),
                    kind: TypeDefKind::Callback {
                        signature: FunctionType {
                            params: vec![Param {
                                name: "message".into(),
                                type_: Type::String(StringEncoding::Utf8),
                            }],
                            result: None,
                            async_: false,
                        },
                    },
                },
            ],
            ..World::default()
        };
        assert!(
            world
                .callback_export_plan("rich-listener")
                .unwrap()
                .blocker
                .unwrap()
                .contains("canonical-memory")
        );
        assert!(
            world
                .callback_export_plan("async-listener")
                .unwrap()
                .blocker
                .unwrap()
                .contains("async")
        );
    }

    #[test]
    fn async_import_plan_assigns_token_ownership_and_terminal_exports() {
        let world = World {
            name: "tasks".into(),
            imports: vec![Function {
                namespace: "fe:tasks".into(),
                name: "load".into(),
                signature: FunctionType {
                    params: vec![Param {
                        name: "request".into(),
                        type_: Type::I32,
                    }],
                    result: Some(Type::I32),
                    async_: true,
                },
            }],
            ..World::default()
        };
        let plan = world.async_import_plan("fe:tasks", "load").unwrap();
        assert_eq!(plan.token_owner, FutureTokenOwner::CallerRuntime);
        assert_eq!(plan.start_import_params, [CoreType::I32, CoreType::I32]);
        assert_eq!(plan.resolve_export_params, [CoreType::I32, CoreType::I32]);
        assert_eq!(plan.reject_export_params, [CoreType::I32, CoreType::I32]);
        assert_eq!(plan.cancel_export_params, [CoreType::I32]);
        assert!(plan.blocker.contains("resumable state machines"));
        assert!(plan.blocker.contains("Future/await"));
    }
}
