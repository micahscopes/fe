use common::facts::SourceSpanKind;
use fe_codegen::debug::{
    BytecodeDebugLineRow, BytecodeDebugSourceFile, OwnedBytecodeDebugLineTableExport,
};

fn main() {
    let file = BytecodeDebugSourceFile {
        path: "src/main.fe".to_string(),
    };

    let row = BytecodeDebugLineRow {
        object: "Foo".to_string(),
        section: "runtime".to_string(),
        pc_start: 0,
        pc_end: 4,
        file_index: 0,
        span_kind: SourceSpanKind::Original,
        start_byte: 0,
        end_byte: 4,
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 5,
        snippet: "main".to_string(),
    };

    let _ = OwnedBytecodeDebugLineTableExport {
        schema_version: 1,
        object: Some("Foo".to_string()),
        section: Some("runtime".to_string()),
        files: vec![file],
        rows: vec![row],
    };
}
