//! Fullscreen fragment helpers must preserve the same numeric semantics as
//! authored raster fragments; a color conversion is not a demo-only ABI.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundle};
use url::Url;

#[test]
fn fullscreen_color_helpers_preserve_packing_and_reject_unproved_overflow() {
    for (name, expression, bitwise, succeeds) in [
        ("literal", "-1", false, true),
        (
            "conversion",
            "__i32_from_f32(clamp01(value) * 255.0 + 0.5)",
            false,
            true,
        ),
        (
            "checked_packing",
            "pack(value, 0.065, 0.080, 1.0)",
            false,
            false,
        ),
        (
            "bitwise_packing",
            "pack(value, 0.065, 0.080, 1.0)",
            true,
            true,
        ),
    ] {
        let source = format!(
            r#"
use std::webgpu::{{GpuProgram,WebGpuBackend,FragmentSurface}}
extern {{ fn __i32_from_f32(_:f32)->i32 }}
fn clamp01(_ x:f32)->f32 {{if x<0.0 {{0.0}} else if x>1.0 {{1.0}} else {{x}}}}
fn pack(_ r:f32,_ g:f32,_ b:f32,_ alpha:f32)->i32 {{
    let red=__i32_from_f32(clamp01(r)*255.0+0.5)
    let green=__i32_from_f32(clamp01(g)*255.0+0.5)
    let blue=__i32_from_f32(clamp01(b)*255.0+0.5)
    let a=__i32_from_f32(clamp01(alpha)*255.0+0.5)
    let high=if a<128 {{a}} else {{a-256}}
    red+green*256+blue*65536+high*16777216
}}
fn color(_ value:f32)->i32 {{{expression}}}
actor Background uses (GpuProgram<WebGpuBackend>) {{
    value:f32,
    fn shade(self,x:i32,y:i32)->i32 uses (FragmentSurface) {{color(self.value)}}
}}
"#
        );
        let source = if bitwise {
            source.replace(
                "red+green*256+blue*65536+high*16777216",
                "(red & 255) | ((green & 255) << 8) | ((blue & 255) << 16) | ((a & 255) << 24)",
            )
        } else {
            source
        };
        let mut db = DriverDataBase::default();
        let url = Url::parse(&format!("file:///fullscreen_color_{name}.fe")).unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let file = db.workspace().get(&db, &url).unwrap();
        let top = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{name}: {diagnostics}");
        let result = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None));
        if succeeds {
            let bundle = result.unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(!bundle.wgsl.is_empty());
            eprintln!("{name}: {} WGSL bytes", bundle.wgsl.len());
        } else {
            let error = result
                .err()
                .expect("unproved overflow cannot silently lose its trap")
                .to_string();
            assert!(error.contains("trap"), "{name}: {error}");
        }
    }
}
