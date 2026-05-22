extern crate fe_common as common;

use common::shape::ShapeDescribe;

#[derive(ShapeDescribe)]
#[shape(unknown = "item")]
struct UnknownItemAttr {
    #[shape(field = Constants)]
    value: u32,
}

#[derive(ShapeDescribe)]
struct UnknownFieldAttr {
    #[shape(origin = "not-supported")]
    value: u32,
}

fn main() {}
