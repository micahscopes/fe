use super::super::{TypedFactRelationColumnName, TypedFactRelationName};
use super::descriptor::TypedFactRelationSchema;

pub fn typed_fact_relation_schemas() -> &'static [TypedFactRelationSchema] {
    TYPED_FACT_RELATION_SCHEMAS
}

pub(in crate::facts) fn typed_fact_relation_schema_for_raw_name(
    name: &str,
) -> Option<TypedFactRelationSchema> {
    let name = TypedFactRelationName::from_str(name)?;
    Some(typed_fact_relation_schema_for_name(name))
}

pub(in crate::facts) fn columns_match(
    actual: &[String],
    expected: &[TypedFactRelationColumnName],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| actual == expected.as_str())
}

pub(super) fn typed_fact_relation_schema_for_name(
    name: TypedFactRelationName,
) -> TypedFactRelationSchema {
    TYPED_FACT_RELATION_SCHEMAS
        .iter()
        .copied()
        .find(|schema| schema.name() == name)
        .expect("typed fact relation name should have a schema descriptor")
}

const TYPED_FACT_RELATION_SCHEMAS: &[TypedFactRelationSchema] = &[
    TypedFactRelationSchema::new(
        TypedFactRelationName::OriginNode,
        &[
            TypedFactRelationColumnName::Id,
            TypedFactRelationColumnName::Kind,
            TypedFactRelationColumnName::OwnerKey,
            TypedFactRelationColumnName::LocalKey,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::OriginLink,
        &[
            TypedFactRelationColumnName::From,
            TypedFactRelationColumnName::To,
            TypedFactRelationColumnName::Kind,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::SourceSpan,
        &[
            TypedFactRelationColumnName::Origin,
            TypedFactRelationColumnName::SpanKind,
            TypedFactRelationColumnName::File,
            TypedFactRelationColumnName::StartByte,
            TypedFactRelationColumnName::EndByte,
            TypedFactRelationColumnName::StartLine,
            TypedFactRelationColumnName::StartCol,
            TypedFactRelationColumnName::EndLine,
            TypedFactRelationColumnName::EndCol,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeNode,
        &[
            TypedFactRelationColumnName::Id,
            TypedFactRelationColumnName::SourceId,
            TypedFactRelationColumnName::StableKey,
            TypedFactRelationColumnName::Kind,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeField,
        &[
            TypedFactRelationColumnName::Node,
            TypedFactRelationColumnName::Dimension,
            TypedFactRelationColumnName::Name,
            TypedFactRelationColumnName::Value,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeChild,
        &[
            TypedFactRelationColumnName::Parent,
            TypedFactRelationColumnName::Child,
            TypedFactRelationColumnName::Label,
            TypedFactRelationColumnName::Order,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeEdge,
        &[
            TypedFactRelationColumnName::From,
            TypedFactRelationColumnName::To,
            TypedFactRelationColumnName::Label,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::TraceEvent,
        &[
            TypedFactRelationColumnName::Node,
            TypedFactRelationColumnName::EventKind,
            TypedFactRelationColumnName::Value,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::DataFlow,
        &[
            TypedFactRelationColumnName::Source,
            TypedFactRelationColumnName::Target,
            TypedFactRelationColumnName::Kind,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeHash,
        &[
            TypedFactRelationColumnName::Node,
            TypedFactRelationColumnName::Scope,
            TypedFactRelationColumnName::Dimension,
            TypedFactRelationColumnName::DigestHex,
        ],
    ),
];
