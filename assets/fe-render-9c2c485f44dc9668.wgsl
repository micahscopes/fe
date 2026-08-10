struct orbit_element {
    re_bits: u32,
    im_bits: u32,
}

@group(0) @binding(0)
var<storage> orbit: array<orbit_element>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>((f32(((vi & 1u) << 2u)) - 1f), (f32(((vi & 2u) << 1u)) - 1f), 0f, 1f);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let _e10 = orbit[i32(0u)].re_bits;
    let _e12 = orbit[i32(0u)].im_bits;
    return unpack4x8unorm((_e10 ^ _e12));
}
