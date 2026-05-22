use common::facts::SourceSpanKind;
use fe_codegen::debug::{BytecodeSourceMapEntry, BytecodeSourceMapEntryKind};

fn main() {
    let _ = BytecodeSourceMapEntry::new(
        "Foo",
        "runtime",
        0,
        4,
        BytecodeSourceMapEntryKind::Source {
            span_kind: SourceSpanKind::Original,
            file: "src/main.fe".to_string(),
            start_byte: 0,
            end_byte: 4,
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 5,
            snippet: "main".to_string(),
        },
    );
}
