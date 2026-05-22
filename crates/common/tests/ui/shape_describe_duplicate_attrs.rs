extern crate fe_common as common;

use common::shape::ShapeDescribe;

#[derive(ShapeDescribe)]
#[shape(kind = "first", kind = "second")]
struct DuplicateKind;

#[derive(ShapeDescribe)]
struct DuplicateLabel {
    #[shape(field = Names, label = "first", label = "second")]
    value: String,
}

fn main() {}
