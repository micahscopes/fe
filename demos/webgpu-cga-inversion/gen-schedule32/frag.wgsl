struct Input {
    p0_: f32,
    p1_: f32,
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
    var phi_162_: f32;
    var phi_32_: u32;
    var edge_0_2_phi_162_: f32;
    var edge_0_2_phi_32_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_33_: bool;
    var phi_88_: f32;
    var edge_5_7_phi_88_: f32;
    var edge_3_7_phi_88_: f32;
    var edge_10_2_phi_162_: f32;
    var edge_10_2_phi_32_: u32;

    let _e7 = input.p0_;
    let _e9 = input.p1_;
    let _e11 = input.p2_;
    let _e13 = input.p3_;
    let _e15 = input.p4_;
    let _e22 = ((f32(bitcast<i32>(u32(pos.x))) - 64f) * _e11);
    let _e25 = ((f32(bitcast<i32>(u32(pos.y))) - 64f) * _e11);
    let _e33 = (1f / sqrt((((_e22 * _e22) + (_e25 * _e25)) + 3.2399998f)));
    edge_0_2_phi_162_ = 0f;
    edge_0_2_phi_32_ = 0u;
    let _e43 = edge_0_2_phi_162_;
    let _e45 = edge_0_2_phi_32_;
    phi_162_ = _e43;
    phi_32_ = _e45;
    loop {
        let _e50 = phi_162_;
        let _e52 = phi_32_;
        let _e56 = (bitcast<i32>(_e52) < bitcast<i32>(72u));
        if _e56 {
            let _e58 = (_e7 + ((_e22 * _e33) * _e50));
            let _e60 = (_e9 + ((_e25 * _e33) * _e50));
            let _e64 = (-(4f) + ((1.8f * _e33) * _e50));
            let _e69 = (((_e58 * _e58) + (_e60 * _e60)) + (_e64 * _e64));
            let _e76 = ((_e69 - 1f) * 0.5f);
            let _e80 = ((_e69 + 1f) * 0.5f);
            let _e82 = (((_e13 * _e13) + (_e15 * _e15)) * 0.5f);
            let _e84 = (_e82 - 1f);
            let _e96 = (_e84 * _e80);
            let _e97 = (_e96 * _e82);
            let _e102 = (_e84 * _e76);
            let _e103 = (_e102 * _e82);
            let _e115 = (_e15 * _e80);
            let _e116 = (_e115 * _e82);
            let _e121 = (_e15 * _e76);
            let _e122 = (_e121 * _e84);
            let _e129 = (_e15 * _e60);
            let _e130 = (_e129 * _e82);
            let _e132 = (_e129 * _e84);
            let _e138 = (_e13 * _e80);
            let _e139 = (_e138 * _e82);
            let _e144 = (_e13 * _e76);
            let _e145 = (_e144 * _e84);
            let _e152 = (_e13 * _e60);
            let _e153 = (_e152 * _e15);
            let _e157 = (_e13 * _e58);
            let _e158 = (_e157 * _e82);
            let _e160 = (_e157 * _e84);
            let _e162 = (_e157 * _e15);
            let _e169 = (0f + 0f);
            let _e174 = (0f + _e169);
            let _e180 = (0f + _e174);
            let _e443 = ((-(((_e82 * _e80) * _e82)) + (0f + (0f + (0f + (0f + (0f + (-((_e96 * _e84)) + ((_e103 + _e103) + (0f + (0f + (0f + (0f + (0f + (-((_e115 * _e15)) + (0f + (0f + (0f + ((_e130 + _e130) + (0f + (0f + (0f + (0f + (-((_e138 * _e13)) + (0f + (0f + (0f + (0f + (0f + ((_e158 + _e158) + _e180))))))))))))))))))))))))))))) - (0f + (((_e82 * _e76) * _e82) + (0f + (0f + (0f + (-((_e97 + _e97)) + (0f + (0f + ((_e102 * _e84) + (0f + (0f + (0f + (0f + (0f + (0f + (-((_e121 * _e15)) + (0f + (0f + ((_e132 + _e132) + (0f + (0f + (0f + (0f + (0f + (-((_e144 * _e13)) + (0f + (0f + (0f + (0f + ((_e160 + _e160) + _e174)))))))))))))))))))))))))))))));
            let _e445 = -(0.0004f);
            if (_e445 < _e443) {
                edge_5_7_phi_88_ = _e445;
                let _e449 = edge_5_7_phi_88_;
                phi_88_ = _e449;
            } else {
                edge_3_7_phi_88_ = _e443;
                let _e453 = edge_3_7_phi_88_;
                phi_88_ = _e453;
            }
            let _e456 = phi_88_;
            let _e457 = (_e456 - _e443);
            let _e463 = (((0f + (0f + (0f + (((_e82 * _e60) * _e82) + (0f + (0f + (0f + (0f + (0f + (0f + (-(((_e84 * _e60) * _e84)) + (0f + (-((_e116 + _e116)) + (0f + ((_e122 + _e122) + (0f + (0f + (0f + (0f + ((_e129 * _e15) + (0f + (0f + (0f + (0f + (0f + (0f + (0f + (-((_e152 * _e13)) + (0f + (0f + ((_e162 + _e162) + _e169))))))))))))))))))))))))))))))) + (_e457 * _e15)) / _e456);
            let _e464 = ((0f + (0f + (((_e82 * _e64) * _e82) + (0f + (0f + (0f + (0f + (0f + (0f + (-(((_e84 * _e64) * _e84)) + (0f + (0f + (0f + (0f + (0f + (0f + (-(((_e15 * _e64) * _e15)) + (0f + (0f + (0f + (0f + (0f + (0f + (0f + (0f + (-(((_e13 * _e64) * _e13)) + (0f + (0f + (0f + _e180))))))))))))))))))))))))))))) / _e456);
            let _e466 = ((((0f + (0f + (0f + (0f + (((_e82 * _e58) * _e82) + (0f + (0f + (0f + (0f + (0f + (0f + (-(((_e84 * _e58) * _e84)) + (0f + (0f + (0f + (0f + (0f + (0f + (0f + (0f + (-(((_e15 * _e58) * _e15)) + (-((_e139 + _e139)) + (0f + ((_e145 + _e145) + (0f + (0f + ((_e153 + _e153) + (0f + (0f + (0f + (0f + ((_e157 * _e13) + 0f)))))))))))))))))))))))))))))))) + (_e457 * _e13)) / _e456) + 0.62f);
            let _e468 = (_e463 - 0.08f);
            let _e474 = (sqrt(((_e466 * _e466) + (_e468 * _e468))) - 0.58f);
            let _e482 = ((sqrt(((_e474 * _e474) + (_e464 * _e464))) - 0.17f) * -(_e456));
            if (_e482 < 0.0022f) {
                let _e495 = (38u + (24u * bitcast<u32>((bitcast<i32>(_e52) >> 3u))));
                if (0f < _e463) {
                    loop_result = (((_e495 + 22528u) + ((255u - _e495) << 16u)) + 4278190080u);
                    loop_did_return = true;
                    break;
                } else {
                    loop_result = (((56u + (_e495 << 8u)) + 14680064u) + 4278190080u);
                    loop_did_return = true;
                    break;
                }
            } else {
                edge_10_2_phi_162_ = (_e50 + (_e482 * 0.18f));
                edge_10_2_phi_32_ = (_e52 + 1u);
                let _e526 = edge_10_2_phi_162_;
                let _e528 = edge_10_2_phi_32_;
                phi_162_ = _e526;
                phi_32_ = _e528;
                continue;
            }
        } else {
            loop_result = 4279831303u;
            loop_did_return = true;
            loop_header_carry_33_ = _e56;
            break;
        }
    }
    let _e544 = loop_did_return;
    if _e544 {
    }
    let _e546 = loop_result;
    return unpack4x8unorm(_e546);
}
