struct Input {
    p2_: f32,
    p3_: f32,
    p4_: f32,
    p5_: f32,
    p6_: u32,
    p7_: f32,
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
    p58_: u32,
}

@group(0) @binding(1)
var<storage> input: Input;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>((f32(((vi & 1u) << 2u)) - 1f), (f32(((vi & 2u) << 1u)) - 1f), 0f, 1f);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var phi_555_: f32;
    var phi_558_: u32;
    var phi_560_: u32;
    var phi_562_: u32;
    var edge_0_245_phi_555_: f32;
    var edge_0_245_phi_558_: u32;
    var edge_0_245_phi_560_: u32;
    var edge_0_245_phi_562_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_563_: bool;
    var phi_730_: f32;
    var phi_731_: u32;
    var phi_732_: u32;
    var edge_330_328_phi_730_: f32;
    var edge_330_328_phi_731_: u32;
    var edge_330_328_phi_732_: u32;
    var edge_339_328_phi_730_: f32;
    var edge_339_328_phi_731_: u32;
    var edge_339_328_phi_732_: u32;
    var edge_250_328_phi_730_: f32;
    var edge_250_328_phi_731_: u32;
    var edge_250_328_phi_732_: u32;
    var edge_246_328_phi_730_: f32;
    var edge_246_328_phi_731_: u32;
    var edge_246_328_phi_732_: u32;
    var edge_328_245_phi_555_: f32;
    var edge_328_245_phi_558_: u32;
    var edge_328_245_phi_560_: u32;
    var edge_328_245_phi_562_: u32;
    var structured_result: u32;
    var structured_did_return: bool = false;
    var phi_895_: f32;
    var phi_736_: f32;
    var phi_738_: u32;
    var edge_247_344_phi_736_: f32;
    var edge_247_344_phi_738_: u32;
    var loop_result_1: u32;
    var loop_did_return_1: bool = false;
    var loop_header_carry_740_: bool;
    var phi_833_: f32;
    var edge_389_403_phi_833_: f32;
    var edge_347_403_phi_833_: f32;
    var edge_403_344_phi_736_: f32;
    var edge_403_344_phi_738_: u32;
    var edge_344_439_phi_895_: f32;
    var edge_247_439_phi_895_: f32;
    var structured_result_1: u32;
    var structured_did_return_1: bool = false;
    var phi_165_: f32;
    var phi_164_: f32;
    var phi_163_: f32;
    var phi_252_: f32;
    var phi_251_: f32;
    var phi_250_: f32;
    var edge_730_7_phi_252_: f32;
    var edge_730_7_phi_251_: f32;
    var edge_730_7_phi_250_: f32;
    var edge_505_7_phi_252_: f32;
    var edge_505_7_phi_251_: f32;
    var edge_505_7_phi_250_: f32;
    var edge_7_4_phi_165_: f32;
    var edge_7_4_phi_164_: f32;
    var edge_7_4_phi_163_: f32;
    var edge_439_4_phi_165_: f32;
    var edge_439_4_phi_164_: f32;
    var edge_439_4_phi_163_: f32;
    var phi_976_: u32;
    var phi_979_: f32;
    var phi_981_: u32;
    var edge_4_548_phi_976_: u32;
    var edge_4_548_phi_979_: f32;
    var edge_4_548_phi_981_: u32;
    var loop_result_2: u32;
    var loop_did_return_2: bool = false;
    var loop_header_carry_983_: bool;
    var phi_997_: f32;
    var phi_998_: f32;
    var phi_999_: f32;
    var edge_572_576_phi_997_: f32;
    var edge_572_576_phi_998_: f32;
    var edge_572_576_phi_999_: f32;
    var edge_575_576_phi_997_: f32;
    var edge_575_576_phi_998_: f32;
    var edge_575_576_phi_999_: f32;
    var edge_569_576_phi_997_: f32;
    var edge_569_576_phi_998_: f32;
    var edge_569_576_phi_999_: f32;
    var edge_566_576_phi_997_: f32;
    var edge_566_576_phi_998_: f32;
    var edge_566_576_phi_999_: f32;
    var edge_563_576_phi_997_: f32;
    var edge_563_576_phi_998_: f32;
    var edge_563_576_phi_999_: f32;
    var edge_560_576_phi_997_: f32;
    var edge_560_576_phi_998_: f32;
    var edge_560_576_phi_999_: f32;
    var edge_557_576_phi_997_: f32;
    var edge_557_576_phi_998_: f32;
    var edge_557_576_phi_999_: f32;
    var edge_554_576_phi_997_: f32;
    var edge_554_576_phi_998_: f32;
    var edge_554_576_phi_999_: f32;
    var edge_551_576_phi_997_: f32;
    var edge_551_576_phi_998_: f32;
    var edge_551_576_phi_999_: f32;
    var phi_1105_: u32;
    var phi_1106_: f32;
    var edge_683_691_phi_1105_: u32;
    var edge_683_691_phi_1106_: f32;
    var edge_696_691_phi_1105_: u32;
    var edge_696_691_phi_1106_: f32;
    var edge_576_691_phi_1105_: u32;
    var edge_576_691_phi_1106_: f32;
    var edge_691_548_phi_976_: u32;
    var edge_691_548_phi_979_: f32;
    var edge_691_548_phi_981_: u32;
    var structured_result_2: u32;
    var structured_did_return_2: bool = false;
    var phi_1125_: f32;
    var phi_1126_: f32;
    var phi_1127_: f32;
    var edge_700_727_phi_1125_: f32;
    var edge_700_727_phi_1126_: f32;
    var edge_700_727_phi_1127_: f32;
    var edge_682_727_phi_1125_: f32;
    var edge_682_727_phi_1126_: f32;
    var edge_682_727_phi_1127_: f32;
    var structured_result_3: u32;
    var structured_did_return_3: bool = false;
    var phi_316_: f32;
    var phi_315_: f32;
    var phi_314_: f32;
    var edge_727_10_phi_316_: f32;
    var edge_727_10_phi_315_: f32;
    var edge_727_10_phi_314_: f32;
    var edge_9_10_phi_316_: f32;
    var edge_9_10_phi_315_: f32;
    var edge_9_10_phi_314_: f32;

    let _e3 = u32(pos.x);
    let _e5 = u32(pos.y);
    let _e7 = input.p2_;
    let _e9 = input.p3_;
    let _e11 = input.p4_;
    let _e13 = input.p5_;
    let _e17 = input.p7_;
    let _e19 = input.p8_;
    let _e21 = input.p9_;
    let _e25 = input.p11_;
    let _e27 = input.p12_;
    let _e29 = input.p13_;
    let _e31 = input.p14_;
    let _e33 = input.p15_;
    let _e35 = input.p16_;
    let _e37 = input.p17_;
    let _e39 = input.p18_;
    let _e41 = input.p19_;
    let _e43 = input.p20_;
    let _e45 = input.p21_;
    let _e47 = input.p22_;
    let _e49 = input.p23_;
    let _e51 = input.p24_;
    let _e53 = input.p25_;
    let _e55 = input.p26_;
    let _e57 = input.p27_;
    let _e59 = input.p28_;
    let _e61 = input.p29_;
    let _e63 = input.p30_;
    let _e65 = input.p31_;
    let _e67 = input.p32_;
    let _e69 = input.p33_;
    let _e71 = input.p34_;
    let _e73 = input.p35_;
    let _e75 = input.p36_;
    let _e77 = input.p37_;
    let _e79 = input.p38_;
    let _e81 = input.p39_;
    let _e83 = input.p40_;
    let _e85 = input.p41_;
    let _e87 = input.p42_;
    let _e89 = input.p43_;
    let _e91 = input.p44_;
    let _e93 = input.p45_;
    let _e95 = input.p46_;
    let _e97 = input.p47_;
    let _e99 = input.p48_;
    let _e101 = input.p49_;
    let _e103 = input.p50_;
    let _e105 = input.p51_;
    let _e107 = input.p52_;
    let _e109 = input.p53_;
    let _e111 = input.p54_;
    let _e113 = input.p55_;
    let _e115 = input.p56_;
    let _e117 = input.p57_;
    let _e125 = f32(bitcast<i32>(_e3));
    let _e134 = f32(bitcast<i32>(_e5));
    let _e155 = (_e9 + 1.5707964f);
    let _e163 = (_e155 - (6.2831855f * floor(((_e155 / 6.2831855f) + 0.5f))));
    let _e173 = ((1.2732395f * _e163) - ((0.40528473f * _e163) * bitcast<f32>((bitcast<u32>(_e163) & 2147483647u))));
    let _e182 = ((0.225f * ((_e173 * bitcast<f32>((bitcast<u32>(_e173) & 2147483647u))) - _e173)) + _e173);
    let _e190 = (_e9 - (6.2831855f * floor(((_e9 / 6.2831855f) + 0.5f))));
    let _e200 = ((1.2732395f * _e190) - ((0.40528473f * _e190) * bitcast<f32>((bitcast<u32>(_e190) & 2147483647u))));
    let _e209 = ((0.225f * ((_e200 * bitcast<f32>((bitcast<u32>(_e200) & 2147483647u))) - _e200)) + _e200);
    let _e211 = (_e11 + 1.5707964f);
    let _e219 = (_e211 - (6.2831855f * floor(((_e211 / 6.2831855f) + 0.5f))));
    let _e229 = ((1.2732395f * _e219) - ((0.40528473f * _e219) * bitcast<f32>((bitcast<u32>(_e219) & 2147483647u))));
    let _e238 = ((0.225f * ((_e229 * bitcast<f32>((bitcast<u32>(_e229) & 2147483647u))) - _e229)) + _e229);
    let _e246 = (_e11 - (6.2831855f * floor(((_e11 / 6.2831855f) + 0.5f))));
    let _e256 = ((1.2732395f * _e246) - ((0.40528473f * _e246) * bitcast<f32>((bitcast<u32>(_e246) & 2147483647u))));
    let _e265 = ((0.225f * ((_e256 * bitcast<f32>((bitcast<u32>(_e256) & 2147483647u))) - _e256)) + _e256);
    let _e268 = (_e17 + ((_e209 * _e238) * _e13));
    let _e270 = (_e19 - (_e265 * _e13));
    let _e273 = (_e21 - ((_e182 * _e238) * _e13));
    let _e275 = ((((((_e125 + 0.375f) + (0.25f * f32(bitcast<i32>((_e3 & 1u))))) / 512f) * 2f) - 1f) / 1.6f);
    let _e277 = ((1f - ((((_e134 + 0.375f) + (0.25f * f32(bitcast<i32>((_e5 & 1u))))) / 512f) * 2f)) / 1.6f);
    let _e281 = ((_e238 * _e277) + (_e265 * 1f));
    let _e287 = ((0f - (_e265 * _e277)) + (_e238 * 1f));
    let _e290 = ((_e182 * _e275) - (_e209 * _e287));
    let _e293 = ((_e209 * _e275) + (_e182 * _e287));
    let _e301 = bitcast<u32>(sqrt((((_e290 * _e290) + (_e281 * _e281)) + (_e293 * _e293))));
    let _e302 = bitcast<u32>(0.0000001f);
    let _e327 = (1f / bitcast<f32>(select(select(_e302, _e301, ((_e301 ^ ((0u - (_e301 >> 31u)) | 2147483648u)) > (_e302 ^ ((0u - (_e302 >> 31u)) | 2147483648u)))), 2143289344u, (((_e301 & 2147483647u) > 2139095040u) || ((_e302 & 2147483647u) > 2139095040u)))));
    let _e328 = (_e290 * _e327);
    let _e329 = (_e281 * _e327);
    let _e330 = (_e293 * _e327);
    let _e332 = (3.1415927f * _e7);
    let _e334 = (_e332 + 1.5707964f);
    let _e342 = (_e334 - (6.2831855f * floor(((_e334 / 6.2831855f) + 0.5f))));
    let _e352 = ((1.2732395f * _e342) - ((0.40528473f * _e342) * bitcast<f32>((bitcast<u32>(_e342) & 2147483647u))));
    let _e361 = ((0.225f * ((_e352 * bitcast<f32>((bitcast<u32>(_e352) & 2147483647u))) - _e352)) + _e352);
    let _e369 = (_e332 - (6.2831855f * floor(((_e332 / 6.2831855f) + 0.5f))));
    let _e379 = ((1.2732395f * _e369) - ((0.40528473f * _e369) * bitcast<f32>((bitcast<u32>(_e369) & 2147483647u))));
    let _e388 = ((0.225f * ((_e379 * bitcast<f32>((bitcast<u32>(_e379) & 2147483647u))) - _e379)) + _e379);
    let _e391 = ((_e361 * _e25) + (_e388 * _e45));
    let _e394 = ((_e361 * _e27) + (_e388 * _e47));
    let _e397 = ((_e361 * _e29) + (_e388 * _e49));
    let _e400 = ((_e361 * _e31) + (_e388 * _e51));
    let _e403 = ((_e361 * _e33) + (_e388 * _e53));
    let _e406 = ((_e361 * _e35) + (_e388 * _e55));
    let _e409 = ((_e361 * _e37) + (_e388 * _e57));
    let _e412 = ((_e361 * _e39) + (_e388 * _e59));
    let _e415 = ((_e361 * _e41) + (_e388 * _e61));
    let _e418 = ((_e361 * _e43) + (_e388 * _e63));
    let _e426 = bitcast<u32>(sqrt((((_e328 * _e328) + (_e329 * _e329)) + (_e330 * _e330))));
    let _e427 = bitcast<u32>(0.0000001f);
    let _e452 = (1f / bitcast<f32>(select(select(_e427, _e426, ((_e426 ^ ((0u - (_e426 >> 31u)) | 2147483648u)) > (_e427 ^ ((0u - (_e427 >> 31u)) | 2147483648u)))), 2143289344u, (((_e426 & 2147483647u) > 2139095040u) || ((_e427 & 2147483647u) > 2139095040u)))));
    let _e453 = (_e328 * _e452);
    let _e454 = (_e329 * _e452);
    let _e455 = (_e330 * _e452);
    edge_0_245_phi_555_ = 0f;
    edge_0_245_phi_558_ = 0u;
    edge_0_245_phi_560_ = 0u;
    edge_0_245_phi_562_ = 0u;
    let _e465 = edge_0_245_phi_555_;
    let _e467 = edge_0_245_phi_558_;
    let _e469 = edge_0_245_phi_560_;
    let _e471 = edge_0_245_phi_562_;
    phi_555_ = _e465;
    phi_558_ = _e467;
    phi_560_ = _e469;
    phi_562_ = _e471;
    loop {
        let _e478 = phi_555_;
        let _e480 = phi_558_;
        let _e482 = phi_560_;
        let _e484 = phi_562_;
        let _e488 = (bitcast<i32>(_e484) < bitcast<i32>(128u));
        if _e488 {
            if (_e482 == 0u) {
                let _e494 = (_e268 + (_e453 * _e478));
                let _e495 = (_e270 + (_e454 * _e478));
                let _e496 = (_e273 + (_e455 * _e478));
                let _e531 = -((1f * (0f - (_e418 / 3f))));
                let _e568 = (((((2f * _e391) * _e494) + (_e400 * _e495)) + (_e403 * _e496)) + _e409);
                let _e576 = (((((2f * _e394) * _e495) + (_e400 * _e494)) + (_e406 * _e496)) + _e412);
                let _e584 = (((((2f * _e397) * _e496) + (_e403 * _e494)) + (_e406 * _e495)) + _e415);
                let _e592 = bitcast<u32>(sqrt((((_e568 * _e568) + (_e576 * _e576)) + (_e584 * _e584))));
                let _e593 = bitcast<u32>(0.00001f);
                let _e617 = (bitcast<f32>((bitcast<u32>(((((((0f + (_e494 * _e409)) + (_e495 * _e412)) + (_e496 * _e415)) + _e531) + ((((0f + -((((_e494 * _e494) * 0.5f) * (-2f * _e391)))) + _e531) + -((((_e495 * _e495) * 0.5f) * (-2f * _e394)))) + _e531)) + ((((0f + -((((_e496 * _e496) * 0.5f) * (-2f * _e397)))) + -(((_e494 * _e495) * -(_e400)))) + -(((_e494 * _e496) * -(_e403)))) + -(((_e495 * _e496) * -(_e406)))))) & 2147483647u)) / bitcast<f32>(select(select(_e593, _e592, ((_e592 ^ ((0u - (_e592 >> 31u)) | 2147483648u)) > (_e593 ^ ((0u - (_e593 >> 31u)) | 2147483648u)))), 2143289344u, (((_e592 & 2147483647u) > 2139095040u) || ((_e593 & 2147483647u) > 2139095040u)))));
                if (_e617 < (0.0014f * (1f + (_e478 * 0.025f)))) {
                    edge_250_328_phi_730_ = _e478;
                    edge_250_328_phi_731_ = 1u;
                    edge_250_328_phi_732_ = 1u;
                    let _e713 = edge_250_328_phi_730_;
                    let _e715 = edge_250_328_phi_731_;
                    let _e717 = edge_250_328_phi_732_;
                    phi_730_ = _e713;
                    phi_731_ = _e715;
                    phi_732_ = _e717;
                } else {
                    let _e629 = bitcast<u32>((_e617 * 0.68f));
                    let _e630 = bitcast<u32>(0.00035f);
                    let _e654 = bitcast<u32>(bitcast<f32>(select(select(_e630, _e629, ((_e629 ^ ((0u - (_e629 >> 31u)) | 2147483648u)) > (_e630 ^ ((0u - (_e630 >> 31u)) | 2147483648u)))), 2143289344u, (((_e629 & 2147483647u) > 2139095040u) || ((_e630 & 2147483647u) > 2139095040u)))));
                    let _e655 = bitcast<u32>(0.48f);
                    let _e679 = (_e478 + bitcast<f32>(select(select(_e655, _e654, ((_e654 ^ ((0u - (_e654 >> 31u)) | 2147483648u)) < (_e655 ^ ((0u - (_e655 >> 31u)) | 2147483648u)))), 2143289344u, (((_e654 & 2147483647u) > 2139095040u) || ((_e655 & 2147483647u) > 2139095040u)))));
                    if (18f < _e679) {
                        edge_330_328_phi_730_ = _e679;
                        edge_330_328_phi_731_ = _e480;
                        edge_330_328_phi_732_ = 1u;
                        let _e687 = edge_330_328_phi_730_;
                        let _e689 = edge_330_328_phi_731_;
                        let _e691 = edge_330_328_phi_732_;
                        phi_730_ = _e687;
                        phi_731_ = _e689;
                        phi_732_ = _e691;
                    } else {
                        edge_339_328_phi_730_ = _e679;
                        edge_339_328_phi_731_ = _e480;
                        edge_339_328_phi_732_ = _e482;
                        let _e699 = edge_339_328_phi_730_;
                        let _e701 = edge_339_328_phi_731_;
                        let _e703 = edge_339_328_phi_732_;
                        phi_730_ = _e699;
                        phi_731_ = _e701;
                        phi_732_ = _e703;
                    }
                }
            } else {
                edge_246_328_phi_730_ = _e478;
                edge_246_328_phi_731_ = _e480;
                edge_246_328_phi_732_ = _e482;
                let _e725 = edge_246_328_phi_730_;
                let _e727 = edge_246_328_phi_731_;
                let _e729 = edge_246_328_phi_732_;
                phi_730_ = _e725;
                phi_731_ = _e727;
                phi_732_ = _e729;
            }
            let _e734 = phi_730_;
            let _e736 = phi_731_;
            let _e738 = phi_732_;
            edge_328_245_phi_555_ = _e734;
            edge_328_245_phi_558_ = _e736;
            edge_328_245_phi_560_ = _e738;
            edge_328_245_phi_562_ = (_e484 + 1u);
            let _e746 = edge_328_245_phi_555_;
            let _e748 = edge_328_245_phi_558_;
            let _e750 = edge_328_245_phi_560_;
            let _e752 = edge_328_245_phi_562_;
            phi_555_ = _e746;
            phi_558_ = _e748;
            phi_560_ = _e750;
            phi_562_ = _e752;
            continue;
        } else {
            loop_header_carry_563_ = _e488;
            break;
        }
    }
    let _e759 = phi_555_;
    let _e761 = phi_558_;
    let _e770 = (_e761 == 1u);
    if _e770 {
        edge_247_344_phi_736_ = _e759;
        edge_247_344_phi_738_ = 0u;
        let _e775 = edge_247_344_phi_736_;
        let _e777 = edge_247_344_phi_738_;
        phi_736_ = _e775;
        phi_738_ = _e777;
        loop {
            let _e782 = phi_736_;
            let _e784 = phi_738_;
            let _e788 = (bitcast<i32>(_e784) < bitcast<i32>(4u));
            if _e788 {
                let _e792 = (_e268 + (_e453 * _e782));
                let _e793 = (_e270 + (_e454 * _e782));
                let _e794 = (_e273 + (_e455 * _e782));
                let _e829 = -((1f * (0f - (_e418 / 3f))));
                let _e883 = ((((((((2f * _e391) * _e792) + (_e400 * _e793)) + (_e403 * _e794)) + _e409) * _e453) + ((((((2f * _e394) * _e793) + (_e400 * _e792)) + (_e406 * _e794)) + _e412) * _e454)) + ((((((2f * _e397) * _e794) + (_e403 * _e792)) + (_e406 * _e793)) + _e415) * _e455));
                if (0.00001f < bitcast<f32>((bitcast<u32>(_e883) & 2147483647u))) {
                    let _e893 = bitcast<u32>((((((((0f + (_e792 * _e409)) + (_e793 * _e412)) + (_e794 * _e415)) + _e829) + ((((0f + -((((_e792 * _e792) * 0.5f) * (-2f * _e391)))) + _e829) + -((((_e793 * _e793) * 0.5f) * (-2f * _e394)))) + _e829)) + ((((0f + -((((_e794 * _e794) * 0.5f) * (-2f * _e397)))) + -(((_e792 * _e793) * -(_e400)))) + -(((_e792 * _e794) * -(_e403)))) + -(((_e793 * _e794) * -(_e406))))) / _e883));
                    let _e894 = bitcast<u32>(-0.12f);
                    let _e918 = bitcast<u32>(bitcast<f32>(select(select(_e894, _e893, ((_e893 ^ ((0u - (_e893 >> 31u)) | 2147483648u)) > (_e894 ^ ((0u - (_e894 >> 31u)) | 2147483648u)))), 2143289344u, (((_e893 & 2147483647u) > 2139095040u) || ((_e894 & 2147483647u) > 2139095040u)))));
                    let _e919 = bitcast<u32>(0.12f);
                    let _e946 = bitcast<u32>((_e782 - bitcast<f32>(select(select(_e919, _e918, ((_e918 ^ ((0u - (_e918 >> 31u)) | 2147483648u)) < (_e919 ^ ((0u - (_e919 >> 31u)) | 2147483648u)))), 2143289344u, (((_e918 & 2147483647u) > 2139095040u) || ((_e919 & 2147483647u) > 2139095040u))))));
                    let _e947 = bitcast<u32>(0f);
                    let _e971 = bitcast<u32>(bitcast<f32>(select(select(_e947, _e946, ((_e946 ^ ((0u - (_e946 >> 31u)) | 2147483648u)) > (_e947 ^ ((0u - (_e947 >> 31u)) | 2147483648u)))), 2143289344u, (((_e946 & 2147483647u) > 2139095040u) || ((_e947 & 2147483647u) > 2139095040u)))));
                    let _e972 = bitcast<u32>(18f);
                    edge_389_403_phi_833_ = bitcast<f32>(select(select(_e972, _e971, ((_e971 ^ ((0u - (_e971 >> 31u)) | 2147483648u)) < (_e972 ^ ((0u - (_e972 >> 31u)) | 2147483648u)))), 2143289344u, (((_e971 & 2147483647u) > 2139095040u) || ((_e972 & 2147483647u) > 2139095040u))));
                    let _e998 = edge_389_403_phi_833_;
                    phi_833_ = _e998;
                } else {
                    edge_347_403_phi_833_ = _e782;
                    let _e1002 = edge_347_403_phi_833_;
                    phi_833_ = _e1002;
                }
                let _e1005 = phi_833_;
                edge_403_344_phi_736_ = _e1005;
                edge_403_344_phi_738_ = (_e784 + 1u);
                let _e1011 = edge_403_344_phi_736_;
                let _e1013 = edge_403_344_phi_738_;
                phi_736_ = _e1011;
                phi_738_ = _e1013;
                continue;
            } else {
                edge_344_439_phi_895_ = _e782;
                let _e1018 = edge_344_439_phi_895_;
                phi_895_ = _e1018;
                loop_header_carry_740_ = _e788;
                break;
            }
        }
    } else {
        edge_247_439_phi_895_ = _e759;
        let _e1029 = edge_247_439_phi_895_;
        phi_895_ = _e1029;
    }
    let _e1033 = phi_895_;
    let _e1040 = bitcast<u32>((0.5f + (0.5f * _e329)));
    let _e1041 = bitcast<u32>(0f);
    let _e1065 = bitcast<u32>(bitcast<f32>(select(select(_e1041, _e1040, ((_e1040 ^ ((0u - (_e1040 >> 31u)) | 2147483648u)) > (_e1041 ^ ((0u - (_e1041 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1040 & 2147483647u) > 2139095040u) || ((_e1041 & 2147483647u) > 2139095040u)))));
    let _e1066 = bitcast<u32>(1f);
    let _e1089 = bitcast<f32>(select(select(_e1066, _e1065, ((_e1065 ^ ((0u - (_e1065 >> 31u)) | 2147483648u)) < (_e1066 ^ ((0u - (_e1066 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1065 & 2147483647u) > 2139095040u) || ((_e1066 & 2147483647u) > 2139095040u))));
    let _e1120 = bitcast<u32>((1f - (bitcast<f32>((bitcast<u32>((_e329 + 0.08f)) & 2147483647u)) * 4f)));
    let _e1121 = bitcast<u32>(0f);
    let _e1145 = bitcast<u32>(bitcast<f32>(select(select(_e1121, _e1120, ((_e1120 ^ ((0u - (_e1120 >> 31u)) | 2147483648u)) > (_e1121 ^ ((0u - (_e1121 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1120 & 2147483647u) > 2139095040u) || ((_e1121 & 2147483647u) > 2139095040u)))));
    let _e1146 = bitcast<u32>(1f);
    let _e1169 = bitcast<f32>(select(select(_e1146, _e1145, ((_e1145 ^ ((0u - (_e1145 >> 31u)) | 2147483648u)) < (_e1146 ^ ((0u - (_e1146 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1145 & 2147483647u) > 2139095040u) || ((_e1146 & 2147483647u) > 2139095040u))));
    let _e1172 = ((_e1169 * _e1169) * 0.14f);
    let _e1179 = ((0.05f + ((0.076f - 0.05f) * _e1089)) + (0.3f * _e1172));
    let _e1180 = ((0.06f + ((0.15959999f - 0.06f) * _e1089)) + (0.9f * _e1172));
    let _e1181 = ((0.09f + ((0.361f - 0.09f) * _e1089)) + (0.42f * _e1172));
    if _e770 {
        let _e1185 = (_e268 + (_e328 * _e1033));
        let _e1186 = (_e270 + (_e329 * _e1033));
        let _e1187 = (_e273 + (_e330 * _e1033));
        let _e1195 = (((((2f * _e391) * _e1185) + (_e400 * _e1186)) + (_e403 * _e1187)) + _e409);
        let _e1203 = (((((2f * _e394) * _e1186) + (_e400 * _e1185)) + (_e406 * _e1187)) + _e412);
        let _e1211 = (((((2f * _e397) * _e1187) + (_e403 * _e1185)) + (_e406 * _e1186)) + _e415);
        let _e1217 = sqrt((((_e1195 * _e1195) + (_e1203 * _e1203)) + (_e1211 * _e1211)));
        let _e1219 = bitcast<u32>(_e1217);
        let _e1220 = bitcast<u32>(0.00001f);
        let _e1245 = (1f / bitcast<f32>(select(select(_e1220, _e1219, ((_e1219 ^ ((0u - (_e1219 >> 31u)) | 2147483648u)) > (_e1220 ^ ((0u - (_e1220 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1219 & 2147483647u) > 2139095040u) || ((_e1220 & 2147483647u) > 2139095040u)))));
        let _e1246 = (_e1195 * _e1245);
        let _e1247 = (_e1203 * _e1245);
        let _e1248 = (_e1211 * _e1245);
        if (0f < (((_e1246 * _e328) + (_e1247 * _e329)) + (_e1248 * _e330))) {
            edge_730_7_phi_252_ = (_e1248 * -1f);
            edge_730_7_phi_251_ = (_e1247 * -1f);
            edge_730_7_phi_250_ = (_e1246 * -1f);
            let _e1266 = edge_730_7_phi_252_;
            let _e1268 = edge_730_7_phi_251_;
            let _e1270 = edge_730_7_phi_250_;
            phi_252_ = _e1266;
            phi_251_ = _e1268;
            phi_250_ = _e1270;
        } else {
            edge_505_7_phi_252_ = _e1248;
            edge_505_7_phi_251_ = _e1247;
            edge_505_7_phi_250_ = _e1246;
            let _e1278 = edge_505_7_phi_252_;
            let _e1280 = edge_505_7_phi_251_;
            let _e1282 = edge_505_7_phi_250_;
            phi_252_ = _e1278;
            phi_251_ = _e1280;
            phi_250_ = _e1282;
        }
        let _e1287 = phi_252_;
        let _e1289 = phi_251_;
        let _e1291 = phi_250_;
        let _e1298 = bitcast<u32>(((_e1217 - 0.001f) / 0.035f));
        let _e1299 = bitcast<u32>(0f);
        let _e1323 = bitcast<u32>(bitcast<f32>(select(select(_e1299, _e1298, ((_e1298 ^ ((0u - (_e1298 >> 31u)) | 2147483648u)) > (_e1299 ^ ((0u - (_e1299 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1298 & 2147483647u) > 2139095040u) || ((_e1299 & 2147483647u) > 2139095040u)))));
        let _e1324 = bitcast<u32>(1f);
        let _e1347 = bitcast<f32>(select(select(_e1324, _e1323, ((_e1323 ^ ((0u - (_e1323 >> 31u)) | 2147483648u)) < (_e1324 ^ ((0u - (_e1324 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1323 & 2147483647u) > 2139095040u) || ((_e1324 & 2147483647u) > 2139095040u))));
        let _e1349 = (_e328 * -1f);
        let _e1351 = (_e329 * -1f);
        let _e1353 = (_e330 * -1f);
        let _e1356 = (_e1349 + ((_e1291 - _e1349) * _e1347));
        let _e1359 = (_e1351 + ((_e1289 - _e1351) * _e1347));
        let _e1362 = (_e1353 + ((_e1287 - _e1353) * _e1347));
        let _e1370 = bitcast<u32>(sqrt((((_e1356 * _e1356) + (_e1359 * _e1359)) + (_e1362 * _e1362))));
        let _e1371 = bitcast<u32>(0.0000001f);
        let _e1396 = (1f / bitcast<f32>(select(select(_e1371, _e1370, ((_e1370 ^ ((0u - (_e1370 >> 31u)) | 2147483648u)) > (_e1371 ^ ((0u - (_e1371 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1370 & 2147483647u) > 2139095040u) || ((_e1371 & 2147483647u) > 2139095040u)))));
        let _e1397 = (_e1356 * _e1396);
        let _e1398 = (_e1359 * _e1396);
        let _e1399 = (_e1362 * _e1396);
        let _e1407 = bitcast<u32>(sqrt((((_e1397 * _e1397) + (_e1398 * _e1398)) + (_e1399 * _e1399))));
        let _e1408 = bitcast<u32>(0.0000001f);
        let _e1433 = (1f / bitcast<f32>(select(select(_e1408, _e1407, ((_e1407 ^ ((0u - (_e1407 >> 31u)) | 2147483648u)) > (_e1408 ^ ((0u - (_e1408 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1407 & 2147483647u) > 2139095040u) || ((_e1408 & 2147483647u) > 2139095040u)))));
        let _e1434 = (_e1397 * _e1433);
        let _e1435 = (_e1398 * _e1433);
        let _e1436 = (_e1399 * _e1433);
        let _e1452 = (0.8f + (0.2f * bitcast<f32>((bitcast<u32>((((_e1434 * 0.37f) + (_e1435 * 0.82f)) + (_e1436 * 0.44f))) & 2147483647u))));
        let _e1463 = (1f - bitcast<f32>((bitcast<u32>((((_e1434 * _e453) + (_e1435 * _e454)) + (_e1436 * _e455))) & 2147483647u)));
        let _e1466 = ((_e1463 * _e1463) * 0.16f);
        let _e1470 = bitcast<u32>(0f);
        let _e1471 = bitcast<u32>(0f);
        let _e1495 = bitcast<u32>(bitcast<f32>(select(select(_e1471, _e1470, ((_e1470 ^ ((0u - (_e1470 >> 31u)) | 2147483648u)) > (_e1471 ^ ((0u - (_e1471 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1470 & 2147483647u) > 2139095040u) || ((_e1471 & 2147483647u) > 2139095040u)))));
        let _e1496 = bitcast<u32>(1f);
        let _e1523 = bitcast<u32>(0.06f);
        let _e1524 = bitcast<u32>(0f);
        let _e1548 = bitcast<u32>(bitcast<f32>(select(select(_e1524, _e1523, ((_e1523 ^ ((0u - (_e1523 >> 31u)) | 2147483648u)) > (_e1524 ^ ((0u - (_e1524 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1523 & 2147483647u) > 2139095040u) || ((_e1524 & 2147483647u) > 2139095040u)))));
        let _e1549 = bitcast<u32>(1f);
        let _e1586 = bitcast<u32>(((0.18f + (0.68f * bitcast<f32>((bitcast<u32>(_e1435) & 2147483647u)))) + (0.14f * bitcast<f32>(select(select(_e1496, _e1495, ((_e1495 ^ ((0u - (_e1495 >> 31u)) | 2147483648u)) < (_e1496 ^ ((0u - (_e1496 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1495 & 2147483647u) > 2139095040u) || ((_e1496 & 2147483647u) > 2139095040u)))))));
        let _e1587 = bitcast<u32>(0f);
        let _e1611 = bitcast<u32>(bitcast<f32>(select(select(_e1587, _e1586, ((_e1586 ^ ((0u - (_e1586 >> 31u)) | 2147483648u)) > (_e1587 ^ ((0u - (_e1587 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1586 & 2147483647u) > 2139095040u) || ((_e1587 & 2147483647u) > 2139095040u)))));
        let _e1612 = bitcast<u32>(1f);
        let _e1635 = bitcast<f32>(select(select(_e1612, _e1611, ((_e1611 ^ ((0u - (_e1611 >> 31u)) | 2147483648u)) < (_e1612 ^ ((0u - (_e1612 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1611 & 2147483647u) > 2139095040u) || ((_e1612 & 2147483647u) > 2139095040u))));
        let _e1658 = ((bitcast<f32>(select(select(_e1549, _e1548, ((_e1548 ^ ((0u - (_e1548 >> 31u)) | 2147483648u)) < (_e1549 ^ ((0u - (_e1549 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1548 & 2147483647u) > 2139095040u) || ((_e1549 & 2147483647u) > 2139095040u)))) * 0.9f) * 0.2f);
        let _e1662 = (((_e1452 * (0.2f + ((0.3f - 0.2f) * _e1635))) + _e1658) + (_e1466 * 0.98f));
        let _e1667 = (((_e1452 * (0.42f + ((0.9f - 0.42f) * _e1635))) + _e1658) + (_e1466 * 0.8f));
        let _e1672 = (((_e1452 * (0.95f + ((0.42f - 0.95f) * _e1635))) + _e1658) + (_e1466 * 0.2f));
        let _e1679 = bitcast<u32>(((_e1033 - 7f) / 11f));
        let _e1680 = bitcast<u32>(0f);
        let _e1704 = bitcast<u32>(bitcast<f32>(select(select(_e1680, _e1679, ((_e1679 ^ ((0u - (_e1679 >> 31u)) | 2147483648u)) > (_e1680 ^ ((0u - (_e1680 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1679 & 2147483647u) > 2139095040u) || ((_e1680 & 2147483647u) > 2139095040u)))));
        let _e1705 = bitcast<u32>(1f);
        let _e1728 = bitcast<f32>(select(select(_e1705, _e1704, ((_e1704 ^ ((0u - (_e1704 >> 31u)) | 2147483648u)) < (_e1705 ^ ((0u - (_e1705 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1704 & 2147483647u) > 2139095040u) || ((_e1705 & 2147483647u) > 2139095040u))));
        let _e1729 = (_e1728 * _e1728);
        edge_7_4_phi_165_ = (_e1672 + ((_e1181 - _e1672) * _e1729));
        edge_7_4_phi_164_ = (_e1667 + ((_e1180 - _e1667) * _e1729));
        edge_7_4_phi_163_ = (_e1662 + ((_e1179 - _e1662) * _e1729));
        let _e1743 = edge_7_4_phi_165_;
        let _e1745 = edge_7_4_phi_164_;
        let _e1747 = edge_7_4_phi_163_;
        phi_165_ = _e1743;
        phi_164_ = _e1745;
        phi_163_ = _e1747;
    } else {
        edge_439_4_phi_165_ = _e1181;
        edge_439_4_phi_164_ = _e1180;
        edge_439_4_phi_163_ = _e1179;
        let _e1755 = edge_439_4_phi_165_;
        let _e1757 = edge_439_4_phi_164_;
        let _e1759 = edge_439_4_phi_163_;
        phi_165_ = _e1755;
        phi_164_ = _e1757;
        phi_163_ = _e1759;
    }
    let _e1764 = phi_165_;
    let _e1766 = phi_164_;
    let _e1768 = phi_163_;
    edge_4_548_phi_976_ = 4294967295u;
    edge_4_548_phi_979_ = 1000000f;
    edge_4_548_phi_981_ = 0u;
    let _e1776 = edge_4_548_phi_976_;
    let _e1778 = edge_4_548_phi_979_;
    let _e1780 = edge_4_548_phi_981_;
    phi_976_ = _e1776;
    phi_979_ = _e1778;
    phi_981_ = _e1780;
    loop {
        let _e1786 = phi_976_;
        let _e1788 = phi_979_;
        let _e1790 = phi_981_;
        let _e1792 = (_e1790 < 9u);
        if _e1792 {
            if (_e1790 == 0u) {
                edge_551_576_phi_997_ = _e69;
                edge_551_576_phi_998_ = _e67;
                edge_551_576_phi_999_ = _e65;
                let _e1909 = edge_551_576_phi_997_;
                let _e1911 = edge_551_576_phi_998_;
                let _e1913 = edge_551_576_phi_999_;
                phi_997_ = _e1909;
                phi_998_ = _e1911;
                phi_999_ = _e1913;
            } else {
                if (_e1790 == 1u) {
                    edge_554_576_phi_997_ = _e75;
                    edge_554_576_phi_998_ = _e73;
                    edge_554_576_phi_999_ = _e71;
                    let _e1897 = edge_554_576_phi_997_;
                    let _e1899 = edge_554_576_phi_998_;
                    let _e1901 = edge_554_576_phi_999_;
                    phi_997_ = _e1897;
                    phi_998_ = _e1899;
                    phi_999_ = _e1901;
                } else {
                    if (_e1790 == 2u) {
                        edge_557_576_phi_997_ = _e81;
                        edge_557_576_phi_998_ = _e79;
                        edge_557_576_phi_999_ = _e77;
                        let _e1885 = edge_557_576_phi_997_;
                        let _e1887 = edge_557_576_phi_998_;
                        let _e1889 = edge_557_576_phi_999_;
                        phi_997_ = _e1885;
                        phi_998_ = _e1887;
                        phi_999_ = _e1889;
                    } else {
                        if (_e1790 == 3u) {
                            edge_560_576_phi_997_ = _e87;
                            edge_560_576_phi_998_ = _e85;
                            edge_560_576_phi_999_ = _e83;
                            let _e1873 = edge_560_576_phi_997_;
                            let _e1875 = edge_560_576_phi_998_;
                            let _e1877 = edge_560_576_phi_999_;
                            phi_997_ = _e1873;
                            phi_998_ = _e1875;
                            phi_999_ = _e1877;
                        } else {
                            if (_e1790 == 4u) {
                                edge_563_576_phi_997_ = _e93;
                                edge_563_576_phi_998_ = _e91;
                                edge_563_576_phi_999_ = _e89;
                                let _e1861 = edge_563_576_phi_997_;
                                let _e1863 = edge_563_576_phi_998_;
                                let _e1865 = edge_563_576_phi_999_;
                                phi_997_ = _e1861;
                                phi_998_ = _e1863;
                                phi_999_ = _e1865;
                            } else {
                                if (_e1790 == 5u) {
                                    edge_566_576_phi_997_ = _e99;
                                    edge_566_576_phi_998_ = _e97;
                                    edge_566_576_phi_999_ = _e95;
                                    let _e1849 = edge_566_576_phi_997_;
                                    let _e1851 = edge_566_576_phi_998_;
                                    let _e1853 = edge_566_576_phi_999_;
                                    phi_997_ = _e1849;
                                    phi_998_ = _e1851;
                                    phi_999_ = _e1853;
                                } else {
                                    if (_e1790 == 6u) {
                                        edge_569_576_phi_997_ = _e105;
                                        edge_569_576_phi_998_ = _e103;
                                        edge_569_576_phi_999_ = _e101;
                                        let _e1837 = edge_569_576_phi_997_;
                                        let _e1839 = edge_569_576_phi_998_;
                                        let _e1841 = edge_569_576_phi_999_;
                                        phi_997_ = _e1837;
                                        phi_998_ = _e1839;
                                        phi_999_ = _e1841;
                                    } else {
                                        if (_e1790 == 7u) {
                                            edge_572_576_phi_997_ = _e111;
                                            edge_572_576_phi_998_ = _e109;
                                            edge_572_576_phi_999_ = _e107;
                                            let _e1813 = edge_572_576_phi_997_;
                                            let _e1815 = edge_572_576_phi_998_;
                                            let _e1817 = edge_572_576_phi_999_;
                                            phi_997_ = _e1813;
                                            phi_998_ = _e1815;
                                            phi_999_ = _e1817;
                                        } else {
                                            edge_575_576_phi_997_ = _e117;
                                            edge_575_576_phi_998_ = _e115;
                                            edge_575_576_phi_999_ = _e113;
                                            let _e1825 = edge_575_576_phi_997_;
                                            let _e1827 = edge_575_576_phi_998_;
                                            let _e1829 = edge_575_576_phi_999_;
                                            phi_997_ = _e1825;
                                            phi_998_ = _e1827;
                                            phi_999_ = _e1829;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _e1918 = phi_997_;
            let _e1920 = phi_998_;
            let _e1922 = phi_999_;
            let _e1923 = (_e1922 - _e17);
            let _e1924 = (_e1920 - _e19);
            let _e1925 = (_e1918 - _e21);
            let _e1931 = ((_e182 * _e1925) - (_e209 * _e1923));
            let _e1938 = (((_e265 * _e1924) + (_e238 * _e1931)) + _e13);
            if (0.25f < _e1938) {
                let _e1959 = ((((((((_e182 * _e1923) + (_e209 * _e1925)) * 1.6f) / _e1938) * 0.5f) + 0.5f) * 512f) - _e125);
                let _e1960 = (((0.5f - (((((_e238 * _e1924) - (_e265 * _e1931)) * 1.6f) / _e1938) * 0.5f)) * 512f) - _e134);
                let _e1963 = ((_e1959 * _e1959) + (_e1960 * _e1960));
                if (_e1963 < _e1788) {
                    edge_683_691_phi_1105_ = _e1790;
                    edge_683_691_phi_1106_ = _e1963;
                    let _e1968 = edge_683_691_phi_1105_;
                    let _e1970 = edge_683_691_phi_1106_;
                    phi_1105_ = _e1968;
                    phi_1106_ = _e1970;
                } else {
                    edge_696_691_phi_1105_ = _e1786;
                    edge_696_691_phi_1106_ = _e1788;
                    let _e1976 = edge_696_691_phi_1105_;
                    let _e1978 = edge_696_691_phi_1106_;
                    phi_1105_ = _e1976;
                    phi_1106_ = _e1978;
                }
            } else {
                edge_576_691_phi_1105_ = _e1786;
                edge_576_691_phi_1106_ = _e1788;
                let _e1984 = edge_576_691_phi_1105_;
                let _e1986 = edge_576_691_phi_1106_;
                phi_1105_ = _e1984;
                phi_1106_ = _e1986;
            }
            let _e1990 = phi_1105_;
            let _e1992 = phi_1106_;
            edge_691_548_phi_976_ = _e1990;
            edge_691_548_phi_979_ = _e1992;
            edge_691_548_phi_981_ = (_e1790 + 1u);
            let _e1999 = edge_691_548_phi_976_;
            let _e2001 = edge_691_548_phi_979_;
            let _e2003 = edge_691_548_phi_981_;
            phi_976_ = _e1999;
            phi_979_ = _e2001;
            phi_981_ = _e2003;
            continue;
        } else {
            loop_header_carry_983_ = _e1792;
            break;
        }
    }
    let _e2009 = phi_976_;
    let _e2011 = phi_979_;
    if (_e2011 < 42f) {
        let _e2022 = (f32(bitcast<i32>(_e2009)) / 8f);
        edge_700_727_phi_1125_ = (0.2f + ((0.9f - 0.2f) * _e2022));
        edge_700_727_phi_1126_ = (0.8f + ((0.9f - 0.8f) * _e2022));
        edge_700_727_phi_1127_ = (0.98f + ((0.9f - 0.98f) * _e2022));
        let _e2045 = edge_700_727_phi_1125_;
        let _e2047 = edge_700_727_phi_1126_;
        let _e2049 = edge_700_727_phi_1127_;
        phi_1125_ = _e2045;
        phi_1126_ = _e2047;
        phi_1127_ = _e2049;
    } else {
        edge_682_727_phi_1125_ = -1f;
        edge_682_727_phi_1126_ = -1f;
        edge_682_727_phi_1127_ = -1f;
        let _e2060 = edge_682_727_phi_1125_;
        let _e2062 = edge_682_727_phi_1126_;
        let _e2064 = edge_682_727_phi_1127_;
        phi_1125_ = _e2060;
        phi_1126_ = _e2062;
        phi_1127_ = _e2064;
    }
    let _e2070 = phi_1125_;
    let _e2072 = phi_1126_;
    let _e2074 = phi_1127_;
    if (0f <= _e2074) {
        edge_727_10_phi_316_ = _e2070;
        edge_727_10_phi_315_ = _e2072;
        edge_727_10_phi_314_ = _e2074;
        let _e2081 = edge_727_10_phi_316_;
        let _e2083 = edge_727_10_phi_315_;
        let _e2085 = edge_727_10_phi_314_;
        phi_316_ = _e2081;
        phi_315_ = _e2083;
        phi_314_ = _e2085;
    } else {
        edge_9_10_phi_316_ = _e1764;
        edge_9_10_phi_315_ = _e1766;
        edge_9_10_phi_314_ = _e1768;
        let _e2093 = edge_9_10_phi_316_;
        let _e2095 = edge_9_10_phi_315_;
        let _e2097 = edge_9_10_phi_314_;
        phi_316_ = _e2093;
        phi_315_ = _e2095;
        phi_314_ = _e2097;
    }
    let _e2102 = phi_316_;
    let _e2104 = phi_315_;
    let _e2106 = phi_314_;
    let _e2109 = bitcast<u32>(_e2106);
    let _e2110 = bitcast<u32>(0f);
    let _e2134 = bitcast<u32>(bitcast<f32>(select(select(_e2110, _e2109, ((_e2109 ^ ((0u - (_e2109 >> 31u)) | 2147483648u)) > (_e2110 ^ ((0u - (_e2110 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2109 & 2147483647u) > 2139095040u) || ((_e2110 & 2147483647u) > 2139095040u)))));
    let _e2135 = bitcast<u32>(1f);
    let _e2161 = bitcast<u32>(_e2104);
    let _e2162 = bitcast<u32>(0f);
    let _e2186 = bitcast<u32>(bitcast<f32>(select(select(_e2162, _e2161, ((_e2161 ^ ((0u - (_e2161 >> 31u)) | 2147483648u)) > (_e2162 ^ ((0u - (_e2162 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2161 & 2147483647u) > 2139095040u) || ((_e2162 & 2147483647u) > 2139095040u)))));
    let _e2187 = bitcast<u32>(1f);
    let _e2213 = bitcast<u32>(_e2102);
    let _e2214 = bitcast<u32>(0f);
    let _e2238 = bitcast<u32>(bitcast<f32>(select(select(_e2214, _e2213, ((_e2213 ^ ((0u - (_e2213 >> 31u)) | 2147483648u)) > (_e2214 ^ ((0u - (_e2214 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2213 & 2147483647u) > 2139095040u) || ((_e2214 & 2147483647u) > 2139095040u)))));
    let _e2239 = bitcast<u32>(1f);
    let _e2265 = bitcast<u32>(bitcast<f32>(select(select(_e2135, _e2134, ((_e2134 ^ ((0u - (_e2134 >> 31u)) | 2147483648u)) < (_e2135 ^ ((0u - (_e2135 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2134 & 2147483647u) > 2139095040u) || ((_e2135 & 2147483647u) > 2139095040u)))));
    let _e2266 = bitcast<u32>(0f);
    let _e2290 = bitcast<u32>(bitcast<f32>(select(select(_e2266, _e2265, ((_e2265 ^ ((0u - (_e2265 >> 31u)) | 2147483648u)) > (_e2266 ^ ((0u - (_e2266 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2265 & 2147483647u) > 2139095040u) || ((_e2266 & 2147483647u) > 2139095040u)))));
    let _e2291 = bitcast<u32>(1f);
    let _e2316 = (bitcast<f32>(select(select(_e2291, _e2290, ((_e2290 ^ ((0u - (_e2290 >> 31u)) | 2147483648u)) < (_e2291 ^ ((0u - (_e2291 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2290 & 2147483647u) > 2139095040u) || ((_e2291 & 2147483647u) > 2139095040u)))) * 255f);
    let _e2332 = bitcast<u32>(bitcast<f32>(select(select(_e2187, _e2186, ((_e2186 ^ ((0u - (_e2186 >> 31u)) | 2147483648u)) < (_e2187 ^ ((0u - (_e2187 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2186 & 2147483647u) > 2139095040u) || ((_e2187 & 2147483647u) > 2139095040u)))));
    let _e2333 = bitcast<u32>(0f);
    let _e2357 = bitcast<u32>(bitcast<f32>(select(select(_e2333, _e2332, ((_e2332 ^ ((0u - (_e2332 >> 31u)) | 2147483648u)) > (_e2333 ^ ((0u - (_e2333 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2332 & 2147483647u) > 2139095040u) || ((_e2333 & 2147483647u) > 2139095040u)))));
    let _e2358 = bitcast<u32>(1f);
    let _e2383 = (bitcast<f32>(select(select(_e2358, _e2357, ((_e2357 ^ ((0u - (_e2357 >> 31u)) | 2147483648u)) < (_e2358 ^ ((0u - (_e2358 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2357 & 2147483647u) > 2139095040u) || ((_e2358 & 2147483647u) > 2139095040u)))) * 255f);
    let _e2399 = bitcast<u32>(bitcast<f32>(select(select(_e2239, _e2238, ((_e2238 ^ ((0u - (_e2238 >> 31u)) | 2147483648u)) < (_e2239 ^ ((0u - (_e2239 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2238 & 2147483647u) > 2139095040u) || ((_e2239 & 2147483647u) > 2139095040u)))));
    let _e2400 = bitcast<u32>(0f);
    let _e2424 = bitcast<u32>(bitcast<f32>(select(select(_e2400, _e2399, ((_e2399 ^ ((0u - (_e2399 >> 31u)) | 2147483648u)) > (_e2400 ^ ((0u - (_e2400 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2399 & 2147483647u) > 2139095040u) || ((_e2400 & 2147483647u) > 2139095040u)))));
    let _e2425 = bitcast<u32>(1f);
    let _e2450 = (bitcast<f32>(select(select(_e2425, _e2424, ((_e2424 ^ ((0u - (_e2424 >> 31u)) | 2147483648u)) < (_e2425 ^ ((0u - (_e2425 >> 31u)) | 2147483648u)))), 2143289344u, (((_e2424 & 2147483647u) > 2139095040u) || ((_e2425 & 2147483647u) > 2139095040u)))) * 255f);
    return unpack4x8unorm((((select(0u, select(select(bitcast<u32>(i32(_e2316)), 2147483648u, (_e2316 <= -2147483600f)), 2147483647u, (_e2316 >= 2147483600f)), (_e2316 == _e2316)) + (select(0u, select(select(bitcast<u32>(i32(_e2383)), 2147483648u, (_e2383 <= -2147483600f)), 2147483647u, (_e2383 >= 2147483600f)), (_e2383 == _e2383)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e2450)), 2147483648u, (_e2450 <= -2147483600f)), 2147483647u, (_e2450 >= 2147483600f)), (_e2450 == _e2450)) << 16u)) + 4278190080u));
}
