//! Const projection of one role-selected Fe page actor.
//!
//! The page vocabulary is ordinary Fe library data. This module selects the
//! nominal `#[actor_page_projection]` role, evaluates that behavior, and
//! validates/walks the closed `std::web::page` value contract into typed Rust
//! operations. HTML syntax and browser realization remain outside the
//! compiler.

use std::fmt;

use compiler_db::DriverDataBase;
use hir::{
    analysis::{
        semantic::{
            SemConstId, SemConstScalar, SemConstValue,
            ctfe::{CtfeError, eval_body_owner_const},
        },
        ty::{adt_def::AdtRef, ty_check::BodyOwner},
    },
    hir_def::{EnumVariant, TopLevelMod},
};

use crate::actor_semantics::{nominal_attrs, resolve_metadata_ty, semantic_actors};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageElement {
    Header,
    Main,
    Div,
    Figure,
    Figcaption,
    Span,
    Bold,
    Paragraph,
    Code,
    Section,
    Footer,
    Heading1,
    Heading2,
    Input,
    Label,
    UnorderedList,
    ListItem,
    Button,
    Template,
    Strong,
    Pre,
    Anchor,
    FeComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAttributeKind {
    Id,
    LocalId,
    Class,
    Role,
    AriaLabel,
    AriaModal,
    InputType,
    For,
    LocalFor,
    Title,
    Placeholder,
    Autocomplete,
    Target,
    Rel,
    Href,
    Hidden,
    Action,
    Node,
    View,
    Template,
    ClassToken,
    Publish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedPageAttribute {
    pub kind: PageAttributeKind,
    pub text: String,
    pub number: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedPageRender {
    pub source: String,
    pub entry: String,
    pub wgsl_action: u32,
    pub wasm_action: u32,
    pub manifest_action: u32,
    pub sequenced: bool,
    pub sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedPageComponent {
    pub source: String,
    pub mount: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageProjectionOp {
    Open(PageElement),
    Attribute(ProjectedPageAttribute),
    Text(String),
    Close,
    Render(ProjectedPageRender),
    Component(ProjectedPageComponent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageProjection {
    pub actor: String,
    pub source_entry: String,
    pub title: String,
    pub body: Vec<PageProjectionOp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentProjection {
    pub actor: String,
    pub source_entry: String,
    pub body: Vec<PageProjectionOp>,
}

#[derive(Debug)]
pub enum PageProjectionError {
    Contract(String),
    NotConstEvaluable(String),
    Shape(String),
}

impl fmt::Display for PageProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(detail) => write!(formatter, "page projection contract: {detail}"),
            Self::NotConstEvaluable(detail) => {
                write!(
                    formatter,
                    "page projection is not const-evaluable: {detail}"
                )
            }
            Self::Shape(detail) => write!(formatter, "page projection shape: {detail}"),
        }
    }
}

impl std::error::Error for PageProjectionError {}

fn behavior_is_page_projection(db: &DriverDataBase, behavior: hir::hir_def::Func<'_>) -> bool {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.is_actor_page_projection(db))
}

fn behavior_is_component_projection(db: &DriverDataBase, behavior: hir::hir_def::Func<'_>) -> bool {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.is_actor_component_projection(db))
}

/// Find and CTFE-project the module's unique page-composition behavior.
pub fn project_page(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<Option<PageProjection>, PageProjectionError> {
    let selected = semantic_actors(db, top_mod)
        .into_iter()
        .flat_map(|actor| {
            actor
                .behaviors
                .into_iter()
                .filter(|behavior| behavior_is_page_projection(db, *behavior))
                .map(move |behavior| (actor.state, behavior))
        })
        .collect::<Vec<_>>();
    let (state, behavior) = match selected.as_slice() {
        [] => return Ok(None),
        [selected] => *selected,
        _ => {
            return Err(PageProjectionError::Contract(format!(
                "module declares {} page-composition behaviors; exactly one is required",
                selected.len()
            )));
        }
    };
    if !behavior.arg_tys(db).is_empty() {
        return Err(PageProjectionError::Contract(
            "page composition must be self-less and take no arguments".to_owned(),
        ));
    }
    let actor = state
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| PageProjectionError::Contract("page actor has no name".to_owned()))?;
    let source_entry = behavior
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| PageProjectionError::Contract("page behavior has no name".to_owned()))?;
    let value = eval_body_owner_const(db, BodyOwner::Func(behavior), Vec::new())
        .map_err(|error| PageProjectionError::NotConstEvaluable(describe_ctfe_error(&error)))?;
    let (title, body) = walk_page(db, value)?;
    Ok(Some(PageProjection {
        actor,
        source_entry,
        title,
        body,
    }))
}

/// Find and CTFE-project the module's unique resident-component view behavior.
/// `Ok(None)` preserves compatibility for components whose light DOM is still
/// supplied by authored HTML.
pub fn project_component(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<Option<ComponentProjection>, PageProjectionError> {
    let selected = semantic_actors(db, top_mod)
        .into_iter()
        .flat_map(|actor| {
            actor
                .behaviors
                .into_iter()
                .filter(|behavior| behavior_is_component_projection(db, *behavior))
                .map(move |behavior| (actor.state, behavior))
        })
        .collect::<Vec<_>>();
    let (state, behavior) = match selected.as_slice() {
        [] => return Ok(None),
        [selected] => *selected,
        _ => {
            return Err(PageProjectionError::Contract(format!(
                "module declares {} component-view behaviors; exactly one is required",
                selected.len()
            )));
        }
    };
    if !behavior.arg_tys(db).is_empty() {
        return Err(PageProjectionError::Contract(
            "component view composition must be self-less and take no arguments".to_owned(),
        ));
    }
    let actor = state
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| PageProjectionError::Contract("component actor has no name".to_owned()))?;
    let source_entry = behavior
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| {
            PageProjectionError::Contract("component view behavior has no name".to_owned())
        })?;
    let value = eval_body_owner_const(db, BodyOwner::Func(behavior), Vec::new())
        .map_err(|error| PageProjectionError::NotConstEvaluable(describe_ctfe_error(&error)))?;
    let body = walk_component(db, value)?;
    Ok(Some(ComponentProjection {
        actor,
        source_entry,
        body,
    }))
}

fn describe_ctfe_error(error: &CtfeError<'_>) -> String {
    match error {
        CtfeError::NonConstCall { .. } => {
            "the behavior (or something it calls) is not a `const fn`".to_owned()
        }
        CtfeError::NotConstEvaluable { .. } => "the body is not const-evaluable".to_owned(),
        CtfeError::InvalidBody { .. } => "the body has type errors".to_owned(),
        CtfeError::StepLimitExceeded { .. } | CtfeError::RecursionLimitExceeded { .. } => {
            "evaluation exceeded the const-evaluation budget".to_owned()
        }
        CtfeError::CalleeError { source, .. } => describe_ctfe_error(source),
        other => format!("{other:?}"),
    }
}

fn struct_fields<'db>(
    db: &'db DriverDataBase,
    value: SemConstId<'db>,
    what: &str,
) -> Result<Vec<(String, SemConstId<'db>)>, PageProjectionError> {
    let SemConstValue::Struct { ty, fields } = value.value(db) else {
        return Err(PageProjectionError::Shape(format!(
            "expected {what} to be a struct value"
        )));
    };
    let adt = ty
        .adt_def(db)
        .ok_or_else(|| PageProjectionError::Shape(format!("{what} is not a struct")))?;
    let AdtRef::Struct(definition) = adt.adt_ref(db) else {
        return Err(PageProjectionError::Shape(format!(
            "expected {what} to be a struct value"
        )));
    };
    let definitions = definition.hir_fields(db).data(db);
    if definitions.len() != fields.len() {
        return Err(PageProjectionError::Shape(format!(
            "{what} field count mismatch"
        )));
    }
    definitions
        .iter()
        .zip(fields.iter().copied())
        .map(|(definition, value)| {
            let name = definition
                .name
                .to_opt()
                .map(|name| name.data(db).to_string())
                .ok_or_else(|| {
                    PageProjectionError::Shape(format!("{what} has an unnamed field"))
                })?;
            Ok((name, value))
        })
        .collect()
}

fn named_field<'db>(
    fields: &'db [(String, SemConstId<'db>)],
    name: &str,
    what: &str,
) -> Result<SemConstId<'db>, PageProjectionError> {
    fields
        .iter()
        .find_map(|(candidate, value)| (candidate == name).then_some(*value))
        .ok_or_else(|| PageProjectionError::Shape(format!("{what} has no `{name}` field")))
}

fn read_string(
    db: &DriverDataBase,
    value: SemConstId<'_>,
    what: &str,
) -> Result<String, PageProjectionError> {
    let SemConstValue::Scalar {
        value: SemConstScalar::Bytes(bytes),
        ..
    } = value.value(db)
    else {
        return Err(PageProjectionError::Shape(format!(
            "{what} is not a fixed string"
        )));
    };
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    let bytes = &bytes[first..];
    if bytes.contains(&0) {
        return Err(PageProjectionError::Shape(format!(
            "{what} contains an embedded NUL"
        )));
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|error| PageProjectionError::Shape(format!("{what} is not UTF-8: {error}")))
}

fn read_page_text(
    db: &DriverDataBase,
    value: SemConstId<'_>,
    what: &str,
) -> Result<String, PageProjectionError> {
    let fields = struct_fields(db, value, what)?;
    let length = read_u32(
        db,
        named_field(&fields, "length", what)?,
        &format!("{what}.length"),
    )?;
    if length > 3 {
        return Err(PageProjectionError::Shape(format!(
            "{what}.length {length} exceeds three text pieces"
        )));
    }

    let mut output = String::new();
    for (index, name) in ["first", "second", "third"].iter().enumerate() {
        let piece = read_string(
            db,
            named_field(&fields, name, what)?,
            &format!("{what}.{name}"),
        )?;
        if index < length as usize {
            output.push_str(&piece);
        } else if !piece.is_empty() {
            return Err(PageProjectionError::Shape(format!(
                "{what}.{name} contains text beyond declared length {length}"
            )));
        }
    }
    Ok(output)
}

fn read_u32(
    db: &DriverDataBase,
    value: SemConstId<'_>,
    what: &str,
) -> Result<u32, PageProjectionError> {
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value },
        ..
    } = value.value(db)
    else {
        return Err(PageProjectionError::Shape(format!(
            "{what} is not an integer"
        )));
    };
    u32::try_from(value).map_err(|_| PageProjectionError::Shape(format!("{what} is not a u32")))
}

fn read_bool(
    db: &DriverDataBase,
    value: SemConstId<'_>,
    what: &str,
) -> Result<bool, PageProjectionError> {
    let SemConstValue::Scalar {
        value: SemConstScalar::Bool(value),
        ..
    } = value.value(db)
    else {
        return Err(PageProjectionError::Shape(format!("{what} is not a bool")));
    };
    Ok(value)
}

fn enum_parts<'db>(
    db: &'db DriverDataBase,
    value: SemConstId<'db>,
    what: &str,
) -> Result<(String, Box<[SemConstId<'db>]>), PageProjectionError> {
    let SemConstValue::Enum {
        ty,
        variant,
        fields,
    } = value.value(db)
    else {
        return Err(PageProjectionError::Shape(format!("{what} is not an enum")));
    };
    let adt = ty
        .adt_def(db)
        .ok_or_else(|| PageProjectionError::Shape(format!("{what} is not an enum")))?;
    let AdtRef::Enum(definition) = adt.adt_ref(db) else {
        return Err(PageProjectionError::Shape(format!("{what} is not an enum")));
    };
    let name = EnumVariant::new(definition, variant.0 as usize)
        .name(db)
        .ok_or_else(|| PageProjectionError::Shape(format!("{what} variant has no name")))?
        .to_owned();
    Ok((name, fields))
}

fn one_field<'db>(
    fields: Box<[SemConstId<'db>]>,
    what: &str,
) -> Result<SemConstId<'db>, PageProjectionError> {
    let [value] = fields.as_ref() else {
        return Err(PageProjectionError::Shape(format!(
            "{what} must carry exactly one field"
        )));
    };
    Ok(*value)
}

fn no_fields(fields: Box<[SemConstId<'_>]>, what: &str) -> Result<(), PageProjectionError> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(PageProjectionError::Shape(format!(
            "{what} must not carry fields"
        )))
    }
}

fn read_element(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<PageElement, PageProjectionError> {
    let (name, fields) = enum_parts(db, value, "PageElement")?;
    no_fields(fields, "PageElement")?;
    Ok(match name.as_str() {
        "Header" => PageElement::Header,
        "Main" => PageElement::Main,
        "Div" => PageElement::Div,
        "Figure" => PageElement::Figure,
        "Figcaption" => PageElement::Figcaption,
        "Span" => PageElement::Span,
        "Bold" => PageElement::Bold,
        "Paragraph" => PageElement::Paragraph,
        "Code" => PageElement::Code,
        "Section" => PageElement::Section,
        "Footer" => PageElement::Footer,
        "Heading1" => PageElement::Heading1,
        "Heading2" => PageElement::Heading2,
        "Input" => PageElement::Input,
        "Label" => PageElement::Label,
        "UnorderedList" => PageElement::UnorderedList,
        "ListItem" => PageElement::ListItem,
        "Button" => PageElement::Button,
        "Template" => PageElement::Template,
        "Strong" => PageElement::Strong,
        "Pre" => PageElement::Pre,
        "Anchor" => PageElement::Anchor,
        "FeComponent" => PageElement::FeComponent,
        _ => {
            return Err(PageProjectionError::Shape(format!(
                "unknown PageElement variant `{name}`"
            )));
        }
    })
}

fn read_attribute_kind(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<PageAttributeKind, PageProjectionError> {
    let (name, fields) = enum_parts(db, value, "PageAttributeKind")?;
    no_fields(fields, "PageAttributeKind")?;
    Ok(match name.as_str() {
        "Id" => PageAttributeKind::Id,
        "LocalId" => PageAttributeKind::LocalId,
        "Class" => PageAttributeKind::Class,
        "Role" => PageAttributeKind::Role,
        "AriaLabel" => PageAttributeKind::AriaLabel,
        "AriaModal" => PageAttributeKind::AriaModal,
        "InputType" => PageAttributeKind::InputType,
        "For" => PageAttributeKind::For,
        "LocalFor" => PageAttributeKind::LocalFor,
        "Title" => PageAttributeKind::Title,
        "Placeholder" => PageAttributeKind::Placeholder,
        "Autocomplete" => PageAttributeKind::Autocomplete,
        "Target" => PageAttributeKind::Target,
        "Rel" => PageAttributeKind::Rel,
        "Href" => PageAttributeKind::Href,
        "Hidden" => PageAttributeKind::Hidden,
        "Action" => PageAttributeKind::Action,
        "Node" => PageAttributeKind::Node,
        "View" => PageAttributeKind::View,
        "Template" => PageAttributeKind::Template,
        "ClassToken" => PageAttributeKind::ClassToken,
        "Publish" => PageAttributeKind::Publish,
        _ => {
            return Err(PageProjectionError::Shape(format!(
                "unknown PageAttributeKind variant `{name}`"
            )));
        }
    })
}

fn read_attribute(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<ProjectedPageAttribute, PageProjectionError> {
    let fields = struct_fields(db, value, "PageAttribute")?;
    Ok(ProjectedPageAttribute {
        kind: read_attribute_kind(db, named_field(&fields, "kind", "PageAttribute")?)?,
        text: read_page_text(
            db,
            named_field(&fields, "text", "PageAttribute")?,
            "PageAttribute.text",
        )?,
        number: read_u32(
            db,
            named_field(&fields, "number", "PageAttribute")?,
            "PageAttribute.number",
        )?,
        enabled: read_bool(
            db,
            named_field(&fields, "enabled", "PageAttribute")?,
            "PageAttribute.enabled",
        )?,
    })
}

fn read_render(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<ProjectedPageRender, PageProjectionError> {
    let fields = struct_fields(db, value, "PageRender")?;
    Ok(ProjectedPageRender {
        source: read_page_text(
            db,
            named_field(&fields, "source", "PageRender")?,
            "PageRender.source",
        )?,
        entry: read_string(
            db,
            named_field(&fields, "entry", "PageRender")?,
            "PageRender.entry",
        )?,
        wgsl_action: read_u32(
            db,
            named_field(&fields, "wgsl_action", "PageRender")?,
            "PageRender.wgsl_action",
        )?,
        wasm_action: read_u32(
            db,
            named_field(&fields, "wasm_action", "PageRender")?,
            "PageRender.wasm_action",
        )?,
        manifest_action: read_u32(
            db,
            named_field(&fields, "manifest_action", "PageRender")?,
            "PageRender.manifest_action",
        )?,
        sequenced: read_bool(
            db,
            named_field(&fields, "sequenced", "PageRender")?,
            "PageRender.sequenced",
        )?,
        sequence: read_u32(
            db,
            named_field(&fields, "sequence", "PageRender")?,
            "PageRender.sequence",
        )?,
    })
}

fn read_component(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<ProjectedPageComponent, PageProjectionError> {
    let fields = struct_fields(db, value, "PageComponent")?;
    Ok(ProjectedPageComponent {
        source: read_page_text(
            db,
            named_field(&fields, "source", "PageComponent")?,
            "PageComponent.source",
        )?,
        mount: read_string(
            db,
            named_field(&fields, "mount", "PageComponent")?,
            "PageComponent.mount",
        )?,
    })
}

fn read_op(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<PageProjectionOp, PageProjectionError> {
    let (name, fields) = enum_parts(db, value, "PageOp")?;
    Ok(match name.as_str() {
        "Open" => PageProjectionOp::Open(read_element(db, one_field(fields, "PageOp::Open")?)?),
        "Attribute" => PageProjectionOp::Attribute(read_attribute(
            db,
            one_field(fields, "PageOp::Attribute")?,
        )?),
        "Text" => PageProjectionOp::Text(read_page_text(
            db,
            one_field(fields, "PageOp::Text")?,
            "PageOp::Text",
        )?),
        "Close" => {
            no_fields(fields, "PageOp::Close")?;
            PageProjectionOp::Close
        }
        "Render" => {
            PageProjectionOp::Render(read_render(db, one_field(fields, "PageOp::Render")?)?)
        }
        "Component" => PageProjectionOp::Component(read_component(
            db,
            one_field(fields, "PageOp::Component")?,
        )?),
        _ => {
            return Err(PageProjectionError::Shape(format!(
                "unknown PageOp variant `{name}`"
            )));
        }
    })
}

fn walk_page(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<(String, Vec<PageProjectionOp>), PageProjectionError> {
    let fields = struct_fields(db, value, "Page")?;
    let title = read_page_text(db, named_field(&fields, "title", "Page")?, "Page.title")?;
    let body = walk_body(db, &fields, "Page")?;
    Ok((title, body))
}

fn walk_component(
    db: &DriverDataBase,
    value: SemConstId<'_>,
) -> Result<Vec<PageProjectionOp>, PageProjectionError> {
    let fields = struct_fields(db, value, "ComponentView")?;
    walk_body(db, &fields, "ComponentView")
}

fn walk_body(
    db: &DriverDataBase,
    fields: &[(String, SemConstId<'_>)],
    what: &str,
) -> Result<Vec<PageProjectionOp>, PageProjectionError> {
    let length = read_u32(
        db,
        named_field(fields, "length", what)?,
        &format!("{what}.length"),
    )? as usize;
    let body = named_field(fields, "body", what)?;
    let SemConstValue::Array { elems, .. } = body.value(db) else {
        return Err(PageProjectionError::Shape(format!(
            "{what}.body is not an array"
        )));
    };
    if length > elems.len() {
        return Err(PageProjectionError::Shape(format!(
            "{what}.length {length} exceeds its body capacity {}",
            elems.len()
        )));
    }
    elems[..length]
        .iter()
        .copied()
        .map(|value| read_op(db, value))
        .collect::<Result<Vec<_>, _>>()
}
