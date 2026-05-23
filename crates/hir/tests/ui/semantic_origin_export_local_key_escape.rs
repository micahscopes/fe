use fe_hir::origin::SemanticOrigin;

fn semantic_origin_does_not_expose_export_local_key<'db>(origin: SemanticOrigin<'db>) {
    let _ = origin.export_local_key();
}

fn main() {}
