struct orbit_element {
    re_w0_bits: u32,
    re_w1_bits: u32,
    re_w2_bits: u32,
    re_w3_bits: u32,
    im_w0_bits: u32,
    im_w1_bits: u32,
    im_w2_bits: u32,
    im_w3_bits: u32,
}

struct Input {
    p3_: f32,
    p4_: f32,
    p5_: f32,
    p6_: f32,
    p7_: f32,
    p8_: f32,
    p9_: f32,
    p10_: f32,
    p11_: f32,
    p12_: f32,
}

@group(0) @binding(0)
var<storage> orbit: array<orbit_element>;
@group(0) @binding(1)
var<storage> input: Input;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>((f32(((vi & 1u) << 2u)) - 1f), (f32(((vi & 2u) << 1u)) - 1f), 0f, 1f);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var phi_566_: f32;
    var phi_554_: u32;
    var phi_406_: f32;
    var phi_399_: f32;
    var phi_347_: f32;
    var phi_343_: f32;
    var phi_335_: u32;
    var phi_320_: u32;
    var phi_318_: u32;
    var phi_121_: u32;
    var edge_0_2_phi_566_: f32;
    var edge_0_2_phi_554_: u32;
    var edge_0_2_phi_406_: f32;
    var edge_0_2_phi_399_: f32;
    var edge_0_2_phi_347_: f32;
    var edge_0_2_phi_343_: f32;
    var edge_0_2_phi_335_: u32;
    var edge_0_2_phi_320_: u32;
    var edge_0_2_phi_318_: u32;
    var edge_0_2_phi_121_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_122_: bool;
    var phi_567_: f32;
    var phi_555_: u32;
    var phi_407_: f32;
    var phi_400_: f32;
    var phi_348_: f32;
    var phi_344_: f32;
    var phi_336_: u32;
    var phi_321_: u32;
    var phi_319_: u32;
    var phi_405_: f32;
    var phi_398_: f32;
    var phi_140_: u32;
    var edge_10_11_phi_405_: f32;
    var edge_10_11_phi_398_: f32;
    var edge_10_11_phi_140_: u32;
    var edge_5_11_phi_405_: f32;
    var edge_5_11_phi_398_: f32;
    var edge_5_11_phi_140_: u32;
    var phi_783_: bool;
    var edge_254_253_phi_783_: bool;
    var edge_252_253_phi_783_: bool;
    var edge_11_253_phi_783_: bool;
    var phi_842_: bool;
    var edge_291_290_phi_842_: bool;
    var edge_289_290_phi_842_: bool;
    var edge_287_290_phi_842_: bool;
    var phi_277_: bool;
    var edge_19_18_phi_277_: bool;
    var edge_17_18_phi_277_: bool;
    var edge_17_18_phi_277_1: bool;
    var edge_27_7_phi_567_: f32;
    var edge_27_7_phi_555_: u32;
    var edge_27_7_phi_407_: f32;
    var edge_27_7_phi_400_: f32;
    var edge_27_7_phi_348_: f32;
    var edge_27_7_phi_344_: f32;
    var edge_27_7_phi_336_: u32;
    var edge_27_7_phi_321_: u32;
    var edge_27_7_phi_319_: u32;
    var edge_30_7_phi_567_: f32;
    var edge_30_7_phi_555_: u32;
    var edge_30_7_phi_407_: f32;
    var edge_30_7_phi_400_: f32;
    var edge_30_7_phi_348_: f32;
    var edge_30_7_phi_344_: f32;
    var edge_30_7_phi_336_: u32;
    var edge_30_7_phi_321_: u32;
    var edge_30_7_phi_319_: u32;
    var edge_26_7_phi_567_: f32;
    var edge_26_7_phi_555_: u32;
    var edge_26_7_phi_407_: f32;
    var edge_26_7_phi_400_: f32;
    var edge_26_7_phi_348_: f32;
    var edge_26_7_phi_344_: f32;
    var edge_26_7_phi_336_: u32;
    var edge_26_7_phi_321_: u32;
    var edge_26_7_phi_319_: u32;
    var edge_23_7_phi_567_: f32;
    var edge_23_7_phi_555_: u32;
    var edge_23_7_phi_407_: f32;
    var edge_23_7_phi_400_: f32;
    var edge_23_7_phi_348_: f32;
    var edge_23_7_phi_344_: f32;
    var edge_23_7_phi_336_: u32;
    var edge_23_7_phi_321_: u32;
    var edge_23_7_phi_319_: u32;
    var edge_309_7_phi_567_: f32;
    var edge_309_7_phi_555_: u32;
    var edge_309_7_phi_407_: f32;
    var edge_309_7_phi_400_: f32;
    var edge_309_7_phi_348_: f32;
    var edge_309_7_phi_344_: f32;
    var edge_309_7_phi_336_: u32;
    var edge_309_7_phi_321_: u32;
    var edge_309_7_phi_319_: u32;
    var edge_312_7_phi_567_: f32;
    var edge_312_7_phi_555_: u32;
    var edge_312_7_phi_407_: f32;
    var edge_312_7_phi_400_: f32;
    var edge_312_7_phi_348_: f32;
    var edge_312_7_phi_344_: f32;
    var edge_312_7_phi_336_: u32;
    var edge_312_7_phi_321_: u32;
    var edge_312_7_phi_319_: u32;
    var edge_18_7_phi_567_: f32;
    var edge_18_7_phi_555_: u32;
    var edge_18_7_phi_407_: f32;
    var edge_18_7_phi_400_: f32;
    var edge_18_7_phi_348_: f32;
    var edge_18_7_phi_344_: f32;
    var edge_18_7_phi_336_: u32;
    var edge_18_7_phi_321_: u32;
    var edge_18_7_phi_319_: u32;
    var edge_290_7_phi_567_: f32;
    var edge_290_7_phi_555_: u32;
    var edge_290_7_phi_407_: f32;
    var edge_290_7_phi_400_: f32;
    var edge_290_7_phi_348_: f32;
    var edge_290_7_phi_344_: f32;
    var edge_290_7_phi_336_: u32;
    var edge_290_7_phi_321_: u32;
    var edge_290_7_phi_319_: u32;
    var edge_253_7_phi_567_: f32;
    var edge_253_7_phi_555_: u32;
    var edge_253_7_phi_407_: f32;
    var edge_253_7_phi_400_: f32;
    var edge_253_7_phi_348_: f32;
    var edge_253_7_phi_344_: f32;
    var edge_253_7_phi_336_: u32;
    var edge_253_7_phi_321_: u32;
    var edge_253_7_phi_319_: u32;
    var edge_8_7_phi_567_: f32;
    var edge_8_7_phi_555_: u32;
    var edge_8_7_phi_407_: f32;
    var edge_8_7_phi_400_: f32;
    var edge_8_7_phi_348_: f32;
    var edge_8_7_phi_344_: f32;
    var edge_8_7_phi_336_: u32;
    var edge_8_7_phi_321_: u32;
    var edge_8_7_phi_319_: u32;
    var edge_3_7_phi_567_: f32;
    var edge_3_7_phi_555_: u32;
    var edge_3_7_phi_407_: f32;
    var edge_3_7_phi_400_: f32;
    var edge_3_7_phi_348_: f32;
    var edge_3_7_phi_344_: f32;
    var edge_3_7_phi_336_: u32;
    var edge_3_7_phi_321_: u32;
    var edge_3_7_phi_319_: u32;
    var edge_7_2_phi_566_: f32;
    var edge_7_2_phi_554_: u32;
    var edge_7_2_phi_406_: f32;
    var edge_7_2_phi_399_: f32;
    var edge_7_2_phi_347_: f32;
    var edge_7_2_phi_343_: f32;
    var edge_7_2_phi_335_: u32;
    var edge_7_2_phi_320_: u32;
    var edge_7_2_phi_318_: u32;
    var edge_7_2_phi_121_: u32;
    var structured_result: u32;
    var structured_did_return: bool = false;
    var phi_310_: u32;
    var edge_320_34_phi_310_: u32;
    var edge_33_34_phi_310_: u32;
    var edge_4_34_phi_310_: u32;

    let _e4 = u32(pos.x);
    let _e6 = u32(pos.y);
    let _e8 = input.p3_;
    let _e10 = input.p4_;
    let _e12 = input.p5_;
    let _e14 = input.p6_;
    let _e16 = input.p7_;
    let _e18 = input.p8_;
    let _e20 = input.p9_;
    let _e22 = input.p10_;
    let _e24 = input.p11_;
    let _e26 = input.p12_;
    let _e63 = orbit[i32(2001u)].re_w0_bits;
    let _e65 = orbit[i32(2001u)].re_w1_bits;
    let _e67 = orbit[i32(2001u)].re_w2_bits;
    let _e69 = orbit[i32(2001u)].re_w3_bits;
    let _e71 = orbit[i32(2001u)].im_w0_bits;
    let _e73 = orbit[i32(2001u)].im_w1_bits;
    let _e75 = orbit[i32(2001u)].im_w2_bits;
    let _e77 = orbit[i32(2001u)].im_w3_bits;
    let _e86 = -(bitcast<f32>(_e63));
    let _e87 = -(bitcast<f32>(_e65));
    let _e88 = -(bitcast<f32>(_e67));
    let _e91 = (_e8 + 0f);
    let _e92 = (_e91 - _e8);
    let _e98 = (_e91 + _e86);
    let _e99 = (_e98 - _e91);
    let _e104 = (((_e8 - (_e91 - _e92)) + (0f - _e92)) + ((_e91 - (_e98 - _e99)) + (_e86 - _e99)));
    let _e105 = (_e10 + _e104);
    let _e106 = (_e105 - _e10);
    let _e111 = (_e105 + _e87);
    let _e112 = (_e111 - _e105);
    let _e117 = (((_e10 - (_e105 - _e106)) + (_e104 - _e106)) + ((_e105 - (_e111 - _e112)) + (_e87 - _e112)));
    let _e118 = (_e12 + _e117);
    let _e119 = (_e118 - _e12);
    let _e124 = (_e118 + _e88);
    let _e125 = (_e124 - _e118);
    let _e132 = (((((_e12 - (_e118 - _e119)) + (_e117 - _e119)) + ((_e118 - (_e124 - _e125)) + (_e88 - _e125))) + _e14) + -(bitcast<f32>(_e69)));
    let _e133 = (_e124 + _e132);
    let _e134 = (_e133 - _e124);
    let _e138 = ((_e124 - (_e133 - _e134)) + (_e132 - _e134));
    let _e139 = (_e111 + _e133);
    let _e140 = (_e139 - _e111);
    let _e144 = ((_e111 - (_e139 - _e140)) + (_e133 - _e140));
    let _e145 = (_e98 + _e139);
    let _e146 = (_e145 - _e98);
    let _e150 = ((_e98 - (_e145 - _e146)) + (_e139 - _e146));
    let _e151 = -(bitcast<f32>(_e71));
    let _e152 = -(bitcast<f32>(_e73));
    let _e153 = -(bitcast<f32>(_e75));
    let _e156 = (_e16 + 0f);
    let _e157 = (_e156 - _e16);
    let _e163 = (_e156 + _e151);
    let _e164 = (_e163 - _e156);
    let _e169 = (((_e16 - (_e156 - _e157)) + (0f - _e157)) + ((_e156 - (_e163 - _e164)) + (_e151 - _e164)));
    let _e170 = (_e18 + _e169);
    let _e171 = (_e170 - _e18);
    let _e176 = (_e170 + _e152);
    let _e177 = (_e176 - _e170);
    let _e182 = (((_e18 - (_e170 - _e171)) + (_e169 - _e171)) + ((_e170 - (_e176 - _e177)) + (_e152 - _e177)));
    let _e183 = (_e20 + _e182);
    let _e184 = (_e183 - _e20);
    let _e189 = (_e183 + _e153);
    let _e190 = (_e189 - _e183);
    let _e197 = (((((_e20 - (_e183 - _e184)) + (_e182 - _e184)) + ((_e183 - (_e189 - _e190)) + (_e153 - _e190))) + _e22) + -(bitcast<f32>(_e77)));
    let _e198 = (_e189 + _e197);
    let _e199 = (_e198 - _e189);
    let _e203 = ((_e189 - (_e198 - _e199)) + (_e197 - _e199));
    let _e204 = (_e176 + _e198);
    let _e205 = (_e204 - _e176);
    let _e209 = ((_e176 - (_e204 - _e205)) + (_e198 - _e205));
    let _e210 = (_e163 + _e204);
    let _e211 = (_e210 - _e163);
    let _e215 = ((_e163 - (_e210 - _e211)) + (_e204 - _e211));
    let _e216 = ((((((f32(bitcast<i32>(_e4)) + 0.375f) + (0.25f * f32(bitcast<i32>((_e4 & 1u))))) / _e26) * 2f) - 1f) * _e24);
    let _e217 = (_e145 + _e216);
    let _e218 = (_e217 - _e145);
    let _e223 = (_e217 + _e150);
    let _e224 = (_e223 - _e217);
    let _e229 = (_e223 + _e144);
    let _e230 = (_e229 - _e223);
    let _e235 = (_e229 + _e138);
    let _e236 = (_e235 - _e229);
    let _e245 = ((1f - ((((f32(bitcast<i32>(_e6)) + 0.375f) + (0.25f * f32(bitcast<i32>((_e6 & 1u))))) / _e26) * 2f)) * _e24);
    let _e246 = (_e210 + _e245);
    let _e247 = (_e246 - _e210);
    let _e252 = (_e246 + _e215);
    let _e253 = (_e252 - _e246);
    let _e258 = (_e252 + _e209);
    let _e259 = (_e258 - _e252);
    let _e264 = (_e258 + _e203);
    let _e265 = (_e264 - _e258);
    let _e278 = orbit[i32(2002u)].re_w0_bits;
    let _e280 = orbit[i32(2002u)].re_w1_bits;
    edge_0_2_phi_566_ = 4f;
    edge_0_2_phi_554_ = _e280;
    edge_0_2_phi_406_ = 0f;
    edge_0_2_phi_399_ = 0f;
    edge_0_2_phi_347_ = 0f;
    edge_0_2_phi_343_ = 0f;
    edge_0_2_phi_335_ = 0u;
    edge_0_2_phi_320_ = 0u;
    edge_0_2_phi_318_ = 0u;
    edge_0_2_phi_121_ = 0u;
    let _e301 = edge_0_2_phi_566_;
    let _e303 = edge_0_2_phi_554_;
    let _e305 = edge_0_2_phi_406_;
    let _e307 = edge_0_2_phi_399_;
    let _e309 = edge_0_2_phi_347_;
    let _e311 = edge_0_2_phi_343_;
    let _e313 = edge_0_2_phi_335_;
    let _e315 = edge_0_2_phi_320_;
    let _e317 = edge_0_2_phi_318_;
    let _e319 = edge_0_2_phi_121_;
    phi_566_ = _e301;
    phi_554_ = _e303;
    phi_406_ = _e305;
    phi_399_ = _e307;
    phi_347_ = _e309;
    phi_343_ = _e311;
    phi_335_ = _e313;
    phi_320_ = _e315;
    phi_318_ = _e317;
    phi_121_ = _e319;
    loop {
        let _e332 = phi_566_;
        let _e334 = phi_554_;
        let _e336 = phi_406_;
        let _e338 = phi_399_;
        let _e340 = phi_347_;
        let _e342 = phi_343_;
        let _e344 = phi_335_;
        let _e346 = phi_320_;
        let _e348 = phi_318_;
        let _e350 = phi_121_;
        let _e351 = (_e350 < _e280);
        if _e351 {
            if (_e348 == 0u) {
                if (_e346 == 0u) {
                    if ((_e344 + 1u) < _e278) {
                        edge_10_11_phi_405_ = _e336;
                        edge_10_11_phi_398_ = _e338;
                        edge_10_11_phi_140_ = _e344;
                        let _e363 = edge_10_11_phi_405_;
                        let _e365 = edge_10_11_phi_398_;
                        let _e367 = edge_10_11_phi_140_;
                        phi_405_ = _e363;
                        phi_398_ = _e365;
                        phi_140_ = _e367;
                    } else {
                        edge_5_11_phi_405_ = _e340;
                        edge_5_11_phi_398_ = _e342;
                        edge_5_11_phi_140_ = 0u;
                        let _e376 = edge_5_11_phi_405_;
                        let _e378 = edge_5_11_phi_398_;
                        let _e380 = edge_5_11_phi_140_;
                        phi_405_ = _e376;
                        phi_398_ = _e378;
                        phi_140_ = _e380;
                    }
                    let _e385 = phi_405_;
                    let _e387 = phi_398_;
                    let _e389 = phi_140_;
                    let _e393 = orbit[i32(_e389)].re_w0_bits;
                    let _e395 = orbit[i32(_e389)].re_w1_bits;
                    let _e397 = orbit[i32(_e389)].re_w2_bits;
                    let _e399 = orbit[i32(_e389)].re_w3_bits;
                    let _e401 = orbit[i32(_e389)].im_w0_bits;
                    let _e403 = orbit[i32(_e389)].im_w1_bits;
                    let _e405 = orbit[i32(_e389)].im_w2_bits;
                    let _e407 = orbit[i32(_e389)].im_w3_bits;
                    let _e409 = (_e389 + 1u);
                    let _e413 = orbit[i32(_e409)].re_w0_bits;
                    let _e415 = orbit[i32(_e409)].re_w1_bits;
                    let _e417 = orbit[i32(_e409)].re_w2_bits;
                    let _e419 = orbit[i32(_e409)].re_w3_bits;
                    let _e421 = orbit[i32(_e409)].im_w0_bits;
                    let _e423 = orbit[i32(_e409)].im_w1_bits;
                    let _e425 = orbit[i32(_e409)].im_w2_bits;
                    let _e427 = orbit[i32(_e409)].im_w3_bits;
                    if (_e393 == 2143289344u) {
                        edge_11_253_phi_783_ = true;
                        let _e445 = edge_11_253_phi_783_;
                        phi_783_ = _e445;
                    } else {
                        if (_e401 == 2143289344u) {
                            edge_254_253_phi_783_ = true;
                            let _e435 = edge_254_253_phi_783_;
                            phi_783_ = _e435;
                        } else {
                            edge_252_253_phi_783_ = false;
                            let _e440 = edge_252_253_phi_783_;
                            phi_783_ = _e440;
                        }
                    }
                    let _e448 = phi_783_;
                    if _e448 {
                        edge_253_7_phi_567_ = _e332;
                        edge_253_7_phi_555_ = _e334;
                        edge_253_7_phi_407_ = _e385;
                        edge_253_7_phi_400_ = _e387;
                        edge_253_7_phi_348_ = _e340;
                        edge_253_7_phi_344_ = _e342;
                        edge_253_7_phi_336_ = _e389;
                        edge_253_7_phi_321_ = 1u;
                        edge_253_7_phi_319_ = _e348;
                        let _e968 = edge_253_7_phi_567_;
                        let _e970 = edge_253_7_phi_555_;
                        let _e972 = edge_253_7_phi_407_;
                        let _e974 = edge_253_7_phi_400_;
                        let _e976 = edge_253_7_phi_348_;
                        let _e978 = edge_253_7_phi_344_;
                        let _e980 = edge_253_7_phi_336_;
                        let _e982 = edge_253_7_phi_321_;
                        let _e984 = edge_253_7_phi_319_;
                        phi_567_ = _e968;
                        phi_555_ = _e970;
                        phi_407_ = _e972;
                        phi_400_ = _e974;
                        phi_348_ = _e976;
                        phi_344_ = _e978;
                        phi_336_ = _e980;
                        phi_321_ = _e982;
                        phi_319_ = _e984;
                    } else {
                        if (_e413 == 2143289344u) {
                            edge_287_290_phi_842_ = true;
                            let _e466 = edge_287_290_phi_842_;
                            phi_842_ = _e466;
                        } else {
                            if (_e421 == 2143289344u) {
                                edge_291_290_phi_842_ = true;
                                let _e456 = edge_291_290_phi_842_;
                                phi_842_ = _e456;
                            } else {
                                edge_289_290_phi_842_ = false;
                                let _e461 = edge_289_290_phi_842_;
                                phi_842_ = _e461;
                            }
                        }
                        let _e469 = phi_842_;
                        if _e469 {
                            edge_290_7_phi_567_ = _e332;
                            edge_290_7_phi_555_ = _e334;
                            edge_290_7_phi_407_ = _e385;
                            edge_290_7_phi_400_ = _e387;
                            edge_290_7_phi_348_ = _e340;
                            edge_290_7_phi_344_ = _e342;
                            edge_290_7_phi_336_ = _e389;
                            edge_290_7_phi_321_ = 1u;
                            edge_290_7_phi_319_ = _e348;
                            let _e931 = edge_290_7_phi_567_;
                            let _e933 = edge_290_7_phi_555_;
                            let _e935 = edge_290_7_phi_407_;
                            let _e937 = edge_290_7_phi_400_;
                            let _e939 = edge_290_7_phi_348_;
                            let _e941 = edge_290_7_phi_344_;
                            let _e943 = edge_290_7_phi_336_;
                            let _e945 = edge_290_7_phi_321_;
                            let _e947 = edge_290_7_phi_319_;
                            phi_567_ = _e931;
                            phi_555_ = _e933;
                            phi_407_ = _e935;
                            phi_400_ = _e937;
                            phi_348_ = _e939;
                            phi_344_ = _e941;
                            phi_336_ = _e943;
                            phi_321_ = _e945;
                            phi_319_ = _e947;
                        } else {
                            let _e470 = bitcast<f32>(_e393);
                            let _e471 = bitcast<f32>(_e395);
                            let _e472 = bitcast<f32>(_e397);
                            let _e473 = bitcast<f32>(_e399);
                            let _e474 = bitcast<f32>(_e401);
                            let _e475 = bitcast<f32>(_e403);
                            let _e476 = bitcast<f32>(_e405);
                            let _e477 = bitcast<f32>(_e407);
                            let _e478 = bitcast<f32>(_e413);
                            let _e479 = bitcast<f32>(_e415);
                            let _e480 = bitcast<f32>(_e417);
                            let _e481 = bitcast<f32>(_e419);
                            let _e482 = bitcast<f32>(_e421);
                            let _e483 = bitcast<f32>(_e423);
                            let _e484 = bitcast<f32>(_e425);
                            let _e485 = bitcast<f32>(_e427);
                            let _e523 = ((((2f * ((_e470 * _e387) - (_e474 * _e385))) + (2f * ((((((_e471 * _e387) - (_e475 * _e385)) + (_e472 * _e387)) - (_e476 * _e385)) + (_e473 * _e387)) - (_e477 * _e385)))) + ((_e387 * _e387) - (_e385 * _e385))) + (_e235 + (((((_e229 - (_e235 - _e236)) + (_e138 - _e236)) + ((_e223 - (_e229 - _e230)) + (_e144 - _e230))) + ((_e217 - (_e223 - _e224)) + (_e150 - _e224))) + ((_e145 - (_e217 - _e218)) + (_e216 - _e218)))));
                            let _e533 = ((((2f * ((_e470 * _e385) + (_e474 * _e387))) + (2f * ((((((_e471 * _e385) + (_e475 * _e387)) + (_e472 * _e385)) + (_e476 * _e387)) + (_e473 * _e385)) + (_e477 * _e387)))) + ((2f * _e387) * _e385)) + (_e264 + (((((_e258 - (_e264 - _e265)) + (_e203 - _e265)) + ((_e252 - (_e258 - _e259)) + (_e209 - _e259))) + ((_e246 - (_e252 - _e253)) + (_e215 - _e253))) + ((_e210 - (_e246 - _e247)) + (_e245 - _e247)))));
                            let _e534 = (_e478 + _e523);
                            let _e535 = (_e534 - _e478);
                            let _e540 = (_e534 + _e479);
                            let _e541 = (_e540 - _e534);
                            let _e546 = (_e540 + _e480);
                            let _e547 = (_e546 - _e540);
                            let _e552 = (_e546 + _e481);
                            let _e553 = (_e552 - _e546);
                            let _e561 = (_e552 + (((((_e546 - (_e552 - _e553)) + (_e481 - _e553)) + ((_e540 - (_e546 - _e547)) + (_e480 - _e547))) + ((_e534 - (_e540 - _e541)) + (_e479 - _e541))) + ((_e478 - (_e534 - _e535)) + (_e523 - _e535))));
                            let _e562 = (_e482 + _e533);
                            let _e563 = (_e562 - _e482);
                            let _e568 = (_e562 + _e483);
                            let _e569 = (_e568 - _e562);
                            let _e574 = (_e568 + _e484);
                            let _e575 = (_e574 - _e568);
                            let _e580 = (_e574 + _e485);
                            let _e581 = (_e580 - _e574);
                            let _e589 = (_e580 + (((((_e574 - (_e580 - _e581)) + (_e485 - _e581)) + ((_e568 - (_e574 - _e575)) + (_e484 - _e575))) + ((_e562 - (_e568 - _e569)) + (_e483 - _e569))) + ((_e482 - (_e562 - _e563)) + (_e533 - _e563))));
                            let _e592 = ((_e561 * _e561) + (_e589 * _e589));
                            let _e598 = (4f < _e592);
                            if _e598 {
                                let _e606 = bitcast<u32>(1f);
                                let _e607 = bitcast<u32>(_e592);
                                if (bitcast<f32>((bitcast<u32>((_e592 - 4f)) & 2147483647u)) <= (0.000004f * bitcast<f32>(select(select(_e607, _e606, ((_e606 ^ ((0u - (_e606 >> 31u)) | 2147483648u)) > (_e607 ^ ((0u - (_e607 >> 31u)) | 2147483648u)))), 2143289344u, (((_e606 & 2147483647u) > 2139095040u) || ((_e607 & 2147483647u) > 2139095040u)))))) {
                                    edge_19_18_phi_277_ = true;
                                    let _e637 = edge_19_18_phi_277_;
                                    phi_277_ = _e637;
                                } else {
                                    edge_17_18_phi_277_ = false;
                                    let _e642 = edge_17_18_phi_277_;
                                    phi_277_ = _e642;
                                }
                            } else {
                                edge_17_18_phi_277_1 = false;
                                let _e647 = edge_17_18_phi_277_1;
                                phi_277_ = _e647;
                            }
                            let _e650 = phi_277_;
                            if ((_e523 - _e523) == 0f) {
                                if ((_e533 - _e533) == 0f) {
                                    if ((_e592 - _e592) == 0f) {
                                        if _e650 {
                                            edge_23_7_phi_567_ = _e332;
                                            edge_23_7_phi_555_ = _e334;
                                            edge_23_7_phi_407_ = _e385;
                                            edge_23_7_phi_400_ = _e387;
                                            edge_23_7_phi_348_ = _e340;
                                            edge_23_7_phi_344_ = _e342;
                                            edge_23_7_phi_336_ = _e389;
                                            edge_23_7_phi_321_ = 1u;
                                            edge_23_7_phi_319_ = _e348;
                                            let _e783 = edge_23_7_phi_567_;
                                            let _e785 = edge_23_7_phi_555_;
                                            let _e787 = edge_23_7_phi_407_;
                                            let _e789 = edge_23_7_phi_400_;
                                            let _e791 = edge_23_7_phi_348_;
                                            let _e793 = edge_23_7_phi_344_;
                                            let _e795 = edge_23_7_phi_336_;
                                            let _e797 = edge_23_7_phi_321_;
                                            let _e799 = edge_23_7_phi_319_;
                                            phi_567_ = _e783;
                                            phi_555_ = _e785;
                                            phi_407_ = _e787;
                                            phi_400_ = _e789;
                                            phi_348_ = _e791;
                                            phi_344_ = _e793;
                                            phi_336_ = _e795;
                                            phi_321_ = _e797;
                                            phi_319_ = _e799;
                                        } else {
                                            if _e598 {
                                                edge_26_7_phi_567_ = _e592;
                                                edge_26_7_phi_555_ = (_e350 + 1u);
                                                edge_26_7_phi_407_ = _e533;
                                                edge_26_7_phi_400_ = _e523;
                                                edge_26_7_phi_348_ = _e589;
                                                edge_26_7_phi_344_ = _e561;
                                                edge_26_7_phi_336_ = _e389;
                                                edge_26_7_phi_321_ = _e346;
                                                edge_26_7_phi_319_ = 1u;
                                                let _e746 = edge_26_7_phi_567_;
                                                let _e748 = edge_26_7_phi_555_;
                                                let _e750 = edge_26_7_phi_407_;
                                                let _e752 = edge_26_7_phi_400_;
                                                let _e754 = edge_26_7_phi_348_;
                                                let _e756 = edge_26_7_phi_344_;
                                                let _e758 = edge_26_7_phi_336_;
                                                let _e760 = edge_26_7_phi_321_;
                                                let _e762 = edge_26_7_phi_319_;
                                                phi_567_ = _e746;
                                                phi_555_ = _e748;
                                                phi_407_ = _e750;
                                                phi_400_ = _e752;
                                                phi_348_ = _e754;
                                                phi_344_ = _e756;
                                                phi_336_ = _e758;
                                                phi_321_ = _e760;
                                                phi_319_ = _e762;
                                            } else {
                                                if (_e592 < ((_e523 * _e523) + (_e533 * _e533))) {
                                                    edge_27_7_phi_567_ = _e332;
                                                    edge_27_7_phi_555_ = _e334;
                                                    edge_27_7_phi_407_ = _e589;
                                                    edge_27_7_phi_400_ = _e561;
                                                    edge_27_7_phi_348_ = _e589;
                                                    edge_27_7_phi_344_ = _e561;
                                                    edge_27_7_phi_336_ = 0u;
                                                    edge_27_7_phi_321_ = _e346;
                                                    edge_27_7_phi_319_ = _e348;
                                                    let _e673 = edge_27_7_phi_567_;
                                                    let _e675 = edge_27_7_phi_555_;
                                                    let _e677 = edge_27_7_phi_407_;
                                                    let _e679 = edge_27_7_phi_400_;
                                                    let _e681 = edge_27_7_phi_348_;
                                                    let _e683 = edge_27_7_phi_344_;
                                                    let _e685 = edge_27_7_phi_336_;
                                                    let _e687 = edge_27_7_phi_321_;
                                                    let _e689 = edge_27_7_phi_319_;
                                                    phi_567_ = _e673;
                                                    phi_555_ = _e675;
                                                    phi_407_ = _e677;
                                                    phi_400_ = _e679;
                                                    phi_348_ = _e681;
                                                    phi_344_ = _e683;
                                                    phi_336_ = _e685;
                                                    phi_321_ = _e687;
                                                    phi_319_ = _e689;
                                                } else {
                                                    edge_30_7_phi_567_ = _e332;
                                                    edge_30_7_phi_555_ = _e334;
                                                    edge_30_7_phi_407_ = _e533;
                                                    edge_30_7_phi_400_ = _e523;
                                                    edge_30_7_phi_348_ = _e589;
                                                    edge_30_7_phi_344_ = _e561;
                                                    edge_30_7_phi_336_ = _e409;
                                                    edge_30_7_phi_321_ = _e346;
                                                    edge_30_7_phi_319_ = _e348;
                                                    let _e709 = edge_30_7_phi_567_;
                                                    let _e711 = edge_30_7_phi_555_;
                                                    let _e713 = edge_30_7_phi_407_;
                                                    let _e715 = edge_30_7_phi_400_;
                                                    let _e717 = edge_30_7_phi_348_;
                                                    let _e719 = edge_30_7_phi_344_;
                                                    let _e721 = edge_30_7_phi_336_;
                                                    let _e723 = edge_30_7_phi_321_;
                                                    let _e725 = edge_30_7_phi_319_;
                                                    phi_567_ = _e709;
                                                    phi_555_ = _e711;
                                                    phi_407_ = _e713;
                                                    phi_400_ = _e715;
                                                    phi_348_ = _e717;
                                                    phi_344_ = _e719;
                                                    phi_336_ = _e721;
                                                    phi_321_ = _e723;
                                                    phi_319_ = _e725;
                                                }
                                            }
                                        }
                                    } else {
                                        edge_309_7_phi_567_ = _e332;
                                        edge_309_7_phi_555_ = _e334;
                                        edge_309_7_phi_407_ = _e385;
                                        edge_309_7_phi_400_ = _e387;
                                        edge_309_7_phi_348_ = _e340;
                                        edge_309_7_phi_344_ = _e342;
                                        edge_309_7_phi_336_ = _e389;
                                        edge_309_7_phi_321_ = 1u;
                                        edge_309_7_phi_319_ = _e348;
                                        let _e820 = edge_309_7_phi_567_;
                                        let _e822 = edge_309_7_phi_555_;
                                        let _e824 = edge_309_7_phi_407_;
                                        let _e826 = edge_309_7_phi_400_;
                                        let _e828 = edge_309_7_phi_348_;
                                        let _e830 = edge_309_7_phi_344_;
                                        let _e832 = edge_309_7_phi_336_;
                                        let _e834 = edge_309_7_phi_321_;
                                        let _e836 = edge_309_7_phi_319_;
                                        phi_567_ = _e820;
                                        phi_555_ = _e822;
                                        phi_407_ = _e824;
                                        phi_400_ = _e826;
                                        phi_348_ = _e828;
                                        phi_344_ = _e830;
                                        phi_336_ = _e832;
                                        phi_321_ = _e834;
                                        phi_319_ = _e836;
                                    }
                                } else {
                                    edge_312_7_phi_567_ = _e332;
                                    edge_312_7_phi_555_ = _e334;
                                    edge_312_7_phi_407_ = _e385;
                                    edge_312_7_phi_400_ = _e387;
                                    edge_312_7_phi_348_ = _e340;
                                    edge_312_7_phi_344_ = _e342;
                                    edge_312_7_phi_336_ = _e389;
                                    edge_312_7_phi_321_ = 1u;
                                    edge_312_7_phi_319_ = _e348;
                                    let _e857 = edge_312_7_phi_567_;
                                    let _e859 = edge_312_7_phi_555_;
                                    let _e861 = edge_312_7_phi_407_;
                                    let _e863 = edge_312_7_phi_400_;
                                    let _e865 = edge_312_7_phi_348_;
                                    let _e867 = edge_312_7_phi_344_;
                                    let _e869 = edge_312_7_phi_336_;
                                    let _e871 = edge_312_7_phi_321_;
                                    let _e873 = edge_312_7_phi_319_;
                                    phi_567_ = _e857;
                                    phi_555_ = _e859;
                                    phi_407_ = _e861;
                                    phi_400_ = _e863;
                                    phi_348_ = _e865;
                                    phi_344_ = _e867;
                                    phi_336_ = _e869;
                                    phi_321_ = _e871;
                                    phi_319_ = _e873;
                                }
                            } else {
                                edge_18_7_phi_567_ = _e332;
                                edge_18_7_phi_555_ = _e334;
                                edge_18_7_phi_407_ = _e385;
                                edge_18_7_phi_400_ = _e387;
                                edge_18_7_phi_348_ = _e340;
                                edge_18_7_phi_344_ = _e342;
                                edge_18_7_phi_336_ = _e389;
                                edge_18_7_phi_321_ = 1u;
                                edge_18_7_phi_319_ = _e348;
                                let _e894 = edge_18_7_phi_567_;
                                let _e896 = edge_18_7_phi_555_;
                                let _e898 = edge_18_7_phi_407_;
                                let _e900 = edge_18_7_phi_400_;
                                let _e902 = edge_18_7_phi_348_;
                                let _e904 = edge_18_7_phi_344_;
                                let _e906 = edge_18_7_phi_336_;
                                let _e908 = edge_18_7_phi_321_;
                                let _e910 = edge_18_7_phi_319_;
                                phi_567_ = _e894;
                                phi_555_ = _e896;
                                phi_407_ = _e898;
                                phi_400_ = _e900;
                                phi_348_ = _e902;
                                phi_344_ = _e904;
                                phi_336_ = _e906;
                                phi_321_ = _e908;
                                phi_319_ = _e910;
                            }
                        }
                    }
                } else {
                    edge_8_7_phi_567_ = _e332;
                    edge_8_7_phi_555_ = _e334;
                    edge_8_7_phi_407_ = _e336;
                    edge_8_7_phi_400_ = _e338;
                    edge_8_7_phi_348_ = _e340;
                    edge_8_7_phi_344_ = _e342;
                    edge_8_7_phi_336_ = _e344;
                    edge_8_7_phi_321_ = _e346;
                    edge_8_7_phi_319_ = _e348;
                    let _e1004 = edge_8_7_phi_567_;
                    let _e1006 = edge_8_7_phi_555_;
                    let _e1008 = edge_8_7_phi_407_;
                    let _e1010 = edge_8_7_phi_400_;
                    let _e1012 = edge_8_7_phi_348_;
                    let _e1014 = edge_8_7_phi_344_;
                    let _e1016 = edge_8_7_phi_336_;
                    let _e1018 = edge_8_7_phi_321_;
                    let _e1020 = edge_8_7_phi_319_;
                    phi_567_ = _e1004;
                    phi_555_ = _e1006;
                    phi_407_ = _e1008;
                    phi_400_ = _e1010;
                    phi_348_ = _e1012;
                    phi_344_ = _e1014;
                    phi_336_ = _e1016;
                    phi_321_ = _e1018;
                    phi_319_ = _e1020;
                }
            } else {
                edge_3_7_phi_567_ = _e332;
                edge_3_7_phi_555_ = _e334;
                edge_3_7_phi_407_ = _e336;
                edge_3_7_phi_400_ = _e338;
                edge_3_7_phi_348_ = _e340;
                edge_3_7_phi_344_ = _e342;
                edge_3_7_phi_336_ = _e344;
                edge_3_7_phi_321_ = _e346;
                edge_3_7_phi_319_ = _e348;
                let _e1040 = edge_3_7_phi_567_;
                let _e1042 = edge_3_7_phi_555_;
                let _e1044 = edge_3_7_phi_407_;
                let _e1046 = edge_3_7_phi_400_;
                let _e1048 = edge_3_7_phi_348_;
                let _e1050 = edge_3_7_phi_344_;
                let _e1052 = edge_3_7_phi_336_;
                let _e1054 = edge_3_7_phi_321_;
                let _e1056 = edge_3_7_phi_319_;
                phi_567_ = _e1040;
                phi_555_ = _e1042;
                phi_407_ = _e1044;
                phi_400_ = _e1046;
                phi_348_ = _e1048;
                phi_344_ = _e1050;
                phi_336_ = _e1052;
                phi_321_ = _e1054;
                phi_319_ = _e1056;
            }
            let _e1067 = phi_567_;
            let _e1069 = phi_555_;
            let _e1071 = phi_407_;
            let _e1073 = phi_400_;
            let _e1075 = phi_348_;
            let _e1077 = phi_344_;
            let _e1079 = phi_336_;
            let _e1081 = phi_321_;
            let _e1083 = phi_319_;
            edge_7_2_phi_566_ = _e1067;
            edge_7_2_phi_554_ = _e1069;
            edge_7_2_phi_406_ = _e1071;
            edge_7_2_phi_399_ = _e1073;
            edge_7_2_phi_347_ = _e1075;
            edge_7_2_phi_343_ = _e1077;
            edge_7_2_phi_335_ = _e1079;
            edge_7_2_phi_320_ = _e1081;
            edge_7_2_phi_318_ = _e1083;
            edge_7_2_phi_121_ = (_e350 + 1u);
            let _e1097 = edge_7_2_phi_566_;
            let _e1099 = edge_7_2_phi_554_;
            let _e1101 = edge_7_2_phi_406_;
            let _e1103 = edge_7_2_phi_399_;
            let _e1105 = edge_7_2_phi_347_;
            let _e1107 = edge_7_2_phi_343_;
            let _e1109 = edge_7_2_phi_335_;
            let _e1111 = edge_7_2_phi_320_;
            let _e1113 = edge_7_2_phi_318_;
            let _e1115 = edge_7_2_phi_121_;
            phi_566_ = _e1097;
            phi_554_ = _e1099;
            phi_406_ = _e1101;
            phi_399_ = _e1103;
            phi_347_ = _e1105;
            phi_343_ = _e1107;
            phi_335_ = _e1109;
            phi_320_ = _e1111;
            phi_318_ = _e1113;
            phi_121_ = _e1115;
            continue;
        } else {
            loop_header_carry_122_ = _e351;
            break;
        }
    }
    let _e1128 = phi_566_;
    let _e1130 = phi_554_;
    let _e1142 = phi_320_;
    let _e1144 = phi_318_;
    if (_e1142 == 1u) {
        edge_4_34_phi_310_ = 4294902015u;
        let _e1493 = edge_4_34_phi_310_;
        phi_310_ = _e1493;
    } else {
        if (_e1144 == 1u) {
            let _e1160 = ((f32(bitcast<i32>(_e1130)) + (4f / _e1128)) / 256f);
            let _e1166 = ((6.2831855f * (_e1160 + 0f)) + 1.5707964f);
            let _e1174 = (_e1166 - (6.2831855f * floor(((_e1166 / 6.2831855f) + 0.5f))));
            let _e1184 = ((1.2732395f * _e1174) - ((0.40528473f * _e1174) * bitcast<f32>((bitcast<u32>(_e1174) & 2147483647u))));
            let _e1203 = ((6.2831855f * (_e1160 + 0.33f)) + 1.5707964f);
            let _e1211 = (_e1203 - (6.2831855f * floor(((_e1203 / 6.2831855f) + 0.5f))));
            let _e1221 = ((1.2732395f * _e1211) - ((0.40528473f * _e1211) * bitcast<f32>((bitcast<u32>(_e1211) & 2147483647u))));
            let _e1240 = ((6.2831855f * (_e1160 + 0.67f)) + 1.5707964f);
            let _e1248 = (_e1240 - (6.2831855f * floor(((_e1240 / 6.2831855f) + 0.5f))));
            let _e1258 = ((1.2732395f * _e1248) - ((0.40528473f * _e1248) * bitcast<f32>((bitcast<u32>(_e1248) & 2147483647u))));
            let _e1274 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e1184 * bitcast<f32>((bitcast<u32>(_e1184) & 2147483647u))) - _e1184)) + _e1184))));
            let _e1275 = bitcast<u32>(0f);
            let _e1299 = bitcast<u32>(bitcast<f32>(select(select(_e1275, _e1274, ((_e1274 ^ ((0u - (_e1274 >> 31u)) | 2147483648u)) > (_e1275 ^ ((0u - (_e1275 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1274 & 2147483647u) > 2139095040u) || ((_e1275 & 2147483647u) > 2139095040u)))));
            let _e1300 = bitcast<u32>(1f);
            let _e1325 = (bitcast<f32>(select(select(_e1300, _e1299, ((_e1299 ^ ((0u - (_e1299 >> 31u)) | 2147483648u)) < (_e1300 ^ ((0u - (_e1300 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1299 & 2147483647u) > 2139095040u) || ((_e1300 & 2147483647u) > 2139095040u)))) * 255f);
            let _e1341 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e1221 * bitcast<f32>((bitcast<u32>(_e1221) & 2147483647u))) - _e1221)) + _e1221))));
            let _e1342 = bitcast<u32>(0f);
            let _e1366 = bitcast<u32>(bitcast<f32>(select(select(_e1342, _e1341, ((_e1341 ^ ((0u - (_e1341 >> 31u)) | 2147483648u)) > (_e1342 ^ ((0u - (_e1342 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1341 & 2147483647u) > 2139095040u) || ((_e1342 & 2147483647u) > 2139095040u)))));
            let _e1367 = bitcast<u32>(1f);
            let _e1392 = (bitcast<f32>(select(select(_e1367, _e1366, ((_e1366 ^ ((0u - (_e1366 >> 31u)) | 2147483648u)) < (_e1367 ^ ((0u - (_e1367 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1366 & 2147483647u) > 2139095040u) || ((_e1367 & 2147483647u) > 2139095040u)))) * 255f);
            let _e1408 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e1258 * bitcast<f32>((bitcast<u32>(_e1258) & 2147483647u))) - _e1258)) + _e1258))));
            let _e1409 = bitcast<u32>(0f);
            let _e1433 = bitcast<u32>(bitcast<f32>(select(select(_e1409, _e1408, ((_e1408 ^ ((0u - (_e1408 >> 31u)) | 2147483648u)) > (_e1409 ^ ((0u - (_e1409 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1408 & 2147483647u) > 2139095040u) || ((_e1409 & 2147483647u) > 2139095040u)))));
            let _e1434 = bitcast<u32>(1f);
            let _e1459 = (bitcast<f32>(select(select(_e1434, _e1433, ((_e1433 ^ ((0u - (_e1433 >> 31u)) | 2147483648u)) < (_e1434 ^ ((0u - (_e1434 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1433 & 2147483647u) > 2139095040u) || ((_e1434 & 2147483647u) > 2139095040u)))) * 255f);
            edge_320_34_phi_310_ = (((select(0u, select(select(bitcast<u32>(i32(_e1325)), 2147483648u, (_e1325 <= -2147483600f)), 2147483647u, (_e1325 >= 2147483600f)), (_e1325 == _e1325)) + (select(0u, select(select(bitcast<u32>(i32(_e1392)), 2147483648u, (_e1392 <= -2147483600f)), 2147483647u, (_e1392 >= 2147483600f)), (_e1392 == _e1392)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e1459)), 2147483648u, (_e1459 <= -2147483600f)), 2147483647u, (_e1459 >= 2147483600f)), (_e1459 == _e1459)) << 16u)) + 4278190080u);
            let _e1483 = edge_320_34_phi_310_;
            phi_310_ = _e1483;
        } else {
            edge_33_34_phi_310_ = 4278190080u;
            let _e1488 = edge_33_34_phi_310_;
            phi_310_ = _e1488;
        }
    }
    let _e1496 = phi_310_;
    return unpack4x8unorm(_e1496);
}
