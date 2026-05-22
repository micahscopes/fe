extern crate fe_common as common;

use common::shape::ShapeDescribe;

#[derive(ShapeDescribe)]
struct MissingPolicy {
    value: u32,
}

fn main() {}
