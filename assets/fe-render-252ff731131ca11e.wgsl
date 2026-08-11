struct Input {
    p2_: f32,
    p3_: f32,
    p4_: f32,
    p5_: f32,
    p6_: f32,
    p7_: f32,
    p8_: f32,
    p9_: f32,
    p10_: f32,
}

@group(0) @binding(1)
var<storage> input: Input;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>((f32(((vi & 1u) << 2u)) - 1f), (f32(((vi & 2u) << 1u)) - 1f), 0f, 1f);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var structured_result: u32;
    var structured_did_return: bool = false;
    var phi_58_: f32;
    var phi_57_: f32;
    var phi_56_: f32;
    var phi_55_: f32;
    var phi_54_: f32;
    var phi_53_: f32;
    var phi_52_: f32;
    var phi_552_: f32;
    var phi_554_: f32;
    var phi_556_: f32;
    var phi_558_: f32;
    var phi_560_: f32;
    var phi_562_: f32;
    var phi_564_: f32;
    var phi_566_: f32;
    var phi_568_: f32;
    var phi_570_: f32;
    var phi_572_: f32;
    var phi_574_: f32;
    var phi_576_: f32;
    var phi_578_: f32;
    var phi_580_: f32;
    var phi_582_: f32;
    var phi_584_: u32;
    var edge_0_414_phi_552_: f32;
    var edge_0_414_phi_554_: f32;
    var edge_0_414_phi_556_: f32;
    var edge_0_414_phi_558_: f32;
    var edge_0_414_phi_560_: f32;
    var edge_0_414_phi_562_: f32;
    var edge_0_414_phi_564_: f32;
    var edge_0_414_phi_566_: f32;
    var edge_0_414_phi_568_: f32;
    var edge_0_414_phi_570_: f32;
    var edge_0_414_phi_572_: f32;
    var edge_0_414_phi_574_: f32;
    var edge_0_414_phi_576_: f32;
    var edge_0_414_phi_578_: f32;
    var edge_0_414_phi_580_: f32;
    var edge_0_414_phi_582_: f32;
    var edge_0_414_phi_584_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_586_: bool;
    var phi_771_: f32;
    var phi_773_: f32;
    var phi_775_: f32;
    var phi_777_: f32;
    var phi_779_: f32;
    var phi_781_: f32;
    var phi_783_: f32;
    var phi_785_: f32;
    var phi_787_: f32;
    var phi_789_: f32;
    var phi_791_: f32;
    var phi_793_: f32;
    var phi_795_: f32;
    var phi_797_: f32;
    var phi_799_: f32;
    var phi_801_: f32;
    var phi_803_: u32;
    var phi_588_: f32;
    var phi_590_: u32;
    var edge_414_417_phi_588_: f32;
    var edge_414_417_phi_590_: u32;
    var loop_result_1: u32;
    var loop_did_return_1: bool = false;
    var loop_header_carry_592_: bool;
    var phi_622_: u32;
    var edge_428_423_phi_622_: u32;
    var edge_431_423_phi_622_: u32;
    var edge_424_423_phi_622_: u32;
    var phi_606_: u32;
    var edge_436_439_phi_606_: u32;
    var edge_438_439_phi_606_: u32;
    var edge_439_423_phi_622_: u32;
    var edge_443_423_phi_622_: u32;
    var edge_425_423_phi_622_: u32;
    var phi_657_: f32;
    var phi_652_: f32;
    var edge_560_593_phi_652_: f32;
    var edge_562_593_phi_652_: f32;
    var edge_565_593_phi_652_: f32;
    var edge_569_593_phi_652_: f32;
    var edge_579_593_phi_652_: f32;
    var edge_581_593_phi_652_: f32;
    var edge_584_593_phi_652_: f32;
    var edge_588_593_phi_652_: f32;
    var edge_597_596_phi_657_: f32;
    var edge_598_596_phi_657_: f32;
    var edge_423_596_phi_657_: f32;
    var edge_596_417_phi_588_: f32;
    var edge_596_417_phi_590_: u32;
    var phi_753_: f32;
    var phi_754_: f32;
    var phi_755_: f32;
    var phi_756_: f32;
    var phi_757_: f32;
    var phi_758_: f32;
    var phi_759_: f32;
    var phi_760_: f32;
    var phi_761_: f32;
    var phi_762_: f32;
    var phi_763_: f32;
    var phi_764_: f32;
    var phi_765_: f32;
    var phi_766_: f32;
    var phi_767_: f32;
    var phi_768_: f32;
    var edge_472_550_phi_753_: f32;
    var edge_472_550_phi_754_: f32;
    var edge_472_550_phi_755_: f32;
    var edge_472_550_phi_756_: f32;
    var edge_472_550_phi_757_: f32;
    var edge_472_550_phi_758_: f32;
    var edge_472_550_phi_759_: f32;
    var edge_472_550_phi_760_: f32;
    var edge_472_550_phi_761_: f32;
    var edge_472_550_phi_762_: f32;
    var edge_472_550_phi_763_: f32;
    var edge_472_550_phi_764_: f32;
    var edge_472_550_phi_765_: f32;
    var edge_472_550_phi_766_: f32;
    var edge_472_550_phi_767_: f32;
    var edge_472_550_phi_768_: f32;
    var edge_474_550_phi_753_: f32;
    var edge_474_550_phi_754_: f32;
    var edge_474_550_phi_755_: f32;
    var edge_474_550_phi_756_: f32;
    var edge_474_550_phi_757_: f32;
    var edge_474_550_phi_758_: f32;
    var edge_474_550_phi_759_: f32;
    var edge_474_550_phi_760_: f32;
    var edge_474_550_phi_761_: f32;
    var edge_474_550_phi_762_: f32;
    var edge_474_550_phi_763_: f32;
    var edge_474_550_phi_764_: f32;
    var edge_474_550_phi_765_: f32;
    var edge_474_550_phi_766_: f32;
    var edge_474_550_phi_767_: f32;
    var edge_474_550_phi_768_: f32;
    var edge_477_550_phi_753_: f32;
    var edge_477_550_phi_754_: f32;
    var edge_477_550_phi_755_: f32;
    var edge_477_550_phi_756_: f32;
    var edge_477_550_phi_757_: f32;
    var edge_477_550_phi_758_: f32;
    var edge_477_550_phi_759_: f32;
    var edge_477_550_phi_760_: f32;
    var edge_477_550_phi_761_: f32;
    var edge_477_550_phi_762_: f32;
    var edge_477_550_phi_763_: f32;
    var edge_477_550_phi_764_: f32;
    var edge_477_550_phi_765_: f32;
    var edge_477_550_phi_766_: f32;
    var edge_477_550_phi_767_: f32;
    var edge_477_550_phi_768_: f32;
    var edge_481_550_phi_753_: f32;
    var edge_481_550_phi_754_: f32;
    var edge_481_550_phi_755_: f32;
    var edge_481_550_phi_756_: f32;
    var edge_481_550_phi_757_: f32;
    var edge_481_550_phi_758_: f32;
    var edge_481_550_phi_759_: f32;
    var edge_481_550_phi_760_: f32;
    var edge_481_550_phi_761_: f32;
    var edge_481_550_phi_762_: f32;
    var edge_481_550_phi_763_: f32;
    var edge_481_550_phi_764_: f32;
    var edge_481_550_phi_765_: f32;
    var edge_481_550_phi_766_: f32;
    var edge_481_550_phi_767_: f32;
    var edge_481_550_phi_768_: f32;
    var edge_491_550_phi_753_: f32;
    var edge_491_550_phi_754_: f32;
    var edge_491_550_phi_755_: f32;
    var edge_491_550_phi_756_: f32;
    var edge_491_550_phi_757_: f32;
    var edge_491_550_phi_758_: f32;
    var edge_491_550_phi_759_: f32;
    var edge_491_550_phi_760_: f32;
    var edge_491_550_phi_761_: f32;
    var edge_491_550_phi_762_: f32;
    var edge_491_550_phi_763_: f32;
    var edge_491_550_phi_764_: f32;
    var edge_491_550_phi_765_: f32;
    var edge_491_550_phi_766_: f32;
    var edge_491_550_phi_767_: f32;
    var edge_491_550_phi_768_: f32;
    var edge_493_550_phi_753_: f32;
    var edge_493_550_phi_754_: f32;
    var edge_493_550_phi_755_: f32;
    var edge_493_550_phi_756_: f32;
    var edge_493_550_phi_757_: f32;
    var edge_493_550_phi_758_: f32;
    var edge_493_550_phi_759_: f32;
    var edge_493_550_phi_760_: f32;
    var edge_493_550_phi_761_: f32;
    var edge_493_550_phi_762_: f32;
    var edge_493_550_phi_763_: f32;
    var edge_493_550_phi_764_: f32;
    var edge_493_550_phi_765_: f32;
    var edge_493_550_phi_766_: f32;
    var edge_493_550_phi_767_: f32;
    var edge_493_550_phi_768_: f32;
    var edge_496_550_phi_753_: f32;
    var edge_496_550_phi_754_: f32;
    var edge_496_550_phi_755_: f32;
    var edge_496_550_phi_756_: f32;
    var edge_496_550_phi_757_: f32;
    var edge_496_550_phi_758_: f32;
    var edge_496_550_phi_759_: f32;
    var edge_496_550_phi_760_: f32;
    var edge_496_550_phi_761_: f32;
    var edge_496_550_phi_762_: f32;
    var edge_496_550_phi_763_: f32;
    var edge_496_550_phi_764_: f32;
    var edge_496_550_phi_765_: f32;
    var edge_496_550_phi_766_: f32;
    var edge_496_550_phi_767_: f32;
    var edge_496_550_phi_768_: f32;
    var edge_500_550_phi_753_: f32;
    var edge_500_550_phi_754_: f32;
    var edge_500_550_phi_755_: f32;
    var edge_500_550_phi_756_: f32;
    var edge_500_550_phi_757_: f32;
    var edge_500_550_phi_758_: f32;
    var edge_500_550_phi_759_: f32;
    var edge_500_550_phi_760_: f32;
    var edge_500_550_phi_761_: f32;
    var edge_500_550_phi_762_: f32;
    var edge_500_550_phi_763_: f32;
    var edge_500_550_phi_764_: f32;
    var edge_500_550_phi_765_: f32;
    var edge_500_550_phi_766_: f32;
    var edge_500_550_phi_767_: f32;
    var edge_500_550_phi_768_: f32;
    var edge_515_550_phi_753_: f32;
    var edge_515_550_phi_754_: f32;
    var edge_515_550_phi_755_: f32;
    var edge_515_550_phi_756_: f32;
    var edge_515_550_phi_757_: f32;
    var edge_515_550_phi_758_: f32;
    var edge_515_550_phi_759_: f32;
    var edge_515_550_phi_760_: f32;
    var edge_515_550_phi_761_: f32;
    var edge_515_550_phi_762_: f32;
    var edge_515_550_phi_763_: f32;
    var edge_515_550_phi_764_: f32;
    var edge_515_550_phi_765_: f32;
    var edge_515_550_phi_766_: f32;
    var edge_515_550_phi_767_: f32;
    var edge_515_550_phi_768_: f32;
    var edge_517_550_phi_753_: f32;
    var edge_517_550_phi_754_: f32;
    var edge_517_550_phi_755_: f32;
    var edge_517_550_phi_756_: f32;
    var edge_517_550_phi_757_: f32;
    var edge_517_550_phi_758_: f32;
    var edge_517_550_phi_759_: f32;
    var edge_517_550_phi_760_: f32;
    var edge_517_550_phi_761_: f32;
    var edge_517_550_phi_762_: f32;
    var edge_517_550_phi_763_: f32;
    var edge_517_550_phi_764_: f32;
    var edge_517_550_phi_765_: f32;
    var edge_517_550_phi_766_: f32;
    var edge_517_550_phi_767_: f32;
    var edge_517_550_phi_768_: f32;
    var edge_520_550_phi_753_: f32;
    var edge_520_550_phi_754_: f32;
    var edge_520_550_phi_755_: f32;
    var edge_520_550_phi_756_: f32;
    var edge_520_550_phi_757_: f32;
    var edge_520_550_phi_758_: f32;
    var edge_520_550_phi_759_: f32;
    var edge_520_550_phi_760_: f32;
    var edge_520_550_phi_761_: f32;
    var edge_520_550_phi_762_: f32;
    var edge_520_550_phi_763_: f32;
    var edge_520_550_phi_764_: f32;
    var edge_520_550_phi_765_: f32;
    var edge_520_550_phi_766_: f32;
    var edge_520_550_phi_767_: f32;
    var edge_520_550_phi_768_: f32;
    var edge_524_550_phi_753_: f32;
    var edge_524_550_phi_754_: f32;
    var edge_524_550_phi_755_: f32;
    var edge_524_550_phi_756_: f32;
    var edge_524_550_phi_757_: f32;
    var edge_524_550_phi_758_: f32;
    var edge_524_550_phi_759_: f32;
    var edge_524_550_phi_760_: f32;
    var edge_524_550_phi_761_: f32;
    var edge_524_550_phi_762_: f32;
    var edge_524_550_phi_763_: f32;
    var edge_524_550_phi_764_: f32;
    var edge_524_550_phi_765_: f32;
    var edge_524_550_phi_766_: f32;
    var edge_524_550_phi_767_: f32;
    var edge_524_550_phi_768_: f32;
    var edge_534_550_phi_753_: f32;
    var edge_534_550_phi_754_: f32;
    var edge_534_550_phi_755_: f32;
    var edge_534_550_phi_756_: f32;
    var edge_534_550_phi_757_: f32;
    var edge_534_550_phi_758_: f32;
    var edge_534_550_phi_759_: f32;
    var edge_534_550_phi_760_: f32;
    var edge_534_550_phi_761_: f32;
    var edge_534_550_phi_762_: f32;
    var edge_534_550_phi_763_: f32;
    var edge_534_550_phi_764_: f32;
    var edge_534_550_phi_765_: f32;
    var edge_534_550_phi_766_: f32;
    var edge_534_550_phi_767_: f32;
    var edge_534_550_phi_768_: f32;
    var edge_536_550_phi_753_: f32;
    var edge_536_550_phi_754_: f32;
    var edge_536_550_phi_755_: f32;
    var edge_536_550_phi_756_: f32;
    var edge_536_550_phi_757_: f32;
    var edge_536_550_phi_758_: f32;
    var edge_536_550_phi_759_: f32;
    var edge_536_550_phi_760_: f32;
    var edge_536_550_phi_761_: f32;
    var edge_536_550_phi_762_: f32;
    var edge_536_550_phi_763_: f32;
    var edge_536_550_phi_764_: f32;
    var edge_536_550_phi_765_: f32;
    var edge_536_550_phi_766_: f32;
    var edge_536_550_phi_767_: f32;
    var edge_536_550_phi_768_: f32;
    var edge_539_550_phi_753_: f32;
    var edge_539_550_phi_754_: f32;
    var edge_539_550_phi_755_: f32;
    var edge_539_550_phi_756_: f32;
    var edge_539_550_phi_757_: f32;
    var edge_539_550_phi_758_: f32;
    var edge_539_550_phi_759_: f32;
    var edge_539_550_phi_760_: f32;
    var edge_539_550_phi_761_: f32;
    var edge_539_550_phi_762_: f32;
    var edge_539_550_phi_763_: f32;
    var edge_539_550_phi_764_: f32;
    var edge_539_550_phi_765_: f32;
    var edge_539_550_phi_766_: f32;
    var edge_539_550_phi_767_: f32;
    var edge_539_550_phi_768_: f32;
    var edge_543_550_phi_753_: f32;
    var edge_543_550_phi_754_: f32;
    var edge_543_550_phi_755_: f32;
    var edge_543_550_phi_756_: f32;
    var edge_543_550_phi_757_: f32;
    var edge_543_550_phi_758_: f32;
    var edge_543_550_phi_759_: f32;
    var edge_543_550_phi_760_: f32;
    var edge_543_550_phi_761_: f32;
    var edge_543_550_phi_762_: f32;
    var edge_543_550_phi_763_: f32;
    var edge_543_550_phi_764_: f32;
    var edge_543_550_phi_765_: f32;
    var edge_543_550_phi_766_: f32;
    var edge_543_550_phi_767_: f32;
    var edge_543_550_phi_768_: f32;
    var edge_550_414_phi_552_: f32;
    var edge_550_414_phi_554_: f32;
    var edge_550_414_phi_556_: f32;
    var edge_550_414_phi_558_: f32;
    var edge_550_414_phi_560_: f32;
    var edge_550_414_phi_562_: f32;
    var edge_550_414_phi_564_: f32;
    var edge_550_414_phi_566_: f32;
    var edge_550_414_phi_568_: f32;
    var edge_550_414_phi_570_: f32;
    var edge_550_414_phi_572_: f32;
    var edge_550_414_phi_574_: f32;
    var edge_550_414_phi_576_: f32;
    var edge_550_414_phi_578_: f32;
    var edge_550_414_phi_580_: f32;
    var edge_550_414_phi_582_: f32;
    var edge_550_414_phi_584_: u32;
    var edge_414_696_phi_771_: f32;
    var edge_414_696_phi_773_: f32;
    var edge_414_696_phi_775_: f32;
    var edge_414_696_phi_777_: f32;
    var edge_414_696_phi_779_: f32;
    var edge_414_696_phi_781_: f32;
    var edge_414_696_phi_783_: f32;
    var edge_414_696_phi_785_: f32;
    var edge_414_696_phi_787_: f32;
    var edge_414_696_phi_789_: f32;
    var edge_414_696_phi_791_: f32;
    var edge_414_696_phi_793_: f32;
    var edge_414_696_phi_795_: f32;
    var edge_414_696_phi_797_: f32;
    var edge_414_696_phi_799_: f32;
    var edge_414_696_phi_801_: f32;
    var edge_414_696_phi_803_: u32;
    var edge_414_696_phi_771_1: f32;
    var edge_414_696_phi_773_1: f32;
    var edge_414_696_phi_775_1: f32;
    var edge_414_696_phi_777_1: f32;
    var edge_414_696_phi_779_1: f32;
    var edge_414_696_phi_781_1: f32;
    var edge_414_696_phi_783_1: f32;
    var edge_414_696_phi_785_1: f32;
    var edge_414_696_phi_787_1: f32;
    var edge_414_696_phi_789_1: f32;
    var edge_414_696_phi_791_1: f32;
    var edge_414_696_phi_793_1: f32;
    var edge_414_696_phi_795_1: f32;
    var edge_414_696_phi_797_1: f32;
    var edge_414_696_phi_799_1: f32;
    var edge_414_696_phi_801_1: f32;
    var edge_414_696_phi_803_1: u32;
    var loop_result_2: u32;
    var loop_did_return_2: bool = false;
    var loop_header_carry_804_: bool;
    var phi_994_: f32;
    var phi_996_: f32;
    var phi_998_: f32;
    var phi_1000_: f32;
    var phi_1002_: f32;
    var phi_1004_: f32;
    var phi_1006_: f32;
    var phi_1008_: f32;
    var phi_1010_: u32;
    var phi_864_: f32;
    var edge_708_786_phi_864_: f32;
    var edge_710_786_phi_864_: f32;
    var edge_713_786_phi_864_: f32;
    var edge_717_786_phi_864_: f32;
    var edge_727_786_phi_864_: f32;
    var edge_729_786_phi_864_: f32;
    var edge_732_786_phi_864_: f32;
    var edge_736_786_phi_864_: f32;
    var edge_751_786_phi_864_: f32;
    var edge_753_786_phi_864_: f32;
    var edge_756_786_phi_864_: f32;
    var edge_760_786_phi_864_: f32;
    var edge_770_786_phi_864_: f32;
    var edge_772_786_phi_864_: f32;
    var edge_775_786_phi_864_: f32;
    var edge_779_786_phi_864_: f32;
    var phi_881_: f32;
    var edge_802_795_phi_881_: f32;
    var edge_803_795_phi_881_: f32;
    var phi_976_: f32;
    var phi_977_: f32;
    var phi_978_: f32;
    var phi_979_: f32;
    var phi_980_: f32;
    var phi_981_: f32;
    var phi_982_: f32;
    var phi_983_: f32;
    var phi_984_: f32;
    var phi_985_: f32;
    var phi_986_: f32;
    var phi_987_: f32;
    var phi_988_: f32;
    var phi_989_: f32;
    var phi_990_: f32;
    var phi_991_: f32;
    var edge_816_894_phi_976_: f32;
    var edge_816_894_phi_977_: f32;
    var edge_816_894_phi_978_: f32;
    var edge_816_894_phi_979_: f32;
    var edge_816_894_phi_980_: f32;
    var edge_816_894_phi_981_: f32;
    var edge_816_894_phi_982_: f32;
    var edge_816_894_phi_983_: f32;
    var edge_816_894_phi_984_: f32;
    var edge_816_894_phi_985_: f32;
    var edge_816_894_phi_986_: f32;
    var edge_816_894_phi_987_: f32;
    var edge_816_894_phi_988_: f32;
    var edge_816_894_phi_989_: f32;
    var edge_816_894_phi_990_: f32;
    var edge_816_894_phi_991_: f32;
    var edge_818_894_phi_976_: f32;
    var edge_818_894_phi_977_: f32;
    var edge_818_894_phi_978_: f32;
    var edge_818_894_phi_979_: f32;
    var edge_818_894_phi_980_: f32;
    var edge_818_894_phi_981_: f32;
    var edge_818_894_phi_982_: f32;
    var edge_818_894_phi_983_: f32;
    var edge_818_894_phi_984_: f32;
    var edge_818_894_phi_985_: f32;
    var edge_818_894_phi_986_: f32;
    var edge_818_894_phi_987_: f32;
    var edge_818_894_phi_988_: f32;
    var edge_818_894_phi_989_: f32;
    var edge_818_894_phi_990_: f32;
    var edge_818_894_phi_991_: f32;
    var edge_821_894_phi_976_: f32;
    var edge_821_894_phi_977_: f32;
    var edge_821_894_phi_978_: f32;
    var edge_821_894_phi_979_: f32;
    var edge_821_894_phi_980_: f32;
    var edge_821_894_phi_981_: f32;
    var edge_821_894_phi_982_: f32;
    var edge_821_894_phi_983_: f32;
    var edge_821_894_phi_984_: f32;
    var edge_821_894_phi_985_: f32;
    var edge_821_894_phi_986_: f32;
    var edge_821_894_phi_987_: f32;
    var edge_821_894_phi_988_: f32;
    var edge_821_894_phi_989_: f32;
    var edge_821_894_phi_990_: f32;
    var edge_821_894_phi_991_: f32;
    var edge_825_894_phi_976_: f32;
    var edge_825_894_phi_977_: f32;
    var edge_825_894_phi_978_: f32;
    var edge_825_894_phi_979_: f32;
    var edge_825_894_phi_980_: f32;
    var edge_825_894_phi_981_: f32;
    var edge_825_894_phi_982_: f32;
    var edge_825_894_phi_983_: f32;
    var edge_825_894_phi_984_: f32;
    var edge_825_894_phi_985_: f32;
    var edge_825_894_phi_986_: f32;
    var edge_825_894_phi_987_: f32;
    var edge_825_894_phi_988_: f32;
    var edge_825_894_phi_989_: f32;
    var edge_825_894_phi_990_: f32;
    var edge_825_894_phi_991_: f32;
    var edge_835_894_phi_976_: f32;
    var edge_835_894_phi_977_: f32;
    var edge_835_894_phi_978_: f32;
    var edge_835_894_phi_979_: f32;
    var edge_835_894_phi_980_: f32;
    var edge_835_894_phi_981_: f32;
    var edge_835_894_phi_982_: f32;
    var edge_835_894_phi_983_: f32;
    var edge_835_894_phi_984_: f32;
    var edge_835_894_phi_985_: f32;
    var edge_835_894_phi_986_: f32;
    var edge_835_894_phi_987_: f32;
    var edge_835_894_phi_988_: f32;
    var edge_835_894_phi_989_: f32;
    var edge_835_894_phi_990_: f32;
    var edge_835_894_phi_991_: f32;
    var edge_837_894_phi_976_: f32;
    var edge_837_894_phi_977_: f32;
    var edge_837_894_phi_978_: f32;
    var edge_837_894_phi_979_: f32;
    var edge_837_894_phi_980_: f32;
    var edge_837_894_phi_981_: f32;
    var edge_837_894_phi_982_: f32;
    var edge_837_894_phi_983_: f32;
    var edge_837_894_phi_984_: f32;
    var edge_837_894_phi_985_: f32;
    var edge_837_894_phi_986_: f32;
    var edge_837_894_phi_987_: f32;
    var edge_837_894_phi_988_: f32;
    var edge_837_894_phi_989_: f32;
    var edge_837_894_phi_990_: f32;
    var edge_837_894_phi_991_: f32;
    var edge_840_894_phi_976_: f32;
    var edge_840_894_phi_977_: f32;
    var edge_840_894_phi_978_: f32;
    var edge_840_894_phi_979_: f32;
    var edge_840_894_phi_980_: f32;
    var edge_840_894_phi_981_: f32;
    var edge_840_894_phi_982_: f32;
    var edge_840_894_phi_983_: f32;
    var edge_840_894_phi_984_: f32;
    var edge_840_894_phi_985_: f32;
    var edge_840_894_phi_986_: f32;
    var edge_840_894_phi_987_: f32;
    var edge_840_894_phi_988_: f32;
    var edge_840_894_phi_989_: f32;
    var edge_840_894_phi_990_: f32;
    var edge_840_894_phi_991_: f32;
    var edge_844_894_phi_976_: f32;
    var edge_844_894_phi_977_: f32;
    var edge_844_894_phi_978_: f32;
    var edge_844_894_phi_979_: f32;
    var edge_844_894_phi_980_: f32;
    var edge_844_894_phi_981_: f32;
    var edge_844_894_phi_982_: f32;
    var edge_844_894_phi_983_: f32;
    var edge_844_894_phi_984_: f32;
    var edge_844_894_phi_985_: f32;
    var edge_844_894_phi_986_: f32;
    var edge_844_894_phi_987_: f32;
    var edge_844_894_phi_988_: f32;
    var edge_844_894_phi_989_: f32;
    var edge_844_894_phi_990_: f32;
    var edge_844_894_phi_991_: f32;
    var edge_859_894_phi_976_: f32;
    var edge_859_894_phi_977_: f32;
    var edge_859_894_phi_978_: f32;
    var edge_859_894_phi_979_: f32;
    var edge_859_894_phi_980_: f32;
    var edge_859_894_phi_981_: f32;
    var edge_859_894_phi_982_: f32;
    var edge_859_894_phi_983_: f32;
    var edge_859_894_phi_984_: f32;
    var edge_859_894_phi_985_: f32;
    var edge_859_894_phi_986_: f32;
    var edge_859_894_phi_987_: f32;
    var edge_859_894_phi_988_: f32;
    var edge_859_894_phi_989_: f32;
    var edge_859_894_phi_990_: f32;
    var edge_859_894_phi_991_: f32;
    var edge_861_894_phi_976_: f32;
    var edge_861_894_phi_977_: f32;
    var edge_861_894_phi_978_: f32;
    var edge_861_894_phi_979_: f32;
    var edge_861_894_phi_980_: f32;
    var edge_861_894_phi_981_: f32;
    var edge_861_894_phi_982_: f32;
    var edge_861_894_phi_983_: f32;
    var edge_861_894_phi_984_: f32;
    var edge_861_894_phi_985_: f32;
    var edge_861_894_phi_986_: f32;
    var edge_861_894_phi_987_: f32;
    var edge_861_894_phi_988_: f32;
    var edge_861_894_phi_989_: f32;
    var edge_861_894_phi_990_: f32;
    var edge_861_894_phi_991_: f32;
    var edge_864_894_phi_976_: f32;
    var edge_864_894_phi_977_: f32;
    var edge_864_894_phi_978_: f32;
    var edge_864_894_phi_979_: f32;
    var edge_864_894_phi_980_: f32;
    var edge_864_894_phi_981_: f32;
    var edge_864_894_phi_982_: f32;
    var edge_864_894_phi_983_: f32;
    var edge_864_894_phi_984_: f32;
    var edge_864_894_phi_985_: f32;
    var edge_864_894_phi_986_: f32;
    var edge_864_894_phi_987_: f32;
    var edge_864_894_phi_988_: f32;
    var edge_864_894_phi_989_: f32;
    var edge_864_894_phi_990_: f32;
    var edge_864_894_phi_991_: f32;
    var edge_868_894_phi_976_: f32;
    var edge_868_894_phi_977_: f32;
    var edge_868_894_phi_978_: f32;
    var edge_868_894_phi_979_: f32;
    var edge_868_894_phi_980_: f32;
    var edge_868_894_phi_981_: f32;
    var edge_868_894_phi_982_: f32;
    var edge_868_894_phi_983_: f32;
    var edge_868_894_phi_984_: f32;
    var edge_868_894_phi_985_: f32;
    var edge_868_894_phi_986_: f32;
    var edge_868_894_phi_987_: f32;
    var edge_868_894_phi_988_: f32;
    var edge_868_894_phi_989_: f32;
    var edge_868_894_phi_990_: f32;
    var edge_868_894_phi_991_: f32;
    var edge_878_894_phi_976_: f32;
    var edge_878_894_phi_977_: f32;
    var edge_878_894_phi_978_: f32;
    var edge_878_894_phi_979_: f32;
    var edge_878_894_phi_980_: f32;
    var edge_878_894_phi_981_: f32;
    var edge_878_894_phi_982_: f32;
    var edge_878_894_phi_983_: f32;
    var edge_878_894_phi_984_: f32;
    var edge_878_894_phi_985_: f32;
    var edge_878_894_phi_986_: f32;
    var edge_878_894_phi_987_: f32;
    var edge_878_894_phi_988_: f32;
    var edge_878_894_phi_989_: f32;
    var edge_878_894_phi_990_: f32;
    var edge_878_894_phi_991_: f32;
    var edge_880_894_phi_976_: f32;
    var edge_880_894_phi_977_: f32;
    var edge_880_894_phi_978_: f32;
    var edge_880_894_phi_979_: f32;
    var edge_880_894_phi_980_: f32;
    var edge_880_894_phi_981_: f32;
    var edge_880_894_phi_982_: f32;
    var edge_880_894_phi_983_: f32;
    var edge_880_894_phi_984_: f32;
    var edge_880_894_phi_985_: f32;
    var edge_880_894_phi_986_: f32;
    var edge_880_894_phi_987_: f32;
    var edge_880_894_phi_988_: f32;
    var edge_880_894_phi_989_: f32;
    var edge_880_894_phi_990_: f32;
    var edge_880_894_phi_991_: f32;
    var edge_883_894_phi_976_: f32;
    var edge_883_894_phi_977_: f32;
    var edge_883_894_phi_978_: f32;
    var edge_883_894_phi_979_: f32;
    var edge_883_894_phi_980_: f32;
    var edge_883_894_phi_981_: f32;
    var edge_883_894_phi_982_: f32;
    var edge_883_894_phi_983_: f32;
    var edge_883_894_phi_984_: f32;
    var edge_883_894_phi_985_: f32;
    var edge_883_894_phi_986_: f32;
    var edge_883_894_phi_987_: f32;
    var edge_883_894_phi_988_: f32;
    var edge_883_894_phi_989_: f32;
    var edge_883_894_phi_990_: f32;
    var edge_883_894_phi_991_: f32;
    var edge_887_894_phi_976_: f32;
    var edge_887_894_phi_977_: f32;
    var edge_887_894_phi_978_: f32;
    var edge_887_894_phi_979_: f32;
    var edge_887_894_phi_980_: f32;
    var edge_887_894_phi_981_: f32;
    var edge_887_894_phi_982_: f32;
    var edge_887_894_phi_983_: f32;
    var edge_887_894_phi_984_: f32;
    var edge_887_894_phi_985_: f32;
    var edge_887_894_phi_986_: f32;
    var edge_887_894_phi_987_: f32;
    var edge_887_894_phi_988_: f32;
    var edge_887_894_phi_989_: f32;
    var edge_887_894_phi_990_: f32;
    var edge_887_894_phi_991_: f32;
    var edge_894_696_phi_771_: f32;
    var edge_894_696_phi_773_: f32;
    var edge_894_696_phi_775_: f32;
    var edge_894_696_phi_777_: f32;
    var edge_894_696_phi_779_: f32;
    var edge_894_696_phi_781_: f32;
    var edge_894_696_phi_783_: f32;
    var edge_894_696_phi_785_: f32;
    var edge_894_696_phi_787_: f32;
    var edge_894_696_phi_789_: f32;
    var edge_894_696_phi_791_: f32;
    var edge_894_696_phi_793_: f32;
    var edge_894_696_phi_795_: f32;
    var edge_894_696_phi_797_: f32;
    var edge_894_696_phi_799_: f32;
    var edge_894_696_phi_801_: f32;
    var edge_894_696_phi_803_: u32;
    var edge_696_945_phi_994_: f32;
    var edge_696_945_phi_996_: f32;
    var edge_696_945_phi_998_: f32;
    var edge_696_945_phi_1000_: f32;
    var edge_696_945_phi_1002_: f32;
    var edge_696_945_phi_1004_: f32;
    var edge_696_945_phi_1006_: f32;
    var edge_696_945_phi_1008_: f32;
    var edge_696_945_phi_1010_: u32;
    var edge_696_945_phi_994_1: f32;
    var edge_696_945_phi_996_1: f32;
    var edge_696_945_phi_998_1: f32;
    var edge_696_945_phi_1000_1: f32;
    var edge_696_945_phi_1002_1: f32;
    var edge_696_945_phi_1004_1: f32;
    var edge_696_945_phi_1006_1: f32;
    var edge_696_945_phi_1008_1: f32;
    var edge_696_945_phi_1010_1: u32;
    var loop_result_3: u32;
    var loop_did_return_3: bool = false;
    var loop_header_carry_1011_: bool;
    var phi_1163_: f32;
    var phi_1165_: f32;
    var phi_1167_: f32;
    var phi_1169_: f32;
    var phi_1171_: f32;
    var phi_1173_: f32;
    var phi_1175_: f32;
    var phi_1177_: u32;
    var phi_1013_: f32;
    var phi_1015_: u32;
    var edge_945_948_phi_1013_: f32;
    var edge_945_948_phi_1015_: u32;
    var loop_result_4: u32;
    var loop_did_return_4: bool = false;
    var loop_header_carry_1016_: bool;
    var phi_1045_: u32;
    var edge_959_954_phi_1045_: u32;
    var edge_962_954_phi_1045_: u32;
    var edge_955_954_phi_1045_: u32;
    var phi_1029_: u32;
    var edge_967_970_phi_1029_: u32;
    var edge_969_970_phi_1029_: u32;
    var edge_970_954_phi_1045_: u32;
    var edge_974_954_phi_1045_: u32;
    var edge_956_954_phi_1045_: u32;
    var phi_1112_: f32;
    var phi_1107_: f32;
    var edge_1046_1124_phi_1107_: f32;
    var edge_1048_1124_phi_1107_: f32;
    var edge_1051_1124_phi_1107_: f32;
    var edge_1055_1124_phi_1107_: f32;
    var edge_1065_1124_phi_1107_: f32;
    var edge_1067_1124_phi_1107_: f32;
    var edge_1070_1124_phi_1107_: f32;
    var edge_1074_1124_phi_1107_: f32;
    var edge_1089_1124_phi_1107_: f32;
    var edge_1091_1124_phi_1107_: f32;
    var edge_1094_1124_phi_1107_: f32;
    var edge_1098_1124_phi_1107_: f32;
    var edge_1108_1124_phi_1107_: f32;
    var edge_1110_1124_phi_1107_: f32;
    var edge_1113_1124_phi_1107_: f32;
    var edge_1117_1124_phi_1107_: f32;
    var edge_1128_1127_phi_1112_: f32;
    var edge_1129_1127_phi_1112_: f32;
    var edge_954_1127_phi_1112_: f32;
    var edge_1127_948_phi_1013_: f32;
    var edge_1127_948_phi_1015_: u32;
    var phi_1151_: f32;
    var phi_1152_: f32;
    var phi_1153_: f32;
    var phi_1154_: f32;
    var phi_1155_: f32;
    var phi_1156_: f32;
    var phi_1157_: f32;
    var phi_1158_: f32;
    var edge_1000_1033_phi_1151_: f32;
    var edge_1000_1033_phi_1152_: f32;
    var edge_1000_1033_phi_1153_: f32;
    var edge_1000_1033_phi_1154_: f32;
    var edge_1000_1033_phi_1155_: f32;
    var edge_1000_1033_phi_1156_: f32;
    var edge_1000_1033_phi_1157_: f32;
    var edge_1000_1033_phi_1158_: f32;
    var edge_1002_1033_phi_1151_: f32;
    var edge_1002_1033_phi_1152_: f32;
    var edge_1002_1033_phi_1153_: f32;
    var edge_1002_1033_phi_1154_: f32;
    var edge_1002_1033_phi_1155_: f32;
    var edge_1002_1033_phi_1156_: f32;
    var edge_1002_1033_phi_1157_: f32;
    var edge_1002_1033_phi_1158_: f32;
    var edge_1005_1033_phi_1151_: f32;
    var edge_1005_1033_phi_1152_: f32;
    var edge_1005_1033_phi_1153_: f32;
    var edge_1005_1033_phi_1154_: f32;
    var edge_1005_1033_phi_1155_: f32;
    var edge_1005_1033_phi_1156_: f32;
    var edge_1005_1033_phi_1157_: f32;
    var edge_1005_1033_phi_1158_: f32;
    var edge_1009_1033_phi_1151_: f32;
    var edge_1009_1033_phi_1152_: f32;
    var edge_1009_1033_phi_1153_: f32;
    var edge_1009_1033_phi_1154_: f32;
    var edge_1009_1033_phi_1155_: f32;
    var edge_1009_1033_phi_1156_: f32;
    var edge_1009_1033_phi_1157_: f32;
    var edge_1009_1033_phi_1158_: f32;
    var edge_1019_1033_phi_1151_: f32;
    var edge_1019_1033_phi_1152_: f32;
    var edge_1019_1033_phi_1153_: f32;
    var edge_1019_1033_phi_1154_: f32;
    var edge_1019_1033_phi_1155_: f32;
    var edge_1019_1033_phi_1156_: f32;
    var edge_1019_1033_phi_1157_: f32;
    var edge_1019_1033_phi_1158_: f32;
    var edge_1021_1033_phi_1151_: f32;
    var edge_1021_1033_phi_1152_: f32;
    var edge_1021_1033_phi_1153_: f32;
    var edge_1021_1033_phi_1154_: f32;
    var edge_1021_1033_phi_1155_: f32;
    var edge_1021_1033_phi_1156_: f32;
    var edge_1021_1033_phi_1157_: f32;
    var edge_1021_1033_phi_1158_: f32;
    var edge_1024_1033_phi_1151_: f32;
    var edge_1024_1033_phi_1152_: f32;
    var edge_1024_1033_phi_1153_: f32;
    var edge_1024_1033_phi_1154_: f32;
    var edge_1024_1033_phi_1155_: f32;
    var edge_1024_1033_phi_1156_: f32;
    var edge_1024_1033_phi_1157_: f32;
    var edge_1024_1033_phi_1158_: f32;
    var edge_1028_1033_phi_1151_: f32;
    var edge_1028_1033_phi_1152_: f32;
    var edge_1028_1033_phi_1153_: f32;
    var edge_1028_1033_phi_1154_: f32;
    var edge_1028_1033_phi_1155_: f32;
    var edge_1028_1033_phi_1156_: f32;
    var edge_1028_1033_phi_1157_: f32;
    var edge_1028_1033_phi_1158_: f32;
    var edge_1033_945_phi_994_: f32;
    var edge_1033_945_phi_996_: f32;
    var edge_1033_945_phi_998_: f32;
    var edge_1033_945_phi_1000_: f32;
    var edge_1033_945_phi_1002_: f32;
    var edge_1033_945_phi_1004_: f32;
    var edge_1033_945_phi_1006_: f32;
    var edge_1033_945_phi_1008_: f32;
    var edge_1033_945_phi_1010_: u32;
    var edge_945_1179_phi_1163_: f32;
    var edge_945_1179_phi_1165_: f32;
    var edge_945_1179_phi_1167_: f32;
    var edge_945_1179_phi_1169_: f32;
    var edge_945_1179_phi_1171_: f32;
    var edge_945_1179_phi_1173_: f32;
    var edge_945_1179_phi_1175_: f32;
    var edge_945_1179_phi_1177_: u32;
    var edge_945_1179_phi_1163_1: f32;
    var edge_945_1179_phi_1165_1: f32;
    var edge_945_1179_phi_1167_1: f32;
    var edge_945_1179_phi_1169_1: f32;
    var edge_945_1179_phi_1171_1: f32;
    var edge_945_1179_phi_1173_1: f32;
    var edge_945_1179_phi_1175_1: f32;
    var edge_945_1179_phi_1177_1: u32;
    var loop_result_5: u32;
    var loop_did_return_5: bool = false;
    var loop_header_carry_1178_: bool;
    var phi_1206_: f32;
    var edge_1188_1221_phi_1206_: f32;
    var edge_1190_1221_phi_1206_: f32;
    var edge_1193_1221_phi_1206_: f32;
    var edge_1197_1221_phi_1206_: f32;
    var edge_1207_1221_phi_1206_: f32;
    var edge_1209_1221_phi_1206_: f32;
    var edge_1212_1221_phi_1206_: f32;
    var edge_1216_1221_phi_1206_: f32;
    var phi_1220_: f32;
    var edge_1231_1230_phi_1220_: f32;
    var edge_1232_1230_phi_1220_: f32;
    var phi_1260_: f32;
    var phi_1261_: f32;
    var phi_1262_: f32;
    var phi_1263_: f32;
    var phi_1264_: f32;
    var phi_1265_: f32;
    var phi_1266_: f32;
    var edge_1248_1281_phi_1260_: f32;
    var edge_1248_1281_phi_1261_: f32;
    var edge_1248_1281_phi_1262_: f32;
    var edge_1248_1281_phi_1263_: f32;
    var edge_1248_1281_phi_1264_: f32;
    var edge_1248_1281_phi_1265_: f32;
    var edge_1248_1281_phi_1266_: f32;
    var edge_1250_1281_phi_1260_: f32;
    var edge_1250_1281_phi_1261_: f32;
    var edge_1250_1281_phi_1262_: f32;
    var edge_1250_1281_phi_1263_: f32;
    var edge_1250_1281_phi_1264_: f32;
    var edge_1250_1281_phi_1265_: f32;
    var edge_1250_1281_phi_1266_: f32;
    var edge_1253_1281_phi_1260_: f32;
    var edge_1253_1281_phi_1261_: f32;
    var edge_1253_1281_phi_1262_: f32;
    var edge_1253_1281_phi_1263_: f32;
    var edge_1253_1281_phi_1264_: f32;
    var edge_1253_1281_phi_1265_: f32;
    var edge_1253_1281_phi_1266_: f32;
    var edge_1257_1281_phi_1260_: f32;
    var edge_1257_1281_phi_1261_: f32;
    var edge_1257_1281_phi_1262_: f32;
    var edge_1257_1281_phi_1263_: f32;
    var edge_1257_1281_phi_1264_: f32;
    var edge_1257_1281_phi_1265_: f32;
    var edge_1257_1281_phi_1266_: f32;
    var edge_1267_1281_phi_1260_: f32;
    var edge_1267_1281_phi_1261_: f32;
    var edge_1267_1281_phi_1262_: f32;
    var edge_1267_1281_phi_1263_: f32;
    var edge_1267_1281_phi_1264_: f32;
    var edge_1267_1281_phi_1265_: f32;
    var edge_1267_1281_phi_1266_: f32;
    var edge_1269_1281_phi_1260_: f32;
    var edge_1269_1281_phi_1261_: f32;
    var edge_1269_1281_phi_1262_: f32;
    var edge_1269_1281_phi_1263_: f32;
    var edge_1269_1281_phi_1264_: f32;
    var edge_1269_1281_phi_1265_: f32;
    var edge_1269_1281_phi_1266_: f32;
    var edge_1272_1281_phi_1260_: f32;
    var edge_1272_1281_phi_1261_: f32;
    var edge_1272_1281_phi_1262_: f32;
    var edge_1272_1281_phi_1263_: f32;
    var edge_1272_1281_phi_1264_: f32;
    var edge_1272_1281_phi_1265_: f32;
    var edge_1272_1281_phi_1266_: f32;
    var edge_1276_1281_phi_1260_: f32;
    var edge_1276_1281_phi_1261_: f32;
    var edge_1276_1281_phi_1262_: f32;
    var edge_1276_1281_phi_1263_: f32;
    var edge_1276_1281_phi_1264_: f32;
    var edge_1276_1281_phi_1265_: f32;
    var edge_1276_1281_phi_1266_: f32;
    var edge_1281_1179_phi_1163_: f32;
    var edge_1281_1179_phi_1165_: f32;
    var edge_1281_1179_phi_1167_: f32;
    var edge_1281_1179_phi_1169_: f32;
    var edge_1281_1179_phi_1171_: f32;
    var edge_1281_1179_phi_1173_: f32;
    var edge_1281_1179_phi_1175_: f32;
    var edge_1281_1179_phi_1177_: u32;
    var edge_1179_4_phi_58_: f32;
    var edge_1179_4_phi_57_: f32;
    var edge_1179_4_phi_56_: f32;
    var edge_1179_4_phi_55_: f32;
    var edge_1179_4_phi_54_: f32;
    var edge_1179_4_phi_53_: f32;
    var edge_1179_4_phi_52_: f32;
    var edge_0_4_phi_58_: f32;
    var edge_0_4_phi_57_: f32;
    var edge_0_4_phi_56_: f32;
    var edge_0_4_phi_55_: f32;
    var edge_0_4_phi_54_: f32;
    var edge_0_4_phi_53_: f32;
    var edge_0_4_phi_52_: f32;
    var phi_232_: f32;
    var phi_165_: bool;
    var phi_95_: u32;
    var edge_4_5_phi_232_: f32;
    var edge_4_5_phi_165_: bool;
    var edge_4_5_phi_95_: u32;
    var loop_result_6: u32;
    var loop_did_return_6: bool = false;
    var loop_header_carry_96_: bool;
    var phi_102_: u32;
    var edge_6_10_phi_102_: u32;
    var edge_9_10_phi_102_: u32;
    var phi_1792_: f32;
    var edge_1895_1887_phi_1792_: f32;
    var edge_1898_1887_phi_1792_: f32;
    var edge_1892_1887_phi_1792_: f32;
    var edge_1889_1887_phi_1792_: f32;
    var edge_1886_1887_phi_1792_: f32;
    var edge_10_1887_phi_1792_: f32;
    var phi_1803_: f32;
    var edge_1910_1905_phi_1803_: f32;
    var edge_1913_1905_phi_1803_: f32;
    var edge_1907_1905_phi_1803_: f32;
    var edge_1904_1905_phi_1803_: f32;
    var edge_1887_1905_phi_1803_: f32;
    var phi_1813_: f32;
    var edge_1931_1923_phi_1813_: f32;
    var edge_1934_1923_phi_1813_: f32;
    var edge_1928_1923_phi_1813_: f32;
    var edge_1925_1923_phi_1813_: f32;
    var edge_1922_1923_phi_1813_: f32;
    var edge_1905_1923_phi_1813_: f32;
    var phi_1822_: f32;
    var edge_1946_1941_phi_1822_: f32;
    var edge_1949_1941_phi_1822_: f32;
    var edge_1943_1941_phi_1822_: f32;
    var edge_1940_1941_phi_1822_: f32;
    var edge_1923_1941_phi_1822_: f32;
    var phi_233_: f32;
    var phi_166_: bool;
    var phi_1834_: f32;
    var edge_1970_1959_phi_1834_: f32;
    var edge_1973_1959_phi_1834_: f32;
    var edge_1967_1959_phi_1834_: f32;
    var edge_1964_1959_phi_1834_: f32;
    var edge_1961_1959_phi_1834_: f32;
    var edge_1958_1959_phi_1834_: f32;
    var edge_11_1959_phi_1834_: f32;
    var phi_1846_: f32;
    var edge_1991_1980_phi_1846_: f32;
    var edge_1994_1980_phi_1846_: f32;
    var edge_1988_1980_phi_1846_: f32;
    var edge_1985_1980_phi_1846_: f32;
    var edge_1982_1980_phi_1846_: f32;
    var edge_1979_1980_phi_1846_: f32;
    var edge_1959_1980_phi_1846_: f32;
    var edge_1980_13_phi_233_: f32;
    var edge_1980_13_phi_166_: bool;
    var edge_14_13_phi_233_: f32;
    var edge_14_13_phi_166_: bool;
    var edge_15_13_phi_233_: f32;
    var edge_15_13_phi_166_: bool;
    var edge_1941_13_phi_233_: f32;
    var edge_1941_13_phi_166_: bool;
    var edge_13_5_phi_232_: f32;
    var edge_13_5_phi_165_: bool;
    var edge_13_5_phi_95_: u32;
    var structured_result_1: u32;
    var structured_did_return_1: bool = false;
    var phi_164_: u32;
    var edge_16_18_phi_164_: u32;
    var edge_2127_18_phi_164_: u32;

    let _e7 = input.p2_;
    let _e9 = input.p3_;
    let _e11 = input.p4_;
    let _e13 = input.p5_;
    let _e15 = input.p6_;
    let _e17 = input.p7_;
    let _e19 = input.p8_;
    let _e21 = input.p9_;
    let _e23 = input.p10_;
    if (0.5f <= _e21) {
        edge_0_414_phi_552_ = 0f;
        edge_0_414_phi_554_ = 0f;
        edge_0_414_phi_556_ = 0f;
        edge_0_414_phi_558_ = 0f;
        edge_0_414_phi_560_ = 0f;
        edge_0_414_phi_562_ = 0f;
        edge_0_414_phi_564_ = 0f;
        edge_0_414_phi_566_ = 0f;
        edge_0_414_phi_568_ = 0f;
        edge_0_414_phi_570_ = 0f;
        edge_0_414_phi_572_ = 0f;
        edge_0_414_phi_574_ = 0f;
        edge_0_414_phi_576_ = 0f;
        edge_0_414_phi_578_ = 0f;
        edge_0_414_phi_580_ = 0f;
        edge_0_414_phi_582_ = 0f;
        edge_0_414_phi_584_ = 0u;
        let _e62 = edge_0_414_phi_552_;
        let _e64 = edge_0_414_phi_554_;
        let _e66 = edge_0_414_phi_556_;
        let _e68 = edge_0_414_phi_558_;
        let _e70 = edge_0_414_phi_560_;
        let _e72 = edge_0_414_phi_562_;
        let _e74 = edge_0_414_phi_564_;
        let _e76 = edge_0_414_phi_566_;
        let _e78 = edge_0_414_phi_568_;
        let _e80 = edge_0_414_phi_570_;
        let _e82 = edge_0_414_phi_572_;
        let _e84 = edge_0_414_phi_574_;
        let _e86 = edge_0_414_phi_576_;
        let _e88 = edge_0_414_phi_578_;
        let _e90 = edge_0_414_phi_580_;
        let _e92 = edge_0_414_phi_582_;
        let _e94 = edge_0_414_phi_584_;
        phi_552_ = _e62;
        phi_554_ = _e64;
        phi_556_ = _e66;
        phi_558_ = _e68;
        phi_560_ = _e70;
        phi_562_ = _e72;
        phi_564_ = _e74;
        phi_566_ = _e76;
        phi_568_ = _e78;
        phi_570_ = _e80;
        phi_572_ = _e82;
        phi_574_ = _e84;
        phi_576_ = _e86;
        phi_578_ = _e88;
        phi_580_ = _e90;
        phi_582_ = _e92;
        phi_584_ = _e94;
        loop {
            let _e114 = phi_552_;
            let _e116 = phi_554_;
            let _e118 = phi_556_;
            let _e120 = phi_558_;
            let _e122 = phi_560_;
            let _e124 = phi_562_;
            let _e126 = phi_564_;
            let _e128 = phi_566_;
            let _e130 = phi_568_;
            let _e132 = phi_570_;
            let _e134 = phi_572_;
            let _e136 = phi_574_;
            let _e138 = phi_576_;
            let _e140 = phi_578_;
            let _e142 = phi_580_;
            let _e144 = phi_582_;
            let _e146 = phi_584_;
            let _e150 = (bitcast<i32>(_e146) < bitcast<i32>(12u));
            if _e150 {
                edge_414_417_phi_588_ = 0f;
                edge_414_417_phi_590_ = 0u;
                let _e156 = edge_414_417_phi_588_;
                let _e158 = edge_414_417_phi_590_;
                phi_588_ = _e156;
                phi_590_ = _e158;
                loop {
                    let _e163 = phi_588_;
                    let _e165 = phi_590_;
                    let _e169 = (bitcast<i32>(_e165) < bitcast<i32>(7u));
                    if _e169 {
                        if (bitcast<i32>(_e146) < bitcast<i32>(6u)) {
                            if (_e165 == 0u) {
                                edge_424_423_phi_622_ = 4294967295u;
                                let _e192 = edge_424_423_phi_622_;
                                phi_622_ = _e192;
                            } else {
                                if (_e165 == (_e146 + 1u)) {
                                    edge_428_423_phi_622_ = 1u;
                                    let _e182 = edge_428_423_phi_622_;
                                    phi_622_ = _e182;
                                } else {
                                    edge_431_423_phi_622_ = 0u;
                                    let _e187 = edge_431_423_phi_622_;
                                    phi_622_ = _e187;
                                }
                            }
                        } else {
                            let _e195 = (_e146 - 6u);
                            let _e197 = (_e195 + 1u);
                            if (_e165 == _e197) {
                                edge_425_423_phi_622_ = 4294967295u;
                                let _e228 = edge_425_423_phi_622_;
                                phi_622_ = _e228;
                            } else {
                                if (_e195 == 5u) {
                                    edge_436_439_phi_606_ = 0u;
                                    let _e204 = edge_436_439_phi_606_;
                                    phi_606_ = _e204;
                                } else {
                                    edge_438_439_phi_606_ = _e197;
                                    let _e208 = edge_438_439_phi_606_;
                                    phi_606_ = _e208;
                                }
                                let _e211 = phi_606_;
                                if (_e165 == (_e211 + 1u)) {
                                    edge_439_423_phi_622_ = 1u;
                                    let _e218 = edge_439_423_phi_622_;
                                    phi_622_ = _e218;
                                } else {
                                    edge_443_423_phi_622_ = 0u;
                                    let _e223 = edge_443_423_phi_622_;
                                    phi_622_ = _e223;
                                }
                            }
                        }
                        let _e231 = phi_622_;
                        if (_e231 == 0u) {
                            edge_423_596_phi_657_ = _e163;
                            let _e319 = edge_423_596_phi_657_;
                            phi_657_ = _e319;
                        } else {
                            if (bitcast<i32>(_e165) < bitcast<i32>(4u)) {
                                if (bitcast<i32>(_e165) < bitcast<i32>(2u)) {
                                    if (bitcast<i32>(_e165) < bitcast<i32>(1u)) {
                                        edge_560_593_phi_652_ = _e7;
                                        let _e248 = edge_560_593_phi_652_;
                                        phi_652_ = _e248;
                                    } else {
                                        edge_562_593_phi_652_ = _e9;
                                        let _e252 = edge_562_593_phi_652_;
                                        phi_652_ = _e252;
                                    }
                                } else {
                                    if (bitcast<i32>((_e165 - 2u)) < bitcast<i32>(1u)) {
                                        edge_565_593_phi_652_ = _e11;
                                        let _e262 = edge_565_593_phi_652_;
                                        phi_652_ = _e262;
                                    } else {
                                        edge_569_593_phi_652_ = _e13;
                                        let _e266 = edge_569_593_phi_652_;
                                        phi_652_ = _e266;
                                    }
                                }
                            } else {
                                let _e269 = (_e165 - 4u);
                                if (bitcast<i32>(_e269) < bitcast<i32>(2u)) {
                                    if (bitcast<i32>(_e269) < bitcast<i32>(1u)) {
                                        edge_579_593_phi_652_ = _e15;
                                        let _e280 = edge_579_593_phi_652_;
                                        phi_652_ = _e280;
                                    } else {
                                        edge_581_593_phi_652_ = _e17;
                                        let _e284 = edge_581_593_phi_652_;
                                        phi_652_ = _e284;
                                    }
                                } else {
                                    if (bitcast<i32>((_e269 - 2u)) < bitcast<i32>(1u)) {
                                        edge_584_593_phi_652_ = _e19;
                                        let _e294 = edge_584_593_phi_652_;
                                        phi_652_ = _e294;
                                    } else {
                                        edge_588_593_phi_652_ = 0f;
                                        let _e299 = edge_588_593_phi_652_;
                                        phi_652_ = _e299;
                                    }
                                }
                            }
                            let _e302 = phi_652_;
                            if (bitcast<i32>(0u) < bitcast<i32>(_e231)) {
                                edge_597_596_phi_657_ = (_e163 + _e302);
                                let _e311 = edge_597_596_phi_657_;
                                phi_657_ = _e311;
                            } else {
                                edge_598_596_phi_657_ = (_e163 - _e302);
                                let _e315 = edge_598_596_phi_657_;
                                phi_657_ = _e315;
                            }
                        }
                        let _e322 = phi_657_;
                        edge_596_417_phi_588_ = _e322;
                        edge_596_417_phi_590_ = (_e165 + 1u);
                        let _e328 = edge_596_417_phi_588_;
                        let _e330 = edge_596_417_phi_590_;
                        phi_588_ = _e328;
                        phi_590_ = _e330;
                        continue;
                    } else {
                        loop_header_carry_592_ = _e169;
                        break;
                    }
                }
                let _e335 = phi_588_;
                if (bitcast<i32>(_e146) < bitcast<i32>(8u)) {
                    if (bitcast<i32>(_e146) < bitcast<i32>(4u)) {
                        if (bitcast<i32>(_e146) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e146) < bitcast<i32>(1u)) {
                                edge_472_550_phi_753_ = _e114;
                                edge_472_550_phi_754_ = _e116;
                                edge_472_550_phi_755_ = _e118;
                                edge_472_550_phi_756_ = _e120;
                                edge_472_550_phi_757_ = _e122;
                                edge_472_550_phi_758_ = _e124;
                                edge_472_550_phi_759_ = _e126;
                                edge_472_550_phi_760_ = _e128;
                                edge_472_550_phi_761_ = _e130;
                                edge_472_550_phi_762_ = _e132;
                                edge_472_550_phi_763_ = _e134;
                                edge_472_550_phi_764_ = _e136;
                                edge_472_550_phi_765_ = _e138;
                                edge_472_550_phi_766_ = _e140;
                                edge_472_550_phi_767_ = _e142;
                                edge_472_550_phi_768_ = _e335;
                                let _e373 = edge_472_550_phi_753_;
                                let _e375 = edge_472_550_phi_754_;
                                let _e377 = edge_472_550_phi_755_;
                                let _e379 = edge_472_550_phi_756_;
                                let _e381 = edge_472_550_phi_757_;
                                let _e383 = edge_472_550_phi_758_;
                                let _e385 = edge_472_550_phi_759_;
                                let _e387 = edge_472_550_phi_760_;
                                let _e389 = edge_472_550_phi_761_;
                                let _e391 = edge_472_550_phi_762_;
                                let _e393 = edge_472_550_phi_763_;
                                let _e395 = edge_472_550_phi_764_;
                                let _e397 = edge_472_550_phi_765_;
                                let _e399 = edge_472_550_phi_766_;
                                let _e401 = edge_472_550_phi_767_;
                                let _e403 = edge_472_550_phi_768_;
                                phi_753_ = _e373;
                                phi_754_ = _e375;
                                phi_755_ = _e377;
                                phi_756_ = _e379;
                                phi_757_ = _e381;
                                phi_758_ = _e383;
                                phi_759_ = _e385;
                                phi_760_ = _e387;
                                phi_761_ = _e389;
                                phi_762_ = _e391;
                                phi_763_ = _e393;
                                phi_764_ = _e395;
                                phi_765_ = _e397;
                                phi_766_ = _e399;
                                phi_767_ = _e401;
                                phi_768_ = _e403;
                            } else {
                                edge_474_550_phi_753_ = _e114;
                                edge_474_550_phi_754_ = _e116;
                                edge_474_550_phi_755_ = _e118;
                                edge_474_550_phi_756_ = _e120;
                                edge_474_550_phi_757_ = _e122;
                                edge_474_550_phi_758_ = _e124;
                                edge_474_550_phi_759_ = _e126;
                                edge_474_550_phi_760_ = _e128;
                                edge_474_550_phi_761_ = _e130;
                                edge_474_550_phi_762_ = _e132;
                                edge_474_550_phi_763_ = _e134;
                                edge_474_550_phi_764_ = _e136;
                                edge_474_550_phi_765_ = _e138;
                                edge_474_550_phi_766_ = _e140;
                                edge_474_550_phi_767_ = _e335;
                                edge_474_550_phi_768_ = _e144;
                                let _e437 = edge_474_550_phi_753_;
                                let _e439 = edge_474_550_phi_754_;
                                let _e441 = edge_474_550_phi_755_;
                                let _e443 = edge_474_550_phi_756_;
                                let _e445 = edge_474_550_phi_757_;
                                let _e447 = edge_474_550_phi_758_;
                                let _e449 = edge_474_550_phi_759_;
                                let _e451 = edge_474_550_phi_760_;
                                let _e453 = edge_474_550_phi_761_;
                                let _e455 = edge_474_550_phi_762_;
                                let _e457 = edge_474_550_phi_763_;
                                let _e459 = edge_474_550_phi_764_;
                                let _e461 = edge_474_550_phi_765_;
                                let _e463 = edge_474_550_phi_766_;
                                let _e465 = edge_474_550_phi_767_;
                                let _e467 = edge_474_550_phi_768_;
                                phi_753_ = _e437;
                                phi_754_ = _e439;
                                phi_755_ = _e441;
                                phi_756_ = _e443;
                                phi_757_ = _e445;
                                phi_758_ = _e447;
                                phi_759_ = _e449;
                                phi_760_ = _e451;
                                phi_761_ = _e453;
                                phi_762_ = _e455;
                                phi_763_ = _e457;
                                phi_764_ = _e459;
                                phi_765_ = _e461;
                                phi_766_ = _e463;
                                phi_767_ = _e465;
                                phi_768_ = _e467;
                            }
                        } else {
                            if (bitcast<i32>((_e146 - 2u)) < bitcast<i32>(1u)) {
                                edge_477_550_phi_753_ = _e114;
                                edge_477_550_phi_754_ = _e116;
                                edge_477_550_phi_755_ = _e118;
                                edge_477_550_phi_756_ = _e120;
                                edge_477_550_phi_757_ = _e122;
                                edge_477_550_phi_758_ = _e124;
                                edge_477_550_phi_759_ = _e126;
                                edge_477_550_phi_760_ = _e128;
                                edge_477_550_phi_761_ = _e130;
                                edge_477_550_phi_762_ = _e132;
                                edge_477_550_phi_763_ = _e134;
                                edge_477_550_phi_764_ = _e136;
                                edge_477_550_phi_765_ = _e138;
                                edge_477_550_phi_766_ = _e335;
                                edge_477_550_phi_767_ = _e142;
                                edge_477_550_phi_768_ = _e144;
                                let _e507 = edge_477_550_phi_753_;
                                let _e509 = edge_477_550_phi_754_;
                                let _e511 = edge_477_550_phi_755_;
                                let _e513 = edge_477_550_phi_756_;
                                let _e515 = edge_477_550_phi_757_;
                                let _e517 = edge_477_550_phi_758_;
                                let _e519 = edge_477_550_phi_759_;
                                let _e521 = edge_477_550_phi_760_;
                                let _e523 = edge_477_550_phi_761_;
                                let _e525 = edge_477_550_phi_762_;
                                let _e527 = edge_477_550_phi_763_;
                                let _e529 = edge_477_550_phi_764_;
                                let _e531 = edge_477_550_phi_765_;
                                let _e533 = edge_477_550_phi_766_;
                                let _e535 = edge_477_550_phi_767_;
                                let _e537 = edge_477_550_phi_768_;
                                phi_753_ = _e507;
                                phi_754_ = _e509;
                                phi_755_ = _e511;
                                phi_756_ = _e513;
                                phi_757_ = _e515;
                                phi_758_ = _e517;
                                phi_759_ = _e519;
                                phi_760_ = _e521;
                                phi_761_ = _e523;
                                phi_762_ = _e525;
                                phi_763_ = _e527;
                                phi_764_ = _e529;
                                phi_765_ = _e531;
                                phi_766_ = _e533;
                                phi_767_ = _e535;
                                phi_768_ = _e537;
                            } else {
                                edge_481_550_phi_753_ = _e114;
                                edge_481_550_phi_754_ = _e116;
                                edge_481_550_phi_755_ = _e118;
                                edge_481_550_phi_756_ = _e120;
                                edge_481_550_phi_757_ = _e122;
                                edge_481_550_phi_758_ = _e124;
                                edge_481_550_phi_759_ = _e126;
                                edge_481_550_phi_760_ = _e128;
                                edge_481_550_phi_761_ = _e130;
                                edge_481_550_phi_762_ = _e132;
                                edge_481_550_phi_763_ = _e134;
                                edge_481_550_phi_764_ = _e136;
                                edge_481_550_phi_765_ = _e335;
                                edge_481_550_phi_766_ = _e140;
                                edge_481_550_phi_767_ = _e142;
                                edge_481_550_phi_768_ = _e144;
                                let _e571 = edge_481_550_phi_753_;
                                let _e573 = edge_481_550_phi_754_;
                                let _e575 = edge_481_550_phi_755_;
                                let _e577 = edge_481_550_phi_756_;
                                let _e579 = edge_481_550_phi_757_;
                                let _e581 = edge_481_550_phi_758_;
                                let _e583 = edge_481_550_phi_759_;
                                let _e585 = edge_481_550_phi_760_;
                                let _e587 = edge_481_550_phi_761_;
                                let _e589 = edge_481_550_phi_762_;
                                let _e591 = edge_481_550_phi_763_;
                                let _e593 = edge_481_550_phi_764_;
                                let _e595 = edge_481_550_phi_765_;
                                let _e597 = edge_481_550_phi_766_;
                                let _e599 = edge_481_550_phi_767_;
                                let _e601 = edge_481_550_phi_768_;
                                phi_753_ = _e571;
                                phi_754_ = _e573;
                                phi_755_ = _e575;
                                phi_756_ = _e577;
                                phi_757_ = _e579;
                                phi_758_ = _e581;
                                phi_759_ = _e583;
                                phi_760_ = _e585;
                                phi_761_ = _e587;
                                phi_762_ = _e589;
                                phi_763_ = _e591;
                                phi_764_ = _e593;
                                phi_765_ = _e595;
                                phi_766_ = _e597;
                                phi_767_ = _e599;
                                phi_768_ = _e601;
                            }
                        }
                    } else {
                        let _e619 = (_e146 - 4u);
                        if (bitcast<i32>(_e619) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e619) < bitcast<i32>(1u)) {
                                edge_491_550_phi_753_ = _e114;
                                edge_491_550_phi_754_ = _e116;
                                edge_491_550_phi_755_ = _e118;
                                edge_491_550_phi_756_ = _e120;
                                edge_491_550_phi_757_ = _e122;
                                edge_491_550_phi_758_ = _e124;
                                edge_491_550_phi_759_ = _e126;
                                edge_491_550_phi_760_ = _e128;
                                edge_491_550_phi_761_ = _e130;
                                edge_491_550_phi_762_ = _e132;
                                edge_491_550_phi_763_ = _e134;
                                edge_491_550_phi_764_ = _e335;
                                edge_491_550_phi_765_ = _e138;
                                edge_491_550_phi_766_ = _e140;
                                edge_491_550_phi_767_ = _e142;
                                edge_491_550_phi_768_ = _e144;
                                let _e645 = edge_491_550_phi_753_;
                                let _e647 = edge_491_550_phi_754_;
                                let _e649 = edge_491_550_phi_755_;
                                let _e651 = edge_491_550_phi_756_;
                                let _e653 = edge_491_550_phi_757_;
                                let _e655 = edge_491_550_phi_758_;
                                let _e657 = edge_491_550_phi_759_;
                                let _e659 = edge_491_550_phi_760_;
                                let _e661 = edge_491_550_phi_761_;
                                let _e663 = edge_491_550_phi_762_;
                                let _e665 = edge_491_550_phi_763_;
                                let _e667 = edge_491_550_phi_764_;
                                let _e669 = edge_491_550_phi_765_;
                                let _e671 = edge_491_550_phi_766_;
                                let _e673 = edge_491_550_phi_767_;
                                let _e675 = edge_491_550_phi_768_;
                                phi_753_ = _e645;
                                phi_754_ = _e647;
                                phi_755_ = _e649;
                                phi_756_ = _e651;
                                phi_757_ = _e653;
                                phi_758_ = _e655;
                                phi_759_ = _e657;
                                phi_760_ = _e659;
                                phi_761_ = _e661;
                                phi_762_ = _e663;
                                phi_763_ = _e665;
                                phi_764_ = _e667;
                                phi_765_ = _e669;
                                phi_766_ = _e671;
                                phi_767_ = _e673;
                                phi_768_ = _e675;
                            } else {
                                edge_493_550_phi_753_ = _e114;
                                edge_493_550_phi_754_ = _e116;
                                edge_493_550_phi_755_ = _e118;
                                edge_493_550_phi_756_ = _e120;
                                edge_493_550_phi_757_ = _e122;
                                edge_493_550_phi_758_ = _e124;
                                edge_493_550_phi_759_ = _e126;
                                edge_493_550_phi_760_ = _e128;
                                edge_493_550_phi_761_ = _e130;
                                edge_493_550_phi_762_ = _e132;
                                edge_493_550_phi_763_ = _e335;
                                edge_493_550_phi_764_ = _e136;
                                edge_493_550_phi_765_ = _e138;
                                edge_493_550_phi_766_ = _e140;
                                edge_493_550_phi_767_ = _e142;
                                edge_493_550_phi_768_ = _e144;
                                let _e709 = edge_493_550_phi_753_;
                                let _e711 = edge_493_550_phi_754_;
                                let _e713 = edge_493_550_phi_755_;
                                let _e715 = edge_493_550_phi_756_;
                                let _e717 = edge_493_550_phi_757_;
                                let _e719 = edge_493_550_phi_758_;
                                let _e721 = edge_493_550_phi_759_;
                                let _e723 = edge_493_550_phi_760_;
                                let _e725 = edge_493_550_phi_761_;
                                let _e727 = edge_493_550_phi_762_;
                                let _e729 = edge_493_550_phi_763_;
                                let _e731 = edge_493_550_phi_764_;
                                let _e733 = edge_493_550_phi_765_;
                                let _e735 = edge_493_550_phi_766_;
                                let _e737 = edge_493_550_phi_767_;
                                let _e739 = edge_493_550_phi_768_;
                                phi_753_ = _e709;
                                phi_754_ = _e711;
                                phi_755_ = _e713;
                                phi_756_ = _e715;
                                phi_757_ = _e717;
                                phi_758_ = _e719;
                                phi_759_ = _e721;
                                phi_760_ = _e723;
                                phi_761_ = _e725;
                                phi_762_ = _e727;
                                phi_763_ = _e729;
                                phi_764_ = _e731;
                                phi_765_ = _e733;
                                phi_766_ = _e735;
                                phi_767_ = _e737;
                                phi_768_ = _e739;
                            }
                        } else {
                            if (bitcast<i32>((_e619 - 2u)) < bitcast<i32>(1u)) {
                                edge_496_550_phi_753_ = _e114;
                                edge_496_550_phi_754_ = _e116;
                                edge_496_550_phi_755_ = _e118;
                                edge_496_550_phi_756_ = _e120;
                                edge_496_550_phi_757_ = _e122;
                                edge_496_550_phi_758_ = _e124;
                                edge_496_550_phi_759_ = _e126;
                                edge_496_550_phi_760_ = _e128;
                                edge_496_550_phi_761_ = _e130;
                                edge_496_550_phi_762_ = _e335;
                                edge_496_550_phi_763_ = _e134;
                                edge_496_550_phi_764_ = _e136;
                                edge_496_550_phi_765_ = _e138;
                                edge_496_550_phi_766_ = _e140;
                                edge_496_550_phi_767_ = _e142;
                                edge_496_550_phi_768_ = _e144;
                                let _e779 = edge_496_550_phi_753_;
                                let _e781 = edge_496_550_phi_754_;
                                let _e783 = edge_496_550_phi_755_;
                                let _e785 = edge_496_550_phi_756_;
                                let _e787 = edge_496_550_phi_757_;
                                let _e789 = edge_496_550_phi_758_;
                                let _e791 = edge_496_550_phi_759_;
                                let _e793 = edge_496_550_phi_760_;
                                let _e795 = edge_496_550_phi_761_;
                                let _e797 = edge_496_550_phi_762_;
                                let _e799 = edge_496_550_phi_763_;
                                let _e801 = edge_496_550_phi_764_;
                                let _e803 = edge_496_550_phi_765_;
                                let _e805 = edge_496_550_phi_766_;
                                let _e807 = edge_496_550_phi_767_;
                                let _e809 = edge_496_550_phi_768_;
                                phi_753_ = _e779;
                                phi_754_ = _e781;
                                phi_755_ = _e783;
                                phi_756_ = _e785;
                                phi_757_ = _e787;
                                phi_758_ = _e789;
                                phi_759_ = _e791;
                                phi_760_ = _e793;
                                phi_761_ = _e795;
                                phi_762_ = _e797;
                                phi_763_ = _e799;
                                phi_764_ = _e801;
                                phi_765_ = _e803;
                                phi_766_ = _e805;
                                phi_767_ = _e807;
                                phi_768_ = _e809;
                            } else {
                                edge_500_550_phi_753_ = _e114;
                                edge_500_550_phi_754_ = _e116;
                                edge_500_550_phi_755_ = _e118;
                                edge_500_550_phi_756_ = _e120;
                                edge_500_550_phi_757_ = _e122;
                                edge_500_550_phi_758_ = _e124;
                                edge_500_550_phi_759_ = _e126;
                                edge_500_550_phi_760_ = _e128;
                                edge_500_550_phi_761_ = _e335;
                                edge_500_550_phi_762_ = _e132;
                                edge_500_550_phi_763_ = _e134;
                                edge_500_550_phi_764_ = _e136;
                                edge_500_550_phi_765_ = _e138;
                                edge_500_550_phi_766_ = _e140;
                                edge_500_550_phi_767_ = _e142;
                                edge_500_550_phi_768_ = _e144;
                                let _e843 = edge_500_550_phi_753_;
                                let _e845 = edge_500_550_phi_754_;
                                let _e847 = edge_500_550_phi_755_;
                                let _e849 = edge_500_550_phi_756_;
                                let _e851 = edge_500_550_phi_757_;
                                let _e853 = edge_500_550_phi_758_;
                                let _e855 = edge_500_550_phi_759_;
                                let _e857 = edge_500_550_phi_760_;
                                let _e859 = edge_500_550_phi_761_;
                                let _e861 = edge_500_550_phi_762_;
                                let _e863 = edge_500_550_phi_763_;
                                let _e865 = edge_500_550_phi_764_;
                                let _e867 = edge_500_550_phi_765_;
                                let _e869 = edge_500_550_phi_766_;
                                let _e871 = edge_500_550_phi_767_;
                                let _e873 = edge_500_550_phi_768_;
                                phi_753_ = _e843;
                                phi_754_ = _e845;
                                phi_755_ = _e847;
                                phi_756_ = _e849;
                                phi_757_ = _e851;
                                phi_758_ = _e853;
                                phi_759_ = _e855;
                                phi_760_ = _e857;
                                phi_761_ = _e859;
                                phi_762_ = _e861;
                                phi_763_ = _e863;
                                phi_764_ = _e865;
                                phi_765_ = _e867;
                                phi_766_ = _e869;
                                phi_767_ = _e871;
                                phi_768_ = _e873;
                            }
                        }
                    }
                } else {
                    let _e891 = (_e146 - 8u);
                    if (bitcast<i32>(_e891) < bitcast<i32>(4u)) {
                        if (bitcast<i32>(_e891) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e891) < bitcast<i32>(1u)) {
                                edge_515_550_phi_753_ = _e114;
                                edge_515_550_phi_754_ = _e116;
                                edge_515_550_phi_755_ = _e118;
                                edge_515_550_phi_756_ = _e120;
                                edge_515_550_phi_757_ = _e122;
                                edge_515_550_phi_758_ = _e124;
                                edge_515_550_phi_759_ = _e126;
                                edge_515_550_phi_760_ = _e335;
                                edge_515_550_phi_761_ = _e130;
                                edge_515_550_phi_762_ = _e132;
                                edge_515_550_phi_763_ = _e134;
                                edge_515_550_phi_764_ = _e136;
                                edge_515_550_phi_765_ = _e138;
                                edge_515_550_phi_766_ = _e140;
                                edge_515_550_phi_767_ = _e142;
                                edge_515_550_phi_768_ = _e144;
                                let _e921 = edge_515_550_phi_753_;
                                let _e923 = edge_515_550_phi_754_;
                                let _e925 = edge_515_550_phi_755_;
                                let _e927 = edge_515_550_phi_756_;
                                let _e929 = edge_515_550_phi_757_;
                                let _e931 = edge_515_550_phi_758_;
                                let _e933 = edge_515_550_phi_759_;
                                let _e935 = edge_515_550_phi_760_;
                                let _e937 = edge_515_550_phi_761_;
                                let _e939 = edge_515_550_phi_762_;
                                let _e941 = edge_515_550_phi_763_;
                                let _e943 = edge_515_550_phi_764_;
                                let _e945 = edge_515_550_phi_765_;
                                let _e947 = edge_515_550_phi_766_;
                                let _e949 = edge_515_550_phi_767_;
                                let _e951 = edge_515_550_phi_768_;
                                phi_753_ = _e921;
                                phi_754_ = _e923;
                                phi_755_ = _e925;
                                phi_756_ = _e927;
                                phi_757_ = _e929;
                                phi_758_ = _e931;
                                phi_759_ = _e933;
                                phi_760_ = _e935;
                                phi_761_ = _e937;
                                phi_762_ = _e939;
                                phi_763_ = _e941;
                                phi_764_ = _e943;
                                phi_765_ = _e945;
                                phi_766_ = _e947;
                                phi_767_ = _e949;
                                phi_768_ = _e951;
                            } else {
                                edge_517_550_phi_753_ = _e114;
                                edge_517_550_phi_754_ = _e116;
                                edge_517_550_phi_755_ = _e118;
                                edge_517_550_phi_756_ = _e120;
                                edge_517_550_phi_757_ = _e122;
                                edge_517_550_phi_758_ = _e124;
                                edge_517_550_phi_759_ = _e335;
                                edge_517_550_phi_760_ = _e128;
                                edge_517_550_phi_761_ = _e130;
                                edge_517_550_phi_762_ = _e132;
                                edge_517_550_phi_763_ = _e134;
                                edge_517_550_phi_764_ = _e136;
                                edge_517_550_phi_765_ = _e138;
                                edge_517_550_phi_766_ = _e140;
                                edge_517_550_phi_767_ = _e142;
                                edge_517_550_phi_768_ = _e144;
                                let _e985 = edge_517_550_phi_753_;
                                let _e987 = edge_517_550_phi_754_;
                                let _e989 = edge_517_550_phi_755_;
                                let _e991 = edge_517_550_phi_756_;
                                let _e993 = edge_517_550_phi_757_;
                                let _e995 = edge_517_550_phi_758_;
                                let _e997 = edge_517_550_phi_759_;
                                let _e999 = edge_517_550_phi_760_;
                                let _e1001 = edge_517_550_phi_761_;
                                let _e1003 = edge_517_550_phi_762_;
                                let _e1005 = edge_517_550_phi_763_;
                                let _e1007 = edge_517_550_phi_764_;
                                let _e1009 = edge_517_550_phi_765_;
                                let _e1011 = edge_517_550_phi_766_;
                                let _e1013 = edge_517_550_phi_767_;
                                let _e1015 = edge_517_550_phi_768_;
                                phi_753_ = _e985;
                                phi_754_ = _e987;
                                phi_755_ = _e989;
                                phi_756_ = _e991;
                                phi_757_ = _e993;
                                phi_758_ = _e995;
                                phi_759_ = _e997;
                                phi_760_ = _e999;
                                phi_761_ = _e1001;
                                phi_762_ = _e1003;
                                phi_763_ = _e1005;
                                phi_764_ = _e1007;
                                phi_765_ = _e1009;
                                phi_766_ = _e1011;
                                phi_767_ = _e1013;
                                phi_768_ = _e1015;
                            }
                        } else {
                            if (bitcast<i32>((_e891 - 2u)) < bitcast<i32>(1u)) {
                                edge_520_550_phi_753_ = _e114;
                                edge_520_550_phi_754_ = _e116;
                                edge_520_550_phi_755_ = _e118;
                                edge_520_550_phi_756_ = _e120;
                                edge_520_550_phi_757_ = _e122;
                                edge_520_550_phi_758_ = _e335;
                                edge_520_550_phi_759_ = _e126;
                                edge_520_550_phi_760_ = _e128;
                                edge_520_550_phi_761_ = _e130;
                                edge_520_550_phi_762_ = _e132;
                                edge_520_550_phi_763_ = _e134;
                                edge_520_550_phi_764_ = _e136;
                                edge_520_550_phi_765_ = _e138;
                                edge_520_550_phi_766_ = _e140;
                                edge_520_550_phi_767_ = _e142;
                                edge_520_550_phi_768_ = _e144;
                                let _e1055 = edge_520_550_phi_753_;
                                let _e1057 = edge_520_550_phi_754_;
                                let _e1059 = edge_520_550_phi_755_;
                                let _e1061 = edge_520_550_phi_756_;
                                let _e1063 = edge_520_550_phi_757_;
                                let _e1065 = edge_520_550_phi_758_;
                                let _e1067 = edge_520_550_phi_759_;
                                let _e1069 = edge_520_550_phi_760_;
                                let _e1071 = edge_520_550_phi_761_;
                                let _e1073 = edge_520_550_phi_762_;
                                let _e1075 = edge_520_550_phi_763_;
                                let _e1077 = edge_520_550_phi_764_;
                                let _e1079 = edge_520_550_phi_765_;
                                let _e1081 = edge_520_550_phi_766_;
                                let _e1083 = edge_520_550_phi_767_;
                                let _e1085 = edge_520_550_phi_768_;
                                phi_753_ = _e1055;
                                phi_754_ = _e1057;
                                phi_755_ = _e1059;
                                phi_756_ = _e1061;
                                phi_757_ = _e1063;
                                phi_758_ = _e1065;
                                phi_759_ = _e1067;
                                phi_760_ = _e1069;
                                phi_761_ = _e1071;
                                phi_762_ = _e1073;
                                phi_763_ = _e1075;
                                phi_764_ = _e1077;
                                phi_765_ = _e1079;
                                phi_766_ = _e1081;
                                phi_767_ = _e1083;
                                phi_768_ = _e1085;
                            } else {
                                edge_524_550_phi_753_ = _e114;
                                edge_524_550_phi_754_ = _e116;
                                edge_524_550_phi_755_ = _e118;
                                edge_524_550_phi_756_ = _e120;
                                edge_524_550_phi_757_ = _e335;
                                edge_524_550_phi_758_ = _e124;
                                edge_524_550_phi_759_ = _e126;
                                edge_524_550_phi_760_ = _e128;
                                edge_524_550_phi_761_ = _e130;
                                edge_524_550_phi_762_ = _e132;
                                edge_524_550_phi_763_ = _e134;
                                edge_524_550_phi_764_ = _e136;
                                edge_524_550_phi_765_ = _e138;
                                edge_524_550_phi_766_ = _e140;
                                edge_524_550_phi_767_ = _e142;
                                edge_524_550_phi_768_ = _e144;
                                let _e1119 = edge_524_550_phi_753_;
                                let _e1121 = edge_524_550_phi_754_;
                                let _e1123 = edge_524_550_phi_755_;
                                let _e1125 = edge_524_550_phi_756_;
                                let _e1127 = edge_524_550_phi_757_;
                                let _e1129 = edge_524_550_phi_758_;
                                let _e1131 = edge_524_550_phi_759_;
                                let _e1133 = edge_524_550_phi_760_;
                                let _e1135 = edge_524_550_phi_761_;
                                let _e1137 = edge_524_550_phi_762_;
                                let _e1139 = edge_524_550_phi_763_;
                                let _e1141 = edge_524_550_phi_764_;
                                let _e1143 = edge_524_550_phi_765_;
                                let _e1145 = edge_524_550_phi_766_;
                                let _e1147 = edge_524_550_phi_767_;
                                let _e1149 = edge_524_550_phi_768_;
                                phi_753_ = _e1119;
                                phi_754_ = _e1121;
                                phi_755_ = _e1123;
                                phi_756_ = _e1125;
                                phi_757_ = _e1127;
                                phi_758_ = _e1129;
                                phi_759_ = _e1131;
                                phi_760_ = _e1133;
                                phi_761_ = _e1135;
                                phi_762_ = _e1137;
                                phi_763_ = _e1139;
                                phi_764_ = _e1141;
                                phi_765_ = _e1143;
                                phi_766_ = _e1145;
                                phi_767_ = _e1147;
                                phi_768_ = _e1149;
                            }
                        }
                    } else {
                        let _e1167 = (_e891 - 4u);
                        if (bitcast<i32>(_e1167) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e1167) < bitcast<i32>(1u)) {
                                edge_534_550_phi_753_ = _e114;
                                edge_534_550_phi_754_ = _e116;
                                edge_534_550_phi_755_ = _e118;
                                edge_534_550_phi_756_ = _e335;
                                edge_534_550_phi_757_ = _e122;
                                edge_534_550_phi_758_ = _e124;
                                edge_534_550_phi_759_ = _e126;
                                edge_534_550_phi_760_ = _e128;
                                edge_534_550_phi_761_ = _e130;
                                edge_534_550_phi_762_ = _e132;
                                edge_534_550_phi_763_ = _e134;
                                edge_534_550_phi_764_ = _e136;
                                edge_534_550_phi_765_ = _e138;
                                edge_534_550_phi_766_ = _e140;
                                edge_534_550_phi_767_ = _e142;
                                edge_534_550_phi_768_ = _e144;
                                let _e1193 = edge_534_550_phi_753_;
                                let _e1195 = edge_534_550_phi_754_;
                                let _e1197 = edge_534_550_phi_755_;
                                let _e1199 = edge_534_550_phi_756_;
                                let _e1201 = edge_534_550_phi_757_;
                                let _e1203 = edge_534_550_phi_758_;
                                let _e1205 = edge_534_550_phi_759_;
                                let _e1207 = edge_534_550_phi_760_;
                                let _e1209 = edge_534_550_phi_761_;
                                let _e1211 = edge_534_550_phi_762_;
                                let _e1213 = edge_534_550_phi_763_;
                                let _e1215 = edge_534_550_phi_764_;
                                let _e1217 = edge_534_550_phi_765_;
                                let _e1219 = edge_534_550_phi_766_;
                                let _e1221 = edge_534_550_phi_767_;
                                let _e1223 = edge_534_550_phi_768_;
                                phi_753_ = _e1193;
                                phi_754_ = _e1195;
                                phi_755_ = _e1197;
                                phi_756_ = _e1199;
                                phi_757_ = _e1201;
                                phi_758_ = _e1203;
                                phi_759_ = _e1205;
                                phi_760_ = _e1207;
                                phi_761_ = _e1209;
                                phi_762_ = _e1211;
                                phi_763_ = _e1213;
                                phi_764_ = _e1215;
                                phi_765_ = _e1217;
                                phi_766_ = _e1219;
                                phi_767_ = _e1221;
                                phi_768_ = _e1223;
                            } else {
                                edge_536_550_phi_753_ = _e114;
                                edge_536_550_phi_754_ = _e116;
                                edge_536_550_phi_755_ = _e335;
                                edge_536_550_phi_756_ = _e120;
                                edge_536_550_phi_757_ = _e122;
                                edge_536_550_phi_758_ = _e124;
                                edge_536_550_phi_759_ = _e126;
                                edge_536_550_phi_760_ = _e128;
                                edge_536_550_phi_761_ = _e130;
                                edge_536_550_phi_762_ = _e132;
                                edge_536_550_phi_763_ = _e134;
                                edge_536_550_phi_764_ = _e136;
                                edge_536_550_phi_765_ = _e138;
                                edge_536_550_phi_766_ = _e140;
                                edge_536_550_phi_767_ = _e142;
                                edge_536_550_phi_768_ = _e144;
                                let _e1257 = edge_536_550_phi_753_;
                                let _e1259 = edge_536_550_phi_754_;
                                let _e1261 = edge_536_550_phi_755_;
                                let _e1263 = edge_536_550_phi_756_;
                                let _e1265 = edge_536_550_phi_757_;
                                let _e1267 = edge_536_550_phi_758_;
                                let _e1269 = edge_536_550_phi_759_;
                                let _e1271 = edge_536_550_phi_760_;
                                let _e1273 = edge_536_550_phi_761_;
                                let _e1275 = edge_536_550_phi_762_;
                                let _e1277 = edge_536_550_phi_763_;
                                let _e1279 = edge_536_550_phi_764_;
                                let _e1281 = edge_536_550_phi_765_;
                                let _e1283 = edge_536_550_phi_766_;
                                let _e1285 = edge_536_550_phi_767_;
                                let _e1287 = edge_536_550_phi_768_;
                                phi_753_ = _e1257;
                                phi_754_ = _e1259;
                                phi_755_ = _e1261;
                                phi_756_ = _e1263;
                                phi_757_ = _e1265;
                                phi_758_ = _e1267;
                                phi_759_ = _e1269;
                                phi_760_ = _e1271;
                                phi_761_ = _e1273;
                                phi_762_ = _e1275;
                                phi_763_ = _e1277;
                                phi_764_ = _e1279;
                                phi_765_ = _e1281;
                                phi_766_ = _e1283;
                                phi_767_ = _e1285;
                                phi_768_ = _e1287;
                            }
                        } else {
                            if (bitcast<i32>((_e1167 - 2u)) < bitcast<i32>(1u)) {
                                edge_539_550_phi_753_ = _e114;
                                edge_539_550_phi_754_ = _e335;
                                edge_539_550_phi_755_ = _e118;
                                edge_539_550_phi_756_ = _e120;
                                edge_539_550_phi_757_ = _e122;
                                edge_539_550_phi_758_ = _e124;
                                edge_539_550_phi_759_ = _e126;
                                edge_539_550_phi_760_ = _e128;
                                edge_539_550_phi_761_ = _e130;
                                edge_539_550_phi_762_ = _e132;
                                edge_539_550_phi_763_ = _e134;
                                edge_539_550_phi_764_ = _e136;
                                edge_539_550_phi_765_ = _e138;
                                edge_539_550_phi_766_ = _e140;
                                edge_539_550_phi_767_ = _e142;
                                edge_539_550_phi_768_ = _e144;
                                let _e1327 = edge_539_550_phi_753_;
                                let _e1329 = edge_539_550_phi_754_;
                                let _e1331 = edge_539_550_phi_755_;
                                let _e1333 = edge_539_550_phi_756_;
                                let _e1335 = edge_539_550_phi_757_;
                                let _e1337 = edge_539_550_phi_758_;
                                let _e1339 = edge_539_550_phi_759_;
                                let _e1341 = edge_539_550_phi_760_;
                                let _e1343 = edge_539_550_phi_761_;
                                let _e1345 = edge_539_550_phi_762_;
                                let _e1347 = edge_539_550_phi_763_;
                                let _e1349 = edge_539_550_phi_764_;
                                let _e1351 = edge_539_550_phi_765_;
                                let _e1353 = edge_539_550_phi_766_;
                                let _e1355 = edge_539_550_phi_767_;
                                let _e1357 = edge_539_550_phi_768_;
                                phi_753_ = _e1327;
                                phi_754_ = _e1329;
                                phi_755_ = _e1331;
                                phi_756_ = _e1333;
                                phi_757_ = _e1335;
                                phi_758_ = _e1337;
                                phi_759_ = _e1339;
                                phi_760_ = _e1341;
                                phi_761_ = _e1343;
                                phi_762_ = _e1345;
                                phi_763_ = _e1347;
                                phi_764_ = _e1349;
                                phi_765_ = _e1351;
                                phi_766_ = _e1353;
                                phi_767_ = _e1355;
                                phi_768_ = _e1357;
                            } else {
                                edge_543_550_phi_753_ = _e335;
                                edge_543_550_phi_754_ = _e116;
                                edge_543_550_phi_755_ = _e118;
                                edge_543_550_phi_756_ = _e120;
                                edge_543_550_phi_757_ = _e122;
                                edge_543_550_phi_758_ = _e124;
                                edge_543_550_phi_759_ = _e126;
                                edge_543_550_phi_760_ = _e128;
                                edge_543_550_phi_761_ = _e130;
                                edge_543_550_phi_762_ = _e132;
                                edge_543_550_phi_763_ = _e134;
                                edge_543_550_phi_764_ = _e136;
                                edge_543_550_phi_765_ = _e138;
                                edge_543_550_phi_766_ = _e140;
                                edge_543_550_phi_767_ = _e142;
                                edge_543_550_phi_768_ = _e144;
                                let _e1391 = edge_543_550_phi_753_;
                                let _e1393 = edge_543_550_phi_754_;
                                let _e1395 = edge_543_550_phi_755_;
                                let _e1397 = edge_543_550_phi_756_;
                                let _e1399 = edge_543_550_phi_757_;
                                let _e1401 = edge_543_550_phi_758_;
                                let _e1403 = edge_543_550_phi_759_;
                                let _e1405 = edge_543_550_phi_760_;
                                let _e1407 = edge_543_550_phi_761_;
                                let _e1409 = edge_543_550_phi_762_;
                                let _e1411 = edge_543_550_phi_763_;
                                let _e1413 = edge_543_550_phi_764_;
                                let _e1415 = edge_543_550_phi_765_;
                                let _e1417 = edge_543_550_phi_766_;
                                let _e1419 = edge_543_550_phi_767_;
                                let _e1421 = edge_543_550_phi_768_;
                                phi_753_ = _e1391;
                                phi_754_ = _e1393;
                                phi_755_ = _e1395;
                                phi_756_ = _e1397;
                                phi_757_ = _e1399;
                                phi_758_ = _e1401;
                                phi_759_ = _e1403;
                                phi_760_ = _e1405;
                                phi_761_ = _e1407;
                                phi_762_ = _e1409;
                                phi_763_ = _e1411;
                                phi_764_ = _e1413;
                                phi_765_ = _e1415;
                                phi_766_ = _e1417;
                                phi_767_ = _e1419;
                                phi_768_ = _e1421;
                            }
                        }
                    }
                }
                let _e1439 = phi_753_;
                let _e1441 = phi_754_;
                let _e1443 = phi_755_;
                let _e1445 = phi_756_;
                let _e1447 = phi_757_;
                let _e1449 = phi_758_;
                let _e1451 = phi_759_;
                let _e1453 = phi_760_;
                let _e1455 = phi_761_;
                let _e1457 = phi_762_;
                let _e1459 = phi_763_;
                let _e1461 = phi_764_;
                let _e1463 = phi_765_;
                let _e1465 = phi_766_;
                let _e1467 = phi_767_;
                let _e1469 = phi_768_;
                edge_550_414_phi_552_ = _e1439;
                edge_550_414_phi_554_ = _e1441;
                edge_550_414_phi_556_ = _e1443;
                edge_550_414_phi_558_ = _e1445;
                edge_550_414_phi_560_ = _e1447;
                edge_550_414_phi_562_ = _e1449;
                edge_550_414_phi_564_ = _e1451;
                edge_550_414_phi_566_ = _e1453;
                edge_550_414_phi_568_ = _e1455;
                edge_550_414_phi_570_ = _e1457;
                edge_550_414_phi_572_ = _e1459;
                edge_550_414_phi_574_ = _e1461;
                edge_550_414_phi_576_ = _e1463;
                edge_550_414_phi_578_ = _e1465;
                edge_550_414_phi_580_ = _e1467;
                edge_550_414_phi_582_ = _e1469;
                edge_550_414_phi_584_ = (_e146 + 1u);
                let _e1490 = edge_550_414_phi_552_;
                let _e1492 = edge_550_414_phi_554_;
                let _e1494 = edge_550_414_phi_556_;
                let _e1496 = edge_550_414_phi_558_;
                let _e1498 = edge_550_414_phi_560_;
                let _e1500 = edge_550_414_phi_562_;
                let _e1502 = edge_550_414_phi_564_;
                let _e1504 = edge_550_414_phi_566_;
                let _e1506 = edge_550_414_phi_568_;
                let _e1508 = edge_550_414_phi_570_;
                let _e1510 = edge_550_414_phi_572_;
                let _e1512 = edge_550_414_phi_574_;
                let _e1514 = edge_550_414_phi_576_;
                let _e1516 = edge_550_414_phi_578_;
                let _e1518 = edge_550_414_phi_580_;
                let _e1520 = edge_550_414_phi_582_;
                let _e1522 = edge_550_414_phi_584_;
                phi_552_ = _e1490;
                phi_554_ = _e1492;
                phi_556_ = _e1494;
                phi_558_ = _e1496;
                phi_560_ = _e1498;
                phi_562_ = _e1500;
                phi_564_ = _e1502;
                phi_566_ = _e1504;
                phi_568_ = _e1506;
                phi_570_ = _e1508;
                phi_572_ = _e1510;
                phi_574_ = _e1512;
                phi_576_ = _e1514;
                phi_578_ = _e1516;
                phi_580_ = _e1518;
                phi_582_ = _e1520;
                phi_584_ = _e1522;
                continue;
            } else {
                edge_414_696_phi_771_ = 0f;
                edge_414_696_phi_773_ = 0f;
                edge_414_696_phi_775_ = 0f;
                edge_414_696_phi_777_ = 0f;
                edge_414_696_phi_779_ = 0f;
                edge_414_696_phi_781_ = 0f;
                edge_414_696_phi_783_ = 0f;
                edge_414_696_phi_785_ = 0f;
                edge_414_696_phi_787_ = 0f;
                edge_414_696_phi_789_ = 0f;
                edge_414_696_phi_791_ = 0f;
                edge_414_696_phi_793_ = 0f;
                edge_414_696_phi_795_ = 0f;
                edge_414_696_phi_797_ = 0f;
                edge_414_696_phi_799_ = 0f;
                edge_414_696_phi_801_ = 0f;
                edge_414_696_phi_803_ = 0u;
                let _e1575 = edge_414_696_phi_771_;
                let _e1577 = edge_414_696_phi_773_;
                let _e1579 = edge_414_696_phi_775_;
                let _e1581 = edge_414_696_phi_777_;
                let _e1583 = edge_414_696_phi_779_;
                let _e1585 = edge_414_696_phi_781_;
                let _e1587 = edge_414_696_phi_783_;
                let _e1589 = edge_414_696_phi_785_;
                let _e1591 = edge_414_696_phi_787_;
                let _e1593 = edge_414_696_phi_789_;
                let _e1595 = edge_414_696_phi_791_;
                let _e1597 = edge_414_696_phi_793_;
                let _e1599 = edge_414_696_phi_795_;
                let _e1601 = edge_414_696_phi_797_;
                let _e1603 = edge_414_696_phi_799_;
                let _e1605 = edge_414_696_phi_801_;
                let _e1607 = edge_414_696_phi_803_;
                phi_771_ = _e1575;
                phi_773_ = _e1577;
                phi_775_ = _e1579;
                phi_777_ = _e1581;
                phi_779_ = _e1583;
                phi_781_ = _e1585;
                phi_783_ = _e1587;
                phi_785_ = _e1589;
                phi_787_ = _e1591;
                phi_789_ = _e1593;
                phi_791_ = _e1595;
                phi_793_ = _e1597;
                phi_795_ = _e1599;
                phi_797_ = _e1601;
                phi_799_ = _e1603;
                phi_801_ = _e1605;
                phi_803_ = _e1607;
                loop_header_carry_586_ = _e150;
                break;
            }
        }
        let _e1627 = phi_552_;
        let _e1629 = phi_554_;
        let _e1631 = phi_556_;
        let _e1633 = phi_558_;
        let _e1635 = phi_560_;
        let _e1637 = phi_562_;
        let _e1639 = phi_564_;
        let _e1641 = phi_566_;
        let _e1643 = phi_568_;
        let _e1645 = phi_570_;
        let _e1647 = phi_572_;
        let _e1649 = phi_574_;
        let _e1651 = phi_576_;
        let _e1653 = phi_578_;
        let _e1655 = phi_580_;
        let _e1657 = phi_582_;
        edge_414_696_phi_771_1 = 0f;
        edge_414_696_phi_773_1 = 0f;
        edge_414_696_phi_775_1 = 0f;
        edge_414_696_phi_777_1 = 0f;
        edge_414_696_phi_779_1 = 0f;
        edge_414_696_phi_781_1 = 0f;
        edge_414_696_phi_783_1 = 0f;
        edge_414_696_phi_785_1 = 0f;
        edge_414_696_phi_787_1 = 0f;
        edge_414_696_phi_789_1 = 0f;
        edge_414_696_phi_791_1 = 0f;
        edge_414_696_phi_793_1 = 0f;
        edge_414_696_phi_795_1 = 0f;
        edge_414_696_phi_797_1 = 0f;
        edge_414_696_phi_799_1 = 0f;
        edge_414_696_phi_801_1 = 0f;
        edge_414_696_phi_803_1 = 0u;
        let _e1697 = edge_414_696_phi_771_1;
        let _e1699 = edge_414_696_phi_773_1;
        let _e1701 = edge_414_696_phi_775_1;
        let _e1703 = edge_414_696_phi_777_1;
        let _e1705 = edge_414_696_phi_779_1;
        let _e1707 = edge_414_696_phi_781_1;
        let _e1709 = edge_414_696_phi_783_1;
        let _e1711 = edge_414_696_phi_785_1;
        let _e1713 = edge_414_696_phi_787_1;
        let _e1715 = edge_414_696_phi_789_1;
        let _e1717 = edge_414_696_phi_791_1;
        let _e1719 = edge_414_696_phi_793_1;
        let _e1721 = edge_414_696_phi_795_1;
        let _e1723 = edge_414_696_phi_797_1;
        let _e1725 = edge_414_696_phi_799_1;
        let _e1727 = edge_414_696_phi_801_1;
        let _e1729 = edge_414_696_phi_803_1;
        phi_771_ = _e1697;
        phi_773_ = _e1699;
        phi_775_ = _e1701;
        phi_777_ = _e1703;
        phi_779_ = _e1705;
        phi_781_ = _e1707;
        phi_783_ = _e1709;
        phi_785_ = _e1711;
        phi_787_ = _e1713;
        phi_789_ = _e1715;
        phi_791_ = _e1717;
        phi_793_ = _e1719;
        phi_795_ = _e1721;
        phi_797_ = _e1723;
        phi_799_ = _e1725;
        phi_801_ = _e1727;
        phi_803_ = _e1729;
        loop {
            let _e1749 = phi_771_;
            let _e1751 = phi_773_;
            let _e1753 = phi_775_;
            let _e1755 = phi_777_;
            let _e1757 = phi_779_;
            let _e1759 = phi_781_;
            let _e1761 = phi_783_;
            let _e1763 = phi_785_;
            let _e1765 = phi_787_;
            let _e1767 = phi_789_;
            let _e1769 = phi_791_;
            let _e1771 = phi_793_;
            let _e1773 = phi_795_;
            let _e1775 = phi_797_;
            let _e1777 = phi_799_;
            let _e1779 = phi_801_;
            let _e1781 = phi_803_;
            let _e1785 = (bitcast<i32>(_e1781) < bitcast<i32>(12u));
            if _e1785 {
                let _e1789 = (bitcast<i32>(_e1781) < bitcast<i32>(8u));
                if _e1789 {
                    if (bitcast<i32>(_e1781) < bitcast<i32>(4u)) {
                        if (bitcast<i32>(_e1781) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e1781) < bitcast<i32>(1u)) {
                                edge_708_786_phi_864_ = _e1657;
                                let _e1804 = edge_708_786_phi_864_;
                                phi_864_ = _e1804;
                            } else {
                                edge_710_786_phi_864_ = _e1655;
                                let _e1808 = edge_710_786_phi_864_;
                                phi_864_ = _e1808;
                            }
                        } else {
                            if (bitcast<i32>((_e1781 - 2u)) < bitcast<i32>(1u)) {
                                edge_713_786_phi_864_ = _e1653;
                                let _e1818 = edge_713_786_phi_864_;
                                phi_864_ = _e1818;
                            } else {
                                edge_717_786_phi_864_ = _e1651;
                                let _e1822 = edge_717_786_phi_864_;
                                phi_864_ = _e1822;
                            }
                        }
                    } else {
                        let _e1825 = (_e1781 - 4u);
                        if (bitcast<i32>(_e1825) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e1825) < bitcast<i32>(1u)) {
                                edge_727_786_phi_864_ = _e1649;
                                let _e1836 = edge_727_786_phi_864_;
                                phi_864_ = _e1836;
                            } else {
                                edge_729_786_phi_864_ = _e1647;
                                let _e1840 = edge_729_786_phi_864_;
                                phi_864_ = _e1840;
                            }
                        } else {
                            if (bitcast<i32>((_e1825 - 2u)) < bitcast<i32>(1u)) {
                                edge_732_786_phi_864_ = _e1645;
                                let _e1850 = edge_732_786_phi_864_;
                                phi_864_ = _e1850;
                            } else {
                                edge_736_786_phi_864_ = _e1643;
                                let _e1854 = edge_736_786_phi_864_;
                                phi_864_ = _e1854;
                            }
                        }
                    }
                } else {
                    let _e1857 = (_e1781 - 8u);
                    if (bitcast<i32>(_e1857) < bitcast<i32>(4u)) {
                        if (bitcast<i32>(_e1857) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e1857) < bitcast<i32>(1u)) {
                                edge_751_786_phi_864_ = _e1641;
                                let _e1872 = edge_751_786_phi_864_;
                                phi_864_ = _e1872;
                            } else {
                                edge_753_786_phi_864_ = _e1639;
                                let _e1876 = edge_753_786_phi_864_;
                                phi_864_ = _e1876;
                            }
                        } else {
                            if (bitcast<i32>((_e1857 - 2u)) < bitcast<i32>(1u)) {
                                edge_756_786_phi_864_ = _e1637;
                                let _e1886 = edge_756_786_phi_864_;
                                phi_864_ = _e1886;
                            } else {
                                edge_760_786_phi_864_ = _e1635;
                                let _e1890 = edge_760_786_phi_864_;
                                phi_864_ = _e1890;
                            }
                        }
                    } else {
                        let _e1893 = (_e1857 - 4u);
                        if (bitcast<i32>(_e1893) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e1893) < bitcast<i32>(1u)) {
                                edge_770_786_phi_864_ = _e1633;
                                let _e1904 = edge_770_786_phi_864_;
                                phi_864_ = _e1904;
                            } else {
                                edge_772_786_phi_864_ = _e1631;
                                let _e1908 = edge_772_786_phi_864_;
                                phi_864_ = _e1908;
                            }
                        } else {
                            if (bitcast<i32>((_e1893 - 2u)) < bitcast<i32>(1u)) {
                                edge_775_786_phi_864_ = _e1629;
                                let _e1918 = edge_775_786_phi_864_;
                                phi_864_ = _e1918;
                            } else {
                                edge_779_786_phi_864_ = _e1627;
                                let _e1922 = edge_779_786_phi_864_;
                                phi_864_ = _e1922;
                            }
                        }
                    }
                }
                let _e1925 = phi_864_;
                let _e1927 = sqrt(3f);
                if (bitcast<i32>(_e1781) < bitcast<i32>(6u)) {
                    edge_802_795_phi_881_ = (_e1927 / 3f);
                    let _e1938 = edge_802_795_phi_881_;
                    phi_881_ = _e1938;
                } else {
                    edge_803_795_phi_881_ = (_e1927 / 6f);
                    let _e1942 = edge_803_795_phi_881_;
                    phi_881_ = _e1942;
                }
                let _e1945 = phi_881_;
                let _e1946 = (_e1925 * _e1945);
                if _e1789 {
                    if (bitcast<i32>(_e1781) < bitcast<i32>(4u)) {
                        if (bitcast<i32>(_e1781) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e1781) < bitcast<i32>(1u)) {
                                edge_816_894_phi_976_ = _e1749;
                                edge_816_894_phi_977_ = _e1751;
                                edge_816_894_phi_978_ = _e1753;
                                edge_816_894_phi_979_ = _e1755;
                                edge_816_894_phi_980_ = _e1757;
                                edge_816_894_phi_981_ = _e1759;
                                edge_816_894_phi_982_ = _e1761;
                                edge_816_894_phi_983_ = _e1763;
                                edge_816_894_phi_984_ = _e1765;
                                edge_816_894_phi_985_ = _e1767;
                                edge_816_894_phi_986_ = _e1769;
                                edge_816_894_phi_987_ = _e1771;
                                edge_816_894_phi_988_ = _e1773;
                                edge_816_894_phi_989_ = _e1775;
                                edge_816_894_phi_990_ = _e1777;
                                edge_816_894_phi_991_ = _e1946;
                                let _e1976 = edge_816_894_phi_976_;
                                let _e1978 = edge_816_894_phi_977_;
                                let _e1980 = edge_816_894_phi_978_;
                                let _e1982 = edge_816_894_phi_979_;
                                let _e1984 = edge_816_894_phi_980_;
                                let _e1986 = edge_816_894_phi_981_;
                                let _e1988 = edge_816_894_phi_982_;
                                let _e1990 = edge_816_894_phi_983_;
                                let _e1992 = edge_816_894_phi_984_;
                                let _e1994 = edge_816_894_phi_985_;
                                let _e1996 = edge_816_894_phi_986_;
                                let _e1998 = edge_816_894_phi_987_;
                                let _e2000 = edge_816_894_phi_988_;
                                let _e2002 = edge_816_894_phi_989_;
                                let _e2004 = edge_816_894_phi_990_;
                                let _e2006 = edge_816_894_phi_991_;
                                phi_976_ = _e1976;
                                phi_977_ = _e1978;
                                phi_978_ = _e1980;
                                phi_979_ = _e1982;
                                phi_980_ = _e1984;
                                phi_981_ = _e1986;
                                phi_982_ = _e1988;
                                phi_983_ = _e1990;
                                phi_984_ = _e1992;
                                phi_985_ = _e1994;
                                phi_986_ = _e1996;
                                phi_987_ = _e1998;
                                phi_988_ = _e2000;
                                phi_989_ = _e2002;
                                phi_990_ = _e2004;
                                phi_991_ = _e2006;
                            } else {
                                edge_818_894_phi_976_ = _e1749;
                                edge_818_894_phi_977_ = _e1751;
                                edge_818_894_phi_978_ = _e1753;
                                edge_818_894_phi_979_ = _e1755;
                                edge_818_894_phi_980_ = _e1757;
                                edge_818_894_phi_981_ = _e1759;
                                edge_818_894_phi_982_ = _e1761;
                                edge_818_894_phi_983_ = _e1763;
                                edge_818_894_phi_984_ = _e1765;
                                edge_818_894_phi_985_ = _e1767;
                                edge_818_894_phi_986_ = _e1769;
                                edge_818_894_phi_987_ = _e1771;
                                edge_818_894_phi_988_ = _e1773;
                                edge_818_894_phi_989_ = _e1775;
                                edge_818_894_phi_990_ = _e1946;
                                edge_818_894_phi_991_ = _e1779;
                                let _e2040 = edge_818_894_phi_976_;
                                let _e2042 = edge_818_894_phi_977_;
                                let _e2044 = edge_818_894_phi_978_;
                                let _e2046 = edge_818_894_phi_979_;
                                let _e2048 = edge_818_894_phi_980_;
                                let _e2050 = edge_818_894_phi_981_;
                                let _e2052 = edge_818_894_phi_982_;
                                let _e2054 = edge_818_894_phi_983_;
                                let _e2056 = edge_818_894_phi_984_;
                                let _e2058 = edge_818_894_phi_985_;
                                let _e2060 = edge_818_894_phi_986_;
                                let _e2062 = edge_818_894_phi_987_;
                                let _e2064 = edge_818_894_phi_988_;
                                let _e2066 = edge_818_894_phi_989_;
                                let _e2068 = edge_818_894_phi_990_;
                                let _e2070 = edge_818_894_phi_991_;
                                phi_976_ = _e2040;
                                phi_977_ = _e2042;
                                phi_978_ = _e2044;
                                phi_979_ = _e2046;
                                phi_980_ = _e2048;
                                phi_981_ = _e2050;
                                phi_982_ = _e2052;
                                phi_983_ = _e2054;
                                phi_984_ = _e2056;
                                phi_985_ = _e2058;
                                phi_986_ = _e2060;
                                phi_987_ = _e2062;
                                phi_988_ = _e2064;
                                phi_989_ = _e2066;
                                phi_990_ = _e2068;
                                phi_991_ = _e2070;
                            }
                        } else {
                            if (bitcast<i32>((_e1781 - 2u)) < bitcast<i32>(1u)) {
                                edge_821_894_phi_976_ = _e1749;
                                edge_821_894_phi_977_ = _e1751;
                                edge_821_894_phi_978_ = _e1753;
                                edge_821_894_phi_979_ = _e1755;
                                edge_821_894_phi_980_ = _e1757;
                                edge_821_894_phi_981_ = _e1759;
                                edge_821_894_phi_982_ = _e1761;
                                edge_821_894_phi_983_ = _e1763;
                                edge_821_894_phi_984_ = _e1765;
                                edge_821_894_phi_985_ = _e1767;
                                edge_821_894_phi_986_ = _e1769;
                                edge_821_894_phi_987_ = _e1771;
                                edge_821_894_phi_988_ = _e1773;
                                edge_821_894_phi_989_ = _e1946;
                                edge_821_894_phi_990_ = _e1777;
                                edge_821_894_phi_991_ = _e1779;
                                let _e2110 = edge_821_894_phi_976_;
                                let _e2112 = edge_821_894_phi_977_;
                                let _e2114 = edge_821_894_phi_978_;
                                let _e2116 = edge_821_894_phi_979_;
                                let _e2118 = edge_821_894_phi_980_;
                                let _e2120 = edge_821_894_phi_981_;
                                let _e2122 = edge_821_894_phi_982_;
                                let _e2124 = edge_821_894_phi_983_;
                                let _e2126 = edge_821_894_phi_984_;
                                let _e2128 = edge_821_894_phi_985_;
                                let _e2130 = edge_821_894_phi_986_;
                                let _e2132 = edge_821_894_phi_987_;
                                let _e2134 = edge_821_894_phi_988_;
                                let _e2136 = edge_821_894_phi_989_;
                                let _e2138 = edge_821_894_phi_990_;
                                let _e2140 = edge_821_894_phi_991_;
                                phi_976_ = _e2110;
                                phi_977_ = _e2112;
                                phi_978_ = _e2114;
                                phi_979_ = _e2116;
                                phi_980_ = _e2118;
                                phi_981_ = _e2120;
                                phi_982_ = _e2122;
                                phi_983_ = _e2124;
                                phi_984_ = _e2126;
                                phi_985_ = _e2128;
                                phi_986_ = _e2130;
                                phi_987_ = _e2132;
                                phi_988_ = _e2134;
                                phi_989_ = _e2136;
                                phi_990_ = _e2138;
                                phi_991_ = _e2140;
                            } else {
                                edge_825_894_phi_976_ = _e1749;
                                edge_825_894_phi_977_ = _e1751;
                                edge_825_894_phi_978_ = _e1753;
                                edge_825_894_phi_979_ = _e1755;
                                edge_825_894_phi_980_ = _e1757;
                                edge_825_894_phi_981_ = _e1759;
                                edge_825_894_phi_982_ = _e1761;
                                edge_825_894_phi_983_ = _e1763;
                                edge_825_894_phi_984_ = _e1765;
                                edge_825_894_phi_985_ = _e1767;
                                edge_825_894_phi_986_ = _e1769;
                                edge_825_894_phi_987_ = _e1771;
                                edge_825_894_phi_988_ = _e1946;
                                edge_825_894_phi_989_ = _e1775;
                                edge_825_894_phi_990_ = _e1777;
                                edge_825_894_phi_991_ = _e1779;
                                let _e2174 = edge_825_894_phi_976_;
                                let _e2176 = edge_825_894_phi_977_;
                                let _e2178 = edge_825_894_phi_978_;
                                let _e2180 = edge_825_894_phi_979_;
                                let _e2182 = edge_825_894_phi_980_;
                                let _e2184 = edge_825_894_phi_981_;
                                let _e2186 = edge_825_894_phi_982_;
                                let _e2188 = edge_825_894_phi_983_;
                                let _e2190 = edge_825_894_phi_984_;
                                let _e2192 = edge_825_894_phi_985_;
                                let _e2194 = edge_825_894_phi_986_;
                                let _e2196 = edge_825_894_phi_987_;
                                let _e2198 = edge_825_894_phi_988_;
                                let _e2200 = edge_825_894_phi_989_;
                                let _e2202 = edge_825_894_phi_990_;
                                let _e2204 = edge_825_894_phi_991_;
                                phi_976_ = _e2174;
                                phi_977_ = _e2176;
                                phi_978_ = _e2178;
                                phi_979_ = _e2180;
                                phi_980_ = _e2182;
                                phi_981_ = _e2184;
                                phi_982_ = _e2186;
                                phi_983_ = _e2188;
                                phi_984_ = _e2190;
                                phi_985_ = _e2192;
                                phi_986_ = _e2194;
                                phi_987_ = _e2196;
                                phi_988_ = _e2198;
                                phi_989_ = _e2200;
                                phi_990_ = _e2202;
                                phi_991_ = _e2204;
                            }
                        }
                    } else {
                        let _e2222 = (_e1781 - 4u);
                        if (bitcast<i32>(_e2222) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e2222) < bitcast<i32>(1u)) {
                                edge_835_894_phi_976_ = _e1749;
                                edge_835_894_phi_977_ = _e1751;
                                edge_835_894_phi_978_ = _e1753;
                                edge_835_894_phi_979_ = _e1755;
                                edge_835_894_phi_980_ = _e1757;
                                edge_835_894_phi_981_ = _e1759;
                                edge_835_894_phi_982_ = _e1761;
                                edge_835_894_phi_983_ = _e1763;
                                edge_835_894_phi_984_ = _e1765;
                                edge_835_894_phi_985_ = _e1767;
                                edge_835_894_phi_986_ = _e1769;
                                edge_835_894_phi_987_ = _e1946;
                                edge_835_894_phi_988_ = _e1773;
                                edge_835_894_phi_989_ = _e1775;
                                edge_835_894_phi_990_ = _e1777;
                                edge_835_894_phi_991_ = _e1779;
                                let _e2248 = edge_835_894_phi_976_;
                                let _e2250 = edge_835_894_phi_977_;
                                let _e2252 = edge_835_894_phi_978_;
                                let _e2254 = edge_835_894_phi_979_;
                                let _e2256 = edge_835_894_phi_980_;
                                let _e2258 = edge_835_894_phi_981_;
                                let _e2260 = edge_835_894_phi_982_;
                                let _e2262 = edge_835_894_phi_983_;
                                let _e2264 = edge_835_894_phi_984_;
                                let _e2266 = edge_835_894_phi_985_;
                                let _e2268 = edge_835_894_phi_986_;
                                let _e2270 = edge_835_894_phi_987_;
                                let _e2272 = edge_835_894_phi_988_;
                                let _e2274 = edge_835_894_phi_989_;
                                let _e2276 = edge_835_894_phi_990_;
                                let _e2278 = edge_835_894_phi_991_;
                                phi_976_ = _e2248;
                                phi_977_ = _e2250;
                                phi_978_ = _e2252;
                                phi_979_ = _e2254;
                                phi_980_ = _e2256;
                                phi_981_ = _e2258;
                                phi_982_ = _e2260;
                                phi_983_ = _e2262;
                                phi_984_ = _e2264;
                                phi_985_ = _e2266;
                                phi_986_ = _e2268;
                                phi_987_ = _e2270;
                                phi_988_ = _e2272;
                                phi_989_ = _e2274;
                                phi_990_ = _e2276;
                                phi_991_ = _e2278;
                            } else {
                                edge_837_894_phi_976_ = _e1749;
                                edge_837_894_phi_977_ = _e1751;
                                edge_837_894_phi_978_ = _e1753;
                                edge_837_894_phi_979_ = _e1755;
                                edge_837_894_phi_980_ = _e1757;
                                edge_837_894_phi_981_ = _e1759;
                                edge_837_894_phi_982_ = _e1761;
                                edge_837_894_phi_983_ = _e1763;
                                edge_837_894_phi_984_ = _e1765;
                                edge_837_894_phi_985_ = _e1767;
                                edge_837_894_phi_986_ = _e1946;
                                edge_837_894_phi_987_ = _e1771;
                                edge_837_894_phi_988_ = _e1773;
                                edge_837_894_phi_989_ = _e1775;
                                edge_837_894_phi_990_ = _e1777;
                                edge_837_894_phi_991_ = _e1779;
                                let _e2312 = edge_837_894_phi_976_;
                                let _e2314 = edge_837_894_phi_977_;
                                let _e2316 = edge_837_894_phi_978_;
                                let _e2318 = edge_837_894_phi_979_;
                                let _e2320 = edge_837_894_phi_980_;
                                let _e2322 = edge_837_894_phi_981_;
                                let _e2324 = edge_837_894_phi_982_;
                                let _e2326 = edge_837_894_phi_983_;
                                let _e2328 = edge_837_894_phi_984_;
                                let _e2330 = edge_837_894_phi_985_;
                                let _e2332 = edge_837_894_phi_986_;
                                let _e2334 = edge_837_894_phi_987_;
                                let _e2336 = edge_837_894_phi_988_;
                                let _e2338 = edge_837_894_phi_989_;
                                let _e2340 = edge_837_894_phi_990_;
                                let _e2342 = edge_837_894_phi_991_;
                                phi_976_ = _e2312;
                                phi_977_ = _e2314;
                                phi_978_ = _e2316;
                                phi_979_ = _e2318;
                                phi_980_ = _e2320;
                                phi_981_ = _e2322;
                                phi_982_ = _e2324;
                                phi_983_ = _e2326;
                                phi_984_ = _e2328;
                                phi_985_ = _e2330;
                                phi_986_ = _e2332;
                                phi_987_ = _e2334;
                                phi_988_ = _e2336;
                                phi_989_ = _e2338;
                                phi_990_ = _e2340;
                                phi_991_ = _e2342;
                            }
                        } else {
                            if (bitcast<i32>((_e2222 - 2u)) < bitcast<i32>(1u)) {
                                edge_840_894_phi_976_ = _e1749;
                                edge_840_894_phi_977_ = _e1751;
                                edge_840_894_phi_978_ = _e1753;
                                edge_840_894_phi_979_ = _e1755;
                                edge_840_894_phi_980_ = _e1757;
                                edge_840_894_phi_981_ = _e1759;
                                edge_840_894_phi_982_ = _e1761;
                                edge_840_894_phi_983_ = _e1763;
                                edge_840_894_phi_984_ = _e1765;
                                edge_840_894_phi_985_ = _e1946;
                                edge_840_894_phi_986_ = _e1769;
                                edge_840_894_phi_987_ = _e1771;
                                edge_840_894_phi_988_ = _e1773;
                                edge_840_894_phi_989_ = _e1775;
                                edge_840_894_phi_990_ = _e1777;
                                edge_840_894_phi_991_ = _e1779;
                                let _e2382 = edge_840_894_phi_976_;
                                let _e2384 = edge_840_894_phi_977_;
                                let _e2386 = edge_840_894_phi_978_;
                                let _e2388 = edge_840_894_phi_979_;
                                let _e2390 = edge_840_894_phi_980_;
                                let _e2392 = edge_840_894_phi_981_;
                                let _e2394 = edge_840_894_phi_982_;
                                let _e2396 = edge_840_894_phi_983_;
                                let _e2398 = edge_840_894_phi_984_;
                                let _e2400 = edge_840_894_phi_985_;
                                let _e2402 = edge_840_894_phi_986_;
                                let _e2404 = edge_840_894_phi_987_;
                                let _e2406 = edge_840_894_phi_988_;
                                let _e2408 = edge_840_894_phi_989_;
                                let _e2410 = edge_840_894_phi_990_;
                                let _e2412 = edge_840_894_phi_991_;
                                phi_976_ = _e2382;
                                phi_977_ = _e2384;
                                phi_978_ = _e2386;
                                phi_979_ = _e2388;
                                phi_980_ = _e2390;
                                phi_981_ = _e2392;
                                phi_982_ = _e2394;
                                phi_983_ = _e2396;
                                phi_984_ = _e2398;
                                phi_985_ = _e2400;
                                phi_986_ = _e2402;
                                phi_987_ = _e2404;
                                phi_988_ = _e2406;
                                phi_989_ = _e2408;
                                phi_990_ = _e2410;
                                phi_991_ = _e2412;
                            } else {
                                edge_844_894_phi_976_ = _e1749;
                                edge_844_894_phi_977_ = _e1751;
                                edge_844_894_phi_978_ = _e1753;
                                edge_844_894_phi_979_ = _e1755;
                                edge_844_894_phi_980_ = _e1757;
                                edge_844_894_phi_981_ = _e1759;
                                edge_844_894_phi_982_ = _e1761;
                                edge_844_894_phi_983_ = _e1763;
                                edge_844_894_phi_984_ = _e1946;
                                edge_844_894_phi_985_ = _e1767;
                                edge_844_894_phi_986_ = _e1769;
                                edge_844_894_phi_987_ = _e1771;
                                edge_844_894_phi_988_ = _e1773;
                                edge_844_894_phi_989_ = _e1775;
                                edge_844_894_phi_990_ = _e1777;
                                edge_844_894_phi_991_ = _e1779;
                                let _e2446 = edge_844_894_phi_976_;
                                let _e2448 = edge_844_894_phi_977_;
                                let _e2450 = edge_844_894_phi_978_;
                                let _e2452 = edge_844_894_phi_979_;
                                let _e2454 = edge_844_894_phi_980_;
                                let _e2456 = edge_844_894_phi_981_;
                                let _e2458 = edge_844_894_phi_982_;
                                let _e2460 = edge_844_894_phi_983_;
                                let _e2462 = edge_844_894_phi_984_;
                                let _e2464 = edge_844_894_phi_985_;
                                let _e2466 = edge_844_894_phi_986_;
                                let _e2468 = edge_844_894_phi_987_;
                                let _e2470 = edge_844_894_phi_988_;
                                let _e2472 = edge_844_894_phi_989_;
                                let _e2474 = edge_844_894_phi_990_;
                                let _e2476 = edge_844_894_phi_991_;
                                phi_976_ = _e2446;
                                phi_977_ = _e2448;
                                phi_978_ = _e2450;
                                phi_979_ = _e2452;
                                phi_980_ = _e2454;
                                phi_981_ = _e2456;
                                phi_982_ = _e2458;
                                phi_983_ = _e2460;
                                phi_984_ = _e2462;
                                phi_985_ = _e2464;
                                phi_986_ = _e2466;
                                phi_987_ = _e2468;
                                phi_988_ = _e2470;
                                phi_989_ = _e2472;
                                phi_990_ = _e2474;
                                phi_991_ = _e2476;
                            }
                        }
                    }
                } else {
                    let _e2494 = (_e1781 - 8u);
                    if (bitcast<i32>(_e2494) < bitcast<i32>(4u)) {
                        if (bitcast<i32>(_e2494) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e2494) < bitcast<i32>(1u)) {
                                edge_859_894_phi_976_ = _e1749;
                                edge_859_894_phi_977_ = _e1751;
                                edge_859_894_phi_978_ = _e1753;
                                edge_859_894_phi_979_ = _e1755;
                                edge_859_894_phi_980_ = _e1757;
                                edge_859_894_phi_981_ = _e1759;
                                edge_859_894_phi_982_ = _e1761;
                                edge_859_894_phi_983_ = _e1946;
                                edge_859_894_phi_984_ = _e1765;
                                edge_859_894_phi_985_ = _e1767;
                                edge_859_894_phi_986_ = _e1769;
                                edge_859_894_phi_987_ = _e1771;
                                edge_859_894_phi_988_ = _e1773;
                                edge_859_894_phi_989_ = _e1775;
                                edge_859_894_phi_990_ = _e1777;
                                edge_859_894_phi_991_ = _e1779;
                                let _e2524 = edge_859_894_phi_976_;
                                let _e2526 = edge_859_894_phi_977_;
                                let _e2528 = edge_859_894_phi_978_;
                                let _e2530 = edge_859_894_phi_979_;
                                let _e2532 = edge_859_894_phi_980_;
                                let _e2534 = edge_859_894_phi_981_;
                                let _e2536 = edge_859_894_phi_982_;
                                let _e2538 = edge_859_894_phi_983_;
                                let _e2540 = edge_859_894_phi_984_;
                                let _e2542 = edge_859_894_phi_985_;
                                let _e2544 = edge_859_894_phi_986_;
                                let _e2546 = edge_859_894_phi_987_;
                                let _e2548 = edge_859_894_phi_988_;
                                let _e2550 = edge_859_894_phi_989_;
                                let _e2552 = edge_859_894_phi_990_;
                                let _e2554 = edge_859_894_phi_991_;
                                phi_976_ = _e2524;
                                phi_977_ = _e2526;
                                phi_978_ = _e2528;
                                phi_979_ = _e2530;
                                phi_980_ = _e2532;
                                phi_981_ = _e2534;
                                phi_982_ = _e2536;
                                phi_983_ = _e2538;
                                phi_984_ = _e2540;
                                phi_985_ = _e2542;
                                phi_986_ = _e2544;
                                phi_987_ = _e2546;
                                phi_988_ = _e2548;
                                phi_989_ = _e2550;
                                phi_990_ = _e2552;
                                phi_991_ = _e2554;
                            } else {
                                edge_861_894_phi_976_ = _e1749;
                                edge_861_894_phi_977_ = _e1751;
                                edge_861_894_phi_978_ = _e1753;
                                edge_861_894_phi_979_ = _e1755;
                                edge_861_894_phi_980_ = _e1757;
                                edge_861_894_phi_981_ = _e1759;
                                edge_861_894_phi_982_ = _e1946;
                                edge_861_894_phi_983_ = _e1763;
                                edge_861_894_phi_984_ = _e1765;
                                edge_861_894_phi_985_ = _e1767;
                                edge_861_894_phi_986_ = _e1769;
                                edge_861_894_phi_987_ = _e1771;
                                edge_861_894_phi_988_ = _e1773;
                                edge_861_894_phi_989_ = _e1775;
                                edge_861_894_phi_990_ = _e1777;
                                edge_861_894_phi_991_ = _e1779;
                                let _e2588 = edge_861_894_phi_976_;
                                let _e2590 = edge_861_894_phi_977_;
                                let _e2592 = edge_861_894_phi_978_;
                                let _e2594 = edge_861_894_phi_979_;
                                let _e2596 = edge_861_894_phi_980_;
                                let _e2598 = edge_861_894_phi_981_;
                                let _e2600 = edge_861_894_phi_982_;
                                let _e2602 = edge_861_894_phi_983_;
                                let _e2604 = edge_861_894_phi_984_;
                                let _e2606 = edge_861_894_phi_985_;
                                let _e2608 = edge_861_894_phi_986_;
                                let _e2610 = edge_861_894_phi_987_;
                                let _e2612 = edge_861_894_phi_988_;
                                let _e2614 = edge_861_894_phi_989_;
                                let _e2616 = edge_861_894_phi_990_;
                                let _e2618 = edge_861_894_phi_991_;
                                phi_976_ = _e2588;
                                phi_977_ = _e2590;
                                phi_978_ = _e2592;
                                phi_979_ = _e2594;
                                phi_980_ = _e2596;
                                phi_981_ = _e2598;
                                phi_982_ = _e2600;
                                phi_983_ = _e2602;
                                phi_984_ = _e2604;
                                phi_985_ = _e2606;
                                phi_986_ = _e2608;
                                phi_987_ = _e2610;
                                phi_988_ = _e2612;
                                phi_989_ = _e2614;
                                phi_990_ = _e2616;
                                phi_991_ = _e2618;
                            }
                        } else {
                            if (bitcast<i32>((_e2494 - 2u)) < bitcast<i32>(1u)) {
                                edge_864_894_phi_976_ = _e1749;
                                edge_864_894_phi_977_ = _e1751;
                                edge_864_894_phi_978_ = _e1753;
                                edge_864_894_phi_979_ = _e1755;
                                edge_864_894_phi_980_ = _e1757;
                                edge_864_894_phi_981_ = _e1946;
                                edge_864_894_phi_982_ = _e1761;
                                edge_864_894_phi_983_ = _e1763;
                                edge_864_894_phi_984_ = _e1765;
                                edge_864_894_phi_985_ = _e1767;
                                edge_864_894_phi_986_ = _e1769;
                                edge_864_894_phi_987_ = _e1771;
                                edge_864_894_phi_988_ = _e1773;
                                edge_864_894_phi_989_ = _e1775;
                                edge_864_894_phi_990_ = _e1777;
                                edge_864_894_phi_991_ = _e1779;
                                let _e2658 = edge_864_894_phi_976_;
                                let _e2660 = edge_864_894_phi_977_;
                                let _e2662 = edge_864_894_phi_978_;
                                let _e2664 = edge_864_894_phi_979_;
                                let _e2666 = edge_864_894_phi_980_;
                                let _e2668 = edge_864_894_phi_981_;
                                let _e2670 = edge_864_894_phi_982_;
                                let _e2672 = edge_864_894_phi_983_;
                                let _e2674 = edge_864_894_phi_984_;
                                let _e2676 = edge_864_894_phi_985_;
                                let _e2678 = edge_864_894_phi_986_;
                                let _e2680 = edge_864_894_phi_987_;
                                let _e2682 = edge_864_894_phi_988_;
                                let _e2684 = edge_864_894_phi_989_;
                                let _e2686 = edge_864_894_phi_990_;
                                let _e2688 = edge_864_894_phi_991_;
                                phi_976_ = _e2658;
                                phi_977_ = _e2660;
                                phi_978_ = _e2662;
                                phi_979_ = _e2664;
                                phi_980_ = _e2666;
                                phi_981_ = _e2668;
                                phi_982_ = _e2670;
                                phi_983_ = _e2672;
                                phi_984_ = _e2674;
                                phi_985_ = _e2676;
                                phi_986_ = _e2678;
                                phi_987_ = _e2680;
                                phi_988_ = _e2682;
                                phi_989_ = _e2684;
                                phi_990_ = _e2686;
                                phi_991_ = _e2688;
                            } else {
                                edge_868_894_phi_976_ = _e1749;
                                edge_868_894_phi_977_ = _e1751;
                                edge_868_894_phi_978_ = _e1753;
                                edge_868_894_phi_979_ = _e1755;
                                edge_868_894_phi_980_ = _e1946;
                                edge_868_894_phi_981_ = _e1759;
                                edge_868_894_phi_982_ = _e1761;
                                edge_868_894_phi_983_ = _e1763;
                                edge_868_894_phi_984_ = _e1765;
                                edge_868_894_phi_985_ = _e1767;
                                edge_868_894_phi_986_ = _e1769;
                                edge_868_894_phi_987_ = _e1771;
                                edge_868_894_phi_988_ = _e1773;
                                edge_868_894_phi_989_ = _e1775;
                                edge_868_894_phi_990_ = _e1777;
                                edge_868_894_phi_991_ = _e1779;
                                let _e2722 = edge_868_894_phi_976_;
                                let _e2724 = edge_868_894_phi_977_;
                                let _e2726 = edge_868_894_phi_978_;
                                let _e2728 = edge_868_894_phi_979_;
                                let _e2730 = edge_868_894_phi_980_;
                                let _e2732 = edge_868_894_phi_981_;
                                let _e2734 = edge_868_894_phi_982_;
                                let _e2736 = edge_868_894_phi_983_;
                                let _e2738 = edge_868_894_phi_984_;
                                let _e2740 = edge_868_894_phi_985_;
                                let _e2742 = edge_868_894_phi_986_;
                                let _e2744 = edge_868_894_phi_987_;
                                let _e2746 = edge_868_894_phi_988_;
                                let _e2748 = edge_868_894_phi_989_;
                                let _e2750 = edge_868_894_phi_990_;
                                let _e2752 = edge_868_894_phi_991_;
                                phi_976_ = _e2722;
                                phi_977_ = _e2724;
                                phi_978_ = _e2726;
                                phi_979_ = _e2728;
                                phi_980_ = _e2730;
                                phi_981_ = _e2732;
                                phi_982_ = _e2734;
                                phi_983_ = _e2736;
                                phi_984_ = _e2738;
                                phi_985_ = _e2740;
                                phi_986_ = _e2742;
                                phi_987_ = _e2744;
                                phi_988_ = _e2746;
                                phi_989_ = _e2748;
                                phi_990_ = _e2750;
                                phi_991_ = _e2752;
                            }
                        }
                    } else {
                        let _e2770 = (_e2494 - 4u);
                        if (bitcast<i32>(_e2770) < bitcast<i32>(2u)) {
                            if (bitcast<i32>(_e2770) < bitcast<i32>(1u)) {
                                edge_878_894_phi_976_ = _e1749;
                                edge_878_894_phi_977_ = _e1751;
                                edge_878_894_phi_978_ = _e1753;
                                edge_878_894_phi_979_ = _e1946;
                                edge_878_894_phi_980_ = _e1757;
                                edge_878_894_phi_981_ = _e1759;
                                edge_878_894_phi_982_ = _e1761;
                                edge_878_894_phi_983_ = _e1763;
                                edge_878_894_phi_984_ = _e1765;
                                edge_878_894_phi_985_ = _e1767;
                                edge_878_894_phi_986_ = _e1769;
                                edge_878_894_phi_987_ = _e1771;
                                edge_878_894_phi_988_ = _e1773;
                                edge_878_894_phi_989_ = _e1775;
                                edge_878_894_phi_990_ = _e1777;
                                edge_878_894_phi_991_ = _e1779;
                                let _e2796 = edge_878_894_phi_976_;
                                let _e2798 = edge_878_894_phi_977_;
                                let _e2800 = edge_878_894_phi_978_;
                                let _e2802 = edge_878_894_phi_979_;
                                let _e2804 = edge_878_894_phi_980_;
                                let _e2806 = edge_878_894_phi_981_;
                                let _e2808 = edge_878_894_phi_982_;
                                let _e2810 = edge_878_894_phi_983_;
                                let _e2812 = edge_878_894_phi_984_;
                                let _e2814 = edge_878_894_phi_985_;
                                let _e2816 = edge_878_894_phi_986_;
                                let _e2818 = edge_878_894_phi_987_;
                                let _e2820 = edge_878_894_phi_988_;
                                let _e2822 = edge_878_894_phi_989_;
                                let _e2824 = edge_878_894_phi_990_;
                                let _e2826 = edge_878_894_phi_991_;
                                phi_976_ = _e2796;
                                phi_977_ = _e2798;
                                phi_978_ = _e2800;
                                phi_979_ = _e2802;
                                phi_980_ = _e2804;
                                phi_981_ = _e2806;
                                phi_982_ = _e2808;
                                phi_983_ = _e2810;
                                phi_984_ = _e2812;
                                phi_985_ = _e2814;
                                phi_986_ = _e2816;
                                phi_987_ = _e2818;
                                phi_988_ = _e2820;
                                phi_989_ = _e2822;
                                phi_990_ = _e2824;
                                phi_991_ = _e2826;
                            } else {
                                edge_880_894_phi_976_ = _e1749;
                                edge_880_894_phi_977_ = _e1751;
                                edge_880_894_phi_978_ = _e1946;
                                edge_880_894_phi_979_ = _e1755;
                                edge_880_894_phi_980_ = _e1757;
                                edge_880_894_phi_981_ = _e1759;
                                edge_880_894_phi_982_ = _e1761;
                                edge_880_894_phi_983_ = _e1763;
                                edge_880_894_phi_984_ = _e1765;
                                edge_880_894_phi_985_ = _e1767;
                                edge_880_894_phi_986_ = _e1769;
                                edge_880_894_phi_987_ = _e1771;
                                edge_880_894_phi_988_ = _e1773;
                                edge_880_894_phi_989_ = _e1775;
                                edge_880_894_phi_990_ = _e1777;
                                edge_880_894_phi_991_ = _e1779;
                                let _e2860 = edge_880_894_phi_976_;
                                let _e2862 = edge_880_894_phi_977_;
                                let _e2864 = edge_880_894_phi_978_;
                                let _e2866 = edge_880_894_phi_979_;
                                let _e2868 = edge_880_894_phi_980_;
                                let _e2870 = edge_880_894_phi_981_;
                                let _e2872 = edge_880_894_phi_982_;
                                let _e2874 = edge_880_894_phi_983_;
                                let _e2876 = edge_880_894_phi_984_;
                                let _e2878 = edge_880_894_phi_985_;
                                let _e2880 = edge_880_894_phi_986_;
                                let _e2882 = edge_880_894_phi_987_;
                                let _e2884 = edge_880_894_phi_988_;
                                let _e2886 = edge_880_894_phi_989_;
                                let _e2888 = edge_880_894_phi_990_;
                                let _e2890 = edge_880_894_phi_991_;
                                phi_976_ = _e2860;
                                phi_977_ = _e2862;
                                phi_978_ = _e2864;
                                phi_979_ = _e2866;
                                phi_980_ = _e2868;
                                phi_981_ = _e2870;
                                phi_982_ = _e2872;
                                phi_983_ = _e2874;
                                phi_984_ = _e2876;
                                phi_985_ = _e2878;
                                phi_986_ = _e2880;
                                phi_987_ = _e2882;
                                phi_988_ = _e2884;
                                phi_989_ = _e2886;
                                phi_990_ = _e2888;
                                phi_991_ = _e2890;
                            }
                        } else {
                            if (bitcast<i32>((_e2770 - 2u)) < bitcast<i32>(1u)) {
                                edge_883_894_phi_976_ = _e1749;
                                edge_883_894_phi_977_ = _e1946;
                                edge_883_894_phi_978_ = _e1753;
                                edge_883_894_phi_979_ = _e1755;
                                edge_883_894_phi_980_ = _e1757;
                                edge_883_894_phi_981_ = _e1759;
                                edge_883_894_phi_982_ = _e1761;
                                edge_883_894_phi_983_ = _e1763;
                                edge_883_894_phi_984_ = _e1765;
                                edge_883_894_phi_985_ = _e1767;
                                edge_883_894_phi_986_ = _e1769;
                                edge_883_894_phi_987_ = _e1771;
                                edge_883_894_phi_988_ = _e1773;
                                edge_883_894_phi_989_ = _e1775;
                                edge_883_894_phi_990_ = _e1777;
                                edge_883_894_phi_991_ = _e1779;
                                let _e2930 = edge_883_894_phi_976_;
                                let _e2932 = edge_883_894_phi_977_;
                                let _e2934 = edge_883_894_phi_978_;
                                let _e2936 = edge_883_894_phi_979_;
                                let _e2938 = edge_883_894_phi_980_;
                                let _e2940 = edge_883_894_phi_981_;
                                let _e2942 = edge_883_894_phi_982_;
                                let _e2944 = edge_883_894_phi_983_;
                                let _e2946 = edge_883_894_phi_984_;
                                let _e2948 = edge_883_894_phi_985_;
                                let _e2950 = edge_883_894_phi_986_;
                                let _e2952 = edge_883_894_phi_987_;
                                let _e2954 = edge_883_894_phi_988_;
                                let _e2956 = edge_883_894_phi_989_;
                                let _e2958 = edge_883_894_phi_990_;
                                let _e2960 = edge_883_894_phi_991_;
                                phi_976_ = _e2930;
                                phi_977_ = _e2932;
                                phi_978_ = _e2934;
                                phi_979_ = _e2936;
                                phi_980_ = _e2938;
                                phi_981_ = _e2940;
                                phi_982_ = _e2942;
                                phi_983_ = _e2944;
                                phi_984_ = _e2946;
                                phi_985_ = _e2948;
                                phi_986_ = _e2950;
                                phi_987_ = _e2952;
                                phi_988_ = _e2954;
                                phi_989_ = _e2956;
                                phi_990_ = _e2958;
                                phi_991_ = _e2960;
                            } else {
                                edge_887_894_phi_976_ = _e1946;
                                edge_887_894_phi_977_ = _e1751;
                                edge_887_894_phi_978_ = _e1753;
                                edge_887_894_phi_979_ = _e1755;
                                edge_887_894_phi_980_ = _e1757;
                                edge_887_894_phi_981_ = _e1759;
                                edge_887_894_phi_982_ = _e1761;
                                edge_887_894_phi_983_ = _e1763;
                                edge_887_894_phi_984_ = _e1765;
                                edge_887_894_phi_985_ = _e1767;
                                edge_887_894_phi_986_ = _e1769;
                                edge_887_894_phi_987_ = _e1771;
                                edge_887_894_phi_988_ = _e1773;
                                edge_887_894_phi_989_ = _e1775;
                                edge_887_894_phi_990_ = _e1777;
                                edge_887_894_phi_991_ = _e1779;
                                let _e2994 = edge_887_894_phi_976_;
                                let _e2996 = edge_887_894_phi_977_;
                                let _e2998 = edge_887_894_phi_978_;
                                let _e3000 = edge_887_894_phi_979_;
                                let _e3002 = edge_887_894_phi_980_;
                                let _e3004 = edge_887_894_phi_981_;
                                let _e3006 = edge_887_894_phi_982_;
                                let _e3008 = edge_887_894_phi_983_;
                                let _e3010 = edge_887_894_phi_984_;
                                let _e3012 = edge_887_894_phi_985_;
                                let _e3014 = edge_887_894_phi_986_;
                                let _e3016 = edge_887_894_phi_987_;
                                let _e3018 = edge_887_894_phi_988_;
                                let _e3020 = edge_887_894_phi_989_;
                                let _e3022 = edge_887_894_phi_990_;
                                let _e3024 = edge_887_894_phi_991_;
                                phi_976_ = _e2994;
                                phi_977_ = _e2996;
                                phi_978_ = _e2998;
                                phi_979_ = _e3000;
                                phi_980_ = _e3002;
                                phi_981_ = _e3004;
                                phi_982_ = _e3006;
                                phi_983_ = _e3008;
                                phi_984_ = _e3010;
                                phi_985_ = _e3012;
                                phi_986_ = _e3014;
                                phi_987_ = _e3016;
                                phi_988_ = _e3018;
                                phi_989_ = _e3020;
                                phi_990_ = _e3022;
                                phi_991_ = _e3024;
                            }
                        }
                    }
                }
                let _e3042 = phi_976_;
                let _e3044 = phi_977_;
                let _e3046 = phi_978_;
                let _e3048 = phi_979_;
                let _e3050 = phi_980_;
                let _e3052 = phi_981_;
                let _e3054 = phi_982_;
                let _e3056 = phi_983_;
                let _e3058 = phi_984_;
                let _e3060 = phi_985_;
                let _e3062 = phi_986_;
                let _e3064 = phi_987_;
                let _e3066 = phi_988_;
                let _e3068 = phi_989_;
                let _e3070 = phi_990_;
                let _e3072 = phi_991_;
                edge_894_696_phi_771_ = _e3042;
                edge_894_696_phi_773_ = _e3044;
                edge_894_696_phi_775_ = _e3046;
                edge_894_696_phi_777_ = _e3048;
                edge_894_696_phi_779_ = _e3050;
                edge_894_696_phi_781_ = _e3052;
                edge_894_696_phi_783_ = _e3054;
                edge_894_696_phi_785_ = _e3056;
                edge_894_696_phi_787_ = _e3058;
                edge_894_696_phi_789_ = _e3060;
                edge_894_696_phi_791_ = _e3062;
                edge_894_696_phi_793_ = _e3064;
                edge_894_696_phi_795_ = _e3066;
                edge_894_696_phi_797_ = _e3068;
                edge_894_696_phi_799_ = _e3070;
                edge_894_696_phi_801_ = _e3072;
                edge_894_696_phi_803_ = (_e1781 + 1u);
                let _e3093 = edge_894_696_phi_771_;
                let _e3095 = edge_894_696_phi_773_;
                let _e3097 = edge_894_696_phi_775_;
                let _e3099 = edge_894_696_phi_777_;
                let _e3101 = edge_894_696_phi_779_;
                let _e3103 = edge_894_696_phi_781_;
                let _e3105 = edge_894_696_phi_783_;
                let _e3107 = edge_894_696_phi_785_;
                let _e3109 = edge_894_696_phi_787_;
                let _e3111 = edge_894_696_phi_789_;
                let _e3113 = edge_894_696_phi_791_;
                let _e3115 = edge_894_696_phi_793_;
                let _e3117 = edge_894_696_phi_795_;
                let _e3119 = edge_894_696_phi_797_;
                let _e3121 = edge_894_696_phi_799_;
                let _e3123 = edge_894_696_phi_801_;
                let _e3125 = edge_894_696_phi_803_;
                phi_771_ = _e3093;
                phi_773_ = _e3095;
                phi_775_ = _e3097;
                phi_777_ = _e3099;
                phi_779_ = _e3101;
                phi_781_ = _e3103;
                phi_783_ = _e3105;
                phi_785_ = _e3107;
                phi_787_ = _e3109;
                phi_789_ = _e3111;
                phi_791_ = _e3113;
                phi_793_ = _e3115;
                phi_795_ = _e3117;
                phi_797_ = _e3119;
                phi_799_ = _e3121;
                phi_801_ = _e3123;
                phi_803_ = _e3125;
                continue;
            } else {
                edge_696_945_phi_994_ = 0f;
                edge_696_945_phi_996_ = 0f;
                edge_696_945_phi_998_ = 0f;
                edge_696_945_phi_1000_ = 0f;
                edge_696_945_phi_1002_ = 0f;
                edge_696_945_phi_1004_ = 0f;
                edge_696_945_phi_1006_ = 0f;
                edge_696_945_phi_1008_ = 0f;
                edge_696_945_phi_1010_ = 0u;
                let _e3162 = edge_696_945_phi_994_;
                let _e3164 = edge_696_945_phi_996_;
                let _e3166 = edge_696_945_phi_998_;
                let _e3168 = edge_696_945_phi_1000_;
                let _e3170 = edge_696_945_phi_1002_;
                let _e3172 = edge_696_945_phi_1004_;
                let _e3174 = edge_696_945_phi_1006_;
                let _e3176 = edge_696_945_phi_1008_;
                let _e3178 = edge_696_945_phi_1010_;
                phi_994_ = _e3162;
                phi_996_ = _e3164;
                phi_998_ = _e3166;
                phi_1000_ = _e3168;
                phi_1002_ = _e3170;
                phi_1004_ = _e3172;
                phi_1006_ = _e3174;
                phi_1008_ = _e3176;
                phi_1010_ = _e3178;
                loop_header_carry_804_ = _e1785;
                break;
            }
        }
        let _e3190 = phi_771_;
        let _e3192 = phi_773_;
        let _e3194 = phi_775_;
        let _e3196 = phi_777_;
        let _e3198 = phi_779_;
        let _e3200 = phi_781_;
        let _e3202 = phi_783_;
        let _e3204 = phi_785_;
        let _e3206 = phi_787_;
        let _e3208 = phi_789_;
        let _e3210 = phi_791_;
        let _e3212 = phi_793_;
        let _e3214 = phi_795_;
        let _e3216 = phi_797_;
        let _e3218 = phi_799_;
        let _e3220 = phi_801_;
        edge_696_945_phi_994_1 = 0f;
        edge_696_945_phi_996_1 = 0f;
        edge_696_945_phi_998_1 = 0f;
        edge_696_945_phi_1000_1 = 0f;
        edge_696_945_phi_1002_1 = 0f;
        edge_696_945_phi_1004_1 = 0f;
        edge_696_945_phi_1006_1 = 0f;
        edge_696_945_phi_1008_1 = 0f;
        edge_696_945_phi_1010_1 = 0u;
        let _e3244 = edge_696_945_phi_994_1;
        let _e3246 = edge_696_945_phi_996_1;
        let _e3248 = edge_696_945_phi_998_1;
        let _e3250 = edge_696_945_phi_1000_1;
        let _e3252 = edge_696_945_phi_1002_1;
        let _e3254 = edge_696_945_phi_1004_1;
        let _e3256 = edge_696_945_phi_1006_1;
        let _e3258 = edge_696_945_phi_1008_1;
        let _e3260 = edge_696_945_phi_1010_1;
        phi_994_ = _e3244;
        phi_996_ = _e3246;
        phi_998_ = _e3248;
        phi_1000_ = _e3250;
        phi_1002_ = _e3252;
        phi_1004_ = _e3254;
        phi_1006_ = _e3256;
        phi_1008_ = _e3258;
        phi_1010_ = _e3260;
        loop {
            let _e3272 = phi_994_;
            let _e3274 = phi_996_;
            let _e3276 = phi_998_;
            let _e3278 = phi_1000_;
            let _e3280 = phi_1002_;
            let _e3282 = phi_1004_;
            let _e3284 = phi_1006_;
            let _e3286 = phi_1008_;
            let _e3288 = phi_1010_;
            let _e3292 = (bitcast<i32>(_e3288) < bitcast<i32>(7u));
            if _e3292 {
                edge_945_948_phi_1013_ = 0f;
                edge_945_948_phi_1015_ = 0u;
                let _e3298 = edge_945_948_phi_1013_;
                let _e3300 = edge_945_948_phi_1015_;
                phi_1013_ = _e3298;
                phi_1015_ = _e3300;
                loop {
                    let _e3305 = phi_1013_;
                    let _e3307 = phi_1015_;
                    let _e3311 = (bitcast<i32>(_e3307) < bitcast<i32>(12u));
                    if _e3311 {
                        if (bitcast<i32>(_e3307) < bitcast<i32>(6u)) {
                            if (_e3288 == 0u) {
                                edge_955_954_phi_1045_ = 4294967295u;
                                let _e3334 = edge_955_954_phi_1045_;
                                phi_1045_ = _e3334;
                            } else {
                                if (_e3288 == (_e3307 + 1u)) {
                                    edge_959_954_phi_1045_ = 1u;
                                    let _e3324 = edge_959_954_phi_1045_;
                                    phi_1045_ = _e3324;
                                } else {
                                    edge_962_954_phi_1045_ = 0u;
                                    let _e3329 = edge_962_954_phi_1045_;
                                    phi_1045_ = _e3329;
                                }
                            }
                        } else {
                            let _e3337 = (_e3307 - 6u);
                            let _e3339 = (_e3337 + 1u);
                            if (_e3288 == _e3339) {
                                edge_956_954_phi_1045_ = 4294967295u;
                                let _e3370 = edge_956_954_phi_1045_;
                                phi_1045_ = _e3370;
                            } else {
                                if (_e3337 == 5u) {
                                    edge_967_970_phi_1029_ = 0u;
                                    let _e3346 = edge_967_970_phi_1029_;
                                    phi_1029_ = _e3346;
                                } else {
                                    edge_969_970_phi_1029_ = _e3339;
                                    let _e3350 = edge_969_970_phi_1029_;
                                    phi_1029_ = _e3350;
                                }
                                let _e3353 = phi_1029_;
                                if (_e3288 == (_e3353 + 1u)) {
                                    edge_970_954_phi_1045_ = 1u;
                                    let _e3360 = edge_970_954_phi_1045_;
                                    phi_1045_ = _e3360;
                                } else {
                                    edge_974_954_phi_1045_ = 0u;
                                    let _e3365 = edge_974_954_phi_1045_;
                                    phi_1045_ = _e3365;
                                }
                            }
                        }
                        let _e3373 = phi_1045_;
                        if (_e3373 == 0u) {
                            edge_954_1127_phi_1112_ = _e3305;
                            let _e3532 = edge_954_1127_phi_1112_;
                            phi_1112_ = _e3532;
                        } else {
                            if (bitcast<i32>(_e3307) < bitcast<i32>(8u)) {
                                if (bitcast<i32>(_e3307) < bitcast<i32>(4u)) {
                                    if (bitcast<i32>(_e3307) < bitcast<i32>(2u)) {
                                        if (bitcast<i32>(_e3307) < bitcast<i32>(1u)) {
                                            edge_1046_1124_phi_1107_ = _e3220;
                                            let _e3394 = edge_1046_1124_phi_1107_;
                                            phi_1107_ = _e3394;
                                        } else {
                                            edge_1048_1124_phi_1107_ = _e3218;
                                            let _e3398 = edge_1048_1124_phi_1107_;
                                            phi_1107_ = _e3398;
                                        }
                                    } else {
                                        if (bitcast<i32>((_e3307 - 2u)) < bitcast<i32>(1u)) {
                                            edge_1051_1124_phi_1107_ = _e3216;
                                            let _e3408 = edge_1051_1124_phi_1107_;
                                            phi_1107_ = _e3408;
                                        } else {
                                            edge_1055_1124_phi_1107_ = _e3214;
                                            let _e3412 = edge_1055_1124_phi_1107_;
                                            phi_1107_ = _e3412;
                                        }
                                    }
                                } else {
                                    let _e3415 = (_e3307 - 4u);
                                    if (bitcast<i32>(_e3415) < bitcast<i32>(2u)) {
                                        if (bitcast<i32>(_e3415) < bitcast<i32>(1u)) {
                                            edge_1065_1124_phi_1107_ = _e3212;
                                            let _e3426 = edge_1065_1124_phi_1107_;
                                            phi_1107_ = _e3426;
                                        } else {
                                            edge_1067_1124_phi_1107_ = _e3210;
                                            let _e3430 = edge_1067_1124_phi_1107_;
                                            phi_1107_ = _e3430;
                                        }
                                    } else {
                                        if (bitcast<i32>((_e3415 - 2u)) < bitcast<i32>(1u)) {
                                            edge_1070_1124_phi_1107_ = _e3208;
                                            let _e3440 = edge_1070_1124_phi_1107_;
                                            phi_1107_ = _e3440;
                                        } else {
                                            edge_1074_1124_phi_1107_ = _e3206;
                                            let _e3444 = edge_1074_1124_phi_1107_;
                                            phi_1107_ = _e3444;
                                        }
                                    }
                                }
                            } else {
                                let _e3447 = (_e3307 - 8u);
                                if (bitcast<i32>(_e3447) < bitcast<i32>(4u)) {
                                    if (bitcast<i32>(_e3447) < bitcast<i32>(2u)) {
                                        if (bitcast<i32>(_e3447) < bitcast<i32>(1u)) {
                                            edge_1089_1124_phi_1107_ = _e3204;
                                            let _e3462 = edge_1089_1124_phi_1107_;
                                            phi_1107_ = _e3462;
                                        } else {
                                            edge_1091_1124_phi_1107_ = _e3202;
                                            let _e3466 = edge_1091_1124_phi_1107_;
                                            phi_1107_ = _e3466;
                                        }
                                    } else {
                                        if (bitcast<i32>((_e3447 - 2u)) < bitcast<i32>(1u)) {
                                            edge_1094_1124_phi_1107_ = _e3200;
                                            let _e3476 = edge_1094_1124_phi_1107_;
                                            phi_1107_ = _e3476;
                                        } else {
                                            edge_1098_1124_phi_1107_ = _e3198;
                                            let _e3480 = edge_1098_1124_phi_1107_;
                                            phi_1107_ = _e3480;
                                        }
                                    }
                                } else {
                                    let _e3483 = (_e3447 - 4u);
                                    if (bitcast<i32>(_e3483) < bitcast<i32>(2u)) {
                                        if (bitcast<i32>(_e3483) < bitcast<i32>(1u)) {
                                            edge_1108_1124_phi_1107_ = _e3196;
                                            let _e3494 = edge_1108_1124_phi_1107_;
                                            phi_1107_ = _e3494;
                                        } else {
                                            edge_1110_1124_phi_1107_ = _e3194;
                                            let _e3498 = edge_1110_1124_phi_1107_;
                                            phi_1107_ = _e3498;
                                        }
                                    } else {
                                        if (bitcast<i32>((_e3483 - 2u)) < bitcast<i32>(1u)) {
                                            edge_1113_1124_phi_1107_ = _e3192;
                                            let _e3508 = edge_1113_1124_phi_1107_;
                                            phi_1107_ = _e3508;
                                        } else {
                                            edge_1117_1124_phi_1107_ = _e3190;
                                            let _e3512 = edge_1117_1124_phi_1107_;
                                            phi_1107_ = _e3512;
                                        }
                                    }
                                }
                            }
                            let _e3515 = phi_1107_;
                            if (bitcast<i32>(0u) < bitcast<i32>(_e3373)) {
                                edge_1128_1127_phi_1112_ = (_e3305 + _e3515);
                                let _e3524 = edge_1128_1127_phi_1112_;
                                phi_1112_ = _e3524;
                            } else {
                                edge_1129_1127_phi_1112_ = (_e3305 - _e3515);
                                let _e3528 = edge_1129_1127_phi_1112_;
                                phi_1112_ = _e3528;
                            }
                        }
                        let _e3535 = phi_1112_;
                        edge_1127_948_phi_1013_ = _e3535;
                        edge_1127_948_phi_1015_ = (_e3307 + 1u);
                        let _e3541 = edge_1127_948_phi_1013_;
                        let _e3543 = edge_1127_948_phi_1015_;
                        phi_1013_ = _e3541;
                        phi_1015_ = _e3543;
                        continue;
                    } else {
                        loop_header_carry_1016_ = _e3311;
                        break;
                    }
                }
                let _e3548 = phi_1013_;
                if (bitcast<i32>(_e3288) < bitcast<i32>(4u)) {
                    if (bitcast<i32>(_e3288) < bitcast<i32>(2u)) {
                        if (bitcast<i32>(_e3288) < bitcast<i32>(1u)) {
                            edge_1000_1033_phi_1151_ = _e3272;
                            edge_1000_1033_phi_1152_ = _e3274;
                            edge_1000_1033_phi_1153_ = _e3276;
                            edge_1000_1033_phi_1154_ = _e3278;
                            edge_1000_1033_phi_1155_ = _e3280;
                            edge_1000_1033_phi_1156_ = _e3282;
                            edge_1000_1033_phi_1157_ = _e3284;
                            edge_1000_1033_phi_1158_ = _e3548;
                            let _e3574 = edge_1000_1033_phi_1151_;
                            let _e3576 = edge_1000_1033_phi_1152_;
                            let _e3578 = edge_1000_1033_phi_1153_;
                            let _e3580 = edge_1000_1033_phi_1154_;
                            let _e3582 = edge_1000_1033_phi_1155_;
                            let _e3584 = edge_1000_1033_phi_1156_;
                            let _e3586 = edge_1000_1033_phi_1157_;
                            let _e3588 = edge_1000_1033_phi_1158_;
                            phi_1151_ = _e3574;
                            phi_1152_ = _e3576;
                            phi_1153_ = _e3578;
                            phi_1154_ = _e3580;
                            phi_1155_ = _e3582;
                            phi_1156_ = _e3584;
                            phi_1157_ = _e3586;
                            phi_1158_ = _e3588;
                        } else {
                            edge_1002_1033_phi_1151_ = _e3272;
                            edge_1002_1033_phi_1152_ = _e3274;
                            edge_1002_1033_phi_1153_ = _e3276;
                            edge_1002_1033_phi_1154_ = _e3278;
                            edge_1002_1033_phi_1155_ = _e3280;
                            edge_1002_1033_phi_1156_ = _e3282;
                            edge_1002_1033_phi_1157_ = _e3548;
                            edge_1002_1033_phi_1158_ = _e3286;
                            let _e3606 = edge_1002_1033_phi_1151_;
                            let _e3608 = edge_1002_1033_phi_1152_;
                            let _e3610 = edge_1002_1033_phi_1153_;
                            let _e3612 = edge_1002_1033_phi_1154_;
                            let _e3614 = edge_1002_1033_phi_1155_;
                            let _e3616 = edge_1002_1033_phi_1156_;
                            let _e3618 = edge_1002_1033_phi_1157_;
                            let _e3620 = edge_1002_1033_phi_1158_;
                            phi_1151_ = _e3606;
                            phi_1152_ = _e3608;
                            phi_1153_ = _e3610;
                            phi_1154_ = _e3612;
                            phi_1155_ = _e3614;
                            phi_1156_ = _e3616;
                            phi_1157_ = _e3618;
                            phi_1158_ = _e3620;
                        }
                    } else {
                        if (bitcast<i32>((_e3288 - 2u)) < bitcast<i32>(1u)) {
                            edge_1005_1033_phi_1151_ = _e3272;
                            edge_1005_1033_phi_1152_ = _e3274;
                            edge_1005_1033_phi_1153_ = _e3276;
                            edge_1005_1033_phi_1154_ = _e3278;
                            edge_1005_1033_phi_1155_ = _e3280;
                            edge_1005_1033_phi_1156_ = _e3548;
                            edge_1005_1033_phi_1157_ = _e3284;
                            edge_1005_1033_phi_1158_ = _e3286;
                            let _e3644 = edge_1005_1033_phi_1151_;
                            let _e3646 = edge_1005_1033_phi_1152_;
                            let _e3648 = edge_1005_1033_phi_1153_;
                            let _e3650 = edge_1005_1033_phi_1154_;
                            let _e3652 = edge_1005_1033_phi_1155_;
                            let _e3654 = edge_1005_1033_phi_1156_;
                            let _e3656 = edge_1005_1033_phi_1157_;
                            let _e3658 = edge_1005_1033_phi_1158_;
                            phi_1151_ = _e3644;
                            phi_1152_ = _e3646;
                            phi_1153_ = _e3648;
                            phi_1154_ = _e3650;
                            phi_1155_ = _e3652;
                            phi_1156_ = _e3654;
                            phi_1157_ = _e3656;
                            phi_1158_ = _e3658;
                        } else {
                            edge_1009_1033_phi_1151_ = _e3272;
                            edge_1009_1033_phi_1152_ = _e3274;
                            edge_1009_1033_phi_1153_ = _e3276;
                            edge_1009_1033_phi_1154_ = _e3278;
                            edge_1009_1033_phi_1155_ = _e3548;
                            edge_1009_1033_phi_1156_ = _e3282;
                            edge_1009_1033_phi_1157_ = _e3284;
                            edge_1009_1033_phi_1158_ = _e3286;
                            let _e3676 = edge_1009_1033_phi_1151_;
                            let _e3678 = edge_1009_1033_phi_1152_;
                            let _e3680 = edge_1009_1033_phi_1153_;
                            let _e3682 = edge_1009_1033_phi_1154_;
                            let _e3684 = edge_1009_1033_phi_1155_;
                            let _e3686 = edge_1009_1033_phi_1156_;
                            let _e3688 = edge_1009_1033_phi_1157_;
                            let _e3690 = edge_1009_1033_phi_1158_;
                            phi_1151_ = _e3676;
                            phi_1152_ = _e3678;
                            phi_1153_ = _e3680;
                            phi_1154_ = _e3682;
                            phi_1155_ = _e3684;
                            phi_1156_ = _e3686;
                            phi_1157_ = _e3688;
                            phi_1158_ = _e3690;
                        }
                    }
                } else {
                    let _e3700 = (_e3288 - 4u);
                    if (bitcast<i32>(_e3700) < bitcast<i32>(2u)) {
                        if (bitcast<i32>(_e3700) < bitcast<i32>(1u)) {
                            edge_1019_1033_phi_1151_ = _e3272;
                            edge_1019_1033_phi_1152_ = _e3274;
                            edge_1019_1033_phi_1153_ = _e3276;
                            edge_1019_1033_phi_1154_ = _e3548;
                            edge_1019_1033_phi_1155_ = _e3280;
                            edge_1019_1033_phi_1156_ = _e3282;
                            edge_1019_1033_phi_1157_ = _e3284;
                            edge_1019_1033_phi_1158_ = _e3286;
                            let _e3718 = edge_1019_1033_phi_1151_;
                            let _e3720 = edge_1019_1033_phi_1152_;
                            let _e3722 = edge_1019_1033_phi_1153_;
                            let _e3724 = edge_1019_1033_phi_1154_;
                            let _e3726 = edge_1019_1033_phi_1155_;
                            let _e3728 = edge_1019_1033_phi_1156_;
                            let _e3730 = edge_1019_1033_phi_1157_;
                            let _e3732 = edge_1019_1033_phi_1158_;
                            phi_1151_ = _e3718;
                            phi_1152_ = _e3720;
                            phi_1153_ = _e3722;
                            phi_1154_ = _e3724;
                            phi_1155_ = _e3726;
                            phi_1156_ = _e3728;
                            phi_1157_ = _e3730;
                            phi_1158_ = _e3732;
                        } else {
                            edge_1021_1033_phi_1151_ = _e3272;
                            edge_1021_1033_phi_1152_ = _e3274;
                            edge_1021_1033_phi_1153_ = _e3548;
                            edge_1021_1033_phi_1154_ = _e3278;
                            edge_1021_1033_phi_1155_ = _e3280;
                            edge_1021_1033_phi_1156_ = _e3282;
                            edge_1021_1033_phi_1157_ = _e3284;
                            edge_1021_1033_phi_1158_ = _e3286;
                            let _e3750 = edge_1021_1033_phi_1151_;
                            let _e3752 = edge_1021_1033_phi_1152_;
                            let _e3754 = edge_1021_1033_phi_1153_;
                            let _e3756 = edge_1021_1033_phi_1154_;
                            let _e3758 = edge_1021_1033_phi_1155_;
                            let _e3760 = edge_1021_1033_phi_1156_;
                            let _e3762 = edge_1021_1033_phi_1157_;
                            let _e3764 = edge_1021_1033_phi_1158_;
                            phi_1151_ = _e3750;
                            phi_1152_ = _e3752;
                            phi_1153_ = _e3754;
                            phi_1154_ = _e3756;
                            phi_1155_ = _e3758;
                            phi_1156_ = _e3760;
                            phi_1157_ = _e3762;
                            phi_1158_ = _e3764;
                        }
                    } else {
                        if (bitcast<i32>((_e3700 - 2u)) < bitcast<i32>(1u)) {
                            edge_1024_1033_phi_1151_ = _e3272;
                            edge_1024_1033_phi_1152_ = _e3548;
                            edge_1024_1033_phi_1153_ = _e3276;
                            edge_1024_1033_phi_1154_ = _e3278;
                            edge_1024_1033_phi_1155_ = _e3280;
                            edge_1024_1033_phi_1156_ = _e3282;
                            edge_1024_1033_phi_1157_ = _e3284;
                            edge_1024_1033_phi_1158_ = _e3286;
                            let _e3788 = edge_1024_1033_phi_1151_;
                            let _e3790 = edge_1024_1033_phi_1152_;
                            let _e3792 = edge_1024_1033_phi_1153_;
                            let _e3794 = edge_1024_1033_phi_1154_;
                            let _e3796 = edge_1024_1033_phi_1155_;
                            let _e3798 = edge_1024_1033_phi_1156_;
                            let _e3800 = edge_1024_1033_phi_1157_;
                            let _e3802 = edge_1024_1033_phi_1158_;
                            phi_1151_ = _e3788;
                            phi_1152_ = _e3790;
                            phi_1153_ = _e3792;
                            phi_1154_ = _e3794;
                            phi_1155_ = _e3796;
                            phi_1156_ = _e3798;
                            phi_1157_ = _e3800;
                            phi_1158_ = _e3802;
                        } else {
                            edge_1028_1033_phi_1151_ = _e3548;
                            edge_1028_1033_phi_1152_ = _e3274;
                            edge_1028_1033_phi_1153_ = _e3276;
                            edge_1028_1033_phi_1154_ = _e3278;
                            edge_1028_1033_phi_1155_ = _e3280;
                            edge_1028_1033_phi_1156_ = _e3282;
                            edge_1028_1033_phi_1157_ = _e3284;
                            edge_1028_1033_phi_1158_ = _e3286;
                            let _e3820 = edge_1028_1033_phi_1151_;
                            let _e3822 = edge_1028_1033_phi_1152_;
                            let _e3824 = edge_1028_1033_phi_1153_;
                            let _e3826 = edge_1028_1033_phi_1154_;
                            let _e3828 = edge_1028_1033_phi_1155_;
                            let _e3830 = edge_1028_1033_phi_1156_;
                            let _e3832 = edge_1028_1033_phi_1157_;
                            let _e3834 = edge_1028_1033_phi_1158_;
                            phi_1151_ = _e3820;
                            phi_1152_ = _e3822;
                            phi_1153_ = _e3824;
                            phi_1154_ = _e3826;
                            phi_1155_ = _e3828;
                            phi_1156_ = _e3830;
                            phi_1157_ = _e3832;
                            phi_1158_ = _e3834;
                        }
                    }
                }
                let _e3844 = phi_1151_;
                let _e3846 = phi_1152_;
                let _e3848 = phi_1153_;
                let _e3850 = phi_1154_;
                let _e3852 = phi_1155_;
                let _e3854 = phi_1156_;
                let _e3856 = phi_1157_;
                let _e3858 = phi_1158_;
                edge_1033_945_phi_994_ = _e3844;
                edge_1033_945_phi_996_ = _e3846;
                edge_1033_945_phi_998_ = _e3848;
                edge_1033_945_phi_1000_ = _e3850;
                edge_1033_945_phi_1002_ = _e3852;
                edge_1033_945_phi_1004_ = _e3854;
                edge_1033_945_phi_1006_ = _e3856;
                edge_1033_945_phi_1008_ = _e3858;
                edge_1033_945_phi_1010_ = (_e3288 + 1u);
                let _e3871 = edge_1033_945_phi_994_;
                let _e3873 = edge_1033_945_phi_996_;
                let _e3875 = edge_1033_945_phi_998_;
                let _e3877 = edge_1033_945_phi_1000_;
                let _e3879 = edge_1033_945_phi_1002_;
                let _e3881 = edge_1033_945_phi_1004_;
                let _e3883 = edge_1033_945_phi_1006_;
                let _e3885 = edge_1033_945_phi_1008_;
                let _e3887 = edge_1033_945_phi_1010_;
                phi_994_ = _e3871;
                phi_996_ = _e3873;
                phi_998_ = _e3875;
                phi_1000_ = _e3877;
                phi_1002_ = _e3879;
                phi_1004_ = _e3881;
                phi_1006_ = _e3883;
                phi_1008_ = _e3885;
                phi_1010_ = _e3887;
                continue;
            } else {
                edge_945_1179_phi_1163_ = 0f;
                edge_945_1179_phi_1165_ = 0f;
                edge_945_1179_phi_1167_ = 0f;
                edge_945_1179_phi_1169_ = 0f;
                edge_945_1179_phi_1171_ = 0f;
                edge_945_1179_phi_1173_ = 0f;
                edge_945_1179_phi_1175_ = 0f;
                edge_945_1179_phi_1177_ = 0u;
                let _e3914 = edge_945_1179_phi_1163_;
                let _e3916 = edge_945_1179_phi_1165_;
                let _e3918 = edge_945_1179_phi_1167_;
                let _e3920 = edge_945_1179_phi_1169_;
                let _e3922 = edge_945_1179_phi_1171_;
                let _e3924 = edge_945_1179_phi_1173_;
                let _e3926 = edge_945_1179_phi_1175_;
                let _e3928 = edge_945_1179_phi_1177_;
                phi_1163_ = _e3914;
                phi_1165_ = _e3916;
                phi_1167_ = _e3918;
                phi_1169_ = _e3920;
                phi_1171_ = _e3922;
                phi_1173_ = _e3924;
                phi_1175_ = _e3926;
                phi_1177_ = _e3928;
                loop_header_carry_1011_ = _e3292;
                break;
            }
        }
        let _e3939 = phi_994_;
        let _e3941 = phi_996_;
        let _e3943 = phi_998_;
        let _e3945 = phi_1000_;
        let _e3947 = phi_1002_;
        let _e3949 = phi_1004_;
        let _e3951 = phi_1006_;
        let _e3953 = phi_1008_;
        edge_945_1179_phi_1163_1 = 0f;
        edge_945_1179_phi_1165_1 = 0f;
        edge_945_1179_phi_1167_1 = 0f;
        edge_945_1179_phi_1169_1 = 0f;
        edge_945_1179_phi_1171_1 = 0f;
        edge_945_1179_phi_1173_1 = 0f;
        edge_945_1179_phi_1175_1 = 0f;
        edge_945_1179_phi_1177_1 = 0u;
        let _e3975 = edge_945_1179_phi_1163_1;
        let _e3977 = edge_945_1179_phi_1165_1;
        let _e3979 = edge_945_1179_phi_1167_1;
        let _e3981 = edge_945_1179_phi_1169_1;
        let _e3983 = edge_945_1179_phi_1171_1;
        let _e3985 = edge_945_1179_phi_1173_1;
        let _e3987 = edge_945_1179_phi_1175_1;
        let _e3989 = edge_945_1179_phi_1177_1;
        phi_1163_ = _e3975;
        phi_1165_ = _e3977;
        phi_1167_ = _e3979;
        phi_1169_ = _e3981;
        phi_1171_ = _e3983;
        phi_1173_ = _e3985;
        phi_1175_ = _e3987;
        phi_1177_ = _e3989;
        loop {
            let _e4000 = phi_1163_;
            let _e4002 = phi_1165_;
            let _e4004 = phi_1167_;
            let _e4006 = phi_1169_;
            let _e4008 = phi_1171_;
            let _e4010 = phi_1173_;
            let _e4012 = phi_1175_;
            let _e4014 = phi_1177_;
            let _e4018 = (bitcast<i32>(_e4014) < bitcast<i32>(7u));
            if _e4018 {
                let _e4022 = (bitcast<i32>(_e4014) < bitcast<i32>(4u));
                if _e4022 {
                    if (bitcast<i32>(_e4014) < bitcast<i32>(2u)) {
                        if (bitcast<i32>(_e4014) < bitcast<i32>(1u)) {
                            edge_1188_1221_phi_1206_ = _e3953;
                            let _e4033 = edge_1188_1221_phi_1206_;
                            phi_1206_ = _e4033;
                        } else {
                            edge_1190_1221_phi_1206_ = _e3951;
                            let _e4037 = edge_1190_1221_phi_1206_;
                            phi_1206_ = _e4037;
                        }
                    } else {
                        if (bitcast<i32>((_e4014 - 2u)) < bitcast<i32>(1u)) {
                            edge_1193_1221_phi_1206_ = _e3949;
                            let _e4047 = edge_1193_1221_phi_1206_;
                            phi_1206_ = _e4047;
                        } else {
                            edge_1197_1221_phi_1206_ = _e3947;
                            let _e4051 = edge_1197_1221_phi_1206_;
                            phi_1206_ = _e4051;
                        }
                    }
                } else {
                    let _e4054 = (_e4014 - 4u);
                    if (bitcast<i32>(_e4054) < bitcast<i32>(2u)) {
                        if (bitcast<i32>(_e4054) < bitcast<i32>(1u)) {
                            edge_1207_1221_phi_1206_ = _e3945;
                            let _e4065 = edge_1207_1221_phi_1206_;
                            phi_1206_ = _e4065;
                        } else {
                            edge_1209_1221_phi_1206_ = _e3943;
                            let _e4069 = edge_1209_1221_phi_1206_;
                            phi_1206_ = _e4069;
                        }
                    } else {
                        if (bitcast<i32>((_e4054 - 2u)) < bitcast<i32>(1u)) {
                            edge_1212_1221_phi_1206_ = _e3941;
                            let _e4079 = edge_1212_1221_phi_1206_;
                            phi_1206_ = _e4079;
                        } else {
                            edge_1216_1221_phi_1206_ = _e3939;
                            let _e4083 = edge_1216_1221_phi_1206_;
                            phi_1206_ = _e4083;
                        }
                    }
                }
                let _e4086 = phi_1206_;
                let _e4088 = sqrt(3f);
                if (_e4014 == 0u) {
                    edge_1231_1230_phi_1220_ = (_e4088 * 0.5f);
                    let _e4097 = edge_1231_1230_phi_1220_;
                    phi_1220_ = _e4097;
                } else {
                    edge_1232_1230_phi_1220_ = (_e4088 / 6f);
                    let _e4101 = edge_1232_1230_phi_1220_;
                    phi_1220_ = _e4101;
                }
                let _e4104 = phi_1220_;
                let _e4105 = (_e4086 / _e4104);
                if _e4022 {
                    if (bitcast<i32>(_e4014) < bitcast<i32>(2u)) {
                        if (bitcast<i32>(_e4014) < bitcast<i32>(1u)) {
                            edge_1248_1281_phi_1260_ = _e4000;
                            edge_1248_1281_phi_1261_ = _e4002;
                            edge_1248_1281_phi_1262_ = _e4004;
                            edge_1248_1281_phi_1263_ = _e4006;
                            edge_1248_1281_phi_1264_ = _e4008;
                            edge_1248_1281_phi_1265_ = _e4010;
                            edge_1248_1281_phi_1266_ = _e4105;
                            let _e4122 = edge_1248_1281_phi_1260_;
                            let _e4124 = edge_1248_1281_phi_1261_;
                            let _e4126 = edge_1248_1281_phi_1262_;
                            let _e4128 = edge_1248_1281_phi_1263_;
                            let _e4130 = edge_1248_1281_phi_1264_;
                            let _e4132 = edge_1248_1281_phi_1265_;
                            let _e4134 = edge_1248_1281_phi_1266_;
                            phi_1260_ = _e4122;
                            phi_1261_ = _e4124;
                            phi_1262_ = _e4126;
                            phi_1263_ = _e4128;
                            phi_1264_ = _e4130;
                            phi_1265_ = _e4132;
                            phi_1266_ = _e4134;
                        } else {
                            edge_1250_1281_phi_1260_ = _e4000;
                            edge_1250_1281_phi_1261_ = _e4002;
                            edge_1250_1281_phi_1262_ = _e4004;
                            edge_1250_1281_phi_1263_ = _e4006;
                            edge_1250_1281_phi_1264_ = _e4008;
                            edge_1250_1281_phi_1265_ = _e4105;
                            edge_1250_1281_phi_1266_ = _e4012;
                            let _e4150 = edge_1250_1281_phi_1260_;
                            let _e4152 = edge_1250_1281_phi_1261_;
                            let _e4154 = edge_1250_1281_phi_1262_;
                            let _e4156 = edge_1250_1281_phi_1263_;
                            let _e4158 = edge_1250_1281_phi_1264_;
                            let _e4160 = edge_1250_1281_phi_1265_;
                            let _e4162 = edge_1250_1281_phi_1266_;
                            phi_1260_ = _e4150;
                            phi_1261_ = _e4152;
                            phi_1262_ = _e4154;
                            phi_1263_ = _e4156;
                            phi_1264_ = _e4158;
                            phi_1265_ = _e4160;
                            phi_1266_ = _e4162;
                        }
                    } else {
                        if (bitcast<i32>((_e4014 - 2u)) < bitcast<i32>(1u)) {
                            edge_1253_1281_phi_1260_ = _e4000;
                            edge_1253_1281_phi_1261_ = _e4002;
                            edge_1253_1281_phi_1262_ = _e4004;
                            edge_1253_1281_phi_1263_ = _e4006;
                            edge_1253_1281_phi_1264_ = _e4105;
                            edge_1253_1281_phi_1265_ = _e4010;
                            edge_1253_1281_phi_1266_ = _e4012;
                            let _e4184 = edge_1253_1281_phi_1260_;
                            let _e4186 = edge_1253_1281_phi_1261_;
                            let _e4188 = edge_1253_1281_phi_1262_;
                            let _e4190 = edge_1253_1281_phi_1263_;
                            let _e4192 = edge_1253_1281_phi_1264_;
                            let _e4194 = edge_1253_1281_phi_1265_;
                            let _e4196 = edge_1253_1281_phi_1266_;
                            phi_1260_ = _e4184;
                            phi_1261_ = _e4186;
                            phi_1262_ = _e4188;
                            phi_1263_ = _e4190;
                            phi_1264_ = _e4192;
                            phi_1265_ = _e4194;
                            phi_1266_ = _e4196;
                        } else {
                            edge_1257_1281_phi_1260_ = _e4000;
                            edge_1257_1281_phi_1261_ = _e4002;
                            edge_1257_1281_phi_1262_ = _e4004;
                            edge_1257_1281_phi_1263_ = _e4105;
                            edge_1257_1281_phi_1264_ = _e4008;
                            edge_1257_1281_phi_1265_ = _e4010;
                            edge_1257_1281_phi_1266_ = _e4012;
                            let _e4212 = edge_1257_1281_phi_1260_;
                            let _e4214 = edge_1257_1281_phi_1261_;
                            let _e4216 = edge_1257_1281_phi_1262_;
                            let _e4218 = edge_1257_1281_phi_1263_;
                            let _e4220 = edge_1257_1281_phi_1264_;
                            let _e4222 = edge_1257_1281_phi_1265_;
                            let _e4224 = edge_1257_1281_phi_1266_;
                            phi_1260_ = _e4212;
                            phi_1261_ = _e4214;
                            phi_1262_ = _e4216;
                            phi_1263_ = _e4218;
                            phi_1264_ = _e4220;
                            phi_1265_ = _e4222;
                            phi_1266_ = _e4224;
                        }
                    }
                } else {
                    let _e4233 = (_e4014 - 4u);
                    if (bitcast<i32>(_e4233) < bitcast<i32>(2u)) {
                        if (bitcast<i32>(_e4233) < bitcast<i32>(1u)) {
                            edge_1267_1281_phi_1260_ = _e4000;
                            edge_1267_1281_phi_1261_ = _e4002;
                            edge_1267_1281_phi_1262_ = _e4105;
                            edge_1267_1281_phi_1263_ = _e4006;
                            edge_1267_1281_phi_1264_ = _e4008;
                            edge_1267_1281_phi_1265_ = _e4010;
                            edge_1267_1281_phi_1266_ = _e4012;
                            let _e4250 = edge_1267_1281_phi_1260_;
                            let _e4252 = edge_1267_1281_phi_1261_;
                            let _e4254 = edge_1267_1281_phi_1262_;
                            let _e4256 = edge_1267_1281_phi_1263_;
                            let _e4258 = edge_1267_1281_phi_1264_;
                            let _e4260 = edge_1267_1281_phi_1265_;
                            let _e4262 = edge_1267_1281_phi_1266_;
                            phi_1260_ = _e4250;
                            phi_1261_ = _e4252;
                            phi_1262_ = _e4254;
                            phi_1263_ = _e4256;
                            phi_1264_ = _e4258;
                            phi_1265_ = _e4260;
                            phi_1266_ = _e4262;
                        } else {
                            edge_1269_1281_phi_1260_ = _e4000;
                            edge_1269_1281_phi_1261_ = _e4105;
                            edge_1269_1281_phi_1262_ = _e4004;
                            edge_1269_1281_phi_1263_ = _e4006;
                            edge_1269_1281_phi_1264_ = _e4008;
                            edge_1269_1281_phi_1265_ = _e4010;
                            edge_1269_1281_phi_1266_ = _e4012;
                            let _e4278 = edge_1269_1281_phi_1260_;
                            let _e4280 = edge_1269_1281_phi_1261_;
                            let _e4282 = edge_1269_1281_phi_1262_;
                            let _e4284 = edge_1269_1281_phi_1263_;
                            let _e4286 = edge_1269_1281_phi_1264_;
                            let _e4288 = edge_1269_1281_phi_1265_;
                            let _e4290 = edge_1269_1281_phi_1266_;
                            phi_1260_ = _e4278;
                            phi_1261_ = _e4280;
                            phi_1262_ = _e4282;
                            phi_1263_ = _e4284;
                            phi_1264_ = _e4286;
                            phi_1265_ = _e4288;
                            phi_1266_ = _e4290;
                        }
                    } else {
                        if (bitcast<i32>((_e4233 - 2u)) < bitcast<i32>(1u)) {
                            edge_1272_1281_phi_1260_ = _e4105;
                            edge_1272_1281_phi_1261_ = _e4002;
                            edge_1272_1281_phi_1262_ = _e4004;
                            edge_1272_1281_phi_1263_ = _e4006;
                            edge_1272_1281_phi_1264_ = _e4008;
                            edge_1272_1281_phi_1265_ = _e4010;
                            edge_1272_1281_phi_1266_ = _e4012;
                            let _e4312 = edge_1272_1281_phi_1260_;
                            let _e4314 = edge_1272_1281_phi_1261_;
                            let _e4316 = edge_1272_1281_phi_1262_;
                            let _e4318 = edge_1272_1281_phi_1263_;
                            let _e4320 = edge_1272_1281_phi_1264_;
                            let _e4322 = edge_1272_1281_phi_1265_;
                            let _e4324 = edge_1272_1281_phi_1266_;
                            phi_1260_ = _e4312;
                            phi_1261_ = _e4314;
                            phi_1262_ = _e4316;
                            phi_1263_ = _e4318;
                            phi_1264_ = _e4320;
                            phi_1265_ = _e4322;
                            phi_1266_ = _e4324;
                        } else {
                            edge_1276_1281_phi_1260_ = _e4000;
                            edge_1276_1281_phi_1261_ = _e4002;
                            edge_1276_1281_phi_1262_ = _e4004;
                            edge_1276_1281_phi_1263_ = _e4006;
                            edge_1276_1281_phi_1264_ = _e4008;
                            edge_1276_1281_phi_1265_ = _e4010;
                            edge_1276_1281_phi_1266_ = _e4012;
                            let _e4340 = edge_1276_1281_phi_1260_;
                            let _e4342 = edge_1276_1281_phi_1261_;
                            let _e4344 = edge_1276_1281_phi_1262_;
                            let _e4346 = edge_1276_1281_phi_1263_;
                            let _e4348 = edge_1276_1281_phi_1264_;
                            let _e4350 = edge_1276_1281_phi_1265_;
                            let _e4352 = edge_1276_1281_phi_1266_;
                            phi_1260_ = _e4340;
                            phi_1261_ = _e4342;
                            phi_1262_ = _e4344;
                            phi_1263_ = _e4346;
                            phi_1264_ = _e4348;
                            phi_1265_ = _e4350;
                            phi_1266_ = _e4352;
                        }
                    }
                }
                let _e4361 = phi_1260_;
                let _e4363 = phi_1261_;
                let _e4365 = phi_1262_;
                let _e4367 = phi_1263_;
                let _e4369 = phi_1264_;
                let _e4371 = phi_1265_;
                let _e4373 = phi_1266_;
                edge_1281_1179_phi_1163_ = _e4361;
                edge_1281_1179_phi_1165_ = _e4363;
                edge_1281_1179_phi_1167_ = _e4365;
                edge_1281_1179_phi_1169_ = _e4367;
                edge_1281_1179_phi_1171_ = _e4369;
                edge_1281_1179_phi_1173_ = _e4371;
                edge_1281_1179_phi_1175_ = _e4373;
                edge_1281_1179_phi_1177_ = (_e4014 + 1u);
                let _e4385 = edge_1281_1179_phi_1163_;
                let _e4387 = edge_1281_1179_phi_1165_;
                let _e4389 = edge_1281_1179_phi_1167_;
                let _e4391 = edge_1281_1179_phi_1169_;
                let _e4393 = edge_1281_1179_phi_1171_;
                let _e4395 = edge_1281_1179_phi_1173_;
                let _e4397 = edge_1281_1179_phi_1175_;
                let _e4399 = edge_1281_1179_phi_1177_;
                phi_1163_ = _e4385;
                phi_1165_ = _e4387;
                phi_1167_ = _e4389;
                phi_1169_ = _e4391;
                phi_1171_ = _e4393;
                phi_1173_ = _e4395;
                phi_1175_ = _e4397;
                phi_1177_ = _e4399;
                continue;
            } else {
                edge_1179_4_phi_58_ = _e4000;
                edge_1179_4_phi_57_ = _e4002;
                edge_1179_4_phi_56_ = _e4004;
                edge_1179_4_phi_55_ = _e4006;
                edge_1179_4_phi_54_ = _e4008;
                edge_1179_4_phi_53_ = _e4010;
                edge_1179_4_phi_52_ = _e4012;
                let _e4416 = edge_1179_4_phi_58_;
                let _e4418 = edge_1179_4_phi_57_;
                let _e4420 = edge_1179_4_phi_56_;
                let _e4422 = edge_1179_4_phi_55_;
                let _e4424 = edge_1179_4_phi_54_;
                let _e4426 = edge_1179_4_phi_53_;
                let _e4428 = edge_1179_4_phi_52_;
                phi_58_ = _e4416;
                phi_57_ = _e4418;
                phi_56_ = _e4420;
                phi_55_ = _e4422;
                phi_54_ = _e4424;
                phi_53_ = _e4426;
                phi_52_ = _e4428;
                loop_header_carry_1178_ = _e4018;
                break;
            }
        }
    } else {
        edge_0_4_phi_58_ = _e19;
        edge_0_4_phi_57_ = _e17;
        edge_0_4_phi_56_ = _e15;
        edge_0_4_phi_55_ = _e13;
        edge_0_4_phi_54_ = _e11;
        edge_0_4_phi_53_ = _e9;
        edge_0_4_phi_52_ = _e7;
        let _e4463 = edge_0_4_phi_58_;
        let _e4465 = edge_0_4_phi_57_;
        let _e4467 = edge_0_4_phi_56_;
        let _e4469 = edge_0_4_phi_55_;
        let _e4471 = edge_0_4_phi_54_;
        let _e4473 = edge_0_4_phi_53_;
        let _e4475 = edge_0_4_phi_52_;
        phi_58_ = _e4463;
        phi_57_ = _e4465;
        phi_56_ = _e4467;
        phi_55_ = _e4469;
        phi_54_ = _e4471;
        phi_53_ = _e4473;
        phi_52_ = _e4475;
    }
    let _e4484 = phi_58_;
    let _e4486 = phi_57_;
    let _e4488 = phi_56_;
    let _e4490 = phi_55_;
    let _e4492 = phi_54_;
    let _e4494 = phi_53_;
    let _e4496 = phi_52_;
    let _e4507 = (((((f32(bitcast<i32>(u32(pos.x))) + 0.5f) / _e23) * 2f) - 1f) * 1.15f);
    let _e4518 = ((1f - (((f32(bitcast<i32>(u32(pos.y))) + 0.5f) / _e23) * 2f)) * 1.15f);
    edge_4_5_phi_232_ = 0f;
    edge_4_5_phi_165_ = false;
    edge_4_5_phi_95_ = 0u;
    let _e4526 = edge_4_5_phi_232_;
    let _e4528 = edge_4_5_phi_165_;
    let _e4530 = edge_4_5_phi_95_;
    phi_232_ = _e4526;
    phi_165_ = _e4528;
    phi_95_ = _e4530;
    loop {
        let _e4536 = phi_232_;
        let _e4538 = phi_165_;
        let _e4540 = phi_95_;
        let _e4544 = (bitcast<i32>(_e4540) < bitcast<i32>(6u));
        if _e4544 {
            if (_e4540 == 5u) {
                edge_6_10_phi_102_ = 0u;
                let _e4552 = edge_6_10_phi_102_;
                phi_102_ = _e4552;
            } else {
                edge_9_10_phi_102_ = (_e4540 + 1u);
                let _e4556 = edge_9_10_phi_102_;
                phi_102_ = _e4556;
            }
            let _e4559 = phi_102_;
            let _e4561 = (_e4540 == 0u);
            if _e4561 {
                edge_10_1887_phi_1792_ = 1f;
                let _e4598 = edge_10_1887_phi_1792_;
                phi_1792_ = _e4598;
            } else {
                if (_e4540 == 1u) {
                    edge_1886_1887_phi_1792_ = 0.5f;
                    let _e4593 = edge_1886_1887_phi_1792_;
                    phi_1792_ = _e4593;
                } else {
                    if (_e4540 == 2u) {
                        edge_1889_1887_phi_1792_ = -0.5f;
                        let _e4588 = edge_1889_1887_phi_1792_;
                        phi_1792_ = _e4588;
                    } else {
                        if (_e4540 == 3u) {
                            edge_1892_1887_phi_1792_ = -1f;
                            let _e4583 = edge_1892_1887_phi_1792_;
                            phi_1792_ = _e4583;
                        } else {
                            if (_e4540 == 4u) {
                                edge_1895_1887_phi_1792_ = -0.5f;
                                let _e4573 = edge_1895_1887_phi_1792_;
                                phi_1792_ = _e4573;
                            } else {
                                edge_1898_1887_phi_1792_ = 0.5f;
                                let _e4578 = edge_1898_1887_phi_1792_;
                                phi_1792_ = _e4578;
                            }
                        }
                    }
                }
            }
            let _e4601 = phi_1792_;
            if _e4561 {
                edge_1887_1905_phi_1803_ = 0f;
                let _e4631 = edge_1887_1905_phi_1803_;
                phi_1803_ = _e4631;
            } else {
                if (_e4540 == 1u) {
                    edge_1904_1905_phi_1803_ = 0.8660254f;
                    let _e4626 = edge_1904_1905_phi_1803_;
                    phi_1803_ = _e4626;
                } else {
                    if (_e4540 == 2u) {
                        edge_1907_1905_phi_1803_ = 0.8660254f;
                        let _e4621 = edge_1907_1905_phi_1803_;
                        phi_1803_ = _e4621;
                    } else {
                        if (_e4540 == 3u) {
                            edge_1910_1905_phi_1803_ = 0f;
                            let _e4611 = edge_1910_1905_phi_1803_;
                            phi_1803_ = _e4611;
                        } else {
                            edge_1913_1905_phi_1803_ = -0.8660254f;
                            let _e4616 = edge_1913_1905_phi_1803_;
                            phi_1803_ = _e4616;
                        }
                    }
                }
            }
            let _e4634 = phi_1803_;
            let _e4636 = (_e4559 == 0u);
            if _e4636 {
                edge_1905_1923_phi_1813_ = 1f;
                let _e4673 = edge_1905_1923_phi_1813_;
                phi_1813_ = _e4673;
            } else {
                if (_e4559 == 1u) {
                    edge_1922_1923_phi_1813_ = 0.5f;
                    let _e4668 = edge_1922_1923_phi_1813_;
                    phi_1813_ = _e4668;
                } else {
                    if (_e4559 == 2u) {
                        edge_1925_1923_phi_1813_ = -0.5f;
                        let _e4663 = edge_1925_1923_phi_1813_;
                        phi_1813_ = _e4663;
                    } else {
                        if (_e4559 == 3u) {
                            edge_1928_1923_phi_1813_ = -1f;
                            let _e4658 = edge_1928_1923_phi_1813_;
                            phi_1813_ = _e4658;
                        } else {
                            if (_e4559 == 4u) {
                                edge_1931_1923_phi_1813_ = -0.5f;
                                let _e4648 = edge_1931_1923_phi_1813_;
                                phi_1813_ = _e4648;
                            } else {
                                edge_1934_1923_phi_1813_ = 0.5f;
                                let _e4653 = edge_1934_1923_phi_1813_;
                                phi_1813_ = _e4653;
                            }
                        }
                    }
                }
            }
            let _e4676 = phi_1813_;
            if _e4636 {
                edge_1923_1941_phi_1822_ = 0f;
                let _e4706 = edge_1923_1941_phi_1822_;
                phi_1822_ = _e4706;
            } else {
                if (_e4559 == 1u) {
                    edge_1940_1941_phi_1822_ = 0.8660254f;
                    let _e4701 = edge_1940_1941_phi_1822_;
                    phi_1822_ = _e4701;
                } else {
                    if (_e4559 == 2u) {
                        edge_1943_1941_phi_1822_ = 0.8660254f;
                        let _e4696 = edge_1943_1941_phi_1822_;
                        phi_1822_ = _e4696;
                    } else {
                        if (_e4559 == 3u) {
                            edge_1946_1941_phi_1822_ = 0f;
                            let _e4686 = edge_1946_1941_phi_1822_;
                            phi_1822_ = _e4686;
                        } else {
                            edge_1949_1941_phi_1822_ = -0.8660254f;
                            let _e4691 = edge_1949_1941_phi_1822_;
                            phi_1822_ = _e4691;
                        }
                    }
                }
            }
            let _e4709 = phi_1822_;
            let _e4712 = ((_e4601 * _e4709) - (_e4634 * _e4676));
            let _e4716 = (((_e4507 * _e4709) - (_e4518 * _e4676)) / _e4712);
            let _e4720 = (((_e4601 * _e4518) - (_e4634 * _e4507)) / _e4712);
            let _e4723 = ((1f - _e4716) - _e4720);
            if (0f <= _e4716) {
                if (0f <= _e4720) {
                    if (0f <= _e4723) {
                        let _e4732 = (_e4540 + 1u);
                        if (_e4732 == 0u) {
                            edge_11_1959_phi_1834_ = _e4496;
                            let _e4771 = edge_11_1959_phi_1834_;
                            phi_1834_ = _e4771;
                        } else {
                            if (_e4732 == 1u) {
                                edge_1958_1959_phi_1834_ = _e4494;
                                let _e4767 = edge_1958_1959_phi_1834_;
                                phi_1834_ = _e4767;
                            } else {
                                if (_e4732 == 2u) {
                                    edge_1961_1959_phi_1834_ = _e4492;
                                    let _e4763 = edge_1961_1959_phi_1834_;
                                    phi_1834_ = _e4763;
                                } else {
                                    if (_e4732 == 3u) {
                                        edge_1964_1959_phi_1834_ = _e4490;
                                        let _e4759 = edge_1964_1959_phi_1834_;
                                        phi_1834_ = _e4759;
                                    } else {
                                        if (_e4732 == 4u) {
                                            edge_1967_1959_phi_1834_ = _e4488;
                                            let _e4755 = edge_1967_1959_phi_1834_;
                                            phi_1834_ = _e4755;
                                        } else {
                                            if (_e4732 == 5u) {
                                                edge_1970_1959_phi_1834_ = _e4486;
                                                let _e4747 = edge_1970_1959_phi_1834_;
                                                phi_1834_ = _e4747;
                                            } else {
                                                edge_1973_1959_phi_1834_ = _e4484;
                                                let _e4751 = edge_1973_1959_phi_1834_;
                                                phi_1834_ = _e4751;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let _e4774 = phi_1834_;
                        let _e4778 = (_e4559 + 1u);
                        if (_e4778 == 0u) {
                            edge_1959_1980_phi_1846_ = _e4496;
                            let _e4817 = edge_1959_1980_phi_1846_;
                            phi_1846_ = _e4817;
                        } else {
                            if (_e4778 == 1u) {
                                edge_1979_1980_phi_1846_ = _e4494;
                                let _e4813 = edge_1979_1980_phi_1846_;
                                phi_1846_ = _e4813;
                            } else {
                                if (_e4778 == 2u) {
                                    edge_1982_1980_phi_1846_ = _e4492;
                                    let _e4809 = edge_1982_1980_phi_1846_;
                                    phi_1846_ = _e4809;
                                } else {
                                    if (_e4778 == 3u) {
                                        edge_1985_1980_phi_1846_ = _e4490;
                                        let _e4805 = edge_1985_1980_phi_1846_;
                                        phi_1846_ = _e4805;
                                    } else {
                                        if (_e4778 == 4u) {
                                            edge_1988_1980_phi_1846_ = _e4488;
                                            let _e4801 = edge_1988_1980_phi_1846_;
                                            phi_1846_ = _e4801;
                                        } else {
                                            if (_e4778 == 5u) {
                                                edge_1991_1980_phi_1846_ = _e4486;
                                                let _e4793 = edge_1991_1980_phi_1846_;
                                                phi_1846_ = _e4793;
                                            } else {
                                                edge_1994_1980_phi_1846_ = _e4484;
                                                let _e4797 = edge_1994_1980_phi_1846_;
                                                phi_1846_ = _e4797;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let _e4820 = phi_1846_;
                        edge_1980_13_phi_233_ = (((_e4723 * _e4496) + (_e4716 * _e4774)) + (_e4720 * _e4820));
                        edge_1980_13_phi_166_ = true;
                        let _e4827 = edge_1980_13_phi_233_;
                        let _e4829 = edge_1980_13_phi_166_;
                        phi_233_ = _e4827;
                        phi_166_ = _e4829;
                    } else {
                        edge_14_13_phi_233_ = _e4536;
                        edge_14_13_phi_166_ = _e4538;
                        let _e4835 = edge_14_13_phi_233_;
                        let _e4837 = edge_14_13_phi_166_;
                        phi_233_ = _e4835;
                        phi_166_ = _e4837;
                    }
                } else {
                    edge_15_13_phi_233_ = _e4536;
                    edge_15_13_phi_166_ = _e4538;
                    let _e4843 = edge_15_13_phi_233_;
                    let _e4845 = edge_15_13_phi_166_;
                    phi_233_ = _e4843;
                    phi_166_ = _e4845;
                }
            } else {
                edge_1941_13_phi_233_ = _e4536;
                edge_1941_13_phi_166_ = _e4538;
                let _e4851 = edge_1941_13_phi_233_;
                let _e4853 = edge_1941_13_phi_166_;
                phi_233_ = _e4851;
                phi_166_ = _e4853;
            }
            let _e4857 = phi_233_;
            let _e4859 = phi_166_;
            edge_13_5_phi_232_ = _e4857;
            edge_13_5_phi_165_ = _e4859;
            edge_13_5_phi_95_ = (_e4540 + 1u);
            let _e4866 = edge_13_5_phi_232_;
            let _e4868 = edge_13_5_phi_165_;
            let _e4870 = edge_13_5_phi_95_;
            phi_232_ = _e4866;
            phi_165_ = _e4868;
            phi_95_ = _e4870;
            continue;
        } else {
            loop_header_carry_96_ = _e4544;
            break;
        }
    }
    let _e4876 = phi_232_;
    let _e4878 = phi_165_;
    if _e4878 {
        let _e4890 = bitcast<u32>((0.5f + (_e4876 * 0.18f)));
        let _e4891 = bitcast<u32>(0f);
        let _e4915 = bitcast<u32>(bitcast<f32>(select(select(_e4891, _e4890, ((_e4890 ^ ((0u - (_e4890 >> 31u)) | 2147483648u)) > (_e4891 ^ ((0u - (_e4891 >> 31u)) | 2147483648u)))), 2143289344u, (((_e4890 & 2147483647u) > 2139095040u) || ((_e4891 & 2147483647u) > 2139095040u)))));
        let _e4916 = bitcast<u32>(1f);
        let _e4939 = bitcast<f32>(select(select(_e4916, _e4915, ((_e4915 ^ ((0u - (_e4915 >> 31u)) | 2147483648u)) < (_e4916 ^ ((0u - (_e4916 >> 31u)) | 2147483648u)))), 2143289344u, (((_e4915 & 2147483647u) > 2139095040u) || ((_e4916 & 2147483647u) > 2139095040u))));
        let _e4945 = ((6.2831855f * (_e4939 + 0f)) + 1.5707964f);
        let _e4953 = (_e4945 - (6.2831855f * floor(((_e4945 / 6.2831855f) + 0.5f))));
        let _e4963 = ((1.2732395f * _e4953) - ((0.40528473f * _e4953) * bitcast<f32>((bitcast<u32>(_e4953) & 2147483647u))));
        let _e4982 = ((6.2831855f * (_e4939 + 0.33f)) + 1.5707964f);
        let _e4990 = (_e4982 - (6.2831855f * floor(((_e4982 / 6.2831855f) + 0.5f))));
        let _e5000 = ((1.2732395f * _e4990) - ((0.40528473f * _e4990) * bitcast<f32>((bitcast<u32>(_e4990) & 2147483647u))));
        let _e5019 = ((6.2831855f * (_e4939 + 0.67f)) + 1.5707964f);
        let _e5027 = (_e5019 - (6.2831855f * floor(((_e5019 / 6.2831855f) + 0.5f))));
        let _e5037 = ((1.2732395f * _e5027) - ((0.40528473f * _e5027) * bitcast<f32>((bitcast<u32>(_e5027) & 2147483647u))));
        let _e5053 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e4963 * bitcast<f32>((bitcast<u32>(_e4963) & 2147483647u))) - _e4963)) + _e4963))));
        let _e5054 = bitcast<u32>(0f);
        let _e5078 = bitcast<u32>(bitcast<f32>(select(select(_e5054, _e5053, ((_e5053 ^ ((0u - (_e5053 >> 31u)) | 2147483648u)) > (_e5054 ^ ((0u - (_e5054 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5053 & 2147483647u) > 2139095040u) || ((_e5054 & 2147483647u) > 2139095040u)))));
        let _e5079 = bitcast<u32>(1f);
        let _e5104 = (bitcast<f32>(select(select(_e5079, _e5078, ((_e5078 ^ ((0u - (_e5078 >> 31u)) | 2147483648u)) < (_e5079 ^ ((0u - (_e5079 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5078 & 2147483647u) > 2139095040u) || ((_e5079 & 2147483647u) > 2139095040u)))) * 255f);
        let _e5120 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e5000 * bitcast<f32>((bitcast<u32>(_e5000) & 2147483647u))) - _e5000)) + _e5000))));
        let _e5121 = bitcast<u32>(0f);
        let _e5145 = bitcast<u32>(bitcast<f32>(select(select(_e5121, _e5120, ((_e5120 ^ ((0u - (_e5120 >> 31u)) | 2147483648u)) > (_e5121 ^ ((0u - (_e5121 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5120 & 2147483647u) > 2139095040u) || ((_e5121 & 2147483647u) > 2139095040u)))));
        let _e5146 = bitcast<u32>(1f);
        let _e5171 = (bitcast<f32>(select(select(_e5146, _e5145, ((_e5145 ^ ((0u - (_e5145 >> 31u)) | 2147483648u)) < (_e5146 ^ ((0u - (_e5146 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5145 & 2147483647u) > 2139095040u) || ((_e5146 & 2147483647u) > 2139095040u)))) * 255f);
        let _e5187 = bitcast<u32>((0.5f + (0.5f * ((0.225f * ((_e5037 * bitcast<f32>((bitcast<u32>(_e5037) & 2147483647u))) - _e5037)) + _e5037))));
        let _e5188 = bitcast<u32>(0f);
        let _e5212 = bitcast<u32>(bitcast<f32>(select(select(_e5188, _e5187, ((_e5187 ^ ((0u - (_e5187 >> 31u)) | 2147483648u)) > (_e5188 ^ ((0u - (_e5188 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5187 & 2147483647u) > 2139095040u) || ((_e5188 & 2147483647u) > 2139095040u)))));
        let _e5213 = bitcast<u32>(1f);
        let _e5238 = (bitcast<f32>(select(select(_e5213, _e5212, ((_e5212 ^ ((0u - (_e5212 >> 31u)) | 2147483648u)) < (_e5213 ^ ((0u - (_e5213 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5212 & 2147483647u) > 2139095040u) || ((_e5213 & 2147483647u) > 2139095040u)))) * 255f);
        edge_16_18_phi_164_ = (((select(0u, select(select(bitcast<u32>(i32(_e5104)), 2147483648u, (_e5104 <= -2147483600f)), 2147483647u, (_e5104 >= 2147483600f)), (_e5104 == _e5104)) + (select(0u, select(select(bitcast<u32>(i32(_e5171)), 2147483648u, (_e5171 <= -2147483600f)), 2147483647u, (_e5171 >= 2147483600f)), (_e5171 == _e5171)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e5238)), 2147483648u, (_e5238 <= -2147483600f)), 2147483647u, (_e5238 >= 2147483600f)), (_e5238 == _e5238)) << 16u)) + 4278190080u);
        let _e5474 = edge_16_18_phi_164_;
        phi_164_ = _e5474;
    } else {
        let _e5263 = bitcast<u32>(0.06f);
        let _e5264 = bitcast<u32>(0f);
        let _e5288 = bitcast<u32>(bitcast<f32>(select(select(_e5264, _e5263, ((_e5263 ^ ((0u - (_e5263 >> 31u)) | 2147483648u)) > (_e5264 ^ ((0u - (_e5264 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5263 & 2147483647u) > 2139095040u) || ((_e5264 & 2147483647u) > 2139095040u)))));
        let _e5289 = bitcast<u32>(1f);
        let _e5314 = (bitcast<f32>(select(select(_e5289, _e5288, ((_e5288 ^ ((0u - (_e5288 >> 31u)) | 2147483648u)) < (_e5289 ^ ((0u - (_e5289 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5288 & 2147483647u) > 2139095040u) || ((_e5289 & 2147483647u) > 2139095040u)))) * 255f);
        let _e5331 = bitcast<u32>(0.07f);
        let _e5332 = bitcast<u32>(0f);
        let _e5356 = bitcast<u32>(bitcast<f32>(select(select(_e5332, _e5331, ((_e5331 ^ ((0u - (_e5331 >> 31u)) | 2147483648u)) > (_e5332 ^ ((0u - (_e5332 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5331 & 2147483647u) > 2139095040u) || ((_e5332 & 2147483647u) > 2139095040u)))));
        let _e5357 = bitcast<u32>(1f);
        let _e5382 = (bitcast<f32>(select(select(_e5357, _e5356, ((_e5356 ^ ((0u - (_e5356 >> 31u)) | 2147483648u)) < (_e5357 ^ ((0u - (_e5357 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5356 & 2147483647u) > 2139095040u) || ((_e5357 & 2147483647u) > 2139095040u)))) * 255f);
        let _e5399 = bitcast<u32>(0.09f);
        let _e5400 = bitcast<u32>(0f);
        let _e5424 = bitcast<u32>(bitcast<f32>(select(select(_e5400, _e5399, ((_e5399 ^ ((0u - (_e5399 >> 31u)) | 2147483648u)) > (_e5400 ^ ((0u - (_e5400 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5399 & 2147483647u) > 2139095040u) || ((_e5400 & 2147483647u) > 2139095040u)))));
        let _e5425 = bitcast<u32>(1f);
        let _e5450 = (bitcast<f32>(select(select(_e5425, _e5424, ((_e5424 ^ ((0u - (_e5424 >> 31u)) | 2147483648u)) < (_e5425 ^ ((0u - (_e5425 >> 31u)) | 2147483648u)))), 2143289344u, (((_e5424 & 2147483647u) > 2139095040u) || ((_e5425 & 2147483647u) > 2139095040u)))) * 255f);
        edge_2127_18_phi_164_ = (((select(0u, select(select(bitcast<u32>(i32(_e5314)), 2147483648u, (_e5314 <= -2147483600f)), 2147483647u, (_e5314 >= 2147483600f)), (_e5314 == _e5314)) + (select(0u, select(select(bitcast<u32>(i32(_e5382)), 2147483648u, (_e5382 <= -2147483600f)), 2147483647u, (_e5382 >= 2147483600f)), (_e5382 == _e5382)) << 8u)) + (select(0u, select(select(bitcast<u32>(i32(_e5450)), 2147483648u, (_e5450 <= -2147483600f)), 2147483647u, (_e5450 >= 2147483600f)), (_e5450 == _e5450)) << 16u)) + 4278190080u);
        let _e5478 = edge_2127_18_phi_164_;
        phi_164_ = _e5478;
    }
    let _e5481 = phi_164_;
    return unpack4x8unorm(_e5481);
}
