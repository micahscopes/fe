use fe_codegen::origin::BytecodePcRange;

fn bytecode_pc_range_does_not_expose_export_local_key() {
    let range = BytecodePcRange::new(0, 4).expect("non-empty PC range");
    let _ = range.export_local_key();
}

fn main() {}
