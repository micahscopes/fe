use super::TypedFactRelationError;

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum TypedFactRelationName {
        OriginNode => "origin_node",
        OriginLink => "origin_link",
        SourceSpan => "source_span",
        ShapeNode => "shape_node",
        ShapeField => "shape_field",
        ShapeChild => "shape_child",
        ShapeEdge => "shape_edge",
        TraceEvent => "trace_event",
        DataFlow => "data_flow",
        ShapeHash => "shape_hash",
    }
}

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum TypedFactRelationColumnName {
        Id => "id",
        Kind => "kind",
        OwnerKey => "owner_key",
        LocalKey => "local_key",
        From => "from",
        To => "to",
        Origin => "origin",
        SpanKind => "span_kind",
        File => "file",
        StartByte => "start_byte",
        EndByte => "end_byte",
        StartLine => "start_line",
        StartCol => "start_col",
        EndLine => "end_line",
        EndCol => "end_col",
        SourceId => "source_id",
        StableKey => "stable_key",
        Node => "node",
        Dimension => "dimension",
        Name => "name",
        Value => "value",
        Parent => "parent",
        Child => "child",
        Label => "label",
        Order => "order",
        EventKind => "event_kind",
        Source => "source",
        Target => "target",
        Scope => "scope",
        DigestHex => "digest_hex",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedFactRelationSchema {
    name: TypedFactRelationName,
    columns: &'static [TypedFactRelationColumnName],
}

impl TypedFactRelationSchema {
    pub const fn new(
        name: TypedFactRelationName,
        columns: &'static [TypedFactRelationColumnName],
    ) -> Self {
        Self { name, columns }
    }

    pub const fn name(self) -> TypedFactRelationName {
        self.name
    }

    pub const fn columns(self) -> &'static [TypedFactRelationColumnName] {
        self.columns
    }

    pub fn column_names(self) -> impl Iterator<Item = &'static str> {
        self.columns.iter().map(|column| column.as_str())
    }
}

impl TypedFactRelationName {
    pub const fn is_origin_relation(self) -> bool {
        matches!(self, Self::OriginNode | Self::OriginLink | Self::SourceSpan)
    }

    pub const fn is_shape_relation(self) -> bool {
        matches!(
            self,
            Self::ShapeNode
                | Self::ShapeField
                | Self::ShapeChild
                | Self::ShapeEdge
                | Self::TraceEvent
                | Self::DataFlow
                | Self::ShapeHash
        )
    }

    pub fn schema(self) -> TypedFactRelationSchema {
        typed_fact_relation_schema_for_name(self)
    }

    pub fn column_index(
        self,
        column: TypedFactRelationColumnName,
    ) -> Result<usize, TypedFactRelationError> {
        self.schema()
            .columns()
            .iter()
            .position(|candidate| *candidate == column)
            .ok_or_else(|| TypedFactRelationError::UnknownColumn {
                relation: self.as_str().to_string(),
                column: column.as_str().to_string(),
            })
    }
}

pub fn typed_fact_relation_schemas() -> &'static [TypedFactRelationSchema] {
    TYPED_FACT_RELATION_SCHEMAS
}

pub(super) fn typed_fact_relation_schema_for_raw_name(
    name: &str,
) -> Option<TypedFactRelationSchema> {
    let name = TypedFactRelationName::from_str(name)?;
    Some(typed_fact_relation_schema_for_name(name))
}

pub(super) fn columns_match(actual: &[String], expected: &[TypedFactRelationColumnName]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| actual == expected.as_str())
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

fn typed_fact_relation_schema_for_name(name: TypedFactRelationName) -> TypedFactRelationSchema {
    TYPED_FACT_RELATION_SCHEMAS
        .iter()
        .copied()
        .find(|schema| schema.name() == name)
        .expect("typed fact relation name should have a schema descriptor")
}
