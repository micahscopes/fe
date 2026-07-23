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
            let _e91 = (_e13 * _e84);
            let _e92 = (_e91 * _e42);
            let _e93 = (_e13 * _e80);
            let _e94 = (_e93 * _e44);
            let _e95 = (_e13 * _e67);
            let _e96 = (_e95 * _e15);
            let _e97 = (_e13 * _e65);
            let _e103 = (_e15 * _e84);
            let _e104 = (_e103 * _e42);
            let _e105 = (_e15 * _e80);
            let _e106 = (_e105 * _e44);
            let _e107 = (_e15 * _e67);
            let _e110 = (_e97 * _e15);
            let _e121 = (_e44 * _e84);
            let _e122 = (_e121 * _e42);
            let _e123 = (_e44 * _e80);
            let _e126 = (_e107 * _e44);
            let _e128 = (_e97 * _e44);
            let _e132 = (_e123 * _e42);
            let _e134 = (_e107 * _e42);
            let _e136 = (_e97 * _e42);
            let _e137 = (_e13 - _e13);
            let _e213 = ((((-(((_e42 * _e84) * _e42)) + (-((_e121 * _e44)) + ((_e132 + _e132) + _e137))) + (-((_e103 * _e15)) + _e137)) + (((_e134 + _e134) + (-((_e91 * _e13)) + _e137)) + ((_e136 + _e136) + _e137))) - (((((_e42 * _e80) * _e42) + (-((_e122 + _e122)) + _e137)) + ((_e123 * _e44) + (-((_e105 * _e15)) + _e137))) + (((_e126 + _e126) + _e137) + (-((_e93 * _e13)) + ((_e128 + _e128) + _e137)))));
            let _e215 = -(0.0004f);
            if (_e215 < _e213) {
                edge_5_7_phi_93_ = _e215;
                let _e219 = edge_5_7_phi_93_;
                phi_93_ = _e219;
            } else {
                edge_3_7_phi_93_ = _e213;
                let _e223 = edge_3_7_phi_93_;
                phi_93_ = _e223;
            }
            let _e226 = phi_93_;
            let _e227 = (_e226 - _e213);
            let _e233 = (((((((_e42 * _e67) * _e42) + _e137) + (-(((_e44 * _e67) * _e44)) + (-((_e104 + _e104)) + ((_e106 + _e106) + _e137)))) + (((_e107 * _e15) + _e137) + (-((_e95 * _e13)) + ((_e110 + _e110) + _e137)))) + (_e227 * _e15)) / _e226);
            let _e234 = ((((((_e42 * _e71) * _e42) + _e137) + (-(((_e44 * _e71) * _e44)) + _e137)) + ((-(((_e15 * _e71) * _e15)) + _e137) + (-(((_e13 * _e71) * _e13)) + _e137))) / _e226);
            let _e236 = ((((((((_e42 * _e65) * _e42) + _e137) + (-(((_e44 * _e65) * _e44)) + _e137)) + ((-(((_e15 * _e65) * _e15)) + (-((_e92 + _e92)) + ((_e94 + _e94) + _e137))) + ((_e96 + _e96) + ((_e97 * _e13) + _e137)))) + (_e227 * _e13)) / _e226) + 0.62f);
            let _e238 = (_e233 - 0.08f);
            let _e244 = (sqrt(((_e236 * _e236) + (_e238 * _e238))) - 0.58f);
            let _e252 = ((sqrt(((_e244 * _e244) + (_e234 * _e234))) - 0.17f) * -(_e226));
            if (_e252 < 0.0022f) {
                let _e265 = (38u + (24u * bitcast<u32>((bitcast<i32>(_e59) >> 3u))));
                if (0f < _e233) {
                    loop_result = (((_e265 + 22528u) + ((255u - _e265) << 16u)) + 4278190080u);
                    loop_did_return = true;
                    break;
                } else {
                    loop_result = (((56u + (_e265 << 8u)) + 14680064u) + 4278190080u);
                    loop_did_return = true;
                    break;
                }
            } else {
                edge_10_2_phi_167_ = (_e57 + (_e252 * 0.18f));
                edge_10_2_phi_41_ = (_e59 + 1u);
                let _e296 = edge_10_2_phi_167_;
                let _e298 = edge_10_2_phi_41_;
                phi_167_ = _e296;
                phi_41_ = _e298;
                continue;
            }
        } else {
            loop_result = 4279831303u;
            loop_did_return = true;
            loop_header_carry_42_ = _e63;
            break;
        }
    }
    let _e314 = loop_did_return;
    if _e314 {
    }
    let _e316 = loop_result;
    return unpack4x8unorm(_e316);
}
