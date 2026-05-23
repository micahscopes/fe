use std::fmt;

use crate::facts::{FactId, FactNamespace, FactNamespaceError, ids::validated_fact_namespace};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeFactTextError {
    Empty { field: &'static str },
    WrongNamespace(FactNamespaceError),
}

impl fmt::Display for ShapeFactTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} must not be empty"),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ShapeFactTextError {}

impl From<FactNamespaceError> for ShapeFactTextError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

pub(super) fn validated_shape_node_id(id: FactId) -> Result<FactId, ShapeFactTextError> {
    validated_fact_namespace(id, FactNamespace::ShapeNode).map_err(Into::into)
}

pub(super) fn non_empty_shape_fact_text(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, ShapeFactTextError> {
    let value = value.into();
    if value.is_empty() {
        return Err(ShapeFactTextError::Empty { field });
    }
    Ok(value)
}
