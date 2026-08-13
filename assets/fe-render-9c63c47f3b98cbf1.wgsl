struct RasterState {
    p7_: f32,
    p8_: f32,
    p9_: f32,
    p10_: f32,
    p11_: u32,
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
    p60_: f32,
    p61_: f32,
    p62_: f32,
    p63_: u32,
}

struct RasterVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) v0_: f32,
    @location(1) v1_: f32,
    @location(2) v2_: f32,
    @location(3) v3_: f32,
    @location(4) v4_: f32,
    @location(5) v5_: f32,
    @location(6) v6_: f32,
}

@group(0) @binding(0)
var<storage> state: RasterState;

@vertex
fn surface_vertex(@builtin(vertex_index) vertex_index: u32) -> RasterVertexOutput {
    var structured_result: f32;
    var structured_did_return: bool = false;
    var phi_1200_: f32;
    var phi_1201_: f32;
    var phi_1202_: f32;
    var phi_1203_: f32;
    var phi_1204_: f32;
    var phi_1205_: f32;
    var phi_1206_: f32;
    var phi_1207_: f32;
    var phi_1208_: f32;
    var phi_1209_: f32;
    var phi_1210_: f32;
    var phi_141_: u32;
    var edge_9_18_phi_141_: u32;
    var edge_17_18_phi_141_: u32;
    var phi_144_: u32;
    var edge_18_21_phi_144_: u32;
    var edge_20_21_phi_144_: u32;
    var phi_147_: u32;
    var edge_21_24_phi_147_: u32;
    var edge_23_24_phi_147_: u32;
    var phi_150_: u32;
    var edge_24_27_phi_150_: u32;
    var edge_26_27_phi_150_: u32;
    var phi_153_: u32;
    var phi_154_: u32;
    var edge_27_30_phi_153_: u32;
    var edge_27_30_phi_154_: u32;
    var edge_29_30_phi_153_: u32;
    var edge_29_30_phi_154_: u32;
    var phi_525_: f32;
    var phi_526_: u32;
    var phi_527_: f32;
    var phi_528_: f32;
    var edge_247_246_phi_525_: f32;
    var edge_247_246_phi_526_: u32;
    var edge_247_246_phi_527_: f32;
    var edge_247_246_phi_528_: f32;
    var edge_251_246_phi_525_: f32;
    var edge_251_246_phi_526_: u32;
    var edge_251_246_phi_527_: f32;
    var edge_251_246_phi_528_: f32;
    var edge_242_246_phi_525_: f32;
    var edge_242_246_phi_526_: u32;
    var edge_242_246_phi_527_: f32;
    var edge_242_246_phi_528_: f32;
    var phi_504_: f32;
    var edge_259_261_phi_504_: f32;
    var edge_255_261_phi_504_: f32;
    var phi_512_: f32;
    var phi_513_: f32;
    var edge_268_270_phi_512_: f32;
    var edge_268_270_phi_513_: f32;
    var edge_261_270_phi_512_: f32;
    var edge_261_270_phi_513_: f32;
    var edge_283_246_phi_525_: f32;
    var edge_283_246_phi_526_: u32;
    var edge_283_246_phi_527_: f32;
    var edge_283_246_phi_528_: f32;
    var edge_287_246_phi_525_: f32;
    var edge_287_246_phi_526_: u32;
    var edge_287_246_phi_527_: f32;
    var edge_287_246_phi_528_: f32;
    var edge_270_246_phi_525_: f32;
    var edge_270_246_phi_526_: u32;
    var edge_270_246_phi_527_: f32;
    var edge_270_246_phi_528_: f32;
    var edge_245_246_phi_525_: f32;
    var edge_245_246_phi_526_: u32;
    var edge_245_246_phi_527_: f32;
    var edge_245_246_phi_528_: f32;
    var phi_532_: f32;
    var edge_289_291_phi_532_: f32;
    var edge_293_291_phi_532_: f32;
    var edge_246_291_phi_532_: f32;
    var phi_534_: f32;
    var edge_291_297_phi_534_: f32;
    var edge_296_297_phi_534_: f32;
    var phi_537_: f32;
    var edge_297_300_phi_537_: f32;
    var edge_299_300_phi_537_: f32;
    var phi_606_: f32;
    var edge_306_324_phi_606_: f32;
    var edge_323_324_phi_606_: f32;
    var phi_957_: f32;
    var edge_382_384_phi_957_: f32;
    var edge_324_384_phi_957_: f32;
    var edge_384_670_phi_1200_: f32;
    var edge_384_670_phi_1201_: f32;
    var edge_384_670_phi_1202_: f32;
    var edge_384_670_phi_1203_: f32;
    var edge_384_670_phi_1204_: f32;
    var edge_384_670_phi_1205_: f32;
    var edge_384_670_phi_1206_: f32;
    var edge_384_670_phi_1207_: f32;
    var edge_384_670_phi_1208_: f32;
    var edge_384_670_phi_1209_: f32;
    var edge_384_670_phi_1210_: f32;
    var edge_510_670_phi_1200_: f32;
    var edge_510_670_phi_1201_: f32;
    var edge_510_670_phi_1202_: f32;
    var edge_510_670_phi_1203_: f32;
    var edge_510_670_phi_1204_: f32;
    var edge_510_670_phi_1205_: f32;
    var edge_510_670_phi_1206_: f32;
    var edge_510_670_phi_1207_: f32;
    var edge_510_670_phi_1208_: f32;
    var edge_510_670_phi_1209_: f32;
    var edge_510_670_phi_1210_: f32;
    var edge_300_670_phi_1200_: f32;
    var edge_300_670_phi_1201_: f32;
    var edge_300_670_phi_1202_: f32;
    var edge_300_670_phi_1203_: f32;
    var edge_300_670_phi_1204_: f32;
    var edge_300_670_phi_1205_: f32;
    var edge_300_670_phi_1206_: f32;
    var edge_300_670_phi_1207_: f32;
    var edge_300_670_phi_1208_: f32;
    var edge_300_670_phi_1209_: f32;
    var edge_300_670_phi_1210_: f32;
    var phi_1092_: f32;
    var phi_1093_: f32;
    var phi_1094_: f32;
    var edge_538_542_phi_1092_: f32;
    var edge_538_542_phi_1093_: f32;
    var edge_538_542_phi_1094_: f32;
    var edge_541_542_phi_1092_: f32;
    var edge_541_542_phi_1093_: f32;
    var edge_541_542_phi_1094_: f32;
    var edge_535_542_phi_1092_: f32;
    var edge_535_542_phi_1093_: f32;
    var edge_535_542_phi_1094_: f32;
    var edge_532_542_phi_1092_: f32;
    var edge_532_542_phi_1093_: f32;
    var edge_532_542_phi_1094_: f32;
    var edge_529_542_phi_1092_: f32;
    var edge_529_542_phi_1093_: f32;
    var edge_529_542_phi_1094_: f32;
    var edge_526_542_phi_1092_: f32;
    var edge_526_542_phi_1093_: f32;
    var edge_526_542_phi_1094_: f32;
    var edge_523_542_phi_1092_: f32;
    var edge_523_542_phi_1093_: f32;
    var edge_523_542_phi_1094_: f32;
    var edge_520_542_phi_1092_: f32;
    var edge_520_542_phi_1093_: f32;
    var edge_520_542_phi_1094_: f32;
    var edge_515_542_phi_1092_: f32;
    var edge_515_542_phi_1093_: f32;
    var edge_515_542_phi_1094_: f32;
    var phi_1181_: f32;
    var edge_542_650_phi_1181_: f32;
    var edge_649_650_phi_1181_: f32;
    var phi_1183_: f32;
    var edge_650_653_phi_1183_: f32;
    var edge_652_653_phi_1183_: f32;
    var phi_1185_: f32;
    var edge_653_656_phi_1185_: f32;
    var edge_655_656_phi_1185_: f32;
    var phi_1187_: f32;
    var edge_656_659_phi_1187_: f32;
    var edge_658_659_phi_1187_: f32;
    var phi_1189_: f32;
    var phi_1190_: f32;
    var edge_659_662_phi_1189_: f32;
    var edge_659_662_phi_1190_: f32;
    var edge_661_662_phi_1189_: f32;
    var edge_661_662_phi_1190_: f32;
    var edge_662_670_phi_1200_: f32;
    var edge_662_670_phi_1201_: f32;
    var edge_662_670_phi_1202_: f32;
    var edge_662_670_phi_1203_: f32;
    var edge_662_670_phi_1204_: f32;
    var edge_662_670_phi_1205_: f32;
    var edge_662_670_phi_1206_: f32;
    var edge_662_670_phi_1207_: f32;
    var edge_662_670_phi_1208_: f32;
    var edge_662_670_phi_1209_: f32;
    var edge_662_670_phi_1210_: f32;

    let _e3 = state.p7_;
    let _e5 = state.p8_;
    let _e7 = state.p9_;
    let _e9 = state.p10_;
    let _e13 = state.p12_;
    let _e15 = state.p13_;
    let _e17 = state.p14_;
    let _e21 = state.p16_;
    let _e23 = state.p17_;
    let _e25 = state.p18_;
    let _e27 = state.p19_;
    let _e29 = state.p20_;
    let _e31 = state.p21_;
    let _e33 = state.p22_;
    let _e35 = state.p23_;
    let _e37 = state.p24_;
    let _e39 = state.p25_;
    let _e41 = state.p26_;
    let _e43 = state.p27_;
    let _e45 = state.p28_;
    let _e47 = state.p29_;
    let _e49 = state.p30_;
    let _e51 = state.p31_;
    let _e53 = state.p32_;
    let _e55 = state.p33_;
    let _e57 = state.p34_;
    let _e59 = state.p35_;
    let _e61 = state.p36_;
    let _e63 = state.p37_;
    let _e65 = state.p38_;
    let _e67 = state.p39_;
    let _e69 = state.p40_;
    let _e71 = state.p41_;
    let _e73 = state.p42_;
    let _e75 = state.p43_;
    let _e77 = state.p44_;
    let _e79 = state.p45_;
    let _e81 = state.p46_;
    let _e83 = state.p47_;
    let _e85 = state.p48_;
    let _e87 = state.p49_;
    let _e89 = state.p50_;
    let _e91 = state.p51_;
    let _e93 = state.p52_;
    let _e95 = state.p53_;
    let _e97 = state.p54_;
    let _e99 = state.p55_;
    let _e101 = state.p56_;
    let _e103 = state.p57_;
    let _e105 = state.p58_;
    let _e107 = state.p59_;
    let _e109 = state.p60_;
    let _e111 = state.p61_;
    let _e113 = state.p62_;
    if (vertex_index < 13824u) {
        let _e120 = (vertex_index / 6912u);
        let _e122 = f32(bitcast<i32>(_e120));
        let _e124 = (vertex_index % 6912u);
        let _e126 = (_e124 / 6u);
        let _e128 = (_e124 % 6u);
        if (_e128 == 1u) {
            edge_9_18_phi_141_ = 1u;
            let _e138 = edge_9_18_phi_141_;
            phi_141_ = _e138;
        } else {
            edge_17_18_phi_141_ = 0u;
            let _e143 = edge_17_18_phi_141_;
            phi_141_ = _e143;
        }
        let _e146 = phi_141_;
        if (_e128 == 2u) {
            edge_18_21_phi_144_ = 1u;
            let _e152 = edge_18_21_phi_144_;
            phi_144_ = _e152;
        } else {
            edge_20_21_phi_144_ = 0u;
            let _e157 = edge_20_21_phi_144_;
            phi_144_ = _e157;
        }
        let _e160 = phi_144_;
        if (_e128 == 3u) {
            edge_21_24_phi_147_ = 1u;
            let _e166 = edge_21_24_phi_147_;
            phi_147_ = _e166;
        } else {
            edge_23_24_phi_147_ = _e160;
            let _e170 = edge_23_24_phi_147_;
            phi_147_ = _e170;
        }
        let _e173 = phi_147_;
        if (_e128 == 4u) {
            edge_24_27_phi_150_ = 1u;
            let _e179 = edge_24_27_phi_150_;
            phi_150_ = _e179;
        } else {
            edge_26_27_phi_150_ = _e146;
            let _e183 = edge_26_27_phi_150_;
            phi_150_ = _e183;
        }
        let _e186 = phi_150_;
        if (_e128 == 5u) {
            edge_27_30_phi_153_ = 1u;
            edge_27_30_phi_154_ = 1u;
            let _e194 = edge_27_30_phi_153_;
            let _e196 = edge_27_30_phi_154_;
            phi_153_ = _e194;
            phi_154_ = _e196;
        } else {
            edge_29_30_phi_153_ = _e173;
            edge_29_30_phi_154_ = _e186;
            let _e202 = edge_29_30_phi_153_;
            let _e204 = edge_29_30_phi_154_;
            phi_153_ = _e202;
            phi_154_ = _e204;
        }
        let _e208 = phi_153_;
        let _e210 = phi_154_;
        let _e219 = ((3.1415927f * f32(bitcast<i32>(((_e126 / 48u) + _e210)))) / f32(bitcast<i32>(24u)));
        let _e228 = ((6.2831855f * f32(bitcast<i32>(((_e126 % 48u) + _e208)))) / f32(bitcast<i32>(48u)));
        let _e236 = (_e219 - (6.2831855f * floor(((_e219 / 6.2831855f) + 0.5f))));
        let _e246 = ((1.2732395f * _e236) - ((0.40528473f * _e236) * bitcast<f32>((bitcast<u32>(_e236) & 2147483647u))));
        let _e255 = ((0.225f * ((_e246 * bitcast<f32>((bitcast<u32>(_e246) & 2147483647u))) - _e246)) + _e246);
        let _e257 = (_e228 + 1.5707964f);
        let _e265 = (_e257 - (6.2831855f * floor(((_e257 / 6.2831855f) + 0.5f))));
        let _e275 = ((1.2732395f * _e265) - ((0.40528473f * _e265) * bitcast<f32>((bitcast<u32>(_e265) & 2147483647u))));
        let _e285 = (_e255 * ((0.225f * ((_e275 * bitcast<f32>((bitcast<u32>(_e275) & 2147483647u))) - _e275)) + _e275));
        let _e287 = (_e219 + 1.5707964f);
        let _e295 = (_e287 - (6.2831855f * floor(((_e287 / 6.2831855f) + 0.5f))));
        let _e305 = ((1.2732395f * _e295) - ((0.40528473f * _e295) * bitcast<f32>((bitcast<u32>(_e295) & 2147483647u))));
        let _e314 = ((0.225f * ((_e305 * bitcast<f32>((bitcast<u32>(_e305) & 2147483647u))) - _e305)) + _e305);
        let _e322 = (_e228 - (6.2831855f * floor(((_e228 / 6.2831855f) + 0.5f))));
        let _e332 = ((1.2732395f * _e322) - ((0.40528473f * _e322) * bitcast<f32>((bitcast<u32>(_e322) & 2147483647u))));
        let _e342 = (_e255 * ((0.225f * ((_e332 * bitcast<f32>((bitcast<u32>(_e332) & 2147483647u))) - _e332)) + _e332));
        let _e344 = (3.1415927f * _e3);
        let _e346 = (_e344 + 1.5707964f);
        let _e354 = (_e346 - (6.2831855f * floor(((_e346 / 6.2831855f) + 0.5f))));
        let _e364 = ((1.2732395f * _e354) - ((0.40528473f * _e354) * bitcast<f32>((bitcast<u32>(_e354) & 2147483647u))));
        let _e373 = ((0.225f * ((_e364 * bitcast<f32>((bitcast<u32>(_e364) & 2147483647u))) - _e364)) + _e364);
        let _e381 = (_e344 - (6.2831855f * floor(((_e344 / 6.2831855f) + 0.5f))));
        let _e391 = ((1.2732395f * _e381) - ((0.40528473f * _e381) * bitcast<f32>((bitcast<u32>(_e381) & 2147483647u))));
        let _e400 = ((0.225f * ((_e391 * bitcast<f32>((bitcast<u32>(_e391) & 2147483647u))) - _e391)) + _e391);
        let _e421 = ((_e373 * _e33) + (_e400 * _e53));
        let _e424 = ((_e373 * _e35) + (_e400 * _e55));
        let _e427 = ((_e373 * _e37) + (_e400 * _e57));
        let _e448 = (-2f * ((_e373 * _e21) + (_e400 * _e41)));
        let _e450 = (-2f * ((_e373 * _e23) + (_e400 * _e43)));
        let _e452 = (-2f * ((_e373 * _e25) + (_e400 * _e45)));
        let _e453 = -(((_e373 * _e27) + (_e400 * _e47)));
        let _e454 = -(((_e373 * _e29) + (_e400 * _e49)));
        let _e455 = -(((_e373 * _e31) + (_e400 * _e51)));
        let _e456 = (_e13 - _e13);
        let _e465 = -((1f * (0f - (((_e373 * _e39) + (_e400 * _e59)) / 3f))));
        let _e488 = ((((((_e456 + (_e13 * _e421)) + (_e15 * _e424)) + (_e17 * _e427)) + _e465) + ((((_e456 + _e465) + _e465) + -((((_e13 * _e13) * 0.5f) * _e448))) + -((((_e15 * _e15) * 0.5f) * _e450)))) + ((((_e456 + -((((_e17 * _e17) * 0.5f) * _e452))) + -(((_e13 * _e15) * _e453))) + -(((_e13 * _e17) * _e454))) + -(((_e15 * _e17) * _e455))));
        let _e489 = (_e13 + _e285);
        let _e490 = (_e15 + _e314);
        let _e491 = (_e17 + _e342);
        let _e504 = (_e489 - _e489);
        let _e533 = ((((((_e504 + (_e489 * _e421)) + (_e490 * _e424)) + (_e491 * _e427)) + _e465) + ((((_e504 + _e465) + _e465) + -((((_e489 * _e489) * 0.5f) * _e448))) + -((((_e490 * _e490) * 0.5f) * _e450)))) + ((((_e504 + -((((_e491 * _e491) * 0.5f) * _e452))) + -(((_e489 * _e490) * _e453))) + -(((_e489 * _e491) * _e454))) + -(((_e490 * _e491) * _e455))));
        let _e534 = (_e13 - _e285);
        let _e535 = (_e15 - _e314);
        let _e536 = (_e17 - _e342);
        let _e549 = (_e534 - _e534);
        let _e578 = ((((((_e549 + (_e534 * _e421)) + (_e535 * _e424)) + (_e536 * _e427)) + _e465) + ((((_e549 + _e465) + _e465) + -((((_e534 * _e534) * 0.5f) * _e448))) + -((((_e535 * _e535) * 0.5f) * _e450)))) + ((((_e549 + -((((_e536 * _e536) * 0.5f) * _e452))) + -(((_e534 * _e535) * _e453))) + -(((_e534 * _e536) * _e454))) + -(((_e535 * _e536) * _e455))));
        let _e582 = (((_e533 + _e578) * 0.5f) - _e488);
        let _e585 = ((_e533 - _e578) * 0.5f);
        if (bitcast<f32>((bitcast<u32>(_e582) & 2147483647u)) < 0.0000001f) {
            if (0.0000001f <= bitcast<f32>((bitcast<u32>(_e585) & 2147483647u))) {
                let _e599 = (-(_e488) / _e585);
                if (0.001f < _e599) {
                    edge_247_246_phi_525_ = 0f;
                    edge_247_246_phi_526_ = 1u;
                    edge_247_246_phi_527_ = _e599;
                    edge_247_246_phi_528_ = _e599;
                    let _e609 = edge_247_246_phi_525_;
                    let _e611 = edge_247_246_phi_526_;
                    let _e613 = edge_247_246_phi_527_;
                    let _e615 = edge_247_246_phi_528_;
                    phi_525_ = _e609;
                    phi_526_ = _e611;
                    phi_527_ = _e613;
                    phi_528_ = _e615;
                } else {
                    edge_251_246_phi_525_ = 0f;
                    edge_251_246_phi_526_ = 0u;
                    edge_251_246_phi_527_ = -1f;
                    edge_251_246_phi_528_ = -1f;
                    let _e629 = edge_251_246_phi_525_;
                    let _e631 = edge_251_246_phi_526_;
                    let _e633 = edge_251_246_phi_527_;
                    let _e635 = edge_251_246_phi_528_;
                    phi_525_ = _e629;
                    phi_526_ = _e631;
                    phi_527_ = _e633;
                    phi_528_ = _e635;
                }
            } else {
                edge_242_246_phi_525_ = 0f;
                edge_242_246_phi_526_ = 0u;
                edge_242_246_phi_527_ = -1f;
                edge_242_246_phi_528_ = -1f;
                let _e649 = edge_242_246_phi_525_;
                let _e651 = edge_242_246_phi_526_;
                let _e653 = edge_242_246_phi_527_;
                let _e655 = edge_242_246_phi_528_;
                phi_525_ = _e649;
                phi_526_ = _e651;
                phi_527_ = _e653;
                phi_528_ = _e655;
            }
        } else {
            let _e664 = ((_e585 * _e585) - ((4f * _e582) * _e488));
            if (0f <= _e664) {
                let _e667 = sqrt(_e664);
                if (_e585 < 0f) {
                    edge_259_261_phi_504_ = -(_e667);
                    let _e673 = edge_259_261_phi_504_;
                    phi_504_ = _e673;
                } else {
                    edge_255_261_phi_504_ = _e667;
                    let _e677 = edge_255_261_phi_504_;
                    phi_504_ = _e677;
                }
                let _e680 = phi_504_;
                let _e683 = (-0.5f * (_e585 + _e680));
                if (0.0000001f <= bitcast<f32>((bitcast<u32>(_e683) & 2147483647u))) {
                    edge_268_270_phi_512_ = (_e488 / _e683);
                    edge_268_270_phi_513_ = (_e683 / _e582);
                    let _e695 = edge_268_270_phi_512_;
                    let _e697 = edge_268_270_phi_513_;
                    phi_512_ = _e695;
                    phi_513_ = _e697;
                } else {
                    edge_261_270_phi_512_ = 0f;
                    edge_261_270_phi_513_ = 0f;
                    let _e705 = edge_261_270_phi_512_;
                    let _e707 = edge_261_270_phi_513_;
                    phi_512_ = _e705;
                    phi_513_ = _e707;
                }
                let _e711 = phi_512_;
                let _e713 = phi_513_;
                let _e714 = bitcast<u32>(_e713);
                let _e715 = bitcast<u32>(_e711);
                let _e738 = bitcast<f32>(select(select(_e715, _e714, ((_e714 ^ ((0u - (_e714 >> 31u)) | 2147483648u)) < (_e715 ^ ((0u - (_e715 >> 31u)) | 2147483648u)))), 2143289344u, (((_e714 & 2147483647u) > 2139095040u) || ((_e715 & 2147483647u) > 2139095040u))));
                let _e739 = bitcast<u32>(_e713);
                let _e740 = bitcast<u32>(_e711);
                let _e763 = bitcast<f32>(select(select(_e740, _e739, ((_e739 ^ ((0u - (_e739 >> 31u)) | 2147483648u)) > (_e740 ^ ((0u - (_e740 >> 31u)) | 2147483648u)))), 2143289344u, (((_e739 & 2147483647u) > 2139095040u) || ((_e740 & 2147483647u) > 2139095040u))));
                if (0.001f < _e763) {
                    if (0.001f < _e738) {
                        edge_283_246_phi_525_ = _e664;
                        edge_283_246_phi_526_ = 1u;
                        edge_283_246_phi_527_ = _e763;
                        edge_283_246_phi_528_ = _e738;
                        let _e774 = edge_283_246_phi_525_;
                        let _e776 = edge_283_246_phi_526_;
                        let _e778 = edge_283_246_phi_527_;
                        let _e780 = edge_283_246_phi_528_;
                        phi_525_ = _e774;
                        phi_526_ = _e776;
                        phi_527_ = _e778;
                        phi_528_ = _e780;
                    } else {
                        edge_287_246_phi_525_ = _e664;
                        edge_287_246_phi_526_ = 1u;
                        edge_287_246_phi_527_ = _e763;
                        edge_287_246_phi_528_ = _e763;
                        let _e791 = edge_287_246_phi_525_;
                        let _e793 = edge_287_246_phi_526_;
                        let _e795 = edge_287_246_phi_527_;
                        let _e797 = edge_287_246_phi_528_;
                        phi_525_ = _e791;
                        phi_526_ = _e793;
                        phi_527_ = _e795;
                        phi_528_ = _e797;
                    }
                } else {
                    edge_270_246_phi_525_ = _e664;
                    edge_270_246_phi_526_ = 0u;
                    edge_270_246_phi_527_ = -1f;
                    edge_270_246_phi_528_ = -1f;
                    let _e810 = edge_270_246_phi_525_;
                    let _e812 = edge_270_246_phi_526_;
                    let _e814 = edge_270_246_phi_527_;
                    let _e816 = edge_270_246_phi_528_;
                    phi_525_ = _e810;
                    phi_526_ = _e812;
                    phi_527_ = _e814;
                    phi_528_ = _e816;
                }
            } else {
                edge_245_246_phi_525_ = _e664;
                edge_245_246_phi_526_ = 0u;
                edge_245_246_phi_527_ = -1f;
                edge_245_246_phi_528_ = -1f;
                let _e829 = edge_245_246_phi_525_;
                let _e831 = edge_245_246_phi_526_;
                let _e833 = edge_245_246_phi_527_;
                let _e835 = edge_245_246_phi_528_;
                phi_525_ = _e829;
                phi_526_ = _e831;
                phi_527_ = _e833;
                phi_528_ = _e835;
            }
        }
        let _e841 = phi_525_;
        let _e843 = phi_526_;
        let _e845 = phi_527_;
        let _e847 = phi_528_;
        if (_e120 == 1u) {
            if (_e847 < _e845) {
                edge_289_291_phi_532_ = _e845;
                let _e853 = edge_289_291_phi_532_;
                phi_532_ = _e853;
            } else {
                edge_293_291_phi_532_ = -1f;
                let _e858 = edge_293_291_phi_532_;
                phi_532_ = _e858;
            }
        } else {
            edge_246_291_phi_532_ = _e847;
            let _e862 = edge_246_291_phi_532_;
            phi_532_ = _e862;
        }
        let _e865 = phi_532_;
        if (_e843 == 0u) {
            edge_291_297_phi_534_ = -1f;
            let _e871 = edge_291_297_phi_534_;
            phi_534_ = _e871;
        } else {
            edge_296_297_phi_534_ = _e865;
            let _e875 = edge_296_297_phi_534_;
            phi_534_ = _e875;
        }
        let _e878 = phi_534_;
        if (12f < _e878) {
            edge_297_300_phi_537_ = -1f;
            let _e884 = edge_297_300_phi_537_;
            phi_537_ = _e884;
        } else {
            edge_299_300_phi_537_ = _e878;
            let _e888 = edge_299_300_phi_537_;
            phi_537_ = _e888;
        }
        let _e891 = phi_537_;
        if (_e891 <= 0f) {
            edge_300_670_phi_1200_ = 0f;
            edge_300_670_phi_1201_ = 0f;
            edge_300_670_phi_1202_ = _e122;
            edge_300_670_phi_1203_ = 0f;
            edge_300_670_phi_1204_ = 0f;
            edge_300_670_phi_1205_ = 0f;
            edge_300_670_phi_1206_ = 0f;
            edge_300_670_phi_1207_ = 1f;
            edge_300_670_phi_1208_ = 2f;
            edge_300_670_phi_1209_ = 0f;
            edge_300_670_phi_1210_ = 0f;
            let _e1474 = edge_300_670_phi_1200_;
            let _e1476 = edge_300_670_phi_1201_;
            let _e1478 = edge_300_670_phi_1202_;
            let _e1480 = edge_300_670_phi_1203_;
            let _e1482 = edge_300_670_phi_1204_;
            let _e1484 = edge_300_670_phi_1205_;
            let _e1486 = edge_300_670_phi_1206_;
            let _e1488 = edge_300_670_phi_1207_;
            let _e1490 = edge_300_670_phi_1208_;
            let _e1492 = edge_300_670_phi_1209_;
            let _e1494 = edge_300_670_phi_1210_;
            phi_1200_ = _e1474;
            phi_1201_ = _e1476;
            phi_1202_ = _e1478;
            phi_1203_ = _e1480;
            phi_1204_ = _e1482;
            phi_1205_ = _e1484;
            phi_1206_ = _e1486;
            phi_1207_ = _e1488;
            phi_1208_ = _e1490;
            phi_1209_ = _e1492;
            phi_1210_ = _e1494;
        } else {
            let _e895 = (_e13 + (_e285 * _e891));
            let _e897 = (_e15 + (_e314 * _e891));
            let _e899 = (_e17 + (_e342 * _e891));
            let _e912 = (_e895 - _e895);
            let _e914 = (_e912 + (_e895 * _e421));
            let _e915 = (_e897 * _e424);
            let _e916 = (_e914 + _e915);
            let _e917 = (_e899 * _e427);
            let _e924 = (((_e912 + _e465) + _e465) + -((((_e895 * _e895) * 0.5f) * _e448)));
            let _e926 = -((((_e897 * _e897) * 0.5f) * _e450));
            let _e927 = (_e924 + _e926);
            let _e930 = -((((_e899 * _e899) * 0.5f) * _e452));
            let _e931 = (_e912 + _e930);
            let _e933 = -(((_e895 * _e897) * _e453));
            let _e936 = -(((_e895 * _e899) * _e454));
            let _e939 = -(((_e897 * _e899) * _e455));
            if ((0.004f * (1f + (_e891 * _e891))) < bitcast<f32>((bitcast<u32>(((((_e916 + _e917) + _e465) + _e927) + (((_e931 + _e933) + _e936) + _e939))) & 2147483647u))) {
                edge_306_324_phi_606_ = 1f;
                let _e955 = edge_306_324_phi_606_;
                phi_606_ = _e955;
            } else {
                edge_323_324_phi_606_ = 0f;
                let _e960 = edge_323_324_phi_606_;
                phi_606_ = _e960;
            }
            let _e963 = phi_606_;
            let _e965 = (_e895 + 1f);
            let _e971 = (_e965 - _e965);
            let _e994 = (_e895 - 1f);
            let _e1000 = (_e994 - _e994);
            let _e1024 = ((((((((_e971 + (_e965 * _e421)) + _e915) + _e917) + _e465) + ((((_e971 + _e465) + _e465) + -((((_e965 * _e965) * 0.5f) * _e448))) + _e926)) + ((((_e971 + _e930) + -(((_e965 * _e897) * _e453))) + -(((_e965 * _e899) * _e454))) + _e939)) - ((((((_e1000 + (_e994 * _e421)) + _e915) + _e917) + _e465) + ((((_e1000 + _e465) + _e465) + -((((_e994 * _e994) * 0.5f) * _e448))) + _e926)) + ((((_e1000 + _e930) + -(((_e994 * _e897) * _e453))) + -(((_e994 * _e899) * _e454))) + _e939))) * 0.5f);
            let _e1026 = (_e897 + 1f);
            let _e1049 = (_e897 - 1f);
            let _e1073 = (((((((_e914 + (_e1026 * _e424)) + _e917) + _e465) + (_e924 + -((((_e1026 * _e1026) * 0.5f) * _e450)))) + (((_e931 + -(((_e895 * _e1026) * _e453))) + _e936) + -(((_e1026 * _e899) * _e455)))) - (((((_e914 + (_e1049 * _e424)) + _e917) + _e465) + (_e924 + -((((_e1049 * _e1049) * 0.5f) * _e450)))) + (((_e931 + -(((_e895 * _e1049) * _e453))) + _e936) + -(((_e1049 * _e899) * _e455))))) * 0.5f);
            let _e1075 = (_e899 + 1f);
            let _e1097 = (_e899 - 1f);
            let _e1120 = ((((((_e916 + (_e1075 * _e427)) + _e465) + _e927) + ((((_e912 + -((((_e1075 * _e1075) * 0.5f) * _e452))) + _e933) + -(((_e895 * _e1075) * _e454))) + -(((_e897 * _e1075) * _e455)))) - ((((_e916 + (_e1097 * _e427)) + _e465) + _e927) + ((((_e912 + -((((_e1097 * _e1097) * 0.5f) * _e452))) + _e933) + -(((_e895 * _e1097) * _e454))) + -(((_e897 * _e1097) * _e455))))) * 0.5f);
            let _e1126 = sqrt((((_e1024 * _e1024) + (_e1073 * _e1073)) + (_e1120 * _e1120)));
            if (0.0000001f < _e1126) {
                edge_382_384_phi_957_ = (1f / _e1126);
                let _e1133 = edge_382_384_phi_957_;
                phi_957_ = _e1133;
            } else {
                edge_324_384_phi_957_ = 0f;
                let _e1138 = edge_324_384_phi_957_;
                phi_957_ = _e1138;
            }
            let _e1141 = phi_957_;
            let _e1160 = bitcast<u32>((1f - ((_e841 / (((_e585 * _e585) + (4f * bitcast<f32>((bitcast<u32>((_e582 * _e488)) & 2147483647u)))) + 0.000001f)) * 5f)));
            let _e1161 = bitcast<u32>(0f);
            let _e1185 = bitcast<u32>(bitcast<f32>(select(select(_e1161, _e1160, ((_e1160 ^ ((0u - (_e1160 >> 31u)) | 2147483648u)) > (_e1161 ^ ((0u - (_e1161 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1160 & 2147483647u) > 2139095040u) || ((_e1161 & 2147483647u) > 2139095040u)))));
            let _e1186 = bitcast<u32>(1f);
            let _e1210 = (_e895 - _e13);
            let _e1211 = (_e897 - _e15);
            let _e1212 = (_e899 - _e17);
            let _e1214 = (_e5 + 1.5707964f);
            let _e1222 = (_e1214 - (6.2831855f * floor(((_e1214 / 6.2831855f) + 0.5f))));
            let _e1232 = ((1.2732395f * _e1222) - ((0.40528473f * _e1222) * bitcast<f32>((bitcast<u32>(_e1222) & 2147483647u))));
            let _e1241 = ((0.225f * ((_e1232 * bitcast<f32>((bitcast<u32>(_e1232) & 2147483647u))) - _e1232)) + _e1232);
            let _e1249 = (_e5 - (6.2831855f * floor(((_e5 / 6.2831855f) + 0.5f))));
            let _e1259 = ((1.2732395f * _e1249) - ((0.40528473f * _e1249) * bitcast<f32>((bitcast<u32>(_e1249) & 2147483647u))));
            let _e1268 = ((0.225f * ((_e1259 * bitcast<f32>((bitcast<u32>(_e1259) & 2147483647u))) - _e1259)) + _e1259);
            let _e1270 = (_e7 + 1.5707964f);
            let _e1278 = (_e1270 - (6.2831855f * floor(((_e1270 / 6.2831855f) + 0.5f))));
            let _e1288 = ((1.2732395f * _e1278) - ((0.40528473f * _e1278) * bitcast<f32>((bitcast<u32>(_e1278) & 2147483647u))));
            let _e1297 = ((0.225f * ((_e1288 * bitcast<f32>((bitcast<u32>(_e1288) & 2147483647u))) - _e1288)) + _e1288);
            let _e1305 = (_e7 - (6.2831855f * floor(((_e7 / 6.2831855f) + 0.5f))));
            let _e1315 = ((1.2732395f * _e1305) - ((0.40528473f * _e1305) * bitcast<f32>((bitcast<u32>(_e1305) & 2147483647u))));
            let _e1324 = ((0.225f * ((_e1315 * bitcast<f32>((bitcast<u32>(_e1315) & 2147483647u))) - _e1315)) + _e1315);
            let _e1330 = ((_e1241 * _e1212) - (_e1268 * _e1210));
            let _e1337 = (((_e1324 * _e1211) + (_e1297 * _e1330)) + _e9);
            if (_e1337 < 0.25f) {
                edge_384_670_phi_1200_ = 0f;
                edge_384_670_phi_1201_ = 0f;
                edge_384_670_phi_1202_ = _e122;
                edge_384_670_phi_1203_ = 0f;
                edge_384_670_phi_1204_ = 0f;
                edge_384_670_phi_1205_ = 0f;
                edge_384_670_phi_1206_ = 0f;
                edge_384_670_phi_1207_ = 1f;
                edge_384_670_phi_1208_ = 2f;
                edge_384_670_phi_1209_ = 0f;
                edge_384_670_phi_1210_ = 0f;
                let _e1375 = edge_384_670_phi_1200_;
                let _e1377 = edge_384_670_phi_1201_;
                let _e1379 = edge_384_670_phi_1202_;
                let _e1381 = edge_384_670_phi_1203_;
                let _e1383 = edge_384_670_phi_1204_;
                let _e1385 = edge_384_670_phi_1205_;
                let _e1387 = edge_384_670_phi_1206_;
                let _e1389 = edge_384_670_phi_1207_;
                let _e1391 = edge_384_670_phi_1208_;
                let _e1393 = edge_384_670_phi_1209_;
                let _e1395 = edge_384_670_phi_1210_;
                phi_1200_ = _e1375;
                phi_1201_ = _e1377;
                phi_1202_ = _e1379;
                phi_1203_ = _e1381;
                phi_1204_ = _e1383;
                phi_1205_ = _e1385;
                phi_1206_ = _e1387;
                phi_1207_ = _e1389;
                phi_1208_ = _e1391;
                phi_1209_ = _e1393;
                phi_1210_ = _e1395;
            } else {
                edge_510_670_phi_1200_ = 0f;
                edge_510_670_phi_1201_ = _e963;
                edge_510_670_phi_1202_ = _e122;
                edge_510_670_phi_1203_ = bitcast<f32>(select(select(_e1186, _e1185, ((_e1185 ^ ((0u - (_e1185 >> 31u)) | 2147483648u)) < (_e1186 ^ ((0u - (_e1186 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1185 & 2147483647u) > 2139095040u) || ((_e1186 & 2147483647u) > 2139095040u))));
                edge_510_670_phi_1204_ = (_e1120 * _e1141);
                edge_510_670_phi_1205_ = (_e1073 * _e1141);
                edge_510_670_phi_1206_ = (_e1024 * _e1141);
                edge_510_670_phi_1207_ = _e1337;
                edge_510_670_phi_1208_ = (((_e1337 * 64f) - 16f) / 63.75f);
                edge_510_670_phi_1209_ = (((_e1297 * _e1211) - (_e1324 * _e1330)) * 1.6f);
                edge_510_670_phi_1210_ = (((_e1241 * _e1210) + (_e1268 * _e1212)) * 1.6f);
                let _e1420 = edge_510_670_phi_1200_;
                let _e1422 = edge_510_670_phi_1201_;
                let _e1424 = edge_510_670_phi_1202_;
                let _e1426 = edge_510_670_phi_1203_;
                let _e1428 = edge_510_670_phi_1204_;
                let _e1430 = edge_510_670_phi_1205_;
                let _e1432 = edge_510_670_phi_1206_;
                let _e1434 = edge_510_670_phi_1207_;
                let _e1436 = edge_510_670_phi_1208_;
                let _e1438 = edge_510_670_phi_1209_;
                let _e1440 = edge_510_670_phi_1210_;
                phi_1200_ = _e1420;
                phi_1201_ = _e1422;
                phi_1202_ = _e1424;
                phi_1203_ = _e1426;
                phi_1204_ = _e1428;
                phi_1205_ = _e1430;
                phi_1206_ = _e1432;
                phi_1207_ = _e1434;
                phi_1208_ = _e1436;
                phi_1209_ = _e1438;
                phi_1210_ = _e1440;
            }
        }
    } else {
        let _e1507 = (vertex_index - 13824u);
        let _e1509 = (_e1507 / 6u);
        let _e1511 = (_e1507 % 6u);
        if (_e1509 == 0u) {
            edge_515_542_phi_1092_ = _e65;
            edge_515_542_phi_1093_ = _e63;
            edge_515_542_phi_1094_ = _e61;
            let _e1628 = edge_515_542_phi_1092_;
            let _e1630 = edge_515_542_phi_1093_;
            let _e1632 = edge_515_542_phi_1094_;
            phi_1092_ = _e1628;
            phi_1093_ = _e1630;
            phi_1094_ = _e1632;
        } else {
            if (_e1509 == 1u) {
                edge_520_542_phi_1092_ = _e71;
                edge_520_542_phi_1093_ = _e69;
                edge_520_542_phi_1094_ = _e67;
                let _e1616 = edge_520_542_phi_1092_;
                let _e1618 = edge_520_542_phi_1093_;
                let _e1620 = edge_520_542_phi_1094_;
                phi_1092_ = _e1616;
                phi_1093_ = _e1618;
                phi_1094_ = _e1620;
            } else {
                if (_e1509 == 2u) {
                    edge_523_542_phi_1092_ = _e77;
                    edge_523_542_phi_1093_ = _e75;
                    edge_523_542_phi_1094_ = _e73;
                    let _e1604 = edge_523_542_phi_1092_;
                    let _e1606 = edge_523_542_phi_1093_;
                    let _e1608 = edge_523_542_phi_1094_;
                    phi_1092_ = _e1604;
                    phi_1093_ = _e1606;
                    phi_1094_ = _e1608;
                } else {
                    if (_e1509 == 3u) {
                        edge_526_542_phi_1092_ = _e83;
                        edge_526_542_phi_1093_ = _e81;
                        edge_526_542_phi_1094_ = _e79;
                        let _e1592 = edge_526_542_phi_1092_;
                        let _e1594 = edge_526_542_phi_1093_;
                        let _e1596 = edge_526_542_phi_1094_;
                        phi_1092_ = _e1592;
                        phi_1093_ = _e1594;
                        phi_1094_ = _e1596;
                    } else {
                        if (_e1509 == 4u) {
                            edge_529_542_phi_1092_ = _e89;
                            edge_529_542_phi_1093_ = _e87;
                            edge_529_542_phi_1094_ = _e85;
                            let _e1580 = edge_529_542_phi_1092_;
                            let _e1582 = edge_529_542_phi_1093_;
                            let _e1584 = edge_529_542_phi_1094_;
                            phi_1092_ = _e1580;
                            phi_1093_ = _e1582;
                            phi_1094_ = _e1584;
                        } else {
                            if (_e1509 == 5u) {
                                edge_532_542_phi_1092_ = _e95;
                                edge_532_542_phi_1093_ = _e93;
                                edge_532_542_phi_1094_ = _e91;
                                let _e1568 = edge_532_542_phi_1092_;
                                let _e1570 = edge_532_542_phi_1093_;
                                let _e1572 = edge_532_542_phi_1094_;
                                phi_1092_ = _e1568;
                                phi_1093_ = _e1570;
                                phi_1094_ = _e1572;
                            } else {
                                if (_e1509 == 6u) {
                                    edge_535_542_phi_1092_ = _e101;
                                    edge_535_542_phi_1093_ = _e99;
                                    edge_535_542_phi_1094_ = _e97;
                                    let _e1556 = edge_535_542_phi_1092_;
                                    let _e1558 = edge_535_542_phi_1093_;
                                    let _e1560 = edge_535_542_phi_1094_;
                                    phi_1092_ = _e1556;
                                    phi_1093_ = _e1558;
                                    phi_1094_ = _e1560;
                                } else {
                                    if (_e1509 == 7u) {
                                        edge_538_542_phi_1092_ = _e107;
                                        edge_538_542_phi_1093_ = _e105;
                                        edge_538_542_phi_1094_ = _e103;
                                        let _e1532 = edge_538_542_phi_1092_;
                                        let _e1534 = edge_538_542_phi_1093_;
                                        let _e1536 = edge_538_542_phi_1094_;
                                        phi_1092_ = _e1532;
                                        phi_1093_ = _e1534;
                                        phi_1094_ = _e1536;
                                    } else {
                                        edge_541_542_phi_1092_ = _e113;
                                        edge_541_542_phi_1093_ = _e111;
                                        edge_541_542_phi_1094_ = _e109;
                                        let _e1544 = edge_541_542_phi_1092_;
                                        let _e1546 = edge_541_542_phi_1093_;
                                        let _e1548 = edge_541_542_phi_1094_;
                                        phi_1092_ = _e1544;
                                        phi_1093_ = _e1546;
                                        phi_1094_ = _e1548;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let _e1637 = phi_1092_;
        let _e1639 = phi_1093_;
        let _e1641 = phi_1094_;
        let _e1642 = (_e1641 - _e13);
        let _e1643 = (_e1639 - _e15);
        let _e1644 = (_e1637 - _e17);
        let _e1646 = (_e5 + 1.5707964f);
        let _e1654 = (_e1646 - (6.2831855f * floor(((_e1646 / 6.2831855f) + 0.5f))));
        let _e1664 = ((1.2732395f * _e1654) - ((0.40528473f * _e1654) * bitcast<f32>((bitcast<u32>(_e1654) & 2147483647u))));
        let _e1673 = ((0.225f * ((_e1664 * bitcast<f32>((bitcast<u32>(_e1664) & 2147483647u))) - _e1664)) + _e1664);
        let _e1681 = (_e5 - (6.2831855f * floor(((_e5 / 6.2831855f) + 0.5f))));
        let _e1691 = ((1.2732395f * _e1681) - ((0.40528473f * _e1681) * bitcast<f32>((bitcast<u32>(_e1681) & 2147483647u))));
        let _e1700 = ((0.225f * ((_e1691 * bitcast<f32>((bitcast<u32>(_e1691) & 2147483647u))) - _e1691)) + _e1691);
        let _e1702 = (_e7 + 1.5707964f);
        let _e1710 = (_e1702 - (6.2831855f * floor(((_e1702 / 6.2831855f) + 0.5f))));
        let _e1720 = ((1.2732395f * _e1710) - ((0.40528473f * _e1710) * bitcast<f32>((bitcast<u32>(_e1710) & 2147483647u))));
        let _e1729 = ((0.225f * ((_e1720 * bitcast<f32>((bitcast<u32>(_e1720) & 2147483647u))) - _e1720)) + _e1720);
        let _e1737 = (_e7 - (6.2831855f * floor(((_e7 / 6.2831855f) + 0.5f))));
        let _e1747 = ((1.2732395f * _e1737) - ((0.40528473f * _e1737) * bitcast<f32>((bitcast<u32>(_e1737) & 2147483647u))));
        let _e1756 = ((0.225f * ((_e1747 * bitcast<f32>((bitcast<u32>(_e1747) & 2147483647u))) - _e1747)) + _e1747);
        let _e1762 = ((_e1673 * _e1644) - (_e1700 * _e1642));
        let _e1769 = (((_e1756 * _e1643) + (_e1729 * _e1762)) + _e9);
        if (_e1511 == 1u) {
            edge_542_650_phi_1181_ = 0.018f;
            let _e1785 = edge_542_650_phi_1181_;
            phi_1181_ = _e1785;
        } else {
            edge_649_650_phi_1181_ = -0.018f;
            let _e1790 = edge_649_650_phi_1181_;
            phi_1181_ = _e1790;
        }
        let _e1793 = phi_1181_;
        if (_e1511 == 2u) {
            edge_650_653_phi_1183_ = 0.018f;
            let _e1799 = edge_650_653_phi_1183_;
            phi_1183_ = _e1799;
        } else {
            edge_652_653_phi_1183_ = -0.018f;
            let _e1804 = edge_652_653_phi_1183_;
            phi_1183_ = _e1804;
        }
        let _e1807 = phi_1183_;
        if (_e1511 == 3u) {
            edge_653_656_phi_1185_ = 0.018f;
            let _e1813 = edge_653_656_phi_1185_;
            phi_1185_ = _e1813;
        } else {
            edge_655_656_phi_1185_ = _e1807;
            let _e1817 = edge_655_656_phi_1185_;
            phi_1185_ = _e1817;
        }
        let _e1820 = phi_1185_;
        if (_e1511 == 4u) {
            edge_656_659_phi_1187_ = 0.018f;
            let _e1826 = edge_656_659_phi_1187_;
            phi_1187_ = _e1826;
        } else {
            edge_658_659_phi_1187_ = _e1793;
            let _e1830 = edge_658_659_phi_1187_;
            phi_1187_ = _e1830;
        }
        let _e1833 = phi_1187_;
        if (_e1511 == 5u) {
            edge_659_662_phi_1189_ = 0.018f;
            edge_659_662_phi_1190_ = 0.018f;
            let _e1841 = edge_659_662_phi_1189_;
            let _e1843 = edge_659_662_phi_1190_;
            phi_1189_ = _e1841;
            phi_1190_ = _e1843;
        } else {
            edge_661_662_phi_1189_ = _e1820;
            edge_661_662_phi_1190_ = _e1833;
            let _e1849 = edge_661_662_phi_1189_;
            let _e1851 = edge_661_662_phi_1190_;
            phi_1189_ = _e1849;
            phi_1190_ = _e1851;
        }
        let _e1855 = phi_1189_;
        let _e1857 = phi_1190_;
        edge_662_670_phi_1200_ = 1f;
        edge_662_670_phi_1201_ = 0f;
        edge_662_670_phi_1202_ = 0f;
        edge_662_670_phi_1203_ = (f32(bitcast<i32>(_e1509)) / 8f);
        edge_662_670_phi_1204_ = 1f;
        edge_662_670_phi_1205_ = 0f;
        edge_662_670_phi_1206_ = 0f;
        edge_662_670_phi_1207_ = _e1769;
        edge_662_670_phi_1208_ = ((((_e1769 * 64f) - 16f) / 63.75f) - (0.001f * _e1769));
        edge_662_670_phi_1209_ = ((((_e1729 * _e1643) - (_e1756 * _e1762)) * 1.6f) + (_e1855 * _e1769));
        edge_662_670_phi_1210_ = ((((_e1673 * _e1642) + (_e1700 * _e1644)) * 1.6f) + (_e1857 * _e1769));
        let _e1887 = edge_662_670_phi_1200_;
        let _e1889 = edge_662_670_phi_1201_;
        let _e1891 = edge_662_670_phi_1202_;
        let _e1893 = edge_662_670_phi_1203_;
        let _e1895 = edge_662_670_phi_1204_;
        let _e1897 = edge_662_670_phi_1205_;
        let _e1899 = edge_662_670_phi_1206_;
        let _e1901 = edge_662_670_phi_1207_;
        let _e1903 = edge_662_670_phi_1208_;
        let _e1905 = edge_662_670_phi_1209_;
        let _e1907 = edge_662_670_phi_1210_;
        phi_1200_ = _e1887;
        phi_1201_ = _e1889;
        phi_1202_ = _e1891;
        phi_1203_ = _e1893;
        phi_1204_ = _e1895;
        phi_1205_ = _e1897;
        phi_1206_ = _e1899;
        phi_1207_ = _e1901;
        phi_1208_ = _e1903;
        phi_1209_ = _e1905;
        phi_1210_ = _e1907;
    }
    let _e1920 = phi_1200_;
    let _e1922 = phi_1201_;
    let _e1924 = phi_1202_;
    let _e1926 = phi_1203_;
    let _e1928 = phi_1204_;
    let _e1930 = phi_1205_;
    let _e1932 = phi_1206_;
    let _e1934 = phi_1207_;
    let _e1936 = phi_1208_;
    let _e1938 = phi_1209_;
    let _e1940 = phi_1210_;
    return RasterVertexOutput(vec4<f32>(_e1940, _e1938, _e1936, _e1934), _e1932, _e1930, _e1928, _e1926, _e1924, _e1922, _e1920);
}

@fragment
fn surface_fragment(@location(0) v0_: f32, @location(1) v1_: f32, @location(2) v2_: f32, @location(3) v3_: f32, @location(4) v4_: f32, @location(5) v5_: f32, @location(6) v6_: f32) -> @location(0) vec4<f32> {
    var structured_result_1: u32;
    var structured_did_return_1: bool = false;
    var phi_343_: u32;
    var phi_326_: f32;
    var phi_327_: f32;
    var phi_328_: f32;
    var edge_53_289_phi_326_: f32;
    var edge_53_289_phi_327_: f32;
    var edge_53_289_phi_328_: f32;
    var edge_288_289_phi_326_: f32;
    var edge_288_289_phi_327_: f32;
    var edge_288_289_phi_328_: f32;
    var edge_11_2_phi_343_: u32;
    var edge_289_2_phi_343_: u32;

    let _e11 = state.p8_;
    let _e13 = state.p9_;
    let _e25 = state.p15_;
    if (0.5f < v6_) {
        let _e127 = bitcast<u32>(v3_);
        let _e128 = bitcast<u32>(0f);
        let _e152 = bitcast<u32>(bitcast<f32>(select(select(_e128, _e127, ((_e127 ^ ((0u - (_e127 >> 31u)) | 2147483648u)) > (_e128 ^ ((0u - (_e128 >> 31u)) | 2147483648u)))), 2143289344u, (((_e127 & 2147483647u) > 2139095040u) || ((_e128 & 2147483647u) > 2139095040u)))));
        let _e153 = bitcast<u32>(1f);
        let _e176 = bitcast<f32>(select(select(_e153, _e152, ((_e152 ^ ((0u - (_e152 >> 31u)) | 2147483648u)) < (_e153 ^ ((0u - (_e153 >> 31u)) | 2147483648u)))), 2143289344u, (((_e152 & 2147483647u) > 2139095040u) || ((_e153 & 2147483647u) > 2139095040u))));
        let _e190 = bitcast<u32>(1f);
        let _e191 = bitcast<u32>(0f);
        let _e215 = bitcast<u32>(bitcast<f32>(select(select(_e191, _e190, ((_e190 ^ ((0u - (_e190 >> 31u)) | 2147483648u)) > (_e191 ^ ((0u - (_e191 >> 31u)) | 2147483648u)))), 2143289344u, (((_e190 & 2147483647u) > 2139095040u) || ((_e191 & 2147483647u) > 2139095040u)))));
        let _e216 = bitcast<u32>(1f);
        let _e241 = (bitcast<f32>(select(select(_e216, _e215, ((_e215 ^ ((0u - (_e215 >> 31u)) | 2147483648u)) < (_e216 ^ ((0u - (_e216 >> 31u)) | 2147483648u)))), 2143289344u, (((_e215 & 2147483647u) > 2139095040u) || ((_e216 & 2147483647u) > 2139095040u)))) * 255f);
        let _e257 = bitcast<u32>((0.28f + (0.65f * _e176)));
        let _e258 = bitcast<u32>(0f);
        let _e282 = bitcast<u32>(bitcast<f32>(select(select(_e258, _e257, ((_e257 ^ ((0u - (_e257 >> 31u)) | 2147483648u)) > (_e258 ^ ((0u - (_e258 >> 31u)) | 2147483648u)))), 2143289344u, (((_e257 & 2147483647u) > 2139095040u) || ((_e258 & 2147483647u) > 2139095040u)))));
        let _e283 = bitcast<u32>(1f);
        let _e308 = (bitcast<f32>(select(select(_e283, _e282, ((_e282 ^ ((0u - (_e282 >> 31u)) | 2147483648u)) < (_e283 ^ ((0u - (_e283 >> 31u)) | 2147483648u)))), 2143289344u, (((_e282 & 2147483647u) > 2139095040u) || ((_e283 & 2147483647u) > 2139095040u)))) * 255f);
        let _e324 = bitcast<u32>((0.12f + (0.78f * (1f - _e176))));
        let _e325 = bitcast<u32>(0f);
        let _e349 = bitcast<u32>(bitcast<f32>(select(select(_e325, _e324, ((_e324 ^ ((0u - (_e324 >> 31u)) | 2147483648u)) > (_e325 ^ ((0u - (_e325 >> 31u)) | 2147483648u)))), 2143289344u, (((_e324 & 2147483647u) > 2139095040u) || ((_e325 & 2147483647u) > 2139095040u)))));
        let _e350 = bitcast<u32>(1f);
        let _e375 = (bitcast<f32>(select(select(_e350, _e349, ((_e349 ^ ((0u - (_e349 >> 31u)) | 2147483648u)) < (_e350 ^ ((0u - (_e350 >> 31u)) | 2147483648u)))), 2143289344u, (((_e349 & 2147483647u) > 2139095040u) || ((_e350 & 2147483647u) > 2139095040u)))) * 255f);
        edge_11_2_phi_343_ = (((select(0u, select(select(bitcast<u32>(i32(_e241)), 2147483648u, (_e241 <= -2147483600f)), 2147483647u, (_e241 >= 2147483600f)), (_e241 == _e241)) + (select(0u, select(select(bitcast<u32>(i32(_e308)), 2147483648u, (_e308 <= -2147483600f)), 2147483647u, (_e308 >= 2147483600f)), (_e308 == _e308)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e375)), 2147483648u, (_e375 <= -2147483600f)), 2147483647u, (_e375 >= 2147483600f)), (_e375 == _e375)) << 16u)) + 4278190080u);
        let _e1126 = edge_11_2_phi_343_;
        phi_343_ = _e1126;
    } else {
        let _e398 = (_e11 + 1.5707964f);
        let _e406 = (_e398 - (6.2831855f * floor(((_e398 / 6.2831855f) + 0.5f))));
        let _e416 = ((1.2732395f * _e406) - ((0.40528473f * _e406) * bitcast<f32>((bitcast<u32>(_e406) & 2147483647u))));
        let _e425 = ((0.225f * ((_e416 * bitcast<f32>((bitcast<u32>(_e416) & 2147483647u))) - _e416)) + _e416);
        let _e433 = (_e11 - (6.2831855f * floor(((_e11 / 6.2831855f) + 0.5f))));
        let _e443 = ((1.2732395f * _e433) - ((0.40528473f * _e433) * bitcast<f32>((bitcast<u32>(_e433) & 2147483647u))));
        let _e452 = ((0.225f * ((_e443 * bitcast<f32>((bitcast<u32>(_e443) & 2147483647u))) - _e443)) + _e443);
        let _e454 = (_e13 + 1.5707964f);
        let _e462 = (_e454 - (6.2831855f * floor(((_e454 / 6.2831855f) + 0.5f))));
        let _e472 = ((1.2732395f * _e462) - ((0.40528473f * _e462) * bitcast<f32>((bitcast<u32>(_e462) & 2147483647u))));
        let _e481 = ((0.225f * ((_e472 * bitcast<f32>((bitcast<u32>(_e472) & 2147483647u))) - _e472)) + _e472);
        let _e489 = (_e13 - (6.2831855f * floor(((_e13 / 6.2831855f) + 0.5f))));
        let _e499 = ((1.2732395f * _e489) - ((0.40528473f * _e489) * bitcast<f32>((bitcast<u32>(_e489) & 2147483647u))));
        let _e508 = ((0.225f * ((_e499 * bitcast<f32>((bitcast<u32>(_e499) & 2147483647u))) - _e499)) + _e499);
        let _e511 = (0f / 1.6f);
        let _e515 = ((_e481 * _e511) + (_e508 * 1f));
        let _e521 = ((0f - (_e508 * _e511)) + (_e481 * 1f));
        let _e524 = ((_e425 * _e511) - (_e452 * _e521));
        let _e527 = ((_e452 * _e511) + (_e425 * _e521));
        let _e535 = bitcast<u32>(sqrt((((_e524 * _e524) + (_e515 * _e515)) + (_e527 * _e527))));
        let _e536 = bitcast<u32>(0.0000001f);
        let _e561 = (1f / bitcast<f32>(select(select(_e536, _e535, ((_e535 ^ ((0u - (_e535 >> 31u)) | 2147483648u)) > (_e536 ^ ((0u - (_e536 >> 31u)) | 2147483648u)))), 2143289344u, (((_e535 & 2147483647u) > 2139095040u) || ((_e536 & 2147483647u) > 2139095040u)))));
        let _e562 = (_e524 * _e561);
        let _e563 = (_e515 * _e561);
        let _e564 = (_e527 * _e561);
        let _e572 = bitcast<u32>(sqrt((((v0_ * v0_) + (v1_ * v1_)) + (v2_ * v2_))));
        let _e573 = bitcast<u32>(0.0000001f);
        let _e598 = (1f / bitcast<f32>(select(select(_e573, _e572, ((_e572 ^ ((0u - (_e572 >> 31u)) | 2147483648u)) > (_e573 ^ ((0u - (_e573 >> 31u)) | 2147483648u)))), 2143289344u, (((_e572 & 2147483647u) > 2139095040u) || ((_e573 & 2147483647u) > 2139095040u)))));
        let _e599 = (v0_ * _e598);
        let _e600 = (v1_ * _e598);
        let _e601 = (v2_ * _e598);
        let _e609 = bitcast<u32>(sqrt((((_e562 * _e562) + (_e563 * _e563)) + (_e564 * _e564))));
        let _e610 = bitcast<u32>(0.0000001f);
        let _e635 = (1f / bitcast<f32>(select(select(_e610, _e609, ((_e609 ^ ((0u - (_e609 >> 31u)) | 2147483648u)) > (_e610 ^ ((0u - (_e610 >> 31u)) | 2147483648u)))), 2143289344u, (((_e609 & 2147483647u) > 2139095040u) || ((_e610 & 2147483647u) > 2139095040u)))));
        let _e654 = (0.18f + (0.82f * bitcast<f32>((bitcast<u32>((((_e599 * 0.37f) + (_e600 * 0.82f)) + (_e601 * 0.44f))) & 2147483647u))));
        let _e665 = (1f - bitcast<f32>((bitcast<u32>((((_e599 * (_e562 * _e635)) + (_e600 * (_e563 * _e635))) + (_e601 * (_e564 * _e635)))) & 2147483647u)));
        let _e666 = (_e665 * _e665);
        let _e667 = (_e666 * _e666);
        let _e682 = bitcast<u32>(_e25);
        let _e683 = bitcast<u32>(0f);
        let _e707 = bitcast<u32>(bitcast<f32>(select(select(_e683, _e682, ((_e682 ^ ((0u - (_e682 >> 31u)) | 2147483648u)) > (_e683 ^ ((0u - (_e683 >> 31u)) | 2147483648u)))), 2143289344u, (((_e682 & 2147483647u) > 2139095040u) || ((_e683 & 2147483647u) > 2139095040u)))));
        let _e708 = bitcast<u32>(1f);
        let _e734 = bitcast<u32>(v4_);
        let _e735 = bitcast<u32>(0f);
        let _e759 = bitcast<u32>(bitcast<f32>(select(select(_e735, _e734, ((_e734 ^ ((0u - (_e734 >> 31u)) | 2147483648u)) > (_e735 ^ ((0u - (_e735 >> 31u)) | 2147483648u)))), 2143289344u, (((_e734 & 2147483647u) > 2139095040u) || ((_e735 & 2147483647u) > 2139095040u)))));
        let _e760 = bitcast<u32>(1f);
        let _e783 = bitcast<f32>(select(select(_e760, _e759, ((_e759 ^ ((0u - (_e759 >> 31u)) | 2147483648u)) < (_e760 ^ ((0u - (_e760 >> 31u)) | 2147483648u)))), 2143289344u, (((_e759 & 2147483647u) > 2139095040u) || ((_e760 & 2147483647u) > 2139095040u))));
        let _e786 = bitcast<u32>(v3_);
        let _e787 = bitcast<u32>(0f);
        let _e811 = bitcast<u32>(bitcast<f32>(select(select(_e787, _e786, ((_e786 ^ ((0u - (_e786 >> 31u)) | 2147483648u)) > (_e787 ^ ((0u - (_e787 >> 31u)) | 2147483648u)))), 2143289344u, (((_e786 & 2147483647u) > 2139095040u) || ((_e787 & 2147483647u) > 2139095040u)))));
        let _e812 = bitcast<u32>(1f);
        let _e835 = bitcast<f32>(select(select(_e812, _e811, ((_e811 ^ ((0u - (_e811 >> 31u)) | 2147483648u)) < (_e812 ^ ((0u - (_e812 >> 31u)) | 2147483648u)))), 2143289344u, (((_e811 & 2147483647u) > 2139095040u) || ((_e812 & 2147483647u) > 2139095040u))));
        if (0.5f < v5_) {
            edge_53_289_phi_326_ = 1f;
            edge_53_289_phi_327_ = 0f;
            edge_53_289_phi_328_ = 1f;
            let _e889 = edge_53_289_phi_326_;
            let _e891 = edge_53_289_phi_327_;
            let _e893 = edge_53_289_phi_328_;
            phi_326_ = _e889;
            phi_327_ = _e891;
            phi_328_ = _e893;
        } else {
            edge_288_289_phi_326_ = (((_e654 * ((0.18f + (0.66f * (0.5f + (0.5f * _e601)))) + (0.2f * (1f - _e783)))) + (_e835 * 0.62f)) + (_e667 * 0.88f));
            edge_288_289_phi_327_ = (((_e654 * ((0.12f + (0.62f * (0.5f + (0.5f * _e600)))) + (0.12f * _e783))) + (_e835 * 0.2f)) + (_e667 * 0.42f));
            edge_288_289_phi_328_ = (((_e654 * ((0.1f + (0.6f * (0.5f + (0.5f * _e599)))) + (0.2f * bitcast<f32>(select(select(_e708, _e707, ((_e707 ^ ((0u - (_e707 >> 31u)) | 2147483648u)) < (_e708 ^ ((0u - (_e708 >> 31u)) | 2147483648u)))), 2143289344u, (((_e707 & 2147483647u) > 2139095040u) || ((_e708 & 2147483647u) > 2139095040u))))))) + (_e835 * 0.48f)) + (_e667 * 0.7f));
            let _e901 = edge_288_289_phi_326_;
            let _e903 = edge_288_289_phi_327_;
            let _e905 = edge_288_289_phi_328_;
            phi_326_ = _e901;
            phi_327_ = _e903;
            phi_328_ = _e905;
        }
        let _e910 = phi_326_;
        let _e912 = phi_327_;
        let _e914 = phi_328_;
        let _e917 = bitcast<u32>(_e914);
        let _e918 = bitcast<u32>(0f);
        let _e942 = bitcast<u32>(bitcast<f32>(select(select(_e918, _e917, ((_e917 ^ ((0u - (_e917 >> 31u)) | 2147483648u)) > (_e918 ^ ((0u - (_e918 >> 31u)) | 2147483648u)))), 2143289344u, (((_e917 & 2147483647u) > 2139095040u) || ((_e918 & 2147483647u) > 2139095040u)))));
        let _e943 = bitcast<u32>(1f);
        let _e968 = (bitcast<f32>(select(select(_e943, _e942, ((_e942 ^ ((0u - (_e942 >> 31u)) | 2147483648u)) < (_e943 ^ ((0u - (_e943 >> 31u)) | 2147483648u)))), 2143289344u, (((_e942 & 2147483647u) > 2139095040u) || ((_e943 & 2147483647u) > 2139095040u)))) * 255f);
        let _e984 = bitcast<u32>(_e912);
        let _e985 = bitcast<u32>(0f);
        let _e1009 = bitcast<u32>(bitcast<f32>(select(select(_e985, _e984, ((_e984 ^ ((0u - (_e984 >> 31u)) | 2147483648u)) > (_e985 ^ ((0u - (_e985 >> 31u)) | 2147483648u)))), 2143289344u, (((_e984 & 2147483647u) > 2139095040u) || ((_e985 & 2147483647u) > 2139095040u)))));
        let _e1010 = bitcast<u32>(1f);
        let _e1035 = (bitcast<f32>(select(select(_e1010, _e1009, ((_e1009 ^ ((0u - (_e1009 >> 31u)) | 2147483648u)) < (_e1010 ^ ((0u - (_e1010 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1009 & 2147483647u) > 2139095040u) || ((_e1010 & 2147483647u) > 2139095040u)))) * 255f);
        let _e1051 = bitcast<u32>(_e910);
        let _e1052 = bitcast<u32>(0f);
        let _e1076 = bitcast<u32>(bitcast<f32>(select(select(_e1052, _e1051, ((_e1051 ^ ((0u - (_e1051 >> 31u)) | 2147483648u)) > (_e1052 ^ ((0u - (_e1052 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1051 & 2147483647u) > 2139095040u) || ((_e1052 & 2147483647u) > 2139095040u)))));
        let _e1077 = bitcast<u32>(1f);
        let _e1102 = (bitcast<f32>(select(select(_e1077, _e1076, ((_e1076 ^ ((0u - (_e1076 >> 31u)) | 2147483648u)) < (_e1077 ^ ((0u - (_e1077 >> 31u)) | 2147483648u)))), 2143289344u, (((_e1076 & 2147483647u) > 2139095040u) || ((_e1077 & 2147483647u) > 2139095040u)))) * 255f);
        edge_289_2_phi_343_ = (((select(0u, select(select(bitcast<u32>(i32(_e968)), 2147483648u, (_e968 <= -2147483600f)), 2147483647u, (_e968 >= 2147483600f)), (_e968 == _e968)) + (select(0u, select(select(bitcast<u32>(i32(_e1035)), 2147483648u, (_e1035 <= -2147483600f)), 2147483647u, (_e1035 >= 2147483600f)), (_e1035 == _e1035)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e1102)), 2147483648u, (_e1102 <= -2147483600f)), 2147483647u, (_e1102 >= 2147483600f)), (_e1102 == _e1102)) << 16u)) + 4278190080u);
        let _e1130 = edge_289_2_phi_343_;
        phi_343_ = _e1130;
    }
    let _e1133 = phi_343_;
    return unpack4x8unorm(_e1133);
}
