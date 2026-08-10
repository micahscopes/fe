@group(0) @binding(0)
var<storage> leaves: array<u32>;
@group(0) @binding(1)
var<storage> nodes: array<u32>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>((f32(((vi & 1u) << 2u)) - 1f), (f32(((vi & 2u) << 1u)) - 1f), 0f, 1f);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let _e10 = nodes[i32(0u)];
    let _e14 = nodes[i32(7u)];
    return unpack4x8unorm(((_e10 ^ (_e14 * 65793u)) | 4278190080u));
}
