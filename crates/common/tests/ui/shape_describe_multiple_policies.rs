extern crate fe_common as common;

use common::shape::ShapeDescribe;

#[derive(ShapeDescribe)]
struct MultiplePolicies {
    #[shape(field = Constants, child)]
    value: u32,
}

fn main() {}
