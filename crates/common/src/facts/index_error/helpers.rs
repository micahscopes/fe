use crate::facts::{FactId, FactIndexError, FactNamespace};

pub(in crate::facts) fn require_fact_namespace(
    id: FactId,
    expected: FactNamespace,
) -> Result<(), FactIndexError> {
    if id.namespace() == expected {
        Ok(())
    } else {
        Err(FactIndexError::WrongFactNamespace { id, expected })
    }
}

pub(in crate::facts) fn require_non_empty_shape_fact_text(
    field: &'static str,
    value: &str,
) -> Result<(), FactIndexError> {
    if value.is_empty() {
        Err(FactIndexError::InvalidShapeText { field })
    } else {
        Ok(())
    }
}
