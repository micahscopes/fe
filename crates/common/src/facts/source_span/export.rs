mod error;
mod kind;
mod record;
mod validation;

pub use error::SourceSpanExportError;
pub use kind::SourceSpanKind;
pub use record::SourceSpanExport;
pub(in crate::facts) use record::source_span_export_sort_key;
pub(in crate::facts) use validation::validated_source_span_parts;
