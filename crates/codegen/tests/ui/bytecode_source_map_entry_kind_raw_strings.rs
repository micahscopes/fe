use fe_codegen::debug::BytecodeSourceMapEntryKind;

fn main() {
    let _ = BytecodeSourceMapEntryKind::Source {
        span_kind: "original".to_string(),
        file: "src/main.fe".to_string(),
        start_byte: 0,
        end_byte: 4,
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 5,
        snippet: "main".to_string(),
    };

    let _ = BytecodeSourceMapEntryKind::BytecodeUnmapped {
        reason: "no_ir_inst".to_string(),
    };
}
