@group(0) @binding(0)
var<storage, read_write> leaves: array<u32>;
@group(0) @binding(1)
var<storage, read_write> nodes: array<u32>;

@compute @workgroup_size(1, 1, 1)
fn main() {
    var phi_4_: u32;
    var edge_0_2_phi_4_: u32;
    var loop_result: u32;
    var loop_did_return: bool = false;
    var loop_header_carry_5_: bool;
    var edge_3_2_phi_4_: u32;

    edge_0_2_phi_4_ = 0u;
    let _e5 = edge_0_2_phi_4_;
    phi_4_ = _e5;
    loop {
        let _e9 = phi_4_;
        let _e11 = (_e9 < 8u);
        if _e11 {
            leaves[i32(_e9)] = ((_e9 << 1u) + 3u);
            nodes[i32(_e9)] = 0u;
            edge_3_2_phi_4_ = (_e9 + 1u);
            let _e25 = edge_3_2_phi_4_;
            phi_4_ = _e25;
            continue;
        } else {
            loop_did_return = true;
            loop_header_carry_5_ = _e11;
            break;
        }
    }
    let _e35 = loop_did_return;
    if _e35 {
    }
}
