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
    var phi_171_: f32;
    var phi_32_: u32;
    var edge_0_2_phi_171_: f32;
    var edge_0_2_phi_32_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_33_: bool;
    var phi_97_: f32;
    var edge_5_7_phi_97_: f32;
    var edge_3_7_phi_97_: f32;
    var edge_10_2_phi_171_: f32;
    var edge_10_2_phi_32_: u32;

    let _e7 = input.p0_;
    let _e9 = input.p1_;
    let _e11 = input.p2_;
    let _e13 = input.p3_;
    let _e15 = input.p4_;
    let _e22 = ((f32(bitcast<i32>(u32(pos.x))) - 64f) * _e11);
    let _e25 = ((f32(bitcast<i32>(u32(pos.y))) - 64f) * _e11);
    let _e33 = (1f / sqrt((((_e22 * _e22) + (_e25 * _e25)) + 3.2399998f)));
    edge_0_2_phi_171_ = 0f;
    edge_0_2_phi_32_ = 0u;
    let _e43 = edge_0_2_phi_171_;
    let _e45 = edge_0_2_phi_32_;
    phi_171_ = _e43;
    phi_32_ = _e45;
    loop {
        let _e50 = phi_171_;
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
            let _e107 = (0f + 0f);
            let _e111 = (0f + _e107);
            let _e114 = (0f + _e111);
            let _e120 = (0f + _e114);
            let _e129 = (0f + _e120);
            let _e171 = (_e15 * _e80);
            let _e172 = (_e171 * _e82);
            let _e177 = (_e15 * _e76);
            let _e178 = (_e177 * _e84);
            let _e208 = (0f + _e129);
            let _e239 = (_e15 * _e60);
            let _e240 = (_e239 * _e82);
            let _e242 = (_e239 * _e84);
            let _e248 = (_e13 * _e80);
            let _e249 = (_e248 * _e82);
            let _e254 = (_e13 * _e76);
            let _e255 = (_e254 * _e84);
            let _e303 = (_e13 * _e60);
            let _e304 = (_e303 * _e15);
            let _e308 = (_e13 * _e58);
            let _e309 = (_e308 * _e82);
            let _e311 = (_e308 * _e84);
            let _e313 = (_e308 * _e15);
            let _e375 = ((((-(((_e82 * _e80) * _e82)) + (0f + (0f + (0f + (0f + (0f + (-((_e96 * _e84)) + ((_e103 + _e103) + 0f)))))))) + (0f + (0f + (0f + (0f + (0f + (-((_e171 * _e15)) + _e111))))))) + ((0f + ((_e240 + _e240) + (0f + (0f + (0f + (0f + (-((_e248 * _e13)) + _e107))))))) + (0f + (0f + (0f + (0f + ((_e309 + _e309) + _e114))))))) - (((0f + (((_e82 * _e76) * _e82) + (0f + (0f + (0f + (-((_e97 + _e97)) + _e111)))))) + ((_e102 * _e84) + (0f + (0f + (0f + (0f + (0f + (0f + (-((_e177 * _e15)) + 0f))))))))) + ((0f + (0f + ((_e242 + _e242) + _e129))) + (-((_e254 * _e13)) + (0f + (0f + (0f + (0f + ((_e311 + _e311) + _e111)))))))));
            let _e377 = -(0.0004f);
            if (_e377 < _e375) {
                edge_5_7_phi_97_ = _e377;
                let _e381 = edge_5_7_phi_97_;
                phi_97_ = _e381;
            } else {
                edge_3_7_phi_97_ = _e375;
                let _e385 = edge_3_7_phi_97_;
                phi_97_ = _e385;
            }
            let _e388 = phi_97_;
            let _e389 = (_e388 - _e375);
            let _e395 = (((((0f + (0f + (0f + (((_e82 * _e60) * _e82) + _e120)))) + (0f + (0f + (-(((_e84 * _e60) * _e84)) + (0f + (-((_e172 + _e172)) + (0f + ((_e178 + _e178) + _e107)))))))) + ((0f + (0f + (0f + ((_e239 * _e15) + _e120)))) + (0f + (0f + (0f + (-((_e303 * _e13)) + (0f + (0f + ((_e313 + _e313) + _e107))))))))) + (_e389 * _e15)) / _e388);
            let _e396 = ((((0f + (0f + (((_e82 * _e64) * _e82) + _e129))) + (0f + (-(((_e84 * _e64) * _e84)) + _e208))) + ((-(((_e15 * _e64) * _e15)) + (0f + _e208)) + (0f + (-(((_e13 * _e64) * _e13)) + _e208)))) / _e388);
            let _e398 = ((((((0f + (0f + (0f + (0f + (((_e82 * _e58) * _e82) + _e114))))) + (0f + (0f + (0f + (-(((_e84 * _e58) * _e84)) + _e120))))) + ((0f + (0f + (0f + (0f + (-(((_e15 * _e58) * _e15)) + (-((_e249 + _e249)) + (0f + ((_e255 + _e255) + 0f)))))))) + (0f + (0f + ((_e304 + _e304) + (0f + (0f + (0f + (0f + ((_e308 * _e13) + 0f)))))))))) + (_e389 * _e13)) / _e388) + 0.62f);
            let _e400 = (_e395 - 0.08f);
            let _e406 = (sqrt(((_e398 * _e398) + (_e400 * _e400))) - 0.58f);
            let _e414 = ((sqrt(((_e406 * _e406) + (_e396 * _e396))) - 0.17f) * -(_e388));
            if (_e414 < 0.0022f) {
                let _e427 = (38u + (24u * bitcast<u32>((bitcast<i32>(_e52) >> 3u))));
                if (0f < _e395) {
                    loop_result = (((_e427 + 22528u) + ((255u - _e427) << 16u)) + 4278190080u);
                    loop_did_return = true;
                    break;
                } else {
                    loop_result = (((56u + (_e427 << 8u)) + 14680064u) + 4278190080u);
                    loop_did_return = true;
                    break;
                }
            } else {
                edge_10_2_phi_171_ = (_e50 + (_e414 * 0.18f));
                edge_10_2_phi_32_ = (_e52 + 1u);
                let _e458 = edge_10_2_phi_171_;
                let _e460 = edge_10_2_phi_32_;
                phi_171_ = _e458;
                phi_32_ = _e460;
                continue;
            }
        } else {
            loop_result = 4279831303u;
            loop_did_return = true;
            loop_header_carry_33_ = _e56;
            break;
        }
    }
    let _e476 = loop_did_return;
    if _e476 {
    }
    let _e478 = loop_result;
    return unpack4x8unorm(_e478);
}
