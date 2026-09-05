//! The const-projection seam for a render actor's `view()` behavior (web v5,
//! slice R1b, design decision 2).
//!
//! One new compiler capability: given the actor's reserved `const fn view()`
//! behavior (recognized structurally by name), CTFE-evaluate it to a concrete
//! record value with the existing semantic const machine
//! ([`eval_body_owner_const`]), then WALK the resulting value into a plain,
//! serializable [`ViewSurface`] the bundle projects into the manifest `surface`
//! section. No value is fabricated here: every number and kind comes from the
//! evaluated `Surface` record. The vocabulary types live in `std::web::view`,
//! not the compiler; this module only walks whatever `Surface { extent, params }`
//! shape the evaluation produced.
//!
//! Reconciliation of the params-record field names against the actor's state
//! fields and the lowered uniform binding members happens in the bundle
//! (`fe web`), which owns all three sources; this module returns the params in
//! the params-record declaration order with their names attached.

use num_traits::ToPrimitive;

use crate::{
    analysis::{
        HirAnalysisDb,
        semantic::{
            SemConstId, SemConstScalar, SemConstValue,
            ctfe::{CtfeError, eval_body_owner_const},
        },
        ty::{adt_def::AdtRef, ty_check::BodyOwner},
    },
    hir_def::{EnumVariant, Func},
};

/// The evaluated `view()` surface, projected to plain data.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewSurface {
    pub extent_width: u32,
    pub extent_height: u32,
    /// Params in the params-record's declaration order, each carrying its field
    /// name (the binding key reconciled by the bundle).
    pub params: Vec<ViewParam>,
}

/// One evaluated `Param`, keyed by its params-record field name.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewParam {
    pub name: String,
    /// Opaque snake-case spelling of the Fe enum variant. The compiler does
    /// not own or exhaustively mirror the parameter vocabulary.
    pub kind: String,
    pub min: f32,
    pub max: f32,
    pub init: f32,
    pub bounded: bool,
    pub initialized: bool,
    pub source: String,
    pub presentation: ViewParamPresentation,
}

/// Opaque projection of the Fe-owned `ParamPresentation` value. Strings are
/// enum-variant spellings, not a compiler-owned presentation vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewParamPresentation {
    pub widget: String,
    pub scale: String,
    pub readout: String,
    pub visible: bool,
    pub options: Vec<String>,
}

/// Why a `view()` projection could not be produced. Both variants carry a
/// human-readable reason for the bundle to surface as a compile error; a real
/// gap here is a correct outcome (there is no fallback).
#[derive(Debug, Clone, PartialEq)]
pub enum ViewProjectionError {
    /// The `view()` behavior did not const-evaluate to a value.
    NotConstEvaluable(String),
    /// The evaluated value did not have the expected `Surface` record shape.
    Shape(String),
}

impl std::fmt::Display for ViewProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConstEvaluable(reason) => {
                write!(f, "`view()` is not const-evaluable: {reason}")
            }
            Self::Shape(reason) => write!(f, "`view()` produced an unexpected shape: {reason}"),
        }
    }
}

/// CTFE-evaluate a render actor's `view()` behavior and walk the resulting
/// `Surface` record into plain data.
pub fn project_view_surface<'db>(
    db: &'db dyn HirAnalysisDb,
    view_func: Func<'db>,
) -> Result<ViewSurface, ViewProjectionError> {
    let value = eval_body_owner_const(db, BodyOwner::Func(view_func), Vec::new())
        .map_err(|error| ViewProjectionError::NotConstEvaluable(describe_ctfe_error(&error)))?;
    walk_surface(db, value)
}

fn describe_ctfe_error(error: &CtfeError<'_>) -> String {
    match error {
        CtfeError::NonConstCall { .. } => {
            "the behavior (or something it calls) is not a `const fn`".to_string()
        }
        CtfeError::NotConstEvaluable { .. } => "the body is not const-evaluable".to_string(),
        CtfeError::InvalidBody { .. } => "the body has type errors".to_string(),
        CtfeError::StepLimitExceeded { .. } | CtfeError::RecursionLimitExceeded { .. } => {
            "evaluation exceeded the const-evaluation budget".to_string()
        }
        CtfeError::CalleeError { source, .. } => describe_ctfe_error(source),
        other => format!("{other:?}"),
    }
}

/// Reads a struct value's fields, paired with their declared names in
/// declaration order (which is the order the SMIR aggregate carries).
fn struct_named_fields<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    what: &str,
) -> Result<Vec<(String, SemConstId<'db>)>, ViewProjectionError> {
    let SemConstValue::Struct { ty, fields } = value.value(db) else {
        return Err(ViewProjectionError::Shape(format!(
            "expected {what} to be a struct value"
        )));
    };
    let adt = ty
        .adt_def(db)
        .ok_or_else(|| ViewProjectionError::Shape(format!("{what}'s type is not a struct")))?;
    let AdtRef::Struct(struct_) = adt.adt_ref(db) else {
        return Err(ViewProjectionError::Shape(format!(
            "{what}'s type is not a struct"
        )));
    };
    let field_defs = struct_.hir_fields(db).data(db);
    if field_defs.len() != fields.len() {
        return Err(ViewProjectionError::Shape(format!(
            "{what} field count mismatch ({} declared, {} evaluated)",
            field_defs.len(),
            fields.len()
        )));
    }
    let mut out = Vec::with_capacity(fields.len());
    for (def, field) in field_defs.iter().zip(fields.iter().copied()) {
        let name = def
            .name
            .to_opt()
            .map(|ident| ident.data(db).to_string())
            .ok_or_else(|| ViewProjectionError::Shape(format!("{what} has an unnamed field")))?;
        out.push((name, field));
    }
    Ok(out)
}

fn read_u32<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    what: &str,
) -> Result<u32, ViewProjectionError> {
    match value.value(db) {
        SemConstValue::Scalar {
            value: SemConstScalar::Int { value },
            ..
        } => value
            .to_u32()
            .ok_or_else(|| ViewProjectionError::Shape(format!("{what} is not a u32"))),
        _ => Err(ViewProjectionError::Shape(format!(
            "{what} is not an integer scalar"
        ))),
    }
}

fn read_f32<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    what: &str,
) -> Result<f32, ViewProjectionError> {
    match value.value(db) {
        SemConstValue::Scalar {
            value: SemConstScalar::Float { bits },
            ..
        } => Ok(f32::from_bits(bits)),
        _ => Err(ViewProjectionError::Shape(format!(
            "{what} is not an f32 scalar"
        ))),
    }
}

fn read_bool<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    what: &str,
) -> Result<bool, ViewProjectionError> {
    match value.value(db) {
        SemConstValue::Scalar {
            value: SemConstScalar::Bool(value),
            ..
        } => Ok(value),
        _ => Err(ViewProjectionError::Shape(format!(
            "{what} is not a bool scalar"
        ))),
    }
}

fn read_u64<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    what: &str,
) -> Result<u64, ViewProjectionError> {
    match value.value(db) {
        SemConstValue::Scalar {
            value: SemConstScalar::Int { value },
            ..
        } => value
            .to_u64()
            .ok_or_else(|| ViewProjectionError::Shape(format!("{what} is not a u64"))),
        _ => Err(ViewProjectionError::Shape(format!(
            "{what} is not an integer scalar"
        ))),
    }
}

fn read_choice_label<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    what: &str,
) -> Result<String, ViewProjectionError> {
    let mut lanes = [None; 4];
    for (field_name, field) in struct_named_fields(db, value, what)? {
        let index = match field_name.as_str() {
            "low_0" => 0,
            "low_1" => 1,
            "low_2" => 2,
            "low_3" => 3,
            _ => continue,
        };
        lanes[index] = Some(read_u64(
            db,
            field,
            &format!("{what} packed lane `{field_name}`"),
        )?);
    }
    let mut bytes = Vec::with_capacity(32);
    for (index, lane) in lanes.into_iter().enumerate().rev() {
        bytes.extend_from_slice(
            &lane
                .ok_or_else(|| {
                    ViewProjectionError::Shape(format!("{what} has no packed lane `low_{index}`"))
                })?
                .to_be_bytes(),
        );
    }
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    let bytes = &bytes[first..];
    if bytes.contains(&0) {
        return Err(ViewProjectionError::Shape(format!(
            "{what} contains an embedded NUL"
        )));
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|error| ViewProjectionError::Shape(format!("{what} is not UTF-8: {error}")))
}

fn walk_choice_options<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    name: &str,
) -> Result<Vec<String>, ViewProjectionError> {
    let fields = struct_named_fields(db, value, &format!("param `{name}` choice options"))?;
    let mut labels = Vec::with_capacity(4);
    let mut count = None;
    for (field_name, field) in fields {
        match field_name.as_str() {
            "first" | "second" | "third" | "fourth" => labels.push(read_choice_label(
                db,
                field,
                &format!("param `{name}` choice option `{field_name}`"),
            )?),
            "count" => {
                count = Some(read_u32(
                    db,
                    field,
                    &format!("param `{name}` choice option count"),
                )?)
            }
            _ => {}
        }
    }
    let count = count.ok_or_else(|| {
        ViewProjectionError::Shape(format!("param `{name}` choice options have no `count`"))
    })? as usize;
    if count > labels.len() {
        return Err(ViewProjectionError::Shape(format!(
            "param `{name}` declares {count} choice options but carries only {} labels",
            labels.len()
        )));
    }
    labels.truncate(count);
    if labels.iter().any(String::is_empty) {
        return Err(ViewProjectionError::Shape(format!(
            "param `{name}` has an empty visible choice label"
        )));
    }
    Ok(labels)
}

fn snake_case_variant(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (index, character) in name.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.extend(character.to_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

fn read_enum_case<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    what: &str,
) -> Result<String, ViewProjectionError> {
    let SemConstValue::Enum { ty, variant, .. } = value.value(db) else {
        return Err(ViewProjectionError::Shape(format!(
            "{what} is not an enum value"
        )));
    };
    let adt = ty
        .adt_def(db)
        .ok_or_else(|| ViewProjectionError::Shape(format!("{what}'s type is not an enum")))?;
    let AdtRef::Enum(enum_) = adt.adt_ref(db) else {
        return Err(ViewProjectionError::Shape(format!(
            "{what}'s type is not an enum"
        )));
    };
    let variant_name = EnumVariant::new(enum_, variant.0 as usize)
        .name(db)
        .ok_or_else(|| ViewProjectionError::Shape(format!("{what} variant has no name")))?;
    Ok(snake_case_variant(variant_name))
}

fn walk_param_presentation<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
    name: &str,
) -> Result<ViewParamPresentation, ViewProjectionError> {
    let mut widget = None;
    let mut scale = None;
    let mut readout = None;
    let mut visible = None;
    let mut options = None;
    for (field_name, field) in
        struct_named_fields(db, value, &format!("param `{name}` presentation"))?
    {
        match field_name.as_str() {
            "widget" => {
                widget = Some(read_enum_case(
                    db,
                    field,
                    &format!("param `{name}` presentation widget"),
                )?)
            }
            "scale" => {
                scale = Some(read_enum_case(
                    db,
                    field,
                    &format!("param `{name}` presentation scale"),
                )?)
            }
            "readout" => {
                readout = Some(read_enum_case(
                    db,
                    field,
                    &format!("param `{name}` presentation readout"),
                )?)
            }
            "visible" => {
                visible = Some(read_bool(
                    db,
                    field,
                    &format!("param `{name}` presentation visible"),
                )?)
            }
            "options" => options = Some(walk_choice_options(db, field, name)?),
            _ => {}
        }
    }
    Ok(ViewParamPresentation {
        widget: widget.ok_or_else(|| {
            ViewProjectionError::Shape(format!("param `{name}` presentation has no `widget`"))
        })?,
        scale: scale.ok_or_else(|| {
            ViewProjectionError::Shape(format!("param `{name}` presentation has no `scale`"))
        })?,
        readout: readout.ok_or_else(|| {
            ViewProjectionError::Shape(format!("param `{name}` presentation has no `readout`"))
        })?,
        visible: visible.ok_or_else(|| {
            ViewProjectionError::Shape(format!("param `{name}` presentation has no `visible`"))
        })?,
        options: options.ok_or_else(|| {
            ViewProjectionError::Shape(format!("param `{name}` presentation has no `options`"))
        })?,
    })
}

fn walk_surface<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
) -> Result<ViewSurface, ViewProjectionError> {
    let mut extent = None;
    let mut params = None;
    for (name, field) in struct_named_fields(db, value, "the `Surface` value")? {
        match name.as_str() {
            "extent" => extent = Some(field),
            "params" => params = Some(field),
            _ => {}
        }
    }
    let extent = extent
        .ok_or_else(|| ViewProjectionError::Shape("`Surface` has no `extent` field".into()))?;
    let params = params
        .ok_or_else(|| ViewProjectionError::Shape("`Surface` has no `params` field".into()))?;
    let (extent_width, extent_height) = walk_extent(db, extent)?;
    let params = walk_params(db, params)?;
    Ok(ViewSurface {
        extent_width,
        extent_height,
        params,
    })
}

fn walk_extent<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
) -> Result<(u32, u32), ViewProjectionError> {
    let mut width = None;
    let mut height = None;
    for (name, field) in struct_named_fields(db, value, "the `Extent` value")? {
        match name.as_str() {
            "width" => width = Some(read_u32(db, field, "`extent.width`")?),
            "height" => height = Some(read_u32(db, field, "`extent.height`")?),
            _ => {}
        }
    }
    let width =
        width.ok_or_else(|| ViewProjectionError::Shape("`Extent` has no `width` field".into()))?;
    let height = height
        .ok_or_else(|| ViewProjectionError::Shape("`Extent` has no `height` field".into()))?;
    Ok((width, height))
}

fn walk_params<'db>(
    db: &'db dyn HirAnalysisDb,
    value: SemConstId<'db>,
) -> Result<Vec<ViewParam>, ViewProjectionError> {
    let mut out = Vec::new();
    for (name, field) in struct_named_fields(db, value, "the params record")? {
        out.push(walk_param(db, name, field)?);
    }
    Ok(out)
}

fn walk_param<'db>(
    db: &'db dyn HirAnalysisDb,
    name: String,
    value: SemConstId<'db>,
) -> Result<ViewParam, ViewProjectionError> {
    let mut kind = None;
    let mut min = None;
    let mut max = None;
    let mut init = None;
    let mut bounded = None;
    let mut initialized = None;
    let mut source = None;
    let mut presentation = None;
    for (field_name, field) in struct_named_fields(db, value, &format!("param `{name}`"))? {
        match field_name.as_str() {
            "kind" => kind = Some(read_enum_case(db, field, &format!("param `{name}` kind"))?),
            "min" => min = Some(read_f32(db, field, &format!("param `{name}` min"))?),
            "max" => max = Some(read_f32(db, field, &format!("param `{name}` max"))?),
            "init" => init = Some(read_f32(db, field, &format!("param `{name}` init"))?),
            "bounded" => bounded = Some(read_bool(db, field, &format!("param `{name}` bounded"))?),
            "initialized" => {
                initialized = Some(read_bool(
                    db,
                    field,
                    &format!("param `{name}` initialized"),
                )?)
            }
            "source" => {
                source = Some(read_enum_case(
                    db,
                    field,
                    &format!("param `{name}` source"),
                )?)
            }
            "presentation" => presentation = Some(walk_param_presentation(db, field, &name)?),
            _ => {}
        }
    }
    let kind =
        kind.ok_or_else(|| ViewProjectionError::Shape(format!("param `{name}` has no `kind`")))?;
    let min =
        min.ok_or_else(|| ViewProjectionError::Shape(format!("param `{name}` has no `min`")))?;
    let max =
        max.ok_or_else(|| ViewProjectionError::Shape(format!("param `{name}` has no `max`")))?;
    let init =
        init.ok_or_else(|| ViewProjectionError::Shape(format!("param `{name}` has no `init`")))?;
    let bounded = bounded
        .ok_or_else(|| ViewProjectionError::Shape(format!("param `{name}` has no `bounded`")))?;
    let initialized = initialized.ok_or_else(|| {
        ViewProjectionError::Shape(format!("param `{name}` has no `initialized`"))
    })?;
    let source = source
        .ok_or_else(|| ViewProjectionError::Shape(format!("param `{name}` has no `source`")))?;
    let presentation = presentation.ok_or_else(|| {
        ViewProjectionError::Shape(format!("param `{name}` has no `presentation`"))
    })?;
    Ok(ViewParam {
        name,
        kind,
        min,
        max,
        init,
        bounded,
        initialized,
        source,
        presentation,
    })
}
