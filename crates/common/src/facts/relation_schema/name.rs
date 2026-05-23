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
}
