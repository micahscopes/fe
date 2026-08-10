struct orbit_element {
    re_bits: u32,
    im_bits: u32,
}

@group(0) @binding(0)
var<storage, read_write> orbit: array<orbit_element>;

@compute @workgroup_size(1, 1, 1)
fn main() {
    orbit[i32(0u)].re_bits = 1065353216u;
    orbit[i32(0u)].im_bits = 3221225472u;
}
