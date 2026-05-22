extern crate fe_common as common;

use common::shape::ShapeDescribe;

#[derive(ShapeDescribe)]
struct EmptySkipReason {
    #[shape(skip = "")]
    source_span: u32,
}

fn main() {}
