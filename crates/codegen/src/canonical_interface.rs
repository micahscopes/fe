//! Pure metadata and wasm32 layout model for Fe browser interfaces.
//!
//! This module deliberately does not inspect semantic types, emit Wasm memory,
//! or allocate storage. Those later stages feed validated declarations into
//! [`CanonicalInterfaceManifest::build`] and consume the resulting layouts.

use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Serialize};

pub const CANONICAL_INTERFACE_PROTOCOL: &str = "fe-canonical-browser-interface";
pub const CANONICAL_INTERFACE_VERSION: u32 = 1;

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
    Record(Vec<CanonicalField>),
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
pub struct CanonicalLaneDecl {
    pub name: String,
    pub export: String,
    pub request: CanonicalType,
    pub response: CanonicalType,
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
    pub export: String,
    pub request: CanonicalLayout,
    pub response: CanonicalLayout,
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
    Record {
        fields: Vec<CanonicalFieldLayout>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalFieldLayout {
    pub name: String,
    pub offset: u32,
    pub layout: CanonicalLayout,
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
            validate_export_name(&declaration.export)?;
            if !lane_names.insert(declaration.name.clone()) {
                return Err(error(format!(
                    "duplicate canonical lane `{}`",
                    declaration.name
                )));
            }
            if reserved.contains(&declaration.export.as_str()) {
                return Err(error(format!(
                    "canonical lane export `{}` collides with a reserved ABI export",
                    declaration.export
                )));
            }
            if !exports.insert(declaration.export.clone()) {
                return Err(error(format!(
                    "duplicate canonical lane export `{}`",
                    declaration.export
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
    })
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
            byte.is_ascii_lowercase()
                || (index > 0 && (byte == b'_' || byte.is_ascii_digit()))
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
            export: "render_message".to_owned(),
            request,
            response: CanonicalType::U32,
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
            export: export.to_owned(),
            request,
            response: CanonicalType::U32,
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
                export: format!("export_{index}"),
                request: ty,
                response: CanonicalType::U8,
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
}
