use fe_common::origin::{OriginExportKey, OriginExportKind};

fn main() {
    let _ = OriginExportKey::new(OriginExportKind::Semantic, "semantic:test", "expr:0");
}
