//! Admission of structural function syntax produced by ordinary Fe CTFE.
//! This boundary owns descriptor identity, scope and bounded references.
//! Ordinary source and semantic checking own the generated language semantics.

use common::InputDb;
use hir::{
    analysis::{
        semantic::{SemConstId, SemConstScalar, SemConstValue, StructuralHandle},
        ty::{
            adt_def::AdtRef,
            corelib::resolve_lib_type_path,
            ty_def::{PrimTy, TyBase, TyData, TyId},
        },
    },
    span::LazySpan,
};

use super::RecordInput;
use crate::DriverDataBase;

#[derive(Debug, PartialEq, Eq)]
pub enum ConstructionError {
    Protocol(&'static str),
    Authority,
    Limit,
    Output(String),
}

fn protocol(message: &'static str) -> ConstructionError {
    ConstructionError::Protocol(message)
}

impl<'db> RecordInput<'db> {
    fn designated(&self, ty: TyId<'db>, name: &str) -> bool {
        resolve_lib_type_path(self.db, self.owner.scope(), &format!("core::meta::{name}"))
            .is_some_and(|expected| expected.base_ty(self.db) == ty.base_ty(self.db))
    }

    fn variant(
        &self,
        value: SemConstId<'db>,
        name: &str,
    ) -> Result<(u16, Box<[SemConstId<'db>]>), ConstructionError> {
        let SemConstValue::Enum {
            ty,
            variant,
            fields,
        } = value.value(self.db).clone()
        else {
            return Err(protocol("expected declaration syntax variant"));
        };
        if !self.designated(ty, name) {
            return Err(protocol("unexpected declaration syntax type"));
        }
        Ok((variant.0, fields))
    }

    fn field_index(
        &self,
        value: SemConstId<'db>,
        type_handle: bool,
    ) -> Result<usize, ConstructionError> {
        let SemConstValue::Structural { ty, handle } = value.value(self.db).clone() else {
            return Err(ConstructionError::Authority);
        };
        let expected = if type_handle { "FieldType" } else { "Field" };
        if !self.designated(ty, expected) {
            return Err(ConstructionError::Authority);
        }
        let (owner, index) = match (handle, type_handle) {
            (StructuralHandle::Field { owner, index }, false)
            | (StructuralHandle::FieldType { owner, index }, true) => (owner, index),
            _ => return Err(ConstructionError::Authority),
        };
        if owner != self.owner || index >= self.fields.len() {
            return Err(ConstructionError::Authority);
        }
        Ok(index)
    }

    fn declaration_type(&self, value: SemConstId<'db>) -> Result<String, ConstructionError> {
        let (variant, fields) = self.variant(value, "DeclarationType")?;
        Ok(match (variant, fields.as_ref()) {
            (0, []) => "()".to_owned(),
            (1, []) => "bool".to_owned(),
            (2, []) => "u64".to_owned(),
            (3, []) => "u256".to_owned(),
            (4, [record]) => {
                let SemConstValue::Structural {
                    ty,
                    handle: StructuralHandle::Record(owner),
                } = record.value(self.db).clone()
                else {
                    return Err(ConstructionError::Authority);
                };
                if owner != self.owner || !self.designated(ty, "Record") {
                    return Err(ConstructionError::Authority);
                }
                self.owner
                    .name(self.db)
                    .to_opt()
                    .ok_or(protocol("record name is missing"))?
                    .data(self.db)
                    .to_string()
            }
            (5, [field]) => {
                let index = self.field_index(*field, true)?;
                let ty = self.fields[index].ty;
                match ty.data(self.db) {
                    TyData::TyBase(TyBase::Prim(PrimTy::Bool)) => "bool".to_owned(),
                    TyData::TyBase(TyBase::Prim(PrimTy::U64)) => "u64".to_owned(),
                    TyData::TyBase(TyBase::Prim(PrimTy::U256)) => "u256".to_owned(),
                    TyData::TyBase(TyBase::Adt(adt))
                        if adt.params(self.db).is_empty()
                            && matches!(adt.adt_ref(self.db), AdtRef::Struct(_)) =>
                    {
                        // Output is inserted in the defining record's module.
                        // Reuse its checked field type spelling, including local
                        // aliases and qualified paths, instead of inventing a
                        // possibly ambiguous name from the nominal definition.
                        let span = self
                            .owner
                            .span()
                            .fields()
                            .field(index)
                            .ty()
                            .resolve(self.db)
                            .ok_or(protocol("field type source span is missing"))?;
                        let range: std::ops::Range<usize> = span.range.into();
                        span.file
                            .text(self.db)
                            .get(range)
                            .ok_or(protocol("field type source span is invalid"))?
                            .to_owned()
                    }
                    _ => {
                        return Err(protocol(
                            "field type is outside the admitted declaration surface",
                        ));
                    }
                }
            }
            (6, []) => "*u8".to_owned(),
            (7, [path]) => self.source_path(*path)?,
            _ => return Err(protocol("invalid declaration type")),
        })
    }

    fn text(&self, value: SemConstId<'db>) -> Result<String, ConstructionError> {
        let SemConstValue::Scalar {
            value: SemConstScalar::Bytes(bytes),
            ..
        } = value.value(self.db).clone()
        else {
            return Err(protocol("expected fixed string data"));
        };
        let start = bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(bytes.len());
        std::str::from_utf8(&bytes[start..])
            .map(str::to_owned)
            .map_err(|_| protocol("source name is not UTF-8"))
    }

    fn source_path(&self, value: SemConstId<'db>) -> Result<String, ConstructionError> {
        let path = self.text(value)?;
        let parts = path.split("::").collect::<Vec<_>>();
        if parts.len() < 2
            || !matches!(parts[0], "ingot" | "core" | "std")
            || !parts.iter().all(|part| identifier(part))
        {
            return Err(protocol(
                "expected an explicit ingot, core or std source path",
            ));
        }
        Ok(path)
    }

    fn optional(
        &self,
        value: SemConstId<'db>,
    ) -> Result<Option<SemConstId<'db>>, ConstructionError> {
        let SemConstValue::Enum {
            ty,
            variant,
            fields,
        } = value.value(self.db).clone()
        else {
            return Err(protocol("expected core Option"));
        };
        let option = resolve_lib_type_path(self.db, self.owner.scope(), "core::Option")
            .ok_or(protocol("builtin Option is missing"))?;
        if ty.base_ty(self.db) != option.base_ty(self.db) {
            return Err(protocol("expected core Option"));
        }
        match (variant.0, fields.as_ref()) {
            (0, [value]) => Ok(Some(*value)),
            (1, []) => Ok(None),
            _ => Err(protocol("invalid optional syntax")),
        }
    }

    fn prefix(
        &self,
        value: SemConstId<'db>,
        limit: usize,
    ) -> Result<Vec<SemConstId<'db>>, ConstructionError> {
        let SemConstValue::Array { elems, .. } = value.value(self.db).clone() else {
            return Err(protocol("expected optional syntax array"));
        };
        if elems.len() > limit {
            return Err(ConstructionError::Limit);
        }
        let mut result = Vec::new();
        let mut ended = false;
        for element in elems {
            match self.optional(element)? {
                Some(_) if ended => return Err(protocol("call argument slots must form a prefix")),
                Some(value) => result.push(value),
                None => ended = true,
            }
        }
        Ok(result)
    }

    fn call(&self, value: SemConstId<'db>, names: &[String]) -> Result<String, ConstructionError> {
        let SemConstValue::Struct { ty, fields } = value.value(self.db).clone() else {
            return Err(protocol("expected FunctionCall"));
        };
        if !self.designated(ty, "FunctionCall") || fields.len() != 3 {
            return Err(protocol("expected core FunctionCall"));
        }
        let mut path = self.source_path(fields[0])?;
        let types = self
            .prefix(fields[1], 2)?
            .into_iter()
            .map(|ty| self.declaration_type(ty))
            .collect::<Result<Vec<_>, _>>()?;
        if !types.is_empty() {
            path.push_str(&format!("<{}>", types.join(", ")));
        }
        let arguments = self
            .prefix(fields[2], 4)?
            .into_iter()
            .map(|value| {
                let SemConstValue::Struct { ty, fields } = value.value(self.db).clone() else {
                    return Err(protocol("expected CallArgument"));
                };
                if !self.designated(ty, "CallArgument") || fields.len() != 2 {
                    return Err(protocol("expected core CallArgument"));
                }
                let expression = names
                    .get(index(self.db, fields[1])?)
                    .ok_or(protocol("body reference must point backward"))?;
                match self.optional(fields[0])? {
                    Some(label) => {
                        let label = self.text(label)?;
                        if !identifier(&label) {
                            return Err(protocol("call label must be an identifier"));
                        }
                        Ok(format!("{label}: {expression}"))
                    }
                    None => Ok(expression.clone()),
                }
            })
            .collect::<Result<Vec<_>, ConstructionError>>()?;
        Ok(format!("{path}({})", arguments.join(", ")))
    }

    fn declaration_name(&self, value: SemConstId<'db>) -> Result<String, ConstructionError> {
        let (variant, fields) = self.variant(value, "DeclarationName")?;
        let mut name = match (variant, fields.as_ref()) {
            (0, [text]) | (1, [text, _]) => self.text(*text)?,
            _ => return Err(protocol("invalid declaration name")),
        };
        if variant == 1 {
            let field = self.fields[self.field_index(fields[1], false)?]
                .name
                .data(self.db);
            if field.len() + name.len() > 256 {
                return Err(ConstructionError::Limit);
            }
            name.push_str(field);
        }
        if name.len() > 256 || !identifier(&name) {
            return Err(protocol("generated name must be an identifier"));
        }
        Ok(name)
    }
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
}

fn index(db: &DriverDataBase, value: SemConstId<'_>) -> Result<usize, ConstructionError> {
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value },
        ..
    } = value.value(db).clone()
    else {
        return Err(protocol("expected an index"));
    };
    usize::try_from(value).map_err(|_| ConstructionError::Limit)
}

fn flag(db: &DriverDataBase, value: SemConstId<'_>) -> Result<bool, ConstructionError> {
    match value.value(db).clone() {
        SemConstValue::Scalar {
            value: SemConstScalar::Bool(value),
            ..
        } => Ok(value),
        _ => Err(protocol("expected a boolean flag")),
    }
}

/// Render a complete function for insertion in the authorized owner's module.
/// All expression nodes are checked, including nodes unused by the result.
pub fn render_function<'db>(
    input: &RecordInput<'db>,
    value: SemConstId<'db>,
) -> Result<String, ConstructionError> {
    render_function_in(input, value, false)
}

fn render_function_in<'db>(
    input: &RecordInput<'db>,
    value: SemConstId<'db>,
    method: bool,
) -> Result<String, ConstructionError> {
    let db = input.db;
    let SemConstValue::Struct { ty, fields } = value.value(db).clone() else {
        return Err(protocol("expected a function definition"));
    };
    if !input.designated(ty, "FunctionDefinition") || fields.len() != 7 {
        return Err(protocol("expected core FunctionDefinition"));
    }
    let name = input.declaration_name(fields[0])?;
    let public = if flag(db, fields[1])? { "pub " } else { "" };
    let constant = if flag(db, fields[2])? { "const " } else { "" };
    let SemConstValue::Array {
        elems: parameters, ..
    } = fields[3].value(db).clone()
    else {
        return Err(protocol("expected parameter types"));
    };
    let SemConstValue::Array {
        elems: expressions, ..
    } = fields[5].value(db).clone()
    else {
        return Err(protocol("expected body expressions"));
    };
    if parameters.len() > 8 || expressions.is_empty() || expressions.len() > 512 {
        return Err(ConstructionError::Limit);
    }
    if method {
        if !public.is_empty() || parameters.is_empty() {
            return Err(protocol(
                "trait method requires a receiver and cannot be public",
            ));
        }
        let (variant, _) = input.variant(parameters[0], "DeclarationType")?;
        if variant != 4 {
            return Err(protocol("method receiver must be the authorized record"));
        }
        input.declaration_type(parameters[0])?;
    }
    let parameters = parameters
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            if method && i == 0 {
                return Ok("own self".to_owned());
            }
            Ok(format!(
                "_ __meta_arg_{i}: {}",
                input.declaration_type(*ty)?
            ))
        })
        .collect::<Result<Vec<_>, ConstructionError>>()?;
    let mut output = format!(
        "{public}{constant}fn {name}({}) -> {} {{\n",
        parameters.join(", "),
        input.declaration_type(fields[4])?
    );
    let mut names = Vec::<String>::new();
    let mut ended = false;
    for (i, expression) in expressions.iter().enumerate() {
        let (variant, args) = input.variant(*expression, "BodyExpr")?;
        if variant == 7 && args.is_empty() {
            ended = true;
            continue;
        }
        if ended {
            return Err(protocol("body expressions after End are not allowed"));
        }
        let previous = |value| {
            names
                .get(index(db, value)?)
                .cloned()
                .ok_or(protocol("body reference must point backward"))
        };
        let expression = match (variant, args.as_ref()) {
            (0, [parameter]) => {
                let p = index(db, *parameter)?;
                if p >= parameters.len() {
                    return Err(protocol("parameter index out of bounds"));
                }
                names.push(if method && p == 0 {
                    "self".to_owned()
                } else {
                    format!("__meta_arg_{p}")
                });
                continue;
            }
            (1, [base, field]) => format!(
                "{}.{}",
                previous(*base)?,
                input.fields[input.field_index(*field, false)?]
                    .name
                    .data(db)
            ),
            (2, [value]) => flag(db, *value)?.to_string(),
            (3, [value]) => {
                let SemConstValue::Scalar {
                    ty,
                    value: SemConstScalar::Int { value },
                } = value.value(db).clone()
                else {
                    return Err(protocol("expected u256 literal"));
                };
                if ty != TyId::u256(db) {
                    return Err(protocol("expected u256 literal"));
                }
                format!("({value} as u256)")
            }
            (4, [a, b]) => format!("{} + {}", previous(*a)?, previous(*b)?),
            (5, [call]) => input.call(*call, &names)?,
            (6, []) => "()".to_owned(),
            _ => return Err(protocol("invalid body expression")),
        };
        let local = format!("__meta_node_{i}");
        output.push_str(&format!("let {local} = {expression}\n"));
        names.push(local);
        if output.len() > 65536 {
            return Err(ConstructionError::Limit);
        }
    }
    let result = names
        .get(index(db, fields[6])?)
        .ok_or(protocol("result index out of bounds"))?;
    output.push_str(&format!("{result}\n}}\n"));
    Ok(output)
}

/// Render the same checked function syntax as an owned receiver method, plus
/// Fe-computed constants. No trait-specific policy is implemented here.
pub fn render_implementation<'db>(
    input: &RecordInput<'db>,
    value: SemConstId<'db>,
) -> Result<String, ConstructionError> {
    let db = input.db;
    let SemConstValue::Struct { ty, fields } = value.value(db).clone() else {
        return Err(protocol("expected RecordImplementation"));
    };
    if !input.designated(ty, "RecordImplementation") || fields.len() != 5 {
        return Err(protocol("expected core RecordImplementation"));
    }
    let SemConstValue::Structural {
        ty,
        handle: StructuralHandle::Record(owner),
    } = fields[0].value(db).clone()
    else {
        return Err(ConstructionError::Authority);
    };
    if owner != input.owner || !input.designated(ty, "Record") {
        return Err(ConstructionError::Authority);
    }
    let owner_name = owner
        .name(db)
        .to_opt()
        .ok_or(protocol("record name is missing"))?;
    let mut trait_path = input.source_path(fields[1])?;
    let types = input
        .prefix(fields[2], 2)?
        .into_iter()
        .map(|ty| input.declaration_type(ty))
        .collect::<Result<Vec<_>, _>>()?;
    if !types.is_empty() {
        trait_path.push_str(&format!("<{}>", types.join(", ")));
    }
    let mut output = format!("impl {trait_path} for {} {{\n", owner_name.data(db));
    for constant in input.prefix(fields[3], 2)? {
        let (variant, values) = input.variant(constant, "AssociatedConstant")?;
        if values.len() != 2 {
            return Err(protocol("invalid associated constant"));
        }
        let name = input.declaration_name(values[0])?;
        let (ty, value) = match variant {
            0 => ("bool", flag(db, values[1])?.to_string()),
            1 => {
                let SemConstValue::Scalar {
                    ty,
                    value: SemConstScalar::Int { value },
                } = values[1].value(db).clone()
                else {
                    return Err(protocol("expected u256 constant"));
                };
                if ty != TyId::u256(db) {
                    return Err(protocol("expected u256 constant"));
                }
                ("u256", value.to_string())
            }
            _ => return Err(protocol("invalid associated constant")),
        };
        output.push_str(&format!("const {name}: {ty} = {value}\n"));
    }
    if let Some(method) = input.optional(fields[4])? {
        output.push_str(&render_function_in(input, method, true)?);
    }
    output.push_str("}\n");
    if output.len() > 65536 {
        return Err(ConstructionError::Limit);
    }
    Ok(output)
}

/// Admit one evaluated definition in a fresh output view using ordinary checks.
/// Invocation evaluation and project-wide batching are separate integration work.
pub fn admit_function<'db>(
    input: &RecordInput<'db>,
    value: SemConstId<'db>,
) -> Result<DriverDataBase, ConstructionError> {
    let text = render_function(input, value)?;
    admit_text(input, &text)
}

/// Admit a bounded array of optional complete functions in one output view.
/// Fe decides which slots contain definitions; empty slots carry no declaration.
pub fn admit_functions<'db>(
    input: &RecordInput<'db>,
    value: SemConstId<'db>,
) -> Result<DriverDataBase, ConstructionError> {
    admit_text(input, &render_functions(input, value)?)
}

pub fn render_functions<'db>(
    input: &RecordInput<'db>,
    value: SemConstId<'db>,
) -> Result<String, ConstructionError> {
    let db = input.db;
    let SemConstValue::Array { ty, elems } = value.value(db).clone() else {
        return Err(protocol("expected a function definition array"));
    };
    if elems.len() > 64 {
        return Err(ConstructionError::Limit);
    }
    let option = resolve_lib_type_path(db, input.owner.scope(), "core::Option")
        .ok_or(protocol("builtin Option is missing"))?;
    let element = ty
        .generic_args(db)
        .first()
        .ok_or(protocol("array element type is missing"))?;
    let (base, arguments) = element.decompose_ty_app(db);
    if base != option.base_ty(db)
        || arguments.len() != 1
        || !input.designated(arguments[0], "FunctionDefinition")
    {
        return Err(protocol("expected optional function definitions"));
    }
    let mut text = String::new();
    for element in elems {
        let SemConstValue::Enum {
            ty,
            variant,
            fields,
        } = element.value(db).clone()
        else {
            return Err(protocol("expected an optional function definition"));
        };
        if ty.base_ty(db) != base {
            return Err(protocol("unexpected optional definition type"));
        }
        match (variant.0, fields.as_ref()) {
            (0, [function]) => text.push_str(&render_function(input, *function)?),
            (1, []) => {}
            _ => return Err(protocol("invalid optional function definition")),
        }
        if text.len() > 65536 {
            return Err(ConstructionError::Limit);
        }
    }
    Ok(text)
}

fn admit_text(input: &RecordInput<'_>, text: &str) -> Result<DriverDataBase, ConstructionError> {
    let span = input
        .owner
        .span()
        .resolve(input.db)
        .ok_or(protocol("owner source span is missing"))?;
    let url = span
        .file
        .url(input.db)
        .ok_or(protocol("owner source URL is missing"))?;
    let range: std::ops::Range<usize> = span.range.into();
    let mut source = span.file.text(input.db).clone();
    source.insert_str(range.end, &format!("\n{text}"));
    let mut output = input.db.fork_inputs();
    output.workspace().update(&mut output, url.clone(), source);
    let file = output
        .workspace()
        .get(&output, &url)
        .ok_or(protocol("output file is missing"))?;
    if let Some(diagnostics) = crate::checked_source::diagnostics(&output, file) {
        return Err(ConstructionError::Output(diagnostics));
    }
    Ok(output)
}
