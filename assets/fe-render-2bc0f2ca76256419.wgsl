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
    p60_: f32,
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
    var phi_177_: f32;
    var phi_178_: f32;
    var phi_179_: f32;
    var edge_27_31_phi_177_: f32;
    var edge_27_31_phi_178_: f32;
    var edge_27_31_phi_179_: f32;
    var edge_30_31_phi_177_: f32;
    var edge_30_31_phi_178_: f32;
    var edge_30_31_phi_179_: f32;
    var edge_24_31_phi_177_: f32;
    var edge_24_31_phi_178_: f32;
    var edge_24_31_phi_179_: f32;
    var edge_21_31_phi_177_: f32;
    var edge_21_31_phi_178_: f32;
    var edge_21_31_phi_179_: f32;
    var edge_18_31_phi_177_: f32;
    var edge_18_31_phi_178_: f32;
    var edge_18_31_phi_179_: f32;
    var edge_15_31_phi_177_: f32;
    var edge_15_31_phi_178_: f32;
    var edge_15_31_phi_179_: f32;
    var edge_12_31_phi_177_: f32;
    var edge_12_31_phi_178_: f32;
    var edge_12_31_phi_179_: f32;
    var edge_9_31_phi_177_: f32;
    var edge_9_31_phi_178_: f32;
    var edge_9_31_phi_179_: f32;
    var edge_0_31_phi_177_: f32;
    var edge_0_31_phi_178_: f32;
    var edge_0_31_phi_179_: f32;
    var structured_result_1: f32;
    var structured_did_return_1: bool = false;
    var phi_216_: f32;
    var edge_59_61_phi_216_: f32;
    var edge_31_61_phi_216_: f32;
    var structured_result_2: f32;
    var structured_did_return_2: bool = false;
    var phi_219_: f32;
    var edge_62_64_phi_219_: f32;
    var edge_61_64_phi_219_: f32;
    var structured_result_3: f32;
    var structured_did_return_3: bool = false;
    var phi_222_: f32;
    var edge_65_67_phi_222_: f32;
    var edge_64_67_phi_222_: f32;
    var structured_result_4: f32;
    var structured_did_return_4: bool = false;
    var phi_225_: f32;
    var edge_68_70_phi_225_: f32;
    var edge_67_70_phi_225_: f32;
    var structured_result_5: f32;
    var structured_did_return_5: bool = false;
    var phi_229_: f32;
    var phi_230_: f32;
    var edge_71_73_phi_229_: f32;
    var edge_71_73_phi_230_: f32;
    var edge_70_73_phi_229_: f32;
    var edge_70_73_phi_230_: f32;
    var structured_result_6: f32;
    var structured_did_return_6: bool = false;
    var phi_240_: f32;
    var phi_242_: f32;
    var phi_243_: f32;
    var phi_244_: f32;
    var edge_74_76_phi_240_: f32;
    var edge_74_76_phi_242_: f32;
    var edge_74_76_phi_243_: f32;
    var edge_74_76_phi_244_: f32;
    var edge_73_76_phi_240_: f32;
    var edge_73_76_phi_242_: f32;
    var edge_73_76_phi_243_: f32;
    var edge_73_76_phi_244_: f32;

    let _e11 = state.p5_;
    let _e13 = state.p6_;
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
    let _e121 = state.p60_;
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
    let _e166 = (vertex_index / 6u);
    let _e168 = (vertex_index % 6u);
    if (_e166 == 0u) {
        edge_0_31_phi_177_ = _e69;
        edge_0_31_phi_178_ = _e67;
        edge_0_31_phi_179_ = _e65;
        let _e285 = edge_0_31_phi_177_;
        let _e287 = edge_0_31_phi_178_;
        let _e289 = edge_0_31_phi_179_;
        phi_177_ = _e285;
        phi_178_ = _e287;
        phi_179_ = _e289;
    } else {
        if (_e166 == 1u) {
            edge_9_31_phi_177_ = _e75;
            edge_9_31_phi_178_ = _e73;
            edge_9_31_phi_179_ = _e71;
            let _e273 = edge_9_31_phi_177_;
            let _e275 = edge_9_31_phi_178_;
            let _e277 = edge_9_31_phi_179_;
            phi_177_ = _e273;
            phi_178_ = _e275;
            phi_179_ = _e277;
        } else {
            if (_e166 == 2u) {
                edge_12_31_phi_177_ = _e81;
                edge_12_31_phi_178_ = _e79;
                edge_12_31_phi_179_ = _e77;
                let _e261 = edge_12_31_phi_177_;
                let _e263 = edge_12_31_phi_178_;
                let _e265 = edge_12_31_phi_179_;
                phi_177_ = _e261;
                phi_178_ = _e263;
                phi_179_ = _e265;
            } else {
                if (_e166 == 3u) {
                    edge_15_31_phi_177_ = _e87;
                    edge_15_31_phi_178_ = _e85;
                    edge_15_31_phi_179_ = _e83;
                    let _e249 = edge_15_31_phi_177_;
                    let _e251 = edge_15_31_phi_178_;
                    let _e253 = edge_15_31_phi_179_;
                    phi_177_ = _e249;
                    phi_178_ = _e251;
                    phi_179_ = _e253;
                } else {
                    if (_e166 == 4u) {
                        edge_18_31_phi_177_ = _e93;
                        edge_18_31_phi_178_ = _e91;
                        edge_18_31_phi_179_ = _e89;
                        let _e237 = edge_18_31_phi_177_;
                        let _e239 = edge_18_31_phi_178_;
                        let _e241 = edge_18_31_phi_179_;
                        phi_177_ = _e237;
                        phi_178_ = _e239;
                        phi_179_ = _e241;
                    } else {
                        if (_e166 == 5u) {
                            edge_21_31_phi_177_ = _e99;
                            edge_21_31_phi_178_ = _e97;
                            edge_21_31_phi_179_ = _e95;
                            let _e225 = edge_21_31_phi_177_;
                            let _e227 = edge_21_31_phi_178_;
                            let _e229 = edge_21_31_phi_179_;
                            phi_177_ = _e225;
                            phi_178_ = _e227;
                            phi_179_ = _e229;
                        } else {
                            if (_e166 == 6u) {
                                edge_24_31_phi_177_ = _e105;
                                edge_24_31_phi_178_ = _e103;
                                edge_24_31_phi_179_ = _e101;
                                let _e213 = edge_24_31_phi_177_;
                                let _e215 = edge_24_31_phi_178_;
                                let _e217 = edge_24_31_phi_179_;
                                phi_177_ = _e213;
                                phi_178_ = _e215;
                                phi_179_ = _e217;
                            } else {
                                if (_e166 == 7u) {
                                    edge_27_31_phi_177_ = _e111;
                                    edge_27_31_phi_178_ = _e109;
                                    edge_27_31_phi_179_ = _e107;
                                    let _e189 = edge_27_31_phi_177_;
                                    let _e191 = edge_27_31_phi_178_;
                                    let _e193 = edge_27_31_phi_179_;
                                    phi_177_ = _e189;
                                    phi_178_ = _e191;
                                    phi_179_ = _e193;
                                } else {
                                    edge_30_31_phi_177_ = _e117;
                                    edge_30_31_phi_178_ = _e115;
                                    edge_30_31_phi_179_ = _e113;
                                    let _e201 = edge_30_31_phi_177_;
                                    let _e203 = edge_30_31_phi_178_;
                                    let _e205 = edge_30_31_phi_179_;
                                    phi_177_ = _e201;
                                    phi_178_ = _e203;
                                    phi_179_ = _e205;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let _e295 = phi_177_;
    let _e297 = phi_178_;
    let _e299 = phi_179_;
    let _e300 = (_e299 - _e121);
    let _e301 = (_e297 - _e123);
    let _e302 = (_e295 - _e125);
    let _e317 = (((_e300 * _e139) + (_e301 * _e141)) + (_e302 * _e143));
    let _e329 = bitcast<u32>(_e11);
    let _e330 = bitcast<u32>(1f);
    let _e355 = (-12f / bitcast<f32>(select(select(_e330, _e329, ((_e329 ^ ((0u - (_e329 >> 31u)) | 2147483648u)) > (_e330 ^ ((0u - (_e330 >> 31u)) | 2147483648u)))), 2143289344u, (((_e329 & 2147483647u) > 2139095040u) || ((_e330 & 2147483647u) > 2139095040u)))));
    let _e357 = bitcast<u32>(_e13);
    let _e358 = bitcast<u32>(1f);
    let _e383 = (-12f / bitcast<f32>(select(select(_e358, _e357, ((_e357 ^ ((0u - (_e357 >> 31u)) | 2147483648u)) > (_e358 ^ ((0u - (_e358 >> 31u)) | 2147483648u)))), 2143289344u, (((_e357 & 2147483647u) > 2139095040u) || ((_e358 & 2147483647u) > 2139095040u)))));
    if (_e168 == 1u) {
        edge_59_61_phi_216_ = (0f - _e355);
        let _e390 = edge_59_61_phi_216_;
        phi_216_ = _e390;
    } else {
        edge_31_61_phi_216_ = _e355;
        let _e394 = edge_31_61_phi_216_;
        phi_216_ = _e394;
    }
    let _e398 = phi_216_;
    if (_e168 == 2u) {
        edge_62_64_phi_219_ = (0f - _e383);
        let _e405 = edge_62_64_phi_219_;
        phi_219_ = _e405;
    } else {
        edge_61_64_phi_219_ = _e383;
        let _e409 = edge_61_64_phi_219_;
        phi_219_ = _e409;
    }
    let _e413 = phi_219_;
    if (_e168 == 3u) {
        edge_65_67_phi_222_ = (0f - _e413);
        let _e420 = edge_65_67_phi_222_;
        phi_222_ = _e420;
    } else {
        edge_64_67_phi_222_ = _e413;
        let _e424 = edge_64_67_phi_222_;
        phi_222_ = _e424;
    }
    let _e428 = phi_222_;
    if (_e168 == 4u) {
        edge_68_70_phi_225_ = (0f - _e398);
        let _e435 = edge_68_70_phi_225_;
        phi_225_ = _e435;
    } else {
        edge_67_70_phi_225_ = _e398;
        let _e439 = edge_67_70_phi_225_;
        phi_225_ = _e439;
    }
    let _e443 = phi_225_;
    if (_e168 == 5u) {
        edge_71_73_phi_229_ = (0f - _e428);
        edge_71_73_phi_230_ = (0f - _e443);
        let _e453 = edge_71_73_phi_229_;
        let _e455 = edge_71_73_phi_230_;
        phi_229_ = _e453;
        phi_230_ = _e455;
    } else {
        edge_70_73_phi_229_ = _e428;
        edge_70_73_phi_230_ = _e443;
        let _e461 = edge_70_73_phi_229_;
        let _e463 = edge_70_73_phi_230_;
        phi_229_ = _e461;
        phi_230_ = _e463;
    }
    let _e468 = phi_229_;
    let _e470 = phi_230_;
    if (0.25f < _e317) {
        edge_74_76_phi_240_ = _e317;
        edge_74_76_phi_242_ = ((((_e317 * 64f) - 16f) / 63.75f) - (0.001f * _e317));
        edge_74_76_phi_243_ = (((((_e300 * _e133) + (_e301 * _e135)) + (_e302 * _e137)) * 1.6f) + (_e468 * _e317));
        edge_74_76_phi_244_ = (((((_e300 * _e127) + (_e301 * _e129)) + (_e302 * _e131)) * 1.6f) + (_e470 * _e317));
        let _e485 = edge_74_76_phi_240_;
        let _e487 = edge_74_76_phi_242_;
        let _e489 = edge_74_76_phi_243_;
        let _e491 = edge_74_76_phi_244_;
        phi_240_ = _e485;
        phi_242_ = _e487;
        phi_243_ = _e489;
        phi_244_ = _e491;
    } else {
        edge_73_76_phi_240_ = 1f;
        edge_73_76_phi_242_ = 2f;
        edge_73_76_phi_243_ = 2f;
        edge_73_76_phi_244_ = 2f;
        let _e505 = edge_73_76_phi_240_;
        let _e507 = edge_73_76_phi_242_;
        let _e509 = edge_73_76_phi_243_;
        let _e511 = edge_73_76_phi_244_;
        phi_240_ = _e505;
        phi_242_ = _e507;
        phi_243_ = _e509;
        phi_244_ = _e511;
    }
    let _e517 = phi_240_;
    let _e519 = phi_242_;
    let _e521 = phi_243_;
    let _e523 = phi_244_;
    return RasterVertexOutput(vec4<f32>(_e523, _e521, _e519, _e517), (f32(bitcast<i32>(_e166)) / 8f));
}

@fragment
fn marker_fragment(@location(0) v0_: f32) -> @location(0) vec4<f32> {
    let _e166 = bitcast<u32>(v0_);
    let _e167 = bitcast<u32>(0f);
    let _e191 = bitcast<u32>(bitcast<f32>(select(select(_e167, _e166, ((_e166 ^ ((0u - (_e166 >> 31u)) | 2147483648u)) > (_e167 ^ ((0u - (_e167 >> 31u)) | 2147483648u)))), 2143289344u, (((_e166 & 2147483647u) > 2139095040u) || ((_e167 & 2147483647u) > 2139095040u)))));
    let _e192 = bitcast<u32>(1f);
    let _e215 = bitcast<f32>(select(select(_e192, _e191, ((_e191 ^ ((0u - (_e191 >> 31u)) | 2147483648u)) < (_e192 ^ ((0u - (_e192 >> 31u)) | 2147483648u)))), 2143289344u, (((_e191 & 2147483647u) > 2139095040u) || ((_e192 & 2147483647u) > 2139095040u))));
    let _e230 = bitcast<u32>((0.98f + (-0.08000004f * _e215)));
    let _e231 = bitcast<u32>(0f);
    let _e255 = bitcast<u32>(bitcast<f32>(select(select(_e231, _e230, ((_e230 ^ ((0u - (_e230 >> 31u)) | 2147483648u)) > (_e231 ^ ((0u - (_e231 >> 31u)) | 2147483648u)))), 2143289344u, (((_e230 & 2147483647u) > 2139095040u) || ((_e231 & 2147483647u) > 2139095040u)))));
    let _e256 = bitcast<u32>(1f);
    let _e281 = (bitcast<f32>(select(select(_e256, _e255, ((_e255 ^ ((0u - (_e255 >> 31u)) | 2147483648u)) < (_e256 ^ ((0u - (_e256 >> 31u)) | 2147483648u)))), 2143289344u, (((_e255 & 2147483647u) > 2139095040u) || ((_e256 & 2147483647u) > 2139095040u)))) * 255f);
    let _e297 = bitcast<u32>((0.8f + (0.099999964f * _e215)));
    let _e298 = bitcast<u32>(0f);
    let _e322 = bitcast<u32>(bitcast<f32>(select(select(_e298, _e297, ((_e297 ^ ((0u - (_e297 >> 31u)) | 2147483648u)) > (_e298 ^ ((0u - (_e298 >> 31u)) | 2147483648u)))), 2143289344u, (((_e297 & 2147483647u) > 2139095040u) || ((_e298 & 2147483647u) > 2139095040u)))));
    let _e323 = bitcast<u32>(1f);
    let _e348 = (bitcast<f32>(select(select(_e323, _e322, ((_e322 ^ ((0u - (_e322 >> 31u)) | 2147483648u)) < (_e323 ^ ((0u - (_e323 >> 31u)) | 2147483648u)))), 2143289344u, (((_e322 & 2147483647u) > 2139095040u) || ((_e323 & 2147483647u) > 2139095040u)))) * 255f);
    let _e364 = bitcast<u32>((0.2f + (0.7f * _e215)));
    let _e365 = bitcast<u32>(0f);
    let _e389 = bitcast<u32>(bitcast<f32>(select(select(_e365, _e364, ((_e364 ^ ((0u - (_e364 >> 31u)) | 2147483648u)) > (_e365 ^ ((0u - (_e365 >> 31u)) | 2147483648u)))), 2143289344u, (((_e364 & 2147483647u) > 2139095040u) || ((_e365 & 2147483647u) > 2139095040u)))));
    let _e390 = bitcast<u32>(1f);
    let _e415 = (bitcast<f32>(select(select(_e390, _e389, ((_e389 ^ ((0u - (_e389 >> 31u)) | 2147483648u)) < (_e390 ^ ((0u - (_e390 >> 31u)) | 2147483648u)))), 2143289344u, (((_e389 & 2147483647u) > 2139095040u) || ((_e390 & 2147483647u) > 2139095040u)))) * 255f);
    return unpack4x8unorm((((select(0u, select(select(bitcast<u32>(i32(_e281)), 2147483648u, (_e281 <= -2147483600f)), 2147483647u, (_e281 >= 2147483600f)), (_e281 == _e281)) + (select(0u, select(select(bitcast<u32>(i32(_e348)), 2147483648u, (_e348 <= -2147483600f)), 2147483647u, (_e348 >= 2147483600f)), (_e348 == _e348)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e415)), 2147483648u, (_e415 <= -2147483600f)), 2147483647u, (_e415 >= 2147483600f)), (_e415 == _e415)) << 16u)) + 4278190080u));
}
