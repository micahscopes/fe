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
    var phi_167_: f32;
    var phi_41_: u32;
    var edge_0_2_phi_167_: f32;
    var edge_0_2_phi_41_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_42_: bool;
    var phi_93_: f32;
    var edge_5_7_phi_93_: f32;
    var edge_3_7_phi_93_: f32;
    var edge_10_2_phi_167_: f32;
    var edge_10_2_phi_41_: u32;

    let _e7 = input.p0_;
    let _e9 = input.p1_;
    let _e11 = input.p2_;
    let _e13 = input.p3_;
    let _e15 = input.p4_;
    let _e22 = ((f32(bitcast<i32>(u32(pos.x))) - 64f) * _e11);
    let _e25 = ((f32(bitcast<i32>(u32(pos.y))) - 64f) * _e11);
    let _e33 = (1f / sqrt((((_e22 * _e22) + (_e25 * _e25)) + 3.2399998f)));
    let _e42 = (((_e13 * _e13) + (_e15 * _e15)) * 0.5f);
    let _e44 = (_e42 - 1f);
    edge_0_2_phi_167_ = 0f;
    edge_0_2_phi_41_ = 0u;
    let _e50 = edge_0_2_phi_167_;
    let _e52 = edge_0_2_phi_41_;
    phi_167_ = _e50;
    phi_41_ = _e52;
    loop {
        let _e57 = phi_167_;
        let _e59 = phi_41_;
        let _e63 = (bitcast<i32>(_e59) < bitcast<i32>(72u));
        if _e63 {
            let _e65 = (_e7 + ((_e22 * _e33) * _e57));
            let _e67 = (_e9 + ((_e25 * _e33) * _e57));
            let _e71 = (-(4f) + ((1.8f * _e33) * _e57));
            let _e76 = (((_e65 * _e65) + (_e67 * _e67)) + (_e71 * _e71));
            let _e80 = ((_e76 - 1f) * 0.5f);
            let _e84 = ((_e76 + 1f) * 0.5f);
            let _e85 = (_e13 * _e65);
            let _e87 = (_e13 * _e67);
            let _e93 = (_e13 * _e80);
            let _e96 = (_e13 * _e84);
            let _e100 = (_e15 * _e65);
            let _e104 = (_e15 * _e67);
            let _e108 = (_e44 * _e65);
            let _e113 = (0f + 0f);
            let _e118 = (0f + _e113);
            let _e125 = (0f + _e118);
            let _e133 = (0f + _e125);
            let _e142 = (0f + _e133);
            let _e174 = (_e44 * _e80);
            let _e178 = (_e42 * _e65);
            let _e182 = (_e42 * _e84);
            let _e192 = (_e15 * _e80);
            let _e195 = (_e15 * _e84);
            let _e257 = (_e44 * _e67);
            let _e264 = (_e42 * _e67);
            let _e330 = (_e44 * _e84);
            let _e334 = (_e42 * _e80);
            let _e411 = ((((0f + (0f + (0f + (0f + (-((_e96 * _e13)) + _e125))))) + (0f + (((_e85 * _e42) + (_e178 * _e13)) + (0f + (0f + (0f + (0f + (0f + (-((_e195 * _e15)) + 0f))))))))) + ((0f + (0f + (((_e104 * _e42) + (_e264 * _e15)) + _e142))) + (-((_e330 * _e44)) + (((_e174 * _e42) + (_e334 * _e44)) + (0f + (0f + (0f + (0f + (0f + (-((_e182 * _e42)) + 0f)))))))))) - (((0f + (0f + (0f + (-((_e93 * _e13)) + (0f + (0f + (0f + (((_e85 * _e44) + (_e108 * _e13)) + 0f)))))))) + (0f + (0f + (0f + (0f + (0f + (0f + (-((_e192 * _e15)) + _e113)))))))) + ((((_e104 * _e44) + (_e257 * _e15)) + (0f + (0f + (0f + (0f + (0f + (0f + ((_e174 * _e44) + 0f)))))))) + (0f + (0f + (-(((_e330 * _e42) + (_e182 * _e44))) + (0f + (0f + (0f + ((_e334 * _e42) + _e113))))))))));
            let _e413 = -(0.0004f);
            if (_e413 < _e411) {
                edge_5_7_phi_93_ = _e413;
                let _e417 = edge_5_7_phi_93_;
                phi_93_ = _e417;
            } else {
                edge_3_7_phi_93_ = _e411;
                let _e421 = edge_3_7_phi_93_;
                phi_93_ = _e421;
            }
            let _e424 = phi_93_;
            let _e425 = (_e424 - _e411);
            let _e431 = (((((0f + (-((_e87 * _e13)) + (0f + (0f + (0f + (((_e85 * _e15) + (_e100 * _e13)) + _e118)))))) + (0f + (0f + (0f + (0f + ((_e104 * _e15) + _e125)))))) + ((0f + (((_e192 * _e44) + (_e174 * _e15)) + (0f + (-(((_e195 * _e42) + (_e182 * _e15))) + (0f + (-((_e257 * _e44)) + _e118)))))) + (0f + (0f + (0f + (0f + ((_e264 * _e42) + _e125))))))) + (_e425 * _e15)) / _e424);
            let _e432 = ((((0f + (0f + (-(((_e13 * _e71) * _e13)) + _e142))) + (0f + (0f + (0f + (0f + (0f + (-(((_e15 * _e71) * _e15)) + _e118))))))) + ((0f + (0f + (0f + (0f + (0f + (0f + (-(((_e44 * _e71) * _e44)) + _e113))))))) + (0f + (0f + (0f + (0f + (0f + (((_e42 * _e71) * _e42) + _e118)))))))) / _e424);
            let _e434 = (((((((_e85 * _e13) + (0f + (0f + (0f + (0f + (0f + (((_e87 * _e15) + (_e104 * _e13)) + _e113))))))) + (((_e93 * _e44) + (_e174 * _e13)) + (0f + (-(((_e96 * _e42) + (_e182 * _e13))) + (-((_e100 * _e15)) + _e133))))) + ((0f + (0f + (0f + (0f + (-((_e108 * _e44)) + _e125))))) + (0f + (0f + (0f + ((_e178 * _e42) + _e133)))))) + (_e425 * _e13)) / _e424) + 0.62f);
            let _e436 = (_e431 - 0.08f);
            let _e442 = (sqrt(((_e434 * _e434) + (_e436 * _e436))) - 0.58f);
            let _e450 = ((sqrt(((_e442 * _e442) + (_e432 * _e432))) - 0.17f) * -(_e424));
            if (_e450 < 0.0022f) {
                let _e463 = (38u + (24u * bitcast<u32>((bitcast<i32>(_e59) >> 3u))));
                if (0f < _e431) {
                    loop_result = (((_e463 + 22528u) + ((255u - _e463) << 16u)) + 4278190080u);
                    loop_did_return = true;
                    break;
                } else {
                    loop_result = (((56u + (_e463 << 8u)) + 14680064u) + 4278190080u);
                    loop_did_return = true;
                    break;
                }
            } else {
                edge_10_2_phi_167_ = (_e57 + (_e450 * 0.18f));
                edge_10_2_phi_41_ = (_e59 + 1u);
                let _e494 = edge_10_2_phi_167_;
                let _e496 = edge_10_2_phi_41_;
                phi_167_ = _e494;
                phi_41_ = _e496;
                continue;
            }
        } else {
            loop_result = 4279831303u;
            loop_did_return = true;
            loop_header_carry_42_ = _e63;
            break;
        }
    }
    let _e512 = loop_did_return;
    if _e512 {
    }
    let _e514 = loop_result;
    return unpack4x8unorm(_e514);
}
