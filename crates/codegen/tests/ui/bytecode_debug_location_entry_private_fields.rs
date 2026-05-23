use common::facts::SourceSpanKind;
use fe_codegen::debug::BytecodeDebugLocationEntry;

fn main() {
    let _ = BytecodeDebugLocationEntry {
        object: "Foo".to_string(),
        section: "runtime".to_string(),
        pc_start: 0,
        pc_end: 4,
        span_kind: SourceSpanKind::Original,
        file: "src/main.fe".to_string(),
        start_byte: 0,
        end_byte: 4,
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 5,
        snippet: "main".to_string(),
    };
}
