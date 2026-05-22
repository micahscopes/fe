extern crate fe_common as common;

use common::shape::ShapeDescribe;

#[derive(ShapeDescribe)]
struct UnknownDimension {
    #[shape(field = NotADimension)]
    value: u32,
}

fn main() {}
