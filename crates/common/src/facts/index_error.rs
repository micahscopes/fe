mod fact_index;
mod helpers;
mod source_span;

pub use fact_index::FactIndexError;
pub(in crate::facts) use helpers::{require_fact_namespace, require_non_empty_shape_fact_text};
pub use source_span::SourceSpanFactError;
