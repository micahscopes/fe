//! Compiler-owned description of typed actor messages derived from Wasm exports.
//!
//! This is deliberately below any future Fe actor syntax: callers name lanes and
//! group the flat Wasm parameters into record fields, while this module measures
//! the compiled artifact and refuses metadata that does not match its ABI.

use std::{collections::BTreeMap, fmt};

use serde_json::{Value, json};
use wasmparser::{ExternalKind, Payload, TypeRef, ValType};

pub const ACTOR_PROTOCOL: &str = "fe-demo-actor";
pub const ACTOR_PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorScalar {
    I32,
    F32,
}

impl ActorScalar {
    fn wasm_type(self) -> ValType {
        match self {
            Self::I32 => ValType::I32,
            Self::F32 => ValType::F32,
        }
    }

    fn manifest_kind(self) -> &'static str {
        match self {
            Self::I32 => "i32-array",
            Self::F32 => "f32-array",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ActorRecordField<'a> {
    pub name: &'a str,
    pub scalar: ActorScalar,
    pub length: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ActorLaneSpec<'a> {
    pub lane: &'a str,
    pub export: &'a str,
    pub request: &'a [ActorRecordField<'a>],
    pub result: ActorScalar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorManifestError(String);

impl fmt::Display for ActorManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ActorManifestError {}

fn error(message: impl Into<String>) -> ActorManifestError {
    ActorManifestError(message.into())
}

fn valid_lane_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 || !name.is_ascii() {
        return false;
    }
    let mut previous_separator = false;
    for (index, byte) in name.bytes().enumerate() {
        let alphanumeric = byte.is_ascii_lowercase() || (index > 0 && byte.is_ascii_digit());
        let separator = matches!(byte, b'.' | b'_' | b'-');
        if !alphanumeric && !separator || separator && (index == 0 || previous_separator) {
            return false;
        }
        previous_separator = separator;
    }
    !previous_separator
}

fn valid_field_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.is_ascii()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || byte == b'_' || (index > 0 && byte.is_ascii_digit())
        })
        && name.as_bytes()[0].is_ascii_lowercase()
}

/// Build a versioned actor manifest from the actual signatures of named Wasm
/// exports. Request fields map consecutive homogeneous scalar parameters into
/// typed arrays; all results map to one homogeneous typed array.
pub fn actor_manifest_from_wasm_exports(
    wasm: &[u8],
    lanes: &[ActorLaneSpec<'_>],
) -> Result<Value, ActorManifestError> {
    wasmparser::validate(wasm).map_err(|e| error(format!("invalid Wasm module: {e}")))?;
    if lanes.is_empty() {
        return Err(error("actor manifest requires at least one lane"));
    }

    let signatures = exported_function_signatures(wasm)?;
    let mut manifest_lanes = serde_json::Map::new();
    for lane in lanes {
        if !valid_lane_name(lane.lane) || lane.export.is_empty() {
            return Err(error(format!("invalid actor lane name `{}`", lane.lane)));
        }
        if manifest_lanes.contains_key(lane.lane) {
            return Err(error(format!("duplicate actor lane `{}`", lane.lane)));
        }
        let (params, results) = signatures.get(lane.export).ok_or_else(|| {
            error(format!(
                "Wasm function export `{}` was not found",
                lane.export
            ))
        })?;

        let mut expected_params = Vec::new();
        let mut fields = serde_json::Map::new();
        for field in lane.request {
            if !valid_field_name(field.name) || field.length == 0 {
                return Err(error(format!(
                    "actor lane `{}` request field `{}` requires a lowercase identifier and positive length",
                    lane.lane, field.name
                )));
            }
            if fields.contains_key(field.name) {
                return Err(error(format!("duplicate request field `{}`", field.name)));
            }
            expected_params.extend(std::iter::repeat_n(field.scalar.wasm_type(), field.length));
            fields.insert(
                field.name.to_owned(),
                json!({
                    "kind": field.scalar.manifest_kind(),
                    "length": field.length,
                }),
            );
        }
        if params != &expected_params {
            return Err(error(format!(
                "actor lane `{}` request mapping does not match Wasm export `{}` parameters: expected {expected_params:?}, found {params:?}",
                lane.lane, lane.export
            )));
        }
        if results.is_empty() || results.iter().any(|ty| *ty != lane.result.wasm_type()) {
            return Err(error(format!(
                "actor lane `{}` result mapping requires one or more homogeneous {:?} results from Wasm export `{}`; found {results:?}",
                lane.lane, lane.result, lane.export
            )));
        }
        manifest_lanes.insert(
            lane.lane.to_owned(),
            json!({
                "request": { "kind": "record", "fields": fields },
                "result": {
                    "kind": lane.result.manifest_kind(),
                    "length": results.len(),
                },
            }),
        );
    }

    Ok(json!({
        "protocol": ACTOR_PROTOCOL,
        "version": ACTOR_PROTOCOL_VERSION,
        "lanes": manifest_lanes,
    }))
}

fn exported_function_signatures(
    wasm: &[u8],
) -> Result<BTreeMap<String, (Vec<ValType>, Vec<ValType>)>, ActorManifestError> {
    let mut types = Vec::new();
    let mut function_types = Vec::new();
    let mut imported_functions = Vec::new();
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.map_err(|e| error(format!("invalid Wasm payload: {e}")))? {
            Payload::TypeSection(reader) => {
                for group in reader {
                    for subtype in group.map_err(|e| error(e.to_string()))?.into_types() {
                        let function = subtype.unwrap_func();
                        types.push((function.params().to_vec(), function.results().to_vec()));
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if let TypeRef::Func(index) = import.map_err(|e| error(e.to_string()))?.ty {
                        imported_functions.push(index);
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for index in reader {
                    function_types.push(index.map_err(|e| error(e.to_string()))?);
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(|e| error(e.to_string()))?;
                    if export.kind == ExternalKind::Func {
                        exports.push((export.name.to_owned(), export.index));
                    }
                }
            }
            _ => {}
        }
    }
    let mut answer = BTreeMap::new();
    for (name, function_index) in exports {
        let type_index = if let Some(index) = imported_functions.get(function_index as usize) {
            *index
        } else {
            let defined = function_index as usize - imported_functions.len();
            *function_types.get(defined).ok_or_else(|| {
                error(format!(
                    "function export `{name}` has no function-section type"
                ))
            })?
        };
        let signature = types.get(type_index as usize).ok_or_else(|| {
            error(format!(
                "function export `{name}` refers to missing type {type_index}"
            ))
        })?;
        answer.insert(name, signature.clone());
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: u8, payload: Vec<u8>, module: &mut Vec<u8>) {
        module.push(id);
        module.push(payload.len() as u8);
        module.extend(payload);
    }

    fn one_export(params: &[u8], results: &[u8]) -> Vec<u8> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        let mut ty = vec![1, 0x60, params.len() as u8];
        ty.extend(params);
        ty.push(results.len() as u8);
        ty.extend(results);
        section(1, ty, &mut module);
        section(3, vec![1, 0], &mut module);
        section(
            7,
            vec![1, 6, b'u', b'p', b'd', b'a', b't', b'e', 0, 0],
            &mut module,
        );
        let mut instructions = vec![0]; // zero local declarations
        for result in results {
            match result {
                0x7f => instructions.extend([0x41, 0]),
                0x7e => instructions.extend([0x42, 0]),
                0x7d => instructions.extend([0x43, 0, 0, 0, 0]),
                0x7c => instructions.extend([0x44, 0, 0, 0, 0, 0, 0, 0, 0]),
                other => panic!("unsupported test result type {other:#x}"),
            }
        }
        instructions.push(0x0b);
        let mut code = vec![1, instructions.len() as u8];
        code.extend(instructions);
        section(10, code, &mut module);
        module
    }

    #[test]
    fn exact_typed_record_manifest_comes_from_export_signature() {
        let wasm = one_export(&[0x7f, 0x7f, 0x7d], &[0x7d, 0x7d]);
        let fields = [
            ActorRecordField {
                name: "coords",
                scalar: ActorScalar::I32,
                length: 2,
            },
            ActorRecordField {
                name: "scale",
                scalar: ActorScalar::F32,
                length: 1,
            },
        ];
        let manifest = actor_manifest_from_wasm_exports(
            &wasm,
            &[ActorLaneSpec {
                lane: "render",
                export: "update",
                request: &fields,
                result: ActorScalar::F32,
            }],
        )
        .unwrap();
        assert_eq!(
            manifest,
            json!({
                "protocol": "fe-demo-actor",
                "version": 2,
                "lanes": { "render": {
                    "request": { "kind": "record", "fields": {
                        "coords": { "kind": "i32-array", "length": 2 },
                        "scale": { "kind": "f32-array", "length": 1 }
                    }},
                    "result": { "kind": "f32-array", "length": 2 }
                }}
            })
        );
    }

    #[test]
    fn rejects_request_mapping_that_disagrees_with_wasm() {
        let wasm = one_export(&[0x7f, 0x7d], &[0x7f]);
        let fields = [ActorRecordField {
            name: "args",
            scalar: ActorScalar::I32,
            length: 2,
        }];
        let error = actor_manifest_from_wasm_exports(
            &wasm,
            &[ActorLaneSpec {
                lane: "render",
                export: "update",
                request: &fields,
                result: ActorScalar::I32,
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    fn rejects_unsupported_or_heterogeneous_result_abi() {
        let fields = [ActorRecordField {
            name: "args",
            scalar: ActorScalar::I32,
            length: 1,
        }];
        for results in [&[0x7e][..], &[0x7f, 0x7d][..]] {
            let error = actor_manifest_from_wasm_exports(
                &one_export(&[0x7f], results),
                &[ActorLaneSpec {
                    lane: "verify",
                    export: "update",
                    request: &fields,
                    result: ActorScalar::I32,
                }],
            )
            .unwrap_err();
            assert!(error.to_string().contains("homogeneous I32"));
        }
    }

    #[test]
    fn rejects_missing_exports_and_duplicate_lanes() {
        let wasm = one_export(&[0x7f], &[0x7f]);
        let fields = [ActorRecordField {
            name: "args",
            scalar: ActorScalar::I32,
            length: 1,
        }];
        let lane = ActorLaneSpec {
            lane: "render",
            export: "missing",
            request: &fields,
            result: ActorScalar::I32,
        };
        assert!(
            actor_manifest_from_wasm_exports(&wasm, &[lane])
                .unwrap_err()
                .to_string()
                .contains("was not found")
        );
        let lane = ActorLaneSpec {
            lane: "render",
            export: "update",
            request: &fields,
            result: ActorScalar::I32,
        };
        assert!(
            actor_manifest_from_wasm_exports(&wasm, &[lane, lane])
                .unwrap_err()
                .to_string()
                .contains("duplicate actor lane")
        );
    }

    #[test]
    fn rejects_names_outside_the_wire_grammar() {
        let wasm = one_export(&[0x7f], &[0x7f]);
        let valid_fields = [ActorRecordField {
            name: "args",
            scalar: ActorScalar::I32,
            length: 1,
        }];
        for lane in ["Render", "2render", "render..now", "render_"] {
            let error = actor_manifest_from_wasm_exports(
                &wasm,
                &[ActorLaneSpec {
                    lane,
                    export: "update",
                    request: &valid_fields,
                    result: ActorScalar::I32,
                }],
            )
            .unwrap_err();
            assert!(error.to_string().contains("invalid actor lane"));
        }
        let invalid_fields = [ActorRecordField {
            name: "bad-name",
            scalar: ActorScalar::I32,
            length: 1,
        }];
        let error = actor_manifest_from_wasm_exports(
            &wasm,
            &[ActorLaneSpec {
                lane: "render",
                export: "update",
                request: &invalid_fields,
                result: ActorScalar::I32,
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("lowercase identifier"));
    }
}
