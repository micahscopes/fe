crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum ShapeHashScope {
        Local => "local",
        Tree => "tree",
        Graph => "graph",
    }
}
