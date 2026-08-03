//! Fe Host Wasm ABI v1 memory layouts and an executable checked codec.
//!
//! This crate does not implement asynchronous scheduling. A future is an opaque
//! runtime handle at this layer; async functions and streams fail closed.
//!
//! Records, strings, lists, and variants follow canonical-memory conventions,
//! but this is not presented as the Component Model Canonical ABI. In
//! particular, Fe v1 uses a fixed `u32` variant tag and currently supports at
//! most 32 flags. Resource, callback, and future values are canonical `i32`
//! session tokens; generation-safe [`RawHandle`] values remain behind the
//! runtime's session map and never cross the core-Wasm boundary.

use std::collections::{BTreeMap, BTreeSet};

use fe_host_abi::{
    BufferElement, BufferOwnership, Function, HandleOwnership, StringEncoding, Type, TypeDefKind,
    World,
};
use fe_host_runtime::RawHandle;
use serde::{Deserialize, Serialize};

pub const FE_HOST_WASM_ABI: &str = "fe-host-wasm";
pub const FE_HOST_WASM_ABI_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HandleToken(pub u32);

/// Per-instantiation bridge between canonical `i32` tokens and normative
/// generation-safe runtime handles. Tokens are monotonic and are never reused
/// within a session, so a removed token cannot become valid again.
#[derive(Debug, Default)]
pub struct HandleSession {
    next: u32,
    by_token: BTreeMap<HandleToken, RawHandle>,
    by_handle: BTreeMap<RawHandle, HandleToken>,
}

impl HandleSession {
    pub fn new() -> Self {
        Self {
            next: 1,
            ..Self::default()
        }
    }

    pub fn insert(&mut self, handle: RawHandle) -> Result<HandleToken, CodecError> {
        if let Some(token) = self.by_handle.get(&handle) {
            return Ok(*token);
        }
        let token = HandleToken(self.next);
        self.next = self.next.checked_add(1).ok_or(CodecError::Overflow)?;
        self.by_token.insert(token, handle);
        self.by_handle.insert(handle, token);
        Ok(token)
    }

    pub fn resolve(&self, token: HandleToken) -> Result<RawHandle, CodecError> {
        self.by_token
            .get(&token)
            .copied()
            .ok_or(CodecError::InvalidHandleToken(token.0))
    }

    pub fn remove(&mut self, token: HandleToken) -> Result<RawHandle, CodecError> {
        let handle = self
            .by_token
            .remove(&token)
            .ok_or(CodecError::InvalidHandleToken(token.0))?;
        self.by_handle.remove(&handle);
        Ok(handle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreType {
    I32,
    I64,
    F32,
    F64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub size: u32,
    pub align: u32,
    pub shape: LayoutShape,
    pub flat: Flattening,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "types", rename_all = "snake_case")]
pub enum Flattening {
    Direct(Vec<CoreType>),
    Indirect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LayoutShape {
    Scalar(ScalarKind),
    String(StringEncoding),
    List(Box<Layout>),
    Buffer(BufferElement),
    Handle,
    Record(Vec<FieldLayout>),
    Tuple(Vec<FieldLayout>),
    Enum { cases: u32 },
    Flags { count: u32 },
    Variant(VariantLayout),
    FutureHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarKind {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldLayout {
    pub name: String,
    pub offset: u32,
    pub layout: Layout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantLayout {
    pub cases: Vec<CaseLayout>,
    pub payload_offset: u32,
    pub payload_size: u32,
    pub payload_align: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseLayout {
    pub name: String,
    pub payload: Option<Layout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryDirection {
    GuestToHost,
    HostToGuest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundarySide {
    Guest,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValuePosition {
    Parameter,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanRequirement {
    Realloc,
    PostReturn,
    ResourceTransfer,
    BorrowScope,
    CallbackTable,
    FutureTable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionPlan {
    pub namespace: String,
    pub name: String,
    pub direction: BoundaryDirection,
    pub params: Vec<ValuePlan>,
    pub result: Option<ValuePlan>,
    pub requirements: BTreeSet<PlanRequirement>,
}

pub const JS_CODEC_CONTRACT: &str = "fe:host-wasm-codec/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SerializableCodecPlan {
    pub contract: String,
    pub abi: String,
    pub abi_version: u32,
    pub function: FunctionPlan,
}

pub fn serializable_function_plan(
    world: &World,
    function: &Function,
    direction: BoundaryDirection,
) -> Result<SerializableCodecPlan, CodecError> {
    Ok(SerializableCodecPlan {
        contract: JS_CODEC_CONTRACT.to_owned(),
        abi: FE_HOST_WASM_ABI.to_owned(),
        abi_version: FE_HOST_WASM_ABI_VERSION,
        function: function_plan(world, function, direction)?,
    })
}

pub fn emit_function_plan_json(
    world: &World,
    function: &Function,
    direction: BoundaryDirection,
) -> Result<String, CodecError> {
    serde_json::to_string(&serializable_function_plan(world, function, direction)?)
        .map_err(|error| CodecError::Unsupported(format!("serialize codec plan: {error}")))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValuePlan {
    pub type_: Type,
    pub layout: Layout,
    pub position: ValuePosition,
    pub ownership: TransferOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferOwnership {
    Value,
    Own,
    Borrow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    List(Vec<Value>),
    Buffer(BufferValue),
    Handle(HandleToken),
    Record(Vec<Value>),
    Tuple(Vec<Value>),
    Enum(u32),
    Flags(u32),
    Variant {
        case: u32,
        payload: Option<Box<Value>>,
    },
    Future(HandleToken),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BufferValue {
    I8(Vec<i8>),
    U8(Vec<u8>),
    I16(Vec<i16>),
    U16(Vec<u16>),
    I32(Vec<i32>),
    U32(Vec<u32>),
    I64(Vec<i64>),
    U64(Vec<u64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupKind {
    Realloc,
    PostReturn,
    BorrowEnd,
    ResourceTransfer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleanup {
    pub kind: CleanupKind,
    /// Side responsible for completing this obligation.
    pub actor: BoundarySide,
    pub ptr: u32,
    pub size: u32,
    pub align: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupPlan {
    pub actions: Vec<Cleanup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    InvalidWorld(String),
    UnknownType(String),
    Unsupported(String),
    TypeMismatch {
        expected: &'static str,
    },
    Overflow,
    InvalidAlignment {
        offset: u32,
        align: u32,
    },
    OutOfBounds {
        offset: u32,
        length: u32,
        memory_size: u32,
    },
    InvalidTag {
        tag: u32,
        cases: u32,
    },
    InvalidBool(u8),
    InvalidChar(u32),
    InvalidUtf8,
    InvalidUtf16,
    InvalidLatin1,
    InvalidHandleToken(u32),
    AllocationFailed,
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CodecError {}

pub trait LinearMemory {
    fn size(&self) -> u32;
    fn read(&self, offset: u32, bytes: &mut [u8]) -> Result<(), CodecError>;
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), CodecError>;

    /// Canonical realloc. `old_ptr/old_size == 0` requests fresh storage.
    fn realloc(
        &mut self,
        old_ptr: u32,
        old_size: u32,
        align: u32,
        new_size: u32,
    ) -> Result<u32, CodecError>;
}

pub fn layout(world: &World, type_: &Type) -> Result<Layout, CodecError> {
    world
        .validate()
        .map_err(|error| CodecError::InvalidWorld(error.to_string()))?;
    layout_validated(world, type_, 0)
}

fn layout_validated(world: &World, type_: &Type, depth: u32) -> Result<Layout, CodecError> {
    if depth > 64 {
        return Err(CodecError::Unsupported("type nesting exceeds 64".into()));
    }
    let scalar = |size, align, kind, core| Layout {
        size,
        align,
        shape: LayoutShape::Scalar(kind),
        flat: Flattening::Direct(vec![core]),
    };
    Ok(match type_ {
        Type::Bool => scalar(1, 1, ScalarKind::Bool, CoreType::I32),
        Type::I8 => scalar(1, 1, ScalarKind::I8, CoreType::I32),
        Type::U8 => scalar(1, 1, ScalarKind::U8, CoreType::I32),
        Type::I16 => scalar(2, 2, ScalarKind::I16, CoreType::I32),
        Type::U16 => scalar(2, 2, ScalarKind::U16, CoreType::I32),
        Type::I32 => scalar(4, 4, ScalarKind::I32, CoreType::I32),
        Type::U32 => scalar(4, 4, ScalarKind::U32, CoreType::I32),
        Type::Char => scalar(4, 4, ScalarKind::Char, CoreType::I32),
        Type::I64 => scalar(8, 8, ScalarKind::I64, CoreType::I64),
        Type::U64 => scalar(8, 8, ScalarKind::U64, CoreType::I64),
        Type::F32 => scalar(4, 4, ScalarKind::F32, CoreType::F32),
        Type::F64 => scalar(8, 8, ScalarKind::F64, CoreType::F64),
        Type::String(encoding) => descriptor(LayoutShape::String(*encoding)),
        Type::List(element) => {
            let element = layout_validated(world, element, depth + 1)?;
            if element.size == 0 {
                return Err(CodecError::Unsupported("zero-sized list element".into()));
            }
            descriptor(LayoutShape::List(Box::new(element)))
        }
        Type::Buffer(buffer) => descriptor(LayoutShape::Buffer(buffer.element)),
        Type::Handle(handle) => {
            if !world
                .resources
                .iter()
                .any(|resource| resource.name == handle.resource)
            {
                return Err(CodecError::UnknownType(handle.resource.clone()));
            }
            handle_layout(LayoutShape::Handle)
        }
        Type::Future(payload) => {
            if let Some(payload) = payload {
                let _ = layout_validated(world, payload, depth + 1)?;
            }
            handle_layout(LayoutShape::FutureHandle)
        }
        Type::Stream(_) => {
            return Err(CodecError::Unsupported(
                "stream transport requires runtime polling mechanics".into(),
            ));
        }
        Type::Option(payload) => variant_layout(vec![
            ("none".into(), None),
            (
                "some".into(),
                Some(layout_validated(world, payload, depth + 1)?),
            ),
        ])?,
        Type::Result(result) => variant_layout(vec![
            (
                "ok".into(),
                result
                    .ok
                    .as_deref()
                    .map(|type_| layout_validated(world, type_, depth + 1))
                    .transpose()?,
            ),
            (
                "error".into(),
                result
                    .error
                    .as_deref()
                    .map(|type_| layout_validated(world, type_, depth + 1))
                    .transpose()?,
            ),
        ])?,
        Type::Named(name) => {
            let definition = world
                .types
                .iter()
                .find(|definition| definition.name == *name)
                .ok_or_else(|| CodecError::UnknownType(name.clone()))?;
            match &definition.kind {
                TypeDefKind::Alias { target } => layout_validated(world, target, depth + 1)?,
                TypeDefKind::Record { fields } => aggregate_layout(
                    LayoutAggregate::Record,
                    fields
                        .iter()
                        .map(|field| {
                            Ok((
                                field.name.clone(),
                                layout_validated(world, &field.type_, depth + 1)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?,
                )?,
                TypeDefKind::Tuple { fields } => aggregate_layout(
                    LayoutAggregate::Tuple,
                    fields
                        .iter()
                        .enumerate()
                        .map(|(index, type_)| {
                            Ok((
                                index.to_string(),
                                layout_validated(world, type_, depth + 1)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?,
                )?,
                TypeDefKind::Enum { cases } => Layout {
                    size: 4,
                    align: 4,
                    shape: LayoutShape::Enum {
                        cases: cases.len() as u32,
                    },
                    flat: Flattening::Direct(vec![CoreType::I32]),
                },
                TypeDefKind::Flags { flags } => {
                    if flags.len() > 32 {
                        return Err(CodecError::Unsupported(
                            "more than 32 flags require a multiword flags ABI".into(),
                        ));
                    }
                    Layout {
                        size: 4,
                        align: 4,
                        shape: LayoutShape::Flags {
                            count: flags.len() as u32,
                        },
                        flat: Flattening::Direct(vec![CoreType::I32]),
                    }
                }
                TypeDefKind::Variant { cases } => variant_layout(
                    cases
                        .iter()
                        .map(|case| {
                            Ok((
                                case.name.clone(),
                                case.payload
                                    .as_ref()
                                    .map(|type_| layout_validated(world, type_, depth + 1))
                                    .transpose()?,
                            ))
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?,
                )?,
                TypeDefKind::Callback { .. } => handle_layout(LayoutShape::Handle),
            }
        }
    })
}

fn descriptor(shape: LayoutShape) -> Layout {
    Layout {
        size: 8,
        align: 4,
        shape,
        flat: Flattening::Direct(vec![CoreType::I32, CoreType::I32]),
    }
}

fn handle_layout(shape: LayoutShape) -> Layout {
    Layout {
        size: 4,
        align: 4,
        shape,
        flat: Flattening::Direct(vec![CoreType::I32]),
    }
}

enum LayoutAggregate {
    Record,
    Tuple,
}

fn aggregate_layout(
    kind: LayoutAggregate,
    fields: Vec<(String, Layout)>,
) -> Result<Layout, CodecError> {
    let mut offset = 0;
    let mut align = 1;
    let mut flat = Vec::new();
    let mut layouts = Vec::new();
    for (name, layout) in fields {
        offset = align_to(offset, layout.align)?;
        layouts.push(FieldLayout {
            name,
            offset,
            layout: layout.clone(),
        });
        offset = offset
            .checked_add(layout.size)
            .ok_or(CodecError::Overflow)?;
        align = align.max(layout.align);
        match layout.flat {
            Flattening::Direct(values) => flat.extend(values),
            Flattening::Indirect => flat.resize(17, CoreType::I32),
        }
    }
    let size = align_to(offset, align)?;
    let flat = if flat.len() <= 16 {
        Flattening::Direct(flat)
    } else {
        Flattening::Indirect
    };
    Ok(Layout {
        size,
        align,
        shape: match kind {
            LayoutAggregate::Record => LayoutShape::Record(layouts),
            LayoutAggregate::Tuple => LayoutShape::Tuple(layouts),
        },
        flat,
    })
}

fn variant_layout(cases: Vec<(String, Option<Layout>)>) -> Result<Layout, CodecError> {
    let payload_align = cases
        .iter()
        .filter_map(|(_, payload)| payload.as_ref().map(|layout| layout.align))
        .max()
        .unwrap_or(1);
    let payload_size = cases
        .iter()
        .filter_map(|(_, payload)| payload.as_ref().map(|layout| layout.size))
        .max()
        .unwrap_or(0);
    let payload_offset = align_to(4, payload_align)?;
    let align = 4.max(payload_align);
    let size = align_to(
        payload_offset
            .checked_add(payload_size)
            .ok_or(CodecError::Overflow)?,
        align,
    )?;
    Ok(Layout {
        size,
        align,
        shape: LayoutShape::Variant(VariantLayout {
            cases: cases
                .into_iter()
                .map(|(name, payload)| CaseLayout { name, payload })
                .collect(),
            payload_offset,
            payload_size,
            payload_align,
        }),
        // Variant lane joining belongs in a later direct-call ABI. Canonical
        // memory is executable now and deliberately passes variants indirectly.
        flat: Flattening::Indirect,
    })
}

fn align_to(value: u32, align: u32) -> Result<u32, CodecError> {
    debug_assert!(align.is_power_of_two());
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
        .ok_or(CodecError::Overflow)
}

pub fn function_plan(
    world: &World,
    function: &Function,
    direction: BoundaryDirection,
) -> Result<FunctionPlan, CodecError> {
    world
        .validate()
        .map_err(|error| CodecError::InvalidWorld(error.to_string()))?;
    if function.signature.async_ {
        return Err(CodecError::Unsupported(
            "async functions require a scheduler and wakeup protocol".into(),
        ));
    }
    let mut requirements = BTreeSet::new();
    let params = function
        .signature
        .params
        .iter()
        .map(|param| {
            plan_value(
                world,
                &param.type_,
                ValuePosition::Parameter,
                direction,
                &mut requirements,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = function
        .signature
        .result
        .as_ref()
        .map(|type_| {
            plan_value(
                world,
                type_,
                ValuePosition::Result,
                direction,
                &mut requirements,
            )
        })
        .transpose()?;
    Ok(FunctionPlan {
        namespace: function.namespace.clone(),
        name: function.name.clone(),
        direction,
        params,
        result,
        requirements,
    })
}

fn plan_value(
    world: &World,
    type_: &Type,
    position: ValuePosition,
    _direction: BoundaryDirection,
    requirements: &mut BTreeSet<PlanRequirement>,
) -> Result<ValuePlan, CodecError> {
    let layout = layout_validated(world, type_, 0)?;
    collect_requirements(world, type_, requirements)?;
    let ownership = match type_ {
        Type::Handle(handle) if handle.ownership == HandleOwnership::Own => {
            requirements.insert(PlanRequirement::ResourceTransfer);
            TransferOwnership::Own
        }
        Type::Handle(_) => {
            if position != ValuePosition::Parameter {
                return Err(CodecError::Unsupported(
                    "borrowed resource handles cannot cross as results".into(),
                ));
            }
            requirements.insert(PlanRequirement::BorrowScope);
            TransferOwnership::Borrow
        }
        Type::Buffer(buffer) if buffer.ownership == BufferOwnership::Borrow => {
            if position != ValuePosition::Parameter {
                return Err(CodecError::Unsupported(
                    "borrowed buffers cannot cross as results".into(),
                ));
            }
            requirements.insert(PlanRequirement::BorrowScope);
            TransferOwnership::Borrow
        }
        _ => TransferOwnership::Value,
    };
    if contains_allocation(world, type_)? {
        requirements.extend([PlanRequirement::Realloc, PlanRequirement::PostReturn]);
    }
    Ok(ValuePlan {
        type_: type_.clone(),
        layout,
        position,
        ownership,
    })
}

fn collect_requirements(
    world: &World,
    type_: &Type,
    requirements: &mut BTreeSet<PlanRequirement>,
) -> Result<(), CodecError> {
    match type_ {
        Type::Future(payload) => {
            requirements.insert(PlanRequirement::FutureTable);
            if let Some(payload) = payload {
                collect_requirements(world, payload, requirements)?;
            }
        }
        Type::Handle(handle) => {
            if handle.ownership == HandleOwnership::Own {
                requirements.insert(PlanRequirement::ResourceTransfer);
            } else {
                requirements.insert(PlanRequirement::BorrowScope);
            }
        }
        Type::Stream(_) => {
            return Err(CodecError::Unsupported(
                "stream transport requires runtime polling mechanics".into(),
            ));
        }
        Type::Named(name) => {
            let definition = definition(world, name)?;
            match &definition.kind {
                TypeDefKind::Callback { .. } => {
                    requirements.insert(PlanRequirement::CallbackTable);
                }
                TypeDefKind::Alias { target } => {
                    collect_requirements(world, target, requirements)?;
                }
                TypeDefKind::Record { fields } => {
                    for field in fields {
                        collect_requirements(world, &field.type_, requirements)?;
                    }
                }
                TypeDefKind::Tuple { fields } => {
                    for field in fields {
                        collect_requirements(world, field, requirements)?;
                    }
                }
                TypeDefKind::Variant { cases } => {
                    for payload in cases.iter().filter_map(|case| case.payload.as_ref()) {
                        collect_requirements(world, payload, requirements)?;
                    }
                }
                TypeDefKind::Enum { .. } | TypeDefKind::Flags { .. } => {}
            }
        }
        Type::List(inner) | Type::Option(inner) => {
            collect_requirements(world, inner, requirements)?;
        }
        Type::Result(result) => {
            for inner in [result.ok.as_deref(), result.error.as_deref()]
                .into_iter()
                .flatten()
            {
                collect_requirements(world, inner, requirements)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn contains_allocation(world: &World, type_: &Type) -> Result<bool, CodecError> {
    Ok(match type_ {
        Type::String(_) | Type::List(_) | Type::Buffer(_) => true,
        Type::Option(inner) => contains_allocation(world, inner)?,
        Type::Result(result) => {
            result
                .ok
                .as_deref()
                .map(|type_| contains_allocation(world, type_))
                .transpose()?
                .unwrap_or(false)
                || result
                    .error
                    .as_deref()
                    .map(|type_| contains_allocation(world, type_))
                    .transpose()?
                    .unwrap_or(false)
        }
        Type::Named(name) => match &definition(world, name)?.kind {
            TypeDefKind::Alias { target } => contains_allocation(world, target)?,
            TypeDefKind::Record { fields } => fields
                .iter()
                .map(|field| contains_allocation(world, &field.type_))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|value| value),
            TypeDefKind::Tuple { fields } => fields
                .iter()
                .map(|type_| contains_allocation(world, type_))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|value| value),
            TypeDefKind::Variant { cases } => cases
                .iter()
                .filter_map(|case| case.payload.as_ref())
                .map(|type_| contains_allocation(world, type_))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|value| value),
            TypeDefKind::Enum { .. } | TypeDefKind::Flags { .. } | TypeDefKind::Callback { .. } => {
                false
            }
        },
        _ => false,
    })
}

fn definition<'a>(world: &'a World, name: &str) -> Result<&'a fe_host_abi::TypeDef, CodecError> {
    world
        .types
        .iter()
        .find(|definition| definition.name == name)
        .ok_or_else(|| CodecError::UnknownType(name.into()))
}

pub fn encode(
    world: &World,
    type_: &Type,
    value: &Value,
    memory: &mut dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
) -> Result<CleanupPlan, CodecError> {
    let layout = layout(world, type_)?;
    checked_region(memory, offset, layout.size, layout.align)?;
    let mut cleanup = CleanupPlan::default();
    encode_at(
        world,
        type_,
        &layout,
        value,
        memory,
        offset,
        direction,
        &mut cleanup,
    )?;
    Ok(cleanup)
}

pub fn decode(
    world: &World,
    type_: &Type,
    memory: &dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
) -> Result<(Value, CleanupPlan), CodecError> {
    let layout = layout(world, type_)?;
    checked_region(memory, offset, layout.size, layout.align)?;
    let mut cleanup = CleanupPlan::default();
    let value = decode_at(
        world,
        type_,
        &layout,
        memory,
        offset,
        direction,
        &mut cleanup,
    )?;
    Ok((value, cleanup))
}

#[allow(clippy::too_many_arguments)]
fn encode_at(
    world: &World,
    type_: &Type,
    layout: &Layout,
    value: &Value,
    memory: &mut dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<(), CodecError> {
    macro_rules! number {
        ($variant:ident, $bytes:expr) => {
            if let Value::$variant(value) = value {
                write(memory, offset, &$bytes(*value))?
            } else {
                return Err(CodecError::TypeMismatch {
                    expected: stringify!($variant),
                });
            }
        };
    }
    match type_ {
        Type::Bool => {
            let Value::Bool(value) = value else {
                return mismatch("Bool");
            };
            write(memory, offset, &[*value as u8])?;
        }
        Type::I8 => number!(I8, i8::to_le_bytes),
        Type::U8 => number!(U8, u8::to_le_bytes),
        Type::I16 => number!(I16, i16::to_le_bytes),
        Type::U16 => number!(U16, u16::to_le_bytes),
        Type::I32 => number!(I32, i32::to_le_bytes),
        Type::U32 => number!(U32, u32::to_le_bytes),
        Type::I64 => number!(I64, i64::to_le_bytes),
        Type::U64 => number!(U64, u64::to_le_bytes),
        Type::F32 => number!(F32, f32::to_le_bytes),
        Type::F64 => number!(F64, f64::to_le_bytes),
        Type::Char => {
            let Value::Char(value) = value else {
                return mismatch("Char");
            };
            write_u32(memory, offset, *value as u32)?;
        }
        Type::String(encoding) => {
            let Value::String(value) = value else {
                return mismatch("String");
            };
            let bytes = encode_string(value, *encoding)?;
            let unit = if *encoding == StringEncoding::Utf16 {
                2
            } else {
                1
            };
            let byte_len = u32::try_from(bytes.len()).map_err(|_| CodecError::Overflow)?;
            let len = byte_len.checked_div(unit).ok_or(CodecError::Overflow)?;
            let ptr = allocate(memory, byte_len, unit, direction, cleanup)?;
            write(memory, ptr, &bytes)?;
            write_descriptor(memory, offset, ptr, len)?;
        }
        Type::List(element) => {
            let Value::List(values) = value else {
                return mismatch("List");
            };
            let LayoutShape::List(element_layout) = &layout.shape else {
                unreachable!()
            };
            let len = u32::try_from(values.len()).map_err(|_| CodecError::Overflow)?;
            let bytes = len
                .checked_mul(element_layout.size)
                .ok_or(CodecError::Overflow)?;
            let ptr = allocate(memory, bytes, element_layout.align, direction, cleanup)?;
            for (index, value) in values.iter().enumerate() {
                let item = ptr
                    .checked_add(
                        (index as u32)
                            .checked_mul(element_layout.size)
                            .ok_or(CodecError::Overflow)?,
                    )
                    .ok_or(CodecError::Overflow)?;
                encode_at(
                    world,
                    element,
                    element_layout,
                    value,
                    memory,
                    item,
                    direction,
                    cleanup,
                )?;
            }
            write_descriptor(memory, offset, ptr, len)?;
        }
        Type::Buffer(buffer) => {
            let Value::Buffer(value) = value else {
                return mismatch("Buffer");
            };
            encode_buffer(memory, offset, buffer.element, value, direction, cleanup)?;
            if buffer.ownership == BufferOwnership::Borrow {
                cleanup.actions.push(Cleanup {
                    kind: CleanupKind::BorrowEnd,
                    actor: destination(direction),
                    ptr: read_u32(memory, offset)?,
                    size: buffer_byte_len(buffer.element, value)?,
                    align: buffer_element_layout(buffer.element).1,
                });
            }
        }
        Type::Handle(handle) => {
            let Value::Handle(raw) = value else {
                return mismatch("Handle");
            };
            encode_handle(memory, offset, *raw)?;
            if handle.ownership == HandleOwnership::Own {
                cleanup.actions.push(Cleanup {
                    kind: CleanupKind::ResourceTransfer,
                    actor: destination(direction),
                    ptr: offset,
                    size: 4,
                    align: 4,
                });
            } else {
                cleanup.actions.push(Cleanup {
                    kind: CleanupKind::BorrowEnd,
                    actor: destination(direction),
                    ptr: offset,
                    size: 4,
                    align: 4,
                });
            }
        }
        Type::Future(_) => {
            let Value::Future(raw) = value else {
                return mismatch("Future");
            };
            encode_handle(memory, offset, *raw)?;
        }
        Type::Stream(_) => {
            return Err(CodecError::Unsupported(
                "stream transport requires runtime polling mechanics".into(),
            ));
        }
        Type::Option(_) | Type::Result(_) => {
            encode_variant(
                world, type_, layout, value, memory, offset, direction, cleanup,
            )?;
        }
        Type::Named(name) => {
            let definition = definition(world, name)?;
            match &definition.kind {
                TypeDefKind::Alias { target } => encode_at(
                    world, target, layout, value, memory, offset, direction, cleanup,
                )?,
                TypeDefKind::Record { fields } => {
                    let Value::Record(values) = value else {
                        return mismatch("Record");
                    };
                    encode_fields(
                        world, fields, layout, values, memory, offset, direction, cleanup,
                    )?;
                }
                TypeDefKind::Tuple { fields } => {
                    let Value::Tuple(values) = value else {
                        return mismatch("Tuple");
                    };
                    encode_tuple(
                        world, fields, layout, values, memory, offset, direction, cleanup,
                    )?;
                }
                TypeDefKind::Enum { cases } => {
                    let Value::Enum(tag) = value else {
                        return mismatch("Enum");
                    };
                    check_tag(*tag, cases.len() as u32)?;
                    write_u32(memory, offset, *tag)?;
                }
                TypeDefKind::Flags { flags } => {
                    let Value::Flags(bits) = value else {
                        return mismatch("Flags");
                    };
                    let allowed = if flags.len() == 32 {
                        u32::MAX
                    } else {
                        (1u32 << flags.len()) - 1
                    };
                    if bits & !allowed != 0 {
                        return Err(CodecError::InvalidTag {
                            tag: *bits,
                            cases: flags.len() as u32,
                        });
                    }
                    write_u32(memory, offset, *bits)?;
                }
                TypeDefKind::Variant { .. } => {
                    encode_variant(
                        world, type_, layout, value, memory, offset, direction, cleanup,
                    )?;
                }
                TypeDefKind::Callback { .. } => {
                    let Value::Handle(raw) = value else {
                        return mismatch("Handle");
                    };
                    encode_handle(memory, offset, *raw)?;
                }
            }
        }
    }
    Ok(())
}

fn decode_at(
    world: &World,
    type_: &Type,
    layout: &Layout,
    memory: &dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<Value, CodecError> {
    Ok(match type_ {
        Type::Bool => match read_array::<1>(memory, offset)?[0] {
            0 => Value::Bool(false),
            1 => Value::Bool(true),
            value => return Err(CodecError::InvalidBool(value)),
        },
        Type::I8 => Value::I8(i8::from_le_bytes(read_array(memory, offset)?)),
        Type::U8 => Value::U8(u8::from_le_bytes(read_array(memory, offset)?)),
        Type::I16 => Value::I16(i16::from_le_bytes(read_array(memory, offset)?)),
        Type::U16 => Value::U16(u16::from_le_bytes(read_array(memory, offset)?)),
        Type::I32 => Value::I32(i32::from_le_bytes(read_array(memory, offset)?)),
        Type::U32 => Value::U32(read_u32(memory, offset)?),
        Type::I64 => Value::I64(i64::from_le_bytes(read_array(memory, offset)?)),
        Type::U64 => Value::U64(u64::from_le_bytes(read_array(memory, offset)?)),
        Type::F32 => Value::F32(f32::from_le_bytes(read_array(memory, offset)?)),
        Type::F64 => Value::F64(f64::from_le_bytes(read_array(memory, offset)?)),
        Type::Char => Value::Char(
            char::from_u32(read_u32(memory, offset)?)
                .ok_or_else(|| CodecError::InvalidChar(read_u32(memory, offset).unwrap_or(0)))?,
        ),
        Type::String(encoding) => {
            let (ptr, len) = read_descriptor(memory, offset)?;
            let unit = if *encoding == StringEncoding::Utf16 {
                2
            } else {
                1
            };
            let bytes_len = len.checked_mul(unit).ok_or(CodecError::Overflow)?;
            let bytes = read_vec(memory, ptr, bytes_len, unit)?;
            add_post_return(cleanup, ptr, bytes_len, unit, direction);
            Value::String(decode_string(&bytes, *encoding)?)
        }
        Type::List(element) => {
            let (ptr, len) = read_descriptor(memory, offset)?;
            let LayoutShape::List(element_layout) = &layout.shape else {
                unreachable!()
            };
            let bytes = len
                .checked_mul(element_layout.size)
                .ok_or(CodecError::Overflow)?;
            checked_region(memory, ptr, bytes, element_layout.align)?;
            let mut values = Vec::with_capacity(len as usize);
            for index in 0..len {
                values.push(decode_at(
                    world,
                    element,
                    element_layout,
                    memory,
                    ptr + index * element_layout.size,
                    direction,
                    cleanup,
                )?);
            }
            add_post_return(cleanup, ptr, bytes, element_layout.align, direction);
            Value::List(values)
        }
        Type::Buffer(buffer) => {
            let (value, ptr, bytes, align) = decode_buffer(memory, offset, buffer.element)?;
            cleanup.actions.push(Cleanup {
                kind: if buffer.ownership == BufferOwnership::Borrow {
                    CleanupKind::BorrowEnd
                } else {
                    CleanupKind::PostReturn
                },
                actor: destination(direction),
                ptr,
                size: bytes,
                align,
            });
            Value::Buffer(value)
        }
        Type::Handle(handle) => {
            let raw = decode_handle(memory, offset)?;
            cleanup.actions.push(Cleanup {
                kind: if handle.ownership == HandleOwnership::Borrow {
                    CleanupKind::BorrowEnd
                } else {
                    CleanupKind::ResourceTransfer
                },
                actor: destination(direction),
                ptr: offset,
                size: 4,
                align: 4,
            });
            Value::Handle(raw)
        }
        Type::Future(_) => Value::Future(decode_handle(memory, offset)?),
        Type::Stream(_) => {
            return Err(CodecError::Unsupported(
                "stream transport requires runtime polling mechanics".into(),
            ));
        }
        Type::Option(_) | Type::Result(_) => {
            decode_variant(world, type_, layout, memory, offset, direction, cleanup)?
        }
        Type::Named(name) => {
            let definition = definition(world, name)?;
            match &definition.kind {
                TypeDefKind::Alias { target } => {
                    decode_at(world, target, layout, memory, offset, direction, cleanup)?
                }
                TypeDefKind::Record { fields } => Value::Record(decode_fields(
                    world, fields, layout, memory, offset, direction, cleanup,
                )?),
                TypeDefKind::Tuple { fields } => Value::Tuple(decode_tuple(
                    world, fields, layout, memory, offset, direction, cleanup,
                )?),
                TypeDefKind::Enum { cases } => {
                    let tag = read_u32(memory, offset)?;
                    check_tag(tag, cases.len() as u32)?;
                    Value::Enum(tag)
                }
                TypeDefKind::Flags { flags } => {
                    let bits = read_u32(memory, offset)?;
                    let allowed = if flags.len() == 32 {
                        u32::MAX
                    } else {
                        (1u32 << flags.len()) - 1
                    };
                    if bits & !allowed != 0 {
                        return Err(CodecError::InvalidTag {
                            tag: bits,
                            cases: flags.len() as u32,
                        });
                    }
                    Value::Flags(bits)
                }
                TypeDefKind::Variant { .. } => {
                    decode_variant(world, type_, layout, memory, offset, direction, cleanup)?
                }
                TypeDefKind::Callback { .. } => Value::Handle(decode_handle(memory, offset)?),
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn encode_fields(
    world: &World,
    fields: &[fe_host_abi::Field],
    layout: &Layout,
    values: &[Value],
    memory: &mut dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<(), CodecError> {
    if fields.len() != values.len() {
        return mismatch("Record");
    }
    let LayoutShape::Record(layouts) = &layout.shape else {
        unreachable!()
    };
    for ((field, value), layout) in fields.iter().zip(values).zip(layouts) {
        encode_at(
            world,
            &field.type_,
            &layout.layout,
            value,
            memory,
            offset + layout.offset,
            direction,
            cleanup,
        )?;
    }
    Ok(())
}

fn decode_fields(
    world: &World,
    fields: &[fe_host_abi::Field],
    layout: &Layout,
    memory: &dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<Vec<Value>, CodecError> {
    let LayoutShape::Record(layouts) = &layout.shape else {
        unreachable!()
    };
    fields
        .iter()
        .zip(layouts)
        .map(|(field, layout)| {
            decode_at(
                world,
                &field.type_,
                &layout.layout,
                memory,
                offset + layout.offset,
                direction,
                cleanup,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn encode_tuple(
    world: &World,
    fields: &[Type],
    layout: &Layout,
    values: &[Value],
    memory: &mut dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<(), CodecError> {
    if fields.len() != values.len() {
        return mismatch("Tuple");
    }
    let LayoutShape::Tuple(layouts) = &layout.shape else {
        unreachable!()
    };
    for ((type_, value), layout) in fields.iter().zip(values).zip(layouts) {
        encode_at(
            world,
            type_,
            &layout.layout,
            value,
            memory,
            offset + layout.offset,
            direction,
            cleanup,
        )?;
    }
    Ok(())
}

fn decode_tuple(
    world: &World,
    fields: &[Type],
    layout: &Layout,
    memory: &dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<Vec<Value>, CodecError> {
    let LayoutShape::Tuple(layouts) = &layout.shape else {
        unreachable!()
    };
    fields
        .iter()
        .zip(layouts)
        .map(|(type_, layout)| {
            decode_at(
                world,
                type_,
                &layout.layout,
                memory,
                offset + layout.offset,
                direction,
                cleanup,
            )
        })
        .collect()
}

fn variant_cases<'a>(
    world: &'a World,
    type_: &'a Type,
) -> Result<Vec<Option<&'a Type>>, CodecError> {
    Ok(match type_ {
        Type::Option(payload) => vec![None, Some(payload)],
        Type::Result(result) => vec![result.ok.as_deref(), result.error.as_deref()],
        Type::Named(name) => match &definition(world, name)?.kind {
            TypeDefKind::Variant { cases } => {
                cases.iter().map(|case| case.payload.as_ref()).collect()
            }
            _ => {
                return Err(CodecError::TypeMismatch {
                    expected: "Variant",
                });
            }
        },
        _ => {
            return Err(CodecError::TypeMismatch {
                expected: "Variant",
            });
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn encode_variant(
    world: &World,
    type_: &Type,
    layout: &Layout,
    value: &Value,
    memory: &mut dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<(), CodecError> {
    let Value::Variant { case, payload } = value else {
        return mismatch("Variant");
    };
    let cases = variant_cases(world, type_)?;
    check_tag(*case, cases.len() as u32)?;
    write_u32(memory, offset, *case)?;
    let LayoutShape::Variant(variant) = &layout.shape else {
        unreachable!()
    };
    match (cases[*case as usize], payload.as_deref()) {
        (Some(type_), Some(value)) => encode_at(
            world,
            type_,
            variant.cases[*case as usize]
                .payload
                .as_ref()
                .expect("payload layout"),
            value,
            memory,
            offset + variant.payload_offset,
            direction,
            cleanup,
        ),
        (None, None) => Ok(()),
        _ => mismatch("Variant payload"),
    }
}

fn decode_variant(
    world: &World,
    type_: &Type,
    layout: &Layout,
    memory: &dyn LinearMemory,
    offset: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<Value, CodecError> {
    let cases = variant_cases(world, type_)?;
    let tag = read_u32(memory, offset)?;
    check_tag(tag, cases.len() as u32)?;
    let LayoutShape::Variant(variant) = &layout.shape else {
        unreachable!()
    };
    let payload = cases[tag as usize]
        .map(|type_| {
            decode_at(
                world,
                type_,
                variant.cases[tag as usize]
                    .payload
                    .as_ref()
                    .expect("payload layout"),
                memory,
                offset + variant.payload_offset,
                direction,
                cleanup,
            )
            .map(Box::new)
        })
        .transpose()?;
    Ok(Value::Variant { case: tag, payload })
}

fn check_tag(tag: u32, cases: u32) -> Result<(), CodecError> {
    if tag < cases {
        Ok(())
    } else {
        Err(CodecError::InvalidTag { tag, cases })
    }
}

fn encode_handle(
    memory: &mut dyn LinearMemory,
    offset: u32,
    token: HandleToken,
) -> Result<(), CodecError> {
    write_u32(memory, offset, token.0)
}

fn decode_handle(memory: &dyn LinearMemory, offset: u32) -> Result<HandleToken, CodecError> {
    Ok(HandleToken(read_u32(memory, offset)?))
}

fn encode_string(value: &str, encoding: StringEncoding) -> Result<Vec<u8>, CodecError> {
    Ok(match encoding {
        StringEncoding::Utf8 => value.as_bytes().to_vec(),
        StringEncoding::Utf16 => value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        StringEncoding::Latin1 => value
            .chars()
            .map(|ch| u8::try_from(ch as u32).map_err(|_| CodecError::InvalidLatin1))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn decode_string(bytes: &[u8], encoding: StringEncoding) -> Result<String, CodecError> {
    match encoding {
        StringEncoding::Utf8 => {
            String::from_utf8(bytes.to_vec()).map_err(|_| CodecError::InvalidUtf8)
        }
        StringEncoding::Utf16 => {
            if !bytes.len().is_multiple_of(2) {
                return Err(CodecError::InvalidUtf16);
            }
            char::decode_utf16(
                bytes
                    .chunks_exact(2)
                    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]])),
            )
            .collect::<Result<String, _>>()
            .map_err(|_| CodecError::InvalidUtf16)
        }
        StringEncoding::Latin1 => Ok(bytes.iter().map(|byte| char::from(*byte)).collect()),
    }
}

fn encode_buffer(
    memory: &mut dyn LinearMemory,
    offset: u32,
    element: BufferElement,
    value: &BufferValue,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<(), CodecError> {
    macro_rules! buffer {
        ($variant:ident, $method:ident) => {
            if let BufferValue::$variant(values) = value {
                values
                    .iter()
                    .flat_map(|value| value.$method())
                    .collect::<Vec<_>>()
            } else {
                return mismatch("typed Buffer");
            }
        };
    }
    let bytes = match element {
        BufferElement::I8 => buffer!(I8, to_le_bytes),
        BufferElement::U8 => {
            let BufferValue::U8(values) = value else {
                return mismatch("typed Buffer");
            };
            values.clone()
        }
        BufferElement::I16 => buffer!(I16, to_le_bytes),
        BufferElement::U16 => buffer!(U16, to_le_bytes),
        BufferElement::I32 => buffer!(I32, to_le_bytes),
        BufferElement::U32 => buffer!(U32, to_le_bytes),
        BufferElement::I64 => buffer!(I64, to_le_bytes),
        BufferElement::U64 => buffer!(U64, to_le_bytes),
        BufferElement::F32 => buffer!(F32, to_le_bytes),
        BufferElement::F64 => buffer!(F64, to_le_bytes),
    };
    let (size, align) = buffer_element_layout(element);
    let len = u32::try_from(bytes.len())
        .map_err(|_| CodecError::Overflow)?
        .checked_div(size)
        .ok_or(CodecError::Overflow)?;
    let byte_len = u32::try_from(bytes.len()).map_err(|_| CodecError::Overflow)?;
    let ptr = allocate(memory, byte_len, align, direction, cleanup)?;
    write(memory, ptr, &bytes)?;
    write_descriptor(memory, offset, ptr, len)
}

fn decode_buffer(
    memory: &dyn LinearMemory,
    offset: u32,
    element: BufferElement,
) -> Result<(BufferValue, u32, u32, u32), CodecError> {
    let (ptr, len) = read_descriptor(memory, offset)?;
    let (size, align) = buffer_element_layout(element);
    let bytes_len = len.checked_mul(size).ok_or(CodecError::Overflow)?;
    let bytes = read_vec(memory, ptr, bytes_len, align)?;
    macro_rules! values {
        ($size:literal, $type:ty, $variant:ident) => {
            BufferValue::$variant(
                bytes
                    .chunks_exact($size)
                    .map(|chunk| <$type>::from_le_bytes(chunk.try_into().unwrap()))
                    .collect(),
            )
        };
    }
    let value = match element {
        BufferElement::I8 => BufferValue::I8(bytes.iter().map(|value| *value as i8).collect()),
        BufferElement::U8 => BufferValue::U8(bytes),
        BufferElement::I16 => values!(2, i16, I16),
        BufferElement::U16 => values!(2, u16, U16),
        BufferElement::I32 => values!(4, i32, I32),
        BufferElement::U32 => values!(4, u32, U32),
        BufferElement::I64 => values!(8, i64, I64),
        BufferElement::U64 => values!(8, u64, U64),
        BufferElement::F32 => values!(4, f32, F32),
        BufferElement::F64 => values!(8, f64, F64),
    };
    Ok((value, ptr, bytes_len, align))
}

fn buffer_byte_len(element: BufferElement, value: &BufferValue) -> Result<u32, CodecError> {
    let len = match value {
        BufferValue::I8(values) => values.len(),
        BufferValue::U8(values) => values.len(),
        BufferValue::I16(values) => values.len(),
        BufferValue::U16(values) => values.len(),
        BufferValue::I32(values) => values.len(),
        BufferValue::U32(values) => values.len(),
        BufferValue::I64(values) => values.len(),
        BufferValue::U64(values) => values.len(),
        BufferValue::F32(values) => values.len(),
        BufferValue::F64(values) => values.len(),
    };
    u32::try_from(len)
        .map_err(|_| CodecError::Overflow)?
        .checked_mul(buffer_element_layout(element).0)
        .ok_or(CodecError::Overflow)
}

fn buffer_element_layout(element: BufferElement) -> (u32, u32) {
    match element {
        BufferElement::I8 | BufferElement::U8 => (1, 1),
        BufferElement::I16 | BufferElement::U16 => (2, 2),
        BufferElement::I32 | BufferElement::U32 | BufferElement::F32 => (4, 4),
        BufferElement::I64 | BufferElement::U64 | BufferElement::F64 => (8, 8),
    }
}

fn allocate(
    memory: &mut dyn LinearMemory,
    size: u32,
    align: u32,
    direction: BoundaryDirection,
    cleanup: &mut CleanupPlan,
) -> Result<u32, CodecError> {
    if size == 0 {
        return Ok(0);
    }
    let ptr = memory.realloc(0, 0, align, size)?;
    checked_region(memory, ptr, size, align)?;
    cleanup.actions.push(Cleanup {
        kind: CleanupKind::Realloc,
        actor: source(direction),
        ptr,
        size,
        align,
    });
    Ok(ptr)
}

fn add_post_return(
    cleanup: &mut CleanupPlan,
    ptr: u32,
    size: u32,
    align: u32,
    direction: BoundaryDirection,
) {
    if size != 0 {
        cleanup.actions.push(Cleanup {
            kind: CleanupKind::PostReturn,
            actor: destination(direction),
            ptr,
            size,
            align,
        });
    }
}

const fn source(direction: BoundaryDirection) -> BoundarySide {
    match direction {
        BoundaryDirection::GuestToHost => BoundarySide::Guest,
        BoundaryDirection::HostToGuest => BoundarySide::Host,
    }
}

const fn destination(direction: BoundaryDirection) -> BoundarySide {
    match direction {
        BoundaryDirection::GuestToHost => BoundarySide::Host,
        BoundaryDirection::HostToGuest => BoundarySide::Guest,
    }
}

fn write_descriptor(
    memory: &mut dyn LinearMemory,
    offset: u32,
    ptr: u32,
    len: u32,
) -> Result<(), CodecError> {
    write_u32(memory, offset, ptr)?;
    write_u32(memory, offset + 4, len)
}

fn read_descriptor(memory: &dyn LinearMemory, offset: u32) -> Result<(u32, u32), CodecError> {
    Ok((read_u32(memory, offset)?, read_u32(memory, offset + 4)?))
}

fn write_u32(memory: &mut dyn LinearMemory, offset: u32, value: u32) -> Result<(), CodecError> {
    write(memory, offset, &value.to_le_bytes())
}

fn read_u32(memory: &dyn LinearMemory, offset: u32) -> Result<u32, CodecError> {
    Ok(u32::from_le_bytes(read_array(memory, offset)?))
}

fn read_array<const N: usize>(
    memory: &dyn LinearMemory,
    offset: u32,
) -> Result<[u8; N], CodecError> {
    let mut bytes = [0; N];
    memory.read(offset, &mut bytes)?;
    Ok(bytes)
}

fn read_vec(
    memory: &dyn LinearMemory,
    offset: u32,
    length: u32,
    align: u32,
) -> Result<Vec<u8>, CodecError> {
    checked_region(memory, offset, length, align)?;
    let mut bytes = vec![0; length as usize];
    memory.read(offset, &mut bytes)?;
    Ok(bytes)
}

fn write(memory: &mut dyn LinearMemory, offset: u32, bytes: &[u8]) -> Result<(), CodecError> {
    checked_region(memory, offset, bytes.len() as u32, 1)?;
    memory.write(offset, bytes)
}

fn checked_region(
    memory: &dyn LinearMemory,
    offset: u32,
    length: u32,
    align: u32,
) -> Result<(), CodecError> {
    if align == 0 || !align.is_power_of_two() || !offset.is_multiple_of(align) {
        return Err(CodecError::InvalidAlignment { offset, align });
    }
    let end = offset.checked_add(length).ok_or(CodecError::Overflow)?;
    if end > memory.size() {
        return Err(CodecError::OutOfBounds {
            offset,
            length,
            memory_size: memory.size(),
        });
    }
    Ok(())
}

fn mismatch<T>(expected: &'static str) -> Result<T, CodecError> {
    Err(CodecError::TypeMismatch { expected })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use fe_host_abi::{
        Buffer, Case, Field, FunctionType, Handle, Param, ResultType, TypeDef, TypeDefKind,
    };

    use super::*;

    struct Memory {
        bytes: Vec<u8>,
        cursor: u32,
    }

    impl Memory {
        fn new(size: usize, cursor: u32) -> Self {
            Self {
                bytes: vec![0; size],
                cursor,
            }
        }
    }

    impl LinearMemory for Memory {
        fn size(&self) -> u32 {
            self.bytes.len() as u32
        }

        fn read(&self, offset: u32, bytes: &mut [u8]) -> Result<(), CodecError> {
            let end = offset
                .checked_add(bytes.len() as u32)
                .ok_or(CodecError::Overflow)?;
            let source =
                self.bytes
                    .get(offset as usize..end as usize)
                    .ok_or(CodecError::OutOfBounds {
                        offset,
                        length: bytes.len() as u32,
                        memory_size: self.size(),
                    })?;
            bytes.copy_from_slice(source);
            Ok(())
        }

        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), CodecError> {
            let end = offset
                .checked_add(bytes.len() as u32)
                .ok_or(CodecError::Overflow)?;
            let memory_size = self.size();
            let target = self.bytes.get_mut(offset as usize..end as usize).ok_or(
                CodecError::OutOfBounds {
                    offset,
                    length: bytes.len() as u32,
                    memory_size,
                },
            )?;
            target.copy_from_slice(bytes);
            Ok(())
        }

        fn realloc(
            &mut self,
            _old_ptr: u32,
            _old_size: u32,
            align: u32,
            new_size: u32,
        ) -> Result<u32, CodecError> {
            let ptr = align_to(self.cursor, align)?;
            let end = ptr.checked_add(new_size).ok_or(CodecError::Overflow)?;
            if end > self.size() {
                return Err(CodecError::AllocationFailed);
            }
            self.cursor = end;
            Ok(ptr)
        }
    }

    fn world() -> World {
        World {
            name: "codec".into(),
            types: vec![
                TypeDef {
                    name: "choice".into(),
                    kind: TypeDefKind::Variant {
                        cases: vec![
                            Case {
                                name: "none".into(),
                                payload: None,
                            },
                            Case {
                                name: "text".into(),
                                payload: Some(Type::String(StringEncoding::Utf8)),
                            },
                        ],
                    },
                },
                TypeDef {
                    name: "packet".into(),
                    kind: TypeDefKind::Record {
                        fields: vec![
                            Field {
                                name: "choice".into(),
                                type_: Type::Named("choice".into()),
                            },
                            Field {
                                name: "id".into(),
                                type_: Type::U64,
                            },
                            Field {
                                name: "values".into(),
                                type_: Type::List(Box::new(Type::U16)),
                            },
                        ],
                    },
                },
            ],
            ..World::default()
        }
    }

    fn js_fixture() -> (World, Function) {
        let world = World {
            name: "js-fixture".into(),
            types: vec![
                TypeDef {
                    name: "reply".into(),
                    kind: TypeDefKind::Variant {
                        cases: vec![
                            Case {
                                name: "error".into(),
                                payload: Some(Type::String(StringEncoding::Utf8)),
                            },
                            Case {
                                name: "ok".into(),
                                payload: Some(Type::U32),
                            },
                        ],
                    },
                },
                TypeDef {
                    name: "request".into(),
                    kind: TypeDefKind::Record {
                        fields: vec![
                            Field {
                                name: "message".into(),
                                type_: Type::String(StringEncoding::Utf8),
                            },
                            Field {
                                name: "values".into(),
                                type_: Type::List(Box::new(Type::U32)),
                            },
                        ],
                    },
                },
            ],
            resources: vec![fe_host_abi::Resource {
                name: "channel".into(),
                methods: vec![],
            }],
            ..World::default()
        };
        let function = Function {
            namespace: "fe:fixture".into(),
            name: "send".into(),
            signature: FunctionType {
                params: vec![
                    Param {
                        name: "channel".into(),
                        type_: Type::Handle(Handle {
                            resource: "channel".into(),
                            ownership: HandleOwnership::Own,
                        }),
                    },
                    Param {
                        name: "request".into(),
                        type_: Type::Named("request".into()),
                    },
                ],
                result: Some(Type::Named("reply".into())),
                async_: false,
            },
        };
        (world, function)
    }

    #[test]
    fn checked_in_js_plan_is_exactly_the_rust_emission() {
        let (world, function) = js_fixture();
        let emitted =
            emit_function_plan_json(&world, &function, BoundaryDirection::HostToGuest).unwrap();
        assert_eq!(
            emitted,
            include_str!("../../../demos/shared/host-wasm-codec-v1.fixture.json").trim()
        );
        let decoded: SerializableCodecPlan = serde_json::from_str(&emitted).unwrap();
        assert_eq!(
            decoded.function.params[1].layout,
            layout(&world, &Type::Named("request".into())).unwrap()
        );
    }

    #[test]
    fn checked_in_buffer_layouts_are_exactly_the_rust_emission() {
        let world = World {
            name: "buffer-fixture".into(),
            ..World::default()
        };
        let elements = [
            ("f32", BufferElement::F32),
            ("f64", BufferElement::F64),
            ("i16", BufferElement::I16),
            ("i32", BufferElement::I32),
            ("i64", BufferElement::I64),
            ("i8", BufferElement::I8),
            ("u16", BufferElement::U16),
            ("u32", BufferElement::U32),
            ("u64", BufferElement::U64),
            ("u8", BufferElement::U8),
        ];
        let layouts = elements
            .into_iter()
            .map(|(name, element)| {
                (
                    name,
                    layout(
                        &world,
                        &Type::Buffer(Buffer {
                            element,
                            ownership: BufferOwnership::Own,
                        }),
                    )
                    .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let emitted = serde_json::to_string(&serde_json::json!({
            "contract": JS_CODEC_CONTRACT,
            "layouts": layouts,
        }))
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&emitted).unwrap(),
            serde_json::from_str::<serde_json::Value>(include_str!(
                "../../../demos/shared/host-wasm-codec-v1.buffers.json"
            ))
            .unwrap()
        );
    }

    #[test]
    fn golden_scalar_record_variant_and_handle_layouts() {
        let world = world();
        let packet = layout(&world, &Type::Named("packet".into())).unwrap();
        assert_eq!((packet.size, packet.align), (32, 8));
        let LayoutShape::Record(fields) = packet.shape else {
            panic!("record")
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| (field.name.as_str(), field.offset))
                .collect::<Vec<_>>(),
            [("choice", 0), ("id", 16), ("values", 24)]
        );
        let choice = layout(&world, &Type::Named("choice".into())).unwrap();
        assert_eq!(
            (choice.size, choice.align, choice.flat),
            (12, 4, Flattening::Indirect)
        );
        let handle = handle_layout(LayoutShape::Handle);
        assert_eq!((handle.size, handle.align), (4, 4));
        assert_eq!(handle.flat, Flattening::Direct(vec![CoreType::I32]));
    }

    #[test]
    fn nested_record_variant_and_list_roundtrip_is_deterministic() {
        let world = world();
        let type_ = Type::Named("packet".into());
        let value = Value::Record(vec![
            Value::Variant {
                case: 1,
                payload: Some(Box::new(Value::String("hé".into()))),
            },
            Value::U64(99),
            Value::List(vec![Value::U16(7), Value::U16(9)]),
        ]);
        let mut first = Memory::new(256, 64);
        let cleanup = encode(
            &world,
            &type_,
            &value,
            &mut first,
            0,
            BoundaryDirection::HostToGuest,
        )
        .unwrap();
        let (decoded, post) =
            decode(&world, &type_, &first, 0, BoundaryDirection::GuestToHost).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(cleanup.actions.len(), 2);
        assert_eq!(post.actions.len(), 2);

        let mut second = Memory::new(256, 64);
        encode(
            &world,
            &type_,
            &value,
            &mut second,
            0,
            BoundaryDirection::HostToGuest,
        )
        .unwrap();
        assert_eq!(first.bytes, second.bytes);
    }

    #[test]
    fn all_string_encodings_roundtrip_and_reject_invalid_data() {
        let world = World {
            name: "strings".into(),
            ..World::default()
        };
        for (encoding, text) in [
            (StringEncoding::Utf8, "hé"),
            (StringEncoding::Utf16, "a𝄞"),
            (StringEncoding::Latin1, "hé"),
        ] {
            let mut memory = Memory::new(128, 16);
            encode(
                &world,
                &Type::String(encoding),
                &Value::String(text.into()),
                &mut memory,
                0,
                BoundaryDirection::HostToGuest,
            )
            .unwrap();
            assert_eq!(
                decode(
                    &world,
                    &Type::String(encoding),
                    &memory,
                    0,
                    BoundaryDirection::GuestToHost,
                )
                .unwrap()
                .0,
                Value::String(text.into())
            );
        }

        let mut memory = Memory::new(32, 16);
        write_descriptor(&mut memory, 0, 16, 2).unwrap();
        memory.bytes[16..18].copy_from_slice(&[0xff, 0xff]);
        assert_eq!(
            decode(
                &world,
                &Type::String(StringEncoding::Utf8),
                &memory,
                0,
                BoundaryDirection::GuestToHost,
            )
            .unwrap_err(),
            CodecError::InvalidUtf8
        );
        assert_eq!(
            encode_string("€", StringEncoding::Latin1).unwrap_err(),
            CodecError::InvalidLatin1
        );
        assert_eq!(
            decode_string(&[0x00, 0xd8], StringEncoding::Utf16).unwrap_err(),
            CodecError::InvalidUtf16
        );
    }

    #[test]
    fn malformed_tags_flags_bool_and_char_fail_closed() {
        let world = world();
        let mut memory = Memory::new(64, 32);
        write_u32(&mut memory, 0, 9).unwrap();
        assert_eq!(
            decode(
                &world,
                &Type::Named("choice".into()),
                &memory,
                0,
                BoundaryDirection::GuestToHost,
            )
            .unwrap_err(),
            CodecError::InvalidTag { tag: 9, cases: 2 }
        );
        memory.bytes[0] = 2;
        assert_eq!(
            decode(
                &World {
                    name: "x".into(),
                    ..World::default()
                },
                &Type::Bool,
                &memory,
                0,
                BoundaryDirection::GuestToHost,
            )
            .unwrap_err(),
            CodecError::InvalidBool(2)
        );
        write_u32(&mut memory, 0, 0xd800).unwrap();
        assert!(matches!(
            decode(
                &World {
                    name: "x".into(),
                    ..World::default()
                },
                &Type::Char,
                &memory,
                0,
                BoundaryDirection::GuestToHost,
            ),
            Err(CodecError::InvalidChar(0xd800))
        ));

        let flags_world = World {
            name: "flags".into(),
            types: vec![TypeDef {
                name: "permissions".into(),
                kind: TypeDefKind::Flags {
                    flags: vec!["read".into(), "write".into()],
                },
            }],
            ..World::default()
        };
        write_u32(&mut memory, 0, 4).unwrap();
        assert_eq!(
            decode(
                &flags_world,
                &Type::Named("permissions".into()),
                &memory,
                0,
                BoundaryDirection::GuestToHost,
            )
            .unwrap_err(),
            CodecError::InvalidTag { tag: 4, cases: 2 }
        );
    }

    #[test]
    fn tuples_enums_flags_options_results_and_callbacks_are_canonical() {
        let world = World {
            name: "shapes".into(),
            types: vec![
                TypeDef {
                    name: "callback".into(),
                    kind: TypeDefKind::Callback {
                        signature: FunctionType {
                            params: vec![Param {
                                name: "value".into(),
                                type_: Type::U32,
                            }],
                            result: None,
                            async_: false,
                        },
                    },
                },
                TypeDef {
                    name: "mode".into(),
                    kind: TypeDefKind::Enum {
                        cases: vec!["off".into(), "on".into()],
                    },
                },
                TypeDef {
                    name: "pair".into(),
                    kind: TypeDefKind::Tuple {
                        fields: vec![Type::U8, Type::U64],
                    },
                },
                TypeDef {
                    name: "permissions".into(),
                    kind: TypeDefKind::Flags {
                        flags: vec!["read".into(), "write".into()],
                    },
                },
            ],
            ..World::default()
        };
        let pair = layout(&world, &Type::Named("pair".into())).unwrap();
        assert_eq!((pair.size, pair.align), (16, 8));
        let mut memory = Memory::new(256, 128);
        for (offset, type_, value) in [
            (
                0,
                Type::Named("pair".into()),
                Value::Tuple(vec![Value::U8(3), Value::U64(8)]),
            ),
            (16, Type::Named("mode".into()), Value::Enum(1)),
            (20, Type::Named("permissions".into()), Value::Flags(3)),
            (
                24,
                Type::Option(Box::new(Type::U32)),
                Value::Variant {
                    case: 1,
                    payload: Some(Box::new(Value::U32(55))),
                },
            ),
            (
                32,
                Type::Result(ResultType {
                    ok: Some(Box::new(Type::U32)),
                    error: None,
                }),
                Value::Variant {
                    case: 0,
                    payload: Some(Box::new(Value::U32(77))),
                },
            ),
        ] {
            encode(
                &world,
                &type_,
                &value,
                &mut memory,
                offset,
                BoundaryDirection::HostToGuest,
            )
            .unwrap();
            assert_eq!(
                decode(
                    &world,
                    &type_,
                    &memory,
                    offset,
                    BoundaryDirection::GuestToHost,
                )
                .unwrap()
                .0,
                value
            );
        }

        let callback = layout(&world, &Type::Named("callback".into())).unwrap();
        assert_eq!((callback.size, callback.align), (4, 4));
    }

    #[test]
    fn oob_unaligned_and_overflow_regions_are_rejected() {
        let world = World {
            name: "bounds".into(),
            ..World::default()
        };
        let memory = Memory::new(8, 0);
        assert_eq!(
            decode(
                &world,
                &Type::U64,
                &memory,
                1,
                BoundaryDirection::GuestToHost,
            )
            .unwrap_err(),
            CodecError::InvalidAlignment {
                offset: 1,
                align: 8
            }
        );
        assert!(matches!(
            decode(
                &world,
                &Type::U64,
                &memory,
                8,
                BoundaryDirection::GuestToHost,
            ),
            Err(CodecError::OutOfBounds { .. })
        ));
        assert_eq!(
            checked_region(&memory, u32::MAX, 2, 1).unwrap_err(),
            CodecError::Overflow
        );
    }

    #[test]
    fn typed_buffers_and_full_runtime_handles_roundtrip() {
        let mut world = World {
            name: "handles".into(),
            resources: vec![fe_host_abi::Resource {
                name: "file".into(),
                methods: vec![],
            }],
            ..World::default()
        };
        let handle_type = Type::Handle(Handle {
            resource: "file".into(),
            ownership: HandleOwnership::Own,
        });
        let raw = RawHandle {
            table: fe_host_runtime::TableId::new(NonZeroU64::new(44).unwrap()),
            slot: 7,
            generation: 3,
        };
        let mut session = HandleSession::new();
        let token = session.insert(raw).unwrap();
        let mut memory = Memory::new(128, 32);
        let cleanup = encode(
            &world,
            &handle_type,
            &Value::Handle(token),
            &mut memory,
            0,
            BoundaryDirection::HostToGuest,
        )
        .unwrap();
        assert_eq!(cleanup.actions[0].kind, CleanupKind::ResourceTransfer);
        assert_eq!(
            decode(
                &world,
                &handle_type,
                &memory,
                0,
                BoundaryDirection::GuestToHost,
            )
            .unwrap()
            .0,
            Value::Handle(token)
        );
        assert_eq!(session.resolve(token).unwrap(), raw);
        assert_eq!(session.remove(token).unwrap(), raw);
        assert_eq!(
            session.resolve(token).unwrap_err(),
            CodecError::InvalidHandleToken(token.0)
        );

        let buffer = Type::Buffer(Buffer {
            element: BufferElement::U32,
            ownership: BufferOwnership::Own,
        });
        let value = Value::Buffer(BufferValue::U32(vec![1, 0xfeed_beef]));
        encode(
            &world,
            &buffer,
            &value,
            &mut memory,
            16,
            BoundaryDirection::HostToGuest,
        )
        .unwrap();
        assert_eq!(
            decode(&world, &buffer, &memory, 16, BoundaryDirection::GuestToHost,)
                .unwrap()
                .0,
            value
        );
        world.resources.clear();
    }

    #[test]
    fn plans_are_directional_and_expose_cleanup_obligations() {
        let world = World {
            name: "plan".into(),
            resources: vec![fe_host_abi::Resource {
                name: "file".into(),
                methods: vec![],
            }],
            imports: vec![Function {
                namespace: "fe:host".into(),
                name: "read".into(),
                signature: FunctionType {
                    params: vec![
                        Param {
                            name: "file".into(),
                            type_: Type::Handle(Handle {
                                resource: "file".into(),
                                ownership: HandleOwnership::Borrow,
                            }),
                        },
                        Param {
                            name: "name".into(),
                            type_: Type::String(StringEncoding::Utf8),
                        },
                    ],
                    result: Some(Type::Result(ResultType {
                        ok: Some(Box::new(Type::List(Box::new(Type::U8)))),
                        error: Some(Box::new(Type::U32)),
                    })),
                    async_: false,
                },
            }],
            ..World::default()
        };
        let plan =
            function_plan(&world, &world.imports[0], BoundaryDirection::GuestToHost).unwrap();
        assert_eq!(plan.params[0].ownership, TransferOwnership::Borrow);
        assert!(plan.requirements.contains(&PlanRequirement::BorrowScope));
        assert!(plan.requirements.contains(&PlanRequirement::Realloc));
        assert!(plan.requirements.contains(&PlanRequirement::PostReturn));
        let reverse =
            function_plan(&world, &world.imports[0], BoundaryDirection::HostToGuest).unwrap();
        assert_eq!(reverse.direction, BoundaryDirection::HostToGuest);
        assert_eq!(reverse.params[0].ownership, TransferOwnership::Borrow);
    }

    #[test]
    fn handle_lanes_cannot_drift_from_host_abi_blueprint() {
        let world = World {
            name: "lanes".into(),
            resources: vec![fe_host_abi::Resource {
                name: "resource".into(),
                methods: vec![],
            }],
            imports: vec![Function {
                namespace: "fe:host".into(),
                name: "consume".into(),
                signature: FunctionType {
                    params: vec![Param {
                        name: "value".into(),
                        type_: Type::Handle(Handle {
                            resource: "resource".into(),
                            ownership: HandleOwnership::Own,
                        }),
                    }],
                    result: Some(Type::Future(None)),
                    async_: false,
                },
            }],
            ..World::default()
        };
        let codec =
            function_plan(&world, &world.imports[0], BoundaryDirection::GuestToHost).unwrap();
        let blueprint = world
            .lowering_plan(
                "fe:host",
                "consume",
                fe_host_abi::LoweringProfile::CanonicalV1Blueprint,
            )
            .unwrap();
        assert_eq!(
            codec.params[0].layout.flat,
            Flattening::Direct(vec![CoreType::I32])
        );
        assert_eq!(
            blueprint.params[0].mode,
            fe_host_abi::PassMode::Direct(vec![fe_host_abi::CoreType::I32])
        );
        assert_eq!(
            codec.result.unwrap().layout.flat,
            Flattening::Direct(vec![CoreType::I32])
        );
        assert_eq!(
            blueprint.result,
            Some(fe_host_abi::PassMode::Direct(vec![
                fe_host_abi::CoreType::I32
            ]))
        );
    }

    #[test]
    fn async_and_stream_mechanics_fail_closed_but_future_handles_layout() {
        let world = World {
            name: "asyncs".into(),
            ..World::default()
        };
        assert_eq!(
            layout(&world, &Type::Future(Some(Box::new(Type::U32))))
                .unwrap()
                .shape,
            LayoutShape::FutureHandle
        );
        assert!(matches!(
            layout(&world, &Type::Stream(Some(Box::new(Type::U32)))),
            Err(CodecError::Unsupported(_))
        ));
        let function = Function {
            namespace: "fe:host".into(),
            name: "later".into(),
            signature: FunctionType {
                params: vec![],
                result: Some(Type::U32),
                async_: true,
            },
        };
        assert!(matches!(
            function_plan(&world, &function, BoundaryDirection::GuestToHost),
            Err(CodecError::Unsupported(_))
        ));
    }
}
