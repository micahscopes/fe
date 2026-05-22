use fe_codegen::debug::{BytecodeSourceMapEntry, BytecodeSourceMapEntryKind};

fn main() {
    let _ = BytecodeSourceMapEntry {
        object: "Foo".to_string(),
        section: "runtime".to_string(),
        pc_start: 0,
        pc_end: 4,
        kind: BytecodeSourceMapEntryKind::SemanticSpanMissing,
    };
}
