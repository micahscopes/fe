use super::SourceSpanExport;

pub(in crate::facts) fn source_span_export_sort_key(span: &SourceSpanExport) -> impl Ord + '_ {
    (
        span.origin_key(),
        span.file(),
        span.start_byte(),
        span.end_byte(),
        span.start_line(),
        span.start_col(),
        span.end_line(),
        span.end_col(),
        span.span_kind(),
    )
}
