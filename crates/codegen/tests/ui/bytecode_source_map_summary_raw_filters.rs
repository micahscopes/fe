use fe_codegen::debug::{BytecodeSourceMapEntry, bytecode_source_map_entries_summary};

fn main() {
    let entries: Vec<BytecodeSourceMapEntry> = Vec::new();
    let _summary = bytecode_source_map_entries_summary(&entries, Some("Foo"), Some("runtime"));
}
