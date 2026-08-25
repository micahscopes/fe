//! Compiler-emitted browser adapters for target-neutral resumable machines.
//!
//! The browser must not rediscover continuation states, flattened lanes, or
//! generated export names from Wasm. This module projects those facts directly
//! from the same MIR machine consumed by Wasm lowering into an ES module which
//! closes over the exact exports. The accompanying fixed runtime owns only
//! affine frame custody and typed scalar validation.

use std::collections::HashSet;

use compiler_db::DriverDataBase;
use hir::{analysis::ty::adt_def::AdtRef, hir_def::HostType};
use mir::{Layout, LayoutId, RuntimeClass, RuntimeLinkage, RuntimePackage, ScalarRepr, ScalarRole};

use crate::{CanonicalType, canonical_type_from_semantic};

use super::{LowerError, lower_runtime::assign_sonatina_function_symbols};

pub const MATERIALIZED_TASK_RUNTIME_JS: &str =
    include_str!("../../assets/browser-runtime/materialized-task.js");
pub const HOST_COMPLETION_RUNTIME_JS: &str =
    include_str!("../../assets/browser-runtime/host-completion.js");

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WasmTaskScalar {
    Bool,
    Signed { bits: u16 },
    Unsigned { bits: u16 },
    BorrowedPointer { stride: u32, align: u32, max: u32 },
    FixedBytes { bits: u16 },
    F32,
    EnumTag { bits: u16, variants: u32 },
}

impl WasmTaskScalar {
    fn javascript(&self) -> String {
        match self {
            Self::Bool => "{ kind: \"bool\", bits: 1 }".to_owned(),
            Self::Signed { bits } => format!("{{ kind: \"signed\", bits: {bits} }}"),
            Self::Unsigned { bits } => format!("{{ kind: \"unsigned\", bits: {bits} }}"),
            Self::BorrowedPointer { stride, align, max } => format!(
                "{{ kind: \"borrowed_pointer\", bits: 32, stride: {stride}, align: {align}, max: {max} }}"
            ),
            Self::FixedBytes { bits } => {
                format!("{{ kind: \"fixed_bytes\", bits: {bits} }}")
            }
            Self::F32 => "{ kind: \"f32\", bits: 32 }".to_owned(),
            Self::EnumTag { bits, variants } => {
                format!("{{ kind: \"enum_tag\", bits: {bits}, variants: {variants} }}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmTaskRange {
    pub start: usize,
    pub count: usize,
}

impl WasmTaskRange {
    fn javascript(self) -> String {
        format!("{{ start: {}, count: {} }}", self.start, self.count)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmTaskDelivery {
    pub lanes: Vec<WasmTaskScalar>,
    pub failure: WasmTaskRange,
    pub success: WasmTaskRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmTaskContinuation {
    pub state: u32,
    pub export: String,
    pub range: WasmTaskRange,
    pub pending: WasmTaskRange,
    pub frame: WasmTaskRange,
    pub delivery: WasmTaskDelivery,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmTaskAdapter {
    pub name: String,
    pub start_export: String,
    pub input: Vec<WasmTaskScalar>,
    pub step: Vec<WasmTaskScalar>,
    pub complete: WasmTaskRange,
    pub continuations: Vec<WasmTaskContinuation>,
}

fn unsupported(message: impl Into<String>) -> LowerError {
    LowerError::Unsupported(format!(
        "browser continuation adapter is incomplete: {}",
        message.into()
    ))
}

fn scalar_lane(
    scalar: &mir::ScalarClass<'_>,
    db: &DriverDataBase,
) -> Result<WasmTaskScalar, LowerError> {
    if let ScalarRole::EnumTag { enum_layout } = scalar.role {
        let Layout::Enum(layout) = enum_layout.data(db) else {
            return Err(LowerError::Internal(
                "enum-tag scalar does not reference an enum layout".to_owned(),
            ));
        };
        let bits = match scalar.repr {
            ScalarRepr::Int { bits, .. } => bits,
            _ => 32,
        };
        return Ok(WasmTaskScalar::EnumTag {
            bits,
            variants: u32::try_from(layout.variants.len())
                .map_err(|_| unsupported("enum variant count exceeds u32"))?,
        });
    }
    Ok(match scalar.repr {
        ScalarRepr::Bool => WasmTaskScalar::Bool,
        ScalarRepr::Int { bits, signed } if bits <= 64 => {
            if signed {
                WasmTaskScalar::Signed { bits }
            } else {
                WasmTaskScalar::Unsigned { bits }
            }
        }
        ScalarRepr::FixedBytes { len } if len <= 8 => WasmTaskScalar::FixedBytes {
            bits: len.saturating_mul(8),
        },
        ScalarRepr::Float { bits: 32 } => WasmTaskScalar::F32,
        ScalarRepr::Int { bits, .. } => {
            return Err(unsupported(format!(
                "integer lane u/i{bits} exceeds Wasm64"
            )));
        }
        ScalarRepr::FixedBytes { len } => {
            return Err(unsupported(format!("bytes{len} exceeds one Wasm64 lane")));
        }
        ScalarRepr::Float { bits } => {
            return Err(unsupported(format!("f{bits} has no browser Wasm carrier")));
        }
        ScalarRepr::Address { bits } => {
            return Err(unsupported(format!(
                "address<{bits}> cannot survive a browser suspension frame"
            )));
        }
    })
}

fn canonical_descriptor_pointer_lane(
    class: &RuntimeClass<'_>,
    descriptor: &CanonicalType,
    path: &str,
) -> Result<WasmTaskScalar, LowerError> {
    let valid = match class {
        RuntimeClass::Scalar(scalar)
            if matches!(
                scalar.repr,
                ScalarRepr::Int {
                    bits: 32,
                    signed: false
                } | ScalarRepr::Address { bits: 32 }
            ) =>
        {
            true
        }
        RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => true,
        _ => false,
    };
    if !valid {
        return Err(unsupported(format!(
            "explicit canonical descriptor `{path}` has no wasm32 pointer carrier"
        )));
    }
    let (stride, align, max) = match descriptor {
        CanonicalType::Bytes | CanonicalType::String => (1, 1, u32::MAX),
        CanonicalType::List { max, .. } => (4, 4, *max),
        _ => {
            return Err(LowerError::Internal(
                "borrowed pointer metadata requested for a scalar canonical type".to_owned(),
            ));
        }
    };
    Ok(WasmTaskScalar::BorrowedPointer { stride, align, max })
}

fn canonical_descriptor_length_lane(
    class: &RuntimeClass<'_>,
    path: &str,
) -> Result<WasmTaskScalar, LowerError> {
    let RuntimeClass::Scalar(scalar) = class else {
        return Err(unsupported(format!(
            "explicit canonical descriptor `{path}` has no u32 length carrier"
        )));
    };
    if !matches!(
        scalar.repr,
        ScalarRepr::Int {
            bits: 32,
            signed: false
        }
    ) {
        return Err(unsupported(format!(
            "explicit canonical descriptor `{path}` has no u32 length carrier"
        )));
    }
    Ok(WasmTaskScalar::Unsigned { bits: 32 })
}

fn canonical_descriptor_lanes<'db>(
    db: &'db DriverDataBase,
    layout: LayoutId<'db>,
) -> Result<Option<[WasmTaskScalar; 2]>, LowerError> {
    let Layout::Struct(layout) = layout.data(db) else {
        return Ok(None);
    };
    let source_ty = layout.source_ty.as_view(db).unwrap_or(layout.source_ty);
    let Some(AdtRef::Struct(struct_)) = source_ty.adt_def(db).map(|adt| adt.adt_ref(db)) else {
        return Ok(None);
    };
    let Some(host_type) = struct_
        .scope()
        .attrs(db)
        .and_then(|attrs| attrs.host_type(db))
    else {
        return Ok(None);
    };
    if !matches!(
        host_type,
        HostType::Bytes | HostType::String | HostType::List
    ) {
        return Ok(None);
    }
    let descriptor = canonical_type_from_semantic(db, source_ty, "task_descriptor")
        .map_err(|error| unsupported(error.to_string()))?;
    if !matches!(
        descriptor,
        CanonicalType::Bytes | CanonicalType::String | CanonicalType::List { .. }
    ) {
        return Err(LowerError::Internal(
            "explicit browser host type did not derive a canonical descriptor".to_owned(),
        ));
    }
    let [pointer, length] = layout.fields.as_ref() else {
        return Err(unsupported(
            "explicit canonical descriptor must contain exactly pointer and length fields",
        ));
    };
    Ok(Some([
        canonical_descriptor_pointer_lane(pointer, &descriptor, "task_descriptor.ptr")?,
        canonical_descriptor_length_lane(length, "task_descriptor.len")?,
    ]))
}

fn flatten_class<'db>(
    db: &'db DriverDataBase,
    class: &RuntimeClass<'db>,
    active: &mut HashSet<LayoutId<'db>>,
    output: &mut Vec<WasmTaskScalar>,
) -> Result<(), LowerError> {
    match class {
        RuntimeClass::Scalar(scalar) => output.push(scalar_lane(scalar, db)?),
        RuntimeClass::AggregateValue { layout } => {
            if let Some(lanes) = canonical_descriptor_lanes(db, *layout)? {
                output.extend(lanes);
                return Ok(());
            }
            if !active.insert(*layout) {
                return Err(unsupported("recursive aggregate value layout"));
            }
            match layout.data(db) {
                Layout::Struct(layout) => {
                    for field in &layout.fields {
                        flatten_class(db, field, active, output)?;
                    }
                }
                Layout::Array(layout) => {
                    let len = usize::try_from(layout.len)
                        .map_err(|_| unsupported("array frame length exceeds usize"))?;
                    for _ in 0..len {
                        flatten_class(db, &layout.elem, active, output)?;
                    }
                }
                Layout::Enum(layout) => {
                    output.push(scalar_lane(&layout.tag, db)?);
                    for variant in &layout.variants {
                        for field in &variant.fields {
                            flatten_class(db, field, active, output)?;
                        }
                    }
                }
            }
            active.remove(layout);
        }
        RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => {
            return Err(unsupported(
                "references and raw addresses need owned canonical post-return storage",
            ));
        }
    }
    Ok(())
}

fn flatten_classes<'db>(
    db: &'db DriverDataBase,
    classes: impl IntoIterator<Item = &'db RuntimeClass<'db>>,
) -> Result<Vec<WasmTaskScalar>, LowerError> {
    let mut output = Vec::new();
    let mut active = HashSet::new();
    for class in classes {
        flatten_class(db, class, &mut active, &mut output)?;
    }
    Ok(output)
}

fn delivery_layout<'db>(
    db: &'db DriverDataBase,
    class: &'db RuntimeClass<'db>,
) -> Result<WasmTaskDelivery, LowerError> {
    let RuntimeClass::AggregateValue { layout } = class else {
        return Err(LowerError::Internal(
            "suspension delivery is not the typed TaskOutcome enum".to_owned(),
        ));
    };
    let Layout::Enum(layout) = layout.data(db) else {
        return Err(LowerError::Internal(
            "suspension delivery is not the typed TaskOutcome enum".to_owned(),
        ));
    };
    let [failure, success, cancelled] = layout.variants.as_ref() else {
        return Err(LowerError::Internal(
            "TaskOutcome must retain Failure, Success, Cancelled variants".to_owned(),
        ));
    };
    if !cancelled.fields.is_empty() {
        return Err(LowerError::Internal(
            "TaskOutcome::Cancelled unexpectedly carries runtime fields".to_owned(),
        ));
    }
    let tag = scalar_lane(&layout.tag, db)?;
    let failure_lanes = flatten_classes(db, failure.fields.iter())?;
    let success_lanes = flatten_classes(db, success.fields.iter())?;
    let failure = WasmTaskRange {
        start: 1,
        count: failure_lanes.len(),
    };
    let success = WasmTaskRange {
        start: 1 + failure_lanes.len(),
        count: success_lanes.len(),
    };
    let lanes = std::iter::once(tag)
        .chain(failure_lanes)
        .chain(success_lanes)
        .collect();
    Ok(WasmTaskDelivery {
        lanes,
        failure,
        success,
    })
}

/// Derive every public browser-callable task adapter from the same package and
/// target-neutral machines used by Wasm lowering. Private helper/provider
/// machines deliberately remain absent from this public projection.
pub fn materialized_task_adapters<'db>(
    db: &'db DriverDataBase,
    package: &'db RuntimePackage<'db>,
) -> Result<Vec<WasmTaskAdapter>, LowerError> {
    let symbols = assign_sonatina_function_symbols(db, package);
    let plans = mir::derive_runtime_resumable_plans(db, *package).map_err(|error| {
        LowerError::Unsupported(format!(
            "failed to derive browser continuation adapters: {error:?}"
        ))
    })?;
    let functions = package.functions(db);
    let mut adapters = Vec::new();
    for plan in plans {
        let Some(function) = functions
            .iter()
            .copied()
            .find(|function| function.instance(db) == plan.body)
        else {
            return Err(LowerError::Internal(
                "resumable body has no runtime function declaration".to_owned(),
            ));
        };
        if function.linkage(db) != RuntimeLinkage::Internal {
            continue;
        }
        let machine = mir::materialize_runtime_resumable_machine(db, &plan).map_err(|error| {
            LowerError::Unsupported(format!(
                "browser resumable stack materialization is incomplete for `{}`: {error:?}",
                mir::runtime_instance_symbol_key(db, plan.body)
            ))
        })?;
        let name = symbols
            .get(&plan.body)
            .cloned()
            .unwrap_or_else(|| mir::runtime_instance_symbol_key(db, plan.body));
        let input = flatten_classes(
            db,
            machine
                .entry
                .body
                .signature
                .params
                .iter()
                .map(|param| &param.class),
        )?;
        let Layout::Enum(step_layout) = machine.step_layout.data(db) else {
            return Err(LowerError::Internal(
                "materialized task step is not an enum layout".to_owned(),
            ));
        };
        let mut step = vec![scalar_lane(&step_layout.tag, db)?];
        let mut variant_ranges = Vec::with_capacity(step_layout.variants.len());
        for variant in &step_layout.variants {
            let start = step.len();
            step.extend(flatten_classes(db, variant.fields.iter())?);
            variant_ranges.push(WasmTaskRange {
                start,
                count: step.len() - start,
            });
        }
        let complete = variant_ranges[0];
        if machine.continuations.len() != plan.points.len()
            || step_layout.variants.len() != plan.points.len() + 1
        {
            return Err(LowerError::Internal(
                "materialized task continuation/variant cardinality drift".to_owned(),
            ));
        }
        let mut continuations = Vec::with_capacity(plan.points.len());
        for ((point, segment), range) in plan
            .points
            .iter()
            .zip(machine.continuations.iter())
            .zip(variant_ranges.into_iter().skip(1))
        {
            let pending_class = plan
                .flattened_body
                .value_class(match point.cause {
                    mir::RuntimeSuspensionCause::Effect { pending } => pending,
                    mir::RuntimeSuspensionCause::Callee { .. } => {
                        return Err(LowerError::Internal(
                            "unflattened callee suspension reached browser adapter".to_owned(),
                        ));
                    }
                })
                .ok_or_else(|| {
                    LowerError::Internal("pending local has no runtime class".to_owned())
                })?;
            let pending_count = flatten_classes(db, std::iter::once(pending_class))?.len();
            let frame_count = flatten_classes(
                db,
                point.live_values.iter().map(|local| {
                    plan.flattened_body
                        .value_class(*local)
                        .expect("liveness planner retained only runtime values")
                }),
            )?
            .len();
            if range.count != pending_count + frame_count {
                return Err(LowerError::Internal(
                    "materialized task variant does not match pending plus exact frame".to_owned(),
                ));
            }
            let delivery_class =
                plan.flattened_body
                    .value_class(point.delivery)
                    .ok_or_else(|| {
                        LowerError::Internal("delivery local has no runtime class".to_owned())
                    })?;
            continuations.push(WasmTaskContinuation {
                state: point.continuation_state,
                export: format!("__fe_task_resume_{name}_{}", segment.continuation_state),
                range,
                pending: WasmTaskRange {
                    start: range.start,
                    count: pending_count,
                },
                frame: WasmTaskRange {
                    start: range.start + pending_count,
                    count: frame_count,
                },
                delivery: delivery_layout(db, delivery_class)?,
            });
        }
        adapters.push(WasmTaskAdapter {
            start_export: format!("__fe_task_start_{name}"),
            name,
            input,
            step,
            complete,
            continuations,
        });
    }
    Ok(adapters)
}

fn lanes_javascript(lanes: &[WasmTaskScalar]) -> String {
    format!(
        "[{}]",
        lanes
            .iter()
            .map(WasmTaskScalar::javascript)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Emit an ES module which binds concrete compiler-generated exports to the
/// fixed browser task runtime. The module specifier is build-system data, not
/// an application protocol, and no JSON is emitted or parsed at runtime.
pub fn emit_materialized_task_adapter_js(
    adapters: &[WasmTaskAdapter],
    runtime_module: &str,
) -> Result<Option<String>, LowerError> {
    if adapters.is_empty() {
        return Ok(None);
    }
    if runtime_module.is_empty() {
        return Err(LowerError::Internal(
            "materialized task runtime module specifier is empty".to_owned(),
        ));
    }
    let runtime_module = serde_json::to_string(runtime_module)
        .map_err(|error| LowerError::Internal(error.to_string()))?;
    let mut source = format!(
        "import {{ createMaterializedTaskMachine }} from {runtime_module};\n\n\
         function required(value, name) {{\n  \
         if (typeof value !== \"function\") throw new TypeError(`missing materialized task export ${{name}}`);\n  \
         return value;\n}}\n\n\
         export function createMaterializedTaskRegistry(wasmExports) {{\n  \
         const registry = Object.create(null);\n"
    );
    for (task_index, adapter) in adapters.iter().enumerate() {
        let name = serde_json::to_string(&adapter.name)
            .map_err(|error| LowerError::Internal(error.to_string()))?;
        let start_export = serde_json::to_string(&adapter.start_export)
            .map_err(|error| LowerError::Internal(error.to_string()))?;
        source.push_str(&format!(
            "  const task{task_index}StartName = {start_export};\n  \
             const task{task_index}Start = required(wasmExports[task{task_index}StartName], task{task_index}StartName);\n"
        ));
        for (continuation_index, continuation) in adapter.continuations.iter().enumerate() {
            let export = serde_json::to_string(&continuation.export)
                .map_err(|error| LowerError::Internal(error.to_string()))?;
            source.push_str(&format!(
                "  const task{task_index}Resume{continuation_index}Name = {export};\n  \
                 const task{task_index}Resume{continuation_index} = required(wasmExports[task{task_index}Resume{continuation_index}Name], task{task_index}Resume{continuation_index}Name);\n"
            ));
        }
        source.push_str(&format!(
            "  registry[{name}] = createMaterializedTaskMachine({{\n    \
             input: {},\n    \
             step: {},\n    \
             complete: {},\n    \
             start: (...lanes) => task{task_index}Start(...lanes),\n    \
             continuations: [\n",
            lanes_javascript(&adapter.input),
            lanes_javascript(&adapter.step),
            adapter.complete.javascript(),
        ));
        for (continuation_index, continuation) in adapter.continuations.iter().enumerate() {
            source.push_str(&format!(
                "      {{ state: {}, range: {}, pending: {}, frame: {}, \
                 delivery: {{ lanes: {}, failure: {}, success: {} }}, \
                 invoke: (...lanes) => task{task_index}Resume{continuation_index}(...lanes) }},\n",
                continuation.state,
                continuation.range.javascript(),
                continuation.pending.javascript(),
                continuation.frame.javascript(),
                lanes_javascript(&continuation.delivery.lanes),
                continuation.delivery.failure.javascript(),
                continuation.delivery.success.javascript(),
            ));
        }
        source.push_str("    ],\n  }, wasmExports);\n");
    }
    source.push_str("  return Object.freeze(registry);\n}\n");
    Ok(Some(source))
}
