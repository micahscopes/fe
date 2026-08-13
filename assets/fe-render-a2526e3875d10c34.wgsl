struct RasterState {
    p1_: f32,
    p2_: f32,
    p3_: f32,
    p4_: f32,
    p5_: f32,
    p6_: f32,
    p7_: u32,
    p8_: f32,
    p9_: f32,
    p10_: f32,
    p11_: f32,
    p12_: f32,
    p13_: f32,
    p14_: f32,
    p15_: f32,
    p16_: f32,
    p17_: f32,
    p18_: f32,
    p19_: f32,
    p20_: f32,
    p21_: f32,
    p22_: f32,
    p23_: f32,
    p24_: f32,
    p25_: f32,
    p26_: f32,
    p27_: f32,
    p28_: f32,
    p29_: f32,
    p30_: f32,
    p31_: f32,
    p32_: f32,
    p33_: f32,
    p34_: f32,
    p35_: f32,
    p36_: f32,
    p37_: f32,
    p38_: f32,
    p39_: f32,
    p40_: f32,
    p41_: f32,
    p42_: f32,
    p43_: f32,
    p44_: f32,
    p45_: f32,
    p46_: f32,
    p47_: f32,
    p48_: f32,
    p49_: f32,
    p50_: f32,
    p51_: f32,
    p52_: f32,
    p53_: f32,
    p54_: f32,
    p55_: f32,
    p56_: f32,
    p57_: f32,
    p58_: f32,
    p59_: u32,
}

struct RasterVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) v0_: f32,
}

@group(0) @binding(0)
var<storage> state: RasterState;

@vertex
fn marker_vertices(@builtin(vertex_index) vertex_index: u32) -> RasterVertexOutput {
    var structured_result: f32;
    var structured_did_return: bool = false;
    var phi_142_: f32;
    var phi_143_: f32;
    var phi_144_: f32;
    var edge_27_31_phi_142_: f32;
    var edge_27_31_phi_143_: f32;
    var edge_27_31_phi_144_: f32;
    var edge_30_31_phi_142_: f32;
    var edge_30_31_phi_143_: f32;
    var edge_30_31_phi_144_: f32;
    var edge_24_31_phi_142_: f32;
    var edge_24_31_phi_143_: f32;
    var edge_24_31_phi_144_: f32;
    var edge_21_31_phi_142_: f32;
    var edge_21_31_phi_143_: f32;
    var edge_21_31_phi_144_: f32;
    var edge_18_31_phi_142_: f32;
    var edge_18_31_phi_143_: f32;
    var edge_18_31_phi_144_: f32;
    var edge_15_31_phi_142_: f32;
    var edge_15_31_phi_143_: f32;
    var edge_15_31_phi_144_: f32;
    var edge_12_31_phi_142_: f32;
    var edge_12_31_phi_143_: f32;
    var edge_12_31_phi_144_: f32;
    var edge_9_31_phi_142_: f32;
    var edge_9_31_phi_143_: f32;
    var edge_9_31_phi_144_: f32;
    var edge_0_31_phi_142_: f32;
    var edge_0_31_phi_143_: f32;
    var edge_0_31_phi_144_: f32;
    var structured_result_1: f32;
    var structured_did_return_1: bool = false;
    var phi_247_: f32;
    var edge_149_151_phi_247_: f32;
    var edge_31_151_phi_247_: f32;
    var structured_result_2: f32;
    var structured_did_return_2: bool = false;
    var phi_250_: f32;
    var edge_152_154_phi_250_: f32;
    var edge_151_154_phi_250_: f32;
    var structured_result_3: f32;
    var structured_did_return_3: bool = false;
    var phi_253_: f32;
    var edge_155_157_phi_253_: f32;
    var edge_154_157_phi_253_: f32;
    var structured_result_4: f32;
    var structured_did_return_4: bool = false;
    var phi_256_: f32;
    var edge_158_160_phi_256_: f32;
    var edge_157_160_phi_256_: f32;
    var structured_result_5: f32;
    var structured_did_return_5: bool = false;
    var phi_260_: f32;
    var phi_261_: f32;
    var edge_161_163_phi_260_: f32;
    var edge_161_163_phi_261_: f32;
    var edge_160_163_phi_260_: f32;
    var edge_160_163_phi_261_: f32;
    var structured_result_6: f32;
    var structured_did_return_6: bool = false;
    var phi_271_: f32;
    var phi_273_: f32;
    var phi_274_: f32;
    var phi_275_: f32;
    var edge_164_166_phi_271_: f32;
    var edge_164_166_phi_273_: f32;
    var edge_164_166_phi_274_: f32;
    var edge_164_166_phi_275_: f32;
    var edge_163_166_phi_271_: f32;
    var edge_163_166_phi_273_: f32;
    var edge_163_166_phi_274_: f32;
    var edge_163_166_phi_275_: f32;

    let _e5 = state.p2_;
    let _e7 = state.p3_;
    let _e9 = state.p4_;
    let _e11 = state.p5_;
    let _e13 = state.p6_;
    let _e17 = state.p8_;
    let _e19 = state.p9_;
    let _e21 = state.p10_;
    let _e65 = state.p32_;
    let _e67 = state.p33_;
    let _e69 = state.p34_;
    let _e71 = state.p35_;
    let _e73 = state.p36_;
    let _e75 = state.p37_;
    let _e77 = state.p38_;
    let _e79 = state.p39_;
    let _e81 = state.p40_;
    let _e83 = state.p41_;
    let _e85 = state.p42_;
    let _e87 = state.p43_;
    let _e89 = state.p44_;
    let _e91 = state.p45_;
    let _e93 = state.p46_;
    let _e95 = state.p47_;
    let _e97 = state.p48_;
    let _e99 = state.p49_;
    let _e101 = state.p50_;
    let _e103 = state.p51_;
    let _e105 = state.p52_;
    let _e107 = state.p53_;
    let _e109 = state.p54_;
    let _e111 = state.p55_;
    let _e113 = state.p56_;
    let _e115 = state.p57_;
    let _e117 = state.p58_;
    let _e122 = (vertex_index / 6u);
    let _e124 = (vertex_index % 6u);
    if (_e122 == 0u) {
        edge_0_31_phi_142_ = _e69;
        edge_0_31_phi_143_ = _e67;
        edge_0_31_phi_144_ = _e65;
        let _e241 = edge_0_31_phi_142_;
        let _e243 = edge_0_31_phi_143_;
        let _e245 = edge_0_31_phi_144_;
        phi_142_ = _e241;
        phi_143_ = _e243;
        phi_144_ = _e245;
    } else {
        if (_e122 == 1u) {
            edge_9_31_phi_142_ = _e75;
            edge_9_31_phi_143_ = _e73;
            edge_9_31_phi_144_ = _e71;
            let _e229 = edge_9_31_phi_142_;
            let _e231 = edge_9_31_phi_143_;
            let _e233 = edge_9_31_phi_144_;
            phi_142_ = _e229;
            phi_143_ = _e231;
            phi_144_ = _e233;
        } else {
            if (_e122 == 2u) {
                edge_12_31_phi_142_ = _e81;
                edge_12_31_phi_143_ = _e79;
                edge_12_31_phi_144_ = _e77;
                let _e217 = edge_12_31_phi_142_;
                let _e219 = edge_12_31_phi_143_;
                let _e221 = edge_12_31_phi_144_;
                phi_142_ = _e217;
                phi_143_ = _e219;
                phi_144_ = _e221;
            } else {
                if (_e122 == 3u) {
                    edge_15_31_phi_142_ = _e87;
                    edge_15_31_phi_143_ = _e85;
                    edge_15_31_phi_144_ = _e83;
                    let _e205 = edge_15_31_phi_142_;
                    let _e207 = edge_15_31_phi_143_;
                    let _e209 = edge_15_31_phi_144_;
                    phi_142_ = _e205;
                    phi_143_ = _e207;
                    phi_144_ = _e209;
                } else {
                    if (_e122 == 4u) {
                        edge_18_31_phi_142_ = _e93;
                        edge_18_31_phi_143_ = _e91;
                        edge_18_31_phi_144_ = _e89;
                        let _e193 = edge_18_31_phi_142_;
                        let _e195 = edge_18_31_phi_143_;
                        let _e197 = edge_18_31_phi_144_;
                        phi_142_ = _e193;
                        phi_143_ = _e195;
                        phi_144_ = _e197;
                    } else {
                        if (_e122 == 5u) {
                            edge_21_31_phi_142_ = _e99;
                            edge_21_31_phi_143_ = _e97;
                            edge_21_31_phi_144_ = _e95;
                            let _e181 = edge_21_31_phi_142_;
                            let _e183 = edge_21_31_phi_143_;
                            let _e185 = edge_21_31_phi_144_;
                            phi_142_ = _e181;
                            phi_143_ = _e183;
                            phi_144_ = _e185;
                        } else {
                            if (_e122 == 6u) {
                                edge_24_31_phi_142_ = _e105;
                                edge_24_31_phi_143_ = _e103;
                                edge_24_31_phi_144_ = _e101;
                                let _e169 = edge_24_31_phi_142_;
                                let _e171 = edge_24_31_phi_143_;
                                let _e173 = edge_24_31_phi_144_;
                                phi_142_ = _e169;
                                phi_143_ = _e171;
                                phi_144_ = _e173;
                            } else {
                                if (_e122 == 7u) {
                                    edge_27_31_phi_142_ = _e111;
                                    edge_27_31_phi_143_ = _e109;
                                    edge_27_31_phi_144_ = _e107;
                                    let _e145 = edge_27_31_phi_142_;
                                    let _e147 = edge_27_31_phi_143_;
                                    let _e149 = edge_27_31_phi_144_;
                                    phi_142_ = _e145;
                                    phi_143_ = _e147;
                                    phi_144_ = _e149;
                                } else {
                                    edge_30_31_phi_142_ = _e117;
                                    edge_30_31_phi_143_ = _e115;
                                    edge_30_31_phi_144_ = _e113;
                                    let _e157 = edge_30_31_phi_142_;
                                    let _e159 = edge_30_31_phi_143_;
                                    let _e161 = edge_30_31_phi_144_;
                                    phi_142_ = _e157;
                                    phi_143_ = _e159;
                                    phi_144_ = _e161;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let _e251 = phi_142_;
    let _e253 = phi_143_;
    let _e255 = phi_144_;
    let _e256 = (_e255 - _e17);
    let _e257 = (_e253 - _e19);
    let _e258 = (_e251 - _e21);
    let _e260 = (_e5 + 1.5707964f);
    let _e268 = (_e260 - (6.2831855f * floor(((_e260 / 6.2831855f) + 0.5f))));
    let _e278 = ((1.2732395f * _e268) - ((0.40528473f * _e268) * bitcast<f32>((bitcast<u32>(_e268) & 2147483647u))));
    let _e287 = ((0.225f * ((_e278 * bitcast<f32>((bitcast<u32>(_e278) & 2147483647u))) - _e278)) + _e278);
    let _e295 = (_e5 - (6.2831855f * floor(((_e5 / 6.2831855f) + 0.5f))));
    let _e305 = ((1.2732395f * _e295) - ((0.40528473f * _e295) * bitcast<f32>((bitcast<u32>(_e295) & 2147483647u))));
    let _e314 = ((0.225f * ((_e305 * bitcast<f32>((bitcast<u32>(_e305) & 2147483647u))) - _e305)) + _e305);
    let _e316 = (_e7 + 1.5707964f);
    let _e324 = (_e316 - (6.2831855f * floor(((_e316 / 6.2831855f) + 0.5f))));
    let _e334 = ((1.2732395f * _e324) - ((0.40528473f * _e324) * bitcast<f32>((bitcast<u32>(_e324) & 2147483647u))));
    let _e343 = ((0.225f * ((_e334 * bitcast<f32>((bitcast<u32>(_e334) & 2147483647u))) - _e334)) + _e334);
    let _e351 = (_e7 - (6.2831855f * floor(((_e7 / 6.2831855f) + 0.5f))));
    let _e361 = ((1.2732395f * _e351) - ((0.40528473f * _e351) * bitcast<f32>((bitcast<u32>(_e351) & 2147483647u))));
    let _e370 = ((0.225f * ((_e361 * bitcast<f32>((bitcast<u32>(_e361) & 2147483647u))) - _e361)) + _e361);
    let _e376 = ((_e287 * _e258) - (_e314 * _e256));
    let _e383 = (((_e370 * _e257) + (_e343 * _e376)) + _e9);
    let _e395 = bitcast<u32>(_e11);
    let _e396 = bitcast<u32>(1f);
    let _e421 = (-12f / bitcast<f32>(select(select(_e396, _e395, ((_e395 ^ ((0u - (_e395 >> 31u)) | 2147483648u)) > (_e396 ^ ((0u - (_e396 >> 31u)) | 2147483648u)))), 2143289344u, (((_e395 & 2147483647u) > 2139095040u) || ((_e396 & 2147483647u) > 2139095040u)))));
    let _e423 = bitcast<u32>(_e13);
    let _e424 = bitcast<u32>(1f);
    let _e449 = (-12f / bitcast<f32>(select(select(_e424, _e423, ((_e423 ^ ((0u - (_e423 >> 31u)) | 2147483648u)) > (_e424 ^ ((0u - (_e424 >> 31u)) | 2147483648u)))), 2143289344u, (((_e423 & 2147483647u) > 2139095040u) || ((_e424 & 2147483647u) > 2139095040u)))));
    if (_e124 == 1u) {
        edge_149_151_phi_247_ = (0f - _e421);
        let _e456 = edge_149_151_phi_247_;
        phi_247_ = _e456;
    } else {
        edge_31_151_phi_247_ = _e421;
        let _e460 = edge_31_151_phi_247_;
        phi_247_ = _e460;
    }
    let _e464 = phi_247_;
    if (_e124 == 2u) {
        edge_152_154_phi_250_ = (0f - _e449);
        let _e471 = edge_152_154_phi_250_;
        phi_250_ = _e471;
    } else {
        edge_151_154_phi_250_ = _e449;
        let _e475 = edge_151_154_phi_250_;
        phi_250_ = _e475;
    }
    let _e479 = phi_250_;
    if (_e124 == 3u) {
        edge_155_157_phi_253_ = (0f - _e479);
        let _e486 = edge_155_157_phi_253_;
        phi_253_ = _e486;
    } else {
        edge_154_157_phi_253_ = _e479;
        let _e490 = edge_154_157_phi_253_;
        phi_253_ = _e490;
    }
    let _e494 = phi_253_;
    if (_e124 == 4u) {
        edge_158_160_phi_256_ = (0f - _e464);
        let _e501 = edge_158_160_phi_256_;
        phi_256_ = _e501;
    } else {
        edge_157_160_phi_256_ = _e464;
        let _e505 = edge_157_160_phi_256_;
        phi_256_ = _e505;
    }
    let _e509 = phi_256_;
    if (_e124 == 5u) {
        edge_161_163_phi_260_ = (0f - _e494);
        edge_161_163_phi_261_ = (0f - _e509);
        let _e519 = edge_161_163_phi_260_;
        let _e521 = edge_161_163_phi_261_;
        phi_260_ = _e519;
        phi_261_ = _e521;
    } else {
        edge_160_163_phi_260_ = _e494;
        edge_160_163_phi_261_ = _e509;
        let _e527 = edge_160_163_phi_260_;
        let _e529 = edge_160_163_phi_261_;
        phi_260_ = _e527;
        phi_261_ = _e529;
    }
    let _e534 = phi_260_;
    let _e536 = phi_261_;
    if (0.25f < _e383) {
        edge_164_166_phi_271_ = _e383;
        edge_164_166_phi_273_ = ((((_e383 * 64f) - 16f) / 63.75f) - (0.001f * _e383));
        edge_164_166_phi_274_ = ((((_e343 * _e257) - (_e370 * _e376)) * 1.6f) + (_e534 * _e383));
        edge_164_166_phi_275_ = ((((_e287 * _e256) + (_e314 * _e258)) * 1.6f) + (_e536 * _e383));
        let _e551 = edge_164_166_phi_271_;
        let _e553 = edge_164_166_phi_273_;
        let _e555 = edge_164_166_phi_274_;
        let _e557 = edge_164_166_phi_275_;
        phi_271_ = _e551;
        phi_273_ = _e553;
        phi_274_ = _e555;
        phi_275_ = _e557;
    } else {
        edge_163_166_phi_271_ = 1f;
        edge_163_166_phi_273_ = 2f;
        edge_163_166_phi_274_ = 2f;
        edge_163_166_phi_275_ = 2f;
        let _e571 = edge_163_166_phi_271_;
        let _e573 = edge_163_166_phi_273_;
        let _e575 = edge_163_166_phi_274_;
        let _e577 = edge_163_166_phi_275_;
        phi_271_ = _e571;
        phi_273_ = _e573;
        phi_274_ = _e575;
        phi_275_ = _e577;
    }
    let _e583 = phi_271_;
    let _e585 = phi_273_;
    let _e587 = phi_274_;
    let _e589 = phi_275_;
    return RasterVertexOutput(vec4<f32>(_e589, _e587, _e585, _e583), (f32(bitcast<i32>(_e122)) / 8f));
}

@fragment
fn marker_fragment(@location(0) v0_: f32) -> @location(0) vec4<f32> {
    let _e122 = bitcast<u32>(v0_);
    let _e123 = bitcast<u32>(0f);
    let _e147 = bitcast<u32>(bitcast<f32>(select(select(_e123, _e122, ((_e122 ^ ((0u - (_e122 >> 31u)) | 2147483648u)) > (_e123 ^ ((0u - (_e123 >> 31u)) | 2147483648u)))), 2143289344u, (((_e122 & 2147483647u) > 2139095040u) || ((_e123 & 2147483647u) > 2139095040u)))));
    let _e148 = bitcast<u32>(1f);
    let _e171 = bitcast<f32>(select(select(_e148, _e147, ((_e147 ^ ((0u - (_e147 >> 31u)) | 2147483648u)) < (_e148 ^ ((0u - (_e148 >> 31u)) | 2147483648u)))), 2143289344u, (((_e147 & 2147483647u) > 2139095040u) || ((_e148 & 2147483647u) > 2139095040u))));
    let _e186 = bitcast<u32>((0.98f + (-0.08000004f * _e171)));
    let _e187 = bitcast<u32>(0f);
    let _e211 = bitcast<u32>(bitcast<f32>(select(select(_e187, _e186, ((_e186 ^ ((0u - (_e186 >> 31u)) | 2147483648u)) > (_e187 ^ ((0u - (_e187 >> 31u)) | 2147483648u)))), 2143289344u, (((_e186 & 2147483647u) > 2139095040u) || ((_e187 & 2147483647u) > 2139095040u)))));
    let _e212 = bitcast<u32>(1f);
    let _e237 = (bitcast<f32>(select(select(_e212, _e211, ((_e211 ^ ((0u - (_e211 >> 31u)) | 2147483648u)) < (_e212 ^ ((0u - (_e212 >> 31u)) | 2147483648u)))), 2143289344u, (((_e211 & 2147483647u) > 2139095040u) || ((_e212 & 2147483647u) > 2139095040u)))) * 255f);
    let _e253 = bitcast<u32>((0.8f + (0.099999964f * _e171)));
    let _e254 = bitcast<u32>(0f);
    let _e278 = bitcast<u32>(bitcast<f32>(select(select(_e254, _e253, ((_e253 ^ ((0u - (_e253 >> 31u)) | 2147483648u)) > (_e254 ^ ((0u - (_e254 >> 31u)) | 2147483648u)))), 2143289344u, (((_e253 & 2147483647u) > 2139095040u) || ((_e254 & 2147483647u) > 2139095040u)))));
    let _e279 = bitcast<u32>(1f);
    let _e304 = (bitcast<f32>(select(select(_e279, _e278, ((_e278 ^ ((0u - (_e278 >> 31u)) | 2147483648u)) < (_e279 ^ ((0u - (_e279 >> 31u)) | 2147483648u)))), 2143289344u, (((_e278 & 2147483647u) > 2139095040u) || ((_e279 & 2147483647u) > 2139095040u)))) * 255f);
    let _e320 = bitcast<u32>((0.2f + (0.7f * _e171)));
    let _e321 = bitcast<u32>(0f);
    let _e345 = bitcast<u32>(bitcast<f32>(select(select(_e321, _e320, ((_e320 ^ ((0u - (_e320 >> 31u)) | 2147483648u)) > (_e321 ^ ((0u - (_e321 >> 31u)) | 2147483648u)))), 2143289344u, (((_e320 & 2147483647u) > 2139095040u) || ((_e321 & 2147483647u) > 2139095040u)))));
    let _e346 = bitcast<u32>(1f);
    let _e371 = (bitcast<f32>(select(select(_e346, _e345, ((_e345 ^ ((0u - (_e345 >> 31u)) | 2147483648u)) < (_e346 ^ ((0u - (_e346 >> 31u)) | 2147483648u)))), 2143289344u, (((_e345 & 2147483647u) > 2139095040u) || ((_e346 & 2147483647u) > 2139095040u)))) * 255f);
    return unpack4x8unorm((((select(0u, select(select(bitcast<u32>(i32(_e237)), 2147483648u, (_e237 <= -2147483600f)), 2147483647u, (_e237 >= 2147483600f)), (_e237 == _e237)) + (select(0u, select(select(bitcast<u32>(i32(_e304)), 2147483648u, (_e304 <= -2147483600f)), 2147483647u, (_e304 >= 2147483600f)), (_e304 == _e304)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e371)), 2147483648u, (_e371 <= -2147483600f)), 2147483647u, (_e371 >= 2147483600f)), (_e371 == _e371)) << 16u)) + 4278190080u));
}
