#![cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::compile_runtime_package_native_i32_entry;
use url::Url;

fn compile_entry(
    name: &str,
    source: &str,
    entry: &str,
) -> Result<fe_codegen::NativeI32EntryArtifact, String> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .map_err(|error| error.to_string())?;
    compile_runtime_package_native_i32_entry(&db, &package, entry)
        .map_err(|error| error.to_string())
}

#[test]
fn n0_rejects_an_unverified_entry_signature() {
    let error = compile_entry(
        "native_wrong_signature",
        "pub fn wrong(value: i32) -> i32 { value }",
        "wrong",
    )
    .err()
    .expect("native ABI mismatch must fail closed");
    assert!(error.contains("must have ABI (i32, i32) -> i32"), "{error}");
}

#[test]
fn n1_executes_scalar_arithmetic() {
    let artifact = compile_entry(
        "native_add",
        "pub fn add(lhs: i32, rhs: i32) -> i32 { lhs + rhs }",
        "add",
    )
    .expect("native add should compile");
    assert_eq!(artifact.entry_name(), "add");
    assert_eq!(artifact.call(20, 22), 42);
    assert_eq!(artifact.call(-7, 3), -4);
}

#[test]
fn n2_executes_control_flow_and_a_helper_call() {
    let artifact = compile_entry(
        "native_loop",
        r#"
fn step(value: i32, delta: i32) -> i32 {
    value + delta
}

pub fn accumulate(count: i32, delta: i32) -> i32 {
    let mut index: i32 = 0
    let mut total: i32 = 0
    while index < count {
        total = step(value: total, delta: delta)
        index = index + 1
    }
    total
}
"#,
        "accumulate",
    )
    .expect("native control flow should compile");
    assert_eq!(artifact.call(7, 6), 42);
    assert_eq!(artifact.call(0, 99), 0);
}

fn mandel_oracle_q12(px: i32, py: i32) -> u32 {
    let c_re = -8192 + px * 24;
    let c_im = -6144 + py * 24;
    let mut zr = 0i32;
    let mut zi = 0i32;
    let mut iteration = 0u32;
    while iteration < 100 {
        let rr = zr * zr;
        let ii = zi * zi;
        if rr + ii < 67_108_864 {
            let next_real = rr - ii;
            let next_imaginary = ((zr * 2) * zi) >> 12;
            zr = (next_real >> 12) + c_re;
            zi = next_imaginary + c_im;
            iteration += 1;
        } else {
            return iteration;
        }
    }
    iteration
}

#[test]
fn native_mandelbrot_capstone_matches_the_full_frame_oracle() {
    let artifact = compile_entry(
        "native_mandelbrot_q12",
        include_str!("../../../demos/capstones/mandelbrot/kernel.fe"),
        "mandel_pixel_q12",
    )
    .expect("canonical Mandelbrot kernel should compile through Native");

    let mut hash = 0x811c9dc5u32;
    for py in 0..512i32 {
        for px in 0..512i32 {
            let got = artifact.call(px, py) as u32;
            let expected = mandel_oracle_q12(px, py);
            assert_eq!(
                got, expected,
                "native mandel_pixel_q12({px}, {py}) = {got}, oracle = {expected}"
            );
            for byte in got.to_le_bytes() {
                hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
            }
        }
    }
    assert_eq!(hash, 0x2d29649a);
}
