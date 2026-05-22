use fe_codegen::debug::BytecodeSourceMapFilter;

fn main() {
    let _ = BytecodeSourceMapFilter {
        object: "Foo",
        section: "runtime",
    };

    let _ = BytecodeSourceMapFilter::new("Foo");
}
