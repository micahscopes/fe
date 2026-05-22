use fe_codegen::origin::{BytecodeObjectKey, BytecodePackageOrigins, BytecodeSectionKey};

fn main() {
    let _ = BytecodeSectionKey::new("Foo", "runtime");
    let _ = BytecodeSectionKey::new(BytecodeObjectKey::new("Foo"), "runtime");

    let origins = BytecodePackageOrigins::default();
    let _ = origins.origin_graph_for_object("Foo");
}
