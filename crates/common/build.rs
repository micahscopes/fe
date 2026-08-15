fn main() {
    // `rust-embed` expands these Fe ingots while compiling `fe-common`, but
    // Cargo cannot otherwise see the directory reads performed by the derive
    // macro. Without explicit inputs, editing the builtin standard library can
    // leave `target/{debug,release}/fe` carrying stale Fe source until the
    // Rust crate is cleaned manually.
    println!("cargo:rerun-if-changed=../../ingots/core");
    println!("cargo:rerun-if-changed=../../ingots/core_derives");
    println!("cargo:rerun-if-changed=../../ingots/std");
}
