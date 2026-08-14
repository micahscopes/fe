struct RasterState {
    p1_: f32,
    p2_: f32,
    p3_: f32,
    p4_: f32,
    p5_: f32,
    p6_: f32,
    p7_: f32,
    p8_: u32,
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
    p59_: f32,
    p60_: u32,
    p61_: f32,
    p62_: f32,
    p63_: f32,
    p64_: f32,
    p65_: f32,
    p66_: f32,
    p67_: f32,
    p68_: f32,
    p69_: f32,
    p70_: f32,
    p71_: f32,
    p72_: f32,
    p73_: f32,
    p74_: f32,
    p75_: f32,
    p76_: f32,
    p77_: f32,
    p78_: f32,
    p79_: f32,
    p80_: f32,
    p81_: f32,
    p82_: f32,
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
    var phi_179_: f32;
    var phi_180_: f32;
    var phi_181_: f32;
    var edge_27_31_phi_179_: f32;
    var edge_27_31_phi_180_: f32;
    var edge_27_31_phi_181_: f32;
    var edge_30_31_phi_179_: f32;
    var edge_30_31_phi_180_: f32;
    var edge_30_31_phi_181_: f32;
    var edge_24_31_phi_179_: f32;
    var edge_24_31_phi_180_: f32;
    var edge_24_31_phi_181_: f32;
    var edge_21_31_phi_179_: f32;
    var edge_21_31_phi_180_: f32;
    var edge_21_31_phi_181_: f32;
    var edge_18_31_phi_179_: f32;
    var edge_18_31_phi_180_: f32;
    var edge_18_31_phi_181_: f32;
    var edge_15_31_phi_179_: f32;
    var edge_15_31_phi_180_: f32;
    var edge_15_31_phi_181_: f32;
    var edge_12_31_phi_179_: f32;
    var edge_12_31_phi_180_: f32;
    var edge_12_31_phi_181_: f32;
    var edge_9_31_phi_179_: f32;
    var edge_9_31_phi_180_: f32;
    var edge_9_31_phi_181_: f32;
    var edge_0_31_phi_179_: f32;
    var edge_0_31_phi_180_: f32;
    var edge_0_31_phi_181_: f32;
    var structured_result_1: f32;
    var structured_did_return_1: bool = false;
    var phi_218_: f32;
    var edge_59_61_phi_218_: f32;
    var edge_31_61_phi_218_: f32;
    var structured_result_2: f32;
    var structured_did_return_2: bool = false;
    var phi_221_: f32;
    var edge_62_64_phi_221_: f32;
    var edge_61_64_phi_221_: f32;
    var structured_result_3: f32;
    var structured_did_return_3: bool = false;
    var phi_224_: f32;
    var edge_65_67_phi_224_: f32;
    var edge_64_67_phi_224_: f32;
    var structured_result_4: f32;
    var structured_did_return_4: bool = false;
    var phi_227_: f32;
    var edge_68_70_phi_227_: f32;
    var edge_67_70_phi_227_: f32;
    var structured_result_5: f32;
    var structured_did_return_5: bool = false;
    var phi_231_: f32;
    var phi_232_: f32;
    var edge_71_73_phi_231_: f32;
    var edge_71_73_phi_232_: f32;
    var edge_70_73_phi_231_: f32;
    var edge_70_73_phi_232_: f32;
    var structured_result_6: f32;
    var structured_did_return_6: bool = false;
    var phi_242_: f32;
    var phi_244_: f32;
    var phi_245_: f32;
    var phi_246_: f32;
    var edge_74_76_phi_242_: f32;
    var edge_74_76_phi_244_: f32;
    var edge_74_76_phi_245_: f32;
    var edge_74_76_phi_246_: f32;
    var edge_73_76_phi_242_: f32;
    var edge_73_76_phi_244_: f32;
    var edge_73_76_phi_245_: f32;
    var edge_73_76_phi_246_: f32;

    let _e11 = state.p5_;
    let _e13 = state.p6_;
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
    let _e119 = state.p59_;
    let _e123 = state.p61_;
    let _e125 = state.p62_;
    let _e127 = state.p63_;
    let _e129 = state.p64_;
    let _e131 = state.p65_;
    let _e133 = state.p66_;
    let _e135 = state.p67_;
    let _e137 = state.p68_;
    let _e139 = state.p69_;
    let _e141 = state.p70_;
    let _e143 = state.p71_;
    let _e145 = state.p72_;
    let _e168 = (vertex_index / 6u);
    let _e170 = (vertex_index % 6u);
    if (_e168 == 0u) {
        edge_0_31_phi_179_ = _e71;
        edge_0_31_phi_180_ = _e69;
        edge_0_31_phi_181_ = _e67;
        let _e287 = edge_0_31_phi_179_;
        let _e289 = edge_0_31_phi_180_;
        let _e291 = edge_0_31_phi_181_;
        phi_179_ = _e287;
        phi_180_ = _e289;
        phi_181_ = _e291;
    } else {
        if (_e168 == 1u) {
            edge_9_31_phi_179_ = _e77;
            edge_9_31_phi_180_ = _e75;
            edge_9_31_phi_181_ = _e73;
            let _e275 = edge_9_31_phi_179_;
            let _e277 = edge_9_31_phi_180_;
            let _e279 = edge_9_31_phi_181_;
            phi_179_ = _e275;
            phi_180_ = _e277;
            phi_181_ = _e279;
        } else {
            if (_e168 == 2u) {
                edge_12_31_phi_179_ = _e83;
                edge_12_31_phi_180_ = _e81;
                edge_12_31_phi_181_ = _e79;
                let _e263 = edge_12_31_phi_179_;
                let _e265 = edge_12_31_phi_180_;
                let _e267 = edge_12_31_phi_181_;
                phi_179_ = _e263;
                phi_180_ = _e265;
                phi_181_ = _e267;
            } else {
                if (_e168 == 3u) {
                    edge_15_31_phi_179_ = _e89;
                    edge_15_31_phi_180_ = _e87;
                    edge_15_31_phi_181_ = _e85;
                    let _e251 = edge_15_31_phi_179_;
                    let _e253 = edge_15_31_phi_180_;
                    let _e255 = edge_15_31_phi_181_;
                    phi_179_ = _e251;
                    phi_180_ = _e253;
                    phi_181_ = _e255;
                } else {
                    if (_e168 == 4u) {
                        edge_18_31_phi_179_ = _e95;
                        edge_18_31_phi_180_ = _e93;
                        edge_18_31_phi_181_ = _e91;
                        let _e239 = edge_18_31_phi_179_;
                        let _e241 = edge_18_31_phi_180_;
                        let _e243 = edge_18_31_phi_181_;
                        phi_179_ = _e239;
                        phi_180_ = _e241;
                        phi_181_ = _e243;
                    } else {
                        if (_e168 == 5u) {
                            edge_21_31_phi_179_ = _e101;
                            edge_21_31_phi_180_ = _e99;
                            edge_21_31_phi_181_ = _e97;
                            let _e227 = edge_21_31_phi_179_;
                            let _e229 = edge_21_31_phi_180_;
                            let _e231 = edge_21_31_phi_181_;
                            phi_179_ = _e227;
                            phi_180_ = _e229;
                            phi_181_ = _e231;
                        } else {
                            if (_e168 == 6u) {
                                edge_24_31_phi_179_ = _e107;
                                edge_24_31_phi_180_ = _e105;
                                edge_24_31_phi_181_ = _e103;
                                let _e215 = edge_24_31_phi_179_;
                                let _e217 = edge_24_31_phi_180_;
                                let _e219 = edge_24_31_phi_181_;
                                phi_179_ = _e215;
                                phi_180_ = _e217;
                                phi_181_ = _e219;
                            } else {
                                if (_e168 == 7u) {
                                    edge_27_31_phi_179_ = _e113;
                                    edge_27_31_phi_180_ = _e111;
                                    edge_27_31_phi_181_ = _e109;
                                    let _e191 = edge_27_31_phi_179_;
                                    let _e193 = edge_27_31_phi_180_;
                                    let _e195 = edge_27_31_phi_181_;
                                    phi_179_ = _e191;
                                    phi_180_ = _e193;
                                    phi_181_ = _e195;
                                } else {
                                    edge_30_31_phi_179_ = _e119;
                                    edge_30_31_phi_180_ = _e117;
                                    edge_30_31_phi_181_ = _e115;
                                    let _e203 = edge_30_31_phi_179_;
                                    let _e205 = edge_30_31_phi_180_;
                                    let _e207 = edge_30_31_phi_181_;
                                    phi_179_ = _e203;
                                    phi_180_ = _e205;
                                    phi_181_ = _e207;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let _e297 = phi_179_;
    let _e299 = phi_180_;
    let _e301 = phi_181_;
    let _e302 = (_e301 - _e123);
    let _e303 = (_e299 - _e125);
    let _e304 = (_e297 - _e127);
    let _e319 = (((_e302 * _e141) + (_e303 * _e143)) + (_e304 * _e145));
    let _e331 = bitcast<u32>(_e11);
    let _e332 = bitcast<u32>(1f);
    let _e357 = (-12f / bitcast<f32>(select(select(_e332, _e331, ((_e331 ^ ((0u - (_e331 >> 31u)) | 2147483648u)) > (_e332 ^ ((0u - (_e332 >> 31u)) | 2147483648u)))), 2143289344u, (((_e331 & 2147483647u) > 2139095040u) || ((_e332 & 2147483647u) > 2139095040u)))));
    let _e359 = bitcast<u32>(_e13);
    let _e360 = bitcast<u32>(1f);
    let _e385 = (-12f / bitcast<f32>(select(select(_e360, _e359, ((_e359 ^ ((0u - (_e359 >> 31u)) | 2147483648u)) > (_e360 ^ ((0u - (_e360 >> 31u)) | 2147483648u)))), 2143289344u, (((_e359 & 2147483647u) > 2139095040u) || ((_e360 & 2147483647u) > 2139095040u)))));
    if (_e170 == 1u) {
        edge_59_61_phi_218_ = (0f - _e357);
        let _e392 = edge_59_61_phi_218_;
        phi_218_ = _e392;
    } else {
        edge_31_61_phi_218_ = _e357;
        let _e396 = edge_31_61_phi_218_;
        phi_218_ = _e396;
    }
    let _e400 = phi_218_;
    if (_e170 == 2u) {
        edge_62_64_phi_221_ = (0f - _e385);
        let _e407 = edge_62_64_phi_221_;
        phi_221_ = _e407;
    } else {
        edge_61_64_phi_221_ = _e385;
        let _e411 = edge_61_64_phi_221_;
        phi_221_ = _e411;
    }
    let _e415 = phi_221_;
    if (_e170 == 3u) {
        edge_65_67_phi_224_ = (0f - _e415);
        let _e422 = edge_65_67_phi_224_;
        phi_224_ = _e422;
    } else {
        edge_64_67_phi_224_ = _e415;
        let _e426 = edge_64_67_phi_224_;
        phi_224_ = _e426;
    }
    let _e430 = phi_224_;
    if (_e170 == 4u) {
        edge_68_70_phi_227_ = (0f - _e400);
        let _e437 = edge_68_70_phi_227_;
        phi_227_ = _e437;
    } else {
        edge_67_70_phi_227_ = _e400;
        let _e441 = edge_67_70_phi_227_;
        phi_227_ = _e441;
    }
    let _e445 = phi_227_;
    if (_e170 == 5u) {
        edge_71_73_phi_231_ = (0f - _e430);
        edge_71_73_phi_232_ = (0f - _e445);
        let _e455 = edge_71_73_phi_231_;
        let _e457 = edge_71_73_phi_232_;
        phi_231_ = _e455;
        phi_232_ = _e457;
    } else {
        edge_70_73_phi_231_ = _e430;
        edge_70_73_phi_232_ = _e445;
        let _e463 = edge_70_73_phi_231_;
        let _e465 = edge_70_73_phi_232_;
        phi_231_ = _e463;
        phi_232_ = _e465;
    }
    let _e470 = phi_231_;
    let _e472 = phi_232_;
    if (0.25f < _e319) {
        edge_74_76_phi_242_ = _e319;
        edge_74_76_phi_244_ = ((((_e319 * 64f) - 16f) / 63.75f) - (0.001f * _e319));
        edge_74_76_phi_245_ = (((((_e302 * _e135) + (_e303 * _e137)) + (_e304 * _e139)) * 1.6f) + (_e470 * _e319));
        edge_74_76_phi_246_ = (((((_e302 * _e129) + (_e303 * _e131)) + (_e304 * _e133)) * 1.6f) + (_e472 * _e319));
        let _e487 = edge_74_76_phi_242_;
        let _e489 = edge_74_76_phi_244_;
        let _e491 = edge_74_76_phi_245_;
        let _e493 = edge_74_76_phi_246_;
        phi_242_ = _e487;
        phi_244_ = _e489;
        phi_245_ = _e491;
        phi_246_ = _e493;
    } else {
        edge_73_76_phi_242_ = 1f;
        edge_73_76_phi_244_ = 2f;
        edge_73_76_phi_245_ = 2f;
        edge_73_76_phi_246_ = 2f;
        let _e507 = edge_73_76_phi_242_;
        let _e509 = edge_73_76_phi_244_;
        let _e511 = edge_73_76_phi_245_;
        let _e513 = edge_73_76_phi_246_;
        phi_242_ = _e507;
        phi_244_ = _e509;
        phi_245_ = _e511;
        phi_246_ = _e513;
    }
    let _e519 = phi_242_;
    let _e521 = phi_244_;
    let _e523 = phi_245_;
    let _e525 = phi_246_;
    return RasterVertexOutput(vec4<f32>(_e525, _e523, _e521, _e519), (f32(bitcast<i32>(_e168)) / 8f));
}

@fragment
fn marker_fragment(@location(0) v0_: f32) -> @location(0) vec4<f32> {
    let _e168 = bitcast<u32>(v0_);
    let _e169 = bitcast<u32>(0f);
    let _e193 = bitcast<u32>(bitcast<f32>(select(select(_e169, _e168, ((_e168 ^ ((0u - (_e168 >> 31u)) | 2147483648u)) > (_e169 ^ ((0u - (_e169 >> 31u)) | 2147483648u)))), 2143289344u, (((_e168 & 2147483647u) > 2139095040u) || ((_e169 & 2147483647u) > 2139095040u)))));
    let _e194 = bitcast<u32>(1f);
    let _e217 = bitcast<f32>(select(select(_e194, _e193, ((_e193 ^ ((0u - (_e193 >> 31u)) | 2147483648u)) < (_e194 ^ ((0u - (_e194 >> 31u)) | 2147483648u)))), 2143289344u, (((_e193 & 2147483647u) > 2139095040u) || ((_e194 & 2147483647u) > 2139095040u))));
    let _e232 = bitcast<u32>((0.98f + (-0.08000004f * _e217)));
    let _e233 = bitcast<u32>(0f);
    let _e257 = bitcast<u32>(bitcast<f32>(select(select(_e233, _e232, ((_e232 ^ ((0u - (_e232 >> 31u)) | 2147483648u)) > (_e233 ^ ((0u - (_e233 >> 31u)) | 2147483648u)))), 2143289344u, (((_e232 & 2147483647u) > 2139095040u) || ((_e233 & 2147483647u) > 2139095040u)))));
    let _e258 = bitcast<u32>(1f);
    let _e283 = (bitcast<f32>(select(select(_e258, _e257, ((_e257 ^ ((0u - (_e257 >> 31u)) | 2147483648u)) < (_e258 ^ ((0u - (_e258 >> 31u)) | 2147483648u)))), 2143289344u, (((_e257 & 2147483647u) > 2139095040u) || ((_e258 & 2147483647u) > 2139095040u)))) * 255f);
    let _e299 = bitcast<u32>((0.8f + (0.099999964f * _e217)));
    let _e300 = bitcast<u32>(0f);
    let _e324 = bitcast<u32>(bitcast<f32>(select(select(_e300, _e299, ((_e299 ^ ((0u - (_e299 >> 31u)) | 2147483648u)) > (_e300 ^ ((0u - (_e300 >> 31u)) | 2147483648u)))), 2143289344u, (((_e299 & 2147483647u) > 2139095040u) || ((_e300 & 2147483647u) > 2139095040u)))));
    let _e325 = bitcast<u32>(1f);
    let _e350 = (bitcast<f32>(select(select(_e325, _e324, ((_e324 ^ ((0u - (_e324 >> 31u)) | 2147483648u)) < (_e325 ^ ((0u - (_e325 >> 31u)) | 2147483648u)))), 2143289344u, (((_e324 & 2147483647u) > 2139095040u) || ((_e325 & 2147483647u) > 2139095040u)))) * 255f);
    let _e366 = bitcast<u32>((0.2f + (0.7f * _e217)));
    let _e367 = bitcast<u32>(0f);
    let _e391 = bitcast<u32>(bitcast<f32>(select(select(_e367, _e366, ((_e366 ^ ((0u - (_e366 >> 31u)) | 2147483648u)) > (_e367 ^ ((0u - (_e367 >> 31u)) | 2147483648u)))), 2143289344u, (((_e366 & 2147483647u) > 2139095040u) || ((_e367 & 2147483647u) > 2139095040u)))));
    let _e392 = bitcast<u32>(1f);
    let _e417 = (bitcast<f32>(select(select(_e392, _e391, ((_e391 ^ ((0u - (_e391 >> 31u)) | 2147483648u)) < (_e392 ^ ((0u - (_e392 >> 31u)) | 2147483648u)))), 2143289344u, (((_e391 & 2147483647u) > 2139095040u) || ((_e392 & 2147483647u) > 2139095040u)))) * 255f);
    return unpack4x8unorm((((select(0u, select(select(bitcast<u32>(i32(_e283)), 2147483648u, (_e283 <= -2147483600f)), 2147483647u, (_e283 >= 2147483600f)), (_e283 == _e283)) + (select(0u, select(select(bitcast<u32>(i32(_e350)), 2147483648u, (_e350 <= -2147483600f)), 2147483647u, (_e350 >= 2147483600f)), (_e350 == _e350)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e417)), 2147483648u, (_e417 <= -2147483600f)), 2147483647u, (_e417 >= 2147483600f)), (_e417 == _e417)) << 16u)) + 4278190080u));
}
