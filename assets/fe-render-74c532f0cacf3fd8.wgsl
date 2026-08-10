struct Input {
    p2_: f32,
    p3_: f32,
    p4_: f32,
}

@group(0) @binding(1)
var<storage> input: Input;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>((f32(((vi & 1u) << 2u)) - 1f), (f32(((vi & 2u) << 1u)) - 1f), 0f, 1f);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var phi_45_: f32;
    var phi_16_: u32;
    var edge_0_2_phi_45_: f32;
    var edge_0_2_phi_16_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_17_: bool;
    var edge_7_2_phi_45_: f32;
    var edge_7_2_phi_16_: u32;

    let _e7 = input.p2_;
    let _e9 = input.p3_;
    let _e11 = input.p4_;
    edge_0_2_phi_45_ = (((f32(bitcast<i32>(u32(pos.x))) + f32(bitcast<i32>(u32(pos.y)))) * _e7) + _e9);
    edge_0_2_phi_16_ = 0u;
    let _e23 = edge_0_2_phi_45_;
    let _e25 = edge_0_2_phi_16_;
    phi_45_ = _e23;
    phi_16_ = _e25;
    loop {
        let _e30 = phi_45_;
        let _e32 = phi_16_;
        let _e36 = (bitcast<i32>(_e32) < bitcast<i32>(3u));
        if _e36 {
            let _e39 = ((_e30 * 0.5f) + _e9);
            if (_e11 < _e39) {
                let _e53 = select(0u, select(select(bitcast<u32>(i32(_e39)), 2147483648u, (_e39 <= -2147483600f)), 2147483647u, (_e39 >= 2147483600f)), (_e39 == _e39));
                loop_result = (((_e53 + (_e53 << 8u)) + 16711680u) + 4278190080u);
                loop_did_return = true;
                break;
            } else {
                edge_7_2_phi_45_ = _e39;
                edge_7_2_phi_16_ = (_e32 + 1u);
                let _e69 = edge_7_2_phi_45_;
                let _e71 = edge_7_2_phi_16_;
                phi_45_ = _e69;
                phi_16_ = _e71;
                continue;
            }
        } else {
            let _e86 = select(0u, select(select(bitcast<u32>(i32(_e30)), 2147483648u, (_e30 <= -2147483600f)), 2147483647u, (_e30 >= 2147483600f)), (_e30 == _e30));
            loop_result = (((_e86 + 65280u) + (_e86 << 16u)) + 4278190080u);
            loop_did_return = true;
            loop_header_carry_17_ = _e36;
            break;
        }
    }
    let _e105 = loop_did_return;
    if _e105 {
    }
    let _e107 = loop_result;
    return unpack4x8unorm(_e107);
}
