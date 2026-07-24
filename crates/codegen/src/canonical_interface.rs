//! Pure metadata and wasm32 layout model for Fe browser interfaces.
//!
//! This module derives declarations from semantic Fe record types and computes
//! their deterministic wasm32 layout. It deliberately does not emit Wasm
//! memory or allocate storage.

use std::{collections::BTreeSet, fmt};

use hir::{
    analysis::{
        HirAnalysisDb,
        ty::{
            adt_def::AdtRef,
            const_ty::{ConstTyData, EvaluatedConstTy},
            corelib::{lib_trait_matches, resolve_lib_type_path},
            ty_def::{PrimTy, TyBase, TyData, TyId},
        },
    },
    hir_def::{EnumVariant, FieldParent, TopLevelMod, VariantKind},
    semantic::EffectRequirementKey,
};
use serde::{Deserialize, Serialize};
use wasmparser::{CompositeInnerType, ExternalKind, Payload, TypeRef, ValType};

pub const CANONICAL_INTERFACE_PROTOCOL: &str = "fe-canonical-browser-interface";
pub const CANONICAL_INTERFACE_VERSION: u32 = 4;

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalType {
    Bool,
    U8,
    I32,
    U32,
    I64,
    U64,
    F32,
    Bytes,
    String,
    List {
        element: CanonicalListElement,
        max: u32,
    },
    Record(Vec<CanonicalField>),
    Variant(Vec<CanonicalVariant>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalListElement {
    U32,
    F32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalField {
    pub name: String,
    pub ty: CanonicalType,
}

impl CanonicalField {
    pub fn new(name: impl Into<String>, ty: CanonicalType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalVariant {
    pub name: String,
    pub fields: Vec<CanonicalField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalLaneDecl {
    pub name: String,
    pub export: Option<String>,
    pub request: CanonicalType,
    pub response: CanonicalType,
    pub intent: CanonicalLaneIntent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLaneIntent {
    pub execution: CanonicalExecution,
    pub placement: CanonicalPlacement,
    pub capabilities: Vec<CanonicalCapabilityRequirement>,
}

impl Default for CanonicalLaneIntent {
    fn default() -> Self {
        Self {
            execution: CanonicalExecution::Wasm,
            placement: CanonicalPlacement::Any,
            capabilities: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalExecution {
    Wasm,
    HostEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalPlacement {
    Any,
    MainThread,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCapabilityRequirement {
    pub capability: CanonicalCapability,
    pub mutable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalCapability {
    WebgpuDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalInterfaceManifest {
    pub protocol: String,
    pub version: u32,
    pub abi: CanonicalAbi,
    pub lanes: Vec<CanonicalLane>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalAbi {
    pub pointer_width: u8,
    pub endianness: CanonicalEndianness,
    pub memory_export: String,
    pub alloc_export: String,
    pub reset_export: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalEndianness {
    Little,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLane {
    pub name: String,
    pub export: Option<String>,
    pub request: CanonicalLayout,
    pub response: CanonicalLayout,
    pub intent: CanonicalLaneIntent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLayout {
    pub size: u32,
    pub align: u32,
    #[serde(flatten)]
    pub shape: CanonicalShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CanonicalShape {
    Bool,
    U8,
    I32,
    U32,
    I64,
    U64,
    F32,
    Bytes {
        pointer_offset: u32,
        length_offset: u32,
    },
    String {
        pointer_offset: u32,
        length_offset: u32,
        encoding: String,
    },
    List {
        element: CanonicalListElement,
        max: u32,
        stride: u32,
        pointer_offset: u32,
        length_offset: u32,
    },
    Record {
        fields: Vec<CanonicalFieldLayout>,
    },
    Variant {
        tag_offset: u32,
        variants: Vec<CanonicalVariantLayout>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalFieldLayout {
    pub name: String,
    pub offset: u32,
    pub layout: CanonicalLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalVariantLayout {
    pub name: String,
    pub tag: u32,
    pub fields: Vec<CanonicalFieldLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalInterfaceError(String);

impl fmt::Display for CanonicalInterfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CanonicalInterfaceError {}

impl CanonicalInterfaceManifest {
    pub fn build(declarations: Vec<CanonicalLaneDecl>) -> Result<Self, CanonicalInterfaceError> {
        if declarations.is_empty() {
            return Err(error("canonical interface requires at least one lane"));
        }
        let reserved = ["memory", "fe_cabi_alloc", "fe_cabi_reset"];
        let mut lane_names = BTreeSet::new();
        let mut exports = BTreeSet::new();
        let mut lanes = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            validate_name(&declaration.name, "lane")?;
            if declaration.intent.execution == CanonicalExecution::Wasm
                && declaration.export.is_none()
            {
                return Err(error(format!(
                    "canonical Wasm lane `{}` requires an export",
                    declaration.name
                )));
            }
            if declaration.intent.execution == CanonicalExecution::Wasm
                && (contains_variant(&declaration.request)
                    || contains_variant(&declaration.response))
            {
                return Err(error(format!(
                    "canonical Wasm lane `{}` cannot use variants until enum runtime classes are lowered by the wasm32 backend",
                    declaration.name
                )));
            }
            if declaration.intent.execution == CanonicalExecution::HostEffect
                && declaration.export.is_some()
            {
                return Err(error(format!(
                    "canonical host-effect lane `{}` must not declare a Wasm export",
                    declaration.name
                )));
            }
            if let Some(export) = &declaration.export {
                validate_export_name(export)?;
            }
            if !lane_names.insert(declaration.name.clone()) {
                return Err(error(format!(
                    "duplicate canonical lane `{}`",
                    declaration.name
                )));
            }
            if declaration
                .export
                .as_deref()
                .is_some_and(|export| reserved.contains(&export))
            {
                return Err(error(format!(
                    "canonical lane export `{}` collides with a reserved ABI export",
                    declaration.export.as_deref().unwrap()
                )));
            }
            if declaration
                .export
                .as_ref()
                .is_some_and(|export| !exports.insert(export.clone()))
            {
                return Err(error(format!(
                    "duplicate canonical lane export `{}`",
                    declaration.export.as_deref().unwrap()
                )));
            }
            let mut nodes = 0;
            let request = layout_type(&declaration.request, 0, &mut nodes, "request")?;
            let response = layout_type(&declaration.response, 0, &mut nodes, "response")?;
            lanes.push(CanonicalLane {
                name: declaration.name,
                export: declaration.export,
                request,
                response,
                intent: declaration.intent,
            });
        }
        Ok(Self {
            protocol: CANONICAL_INTERFACE_PROTOCOL.to_owned(),
            version: CANONICAL_INTERFACE_VERSION,
            abi: CanonicalAbi {
                pointer_width: 32,
                endianness: CanonicalEndianness::Little,
                memory_export: "memory".to_owned(),
                alloc_export: "fe_cabi_alloc".to_owned(),
                reset_export: "fe_cabi_reset".to_owned(),
            },
            lanes,
        })
    }
}

/// Derive one canonical lane from the exact selected public Fe entry.
///
/// This semantic operation does not claim that the current Wasm lowering has
/// emitted the canonical pointer ABI. Bundle integration must additionally
/// verify `(i32) -> i32`, memory, allocator, and reset exports before embedding
/// the resulting declaration.
pub fn canonical_lane_decl_from_entry<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
    entry_name: &str,
    lane_name: &str,
) -> Result<CanonicalLaneDecl, CanonicalInterfaceError> {
    let mut matches = top_mod
        .all_funcs(db)
        .iter()
        .copied()
        .filter(|func| func.top_mod(db) == top_mod)
        .filter(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == entry_name)
        });
    let func = matches.next().ok_or_else(|| {
        error(format!(
            "canonical entry `{entry_name}` was not found in the selected module"
        ))
    })?;
    if matches.next().is_some() {
        return Err(error(format!(
            "canonical entry `{entry_name}` is ambiguous in the selected module"
        )));
    }
    if !func.vis(db).is_pub() || func.is_extern(db) || func.is_associated_func(db) {
        return Err(error(format!(
            "canonical entry `{entry_name}` must be a public non-associated Fe function"
        )));
    }
    let args = func.arg_tys(db);
    let [request] = args.as_slice() else {
        return Err(error(format!(
            "canonical entry `{entry_name}` must take exactly one semantic request record; found {} parameters",
            args.len()
        )));
    };
    let request = canonical_type_from_semantic(db, *request.skip_binder(), "request")?;
    let response = canonical_type_from_semantic(db, func.return_ty(db), "response")?;
    let is_message = |ty: &CanonicalType| {
        matches!(
            ty,
            CanonicalType::Record(_)
                | CanonicalType::Variant(_)
                | CanonicalType::Bytes
                | CanonicalType::String
                | CanonicalType::List { .. }
        )
    };
    if !is_message(&request) || !is_message(&response) {
        return Err(error(format!(
            "canonical entry `{entry_name}` request and response must both be nominal browser message types"
        )));
    }
    let intent = canonical_lane_intent(db, func)?;
    Ok(CanonicalLaneDecl {
        name: lane_name.to_owned(),
        // The source entry is not itself a canonical adapter, even when its
        // lowered aggregate ABI happens to have the same raw Wasm signature.
        // Reserve a distinct export so verification cannot bless that
        // accidental shape without the generated marshal/unmarshal wrapper.
        export: (intent.execution == CanonicalExecution::Wasm)
            .then(|| format!("fe_cabi_{entry_name}")),
        request,
        response,
        intent,
    })
}

fn canonical_lane_intent<'db>(
    db: &'db dyn HirAnalysisDb,
    func: hir::hir_def::Func<'db>,
) -> Result<CanonicalLaneIntent, CanonicalInterfaceError> {
    let scope = func.scope();
    let webgpu_backend = resolve_lib_type_path(db, scope, "std::webgpu::WebGpuBackend");
    let mut execution = CanonicalExecution::Wasm;
    let mut placement = CanonicalPlacement::Any;
    let mut capabilities = Vec::new();
    for requirement in func.effect_requirements(db) {
        let EffectRequirementKey::Trait(trait_inst) = &requirement.key else {
            return Err(error("canonical lane has unsupported non-trait effect"));
        };
        let trait_ = trait_inst.def(db);
        if lib_trait_matches(db, trait_, "core::browser::HostEffect") {
            if execution == CanonicalExecution::HostEffect {
                return Err(error("duplicate canonical HostEffect marker"));
            }
            execution = CanonicalExecution::HostEffect;
            continue;
        }
        let marker_placement = if lib_trait_matches(db, trait_, "core::browser::MainThread") {
            Some(CanonicalPlacement::MainThread)
        } else if lib_trait_matches(db, trait_, "core::browser::Worker") {
            Some(CanonicalPlacement::Worker)
        } else {
            None
        };
        if let Some(next) = marker_placement {
            if placement != CanonicalPlacement::Any {
                return Err(error("canonical lane has conflicting placement markers"));
            }
            placement = next;
            continue;
        }
        if lib_trait_matches(db, trait_, "std::webgpu::Dispatch")
            && trait_inst.args(db).len() == 2
            && webgpu_backend.is_some_and(|backend| trait_inst.args(db)[1] == backend)
            && trait_inst.assoc_type_bindings(db).is_empty()
        {
            capabilities.push(CanonicalCapabilityRequirement {
                capability: CanonicalCapability::WebgpuDispatch,
                mutable: requirement.is_mut,
            });
            continue;
        }
        return Err(error("canonical lane has unsupported capability effect"));
    }
    if execution == CanonicalExecution::HostEffect && placement == CanonicalPlacement::Any {
        return Err(error("canonical host-effect lane requires explicit placement"));
    }
    if execution == CanonicalExecution::Wasm && !capabilities.is_empty() {
        return Err(error(
            "canonical Wasm lane cannot externalize host capabilities without HostEffect",
        ));
    }
    capabilities.sort_by_key(|requirement| match requirement.capability {
        CanonicalCapability::WebgpuDispatch => 0,
    });
    capabilities.dedup();
    Ok(CanonicalLaneIntent {
        execution,
        placement,
        capabilities,
    })
}

/// Map a closed semantic Fe type to milestone-1 canonical metadata.
///
/// Bytes, strings, and lists use exact compiler-owned `core::browser`
/// descriptor identities.
/// Primitive `String` and name-based or structural ADT guesses remain rejected.
pub fn canonical_type_from_semantic<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    path: &str,
) -> Result<CanonicalType, CanonicalInterfaceError> {
    // Ordinary by-value record parameters are represented semantically as
    // read-only views. The canonical declaration describes the underlying
    // message record, not Fe's local access capability.
    let ty = ty.as_view(db).unwrap_or(ty);
    if let TyData::TyBase(TyBase::Prim(primitive)) = ty.base_ty(db).data(db) {
        return match primitive {
            PrimTy::Bool => Ok(CanonicalType::Bool),
            PrimTy::U8 => Ok(CanonicalType::U8),
            PrimTy::I32 => Ok(CanonicalType::I32),
            PrimTy::U32 => Ok(CanonicalType::U32),
            PrimTy::I64 => Ok(CanonicalType::I64),
            PrimTy::U64 => Ok(CanonicalType::U64),
            PrimTy::F32 => Ok(CanonicalType::F32),
            PrimTy::String => Err(error(format!(
                "{path}: canonical strings require an explicit nominal BrowserString mapping, which is deferred"
            ))),
            other => Err(error(format!(
                "{path}: unsupported canonical primitive `{other:?}`"
            ))),
        };
    }
    let Some(adt) = ty.adt_def(db) else {
        return Err(error(format!(
            "{path}: unsupported or unresolved canonical semantic type"
        )));
    };
    let AdtRef::Struct(struct_) = adt.adt_ref(db) else {
        let AdtRef::Enum(enum_) = adt.adt_ref(db) else {
            return Err(error(format!("{path}: unsupported canonical ADT")));
        };
        let args = ty.generic_args(db);
        let mut variants = Vec::new();
        for (tag, variant) in enum_.variants(db).enumerate() {
            let name = variant
                .name(db)
                .map(|name| canonical_variant_name(name.data(db)))
                .ok_or_else(|| error(format!("{path}: unnamed enum variant is unsupported")))?;
            validate_name(&name, "variant")?;
            let field_tys = variant
                .field_tys(db)
                .into_iter()
                .map(|field| field.instantiate(db, args))
                .collect::<Vec<_>>();
            let fields = match variant.kind(db) {
                VariantKind::Unit => Vec::new(),
                VariantKind::Tuple(_) => {
                    return Err(error(format!(
                        "{path}.{name}: tuple variants are not canonical; use a record variant with named fields"
                    )));
                }
                VariantKind::Record(_) => {
                    let field_views = FieldParent::Variant(EnumVariant::new(enum_, variant.idx))
                        .fields(db)
                        .collect::<Vec<_>>();
                    if field_views.len() != field_tys.len() {
                        return Err(error(format!(
                            "{path}.{name}: semantic variant field metadata is inconsistent"
                        )));
                    }
                    field_views
                        .into_iter()
                        .zip(field_tys)
                        .map(|(field, field_ty)| {
                            let field_name = field
                                .name(db)
                                .map(|name| name.data(db).to_string())
                                .ok_or_else(|| {
                                error(format!(
                                    "{path}.{name}: unnamed variant field is unsupported"
                                ))
                            })?;
                            Ok(CanonicalField::new(
                                field_name.clone(),
                                canonical_type_from_semantic(
                                    db,
                                    field_ty,
                                    &format!("{path}.{name}.{field_name}"),
                                )?,
                            ))
                        })
                        .collect::<Result<Vec<_>, CanonicalInterfaceError>>()?
                }
            };
            let _ = tag;
            variants.push(CanonicalVariant { name, fields });
        }
        return Ok(CanonicalType::Variant(variants));
    };
    // Descriptor semantics are attached to the exact compiler-owned core ADTs,
    // never to a source name or a structurally similar user record.
    let descriptor = [
        ("core::browser::BrowserBytes", CanonicalType::Bytes),
        ("core::browser::AllocatedBrowserBytes", CanonicalType::Bytes),
        ("core::browser::BrowserString", CanonicalType::String),
    ]
    .into_iter()
    .find_map(|(lib_path, canonical)| {
        let resolved = resolve_lib_type_path(db, struct_.scope(), lib_path)?;
        (resolved.adt_def(db) == Some(adt)).then_some(canonical)
    });
    if let Some(descriptor) = descriptor {
        return Ok(descriptor);
    }
    if resolve_lib_type_path(db, struct_.scope(), "core::browser::BrowserList")
        .is_some_and(|resolved| resolved.adt_def(db) == Some(adt))
    {
        let [element, max] = ty.generic_args(db) else {
            return Err(error(format!(
                "{path}: BrowserList requires exactly one element type and one const maximum"
            )));
        };
        let element = match element.base_ty(db).data(db) {
            TyData::TyBase(TyBase::Prim(PrimTy::U32)) => CanonicalListElement::U32,
            TyData::TyBase(TyBase::Prim(PrimTy::F32)) => CanonicalListElement::F32,
            _ => {
                return Err(error(format!(
                    "{path}: BrowserList element must be exactly `u32` or `f32`"
                )));
            }
        };
        let TyData::ConstTy(max) = max.data(db) else {
            return Err(error(format!(
                "{path}: BrowserList maximum must be a concrete `usize` const"
            )));
        };
        let evaluated = max.evaluate(db, None);
        let ConstTyData::Evaluated(EvaluatedConstTy::LitInt(max), max_ty) = evaluated.data(db)
        else {
            return Err(error(format!(
                "{path}: BrowserList maximum must evaluate to a concrete integer"
            )));
        };
        if !matches!(
            max_ty.base_ty(db).data(db),
            TyData::TyBase(TyBase::Prim(PrimTy::Usize))
        ) {
            return Err(error(format!(
                "{path}: BrowserList maximum must have type `usize`"
            )));
        }
        let max = u32::try_from(max.data(db)).map_err(|_| {
            error(format!(
                "{path}: BrowserList maximum does not fit the wasm32 canonical ABI"
            ))
        })?;
        if max > u32::MAX / 4 {
            return Err(error(format!(
                "{path}: BrowserList maximum {max} exceeds the safe four-byte element bound {}",
                u32::MAX / 4
            )));
        }
        return Ok(CanonicalType::List { element, max });
    }
    let field_types = ty.field_types(db);
    let field_views = FieldParent::Struct(struct_).fields(db).collect::<Vec<_>>();
    if field_views.len() != field_types.len() {
        return Err(error(format!(
            "{path}: semantic record field metadata is inconsistent"
        )));
    }
    let mut fields = Vec::with_capacity(field_views.len());
    for (field, field_ty) in field_views.into_iter().zip(field_types) {
        let name = field
            .name(db)
            .map(|name| name.data(db).to_string())
            .ok_or_else(|| error(format!("{path}: tuple-like record fields are unsupported")))?;
        let field_path = format!("{path}.{name}");
        fields.push(CanonicalField::new(
            name,
            canonical_type_from_semantic(db, field_ty, &field_path)?,
        ));
    }
    Ok(CanonicalType::Record(fields))
}

/// Verify the complete milestone-1 canonical ABI against emitted Wasm.
pub fn verify_canonical_wasm_abi(
    wasm: &[u8],
    interface: &CanonicalInterfaceManifest,
) -> Result<(), CanonicalInterfaceError> {
    wasmparser::validate(wasm)
        .map_err(|error| self::error(format!("canonical ABI received invalid Wasm: {error}")))?;
    let mut types = Vec::<Option<(Vec<ValType>, Vec<ValType>)>>::new();
    let mut imported_functions = Vec::<u32>::new();
    let mut defined_functions = Vec::<u32>::new();
    let mut function_exports = std::collections::BTreeMap::<String, u32>::new();
    let mut memories = Vec::<wasmparser::MemoryType>::new();
    let mut memory_exports = std::collections::BTreeMap::<String, u32>::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.map_err(|error| self::error(error.to_string()))? {
            Payload::TypeSection(reader) => {
                for group in reader {
                    for subtype in group
                        .map_err(|error| self::error(error.to_string()))?
                        .into_types()
                    {
                        let signature = match &subtype.composite_type.inner {
                            CompositeInnerType::Func(function) => {
                                Some((function.params().to_vec(), function.results().to_vec()))
                            }
                            _ => None,
                        };
                        types.push(signature);
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    match import.map_err(|error| self::error(error.to_string()))?.ty {
                        TypeRef::Func(index) => imported_functions.push(index),
                        TypeRef::Memory(memory) => memories.push(memory),
                        _ => {}
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for index in reader {
                    defined_functions.push(index.map_err(|error| self::error(error.to_string()))?);
                }
            }
            Payload::MemorySection(reader) => {
                for memory in reader {
                    memories.push(memory.map_err(|error| self::error(error.to_string()))?);
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(|error| self::error(error.to_string()))?;
                    match export.kind {
                        ExternalKind::Func => {
                            function_exports.insert(export.name.to_owned(), export.index);
                        }
                        ExternalKind::Memory => {
                            memory_exports.insert(export.name.to_owned(), export.index);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let memory_index = *memory_exports
        .get(&interface.abi.memory_export)
        .ok_or_else(|| {
            error(format!(
                "canonical ABI is missing exported memory `{}`",
                interface.abi.memory_export
            ))
        })?;
    let memory = memories.get(memory_index as usize).ok_or_else(|| {
        error(format!(
            "canonical exported memory `{}` has missing memory type at index {memory_index}",
            interface.abi.memory_export
        ))
    })?;
    if memory.memory64 {
        return Err(error(format!(
            "canonical exported memory `{}` is memory64; expected wasm32 memory",
            interface.abi.memory_export
        )));
    }
    let signature = |name: &str| -> Result<&(Vec<ValType>, Vec<ValType>), CanonicalInterfaceError> {
        let function_index = *function_exports
            .get(name)
            .ok_or_else(|| error(format!("canonical ABI is missing function export `{name}`")))?;
        let type_index = if let Some(index) = imported_functions.get(function_index as usize) {
            *index
        } else {
            let defined = function_index as usize - imported_functions.len();
            *defined_functions.get(defined).ok_or_else(|| {
                error(format!(
                    "canonical export `{name}` has no function-section type"
                ))
            })?
        };
        types
            .get(type_index as usize)
            .ok_or_else(|| {
                error(format!(
                    "canonical export `{name}` has missing type {type_index}"
                ))
            })?
            .as_ref()
            .ok_or_else(|| {
                error(format!(
                    "canonical export `{name}` references non-function type {type_index}"
                ))
            })
    };
    let require = |name: &str, params: &[ValType], results: &[ValType]| {
        let actual = signature(name)?;
        if actual.0 != params || actual.1 != results {
            return Err(error(format!(
                "canonical export `{name}` has signature {:?} -> {:?}; expected {params:?} -> {results:?}",
                actual.0, actual.1
            )));
        }
        Ok(())
    };
    require(
        &interface.abi.alloc_export,
        &[ValType::I32, ValType::I32],
        &[ValType::I32],
    )?;
    require(&interface.abi.reset_export, &[], &[])?;
    for lane in &interface.lanes {
        if let Some(export) = &lane.export {
            require(export, &[ValType::I32], &[ValType::I32])?;
        }
    }
    Ok(())
}

fn layout_type(
    ty: &CanonicalType,
    depth: usize,
    nodes: &mut usize,
    path: &str,
) -> Result<CanonicalLayout, CanonicalInterfaceError> {
    if depth > MAX_DEPTH {
        return Err(error(format!(
            "{path} exceeds maximum nesting depth {MAX_DEPTH}"
        )));
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| error("canonical type node count overflow"))?;
    if *nodes > MAX_NODES {
        return Err(error(format!(
            "canonical lane exceeds maximum type node count {MAX_NODES}"
        )));
    }
    let scalar = |size, align, shape| CanonicalLayout { size, align, shape };
    Ok(match ty {
        CanonicalType::Bool => scalar(1, 1, CanonicalShape::Bool),
        CanonicalType::U8 => scalar(1, 1, CanonicalShape::U8),
        CanonicalType::I32 => scalar(4, 4, CanonicalShape::I32),
        CanonicalType::U32 => scalar(4, 4, CanonicalShape::U32),
        CanonicalType::I64 => scalar(8, 8, CanonicalShape::I64),
        CanonicalType::U64 => scalar(8, 8, CanonicalShape::U64),
        CanonicalType::F32 => scalar(4, 4, CanonicalShape::F32),
        CanonicalType::Bytes => scalar(
            8,
            4,
            CanonicalShape::Bytes {
                pointer_offset: 0,
                length_offset: 4,
            },
        ),
        CanonicalType::String => scalar(
            8,
            4,
            CanonicalShape::String {
                pointer_offset: 0,
                length_offset: 4,
                encoding: "utf-8".to_owned(),
            },
        ),
        CanonicalType::List { element, max } => {
            if *max > u32::MAX / 4 {
                return Err(error(format!(
                    "{path}: canonical list maximum exceeds wasm32 byte capacity"
                )));
            }
            scalar(
                8,
                4,
                CanonicalShape::List {
                    element: *element,
                    max: *max,
                    stride: 4,
                    pointer_offset: 0,
                    length_offset: 4,
                },
            )
        }
        CanonicalType::Record(fields) => {
            if fields.is_empty() {
                return Err(error(format!(
                    "{path} record must contain at least one field"
                )));
            }
            let mut names = BTreeSet::new();
            let mut offset = 0u32;
            let mut record_align = 1u32;
            let mut layouts = Vec::with_capacity(fields.len());
            for field in fields {
                validate_name(&field.name, "field")?;
                if !names.insert(field.name.clone()) {
                    return Err(error(format!(
                        "{path} has duplicate field `{}`",
                        field.name
                    )));
                }
                let field_path = format!("{path}.{}", field.name);
                let layout = layout_type(&field.ty, depth + 1, nodes, &field_path)?;
                offset = align_up(offset, layout.align, &field_path)?;
                let field_offset = offset;
                offset = offset.checked_add(layout.size).ok_or_else(|| {
                    error(format!(
                        "{field_path} makes canonical record size overflow u32"
                    ))
                })?;
                record_align = record_align.max(layout.align);
                layouts.push(CanonicalFieldLayout {
                    name: field.name.clone(),
                    offset: field_offset,
                    layout,
                });
            }
            CanonicalLayout {
                size: align_up(offset, record_align, path)?,
                align: record_align,
                shape: CanonicalShape::Record { fields: layouts },
            }
        }
        CanonicalType::Variant(variants) => {
            if variants.is_empty() {
                return Err(error(format!("{path} variant must have at least one case")));
            }
            let mut names = BTreeSet::new();
            let mut variant_layouts = Vec::with_capacity(variants.len());
            let mut overall_align = 4u32;
            let mut overall_size = 4u32;
            for (tag, variant) in variants.iter().enumerate() {
                *nodes = nodes
                    .checked_add(1)
                    .ok_or_else(|| error("canonical type node count overflow"))?;
                if *nodes > MAX_NODES {
                    return Err(error(format!(
                        "{path} exceeds maximum type node count {MAX_NODES}"
                    )));
                }
                validate_name(&variant.name, "variant")?;
                if !names.insert(variant.name.clone()) {
                    return Err(error(format!(
                        "{path} has duplicate variant `{}`",
                        variant.name
                    )));
                }
                let mut field_names = BTreeSet::new();
                let mut offset = 4u32;
                let mut fields = Vec::with_capacity(variant.fields.len());
                for field in &variant.fields {
                    validate_name(&field.name, "field")?;
                    if field.name == "tag" {
                        return Err(error(format!(
                            "{path}.{} reserves field name `tag`",
                            variant.name
                        )));
                    }
                    if !field_names.insert(field.name.clone()) {
                        return Err(error(format!(
                            "{path}.{} has duplicate field `{}`",
                            variant.name, field.name
                        )));
                    }
                    let field_path = format!("{path}.{}.{}", variant.name, field.name);
                    let layout = layout_type(&field.ty, depth + 1, nodes, &field_path)?;
                    offset = align_up(offset, layout.align, &field_path)?;
                    let field_offset = offset;
                    offset = offset.checked_add(layout.size).ok_or_else(|| {
                        error(format!(
                            "{field_path} makes canonical variant size overflow u32"
                        ))
                    })?;
                    overall_align = overall_align.max(layout.align);
                    fields.push(CanonicalFieldLayout {
                        name: field.name.clone(),
                        offset: field_offset,
                        layout,
                    });
                }
                overall_size = overall_size.max(offset);
                variant_layouts.push(CanonicalVariantLayout {
                    name: variant.name.clone(),
                    tag: u32::try_from(tag)
                        .map_err(|_| error(format!("{path} has too many variants")))?,
                    fields,
                });
            }
            CanonicalLayout {
                size: align_up(overall_size, overall_align, path)?,
                align: overall_align,
                shape: CanonicalShape::Variant {
                    tag_offset: 0,
                    variants: variant_layouts,
                },
            }
        }
    })
}

fn canonical_variant_name(name: &str) -> String {
    let mut output = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn contains_variant(ty: &CanonicalType) -> bool {
    match ty {
        CanonicalType::Variant(_) => true,
        CanonicalType::Record(fields) => fields.iter().any(|field| contains_variant(&field.ty)),
        CanonicalType::Bool
        | CanonicalType::U8
        | CanonicalType::I32
        | CanonicalType::U32
        | CanonicalType::I64
        | CanonicalType::U64
        | CanonicalType::F32
        | CanonicalType::Bytes
        | CanonicalType::String
        | CanonicalType::List { .. } => false,
    }
}

fn align_up(value: u32, align: u32, path: &str) -> Result<u32, CanonicalInterfaceError> {
    debug_assert!(align.is_power_of_two());
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
        .ok_or_else(|| error(format!("{path} alignment overflows u32")))
}

fn validate_name(name: &str, kind: &str) -> Result<(), CanonicalInterfaceError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.is_ascii()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || (index > 0 && (byte == b'_' || byte.is_ascii_digit()))
        });
    if !valid {
        return Err(error(format!(
            "invalid canonical {kind} name `{name}`; expected lowercase ASCII identifier"
        )));
    }
    Ok(())
}

fn validate_export_name(name: &str) -> Result<(), CanonicalInterfaceError> {
    if name.is_empty()
        || name.len() > 128
        || !name.is_ascii()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(error(format!(
            "invalid canonical Wasm export name `{name}`"
        )));
    }
    Ok(())
}

fn error(message: impl Into<String>) -> CanonicalInterfaceError {
    CanonicalInterfaceError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::InputDb;
    use driver::DriverDataBase;
    use url::Url;

    fn push_name(out: &mut Vec<u8>, name: &str) {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
    }

    fn section(module: &mut Vec<u8>, id: u8, payload: Vec<u8>) {
        module.push(id);
        module.push(payload.len() as u8);
        module.extend(payload);
    }

    fn canonical_wasm(lane_result: u8, include_memory: bool, memory64: bool) -> Vec<u8> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        // lane (i32)->result, alloc (i32,i32)->i32, reset ()->()
        section(
            &mut module,
            1,
            vec![
                3,
                0x60,
                1,
                0x7f,
                1,
                lane_result,
                0x60,
                2,
                0x7f,
                0x7f,
                1,
                0x7f,
                0x60,
                0,
                0,
            ],
        );
        section(&mut module, 3, vec![3, 0, 1, 2]);
        section(&mut module, 5, vec![1, if memory64 { 0x04 } else { 0 }, 1]);
        let mut exports = vec![if include_memory { 4 } else { 3 }];
        for (name, kind, index) in [
            ("update", 0, 0),
            ("fe_cabi_alloc", 0, 1),
            ("fe_cabi_reset", 0, 2),
        ] {
            push_name(&mut exports, name);
            exports.extend([kind, index]);
        }
        if include_memory {
            push_name(&mut exports, "memory");
            exports.extend([2, 0]);
        }
        section(&mut module, 7, exports);
        let lane_body = if lane_result == 0x7f {
            vec![0, 0x20, 0, 0x0b] // local.get 0
        } else {
            vec![0, 0x42, 0, 0x0b] // i64.const 0
        };
        let mut code = vec![3, lane_body.len() as u8];
        code.extend(lane_body);
        code.extend([4, 0, 0x41, 0, 0x0b]); // alloc returns zero
        code.extend([2, 0, 0x0b]); // reset
        section(&mut module, 10, code);
        module
    }

    fn one_lane_manifest() -> CanonicalInterfaceManifest {
        CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
            name: "update".to_owned(),
            export: Some("update".to_owned()),
            request: CanonicalType::Record(vec![CanonicalField::new("value", CanonicalType::U32)]),
            response: CanonicalType::Record(vec![CanonicalField::new("value", CanonicalType::U32)]),
            intent: CanonicalLaneIntent::default(),
        }])
        .unwrap()
    }

    fn record(fields: Vec<CanonicalField>) -> CanonicalType {
        CanonicalType::Record(fields)
    }

    #[test]
    fn computes_deterministic_nested_wasm32_layout_and_roundtrips() {
        let request = record(vec![
            CanonicalField::new("tag", CanonicalType::U8),
            CanonicalField::new("sequence", CanonicalType::U64),
            CanonicalField::new(
                "message",
                record(vec![
                    CanonicalField::new("text", CanonicalType::String),
                    CanonicalField::new("payload", CanonicalType::Bytes),
                ]),
            ),
            CanonicalField::new("enabled", CanonicalType::Bool),
        ]);
        let manifest = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
            name: "render".to_owned(),
            export: Some("render_message".to_owned()),
            request,
            response: CanonicalType::U32,
            intent: CanonicalLaneIntent::default(),
        }])
        .unwrap();
        let lane = &manifest.lanes[0];
        assert_eq!((lane.request.size, lane.request.align), (40, 8));
        let CanonicalShape::Record { fields } = &lane.request.shape else {
            panic!("request must be a record")
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| (field.name.as_str(), field.offset))
                .collect::<Vec<_>>(),
            [
                ("tag", 0),
                ("sequence", 8),
                ("message", 16),
                ("enabled", 32)
            ]
        );
        assert_eq!((fields[2].layout.size, fields[2].layout.align), (16, 4));
        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: CanonicalInterfaceManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
        assert_eq!(decoded.protocol, CANONICAL_INTERFACE_PROTOCOL);
        assert_eq!(decoded.version, CANONICAL_INTERFACE_VERSION);
    }

    #[test]
    fn rejects_names_collisions_empty_records_and_excessive_depth() {
        let lane = |name: &str, export: &str, request| CanonicalLaneDecl {
            name: name.to_owned(),
            export: Some(export.to_owned()),
            request,
            response: CanonicalType::U32,
            intent: CanonicalLaneIntent::default(),
        };
        assert!(
            CanonicalInterfaceManifest::build(vec![
                lane("render", "a", CanonicalType::U32),
                lane("render", "b", CanonicalType::U32),
            ])
            .unwrap_err()
            .to_string()
            .contains("duplicate canonical lane")
        );
        assert!(
            CanonicalInterfaceManifest::build(vec![
                lane("a", "same", CanonicalType::U32),
                lane("b", "same", CanonicalType::U32),
            ])
            .unwrap_err()
            .to_string()
            .contains("duplicate canonical lane export")
        );
        assert!(
            CanonicalInterfaceManifest::build(vec![lane("render", "memory", CanonicalType::U32),])
                .unwrap_err()
                .to_string()
                .contains("reserved ABI export")
        );
        assert!(
            CanonicalInterfaceManifest::build(vec![lane("Bad", "ok", CanonicalType::U32),])
                .is_err()
        );
        assert!(
            CanonicalInterfaceManifest::build(vec![lane("_hidden", "ok", CanonicalType::U32),])
                .is_err()
        );
        assert!(
            CanonicalInterfaceManifest::build(vec![lane("render", "ok", record(vec![])),])
                .unwrap_err()
                .to_string()
                .contains("at least one field")
        );
        assert!(
            CanonicalInterfaceManifest::build(vec![lane(
                "render",
                "ok",
                record(vec![
                    CanonicalField::new("x", CanonicalType::U32),
                    CanonicalField::new("x", CanonicalType::U32),
                ])
            ),])
            .unwrap_err()
            .to_string()
            .contains("duplicate field")
        );

        let mut nested = CanonicalType::U8;
        for _ in 0..=MAX_DEPTH {
            nested = record(vec![CanonicalField::new("next", nested)]);
        }
        assert!(
            CanonicalInterfaceManifest::build(vec![lane("deep", "deep", nested)])
                .unwrap_err()
                .to_string()
                .contains("nesting depth")
        );
        assert!(align_up(u32::MAX, 8, "overflow_probe").is_err());
    }

    #[test]
    fn primitive_and_descriptor_layouts_are_pinned() {
        let cases = [
            (CanonicalType::Bool, 1, 1),
            (CanonicalType::U8, 1, 1),
            (CanonicalType::I32, 4, 4),
            (CanonicalType::U32, 4, 4),
            (CanonicalType::I64, 8, 8),
            (CanonicalType::U64, 8, 8),
            (CanonicalType::F32, 4, 4),
            (CanonicalType::Bytes, 8, 4),
            (CanonicalType::String, 8, 4),
        ];
        for (index, (ty, size, align)) in cases.into_iter().enumerate() {
            let manifest = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
                name: format!("lane_{index}"),
                export: Some(format!("export_{index}")),
                request: ty,
                response: CanonicalType::U8,
                intent: CanonicalLaneIntent::default(),
            }])
            .unwrap();
            assert_eq!(
                (
                    manifest.lanes[0].request.size,
                    manifest.lanes[0].request.align
                ),
                (size, align)
            );
        }
    }

    #[test]
    fn bounded_list_layout_is_pinned_and_checks_wasm32_capacity() {
        for (element, max) in [
            (CanonicalListElement::U32, 0),
            (CanonicalListElement::F32, 17),
            (CanonicalListElement::U32, u32::MAX / 4),
        ] {
            let manifest = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
                name: "list".to_owned(),
                export: Some("list".to_owned()),
                request: CanonicalType::List { element, max },
                response: CanonicalType::U8,
                intent: CanonicalLaneIntent::default(),
            }])
            .unwrap();
            assert_eq!(
                manifest.lanes[0].request,
                CanonicalLayout {
                    size: 8,
                    align: 4,
                    shape: CanonicalShape::List {
                        element,
                        max,
                        stride: 4,
                        pointer_offset: 0,
                        length_offset: 4,
                    },
                }
            );
        }
        let error = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
            name: "list".to_owned(),
            export: Some("list".to_owned()),
            request: CanonicalType::List {
                element: CanonicalListElement::U32,
                max: u32::MAX / 4 + 1,
            },
            response: CanonicalType::U8,
            intent: CanonicalLaneIntent::default(),
        }])
        .unwrap_err()
        .to_string();
        assert!(error.contains("byte capacity"), "{error}");
    }

    #[test]
    fn semantic_browser_list_requires_exact_nominal_supported_instantiation() {
        let declaration = semantic_lane(
            r#"
use core::BrowserList
const MAX: usize = 8
struct Request { indices: BrowserList<u32, MAX>, weights: BrowserList<f32, 0> }
struct Response { accepted: bool }
pub fn update(request: Request) -> Response {
    Response { accepted: request.indices.len == request.weights.len }
}
"#,
            "update",
        )
        .unwrap();
        let CanonicalType::Record(fields) = declaration.request else {
            panic!("request record")
        };
        assert_eq!(
            fields[0].ty,
            CanonicalType::List {
                element: CanonicalListElement::U32,
                max: 8,
            }
        );
        assert_eq!(
            fields[1].ty,
            CanonicalType::List {
                element: CanonicalListElement::F32,
                max: 0,
            }
        );

        let lookalike = semantic_lane(
            r#"
struct BrowserList<T, const MAX: usize> { ptr: u32, len: u32 }
struct Request { values: BrowserList<u32, 4> }
struct Response { accepted: bool }
pub fn update(request: Request) -> Response { Response { accepted: true } }
"#,
            "update",
        )
        .unwrap();
        let CanonicalType::Record(fields) = lookalike.request else {
            panic!("request record")
        };
        assert!(matches!(fields[0].ty, CanonicalType::Record(_)));

        let unsupported = semantic_lane(
            r#"
use core::BrowserList
struct Request { values: BrowserList<u8, 4> }
struct Response { accepted: bool }
pub fn update(request: Request) -> Response { Response { accepted: true } }
"#,
            "update",
        )
        .unwrap_err();
        assert!(unsupported.contains("exactly `u32` or `f32`"), "{unsupported}");
    }

    #[test]
    fn variants_have_a_pinned_tagged_union_layout_and_wasm_fails_closed() {
        let message = CanonicalType::Variant(vec![
            CanonicalVariant {
                name: "none".to_owned(),
                fields: vec![],
            },
            CanonicalVariant {
                name: "data".to_owned(),
                fields: vec![
                    CanonicalField::new("code", CanonicalType::U8),
                    CanonicalField::new("sequence", CanonicalType::U64),
                    CanonicalField::new("payload", CanonicalType::Bytes),
                ],
            },
        ]);
        let host = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
            name: "deliver".to_owned(),
            export: None,
            request: message.clone(),
            response: CanonicalType::Record(vec![CanonicalField::new(
                "accepted",
                CanonicalType::Bool,
            )]),
            intent: CanonicalLaneIntent {
                execution: CanonicalExecution::HostEffect,
                placement: CanonicalPlacement::Worker,
                capabilities: vec![],
            },
        }])
        .unwrap();
        assert_eq!(
            (host.lanes[0].request.size, host.lanes[0].request.align),
            (24, 8)
        );
        let CanonicalShape::Variant {
            tag_offset,
            variants,
        } = &host.lanes[0].request.shape
        else {
            panic!("tagged variant")
        };
        assert_eq!(*tag_offset, 0);
        assert_eq!(variants[0].tag, 0);
        assert_eq!(variants[1].tag, 1);
        assert_eq!(
            variants[1]
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.offset))
                .collect::<Vec<_>>(),
            [("code", 4), ("sequence", 8), ("payload", 16)]
        );

        let error = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
            name: "deliver".to_owned(),
            export: Some("fe_cabi_deliver".to_owned()),
            request: message,
            response: CanonicalType::U32,
            intent: CanonicalLaneIntent::default(),
        }])
        .unwrap_err()
        .to_string();
        assert!(error.contains("enum runtime classes"), "{error}");
    }

    #[test]
    fn semantic_record_variants_are_derived_but_tuple_variants_are_rejected() {
        let declaration = semantic_lane(
            r#"
enum Message {
    Empty,
    Data { code: u8, payload: u32 },
}
struct Response { accepted: bool }
pub fn update(request: Message) -> Response {
    Response { accepted: true }
}
"#,
            "update",
        )
        .unwrap();
        let CanonicalType::Variant(variants) = declaration.request else {
            panic!("semantic variant")
        };
        assert_eq!(variants[0].name, "empty");
        assert_eq!(variants[1].name, "data");
        assert_eq!(
            variants[1]
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["code", "payload"]
        );

        let error = semantic_lane(
            r#"
enum Message { Empty, Data(u32) }
struct Response { accepted: bool }
pub fn update(request: Message) -> Response {
    Response { accepted: true }
}
"#,
            "update",
        )
        .unwrap_err();
        assert!(
            error.contains("tuple variants are not canonical"),
            "{error}"
        );
    }

    fn semantic_lane(source: &str, entry: &str) -> Result<CanonicalLaneDecl, String> {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///canonical_semantic.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(source.to_owned()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        canonical_lane_decl_from_entry(&db, top_mod, entry, "update")
            .map_err(|error| error.to_string())
    }

    #[test]
    fn derives_nested_records_from_selected_entry_without_field_restatement() {
        let declaration = semantic_lane(
            r#"
struct Position { x: f32, y: f32 }
struct Request { tag: u8, sequence: u64, position: Position, enabled: bool }
struct Response { accepted: bool, code: i32 }
pub fn update(request: Request) -> Response {
    Response { accepted: request.enabled, code: 7 }
}
"#,
            "update",
        )
        .unwrap();
        let manifest = CanonicalInterfaceManifest::build(vec![declaration]).unwrap();
        let lane = &manifest.lanes[0];
        assert_eq!(lane.export.as_deref(), Some("fe_cabi_update"));
        assert_eq!((lane.request.size, lane.request.align), (32, 8));
        let CanonicalShape::Record { fields } = &lane.request.shape else {
            panic!("request record")
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| (field.name.as_str(), field.offset))
                .collect::<Vec<_>>(),
            [
                ("tag", 0),
                ("sequence", 8),
                ("position", 16),
                ("enabled", 24)
            ]
        );
        assert_eq!((lane.response.size, lane.response.align), (8, 4));
    }

    #[test]
    fn derives_host_execution_and_placement_from_effect_requirements() {
        let declaration = semantic_lane(
            r#"
use core::{HostEffect, MainThread}
struct Request { value: u32 }
struct Response { value: u32 }
pub fn update(request: Request) -> Response
    uses (HostEffect, MainThread)
{
    Response { value: request.value }
}
"#,
            "update",
        )
        .unwrap();
        assert_eq!(declaration.export, None);
        assert_eq!(
            declaration.intent,
            CanonicalLaneIntent {
                execution: CanonicalExecution::HostEffect,
                placement: CanonicalPlacement::MainThread,
                capabilities: vec![],
            }
        );
        let manifest = CanonicalInterfaceManifest::build(vec![declaration]).unwrap();
        assert_eq!(manifest.version, CANONICAL_INTERFACE_VERSION);
        assert_eq!(manifest.lanes[0].export, None);
    }

    #[test]
    fn manifest_rejects_exports_that_disagree_with_execution_intent() {
        let message = CanonicalType::Record(vec![CanonicalField::new(
            "value",
            CanonicalType::U32,
        )]);
        let host_with_export = CanonicalLaneDecl {
            name: "host".to_owned(),
            export: Some("fe_cabi_host".to_owned()),
            request: message.clone(),
            response: message.clone(),
            intent: CanonicalLaneIntent {
                execution: CanonicalExecution::HostEffect,
                placement: CanonicalPlacement::Worker,
                capabilities: vec![],
            },
        };
        assert!(
            CanonicalInterfaceManifest::build(vec![host_with_export])
                .unwrap_err()
                .to_string()
                .contains("must not declare a Wasm export")
        );
        let wasm_without_export = CanonicalLaneDecl {
            name: "wasm".to_owned(),
            export: None,
            request: message.clone(),
            response: message,
            intent: CanonicalLaneIntent::default(),
        };
        assert!(
            CanonicalInterfaceManifest::build(vec![wasm_without_export])
                .unwrap_err()
                .to_string()
                .contains("requires an export")
        );
    }

    #[test]
    fn semantic_derivation_fails_closed_for_strings_wide_scalars_and_non_records() {
        let string_error = semantic_lane(
            "struct Request { text: String<5> }\nstruct Response { ok: bool }\n\
             pub fn update(request: Request) -> Response { Response { ok: true } }\n",
            "update",
        )
        .unwrap_err();
        assert!(
            string_error.contains("explicit nominal BrowserString mapping"),
            "{string_error}"
        );

        let wide_error = semantic_lane(
            "struct Request { value: u16 }\nstruct Response { ok: bool }\n\
             pub fn update(request: Request) -> Response { Response { ok: true } }\n",
            "update",
        )
        .unwrap_err();
        assert!(
            wide_error.contains("unsupported canonical primitive `U16`"),
            "{wide_error}"
        );

        let scalar_error =
            semantic_lane("pub fn update(request: u32) -> u32 { request }\n", "update")
                .unwrap_err();
        assert!(
            scalar_error.contains("must both be nominal browser message types"),
            "{scalar_error}"
        );
    }

    #[test]
    fn same_named_user_descriptor_does_not_gain_canonical_string_semantics() {
        let declaration = semantic_lane(
            r#"
struct BrowserString { ptr: u32, len: u32 }
struct Request { value: BrowserString }
struct Response { ok: bool }
pub fn update(request: Request) -> Response { Response { ok: true } }
"#,
            "update",
        )
        .unwrap();
        let manifest = CanonicalInterfaceManifest::build(vec![declaration]).unwrap();
        let CanonicalShape::Record { fields } = &manifest.lanes[0].request.shape else {
            panic!("request record")
        };
        assert!(matches!(
            fields[0].layout.shape,
            CanonicalShape::Record { .. }
        ));
    }

    #[test]
    fn emitted_wasm_verifier_requires_complete_uniform_pointer_abi() {
        let manifest = one_lane_manifest();
        verify_canonical_wasm_abi(&canonical_wasm(0x7f, true, false), &manifest).unwrap();

        let missing_memory =
            verify_canonical_wasm_abi(&canonical_wasm(0x7f, false, false), &manifest).unwrap_err();
        assert!(
            missing_memory
                .to_string()
                .contains("missing exported memory"),
            "{missing_memory}"
        );

        let wrong_lane =
            verify_canonical_wasm_abi(&canonical_wasm(0x7e, true, false), &manifest).unwrap_err();
        assert!(
            wrong_lane.to_string().contains("expected [I32] -> [I32]"),
            "{wrong_lane}"
        );

        let memory64 =
            verify_canonical_wasm_abi(&canonical_wasm(0x7f, true, true), &manifest).unwrap_err();
        assert!(
            memory64
                .to_string()
                .contains("memory64; expected wasm32 memory"),
            "{memory64}"
        );
    }
}
