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
    var phi_580_: f32;
    var phi_568_: u32;
    var phi_420_: f32;
    var phi_413_: f32;
    var phi_361_: f32;
    var phi_357_: f32;
    var phi_349_: u32;
    var phi_334_: u32;
    var phi_332_: u32;
    var phi_130_: u32;
    var edge_0_2_phi_580_: f32;
    var edge_0_2_phi_568_: u32;
    var edge_0_2_phi_420_: f32;
    var edge_0_2_phi_413_: f32;
    var edge_0_2_phi_361_: f32;
    var edge_0_2_phi_357_: f32;
    var edge_0_2_phi_349_: u32;
    var edge_0_2_phi_334_: u32;
    var edge_0_2_phi_332_: u32;
    var edge_0_2_phi_130_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_131_: bool;
    var phi_581_: f32;
    var phi_569_: u32;
    var phi_421_: f32;
    var phi_414_: f32;
    var phi_362_: f32;
    var phi_358_: f32;
    var phi_350_: u32;
    var phi_335_: u32;
    var phi_333_: u32;
    var phi_419_: f32;
    var phi_412_: f32;
    var phi_149_: u32;
    var edge_10_11_phi_419_: f32;
    var edge_10_11_phi_412_: f32;
    var edge_10_11_phi_149_: u32;
    var edge_5_11_phi_419_: f32;
    var edge_5_11_phi_412_: f32;
    var edge_5_11_phi_149_: u32;
    var phi_779_: bool;
    var edge_242_241_phi_779_: bool;
    var edge_240_241_phi_779_: bool;
    var edge_11_241_phi_779_: bool;
    var phi_838_: bool;
    var edge_279_278_phi_838_: bool;
    var edge_277_278_phi_838_: bool;
    var edge_275_278_phi_838_: bool;
    var phi_285_: bool;
    var edge_19_18_phi_285_: bool;
    var edge_17_18_phi_285_: bool;
    var edge_17_18_phi_285_1: bool;
    var edge_27_7_phi_581_: f32;
    var edge_27_7_phi_569_: u32;
    var edge_27_7_phi_421_: f32;
    var edge_27_7_phi_414_: f32;
    var edge_27_7_phi_362_: f32;
    var edge_27_7_phi_358_: f32;
    var edge_27_7_phi_350_: u32;
    var edge_27_7_phi_335_: u32;
    var edge_27_7_phi_333_: u32;
    var edge_30_7_phi_581_: f32;
    var edge_30_7_phi_569_: u32;
    var edge_30_7_phi_421_: f32;
    var edge_30_7_phi_414_: f32;
    var edge_30_7_phi_362_: f32;
    var edge_30_7_phi_358_: f32;
    var edge_30_7_phi_350_: u32;
    var edge_30_7_phi_335_: u32;
    var edge_30_7_phi_333_: u32;
    var edge_26_7_phi_581_: f32;
    var edge_26_7_phi_569_: u32;
    var edge_26_7_phi_421_: f32;
    var edge_26_7_phi_414_: f32;
    var edge_26_7_phi_362_: f32;
    var edge_26_7_phi_358_: f32;
    var edge_26_7_phi_350_: u32;
    var edge_26_7_phi_335_: u32;
    var edge_26_7_phi_333_: u32;
    var edge_23_7_phi_581_: f32;
    var edge_23_7_phi_569_: u32;
    var edge_23_7_phi_421_: f32;
    var edge_23_7_phi_414_: f32;
    var edge_23_7_phi_362_: f32;
    var edge_23_7_phi_358_: f32;
    var edge_23_7_phi_350_: u32;
    var edge_23_7_phi_335_: u32;
    var edge_23_7_phi_333_: u32;
    var edge_297_7_phi_581_: f32;
    var edge_297_7_phi_569_: u32;
    var edge_297_7_phi_421_: f32;
    var edge_297_7_phi_414_: f32;
    var edge_297_7_phi_362_: f32;
    var edge_297_7_phi_358_: f32;
    var edge_297_7_phi_350_: u32;
    var edge_297_7_phi_335_: u32;
    var edge_297_7_phi_333_: u32;
    var edge_300_7_phi_581_: f32;
    var edge_300_7_phi_569_: u32;
    var edge_300_7_phi_421_: f32;
    var edge_300_7_phi_414_: f32;
    var edge_300_7_phi_362_: f32;
    var edge_300_7_phi_358_: f32;
    var edge_300_7_phi_350_: u32;
    var edge_300_7_phi_335_: u32;
    var edge_300_7_phi_333_: u32;
    var edge_18_7_phi_581_: f32;
    var edge_18_7_phi_569_: u32;
    var edge_18_7_phi_421_: f32;
    var edge_18_7_phi_414_: f32;
    var edge_18_7_phi_362_: f32;
    var edge_18_7_phi_358_: f32;
    var edge_18_7_phi_350_: u32;
    var edge_18_7_phi_335_: u32;
    var edge_18_7_phi_333_: u32;
    var edge_278_7_phi_581_: f32;
    var edge_278_7_phi_569_: u32;
    var edge_278_7_phi_421_: f32;
    var edge_278_7_phi_414_: f32;
    var edge_278_7_phi_362_: f32;
    var edge_278_7_phi_358_: f32;
    var edge_278_7_phi_350_: u32;
    var edge_278_7_phi_335_: u32;
    var edge_278_7_phi_333_: u32;
    var edge_241_7_phi_581_: f32;
    var edge_241_7_phi_569_: u32;
    var edge_241_7_phi_421_: f32;
    var edge_241_7_phi_414_: f32;
    var edge_241_7_phi_362_: f32;
    var edge_241_7_phi_358_: f32;
    var edge_241_7_phi_350_: u32;
    var edge_241_7_phi_335_: u32;
    var edge_241_7_phi_333_: u32;
    var edge_8_7_phi_581_: f32;
    var edge_8_7_phi_569_: u32;
    var edge_8_7_phi_421_: f32;
    var edge_8_7_phi_414_: f32;
    var edge_8_7_phi_362_: f32;
    var edge_8_7_phi_358_: f32;
    var edge_8_7_phi_350_: u32;
    var edge_8_7_phi_335_: u32;
    var edge_8_7_phi_333_: u32;
    var edge_3_7_phi_581_: f32;
    var edge_3_7_phi_569_: u32;
    var edge_3_7_phi_421_: f32;
    var edge_3_7_phi_414_: f32;
    var edge_3_7_phi_362_: f32;
    var edge_3_7_phi_358_: f32;
    var edge_3_7_phi_350_: u32;
    var edge_3_7_phi_335_: u32;
    var edge_3_7_phi_333_: u32;
    var edge_7_2_phi_580_: f32;
    var edge_7_2_phi_568_: u32;
    var edge_7_2_phi_420_: f32;
    var edge_7_2_phi_413_: f32;
    var edge_7_2_phi_361_: f32;
    var edge_7_2_phi_357_: f32;
    var edge_7_2_phi_349_: u32;
    var edge_7_2_phi_334_: u32;
    var edge_7_2_phi_332_: u32;
    var edge_7_2_phi_130_: u32;
    var structured_result: u32;
    var structured_did_return: bool = false;
    var phi_317_: u32;
    var edge_306_34_phi_317_: u32;
    var edge_33_34_phi_317_: u32;
    var edge_4_34_phi_317_: u32;

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
    let _e49 = orbit[i32(2001u)].re_w0_bits;
    let _e51 = orbit[i32(2001u)].re_w1_bits;
    let _e53 = orbit[i32(2001u)].re_w2_bits;
    let _e55 = orbit[i32(2001u)].re_w3_bits;
    let _e57 = orbit[i32(2001u)].im_w0_bits;
    let _e59 = orbit[i32(2001u)].im_w1_bits;
    let _e61 = orbit[i32(2001u)].im_w2_bits;
    let _e63 = orbit[i32(2001u)].im_w3_bits;
    let _e72 = -(bitcast<f32>(_e49));
    let _e73 = -(bitcast<f32>(_e51));
    let _e74 = -(bitcast<f32>(_e53));
    let _e77 = (_e8 + 0f);
    let _e78 = (_e77 - _e8);
    let _e84 = (_e77 + _e72);
    let _e85 = (_e84 - _e77);
    let _e90 = (((_e8 - (_e77 - _e78)) + (0f - _e78)) + ((_e77 - (_e84 - _e85)) + (_e72 - _e85)));
    let _e91 = (_e10 + _e90);
    let _e92 = (_e91 - _e10);
    let _e97 = (_e91 + _e73);
    let _e98 = (_e97 - _e91);
    let _e103 = (((_e10 - (_e91 - _e92)) + (_e90 - _e92)) + ((_e91 - (_e97 - _e98)) + (_e73 - _e98)));
    let _e104 = (_e12 + _e103);
    let _e105 = (_e104 - _e12);
    let _e110 = (_e104 + _e74);
    let _e111 = (_e110 - _e104);
    let _e118 = (((((_e12 - (_e104 - _e105)) + (_e103 - _e105)) + ((_e104 - (_e110 - _e111)) + (_e74 - _e111))) + _e14) + -(bitcast<f32>(_e55)));
    let _e119 = (_e110 + _e118);
    let _e120 = (_e119 - _e110);
    let _e124 = ((_e110 - (_e119 - _e120)) + (_e118 - _e120));
    let _e125 = (_e97 + _e119);
    let _e126 = (_e125 - _e97);
    let _e130 = ((_e97 - (_e125 - _e126)) + (_e119 - _e126));
    let _e131 = (_e84 + _e125);
    let _e132 = (_e131 - _e84);
    let _e136 = ((_e84 - (_e131 - _e132)) + (_e125 - _e132));
    let _e137 = -(bitcast<f32>(_e57));
    let _e138 = -(bitcast<f32>(_e59));
    let _e139 = -(bitcast<f32>(_e61));
    let _e142 = (_e16 + 0f);
    let _e143 = (_e142 - _e16);
    let _e149 = (_e142 + _e137);
    let _e150 = (_e149 - _e142);
    let _e155 = (((_e16 - (_e142 - _e143)) + (0f - _e143)) + ((_e142 - (_e149 - _e150)) + (_e137 - _e150)));
    let _e156 = (_e18 + _e155);
    let _e157 = (_e156 - _e18);
    let _e162 = (_e156 + _e138);
    let _e163 = (_e162 - _e156);
    let _e168 = (((_e18 - (_e156 - _e157)) + (_e155 - _e157)) + ((_e156 - (_e162 - _e163)) + (_e138 - _e163)));
    let _e169 = (_e20 + _e168);
    let _e170 = (_e169 - _e20);
    let _e175 = (_e169 + _e139);
    let _e176 = (_e175 - _e169);
    let _e183 = (((((_e20 - (_e169 - _e170)) + (_e168 - _e170)) + ((_e169 - (_e175 - _e176)) + (_e139 - _e176))) + _e22) + -(bitcast<f32>(_e63)));
    let _e184 = (_e175 + _e183);
    let _e185 = (_e184 - _e175);
    let _e189 = ((_e175 - (_e184 - _e185)) + (_e183 - _e185));
    let _e190 = (_e162 + _e184);
    let _e191 = (_e190 - _e162);
    let _e195 = ((_e162 - (_e190 - _e191)) + (_e184 - _e191));
    let _e196 = (_e149 + _e190);
    let _e197 = (_e196 - _e149);
    let _e201 = ((_e149 - (_e196 - _e197)) + (_e190 - _e197));
    let _e202 = (((((f32(bitcast<i32>(u32(pos.x))) + 0.5f) / _e26) * 2f) - 1f) * _e24);
    let _e203 = (_e131 + _e202);
    let _e204 = (_e203 - _e131);
    let _e209 = (_e203 + _e136);
    let _e210 = (_e209 - _e203);
    let _e215 = (_e209 + _e130);
    let _e216 = (_e215 - _e209);
    let _e221 = (_e215 + _e124);
    let _e222 = (_e221 - _e215);
    let _e231 = ((1f - (((f32(bitcast<i32>(u32(pos.y))) + 0.5f) / _e26) * 2f)) * _e24);
    let _e232 = (_e196 + _e231);
    let _e233 = (_e232 - _e196);
    let _e238 = (_e232 + _e201);
    let _e239 = (_e238 - _e232);
    let _e244 = (_e238 + _e195);
    let _e245 = (_e244 - _e238);
    let _e250 = (_e244 + _e189);
    let _e251 = (_e250 - _e244);
    let _e264 = orbit[i32(2002u)].re_w0_bits;
    let _e266 = orbit[i32(2002u)].re_w1_bits;
    edge_0_2_phi_580_ = 4f;
    edge_0_2_phi_568_ = _e266;
    edge_0_2_phi_420_ = 0f;
    edge_0_2_phi_413_ = 0f;
    edge_0_2_phi_361_ = 0f;
    edge_0_2_phi_357_ = 0f;
    edge_0_2_phi_349_ = 0u;
    edge_0_2_phi_334_ = 0u;
    edge_0_2_phi_332_ = 0u;
    edge_0_2_phi_130_ = 0u;
    let _e287 = edge_0_2_phi_580_;
    let _e289 = edge_0_2_phi_568_;
    let _e291 = edge_0_2_phi_420_;
    let _e293 = edge_0_2_phi_413_;
    let _e295 = edge_0_2_phi_361_;
    let _e297 = edge_0_2_phi_357_;
    let _e299 = edge_0_2_phi_349_;
    let _e301 = edge_0_2_phi_334_;
    let _e303 = edge_0_2_phi_332_;
    let _e305 = edge_0_2_phi_130_;
    phi_580_ = _e287;
    phi_568_ = _e289;
    phi_420_ = _e291;
    phi_413_ = _e293;
    phi_361_ = _e295;
    phi_357_ = _e297;
    phi_349_ = _e299;
    phi_334_ = _e301;
    phi_332_ = _e303;
    phi_130_ = _e305;
    loop {
        let _e318 = phi_580_;
        let _e320 = phi_568_;
        let _e322 = phi_420_;
        let _e324 = phi_413_;
        let _e326 = phi_361_;
        let _e328 = phi_357_;
        let _e330 = phi_349_;
        let _e332 = phi_334_;
        let _e334 = phi_332_;
        let _e336 = phi_130_;
        let _e337 = (_e336 < _e266);
        if _e337 {
            if (_e334 == 0u) {
                if (_e332 == 0u) {
                    if ((_e330 + 1u) < _e264) {
                        edge_10_11_phi_419_ = _e322;
                        edge_10_11_phi_412_ = _e324;
                        edge_10_11_phi_149_ = _e330;
                        let _e349 = edge_10_11_phi_419_;
                        let _e351 = edge_10_11_phi_412_;
                        let _e353 = edge_10_11_phi_149_;
                        phi_419_ = _e349;
                        phi_412_ = _e351;
                        phi_149_ = _e353;
                    } else {
                        edge_5_11_phi_419_ = _e326;
                        edge_5_11_phi_412_ = _e328;
                        edge_5_11_phi_149_ = 0u;
                        let _e362 = edge_5_11_phi_419_;
                        let _e364 = edge_5_11_phi_412_;
                        let _e366 = edge_5_11_phi_149_;
                        phi_419_ = _e362;
                        phi_412_ = _e364;
                        phi_149_ = _e366;
                    }
                    let _e371 = phi_419_;
                    let _e373 = phi_412_;
                    let _e375 = phi_149_;
                    let _e379 = orbit[i32(_e375)].re_w0_bits;
                    let _e381 = orbit[i32(_e375)].re_w1_bits;
                    let _e383 = orbit[i32(_e375)].re_w2_bits;
                    let _e385 = orbit[i32(_e375)].re_w3_bits;
                    let _e387 = orbit[i32(_e375)].im_w0_bits;
                    let _e389 = orbit[i32(_e375)].im_w1_bits;
                    let _e391 = orbit[i32(_e375)].im_w2_bits;
                    let _e393 = orbit[i32(_e375)].im_w3_bits;
                    let _e395 = (_e375 + 1u);
                    let _e399 = orbit[i32(_e395)].re_w0_bits;
                    let _e401 = orbit[i32(_e395)].re_w1_bits;
                    let _e403 = orbit[i32(_e395)].re_w2_bits;
                    let _e405 = orbit[i32(_e395)].re_w3_bits;
                    let _e407 = orbit[i32(_e395)].im_w0_bits;
                    let _e409 = orbit[i32(_e395)].im_w1_bits;
                    let _e411 = orbit[i32(_e395)].im_w2_bits;
                    let _e413 = orbit[i32(_e395)].im_w3_bits;
                    if (_e379 == 2143289344u) {
                        edge_11_241_phi_779_ = true;
                        let _e431 = edge_11_241_phi_779_;
                        phi_779_ = _e431;
                    } else {
                        if (_e387 == 2143289344u) {
                            edge_242_241_phi_779_ = true;
                            let _e421 = edge_242_241_phi_779_;
                            phi_779_ = _e421;
                        } else {
                            edge_240_241_phi_779_ = false;
                            let _e426 = edge_240_241_phi_779_;
                            phi_779_ = _e426;
                        }
                    }
                    let _e434 = phi_779_;
                    if _e434 {
                        edge_241_7_phi_581_ = _e318;
                        edge_241_7_phi_569_ = _e320;
                        edge_241_7_phi_421_ = _e371;
                        edge_241_7_phi_414_ = _e373;
                        edge_241_7_phi_362_ = _e326;
                        edge_241_7_phi_358_ = _e328;
                        edge_241_7_phi_350_ = _e375;
                        edge_241_7_phi_335_ = 1u;
                        edge_241_7_phi_333_ = _e334;
                        let _e954 = edge_241_7_phi_581_;
                        let _e956 = edge_241_7_phi_569_;
                        let _e958 = edge_241_7_phi_421_;
                        let _e960 = edge_241_7_phi_414_;
                        let _e962 = edge_241_7_phi_362_;
                        let _e964 = edge_241_7_phi_358_;
                        let _e966 = edge_241_7_phi_350_;
                        let _e968 = edge_241_7_phi_335_;
                        let _e970 = edge_241_7_phi_333_;
                        phi_581_ = _e954;
                        phi_569_ = _e956;
                        phi_421_ = _e958;
                        phi_414_ = _e960;
                        phi_362_ = _e962;
                        phi_358_ = _e964;
                        phi_350_ = _e966;
                        phi_335_ = _e968;
                        phi_333_ = _e970;
                    } else {
                        if (_e399 == 2143289344u) {
                            edge_275_278_phi_838_ = true;
                            let _e452 = edge_275_278_phi_838_;
                            phi_838_ = _e452;
                        } else {
                            if (_e407 == 2143289344u) {
                                edge_279_278_phi_838_ = true;
                                let _e442 = edge_279_278_phi_838_;
                                phi_838_ = _e442;
                            } else {
                                edge_277_278_phi_838_ = false;
                                let _e447 = edge_277_278_phi_838_;
                                phi_838_ = _e447;
                            }
                        }
                        let _e455 = phi_838_;
                        if _e455 {
                            edge_278_7_phi_581_ = _e318;
                            edge_278_7_phi_569_ = _e320;
                            edge_278_7_phi_421_ = _e371;
                            edge_278_7_phi_414_ = _e373;
                            edge_278_7_phi_362_ = _e326;
                            edge_278_7_phi_358_ = _e328;
                            edge_278_7_phi_350_ = _e375;
                            edge_278_7_phi_335_ = 1u;
                            edge_278_7_phi_333_ = _e334;
                            let _e917 = edge_278_7_phi_581_;
                            let _e919 = edge_278_7_phi_569_;
                            let _e921 = edge_278_7_phi_421_;
                            let _e923 = edge_278_7_phi_414_;
                            let _e925 = edge_278_7_phi_362_;
                            let _e927 = edge_278_7_phi_358_;
                            let _e929 = edge_278_7_phi_350_;
                            let _e931 = edge_278_7_phi_335_;
                            let _e933 = edge_278_7_phi_333_;
                            phi_581_ = _e917;
                            phi_569_ = _e919;
                            phi_421_ = _e921;
                            phi_414_ = _e923;
                            phi_362_ = _e925;
                            phi_358_ = _e927;
                            phi_350_ = _e929;
                            phi_335_ = _e931;
                            phi_333_ = _e933;
                        } else {
                            let _e456 = bitcast<f32>(_e379);
                            let _e457 = bitcast<f32>(_e381);
                            let _e458 = bitcast<f32>(_e383);
                            let _e459 = bitcast<f32>(_e385);
                            let _e460 = bitcast<f32>(_e387);
                            let _e461 = bitcast<f32>(_e389);
                            let _e462 = bitcast<f32>(_e391);
                            let _e463 = bitcast<f32>(_e393);
                            let _e464 = bitcast<f32>(_e399);
                            let _e465 = bitcast<f32>(_e401);
                            let _e466 = bitcast<f32>(_e403);
                            let _e467 = bitcast<f32>(_e405);
                            let _e468 = bitcast<f32>(_e407);
                            let _e469 = bitcast<f32>(_e409);
                            let _e470 = bitcast<f32>(_e411);
                            let _e471 = bitcast<f32>(_e413);
                            let _e509 = ((((2f * ((_e456 * _e373) - (_e460 * _e371))) + (2f * ((((((_e457 * _e373) - (_e461 * _e371)) + (_e458 * _e373)) - (_e462 * _e371)) + (_e459 * _e373)) - (_e463 * _e371)))) + ((_e373 * _e373) - (_e371 * _e371))) + (_e221 + (((((_e215 - (_e221 - _e222)) + (_e124 - _e222)) + ((_e209 - (_e215 - _e216)) + (_e130 - _e216))) + ((_e203 - (_e209 - _e210)) + (_e136 - _e210))) + ((_e131 - (_e203 - _e204)) + (_e202 - _e204)))));
                            let _e519 = ((((2f * ((_e456 * _e371) + (_e460 * _e373))) + (2f * ((((((_e457 * _e371) + (_e461 * _e373)) + (_e458 * _e371)) + (_e462 * _e373)) + (_e459 * _e371)) + (_e463 * _e373)))) + ((2f * _e373) * _e371)) + (_e250 + (((((_e244 - (_e250 - _e251)) + (_e189 - _e251)) + ((_e238 - (_e244 - _e245)) + (_e195 - _e245))) + ((_e232 - (_e238 - _e239)) + (_e201 - _e239))) + ((_e196 - (_e232 - _e233)) + (_e231 - _e233)))));
                            let _e520 = (_e464 + _e509);
                            let _e521 = (_e520 - _e464);
                            let _e526 = (_e520 + _e465);
                            let _e527 = (_e526 - _e520);
                            let _e532 = (_e526 + _e466);
                            let _e533 = (_e532 - _e526);
                            let _e538 = (_e532 + _e467);
                            let _e539 = (_e538 - _e532);
                            let _e547 = (_e538 + (((((_e532 - (_e538 - _e539)) + (_e467 - _e539)) + ((_e526 - (_e532 - _e533)) + (_e466 - _e533))) + ((_e520 - (_e526 - _e527)) + (_e465 - _e527))) + ((_e464 - (_e520 - _e521)) + (_e509 - _e521))));
                            let _e548 = (_e468 + _e519);
                            let _e549 = (_e548 - _e468);
                            let _e554 = (_e548 + _e469);
                            let _e555 = (_e554 - _e548);
                            let _e560 = (_e554 + _e470);
                            let _e561 = (_e560 - _e554);
                            let _e566 = (_e560 + _e471);
                            let _e567 = (_e566 - _e560);
                            let _e575 = (_e566 + (((((_e560 - (_e566 - _e567)) + (_e471 - _e567)) + ((_e554 - (_e560 - _e561)) + (_e470 - _e561))) + ((_e548 - (_e554 - _e555)) + (_e469 - _e555))) + ((_e468 - (_e548 - _e549)) + (_e519 - _e549))));
                            let _e578 = ((_e547 * _e547) + (_e575 * _e575));
                            let _e584 = (4f < _e578);
                            if _e584 {
                                let _e592 = bitcast<u32>(1f);
                                let _e593 = bitcast<u32>(_e578);
                                if (bitcast<f32>((bitcast<u32>((_e578 - 4f)) & 2147483647u)) <= (0.000004f * bitcast<f32>(select(select(_e593, _e592, ((_e592 ^ ((0u - (_e592 >> 31u)) | 2147483648u)) > (_e593 ^ ((0u - (_e593 >> 31u)) | 2147483648u)))), 2143289344u, (((_e592 & 2147483647u) > 2139095040u) || ((_e593 & 2147483647u) > 2139095040u)))))) {
                                    edge_19_18_phi_285_ = true;
                                    let _e623 = edge_19_18_phi_285_;
                                    phi_285_ = _e623;
                                } else {
                                    edge_17_18_phi_285_ = false;
                                    let _e628 = edge_17_18_phi_285_;
                                    phi_285_ = _e628;
                                }
                            } else {
                                edge_17_18_phi_285_1 = false;
                                let _e633 = edge_17_18_phi_285_1;
                                phi_285_ = _e633;
                            }
                            let _e636 = phi_285_;
                            if ((_e509 - _e509) == 0f) {
                                if ((_e519 - _e519) == 0f) {
                                    if ((_e578 - _e578) == 0f) {
                                        if _e636 {
                                            edge_23_7_phi_581_ = _e318;
                                            edge_23_7_phi_569_ = _e320;
                                            edge_23_7_phi_421_ = _e371;
                                            edge_23_7_phi_414_ = _e373;
                                            edge_23_7_phi_362_ = _e326;
                                            edge_23_7_phi_358_ = _e328;
                                            edge_23_7_phi_350_ = _e375;
                                            edge_23_7_phi_335_ = 1u;
                                            edge_23_7_phi_333_ = _e334;
                                            let _e769 = edge_23_7_phi_581_;
                                            let _e771 = edge_23_7_phi_569_;
                                            let _e773 = edge_23_7_phi_421_;
                                            let _e775 = edge_23_7_phi_414_;
                                            let _e777 = edge_23_7_phi_362_;
                                            let _e779 = edge_23_7_phi_358_;
                                            let _e781 = edge_23_7_phi_350_;
                                            let _e783 = edge_23_7_phi_335_;
                                            let _e785 = edge_23_7_phi_333_;
                                            phi_581_ = _e769;
                                            phi_569_ = _e771;
                                            phi_421_ = _e773;
                                            phi_414_ = _e775;
                                            phi_362_ = _e777;
                                            phi_358_ = _e779;
                                            phi_350_ = _e781;
                                            phi_335_ = _e783;
                                            phi_333_ = _e785;
                                        } else {
                                            if _e584 {
                                                edge_26_7_phi_581_ = _e578;
                                                edge_26_7_phi_569_ = (_e336 + 1u);
                                                edge_26_7_phi_421_ = _e519;
                                                edge_26_7_phi_414_ = _e509;
                                                edge_26_7_phi_362_ = _e575;
                                                edge_26_7_phi_358_ = _e547;
                                                edge_26_7_phi_350_ = _e375;
                                                edge_26_7_phi_335_ = _e332;
                                                edge_26_7_phi_333_ = 1u;
                                                let _e732 = edge_26_7_phi_581_;
                                                let _e734 = edge_26_7_phi_569_;
                                                let _e736 = edge_26_7_phi_421_;
                                                let _e738 = edge_26_7_phi_414_;
                                                let _e740 = edge_26_7_phi_362_;
                                                let _e742 = edge_26_7_phi_358_;
                                                let _e744 = edge_26_7_phi_350_;
                                                let _e746 = edge_26_7_phi_335_;
                                                let _e748 = edge_26_7_phi_333_;
                                                phi_581_ = _e732;
                                                phi_569_ = _e734;
                                                phi_421_ = _e736;
                                                phi_414_ = _e738;
                                                phi_362_ = _e740;
                                                phi_358_ = _e742;
                                                phi_350_ = _e744;
                                                phi_335_ = _e746;
                                                phi_333_ = _e748;
                                            } else {
                                                if (_e578 < ((_e509 * _e509) + (_e519 * _e519))) {
                                                    edge_27_7_phi_581_ = _e318;
                                                    edge_27_7_phi_569_ = _e320;
                                                    edge_27_7_phi_421_ = _e575;
                                                    edge_27_7_phi_414_ = _e547;
                                                    edge_27_7_phi_362_ = _e575;
                                                    edge_27_7_phi_358_ = _e547;
                                                    edge_27_7_phi_350_ = 0u;
                                                    edge_27_7_phi_335_ = _e332;
                                                    edge_27_7_phi_333_ = _e334;
                                                    let _e659 = edge_27_7_phi_581_;
                                                    let _e661 = edge_27_7_phi_569_;
                                                    let _e663 = edge_27_7_phi_421_;
                                                    let _e665 = edge_27_7_phi_414_;
                                                    let _e667 = edge_27_7_phi_362_;
                                                    let _e669 = edge_27_7_phi_358_;
                                                    let _e671 = edge_27_7_phi_350_;
                                                    let _e673 = edge_27_7_phi_335_;
                                                    let _e675 = edge_27_7_phi_333_;
                                                    phi_581_ = _e659;
                                                    phi_569_ = _e661;
                                                    phi_421_ = _e663;
                                                    phi_414_ = _e665;
                                                    phi_362_ = _e667;
                                                    phi_358_ = _e669;
                                                    phi_350_ = _e671;
                                                    phi_335_ = _e673;
                                                    phi_333_ = _e675;
                                                } else {
                                                    edge_30_7_phi_581_ = _e318;
                                                    edge_30_7_phi_569_ = _e320;
                                                    edge_30_7_phi_421_ = _e519;
                                                    edge_30_7_phi_414_ = _e509;
                                                    edge_30_7_phi_362_ = _e575;
                                                    edge_30_7_phi_358_ = _e547;
                                                    edge_30_7_phi_350_ = _e395;
                                                    edge_30_7_phi_335_ = _e332;
                                                    edge_30_7_phi_333_ = _e334;
                                                    let _e695 = edge_30_7_phi_581_;
                                                    let _e697 = edge_30_7_phi_569_;
                                                    let _e699 = edge_30_7_phi_421_;
                                                    let _e701 = edge_30_7_phi_414_;
                                                    let _e703 = edge_30_7_phi_362_;
                                                    let _e705 = edge_30_7_phi_358_;
                                                    let _e707 = edge_30_7_phi_350_;
                                                    let _e709 = edge_30_7_phi_335_;
                                                    let _e711 = edge_30_7_phi_333_;
                                                    phi_581_ = _e695;
                                                    phi_569_ = _e697;
                                                    phi_421_ = _e699;
                                                    phi_414_ = _e701;
                                                    phi_362_ = _e703;
                                                    phi_358_ = _e705;
                                                    phi_350_ = _e707;
                                                    phi_335_ = _e709;
                                                    phi_333_ = _e711;
                                                }
                                            }
                                        }
                                    } else {
                                        edge_297_7_phi_581_ = _e318;
                                        edge_297_7_phi_569_ = _e320;
                                        edge_297_7_phi_421_ = _e371;
                                        edge_297_7_phi_414_ = _e373;
                                        edge_297_7_phi_362_ = _e326;
                                        edge_297_7_phi_358_ = _e328;
                                        edge_297_7_phi_350_ = _e375;
                                        edge_297_7_phi_335_ = 1u;
                                        edge_297_7_phi_333_ = _e334;
                                        let _e806 = edge_297_7_phi_581_;
                                        let _e808 = edge_297_7_phi_569_;
                                        let _e810 = edge_297_7_phi_421_;
                                        let _e812 = edge_297_7_phi_414_;
                                        let _e814 = edge_297_7_phi_362_;
                                        let _e816 = edge_297_7_phi_358_;
                                        let _e818 = edge_297_7_phi_350_;
                                        let _e820 = edge_297_7_phi_335_;
                                        let _e822 = edge_297_7_phi_333_;
                                        phi_581_ = _e806;
                                        phi_569_ = _e808;
                                        phi_421_ = _e810;
                                        phi_414_ = _e812;
                                        phi_362_ = _e814;
                                        phi_358_ = _e816;
                                        phi_350_ = _e818;
                                        phi_335_ = _e820;
                                        phi_333_ = _e822;
                                    }
                                } else {
                                    edge_300_7_phi_581_ = _e318;
                                    edge_300_7_phi_569_ = _e320;
                                    edge_300_7_phi_421_ = _e371;
                                    edge_300_7_phi_414_ = _e373;
                                    edge_300_7_phi_362_ = _e326;
                                    edge_300_7_phi_358_ = _e328;
                                    edge_300_7_phi_350_ = _e375;
                                    edge_300_7_phi_335_ = 1u;
                                    edge_300_7_phi_333_ = _e334;
                                    let _e843 = edge_300_7_phi_581_;
                                    let _e845 = edge_300_7_phi_569_;
                                    let _e847 = edge_300_7_phi_421_;
                                    let _e849 = edge_300_7_phi_414_;
                                    let _e851 = edge_300_7_phi_362_;
                                    let _e853 = edge_300_7_phi_358_;
                                    let _e855 = edge_300_7_phi_350_;
                                    let _e857 = edge_300_7_phi_335_;
                                    let _e859 = edge_300_7_phi_333_;
                                    phi_581_ = _e843;
                                    phi_569_ = _e845;
                                    phi_421_ = _e847;
                                    phi_414_ = _e849;
                                    phi_362_ = _e851;
                                    phi_358_ = _e853;
                                    phi_350_ = _e855;
                                    phi_335_ = _e857;
                                    phi_333_ = _e859;
                                }
                            } else {
                                edge_18_7_phi_581_ = _e318;
                                edge_18_7_phi_569_ = _e320;
                                edge_18_7_phi_421_ = _e371;
                                edge_18_7_phi_414_ = _e373;
                                edge_18_7_phi_362_ = _e326;
                                edge_18_7_phi_358_ = _e328;
                                edge_18_7_phi_350_ = _e375;
                                edge_18_7_phi_335_ = 1u;
                                edge_18_7_phi_333_ = _e334;
                                let _e880 = edge_18_7_phi_581_;
                                let _e882 = edge_18_7_phi_569_;
                                let _e884 = edge_18_7_phi_421_;
                                let _e886 = edge_18_7_phi_414_;
                                let _e888 = edge_18_7_phi_362_;
                                let _e890 = edge_18_7_phi_358_;
                                let _e892 = edge_18_7_phi_350_;
                                let _e894 = edge_18_7_phi_335_;
                                let _e896 = edge_18_7_phi_333_;
                                phi_581_ = _e880;
                                phi_569_ = _e882;
                                phi_421_ = _e884;
                                phi_414_ = _e886;
                                phi_362_ = _e888;
                                phi_358_ = _e890;
                                phi_350_ = _e892;
                                phi_335_ = _e894;
                                phi_333_ = _e896;
                            }
                        }
                    }
                } else {
                    edge_8_7_phi_581_ = _e318;
                    edge_8_7_phi_569_ = _e320;
                    edge_8_7_phi_421_ = _e322;
                    edge_8_7_phi_414_ = _e324;
                    edge_8_7_phi_362_ = _e326;
                    edge_8_7_phi_358_ = _e328;
                    edge_8_7_phi_350_ = _e330;
                    edge_8_7_phi_335_ = _e332;
                    edge_8_7_phi_333_ = _e334;
                    let _e990 = edge_8_7_phi_581_;
                    let _e992 = edge_8_7_phi_569_;
                    let _e994 = edge_8_7_phi_421_;
                    let _e996 = edge_8_7_phi_414_;
                    let _e998 = edge_8_7_phi_362_;
                    let _e1000 = edge_8_7_phi_358_;
                    let _e1002 = edge_8_7_phi_350_;
                    let _e1004 = edge_8_7_phi_335_;
                    let _e1006 = edge_8_7_phi_333_;
                    phi_581_ = _e990;
                    phi_569_ = _e992;
                    phi_421_ = _e994;
                    phi_414_ = _e996;
                    phi_362_ = _e998;
                    phi_358_ = _e1000;
                    phi_350_ = _e1002;
                    phi_335_ = _e1004;
                    phi_333_ = _e1006;
                }
            } else {
                edge_3_7_phi_581_ = _e318;
                edge_3_7_phi_569_ = _e320;
                edge_3_7_phi_421_ = _e322;
                edge_3_7_phi_414_ = _e324;
                edge_3_7_phi_362_ = _e326;
                edge_3_7_phi_358_ = _e328;
                edge_3_7_phi_350_ = _e330;
                edge_3_7_phi_335_ = _e332;
                edge_3_7_phi_333_ = _e334;
                let _e1026 = edge_3_7_phi_581_;
                let _e1028 = edge_3_7_phi_569_;
                let _e1030 = edge_3_7_phi_421_;
                let _e1032 = edge_3_7_phi_414_;
                let _e1034 = edge_3_7_phi_362_;
                let _e1036 = edge_3_7_phi_358_;
                let _e1038 = edge_3_7_phi_350_;
                let _e1040 = edge_3_7_phi_335_;
                let _e1042 = edge_3_7_phi_333_;
                phi_581_ = _e1026;
                phi_569_ = _e1028;
                phi_421_ = _e1030;
                phi_414_ = _e1032;
                phi_362_ = _e1034;
                phi_358_ = _e1036;
                phi_350_ = _e1038;
                phi_335_ = _e1040;
                phi_333_ = _e1042;
            }
            let _e1053 = phi_581_;
            let _e1055 = phi_569_;
            let _e1057 = phi_421_;
            let _e1059 = phi_414_;
            let _e1061 = phi_362_;
            let _e1063 = phi_358_;
            let _e1065 = phi_350_;
            let _e1067 = phi_335_;
            let _e1069 = phi_333_;
            edge_7_2_phi_580_ = _e1053;
            edge_7_2_phi_568_ = _e1055;
            edge_7_2_phi_420_ = _e1057;
            edge_7_2_phi_413_ = _e1059;
            edge_7_2_phi_361_ = _e1061;
            edge_7_2_phi_357_ = _e1063;
            edge_7_2_phi_349_ = _e1065;
            edge_7_2_phi_334_ = _e1067;
            edge_7_2_phi_332_ = _e1069;
            edge_7_2_phi_130_ = (_e336 + 1u);
            let _e1083 = edge_7_2_phi_580_;
            let _e1085 = edge_7_2_phi_568_;
            let _e1087 = edge_7_2_phi_420_;
            let _e1089 = edge_7_2_phi_413_;
            let _e1091 = edge_7_2_phi_361_;
            let _e1093 = edge_7_2_phi_357_;
            let _e1095 = edge_7_2_phi_349_;
            let _e1097 = edge_7_2_phi_334_;
            let _e1099 = edge_7_2_phi_332_;
            let _e1101 = edge_7_2_phi_130_;
            phi_580_ = _e1083;
            phi_568_ = _e1085;
            phi_420_ = _e1087;
            phi_413_ = _e1089;
            phi_361_ = _e1091;
            phi_357_ = _e1093;
            phi_349_ = _e1095;
            phi_334_ = _e1097;
            phi_332_ = _e1099;
            phi_130_ = _e1101;
            continue;
        } else {
            loop_header_carry_131_ = _e337;
            break;
        }
    }
    let _e1114 = phi_580_;
    let _e1116 = phi_568_;
    let _e1128 = phi_334_;
    let _e1130 = phi_332_;
    if (_e1128 == 1u) {
        edge_4_34_phi_317_ = 4294902015u;
        let _e1479 = edge_4_34_phi_317_;
        phi_317_ = _e1479;
    } else {
        if (_e1130 == 1u) {
            let _e1146 = ((f32(bitcast<i32>(_e1116)) + (4f / _e1114)) / 256f);
            let _e1152 = ((6.2831855f * (_e1146 + 0f)) + 1.5707964f);
            let _e1160 = (_e1152 - (6.2831855f * floor(((_e1152 / 6.2831855f) + 0.5f))));
            let _e1170 = ((1.2732395f * _e1160) - ((0.40528473f * _e1160) * bitcast<f32>((bitcast<u32>(_e1160) & 2147483647u))));
            let _e1189 = ((6.2831855f * (_e1146 + 0.33f)) + 1.5707964f);
            let _e1197 = (_e1189 - (6.2831855f * floor(((_e1189 / 6.2831855f) + 0.5f))));
            let _e1207 = ((1.2732395f * _e1197) - ((0.40528473f * _e1197) * bitcast<f32>((bitcast<u32>(_e1197) & 2147483647u))));
            let _e1226 = ((6.2831855f * (_e1146 + 0.67f)) + 1.5707964f);
            let _e1234 = (_e1226 - (6.2831855f * floor(((_e1226 / 6.2831855f) + 0.5f))));
            let _e1244 = ((1.2732395f * _e1234) - ((0.40528473f * _e1234) * bitcast<f32>((bitcast<u32>(_e1234) & 2147483647u))));
            let _e1260 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e1170 * bitcast<f32>((bitcast<u32>(_e1170) & 2147483647u))) - _e1170)) + _e1170))));
            let _e1261 = bitcast<u32>(0f);
            let _e1285 = bitcast<u32>(bitcast<f32>(select(select(_e1261, _e1260, ((_e1260 ^ ((0u - (_e1260 >> 31u)) | 2147483648u)) > (_e1261 ^ ((0u - (_e1261 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1260 & 2147483647u) > 2139095040u) || ((_e1261 & 2147483647u) > 2139095040u)))));
            let _e1286 = bitcast<u32>(1f);
            let _e1311 = (bitcast<f32>(select(select(_e1286, _e1285, ((_e1285 ^ ((0u - (_e1285 >> 31u)) | 2147483648u)) < (_e1286 ^ ((0u - (_e1286 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1285 & 2147483647u) > 2139095040u) || ((_e1286 & 2147483647u) > 2139095040u)))) * 255f);
            let _e1327 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e1207 * bitcast<f32>((bitcast<u32>(_e1207) & 2147483647u))) - _e1207)) + _e1207))));
            let _e1328 = bitcast<u32>(0f);
            let _e1352 = bitcast<u32>(bitcast<f32>(select(select(_e1328, _e1327, ((_e1327 ^ ((0u - (_e1327 >> 31u)) | 2147483648u)) > (_e1328 ^ ((0u - (_e1328 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1327 & 2147483647u) > 2139095040u) || ((_e1328 & 2147483647u) > 2139095040u)))));
            let _e1353 = bitcast<u32>(1f);
            let _e1378 = (bitcast<f32>(select(select(_e1353, _e1352, ((_e1352 ^ ((0u - (_e1352 >> 31u)) | 2147483648u)) < (_e1353 ^ ((0u - (_e1353 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1352 & 2147483647u) > 2139095040u) || ((_e1353 & 2147483647u) > 2139095040u)))) * 255f);
            let _e1394 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e1244 * bitcast<f32>((bitcast<u32>(_e1244) & 2147483647u))) - _e1244)) + _e1244))));
            let _e1395 = bitcast<u32>(0f);
            let _e1419 = bitcast<u32>(bitcast<f32>(select(select(_e1395, _e1394, ((_e1394 ^ ((0u - (_e1394 >> 31u)) | 2147483648u)) > (_e1395 ^ ((0u - (_e1395 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1394 & 2147483647u) > 2139095040u) || ((_e1395 & 2147483647u) > 2139095040u)))));
            let _e1420 = bitcast<u32>(1f);
            let _e1445 = (bitcast<f32>(select(select(_e1420, _e1419, ((_e1419 ^ ((0u - (_e1419 >> 31u)) | 2147483648u)) < (_e1420 ^ ((0u - (_e1420 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1419 & 2147483647u) > 2139095040u) || ((_e1420 & 2147483647u) > 2139095040u)))) * 255f);
            edge_306_34_phi_317_ = (((select(0u, select(select(bitcast<u32>(i32(_e1311)), 2147483648u, (_e1311 <= -2147483600f)), 2147483647u, (_e1311 >= 2147483600f)), (_e1311 == _e1311)) + (select(0u, select(select(bitcast<u32>(i32(_e1378)), 2147483648u, (_e1378 <= -2147483600f)), 2147483647u, (_e1378 >= 2147483600f)), (_e1378 == _e1378)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e1445)), 2147483648u, (_e1445 <= -2147483600f)), 2147483647u, (_e1445 >= 2147483600f)), (_e1445 == _e1445)) << 16u)) + 4278190080u);
            let _e1469 = edge_306_34_phi_317_;
            phi_317_ = _e1469;
        } else {
            edge_33_34_phi_317_ = 4278190080u;
            let _e1474 = edge_33_34_phi_317_;
            phi_317_ = _e1474;
        }
    }
    let _e1482 = phi_317_;
    return unpack4x8unorm(_e1482);
}
