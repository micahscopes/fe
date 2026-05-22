use fe_codegen::debug::{BytecodeSourceMapExportMetadata, BytecodeSourceMapExportOptions};

fn main() {
    let _ = BytecodeSourceMapExportOptions::new().with_object_key("Foo");
    let _ = BytecodeSourceMapExportMetadata::section("runtime");
}
