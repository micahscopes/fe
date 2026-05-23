crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum SourceSpanKind {
        Original => "original",
        Expanded => "expanded",
        NotFound => "not_found",
    }
}
