extern crate fe_common as common;

use common::shape::ShapeDescribe;

#[derive(ShapeDescribe)]
#[shape(kind = "")]
struct EmptyKind {
    #[shape(field = Structure)]
    value: u32,
}

#[derive(ShapeDescribe)]
struct EmptyLabel {
    #[shape(field = Names, label = "")]
    value: String,
}

fn main() {}
